//! Exercise the embedded Skill through the CLI in isolated workspace/home roots.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

struct Scratch {
    root: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "xbot-skill-install-{}-{}",
            std::process::id(),
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let workspace = root.join("workspace");
        let home = root.join("home");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&home).unwrap();
        Self {
            root,
            workspace,
            home,
        }
    }

    fn invoke(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
            .args(args)
            .current_dir(&self.workspace)
            .env("USERPROFILE", &self.home)
            .env("XBOT_SHELL_CLI", self.root.join("missing-backend.exe"))
            .env("XBOT_MCP_ENDPOINT", "http://127.0.0.1:1/mcp")
            .env("XBOT_BROWSER_MCP_ENDPOINT", "http://127.0.0.1:1/mcp")
            .output()
            .unwrap()
    }

    fn skill_root(&self, global: bool) -> PathBuf {
        let base = if global { &self.home } else { &self.workspace };
        base.join(".agents").join("skills").join("xbot-app-dev")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(root.parent(), Some(temporary.as_path()));
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("xbot-skill-install-"));
        fs::remove_dir_all(root).unwrap();
    }
}

fn resources(root: &Path) -> Vec<PathBuf> {
    fn visit(directory: &Path, root: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, root, files);
            } else {
                files.push(path.strip_prefix(root).unwrap().to_owned());
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn assert_complete(output: Output, installed: &Path) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["installed"], true);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/xbot-app-dev");
    let expected = resources(&source);
    assert_eq!(expected.len(), 14);
    assert_eq!(resources(installed), expected);
    let mut reported: Vec<PathBuf> = result["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| {
            Path::new(path.as_str().unwrap())
                .strip_prefix(installed)
                .unwrap()
                .to_owned()
        })
        .collect();
    reported.sort();
    assert_eq!(reported, expected);
    for file in expected {
        assert_eq!(
            fs::read(installed.join(&file)).unwrap(),
            fs::read(source.join(&file)).unwrap()
        );
    }
}

#[test]
fn workspace_install_contains_all_resources_and_is_repeatable() {
    let scratch = Scratch::new();
    let installed = scratch.skill_root(false);
    assert_complete(
        scratch.invoke(&["install", "--skills", "--json"]),
        &installed,
    );
    assert_complete(
        scratch.invoke(&["install", "--skills", "--json"]),
        &installed,
    );
    assert!(!scratch.skill_root(true).exists());
}

#[test]
fn global_install_uses_the_isolated_home() {
    let scratch = Scratch::new();
    let installed = scratch.skill_root(true);
    assert_complete(
        scratch.invoke(&["install", "--skills", "--global", "--json"]),
        &installed,
    );
    assert!(!scratch.skill_root(false).exists());
}

#[test]
fn template_conflict_is_detected_before_any_write_and_explicit_overwrite_restores_it() {
    let scratch = Scratch::new();
    let installed = scratch.skill_root(false);
    assert_complete(
        scratch.invoke(&["install", "--skills", "--json"]),
        &installed,
    );
    let missing = installed.join("SKILL.md");
    fs::remove_file(&missing).unwrap();
    let customized = installed.join("templates/locator-registry.json");
    let local_content = b"local template customization\n";
    fs::write(&customized, local_content).unwrap();

    let output = scratch.invoke(&["install", "--skills", "--json"]);
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "SKILL_CONFLICT");
    assert!(
        !missing.exists(),
        "preflight conflict must not partially install files"
    );
    assert_eq!(fs::read(&customized).unwrap(), local_content);
    assert_complete(
        scratch.invoke(&["install", "--skills", "--yes", "--json"]),
        &installed,
    );
}
