use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};
use crate::{app_cmds, oapi, oapi_cli};

pub fn list(search: Option<&str>, owner: Option<&str>, page: u32, page_size: u32) -> Res<CmdOut> {
    let mut current = page;
    let mut instructions = Vec::new();
    let mut last_page = Value::Null;
    let mut complete = true;
    for fetched in 1..=100 {
        let (data, _) = list_page(search, owner, current, page_size)?;
        let rows = data
            .pointer("/response/data")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                BackendError::new("MCP_BAD_RESPONSE", "app.list 响应缺少指令清单数组")
                    .with_data(data.clone())
            })?;
        instructions.extend(
            rows.iter()
                .filter(|row| row.get("appType").and_then(Value::as_str) == Some("activity"))
                .cloned(),
        );
        last_page = data
            .pointer("/response/page")
            .cloned()
            .unwrap_or(Value::Null);
        let page_count = data.pointer("/response/page/pages").and_then(Value::as_u64);
        let has_more = has_more_pages(rows.len(), page_size as usize, page_count, current);
        if !has_more {
            complete = fetched < 100;
            break;
        }
        complete = false;
        current += 1;
    }
    Ok((
        json!({
            "backend": "oapi",
            "count": instructions.len(),
            "instructions": instructions,
            "page": last_page,
            "complete": complete,
        }),
        vec![],
    ))
}

fn has_more_pages(rows: usize, page_size: usize, page_count: Option<u64>, current: u32) -> bool {
    rows == page_size && page_count.is_none_or(|count| (current as u64) < count)
}

fn list_page(search: Option<&str>, owner: Option<&str>, page: u32, page_size: u32) -> Res<CmdOut> {
    oapi::prepare(oapi_cli::OapiSub::App(oapi_cli::AppSub::List {
        search: search.map(str::to_owned),
        owner: owner.map(str::to_owned),
        page,
        size: page_size,
        http: oapi_cli::HttpArgs { timeout: 30 },
    }))?
    .execute()
    .map_err(|error| {
        if error.code == "PLATFORM_AUTH" {
            error.with_hint("activity list 需要开放平台凭据：运行 xbot-cli oapi auth <AccessKeyId> <AccessKeySecret>，或配置 XBOT_OAPI_ACCESS_KEY_ID / XBOT_OAPI_ACCESS_KEY_SECRET 后重试；activity open 不依赖开放平台")
        } else {
            error
        }
    })
}

pub fn open(activity_id: &str, timeout: u64, force: bool) -> Res<CmdOut> {
    let (mut data, next) = app_cmds::app_open(activity_id, timeout, force)?;
    ensure_activity(&data)?;
    data["command"] = json!("activity.open");
    Ok((data, next))
}

fn ensure_activity(data: &Value) -> Res<()> {
    let robot_type = data.pointer("/app/robot_type").and_then(Value::as_str);
    if robot_type == Some("activity") {
        return Ok(());
    }
    Err(
        BackendError::new("ACTIVITY_TYPE_MISMATCH", "打开的资源不是指令")
            .with_data(json!({"robot_type":robot_type,"app":data.get("app")}))
            .with_hint(
                "从 activity list 返回的 instructions 中选择 appId；普通应用请使用 app open",
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_open_requires_activity_identity() {
        assert!(ensure_activity(&json!({"app":{"robot_type":"activity"}})).is_ok());
        let error = ensure_activity(&json!({"app":{"robot_type":"app"}})).unwrap_err();
        assert_eq!(error.code, "ACTIVITY_TYPE_MISMATCH");
    }

    #[test]
    fn pagination_stops_on_short_or_last_page() {
        assert!(has_more_pages(2, 2, None, 1));
        assert!(!has_more_pages(1, 2, None, 2));
        assert!(has_more_pages(2, 2, Some(2), 1));
        assert!(!has_more_pages(2, 2, Some(2), 2));
    }
}
