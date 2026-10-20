# VisualFlow 开发与维护

## 工作流

Studio 命令建议追加 `--expect-app <app-id>` 核对当前应用；它不会打开或切换应用。

```bash
xbot-cli status
xbot-cli app open <app-id>
xbot-cli flow list
xbot-cli flow get main                       # 自动读取 VisualFlow 详细块
xbot-cli flow params list main
xbot-cli flow visual variables main
xbot-cli wiki read wiki/guides/visualflow wiki/guides/visualflow/value-types
xbot-cli flow visual edit main --file ops.json
xbot-cli flow visual fill main --fills @fills.json
xbot-cli flow verify main --timeout 600
xbot-cli flow visual snapshots main               # 需要块级运行证据时
```

新建流程：

```bash
xbot-cli flow create report --kind visual
xbot-cli flow visual list report --detail
```

插入操作 `ops.json`：

```json
{
  "op":"insert",
  "block_index":-1,
  "blocks":[{"prototype_name":"programing.snippet","comment":"print test message"}]
}
```

`fills.json` 可直接是 fills 数组：

```json
[
  {"block_id":"使用 insert 返回的真实 id","inputs":{"snippet":"print('hi')"}}
]
```

## 规则

- 块原型来自本地 `block_catalog.md` / `block_detail.md` 或 `wiki.read`，不要编造。
- `flow visual edit` 只插/删实例，字段用 `flow visual fill`。
- 长多行 snippet 用普通字符串；空字符串才用 `{"text":""}`。
- 数组、对象、变量、表达式、选择器按官方 value-types 使用语义包装。
- `fill_errors` 是部分成功：只重试失败 block_id。
- 禁用：`flow visual disable <block-id...> --flow main`；启用：`flow visual enable <block-id...> --flow main`。省略 `--flow` 时按当前上下文解析流程。
- 长流程分页：`flow visual list main --detail --offset 0 --limit 20`。
- `flow visual variables` 是静态作用域，`flow visual snapshots` 是运行值，不要混淆。
- 流程 In/Out 使用 `flow params list/set`；Visual Out 的产出块输出须与参数同名。

## 流程组织与 XPath

按 [项目规范](project-conventions.md) 建立配置、业务、测试分组，主流程位于根级；当前接口不支持的分组写入明确记录为未执行，不伪造参数。

网页元素优先使用“获取元素对象（web）”指令（`web.element.get_element`）搭配 XPath，取得动态元素对象后交给后续点击、输入或读取指令。不要为一个可直接 XPath 定位的元素先进行影刀元素捕获。

填充前读取 `wiki/blocks/web.element.get_element` 和目标块最新 form，核对：

| 字段 | 填写规则 |
|---|---|
| `select_type` | `xpath_selector` |
| `xpath_selector` | 当前页面已验证的 XPath 文本 |
| `is_related_parent` / `parent` | 在行、卡片或弹窗内查找时限定已取得的父元素，具体行为以当前块和XPath能力为准 |
| 输出 | 使用当前 form 返回的输出字段，将结果保存为元素对象变量 |

定位方式互斥，不同时填写 `selector`、`css_selector` 等其他方式字段。后续操作使用 `{"variable":"实际元素对象变量名"}`；不要把 XPath 包装成 `{"selector":"XPath文本"}`，也不要把动态对象变量写成普通字符串。下游块是否接受动态元素对象以其 form 为准。

新旧版本兼容要通过有界的查找/存在性判断分支选择唯一目标；不能把两个点击块依次执行，也不能用“忽略异常后点击另一版本”替代消歧。分支、共存情况和验收要求见 [维护指南](web-maintenance.md)。确需元素库的例外遵循 [项目命名规范](project-conventions.md)。

## 验收

`flow check` 检查整个应用的静态诊断，传入流程时先核验目标（`scope: app`），不启动流程；`flow verify` 检查后实际运行。需要定位块级问题时，再读 `flow visual snapshots` 和错误中的 `log_tail`。运行超时后用 `flow wait <run-id> --timeout 600` 继续等待，不重新启动。已发布指令的参数、名称和调用契约不得擅自改变。
