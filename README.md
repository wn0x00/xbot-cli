# xbot-cli

面向 AI Agent 的影刀（ShadowBot）RPA 命令行工具。

[![npm 版本](https://img.shields.io/npm/v/%40guanzhu.me%2Fxbot-cli?label=npm&color=blue)](https://www.npmjs.com/package/@guanzhu.me/xbot-cli)
[![CI](https://github.com/wn0x00/xbot-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/wn0x00/xbot-cli/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/wn0x00/xbot-cli?label=release)](https://github.com/wn0x00/xbot-cli/releases/latest)
[![许可证](https://img.shields.io/badge/license-MIT-green)](LICENSE)

通过影刀客户端内置的 shell-cli、studio-mcp 和 browser-use-mcp 管理应用、编辑流程、运行验证及浏览器自动化。Agent 可以逐层查询帮助和参数 schema，通过稳定的 JSON 结果判断下一步，并安装随包提供的开发 Skill。CLI 使用 Rust 实现，通过 npm 分发预编译程序。

## 安装

**环境要求：Windows x64，Node.js 22 或更高版本。** npm 包包含原生可执行文件，不需要用户安装 Rust，也不在 `postinstall` 阶段下载程序。执行影刀操作前，需要本机已安装影刀客户端并启动、登录；帮助、版本、命令 schema 和内置指南可离线使用。

**npm 包名是 `@guanzhu.me/xbot-cli`，安装后可使用 `xbot` 或 `xbot-cli`，两者调用同一程序。**

```powershell
npm install -g '@guanzhu.me/xbot-cli'
xbot version
```

`xbot version` 和 `xbot-cli version` 离线输出当前 CLI 版本，`--version` / `-V` 仍可使用。版本默认输出一行文本，即使接入管道也不变；Agent 可用 `xbot version --json` 读取 `data.name` 和 `data.version`。

不进行全局安装也可调用：

```powershell
npx --yes --package '@guanzhu.me/xbot-cli' xbot --help
```

这里的 `npx --yes` 是 npm 的安装确认选项，不是 xbot-cli 的操作确认。需要可重复执行的 Agent 环境时，将包名写为 `@guanzhu.me/xbot-cli@<version>`，用已验证的实际版本替换 `<version>`。PowerShell 中带 `@` 的 npm 包参数使用单引号包围。

也可从源码构建并直接运行：

```powershell
npm run build
node .\bin\xbot-cli.cjs --help
```

源码构建需要 Windows x64、Rust stable 和 MSVC 构建工具，详见[本地开发](#本地开发)。

## 使用

### 第一步：确认环境

```powershell
xbot status --json
```

### 一个典型工作流

下面是维护 CodeFlow 的示例；将 `<app-id>` 和 `<flow-id>` 替换为实际标识，准备好 `updated.py` 后再执行写入。流程可能包含业务副作用，应在任务授权范围内运行。

```powershell
xbot-cli status --json
xbot-cli app open <app-id> --json
xbot-cli flow list --expect-app <app-id> --json
xbot-cli flow get <flow-id> --expect-app <app-id> --json
xbot-cli schema flow code write --json

xbot-cli flow code write <flow-id> --file updated.py --expect-app <app-id> --dry-run --json
xbot-cli flow code write <flow-id> --file updated.py --expect-app <app-id> --json
xbot-cli flow check <flow-id> --expect-app <app-id> --json
xbot-cli flow verify <flow-id> --timeout 600 --expect-app <app-id> --json
```

维护已有代码时，优先使用 `flow code edit` 做精确替换。需要 VisualFlow、参数声明或浏览器操作时，先按需阅读指南：

```powershell
xbot-cli guide codeflow
xbot-cli guide visualflow
xbot-cli guide browser
xbot-cli guide recovery
```

若等待超时，保留已有运行并继续查询：

```powershell
xbot-cli flow wait <run-id> --timeout 600 --expect-app <app-id> --json
```

已发布应用通过控制台任务运行，使用另一套标识：

```powershell
xbot-cli task run <app-id> --inputs-json @inputs.json --json
xbot-cli task status <task-id> --json
xbot-cli task logs <task-id> --json
```

### 浏览器交互：`xbot web`

使用类似 Playwright CLI 的短命令完成“打开页面 → 读取快照 → 操作元素”的循环，常用动作无需手写 JSON。执行仍通过影刀的浏览器后端，需要已打开 Studio 应用，并有安装影刀扩展的浏览器，或使用内置影刀浏览器。

```powershell
xbot web open baidu.com
xbot web snapshot
# 从当前快照选择实际元素编号；以下 e4、e7 仅演示语法
xbot web fill e4 "影刀 RPA"
xbot web press Enter --ref e4
xbot web click e7
xbot web snapshot --output after-search.txt
```

裸域名会补全为 HTTPS。`open` 不带 URL 时打开 `about:blank`；附着用户当前活动页使用 `xbot web active`（别名 `attach`），在受控页面内跳转使用 `xbot web goto <url>`。

打开、附着、导航和常用交互成功后会自动读取快照，使用返回的真实 ref 继续操作；可加 `--no-snapshot` 跳过。快照保留后端的元数据，内容可能是增量变化。自动快照失败时，动作仍返回成功并附带 warning 和 `applied: true`，此时单独调用 `web snapshot`，不要重复执行刚才的点击或提交。`close` 关闭当前受控页，不读取关闭后的快照。

`snapshot --output <file>`（别名 `--filename`）保存快照文本，不覆盖已有文件。需要分析 JSON 时显式加 `--json`；需要在管道中读取快照文本时使用 `--format text`。

元素操作可用 `--name <名称>` 指定暂存选择器名称；省略时 CLI 自动生成唯一中文临时名。这些名称会暂存在后端，调用 `web_review` 前不会写入项目元素库。`browser` 是 `web` 的兼容别名，旧的 `xbot-cli browser` 调用仍可使用。

更多命令见 `xbot web --help` 或 `xbot guide browser`；上传、下载等专用能力通过 `tool browser` 按实时 schema 调用。

## 给 AI Agent 使用

### 复制这段话发给你的 Agent

向别人推荐 xbot-cli 时只需一句话：**“复制下面这段话，发给你的 AI Agent。”** 适用于 Codex、Claude Code、Cursor、Kimi Code 等任何 Agent：

```text
请使用 xbot-cli 帮我操作影刀 RPA。步骤：
1. 全局安装：npm install -g '@guanzhu.me/xbot-cli'（需要 Windows x64、Node.js 22+，且本机影刀客户端已启动登录）；
2. 在我的项目目录运行 xbot-cli install --skills --json 安装开发 Skill，并阅读生成的 .agents/skills/xbot-app-dev/SKILL.md，按其规范工作；
3. 先运行 xbot-cli status --json 确认环境，用 xbot-cli help <path> / schema <path> / guide <topic> 逐层了解命令；
4. 解析结果一律显式加 --json；修改前先读现状，写入前先用 --dry-run 预览；
5. 发布、回收等不可逆操作必须经我明确授权，未经确认不要执行。
```

### 安装开发 Skill

在 Agent 的工作目录运行：

```powershell
xbot-cli install --skills --json
```

这会写入当前目录的 `.agents/skills/xbot-app-dev/`。若希望多个工作区共用，可安装到用户目录：

```powershell
xbot-cli install --skills --global --json
```

请使用支持 `.agents/skills` 的 Agent，或明确让 Agent 阅读生成的 `SKILL.md`。不支持自动加载 Skill 的工具也可以直接阅读下方完整指令。已有 Skill 内容不同时，CLI 会返回冲突；先查看差异，确认覆盖后再追加 `--yes`。

从 **CLI 0.7.4** 起内置 **Skill v10**。当前源码包含 39 个资源，包括 Web/Windows 模块、12 个脚本、开放平台排障指南及业务任务、用途分析、定位和修复提议模板。使用 0.7.3 或更早版本时，先升级 CLI，再运行 `xbot-cli install --skills` 安装新 Skill；已有本地改动时先查看差异，再决定是否覆盖。

| 能力 | 实现与范围 |
|---|---|
| XPath 生成 | 从当前快照 ref 获取真实节点，结合加权算法和外部 ROBULA+，验证路径唯一指向原节点 |
| Web 修复辅助 | AI 分析单元素/集合用途；单元素 helper 产生只读提议，普通 DOM 集合解析器检查成员、字段、父子定位和旧新兼容 |
| Windows 修复 | Python UIA 按可信原身份恢复单控件定位，可供编码版和可视化流程调用；当前真实业务验证限本机微信 |
| 观察与验收 | 实验性 `win32 snapshot`、隔离测试、动作前复核和结果核对；证据不足或歧义时停止修改 |

Web 生成 helper 需要支持 `stripTypeScriptTypes` 的 Node 运行时和固定修订的外部 ROBULA+ 源码；用途分析需要 Python 3.8+。Windows helper 需要目标运行环境安装 `uiautomation==2.0.29`，无需影刀元素库。第三方源码与 Python 依赖不会自动下载。完整算法、依赖、测试结果和限制见 [Skill v10 升级说明](docs/skill-v10-upgrade.md)。

有效新旧定位继续保留，并在动作前消歧；相对父元素的子定位一并验证。Web helper 不点击、不写应用，后续修改和验收由 Agent 编排。Skill 不是常驻自愈服务，Windows 集合、通用分页/虚拟列表和所有 iframe/Shadow 集合尚未支持；当前浏览器接口仍共享活动页上下文。安装后的模块、引用、脚本与模板按需使用，并继续遵守工作区 `AGENTS.md` 和用户授权范围。

### 完整的 Agent 工作指令

把下面的片段放入项目的 Agent 指令中，并提供要操作的应用 ID、流程和任务目标：

```text
使用 xbot-cli 操作影刀。先阅读已安装的 xbot-app-dev/SKILL.md，
并遵守当前工作区的 AGENTS.md 和用户授权范围。

1. 先运行 xbot-cli status --json。用根帮助、分组帮助和 help <path>
   逐层发现命令；只查询当前任务需要的 schema <path>、guide <topic>。
2. 解析业务结果时显式使用 --json。分别捕获 stdout、stderr 和退出码：
   成功读 stdout 的 ok/data；失败读 stderr 的 error 和可能存在的 data。
3. 修改前先读现状。Studio 操作显式传入 --expect-app <app-id> 和流程 ID；
   --expect-app 只断言当前应用，不会切换应用。复杂输入优先 @file 或 stdin。
4. 修改可先用 --dry-run 预览。它仅做离线校验，不证明权限、当前应用
   或真实运行正常。不要直接编辑影刀应用目录。
5. flow check 只做静态检查；flow verify 会实际运行。
   超时后用返回的 run_id 调用 flow wait，不重新启动第二份流程。
   控制台 task 命令使用 task_id，不能与 Studio run_id 混用。
6. 失败先检查 error.code、retryable、hint 及 data 中的已应用修改和运行状态。
   不盲目重试整个写入操作；retryable=true 也不代表写入一定可以安全重放。
7. 不默认附加 --yes。发布、回收和强制打开仅在用户明确授权后执行；
   已有授权继续有效，开发完成不自动发布。
8. 汇报区分写入成功、静态检查通过和真实运行成功；给出 app_id、flow_id、
   run_id/task_id、Out 或日志证据。未运行的路径明确标为未验证。
```

## 命令与输出契约

| 入口 | 用途 |
|---|---|
| `status` / `doctor` | 当前上下文与连接诊断 |
| `app` | 应用创建、打开、保存、同步、发布和回收 |
| `flow` | 流程读取、参数、静态检查、实际运行与等待 |
| `flow code` / `flow visual` | CodeFlow 源码和 VisualFlow 块 |
| `task` | 已发布应用的控制台任务 |
| `platform` | 企业开放 API、调度执行记录和云端运行日志 |
| `web`（别名 `browser`） | 浏览器上下文、快照与 click/fill/press 等交互 |
| `win32 snapshot` | 实验性只读 UIA 快照，显式限定 HWND/PID，独立于 Studio 与原生 Selector |
| `tool studio` / `tool browser` | 原始工具的发现、schema 和调用 |
| `wiki` / `schema` / `guide` | 官方知识库、命令参数与内置开发指南 |
| `install --skills` | 安装 Agent Skill |

裸调用 `xbot-cli` 或 `xbot-cli app` 返回简短帮助并退出 0。`help <path>` 和 `--help` 返回详细帮助；完整机器参数定义由 `schema <path>` 提供。叶命令缺参才返回参数错误。

普通业务结果默认在终端显示文本、在管道中显示 JSON。**Agent 应显式使用 `--json`**，避免依赖终端检测。帮助和 `guide` 默认是文本，`schema` 默认是 JSON；`--format text/json/pretty` 可明确选择格式。

成功写 stdout、退出 0；失败写 stderr、退出非零码。JSON 消费者应解析字段，不匹配自然语言消息。以下仅展示封装形状，实际 `data` 随命令变化：

```json
{"ok":true,"data":{"run_id":"...","outputs":{}}}
```

```json
{"ok":false,"error":{"type":"operation","code":"FLOW_RUN_FAILED","message":"流程运行失败","retryable":false},"data":{"run_id":"..."}}
```

| 退出码 | 含义与恢复方向 |
|---|---|
| 0 | 成功，检查 `data` 中的具体结果 |
| 1 | 操作或运行失败，读取错误与已有执行结果 |
| 2 | 参数或输入错误，查帮助/schema 后修正 |
| 3 | 认证、权限或配置问题，按提示处理 |
| 4 | 网络、离线或超时，先确认已有任务状态 |
| 5 | 冲突或资源占用，检查当前状态 |
| 6 | 内部或协议错误，保留诊断信息 |
| 10 | 需要明确确认，核实已有用户授权后处理 |

失败可能伴随 `applied: true` 或仍在运行的任务，不能理解为“没有执行”。`--dry-run` 只预览本地载荷；`--expect-app` 只在实际执行时断言当前应用。两者都不代替真实运行验收。

完整参数语义、兼容命令、确认边界和设计依据见 [CLI 参考](docs/cli-reference.md)；Agent 也可运行 `xbot-cli guide cli-contract`。

## 开放平台与调度日志

从 CLI 0.7.5 起，`platform` 直接连接影刀开放 API，不依赖本机客户端。通过环境变量配置
`XBOT_PLATFORM_ACCESS_KEY_ID` / `XBOT_PLATFORM_ACCESS_KEY_SECRET`，或提供
`XBOT_PLATFORM_ACCESS_TOKEN`。专有云可设置 `XBOT_PLATFORM_BASE_URL` 为 HTTPS origin。

```powershell
xbot-cli platform auth status --json
xbot-cli platform auth check --json
xbot-cli platform job list --status error --json
xbot-cli platform job status <jobUuid> --json
xbot-cli platform job logs <jobUuid> --search "错误" --json
xbot-cli platform job logs <jobUuid> --async --timeout 10 --json
xbot-cli platform schedule runs <scheduleUuid> --json
xbot-cli platform task status <taskUuid> --json
xbot-cli platform api list --json
xbot-cli platform api schema log.search --json
xbot-cli guide platform
```

内置 68 个官方文档索引及 44 个接口契约，覆盖鉴权、企业账号、应用、机器人、
调度、日志、队列、文件和运行数据。`platform api call` 提供通用调用，
写操作支持 `--dry-run` 并要求 `--yes`。查询保留原始分页，不自动遍历历史；
异步日志轮询超时保留 `request_id`。应用运行数据查询的时间跨度限制为 7 天。
配置、标识关系和错误分析流程见 [平台指南](skills/xbot-app-dev/references/platform.md)。

## 项目目录

```text
xbot-cli/
├── Cargo.toml / Cargo.lock   Rust 程序与依赖锁定
├── src/                     命令、帮助、输出、工作流与传输实现
├── tests/                   Rust 集成测试
├── package.json             npm 分发与开发命令
├── bin/xbot-cli.cjs          Node 启动器
├── native/win32-x64/         构建生成的 xbot-cli.exe
├── scripts/                 构建及 npm 包验证脚本
├── skills/xbot-app-dev/      Agent Skill 与按需参考文档
├── docs/                    CLI 参考与发布说明
└── .github/workflows/       持续集成与发布工作流
```

Rust 源码、Skill 和文档进入 GitHub 仓库；原生程序由构建生成，随 npm 包分发。个人本地配置、逆向材料、备份及运行数据不属于发布内容。

## 本地开发

从仓库根目录运行：

```powershell
npm run build          # 构建 Rust release，并准备 npm 包中的原生程序
npm test               # 项目自动化测试
npm run test:skill     # 离线 helper 与 Python AST 回归；需要 Python 3.8+
npm run test:package   # 检查 npm 包与启动器
npm pack               # 生成本地 .tgz，不发布
```

Rust 也可单独构建和检查：

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

直接 `cargo build` 的产物位于 `target/`；准备 npm 包使用 `npm run build`。离线测试和模拟后端验证命令契约，不代表已通过真实影刀客户端的运行、同步或发布验收。

非标准客户端安装可使用 `XBOT_SHELL_CLI`、`XBOT_MCP_ENDPOINT`、`XBOT_BROWSER_MCP_ENDPOINT`、`XBOT_LAUNCHER` 覆盖默认路径或连接地址。不要把账号凭据、生产环境配置或运行日志提交到仓库。

源码托管于 [wn0x00/xbot-cli](https://github.com/wn0x00/xbot-cli)，npm 包名为 [`@guanzhu.me/xbot-cli`](https://www.npmjs.com/package/@guanzhu.me/xbot-cli)，CLI 可通过 `xbot` 或 `xbot-cli` 调用。配置发布身份及正式发布步骤见[发布说明](docs/releasing.md)。

## 参与开发与发版

从功能分支或 Fork 提交 PR，使用 `feat(cli): ...`、`fix(flow): ...` 等 Conventional Commits 标题，并以 Squash 方式合并。开发检查、兼容性要求和 PR 说明见[贡献指南](CONTRIBUTING.md)。普通开发 PR 不手动修改版本号。

维护者决定版本，运行 `npm run release:version -- 0.6.2` 同步 Rust 和 npm 的四个版本文件，更新 `CHANGELOG.md`，将改动合并到 `main` 并等待 CI 通过。然后手动推送对应 tag：

```powershell
git tag -a v0.6.2 -m 'Release v0.6.2'
git push origin v0.6.2
```

示例版本需替换为本次实际版本，并在已同步的 `main` 发布提交上打 tag。推送 `v<version>` 后，`publish.yml` 自动完成构建、安装验证、npm OIDC 发布，以及 GitHub Release 创建和附件上传。版本脚本只修改文件，不提交、不打 tag、不发布；普通提交和 PR 合并不会自动发版。完整步骤、发布身份和失败恢复见[发布说明](docs/releasing.md)。

## 许可

本项目使用 [MIT License](LICENSE)。
