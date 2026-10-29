# xbot-cli 命令与行为参考

面向 AI Agent 的影刀（ShadowBot）RPA CLI。默认通过内置 shell-cli、studio-mcp 和 Chrome Bridge 命名管道操作和运行应用；网页命令全部采用管道协议，不依赖 browser-use-mcp。`oapi` 仅是企业版用户明确选择时的开放 API 备用入口。实验性 Windows UIA 观察使用独立 Python 提供器，不是影刀原生 Selector 引擎。

命令按所操作的资源组织：

| 入口 | 职责 |
|---|---|
| `status` / `doctor` | 查看当前状态、诊断连接 |
| `app` | 创建、打开、保存、同步、发布和回收应用 |
| `flow` | Studio 流程的读取、参数、编辑、静态检查与运行 |
| `flow code` / `flow visual` | CodeFlow 源码和 VisualFlow 块 |
| `task` | 已发布应用的运行任务、状态、日志与停止 |
| `oapi` | 企业版可选备用入口：开放 API 运行、调度记录与日志 |
| `web`（兼容 `browser`） | 打开网页、快照、点击、填写和导航 |
| `win32 snapshot` | 显式窗口作用域下的实验性只读 UIA 观察 |
| `tool studio` / `tool browser` | 按 schema 调用原始工具 |
| `wiki` / `schema` / `guide` | 领域知识、命令契约和开发指南 |

## 安装与 Agent 接入

Windows x64、Node.js 22+ 环境可通过 npm 包 `@guanzhu.me/xbot-cli` 安装预编译程序，无需安装 Rust；安装后以 `xbot-cli` 调用，安装前会自动清理旧版遗留的 `xbot` shim 和 PATH 中的旧程序。安装命令和可复制的 Agent 指令见[项目 README](../README.md#安装)。

```powershell
xbot-cli install --skills --json           # 当前工作区 .agents/skills
xbot-cli install --skills --global --json  # 用户 ~/.agents/skills
```

已有 Skill 内容不同时不会静默覆盖；先查看差异，确认覆盖后追加 `--yes`。自动化调用下文的业务命令时均应显式添加 `--json`，示例中的 `<...>` 需替换为实际标识。

当前源码安装 Skill v10 的 39 个资源。`guide automation-routing/web-module/win-design/win-repair/locator-repair/locator-usage/collection-repair/skill-updates` 分别提供模块选择、Web、Windows、生成修复、用途、集合及更新指南。仅在企业版用户明确选择备用入口时读取 `guide oapi`。

`locator-repair.cjs`、`locator-usage.cjs` 和 Python UIA helper 属于安装后的 Skill 脚本，不是 `xbot-cli repair` 子命令。普通 DOM XPath、影刀分段 Selector 和 UIA 资产不能互换；依赖与已验证边界见 [Skill v10 升级说明](skill-v10-upgrade.md)。

## Windows UIA 快照（实验性）

```powershell
xbot-cli help win32 snapshot
xbot-cli schema win32 snapshot
# 用已确认的实际 HWND/PID 替换示例值
xbot-cli win32 snapshot --hwnd 0x12345 --pid 1234 --max-nodes 1000 --max-depth 12 --timeout 10 --json
```

该命令不要求 Studio 打开，不自动选择活动窗口，不点击或保存元素。HWND 支持非零十进制/十六进制，并须属于显式传入的 PID。`--max-nodes` 范围为 1–5000，`--max-depth` 为 0–64，`--timeout` 为 1–60 秒；CLI 使用子进程超时约束提供器，输出超限、依赖缺失、窗口不符和采集错误分别返回失败。

通过 `XBOT_UIA_PYTHON` 指定已安装 `uiautomation` 的 Python 可执行文件，缺省使用 `python`，不自动安装依赖。快照保留完整性和截断原因；部分树不能据此证明元素不存在。ref 仅为本批证据，不能当作 Web ref、动作入口或长期业务身份。密码字段保守隐藏，其他业务文本仍应按任务范围处理。

`win32 snapshot` 不接受 `--expect-app` 或 `--dry-run`。Python Windows 修复 helper 是另一条按需运行链路，其内部预算不能中断卡住的 UIA/provider 调用；不能据 CLI 快照超时能力声称所有 Windows 动作已有硬超时。

## 开发流程

前提：影刀 Shell 已启动并登录。先确认应用，再读取和修改流程：

```bash
xbot-cli status
xbot-cli app open <app-id>
xbot-cli flow list --expect-app <app-id>
xbot-cli flow get main --expect-app <app-id>

# CodeFlow
xbot-cli flow create hello --kind code --expect-app <app-id>
xbot-cli flow params set hello --operations @params.json --expect-app <app-id>
xbot-cli flow code write hello --file hello.py --expect-app <app-id>
xbot-cli flow check hello --expect-app <app-id>
xbot-cli flow verify hello --timeout 600 --expect-app <app-id>

# VisualFlow
xbot-cli flow create report --kind visual --expect-app <app-id>
xbot-cli flow visual edit report --file ops.json --expect-app <app-id>
xbot-cli flow visual fill report --fills @fills.json --expect-app <app-id>
xbot-cli flow visual disable <block-id> --flow report --expect-app <app-id>
xbot-cli flow visual enable <block-id> --flow report --expect-app <app-id>
xbot-cli flow verify report --timeout 600 --expect-app <app-id>
xbot-cli flow visual snapshots report --expect-app <app-id>
```

`flow check` 检查整个应用的静态诊断（`scope: app`），传入流程时先核验目标，不启动流程。`flow verify` 解析目标并检查后实际运行，再读取 Out；失败时附日志尾部，用户暂停或停止则立即返回。修改操作成功、静态检查通过和实际运行成功是不同的结果，不应混为一谈。

已有运行超时时，使用返回的 `run_id` 继续等待，不重新启动：

```bash
xbot-cli flow wait <run-id> --timeout 600 --expect-app <app-id>
```

已发布应用使用独立的任务入口；`task_id` 与 Studio `run_id` 不可混用：

```bash
xbot-cli task run <app-id> --inputs-json @inputs.json
xbot-cli task status <task-id>
xbot-cli task logs <task-id>
xbot-cli task stop <task-id>
```

## 企业版备用入口(oapi)

普通应用运行、状态和日志继续使用 `task run/status/logs`，Studio 流程继续使用
`flow run/verify/wait`。只有企业版用户明确选择开放 API 运行、调度或平台日志时才用
`oapi`（`platform` 为兼容别名），不会因凭据存在或普通命令失败而自动切换。

此备用入口不依赖本机客户端或 Studio。`oapi auth <AccessKeyId> <AccessKeySecret>`
一条命令完成登录并把凭据保存到本地配置，token 过期前复用、过期后自动刷新，
`oapi auth logout` 清除当前授权；也可用环境变量 `XBOT_OAPI_ACCESS_KEY_ID` /
`XBOT_OAPI_ACCESS_KEY_SECRET`（兼容 `XBOT_PLATFORM_*`），专有云设置 `XBOT_OAPI_BASE_URL`。
接口范围、调度标识关系、分页和异步日志恢复详见[平台指南](../skills/xbot-app-dev/references/oapi.md)，
也可离线运行 `xbot-cli guide oapi`。

```powershell
xbot-cli oapi auth status --json
xbot-cli oapi robot list --json
xbot-cli oapi app list --json
xbot-cli oapi job start --app <appId> --robot <accountName> --dry-run --json
xbot-cli oapi schedule runs <scheduleUuid> --json
xbot-cli oapi task status <taskUuid> --json
xbot-cli oapi job status <jobUuid> --json
xbot-cli oapi job logs <jobUuid> --async --timeout 10 --json
xbot-cli oapi user list --json
xbot-cli oapi queue list --json
xbot-cli oapi api schema job.start --json
```

结果在 `data.response` 中保留平台封装。API 成功不代表任务运行成功；
分析错误时读取任务状态、`remark`、截屏地址和日志。
写操作（启动、停止、重试、入队、账号变更等）支持离线预览并要求 `--yes`。

## 应用与流程上下文

`--expect-app <id>` 是 Studio 当前应用的前置断言，用于防止 GUI 切换后操作错误的应用。它不会打开或切换应用；不匹配时停止命令。`schema` 中的 `requires_studio` 标明适用命令；在其他命令上使用会返回参数错误。

多数流程命令的 `flow_id` 可省略：优先活动流程，类型不匹配时仅在唯一候选时选择，歧义返回 `FLOW_ID_REQUIRED` 和候选列表。自动推断适合交互探索，连续修改建议显式指定流程及 `--expect-app`。

`app open` 进入编辑模式；`app view` 是只读临时副本。`app sync` 上传并关闭 Studio。所有应用修改通过 CLI/MCP，不直接编辑影刀应用目录文件。

## 帮助、输出与恢复（v0.6）

裸调用用于发现命令，不需要先启动影刀：

```bash
xbot-cli                       # 简短根帮助
xbot-cli version               # 离线输出一行 CLI 版本
xbot-cli version --json        # 机器读取 data.name 和 data.version
xbot-cli update                # 通过 npm 启动器升级到最新版本
xbot-cli update --check        # 只检查是否有新版本
xbot-cli app                   # 应用操作与示例
xbot-cli flow code             # CodeFlow 操作与示例
xbot-cli tool studio           # 原始工具入口
xbot-cli app --help            # 详细帮助
xbot-cli help app              # 同上
xbot-cli app --json            # 紧凑结构化帮助，不是参数 schema
```

裸根命令和纯分组均显示简短帮助，写 stdout、退出 0；`-h`、`--help`、`help <path>` 默认显示详细文本帮助。帮助和版本查询不调用后端、不检查登录、不初始化应用。帮助在管道中也默认保持文本，便于 Agent 逐层发现；需要机器自省时显式运行 `schema <path>`。

业务结果默认使用 `--format auto`：stdout 是终端时显示简洁文本，重定向或管道中输出紧凑 JSON。脚本应显式使用 `--json`，避免依赖运行环境。

| 输出选项 | 行为 |
|---|---|
| `--format auto` | 按命令用途及 stdout 是否为终端选择输出，默认值 |
| `--format text` | 明确请求文本 |
| `--json` / `--format json` | 明确请求紧凑 JSON，两者等价 |
| `--format pretty` | JSON 缩进显示，字段与紧凑 JSON 相同 |

显式格式优先于自动选择。`schema` 默认始终输出 JSON；`guide` 默认输出 Markdown 文本；这两个默认值和帮助文本一样不随管道改变。请求 JSON 帮助得到当前层级的简短发现信息，不会展开完整参数 schema。`xbot-cli version`、`--version` 和 `-V` 离线输出一行版本文本，管道中同样保持文本；追加 `--json` 则返回 `data.name` 和 `data.version`，`--format pretty` 返回缩进 JSON。`schema version` 可查看版本命令的参数定义。`xbot-cli update` 由 npm 启动器在原生程序启动前完成升级（Windows 不允许替换正在运行的可执行文件），`--check` 只检查不升级；直接对原生可执行文件运行 `update` 会返回 `UPDATE_REQUIRES_LAUNCHER`。

成功结果写 stdout、退出 0。以下为 JSON 模式示例：

```json
{"ok":true,"data":{"verified":true,"run_id":"...","outputs":{"result":"ok"}}}
```

失败仅写 stderr、返回非零退出码，文本或 JSON 渲染跟随所选输出模式。JSON 示例：

```json
{"ok":false,"error":{"type":"operation","code":"FLOW_RUN_FAILED","message":"流程运行失败","hint":"...","retryable":false},"data":{"run_id":"...","log_tail":{}}}
```

分组 `xbot-cli app` 是发现请求；叶命令 `xbot-cli app open` 缺少必填应用 ID 才是参数错误，退出 2，并保留缺失参数、当前用法和修复提示。未知命令或选项也返回参数错误，不会自动改写为另一个操作。

JSON 消费者按 `error.type/code/retryable` 和 `data` 判断下一步，不解析自然语言 message。`data` 可包含已应用的修改或仍在运行的任务；失败不等于未执行，不要无条件整批重试。退出码沿用已有契约：

| 退出码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 操作、API 或运行失败 |
| 2 | 参数或输入校验失败 |
| 3 | 认证、权限或配置问题 |
| 4 | 网络、离线或超时 |
| 5 | 冲突或资源占用 |
| 6 | 内部或协议错误 |
| 10 | 高风险操作需要确认 |

## 预览与确认

```bash
xbot-cli flow code write hello --file hello.py --expect-app <app-id> --dry-run
xbot-cli app publish <app-id> --log "版本说明" --dry-run
# 展示动作、确认目标并取得用户明确同意后：
xbot-cli app publish <app-id> --log "版本说明" --yes
```

`--dry-run` 使用与执行相同的本地输入解析和校验，返回动作与载荷预览，不执行后端操作。它保持离线：当前应用、流程推断、编辑权限、远端工具 schema 与后端是否接受载荷均未验证；`--expect-app` 也只记录为运行前条件。预览成功不代表实际操作会成功。

`app publish`、`app recycle`、`app open --force` 需要 `--yes`；缺少确认返回退出码 10。不要默认追加 `--yes`。开发完成不会自动发布。

## 自省与按需文档

```bash
xbot-cli
xbot-cli flow code
xbot-cli help flow code edit
xbot-cli schema
xbot-cli schema flow code edit
xbot-cli schema flow visual disable
xbot-cli schema tool studio call
xbot-cli guide
xbot-cli guide codeflow
```

先用根帮助找到资源，再读所选分组或命令帮助；仅在需要精确入参时查询对应 schema，不把全部参数定义装进 Agent 上下文。`schema` 从 clap 命令树读取参数，再附共享语义元数据：类型、位置、参数数量、枚举、默认值、别名、效果、上下文、dry-run 适用性、确认要求、JSON 输入形状、动态工具 schema 提示和示例。兼容入口查询会解析到推荐路径。`guide` 与安装后的 Skill references 使用同一份内容。

JSON 参数通常接受字面 JSON、`@file` 或 `-` stdin；源码文件参数接受路径或 `-` stdin。具体编码查看对应 schema 的 `input_shapes`。复杂输入优先文件或 stdin，避免 shell 引号和转义问题。

流程等既有命令的 `--timeout` 范围为 1–86400 秒；实验性 `win32 snapshot --timeout` 为 1–60 秒。以对应命令的 schema 为准。

## 网页操作

`web` 使用影刀浏览器后端，以快照中的真实元素编号操作页面：

```powershell
xbot-cli web open example.com
xbot-cli web fill e4 "hello"
xbot-cli web click e5
xbot-cli web press Enter
xbot-cli web goto https://example.com/page
xbot-cli web snapshot --output page.txt
```

示例编号必须替换为最近快照中的编号。`open` 自动为裸域名补全 HTTPS；`active`（别名 `attach`）附着活动页面。`click`、`dblclick`、`fill`、`hover`、`check`、`uncheck`、`select` 接受元素编号；`type` 和 `press` 默认作用于当前焦点，`press --ref <ref>` 指定元素。导航使用 `goto`、`reload`、`go-back`、`go-forward` 和 `close`。

打开、附着、导航（除 `close`）及上述元素、键盘交互成功后默认获取快照，`--no-snapshot` 可关闭这一步；`eval` 和底层工具调用不追加快照。快照失败会保留操作成功及 `applied: true`，并返回 `warning`；不要因此重复点击或提交。快照正文及 full/diff 元数据来自后端，CLI 不改写编号。`snapshot --output` 保存正文且不覆盖已有文件。终端默认文本输出，管道默认 JSON；`--json` 显式保留完整结果，`--format text` 可强制输出文本。

全部网页命令均通过影刀 Chrome Bridge 命名管道和 CDP 执行，命令形式参考最新 Playwright CLI。`--name` 仅为旧调用保留兼容参数，不会写入影刀元素库。`eval` 保持注入 `activePage` 的只读异步函数体语法。完整参数见 `xbot-cli web --help` 和 [浏览器指南](../skills/xbot-app-dev/references/browser.md)。

可用 `XBOT_WEB_PIPE` 显式指定管道，管道前缀不作为浏览器产品身份判据。快照失败保留已执行动作的证据，不自动重试。

## 原始工具

原始工具也有明确的子命令：

```bash
xbot-cli tool studio list
xbot-cli tool studio schema app.run
xbot-cli tool studio call app.get_info '{}'
xbot-cli tool browser list
xbot-cli tool browser schema web_action
xbot-cli tool browser call web_get_active '{}'
```

工具清单默认返回名字和数量，`list --full` 才含长描述。先查询所选工具的实时 schema，原始调用的字段与实际效果由工具决定，不应假定包装器提供高层工作流的全部校验和确认。`schema tool ... call` 描述 CLI 入参，不替代后端工具 schema。

## 旧命令迁移

以下入口保留兼容，新脚本与新 Skill 使用右侧形式：

| 旧入口 | 推荐入口 |
|---|---|
| `code read/write/edit` | `flow code read/write/edit` |
| `visual list/edit/fill/variables/snapshots` | `flow visual list/edit/fill/variables/snapshots` |
| `visual toggle [flow] comment <id...>` | `flow visual disable <id...> --flow <flow>` |
| `visual toggle [flow] uncomment <id...>` | `flow visual enable <id...> --flow <flow>` |
| `app run/stop` | `task run/stop` |
| `app task-status/task-logs` | `task status/logs` |
| `mcp tools/schema/call` | `tool studio list/schema/call` |
| `browser tools/schema/call` | `tool browser list/schema/call` |
| `browser open/active/snapshot/...` | `web open/active/snapshot/...` |

旧 `mcp <tool> [json]` 和 `flow visual toggle` 也仅用于兼容。`app new`、`flow new` 仍是 `create` 的别名。旧入口采用当前的帮助、输入校验和输出契约；叶命令缺少必填参数仍返回参数错误。

## 设计依据与验收要求

本设计参考社区指南与已投入使用的 Agent CLI，取相互一致的原则，不将其中任意一份当作唯一强制标准：

| 来源 | 本项目采用的原则 |
|---|---|
| [Command Line Interface Guidelines](https://clig.dev/#help) | 裸命令显示简短帮助，示例和下一步入口优先；结果与错误分流 |
| [Agent CLI Design Guide](https://github.com/Johnixr/agent-cli-guide/blob/main/GUIDE.md) | 按资源逐层发现，TTY 文本与非 TTY JSON，按需查询 schema，错误可恢复 |
| [CLI Agent Spec：帮助与版本纯度](https://cli-agent-spec.github.io/requirements/f-068-help-and-version-flag-purity/) | 裸调用帮助退出 0，发现路径不初始化应用或访问后端；帮助不隐式替代 schema |
| [Playwright CLI](https://playwright.dev/agent-cli/introduction) / [命令参考](https://playwright.dev/agent-cli/capabilities) | 简短反馈、显式 JSON 和按需文档，控制 Agent 上下文占用 |

这些来源在退出码数值、错误流和自动 JSON 条件上存在差异。本项目保持既有退出码及 stderr 错误契约；成功的 dry-run 退出 0；`NO_COLOR` 只影响颜色，不切换数据格式。帮助和指南是发现信息，默认文本；普通业务数据才按 TTY 自动选择格式。

以下为离线契约的验收矩阵，不能代替真实影刀客户端运行验收：

| 场景 | 预期结果 |
|---|---|
| 裸根命令、`app`、`flow code`、`tool studio` | 简短文本帮助；stdout；退出 0；无后端调用 |
| `--help`、`-h`、`help <path>` | 当前命令详细帮助；stdout；退出 0 |
| 帮助或 `guide` 输出被捕获、重定向或接入管道 | 默认仍是文本，不自动展开 schema |
| 分组加 `--json` | 有当前动作与用法的紧凑 JSON 帮助，不是完整参数 schema |
| `schema <path>` | 默认 JSON；包含精确参数与共享语义 |
| 业务命令默认格式 | stdout 为终端时文本，非终端时 JSON |
| 业务命令加 `--json` / `--format text` / `--format pretty` | 显式格式优先，pretty 为 JSON |
| `app open` 缺必填 ID、未知命令或选项 | stderr；退出 2；保留具体错误、用法与修复提示 |
| 后端未运行时查询帮助、版本、本地 schema 或 guide | 正常完成，不把连接失败当作发现失败 |

## 构建与验证范围

v0.6 在分离输入与执行边界的基础上，统一帮助、业务输出和机器自省的选择规则：

| 模块 | 职责 |
|---|---|
| `cli.rs` | 命令语法、类型和范围 |
| `input.rs` / `plan.rs` | 一次读取外部输入、规范化操作，供预览或执行 |
| `commands.rs` | 命令路由与兼容入口 |
| `app_cmds.rs` / `flow_cmds.rs` / `extra_cmds.rs` | 应用、运行状态与领域操作 |
| `help.rs` / `output.rs` | 离线逐层帮助、终端与 JSON 渲染、stdout/stderr 输出 |
| `core.rs` / `transport.rs` | 结果封装、错误、会话目标解析、子进程与 MCP 协议 |
| `schema_cmds.rs` | 参数和共享语义元数据的自省 |

输入先校验，再检查当前应用与确认要求，最后才执行。等待失败仍携带原运行标识；写入已完成但诊断失败时携带 `applied: true`。离线测试包含独立进程调用、本地假 MCP 服务、双管道大输出和运行状态恢复。

```powershell
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
npm run build
npm test
npm run test:package
npm pack
```

环境变量 `XBOT_SHELL_CLI`、`XBOT_MCP_ENDPOINT`、`XBOT_BROWSER_MCP_ENDPOINT`、`XBOT_LAUNCHER` 可覆盖标准安装默认值，用于非标准安装或隔离测试。离线单元测试和 mock 后端只能证明对应契约，不能代替真实客户端的开发、运行、同步或发布验收。

源码位于仓库根目录的 `src/`，集成测试位于 `tests/`。`npm run build` 将 Rust release 程序放入 `native/win32-x64/xbot-cli.exe`；`bin/xbot-cli.cjs` 是 npm 命令入口。`npm pack` 仅生成本地包，正式发布步骤见[发布说明](releasing.md)。

Skill 位于 `skills/xbot-app-dev/`，由构建嵌入 CLI，也随 npm 包提供。Agent 应遵守当前工作区 `AGENTS.md`、用户授权及项目实际提供的开发规范；不假定存在某个团队服务。已发布指令的名称、参数和调用契约不得擅自改变。
