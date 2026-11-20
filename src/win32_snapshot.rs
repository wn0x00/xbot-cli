//! Compact UIA tree presentation. The captured JSON is never rewritten.
use std::collections::HashMap;

use serde_json::Value;

pub(crate) fn render(data: &Value) -> Option<String> {
    if data["backend"] != "uiautomation" || !data["nodes"].is_array() {
        return None;
    }
    let nodes = data["nodes"].as_array()?;
    let (tree, invalid_structure) = tree_lines(nodes);
    let mut lines = vec![format!("Windows 元素树（{} 个节点）", nodes.len())];
    if let Some(scope) = data["scope"].as_object() {
        lines.push(format!(
            "窗口: PID={} HWND={}",
            scope.get("process_id").unwrap_or(&Value::Null),
            scope.get("hwnd").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    if data["traversal_complete"] == true {
        lines.push("采集: 遍历完成".into());
    } else {
        let reasons = data["partial_reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|reason| {
                let description = match reason {
                    "node_budget" => "达到节点上限",
                    "depth_budget" => "达到深度上限",
                    "time_budget" => "达到时间上限",
                    "read_error" => "部分属性或子节点读取失败",
                    "text_truncated" => "部分文本已截断",
                    "repeated_runtime_id" => "观察到重复 RuntimeId",
                    _ => "未知原因",
                };
                format!("{description} ({})", quoted(reason))
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "警告: 部分树；{}",
            if reasons.is_empty() {
                "未声明遍历完整性".into()
            } else {
                reasons.join("；")
            }
        ));
    }
    for warning in data["observation_warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lines.push(match warning {
            "ROOT_ONLY" => "警告: 仅观察到根节点，不能据此判断子控件不存在".into(),
            "WINDOW_MINIMIZED" => "警告: 目标窗口已最小化".into(),
            "MISSING_RUNTIME_ID" => "警告: 部分控件没有 RuntimeId，已保留元素树；这些控件及其后代的快照 ref 暂不支持安全动作定位".into(),
            other => format!("警告: {}", quoted(other)),
        });
    }
    if invalid_structure {
        lines.push("警告: 父引用缺失、重复或成环，已保留所有节点；异常节点按根节点展示".into());
    }
    for issue in data["issues"].as_array().into_iter().flatten() {
        lines.push(format!(
            "读取异常: ref={} field={} kind={}",
            quoted(issue["ref"].as_str().unwrap_or("?")),
            quoted(issue["field"].as_str().unwrap_or("?")),
            quoted(issue["kind"].as_str().unwrap_or("?"))
        ));
    }
    lines.push(String::new());
    if tree.is_empty() {
        lines.push("（无节点）".into());
    } else {
        lines.extend(tree);
    }
    if let Some(id) = data["snapshot_id"].as_str() {
        lines.push(format!("\n快照: {}", quoted(id)));
    }
    lines.push(if data["refs_resolvable_for_actions"] == true {
        "动作需携带 --snapshot <快照编号>；动作后使用返回的新快照".into()
    } else {
        "该快照仅供观察；执行动作前请用当前 CLI 重新采集".into()
    });
    Some(lines.join("\n"))
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serialize tree label")
}

fn label(node: &Value) -> String {
    let control = node["control_type"].as_str().unwrap_or("unknown");
    let mut result = control
        .strip_suffix("Control")
        .unwrap_or(control)
        .to_lowercase();
    // Mirror the provider's conservative masking even for externally supplied data.
    if node["is_password"] != false {
        result.push_str(" \"[redacted]\"");
    } else if let Some(name) = node["name"].as_str().filter(|name| !name.is_empty()) {
        result.push_str(&format!(" {}", quoted(name)));
    }
    if let Some(id) = node["ref"].as_str() {
        result.push_str(&format!(" [ref={id}]"));
    }
    for (field, name) in [("automation_id", "automation_id"), ("class_name", "class")] {
        if let Some(value) = node[field].as_str().filter(|value| !value.is_empty()) {
            result.push_str(&format!(" [{name}={}]", quoted(value)));
        }
    }
    if node["enabled"] == false {
        result.push_str(" [disabled]");
    }
    if node["offscreen"] == true {
        result.push_str(" [offscreen]");
    }
    if node["is_password"] == true {
        result.push_str(" [password]");
    }
    result
}

fn tree_lines(nodes: &[Value]) -> (Vec<String>, bool) {
    let mut refs = HashMap::new();
    let mut invalid = false;
    for (index, node) in nodes.iter().enumerate() {
        if let Some(id) = node["ref"].as_str() {
            if refs.insert(id, index).is_some() {
                invalid = true;
            }
        }
    }
    let mut children = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent) = node["parent_ref"].as_str() {
            if let Some(&parent) = refs.get(parent).filter(|&&parent| parent != index) {
                children[parent].push(index);
                continue;
            }
            invalid = true;
        }
        roots.push(index);
    }
    let mut seen = vec![false; nodes.len()];
    let mut lines = Vec::new();
    // A final pass retains disconnected cycles as well. Iteration avoids growing
    // the Rust stack on malformed or deeply nested input.
    for root in roots.into_iter().chain(0..nodes.len()) {
        if seen[root] {
            continue;
        }
        if nodes[root]["parent_ref"].is_string() {
            invalid = true;
        }
        let mut stack = vec![(root, String::new(), "", "")];
        while let Some((index, prefix, branch, continuation)) = stack.pop() {
            if seen[index] {
                invalid = true;
                continue;
            }
            seen[index] = true;
            lines.push(format!("{prefix}{branch}{}", label(&nodes[index])));
            let prefix = format!("{prefix}{continuation}");
            for (position, &child) in children[index].iter().enumerate().rev() {
                let last = position + 1 == children[index].len();
                stack.push((
                    child,
                    prefix.clone(),
                    if last { "└─ " } else { "├─ " },
                    if last { "   " } else { "│  " },
                ));
            }
        }
    }
    (lines, invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: &str, parent: Option<&str>, role: &str, name: &str) -> Value {
        json!({"ref":id,"parent_ref":parent,"control_type":role,"name":name,"is_password":false,"enabled":true,"offscreen":false})
    }

    #[test]
    fn branching_tree_retains_empty_wrappers_duplicate_names_ids_states_and_long_text() {
        let mut input = vec![
            node("w1", None, "WindowControl", "窗口"),
            node("w2", Some("w1"), "PaneControl", ""),
            node("w3", Some("w2"), "EditControl", "搜索"),
            node("w4", Some("w1"), "ButtonControl", "发送"),
            node("w5", Some("w1"), "TextControl", "发送"),
        ];
        input[2]["automation_id"] = json!("SearchInput");
        input[2]["class_name"] = json!("QLineEdit");
        input[3]["enabled"] = json!(false);
        input[3]["offscreen"] = json!(true);
        let (lines, invalid) = tree_lines(&input);
        assert!(!invalid);
        assert_eq!(lines, [
            "window \"窗口\" [ref=w1]",
            "├─ pane [ref=w2]",
            "│  └─ edit \"搜索\" [ref=w3] [automation_id=\"SearchInput\"] [class=\"QLineEdit\"]",
            "├─ button \"发送\" [ref=w4] [disabled] [offscreen]",
            "└─ text \"发送\" [ref=w5]",
        ]);
        input[4]["name"] = json!("长文本".repeat(150));
        assert!(tree_lines(&input)
            .0
            .last()
            .unwrap()
            .contains(&"长文本".repeat(150)));
    }

    #[test]
    fn parent_refs_define_hierarchy_and_invalid_references_never_drop_nodes() {
        let input = vec![
            node("w2", Some("w1"), "TextControl", "child"),
            node("w1", None, "WindowControl", "root"),
        ];
        assert_eq!(
            tree_lines(&input).0,
            ["window \"root\" [ref=w1]", "└─ text \"child\" [ref=w2]"]
        );
        for input in [
            vec![node("w1", Some("missing"), "PaneControl", "orphan")],
            vec![
                node("w1", Some("w2"), "PaneControl", "a"),
                node("w2", Some("w1"), "PaneControl", "b"),
            ],
            vec![
                node("w1", None, "PaneControl", "a"),
                node("w1", None, "PaneControl", "b"),
            ],
        ] {
            let (lines, invalid) = tree_lines(&input);
            assert!(invalid);
            assert_eq!(lines.len(), input.len());
        }
    }

    #[test]
    fn snapshot_keeps_incompleteness_issues_and_observation_warnings_visible() {
        let snapshot = json!({"backend":"uiautomation","scope":{"process_id":123,"hwnd":"0x456"},"snapshot_id":"snap-1",
            "nodes":[node("w1",None,"WindowControl","root")],"traversal_complete":false,
            "partial_reasons":["depth_budget","read_error","text_truncated"],"observation_warnings":["ROOT_ONLY","WINDOW_MINIMIZED"],
            "issues":[{"ref":"w1","field":"Name","kind":"COMError"}]});
        let text = render(&snapshot).unwrap();
        for expected in [
            "PID=123 HWND=0x456",
            "警告: 部分树",
            "达到深度上限",
            "部分文本已截断",
            "仅观察到根节点",
            "目标窗口已最小化",
            "COMError",
            "snap-1",
            "该快照仅供观察",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert!(!text.contains("采集: 遍历完成"));
    }

    #[test]
    fn control_text_is_escaped_and_password_or_unknown_sensitivity_is_masked() {
        let mut value = node("w1", None, "EditControl", "秘密\n\u{1b}[31m\"quoted\"");
        let visible = label(&value);
        assert!(visible.contains("\\n\\u001b[31m\\\"quoted\\\""));
        assert!(!visible.contains('\u{1b}'));
        for flag in [json!(true), Value::Null] {
            value["is_password"] = flag;
            let masked = label(&value);
            assert!(masked.contains("[redacted]"));
            assert!(!masked.contains("秘密"));
        }
    }
}
