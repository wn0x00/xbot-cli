//! Prepare literal inputs once; actions resolve only within an explicit snapshot.
use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::cli::{Win32Sub, Win32Target};
use crate::core::{self, BackendError, Res};

const ACTION_WORKER: &str = include_str!("../skills/xbot-activity-fix/scripts/win32/actions.py");

pub(crate) struct PreparedAction {
    action: &'static str,
    target: Win32Target,
    options: Value,
}

fn text_input(text: Option<String>, file: Option<String>) -> Res<String> {
    let text = match (text, file) {
        (Some(text), None) => text,
        (None, Some(file)) => core::read_file_or_stdin(&file)?,
        _ => {
            return Err(BackendError::new(
                "BAD_INPUT",
                "文本和 --file 必须且只能提供一个",
            ))
        }
    };
    if text.len() > 1024 * 1024 || text.contains('\0') {
        return Err(BackendError::new(
            "BAD_INPUT",
            "文本不得超过 1 MiB 或包含 NUL",
        ));
    }
    Ok(text)
}

fn key_sequence(keys: &str) -> Res<String> {
    let parts: Vec<&str> = keys.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        unreachable!()
    };
    let mut sequence = String::new();
    let mut used = Vec::new();
    for modifier in modifiers {
        let modifier = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl",
            "alt" => "Alt",
            "shift" => "Shift",
            "win" => "Win",
            _ => {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    "组合键修饰符仅支持 Ctrl/Alt/Shift/Win",
                ))
            }
        };
        if used.contains(&modifier) {
            return Err(BackendError::new("BAD_INPUT", "组合键修饰符不能重复"));
        }
        used.push(modifier);
        sequence.push_str(&format!("{{{modifier}}}"));
    }
    let normalized = key.to_ascii_lowercase();
    let key = match normalized.as_str() {
        "enter" | "return" => "Enter".to_owned(),
        "esc" | "escape" => "Esc".to_owned(),
        "tab" => "Tab".to_owned(),
        "space" => "Space".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "delete" | "del" => "Delete".to_owned(),
        "insert" => "Insert".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "pageup" => "PageUp".to_owned(),
        "pagedown" => "PageDown".to_owned(),
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "up" => "Up".to_owned(),
        "down" => "Down".to_owned(),
        single if single.len() == 1 && single.as_bytes()[0].is_ascii_alphanumeric() => {
            single.to_owned()
        }
        function
            if function
                .strip_prefix('f')
                .and_then(|value| value.parse::<u8>().ok())
                .is_some_and(|value| (1..=24).contains(&value)) =>
        {
            function.to_uppercase()
        }
        _ => {
            return Err(BackendError::new(
                "BAD_INPUT",
                "按键使用 Enter、Ctrl+A 等单键或组合键，不接受按键脚本",
            ))
        }
    };
    sequence.push_str(&format!("{{{key}}}"));
    Ok(sequence)
}

pub(crate) fn prepare(sub: Win32Sub) -> Res<PreparedAction> {
    let (action, target, options) = match sub {
        Win32Sub::Inspect(target) => ("inspect", target, json!({})),
        Win32Sub::Click { target, method } => ("click", target, json!({"method":method})),
        Win32Sub::Dblclick(target) => ("dblclick", target, json!({})),
        Win32Sub::Fill { target, text, file } => {
            ("fill", target, json!({"text":text_input(text,file)?}))
        }
        Win32Sub::Type { target, text, file } => {
            ("type", target, json!({"text":text_input(text,file)?}))
        }
        Win32Sub::Press { target, keys } => (
            "press",
            target,
            json!({"keys":keys,"key_sequence":key_sequence(&keys)?}),
        ),
        Win32Sub::Check(target) => ("check", target, json!({})),
        Win32Sub::Uncheck(target) => ("uncheck", target, json!({})),
        Win32Sub::Select(target) => ("select", target, json!({})),
        Win32Sub::Expand(target) => ("expand", target, json!({})),
        Win32Sub::Collapse(target) => ("collapse", target, json!({})),
        Win32Sub::Scroll {
            target,
            direction,
            large,
        } => (
            "scroll",
            target,
            json!({"direction":direction,"large":large}),
        ),
        Win32Sub::Focus(target) => ("focus", target, json!({})),
        Win32Sub::Hover(target) => ("hover", target, json!({})),
        _ => return Err(BackendError::new("BAD_ARGS", "不是 Windows 控件动作")),
    };
    crate::win32_session::validate_id(&target.snapshot)?;
    let number = target.reference.strip_prefix('w').unwrap_or("");
    if number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || number
            .parse::<u32>()
            .ok()
            .is_none_or(|value| value == 0 || value > 5000)
        || target.reference != format!("w{}", number.parse::<u32>().unwrap_or(0))
    {
        return Err(BackendError::new(
            "BAD_INPUT",
            "ref 必须是快照中的 w1–w5000",
        ));
    }
    Ok(PreparedAction {
        action,
        target,
        options,
    })
}

impl PreparedAction {
    pub(crate) fn preview(&self) -> Option<Value> {
        if self.action == "inspect" {
            return None;
        }
        let mut options = self.options.clone();
        if let Some(text) = options.get("text").and_then(Value::as_str) {
            options = json!({"text_chars":text.chars().count(),"text_included":false});
        }
        Some(json!({"action":format!("win32.{}",self.action),
            "target":{"snapshot_id":self.target.snapshot,"ref":self.target.reference},
            "arguments":{"options":options,"timeout":self.target.timeout,
                "resolution":"process_instance_and_uia_ancestry","automatic_retry":false}}))
    }

    pub(crate) fn execute(self) -> Res<CmdOut> {
        let snapshot = crate::win32_session::load(&self.target.snapshot)?;
        let _lock = if self.action != "inspect" {
            Some(crate::win32_session::ActionLock::acquire(&snapshot)?)
        } else {
            None
        };
        let receipt = crate::win32_session::receipt(&self.target.snapshot)?;
        let source = format!(
            "import types,sys\nm=types.ModuleType('xbot_win32_snapshot')\nsys.modules[m.__name__]=m\nexec({},m.__dict__)\n{}",
            serde_json::to_string(crate::win32_cmds::SNAPSHOT_WORKER).expect("serialize capture source"),
            ACTION_WORKER
        );
        let request = json!({"action":self.action,"snapshot":snapshot,"ref":self.target.reference,
            "options":self.options,"timeout":self.target.timeout,"receipt_path":receipt});
        let action =
            crate::win32_cmds::run_worker(&source, &[], Some(request), self.target.timeout);
        let mut result = action.map_err(|error| {
            let issued = self.action != "inspect" && receipt.exists();
            BackendError::new(error.code, if issued {
                "Windows 动作可能已生效，请重新采集并核对结果，不要直接重放动作"
            } else { "Windows 控件读取或动作校验失败，未执行动作" })
                .with_retryable(false)
                .with_hint("使用 win32 snapshot 重新观察；不支持的 Pattern 不会自动回退物理点击")
                .with_data(json!({"operation":format!("win32.{}",self.action),
                    "source_snapshot_id":self.target.snapshot,"ref":self.target.reference,
                    "applied":if issued {Value::Null} else {json!(false)},
                    "outcome":if issued {"unknown"} else {"not_applied"},"receipt_file":if issued {Some(&receipt)} else {None}}))
        })?;
        if self.action != "inspect" {
            let hwnd = snapshot["scope"]["hwnd"].as_str().unwrap_or("");
            let pid = snapshot["scope"]["process_id"].as_u64().unwrap_or(0) as u32;
            let max_nodes = snapshot["limits"]["max_nodes"].as_u64().unwrap_or(1000) as u32;
            let max_depth = snapshot["limits"]["max_depth"].as_u64().unwrap_or(12) as u32;
            match crate::win32_cmds::snapshot(
                hwnd,
                pid,
                max_nodes,
                max_depth,
                self.target.timeout,
                snapshot["process_instance"].as_str(),
            ) {
                Ok(tree) => result["snapshot"] = tree,
                Err(error) => {
                    result["warning"] = json!({"code":error.code,
                    "message":"动作已返回，但后续快照获取失败；请只补快照并核对结果，不要重复动作"})
                }
            }
        }
        Ok((result, vec![]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_contract_rejects_macros_and_normalizes_chords() {
        assert_eq!(key_sequence("Ctrl+A").unwrap(), "{Ctrl}{a}");
        assert_eq!(
            key_sequence("Ctrl+Shift+End").unwrap(),
            "{Ctrl}{Shift}{End}"
        );
        for value in [
            "",
            "{Enter}",
            "Ctrl++A",
            "Ctrl+Ctrl+A",
            "Enter+Delete",
            "Ctrl+",
            "F25",
        ] {
            assert!(key_sequence(value).is_err(), "{value}");
        }
    }
}
