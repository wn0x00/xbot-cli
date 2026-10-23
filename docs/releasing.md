# 发布 xbot-cli

源码仓库为 [`wn0x00/xbot-cli`](https://github.com/wn0x00/xbot-cli)，npm 包为 [`@guanzhu.me/xbot-cli`](https://www.npmjs.com/package/@guanzhu.me/xbot-cli)，安装后可使用 `xbot` 或 `xbot-cli`。发布目标是 Windows x64，npm 包自带原生程序。

## 发布渠道

| 渠道 | 用途 |
| --- | --- |
| [npmjs](https://www.npmjs.com/package/@guanzhu.me/xbot-cli) | 正式安装来源：`npm install -g '@guanzhu.me/xbot-cli'`，registry 为 `registry.npmjs.org`。 |
| [GitHub Releases](https://github.com/wn0x00/xbot-cli/releases) | 版本说明、Windows 可执行程序压缩包、同版 npm `.tgz` 和校验文件。[v0.6.0](https://github.com/wn0x00/xbot-cli/releases/tag/v0.6.0) 是首次版本入口。 |
| [GitHub Packages](https://github.com/users/wn0x00/packages?repo_name=xbot-cli) | 独立服务，其 npm registry 为 `npm.pkg.github.com`。本项目未向这里发布，它不会自动镜像 npmjs 的包。 |

## 日常发版

1. 开发者按[贡献指南](../CONTRIBUTING.md)提交 PR，通过检查和评审后 Squash 合并到 `main`。
2. 维护者决定新版本，使用 `npm run release:version -- <version>` 同步 `package.json`、`package-lock.json`、`Cargo.toml`、`Cargo.lock` 中的项目版本，并手动更新 `CHANGELOG.md`。普通开发 PR 不改版本。版本脚本只修改这四个文件，不创建提交、tag 或发布。
3. 将版本和变更记录提交到 `main`；需要评审时使用单独的发版准备 PR。等待该提交的 CI 通过后，维护者在该提交上创建并推送 `v<version>` tag。
4. tag 推送触发 `publish.yml`。工作流校验 tag、版本、包名、仓库身份及提交属于 `main`，完成 Rust 检查、测试和 npm 安装验收。发布 job 核对制品校验值，通过 OIDC 将这份 `.tgz` 发布到 npm；随后创建 GitHub Release 并上传附件。

普通提交和 PR 合并不会发布，也不再自动创建发版 PR。Conventional Commits 便于评审和整理记录；版本由维护者决定。`0.x` 阶段通常以 patch 发布修复，以 minor 发布功能或不兼容变更；不兼容变更必须提供迁移说明。是否进入 `1.0.0` 也由维护者决定。

以 `0.6.2` 为例，先准备版本提交；将示例版本替换为实际需要发布的版本：

```powershell
git switch main
git pull --ff-only origin main
git switch -c release/v0.6.2
npm run release:version -- 0.6.2
```

编辑 `CHANGELOG.md`，记录本次改动及必要的迁移说明，再检查并提交：

```powershell
git diff --check
git diff -- Cargo.toml Cargo.lock package.json package-lock.json CHANGELOG.md
npm run check
npm test
npm run test:package
git add Cargo.toml Cargo.lock package.json package-lock.json CHANGELOG.md
git commit -m 'chore(release): prepare 0.6.2'
git push -u origin release/v0.6.2
```

在 GitHub 创建 PR 并合并。确认 `main` 上该发布提交的 CI 通过后，回到本地同步并检查即将打 tag 的提交：

```powershell
git switch main
git pull --ff-only origin main
git status --short
git log -1 --oneline
node -p "require('./package.json').version"
git tag -a v0.6.2 -m 'Release v0.6.2'
git push origin v0.6.2
```

工作区应干净，版本应为 `0.6.2`，当前提交应是已经通过 CI、准备发布的 `main` 提交。逐条执行并检查结果；命令失败时先处理原因，再继续。仅推送本次 tag，不使用 `git push --tags`。推送后可在 [Actions](https://github.com/wn0x00/xbot-cli/actions/workflows/publish.yml) 查看构建、npm 发布及 Release 附件上传的结果。

tag 必须为稳定版 `v<major>.<minor>.<patch>`，并精确匹配四个文件中的项目版本。不要把 tag 打在仅存在于功能分支的提交上，也不要移动已发布的 tag。

## 配置发布身份

GitHub 仓库需要启用 Actions，并保留名为 `npm` 的 Environment；当前该环境仅允许 `v*` tag 部署。若为该环境配置人工审批，发布将停在审批步骤。合并方式、必需检查和评审人数在仓库 Settings 中配置；文档中的协作约定不会自动启用 Rulesets。手动 tag 流程不需要 Actions 创建 PR 的权限。

npm trusted publisher 必须绑定如下身份，字段区分大小写：

| 字段 | 值 |
| --- | --- |
| 包 | `@guanzhu.me/xbot-cli` |
| GitHub owner / repository | `wn0x00/xbot-cli` |
| Workflow filename | `publish.yml` |
| Environment name | `npm` |
| Allowed actions | 直接 `npm publish` |

包已存在后，具有包写权限且启用 2FA 的维护者可以在 npm 包 Settings → Trusted publishing 配置，也可使用 npm 11.15+ 的 `npm trust`。以下命令使用固定 CLI 版本，不修改全局 npm；按提示完成 npm 身份验证：

```powershell
npx --yes npm@11.21.1 trust list '@guanzhu.me/xbot-cli' --registry=https://registry.npmjs.org
npx --yes npm@11.21.1 trust github '@guanzhu.me/xbot-cli' --repo wn0x00/xbot-cli --file publish.yml --environment npm --allow-publish --registry=https://registry.npmjs.org
```

先检查已有绑定；一致时保留，不要重复创建。若需要更换绑定，先核实旧配置，再按 [npm trust 文档](https://docs.npmjs.com/cli/v11/commands/npm-trust/)撤销并重新配置。发布 job 使用 GitHub 托管 runner、Node.js 24 和支持 OIDC 的 npm，授予 `id-token: write`，无需设置 `NPM_TOKEN`。

新绑定必须在两天内成功完成首次 OIDC 发布，否则需要重新创建。公开仓库通过 OIDC 发布公开包会生成 provenance。[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/)。配置成功本身不代表发布已验收：应以新版本的 Actions 发布成功、npm 版本及完整性校验、Release 附件齐全为准。`0.6.0` 的本地发布不能证明 OIDC 链路有效。

## 首次 OIDC 发布验收

`0.6.1` 已完成真实 OIDC 发布：[Actions 运行](https://github.com/wn0x00/xbot-cli/actions/runs/39233553232)、[GitHub Release](https://github.com/wn0x00/xbot-cli/releases/tag/v0.6.1)、[npm 版本](https://www.npmjs.com/package/@guanzhu.me/xbot-cli/v/0.6.1)。这是历史验收：当时使用 Release Please 合并发版 PR 后创建 tag 和 Release，再 dispatch 发布工作流，未使用长期 npm token。当前改用维护者手动推送 tag 触发同一个 `publish.yml`，OIDC 的仓库、工作流和环境绑定保持不变。

验收从公开下载入口核对了三个 Release 附件、SHA256SUMS 和 npm `dist.integrity`；npm provenance 中的仓库、`publish.yml`、`refs/tags/v0.6.1`、源码提交 `2ce27cd5ff0ac5f4703b32a1ea5568cd01f9b379` 及包摘要均匹配。此处核对的是公开 provenance 声明和摘要，未单独执行 Sigstore 签名的密码学校验。

使用空 npm 缓存和临时目录从 registry 安装后，实际 Windows `.cmd` 入口的版本、根帮助、分组帮助、schema 和 Agent Skill 安装均通过；安装程序与 npm 包、Windows ZIP 内的可执行文件字节一致。这些结果证明历史版本的 OIDC 发布及安装路径，不代替后续新版本的发布验收。

## 本地验包

维护者需要 Windows x64、Node.js 22+、Rust stable 和 MSVC 构建工具。从仓库根目录运行：

```powershell
npm ci --ignore-scripts
npm run check
npm test
npm run test:package
```

`test:package` 构建 Rust release，检查包内文件白名单，把生成的 `.tgz` 离线安装到独立临时目录，再验证命令、JSON 和 Skill。通过后保留 `dist/guanzhu.me-xbot-cli-<version>.tgz`。发布使用已验收的包，不重新打包；`.backups/`、`.local-archive/`、`target/` 和本地配置不属于发布内容。

每次 Release 包含 `guanzhu.me-xbot-cli-<version>.tgz`、`xbot-cli-<version>-windows-x64.zip` 和 `SHA256SUMS.txt`。Windows 压缩包中的程序来自同一份已验收的 npm 包。CI 通过不代表真实影刀应用运行已验证；后者需要已启动并登录影刀的本机环境。

## 失败恢复与重新运行

优先在 Actions 页面重跑失败 job，复用该次运行已经验收的制品。也可重新 dispatch 指定 tag；安装并登录 GitHub CLI 后运行：

```powershell
gh workflow run publish.yml --ref v0.6.1
npm view '@guanzhu.me/xbot-cli@0.6.1' version dist.integrity --registry=https://registry.npmjs.org
```

将示例版本替换为需要恢复的版本；`publish.yml` 必须已存在于默认分支及该 tag。tag 必须等于 `v<package.json version>`，分支不能直接发布。

重试时，已存在的 npm 版本只有在 `dist.integrity` 与本次制品一致时才跳过发布；同名 Release 附件只有校验值相同时才跳过上传。不同内容会失败，不覆盖已有版本或附件。若 npm 已成功而附件上传失败，恢复附件步骤即可；若源码或包内容需要修改，应通过新的 PR 发布新版本，不移动旧 tag。

## 明确要求同版本重打包时

常规发布仍按上文使用新版本。若维护者明确要求版本不变，只更新 GitHub 制品，
先将修正合并到 `main`、通过 CI，并对新构建完成安装验收。为新源码创建独立
`repack/<version>-<date>` 标签，在 Release 说明中记录该来源、重打包日期及校验值；
原 `v<version>` 标签继续对应首次发布的 npm 内容。

npm 不允许将同名同版本替换为不同内容，因此重打包不调用 npm 发布，也不绕过
发布脚本的完整性保护。替换 GitHub 附件前保留旧文件，先上传并校验新附件，
再替换名称和校验文件。Release 必须明确说明附件属于重打包版、npm registry
仍保留旧包，并提供从 Release `.tgz` 安装的命令。
