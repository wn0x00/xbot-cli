# 影刀 6.3 / 6.4 MCP 兼容

先执行 `xbot-cli client info --json`，查看 `client.version`、`client.processes`、`shell_cli.backend` 及 `studio_mcp.connection.provider` 和实际端点；连接故障再运行 `doctor --json`。
当前支持旧版 Studio MCP 和 6.4 Copilot MCP；运行时以实际接口发现结果分流，不仅凭安装目录版本号。
`cli.version` 是 xbot-cli 版本；`client.version` 优先取运行进程的产品文件版本，目录推断会明确标注。后端文件可被环境变量覆盖，不能用其所在目录反推运行版本。查询外层成功不代表所有通道正常，须读取 `partial` 和各通道 `ok/error`。不自动升级或强制切换客户端。

| 项目 | 旧版 Studio MCP | 6.4 Copilot MCP |
| --- | --- | --- |
| 默认路径 | `/api/v1/mcp` | `/api/v1/copilot-mcp/mcp` |
| 工具 | `app.get_info`、`codeflow.*`、`flow.*` 等 | `xbot-bash`、`execute_cell` |
| 连接 | 原调用方式 | initialize、initialized 通知、会话与协议请求头 |
| CodeFlow | 保留 `flow code read/write/edit` | 同样可用，由兼容层转换 |
| VisualFlow | `flow visual list/edit/fill/enable/disable` | `flow source read/write/edit` |
| 参数修改 | 本地元数据适配、保存/reload/回读 | 原生 `flow parameter set/remove` |
| Python 依赖 | `python package` 映射到 `python_pip` | 同一入口映射到原生 `python package` |

## 常用命令

```powershell
xbot-cli client info --json
xbot-cli doctor --json
xbot-cli flow list --expect-app <app-id> --json
xbot-cli flow source read main --offset 1 --limit 200 --expect-app <app-id>
xbot-cli flow source read main --block-nos 10:20,25 --expect-app <app-id>
xbot-cli flow source edit main --old @before.txt --new @after.txt --expect-app <app-id>
xbot-cli flow source write main --file main.py --expect-app <app-id>
xbot-cli flow check main --expect-app <app-id>
```

`source` 在 6.4 支持 VisualFlow 和 CodeFlow，在旧版仅支持 CodeFlow。
原 `flow get/read` 入口保留；6.4 返回源码，旧版按流程类型返回块或代码。
新入口 read 的页大小由后端决定，6.4 默认 200 行、上限 2000 行；原 `flow code read` 在 6.4 显式请求 2000 行，以保留原默认值。
6.4 的源码页还受 40 KiB 预算限制，使用 `returned_lines/total_lines` 和截断提示判断完整性。
`--offset` 是从 1 开始的源码行号；`--block-nos` 是可视化指令编号，最多 100 个不同编号，不能和 offset/limit 同用，也不能拿旧 `block_id` 代替。

VisualFlow 写入的是客户端支持的 `xbot_visual.*` 流程源码表示；普通 Python 模块写到 CodeFlow。
新建或全量写入 VisualFlow 时只提交 Flow body，Studio 提供 `main(args)`、脚手架和已声明的 In 参数；body 直接使用参数名。普通业务 Python 放入 CodeFlow，再由可视化调用流程指令连接。
编辑前先读当前源码并保存原文，优先精确替换；`--old/--new` 支持字面文本、`@file` 或 `-`，一次仅一个 stdin 消费者。
全量写入使用 `--file <path>` 或 `--file -`。
客户端编译可能改写 VisualFlow 源码，后续替换以返回的已持久化源码为准。
写入成功与 `ready_to_run` 分开判断；静态检查不可用时只补 `flow check`，不要重复写入。
读到截断区域再按行分页，不把省略标记当源码。

## 流程参数与运行

`flow params list/set` 继续接受原 CLI 契约；6.4 将 `kind` 转为原生 `value_kind`。
同类型、同方向更新时，CLI 先读取现有参数，保留省略的 kind/value/description。
新增或改变类型后的默认值由所选后端决定；6.4 未指定的表达式初值为 `None`，需要 `0`、`[]` 等时明确填写。
6.4 主流程 In 仅接受 int/float/str/bool/file，子流程使用客户端注册类型。
连续且名称不重复的 upsert 合为一次原生批量设置；remove 和后续批次按顺序执行。
混合操作没有跨命令事务，失败读取 `completed_count/results/cause`，先核对参数再补未完成部分，不能整批重放。
原生调用会保存客户端工作区；CLI 不额外执行旧版磁盘写入或 reload。

`flow run/wait/check/verify/logs/stop` 沿用原入口。超时、断连先查原 run_id；已完成则使用已有结果，不能重启。
6.4 的 `running` 和 `stop_requested` 仍属运行中，`completed` 映射成功，`failed` 保留运行错误与堆栈，`canceled` 按停止处理。未知或相互矛盾的完成证据不认定成功；旧版仍要求 canonical `run_succeeded`。
6.4 只保留当前一次 Studio 运行记录；启动下一次运行前保存所需输出和日志。旧 run_id 返回 `run_id_mismatch` 时，CLI 报 `FLOW_RUN_MISMATCH`，不再建议反复等待，也不重启原流程。
原生 `xbot-bash run start` 还提供仅对本次运行生效的输入参数，先查 `run start --help` 的 `--args`。
运行完成、Out 输出和业务结果分别核验。

## 动态工具和能力边界

应用依赖优先使用短命令 `python package list/install/uninstall`，不必拼 raw JSON；示例、源、版本及超时处理见 [依赖管理](python-packages.md)（`guide python-packages`）。

```powershell
xbot-cli tool studio list --json
xbot-cli tool studio schema xbot-bash --json
xbot-cli tool studio call xbot-bash @command.json --expect-app <app-id> --json
```

`command.json` 示例（只读）：

```json
{"command":"xbot-bash --help","explanation":"查看当前客户端支持的命令"}
```

通过同一路径调用 `xbot-bash flow create --help`、`api get --help`、`run variables --help` 等读取当前契约。
6.4 的命令目录由客户端提供，可能覆盖流程、参数、运行、API 定义、自定义指令、图像、资源、变量和 Python 包；以当前 `tool studio list/schema` 和 `xbot-bash --help` 为准，不假定固定命令数量。
也支持直接传递 `execute_cell`，参数以现场 schema 为准；返回的图像等非文本块在 `mcp_content` 中保留。
`execute_cell` 额外依赖客户端绑定的工作区与探索会话，通用端点可能返回 `session_unavailable` 和缺少 workspace root；工具列出不代表运行时就绪。需要此能力时，使用客户端生成的已绑定地址设置 `XBOT_MCP_ENDPOINT`，核对当前应用；CLI 不猜测工作区，也不保证不同 CLI 进程共享 Python 状态。常规源码编辑、参数修改及 Studio 流程运行不依赖探索运行时。
这两个工具属于客户端工具接口，`xbot-bash` 命令文本不交给 PowerShell/cmd 执行。

旧版块 JSON 编辑、静态作用域变量、按 block_id 的运行快照、wiki 接口在当前 6.4 工具目录没有相同接口。
CLI 对这些旧调用返回 `MCP_CAPABILITY_UNAVAILABLE`，不伪装成等价操作。
VisualFlow 改用源码入口；API/对象签名用 `xbot-bash api get --names <精确名称>`；运行值查 `run variables --help`。
不要把源码行号、指令编号、block_id 或不同 API 文档格式混用。
旧客户端的原命令保留；不存在的能力不能仅靠换端点补回。

## 显式配置及失败处理

- 默认 `XBOT_MCP_MODE=auto`：只读发现旧接口，标准旧路径返回 404 时才尝试同一服务的新路径。认证、503 或连接异常不切换后端。
- `XBOT_MCP_MODE=legacy`：强制原 MCP 方式；`copilot`：强制会话接口，标准旧路径会换为新路径。
- `XBOT_MCP_ENDPOINT`：显式指定服务地址；任意自定义路径的 404 不会被自动改写。已含 `/copilot-mcp/mcp` 的地址直接初始化。
- 会话仅在单次 CLI 进程内复用，不落盘。会话失效返回 `MCP_SESSION_EXPIRED`，当前写入/运行不重试；后续命令重新连接。
- 协商在实际操作前完成。已发出的工具调用失败时不自动重试、不跨新旧通道重放。
- 无 Studio 打开时，doctor 可报告通道正常、当前应用为空；需要工作区的命令仍应先 `app open`。

这些配置只选择协议，不改变任务授权、`--expect-app`、只读 View 或运行结果核验要求。
