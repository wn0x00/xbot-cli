# CLI 操作参考

目录：[应用与源码](#flows) · [In/Out](#parameters) · [运行与恢复](#runs) · [Web](#web) · [Windows](#windows) · [MCP](#mcp) · [依赖与企业 API](#environment)。示例是独立操作，按任务选择执行，参数以现场 schema 为准。sync 见 [主文件](../SKILL.md#sync)。

PowerShell 中 `@file` 必须作为字符串传递，例如 `'@params.json'`、`'@C:\任务目录\before.txt'`；裸写 `@params.json` 会被按 splatting 语法解析并报错。先替换 `<app-id>` 等示意占位符，再执行选中的命令，不能整段无条件粘贴运行。

命令形状于 2026-11-25 用本机 CLI 0.11.5 离线 schema 核对；2026-11-26 再核对 version、app sync、flow params set、python package list，版本仍为 0.11.5，sync 的 confirmation 为 none。此记录不表示客户端运行验收，也不保证其他版本相同。下面 legacy/copilot 差异是适配指引，现场先发现实际 provider，未知版本不凭版本号套用磁盘字段。固定上限、状态和字段有差异时以现场 schema/有效样本为准，在本任务清单记录“CLI/客户端/provider → 目标命令或字段 → schema/样本来源 → 实际回读结果”，无需另造版本说明文件。

<a id="flows"></a>
## 应用、流程与源码

```powershell
xbot-cli status --json
xbot-cli app list --json
xbot-cli activity --json
xbot-cli app open <app-id> --json
xbot-cli flow list --expect-app <app-id> --json
xbot-cli flow get <flow-id> --expect-app <app-id> --json
xbot-cli flow create <flow-name> --kind code --expect-app <app-id> --json
xbot-cli flow source read <flow-id> --offset 1 --limit 200 --expect-app <app-id> --json
xbot-cli flow code edit <flow-id> --old '@before.txt' --new '@after.txt' --expect-app <app-id> --dry-run --json
xbot-cli flow code edit <flow-id> --old '@before.txt' --new '@after.txt' --expect-app <app-id> --json
```

打开/切换前保护当前草稿，只读查看按现场 `app view` 使用，不能在 View 保存开发修改。记录创建返回的 flow_id。已有源码先完整读取受影响范围、备份原文，写前再回读；优先唯一精确替换，新文件或确需整份替换再用 `flow code write ... --file entry.py`。不把分页省略标记写回源码。

| provider | CodeFlow | VisualFlow |
| --- | --- | --- |
| legacy | `flow code read/write/edit` 或支持的 source 入口 | `flow visual list/edit/fill/enable/disable`，按真实 block_id 与最新 form 操作 |
| copilot | `flow source read/write/edit`，code 入口兼容 | 同样走 source；写入客户端支持的 `xbot_visual.*` Flow body |

copilot source 默认 200 行、上限 2000 行，另受 40 KiB 预算约束，检查 returned_lines/total_lines。offset 从 1 开始；`--block-nos 10:20,25` 指可视化指令编号，最多 100 个，不能和 offset/limit 同用，也不是 legacy block_id。

copilot VisualFlow 仅提交 body，Studio 提供 main、脚手架及已声明 In；普通 Python 业务放 CodeFlow。精确编辑保留 `_block` 等客户端注解，以编译后已持久化源码为下次基线。写入成功但 ready_to_run/诊断未完成时只补检查，不重复写。

legacy 编辑后回读块与 form；`fill_errors` 只修失败 block_id，不能整批重放已应用修改。具体插入/删除/填字段格式查叶命令 schema，不跨版本猜块结构。

<a id="parameters"></a>
## 流程 In/Out

CodeFlow 与 VisualFlow 使用同一命令：

```powershell
xbot-cli flow params types --json
xbot-cli flow params list <flow-id> --expect-app <app-id> --json
xbot-cli flow params set <flow-id> --operations '@params.json' --expect-app <app-id> --dry-run --json
xbot-cli flow params set <flow-id> --operations '@params.json' --expect-app <app-id> --json
xbot-cli flow params list <flow-id> --expect-app <app-id> --json
```

`params.json` 可为数组或 `{"operations":[...]}`；stdin 用 `--operations -`。

```json
[
  {"op":"upsert","name":"keyword","direction":"In","type":"str","kind":"Text","value":"","description":"检索关键词"},
  {"op":"upsert","name":"records","direction":"Out","type":"list","kind":"Expression","value":"[]","description":"记录列表"},
  {"op":"remove","name":"obsolete_input"}
]
```

- upsert 的 direction/type 必填；新增 name 为合法 Python 标识符。type 使用本机 types 返回的精确 ID，不把对象类型改成 any；目录是静态资源，不保证所有编辑器都可选。没有 password 参数类型。
- kind 为 Text/Expression，建议显式给出；value 是文本或 Python 表达式源码的**字符串**，不是 JSON 原生数组或布尔值。省略值、空串、`None` 与对象创建分别处理，不为获得默认值执行表达式。
- 同类型同方向更新保留省略字段；改类型/方向的默认行为按实际后端。正式默认值见 [开发规范](../../xbot-activity-dev/SKILL.md#codeflow)，不要把示例变成正式兜底值。
- copilot 主流程 In 仅支持 int/float/str/bool/file；子流程按客户端注册类型。新 str/file 默认 Text，表达式省略初值为 `None`；需要 `0/False/[]` 时显式写 kind/value。
- copilot 连续不重名 upsert 合并请求，remove 或重复名称分批顺序执行；无跨请求事务。读取 completed_count/results/cause，回读现状后只补未完成项。客户端原生保存，CLI 不附加 legacy 磁盘/reload 流程。
- legacy 适配要求目标流程已本地保存、无运行任务，操作期间不并发编辑；CLI 完成保存、本地元数据适配、reload 与回读，**此参数命令不调用 sync 关闭设计器**。失败按阶段和回读恢复，不手工重放整批。省略值的详细旧版规则直接查 `guide flow-parameters`。
- dry-run 不验证实时类型目录、表达式、权限或客户端接受性。参数声明存在也不证明原入口实际消费了该值。

<a id="runs"></a>
## 运行、输出与恢复

```powershell
xbot-cli flow check <flow-id> --expect-app <app-id> --json
# 仅在任务要求真实运行时执行 verify。
xbot-cli flow verify <flow-id> --expect-app <app-id> --timeout 600 --json
xbot-cli flow wait <run-id> --expect-app <app-id> --timeout 600 --json
xbot-cli help task run
xbot-cli schema task run
```

check 传 flow_id 也可能返回 `scope:app` 静态诊断；verify 会执行。编辑版走 flow，已发布版走 task；先查 task run/status/wait/logs 的实际参数并保留 task_id，不混用 Studio run_id。普通应用从已有主流程运行。

成功 JSON 为 `ok:true,data`；失败读 `ok:false,error` 以及 partial data，error 常含 type/code/message/hint/retryable。默认终端文本、管道 JSON，解析显式 `--json`。退出码：0 命令成功；1 操作失败；2 参数；3 认证/权限/配置；4 网络/超时；5 上下文冲突；6 协议/内部；10 CLI 确认门槛，已有具体授权才加 `--yes`。命令成功、运行终态、Out 和业务结果分别判断。

| 症状 | 处理 |
| --- | --- |
| 超时/断连/结果 unknown | 查原 run_id/task_id、日志和业务回执，不新启运行 |
| 写入成功，诊断或快照失败 | 只补观测/检查，保留 applied、已完成前缀 |
| `FLOW_STATIC_ERRORS` | 处理 static_errors；该阶段未运行 |
| `FLOW_FILL_FAILED` | 只处理 fill_errors，先回读已写项 |
| 暂停、停止或取消 | 记录终态/现场，按当前任务范围决定后续 |
| `VIEW_MODE_READONLY` / `studio_already_open` | 确认正确工程与草稿，按正常关闭/open 处理，不直接 force 抢占 |
| `MCP_OFFLINE` / `SHELL_CLI_MISSING` | 查客户端、登录、实际安装路径和通道；不换通道重放已发出的动作 |

copilot 的 running/stop_requested 仍在运行，completed 为终态成功，failed 保留堆栈，canceled 为取消；矛盾证据不算成功，legacy 仍按 canonical run_succeeded。copilot 仅保留当前一次 Studio 运行，启动下一次前保存日志；`FLOW_RUN_MISMATCH` 表示记录已替换，停止等待旧 ID，也不能重启原流程补证据。

<a id="web"></a>
## Web 浏览器操作

```powershell
xbot-cli web browser list --json
xbot-cli web active --browser edge --json
xbot-cli web snapshot --browser edge --json
# 实际 ref 来自该页面当前快照。
xbot-cli web fill <ref> '检索词' --browser edge --json
xbot-cli web press Enter --ref <ref> --browser edge --json
xbot-cli web xpath <ref> --browser edge --json
```

`web` 通过 Chrome Bridge/CDP，不调用 browser-use-mcp，不要求 Studio。浏览器 ID 取自 list；installed/registered/webSupported 不证明扩展在线，Firefox 可查询但当前不能操作。显式 `--browser` 每条都传，不自动沿用上一条选择，也不离线回退另一浏览器。

open/active 绑定管道与标签，用户切换活动标签不会自动换页；active 显式重绑，goto 在受控页跳转。`XBOT_WEB_SESSION` 隔离脚本绑定，不创建独立 profile；`XBOT_WEB_PIPE/TAB_ID` 可指定上下文，绑定过期后重绑。显式 pipe 与 browser 冲突会拒绝。

仅 `web open --browser <id>` 可在目标未运行时按发现的绝对 exePath 启动一次，最多等 20 秒扩展连接；不指定 browser、显式 pipe、cef 及其他动作不自动启动。失败不换浏览器、不重放动作。

常用命令有 open/active/snapshot、click/dblclick/hover、fill/type/press、check/uncheck/select、goto/reload/go-back/go-forward/close。fill 清空后填写，type 向焦点输入，select 使用 option value。日期、上传、下载等专用能力查 `tool browser list/schema/call`。当前没有通用 tabs/storage/trace 或独立 screenshot 命令。

ref 是当前快照编号，不接受 CSS/XPath 或其他页面编号；页面变化用新快照，快照可能是增量。动作后自动快照失败可能仍返回 `ok:true,applied:true,warning`，只补 snapshot；selector 暂存提示不代表填写成功。快照 `--output` 不覆盖文件；role/filter/limit/offset 是部分显示，raw 查看原始文本。

支持的动作可返回 `data.python`；`runtime_verified:false` 表示未在影刀执行验证。生成失败不代表已执行动作失败。`--no-python` 与 `--no-snapshot` 独立；兼容 `--name` 不会持久写入元素库。

只读求值示例：`web eval "return await activePage.title();" --json`。这是注入 activePage 的异步函数体，返回可序列化值；DOM 读取放 `activePage.evaluate(...)`，顶层没有 document。不用 eval 代替交互或提交工具。

XPath 来自当前 DOM 并验证身份、作用域和唯一性。`web xpath` 单 ref 最多 5 组备选，多 ref 得到共同规则，不能证明集合完整。单目标多匹配应缩小业务区域，不取 `[1]`。iframe 输出分段数组交给 `Selector.xpath.segments`，不能直接传 find_by_xpath；open ShadowRoot 使用 `/xbotShadowRoot` 影刀语法，需影刀实测。目标或祖先实际经过 closed ShadowRoot 时不生成等价 XPath/Python；`shadowRoot===null` 本身不能证明 closed。

长期网页指令使用当前 SDK 已核实的 XPath/Selector API；失效路径、历史身份、单元素/集合和复杂上下文修复，按 [Web 修复规程](../../xbot-activity-fix/references/web-repair.md)。

<a id="windows"></a>
## Windows 命令

```powershell
xbot-cli win32 list --title '目标软件' --json
xbot-cli win32 snapshot --hwnd <HWND> --pid <PID> --max-nodes 1000 --max-depth 12 --timeout 10 --json
xbot-cli win32 inspect <ref> --snapshot <snapshot-id> --json
xbot-cli win32 click <ref> --snapshot <snapshot-id> --json
```

list 原生只读枚举，标题默认字面包含匹配，exact 完整匹配，all 含隐藏/无标题窗口；多匹配不取首项，重启后重查 PID/HWND。确需激活元素树时查 `win32 activate`，它会改变目标进程：auto 对支持的 x86 企微使用内置适配，其他按架构使用本机组件。未知结果不重复注入/换 provider，成功后仍须 snapshot 核对。

click 默认 physical，显式 `--method invoke` 才用 InvokePattern；缺 Pattern 或无效果不自动切换方式。fill 用 Value、type 输入文字、press 按键，check/select/expand/scroll 等依实际 Pattern。动作前确认窗口、前台、实时身份与遮挡。

ref 绑定进程实例、窗口、祖先链和 RuntimeId；缺身份拒绝动作。当前快照最长 24 小时，动作开始即标记 used，后续用返回的新 `data.snapshot`。`applied:true,effect_verified:false` 只说明调用返回，另验业务结果；动作后快照失败只补观测。动作/快照预算按 schema，当前为 1–60 秒。独立 Python UIA 资产不能替代 CLI snapshot；持久修复见 [Windows 专题](../../xbot-activity-fix/references/windows-repair.md)。

<a id="mcp"></a>
## MCP 兼容与能力发现

`client info` 分开读取 cli.version、运行 client.version/processes、shell_cli.backend、studio_mcp provider/endpoint；后端目录可能被覆盖，不代表运行客户端版本。外层 ok 不代表各通道成功，检查 partial/detection_complete。多实例或不可读路径保留未知。

legacy 常用 `/api/v1/mcp`，copilot 常用 `/api/v1/copilot-mcp/mcp`；按实际接口发现分流。copilot 提供 xbot-bash/execute_cell，旧 wiki/块 JSON/按 block_id 运行快照等没有完全对应接口，缺能力返回 `MCP_CAPABILITY_UNAVAILABLE`。

```powershell
xbot-cli tool studio list --json
xbot-cli tool studio schema xbot-bash --json
xbot-cli tool studio call xbot-bash '@command.json' --expect-app <app-id> --json
```

只读 `command.json`：`{"command":"xbot-bash --help","explanation":"查看客户端命令"}`。继续通过该接口查 api get、run variables、run start 的 help；精确 API 可查 `api get --names`。xbot-bash 文本不交给 PowerShell/cmd 执行。运行输入仅在现场 `run start --help` 确认支持 --args 后使用。

execute_cell 需要客户端绑定的工作区/探索会话，列出不等于可运行；通用端点 session_unavailable 时核对客户端实际绑定地址，不能猜 workspace，也不假定跨 CLI 进程共享状态。常规编辑/运行不依赖它，非文本内容保留在 mcp_content。

`XBOT_MCP_MODE=auto` 只在标准旧路径 404 时尝试同服务新路径，认证/503/断连不切换；legacy/copilot 显式选协议。`XBOT_MCP_ENDPOINT` 自定义路径不自动改写；会话仅单 CLI 进程有效，过期后当前写入/运行不重试。`XBOT_SHELL_CLI` 只选后端文件，不改变运行客户端身份。配置不能绕过目标断言和任务范围。

<a id="environment"></a>
## 依赖、安装与企业 API

应用依赖用 `python package list/install/uninstall`，先查具体 schema，用 expect-app 和实际应用引擎核对；固定版本用 `NAME==VERSION`。包安装到应用环境，不用系统 pip 代替。安装/卸载超时先回读，不自动重试；源和约束按当前能力填写。

```powershell
xbot-cli python package list --expect-app <app-id> --json
xbot-cli python package install 'openpyxl==3.1.5' --expect-app <app-id> --dry-run --json
xbot-cli python package install 'openpyxl==3.1.5' --expect-app <app-id> --json
xbot-cli python package uninstall openpyxl --expect-app <app-id> --json
```

`list` 表示包可用，不等于已声明为直接依赖；发行包名与 import 名可能不同。安装/卸载后回读应用依赖，安装源按当前 `--index-url` 契约填写。`--timeout` 只限制等待，不会停止客户端安装；View 模式拒绝修改，也不切换系统 pip 或另一协议重放。

缺 CLI 且任务要求安装时，当前分发包为 `@guanzhu.me/xbot-cli`，面向 Windows x64、Node.js 22+；已有安装不无理由重装或改 PATH。`install --skills` 安装当前 CLI 内置的 `xbot-cli-doc`、`xbot-activity-dev`、`xbot-activity-fix` 三份 Skill；旧 CLI 的内置清单可能仍是两份，先核对版本与返回的 `data.skills`。新安装器会把原两份已安装 Skill 整目录移到同级 `.agents/skill-backups/`，保留定制与附加文件。冲突在迁移前统一检查，`--dry-run` 不迁移或写入。

企业 `oapi` 是用户明确选择的独立通道，不因客户端离线自动切换。先查 `oapi auth` 和目标叶命令 schema；密钥经受支持 stdin 输入，本地按 DPAPI 存储，不写报告。机器人、scheduleUuid/taskUuid/jobUuid 与本机任务标识分开；raw endpoint 先确认 schema/语义，不能把调度创建当作业务已运行。
