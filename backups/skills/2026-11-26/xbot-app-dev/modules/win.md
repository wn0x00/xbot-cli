# Windows 自动化与修复

本文只描述当前 CLI 和随包 helper 的支持路径；实际安装版本以自身 help/schema 为准。

临时操作与诊断优先使用 CLI 的窗口搜索、激活、快照与 ref 动作；需要沉淀到影刀流程的单控件定位修复使用 **Python + uiautomation**，不依赖影刀元素库。两者的资产、快照和部署条件分别核对。可由编码版流程执行，也可由可视化流程通过 `process.run` 调用同一编码版。实际用法见 [Python 修复链路](../references/win-repair.md)。

## 如何选择

先按 [平台识别规则](../references/automation-routing.md) 查看报错的真实调用和对象来源。网页 DOM 走 Web；业务程序原生控件走本模块。影刀是流程运行工具，不能默认把影刀自身当作待修复目标。

| 组件 | 用途 | 当前边界 |
|---|---|---|
| `scripts/win32/uia_runtime.py` | 实时查找、追加兼容定位、单控件身份校验、微信测试动作与对账 | 需安装对应依赖，按目标程序单独验证 |
| `scripts/win32/run_uia.py` | 用可信本地 JSON 请求调用上述 Python helper | 独立入口；需安装对应依赖 |
| `xbot-cli win32 snapshot` / `snapshot.py` | 指定 HWND/PID 的受限只读 UIA 树 | CLI 保存动作快照；独立脚本仍只供观察 |
| `xbot-cli win32 inspect/click/fill/...` | 按快照 ref 校验实时控件并执行 UIA 动作 | click 默认物理点击；InvokePattern 需显式指定，其他 Pattern 按目标支持情况使用 |
| `xbot-cli win32 list` | 按窗口标题搜索，返回 PID/HWND、进程名和状态 | 原生只读枚举，不依赖影刀或 Python；终端表格、管道 JSON |
| `xbot-cli win32 activate` | 自动使用企微内置适配或本机影刀组件激活显式 HWND/PID | 独立于 Studio；修改目标进程，验证身份和已知钩子后防止重复安装 |

微信具有专用搜索、消息动作和对账适配。其他业务程序必须单独确认元素树、权限、控件身份和动作方式，不把专用适配当成通用桌面能力。

## 激活元素树

用户明确要求激活软件元素树时，先搜索并确认实际业务窗口的 HWND/PID，再运行：

```powershell
xbot-cli win32 list --title "咚咚" --format text
xbot-cli win32 activate --hwnd <HWND> --pid <PID> --dry-run --json
xbot-cli win32 activate --hwnd <HWND> --pid <PID> --timeout 30 --json
xbot-cli win32 snapshot --hwnd <HWND> --pid <PID> --format text
xbot-cli win32 snapshot --hwnd <HWND> --pid <PID> --json
```

`snapshot` 在交互终端以 `├─` / `└─` 树形展示控件类型、名称、ref、AutomationId、类名及异常状态，保留无名称的中间节点，并在树前提示采集不完整的原因。管道默认 JSON，人工查看或重定向树形文本时显式加 `--format text`；`--json` / `--format pretty` 保留坐标、RuntimeId 和完整采集元数据。ref 与明确的 `snapshot_id` 配对才能执行动作。

`list --title` 是不区分大小写的字面包含匹配，`--exact` 改为完整标题匹配；`--pid` 可组合筛选，`--all` 包含隐藏和无标题窗口。多匹配全部返回，不默认选择第一个目标；零匹配不改窗口。JSON 从 `data.windows` 读取数字 `pid/hwnd` 或字符串 `hwnd_hex`。软件重启后重新搜索，不复用旧 PID/HWND。

路径发现固定使用 64 位 `HKLM\SOFTWARE\ShadowBot x64` 的 `InstallPath`，选择最大数字版本 `shadowbot-*` 子目录。按目标进程架构使用 `Native\x86\RpaBuilder.dll` + `SysWOW64` 32 位 PowerShell，或 `Native\x64\RpaBuilder.dll` + `System32` 64 位 PowerShell；结果包含目标与工作进程架构。工作进程再次核对架构与进程创建时间。不设影刀版本门槛、不回退旧版本或错误架构，使用原 DLL 的 `InstallRpaDllSimple`，无额外桥接 exe 或 Python 依赖。支持 Windows x64 系统上的 x86/x64 目标，需要足够进程权限；是否读到完整元素树须再用快照验证。

默认 `--provider auto` 对 x86 `WXWork.exe` + `WeWorkWindow` 使用内置企微适配，直接解析 DuiLib 导出、虚表和消息入口，无需影刀或 Python。`--provider wework` 可显式选择；ABI 不符时拒绝，不自动换注入方式。内置适配使用独立内存页并保留 this，事务结束恢复线程执行，激活代码保留到应用退出；重复调用只校验完整已知钩子。`activated/already_active` 与 `accessibility_verification: object_returned` 不证明业务控件完整，仍需 snapshot。`--provider shadowbot` 对 x86 企微调用原 `misc.exe wxwork <HWND>`，必须通过厂商标记及消息探针；exit 0 不算成功。结果未知时不重复注入。

Java/WinForms 尚未接入其专用协议，不能承诺调用对应 DLL 后 UIA 自动完整。Qt4 插件与运行库按原目录布局作为依赖使用。

空 RuntimeId 不作为重复节点处理，快照保留其子树并提示 `MISSING_RUNTIME_ID`。这类控件及其后代的 ref 当前不能安全执行动作，不能仅凭名称或遍历下标替代身份校验。

`activated` 表示入口成功且目标确已加载 DLL；`already_loaded` 表示检测到目标已有该 DLL，未再次安装。后者即使与当前最大目录版本不同也不重复加载。`tree_verification: not_checked` 不证明元素树完整，随后只读快照确认需要的控件。命令退出后不恢复、不卸载、不保留辅助进程。超时或错误后保留目标状态，不强制重放安装。激活本身不点击、输入或发送；控件操作由下述 UIA 动作执行。

## CLI 控件动作

```powershell
xbot-cli win32 inspect w4 --snapshot <SNAPSHOT_ID> --json
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --dry-run --json
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --format text
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --method invoke
xbot-cli win32 fill w8 "文本" --snapshot <SNAPSHOT_ID>
xbot-cli win32 press w8 Ctrl+A --snapshot <SNAPSHOT_ID>
```

从实际快照选择 ref，不能照抄示例编号。`inspect` 返回当前属性、可读取的 Value 和支持的 Pattern；`click` 默认 `physical`，物理点击控件实时中心，`--method invoke` 可选用 InvokePattern。`fill` 使用 Value，`check/uncheck` 使用 Toggle，`select` 使用 SelectionItem，`expand/collapse` 使用 ExpandCollapse，`scroll <ref> up/down/left/right [--large]` 使用 Scroll。已是目标状态时不修改。Pattern 动作缺少对应 Pattern 时明确失败，不自动改成物理操作；每次点击只执行所选方式。

默认 `click`（或显式 `--method physical`）、`dblclick`、`hover` 会置前窗口并校验中心命中；`type` 向已核对焦点控件输入字面文本，`press` 接受单键或组合键，不接受任意按键脚本。`fill/type --file <UTF8文件或->` 支持文件和 stdin，预览不回显正文。涉及发送、提交等业务动作仍须已有同范围用户授权。

点击、双击和悬停结果的 `target_control`、`execution` 显示目标名称/类型/bounds、实际方法及物理坐标，`effect_verified: false` 表示界面效果仍需核对。Invoke 没有鼠标点击点。微信的导航按钮可能声明支持 Invoke 但调用后不切换页面；默认物理点击已覆盖此情形。显式 Invoke 后如需改用物理点击，先重新快照核对结果，不要自动重放。选择按钮 ref，窗口根 ref 的物理中心就是窗口中心。

快照保存在 `%LOCALAPPDATA%\xbot-cli\win32-snapshots`（可用 `XBOT_WIN32_STATE_DIR` 覆盖），最多使用 24 小时。动作核对进程实例、窗口、原祖先链和 RuntimeId/身份属性，拒绝跨窗口、控件替换及不完整身份；不会放宽定位或用同名对象替代。每个目标进程的 CLI 动作互斥。动作开始后原快照写入 `.used` 并禁止重放，下一步使用返回的 `data.snapshot` 及其 `snapshot_id` 和 ref。外部人工操作仍可能形成竞态，业务结果需另核对。

动作返回 `applied: true` 只表明调用已返回；后续快照失败只附警告，不重放。已有 `.used` 的超时/异常返回 `applied: null, outcome: unknown`，先重新快照核对结果。动作提供器和补采快照分别受 `--timeout`（1–60 秒）约束。除 `inspect` 外均可离线 `--dry-run`，但不验证实时窗口。独立 `snapshot.py` 和旧版快照不能直接用于动作，需使用当前 CLI 重新采集。

包内 `scripts/win32/actions.py` 是与当前 CLI 同源的 worker 参考，需要 CLI 注入快照模块及执行上下文，不作为独立命令运行。复制这些脚本不会升级已安装 CLI；功能是否可用仍按现场帮助和 schema 判断。

## 修复逻辑

```text
实际报错与调用点
 -> 确认是 Windows、找准业务窗口
 -> 区分定位错误、环境错误、业务状态和动作后的观察错误
 -> AI 从流程反推原用途、单个/集合、父子依赖
 -> Python 查询全部旧、新定位并核对原身份
 -> 全部零匹配时，按原身份找唯一候选
 -> 追加新定位到资产副本，保留旧定位
 -> 按项目类型隔离验证；临时辅助结束恢复，再接入正式业务流程
 -> 动作前复核；动作后查业务结果
 -> 不确定先对账，不换定位重做动作
```

AI 负责说明“原来要操作谁”，Python 负责按明确合同执行检查。当前自动修复只恢复仍符合完整原身份的单控件，不使用名称模糊匹配或分数最高者自动点击。

新旧变体同时查询，通过 UIA 当前对象比较归并同一实例；不同实例、身份冲突、多个候选、窗口不唯一、树观察不完整时拒绝。查找阶段可兼容切换；发出动作后不能切换另一定位重试。

## 能力边界

可修：定位条件被改错，但有可信原身份，当前仍唯一满足身份的单控件。

停止并报告：原身份缺失、目标改名或必要祖先变化、集合用途、窗口歧义、相似干扰无法区分、权限/会话异常、观察不完整、动作结果未知。Windows 集合和父子版本联动尚未实现，不能套用 Web 集合 helper 冒充支持。

本机单客户端验证不代表跨电脑、跨版本、虚拟列表、远程桌面、锁屏、自绘界面均支持。UIA 名称/ID 不是服务器业务主键，唯一匹配仍不能排除业务绑定变化。消息记录只能确认客户端可见结果，不能独立证明服务器送达。

预算检查位于 UIA 调用之间，不能中断卡住的 COM/provider 调用；无人值守部署还需独立 worker 超时、进程隔离及跨环境回归。观察到动作之间存在竞态，当前实现不提供业务事务或分布式幂等。
