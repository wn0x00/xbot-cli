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
- **浏览器自动化。** `xbot web` 提供“打开页面 → 快照 → 操作元素”的短命令循环，无需手写 JSON。
- **开放平台 Open API。** `oapi` 一条命令保存 AccessKey，token 自动刷新；应用、机器人、调度、日志、队列、企业账号全覆盖。
- **内置 Agent Skill。** `xbot-cli install --skills` 安装开发规范、Web/Windows 修复辅助和排障指南。

## 安装

**环境要求：Windows x64，Node.js 22+。** npm 包包含原生可执行文件，不需要 Rust，也不在 `postinstall` 下载程序。执行影刀操作前，需要影刀客户端已启动并登录；帮助、schema 和指南可离线使用。

```powershell
npm install -g '@guanzhu.me/xbot-cli'
xbot version
```

安装后 `xbot` 与 `xbot-cli` 调用同一程序。升级直接运行：

```powershell
xbot-cli update          # 升级到最新版本;--check 只检查不升级
```

不安装也可体验：`npx --yes --package '@guanzhu.me/xbot-cli' xbot --help`。历史版本与 `.tgz` 附件见 [GitHub Releases](https://github.com/wn0x00/xbot-cli/releases)。

## 快速上手

```powershell
xbot-cli status --json                    # 确认客户端环境
xbot-cli app open <app-id> --json         # 打开应用
xbot-cli flow verify <flow-id> --json     # 静态检查并运行流程
xbot-cli task run <app-id> --json         # 运行已发布应用
```

浏览器交互：

```powershell
xbot web open baidu.com
xbot web fill e4 "影刀 RPA"
xbot web click e7
```

裸调用 `xbot-cli` 或任意分组（如 `xbot-cli flow`）会返回该层级的帮助和示例；`xbot-cli help <path>` 查看详细帮助。

## 给 AI Agent 使用

向别人推荐 xbot-cli 时只需一句话：**“复制下面这段话，发给你的 AI Agent。”** 适用于 Codex、Claude Code、Cursor、Kimi Code 等任何 Agent：

```text
请使用 xbot-cli 帮我操作影刀 RPA。步骤：
1. 全局安装：npm install -g '@guanzhu.me/xbot-cli'（需要 Windows x64、Node.js 22+，且本机影刀客户端已启动登录）；
2. 在我的项目目录运行 xbot-cli install --skills --json 安装开发 Skill，并阅读生成的 .agents/skills/xbot-app-dev/SKILL.md，按其规范工作；
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

- [CLI 参考](docs/cli-reference.md)：完整命令、参数语义、输出契约与设计依据
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
