use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Pipes::PeekNamedPipe;

use crate::core::{BackendError, Res};

const CHROME_BRIDGE_PREFIX: &str = "9F5EDA99-AA91-4278-A19C-3F0227CD119B";
const SHADOWBOT_BROWSER_BRIDGE_PREFIX: &str = "b99c93f4-6881-4d74-af43-2f3463fe1cc5";
const ALT_BROWSER_BRIDGE_PREFIX: &str = "678D1584-66CD-4D54-BFE1-B8D4200147B4";
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;
const ENGINE_CORE: &str = include_str!("web_assets/01-uiaDispatcher.core.js");
const ENGINE_PAGE: &str = include_str!("web_assets/02-uiaDispatcher.selector-engine.js");
const PW_ARIA: &str = include_str!("web_assets/pw-aria.js");

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgePipeInfo {
    name: String,
    path: String,
    family: &'static str,
    current_user: bool,
    multi_profile: bool,
}

pub fn tools() -> Vec<Value> {
    [
        ("web_create", "Open a URL through Chrome Bridge"),
        ("web_get_active", "Attach to the active browser tab"),
        ("web_snapshot", "Return an ARIA snapshot through Chrome Bridge"),
        ("web_action", "Navigate or interact through Chrome Bridge"),
        ("web_evaluate_playwright", "Evaluate read-only JavaScript through Chrome Bridge"),
    ]
    .into_iter()
    .map(|(name, description)| {
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{},"additionalProperties":true}})
    })
    .collect()
}

pub fn call(tool: &str, args: Value) -> Res<Value> {
    match tool {
        "web_create" => {
            let url = required_string(&args, "url")?;
            open(url, args.get("mode").and_then(Value::as_str), false)
        }
        "web_get_active" => with_provider(|provider| {
            let tab_id = provider.active_tab()?;
            let mut data = json!({"success":true,"backend":"chrome-bridge"});
            provider.observe(tab_id, "", false, &mut data)?;
            Ok(data)
        }),
        "web_snapshot" => with_provider(|provider| {
            let tab_id = provider.active_tab()?;
            let mut data = json!({"success":true,"backend":"chrome-bridge"});
            provider.observe(tab_id, "", true, &mut data)?;
            let snapshot = data["snapshot"].clone();
            data["result"] = snapshot["result"].clone();
            data["acc_tree_meta"] = snapshot["acc_tree_meta"].clone();
            data.as_object_mut().unwrap().remove("snapshot");
            Ok(data)
        }),
        "web_action" => action(args),
        "web_evaluate_playwright" => evaluate_page(&args),
        _ => Err(BackendError::new(
            "MCP_TOOL_UNKNOWN",
            format!("Chrome Bridge 不支持工具: {tool}"),
        )
        .with_hint("可用工具: web_create, web_get_active, web_snapshot, web_action, web_evaluate_playwright")),
    }
}

fn open(url: &str, mode: Option<&str>, snapshot: bool) -> Res<Value> {
    let mut provider = Provider::connect(mode)?;
    let tab_id = provider.create(url)?;
    let mut data = json!({
        "success": true,
        "backend": "chrome-bridge",
        "url": url,
        "browser_context": provider.context(tab_id, mode),
    });
    if let Err(error) = provider.observe(tab_id, url, snapshot, &mut data) {
        data["warning"] = observation_warning(error);
    }
    Ok(data)
}

fn with_provider(run: impl FnOnce(&mut Provider) -> Res<Value>) -> Res<Value> {
    run(&mut Provider::connect(None)?)
}

fn action(args: Value) -> Res<Value> {
    let target = required_string(&args, "target")?;
    let method = required_string(&args, "method")?;
    let values = args
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    with_provider(|provider| {
        let tab_id = provider.active_tab()?;
        let result = match (target, method) {
            ("page", "goto") => {
                let url = argument_string(&values, 0, "url")?;
                provider.cdp(tab_id, "Page.enable", json!({}))?;
                let result = provider.cdp(tab_id, "Page.navigate", json!({"url":url}))?;
                provider.wait_info(tab_id, url, Duration::from_secs(20))?;
                provider.wait_ready(tab_id, Duration::from_secs(20))?;
                result
            }
            ("page", "reload") => {
                let result = provider.cdp(tab_id, "Page.reload", json!({}))?;
                provider.wait_ready(tab_id, Duration::from_secs(20))?;
                result
            }
            ("page", "goBack") => {
                let result = provider.evaluate(tab_id, "history.back()".into())?;
                provider.wait_ready(tab_id, Duration::from_secs(20))?;
                result
            }
            ("page", "goForward") => {
                let result = provider.evaluate(tab_id, "history.forward()".into())?;
                provider.wait_ready(tab_id, Duration::from_secs(20))?;
                result
            }
            ("page", "close") => provider.cdp(tab_id, "Page.close", json!({}))?,
            ("keyboard", "type") => provider.evaluate(
                tab_id,
                wrap_js(&format!(
                    "return $$.typeActive({},false);",
                    js_string(argument_string(&values, 0, "text")?)
                )),
            )?,
            ("keyboard", "press") => provider.evaluate(
                tab_id,
                wrap_js(&format!(
                    "return $$.press(null,{});",
                    js_string(argument_string(&values, 0, "key")?)
                )),
            )?,
            ("locator", method) => {
                let refid = required_string(&args, "refid")?;
                match method {
                    "click" | "dblclick" => {
                        let button = args
                            .pointer("/kwargs/button")
                            .and_then(Value::as_str)
                            .unwrap_or("left");
                        provider.trusted_click(
                            tab_id,
                            refid,
                            button,
                            if method == "dblclick" { 2 } else { 1 },
                        )?
                    }
                    "hover" => {
                        let (x, y) = provider.point_of(tab_id, refid)?;
                        provider.cdp(
                            tab_id,
                            "Input.dispatchMouseEvent",
                            json!({"type":"mouseMoved","x":x,"y":y}),
                        )?
                    }
                    "fill" => provider.el_eval(
                        tab_id,
                        refid,
                        &format!(
                            "$$.fill(el,{})",
                            js_string(argument_string(&values, 0, "text")?)
                        ),
                    )?,
                    "press" => provider.el_eval(
                        tab_id,
                        refid,
                        &format!(
                            "$$.press(el,{})",
                            js_string(argument_string(&values, 0, "key")?)
                        ),
                    )?,
                    "check" | "uncheck" => provider.el_eval(
                        tab_id,
                        refid,
                        &format!("$$.setChecked(el,{})", method == "check"),
                    )?,
                    "selectOption" => provider.el_eval(
                        tab_id,
                        refid,
                        &format!(
                            "$$.select(el,{})",
                            serde_json::to_string(
                                &values.first().cloned().unwrap_or_else(|| json!([]))
                            )
                            .unwrap()
                        ),
                    )?,
                    _ => {
                        return Err(BackendError::new(
                            "BAD_INPUT",
                            format!("不支持的元素操作: {method}"),
                        ))
                    }
                }
            }
            _ => {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    format!("不支持的网页操作: {target}.{method}"),
                ))
            }
        };
        Ok(json!({
            "success":true,
            "backend":"chrome-bridge",
            "result":result,
            "browser_context":provider.context(tab_id, None),
        }))
    })
}

fn evaluate_page(args: &Value) -> Res<Value> {
    let code = required_string(args, "code")?;
    with_provider(|provider| {
        let tab_id = provider.active_tab()?;
        let expression = format!(
            "(async function(){{const activePage={{title:async()=>document.title,url:async()=>location.href,evaluate:async(fn,...args)=>{{if(typeof fn==='string')return eval(fn);return fn(...args);}}}};return (async function(){{{code}}})();}})()"
        );
        Ok(json!({
            "success":true,
            "backend":"chrome-bridge",
            "result":provider.evaluate(tab_id, expression)?,
        }))
    })
}

fn observation_warning(error: BackendError) -> Value {
    json!({
        "code": "WEB_BRIDGE_OBSERVATION_FAILED",
        "message": "网页操作已执行，但后续页面信息或快照获取失败",
        "hint": "先检查当前标签页；不要重复刚才的操作",
        "cause": {
            "code": error.code,
            "message": error.message,
            "hint": error.hint,
            "data": error.data,
        }
    })
}

fn required_string<'a>(args: &'a Value, field: &str) -> Res<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BackendError::new("BAD_INPUT", format!("缺少参数: {field}")))
}

fn argument_string<'a>(args: &'a [Value], index: usize, field: &str) -> Res<&'a str> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| BackendError::new("BAD_INPUT", format!("缺少参数: {field}")))
}

struct Provider {
    pipe: NamedPipe,
    pipe_info: BridgePipeInfo,
    extension_version: Value,
}

impl Provider {
    fn connect(mode: Option<&str>) -> Res<Self> {
        let pipe_info = resolve_pipe(mode)?;
        let mut provider = Self {
            pipe: NamedPipe::connect(&pipe_info.path, Duration::from_secs(8))?,
            pipe_info,
            extension_version: Value::Null,
        };
        provider.init()?;
        provider.ensure_bridge_engine()?;
        Ok(provider)
    }

    fn init(&mut self) -> Res<()> {
        let _ = self.pipe.send("KeepAlive", Value::Null)?;
        self.extension_version = self.pipe.send("Extension.GetVersion", Value::Null)?;
        Ok(())
    }

    fn context(&self, tab_id: i64, mode: Option<&str>) -> Value {
        json!({
            "mode": mode.unwrap_or("auto"),
            "tab_id": tab_id,
            "pipe": self.pipe_info.path,
            "pipe_family": self.pipe_info.family,
            "extension_version": self.extension_version,
        })
    }

    fn engine_ready(&self) -> bool {
        self.extension_version
            .as_str()
            .map(|version| !version.is_empty())
            .unwrap_or(!self.extension_version.is_null())
    }

    fn ensure_bridge_engine(&mut self) -> Res<()> {
        if self.engine_ready() {
            return Ok(());
        }
        self.pipe
            .send("Extension.Init", json!({"code":ENGINE_CORE}))?;
        self.pipe
            .send("chrome.initContentScript", json!({"code":ENGINE_PAGE}))?;
        let _ = self.pipe.send("logger.setLogLevel", json!({"logLevel":2}));
        self.extension_version = json!("embedded");
        Ok(())
    }

    fn send(&mut self, method: &str, params: Value) -> Res<Value> {
        if method.starts_with("browser.") || method.starts_with("element.") {
            self.ensure_bridge_engine()?;
        }
        self.pipe.send(method, params)
    }

    fn create(&mut self, url: &str) -> Res<i64> {
        self.pipe
            .send("chrome.create", json!({"url":url}))?
            .as_i64()
            .ok_or_else(|| {
                BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "chrome.create 未返回有效 tabId")
            })
    }

    fn active_tab(&mut self) -> Res<i64> {
        let tabs = self.pipe.send("chrome.query", json!({"active":true}))?;
        match tabs {
            Value::Array(ref items) => items.first().and_then(Value::as_i64),
            Value::Number(ref number) => number.as_i64(),
            _ => None,
        }
        .ok_or_else(|| {
            BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "chrome.query 未返回活动 tabId")
                .with_data(tabs)
        })
    }

    fn observe(
        &mut self,
        tab_id: i64,
        expected_url: &str,
        snapshot: bool,
        data: &mut Value,
    ) -> Res<()> {
        let info = self.wait_info(tab_id, expected_url, Duration::from_secs(20))?;
        if let Some(url) = info.get("url").and_then(Value::as_str) {
            data["url"] = json!(url);
        }
        if let Some(title) = info.get("title").and_then(Value::as_str) {
            data["title"] = json!(title);
        }
        if snapshot {
            self.cdp(tab_id, "Page.enable", json!({}))?;
            self.wait_ready(tab_id, Duration::from_secs(20))?;
            let aria = self.aria_snapshot(tab_id)?;
            data["snapshot"] = json!({
                "success": true,
                "result": aria,
                "acc_tree_meta": {"kind":"full"},
            });
        }
        Ok(())
    }

    fn wait_info(&mut self, tab_id: i64, expected_url: &str, timeout: Duration) -> Res<Value> {
        let started = Instant::now();
        let mut last = Value::Null;
        while started.elapsed() < timeout {
            last = self.send("browser.getInfo", json!({"tabId":tab_id}))?;
            let url = last.get("url").and_then(Value::as_str).unwrap_or("");
            if expected_url.is_empty()
                || expected_url == "about:blank"
                || (!url.is_empty() && url != "about:blank")
            {
                return Ok(last);
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(BackendError::new("WEB_BRIDGE_LOAD_TIMEOUT", "等待标签页导航超时").with_data(last))
    }

    fn cdp(&mut self, tab_id: i64, method: &str, command_param: Value) -> Res<Value> {
        self.ensure_bridge_engine()?;
        self.pipe.send(
            "debugger.sendCommand",
            json!({"tabId":tab_id,"method":method,"commandParam":command_param}),
        )
    }

    fn evaluate(&mut self, tab_id: i64, expression: String) -> Res<Value> {
        let response = self.cdp(
            tab_id,
            "Runtime.evaluate",
            json!({
                "expression":expression,
                "returnByValue":true,
                "awaitPromise":true,
                "userGesture":true,
            }),
        )?;
        if let Some(details) = response.get("exceptionDetails") {
            let message = details
                .pointer("/exception/description")
                .or_else(|| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("Runtime.evaluate failed");
            return Err(BackendError::new("WEB_BRIDGE_EVAL_FAILED", message));
        }
        Ok(response
            .get("result")
            .and_then(|result| result.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn wait_ready(&mut self, tab_id: i64, timeout: Duration) -> Res<()> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            let state = self.evaluate(tab_id, "document.readyState".into())?;
            if state.as_str() == Some("complete") {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(BackendError::new(
            "WEB_BRIDGE_LOAD_TIMEOUT",
            "等待页面加载完成超时",
        ))
    }

    fn aria_snapshot(&mut self, tab_id: i64) -> Res<String> {
        let inject = format!(
            "(function(){{try{{if(window.__pwIS)return 'cached';var __pwM={{exports:{{}}}};(function(module,exports){{\n{}\n}})(__pwM,__pwM.exports);var __IS=__pwM.exports.InjectedScript;if(__IS&&!__IS.prototype)__IS=__IS();window.__pwIS=new __IS(window,{{isUnderTest:false,sdkLanguage:'javascript',testIdAttributeName:'data-testid',stableRafCount:1,browserName:'chromium',customEngines:[],isUtilityWorld:false}});return 'injected';}}catch(e){{return 'ERR:'+(e&&e.stack||e);}}}})()",
            PW_ARIA
        );
        if let Some(error) = self
            .evaluate(tab_id, inject)?
            .as_str()
            .and_then(|value| value.strip_prefix("ERR:"))
            .map(str::to_owned)
        {
            return Err(BackendError::new("WEB_BRIDGE_SNAPSHOT_FAILED", error));
        }
        self.evaluate(
            tab_id,
            "window.__pwIS.ariaSnapshot(document.body,{mode:'ai'})".into(),
        )?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "ARIA 快照未返回文本"))
    }

    fn el_eval(&mut self, tab_id: i64, refid: &str, expression: &str) -> Res<Value> {
        self.evaluate(
            tab_id,
            wrap_js(&format!(
                "var el=$$.must({});return ({});",
                js_string(refid),
                expression
            )),
        )
    }

    fn point_of(&mut self, tab_id: i64, refid: &str) -> Res<(i64, i64)> {
        let rect = self.el_eval(tab_id, refid, "$$.rect(el)")?;
        Ok((
            rect.get("x").and_then(Value::as_i64).unwrap_or(0),
            rect.get("y").and_then(Value::as_i64).unwrap_or(0),
        ))
    }

    fn trusted_click(
        &mut self,
        tab_id: i64,
        refid: &str,
        button: &str,
        click_count: i64,
    ) -> Res<Value> {
        let (x, y) = self.point_of(tab_id, refid)?;
        let (button_name, buttons) = match button {
            "middle" => ("middle", 4),
            "right" => ("right", 2),
            _ => ("left", 1),
        };
        self.cdp(
            tab_id,
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":x,"y":y}),
        )?;
        for count in 1..=click_count {
            self.cdp(tab_id, "Input.dispatchMouseEvent", json!({
                "type":"mousePressed","x":x,"y":y,"button":button_name,"buttons":buttons,"clickCount":count,
            }))?;
            self.cdp(tab_id, "Input.dispatchMouseEvent", json!({
                "type":"mouseReleased","x":x,"y":y,"button":button_name,"buttons":0,"clickCount":count,
            }))?;
        }
        Ok(json!({"x":x,"y":y}))
    }
}

struct NamedPipe {
    file: std::fs::File,
    inbuf: Vec<u8>,
}

impl NamedPipe {
    fn connect(path: &str, timeout: Duration) -> Res<Self> {
        let started = Instant::now();
        loop {
            match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        inbuf: Vec::new(),
                    })
                }
                Err(_) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(250)),
                Err(error) => {
                    return Err(BackendError::new(
                        "WEB_BRIDGE_UNAVAILABLE",
                        format!("连接影刀 Chrome Bridge 命名管道失败: {path}: {error}"),
                    )
                    .with_hint("先打开带影刀扩展的 Chrome、Edge 或影刀浏览器"));
                }
            }
        }
    }

    fn send(&mut self, method: &str, params: Value) -> Res<Value> {
        self.write_frame(&json!({"method":method,"options":{},"params":params}))?;
        loop {
            let message = self.read_frame(Duration::from_secs(30))?;
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                match method {
                    "BridgeGetInterfaceVersion" => {
                        self.write_frame(&json!({"method":"Bridge.GetInterfaceVersion","params":{"version":"3.1.1"}}))?;
                    }
                    "BridgeRegisterWindowId" => {
                        static NEXT_HWND: AtomicI64 = AtomicI64::new(1000);
                        let window = message.get("params").cloned().unwrap_or_else(|| json!({}));
                        self.write_frame(&json!({
                            "method":"Bridge.Callback",
                            "requestId":message.get("requestId"),
                            "params":{"wndId":window.get("wndId"),"hwnd":NEXT_HWND.fetch_add(1, Ordering::Relaxed)}
                        }))?;
                    }
                    _ => {}
                }
                continue;
            }
            return response_content(message);
        }
    }

    fn write_frame(&mut self, value: &Value) -> Res<()> {
        let frame = encode_frame(value)?;
        self.file.write_all(&frame).map_err(|error| {
            BackendError::new(
                "WEB_BRIDGE_WRITE_FAILED",
                format!("写入 Chrome Bridge 失败: {error}"),
            )
        })?;
        self.file.flush().map_err(|error| {
            BackendError::new(
                "WEB_BRIDGE_WRITE_FAILED",
                format!("刷新 Chrome Bridge 失败: {error}"),
            )
        })
    }

    fn read_frame(&mut self, timeout: Duration) -> Res<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.inbuf.len() >= 4 {
                let length = u32::from_le_bytes([
                    self.inbuf[0],
                    self.inbuf[1],
                    self.inbuf[2],
                    self.inbuf[3],
                ]) as usize;
                if length > MAX_FRAME_SIZE {
                    return Err(BackendError::new(
                        "WEB_BRIDGE_BAD_RESPONSE",
                        format!("Chrome Bridge 响应帧过大: {length}"),
                    ));
                }
                if self.inbuf.len() >= 4 + length {
                    let payload = self.inbuf[4..4 + length].to_vec();
                    self.inbuf.drain(..4 + length);
                    return decode_payload(&payload);
                }
            }
            if Instant::now() >= deadline {
                return Err(
                    BackendError::new("WEB_BRIDGE_TIMEOUT", "读取 Chrome Bridge 响应超时")
                        .with_hint("操作结果可能未知；先检查页面，不要自动重试"),
                );
            }
            if self.peek_available()? == 0 {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            let mut chunk = [0_u8; 8192];
            let count = self.file.read(&mut chunk).map_err(|error| {
                BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    format!("读取 Chrome Bridge 失败: {error}"),
                )
            })?;
            if count == 0 {
                return Err(BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    "Chrome Bridge 管道已关闭",
                ));
            }
            self.inbuf.extend_from_slice(&chunk[..count]);
        }
    }

    fn peek_available(&self) -> Res<u32> {
        let mut available = 0_u32;
        let handle = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&self.file));
        unsafe { PeekNamedPipe(handle, None, 0, None, Some(&mut available), None) }.map_err(
            |error| {
                BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    format!("检查 Chrome Bridge 响应失败: {error}"),
                )
            },
        )?;
        Ok(available)
    }
}

fn response_content(message: Value) -> Res<Value> {
    if let Some(error) = message
        .pointer("/result/error")
        .filter(|error| !error.is_null())
    {
        let detail = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string());
        return Err(BackendError::new("WEB_BRIDGE_FAILED", detail).with_data(message));
    }
    if message
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 200)
    {
        return Err(
            BackendError::new("WEB_BRIDGE_FAILED", "Chrome Bridge 返回失败").with_data(message),
        );
    }
    Ok(message
        .pointer("/result/content")
        .cloned()
        .unwrap_or(message))
}

fn encode_frame(value: &Value) -> Res<Vec<u8>> {
    let payload = serde_json::to_vec(value).map_err(|error| {
        BackendError::new(
            "WEB_BRIDGE_BAD_REQUEST",
            format!("序列化 Bridge 请求失败: {error}"),
        )
    })?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(BackendError::new(
            "WEB_BRIDGE_BAD_REQUEST",
            "Chrome Bridge 请求帧过大",
        ));
    }
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_payload(payload: &[u8]) -> Res<Value> {
    serde_json::from_slice(payload).map_err(|error| {
        BackendError::new(
            "WEB_BRIDGE_BAD_RESPONSE",
            format!("Chrome Bridge 响应不是有效 JSON: {error}"),
        )
        .with_data(json!({"text":String::from_utf8_lossy(payload)}))
    })
}

fn resolve_pipe(mode: Option<&str>) -> Res<BridgePipeInfo> {
    if let Some(mode) = mode {
        if !matches!(mode, "cef" | "chrome" | "edge") {
            return Err(BackendError::new(
                "WEB_BRIDGE_MODE_UNSUPPORTED",
                format!("Chrome Bridge 不支持浏览器模式: {mode}"),
            )
            .with_hint("Chrome Bridge 支持 cef、chrome、edge"));
        }
    }
    if let Ok(pipe) = std::env::var("XBOT_WEB_PIPE") {
        if !pipe.trim().is_empty() {
            let path = normalize_pipe_path(pipe.trim());
            let name = path.trim_start_matches(r"\\.\pipe\").to_owned();
            let family = pipe_family(&name);
            return Ok(BridgePipeInfo {
                name,
                path,
                family,
                current_user: true,
                multi_profile: true,
            });
        }
    }
    discover_bridge_pipes().into_iter().next().ok_or_else(|| {
        BackendError::new("WEB_BRIDGE_UNAVAILABLE", "未发现影刀 Chrome Bridge 管道")
            .with_hint("先打开带影刀扩展的目标浏览器并访问一个页面，或设置 XBOT_WEB_PIPE")
    })
}

fn discover_bridge_pipes() -> Vec<BridgePipeInfo> {
    let current_suffix = current_user_bridge_suffix();
    let Ok(entries) = fs::read_dir(r"\\.\pipe\") else {
        return Vec::new();
    };
    let mut pipes = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            let family = pipe_family(&name);
            if family.is_empty() {
                return None;
            }
            let current_user = current_suffix
                .as_ref()
                .map(|suffix| name.contains(suffix))
                .unwrap_or(false);
            let multi_profile = name.split('-').count() > 6;
            Some(BridgePipeInfo {
                path: format!(r"\\.\pipe\{name}"),
                name,
                family,
                current_user,
                multi_profile,
            })
        })
        .collect::<Vec<_>>();
    pipes.sort_by(|a, b| {
        bridge_priority(a)
            .cmp(&bridge_priority(b))
            .then_with(|| a.name.cmp(&b.name))
    });
    pipes
}

fn pipe_family(name: &str) -> &'static str {
    if name.starts_with(CHROME_BRIDGE_PREFIX) {
        "chrome.bridge"
    } else if name.starts_with(SHADOWBOT_BROWSER_BRIDGE_PREFIX) {
        "shadowbot.browser.bridge"
    } else if name.starts_with(ALT_BROWSER_BRIDGE_PREFIX) {
        "shadowbot.browser.bridge.alt"
    } else {
        ""
    }
}

fn bridge_priority(pipe: &BridgePipeInfo) -> u8 {
    match (
        pipe.family == "chrome.bridge",
        pipe.current_user,
        pipe.multi_profile,
    ) {
        (true, true, true) => 0,
        (true, true, false) => 1,
        (true, false, true) => 2,
        (true, false, false) => 3,
        (false, true, true) => 4,
        (false, true, false) => 5,
        _ => 6,
    }
}

fn current_user_bridge_suffix() -> Option<String> {
    let username = std::env::var("USERNAME")
        .ok()
        .or_else(|| std::env::var("USER").ok())?;
    if username.trim().is_empty() {
        return None;
    }
    Some(base64::engine::general_purpose::STANDARD.encode(username.as_bytes()))
}

fn normalize_pipe_path(pipe: &str) -> String {
    const PREFIX: &str = r"\\.\pipe\";
    pipe.strip_prefix(PREFIX)
        .map(|name| format!("{PREFIX}{name}"))
        .unwrap_or_else(|| format!("{PREFIX}{pipe}"))
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("JavaScript string serialization")
}

fn wrap_js(body: &str) -> String {
    format!("(function(){{{LIB}\n{body}\n}})()")
}

const LIB: &str = r#"
var $$=window.__xbot||(window.__xbot={
  resolve:function(s){s=String(s);var el=null;
    if(/^(ref=)?e\d+$/.test(s)){var id=s.replace(/^ref=/,'');var found=null;
      (function walk(root){if(found)return;var all=root.querySelectorAll('*');
        for(var i=0;i<all.length;i++){var e=all[i];if(e._ariaRef&&e._ariaRef.ref===id){found=e;return;}if(e.shadowRoot)walk(e.shadowRoot);}})(document);
      return found;}
    if(s.indexOf('xpath=')===0||s.indexOf('//')===0){var xp=s.replace(/^xpath=/,'');el=document.evaluate(xp,document,null,9,null).singleNodeValue;}
    else{el=document.querySelector(s);}return el;},
  must:function(s){var el=$$.resolve(s);if(!el)throw new Error('未找到元素: '+s);return el;},
  rect:function(el){el.scrollIntoView({block:'center',inline:'center'});var r=el.getBoundingClientRect();
    return {x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width),h:Math.round(r.height),left:Math.round(r.left),top:Math.round(r.top)};},
  setval:function(el,v){var P=el.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;
    var d=Object.getOwnPropertyDescriptor(P,'value');d&&d.set?d.set.call(el,v):(el.value=v);},
  fill:function(el,v){el.focus();$$.setval(el,v);el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));return el.value;},
  typeActive:function(v){var el=document.activeElement;
    if(el&&(el.tagName==='INPUT'||el.tagName==='TEXTAREA'||el.isContentEditable)){
      if(el.isContentEditable){el.textContent=(el.textContent||'')+v;el.dispatchEvent(new InputEvent('input',{bubbles:true}));}
      else{el.focus();$$.setval(el,(el.value||'')+v);el.dispatchEvent(new InputEvent('input',{bubbles:true}));}return el.value!==undefined?el.value:el.textContent;}
    throw new Error('当前没有可编辑的聚焦元素;先 click/focus 一个输入框');},
  press:function(el,key){var t=el||document.activeElement||document.body;
    t.dispatchEvent(new KeyboardEvent('keydown',{key:key,bubbles:true,cancelable:true}));
    t.dispatchEvent(new KeyboardEvent('keyup',{key:key,bubbles:true,cancelable:true}));
    if(key==='Enter'&&el&&el.form&&typeof el.form.requestSubmit==='function'){try{el.form.requestSubmit();}catch(e){}}return true;},
  setChecked:function(el,checked){if(el.checked!==checked){el.click();}return el.checked===checked;},
  select:function(el,values){var wanted=Array.isArray(values)?values.map(String):[];
    Array.from(el.options||[]).forEach(function(o){o.selected=wanted.indexOf(o.value)>=0;});
    el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));
    return Array.from(el.selectedOptions||[]).map(function(o){return o.value;});}
});"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_messaging_frames_preserve_json_and_reject_oversized_payloads() {
        let value = json!({"method":"chrome.create","params":{"url":"https://example.com/你好"}});
        let frame = encode_frame(&value).unwrap();
        let length = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(length, frame.len() - 4);
        assert_eq!(decode_payload(&frame[4..]).unwrap(), value);
        let oversized = vec![b'x'; MAX_FRAME_SIZE + 1];
        assert!(decode_payload(&oversized).is_err());
    }

    #[test]
    fn bridge_errors_are_not_successful_content() {
        let error = response_content(json!({"code":500,"result":{"error":{"message":"failed"}}}))
            .unwrap_err();
        assert_eq!(error.code, "WEB_BRIDGE_FAILED");
        assert_eq!(error.message, "failed");
        assert!(response_content(json!({"code":200,"result":{"content":42,"error":null}})).is_ok());
    }

    #[test]
    fn pipe_family_is_metadata_and_priorities_prefer_current_chromium_profile() {
        let chrome = BridgePipeInfo {
            name: "chrome".into(),
            path: "chrome".into(),
            family: "chrome.bridge",
            current_user: true,
            multi_profile: true,
        };
        let alt = BridgePipeInfo {
            family: "shadowbot.browser.bridge.alt",
            ..chrome.clone()
        };
        assert!(bridge_priority(&chrome) < bridge_priority(&alt));
        assert_eq!(
            pipe_family(&format!("{ALT_BROWSER_BRIDGE_PREFIX}-profile")),
            "shadowbot.browser.bridge.alt"
        );
        let full = normalize_pipe_path(ALT_BROWSER_BRIDGE_PREFIX);
        assert_eq!(normalize_pipe_path(&full), full);
    }

    #[test]
    fn bridge_tool_inventory_has_no_mcp_transport_dependency() {
        let names = tools()
            .into_iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "web_create",
                "web_get_active",
                "web_snapshot",
                "web_action",
                "web_evaluate_playwright"
            ]
        );
    }
}
