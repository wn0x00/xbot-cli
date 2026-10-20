# 浏览器自动化：xbot web

`xbot web` 提供类似 Playwright CLI 的命令交互，通过影刀内置 browser-use-mcp 执行。`xbot-cli` 与 `xbot` 等价；`browser` 是 `web` 的兼容别名。

前提：已打开 Studio 应用，并有安装影刀扩展的浏览器；或使用内置影刀浏览器。CLI 自动补充后端要求的 `explanation`。这里使用影刀当前浏览器上下文，没有独立的命名会话。

## 从快照到操作

```bash
xbot web open baidu.com --json
xbot web snapshot --json
# 用当前快照中的实际 ref 替换 e4、e7
xbot web fill e4 "影刀 RPA" --json
xbot web press Enter --ref e4 --json
xbot web click e7 --json
```

裸域名会补全为 `https://`。`open` 不带 URL 时打开 `about:blank`。只在用户指定浏览器类型时传 `--mode`（别名 `--browser`），例如 `--mode cef`。需要用户已经打开的活动页时用 `web active`，它的别名是 `web attach`；在受控页内跳转用 `web goto <url>`。

元素 ref 必须来自当前页面的快照，不能编造或复用其他页面的编号。页面变化后依据返回的新快照选择目标；保留后端的快照元数据，快照可能只包含增量变化，不能默认是完整页面。这里只接受快照 ref，不把 CSS 或 Playwright locator 表达式当作 ref。

排查填写失败时，读取真正的 `error.message`，不要把 `selectors have been stored` 当成填写成功；它只说明后端暂存了选择器。先获取当前快照，再选择可编辑元素（例如 `textbox`）的 ref。点击后若只得到 `generic` 等很少的快照内容，单独获取 `web snapshot` 检查页面状态，不重复点击。

## 常用命令

下列 `e4` 都是语法占位示例。执行前替换为实际 ref；解析业务结果时显式加 `--json`。

| 命令 | 用途 |
|---|---|
| `xbot web open [url]` | 打开页面并建立上下文 |
| `xbot web active` / `attach` | 附着当前活动页 |
| `xbot web snapshot` | 获取页面快照 |
| `xbot web click e4` | 左键点击，可用 `--button right` 或 `middle` |
| `xbot web dblclick e4` | 双击 |
| `xbot web fill e4 "内容"` | 清空并填写输入框 |
| `xbot web type "内容"` | 向当前焦点输入文字 |
| `xbot web press Enter` | 在当前页面按键 |
| `xbot web press Control+A --ref e4` | 对指定元素按键 |
| `xbot web hover e4` | 悬停 |
| `xbot web check e4` / `uncheck e4` | 勾选或取消勾选 |
| `xbot web select e4 value1 value2` | 按 option value 选择一个或多个选项 |
| `xbot web goto https://example.com` | 当前受控页跳转 |
| `xbot web reload` | 刷新 |
| `xbot web go-back` / `go-forward` | 后退或前进，别名 `back` / `forward` |
| `xbot web close` | 关闭当前受控页 |

日期选择控件应展开面板后使用专用日期工具，不用 `fill` 直接填写；文件上传、下载使用专用工具。

## 输出与恢复

打开、附着、导航和常用交互成功后会自动获取快照。需要减少输出时可加 `--no-snapshot`，之后再主动调用 `web snapshot`。`close` 不获取关闭后的快照；只读 `eval` 和底层兼容调用也不承诺自动快照。

动作已经成功，但后续快照失败时，CLI 仍返回成功（`ok: true`、退出 0），并带有 warning 和 `applied: true`。先读取警告，再单独执行 `web snapshot`，不要为了获得快照而重复点击、输入或提交。操作本身超时或响应中断时也不要自动重试，先确认页面状态。

```bash
xbot web click e4 --no-snapshot --json
xbot web snapshot --format text
xbot web snapshot --output after-click.txt --json
```

`snapshot --output <file>`（别名 `--filename`）保存快照文本，不覆盖已有文件。已有文件冲突时选择新路径。普通输出仍遵守终端文本、管道 JSON 的规则；需要文本快照时显式 `--format text`。

## 选择器与只读求值

探索阶段的 `web` 命令仍按当前快照 ref 操作；它们不接受 XPath 作为 ref。交付的网页 Flow 优先直接 XPath 定位，不因探索动作产生了暂存 selector 就默认持久化进元素库。

确需准备元素库资产时，元素操作可传 `--name "百度_搜索_按钮"` 指定符合 [项目命名规范](project-conventions.md) 的暂存名称。省略时 CLI 自动生成唯一中文临时名；操作会将选择器暂存在后端，调用 `web_review` 前不会写入项目元素库。`press` 只有指定 `--ref` 时才接受 `--name`。

```bash
xbot web click e4 --name "百度_搜索_按钮" --json
xbot web eval "return await activePage.title();" --json
```

`web eval` 是只读 Playwright 异步函数体，注入 `activePage`；需要结果时必须 `return` 可序列化值。顶层不能直接访问 `document`，页面 DOM 读取放在 `activePage.evaluate(...)` 中。只在快照或专用工具不足以获得所需事实时使用，不用求值绕过交互命令或进行写入。

## XPath 优先与验证

- 从当前页面的真实 DOM 提取稳定属性、精确业务文本和父区域关系，验证 XPath 后再写入流程；优先业务属性、稳定 id 或名称，避免版本化样式类、绝对层级路径和无业务依据的序号。
- 快照不足以确定 XPath 时，用只读 `web eval` 检查 DOM、匹配数量、可见性和所属区域。记录页面条件、frame/父区域、候选版本和验证时间；未观察到的旧页面只能标为未验证。
- 点击或输入前，在正确的页面/弹窗/行作用域内确认唯一、可见、可操作的业务目标。多个命中时先缩小作用域；不追加 `[1]`、不直接取列表首项来掩盖歧义。
- 等待和存在性判断使用表示业务状态的标志；页面跳转后重新获取元素对象，不跨页面复用旧对象。等待与兼容分支共用有界时间预算。
- iframe、Shadow DOM 先查 SDK 的上下文能力；普通 XPath 不会自动穿过它们。不支持直接 XPath 的场景可保留经过验证的工程 selector，并按项目规范命名。
- `web_build_selector_by_xpath` 构建的是元素库候选，不是直接 XPath 执行入口；不为了实现 XPath 优先而滥用它。确需建库时遵守该工具 schema 的使用条件；已有 ref 的捕获方式也只限确需建库的场景。

VisualFlow 通过“获取元素对象（web）”的 XPath 模式得到对象后再操作；CodeFlow 使用 `find_by_xpath` / `find_all_by_xpath`。新旧版本共存的处理见 [维护指南](web-maintenance.md)。

## 专用工具与开发

```bash
xbot tool browser list
xbot tool browser schema web_download
xbot tool browser call web_download @args.json
```

选择器构建、表格、上传、下载、日期和验证码使用专用工具，先查询当前 schema，不猜参数。`web act <json>`、`browser act <json>` 保留为隐藏兼容入口，常用交互优先上表命令。

本接口不提供 Playwright CLI 的 sessions、tabs、storage、trace 或独立 screenshot 命令。后端实际可用的其他能力通过 `tool browser list/schema` 发现。

写 Web CodeFlow 前查询 `wiki/guides/web/codeflow` 和 SDK 签名。XPath 必须来自当前页面验证；弹窗内 XPath 带弹窗作用域；所有轮询有界。不要向日志或汇报复制 Cookie、Token、密码、验证码或无关的完整页面内容。
