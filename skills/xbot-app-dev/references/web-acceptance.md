# 网页自动化验收与证据

当需要证明网页自动化“能用”“修复完成”或“可以交付”时读取本文。

## 验收层级

按下面顺序报告，不能跳级：

1. **已探索**：读取了页面、元素和业务路径。
2. **已写入**：CLI/MCP 返回了修改成功。
3. **静态通过**：`flow check` 返回空的 `static_errors`。
4. **运行通过**：`flow verify` 或同一次运行的 `flow wait` 成功。
5. **业务通过**：页面、Out 或任务结果满足任务契约中的成功条件。
6. **未验证**：客户端、登录态、权限或真实后端尚未确认。

`--dry-run` 只能证明输入可解析，不能计入静态通过或运行通过。

## 最小测试集

根据业务选择相关用例，至少覆盖：

- 正常输入和正常结果
- 空结果、无匹配或可选字段缺失
- 非法输入
- 登录过期、权限不足或验证码阻断
- 页面元素变化或重复提交风险
- 网络中断、超时和后端返回异常

元素失效修复增加 [新旧兼容矩阵](web-maintenance.md)：旧版单独存在、新版单独存在、新旧指向同一元素、隐藏副本共存、不同可见目标共存，以及均不存在。分别验证页面条件、各候选匹配数、目标身份和动作次数；歧义未解决时不应发生动作。无法取得的页面版本标为未验证，不用当前单页通过代替兼容性证明。

回归流程放在“测试”分组，名称为 `测试_动作名`，调用“业务”分组对应动作；配置和主流程的组织规则见 [项目规范](project-conventions.md)。本次只修改 Skill 文档时，链接与模板检查不能声称这些应用运行用例已经通过。

从 [templates/acceptance-case.json](../templates/acceptance-case.json) 创建用例。对不可逆动作使用测试账号、测试数据或用户明确提供的安全环境。

## 网页 CLI 的验收

每个关键动作都要有观察结果：

```text
动作 -> 新快照或 URL/title/关键文本 -> 业务断言 -> 保存证据
```

可以使用 `web snapshot` 或只读 `web eval` 获取证据。`web eval` 必须返回可序列化的只读值，不用任意脚本代替点击、填写、提交或下载工具。

如果动作返回 `applied: true` 且有 `warning`，动作仍计为已应用；单独补取快照后再判断业务结果，不重复动作。

## Flow 的验收

```powershell
xbot-cli flow check <flow-id> --expect-app <app-id> --json
xbot-cli flow verify <flow-id> --timeout 600 --expect-app <app-id> --json
```

超时后：

```powershell
xbot-cli flow wait <run-id> --timeout 600 --expect-app <app-id> --json
```

已发布应用使用 `task_id`，不能把 Studio 的 `run_id` 传给 `task status` 或 `task logs`。

## 证据报告

建议每个用例保存：

```text
case_id:
app_id:
flow_id:
run_id/task_id:
输入摘要:
关键动作:
期望:
实际:
快照或日志证据:
结论: business_pass / runtime_pass / static_pass / not_verified
```

输入摘要应隐藏密码、Token、Cookie、验证码和无关个人数据。报告只保留能证明结论的最小证据。
