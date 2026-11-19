# Windows 自动化与修复

日期：2026-10-22；Skill 保持 v10。

当前路线是 **Python + uiautomation**，直接操作业务程序，不依赖影刀元素库或 Agent 回传快照。可由编码版流程执行，也可由可视化流程通过 `process.run` 调用同一编码版。实际用法见 [Python 修复链路](../references/win-repair.md)。

## 如何选择

先按 [平台识别规则](../references/automation-routing.md) 查看报错的真实调用和对象来源。网页 DOM 走 Web；业务程序原生控件走本模块。影刀是流程运行工具，不能默认把影刀自身当作待修复目标。

| 组件 | 用途 | 状态 |
|---|---|---|
| `scripts/win32/uia_runtime.py` | 实时查找、追加兼容定位、单控件身份校验、微信测试动作与对账 | 已在真实微信和影刀 Python 3.7 流程运行 |
| `scripts/win32/run_uia.py` | 用可信本地 JSON 请求调用上述 Python helper | 独立入口；需安装对应依赖 |
| `xbot-cli win32 snapshot` / `snapshot.py` | 指定 HWND/PID 的受限只读 UIA 树 | CLI 保存动作快照；独立脚本仍只供观察 |
| `xbot-cli win32 inspect/click/fill/...` | 按快照 ref 校验实时控件并执行 UIA 动作 | Pattern 按目标支持情况使用；物理点击须显式指定 |
| `xbot-cli win32 list` | 按窗口标题搜索，返回 PID/HWND、进程名和状态 | 原生只读枚举，不依赖影刀或 Python；终端表格、管道 JSON |
| `xbot-cli win32 activate` | 使用本机影刀 DLL 激活显式 HWND/PID 对应进程 | 独立于 Studio；修改目标进程，按实际加载状态防止重复安装 |
| `scripts/win32/locator.cjs` | 既有 sky.window2 快照查询/提议 | 历史 Agent 路线保留，与 UIA 资产不通用 |
| Windows-MCP、Umbriel | 未来可替换观察后端 | 仅调研，未接入本批实测 |

业务程序不限于微信，当前业务动作验收覆盖本机微信；咚咚另有元素树激活与只读观察验证。其他程序必须单独确认树、权限、控件身份和动作方式。

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

路径发现固定使用 64 位 `HKLM\SOFTWARE\ShadowBot x64` 的 `InstallPath`，选择最大数字版本 `shadowbot-*` 子目录内的 `Native\x64\RpaBuilder.dll`。不设影刀版本门槛、不回退旧版本，使用原 DLL 的 `InstallRpaDllSimple`，通过系统 PowerShell 隔离调用，无额外桥接 exe 或 Python 依赖。支持 x64 目标，需要足够进程权限。

`activated` 表示入口成功且目标确已加载 DLL；`already_loaded` 表示检测到目标已有该 DLL，未再次安装。后者即使与当前最大目录版本不同也不重复加载。`tree_verification: not_checked` 不证明元素树完整，随后只读快照确认需要的控件。命令退出后不恢复、不卸载、不保留辅助进程。超时或错误后保留目标状态，不强制重放安装。激活本身不点击、输入或发送；控件操作由下述 UIA 动作执行。

## CLI 控件动作

```powershell
xbot-cli win32 inspect w4 --snapshot <SNAPSHOT_ID> --json
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --dry-run --json
xbot-cli win32 click w4 --snapshot <SNAPSHOT_ID> --format text
xbot-cli win32 fill w8 "文本" --snapshot <SNAPSHOT_ID>
xbot-cli win32 press w8 Ctrl+A --snapshot <SNAPSHOT_ID>
```

从实际快照选择 ref，不能照抄示例编号。`inspect` 返回当前属性、可读取的 Value 和支持的 Pattern；`click` 默认 Invoke，`fill` 使用 Value，`check/uncheck` 使用 Toggle，`select` 使用 SelectionItem，`expand/collapse` 使用 ExpandCollapse，`scroll <ref> up/down/left/right [--large]` 使用 Scroll。已是目标状态时不修改。缺少 Pattern 时明确失败，不自动改成物理操作。

`click --method physical`、`dblclick`、`hover` 会置前窗口并校验中心命中；`type` 向已核对焦点控件输入字面文本，`press` 接受单键或组合键，不接受任意按键脚本。`fill/type --file <UTF8文件或->` 支持文件和 stdin，预览不回显正文。涉及发送、提交等业务动作仍须已有同范围用户授权。

快照保存在 `%LOCALAPPDATA%\xbot-cli\win32-snapshots`（可用 `XBOT_WIN32_STATE_DIR` 覆盖），最多使用 24 小时。动作核对进程实例、窗口、原祖先链和 RuntimeId/身份属性，拒绝跨窗口、控件替换及不完整身份；不会放宽定位或用同名对象替代。每个目标进程的 CLI 动作互斥。动作开始后原快照写入 `.used` 并禁止重放，下一步使用返回的 `data.snapshot` 及其 `snapshot_id` 和 ref。外部人工操作仍可能形成竞态，业务结果需另核对。

动作返回 `applied: true` 只表明调用已返回；后续快照失败只附警告，不重放。已有 `.used` 的超时/异常返回 `applied: null, outcome: unknown`，先重新快照核对结果。动作提供器和补采快照分别受 `--timeout`（1–60 秒）约束。除 `inspect` 外均可离线 `--dry-run`，但不验证实时窗口。独立 `snapshot.py` 和旧版快照不能直接用于动作，需使用当前 CLI 重新采集。

## 修复逻辑

```text
实际报错与调用点
 -> 确认是 Windows、找准业务窗口
 -> 区分定位错误、环境错误、业务状态和动作后的观察错误
 -> AI 从流程反推原用途、单个/集合、父子依赖
 -> Python 查询全部旧、新定位并核对原身份
 -> 全部零匹配时，按原身份找唯一候选
 -> 追加新定位到资产副本，保留旧定位
 -> 隔离测试，通过后才接入业务流程
 -> 动作前复核；动作后查业务结果
 -> 不确定先对账，不换定位重做动作
```

AI 负责说明“原来要操作谁”，Python 负责按明确合同执行检查。当前自动修复只恢复仍符合完整原身份的单控件，不使用名称模糊匹配或分数最高者自动点击。

新旧变体同时查询，通过 UIA 当前对象比较归并同一实例；不同实例、身份冲突、多个候选、窗口不唯一、树观察不完整时拒绝。查找阶段可兼容切换；发出动作后不能切换另一定位重试。

## 实测带来的改进

- 微信搜索有三个同名结果：用结果类型、业务 ID 和父区域消歧。
- 存在 `Invoke` 不等于业务动作成功：动作方式单独验证，不能拿接口返回值当发送回执。
- 搜索层、焦点和会话可能变化：发送前检查搜索层已关闭、群标题和输入内容正确；物理点击还检查前台及命中控件。
- 发送后控件消失属于结果核对问题：记录 `SEND_OUTCOME_UNKNOWN`，不再次进入定位修复。
- 操作台账阻止同一请求重复发送；只读 `reconcile` 核对目标群的精确消息与输入状态。
- 修改子流程后回读可视化调用参数，避免实际运行了另一份配置而误判测试通过。
- 窗口作用域使用系统标题而非会变动的 UIA 名称；最小化单独报错，只有显式 `activate_window:true` 才先恢复窗口。

## 能力边界

可修：定位条件被改错，但有可信原身份，当前仍唯一满足身份的单控件。

停止并报告：原身份缺失、目标改名或必要祖先变化、集合用途、窗口歧义、相似干扰无法区分、权限/会话异常、观察不完整、动作结果未知。Windows 集合和父子版本联动尚未实现，不能套用 Web 集合 helper 冒充支持。

本机单客户端验证不代表跨电脑、跨版本、虚拟列表、远程桌面、锁屏、自绘界面均支持。UIA 名称/ID 不是服务器业务主键，唯一匹配仍不能排除业务绑定变化。消息记录只能确认客户端可见结果，不能独立证明服务器送达。

预算检查位于 UIA 调用之间，不能中断卡住的 COM/provider 调用；无人值守部署还需独立 worker 超时、进程隔离及跨环境回归。观察到动作之间存在竞态，当前实现不提供业务事务或分布式幂等。
