//! 渐进式文档与 Skill 安装,内容复用 skills 下的内置资源。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

const CLI_DOC: &str = include_str!("../skills/xbot-cli-doc/SKILL.md");
const ACTIVITY_DEV: &str = include_str!("../skills/xbot-activity-dev/SKILL.md");
const ACTIVITY_FIX: &str = include_str!("../skills/xbot-activity-fix/SKILL.md");
const OPERATIONS: &str = include_str!("../skills/xbot-cli-doc/references/operations.md");
const METADATA: &str = include_str!("../skills/xbot-activity-dev/references/metadata.md");
const WEB_REPAIR: &str = include_str!("../skills/xbot-activity-fix/references/web-repair.md");
const WINDOWS_REPAIR: &str =
    include_str!("../skills/xbot-activity-fix/references/windows-repair.md");
const FLOW_PARAMETERS: &str = include_str!("../docs/flow-parameters.md");

const TOPICS: &[(&str, &str, &str)] = &[
    ("cli-doc", "xbot-cli 使用方式、功能与注意事项", CLI_DOC),
    ("activity-dev", "指令开发规范、任务清单与模板", ACTIVITY_DEV),
    ("activity-fix", "指令维护规范、任务清单与操作", ACTIVITY_FIX),
    (
        "activity-metadata",
        "文案、公开状态与模块分组的修改步骤",
        METADATA,
    ),
    ("maintenance", "维护流程与恢复交付", ACTIVITY_FIX),
    (
        "element-preconditions",
        "原调用、对象来源与前置核对",
        ACTIVITY_FIX,
    ),
    ("run-existing", "运行已有流程与结果续查", OPERATIONS),
    (
        "development-standards",
        "activity-dev 的兼容别名",
        ACTIVITY_DEV,
    ),
    (
        "automation-routing",
        "按失败调用选择 Web 或 Windows",
        ACTIVITY_FIX,
    ),
    ("web-module", "Web 自动化与定位修复", WEB_REPAIR),
    ("win-design", "Windows 窗口、控件与动作边界", WINDOWS_REPAIR),
    ("win-repair", "Windows 单控件修复与动作对账", WINDOWS_REPAIR),
    ("cli-contract", "命令、JSON、退出码与输入文件", OPERATIONS),
    ("codeflow", "CodeFlow 入口、业务实现与验收", ACTIVITY_DEV),
    ("python-packages", "应用 Python 依赖管理", OPERATIONS),
    ("visualflow", "VisualFlow 源码、块与参数", OPERATIONS),
    ("mcp", "MCP 兼容与能力发现", OPERATIONS),
    (
        "flow-parameters",
        "流程 In/Out 参数与默认值",
        FLOW_PARAMETERS,
    ),
    ("browser", "Web 浏览器命令与能力边界", OPERATIONS),
    ("recovery", "故障调查、恢复与交付边界", ACTIVITY_FIX),
    ("locator-repair", "XPath 单元素修复提议", WEB_REPAIR),
    ("locator-usage", "定位用途分析与证据交接", WEB_REPAIR),
    ("collection-repair", "集合定位契约与修复验收", WEB_REPAIR),
    ("oapi", "企业 API 的发现、认证与使用边界", OPERATIONS),
    ("platform", "oapi 主题的兼容别名", OPERATIONS),
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

struct PendingWrite {
    target: PathBuf,
    temp: PathBuf,
    backup: Option<PathBuf>,
    touched: bool,
}

fn rollback(writes: &mut [PendingWrite]) {
    for write in writes.iter_mut().rev() {
        if write.temp.exists() {
            let _ = fs::remove_file(&write.temp);
        }
        if !write.touched {
            continue;
        }
        if write.target.exists() {
            let _ = fs::remove_file(&write.target);
        }
        if let Some(backup) = write.backup.take() {
            let _ = fs::rename(backup, &write.target);
        }
    }
}

fn install_atomically(files: &[(PathBuf, &str)], fail_at: Option<usize>) -> Res<()> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut writes = Vec::new();
    for (index, (target, content)) in files.iter().enumerate() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                rollback(&mut writes);
                BackendError::new("FILE_WRITE", format!("{}: {error}", parent.display()))
            })?;
        }
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        let temp = target.with_file_name(format!(
            ".{name}.xbot-{}-{nonce}-{index}.tmp",
            std::process::id()
        ));
        writes.push(PendingWrite {
            target: target.clone(),
            temp: temp.clone(),
            backup: None,
            touched: false,
        });
        fs::write(&temp, content).map_err(|error| {
            rollback(&mut writes);
            BackendError::new("FILE_WRITE", format!("{}: {error}", temp.display()))
        })?;
    }
    for index in 0..writes.len() {
        if fail_at == Some(index) {
            rollback(&mut writes);
            return Err(BackendError::new("FILE_WRITE", "模拟提交失败"));
        }
        let name = writes[index]
            .target
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let backup = writes[index].target.with_file_name(format!(
            ".{name}.xbot-{}-{nonce}-{index}.bak",
            std::process::id()
        ));
        let result = (|| -> Res<()> {
            if writes[index].target.exists() {
                fs::rename(&writes[index].target, &backup).map_err(|error| {
                    BackendError::new("FILE_WRITE", format!("备份失败: {error}"))
                })?;
                writes[index].backup = Some(backup);
            }
            writes[index].touched = true;
            fs::rename(&writes[index].temp, &writes[index].target)
                .map_err(|error| BackendError::new("FILE_WRITE", format!("替换失败: {error}")))?;
            Ok(())
        })();
        if let Err(error) = result {
            rollback(&mut writes);
            return Err(error);
        }
    }
    for write in &writes {
        if let Some(backup) = &write.backup {
            let _ = fs::remove_file(backup);
        }
    }
    Ok(())
}

pub fn install_skills(global: bool, overwrite: bool) -> Res<CmdOut> {
    let base = if global {
        home_dir()?
    } else {
        std::env::current_dir().map_err(|e| BackendError::new("INTERNAL", e.to_string()))?
    };
    let root = base.join(".agents").join("skills");
    let bundles = [
        ("xbot-cli-doc", cli_doc_files(&root.join("xbot-cli-doc"))),
        (
            "xbot-activity-dev",
            activity_dev_files(&root.join("xbot-activity-dev")),
        ),
        (
            "xbot-activity-fix",
            activity_fix_files(&root.join("xbot-activity-fix")),
        ),
    ];
    let files: Vec<(PathBuf, &str)> = bundles
        .iter()
        .flat_map(|(_, files)| files.iter().map(|(path, content)| (path.clone(), *content)))
        .collect();
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
    let backups = install_with_legacy_backup(&root, &files, None)?;
    let skills: Vec<Value> = bundles
        .iter()
        .map(|(name, files)| {
            let paths: Vec<Value> = files.iter().map(|(path, _)| json!(path)).collect();
            json!({"name": name, "path": root.join(name), "files": paths})
        })
        .collect();
    // Keep the single-skill response shape; it now identifies the first bundle.
    Ok((
        json!({
            "installed": true,
            "path": skills[0]["path"],
            "files": skills[0]["files"],
            "skills": skills,
            "backups": backups,
        }),
        vec![],
    ))
}

fn install_with_legacy_backup(
    root: &Path,
    files: &[(PathBuf, &str)],
    fail_at: Option<usize>,
) -> Res<Vec<Value>> {
    let mut legacy = Vec::new();
    for name in ["xbot-app-dev", "xbot-development-standards"] {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                legacy.push(path);
            }
            Ok(_) => {
                return Err(BackendError::new(
                    "FILE_WRITE",
                    format!("旧 Skill 不是普通目录，请先检查: {}", path.display()),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BackendError::new(
                    "FILE_WRITE",
                    format!("{}: {error}", path.display()),
                ));
            }
        }
    }
    if legacy.is_empty() {
        install_atomically(files, fail_at)?;
        return Ok(vec![]);
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_root = root
        .parent()
        .ok_or_else(|| BackendError::new("FILE_WRITE", "Skill 目录缺少父目录"))?
        .join("skill-backups");
    fs::create_dir_all(&backup_root)
        .map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
    let batch = backup_root.join(format!("legacy-{}-{nonce}", std::process::id()));
    fs::create_dir(&batch).map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
    let mut moved = Vec::new();
    let result = (|| -> Res<()> {
        for source in legacy {
            let target = batch.join(source.file_name().unwrap());
            fs::rename(&source, &target).map_err(|error| {
                BackendError::new(
                    "FILE_WRITE",
                    format!("备份 {} 失败: {error}", source.display()),
                )
            })?;
            moved.push((source, target));
        }
        install_atomically(files, fail_at)
    })();
    if let Err(error) = result {
        let mut pending = Vec::new();
        for (source, target) in moved.iter().rev() {
            if let Err(restore_error) = fs::rename(target, source) {
                pending.push(
                    json!({"original": source, "path": target, "error": restore_error.to_string()}),
                );
            }
        }
        let _ = fs::remove_dir(&batch);
        return Err(error.with_data(json!({"pending_backups": pending})));
    }
    Ok(moved
        .iter()
        .map(|(source, target)| json!({"name": source.file_name().unwrap().to_string_lossy(), "original": source, "path": target}))
        .collect())
}

fn cli_doc_files(root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            root.join("agents/openai.yaml"),
            include_str!("../skills/xbot-cli-doc/agents/openai.yaml"),
        ),
        (
            root.join("references/operations.md"),
            include_str!("../skills/xbot-cli-doc/references/operations.md"),
        ),
        (
            root.join("SKILL.md"),
            include_str!("../skills/xbot-cli-doc/SKILL.md"),
        ),
    ]
}

fn activity_dev_files(root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            root.join("agents/openai.yaml"),
            include_str!("../skills/xbot-activity-dev/agents/openai.yaml"),
        ),
        (
            root.join("assets/activity-entry.py"),
            include_str!("../skills/xbot-activity-dev/assets/activity-entry.py"),
        ),
        (
            root.join("assets/activity-test.py"),
            include_str!("../skills/xbot-activity-dev/assets/activity-test.py"),
        ),
        (
            root.join("assets/flow-params.example.json"),
            include_str!("../skills/xbot-activity-dev/assets/flow-params.example.json"),
        ),
        (
            root.join("assets/metadata-policy.example.json"),
            include_str!("../skills/xbot-activity-dev/assets/metadata-policy.example.json"),
        ),
        (
            root.join("references/metadata.md"),
            include_str!("../skills/xbot-activity-dev/references/metadata.md"),
        ),
        (
            root.join("scripts/check_metadata.py"),
            include_str!("../skills/xbot-activity-dev/scripts/check_metadata.py"),
        ),
        (
            root.join("scripts/test_check_metadata.py"),
            include_str!("../skills/xbot-activity-dev/scripts/test_check_metadata.py"),
        ),
        (
            root.join("SKILL.md"),
            include_str!("../skills/xbot-activity-dev/SKILL.md"),
        ),
    ]
}

fn activity_fix_files(root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            root.join("agents/openai.yaml"),
            include_str!("../skills/xbot-activity-fix/agents/openai.yaml"),
        ),
        (
            root.join("assets/acceptance-case.json"),
            include_str!("../skills/xbot-activity-fix/assets/acceptance-case.json"),
        ),
        (
            root.join("assets/collection-analysis.json"),
            include_str!("../skills/xbot-activity-fix/assets/collection-analysis.json"),
        ),
        (
            root.join("assets/locator-registry.json"),
            include_str!("../skills/xbot-activity-fix/assets/locator-registry.json"),
        ),
        (
            root.join("assets/repair-contract.json"),
            include_str!("../skills/xbot-activity-fix/assets/repair-contract.json"),
        ),
        (
            root.join("assets/repair-plan.json"),
            include_str!("../skills/xbot-activity-fix/assets/repair-plan.json"),
        ),
        (
            root.join("assets/usage-analysis.json"),
            include_str!("../skills/xbot-activity-fix/assets/usage-analysis.json"),
        ),
        (
            root.join("references/web-repair.md"),
            include_str!("../skills/xbot-activity-fix/references/web-repair.md"),
        ),
        (
            root.join("references/windows-repair.md"),
            include_str!("../skills/xbot-activity-fix/references/windows-repair.md"),
        ),
        (
            root.join("scripts/collection-compat.py"),
            include_str!("../skills/xbot-activity-fix/scripts/collection-compat.py"),
        ),
        (
            root.join("scripts/locator/guarded-core.js"),
            include_str!("../skills/xbot-activity-fix/scripts/locator/guarded-core.js"),
        ),
        (
            root.join("scripts/locator/hybrid-core.js"),
            include_str!("../skills/xbot-activity-fix/scripts/locator/hybrid-core.js"),
        ),
        (
            root.join("scripts/locator/robula-adapter.js"),
            include_str!("../skills/xbot-activity-fix/scripts/locator/robula-adapter.js"),
        ),
        (
            root.join("scripts/locator/usage-ast.py"),
            include_str!("../skills/xbot-activity-fix/scripts/locator/usage-ast.py"),
        ),
        (
            root.join("scripts/locator/weighted.js"),
            include_str!("../skills/xbot-activity-fix/scripts/locator/weighted.js"),
        ),
        (
            root.join("scripts/locator-repair.cjs"),
            include_str!("../skills/xbot-activity-fix/scripts/locator-repair.cjs"),
        ),
        (
            root.join("scripts/locator-usage.cjs"),
            include_str!("../skills/xbot-activity-fix/scripts/locator-usage.cjs"),
        ),
        (
            root.join("scripts/win32/actions.py"),
            include_str!("../skills/xbot-activity-fix/scripts/win32/actions.py"),
        ),
        (
            root.join("scripts/win32/locator.cjs"),
            include_str!("../skills/xbot-activity-fix/scripts/win32/locator.cjs"),
        ),
        (
            root.join("scripts/win32/run_uia.py"),
            include_str!("../skills/xbot-activity-fix/scripts/win32/run_uia.py"),
        ),
        (
            root.join("scripts/win32/snapshot.py"),
            include_str!("../skills/xbot-activity-fix/scripts/win32/snapshot.py"),
        ),
        (
            root.join("scripts/win32/uia_runtime.py"),
            include_str!("../skills/xbot-activity-fix/scripts/win32/uia_runtime.py"),
        ),
        (
            root.join("SKILL.md"),
            include_str!("../skills/xbot-activity-fix/SKILL.md"),
        ),
    ]
}

fn home_dir() -> Res<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::new("INTERNAL", "无法确定用户主目录"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn failed_file_backup_does_not_delete_the_original() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = std::env::temp_dir().join(format!(
            "xbot-skill-locked-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("SKILL.md");
        fs::write(&target, "original").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        install_atomically(&[(target.clone(), "replacement")], None).unwrap_err();
        drop(locked);
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_install_restores_archived_legacy_skills() {
        let root = std::env::temp_dir().join(format!(
            "xbot-skill-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let skills = root.join(".agents/skills");
        for name in ["xbot-app-dev", "xbot-development-standards"] {
            fs::create_dir_all(skills.join(name)).unwrap();
            fs::write(skills.join(name).join("SKILL.md"), name).unwrap();
        }
        let files = vec![(skills.join("xbot-cli-doc/SKILL.md"), "new")];
        install_with_legacy_backup(&skills, &files, Some(0)).unwrap_err();
        for name in ["xbot-app-dev", "xbot-development-standards"] {
            assert_eq!(
                fs::read_to_string(skills.join(name).join("SKILL.md")).unwrap(),
                name
            );
        }
        assert!(!files[0].0.exists());
        assert_eq!(
            fs::read_dir(root.join(".agents/skill-backups"))
                .unwrap()
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skill_file_commit_rolls_back_without_touching_uncommitted_targets() {
        let root = std::env::temp_dir().join(format!(
            "xbot-skill-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let files = vec![
            (root.join("a.md"), "new-a"),
            (root.join("b.md"), "new-b"),
            (root.join("c.md"), "new-c"),
        ];
        for (path, name) in [("a.md", "old-a"), ("b.md", "old-b"), ("c.md", "old-c")] {
            fs::write(root.join(path), name).unwrap();
        }
        install_atomically(&files, Some(1)).unwrap_err();
        for (path, expected) in [("a.md", "old-a"), ("b.md", "old-b"), ("c.md", "old-c")] {
            assert_eq!(fs::read_to_string(root.join(path)).unwrap(), expected);
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        install_atomically(&files, None).unwrap();
        for (path, expected) in [("a.md", "new-a"), ("b.md", "new-b"), ("c.md", "new-c")] {
            assert_eq!(fs::read_to_string(root.join(path)).unwrap(), expected);
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        fs::remove_dir_all(root).unwrap();
    }
}
