# xbot-cli 契约

## 安装与运行环境

npm 包 `@guanzhu.me/xbot-cli` 为 Windows x64 提供预编译程序，要求 Node.js 22+，安装后 `xbot` 与 `xbot-cli` 调用同一程序；用户无需 Rust，不通过 postinstall 下载程序。默认操作要求客户端已安装、启动并登录；仅企业版用户明确选择 `platform` 备用入口时使用企业 API 凭据。`xbot install --skills` 将 Skill 写入当前工作区的 `.agents/skills/xbot-app-dev`，`--global` 则写入用户目录；已有内容不同时先报冲突，确认覆盖后才使用 `--yes`。

需要解析业务结果时始终显式使用 `--json`；下文省略此参数的示例只展示操作语法。自动化必须分别捕获 stdout、stderr 和退出码，不把两条输出流拼接成一个 JSON 文档。

## 资源和工作流

- `app` 管应用生命周期；`flow` 管当前 Studio 流程，源码使用 `flow code`，块使用 `flow visual`。
- `flow check` 检查整个应用的静态诊断，传入流程时先核验目标（`scope: app`），不启动流程；`flow verify` = 静态诊断 + 实际运行 + Out；已有运行用 `flow wait <run-id>` 继续等待。
- `task run/stop/status/logs` 管已发布应用的运行任务，使用 task_id，不与 Studio run_id 混用。
- `oapi` 是企业版可选备用入口（`platform` 为兼容别名），只有用户明确选择开放 API 运行/调度或平台日志时才使用。`oapi auth <AccessKeyId> <AccessKeySecret>` 一条命令登录并保存凭据，token 自动刷新，`oapi auth logout` 清除授权。普通命令不读取企业凭据来选择通道，报错后不自动切换；应用仍用 `task run/status/logs`，流程仍用 `flow run/verify/wait`。备用入口的配置及标识关系见 [oapi.md](oapi.md)。
- `web` 管浏览器上下文、快照与交互，`browser` 是兼容别名。使用 `web click <ref>`、`web fill <ref> <text>`、`web press <key>` 等高层命令，不为常用动作编写 JSON。
- `tool studio` / `tool browser` 提供 `list/schema/call`；优先资源命令，不足时才用原始工具。

```bash
xbot-cli status
xbot-cli flow get main --expect-app <app-id>
xbot-cli flow check main --expect-app <app-id>
xbot-cli flow verify main --timeout 600 --expect-app <app-id>
xbot-cli flow wait <run-id> --timeout 600 --expect-app <app-id>
```

## 上下文

`--expect-app <id>` 在 Studio 命令开始前核对当前应用，不打开或切换应用，不匹配时停止。其他命令携带它会返回参数错误；适用性见 schema 的 `requires_studio`。

多数流程命令的 `flow_id` 可省略：优先活动流程；类型不匹配时仅在唯一候选时自动选择；歧义时读 `FLOW_ID_REQUIRED.data.candidates` 并显式指定。连续修改建议显式 app_id 断言和 flow_id。

`flow visual enable/disable <block-id...> --flow <flow-id>` 用明确的操作代替 toggle；省略 `--flow` 时采用同样的上下文解析规则。

`web open [url]` 建立浏览器上下文，`web active`（别名 `attach`）附着当前活动页，`web goto <url>` 在受控页内跳转。它们使用影刀当前浏览器上下文，不提供 Playwright CLI 的命名会话隔离。`web` 依赖的 Studio 和浏览器条件见 [browser.md](browser.md)。

打开、附着、导航和常用交互成功后自动读取快照，可加 `--no-snapshot` 跳过；`web close` 不读取快照。动作成功后取快照失败仍返回 `ok: true`、退出 0，并提供 warning 和 `applied: true`。此时只补取 `web snapshot`，不要重放动作。快照保留后端元数据，可能是增量；ref 必须来自当前页面证据。`web snapshot --output <file>`（别名 `--filename`）保存文本，不覆盖已有文件。

`--name` 指定元素暂存选择器名称；省略时 CLI 自动生成唯一中文临时名。操作会在后端暂存名称，只有 `web_review` 才将选定的选择器写入项目元素库。`web eval` 是注入 `activePage` 的只读 Playwright 函数体，不接受任意写入脚本来代替交互命令。

## 帮助与输出（v0.6）

```bash
xbot-cli                       # 简短根帮助，退出 0
xbot-cli version               # 离线输出一行 CLI 版本
xbot-cli version --json        # 机器读取 data.name 和 data.version
xbot-cli update                # 升级 CLI 到最新版本(npm 启动器执行)
xbot-cli update --check        # 只检查是否有新版本
xbot-cli app                   # 分组帮助，退出 0
xbot-cli flow code             # 嵌套分组帮助，退出 0
xbot-cli tool studio           # 原始工具分组帮助，退出 0
xbot-cli help app open         # 详细命令帮助
xbot-cli app --json            # 当前分组的结构化帮助，不是完整 schema
xbot-cli status --json         # 显式机器结果
```

裸根命令和纯分组是发现请求，默认显示简短文本帮助；`-h`、`--help`、`help <path>` 默认显示详细文本帮助。帮助写 stdout、退出 0，不访问后端、不检查登录、不初始化应用，即使在管道中也默认保持文本。`xbot-cli version`（也可使用 `xbot version`）、`--version` 和 `-V` 离线返回一行版本文本，管道中同样保持文本；追加 `--json` 则返回 `data.name` 和 `data.version`，`--format pretty` 返回缩进 JSON。`schema version` 可查看版本命令的参数定义。`xbot-cli update` 由 npm 启动器在原生程序启动前执行升级（Windows 不允许替换正在运行的可执行文件），`--check` 只检查不升级；升级全局安装时优先使用 `xbot-cli update` 而不是让用户手动拼 npm 命令。

业务结果默认 `--format auto`：stdout 是终端时为简洁文本，非终端时为紧凑 JSON。需要解析结果时显式使用 `--json`，它等价于 `--format json`。`--format text` 强制文本；`--format pretty` 仍是 JSON，仅增加缩进。显式格式优先于自动选择，也可用于帮助、schema 和 guide。

`schema` 默认始终为 JSON；`guide` 默认始终为 Markdown 文本，不随管道改变。JSON 帮助只给出当前层级的发现信息；需要精确参数和语义时另查 `schema <path>`。

- 成功：结果写 stdout，退出 0；JSON 模式为 `{"ok":true,"data":...}`。
- 失败：仅 stderr、非零退出码，按所选模式渲染；JSON 模式为 `{"ok":false,"error":{"type","code","message","hint","retryable"},"data":...}`。
- 分组 `app` 不报缺少子命令；叶命令 `app open` 缺少必填 ID 返回退出 2，保留缺失参数、当前用法和修复提示。未知命令和选项也返回参数错误。
- `data` 可包含部分成功、run_id、终态或日志尾部。先读实际应用状态和 hint，再决定重试；不只看 message。`retryable=true` 表示错误可恢复，不保证写入可安全重放；有 `applied: true` 时先确认修改已经应用。

| 退出码 | 含义 | Agent 处理 |
|---|---|---|
| 0 | 成功 | 继续 |
| 1 | 操作/API/运行失败 | 读 data 和 hint |
| 2 | 参数或输入错误 | 用 `schema <path>` 修正，不原样重试 |
| 3 | 认证/权限/配置 | 请用户登录或授权 |
| 4 | 网络/离线/超时 | 先处理 hint，再有界重试 |
| 5 | 冲突/占用 | 检查当前状态，避免并发 |
| 6 | 内部或协议错误 | 保留响应，报告问题 |
| 10 | 高风险确认门禁 | 展示动作，在用户明确授权后追加 `--yes`；已有授权无需重复请求 |

## 预览和安全

```bash
xbot-cli flow code write main --file main.py --expect-app <app-id> --dry-run
xbot-cli app publish <app-id> --log "..." --dry-run
xbot-cli app publish <app-id> --log "..." --yes  # 仅在用户明确同意后
```

- `--dry-run` 与执行共用本地输入解析和校验，返回 `data.dry_run=true`，不执行后端操作。
- 预览保持离线：运行时上下文、目标解析、权限、动态工具 schema 和后端接受性未验证；`--expect-app` 只是待执行前置条件。预览通过不能声称已验证应用。
- `app publish`、`app recycle`、`app open --force` 需要用户明确确认和 `--yes`；不替用户确认，不将它作为默认参数。
- 原始工具的效果由工具决定；高层工作流的额外校验和确认不应假定同样存在于任意 raw call。
- JSON 参数支持字面 JSON、`@file`、`-` stdin；源码文件参数支持路径和 `-` stdin。复杂输入优先文件或 stdin。

## 自省

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
xbot-cli tool studio list
xbot-cli tool studio schema app.run
xbot-cli tool studio call app.get_info '{}'
```

先看当前层级的帮助，再按需查询所选命令的 schema。schema 返回 clap 参数与共享语义：aliases、value_type、position、arity、choices、defaults、effects、context、requires_studio、supports_dry_run、confirmation、input_shapes、examples。`arity.max=null` 表示不限数量；`input_shapes` 给出本地接受的形状，`dynamic_schema_hint` 指向运行中客户端的具体工具 schema。

`tool ... list --full` 才返回长描述。先按需查工具 schema，再调用。默认不把长源码、原始 MCP 数据或全部 Wiki 装进上下文。

## 兼容入口

旧顶层 `code` / `visual` 对应 `flow code` / `flow visual`；旧 `app run/stop/task-status/task-logs` 对应 `task run/stop/status/logs`；旧 `mcp tools/schema/call` 与 `browser tools/schema/call` 对应 `tool studio` 与 `tool browser`。这些入口保留兼容但隐藏，新任务使用推荐命令。

`browser` 分组本身仍作为 `web` 的可见别名；`browser act <json>` 保留为隐藏的高级兼容入口。新任务使用 `web click/fill/press` 等明确命令。`web attach`、`web back`、`web forward` 分别是 `active`、`go-back`、`go-forward` 的别名。

`visual toggle [flow] comment <id...>` 可迁移为 `flow visual disable <id...> --flow <flow>`，`uncomment` 迁移为 `enable`。
