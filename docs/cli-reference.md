# xbot-cli 命令与行为参考

面向 AI Agent 的影刀（ShadowBot）RPA CLI。默认通过内置 shell-cli、studio-mcp 和 Chrome Bridge 命名管道操作和运行应用；网页命令全部采用管道协议，不依赖 browser-use-mcp。`oapi` 仅是企业版用户明确选择时的开放 API 备用入口。实验性 Windows UIA 观察使用独立 Python 提供器，不是影刀原生 Selector 引擎。

命令按所操作的资源组织：

| 入口 | 职责 |
|---|---|
| `client info` | 查询运行中的客户端版本、进程/目录、后端配置、状态和实际 MCP 通道 |
| `status` / `doctor` | 查看当前状态、诊断连接 |
| `app` | 创建、打开、保存、同步、发布和回收应用 |
| `flow` | Studio 流程的读取、参数、编辑、静态检查与运行 |
| `flow code` / `flow visual` | CodeFlow 源码和 VisualFlow 块 |
| `python package list/install/uninstall` | 管理当前 Studio 应用的 Python 依赖，兼容新旧 MCP |
| `task` | 已发布应用的运行任务、状态、日志与停止 |
| `oapi` | 企业版可选备用入口：开放 API 运行、调度记录与日志 |
| `web`（兼容 `browser`） | 打开网页、快照、点击、填写和导航 |
| `win32 snapshot` | 读取 UIA 树并保存快照，供 ref 动作使用 |
| `win32 inspect/click/fill/...` | 按快照 ref 校验实时控件并执行 UIA 动作 |
| `win32 list` | 按窗口标题搜索，表格显示 PID、HWND 与进程名 |
| `win32 activate` | 自动使用企微内置适配或本机影刀组件，激活指定进程的元素树 |
| `tool studio` / `tool browser` | 按 schema 调用原始工具 |
| `wiki` / `schema` / `guide` | 领域知识、命令契约和开发指南 |

## 安装与 Agent 接入

Windows x64、Node.js 22+ 环境可通过 npm 包 `@guanzhu.me/xbot-cli` 安装预编译程序，无需安装 Rust；安装后以 `xbot-cli` 调用，安装前只会清理当前 npm 目录中属于本包的旧版 `xbot` shim，不修改 PATH 中的其他程序。安装命令和可复制的 Agent 指令见[项目 README](../README.md#安装)。

```powershell
xbot-cli install --skills --json           # 当前工作区 .agents/skills
xbot-cli install --skills --global --json  # 用户 ~/.agents/skills
```

一次安装 `xbot-cli-doc`、`xbot-activity-dev`、`xbot-activity-fix` 三份 Skill。全部文件写入前统一检查冲突；内容不同时先查看差异，明确覆盖时追加 `--yes`。JSON 的 `data.skills` 按 Skill 返回 `name/path/files`；`data.path/files` 对应第一份 `xbot-cli-doc`，依赖旧名称的调用方应按 `data.skills[].name` 选择。旧版两份已安装目录在冲突检查通过后整体移入 `.agents/skill-backups/<唯一批次>/`，完整保留定制与附加文件；`data.backups` 返回原路径与备份路径，失败会尝试恢复并返回未恢复路径。重复安装不重复备份，`--dry-run` 不迁移或写入。自动化业务命令显式加 `--json`，替换示例标识。

CLI 文档、开发和维护三份 Skill 同级安装，跨目录引用随包提供。`guide cli-doc/activity-dev/activity-fix/activity-metadata` 提供新入口；原 `guide development-standards/maintenance/run-existing/mcp/python-packages/automation-routing/web-module/win-design/win-repair/locator-repair/locator-usage/collection-repair/oapi` 等主题保留为对应新资料的入口，可能返回合并参考，按章节查阅。企业 API 仅在任务明确选择时使用。

`locator-repair.cjs`、`locator-usage.cjs` 和 Python UIA helper 属于安装后的 Skill 脚本，不是 `xbot-cli repair` 子命令。普通 DOM XPath、影刀分段 Selector 和 UIA 资产不能互换；当前用法与能力边界见 [Web 模块](../skills/xbot-activity-fix/references/web-repair.md)和 [Windows 模块](../skills/xbot-activity-fix/references/windows-repair.md)。

## 当前客户端信息

```powershell
xbot-cli client info
xbot-cli client info --json
xbot-cli schema client info
```

只读查询，不启动客户端、不打开或切换应用，兼容 6.3 和 6.4。`cli.version/executable` 是 xbot-cli 自身；`client.version/install_dir/processes` 来自正在运行的影刀进程，优先读取可执行文件产品版本，必要时用进程所在的版本目录并标记来源。多个安装同时运行、权限不足或版本未知时不选择任意一个。

`shell_cli.backend` 单独列出实际选用文件、来源和环境覆盖；它不代表正在运行的客户端。`shell_cli.state` 返回模式、Studio 和任务状态，`studio_mcp.connection` 显示实际 provider、endpoint 和协议信息。协议仍由接口发现决定，不依赖版本字符串。内置 shell-cli 的 `system version` 仅表示该工具版本，不能当成影刀版本。

此信息命令在部分查询失败时仍返回成功及已取得信息；机器调用须查看 `partial`、`client.detection_complete` 和各通道 `ok/error`，服务不可达不表示客户端进程未运行。`doctor` 继续提供必需通道失败时的非零退出语义。

## 应用 Python 依赖

先打开目标 Studio 应用，命令作用于应用工作区，供其中的流程使用。

```powershell
xbot-cli python package list --expect-app <app-id> --json
xbot-cli python package install openpyxl requests --expect-app <app-id> --json
xbot-cli python package install "openpyxl==3.1.5" --expect-app <app-id> --dry-run --json
xbot-cli python package install openpyxl --index-url https://pypi.org/simple --expect-app <app-id> --json
xbot-cli python package uninstall openpyxl --expect-app <app-id> --json
```

安装接受包名或 `NAME==VERSION`，卸载只接受包名，每批最多 50 个；客户端成功后更新应用依赖配置。`list` 返回工作区可用包，不等于直接依赖清单。支持 `--verbosity 0..4`（默认 1）、`--timeout <秒>`（默认 390），安装/卸载支持离线预览。超时只结束 CLI 等待，不代表客户端 pip 已停止；失败保留原始日志及部分结果，不自动重试。

旧版使用 `python_pip`，新版使用 `xbot-bash python package`，不需要 `execute_cell`。完整参数与兼容边界见 `schema python package install` 和 `guide python-packages`。

## Windows UIA 快照（实验性）

### 搜索窗口

```powershell
xbot-cli win32 list
xbot-cli win32 list --title "咚咚"
xbot-cli win32 list --title "咚咚" --format text
xbot-cli win32 list --title "完整窗口标题" --exact
xbot-cli win32 list --pid 1234 --all --json
```

`win32 list` 默认列出当前桌面可见且有标题的顶层窗口，`--title` 对标题做不区分大小写的字面包含匹配；`--exact` 改为完整标题匹配，需同时指定 `--title`。`--pid` 可与标题筛选组合；`--all` 包含隐藏和无标题窗口。空白关键词会报参数错误，零匹配成功返回空列表，同名窗口全部保留，不自动激活任何目标。

终端默认显示表格，包含 PID、十进制/十六进制 HWND、状态、进程名和窗口标题；管道默认 JSON，`--format text` 可强制表格。JSON 的 `data.windows` 还包含进程路径、窗口类名、可见和最小化状态。进程信息因权限无法读取时保留窗口并将对应字段设为 `null`。该命令直接只读枚举系统窗口，不依赖 Python、UIA 或影刀安装，不接受 `--expect-app`、`--dry-run`。

从表格选择目标的 PID/HWND，再调用 `activate` 或 `snapshot`。这些值仅表示采集时的窗口，软件重启后重新搜索；动作命令仍会核对窗口归属。

### 读取元素树

```powershell
xbot-cli help win32 snapshot
xbot-cli schema win32 snapshot
# 用已确认的实际 HWND/PID 替换示例值
xbot-cli win32 snapshot --hwnd 0x12345 --pid 1234 --format text
xbot-cli win32 snapshot --hwnd 0x12345 --pid 1234 --max-nodes 1000 --max-depth 12 --timeout 10 --json
```

文本模式与 Web 快照一样展示树形层级，每行显示控件类型、名称、ref，以及非空的 AutomationId、类名和禁用/屏幕外状态，例如：

```text
window "窗口" [ref=w1]
├─ pane [ref=w2]
│  └─ edit "搜索" [ref=w3] [automation_id="SearchInput"] [class="QLineEdit"]
└─ button "发送" [ref=w4]
```

文本保留无名称的中间节点，不额外缩短控件名称；节点、深度、时间等限制导致的部分树会在树前提示原因。交互终端默认树形文本；管道和重定向默认 JSON，可用 `--format text` 强制树形。`--json` 或 `--format pretty` 保留完整节点属性、坐标和采集元数据。

该命令不要求 Studio 打开，不自动选择活动窗口，不点击或保存元素。HWND 支持非零十进制/十六进制，并须属于显式传入的 PID。`--max-nodes` 范围为 1–5000，`--max-depth` 为 0–64，`--timeout` 为 1–60 秒；CLI 使用子进程超时约束提供器，输出超限、依赖缺失、窗口不符和采集错误分别返回失败。

通过 `XBOT_UIA_PYTHON` 指定已安装 `uiautomation` 的 Python 可执行文件，缺省使用 `python`，不自动安装依赖。快照保留完整性和截断原因；部分树不能据此证明元素不存在。ref 与 `snapshot_id` 配对用于 Windows 动作，不能当作 Web ref 或长期业务身份。密码字段保守隐藏，其他业务文本仍应按任务范围处理。

CLI 将快照保存到 `%LOCALAPPDATA%\xbot-cli\win32-snapshots`，也可用 `XBOT_WIN32_STATE_DIR` 指定本地目录。JSON 返回 `snapshot_file`、`ref_scope: snapshot_scoped_actions` 和 `refs_resolvable_for_actions: true`。快照最多使用 24 小时；进程重启、控件被替换、祖先或身份属性变化时即使未到期也会拒绝。独立运行 `snapshot.py` 仍只提供观察数据，不能代替 CLI 的动作快照。

`win32 snapshot` 不接受 `--expect-app` 或 `--dry-run`。Python Windows 修复 helper 是另一条按需运行链路，其内部预算不能中断卡住的 UIA/provider 调用；不能据 CLI 快照超时能力声称所有 Windows 动作已有硬超时。

### 执行控件动作

先采集快照，从当前树里选实际 ref；下面每行分别展示用法，连续动作须使用上一步返回的 `data.snapshot.snapshot_id` 和该树内的新 ref：

```powershell
xbot-cli win32 inspect w4 --snapshot <SNAPSHOT_ID> --json
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --format text
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --method invoke
xbot-cli win32 dblclick w4 --snapshot <SNAPSHOT_ID>
xbot-cli win32 fill w8 "替换文本" --snapshot <SNAPSHOT_ID>
xbot-cli win32 fill w8 --file input.txt --snapshot <SNAPSHOT_ID>
xbot-cli win32 type w8 "字面文本 {Enter}" --snapshot <SNAPSHOT_ID>
xbot-cli win32 press w8 Ctrl+A --snapshot <SNAPSHOT_ID>
xbot-cli win32 check w6 --snapshot <SNAPSHOT_ID>
xbot-cli win32 uncheck w6 --snapshot <SNAPSHOT_ID>
xbot-cli win32 select w9 --snapshot <SNAPSHOT_ID>
xbot-cli win32 expand w10 --snapshot <SNAPSHOT_ID>
xbot-cli win32 collapse w10 --snapshot <SNAPSHOT_ID>
xbot-cli win32 scroll w12 down --large --snapshot <SNAPSHOT_ID>
xbot-cli win32 focus w8 --snapshot <SNAPSHOT_ID>
xbot-cli win32 hover w4 --snapshot <SNAPSHOT_ID>
```

`inspect` 只读返回当前属性、支持的 Pattern，以及可读取的 Value/勾选/选择/展开/滚动状态。`click` 默认使用 `physical`，物理点击控件实时中心；`--method invoke` 可选用 InvokePattern，不发送鼠标。`fill` 使用 ValuePattern，不支持或只读时失败。`check/uncheck` 使用 TogglePattern，`select` 使用 SelectionItemPattern，`expand/collapse` 使用 ExpandCollapsePattern，`scroll` 使用 ScrollPattern，方向为 `up/down/left/right`，默认小步长。已是目标值或状态时不重复修改，返回 `applied: false, outcome: no_change`。控件支持哪些 Pattern 取决于目标软件，可先用 `inspect` 确认。

默认 `click`（或显式 `--method physical`）、`dblclick` 和 `hover` 会将目标窗口置前，并核对控件中心的实时 UIA 命中对象。`type` 将控件置焦后输入字面文本，`{Enter}` 不会作为按键执行；`press` 只接受一个按键或组合键（如 `Enter`、`Ctrl+A`），不接受 UIAutomation 的任意按键脚本。`fill/type --file` 支持 UTF-8 文件或 `-` 标准输入，文本经 stdin 传给提供器，不放入 Python 进程命令行；预览不回显输入内容。每次点击只执行所选方式，physical 与 invoke 不会相互自动回退；其他 Pattern 动作失败也不会自动改成物理操作。

点击、双击和悬停返回 `target_control`（名称、类型、AutomationId、类名、实时 bounds）及 `execution`（实际 method、是否使用鼠标、物理操作点及坐标系）。文本也显示这些信息。`execution.effect_verified: false` 表示尚未验证界面效果，不能仅凭调用返回成功判断页面已经切换。`invoke` 不发送鼠标点击，也没有点击坐标。微信 4.2.0.52 的通讯录导航实测会出现 Invoke 返回但页面不切换；重新观察后，使用 `click <按钮ref> --snapshot <新快照ID> --method physical` 可切换。物理点击选中的控件中心；若误选窗口根节点，点击点便是窗口中心，应先用 `inspect` 确认 ref 对应按钮。

动作前核对 PID/HWND、进程启动时间、根及祖先 RuntimeId、控件类型/名称/AutomationId/类名/框架及启用状态。查找只沿原祖先链精确匹配，不按遍历下标或同名控件替代。没有 RuntimeId 或采集属性不完整的 ref 不能执行。每个目标进程只允许一个 CLI 动作并发；动作开始前独占写入 `.used` 标记，原快照随后拒绝再次执行或读取动作 ref。标记只防止同一快照重放，不等同业务幂等。

动作返回后自动补采新树，文本直接显示，JSON 位于 `data.snapshot`。动作返回 `applied: true, outcome: action_returned` 只表示调用返回，业务结果应另行核对。后续快照失败保留动作结果并附 `warning`，不把它当成动作失败或自动再次执行。动作超时或 provider 异常且已有 `.used` 标记时返回 `applied: null, outcome: unknown`，应重新观察并对账。`--timeout` 为单个动作提供器上限（1–60 秒），随后补采快照另使用同样预算。

除只读 `inspect` 外，上述动作支持离线 `--dry-run`；预览只校验输入，不读取快照、加载 UIA 或操作窗口。所有 Windows 命令均独立于 Studio，不接受 `--expect-app`。旧版仅观察快照需重新采集才能用于动作。

### 激活软件元素树

```powershell
xbot-cli win32 activate --hwnd 0x12345 --pid 1234 --dry-run --json
xbot-cli win32 activate --hwnd 0x12345 --pid 1234 --timeout 30 --json
xbot-cli win32 activate --hwnd 0x12345 --pid 1234 --provider wework --json
xbot-cli win32 snapshot --hwnd 0x12345 --pid 1234 --json
```

`--provider auto` 为默认值：对 x86 `WXWork.exe` 的 `WeWorkWindow` 使用内置企微适配，其他目标使用影刀 RpaBuilder。`--provider wework` 强制使用内置企微适配，目标身份或 ABI 不符即拒绝，不会改用其他注入方式。内置适配没有 Python/pefile/影刀依赖，从 DuiLib 的 OnGetObject 导出和活对象虚表解析消息入口；只接受经过校验的门控、虚表及完整五字节序言。使用独立代码页，不占用原程序代码洞穴；保留 this 指针，短暂暂停目标并检查线程指令位置，写入失败时回滚。成功后保留激活代码，脚本式执行一次即结束，无常驻 watch。

影刀路径从 **64 位注册表 `HKLM\SOFTWARE\ShadowBot x64` 的 `InstallPath`** 读取安装根目录，按数字版本号选择最大的 `shadowbot-*` 子目录。根据目标进程架构定位 `Native\x86\RpaBuilder.dll` 或 `Native\x64\RpaBuilder.dll`，并分别使用 `SysWOW64` 下的 32 位或 `System32` 下的 64 位 Windows PowerShell 隔离调用 `InstallRpaDllSimple`。不限制影刀版本，不回退到旧目录或其他架构。`--provider shadowbot` 对 x86 企微使用 `Native\x86\misc.exe wxwork <显式 HWND>`，其他应用使用 RpaBuilder。不需要打开 Studio，也不打包厂商 DLL。

该命令修改显式指定的 x86 或 x64 目标进程，需要 Windows x64 系统及足够进程权限。调用前后核对 HWND/PID 和进程实例，工作进程再次核对架构及进程创建时间。影刀 DLL 结果提供 `target_architecture`、`worker_architecture` 和实际 DLL 路径。相同进程的并发调用会拒绝；RpaBuilder 路径检测到任一位置已有该 DLL 时返回 `status: already_loaded`，并保留实际 `loaded_dll` 路径，不再次安装。首次安装返回成功且确认目标加载 DLL 后返回 `status: activated`。命令结束不恢复、不卸载，没有常驻监听。没有 `--expect-app`；`--dry-run` 只校验参数与预览，完全不读取目标或加载 DLL。DLL 加载成功不代表应用已暴露完整元素树，应再通过 `win32 snapshot` 验证。

`tree_verification: not_checked` 表示加载结果与树完整性分别核验，请随后用 `snapshot` 检查所需控件。该能力取决于 DLL 对目标程序的支持，不能保证所有软件均暴露完整元素树或自动获得点击、输入能力。`--timeout` 为 1–60 秒，默认 30 秒；超时、错误或子进程中断可能已经加载 DLL，错误标记为不可自动重试，保留状态后先核对。目标权限较高时使用同等权限运行 CLI。

企微内置适配返回 `backend: wework_builtin`、首次 `activated`、重复 `already_active`。重复调用会核对完整已知钩子字节，`install_called: false`；已有未知跳转不会被覆盖。`hook_verification: confirmed_by_bytes` 与 `accessibility_verification: object_returned` 仍不证明元素树完整，应再采集 UIA 快照。代码已安装但 WM_GETOBJECT 未确认时返回 `WIN32_ACTIVATION_UNVERIFIED / installed_unverified`，下次只验证现有钩子，不再安装。内置适配的时间预算限制解析和消息验证，短暂写入事务会先恢复目标线程再返回。

`misc.exe` 只有在厂商标记及两次消息回声探针都通过时才返回 `activated/already_active`。exit 0 或只有 `RpaBugFixFlags` 标记都不算成功；结果未知时记录进程实例并阻止重复安装。厂商 helper 超时仍可能运行，不强制杀死它，避免中断其内部短暂暂停操作。

部分 DuiLib/MSAA 控件没有 RuntimeId。快照保留这些节点和子树，并提示 `MISSING_RUNTIME_ID`；真实重复的非空 ID 仍中止相应分支并提示采集不完整。无 ID 控件及其后代的 ref 当前不能通过安全动作定位。

Java 引导器、WinForms 检查器、RPC 宿主及运行库的接入边界见 [Windows 原生组件](windows-native-components.md)。文件存在本身不证明应用受支持。

若返回 `WIN32_ACTIVATION_PROVIDER_UNSUPPORTED`、`hresult: 0x80004001`，表示调用已进入影刀 DLL，但该入口对当前应用返回 `E_NOTIMPL`。它与 `WIN32_TARGET_ARCH_UNSUPPORTED` 不同，不能用更换 32/64 位调用解决。

开发者可设置 `XBOT_TEST_QT_X86`、`XBOT_TEST_QT_X64` 为已有的匹配架构 Qt 5 运行库目录（包含 `Qt5Widgets.dll`，`platforms\qwindows.dll` 在该目录或上级目录），运行 `npm run test:win32-activation`。此可选实测需要本机影刀及 Visual Studio C++ 工具，只编译并操作临时目录中的自建隐藏窗口，验证两种架构的首次激活、重复调用、进程/窗口/位数校验、并发拒绝及 DLL 不支持应用时的返回结果。

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

# VisualFlow（6.4 copilot）
xbot-cli flow create report --kind visual --expect-app <app-id>
xbot-cli flow source read report --offset 1 --limit 200 --expect-app <app-id>
xbot-cli flow source write report --file visual_source.py --expect-app <app-id>
xbot-cli flow source edit report --old @before.txt --new @after.txt --expect-app <app-id>
xbot-cli flow verify report --timeout 600 --expect-app <app-id>

# VisualFlow（旧版 legacy）
xbot-cli flow create report --kind visual --expect-app <app-id>
xbot-cli flow visual edit report --file ops.json --expect-app <app-id>
xbot-cli flow visual fill report --fills @fills.json --expect-app <app-id>
xbot-cli flow visual disable <block-id> --flow report --expect-app <app-id>
xbot-cli flow visual enable <block-id> --flow report --expect-app <app-id>
xbot-cli flow verify report --timeout 600 --expect-app <app-id>
xbot-cli flow visual snapshots report --expect-app <app-id>
```

`flow check` 检查整个应用的静态诊断（`scope: app`），传入流程时先核验目标，不启动流程。`flow verify` 解析目标并检查后实际运行，再读取 Out；失败时附日志尾部，用户暂停或停止则立即返回。修改操作成功、静态检查通过和实际运行成功是不同的结果，不应混为一谈。

MCP 通过只读发现自动选择旧版 Studio 接口或 6.4 Copilot 会话接口；`doctor --json` 返回实际 provider/endpoint。原 `flow get/read` 保留，6.4 返回源码；`flow code` 在两种后端均可用。`flow source` 在旧版仅支持 CodeFlow，在 6.4 也支持 VisualFlow；`--block-nos` 为指令编号而非旧 block_id，不能与源码 offset/limit 混用。详见 [MCP 兼容指南](../skills/xbot-cli-doc/references/operations.md#mcp) 或 `xbot-cli guide mcp`。

### 流程参数类型与写入

```bash
xbot-cli flow params types --json
xbot-cli flow params list process2 --expect-app <app-id> --json
xbot-cli flow params set process2 --operations @params.json --expect-app <app-id> --json
```

`types` 离线读取已安装客户端 DLL 内的类型资源，不加载或执行该 DLL，文本输出为表格。
使用返回的真实 `id`，例如 `list`、`dict`、`xbot._web.browser.WebBrowser`、
`openpyxl.Workbook`，不再用 `any` 代替集合或对象。目录是静态类型定义，不等同于所有
场景的可选参数清单，也不保证包含扩展或动态类型；未知的新类型明确拒绝，既有未知类型可保留或删除。
保留目录声明的 scope；`file/directory` 标注为应用主流程参数用途。不提供 `password` 参数类型。
6.3.39 子流程实测可加载并运行，因此 CLI 允许写入但给出范围 warning，不推断编辑器一定提供该选项。
未知的新 scope 明确拒绝，既有同类型同方向参数可保留并提示。

legacy 后端的 CodeFlow 和 VisualFlow 共用本地参数元数据适配器：先整批校验，必要时保存已有编辑，
仅替换 `.dev/*.flow.json` 的 `parameters` 区域，然后调用一次 `app.reload` 并通过
MCP 回读。无净变化时不写入、不 reload。不调用 sync，不关闭编辑器，不手工修改
Python、PDB 或签名文件。原始 `tool studio call flow.write_parameters` 仍受客户端自身限制。

6.4 copilot 后端通过原生 parameter set/remove 修改参数，保留 operations 输入格式及既有参数省略字段。连续且名称不重复的 upsert 合批，混合修改按顺序执行；失败保留 `completed_count/results/cause`，没有跨命令事务，不自动整批重试。新增表达式参数省略初值时使用原生默认 `None`；需要下文 legacy 默认值时明确填写。主流程 In 仅支持 int/float/str/bool/file；子流程以客户端注册类型为准。结果使用 `backend:copilot-mcp` 和原生 results，不额外执行磁盘写入或 reload。

`operations` 继续接受数组或 `{"operations":[...]}`；`upsert` 需要
`name/direction/type`，可选 `kind/value/description`。同类型同方向更新时省略字段保留旧值，
改变类型或方向时重置不兼容的初始值；`remove` 仅接受 `op/name`。
In 常用默认值为：字符串/路径为空文本，整数 `0`、小数 `0.0`、布尔 `False`、
列表 `[]`、字典 `{}`、字节 `b''`；其他对象默认 `None`，类型化列表按目录的 `base=list` 使用 `[]`。
`None` 只表示未取得对象，不会创建浏览器、Excel 或数据库连接。
`any` 显式提供非空 `value` 时必须同时指定 `kind`。表达式作为文本保存，不由 CLI 求值。
计算型 Out 省略 `value`，使用无初始化表达式语义；VisualFlow 的产出变量须与 Out 同名。

legacy 成功结果区分 `written/reloaded/readback_verified/runtime_verified`。
reload 丢弃未保存的内存编辑，因此保存已有工作必须发生在外部写入之前。
目标切换、并发编辑、磁盘与内存不一致时拒绝继续；写入后失败会保留阶段和已知/未知结果，
不自动回滚或整批重试。`--dry-run` 仍保持离线，只校验输入结构，
不验证当前流程、类型目录、scope 或客户端接受性。参数回读成功不等同实际运行通过。

完整示例、默认值和故障处理见[流程参数使用文档](flow-parameters.md)，
也可离线运行 `xbot-cli guide flow-parameters`。

已有运行超时时，使用返回的 `run_id` 继续等待，不重新启动：

```bash
xbot-cli flow wait <run-id> --timeout 600 --expect-app <app-id>
```

同步当前编辑应用并关闭编辑器：

```bash
xbot-cli app sync --expect-app <app-id> --dry-run --json
xbot-cli app sync --expect-app <app-id> --json
```

调用原生 `shadowbot.shell-cli.exe studio current sync`，不发布应用。`synced`、
`studio_closed`、`uploaded` 分别记录同步结果、编辑器关闭状态和本次上传日志证据；
无改动可能没有上传日志，编辑器关闭也不能单独证明同步成功。失败后先核对结果，不自动重试。

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

此备用入口不依赖本机客户端或 Studio。推荐使用
`oapi auth login --ak <AccessKeyId> --sk-stdin`，从标准输入读取密钥，避免密钥进入命令行。
凭据使用当前 Windows 用户的 DPAPI 加密后原子写入本地配置，旧明文 JSON 在下次登录或 token 刷新时迁移。
token 过期前复用、过期后自动刷新，`oapi auth logout` 清除当前授权；也可用环境变量
`XBOT_OAPI_ACCESS_KEY_ID` / `XBOT_OAPI_ACCESS_KEY_SECRET`（兼容 `XBOT_PLATFORM_*`），
专有云设置 `XBOT_OAPI_BASE_URL`。旧版双位置参数和 `--sk` 仍兼容，但新脚本不应使用。
接口范围、调度标识关系、分页和异步日志恢复详见[平台指南](../skills/xbot-cli-doc/references/operations.md#environment)，
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

## 指令操作

`activity list` 通过开放平台 `app.list` 查询并筛选 `appType=activity`，**先筛选指令，再按指令数量分页**。默认只返回 `--page` 指定的一页（默认第 1 页），`--page-size` 是每页指令数，只有末页可以不足；`--all` 返回从指定指令页开始的全部剩余指令。底层接口不支持按类型过滤，因此每次从应用列表第 1 页开始、每批最多 100 条读取混合资源，直到填满请求页并找到下一条指令，或扫描结束；一次命令最多扫描 100 页应用列表。`--search` 和 `--owner` 在每次底层请求中保持一致。

清单字段为 `activity`，每项仅保留 `appId`、`appName`、`appType` 和 `updateTime`，文本输出使用与 `app list` 相同的表格结构。`count` 是本次返回的指令数，`page.page/size/offset` 均对应指令分页，不再透传混合资源的分页数据。`page.total/pages` 只有在底层应用列表扫描完成后才是准确的指令总数/总页数，提前停止时为 `null`。`complete=true` 表示返回结果之后没有剩余指令；`truncated=true` 表示达到扫描上限、尚未扫描完整。普通分页若因上限无法填满请求页，返回 `ACTIVITY_PAGINATION_LIMIT` 而不是伪装成不足一页的末页；可用 `--search/--owner` 缩小范围，或用 `--all` 读取上限内的部分结果并检查 `truncated`。

未配置凭据时返回 `PLATFORM_AUTH` 和配置引导，需要运行 `xbot-cli oapi auth login --ak <AccessKeyId> --sk-stdin`，或设置 `XBOT_OAPI_ACCESS_KEY_ID` / `XBOT_OAPI_ACCESS_KEY_SECRET`。

```powershell
xbot-cli activity list --json
xbot-cli activity list --page 2 --page-size 20 --json
xbot-cli activity list --all --json
xbot-cli activity list --owner admin@bigdata --json
xbot-cli activity open <activity-id> --json
xbot-cli flow list --expect-app <activity-id> --json
xbot-cli flow get <flow-id> --expect-app <activity-id> --json
```

`activity open` 使用本机客户端，不需要开放平台凭据；打开后校验 `robot_type=activity`，防止误开普通应用。流程读取仍使用 `flow list/get` 和 `--expect-app`。

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

流程等既有命令的 `--timeout` 范围为 1–86400 秒；`win32 snapshot/activate --timeout` 为 1–60 秒。以对应命令的 schema 为准。

## 网页操作

`web` 使用影刀浏览器后端，以快照中的真实元素编号操作页面：

```powershell
xbot-cli web open example.com
xbot-cli web fill e4 "hello"
xbot-cli web click e5
xbot-cli web press Enter
xbot-cli web goto https://example.com/page
xbot-cli web snapshot --output page.txt
xbot-cli web xpath e4
xbot-cli web xpath e4 e5
```

示例编号必须替换为最近快照中的编号。`open` 自动为裸域名补全 HTTPS；`active`（别名 `attach`）附着活动页面。`click`、`dblclick`、`fill`、`hover`、`check`、`uncheck`、`select` 接受元素编号；`type` 和 `press` 默认作用于当前焦点，`press --ref <ref>` 指定元素。导航使用 `goto`、`reload`、`go-back`、`go-forward` 和 `close`。

打开、附着、导航（除 `close`）及上述元素、键盘交互成功后默认获取快照，`--no-snapshot` 可关闭这一步；`eval` 和底层工具调用不追加快照。快照失败会保留操作成功及 `applied: true`，并返回 `warning`；不要因此重复点击或提交。默认文本快照以完整元素树展示，不隐式折叠或截断；iframe ref 带文档作用域前缀，不能去掉前缀或跨文档复用。`--snapshot-view raw` 显示原始快照；`--role/--filter/--limit/--offset` 显式筛选或分页，JSON 保留完整树。`snapshot --output` 保存正文且不覆盖已有文件。终端默认文本输出，管道默认 JSON；`--json` 显式保留完整结果，`--format text` 可强制输出文本。

支持的页面及元素操作还返回影刀 Python，文本标题为“影刀 Python：”，JSON 字段为 `data.python`。`--no-python` 关闭代码生成，和 `--no-snapshot` 独立。代码生成失败不会重放或否定已成功的动作；`runtime_verified: false` 表示尚未在影刀运行时验证。

`web xpath <ref...>` 只读生成至多 5 组候选：单 ref 返回同一元素的备选 XPath，多 ref 返回覆盖所选同类元素的共同规则。iframe 返回按外层到内层排序的 `Selector.xpath.segments` 列表，open ShadowRoot 输出影刀语法并提示需在影刀验证；closed ShadowRoot 明确拒绝生成。类型不一致、上下文不兼容或缺少可靠规则时给出原因，歧义候选为 `needs_review`，不能用于未经复核的单元素点击。

全部网页命令均通过影刀 Chrome Bridge 命名管道和 CDP 执行，命令形式参考最新 Playwright CLI。`--name` 仅为旧调用保留兼容参数，不会写入影刀元素库。`eval` 保持注入 `activePage` 的只读异步函数体语法。完整参数见 `xbot-cli web --help` 和 [浏览器指南](../skills/xbot-cli-doc/references/operations.md#web)。

`web open/active` 将后续命令绑定到同一调用端的管道及标签页，切换活动标签不会暗中换页；`web active` 显式重新绑定。可用 `XBOT_WEB_PIPE`、`XBOT_WEB_TAB_ID` 指定上下文，多进程脚本可设置独立的 `XBOT_WEB_SESSION` 名称；这不是独立浏览器 profile。管道前缀不作为浏览器产品身份判据。快照失败保留已执行动作的证据，不自动重试。

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

客户端路径优先自动发现运行中客户端、注册表和有限已知安装目录，支持非默认盘符。`XBOT_SHELL_CLI`、`XBOT_LAUNCHER` 是权威覆盖项，即使路径无效也不会静默回退；`doctor` 保留实际路径、发现来源及具体修复建议。`XBOT_MCP_ENDPOINT`、`XBOT_BROWSER_MCP_ENDPOINT` 可覆盖对应通道配置，用于隔离测试。离线单元测试和 mock 后端只能证明对应契约，不能代替真实客户端的开发、运行、同步或发布验收。

`XBOT_MCP_MODE=auto|legacy|copilot` 控制 Studio MCP 协议。auto 先只读发现旧接口，仅标准旧路径返回 404 时尝试同源 Copilot 路径；显式自定义路径、认证或服务异常不被静默改写。6.4 会话仅在当前 CLI 进程内复用；已发出的写入/运行不自动重试或跨协议重放。

源码位于仓库根目录的 `src/`，集成测试位于 `tests/`。`npm run build` 将 Rust release 程序放入 `native/win32-x64/xbot-cli.exe`；`bin/xbot-cli.cjs` 是 npm 命令入口。`npm pack` 仅生成本地包，正式发布步骤见[发布说明](releasing.md)。

活动 Skill 源码位于 `skills/xbot-cli-doc/`、`skills/xbot-activity-dev/`、`skills/xbot-activity-fix/`，构建时嵌入 CLI，随 npm 包中的可执行文件提供。原 `xbot-app-dev` 和 `xbot-development-standards` 原样保留在 [备份目录](../backups/skills/2026-11-26/)，不嵌入活动安装清单。开发和维护先安排实际任务、跟踪状态与证据；标题、label、说明、业务展示、指令/动作描述、启用禁用及分组步骤集中在 [元数据参考](../skills/xbot-activity-dev/references/metadata.md)。Agent 遵守用户授权与项目规范；已发布指令的公开身份和调用契约保持兼容。