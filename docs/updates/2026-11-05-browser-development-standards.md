# xbot-cli 0.9.2 更新说明

版本：0.9.2。日期：2026-11-05。

本次新增浏览器列表、指定浏览器操作和自动启动能力，并内置指令集、应用开发规范 Skill。浏览器操作继续通过影刀 Chrome Bridge 执行，不切换到独立浏览器自动化引擎。

## 浏览器列表与指定浏览器

```powershell
xbot-cli web browser list
xbot-cli web browser list --json
xbot-cli web browser list --all --json
xbot-cli web open yingdao.com --browser chrome
xbot-cli web active --browser edge --json
xbot-cli web snapshot --browser chrome --json
```

列表通过已安装影刀 DLL 的公开接口查询，文本与 `app list` 一样使用表格。默认显示已安装插件或已注册的自定义浏览器，始终排除 `exePath` 缺失、空值或纯空白的项；`--all` 也不会显示这些无程序路径项。

`installed` 是影刀 DLL 的插件安装判定，不是当前浏览器 profile 的启用检查。自定义浏览器的 `registered` 仅表示注册，不证明插件已启用或已连接。`webSupported` 表示取得兼容管道标识，也不表示连接成功。

全部 Web 操作可使用 `--browser <id>`，以列表返回的唯一 ID 为准，不猜自定义浏览器名称。不同浏览器分别保存页面会话、快照和 ref；后续快照继续绑定实际操作的浏览器、管道和标签页。指定 Chrome 不会因 Chrome 未连接而转向 Edge；显式管道与浏览器不符时直接报错。

```powershell
# e7 必须来自 Chrome 当前页面的最新快照，不能复用 Edge 的 ref。
xbot-cli web click e7 --browser chrome --json
xbot-cli web xpath e7 --browser chrome --json
```

## 自动启动浏览器

`web open --browser <id>` 先尝试连接目标浏览器。连接不可用且目标程序未运行时，仅按 DLL 返回的有效绝对 `exePath` 启动该浏览器的空白页，等待最多 20 秒连接影刀插件，再打开目标 URL 一次。

- 已有该程序路径的进程时不重复启动；插件未连接则等待并报告超时。
- 路径无效、插件未启用或启动失败时不回退其他浏览器。
- 浏览器进程不继承 CLI 的输出句柄，避免 PowerShell 或调用方因捕获输出而持续等待。
- 其他网页动作不自动重启或重放；不指定浏览器、显式设置 `XBOT_WEB_PIPE` 或使用 `cef` 时不自动启动。
- `--dry-run` 仍离线校验和预览，不启动浏览器、不查询运行时 DLL。

## 指令集、应用开发规范 Skill

```powershell
xbot-cli install --skills --json
# 安装到用户 ~/.agents/skills 时使用：
xbot-cli install --skills --global --json
xbot-cli guide development-standards
```

安装命令现在同时提供 `xbot-app-dev` 的 39 个资源和 `xbot-development-standards` 的 13 个资源。新增 Skill 包含规范、参考资料、只读元数据检查脚本、52 项测试、策略及验收报告模板，均嵌入可执行文件，不依赖维护者桌面目录。

两份 Skill 在写入前统一检查文件冲突；内容不同时返回 `SKILL_CONFLICT`，不先写入另一份 Skill。先检查差异，需要覆盖时再显式追加 `--yes`。JSON 新增 `data.skills`，分别返回 `name/path/files`；原 `data.path/files` 仍指 `xbot-app-dev`，保持旧调用兼容。

规范先区分普通应用和指令集：

- 普通应用从主流程进入主运行模块的 `main(args)`，业务复用代码按职责拆分，不新增测试入口、测试函数或测试流程。
- 正式默认值在流程参数中配置，入口直接读取，不在业务代码中补测试值或兜底默认值。
- 指令集另行维护公开调用契约、中文参数和描述面板、分组、启用状态及旧消费者兼容。
- 客户端操作继续配合 `xbot-app-dev`，实际写入、静态通过、运行完成和业务验收分别报告，不自动发布或同步应用。

原 Skill 的引用、CodeFlow 参数示例和流程组织说明同步调整；Skill v10 标记不变。

## 安装与附件

```powershell
npm install -g '@guanzhu.me/xbot-cli@0.9.2'
xbot-cli version
```

GitHub Release 提供：

- `xbot-cli-0.9.2-windows-x64.zip`：Windows x64 可执行程序、README 和许可证。
- `guanzhu.me-xbot-cli-0.9.2.tgz`：npm 安装包。
- `SHA256SUMS.txt`：两份安装包的 SHA-256 校验值。

ZIP 与 npm 包中的程序来自同一份测试构建。自动发布不会执行用户的影刀应用，也不会代替业务运行验收。

## 验证范围与限制

本地已验证浏览器列表、空程序路径过滤、浏览器选择与管道冲突拒绝、独立会话、启动等待和动作不重放；此前已在本机从未运行状态启动 Chrome，并确认打开的是 Chrome 而不是 Edge。

两份 Skill 已验证工作区/用户目录安装、重复安装、跨 Skill 冲突前置检查、逐文件内容与离线安装包。新增规范自带 52 项检查器测试。

DLL 适配器使用已安装影刀的公开成员，不反射私有成员、不注入客户端，也不分发影刀官方 DLL。它依赖影刀自带的 .NET 6 Windows Desktop 运行时，已在影刀 6.3.39 验证，不保证跨版本稳定。Firefox 目前不能通过 Web 管道操作；自定义浏览器的实际插件启用与连接仍需在对应环境核验。
