//! 渐进式文档与 Skill 安装,内容复用 skills/xbot-app-dev。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

const SKILL: &str = include_str!("../skills/xbot-app-dev/SKILL.md");
const WEB_MODULE: &str = include_str!("../skills/xbot-app-dev/modules/web.md");
const WIN_MODULE: &str = include_str!("../skills/xbot-app-dev/modules/win.md");
const SKILL_UPDATES: &str = include_str!("../skills/xbot-app-dev/UPDATE-RECORD.md");
const AUTOMATION_ROUTING: &str =
    include_str!("../skills/xbot-app-dev/references/automation-routing.md");
const WIN_SNAPSHOT: &str = include_str!("../skills/xbot-app-dev/scripts/win32/snapshot.py");
const WIN_LOCATOR: &str = include_str!("../skills/xbot-app-dev/scripts/win32/locator.cjs");
const WIN_UIA_RUNTIME: &str = include_str!("../skills/xbot-app-dev/scripts/win32/uia_runtime.py");
const WIN_UIA_RUNNER: &str = include_str!("../skills/xbot-app-dev/scripts/win32/run_uia.py");
const WIN_REPAIR: &str = include_str!("../skills/xbot-app-dev/references/win-repair.md");
const CLI_CONTRACT: &str = include_str!("../skills/xbot-app-dev/references/cli-contract.md");
const OAPI: &str = include_str!("../skills/xbot-app-dev/references/oapi.md");
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
const LOCATOR_REPAIR: &str = include_str!("../skills/xbot-app-dev/references/locator-repair.md");
const REPAIR_CONTRACT: &str = include_str!("../skills/xbot-app-dev/templates/repair-contract.json");
const REPAIR_PLAN: &str = include_str!("../skills/xbot-app-dev/templates/repair-plan.json");
const REPAIR_SCRIPT: &str = include_str!("../skills/xbot-app-dev/scripts/locator-repair.cjs");
const REPAIR_GUARD: &str = include_str!("../skills/xbot-app-dev/scripts/locator/guarded-core.js");
const REPAIR_HYBRID: &str = include_str!("../skills/xbot-app-dev/scripts/locator/hybrid-core.js");
const REPAIR_WEIGHTED: &str = include_str!("../skills/xbot-app-dev/scripts/locator/weighted.js");
const REPAIR_ROBULA_ADAPTER: &str =
    include_str!("../skills/xbot-app-dev/scripts/locator/robula-adapter.js");
const LOCATOR_USAGE: &str = include_str!("../skills/xbot-app-dev/references/locator-usage.md");
const USAGE_SCRIPT: &str = include_str!("../skills/xbot-app-dev/scripts/locator-usage.cjs");
const USAGE_AST: &str = include_str!("../skills/xbot-app-dev/scripts/locator/usage-ast.py");
const USAGE_ANALYSIS: &str = include_str!("../skills/xbot-app-dev/templates/usage-analysis.json");
const COLLECTION_REPAIR: &str =
    include_str!("../skills/xbot-app-dev/references/collection-repair.md");
const COLLECTION_COMPAT: &str = include_str!("../skills/xbot-app-dev/scripts/collection-compat.py");
const COLLECTION_ANALYSIS: &str =
    include_str!("../skills/xbot-app-dev/templates/collection-analysis.json");

const TOPICS: &[(&str, &str, &str)] = &[
    (
        "automation-routing",
        "按失败步骤识别 Web/Windows 业务目标并选择模块",
        AUTOMATION_ROUTING,
    ),
    (
        "web-module",
        "Web 自动化模块与既有 v10 能力路由",
        WEB_MODULE,
    ),
    (
        "win-design",
        "Windows 自动化设计、快照和单控件实验边界",
        WIN_MODULE,
    ),
    (
        "win-repair",
        "Python Windows 单控件修复、流程调用和动作对账",
        WIN_REPAIR,
    ),
    (
        "skill-updates",
        "Skill v10 功能更新与能力边界",
        SKILL_UPDATES,
    ),
    (
        "cli-contract",
        "命令层、JSON、退出码、dry-run 与确认门禁",
        CLI_CONTRACT,
    ),
    ("codeflow", "CodeFlow 开发、维护、参数和验收", CODEFLOW),
    ("visualflow", "VisualFlow 块、参数、快照和验收", VISUALFLOW),
    ("browser", "浏览器快照、交互、选择器和专用工具", BROWSER),
    ("recovery", "维护、排障、发布与回收边界", RECOVERY),
    (
        "locator-repair",
        "XPath 零匹配、强身份校验、组合生成器和只读修复提议",
        LOCATOR_REPAIR,
    ),
    (
        "locator-usage",
        "修复前 AI 分析单元素/集合用途、源码证据和交接校验",
        LOCATOR_USAGE,
    ),
    (
        "collection-repair",
        "相似元素契约、旧新父子定位兼容和隔离修复验收",
        COLLECTION_REPAIR,
    ),
    (
        "oapi",
        "开放平台 Open API:授权、机器人、原子调度与调度日志",
        OAPI,
    ),
    ("platform", "oapi 主题的兼容别名", OAPI),
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
        (root.join("modules").join("web.md"), WEB_MODULE),
        (root.join("modules").join("win.md"), WIN_MODULE),
        (root.join("UPDATE-RECORD.md"), SKILL_UPDATES),
        (
            root.join("references").join("automation-routing.md"),
            AUTOMATION_ROUTING,
        ),
        (
            root.join("scripts").join("win32").join("snapshot.py"),
            WIN_SNAPSHOT,
        ),
        (
            root.join("scripts").join("win32").join("locator.cjs"),
            WIN_LOCATOR,
        ),
        (
            root.join("scripts").join("win32").join("uia_runtime.py"),
            WIN_UIA_RUNTIME,
        ),
        (
            root.join("scripts").join("win32").join("run_uia.py"),
            WIN_UIA_RUNNER,
        ),
        (root.join("references").join("win-repair.md"), WIN_REPAIR),
        (root.join("references").join("oapi.md"), OAPI),
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
        (
            root.join("references").join("locator-repair.md"),
            LOCATOR_REPAIR,
        ),
        (
            root.join("templates").join("repair-contract.json"),
            REPAIR_CONTRACT,
        ),
        (root.join("templates").join("repair-plan.json"), REPAIR_PLAN),
        (
            root.join("scripts").join("locator-repair.cjs"),
            REPAIR_SCRIPT,
        ),
        (
            root.join("scripts").join("locator").join("guarded-core.js"),
            REPAIR_GUARD,
        ),
        (
            root.join("scripts").join("locator").join("hybrid-core.js"),
            REPAIR_HYBRID,
        ),
        (
            root.join("scripts").join("locator").join("weighted.js"),
            REPAIR_WEIGHTED,
        ),
        (
            root.join("scripts")
                .join("locator")
                .join("robula-adapter.js"),
            REPAIR_ROBULA_ADAPTER,
        ),
        (
            root.join("references").join("locator-usage.md"),
            LOCATOR_USAGE,
        ),
        (root.join("scripts").join("locator-usage.cjs"), USAGE_SCRIPT),
        (
            root.join("scripts").join("locator").join("usage-ast.py"),
            USAGE_AST,
        ),
        (
            root.join("templates").join("usage-analysis.json"),
            USAGE_ANALYSIS,
        ),
        (
            root.join("references").join("collection-repair.md"),
            COLLECTION_REPAIR,
        ),
        (
            root.join("scripts").join("collection-compat.py"),
            COLLECTION_COMPAT,
        ),
        (
            root.join("templates").join("collection-analysis.json"),
            COLLECTION_ANALYSIS,
        ),
    ]
}

fn home_dir() -> Res<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::new("INTERNAL", "无法确定用户主目录"))
}
