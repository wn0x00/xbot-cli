//! Readable ARIA tree presentation without implicit collapsing or truncation.
use serde_json::Value;

fn quoted(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

pub fn render(snapshot: &Value, display: &Value) -> Option<String> {
    if display["view"] == "raw" || !snapshot["tree"].is_object() {
        return None;
    }
    let role = display["role"].as_str();
    let filter = display["filter"].as_str().unwrap_or("").to_lowercase();
    fn filtered(node: &Value, role: Option<&str>, filter: &str) -> Option<Value> {
        let children: Vec<_> = node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| filtered(c, role, filter))
            .collect();
        let matches = role.is_none_or(|r| node["role"] == r)
            && format!(
                "{} {} {}",
                node["ref"].as_str().unwrap_or(""),
                node["role"].as_str().unwrap_or(""),
                node["name"].as_str().unwrap_or("")
            )
            .to_lowercase()
            .contains(filter);
        if !matches && children.is_empty() {
            return None;
        }
        let mut result = node.clone();
        result["children"] = Value::Array(children);
        if !matches {
            result["filter_context"] = Value::Bool(true);
        }
        Some(result)
    }
    fn lines(node: &Value, prefix: &str, branch: &str, continuation: &str, out: &mut Vec<String>) {
        let mut label = node["role"].as_str().unwrap_or("generic").to_owned();
        if node["name"].as_str().is_some_and(|s| !s.is_empty()) {
            label.push_str(&format!(" {}", quoted(&node["name"])));
        }
        if let Some(id) = node["ref"].as_str() {
            label.push_str(&format!(" [ref={id}]"));
        }
        if let Some(states) = node["states"].as_object() {
            for (key, value) in states {
                label.push_str(&format!(" [{key}={}]", quoted(value)));
            }
        }
        if let Some(level) = node["level"].as_u64().filter(|v| *v != 0) {
            label.push_str(&format!(" [level={level}]"));
        }
        if node["cursor"] == "pointer" {
            label.push_str(" [cursor=pointer]");
        }
        if let Some(hosts) = node["shadow_hosts"].as_array().filter(|h| !h.is_empty()) {
            let modes = node["shadow_modes"].as_array();
            let closed = node["xpath_unavailable"] == "closed-shadow-root"
                || modes.is_some_and(|m| m.iter().any(|v| v == "closed"));
            let mode = if closed {
                "closed"
            } else if modes.is_some_and(|m| m.len() == hosts.len() && m.iter().all(|v| v == "open"))
            {
                "open"
            } else {
                "unknown"
            };
            label.push_str(&format!(
                " [shadow-root={mode} hosts={}]",
                quoted(&Value::Array(hosts.clone()))
            ));
            if closed {
                label.push_str(" [不支持：影刀无法使用 closed 内元素，无法生成 XPath]");
            }
        }
        if node["filter_context"] == true {
            label.push_str(" [context]");
        }
        out.push(format!("{prefix}{branch}{label}"));
        let prefix = format!("{prefix}{continuation}");
        let children = node["children"].as_array();
        let props = node["props"].as_object();
        let count = children.map_or(0, Vec::len) + props.map_or(0, |p| p.len());
        let mut index = 0;
        for (key, value) in props.into_iter().flatten() {
            index += 1;
            out.push(format!(
                "{prefix}{} /{key}: {}",
                if index == count { "└─" } else { "├─" },
                quoted(value)
            ));
        }
        for child in children.into_iter().flatten() {
            index += 1;
            let last = index == count;
            lines(
                child,
                &prefix,
                if last { "└─ " } else { "├─ " },
                if last { "   " } else { "│  " },
                out,
            );
        }
    }
    let mut output = Vec::new();
    if let Some(tree) = filtered(&snapshot["tree"], role, &filter) {
        lines(&tree, "", "", "", &mut output);
    }
    let total = output.len();
    let offset = display["offset"].as_u64().unwrap_or(0) as usize;
    let limit = display["limit"].as_u64().map_or(usize::MAX, |n| n as usize);
    let mut selected = output
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    if role.is_some() || !filter.is_empty() {
        selected.insert(
            0,
            "筛选树（[context] 为保留层级的祖先；JSON 保留完整树）".into(),
        );
    }
    if offset > 0 || offset.saturating_add(limit) < total {
        selected.push(format!(
            "部分显示: 第 {}-{} 行，共 {total} 行；完整树: web snapshot",
            if offset < total { offset + 1 } else { 0 },
            offset.saturating_add(limit).min(total),
        ));
    }
    Some(selected.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> Value {
        json!({"tree":{"role":"fragment","children":[
            {"role":"generic","ref":"e1","name":"","children":[
                {"role":"heading","name":"Title","level":2,"ref":"e2","children":[
                    {"role":"text","name":"Title"}]},
                {"role":"link","name":"Help","ref":"e3","props":{"url":"https://example.com/help"}},
                {"role":"checkbox","name":"Toggle","ref":"e4",
                    "states":{"checked":"mixed","expanded":false},"cursor":"pointer"}
            ]}
        ]}})
    }
    #[test]
    fn tree_preserves_wrappers_refs_duplicate_text_properties_and_states() {
        let text = render(&snapshot(), &json!({})).unwrap();
        for part in [
            "generic [ref=e1]",
            "heading \"Title\" [ref=e2] [level=2]",
            "text \"Title\"",
            "/url: \"https://example.com/help\"",
            "[checked=\"mixed\"]",
            "[expanded=false]",
            "[cursor=pointer]",
            "│  ",
        ] {
            assert!(text.contains(part), "missing {part}: {text}");
        }
        assert!(render(&snapshot(), &json!({"view":"raw"})).is_none());
    }
    #[test]
    fn default_keeps_more_than_sixty_nodes_and_long_names() {
        let name = "A".repeat(1000);
        let children: Vec<_> = (0..100)
            .map(|i| {
                json!({
                    "role":"button","ref":format!("e{i}"),"name":name
                })
            })
            .collect();
        let text = render(
            &json!({"tree":{"role":"fragment","children":children}}),
            &json!({}),
        )
        .unwrap();
        assert!(text.contains("[ref=e99]"));
        assert_eq!(text.matches(&name).count(), 100);
        assert!(!text.contains("部分显示"));
    }
    #[test]
    fn explicit_filter_retains_ancestors_and_explicit_limit_is_marked() {
        let text = render(&snapshot(), &json!({"role":"link"})).unwrap();
        assert!(text.contains("generic [ref=e1] [context]"));
        assert!(text.contains("link \"Help\""));
        assert!(!text.contains("checkbox"));
        let text = render(&snapshot(), &json!({"limit":2})).unwrap();
        assert!(text.contains("部分显示: 第 1-2 行"));
    }
    #[test]
    fn observed_closed_shadow_is_prominent_and_missing_modes_are_not_assumed_open() {
        for (modes, expected) in [
            (json!(["open", "closed"]), "shadow-root=closed"),
            (json!(["closed", "open"]), "shadow-root=closed"),
            (json!(["open", "open"]), "shadow-root=open"),
            (Value::Null, "shadow-root=unknown"),
        ] {
            let tree = json!({"tree":{"role":"button","name":"Target","ref":"e1",
                "shadow_hosts":["outer","inner"],"shadow_modes":modes}});
            let text = render(&tree, &json!({})).unwrap();
            assert!(text.contains(expected));
            assert_eq!(
                text.contains("无法生成 XPath"),
                expected.ends_with("closed")
            );
        }
    }
}
