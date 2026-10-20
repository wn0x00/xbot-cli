---
name: xbot-app-dev
description: 使用 xbot-cli 探索、开发、验收、维护或发布影刀 ShadowBot 应用和网页自动化。适用于 xbot web、CodeFlow、VisualFlow、浏览器取数及运行故障恢复。
metadata:
  version: "9"
---

# 影刀应用与指令开发

通过 `xbot` 或 `xbot-cli` 调用影刀内置 shell-cli、studio-mcp 和 browser-use-mcp，两个命令名等价，零注入、零反射。不要使用旧的 `xbot_agent` / `python -m xbot_cli` 工作流。

本 Skill 随 npm 包 `@guanzhu.me/xbot-cli` 分发，适用于 Windows x64 和 Node.js 22+。使用 `xbot install --skills` 安装到当前工作区，或加 `--global` 安装到用户的 `.agents/skills`。运行影刀操作前需要客户端已启动并登录；帮助、schema 和 guide 可离线读取。

这个 Skill 的目标是把业务需求变成可重复、可验收、可维护的自动化流程。网页 CLI 适合探索和短流程；需要参数、输入输出、长期运行或维护时，优先沉淀为 CodeFlow，只有确实适合块编排时才使用 VisualFlow。

## 工作模式

先判断当前任务属于哪一种模式，再读取对应参考文档：

| 模式 | 适用场景 | 参考文档 |
|---|---|---|
| `explore` | 探索页面、登录态、元素、分页和业务路径 | [web-lifecycle.md](references/web-lifecycle.md)、[browser.md](references/browser.md) |
| `build` | 根据已确认路径创建或修改自动化流程 | [web-lifecycle.md](references/web-lifecycle.md)、[codeflow.md](references/codeflow.md) 或 [visualflow.md](references/visualflow.md) |
| `verify` | 检查静态诊断、真实运行、输出和页面结果 | [web-acceptance.md](references/web-acceptance.md) |
| `maintain` | 处理页面变化、选择器失效、运行失败和超时 | [web-maintenance.md](references/web-maintenance.md)、[recovery.md](references/recovery.md) |
| `release` | 保存、同步、发布、回收或运行已发布应用 | [recovery.md](references/recovery.md)、[cli-contract.md](references/cli-contract.md) |

## 先读这些规则

1. 操作前先运行 `xbot-cli status`；不确定命令时用 `xbot-cli`、`xbot-cli <group>`、`xbot-cli help <path>` 逐层发现，再按需读 `schema <path>` 或 `guide <topic>`，不要猜 flag。裸根命令和纯分组显示简短帮助，退出 0，不调用后端。
2. 解析业务结果时显式加 `--json`：成功读 stdout 的 `ok/data`，失败读 stderr 的 `error/type/code/hint/retryable` 和可能的 `data`。默认业务输出在终端为文本、管道为 JSON；帮助和 guide 默认始终为文本，schema 默认始终为 JSON。失败可能已产生部分修改，不只按自然语言 message 分支，不盲目整批重试。
3. 修改前先读现状，最小修改，不直接编辑影刀应用目录文件。Studio 工作显式指定 `--expect-app <id>`，防止 GUI 切换后操作另一应用；它是断言，不负责切换应用。
4. 副作用命令可先用 `--dry-run`；它只做离线输入校验与预览，未验证当前应用、目标流程、权限和后端接受性，不等同运行验收。
5. `app publish`、`app recycle`、`app open --force` 必须展示动作并取得用户明确同意后，才能追加 `--yes`。绝不默认携带 `--yes`。
6. `flow check` 仅静态检查；`flow verify` 会实际运行。超时保留 `run_id`，用 `flow wait` 继续等待，不启动第二份流程。开发运行完成不自动发布；未实际运行的路径不得声称已验证。
7. 遵守当前工作区的 `AGENTS.md`、用户授权范围和项目实际提供的开发规范。不猜测团队规则，不依赖个人或团队专用服务。用户已有明确授权时不重复索要相同确认；授权不明确时先展示具体操作及影响。
8. 创建或维护网页自动化时，先明确输入、输出、成功条件、失败条件、允许的副作用和需要确认的动作；可从 [templates/task-spec.md](templates/task-spec.md) 开始。
9. 网页 ref 只在当前快照中有效。页面变化、导航、提交或弹窗变化后重新 snapshot；不能编造、跨页面复用或把 CSS/XPath 当作 ref。
10. 动作成功但快照失败时，保留 `applied: true`，只补执行 `web snapshot`；超时或连接中断时先确认页面状态，不自动重放可能已生效的动作。
11. 只读、可逆和不可逆动作按 [web-risk.md](references/web-risk.md) 处理。提交、删除、发布、付款、发消息和账户修改必须在执行前说明影响并取得用户授权。
12. 完成后区分“已探索”“已写入”“静态通过”“运行通过”“业务通过”和“未验证”，报告至少包含 `app_id`、`flow_id`、`run_id` 或 `task_id` 及关键证据。

## 默认生命周期

创建或维护流程前，读取 [项目规范](references/project-conventions.md)：元素库名称采用 `平台名_业务含义_按钮/输入框/标志`，流程按“配置、业务、测试、主流程”组织。编码版变量默认使用英文 `snake_case`，保留既有对外参数契约。网页 Flow 优先使用已验证的 XPath；VisualFlow 用“获取元素对象（web）”搭配 XPath，CodeFlow 用 XPath API。元素失效修复须兼容有效的新旧定位，尤其检查新旧元素同时存在时的歧义，见 [维护指南](references/web-maintenance.md)。

```text
业务目标
  -> 任务契约
  -> status/doctor
  -> 页面探索与快照
  -> XPath、页面状态和兼容分支记录
  -> 选择 web / CodeFlow / VisualFlow
  -> dry-run
  -> 实际写入与回读
  -> flow check
  -> flow verify 或业务级网页验收
  -> 保存证据和报告
```

网页 CLI 适合探索、短流程和一次性取数；需要重复执行、参数化、明确输入输出或长期维护时，优先沉淀为 CodeFlow。只有已有流程或业务需求适合块编排时才使用 VisualFlow。

长期资产不要只保存 `e4` 这类临时 ref。记录经过页面验证的 XPath、作用域、有效的新旧版本及匹配数，参考 [templates/locator-registry.json](templates/locator-registry.json)。该文件是说明模板，不是执行器。只有确需使用项目元素库时才持久化命名选择器，并先用 `xbot tool browser schema web_review` 确认当前参数。

## 结果报告

完成后区分以下状态，不要混为“已完成”：

- 已探索：页面、元素和业务路径已读取。
- 已写入：CLI/MCP 返回写入成功。
- 静态通过：`flow check` 返回无静态错误。
- 运行通过：`flow verify` 或 `flow wait` 返回成功。
- 业务通过：页面或 Out 满足任务契约中的成功条件。
- 未验证：客户端、登录态、权限或真实运行尚未确认。

报告至少包含 `app_id`、`flow_id`、`run_id` 或 `task_id`、输入摘要、实际输出、错误和关键快照证据。不要只报告命令退出码。

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

### 浏览器交互

先阅读 [references/browser.md](references/browser.md)。使用 `web` 的位置参数操作页面，无需为常用动作编写 JSON：

```bash
xbot web open https://example.com --json
xbot web snapshot --json
# e4 是语法示例，必须替换为当前快照里的实际 ref
xbot web fill e4 "搜索内容" --json
xbot web press Enter --ref e4 --json
```

打开、附着、导航和常用交互后默认返回新快照；`--no-snapshot` 跳过，`close` 不取快照。保留快照元数据和真实 ref；增量快照不能当成完整页面。动作成功但自动快照失败时，读取 warning 和 `applied: true`，只补取快照，不重复动作。元素选择器名称会暂存于后端，省略 `--name` 时由 CLI 生成临时名称；只有后续 `web_review` 才将选定的选择器写入项目元素库。

## Reference 强触发索引

命中任一条件时，在下一步前读取对应 reference；同一文件只读一次。本地新增规范以本目录文件为准；`xbot-cli guide <topic>` 只提供 CLI 已内置的指南，不保证包含这些定制内容。

| 触发条件 | 必读 |
|---|---|
| 查询帮助、选择输出格式、解析 stdout/stderr、退出码、`--dry-run`、`--yes`、schema、上下文或原始工具 | [references/cli-contract.md](references/cli-contract.md) |
| 新建或维护流程、流程分组、元素库命名、编码变量命名 | [references/project-conventions.md](references/project-conventions.md) |
| 新建/修改/调试 CodeFlow、声明 In/Out、安装 pip 依赖 | [references/codeflow.md](references/codeflow.md) |
| 新建/修改/调试 VisualFlow、块字段、变量或快照 | [references/visualflow.md](references/visualflow.md) |
| 网页探索、交互、XPath、选择器、上传下载或验证码 | [references/browser.md](references/browser.md) |
| 从业务需求到网页流程实现 | [references/web-lifecycle.md](references/web-lifecycle.md) |
| 失败定位、选择器漂移、重试和最小修复 | [references/web-maintenance.md](references/web-maintenance.md) |
| 验收用例、业务断言和证据报告 | [references/web-acceptance.md](references/web-acceptance.md) |
| 动作副作用和用户确认边界 | [references/web-risk.md](references/web-risk.md) |
| 维护既有应用、编辑模式冲突、运行失败、同步、发布或回收 | [references/recovery.md](references/recovery.md) |

原始通道使用 `tool studio list/schema/call`、`tool browser list/schema/call`；已发布应用使用 `task run/stop/status/logs`。`browser` 是 `web` 的兼容别名；旧 `code`、`visual`、`mcp` 入口只用于兼容。影刀官方细节通过 `xbot-cli wiki list/search/read` 查询；不要用记忆替代当前契约。
