//! Playwright-style web commands executed through ShadowBot Chrome Bridge.
//! No browser action is retried, including when its following snapshot fails.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::cli::{BrowserSub, SnapshotDisplay, WebElement, WebOptions};
use crate::core::{self, BackendError, Res};

#[derive(Debug)]
pub struct PreparedWeb {
    command: &'static str,
    tool: &'static str,
    arguments: Value,
    timeout: u64,
    snapshot_after: bool,
    python_after: bool,
    output: Option<PathBuf>,
    display: Value,
}

impl PreparedWeb {
    pub fn preview(&self) -> Value {
        let mut effects = vec![if matches!(self.command, "web.snapshot" | "web.xpath") {
            "read"
        } else {
            "browser-action"
        }];
        if self.output.is_some() {
            effects.push("write-file");
        }
        json!({
            "action": self.command,
            "target": "current_browser",
            "tool": self.tool,
            "arguments": self.arguments,
            "effects": effects,
            "snapshot_after": self.snapshot_after,
            "python_after": self.python_after,
            "output": self.output,
        })
    }

    pub fn execute(self) -> Res<CmdOut> {
        let mut arguments = self.arguments.clone();
        if !self.python_after {
            arguments["no_python"] = json!(true);
        }
        let response = core::mcp_browser(self.tool, arguments, self.timeout).map_err(|error| {
            if self.command == "web.fill"
                && matches!(error.code.as_str(), "MCP_TOOL_FAILED" | "WEB_BRIDGE_FAILED")
                && error.hint.is_none()
            {
                error.with_hint("先运行 xbot-cli web snapshot 查看当前页面，确认使用最新快照中可编辑元素（例如 textbox）的编号；选择器已保存不代表填写成功，不要盲目重试")
            } else {
                error
            }
        })?;
        let mut data = response_object(response);
        data["command"] = json!(self.command);
        data["snapshot_display"] = self.display;
        if self.python_after {
            data["python"] = crate::web_python::generate(self.command, &self.arguments, &data);
        }
        data.as_object_mut().unwrap().remove("python_locator");
        if self.command == "web.snapshot" {
            if let Some(path) = self.output {
                save_snapshot(&path, &data)?;
                data["snapshot_file"] = json!(path);
            }
        } else if self.command != "web.xpath" {
            data["applied"] = json!(true);
            if self.snapshot_after {
                let context = &data["browser_context"];
                let snapshot_args = json!({"tab_id":context["tab_id"],"pipe":context["pipe"]});
                match core::mcp_browser("web_snapshot", snapshot_args, 60) {
                    Ok(snapshot) => data["snapshot"] = snapshot,
                    Err(error) => {
                        // The action succeeded. Reporting failure here could cause a caller
                        // to repeat a click or submit; retain success with separate evidence.
                        data["warning"] = json!({
                            "code": "WEB_SNAPSHOT_FAILED",
                            "message": "网页操作已成功，但后续快照获取失败",
                            "hint": "先运行 xbot-cli web snapshot 查看当前页面；不要重复刚才的操作",
                            "cause": {
                                "code": error.code,
                                "message": error.message,
                                "hint": error.hint,
                                "data": error.data,
                            }
                        });
                    }
                }
            }
        }
        Ok((data, vec![]))
    }
}

fn response_object(response: Value) -> Value {
    if response.is_object() {
        response
    } else {
        json!({"result": response})
    }
}

fn nonempty(value: &str, field: &str) -> Res<()> {
    if value.trim().is_empty() {
        Err(BackendError::new("BAD_INPUT", format!("{field} 不能为空")))
    } else {
        Ok(())
    }
}

/// Parsing is local only: Request::request_url does not send an HTTP request.
fn normalize_url(input: &str) -> Res<String> {
    let invalid = || {
        BackendError::new("BAD_INPUT", "URL 无效").with_hint(
            "使用 http://、https://、about:blank 或裸域名，例如 baidu.com、localhost:8080",
        )
    };
    if input.is_empty() || input.chars().any(char::is_whitespace) || input.contains('\\') {
        return Err(invalid());
    }
    if input == "about:blank" {
        return Ok(input.into());
    }
    let explicit = input.to_ascii_lowercase().starts_with("http://")
        || input.to_ascii_lowercase().starts_with("https://");
    let candidate = if explicit {
        input.to_owned()
    } else {
        if input.contains("://") || input.starts_with(['/', '?', '#']) {
            return Err(invalid());
        }
        let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
        // A colon in bare input must be a numeric port, or part of bracketed IPv6.
        // This rejects javascript:, file:, data:, mailto:, and other URI schemes.
        if !authority.starts_with('[') {
            if let Some((_, port)) = authority.split_once(':') {
                if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                    return Err(invalid());
                }
            }
        }
        format!("https://{input}")
    };
    let parsed = ureq::get(&candidate).request_url().map_err(|_| invalid())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.as_url().username().is_empty()
        || parsed.as_url().password().is_some()
        || (!explicit
            && !parsed.host().contains('.')
            && parsed.host() != "localhost"
            && !parsed.host().starts_with('['))
    {
        return Err(invalid());
    }
    Ok(parsed.as_url().to_string())
}

fn temporary_name(refid: &str) -> String {
    static NEXT_NAME: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Names are only passed to the browser backend; no project file is created.
    format!(
        "临时元素_{}_{}_{}_{}",
        std::process::id(),
        timestamp,
        NEXT_NAME.fetch_add(1, Ordering::Relaxed),
        refid
    )
}

fn element_arguments(refid: &str, name: Option<&str>, method: &str, args: Value) -> Res<Value> {
    nonempty(refid, "元素编号")?;
    if let Some(name) = name {
        nonempty(name, "选择器名称")?;
    }
    Ok(json!({
        "target": "locator", "method": method, "refid": refid,
        "selector_name": name.map(str::to_owned).unwrap_or_else(|| temporary_name(refid)),
        "args": args,
    }))
}

fn action(command: &'static str, arguments: Value, options: &WebOptions) -> PreparedWeb {
    PreparedWeb {
        command,
        tool: "web_action",
        arguments,
        timeout: 60,
        snapshot_after: !options.no_snapshot,
        python_after: !options.no_python,
        output: None,
        display: display_options(&options.display),
    }
}

fn display_options(display: &SnapshotDisplay) -> Value {
    json!({"view":display.snapshot_view,"limit":display.limit,"offset":display.offset,
        "filter":display.filter,"role":display.role})
}

fn element_action(
    command: &'static str,
    element: &WebElement,
    method: &str,
    args: Value,
    options: &WebOptions,
) -> Res<PreparedWeb> {
    Ok(action(
        command,
        element_arguments(&element.refid, element.name.as_deref(), method, args)?,
        options,
    ))
}

fn output_path(output: &Option<String>) -> Res<Option<PathBuf>> {
    let Some(output) = output else {
        return Ok(None);
    };
    nonempty(output, "快照文件路径")?;
    let path = PathBuf::from(output);
    if path.exists() {
        return Err(
            BackendError::new("BAD_INPUT", format!("快照文件已存在: {}", path.display()))
                .with_hint("选择一个新文件名；snapshot 不覆盖已有文件"),
        );
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(BackendError::new(
            "BAD_INPUT",
            format!("快照目录不存在: {}", parent.display()),
        )
        .with_hint("先创建目录，或使用已有目录中的新文件名"));
    }
    Ok(Some(path))
}

/// The tree is a backend-produced string; refs and full/diff content are not rewritten.
pub(crate) fn snapshot_text(snapshot: &Value) -> Option<&str> {
    snapshot
        .get("result")
        .and_then(Value::as_str)
        .or_else(|| snapshot.get("text").and_then(Value::as_str))
}

fn save_snapshot(path: &Path, data: &Value) -> Res<()> {
    let text = snapshot_text(data).ok_or_else(|| {
        BackendError::new("MCP_BAD_RESPONSE", "快照响应没有可保存的文本")
            .with_data(data.clone())
            .with_hint("快照已获取；使用 --json 检查原始结果，无需重复网页操作")
    })?;
    let fail = |error: std::io::Error| {
        BackendError::new(
            "FILE_WRITE",
            format!("无法保存快照 {}: {error}", path.display()),
        )
        .with_data(data.clone())
        .with_hint("快照已获取；检查路径和权限，使用新文件名。不要重复之前的网页操作")
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(fail)?;
    file.write_all(text.as_bytes()).map_err(fail)
}

pub fn prepare(sub: &BrowserSub) -> Res<Option<PreparedWeb>> {
    let prepared = match sub {
        BrowserSub::Open { url, mode, options } => {
            let mut arguments =
                json!({"url": normalize_url(url.as_deref().unwrap_or("about:blank"))?});
            if let Some(mode) = mode {
                nonempty(mode, "浏览器模式")?;
                arguments["mode"] = json!(mode);
            }
            PreparedWeb {
                command: "web.open",
                tool: "web_create",
                arguments,
                timeout: 120,
                snapshot_after: !options.no_snapshot,
                python_after: !options.no_python,
                output: None,
                display: display_options(&options.display),
            }
        }
        BrowserSub::Active { options } => PreparedWeb {
            command: "web.active",
            tool: "web_get_active",
            arguments: json!({}),
            timeout: 60,
            snapshot_after: !options.no_snapshot,
            python_after: !options.no_python,
            output: None,
            display: display_options(&options.display),
        },
        BrowserSub::Snapshot { output, display } => PreparedWeb {
            command: "web.snapshot",
            tool: "web_snapshot",
            arguments: json!({}),
            timeout: 60,
            snapshot_after: false,
            python_after: false,
            output: output_path(output)?,
            display: display_options(display),
        },
        BrowserSub::Xpath { refs } => PreparedWeb {
            command: "web.xpath",
            tool: "web_xpath",
            arguments: json!({"refs":refs}),
            timeout: 60,
            snapshot_after: false,
            python_after: false,
            output: None,
            display: Value::Null,
        },
        BrowserSub::Click {
            element,
            button,
            options,
        } => {
            let mut prepared = element_action("web.click", element, "click", json!([]), options)?;
            prepared.arguments["kwargs"] = json!({"button": button});
            prepared
        }
        BrowserSub::Dblclick { element, options } => {
            element_action("web.dblclick", element, "dblclick", json!([]), options)?
        }
        BrowserSub::Fill {
            element,
            text,
            options,
        } => element_action("web.fill", element, "fill", json!([text]), options)?,
        BrowserSub::Type { text, options } => action(
            "web.type",
            json!({"target": "keyboard", "method": "type", "args": [text]}),
            options,
        ),
        BrowserSub::Press {
            key,
            refid,
            name,
            options,
        } => {
            nonempty(key, "按键")?;
            let arguments = match refid {
                Some(refid) => element_arguments(refid, name.as_deref(), "press", json!([key]))?,
                None => json!({"target": "keyboard", "method": "press", "args": [key]}),
            };
            action("web.press", arguments, options)
        }
        BrowserSub::Hover { element, options } => {
            element_action("web.hover", element, "hover", json!([]), options)?
        }
        BrowserSub::Check { element, options } => {
            element_action("web.check", element, "check", json!([]), options)?
        }
        BrowserSub::Uncheck { element, options } => {
            element_action("web.uncheck", element, "uncheck", json!([]), options)?
        }
        BrowserSub::Select {
            element,
            values,
            options,
        } => element_action(
            "web.select",
            element,
            "selectOption",
            json!([values]),
            options,
        )?,
        BrowserSub::Goto { url, options } => action(
            "web.goto",
            json!({"target": "page", "method": "goto", "args": [normalize_url(url)?]}),
            options,
        ),
        BrowserSub::Reload { options } => action(
            "web.reload",
            json!({"target": "page", "method": "reload", "args": []}),
            options,
        ),
        BrowserSub::GoBack { options } => action(
            "web.go-back",
            json!({"target": "page", "method": "goBack", "args": []}),
            options,
        ),
        BrowserSub::GoForward { options } => action(
            "web.go-forward",
            json!({"target": "page", "method": "goForward", "args": []}),
            options,
        ),
        BrowserSub::Close => action(
            "web.close",
            json!({"target": "page", "method": "close", "args": []}),
            &WebOptions {
                no_snapshot: true,
                no_python: false,
                display: SnapshotDisplay {
                    snapshot_view: "tree".into(),
                    limit: None,
                    offset: 0,
                    filter: None,
                    role: None,
                },
            },
        ),
        BrowserSub::Act { .. }
        | BrowserSub::Eval { .. }
        | BrowserSub::Tools { .. }
        | BrowserSub::Schema { .. }
        | BrowserSub::Call { .. } => return Ok(None),
    };
    Ok(Some(prepared))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Group};
    use clap::Parser;

    fn parse(args: &[&str]) -> PreparedWeb {
        let cli = Cli::try_parse_from(["xbot-cli", "web"].into_iter().chain(args.iter().copied()))
            .unwrap();
        let Some(Group::Browser(sub)) = cli.group else {
            panic!("web command expected");
        };
        prepare(&sub).unwrap().unwrap()
    }

    #[test]
    fn url_normalization_is_offline_and_rejects_unsupported_or_invalid_urls() {
        for (input, expected) in [
            ("baidu.com", "https://baidu.com/"),
            ("localhost:8080/path", "https://localhost:8080/path"),
            ("about:blank", "about:blank"),
            ("http://127.0.0.1:1234", "http://127.0.0.1:1234/"),
            (
                "https://example.com/a?q=你好",
                "https://example.com/a?q=%E4%BD%A0%E5%A5%BD",
            ),
        ] {
            assert_eq!(normalize_url(input).unwrap(), expected);
        }
        for input in [
            "",
            " ",
            "https://",
            "javascript:alert(1)",
            "data:text/html,hello",
            "file:///tmp/x",
            "mailto:a@example.com",
            "https://a b.com",
            "localhost:65536",
            "not-a-domain",
            "//example.com",
            "example.com\\path",
        ] {
            assert!(normalize_url(input).is_err(), "accepted {input}");
        }
        assert_eq!(parse(&["open"]).arguments, json!({"url":"about:blank"}));
        assert_eq!(
            parse(&["open", "baidu.com"]).arguments,
            json!({"url":"https://baidu.com/"})
        );
        assert_eq!(
            parse(&["open", "--mode", "cef"]).arguments,
            json!({"url":"about:blank","mode":"cef"})
        );
    }

    #[test]
    fn locator_arguments_preserve_literals_and_generate_unique_names() {
        let first = parse(&[
            "fill",
            "observed-ref",
            "@file\n'你好'",
            "--name",
            "搜索输入",
        ]);
        assert_eq!(
            first.arguments,
            json!({"target":"locator","method":"fill","refid":"observed-ref","selector_name":"搜索输入","args":["@file\n'你好'"]})
        );
        let a = parse(&["click", "e4"]);
        let b = parse(&["click", "e4"]);
        assert_ne!(a.arguments["selector_name"], b.arguments["selector_name"]);
        assert!(a.arguments["selector_name"]
            .as_str()
            .unwrap()
            .starts_with("临时元素_"));
        assert_eq!(a.arguments["args"], json!([]));
        assert_eq!(a.arguments["kwargs"], json!({"button":"left"}));
        assert!(element_arguments(" ", None, "click", json!([])).is_err());
        assert!(element_arguments("e4", Some(" "), "click", json!([])).is_err());
        assert_eq!(
            parse(&["select", "e7", "red", "blue"]).arguments["args"],
            json!([["red", "blue"]])
        );
        assert_eq!(parse(&["fill", "e4", ""]).arguments["args"], json!([""]));
    }

    #[test]
    fn keyboard_and_page_methods_are_native_backend_operations() {
        assert_eq!(
            parse(&["press", "Control+A"]).arguments,
            json!({"target":"keyboard","method":"press","args":["Control+A"]})
        );
        let press = parse(&["press", "Enter", "--ref", "e3", "--name", "搜索"]);
        assert_eq!(
            press.arguments,
            json!({"target":"locator","method":"press","args":["Enter"],"refid":"e3","selector_name":"搜索"})
        );
        assert_eq!(
            parse(&["type", "--", "-quoted value"]).arguments["args"],
            json!(["-quoted value"])
        );
        for (command, method) in [
            ("go-back", "goBack"),
            ("go-forward", "goForward"),
            ("reload", "reload"),
            ("close", "close"),
        ] {
            assert_eq!(
                parse(&[command]).arguments,
                json!({"target":"page","method":method,"args":[]})
            );
        }
        assert_eq!(
            parse(&["goto", "baidu.com"]).arguments["args"],
            json!(["https://baidu.com/"])
        );
    }

    #[test]
    fn preview_describes_the_exact_execution_and_snapshot_policy() {
        let command = parse(&["click", "e4", "--name", "提交", "--no-snapshot"]);
        assert_eq!(command.preview()["arguments"], command.arguments);
        assert_eq!(command.preview()["snapshot_after"], false);
        assert!(parse(&["click", "e4"]).snapshot_after);
        assert!(!parse(&["close"]).snapshot_after);
        assert!(!parse(&["click", "e4", "--no-python"]).python_after);
        assert!(parse(&["click", "e4", "--no-snapshot"]).python_after);
        let path = std::env::temp_dir()
            .join(temporary_name("snapshot"))
            .with_extension("txt");
        let command = parse(&["snapshot", "--output", path.to_str().unwrap()]);
        assert_eq!(command.preview()["effects"], json!(["read", "write-file"]));
        assert!(!path.exists(), "preview wrote a file");
    }

    #[test]
    fn snapshot_file_preserves_refs_and_never_overwrites_existing_content() {
        let path = std::env::temp_dir()
            .join(temporary_name("snapshot"))
            .with_extension("txt");
        let snapshot = json!({"result":"### AccTree\n- textbox Name [e4]\n","acc_tree_meta":{"kind":"diff","diff_index":3}});
        save_snapshot(&path, &snapshot).unwrap();
        assert!(save_snapshot(&path, &json!({"result":"replacement"})).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            snapshot["result"].as_str().unwrap()
        );
        assert!(output_path(&Some(path.to_string_lossy().into())).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
