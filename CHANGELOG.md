# Changelog

## [0.10.3](https://github.com/wn0x00/xbot-cli/compare/v0.10.2...v0.10.3) (2026-11-10)

### 修复指令分页数量

- 修复 `activity list` 先对普通应用与指令的混合列表分页、再过滤指令，导致实际返回条数少于 `--page-size` 的问题。现在先筛选指令再分页，除末页外返回指定数量的指令，连续页不会因为混合资源占位而缺项。
- `page.page/size/offset` 统一使用指令分页口径，不再透传混合资源的总数和页数。只有扫描完整个底层列表后，`page.total/pages` 才返回准确的指令总数和总页数；尚未算出时返回 `null`。
- 优先依据服务端总页数判断是否继续读取，避免服务端返回较短批次时提前停止；缺少总页数时按实际页大小判断。

### 兼容与迁移

- `--page` 现在是筛选后的指令页码，`--page-size` 是每页指令数；`--all` 返回从指定指令页开始的全部剩余指令。名称、所有者过滤在所有底层请求中保持一致。
- 开放平台接口未提供类型过滤参数，因此填满一个指令页可能需要读取多页应用列表；每批最多 100 条，一次命令最多扫描 100 页。默认仍只返回一页指令，不默认返回全部指令。
- 新增 `truncated`，表示触及扫描上限而未扫描完整。普通分页达到上限且无法填满请求页时返回 `ACTIVITY_PAGINATION_LIMIT`，不再将不完整结果伪装成末页；可用 `--search`、`--owner` 缩小范围，或使用 `--all` 获取上限内的部分结果并检查 `truncated`。
- **调用方请注意**：`page.total/pages` 需允许 `null`，不能再将平台应用总数当作指令总数；`complete=true` 表示本次返回结果之后没有剩余指令，`complete=false` 时需结合 `truncated` 区分后续结果与扫描上限。

### 验证

- 补充混合资源、跨底层页补齐、指定指令页、末页与越界页、显式全量、缺失分页信息、过滤参数传递和扫描上限的回归覆盖。
- 已在真实环境只读验证连续页数量、跨页无重复、全量数量一致，以及末页和越界页行为。

## [0.10.2](https://github.com/wn0x00/xbot-cli/compare/v0.10.1...v0.10.2) (2026-11-10)

### 指令清单分页

- `activity list` 默认只获取 `--page` 指定的一页（默认第 1 页），与 `app list` 一致；`--page-size` 控制每页数量。
- 新增 `--all`，从 `--page` 开始自动翻页，最多 100 页；输出增加 `all`，`complete=false` 表示仍有后续页或达到全量上限。
- **迁移说明**：原先依赖默认全量拉取的调用需改为 `xbot-cli activity list --all`。
- 更新帮助、schema 示例和分页文档，覆盖默认单页、指定页和显式全量请求。

## [0.10.1](https://github.com/wn0x00/xbot-cli/compare/v0.10.0...v0.10.1) (2026-11-10)

### 指令清单输出

- `activity list` 与 `app list` 复用相同的精简行和四列表格，每项仅保留 `appId`、`appName`、`appType`、`updateTime`。
- **迁移说明**：JSON 清单字段由 `data.instructions` 改为 `data.activity`，不再返回 `data.instructions`；读取清单的程序需改用 `data.activity`。
- 同步更新指令类型不匹配时的恢复提示和 CLI 文档。

## [0.10.0](https://github.com/wn0x00/xbot-cli/compare/v0.9.3...v0.10.0) (2026-11-10)

### 指令管理

- 新增 `xbot-cli activity list`，通过开放平台 `app.list` 查询并筛选 `appType=activity`，支持名称、所有者、页码和页大小参数。
- 清单自动有界翻页，最多 100 页，返回 `instructions`、`count` 和 `complete`，避免只返回第一页中的指令。
- 未配置开放平台凭据时返回 `PLATFORM_AUTH` 和明确的配置引导。
- 新增 `xbot-cli activity open`，通过本机客户端打开指令并校验 `robot_type=activity`，防止误开普通应用。
- 补充命令 schema、帮助、迁移文档和真实指令清单及打开验证；`flow list/get` 继续复用现有流程源码读取能力。

## [0.9.3](https://github.com/wn0x00/xbot-cli/compare/v0.9.2...v0.9.3) (2026-11-05)

### 流程参数能力增强

- 新增 `xbot-cli flow params types`，直接读取本机影刀客户端 DLL 中的类型资源，不调用客户端 MCP，支持表格与 JSON 输出。
- 参数写入改用 CLI 本地元数据适配器，不再依赖原生 MCP 的 `flow.write_parameters` 接口，避免该接口仅支持 6 种类型、拒绝其他类型写入的限制。写入后仍通过 MCP 执行 reload 和回读确认。
- 扩展 `flow params set`，支持列表、字典、网页、Excel、窗口等真实类型，不再需要统一声明为 `any`；支持 CodeFlow、VisualFlow 的 In/Out 参数批量新增、更新和删除。
- 参数修改后自动调用 reload，并回读确认参数契约；无实际变化时不写入、不 reload。
- 完善默认值处理：同类型、同方向更新保留未指定字段，类型或方向变化时重置不兼容初值。

### 安全与修复

- 增加应用身份、编辑模式、运行状态、并发修改及磁盘与内存一致性检查。
- 整批操作校验通过后再写入，仅修改目标流程的参数区域。
- 修复未初始化 Out 参数被无意义改写的问题。
- 失败结果保留写入、reload 和回读阶段状态，避免盲目重复执行。

### 文档与 Skill

- 新增流程参数使用文档及离线指南：`xbot-cli guide flow-parameters`。
- 更新应用开发 Skill 的参数类型、默认值和故障处理说明。
- 补充开发规范：业务函数在关键阶段前适当添加中文业务注释，不加序号、分割线或装饰性标题，不逐行复述代码。

### 验证与限制

- 已在影刀 6.3.39 子流程验证 62 种类型、124 项 In/Out 参数的写入、回读及默认值传递。
- 对象类型仅验证类型契约与空值占位，不代表真实对象的业务操作已验证；其他客户端版本与动态扩展类型仍需单独验证。
- CodeFlow 参数元数据与 Python 文件不被改写由离线测试覆盖，真实 CodeFlow 运行不在本次验证范围内。

## [0.9.2](https://github.com/wn0x00/xbot-cli/compare/v0.9.1...v0.9.2) (2026-11-05)

### 新增浏览器管理

- 新增 `xbot-cli web browser list`，通过已安装影刀 DLL 的公开接口查询浏览器插件状态和自定义浏览器注册信息，以与 `app list` 一致的表格展示；JSON 保留程序路径、管道标识等完整字段。
- 默认列出已安装插件或已注册的浏览器，始终排除 `exePath` 为空的项；`--all` 可额外查看有程序路径的未安装项。`installed` 是 DLL 判定，`registered` 只代表自定义浏览器已注册，不代表插件已启用或已连接。
- Web 操作新增 `--browser <id>`，支持按列表中的 ID 选择 Chrome、Edge 或有兼容管道的自定义浏览器；页面会话、快照和 ref 按浏览器隔离，显式选择失败时不再悄悄回退其他浏览器。
- `web open --browser <id>` 在指定浏览器未运行时，使用 DLL 返回的有效 `exePath` 启动浏览器，等待最多 20 秒连接其影刀插件，再打开目标 URL。已运行的程序不重复启动，失败不切换浏览器、不重放网页动作。
- 浏览器进程不继承 CLI 的输出句柄，避免自动启动后 PowerShell 或捕获输出的调用方一直等待。

```powershell
xbot-cli web browser list
xbot-cli web open yingdao.com --browser chrome
xbot-cli web snapshot --browser chrome
# e7 需替换为该浏览器最新快照中的实际 ref
xbot-cli web click e7 --browser chrome
```

### 新增指令集、应用开发规范 Skill

- 内置 `xbot-development-standards`，完整包含 13 个文件：规范入口、UI 元数据、应用与指令集参考资料、只读元数据检查器、52 项回归测试及验收/策略模板。
- `xbot-cli install --skills` 同时安装 `xbot-app-dev` 和 `xbot-development-standards`，支持当前工作区或 `--global` 用户目录；全部文件写入前统一检查冲突，覆盖仍需显式 `--yes`。
- 新增离线指南 `xbot-cli guide development-standards`。JSON 结果新增 `data.skills` 返回各 Skill 的 `name/path/files`，原 `data.path/files` 保持兼容。
- 明确普通应用与指令集的不同约定：普通应用由主流程进入主运行模块的 `main(args)`，不新增测试入口、测试函数或测试流程；正式默认值在流程参数中配置。指令集另行维护公开契约、中文面板、分组、启用状态和兼容性。
- 更新原有应用开发 Skill 的引用、CodeFlow 示例及流程组织说明，避免测试入口和代码默认值示例与新规范冲突。

```powershell
xbot-cli install --skills --json
xbot-cli guide development-standards
```

### 兼容与说明

- 未指定 `--browser` 时沿用原默认上下文；原 `browser` 别名及 `web open --mode` 保持兼容。`--dry-run` 仍离线预览，不启动浏览器。
- 自动启动仅用于显式指定浏览器的 `web open`；显式设置 `XBOT_WEB_PIPE`、使用 `cef` 或其他网页动作时不自动启动。选择浏览器后请重新采集快照，不跨浏览器复用 ref。
- DLL 查询适配器依赖影刀自带的 .NET 6 Windows Desktop 运行时，已在影刀 6.3.39 验证，不是稳定 SDK；升级影刀后可能需要重新适配。Firefox 的插件状态可查询，但目前不能通过 Web 管道操作。
- 附件提供 Windows x64 ZIP、同版 npm `.tgz` 和 `SHA256SUMS.txt`。完整中文说明与验证范围见 [0.9.2 更新说明](https://github.com/wn0x00/xbot-cli/blob/v0.9.2/docs/updates/2026-11-05-browser-development-standards.md)。

## [0.9.1](https://github.com/wn0x00/xbot-cli/compare/v0.9.0...v0.9.1) (2026-10-30)

### Added

- Return ShadowBot Python alongside supported web action results and snapshots. Ordinary DOM actions use semantic XPath; iframe actions use ordered `Selector.xpath.segments` with `web_page.find`. Text output is headed `影刀 Python：`; JSON exposes `data.python`, including dependencies, locator evidence, warnings and validation status. Use `--no-python` to disable generation independently of `--no-snapshot`.
- Add `xbot-cli web xpath <ref...>`: return up to five single-element alternatives, or shared similar-element rules for multiple refs. Reject incompatible element types or contexts; retain match counts and review warnings for ambiguous candidates.
- Generate ordered iframe XPath lists and open-ShadowRoot proposals, labeled as ShadowBot syntax requiring validation in ShadowBot. Explicitly refuse XPath and equivalent Python when the target or an outer context crosses a closed ShadowRoot.
- Preserve the complete snapshot element tree with clearer branch layout, refs, names and states. Filtering and pagination are explicit rather than silently hiding nodes.

### Fixed

- Keep refs bound to the selected browser tab across CLI invocations, including short-lived npm launchers; changing the active tab no longer silently redirects ref actions.
- Discover non-default ShadowBot installations from the running client, registry and known directories. Report the actual backend path, discovery source and targeted repair advice instead of blindly using the default path or only suggesting startup/login.
- Preserve successful web actions when code generation or the follow-up snapshot fails; do not replay an action to recover generated output.

### Upgrade notes

- Reattach with `web active` or open a page before using fresh refs. Browser bindings now live in `%LOCALAPPDATA%\xbot-cli\web-sessions`; independent-preview bindings are not imported.
- Generated code and candidates are proposals, not completed ShadowBot runtime verification (`runtime_verified: false`). iframe lists are Selector segments, not a single XPath for `find_by_xpath`; open ShadowRoot expressions must be tested in ShadowBot. Closed ShadowRoot elements cannot be used.
- Page-keyboard and arbitrary-script operations do not yet emit equivalent Python. Some pure open-ShadowRoot operations use explicitly labeled JavaScript synthetic-event adapters rather than trusted native input.
- Explicit `XBOT_SHELL_CLI` / `XBOT_LAUNCHER` overrides remain authoritative; an invalid override is reported instead of silently falling back.
- The bundled Skill remains v10 with updated Web and diagnostic guidance. See the [Chinese release notes](docs/updates/2026-10-30-web-python-xpath.md) for examples and verification scope.

## [0.9.0](https://github.com/wn0x00/xbot-cli/compare/v0.8.3...v0.9.0) (2026-10-29)

- Replace browser-use-mcp with Chrome Bridge named-pipe transport for all Playwright-style web commands, including navigation, element interaction, snapshots, and read-only evaluation.
- Add CDP trusted input, ARIA refs, on-demand browser engine initialization, multi-browser pipe discovery, bounded reads, and `XBOT_WEB_PIPE` selection.
- Preserve action/snapshot recovery contracts and structured errors, and add isolated named-pipe contract tests alongside real Chrome and ShadowBot Browser verification.
- Update browser documentation and Windows dependencies; Studio MCP behavior is unchanged.

### Upgrade notes

- Browser automation now requires Chrome, Edge, or ShadowBot Browser with a compatible ShadowBot extension and named pipe. Set `XBOT_WEB_PIPE` to select a pipe explicitly; pipe prefixes are not browser product identities.
- The legacy `--name` option is accepted for compatibility but no longer stores selectors in the element library.
- Raw browser compatibility is limited to `web_create`, `web_get_active`, `web_snapshot`, `web_action`, and `web_evaluate_playwright` implemented by Chrome Bridge.

## [0.8.3](https://github.com/wn0x00/xbot-cli/compare/v0.8.2...v0.8.3) (2026-10-23)

- Reorder root help: business commands (app, flow, task, web, win32, tool, wiki, oapi) come first, followed by discovery, diagnostics and self-management (schema, guide, install, status, doctor, version, update, help).
- Simplify the `oapi` summary line to describe the Open API surface: auth, apps, robots, dispatch and logs.

## [0.8.2](https://github.com/wn0x00/xbot-cli/compare/v0.8.1...v0.8.2) (2026-10-23)

- Keep only the `xbot-cli` entry point; the `xbot` shim is no longer created. A stray older native program earlier in PATH (for example a manually copied `xbot-cli.exe`) used to shadow the npm shim and diverge command output between `xbot` and `xbot-cli`.
- Add a `preinstall` cleanup that removes legacy `xbot` shims belonging to this package and quarantines conflicting `xbot-cli` programs outside the npm bin directory to `.bak`, so the command always resolves to the current install. Set `XBOT_CLI_SKIP_CLEANUP=1` to opt out.
- Documentation and built-in examples now consistently use `xbot-cli`.

## [0.8.1](https://github.com/wn0x00/xbot-cli/compare/v0.7.6...v0.8.1) (2026-10-23)

- Add `xbot-cli update`: the npm launcher checks the registry for the latest release and runs the global npm upgrade before the native process starts, since Windows cannot replace a running executable. `xbot-cli update --check` only reports whether a newer version exists.
- The native `update` subcommand keeps the command discoverable in help and schema; invoked on the bare executable it returns `UPDATE_REQUIRES_LAUNCHER` pointing at the npm entry point. The update notifier now recommends `xbot-cli update`.
- Rename the enterprise entry point to `xbot-cli oapi` (`platform` remains a compatibility alias). One-line login `xbot-cli oapi auth <AccessKeyId> <AccessKeySecret>` verifies and stores credentials locally; the token is cached, reused until expiry and refreshed automatically with the stored keys. `oapi auth logout` clears the current authorization; `XBOT_OAPI_*` environment variables take precedence and legacy `XBOT_PLATFORM_*` names keep working.
- Wrap the full Open API with friendly commands: `robot list/groups/detail`, `app list/detail/versions/flows/transfer/records`, `job start/stop/retry/list/status/logs`, `task start/stop`, `user list/create/modify/delete/reset-password` and the complete `queue` group. All 44 registered endpoints remain reachable through `oapi api call`; mutating operations require `--yes` and support offline `--dry-run` previews.

## [0.7.6](https://github.com/wn0x00/xbot-cli/compare/v0.7.5...v0.7.6) (2026-10-23)

- Fix command routing so application runs and diagnostics continue to use the existing client CLI by default. Enterprise `platform` API operations require explicit selection and never become an automatic fallback.
- Update help, schema, README and the bundled Agent Skill to describe `platform` as an optional enterprise entry point; add regression coverage for default routing.
- Deprecate 0.7.5 because of an issue in the newly added Open Platform API execution logic. Upgrade to 0.7.6.

## [0.7.5](https://github.com/wn0x00/xbot-cli/compare/v0.7.4...v0.7.5) (2026-10-23)

> Deprecated: the newly added Open Platform API execution logic had an issue. Upgrade to 0.7.6.

- Add the `platform` command group for the official enterprise Open API, independent of the local ShadowBot client. Bundle 68 documentation references and 44 endpoint contracts covering authentication, accounts, applications, robots, dispatch, logs, queues, files and run records.
- Add schedule history, task results, job status and paginated log queries for investigating dispatch failures. Support synchronous log search and bounded notify-and-poll queries that retain the request ID on timeout.
- Support environment-based API keys or access tokens and private-cloud HTTPS origins. Redact credentials and password fields, encode query/path parameters, preserve server error codes and pagination, and require confirmation for mutating endpoints.
- Add offline API discovery and schemas, a reusable platform troubleshooting guide, seven-day validation for run-record queries and multipart file uploads.
- Verify authentication, dispatch filters, cursor precision, log polling, timeout recovery, redaction, upload encoding and installed package resources with local HTTP fixtures. Live enterprise API permissions and production logs remain unverified.

## [0.7.4](https://github.com/wn0x00/xbot-cli/compare/v0.7.3...v0.7.4) (2026-10-22)

- Add Skill v10 Web and Windows modules, with Agent routing based on the failed call and its actual target. Embed and install all 38 Skill resources and expose the corresponding offline guides.
- Add ref-to-XPath generation using the weighted DOM generator and a hash-pinned, externally supplied ROBULA+ source. Verify that generated paths identify the original node; support segmented iframe/open Shadow paths within the documented limits.
- Add workflow-usage analysis and read-only repair proposals for single elements. Add ordinary-DOM collection contracts, dependent child locator checks, dense-page candidate screening and compatible old/new locator variants.
- Add an experimental Python UIA single-control repair helper and `win32 snapshot --hwnd --pid`. Keep native observation separate from ShadowBot selectors; preserve original assets and reject ambiguity, incomplete observation and unproven identities.
- Document real test results and remaining limits, including same-content node replacement, Windows collection repair and uncertain WeChat send results. These helpers are not an unattended repair service.
- Extend offline helper regressions and package verification to cover the complete installed Skill, current embedded guides and the Windows snapshot discovery contract. ROBULA+ source, UIA dependencies and business runtime data are not bundled.

## [0.7.3](https://github.com/wn0x00/xbot-cli/compare/v0.7.2...v0.7.3) (2026-10-21)

- The npm launcher now reminds interactive users when a newer release is available, printing a one-line upgrade hint to stderr without touching stdout or JSON output.
- The registry check runs at most once every 24 hours with a local cache, reminds at most once per new version, times out silently after one second, and stays off in CI. Set `XBOT_NO_UPDATE_NOTIFIER=1` to disable it completely.

## [0.7.2](https://github.com/wn0x00/xbot-cli/compare/v0.7.1...v0.7.2) (2026-10-20)

- Ship Agent Skill v9 with exploration, development, acceptance and maintenance modes; add task contracts, locator records, business evidence, action risk guidance, bounded waits and a minimal repair/regression workflow.
- Prefer verified XPath in VisualFlow and CodeFlow. Document element-library naming, configuration/business/test flow organization, English snake_case code variables, and compatibility checks for old and new elements that coexist.
- Install all 14 Skill resources, including the new references and descriptive templates. Verify complete content, isolated local/global installs, repeat installation and conflict-before-write behavior; validate the same resources through the packaged Windows executable.
- Browser automation still uses the shared active-page context; this release does not add multi-session isolation or an automatic template executor.

## [0.7.1](https://github.com/wn0x00/xbot-cli/compare/v0.7.0...v0.7.1) (2026-10-19)

- Show the actual backend error when a browser action fails; selector-storage messages no longer hide fill failures. Keep the complete response and call log in JSON while showing a concise error and snapshot guidance in text.
- Add offline `xbot version` and `xbot-cli version` commands, with the same JSON result as `--version`.

## [0.7.0](https://github.com/wn0x00/xbot-cli/compare/v0.6.1...v0.7.0) (2026-10-19)

- Add the `xbot` command and a `web` interface with direct element actions, keyboard input and navigation backed by ShadowBot; retain `xbot-cli` and `browser` aliases.
- Show page snapshots after successful web actions, preserve action success when a snapshot fails, and allow saving snapshot text without overwriting files.
- Fix browser commands reporting `MCP_BAD_RESPONSE` after a successful action when the service returns a numeric JSON-RPC ID such as `1.0` instead of `1`. JSON and SSE responses accept equivalent numeric IDs while rejecting unrelated or string IDs.
- Publish from manually pushed version tags, with a helper to synchronize npm and Rust versions.

### Upgrade notes

- Install `@guanzhu.me/xbot-cli@0.7.0` to receive these changes; `0.6.1` does not contain the response-ID fix or the `xbot web` interface.
- Existing `xbot-cli browser` command names remain supported. Open and navigation URLs now accept HTTP(S), bare domains (normalized to HTTPS), and `about:blank`; other schemes are rejected locally.
- High-level open, attach, navigation and element/keyboard commands fetch a snapshot after success. Use `--no-snapshot` when only the action result is needed; raw tool calls keep their existing behavior.

## [0.6.1](https://github.com/wn0x00/xbot-cli/compare/v0.6.0...v0.6.1) (2026-10-19)


### Bug Fixes

* **release:** automate trusted npm and GitHub publishing ([bd12480](https://github.com/wn0x00/xbot-cli/commit/bd12480b7d1325ca8658c2cf42735a790a40b98c))
