//! Parse external input once, then share the same payload between preview and execution.

use crate::core::{self, BackendError, Res};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

fn bad(message: impl Into<String>) -> BackendError {
    BackendError::new("BAD_INPUT", message)
}

pub fn object(value: Value, label: &str) -> Res<Value> {
    if !value.is_object() {
        return Err(bad(format!("{label} 必须是 JSON 对象")));
    }
    Ok(value)
}

pub fn browser_args(raw: Option<&str>) -> Res<Value> {
    object(
        match raw {
            Some(raw) => core::json_arg(raw)?,
            None => json!({}),
        },
        "浏览器参数",
    )
}

pub fn app_inputs(inputs: &[String], inputs_json: Option<&str>) -> Res<Value> {
    let mut merged = Map::new();
    for entry in inputs {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| bad(format!("--input 需要 K=V 形式: {entry}")))?;
        if key.trim().is_empty() {
            return Err(bad("--input 的键不能为空"));
        }
        merged.insert(
            key.to_owned(),
            serde_json::from_str(value).unwrap_or_else(|_| json!(value)),
        );
    }
    if let Some(raw) = inputs_json {
        let value = object(core::json_arg(raw)?, "--inputs-json")?;
        merged.extend(value.as_object().unwrap().clone());
    }
    Ok(Value::Object(merged))
}

fn fields(value: &Value, allowed: &[&str], label: &str) -> Res<()> {
    let map = value
        .as_object()
        .ok_or_else(|| bad(format!("{label} 必须是对象")))?;
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(bad(format!("{label} 不支持字段 {key}")));
    }
    Ok(())
}

fn required_text<'a>(value: &'a Value, field: &str, label: &str) -> Res<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad(format!("{label}.{field} 必须是非空字符串")))
}

fn array<'a>(value: &'a Value, field: &str, label: &str) -> Res<&'a Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| bad(format!("{label}.{field} 必须是数组")))
}

fn nonempty(items: &[Value], label: &str) -> Res<()> {
    if items.is_empty() {
        return Err(bad(format!("{label} 不能为空")));
    }
    Ok(())
}

fn optional_type(value: &Value, field: &str, check: fn(&Value) -> bool, label: &str) -> Res<()> {
    if value.get(field).is_some_and(|value| !check(value)) {
        return Err(bad(format!("{label}.{field} 类型无效")));
    }
    Ok(())
}

pub fn flow_parameters(raw: &str) -> Res<Value> {
    normalize_parameters(core::json_arg(raw)?)
}

pub fn normalize_parameters(value: Value) -> Res<Value> {
    let value = if value.is_array() {
        json!({"operations": value})
    } else {
        value
    };
    fields(&value, &["operations"], "参数写入")?;
    let operations = array(&value, "operations", "参数写入")?;
    nonempty(operations, "operations")?;
    for (index, operation) in operations.iter().enumerate() {
        let label = format!("operations[{index}]");
        fields(
            operation,
            &[
                "op",
                "name",
                "direction",
                "kind",
                "type",
                "value",
                "description",
            ],
            &label,
        )?;
        required_text(operation, "name", &label)?;
        match required_text(operation, "op", &label)? {
            "upsert" => {
                if !matches!(required_text(operation, "direction", &label)?, "In" | "Out") {
                    return Err(bad(format!("{label}.direction 必须是 In 或 Out")));
                }
                if required_text(operation, "type", &label)? == "password" {
                    return Err(bad(format!("{label}: password 不是支持的流程参数类型")));
                }
            }
            "remove" => {
                if operation
                    .as_object()
                    .is_some_and(|fields| fields.len() != 2)
                {
                    return Err(bad(format!("{label}: remove 只接受 op 和 name")));
                }
            }
            _ => return Err(bad(format!("{label}.op 必须是 upsert 或 remove"))),
        }
        if let Some(kind) = operation.get("kind") {
            if !matches!(kind.as_str(), Some("Text" | "Expression")) {
                return Err(bad(format!("{label}.kind 必须是 Text 或 Expression")));
            }
        }
        for field in ["value", "description"] {
            optional_type(operation, field, Value::is_string, &label)?;
        }
    }
    Ok(value)
}

pub fn visual_edit(file: &str) -> Res<Value> {
    let value = serde_json::from_str(&core::read_file_or_stdin(file)?)
        .map_err(|e| BackendError::new("BAD_JSON", format!("{file}: {e}")))?;
    normalize_visual_edit(value)
}

pub fn normalize_visual_edit(value: Value) -> Res<Value> {
    let value = if value.is_array() {
        json!({"op":"insert", "block_index":-1, "blocks":value})
    } else {
        value
    };
    match required_text(&value, "op", "块编辑")? {
        "insert" => {
            fields(&value, &["op", "block_index", "blocks"], "块插入")?;
            if value
                .get("block_index")
                .and_then(Value::as_i64)
                .is_none_or(|n| n < -1)
            {
                return Err(bad("block_index 必须是大于等于 -1 的整数"));
            }
            let blocks = array(&value, "blocks", "块插入")?;
            nonempty(blocks, "blocks")?;
            for (index, block) in blocks.iter().enumerate() {
                let label = format!("blocks[{index}]");
                fields(block, &["prototype_name", "comment"], &label)?;
                required_text(block, "prototype_name", &label)?;
                required_text(block, "comment", &label)?;
            }
        }
        "remove" => {
            fields(&value, &["op", "block_ids"], "块删除")?;
            let ids = array(&value, "block_ids", "块删除")?;
            nonempty(ids, "block_ids")?;
            if ids
                .iter()
                .any(|id| id.as_str().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(bad("block_ids 必须仅包含非空字符串"));
            }
        }
        _ => return Err(bad("块编辑 op 必须是 insert 或 remove")),
    }
    Ok(value)
}

pub fn visual_fill(raw: &str) -> Res<Value> {
    normalize_visual_fill(core::json_arg(raw)?)
}

pub fn normalize_visual_fill(value: Value) -> Res<Value> {
    let value = if value.is_array() {
        json!({"fills":value})
    } else {
        value
    };
    fields(&value, &["fills", "group_fills"], "块填充")?;
    let mut count = 0;
    let mut ids = HashSet::new();
    if value.get("fills").is_some() {
        for (index, fill) in array(&value, "fills", "块填充")?.iter().enumerate() {
            let label = format!("fills[{index}]");
            fields(fill, &["block_id", "inputs", "outputs"], &label)?;
            let id = required_text(fill, "block_id", &label)?;
            if !ids.insert(id) {
                return Err(bad(format!("fills 包含重复 block_id: {id}")));
            }
            optional_type(fill, "inputs", Value::is_object, &label)?;
            optional_type(fill, "outputs", Value::is_object, &label)?;
            if let Some(outputs) = fill.get("outputs").and_then(Value::as_object) {
                if outputs
                    .values()
                    .any(|v| v.as_str().is_none_or(|s| s.trim().is_empty()))
                {
                    return Err(bad(format!("{label}.outputs 的值必须是非空字符串")));
                }
            }
            count += 1;
        }
    }
    let mut groups = HashSet::new();
    if value.get("group_fills").is_some() {
        for (index, group) in array(&value, "group_fills", "块填充")?.iter().enumerate() {
            let label = format!("group_fills[{index}]");
            fields(group, &["block_id", "input", "columns", "fixed"], &label)?;
            let id = required_text(group, "block_id", &label)?;
            let input = required_text(group, "input", &label)?;
            if !groups.insert((id, input)) {
                return Err(bad(format!("{label} 与之前的 block_id/input 重复")));
            }
            optional_type(group, "fixed", Value::is_object, &label)?;
            let columns = group
                .get("columns")
                .and_then(Value::as_object)
                .filter(|c| !c.is_empty())
                .ok_or_else(|| bad(format!("{label}.columns 必须是非空对象")))?;
            let mut length = None;
            for column in columns.values() {
                let rows = column
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad(format!("{label}.columns 的每列必须是非空数组")))?;
                if length.is_some_and(|n| n != rows.len()) {
                    return Err(bad(format!("{label}.columns 各列长度必须相同")));
                }
                length = Some(rows.len());
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err(bad("至少提供一个 fills 或 group_fills 项"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_typed_values_and_json_precedence() {
        let value = app_inputs(
            &["count=2".into(), "name=a=b".into(), "enabled=true".into()],
            Some(r#"{"count":3,"items":[1]}"#),
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"count":3,"name":"a=b","enabled":true,"items":[1]})
        );
    }

    #[test]
    fn invalid_inputs_do_not_silently_disappear() {
        for raw in ["[]", "null", "42", "true"] {
            assert!(app_inputs(&[], Some(raw)).is_err());
            assert!(browser_args(Some(raw)).is_err());
        }
        assert!(app_inputs(&["=secret".into()], None).is_err());
        assert!(app_inputs(&["missing-equals".into()], None).is_err());
    }

    #[test]
    fn parameter_operations_have_a_real_structure() {
        assert_eq!(
            flow_parameters(r#"[{"op":"remove","name":"old"}]"#).unwrap(),
            json!({"operations":[{"op":"remove","name":"old"}]})
        );
        for value in [
            json!(true),
            json!([]),
            json!({}),
            json!([{"op":"upsert","name":"x","direction":"In","type":""}]),
            json!([{"op":"upsert","name":"x","direction":"In","type":"password"}]),
            json!({"operations":[{"op":"remove","name":"x"}],"flow_id":"other"}),
        ] {
            assert!(normalize_parameters(value).is_err());
        }
        for id in [
            "dict",
            "list",
            "xbot._web.browser.WebBrowser",
            "list<custom>",
        ] {
            assert!(normalize_parameters(json!([{
                "op":"upsert","name":"x","direction":"In","type":id
            }]))
            .is_ok());
        }
    }

    #[test]
    fn visual_edit_validates_operation_specific_fields() {
        let blocks = json!([{"prototype_name":"programing.snippet","comment":"test"}]);
        assert_eq!(
            normalize_visual_edit(blocks.clone()).unwrap(),
            json!({"op":"insert","block_index":-1,"blocks":blocks})
        );
        for value in [
            json!({"op":"remove","block_ids":[]}),
            json!({"op":"insert","blocks":[]}),
            json!({"op":"remove","block_ids":[42]}),
            json!([{"prototype_name":"x"}]),
        ] {
            assert!(normalize_visual_edit(value).is_err());
        }
    }

    #[test]
    fn fills_reject_duplicates_and_ragged_columns_before_mutation() {
        assert!(
            normalize_visual_fill(json!([{"block_id":"x","inputs":{}},{"block_id":"x"}])).is_err()
        );
        assert!(normalize_visual_fill(
            json!({"group_fills":[{"block_id":"x","input":"rows","columns":{"a":[1],"b":[1,2]}}]})
        )
        .is_err());
        assert!(normalize_visual_fill(json!({"fills":[]})).is_err());
        assert!(normalize_visual_fill(
            json!({"group_fills":[{"block_id":"x","input":"rows","columns":{"a":[1],"b":[2]}}]})
        )
        .is_ok());
    }
}
