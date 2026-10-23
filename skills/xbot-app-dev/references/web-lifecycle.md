# 网页自动化生命周期

当用户要求“自动开发网页流程”“把网页操作做成影刀应用”或“把一次操作沉淀为可重复流程”时读取本文。

## 1. 先写任务契约

在执行网页动作前明确：

- 目标业务和起始页面
- 输入字段及其来源
- 关键页面和状态转移
- 输出字段及类型
- 成功条件和失败条件
- 允许的副作用
- 需要用户确认的动作
- 验收用例和证据

使用 `templates/task-spec.md` 保存契约。没有明确成功条件时，只能探索，不能声称开发完成。

## 2. 运行前检查

```powershell
xbot-cli doctor --json
xbot-cli status --json
```

真实网页操作需要影刀客户端已启动并登录、Studio MCP 可用，并且浏览器扩展或内置影刀浏览器可用。检查失败时先报告通道状态；帮助、schema、guide 和 `--dry-run` 仍可离线使用。

如果操作依赖 Studio 应用：

```powershell
xbot-cli app open <app-id> --json
xbot-cli flow list --expect-app <app-id> --json
```

`app view` 是只读临时副本，不用于开发和保存。

## 3. 探索页面

优先使用高层网页命令：

```powershell
xbot-cli web active --json
xbot-cli web open <url> --json
xbot-cli web snapshot --format text
```

每次动作后重新查看返回快照或主动运行 `web snapshot`。记录页面 URL、标题、快照类型、关键文本、元素角色和当前 ref。快照可能是增量内容，不能把一次快照当成永久页面模型。

探索阶段只做确认业务路径所需的最小动作。登录、提交、删除、发送、支付等动作先停在确认点；不要为了“看看效果”触发真实副作用。

## 4. 选择实现方式

| 方式 | 默认用途 | 选择理由 |
|---|---|---|
| `xbot-cli web` | 一次性取数、页面核验、短交互和探索 | 反馈快，但依赖当前活动浏览器上下文 |
| CodeFlow | 重复执行、参数化、明确 In/Out、需要长期维护 | 代码和输入输出更容易审查、修改和回归 |
| VisualFlow | 块结构需要业务人员维护，或已有流程已经是 VisualFlow | 与既有应用一致，避免混用模型 |

不要把一串临时 `web` 命令直接当成长期自动化。长期流程需要把业务输入、输出、等待、错误分支和验收条件沉淀到 Flow 中。

创建和组织流程遵循 [项目规范](project-conventions.md)：配置、业务、测试分组与根级主流程。维护已有应用保持当前实现方式，仅调整涉及的流程。

## 5. 选择器和页面状态

临时 ref 只属于当前页面快照。网页 Flow 默认以当前 DOM 验证过的 XPath 实现，不默认捕获元素或写入元素库：VisualFlow 先用“获取元素对象（web）”配合 XPath 取得对象，CodeFlow 直接使用 XPath API，见对应开发指南。

用 [定位记录模板](../templates/locator-registry.json) 记录业务目标、作用域、XPath 候选、页面条件、匹配数和验证时间。模板是说明材料，未填写的 XPath 或未验证的旧版候选不能用于执行。新旧版本并存时按 [维护指南](web-maintenance.md) 消歧，不取第一个匹配。

确需使用元素库时才用 `--name` 准备命名资产，采用 `平台名_业务含义_按钮/输入框/标志`，例如 `百度_搜索_输入框`。先查询 `web_review` schema，再选择需要的 selector 持久化。不要假设自动生成的临时名称或旧 XPath 永远有效。

## 6. 构建和验收

输入复杂时使用文件或 stdin，先 dry-run：

```powershell
xbot-cli flow code write <flow-id> --file main.py --expect-app <app-id> --dry-run --json
xbot-cli flow code write <flow-id> --file main.py --expect-app <app-id> --json
xbot-cli flow get <flow-id> --expect-app <app-id> --json
xbot-cli flow check <flow-id> --expect-app <app-id> --json
xbot-cli flow verify <flow-id> --timeout 600 --expect-app <app-id> --json
```

网页 CLI 流程要用当前快照、URL、标题、关键文本或只读 `web eval` 验证业务结果。只看到动作返回成功，不等于业务目标完成。

完成后保存 `app_id`、`flow_id`、`run_id` 或 `task_id`、输入摘要、输出和关键快照。报告中区分“已写入”“静态通过”“运行通过”和“业务通过”。
