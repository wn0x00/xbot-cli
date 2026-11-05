# xbot-cli

面向 AI Agent 的影刀（ShadowBot）RPA 命令行工具。

[![npm 版本](https://img.shields.io/npm/v/%40guanzhu.me%2Fxbot-cli?label=npm&color=blue)](https://www.npmjs.com/package/@guanzhu.me/xbot-cli)
[![CI](https://github.com/wn0x00/xbot-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/wn0x00/xbot-cli/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/wn0x00/xbot-cli?label=release)](https://github.com/wn0x00/xbot-cli/releases/latest)
[![许可证](https://img.shields.io/badge/license-MIT-green)](LICENSE)

[CLI 参考](docs/cli-reference.md) · [Issues](https://github.com/wn0x00/xbot-cli/issues) · [发布说明](docs/releasing.md)

## 什么是 xbot-cli

xbot-cli 通过影刀客户端内置通道管理应用、编辑流程、运行验证和浏览器自动化。Agent 可以逐层查询帮助和参数 schema，通过稳定的 JSON 结果判断下一步，并安装随包提供的开发 Skill。CLI 使用 Rust 实现，通过 npm 分发预编译程序。

## 核心特性

- **Agent 优先的输出契约。** 裸调用逐层返回帮助，`schema <path>` 给出机器可读的参数定义；成功写 stdout、失败写 stderr，配合明确的退出码与 `ok/data/error` JSON 信封。
- **应用与流程全生命周期。** `app` 创建、打开、保存、同步、发布；`flow` 读取、静态检查、运行与等待；`task` 运行已发布应用。
- **安全的写入边界。** `--dry-run` 离线预览，`--expect-app` 断言当前应用，高风险操作必须显式 `--yes`。
- **浏览器自动化。** `xbot-cli web` 提供“打开页面 → 快照 → 操作元素”的短命令循环，同时返回支持操作的影刀 Python；`web browser list` 查询插件安装与浏览器注册信息，`--browser <id>` 指定目标，打开网页时可自动启动目标浏览器；`web xpath <ref...>` 生成单元素或相似元素 XPath。
- **安装环境自动发现。** 支持识别非默认影刀安装目录，诊断失败时显示实际后端路径和针对性修复建议。
- **开放平台 Open API。** `oapi` 一条命令保存 AccessKey，token 自动刷新；应用、机器人、调度、日志、队列、企业账号全覆盖。
- **内置 Agent Skill。** `xbot-cli install --skills` 同时安装 `xbot-app-dev` 和 `xbot-development-standards`，包含应用/指令集开发规范、Web/Windows 修复辅助和排障指南。

## 安装

**环境要求：Windows x64，Node.js 22+。** npm 包包含原生可执行文件，不需要 Rust，也不在 `postinstall` 下载程序。执行影刀操作前，需要影刀客户端已启动并登录；帮助、schema 和指南可离线使用。

```powershell
npm install -g '@guanzhu.me/xbot-cli'
xbot-cli version
```

安装后通过 `xbot-cli` 调用（0.8.2 起仅保留此入口；安装时会自动清理旧版遗留的 `xbot` shim 和 PATH 中的旧程序）。升级直接运行：

```powershell
xbot-cli update          # 升级到最新版本;--check 只检查不升级
```

不安装也可体验：`npx --yes --package '@guanzhu.me/xbot-cli' xbot-cli --help`。历史版本与 `.tgz` 附件见 [GitHub Releases](https://github.com/wn0x00/xbot-cli/releases)。

## 快速上手

```powershell
xbot-cli status --json                    # 确认客户端环境
xbot-cli app open <app-id> --json         # 打开应用
xbot-cli flow verify <flow-id> --json     # 静态检查并运行流程
xbot-cli task run <app-id> --json         # 运行已发布应用
```

同步当前应用并关闭编辑器：

```powershell
xbot-cli app sync --expect-app <app-id> --json
```

该命令调用 `shadowbot.shell-cli.exe studio current sync`，将当前编辑应用同步到云端并关闭
Studio，不发布应用。`--dry-run` 可离线预览；结果分别报告同步、编辑器关闭和上传日志证据。

浏览器交互：

```powershell
xbot-cli web browser list --json
xbot-cli web open baidu.com
xbot-cli web fill e4 "影刀 RPA"
xbot-cli web click e7
# 使用当前快照中的实际 ref
xbot-cli web xpath e7
xbot-cli web xpath e7 e8
```

网页操作保留 snapshot，并显示“影刀 Python：”；`--no-python` 关闭代码生成。XPath 每次最多返回 5 组候选；iframe 使用分层 Selector，open ShadowRoot 标明需要影刀验证，closed ShadowRoot 明确拒绝生成。详见 [0.9.1 更新说明](docs/updates/2026-10-30-web-python-xpath.md)。

`web browser list` 通过影刀 DLL 查询 Chrome、Edge、360、Firefox 插件状态和自定义浏览器注册，
文本输出与 `app list` 一样使用表格，始终排除 `exePath` 为空的项。默认隐藏未安装项，
`--all` 也显示有程序路径的未安装项。`installed` 为 DLL 判定，`registered` 仅代表注册，
不保证插件启用或已连接。全部网页操作可用 `--browser <id>`，例如：

```bash
xbot-cli web active --browser edge --json
xbot-cli web snapshot --browser edge --json
xbot-cli web click e7 --browser edge --json
xbot-cli web open example.com --browser ziniaobrowser --json
```

`web open --browser <id>` 在目标未运行时按 DLL 的 `exePath` 启动浏览器，等待最多 20 秒
连接其影刀插件后再打开目标 URL；失败不回退到其他浏览器。不传浏览器或显式指定
`XBOT_WEB_PIPE` 时不自动启动，其他网页动作也不会因断开而自动重启或重放。

使用列表中的 `id`，每个浏览器独立保存页面会话；不传参数沿用原默认上下文。
指定浏览器未连接时明确报错，不回退其他浏览器。旧 `web open --mode` 仍兼容。
查询适配器需要影刀自带的 .NET 6 Windows Desktop 运行时，已在影刀 6.3.39 验证；
它不是稳定 SDK，影刀升级可能需要重新适配。Firefox 可查询安装状态，当前 Web 管道不支持操作。

裸调用 `xbot-cli` 或任意分组（如 `xbot-cli flow`）会返回该层级的帮助和示例；`xbot-cli help <path>` 查看详细帮助。

## 给 AI Agent 使用

向别人推荐 xbot-cli 时只需一句话：**“复制下面这段话，发给你的 AI Agent。”** 适用于 Codex、Claude Code、Cursor、Kimi Code 等任何 Agent：

```text
请使用 xbot-cli 帮我操作影刀 RPA。步骤：
1. 全局安装：npm install -g '@guanzhu.me/xbot-cli'（需要 Windows x64、Node.js 22+，且本机影刀客户端已启动登录）；
2. 在我的项目目录运行 xbot-cli install --skills --json 安装两份 Skill，并阅读生成的 .agents/skills/xbot-app-dev/SKILL.md 和 .agents/skills/xbot-development-standards/SKILL.md，先区分应用与指令集，再按对应规范工作；
3. 先运行 xbot-cli status --json 确认环境，用 xbot-cli help <path> / schema <path> / guide <topic> 逐层了解命令；
4. 解析结果一律显式加 --json；修改前先读现状，写入前先用 --dry-run 预览；
5. 发布、回收等不可逆操作必须经我明确授权，未经确认不要执行。
```

完整的 Agent 工作指令、输出契约、退出码和 Skill 能力边界见 [CLI 参考](docs/cli-reference.md)。

## 开放平台 oapi

企业版用户可选择开放 API 通道，独立于本机客户端。

```powershell
xbot-cli oapi auth <AccessKeyId> <AccessKeySecret>   # 登录并保存凭据,token 自动刷新
xbot-cli oapi auth status                            # 查看凭据来源与 token 缓存
xbot-cli oapi auth logout                            # 清除当前授权

xbot-cli oapi robot list --json                      # 找到在线机器人
xbot-cli oapi app list --json                        # 应用列表
xbot-cli oapi app detail <appId> --json              # 已发布版本与运行参数
xbot-cli oapi job start --app <appId> --robot <账号> --yes --json
xbot-cli oapi job logs <jobUuid> --json
```

凭据也可用环境变量 `XBOT_OAPI_ACCESS_KEY_ID` / `XBOT_OAPI_ACCESS_KEY_SECRET` 提供（兼容 `XBOT_PLATFORM_*`）；专有云设置 `XBOT_OAPI_BASE_URL`。`oapi` 覆盖应用、机器人、原子调度、常规任务、工作队列、企业账号和日志接口；`oapi api list/schema/call` 提供全部 44 个接口的离线契约与通用调用，`platform` 是兼容别名。正常开发仍默认使用客户端通道，凭据存在也不会自动切换。详见 [平台指南](skills/xbot-app-dev/references/oapi.md)。

## 文档

- [0.9.3 更新说明](docs/updates/2026-11-05-flow-parameters.md)：不受 MCP 六类型白名单限制的流程参数写入、类型查询和使用文档
- [0.9.2 更新说明](docs/updates/2026-11-05-browser-development-standards.md)：浏览器列表、指定浏览器、自动启动及指令集/应用开发规范 Skill
- [0.9.1 更新说明](docs/updates/2026-10-30-web-python-xpath.md)：Web Python、XPath、安装环境发现及验证范围
- [CLI 参考](docs/cli-reference.md)：完整命令、参数语义、输出契约与设计依据
- [流程参数使用文档](docs/flow-parameters.md)：真实类型 ID、批量增改删、默认值及 reload 回读
- [Skill v10 升级说明](docs/skill-v10-upgrade.md)：Skill 模块、依赖与限制
- [平台指南](skills/xbot-app-dev/references/oapi.md)：开放 API 的配置、概念与排障
- [发布说明](docs/releasing.md)：版本同步与发布流程

## 本地开发

```powershell
npm run build          # 构建 Rust release 并准备 npm 原生程序
npm test               # 全部自动化测试
npm run test:package   # 检查 npm 包与启动器
```

Rust 也可单独构建：`cargo test`、`cargo clippy --all-targets -- -D warnings`。源码托管于 [wn0x00/xbot-cli](https://github.com/wn0x00/xbot-cli)；贡献指南见 [CONTRIBUTING.md](CONTRIBUTING.md)，发版流程见[发布说明](docs/releasing.md)。

## 许可

本项目使用 [MIT License](LICENSE)。
