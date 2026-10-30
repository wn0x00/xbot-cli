# xbot-cli 0.9.1 更新说明

版本：0.9.1。日期：2026-10-30。

本次主要新增网页操作返回影刀 Python、按 ref 生成 XPath，并修复非默认影刀安装目录识别问题。独立预览版能力已整合到正式 `xbot-cli`，无需使用预览版命令。

## 1. Web 操作同时返回影刀 Python

网页打开、附着、导航和支持的元素操作，在保留原操作结果及 snapshot 的同时，新增对应的影刀 Python 代码。文本标题统一为：

```text
影刀 Python：
```

普通 DOM 使用 `web_page.find_by_xpath(...)` 配合元素操作方法；iframe 按外层到内层构建 `Selector.xpath.segments`，通过 `web_page.find(selector, timeout=20)` 查找后操作。输入、勾选和下拉选择保留对应 SDK 参数，敏感输入参数化。

JSON 通过 `data.python` 返回代码、依赖、定位依据、警告和验证状态。`--no-python` 可关闭代码生成；`--no-snapshot` 仅关闭后续快照，不关闭 Python。生成失败不否定已经成功的网页动作，也不会重放动作。

快照仍采用完整元素树，优化父子层级和分支连线，保留 ref、名称、状态、属性、iframe 层级及可观察的 ShadowRoot 信息。默认不折叠、不截断；筛选或分页必须显式指定。

## 2. 新增 `web xpath`

```powershell
# 使用当前页面最新快照中的实际 ref，以下编号仅为示意
xbot-cli web xpath e12
xbot-cli web xpath e12 e13
xbot-cli web xpath e12 e13 --json
```

- 单个 ref 返回至多 5 条候选 XPath，文本每行一条，优先使用语义属性、文本及区域锚点。
- 多个 ref 返回覆盖所选同类元素的相似元素 XPath，不是把多个单元素 XPath 简单拼成并集。
- 元素类型不一致、上下文不兼容或无法找到可靠共同规则时，返回明确提示。
- iframe 每条候选返回一个按层级排序的 XPath 列表，供 `Selector.xpath.segments` 使用，不能当作一条普通 XPath。
- open ShadowRoot 可生成影刀专用 XPath，明确标注“影刀语法，请使用影刀验证”；不以普通浏览器 XPath 验证整条影刀表达式。
- **目标或任一外层上下文经过 shadow-root(closed) 时，显著提示影刀无法使用该元素，不生成 XPath 或等价 Python。** 无法观察到的 closed 内部节点不保证出现在快照中。
- 歧义候选标记为 `needs_review` 并保留匹配数，不能直接当作唯一定位用于点击。当前唯一匹配也不代表永久稳定或业务身份已验证。

此命令只生成定位提议，不点击、不保存元素库，也不自动修改已有流程。

## 3. 修复影刀安装环境识别

修复部分电脑因未识别实际安装目录而错误使用默认后端路径、导致命令无法使用的问题。自动发现运行中客户端、注册表安装信息及有限的已知目录，支持非默认盘符和版本子目录。

`XBOT_SHELL_CLI`、`XBOT_LAUNCHER` 仍可显式覆盖，配置错误时不会偷偷切换其他路径。`doctor` 在失败时提供实际后端路径、发现来源和针对性修复建议，区分路径不存在、启动失败与客户端状态问题。

## 使用与兼容性

直接使用 `xbot-cli` 命令。npm 启动器与直接运行程序均使用调用端的页面绑定，修复部分情况下连续命令找不到快照 ref 的问题；切换活动标签页不会让旧 ref 操作悄悄转向另一页。

正式版会话缓存位于 `%LOCALAPPDATA%\xbot-cli\web-sessions`，不自动复用独立预览版的旧 ref。升级后先用 `web active` 附着或 `web open` 打开页面，重新取得快照，再使用实际 ref。

页面键盘、任意脚本等尚未提供等价 Python；纯 ShadowRoot 的部分操作仍使用明确标注的 JavaScript 合成事件适配器。候选表达式与代码尚不能等同于影刀运行验收，`runtime_verified` 保持 `false`，请在影刀中测试实际效果。随包 Skill 保持 v10，并同步 Web 与环境诊断说明。

## 安装与升级

```powershell
npm install -g '@guanzhu.me/xbot-cli@0.9.1'
xbot-cli version
xbot-cli doctor --json
```

GitHub Release 提供以下附件，Windows ZIP 与 npm 包内程序来自同一份验收构建：

- `guanzhu.me-xbot-cli-0.9.1.tgz`：npm 安装包。
- `xbot-cli-0.9.1-windows-x64.zip`：Windows x64 可执行程序、README 和许可证。
- `SHA256SUMS.txt`：两个安装包的 SHA-256 校验值。

## 本机验证记录

- Rust 自动测试：175 项通过，2 项默认忽略；另单独运行 iframe Python 3.7 语法与 API 桩测试，通过。
- npm 启动器测试 36 项、Skill 测试 16 项通过；格式、Clippy 和本地打包安装检查通过。
- 实际 Edge/Chrome Bridge 回归 104 项通过，覆盖普通 DOM、嵌套 iframe、跨域、open/closed shadow、相似元素 XPath、页面绑定及动作不重放。
- npm 启动器浏览器回归 4 项通过，确认连续命令保留页面绑定，以及输出标题为“影刀 Python：”。
- 百度独立测试页完成打开、填写快照中的 e38、回读值与生成 XPath，未提交搜索；测试标签页已关闭。
- `doctor` 无需配置覆盖即识别本机非默认安装目录 `D:\Program Files\ShadowBot`，后端检查通过。

以上是 CLI、浏览器和测试桩验证，不是将生成代码放入影刀流程运行的验收；iframe Selector 与 open ShadowRoot 影刀语法仍需在影刀中验证。
