use std::collections::HashSet;
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
const PYTHON_LOCATOR: &str = include_str!("web_assets/python-locator.js");
const PREVIEW_SNAPSHOT: &str = include_str!("web_assets/preview-snapshot.js");

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
        ("web_xpath", "Generate DOM XPath or unverified ShadowBot context XPath candidates"),
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
            let context = provider.verified_context(tab_id, None);
            let mut data = json!({"success":true,"backend":"chrome-bridge","browser_context":context});
            provider.observe(tab_id, "", false, &mut data)?;
            provider.remember(tab_id, None, &mut data);
            Ok(data)
        }),
        "web_snapshot" => with_provider_at(args.get("pipe").and_then(Value::as_str), |provider| {
            let tab_id = match args.get("tab_id").and_then(Value::as_i64) {
                Some(tab) => tab,
                None => provider.bound_tab()?,
            };
            if args.get("pipe").and_then(Value::as_str).is_some_and(|p| p != provider.pipe_info.path) {
                return Err(BackendError::new("WEB_CONTEXT_CHANGED","Browser pipe changed after action; snapshot not taken"));
            }
            let context = provider.verified_context(tab_id, None);
            let mut data = json!({"success":true,"backend":"chrome-bridge","browser_context":context});
            provider.observe(tab_id, "", true, &mut data)?;
            let snapshot = data["snapshot"].clone();
            data["result"] = snapshot["result"].clone();
            data["acc_tree_meta"] = snapshot["acc_tree_meta"].clone();
            data["frames"] = snapshot["frames"].clone();
            data["frame_warnings"] = snapshot["frame_warnings"].clone();
            data["elements"] = snapshot["elements"].clone();
            data["tree"] = snapshot["tree"].clone();
            data["document_id"] = snapshot["document_id"].clone();
            data.as_object_mut().unwrap().remove("snapshot");
            Ok(data)
        }),
        "web_action" => action(args),
        "web_xpath" => xpath_candidates(&args),
        "web_evaluate_playwright" => evaluate_page(&args),
        _ => Err(BackendError::new(
            "MCP_TOOL_UNKNOWN",
            format!("Chrome Bridge 不支持工具: {tool}"),
        )
        .with_hint("可用工具: web_create, web_get_active, web_snapshot, web_xpath, web_action, web_evaluate_playwright")),
    }
}

fn open(url: &str, mode: Option<&str>, snapshot: bool) -> Res<Value> {
    let mut provider = Provider::connect(mode)?;
    let tab_id = provider.create(url)?;
    let context = provider.verified_context(tab_id, mode);
    let mut data = json!({
        "success": true,
        "backend": "chrome-bridge",
        "url": url,
        "browser_context": context,
    });
    if let Err(error) = provider.observe(tab_id, url, snapshot, &mut data) {
        data["warning"] = observation_warning(error);
    }
    provider.remember(tab_id, None, &mut data);
    Ok(data)
}

fn with_provider(run: impl FnOnce(&mut Provider) -> Res<Value>) -> Res<Value> {
    run(&mut Provider::connect(None)?)
}

fn with_provider_at(
    path: Option<&str>,
    run: impl FnOnce(&mut Provider) -> Res<Value>,
) -> Res<Value> {
    let mut provider = match path {
        Some(path) => Provider::connect_path(path)?,
        None => Provider::connect_bound()?,
    };
    run(&mut provider)
}

fn action(args: Value) -> Res<Value> {
    let target = required_string(&args, "target")?;
    let method = required_string(&args, "method")?;
    let values = args
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    with_provider_at(None, |provider| {
        let tab_id = provider.bound_tab()?;
        let context = provider.verified_context(tab_id, None);
        if target == "locator" {
            provider.validate_ref(tab_id, required_string(&args, "refid")?)?;
        }
        // Capture before the side effect, on this exact tab and provider.
        let locator = if target == "locator" && args["no_python"] != true {
            match provider.python_locator(tab_id, required_string(&args, "refid")?) {
                Ok(locator) => locator,
                Err(error) => json!({"capture_error":{"code":error.code,"message":error.message}}),
            }
        } else {
            Value::Null
        };
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
        if target == "page" && method == "close" {
            crate::web_session::clear();
        }
        Ok(json!({
            "success":true,
            "backend":"chrome-bridge",
            "result":result,
            "browser_context":context,
            "python_locator":locator,
        }))
    })
}

fn evaluate_page(args: &Value) -> Res<Value> {
    let code = required_string(args, "code")?;
    with_provider_at(None, |provider| {
        let tab_id = provider.bound_tab()?;
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

fn attach_frame_tree(node: &mut Value, owner: &str, document: &Value) -> bool {
    if !owner.is_empty() && node["ref"] == owner {
        if let Some(children) = node["children"].as_array_mut() {
            children.push(document.clone());
            return true;
        }
    }
    node["children"].as_array_mut().is_some_and(|children| {
        children
            .iter_mut()
            .any(|child| attach_frame_tree(child, owner, document))
    })
}

fn xpath_candidates(args: &Value) -> Res<Value> {
    let refs = args["refs"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 20)
        .ok_or_else(|| BackendError::new("BAD_INPUT", "请提供 1 到 20 个当前快照 ref"))?;
    with_provider_at(None, |provider| {
        let tab = provider.bound_tab()?;
        let mut locals = Vec::new();
        let mut scope = None;
        let mut scope_set = false;
        for value in refs {
            let id = value
                .as_str()
                .ok_or_else(|| BackendError::new("BAD_INPUT", "ref 必须是字符串"))?;
            provider.validate_ref(tab, id)?;
            let (context, local, _) = provider.ref_context(tab, id)?;
            if scope_set && scope != context {
                return Ok(
                    json!({"status":"unavailable","xpaths":[],"reason":"元素不在同一个 iframe 文档，无法生成相似元素 XPath"}),
                );
            }
            scope = context;
            scope_set = true;
            locals.push(local);
        }
        let script = format!(
            "{PYTHON_LOCATOR}\nreturn window.__xbotPythonXPaths({}.map(id=>$$.must(id)));",
            serde_json::to_string(&locals).unwrap()
        );
        let result = provider.evaluate_in(tab, scope.as_ref(), wrap_js(&script))?;
        if crate::web_python::closed_shadow(&result) {
            return Ok(closed_xpath_result(refs));
        }
        let frame_locator = if scope.is_some() {
            let locator = provider.python_locator(tab, refs[0].as_str().unwrap())?;
            if crate::web_python::closed_shadow(&locator) {
                return Ok(closed_xpath_result(refs));
            }
            Some(locator)
        } else {
            None
        };
        let mut xpaths = Vec::new();
        let shadow = result["shadow"] == true;
        let mut warnings = if shadow {
            vec!["影刀语法，请使用影刀验证；仅以分层 DOM 查询核对目标，未验证影刀执行结果，不是普通浏览器 XPath".to_owned()]
        } else {
            vec![]
        };
        let requires_review = result["candidates"]
            .as_array()
            .is_some_and(|c| !c.is_empty() && c.iter().all(|item| item["requires_review"] == true));
        if requires_review {
            warnings.push(format!(
                "注意：当前候选匹配 {} 个同类元素，未唯一定位到该 ref；请勿直接用于单元素点击。",
                result["candidates"][0]["count"]
            ));
        }
        if let Some(candidates) = result["candidates"].as_array().filter(|c| !c.is_empty()) {
            if scope.is_some() {
                let locator = frame_locator.as_ref().unwrap();
                let steps = locator["steps"].as_array().ok_or_else(|| {
                    BackendError::new("WEB_LOCATOR_CAPTURE", "Missing iframe locator steps")
                })?;
                let Some(last_frame) = steps.iter().rposition(|s| s["kind"] == "frame") else {
                    return Ok(json!({"status":"unavailable","xpaths":[],
                        "reason":"缺少完整的 iframe 层级，无法构建 Selector"}));
                };
                let owners = &steps[..=last_frame];
                if owners.iter().any(|s| {
                    s["requires_review"] == true
                        || matches!(s["quality"].as_str(), Some("weak" | "structural"))
                }) {
                    return Ok(json!({"status":"unavailable","xpaths":[],
                        "reason":"iframe 或外层 Shadow Host 缺少可靠语义锚点，无法生成稳定的 Selector 层级；请补充稳定属性"}));
                }
                let mut composed = Vec::new();
                for candidate in candidates.iter().take(5) {
                    let mut full = owners.to_vec();
                    full.push(json!({"kind":"element","xpath":candidate["xpath"]}));
                    let Some(segments) = crate::web_python::xpath_segments(&full) else {
                        return Ok(json!({"status":"unavailable","xpaths":[],
                            "reason":"iframe XPath 层级不完整，无法构建 Selector"}));
                    };
                    let mut item = candidate.clone();
                    item.as_object_mut().unwrap().remove("xpath");
                    item["local_xpath"] = candidate["xpath"].clone();
                    item["segments"] = json!(segments);
                    item["syntax"] = json!("shadowbot-selector");
                    item["validation"] = json!("requires_shadowbot");
                    item["runtime_verified"] = json!(false);
                    item["segments_scope_verified"] = json!(true);
                    item["unique_verified"] = json!(false);
                    composed.push(item);
                    xpaths.push(serde_json::to_string(&segments).unwrap());
                }
                if !shadow {
                    warnings.push("影刀语法，请使用影刀验证；以下每行是 Selector.xpath.segments 列表，不是可直接传给 find_by_xpath 的一条 XPath".into());
                } else {
                    warnings.push("以下每行是 Selector.xpath.segments 列表；shadow 边界保留在所属文档的 segment 内".into());
                }
                return Ok(json!({
                    "status":if requires_review{"needs_review"}else{"ready"},
                    "kind":if refs.len()==1{"single"}else{"similar"},"refs":refs,
                    "xpaths":xpaths, "xpath_lists":composed.iter().map(|c| c["segments"].clone()).collect::<Vec<_>>(),
                    "candidates":composed, "reason":result["reason"], "warnings":warnings,
                    "requires_review":requires_review, "requires_selector":true,
                    "syntax":"shadowbot-selector","validation":"requires_shadowbot",
                    "context_format":"Selector.xpath.segments: outer iframe -> inner iframe -> target",
                    "iframe_context_required":true,"runtime_verified":false,
                    "browser_context":provider.verified_context(tab,None)
                }));
            }
            for candidate in candidates.iter().take(5) {
                xpaths.push(candidate["xpath"].as_str().unwrap_or("").to_owned());
            }
        }
        Ok(
            json!({"status":if xpaths.is_empty(){"unavailable"}else if requires_review{"needs_review"}else{"ready"},
            "kind":if refs.len()==1{"single"}else{"similar"},"refs":refs,
            "xpaths":xpaths,"candidates":result["candidates"],"reason":result["reason"],
            "syntax":if shadow{"shadowbot"}else{"xpath1"},
            "validation":if shadow{"requires_shadowbot"}else{"browser_verified"},
            "runtime_verified":false,
            "warnings":warnings, "requires_review":requires_review,
            "context_format":"ordinary document: XPath; open ShadowRoot: ShadowBot XPath string; iframe: Selector.xpath.segments list",
            "browser_context":provider.verified_context(tab,None)}),
        )
    })
}

fn closed_xpath_result(refs: &[Value]) -> Value {
    json!({"status":"unavailable","xpaths":[],"candidates":[],"refs":refs,
        "error_code":"WEB_SHADOW_CLOSED","closed_shadow":true,
        "reason":crate::web_python::CLOSED_SHADOW_REASON,
        "warnings":[crate::web_python::CLOSED_SHADOW_REASON],"runtime_verified":false})
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
    attached_targets: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FrameContext {
    id: i64,
    target: Option<String>,
}

impl FrameContext {
    fn from_frame(frame: &Value) -> Option<Self> {
        Some(Self {
            id: frame["context_id"].as_i64()?,
            target: frame["target_id"].as_str().map(str::to_owned),
        })
    }
}

impl Provider {
    fn connect_bound() -> Res<Self> {
        match crate::web_session::load()? {
            Some(binding) => Self::connect_path(&binding.pipe).map_err(|e| e.with_hint(
                "绑定的浏览器已断开；请运行 web active 或 web open 重新绑定，不会自动切换其他浏览器")),
            None => Self::connect(None),
        }
    }

    fn bound_tab(&mut self) -> Res<i64> {
        Ok(match crate::web_session::load()? {
            Some(binding) if binding.pipe == self.pipe_info.path => binding.tab_id,
            _ => return self.active_tab(),
        })
    }

    fn remember(&mut self, tab: i64, document: Option<&str>, data: &mut Value) {
        if let Err(error) = crate::web_session::save(&self.pipe_info.path, tab, document) {
            data["session_warning"] = json!(format!("无法保存浏览器会话: {error}"));
        }
    }

    fn validate_ref(&mut self, tab: i64, refid: &str) -> Res<()> {
        if !(refid.starts_with('e') || refid.starts_with('f') || refid.starts_with("ref=")) {
            return Ok(());
        }
        if let Some(binding) = crate::web_session::load()? {
            if let Some(expected) = binding.document_id {
                let actual = self.evaluate(tab, "window.__xbotPreviewDocumentId||null".into())?;
                if actual.as_str() != Some(&expected) {
                    return Err(BackendError::new(
                        "WEB_REF_STALE",
                        "页面已导航或刷新，旧快照编号已失效",
                    )
                    .with_hint("先运行 web snapshot 获取当前页面编号；未执行本次元素操作"));
                }
            }
        }
        let (context, local, _) = self.ref_context(tab, refid)?;
        self.evaluate_in(
            tab,
            context.as_ref(),
            wrap_js(&format!("return Boolean($$.must({}));", js_string(&local))),
        )
        .map(|_| ())
        .map_err(|e| {
            if e.code == "WEB_BRIDGE_EVAL_FAILED" {
                BackendError::new(
                    "WEB_REF_STALE",
                    format!("快照编号 {refid} 已失效或不在当前页面；元素可能已被替换"),
                )
                .with_hint(
                    "先运行 web snapshot 重新采集编号；未执行本次元素操作，不会自动改点其他元素",
                )
            } else {
                e
            }
        })
    }

    fn connect(mode: Option<&str>) -> Res<Self> {
        let pipe_info = resolve_pipe(mode)?;
        Self::connect_info(pipe_info)
    }

    fn connect_path(path: &str) -> Res<Self> {
        let path = normalize_pipe_path(path);
        let name = path.trim_start_matches(r"\\.\pipe\").to_owned();
        let family = pipe_family(&name);
        Self::connect_info(BridgePipeInfo {
            name,
            path,
            family,
            current_user: true,
            multi_profile: false,
        })
    }

    fn connect_info(pipe_info: BridgePipeInfo) -> Res<Self> {
        let mut provider = Self {
            pipe: NamedPipe::connect(&pipe_info.path, Duration::from_secs(8))?,
            pipe_info,
            extension_version: Value::Null,
            attached_targets: HashSet::new(),
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

    fn verified_context(&mut self, tab_id: i64, mode: Option<&str>) -> Value {
        let mut context = self.context(tab_id, mode);
        // Pipe family is transport metadata, not evidence of browser product.
        if let Ok(info) = self.evaluate(tab_id, "JSON.stringify({ua:navigator.userAgent,brands:navigator.userAgentData?navigator.userAgentData.brands:[]})".into()) {
            if let Some(info) = info.as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()) {
                let ua = info["ua"].as_str().unwrap_or("");
                let brands = info["brands"].to_string();
                let detected = if ua.contains("Edg/") { Some(("edge", "Microsoft Edge")) }
                    else if brands.contains("Google Chrome") { Some(("chrome", "Google Chrome")) }
                    else { None };
                if let Some((sdk, product)) = detected {
                    context["sdk_mode"] = json!(sdk);
                    context["product_name"] = json!(product);
                    context["mode_source"] = json!("navigator.userAgent/userAgentData");
                }
            }
        }
        context
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
        if let Ok(tab) = std::env::var("XBOT_WEB_TAB_ID") {
            return tab.parse::<i64>().ok().filter(|id| *id > 0).ok_or_else(|| {
                BackendError::new("BAD_INPUT", "XBOT_WEB_TAB_ID must be a positive tab id")
            });
        }
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
                "result": aria["text"],
                "elements": aria["elements"],
                "tree": aria["tree"],
                "document_id": aria["document_id"],
                "acc_tree_meta": {"kind":"full"},
            });
            self.remember(tab_id, aria["document_id"].as_str(), data);
            // Individual unavailable frames do not discard the top document.
            match self.frame_snapshots(tab_id) {
                Ok((frames, warnings)) => {
                    for frame in &frames {
                        if let Some(text) = frame["snapshot"].as_str() {
                            let tree = data["snapshot"]["result"].as_str().unwrap_or("");
                            data["snapshot"]["result"] = json!(format!(
                                "{tree}\n\nFrame {} ({})\n{text}",
                                frame["prefix"].as_str().unwrap_or(""),
                                frame["url"].as_str().unwrap_or("")
                            ));
                        }
                        if let Some(items) = frame["elements"].as_array() {
                            if let Some(all) = data["snapshot"]["elements"].as_array_mut() {
                                all.extend(items.iter().cloned());
                            }
                        }
                    }
                    let mut nested = frames.clone();
                    for i in (0..nested.len()).rev() {
                        if !nested[i]["tree"].is_object() {
                            continue;
                        }
                        let document = nested[i]["tree"].clone();
                        let owner = nested[i]["owner_ref"].as_str().unwrap_or("").to_owned();
                        let parent_id = nested[i]["parent_id"].clone();
                        let parent = nested.iter().position(|f| f["frame_id"] == parent_id);
                        let target = match parent {
                            Some(p) => &mut nested[p]["tree"],
                            None => &mut data["snapshot"]["tree"],
                        };
                        if !attach_frame_tree(target, &owner, &document) {
                            if let Some(children) =
                                data["snapshot"]["tree"]["children"].as_array_mut()
                            {
                                let mut detached = document;
                                detached["name"] = json!(format!(
                                    "{} (owner ref unavailable; separate frame document)",
                                    detached["name"].as_str().unwrap_or("")
                                ));
                                children.push(detached);
                            }
                        }
                    }
                    data["snapshot"]["frames"] = json!(frames);
                    data["snapshot"]["frame_warnings"] = json!(warnings);
                }
                Err(error) => data["snapshot"]["frame_warnings"] = json!([error.message]),
            }
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
        self.cdp_at(tab_id, None, method, command_param)
    }

    fn cdp_at(
        &mut self,
        tab_id: i64,
        target: Option<&str>,
        method: &str,
        command_param: Value,
    ) -> Res<Value> {
        self.ensure_bridge_engine()?;
        if let Some(target) = target {
            if !self.attached_targets.contains(target) {
                self.pipe.send("chrome.executeJavaScriptInBackground", json!({"code":format!(
                    "function(){{return new Promise((resolve,reject)=>chrome.debugger.attach({{targetId:{}}},'1.3',()=>chrome.runtime.lastError?reject(Error(chrome.runtime.lastError.message)):resolve(true)));}}",
                    js_string(target)
                )}))?;
                self.attached_targets.insert(target.to_owned());
            }
            // The extension's specified-session wrapper discards command results.
            // Use its existing background execution API with a target-scoped Debuggee.
            return self.pipe.send("chrome.executeJavaScriptInBackground", json!({"code":format!(
                "function(){{return new Promise((resolve,reject)=>chrome.debugger.sendCommand({{targetId:{}}},{},{},r=>chrome.runtime.lastError?reject(Error(chrome.runtime.lastError.message)):resolve(r)));}}",
                js_string(target),js_string(method),command_param
            )}));
        }
        self.pipe.send(
            "debugger.sendCommand",
            json!({"tabId":tab_id,"method":method,"commandParam":command_param}),
        )
    }

    fn evaluate(&mut self, tab_id: i64, expression: String) -> Res<Value> {
        self.evaluate_in(tab_id, None, expression)
    }

    fn evaluate_in(
        &mut self,
        tab_id: i64,
        context: Option<&FrameContext>,
        expression: String,
    ) -> Res<Value> {
        let mut params = json!({
            "expression":expression,"returnByValue":true,"awaitPromise":true,"userGesture":true,
        });
        if let Some(context) = context {
            params["contextId"] = json!(context.id);
        }
        let response = self.cdp_at(
            tab_id,
            context.and_then(|c| c.target.as_deref()),
            "Runtime.evaluate",
            params,
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

    fn aria_snapshot(&mut self, tab_id: i64) -> Res<Value> {
        self.aria_snapshot_in(tab_id, None)
    }

    fn aria_snapshot_in(&mut self, tab_id: i64, context: Option<&FrameContext>) -> Res<Value> {
        let inject = format!(
            "(function(){{try{{if(window.__xbotPreviewIS)return 'cached';var __pwM={{exports:{{}}}};(function(module,exports){{\n{}\n}})(__pwM,__pwM.exports);var __IS=__pwM.exports.InjectedScript;if(__IS&&!__IS.prototype)__IS=__IS();window.__xbotPreviewIS=new __IS(window,{{isUnderTest:false,sdkLanguage:'javascript',testIdAttributeName:'data-testid',stableRafCount:1,browserName:'chromium',customEngines:[],isUtilityWorld:false}});return 'injected';}}catch(e){{return 'ERR:'+(e&&e.stack||e);}}}})()",
            PW_ARIA
        );
        if let Some(error) = self
            .evaluate_in(tab_id, context, inject)?
            .as_str()
            .and_then(|value| value.strip_prefix("ERR:"))
            .map(str::to_owned)
        {
            return Err(BackendError::new("WEB_BRIDGE_SNAPSHOT_FAILED", error));
        }
        let result = self.evaluate_in(tab_id, context, PREVIEW_SNAPSHOT.into())?;
        if let Some(text) = result.as_str() {
            return Ok(json!({"text":text,"elements":[]}));
        }
        if result["text"].is_string() {
            Ok(result)
        } else {
            Err(BackendError::new(
                "WEB_BRIDGE_BAD_RESPONSE",
                "ARIA 快照未返回文本",
            ))
        }
    }

    fn el_eval(&mut self, tab_id: i64, refid: &str, expression: &str) -> Res<Value> {
        let (context, local_ref, _) = self.ref_context(tab_id, refid)?;
        self.evaluate_in(
            tab_id,
            context.as_ref(),
            wrap_js(&format!(
                "var el=$$.must({});return ({});",
                js_string(&local_ref),
                expression
            )),
        )
    }

    fn point_of(&mut self, tab_id: i64, refid: &str) -> Res<(i64, i64)> {
        let (_, _, frame) = self.ref_context(tab_id, refid)?;
        // Scroll outer frames first; final rectangles are read after scrolling.
        let chain = self.frame_chain(tab_id, &frame)?;
        for item in chain.iter().rev() {
            self.frame_owner(tab_id, item, true, true)?;
        }
        let rect = self.el_eval(tab_id, refid, "$$.rect(el)")?;
        let mut x = rect["x"]
            .as_i64()
            .ok_or_else(|| BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "Missing element x"))?;
        let mut y = rect["y"]
            .as_i64()
            .ok_or_else(|| BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "Missing element y"))?;
        for item in &chain {
            let owner = self.frame_owner(tab_id, item, false, true)?;
            let r = &owner["rect"];
            self.check_frame_hit(tab_id, item, x, y)?;
            x += r["x"]
                .as_i64()
                .ok_or_else(|| BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "Missing frame x"))?;
            y += r["y"]
                .as_i64()
                .ok_or_else(|| BackendError::new("WEB_BRIDGE_BAD_RESPONSE", "Missing frame y"))?;
        }
        Ok((x, y))
    }

    fn ref_context(&mut self, tab: i64, refid: &str) -> Res<(Option<FrameContext>, String, Value)> {
        let refid = refid.strip_prefix("ref=").unwrap_or(refid);
        if let Some((prefix, digits)) = refid.split_once('e').filter(|(p, n)| {
            p.starts_with('f')
                && p[1..].chars().all(|c| c.is_ascii_digit())
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
        }) {
            let frames = self.evaluate(tab, "window.__xbotPreviewFrames||[]".into())?;
            let frame = frames
                .as_array()
                .and_then(|frames| frames.iter().find(|f| f["prefix"] == prefix))
                .cloned()
                .ok_or_else(|| {
                    BackendError::new(
                        "WEB_REF_STALE",
                        "Frame ref is not in this page's latest snapshot",
                    )
                })?;
            let context = FrameContext::from_frame(&frame).ok_or_else(|| {
                BackendError::new("WEB_REF_STALE", "Missing frame execution context")
            })?;
            let actual = self
                .evaluate_in(
                    tab,
                    Some(&context),
                    "window.__xbotPreviewDocumentId||null".into(),
                )
                .map_err(|error| {
                    BackendError::new(
                        "WEB_REF_STALE",
                        "iframe 上下文已失效或暂不可读取；未执行本次元素操作",
                    )
                    .with_hint("先运行 web snapshot 重新采集；不会自动重试元素动作")
                    .with_data(json!({"cause":{"code":error.code,"message":error.message}}))
                })?;
            if actual != frame["document_id"] || actual.is_null() {
                return Err(BackendError::new(
                    "WEB_REF_STALE",
                    "iframe 已导航或刷新，旧编号已失效",
                )
                .with_hint("先运行 web snapshot，使用新快照中的完整 frame ref"));
            }
            return Ok((Some(context), format!("e{digits}"), frame));
        }
        Ok((None, refid.to_owned(), Value::Null))
    }

    fn frame_chain(&mut self, tab: i64, frame: &Value) -> Res<Vec<Value>> {
        let mut chain = Vec::new();
        if frame.is_null() {
            return Ok(chain);
        }
        let frames = self.evaluate(tab, "window.__xbotPreviewFrames||[]".into())?;
        let mut current = frame.clone();
        for _ in 0..32 {
            chain.push(current.clone());
            let parent = &current["parent_id"];
            match frames
                .as_array()
                .and_then(|fs| fs.iter().find(|f| &f["frame_id"] == parent))
            {
                Some(parent) => current = parent.clone(),
                None if current["parent_id"] == current["top_id"] => return Ok(chain),
                None => {
                    return Err(BackendError::new(
                        "WEB_REF_STALE",
                        "Parent frame no longer belongs to the snapshot",
                    ))
                }
            }
        }
        Err(BackendError::new(
            "WEB_FRAME_DEPTH",
            "Frame nesting exceeds 32",
        ))
    }

    fn frame_owner(&mut self, tab: i64, frame: &Value, scroll: bool, geometry: bool) -> Res<Value> {
        let target = frame["parent_target_id"].as_str();
        let owner = self.cdp_at(
            tab,
            target,
            "DOM.getFrameOwner",
            json!({"frameId":frame["frame_id"]}),
        )?;
        let mut params = json!({"backendNodeId":owner["backendNodeId"]});
        if let Some(context) = frame["parent_context_id"].as_i64() {
            params["executionContextId"] = json!(context);
        }
        let node = self.cdp_at(tab, target, "DOM.resolveNode", params)?;
        let object = node
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BackendError::new("WEB_FRAME_UNAVAILABLE", "Frame owner cannot be resolved")
            })?
            .to_owned();
        let script = format!("function(){{{PYTHON_LOCATOR}\nif({scroll})this.scrollIntoView({{block:'center',inline:'center'}});var r=this.getBoundingClientRect();if({geometry}){{for(var n=this;n;n=n.parentElement||n.getRootNode().host)if(getComputedStyle(n).transform!=='none')throw Error('Transformed iframe/ancestor is unsupported');if(Math.abs(r.width-this.offsetWidth)>1.5||Math.abs(r.height-this.offsetHeight)>1.5)throw Error('Scaled iframe is unsupported');}}return {{locator:window.__xbotPythonLocator(this),rect:{{x:Math.round(r.left+this.clientLeft),y:Math.round(r.top+this.clientTop)}}}};}}");
        let response = self.cdp_at(
            tab,
            target,
            "Runtime.callFunctionOn",
            json!({"objectId":object,"functionDeclaration":script,"returnByValue":true}),
        );
        let _ = self.cdp_at(
            tab,
            target,
            "Runtime.releaseObject",
            json!({"objectId":object}),
        );
        let response = response?;
        if response.get("exceptionDetails").is_some() {
            return Err(BackendError::new(
                "WEB_FRAME_UNAVAILABLE",
                response["exceptionDetails"].to_string(),
            ));
        }
        response.pointer("/result/value").cloned().ok_or_else(|| {
            BackendError::new("WEB_FRAME_UNAVAILABLE", "Missing frame owner information")
        })
    }

    fn check_frame_hit(&mut self, tab: i64, frame: &Value, x: i64, y: i64) -> Res<()> {
        let target = frame["parent_target_id"].as_str();
        let owner = self.cdp_at(
            tab,
            target,
            "DOM.getFrameOwner",
            json!({"frameId":frame["frame_id"]}),
        )?;
        let mut params = json!({"backendNodeId":owner["backendNodeId"]});
        if let Some(context) = frame["parent_context_id"].as_i64() {
            params["executionContextId"] = json!(context);
        }
        let node = self.cdp_at(tab, target, "DOM.resolveNode", params)?;
        let object = node
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| BackendError::new("WEB_FRAME_UNAVAILABLE", "Missing frame owner"))?
            .to_owned();
        let code = format!("function(){{var r=this.getBoundingClientRect(),x=r.left+this.clientLeft+{x},y=r.top+this.clientTop+{y};var hit=document.elementFromPoint(x,y);while(hit&&hit.shadowRoot){{var n=hit.shadowRoot.elementFromPoint(x,y);if(!n||n===hit)break;hit=n;}}return {x}>=0&&{y}>=0&&{x}<this.clientWidth&&{y}<this.clientHeight&&hit===this;}}");
        let response = self.cdp_at(
            tab,
            target,
            "Runtime.callFunctionOn",
            json!({"objectId":object,"functionDeclaration":code,"returnByValue":true}),
        );
        let _ = self.cdp_at(
            tab,
            target,
            "Runtime.releaseObject",
            json!({"objectId":object}),
        );
        if response?.pointer("/result/value").and_then(Value::as_bool) != Some(true) {
            return Err(BackendError::new(
                "WEB_FRAME_OBSCURED",
                "Frame target is clipped or obscured; no mouse event dispatched",
            ));
        }
        Ok(())
    }

    fn python_locator(&mut self, tab: i64, refid: &str) -> Res<Value> {
        let (context, local, frame) = self.ref_context(tab, refid)?;
        let mut locator = self.evaluate_in(
            tab,
            context.as_ref(),
            wrap_js(&format!(
                "{PYTHON_LOCATOR}\nreturn window.__xbotPythonLocator($$.must({}));",
                js_string(&local)
            )),
        )?;
        if !locator.is_object() {
            return Err(BackendError::new(
                "WEB_LOCATOR_CAPTURE",
                "DOM locator did not return an object",
            ));
        }
        locator["refid"] = json!(refid);
        if crate::web_python::closed_shadow(&locator) {
            return Ok(locator);
        }
        let chain = self.frame_chain(tab, &frame)?;
        let mut steps = Vec::new();
        let top_origin = self.evaluate(tab, "location.origin".into())?;
        let mut cross = locator["origin"] != top_origin;
        let mut shadow_frame = false;
        for item in chain.iter().rev() {
            let owner = self.frame_owner(tab, item, false, false)?;
            if crate::web_python::closed_shadow(&owner["locator"]) {
                return Ok(json!({"refid":refid,"steps":[],"closed_shadow":true,
                    "unsupported":"closed-shadow-root","error_code":"WEB_SHADOW_CLOSED",
                    "reason":crate::web_python::CLOSED_SHADOW_REASON,
                    "warnings":[crate::web_python::CLOSED_SHADOW_REASON]}));
            }
            cross |= owner["locator"]["origin"] != top_origin;
            let mut owner_steps =
                owner["locator"]["steps"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| {
                        BackendError::new("WEB_LOCATOR_CAPTURE", "Missing frame locator steps")
                    })?;
            if owner_steps.is_empty() {
                return Err(BackendError::new(
                    "WEB_LOCATOR_CAPTURE",
                    "Missing frame owner locator; refusing an incomplete Selector chain",
                ));
            }
            shadow_frame |= owner_steps.iter().any(|s| s["kind"] == "shadow");
            if let Some(last) = owner_steps.last_mut() {
                last["kind"] = json!("frame");
            }
            steps.extend(owner_steps);
        }
        steps.extend(locator["steps"].as_array().cloned().unwrap_or_default());
        locator["steps"] = json!(steps);
        locator["cross_origin"] = json!(cross);
        locator["shadow_frame"] = json!(shadow_frame);
        if steps.iter().any(|s| s["kind"] == "frame") {
            if let Some(segments) = crate::web_python::xpath_segments(&steps) {
                locator["selector_segments"] = json!(segments);
                locator["selector_validation"] = json!("requires_shadowbot");
            }
        }
        if steps.len() > 1
            && !steps.iter().any(|s| s["kind"] == "frame")
            && steps.iter().all(|s| s["requires_review"] != true)
        {
            let mut tail = String::new();
            for step in &steps {
                tail.push_str(step["xpath"].as_str().unwrap_or(""));
                if step["kind"] == "shadow" {
                    tail.push_str("/xbotShadowRoot");
                }
            }
            locator["shadowbot_xpath"] = json!(tail);
            locator["shadowbot_validation"] = json!("requires_shadowbot");
        }
        Ok(locator)
    }

    fn frame_boundaries(&mut self, tab: i64, context: Option<&FrameContext>) -> Res<Vec<Value>> {
        let target = context.and_then(|c| c.target.as_deref());
        let mut params = json!({"expression":r#"(()=>{
            const elements=[];
            function walk(root){
                for(const el of root.querySelectorAll('*')){
                    if(el.matches('iframe,frame'))elements.push(el);
                    if(elements.length>=65)return;
                    if(el.shadowRoot)walk(el.shadowRoot);
                    if(elements.length>=65)return;
                }
            }
            walk(document);return elements;
        })()"#,"objectGroup":"xbot-preview-frame-scan"});
        if let Some(context) = context {
            params["contextId"] = json!(context.id);
        }
        let array = self.cdp_at(tab, target, "Runtime.evaluate", params)?;
        let object = array
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BackendError::new("WEB_FRAME_UNAVAILABLE", "Cannot enumerate iframe owners")
            })?;
        let result = (|| {
            let properties = self.cdp_at(
                tab,
                target,
                "Runtime.getProperties",
                json!({"objectId":object,"ownProperties":true}),
            )?;
            let mut frames = Vec::new();
            for property in properties["result"].as_array().into_iter().flatten() {
                if !property["name"]
                    .as_str()
                    .is_some_and(|n| n.parse::<usize>().is_ok())
                {
                    continue;
                }
                let Some(object) = property.pointer("/value/objectId").and_then(Value::as_str)
                else {
                    continue;
                };
                let described = self.cdp_at(
                    tab,
                    target,
                    "DOM.describeNode",
                    json!({"objectId":object,"depth":0}),
                )?;
                let metadata = self.cdp_at(tab, target, "Runtime.callFunctionOn", json!({
                    "objectId":object,"returnByValue":true,
                    "functionDeclaration":"function(){let ref=null;for(const [id,el] of window.__xbotPreviewRefs||[])if(el===this){ref=id;break;}return {url:this.src||'about:blank',local_owner_ref:ref,shadow:Boolean(this.getRootNode().host)};}"
                }))?;
                let mut frame = metadata
                    .pointer("/result/value")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                frame["frame_id"] = described["node"]["frameId"].clone();
                frames.push(frame);
            }
            Ok(frames)
        })();
        let _ = self.cdp_at(
            tab,
            target,
            "Runtime.releaseObjectGroup",
            json!({"objectGroup":"xbot-preview-frame-scan"}),
        );
        result
    }

    fn frame_snapshots(&mut self, tab: i64) -> Res<(Vec<Value>, Vec<String>)> {
        let tree = self.cdp(tab, "Page.getFrameTree", json!({}))?;
        let top_id = tree["frameTree"]["frame"]["id"].clone();
        if !top_id.is_string() {
            return Err(BackendError::new(
                "WEB_FRAME_UNAVAILABLE",
                "Missing top-level frame identity",
            ));
        }
        let old = self.evaluate(tab, "window.__xbotPreviewFrames||[]".into())?;
        let previous_counter = self
            .evaluate(tab, "window.__xbotPreviewNextFrame||0".into())?
            .as_u64()
            .unwrap_or(0);
        let mut next = old
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f["prefix"].as_str()?.strip_prefix('f')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .max(previous_counter);
        let mut frames: Vec<Value> = Vec::new();
        let mut warnings = Vec::new();
        let mut scopes =
            std::collections::VecDeque::from([(None, top_id.clone(), String::new(), 0)]);
        let mut seen = HashSet::from([top_id.clone().to_string()]);
        let mut attempted = 0;
        'scopes: while let Some((parent_context, parent_id, parent_prefix, depth)) =
            scopes.pop_front()
        {
            let parent_context: Option<FrameContext> = parent_context;
            let boundaries = match self.frame_boundaries(tab, parent_context.as_ref()) {
                Ok(found) => found,
                Err(error) => {
                    warnings.push(format!(
                        "Frame scan failed ({parent_prefix}): {}",
                        error.message
                    ));
                    continue;
                }
            };
            for mut frame in boundaries {
                if attempted >= 64 {
                    warnings.push("Frame snapshot limit reached (64 subframes / 32 levels)".into());
                    break 'scopes;
                }
                if depth >= 32 {
                    warnings.push("Frame nesting limit reached (32 levels)".into());
                    break;
                }
                attempted += 1;
                let Some(frame_id) = frame["frame_id"].as_str().map(str::to_owned) else {
                    warnings.push(format!("iframe 尚未附着可读取的文档: {}", frame["url"]));
                    continue;
                };
                if !seen.insert(frame_id.clone()) {
                    continue;
                }
                frame["top_id"] = top_id.clone();
                frame["parent_id"] = parent_id.clone();
                frame["parent_context_id"] = parent_context
                    .as_ref()
                    .map(|c| json!(c.id))
                    .unwrap_or(Value::Null);
                frame["parent_target_id"] =
                    json!(parent_context.as_ref().and_then(|c| c.target.as_deref()));
                if let Some(id) = frame["local_owner_ref"].as_str() {
                    frame["owner_ref"] = json!(format!("{parent_prefix}{id}"));
                }
                let mut target = parent_context.as_ref().and_then(|c| c.target.clone());
                let world_params = json!({"frameId":frame_id,"worldName":"xbot-cli-python-preview","grantUniveralAccess":false});
                let context = self
                    .cdp_at(
                        tab,
                        target.as_deref(),
                        "Page.createIsolatedWorld",
                        world_params.clone(),
                    )
                    .and_then(|r| {
                        r["executionContextId"].as_i64().ok_or_else(|| {
                            BackendError::new("WEB_FRAME_UNAVAILABLE", "Missing frame world")
                        })
                    });
                let context = match context {
                    Ok(id) => Ok(id),
                    Err(_) => {
                        // A DOM-owned frame omitted from this renderer may be an OOPIF.
                        // Only attach its exact frame target, never a URL-matched tab.
                        target = Some(frame_id.clone());
                        self.cdp_at(tab, target.as_deref(), "Page.getFrameTree", json!({}))
                            .and_then(|t| {
                                if t["frameTree"]["frame"]["id"] != frame_id
                                    || t["frameTree"]["frame"]
                                        .get("parentId")
                                        .is_some_and(|p| p != &parent_id)
                                {
                                    return Err(BackendError::new(
                                        "WEB_FRAME_UNAVAILABLE",
                                        "Frame target identity changed",
                                    ));
                                }
                                self.cdp_at(
                                    tab,
                                    target.as_deref(),
                                    "Page.createIsolatedWorld",
                                    world_params,
                                )
                                .and_then(|r| {
                                    r["executionContextId"].as_i64().ok_or_else(|| {
                                        BackendError::new(
                                            "WEB_FRAME_UNAVAILABLE",
                                            "Missing OOPIF world",
                                        )
                                    })
                                })
                            })
                    }
                };
                let context = match context {
                    Ok(id) => FrameContext { id, target },
                    Err(error) => {
                        warnings.push(format!(
                            "iframe 文档不可读取 {}: {}",
                            frame["url"], error.message
                        ));
                        continue;
                    }
                };
                let mut captured = match self.aria_snapshot_in(tab, Some(&context)) {
                    Ok(captured) => captured,
                    Err(error) => {
                        warnings.push(format!("{}: {}", frame["url"], error.message));
                        continue;
                    }
                };
                frame["owner_url"] = frame["url"].clone();
                if let Some(url) = captured["url"].as_str() {
                    frame["url"] = json!(url);
                }
                frame["ready_state"] = captured["ready_state"].clone();
                if frame["ready_state"] == "loading"
                    || (frame["url"] == "about:blank" && frame["owner_url"] != "about:blank")
                {
                    warnings.push(format!(
                        "iframe 正在加载 {}；请稍后运行 web snapshot 重新采集，不要重复刚才的操作",
                        frame["owner_url"]
                    ));
                }
                frame["document_id"] = captured["document_id"].clone();
                if frame["document_id"].is_null() {
                    warnings.push(format!("iframe 缺少文档代次: {}", frame["url"]));
                    continue;
                }
                let prefix = old
                    .as_array()
                    .and_then(|fs| {
                        fs.iter().find(|f| {
                            f["frame_id"] == frame["frame_id"]
                                && f["context_id"] == context.id
                                && f["target_id"] == json!(context.target)
                                && f["document_id"] == frame["document_id"]
                        })
                    })
                    .and_then(|f| f["prefix"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        next += 1;
                        format!("f{next}")
                    });
                frame["prefix"] = json!(prefix);
                frame["context_id"] = json!(context.id);
                frame["target_id"] = json!(context.target);
                frame["snapshot"] = json!(prefix_refs(
                    captured["text"].as_str().unwrap_or(""),
                    &prefix
                ));
                for item in captured["elements"].as_array_mut().into_iter().flatten() {
                    scope_tree(item, &prefix);
                    item["frame"] = json!(prefix);
                }
                frame["elements"] = captured["elements"].take();
                fn scope_tree(node: &mut Value, prefix: &str) {
                    if let Some(id) = node["ref"].as_str() {
                        node["ref"] = json!(format!("{prefix}{id}"));
                    }
                    for host in node["shadow_hosts"].as_array_mut().into_iter().flatten() {
                        if let Some(id) = host.as_str().filter(|s| {
                            s.starts_with('e') && s[1..].chars().all(|c| c.is_ascii_digit())
                        }) {
                            *host = json!(format!("{prefix}{id}"));
                        }
                    }
                    for child in node
                        .get_mut("children")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                    {
                        scope_tree(child, prefix);
                    }
                }
                scope_tree(&mut captured["tree"], &prefix);
                frame["tree"] = json!({"role":"document","name":format!("{} {}",prefix,frame["url"].as_str().unwrap_or("")),
                        "children":[captured["tree"]]});
                scopes.push_back((Some(context), frame["frame_id"].clone(), prefix, depth + 1));
                frames.push(frame);
            }
        }
        let registry: Vec<_> = frames
            .iter()
            .map(|f| {
                let mut f = f.clone();
                f.as_object_mut().unwrap().remove("snapshot");
                f.as_object_mut().unwrap().remove("elements");
                f.as_object_mut().unwrap().remove("tree");
                f
            })
            .collect();
        self.evaluate(
            tab,
            format!(
                "window.__xbotPreviewFrames={};window.__xbotPreviewNextFrame={next};true",
                serde_json::to_string(&registry).unwrap(),
            ),
        )?;
        Ok((frames, warnings))
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

impl Drop for Provider {
    fn drop(&mut self) {
        for target in &self.attached_targets {
            let _ = self.pipe.send("chrome.executeJavaScriptInBackground", json!({"code":format!(
                "function(){{return new Promise(resolve=>chrome.debugger.detach({{targetId:{}}},()=>{{void chrome.runtime.lastError;resolve(true);}}));}}",
                js_string(target)
            )}));
        }
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

fn prefix_refs(text: &str, prefix: &str) -> String {
    // Rewrite only ARIA ref markers, never visible page text or names.
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("[ref=e") {
        output.push_str(&rest[..index]);
        rest = &rest[index + 6..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest.as_bytes().get(digits) == Some(&b']') {
            output.push_str(&format!("[ref={prefix}e{}]", &rest[..digits]));
            rest = &rest[digits + 1..];
        } else {
            output.push_str("[ref=e");
        }
    }
    output.push_str(rest);
    output
}

const LIB: &str = r#"
var $$=window.__xbotPythonPreviewHelpersV2||(window.__xbotPythonPreviewHelpersV2={
  resolve:function(s){s=String(s);var el=null;
    if(/^(ref=)?e\d+$/.test(s)){var id=s.replace(/^ref=/,'');
      var found=window.__xbotPreviewRefs&&window.__xbotPreviewRefs.get(id);
      return found&&found.isConnected&&found.ownerDocument===document?found:null;}
    if(s.indexOf('xpath=')===0||s.indexOf('//')===0){var xp=s.replace(/^xpath=/,'');el=document.evaluate(xp,document,null,9,null).singleNodeValue;}
    else{el=document.querySelector(s);}return el;},
  must:function(s){var el=$$.resolve(s);if(!el)throw new Error('未找到元素: '+s);
    if(/^(ref=)?e\d+$/.test(s)&&!el.getClientRects().length)throw new Error('快照元素已隐藏: '+s);return el;},
  rect:function(el){el.scrollIntoView({block:'center',inline:'center'});var r=el.getBoundingClientRect();
    if(r.width<=0||r.height<=0)throw new Error('Element is not visible');
    var hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);
    while(hit&&hit.shadowRoot){var next=hit.shadowRoot.elementFromPoint(r.left+r.width/2,r.top+r.height/2);if(!next||next===hit)break;hit=next;}
    if(hit!==el&&!el.contains(hit))throw new Error('Element center is obscured; no click dispatched');
    return {x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),w:Math.round(r.width),h:Math.round(r.height),left:Math.round(r.left),top:Math.round(r.top)};},
  setval:function(el,v){var P=el.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;
    var d=Object.getOwnPropertyDescriptor(P,'value');d&&d.set?d.set.call(el,v):(el.value=v);},
  fill:function(el,v){el.focus();if(el.isContentEditable)el.textContent=v;else $$.setval(el,v);el.dispatchEvent(new Event('input',{bubbles:true,composed:true}));el.dispatchEvent(new Event('change',{bubbles:true,composed:true}));return el.value;},
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
    fn shadow_hosts_stay_inside_their_frame_segment() {
        let steps = json!([
            {"kind":"shadow","xpath":"//x-panel[@id='outer']"},
            {"kind":"frame","xpath":"//iframe[@id='login']"},
            {"kind":"shadow","xpath":"//x-inner[@id='inner']"},
            {"kind":"frame","xpath":"//iframe[@name='form']"},
            {"kind":"element","xpath":"//input[@name='user']"}
        ]);
        assert_eq!(
            crate::web_python::xpath_segments(steps.as_array().unwrap()).unwrap(),
            [
                "//x-panel[@id='outer']/xbotShadowRoot//iframe[@id='login']",
                "//x-inner[@id='inner']/xbotShadowRoot//iframe[@name='form']",
                "//input[@name='user']"
            ]
        );
    }

    #[test]
    fn closed_xpath_results_never_contain_candidates_or_segments() {
        let result = closed_xpath_result(&[json!("e12")]);
        assert_eq!(result["error_code"], "WEB_SHADOW_CLOSED");
        assert_eq!(result["xpaths"], json!([]));
        assert_eq!(result["candidates"], json!([]));
        assert!(result["reason"]
            .as_str()
            .unwrap()
            .contains("无法生成 XPath"));
        assert!(result.get("local_xpaths").is_none());
        assert!(result.get("xpath_lists").is_none());
    }

    #[test]
    fn context_identity_includes_the_renderer_target() {
        let a = FrameContext {
            id: 7,
            target: Some("frame-a".into()),
        };
        let b = FrameContext {
            id: 7,
            target: Some("frame-b".into()),
        };
        assert_ne!(a, b);
        assert_ne!(
            a,
            FrameContext {
                id: 7,
                target: None
            }
        );
    }

    #[test]
    fn frame_refs_are_scoped_without_rewriting_page_text() {
        assert_eq!(
            prefix_refs("- button e4 [ref=e4]\n[ref=e55]", "f2"),
            "- button e4 [ref=f2e4]\n[ref=f2e55]"
        );
        assert_eq!(
            prefix_refs("[ref=example] [ref=e]", "f2"),
            "[ref=example] [ref=e]"
        );
    }

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
                "web_xpath",
                "web_action",
                "web_evaluate_playwright"
            ]
        );
    }
}
