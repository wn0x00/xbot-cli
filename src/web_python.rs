//! Deterministic, additive Python output. Generating code never replays an action.
use serde_json::{json, Value};

pub const CLOSED_SHADOW_REASON: &str = "【不支持 shadow-root(closed)】影刀无法使用 closed ShadowRoot 内的元素，无法生成 XPath 或等价 Python；请由页面提供 open ShadowRoot 或外部可操作元素。";

fn closed_step(step: &Value) -> bool {
    step["closed_shadow"] == true
        || step["unsupported"] == "closed-shadow-root"
        || step["shadow_mode"] == "closed"
        || (step["kind"] == "shadow" && step["mode"] == "closed")
}

pub fn closed_shadow(locator: &Value) -> bool {
    locator["closed_shadow"] == true
        || locator["unsupported"] == "closed-shadow-root"
        || locator["shadow_mode"] == "closed"
        || locator["steps"]
            .as_array()
            .is_some_and(|steps| steps.iter().any(closed_step))
}

/// Shadow boundaries stay within a document segment; only frames split it.
pub fn xpath_segments(steps: &[Value]) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    if steps.last()?["kind"] != "element" {
        return None;
    }
    for (index, step) in steps.iter().enumerate() {
        if closed_step(step) {
            return None;
        }
        let xpath = step["xpath"].as_str().filter(|s| !s.trim().is_empty())?;
        current.push_str(xpath);
        match step["kind"].as_str()? {
            "shadow" => {
                current.push_str("/xbotShadowRoot");
            }
            "frame" => segments.push(std::mem::take(&mut current)),
            "element" if index == steps.len() - 1 => {}
            _ => return None,
        }
    }
    segments.push(current);
    Some(segments)
}

// Use only the documented page.execute_javascript function interface. XPath is
// evaluated in each document; open shadow trees use their own CSS query scope.
const CONTEXT_ACTION: &str = r#"function(element, args) {
  const p = JSON.parse(args);
  let root = document, el;
  const norm = v => String(v || "").replace(/[ \t\r\n]+/g, " ").replace(/^ | $/g, "");
  function accepts(el, filters) {
    return filters.every(f => {
      if (f.kind === "text") return norm(el.textContent) === f.value;
      if (f.kind === "label") return Array.from(el.labels || []).some(l => norm(l.textContent) === f.value);
      if (f.kind === "has-text") return Array.from(el.querySelectorAll(f.css)).some(n => norm(n.textContent) === f.value);
      if (f.kind === "class-prefix") return Array.from(el.classList).some(c => c.startsWith(f.value));
      if (f.kind === "url-route") return (el.getAttribute(f.name) || "").split("?")[0] === f.value;
      if (f.kind === "has-plan") return Array.from(el.querySelectorAll(f.css)).some(n => accepts(n, f.filters));
      return false;
    });
  }
  for (const s of p.steps) {
    let nodes;
    if (root.nodeType === 9) {
      const r = root.evaluate(s.xpath, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
      nodes = Array.from({length:r.snapshotLength}, (_, i) => r.snapshotItem(i));
    } else {
      nodes = [root];
      for (const part of s.query || [{css:s.css, filters:[]}]) {
        nodes = Array.from(new Set(nodes.flatMap(r => Array.from(r.querySelectorAll(part.css))
          .filter(n => accepts(n, part.filters)))));
        if (nodes.length > 5000) throw new Error("Locator query budget exceeded");
      }
    }
    if (nodes.length !== 1) throw new Error("Locator must match exactly one element");
    el = nodes[0];
    if (s.kind === "frame") {
      root = el.contentDocument;
      if (!root) throw new Error("Cross-origin or unavailable frame: use an engineering Selector");
    } else if (s.kind === "shadow") {
      root = el.shadowRoot;
      if (!root) throw new Error("Open ShadowRoot unavailable");
    }
  }
  const w = el.ownerDocument.defaultView;
  if (p.method === "click" || p.method === "dblclick") {
    if (p.button !== "left") throw new Error("Only left click supported by this adapter");
    el.click();
    if (p.method === "dblclick") {
      el.click(); el.dispatchEvent(new w.MouseEvent("dblclick", {bubbles:true, composed:true}));
    }
  } else if (p.method === "fill") {
    el.focus();
    if (el.isContentEditable) el.textContent = p.text;
    else {
      const proto = el.tagName === "TEXTAREA" ? w.HTMLTextAreaElement.prototype : w.HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, p.text);
    }
    el.dispatchEvent(new w.Event("input", {bubbles:true, composed:true}));
    el.dispatchEvent(new w.Event("change", {bubbles:true, composed:true}));
  } else if (p.method === "check" || p.method === "uncheck") {
    if (el.checked !== (p.method === "check")) el.click();
  } else if (p.method === "selectOption") {
    Array.from(el.options).forEach(o => {o.selected = p.values.includes(o.value);});
    el.dispatchEvent(new w.Event("input", {bubbles:true}));
    el.dispatchEvent(new w.Event("change", {bubbles:true}));
  } else { throw new Error("Unsupported context action"); }
  return true;
}"#;

pub fn quote(value: &str) -> String {
    // JSON string escapes are also valid Python 3.7 string escapes.
    serde_json::to_string(value).unwrap()
}

fn unavailable(reason: &str, locator: &Value) -> Value {
    json!({"status":"unavailable","code":null,"reason":reason,"locator":locator,
        "requires":[],"warnings":[],"runtime_verified":false})
}

fn ready(code: String, requires: Value, locator: &Value, warnings: Value) -> Value {
    json!({"status":"ready","code":code,"requires":requires,"locator":locator,
        "warnings":warnings,"runtime_verified":false})
}

fn native_action(
    element: &str,
    method: &str,
    button: &str,
    input: &str,
    args: &Value,
    locator: &Value,
) -> Result<String, &'static str> {
    Ok(match method {
        "click" => format!("{element}.click(button={})", quote(button)),
        "dblclick" => format!("{element}.dblclick()"),
        "hover" => format!("{element}.hover()"),
        "fill" => format!("{element}.input({input}, append=False)"),
        "check" | "uncheck" => format!("{element}.check(mode={})", quote(method)),
        "selectOption" => {
            let values = args["args"][0].as_array().ok_or("缺少 option value")?;
            let options = locator["options"]
                .as_array()
                .ok_or("缺少动作前的下拉选项映射")?;
            let indices: Vec<Value> = options
                .iter()
                .filter(|o| values.contains(&o["value"]))
                .map(|o| o["index"].clone())
                .collect();
            if indices.is_empty()
                || values
                    .iter()
                    .any(|v| !options.iter().any(|o| &o["value"] == v))
            {
                return Err("传入的 value 无法映射到已有下拉选项");
            }
            if locator["multiple"] == true {
                format!(
                    "{element}.select_multiple_by_index({}, append=False)",
                    serde_json::to_string(&indices).unwrap()
                )
            } else if indices.len() == 1 {
                format!("{element}.select_by_index({})", indices[0])
            } else {
                return Err("单选下拉框不能映射多个选项");
            }
        }
        _ => return Err("此元素操作暂未提供已核实的 SDK 映射"),
    })
}

pub fn generate(command: &str, args: &Value, data: &Value) -> Value {
    let locator = data.get("python_locator").cloned().unwrap_or(Value::Null);
    let mode = data
        .pointer("/browser_context/sdk_mode")
        .and_then(Value::as_str);
    let page_code = match command {
        "web.open" | "web.active" => {
            let Some(mode) = mode else {
                return unavailable(
                    "无法确认实际浏览器的 SDK 模式；auto 不是影刀 SDK 支持的模式",
                    &locator,
                );
            };
            if command == "web.open" {
                // Preserve the requested URL rather than silently persisting a redirect.
                format!(
                    "web_page = xbot.web.create({}, mode={})",
                    quote(args["url"].as_str().unwrap_or("about:blank")),
                    quote(mode)
                )
            } else {
                format!("web_page = xbot.web.get_active(mode={})", quote(mode))
            }
        }
        "web.goto" => format!(
            "web_page.navigate({})",
            quote(args["args"][0].as_str().unwrap_or(""))
        ),
        "web.reload" => "web_page.reload()".into(),
        "web.go-back" => "web_page.go_back()".into(),
        "web.go-forward" => "web_page.go_forward()".into(),
        "web.close" => "web_page.close()".into(),
        _ => String::new(),
    };
    if !page_code.is_empty() {
        return ready(
            page_code,
            if matches!(command, "web.open" | "web.active") {
                json!(["import xbot.web"])
            } else {
                json!(["web_page"])
            },
            &Value::Null,
            json!([]),
        );
    }
    if args["target"] != "locator" {
        return unavailable(
            "本版暂不生成快照、页面键盘或任意脚本的等价 Python",
            &locator,
        );
    }
    if closed_shadow(&locator) {
        let mut blocked = unavailable(CLOSED_SHADOW_REASON, &Value::Null);
        blocked["error_code"] = json!("WEB_SHADOW_CLOSED");
        blocked["warnings"] = json!([CLOSED_SHADOW_REASON]);
        return blocked;
    }
    let Some(steps) = locator["steps"].as_array().filter(|s| !s.is_empty()) else {
        return unavailable("动作前的 DOM 定位采集失败；网页操作没有重放", &locator);
    };
    let has_frame = steps.iter().any(|step| step["kind"] == "frame");
    if locator["cross_origin"] == true && !has_frame {
        return unavailable("缺少跨域 iframe 的完整层级，无法构建 Selector", &locator);
    }
    if steps.iter().any(|s| {
        s["requires_review"] == true || matches!(s["quality"].as_str(), Some("weak" | "structural"))
    }) {
        return unavailable(
            "缺少可靠语义锚点，只能生成弱定位或位置路径；网页动作已完成，但不输出可复用 Python，请补充稳定属性或人工确认定位",
            &locator,
        );
    }
    let method = args["method"].as_str().unwrap_or("");
    let button = args
        .pointer("/kwargs/button")
        .and_then(Value::as_str)
        .unwrap_or("left");
    let sensitive = locator["sensitive"].as_bool() == Some(true);
    let text = args["args"][0].as_str().unwrap_or("");
    let input = if sensitive {
        "secret_text".to_owned()
    } else {
        quote(text)
    };
    let mut warnings = vec!["定位已在动作前唯一性回查；语义稳定性为启发式判断，不保证跨版本有效"];
    for step in steps {
        for warning in step["warnings"].as_array().into_iter().flatten() {
            let message = match warning.as_str() {
                Some("text") => "定位依赖文本或标签文案；文案、语言变化后需重新采集",
                Some("id-prefix") => "动态 ID 仅使用语义前缀；请核对前缀是否为长期业务标识",
                Some("class" | "class-prefix") => "定位使用语义 class；样式重构后需重新采集",
                Some("tracking-query") => "链接定位忽略已识别的跟踪参数；请核对链接的业务身份",
                Some("business-url") => "链接定位保留业务 query/hash；业务参数变化后需重新采集",
                _ => continue,
            };
            if !warnings.contains(&message) {
                warnings.push(message);
            }
        }
    }
    let mut requires = vec!["web_page"];
    if sensitive && method == "fill" {
        requires.push("secret_text（敏感输入已参数化）");
        warnings.push("敏感输入未复制到代码中，请通过 secret_text 提供");
    }
    if has_frame {
        let Some(segments) = xpath_segments(steps) else {
            return unavailable("缺少完整的 iframe XPath 层级，无法构建 Selector", &locator);
        };
        let action = match native_action("ele", method, button, &input, args, &locator) {
            Ok(action) => action,
            Err(reason) => return unavailable(reason, &locator),
        };
        let definition = json!({
            "id":"iframe-xpath", "name":"iframe-xpath", "type":"simple",
            "xpath":{"enable":"true","segments":segments}
        });
        let code = format!(
            "from xbot.selector import Selector\n\nselector = Selector({definition})\ntry:\n    ele = web_page.find(selector, timeout=20)\nexcept Exception:\n    ele = None\nif ele is not None:\n    {action}"
        );
        requires.push("from xbot.selector import Selector");
        warnings.push("影刀语法，请使用影刀验证；segments 按外层 iframe、内层 iframe、目标元素排序，不可直接传给 find_by_xpath");
        warnings.push("仅捕获查找异常并置 ele = None；元素动作异常正常抛出。尚未在影刀运行此代码");
        return ready(code, json!(requires), &locator, json!(warnings));
    }
    if steps.len() == 1 {
        let Some(xpath) = steps[0]["xpath"].as_str() else {
            return unavailable("缺少已验证的 XPath", &locator);
        };
        let element = format!("web_page.find_by_xpath({}, timeout=20)", quote(xpath));
        let action = match native_action(&element, method, button, &input, args, &locator) {
            Ok(action) => action,
            Err(reason) => return unavailable(reason, &locator),
        };
        return ready(action, json!(requires), &locator, json!(warnings));
    }
    if !matches!(
        method,
        "click" | "dblclick" | "fill" | "check" | "uncheck" | "selectOption"
    ) || button != "left"
    {
        return unavailable(
            "复杂上下文中的悬停、右键、中键和按键需要原生工程 Selector；本版暂不生成",
            &locator,
        );
    }
    let mut payload = json!({"steps":steps,"method":method,"button":button});
    if method == "fill" && !sensitive {
        payload["text"] = json!(text);
    }
    if method == "selectOption" {
        payload["values"] = args["args"][0].clone();
    }
    let mut code = format!(
        "import json\ncontext_action = r\"\"\"{CONTEXT_ACTION}\"\"\"\ncontext_args = json.loads({})\n",
        quote(&payload.to_string())
    );
    if method == "fill" && sensitive {
        code.push_str("context_args[\"text\"] = secret_text\n");
    }
    code.push_str("web_page.execute_javascript(context_action, argument=json.dumps(context_args))");
    warnings.push(
        "此代码使用 JavaScript 合成事件，非原生模拟鼠标键盘；要求可信事件的网站需使用工程 Selector",
    );
    ready(code, json!(requires), &locator, json!(warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dom() -> Value {
        json!({"python_locator":{"steps":[{"kind":"element","xpath":"//*[@id='name']","css":"#name"}]}})
    }
    #[test]
    fn sdk_mappings_preserve_literals_and_sensitive_inputs() {
        let args = json!({"target":"locator","method":"fill","args":["\"\\\n你好"]});
        let result = generate("web.fill", &args, &dom());
        assert!(result["code"]
            .as_str()
            .unwrap()
            .contains(".input(\"\\\"\\\\\\n你好\", append=False)"));
        let mut data = dom();
        data["python_locator"]["sensitive"] = json!(true);
        let result = generate("web.fill", &args, &data);
        assert!(result["code"].as_str().unwrap().contains("secret_text"));
        assert!(!result.to_string().contains("你好"));
    }
    #[test]
    fn refuses_fake_complex_xpath_and_auto_mode() {
        assert_eq!(
            generate(
                "web.open",
                &json!({"url":"https://example.com/"}),
                &json!({})
            )["status"],
            "unavailable"
        );
        let mut data = dom();
        data["python_locator"]["cross_origin"] = json!(true);
        assert_eq!(
            generate(
                "web.click",
                &json!({"target":"locator","method":"click"}),
                &data
            )["code"],
            Value::Null
        );
    }
    #[test]
    fn shadow_only_uses_documented_js_adapter() {
        let data = json!({"python_locator":{"steps":[{"kind":"shadow","xpath":"//x-panel","css":"x-panel"},{"kind":"element","xpath":"//button","css":"button"}]}});
        let result = generate(
            "web.click",
            &json!({"target":"locator","method":"click"}),
            &data,
        );
        assert_eq!(result["status"], "ready");
        let code = result["code"].as_str().unwrap();
        assert!(code.contains("execute_javascript"));
        assert!(code.contains("function(element, args)"));
        assert!(!code.contains("xbotShadowRoot"));
    }
    #[test]
    fn nested_iframe_uses_native_selector_including_cross_origin_and_shadow() {
        let data = json!({"python_locator":{"cross_origin":true,"steps":[
            {"kind":"shadow","xpath":"//x-panel[@name='outer']"},
            {"kind":"frame","xpath":"//iframe[@name='login']"},
            {"kind":"frame","xpath":"//iframe[@title='nested']"},
            {"kind":"shadow","xpath":"//x-form"},
            {"kind":"element","xpath":"//button[@name='save']"}
        ]}});
        for (method, suffix) in [
            ("click", "ele.click(button=\"right\")"),
            ("dblclick", "ele.dblclick()"),
            ("hover", "ele.hover()"),
            ("fill", "ele.input(\"hello\", append=False)"),
            ("check", "ele.check(mode=\"check\")"),
            ("uncheck", "ele.check(mode=\"uncheck\")"),
        ] {
            let result = generate(
                "web.action",
                &json!({"target":"locator","method":method,"args":["hello"],"kwargs":{"button":"right"}}),
                &data,
            );
            assert_eq!(result["status"], "ready");
            let code = result["code"].as_str().unwrap();
            assert!(code.starts_with("from xbot.selector import Selector\n"));
            assert!(code.contains("\"enable\":\"true\""));
            assert!(code.contains("//x-panel[@name='outer']/xbotShadowRoot//iframe[@name='login']"));
            assert!(code.contains("//x-form/xbotShadowRoot//button[@name='save']"));
            assert!(code.contains("web_page.find(selector, timeout=20)"));
            assert!(code.contains("except Exception:\n    ele = None\nif ele is not None:"));
            assert!(code.ends_with(suffix), "{code}");
            assert!(!code.contains("execute_javascript"));
            assert!(!code.contains("find_by_xpath"));
            assert_eq!(result["runtime_verified"], false);
        }
    }
    #[test]
    fn iframe_select_and_sensitive_fill_preserve_the_native_contract() {
        let mut data = dom();
        data["python_locator"]["steps"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                json!({"kind":"frame","xpath":"//iframe[@name='settings']"}),
            );
        data["python_locator"]["sensitive"] = json!(true);
        let result = generate(
            "web.fill",
            &json!({"target":"locator","method":"fill","args":["PREVIEW_SECRET"]}),
            &data,
        );
        assert!(result["code"]
            .as_str()
            .unwrap()
            .ends_with("ele.input(secret_text, append=False)"));
        assert!(!result.to_string().contains("PREVIEW_SECRET"));
        data["python_locator"]["options"] =
            json!([{"value":"a","index":0},{"value":"b","index":1}]);
        for (multiple, values, suffix) in [
            (false, json!(["b"]), "ele.select_by_index(1)"),
            (
                true,
                json!(["a", "b"]),
                "ele.select_multiple_by_index([0,1], append=False)",
            ),
        ] {
            data["python_locator"]["multiple"] = json!(multiple);
            let result = generate(
                "web.select",
                &json!({"target":"locator","method":"selectOption","args":[values]}),
                &data,
            );
            assert!(result["code"].as_str().unwrap().ends_with(suffix));
        }
    }
    #[test]
    fn segment_builder_rejects_incomplete_or_closed_chains() {
        for steps in [
            json!([]),
            json!([{"kind":"frame","xpath":"//iframe"}]),
            json!([{"kind":"frame"},{"kind":"element","xpath":"//input"}]),
            json!([{"kind":"element","xpath":"//div"},{"kind":"element","xpath":"//input"}]),
            json!([{"kind":"shadow","mode":"closed","xpath":"//x-panel"},{"kind":"element","xpath":"//input"}]),
        ] {
            assert!(xpath_segments(steps.as_array().unwrap()).is_none());
        }
    }
    #[test]
    fn closed_shadow_metadata_blocks_python_without_leaking_diagnostic_paths() {
        for metadata in [
            json!({"closed_shadow":true}),
            json!({"unsupported":"closed-shadow-root"}),
            json!({"shadow_mode":"closed"}),
            json!({"steps":[{"kind":"shadow","mode":"closed","xpath":"//closed-host"},{"kind":"element","xpath":"//input"}]}),
        ] {
            let result = generate(
                "web.fill",
                &json!({"target":"locator","method":"fill","args":["TEST_SECRET"]}),
                &json!({"python_locator":metadata}),
            );
            assert_eq!(result["status"], "unavailable");
            assert_eq!(result["code"], Value::Null);
            assert_eq!(result["locator"], Value::Null);
            assert_eq!(result["error_code"], "WEB_SHADOW_CLOSED");
            assert_eq!(result["reason"], CLOSED_SHADOW_REASON);
            assert!(!result.to_string().contains("//closed-host"));
            assert!(!result.to_string().contains("TEST_SECRET"));
        }
    }
    #[test]
    #[ignore = "requires PREVIEW_PYTHON pointing to a Python 3.7 interpreter"]
    fn generated_iframe_python37_syntax_and_stub_calls() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let python = std::env::var("PREVIEW_PYTHON").expect("Set PREVIEW_PYTHON");
        let inspect = format!(
            "{}/tests/preview/inspect_python.py",
            env!("CARGO_MANIFEST_DIR")
        );
        let invoke = |code: &str, flags: &[&str]| {
            let mut child = Command::new(&python)
                .arg(&inspect)
                .args(flags)
                .env("PYTHONIOENCODING", "utf-8")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(code.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        };
        let mut data = json!({"python_locator":{"cross_origin":true,"steps":[
            {"kind":"frame","xpath":"//iframe[@name='outer']"},
            {"kind":"frame","xpath":"//iframe[@title='inner']"},
            {"kind":"shadow","xpath":"//x-form"},
            {"kind":"element","xpath":"//input[@name='target']"}
        ],"options":[{"value":"a","index":0},{"value":"b","index":1}]}});
        for (method, expected, values, multiple, sensitive) in [
            ("click", "click", json!(["hello"]), false, false),
            ("dblclick", "dblclick", json!([]), false, false),
            ("hover", "hover", json!([]), false, false),
            ("fill", "input", json!(["quotes \"'\\\n中文"]), false, false),
            ("fill", "input", json!(["PRIVATE_SECRET"]), false, true),
            ("check", "check", json!([]), false, false),
            ("uncheck", "check", json!([]), false, false),
            (
                "selectOption",
                "select_by_index",
                json!([["b"]]),
                false,
                false,
            ),
            (
                "selectOption",
                "select_multiple_by_index",
                json!([["a", "b"]]),
                true,
                false,
            ),
        ] {
            data["python_locator"]["multiple"] = json!(multiple);
            data["python_locator"]["sensitive"] = json!(sensitive);
            let generated = generate(
                "web.action",
                &json!({"target":"locator","method":method,"args":values,"kwargs":{"button":"right"}}),
                &data,
            );
            assert_eq!(generated["status"], "ready");
            let code = generated["code"].as_str().unwrap();
            let output = invoke(code, &[]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let inspected: Value = serde_json::from_slice(&output.stdout).unwrap();
            let calls = inspected["calls"].as_array().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0]["method"], "find");
            assert_eq!(calls[0]["selector"]["xpath"]["enable"], "true");
            assert_eq!(calls[0]["timeout"], 20);
            assert_eq!(
                calls[0]["selector"]["xpath"]["segments"],
                json!([
                    "//iframe[@name='outer']",
                    "//iframe[@title='inner']",
                    "//x-form/xbotShadowRoot//input[@name='target']"
                ])
            );
            assert_eq!(calls[1]["method"], expected);
            if method == "click" {
                assert_eq!(calls[1]["kwargs"]["button"], "right");
            } else if method == "fill" {
                assert_eq!(
                    calls[1]["args"][0],
                    if sensitive {
                        json!("TEST_SECRET")
                    } else {
                        values[0].clone()
                    }
                );
                assert_eq!(calls[1]["kwargs"]["append"], false);
            } else if matches!(method, "check" | "uncheck") {
                assert_eq!(calls[1]["kwargs"]["mode"], method);
            } else if method == "selectOption" {
                assert_eq!(
                    calls[1]["args"][0],
                    if multiple { json!([0, 1]) } else { json!(1) }
                );
            }
            let failed = invoke(code, &["--find-fails"]);
            assert!(failed.status.success());
            let inspected: Value = serde_json::from_slice(&failed.stdout).unwrap();
            assert_eq!(inspected["calls"].as_array().unwrap().len(), 1);
            let failed = invoke(code, &["--action-fails"]);
            assert!(!failed.status.success());
            assert!(String::from_utf8_lossy(&failed.stderr).contains("TEST_ACTION_FAILURE"));
        }
    }
    #[test]
    fn selects_by_indices_not_values_as_labels() {
        let mut data = dom();
        data["python_locator"]["options"] =
            json!([{"value":"a","index":0},{"value":"b","index":1}]);
        let result = generate(
            "web.select",
            &json!({"target":"locator","method":"selectOption","args":[["b"]]}),
            &data,
        );
        assert!(result["code"]
            .as_str()
            .unwrap()
            .ends_with(".select_by_index(1)"));
    }
    #[test]
    fn refuses_weak_steps_including_frame_or_shadow_owners() {
        for quality in ["weak", "structural"] {
            let data = json!({"python_locator":{"steps":[
                {"kind":"frame","xpath":"/html/body/iframe[1]","quality":quality},
                {"kind":"element","xpath":"//button[@name='save']","quality":"semantic"}
            ]}});
            let result = generate(
                "web.click",
                &json!({"target":"locator","method":"click"}),
                &data,
            );
            assert_eq!(result["status"], "unavailable");
            assert_eq!(result["code"], Value::Null);
            assert!(result["reason"].as_str().unwrap().contains("语义锚点"));
        }
    }
    #[test]
    fn semantic_strategy_warnings_are_deduplicated() {
        let mut data = dom();
        data["python_locator"]["steps"][0]["quality"] = json!("semantic");
        data["python_locator"]["steps"][0]["warnings"] = json!(["text", "text", "class"]);
        let result = generate(
            "web.click",
            &json!({"target":"locator","method":"click"}),
            &data,
        );
        assert_eq!(result["status"], "ready");
        assert_eq!(result["warnings"].as_array().unwrap().len(), 3);
    }
}
