//! 渐进式文档与 Skill 安装,内容复用 skills/xbot-app-dev。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

const SKILL: &str = include_str!("../skills/xbot-app-dev/SKILL.md");
const CLI_CONTRACT: &str = include_str!("../skills/xbot-app-dev/references/cli-contract.md");
const CODEFLOW: &str = include_str!("../skills/xbot-app-dev/references/codeflow.md");
const VISUALFLOW: &str = include_str!("../skills/xbot-app-dev/references/visualflow.md");
const BROWSER: &str = include_str!("../skills/xbot-app-dev/references/browser.md");
const RECOVERY: &str = include_str!("../skills/xbot-app-dev/references/recovery.md");
const PROJECT_CONVENTIONS: &str =
    include_str!("../skills/xbot-app-dev/references/project-conventions.md");
const WEB_LIFECYCLE: &str = include_str!("../skills/xbot-app-dev/references/web-lifecycle.md");
const WEB_MAINTENANCE: &str = include_str!("../skills/xbot-app-dev/references/web-maintenance.md");
const WEB_ACCEPTANCE: &str = include_str!("../skills/xbot-app-dev/references/web-acceptance.md");
const WEB_RISK: &str = include_str!("../skills/xbot-app-dev/references/web-risk.md");
const TASK_SPEC: &str = include_str!("../skills/xbot-app-dev/templates/task-spec.md");
const LOCATOR_REGISTRY: &str =
    include_str!("../skills/xbot-app-dev/templates/locator-registry.json");
const ACCEPTANCE_CASE: &str = include_str!("../skills/xbot-app-dev/templates/acceptance-case.json");

const TOPICS: &[(&str, &str, &str)] = &[
    (
        "cli-contract",
        "命令层、JSON、退出码、dry-run 与确认门禁",
        CLI_CONTRACT,
    ),
    ("codeflow", "CodeFlow 开发、维护、参数和验收", CODEFLOW),
    ("visualflow", "VisualFlow 块、参数、快照和验收", VISUALFLOW),
    ("browser", "浏览器快照、交互、选择器和专用工具", BROWSER),
    ("recovery", "维护、排障、发布与回收边界", RECOVERY),
];

pub fn cmd_guide(topic: Option<&str>) -> Res<CmdOut> {
    if let Some(topic) = topic {
        let (_, _, content) = TOPICS
            .iter()
            .find(|(name, _, _)| *name == topic)
            .ok_or_else(|| {
                BackendError::new("BAD_ARGS", format!("未知 guide 主题: {topic}"))
                    .with_hint("xbot-cli guide 查看全部主题")
            })?;
        return Ok((json!({"topic": topic, "markdown": content}), vec![]));
    }
    let topics: Vec<Value> = TOPICS
        .iter()
        .map(|(name, description, _)| json!({"topic": name, "description": description}))
        .collect();
    Ok((json!({"topics": topics}), vec![]))
}

pub fn install_skills(global: bool, overwrite: bool) -> Res<CmdOut> {
    let base = if global {
        home_dir()?
    } else {
        std::env::current_dir().map_err(|e| BackendError::new("INTERNAL", e.to_string()))?
    };
    let root = base.join(".agents").join("skills").join("xbot-app-dev");
    let files = skill_files(&root);
    for (path, content) in &files {
        if path.exists() && fs::read_to_string(path).ok().as_deref() != Some(*content) && !overwrite
        {
            return Err(BackendError::new(
                "SKILL_CONFLICT",
                format!("Skill 文件已被修改: {}", path.display()),
            )
            .with_data(json!({"path": path, "risk": "overwrite-local-file"}))
            .with_hint("确认覆盖后,在原命令末尾追加 --yes"));
        }
    }
    for (path, content) in &files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                BackendError::new("FILE_WRITE", format!("{}: {e}", parent.display()))
            })?;
        }
        fs::write(path, content)
            .map_err(|e| BackendError::new("FILE_WRITE", format!("{}: {e}", path.display())))?;
    }
    let paths: Vec<Value> = files.iter().map(|(path, _)| json!(path)).collect();
    Ok((
        json!({"installed": true, "path": root, "files": paths}),
        vec![],
    ))
}

fn skill_files(root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (root.join("SKILL.md"), SKILL),
        (
            root.join("references").join("cli-contract.md"),
            CLI_CONTRACT,
        ),
        (root.join("references").join("codeflow.md"), CODEFLOW),
        (root.join("references").join("visualflow.md"), VISUALFLOW),
        (root.join("references").join("browser.md"), BROWSER),
        (root.join("references").join("recovery.md"), RECOVERY),
        (
            root.join("references").join("project-conventions.md"),
            PROJECT_CONVENTIONS,
        ),
        (
            root.join("references").join("web-lifecycle.md"),
            WEB_LIFECYCLE,
        ),
        (
            root.join("references").join("web-maintenance.md"),
            WEB_MAINTENANCE,
        ),
        (
            root.join("references").join("web-acceptance.md"),
            WEB_ACCEPTANCE,
        ),
        (root.join("references").join("web-risk.md"), WEB_RISK),
        (root.join("templates").join("task-spec.md"), TASK_SPEC),
        (
            root.join("templates").join("locator-registry.json"),
            LOCATOR_REGISTRY,
        ),
        (
            root.join("templates").join("acceptance-case.json"),
            ACCEPTANCE_CASE,
        ),
    ]
}

fn home_dir() -> Res<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::new("INTERNAL", "无法确定用户主目录"))
}
