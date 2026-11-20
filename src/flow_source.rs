//! Common source interface; legacy VisualFlow block IDs are never treated as source ordinals.
use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};
use serde_json::{json, Value};

fn target(flow: Option<&str>, modern: bool) -> Res<String> {
    core::resolve_flow_id(flow, if modern { None } else { Some("code") })
}

pub fn read(
    flow: Option<&str>,
    offset: Option<u64>,
    limit: Option<u64>,
    block_nos: Option<&str>,
) -> Res<CmdOut> {
    if let Some(numbers) = block_nos {
        validate_block_nos(numbers)?;
    }
    let modern = crate::studio_mcp::is_copilot()?;
    if block_nos.is_some() && !modern {
        return Err(crate::studio_mcp::unsupported(
            "flow read --block-nos (requires 6.4)",
        ));
    }
    let id = target(flow, modern)?;
    let mut args = json!({"flow_id":id});
    if let Some(n) = offset {
        args["offset"] = json!(n);
    }
    if let Some(n) = limit {
        args["limit"] = json!(n);
    }
    if let Some(n) = block_nos {
        args["block_nos"] = json!(n);
    }
    Ok((core::mcp("flow.source.read", args, 130)?, vec![]))
}

pub fn write(flow: Option<&str>, content: &str) -> Res<CmdOut> {
    let modern = crate::studio_mcp::is_copilot()?;
    let id = target(flow, modern)?;
    core::require_current_edit_mode()?;
    let result = core::mcp(
        "flow.source.write",
        json!({"flow_id":id,"content":content}),
        130,
    )?;
    finish("write", result)
}

pub fn edit(flow: Option<&str>, old: &str, new: &str, replace_all: bool) -> Res<CmdOut> {
    let modern = crate::studio_mcp::is_copilot()?;
    let id = target(flow, modern)?;
    core::require_current_edit_mode()?;
    let result = core::mcp(
        "flow.source.edit",
        json!({"flow_id":id,"old_string":old,"new_string":new,"replace_all":replace_all}),
        130,
    )?;
    finish("edit", result)
}

fn finish(operation: &str, result: Value) -> Res<CmdOut> {
    let errors = crate::flow_cmds::diagnose_after_mutation(operation, &result)?;
    Ok((
        json!({"operation":operation,"result":result,"static_errors":errors}),
        vec![],
    ))
}

pub fn validate_block_nos(selection: &str) -> Res<()> {
    let invalid = || {
        BackendError::new(
            "BAD_INPUT",
            "block-nos 必须是正整数、范围或逗号组合，例如 10:20,25",
        )
    };
    if selection.is_empty() || selection.len() > 2048 {
        return Err(invalid());
    }
    let mut selected = std::collections::BTreeSet::new();
    for part in selection.split(',') {
        let mut numbers = part.split(':');
        let start = numbers
            .next()
            .ok_or_else(invalid)?
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        let end = if let Some(end) = numbers.next() {
            let end = end.parse::<u32>().map_err(|_| invalid())?;
            if end < start || numbers.next().is_some() {
                return Err(invalid());
            }
            end
        } else {
            start
        };
        if end - start >= 100 {
            return Err(invalid().with_hint("一次最多选择 100 个不同的可视化指令编号"));
        }
        for number in start..=end {
            selected.insert(number);
            if selected.len() > 100 {
                return Err(invalid().with_hint("一次最多选择 100 个不同的可视化指令编号"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn block_number_ranges_are_not_source_lines_or_ids() {
        for good in ["1", "10:20,25", "1:1", "1:100", "1:100,1:100"] {
            assert!(validate_block_nos(good).is_ok());
        }
        for bad in [
            "",
            "0",
            "2:1",
            "1:2:3",
            "x",
            "1,",
            " 1",
            "1:-2",
            "1:101",
            "1:100,101",
            "1:4294967295",
        ] {
            assert!(validate_block_nos(bad).is_err(), "{bad}");
        }
    }
}
