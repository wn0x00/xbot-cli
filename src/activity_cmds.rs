use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};
use crate::{app_cmds, oapi, oapi_cli};

const SOURCE_PAGE_SIZE: u32 = 100;
const MAX_SOURCE_PAGES: u32 = 100;

pub fn list(
    search: Option<&str>,
    owner: Option<&str>,
    page: u32,
    page_size: u32,
    all: bool,
) -> Res<CmdOut> {
    let mut activity_rows = Vec::new();
    let offset = (page as usize - 1) * page_size as usize;
    let end = offset + page_size as usize;
    let mut source_complete = false;
    // The API cannot filter by appType, so source pages and instruction pages differ.
    for current in 1..=MAX_SOURCE_PAGES {
        let (data, _) = list_page(search, owner, current, SOURCE_PAGE_SIZE)?;
        let rows = data
            .pointer("/response/data")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                BackendError::new("MCP_BAD_RESPONSE", "app.list 响应缺少指令清单数组")
                    .with_data(data.clone())
            })?;
        activity_rows.extend(
            rows.iter()
                .filter(|row| row.get("appType").and_then(Value::as_str) == Some("activity"))
                .cloned(),
        );
        let page_count = data.pointer("/response/page/pages").and_then(Value::as_u64);
        let source_page_size = data
            .pointer("/response/page/size")
            .and_then(Value::as_u64)
            .and_then(|size| usize::try_from(size).ok())
            .filter(|size| *size > 0)
            .unwrap_or(SOURCE_PAGE_SIZE as usize);
        source_complete = !has_more_pages(rows.len(), source_page_size, page_count, current);
        // One extra instruction distinguishes a full last page from a page with more results.
        if source_complete || (!all && activity_rows.len() > end) {
            break;
        }
    }
    if !all && !source_complete && activity_rows.len() < end {
        return Err(BackendError::new(
            "ACTIVITY_PAGINATION_LIMIT",
            "达到应用列表扫描上限,尚不能填满请求的指令页",
        )
        .with_data(json!({
            "page": page,
            "page_size": page_size,
            "matched_count": activity_rows.len(),
            "scanned_pages": MAX_SOURCE_PAGES,
            "complete": false,
            "truncated": true,
        }))
        .with_hint("使用 --search 或 --owner 缩小查询范围;--all 可返回扫描上限内的部分指令并标记 truncated=true"));
    }
    let total = source_complete.then_some(activity_rows.len());
    let complete = source_complete && (all || activity_rows.len() <= end);
    let truncated = !source_complete && (all || activity_rows.len() <= end);
    let start = offset.min(activity_rows.len());
    let stop = if all {
        activity_rows.len()
    } else {
        end.min(activity_rows.len())
    };
    let activity = app_cmds::concise_app_rows(&activity_rows[start..stop]);
    Ok((
        json!({
            "backend": "oapi",
            "count": activity.len(),
            "activity": activity,
            "page": {
                "page": page,
                "size": page_size,
                "offset": offset,
                "total": total,
                "pages": total.map(|total| total.div_ceil(page_size as usize)),
            },
            "all": all,
            "complete": complete,
            "truncated": truncated,
        }),
        vec![],
    ))
}

fn has_more_pages(rows: usize, page_size: usize, page_count: Option<u64>, current: u32) -> bool {
    page_count.map_or(rows >= page_size, |count| u64::from(current) < count)
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
            .with_hint("从 activity list 返回的 activity 中选择 appId；普通应用请使用 app open"),
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
        assert!(has_more_pages(1, 2, Some(2), 1));
        assert!(has_more_pages(0, 2, Some(2), 1));
    }
}
