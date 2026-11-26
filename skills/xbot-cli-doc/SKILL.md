---
name: xbot-cli-doc
description: 查询或使用 xbot-cli 的功能、命令、参数、输出和客户端兼容时使用，覆盖应用与流程读写、sync 关闭设计器、运行任务、依赖及 Web/Windows 操作。指令开发规范和已有指令故障分别交给 xbot-activity-dev、xbot-activity-fix。
---

# xbot-cli 使用手册

说明 CLI 能做什么、怎么调用、怎样判断结果。以现场 `help/schema` 为准，不从旧示例猜参数。单纯查询或运行已有流程，只加载本 skill 的相关章节。

## 发现命令与功能清单

```powershell
Get-Command xbot-cli | Select-Object Source
xbot-cli version --json
xbot-cli help
xbot-cli help flow source read
xbot-cli schema flow source read
```

需要客户端时查 `client info --json`；连接异常再查 `doctor --json`。离线帮助、Web Bridge 和独立 Win32 能力按各自前提使用，不要求一律打开设计器。

| 入口 | 功能与适用条件 |
| --- | --- |
| `help / schema / guide / version` | 离线发现命令、参数、内置指南及 CLI 版本 |
| `client info / status / doctor` | 客户端身份、实际后端和 provider、当前工程、通道诊断；检查 partial 和各通道结果 |
| `app` | 应用查询、创建、只读查看、打开、保存、同步及发布 |
| `activity` | 查询、打开“我开发的指令”；市场标识、安装 ID、Studio app_id 分开核对 |
| `flow` | 流程创建、读取、源码/块编辑、In/Out、静态检查、运行、等待及日志 |
| `task` | 运行已发布应用、查询任务/日志、停止；使用 task_id |
| `python package` | 当前应用环境的依赖查询、安装、卸载 |
| `web` | 浏览器选择、页面绑定、快照、ref 交互、XPath 和只读 eval；需要目标扩展通道 |
| `win32` | 原生窗口枚举、元素树激活、快照及控件动作；激活会改变目标进程 |
| `tool studio / browser` | 底层工具 list/schema/call；必须再发现具体工具的契约 |
| `wiki` | 旧版 SDK 资料；copilot 改查客户端实际 API 定义 |
| `oapi` | 企业 API 的认证、机器人、任务、调度等；用户明确选择时使用 |
| `install / update` | 当前包的 skill 安装和 CLI 更新；查现场清单，不隐式覆盖或升级 |

常用命令、参数批次、VisualFlow、MCP 差异及 Web/Windows 示例统一放在 [操作参考](references/operations.md)。只查询一个命令时先用叶命令 help/schema；需要后端差异或完整步骤时再读对应章节。缺少命令时说明能力差异，不编造 flag；没有通用 `xbot-cli repair` 命令。

<a id="sync"></a>
## sync：同步并关闭设计器

`app sync` 可正常直接使用，本套流程主要用它关闭设计器。同步本身不改指令业务逻辑、In/Out、启用状态或分组，也不发布；当前草稿已有修改会按客户端正常同步。关闭设计器不影响指令正常使用。

1. 确认当前 app_id 和草稿属于本任务；需要先落盘时执行 save。
2. 直接 sync，再独立读取 status。正常开发、维护和关闭重开不另行询问同步确认；当前 `confirmation:none`，无需 `--yes`，dry-run 可选。
3. 直接维护磁盘配置前，必须观察到 `studio.opened=false`。设计器已关闭时直接继续，无需为关闭再打开。
4. 继续编辑时正常 open，核对正确工程和 `studio.opened=true`。

```powershell
xbot-cli app save --expect-app <app-id> --json
xbot-cli app sync --expect-app <app-id> --json
xbot-cli status --json
# 需要维护配置时，在确认 studio.opened=false 后备份并修改。
xbot-cli app open <app-id> --json
```

读取 `synced/studio_closed/uploaded/response`：`uploaded=false` 在无改动时可能表示无需上传，不阻止关闭核验。关闭不单独证明上传成功。超时、失败或状态矛盾时先查当前状态与原响应，不循环 sync 或强制终止客户端。

元数据完整验收需要“真实保存 → sync → 独立确认关闭 → 正常打开保存 → 回读比较”，具体见 [配置维护](../xbot-activity-dev/references/metadata.md#closed-edit)。只读调查无需关闭时不额外执行 sync。

## 输出与操作注意事项

- 机器读取显式加 `--json`；同时检查退出码、`ok/error`、partial/applied、阶段和运行 ID。非零退出不证明动作未发生，`retryable` 不证明写动作可以重放。
- `--expect-app` 只断言应用，不打开或切换应用，也不确认业务页面、窗口、账号。执行时使用真实 flow_id；示例 ID、ref 和路径都须替换。
- `--dry-run` 是支持该选项的命令的离线输入预览；`flow check` 静态；`flow verify` 会真实运行。save、sync、publish 各自取证。
- 超时、断连或动作后快照失败，先查同一次运行及业务结果。保留 Studio run_id、客户端 task_id，部分完成只补已确认未完成项，不整批重放。
- Web ref 来自对应页面当前快照；Win32 ref 配对当前 snapshot_id，动作后使用新快照。ref、PID/HWND、坐标不是长期业务身份。
- 复杂载荷用文件或 stdin，保持类型和表达式原义；PowerShell 的文件载荷写成 `'@params.json'`，不要裸写 `@params.json`。一次只用一个 stdin 消费者，日志与交付不保留凭据或无关完整业务正文。

## 职责配合

| 用户目标 | 主要入口 |
| --- | --- |
| 查能力/命令、查询状态、运行已有流程 | 本 skill；无需加载整套开发或维护规范 |
| 新建指令/指令集、扩展业务能力及所需参数与面板 | [xbot-activity-dev](../xbot-activity-dev/SKILL.md)，先安排开发任务 |
| 调查故障、修复既有行为、修改现有文案/开关/分组 | [xbot-activity-fix](../xbot-activity-fix/SKILL.md)，先安排维护任务；字段规范按需引用 dev |

普通应用沿用正常主流程，不套用指令集公开注册和测试分组规范。

只问 sync 用法时读本文件对应章节即可，不创建开发/维护清单；任务中仅为修复故障而调整参数，由 fix 组织并按需引用 dev 契约，不重复开两套清单。标题、开关和分组的精确步骤集中在元数据参考，CLI 不另复制字段规则。

三份 skill 配套放置时保留同级目录。配套 skill 缺失仍可用现场帮助完成独立 CLI 操作，写入前补足可靠契约，不猜补缺失规范。
