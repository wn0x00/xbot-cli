---
name: xbot-app-dev
description: 使用 xbot-cli 开发、验收和维护影刀应用；按报错步骤加载 Web 或 Windows 模块。Web 支持 XPath 修复；Windows 支持 Python UIA 单控件定位修复与真实流程验证。适用于 CodeFlow、VisualFlow、运行故障恢复和企业开放 API 调度日志排障。
metadata:
  version: "10"
---

# 影刀应用与指令开发

通过 `xbot` 或 `xbot-cli` 调用影刀内置 shell-cli、studio-mcp 和 browser-use-mcp，两个命令名等价。不要使用旧的 `xbot_agent` / `python -m xbot_cli` 工作流。Windows 外部适配器须与影刀官方通道分别标明，不反射、注入或绕过影刀 SDK 的公开边界。

本 Skill 随 npm 包 `@guanzhu.me/xbot-cli` 分发，适用于 Windows x64 和 Node.js 22+。使用 `xbot install --skills` 安装到当前工作区，或加 `--global` 安装到用户的 `.agents/skills`。本地影刀操作需要客户端已启动并登录；`platform` 使用企业开放 API 凭据，无需本机客户端；帮助、schema 和 guide 可离线读取。

这个 Skill 的目标是把业务需求变成可重复、可验收、可维护的自动化流程。需要参数、输入输出、长期运行或维护时，优先沉淀为 CodeFlow，只有确实适合块编排时才使用 VisualFlow。Skill 不是常驻修复服务。

## 平台模块

修复入口先读取 [平台识别与模块选择](references/automation-routing.md)。由当前 Agent 自动读取报错行/块、相关调用代码、对象来源及原元素资料，判断**失败的这一步**属于 Web 还是 Windows，再加载对应模块。不要求用户先选平台，也不按整个应用的名称或大多数步骤判断。

影刀应用是承载自动化流程的项目，业务目标可以是任意受支持的程序窗口或网页。`app_id/flow_id/run_id` 与 `--expect-app` 标识影刀项目和执行上下文；业务窗口/进程、浏览器页面需另外确认。影刀 `status`、当前活动窗口以及变量名不能代替目标识别。

| 平台 | 入口 | 当前状态 |
|---|---|---|
| Web：失败调用实际操作网页 DOM、iframe 或 Shadow DOM | [modules/web.md](modules/web.md) | 现有 v10 能力，按已验证边界使用 |
| Windows：失败调用实际操作业务程序窗口或原生控件，包括浏览器的系统弹窗 | [modules/win.md](modules/win.md) | 优先 Python UIA 单控件修复；无需影刀元素库。按已验证的应用与动作方式使用 |
| 混合：同一流程含 Web 和 Windows 步骤 | 按失败调用逐一选择；确实涉及两侧时分别加载 | Electron/WebView 也按实际操作对象分流，不默认先读 Windows |
| 未知：源码、实际块类型、对象来源缺失或证据冲突 | 先补证据，暂不进入平台修复 | 不猜平台、不改业务流程、不默认采集影刀窗口 |

先记录平台选择依据和业务目标，再分析单元素/相似元素组、父子引用及可修复性。模块选中不等于可以修复：Windows 须核实实际后端和部署条件，如实报告未实现的步骤；不能因该模块暂不支持而改用 Web 修复器。这里的自动选择由加载 Skill 的 Agent 执行，不是 CLI 常驻监控或已实现的通用程序类型分析器。

`explore/build/verify/maintain/release` 是共用工作阶段，平台具体动作由模块规定。更新历史保存在 [UPDATE-RECORD.md](UPDATE-RECORD.md)，不是运行时必读材料。现有 `references/`、`scripts/` 和 `templates/` 路径保留兼容。

## 先读这些规则

1. 本地影刀操作前先运行 `xbot-cli status`；企业开放 API 查询先读 `xbot-cli platform auth status` 和 `guide platform`，不要求本机客户端登录。不确定命令时用 `xbot-cli`、`xbot-cli <group>`、`xbot-cli help <path>` 逐层发现，再按需读 `schema <path>` 或 `guide <topic>`，不要猜 flag。裸根命令和纯分组显示简短帮助，退出 0，不调用后端。独立 UIA 快照不依赖 Studio，不能以影刀 status 代替目标窗口身份校验。
2. 解析业务结果时显式加 `--json`：成功读 stdout 的 `ok/data`，失败读 stderr 的 `error/type/code/hint/retryable` 和可能的 `data`。默认业务输出在终端为文本、管道为 JSON；帮助和 guide 默认始终为文本，schema 默认始终为 JSON。失败可能已产生部分修改，不只按自然语言 message 分支，不盲目整批重试。
3. 修改前先读现状，最小修改，不直接编辑影刀应用目录文件。Studio 工作显式指定 `--expect-app <id>`，防止 GUI 切换后操作另一应用；它是断言，不负责切换应用。
4. 副作用命令可先用 `--dry-run`；它只做离线输入校验与预览，未验证当前应用、目标流程、权限和后端接受性，不等同运行验收。
5. `app publish`、`app recycle`、`app open --force` 必须展示动作并取得用户明确同意后，才能追加 `--yes`。绝不默认携带 `--yes`。
6. `flow check` 仅静态检查；`flow verify` 会实际运行。超时保留 `run_id`，用 `flow wait` 继续等待，不启动第二份流程。开发运行完成不自动发布；未实际运行的路径不得声称已验证。
7. 遵守当前工作区的 `AGENTS.md`、用户授权范围和项目实际提供的开发规范。不猜测团队规则，不依赖个人或团队专用服务。用户已有明确授权时不重复索要相同确认；授权不明确时先展示具体操作及影响。
8. 创建或维护自动化时，先明确输入、输出、成功条件、失败条件、允许的副作用和需要确认的动作；可从 [templates/task-spec.md](templates/task-spec.md) 开始。
9. 快照 ref 属于特定平台、后端、窗口或页面及采集批次。界面变化后重新采集；不能编造、跨上下文复用或把定位表达式当作 ref。临时句柄和 RuntimeId 不是长期业务身份。
10. 动作结果与后续观测分别记录。已生效但快照失败时只补观测；超时或连接中断先核对结果，不自动重放可能已生效的动作，不因旧定位异常就换新定位再点一次。
11. 提交、删除、发布、付款、发消息和账户修改必须在执行前说明影响并取得用户授权。已有明确授权无需重复索要。Web 动作细则见对应模块。
12. 完成后区分“已探索”“已写入”“静态通过”“运行通过”“业务通过”和“未验证”。影刀流程报告包含 `app_id`、`flow_id`、`run_id` 或 `task_id` 及关键证据；独立 Windows 观察记录 `snapshot_id`、后端和窗口作用域。
13. 流程报错先确定根因和失败调用点，按平台识别规则选择模块，再判断可修复性；可新建复用测试子流程隔离验证。只有证据充分且测试通过才最小修改，保留有效旧、新定位及各自子定位。先分析原用途是单元素还是集合及父子依赖，不按失效定位的当前匹配数倒推。不可修复或不确定时不改业务流程，仍交付原因、尝试、建议和报告。平台的证据及验收门槛见对应模块。
14. 未收到用户推送要求时，不提交、不推送、不发布，也不自增 Skill 版本号。本定制 Skill 版本标记保持 `v10`；交付日期和测试批次单独记录。

## 默认生命周期

创建或维护流程前，读取 [项目规范](references/project-conventions.md)：元素库名称采用 `平台名_业务含义_按钮/输入框/标志`，流程按“配置、业务、测试、主流程”组织。编码版变量默认使用英文 `snake_case`，保留既有对外参数契约。先确认平台支持的定位及持久化方式，再选择流程类型。

```text
业务目标
  -> 任务契约
  -> status/doctor
  -> 按实际操作对象识别平台，加载对应模块并确认能力
  -> 页面或窗口探索与快照
  -> 目标契约、定位、状态和兼容分支记录
  -> 选择 web / CodeFlow / VisualFlow
  -> dry-run
  -> 实际写入与回读
  -> flow check
  -> flow verify 与业务结果验收
  -> 保存证据和报告
```

## 结果报告

完成后区分以下状态，不要混为“已完成”：

- 已探索：页面、元素和业务路径已读取。
- 已写入：CLI/MCP 返回写入成功。
- 静态通过：`flow check` 返回无静态错误。
- 运行通过：`flow verify` 或 `flow wait` 返回成功。
- 业务通过：页面或 Out 满足任务契约中的成功条件。
- 未验证：客户端、登录态、权限或真实运行尚未确认。

报告包含适用的应用/流程/运行标识或独立快照标识、输入摘要、实际输出、错误和关键证据。不要只报告命令退出码。

## 常用工作流

### 查看上下文

```bash
xbot-cli status --json
xbot-cli app open <app-id> --json
xbot-cli flow list --expect-app <app-id> --json
xbot-cli flow get <flow-id> --expect-app <app-id> --json
```

### 开发 CodeFlow

```bash
xbot-cli flow create hello --kind code --expect-app <app-id> --json
xbot-cli flow params set hello --operations @params.json --expect-app <app-id> --json
xbot-cli flow code write hello --file hello.py --expect-app <app-id> --json
xbot-cli flow check hello --expect-app <app-id> --json
xbot-cli flow verify hello --timeout 600 --expect-app <app-id> --json
```

维护已有 CodeFlow 时，先 `flow get`，再用 `flow code edit` 做最小精确替换。

### 开发 VisualFlow

```bash
xbot-cli flow create report --kind visual --expect-app <app-id> --json
xbot-cli flow visual edit report --file ops.json --expect-app <app-id> --json
xbot-cli flow visual fill report --fills @fills.json --expect-app <app-id> --json
xbot-cli flow verify report --timeout 600 --expect-app <app-id> --json
xbot-cli flow visual snapshots report --expect-app <app-id> --json
```

## Reference 强触发索引

命中任一条件时，在下一步前读取对应 reference；同一文件只读一次。本地新增规范以本目录文件为准；`xbot-cli guide <topic>` 只提供 CLI 已内置的指南，不保证包含这些定制内容。

| 触发条件 | 必读 |
|---|---|
| 查询帮助、选择输出格式、解析 stdout/stderr、退出码、`--dry-run`、`--yes`、schema、上下文或原始工具 | [references/cli-contract.md](references/cli-contract.md) |
| 企业开放 API、调度执行记录、jobUuid/taskUuid/scheduleUuid、云端运行日志或运行数据 | [references/platform.md](references/platform.md)，平台命令不要求本机客户端已登录 |
| 新建或维护流程、流程分组、元素库命名、编码变量命名 | [references/project-conventions.md](references/project-conventions.md) |
| 新建/修改/调试 CodeFlow、声明 In/Out、安装 pip 依赖 | [references/codeflow.md](references/codeflow.md) |
| 新建/修改/调试 VisualFlow、块字段、变量或快照 | [references/visualflow.md](references/visualflow.md) |
| 元素报错、平台未确定、混合流程、Electron/WebView 或系统弹窗 | 先读 [references/automation-routing.md](references/automation-routing.md)，按失败步骤选择模块 |
| 已确认 Web 的探索、构建、XPath 生成/修复、相似元素、验收或网页动作风险 | [modules/web.md](modules/web.md)，再按其中索引读取对应 reference |
| 已确认 Windows 的业务窗口、原生控件、Python UIA、Windows-MCP、Umbriel、win32 snapshot | [modules/win.md](modules/win.md)，再读 Python 运行与修复链路 |
| 维护既有应用、编辑模式冲突、运行失败、同步、发布或回收 | [references/recovery.md](references/recovery.md) |

原始通道使用 `tool studio list/schema/call`、`tool browser list/schema/call`；已发布应用使用 `task run/stop/status/logs`。`browser` 是 `web` 的兼容别名；旧 `code`、`visual`、`mcp` 入口只用于兼容。影刀官方细节通过 `xbot-cli wiki list/search/read` 查询；不要用记忆替代当前契约。
