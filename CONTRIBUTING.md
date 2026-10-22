# 贡献指南

## 提交 PR

从 `main` 创建功能分支，或通过 Fork 提交 PR。一个 PR 解决一个明确问题，说明改动目的、验证结果和兼容性影响。维护者完成评审和检查后使用 **Squash and merge**；合并提交采用 PR 标题。

PR 标题遵循 [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)，例如：

```text
feat(cli): 添加任务日志筛选
fix(flow): 保留超时后的运行标识
docs(agent): 补充错误恢复示例
refactor(core): 统一传输错误处理
feat(cli)!: 调整任务结果的 JSON 字段
```

本地临时提交无需逐条符合此格式。影响现有调用方式、JSON 字段、退出码或默认行为的不兼容改动，在标题标记 `!`，并在 PR 说明中写清旧行为、新行为和迁移方式。也可在最终合并提交正文使用 `BREAKING CHANGE:`。

Conventional Commits 用于评审和整理变更记录，不自动计算或发布版本。维护者根据兼容性影响决定版本：`0.x` 阶段通常以 patch 发布修复，以 minor 发布功能和不兼容变更；进入 `1.0.0` 也由维护者决定。

普通开发 PR 不修改项目版本或发布 tag。准备发版时，维护者运行 `npm run release:version -- <version>` 同步 npm、Rust 及两个 lockfile 的项目版本，手动更新 `CHANGELOG.md`。这些改动可通过单独的发版准备 PR 评审；合并到 `main`、CI 通过后，维护者手动推送 `v<version>` tag，触发自动构建和 OIDC 发布。版本脚本只修改文件，不创建提交或 tag，也不发布。详见[发布说明](docs/releasing.md)。

上述流程是协作约定。是否强制评审、必需检查和分支保护，由维护者在 GitHub 仓库设置中另行配置。

## 开发与验证

环境为 Windows x64、Node.js 22+、Rust stable 和 MSVC 构建工具。Skill 用途分析回归还需要 Python 3.8+，默认调用 `python`，可用 `XBOT_TEST_PYTHON` 指定测试解释器。在仓库根目录运行：

```powershell
npm ci --ignore-scripts
npm run check
npm test
npm run test:package
```

根据改动验证相关行为：命令或输出契约变更补充有意义的测试；npm 启动器、打包或发布流程变更验证实际安装路径；仅文档改动检查内容和链接即可。PR 写明实际执行的检查，未验证的路径也应说明。

`npm run test:skill` 只验证离线 helper 协议和真实 Python AST 分析，不访问桌面、浏览器或影刀，不下载 ROBULA+，也不能代替真实业务修复验收。测试和运行依赖分别说明，不提交真实窗口快照、聊天记录或定位资产。

修改用户可见行为时，同步 README、CLI 参考或内置指南。Agent 依赖稳定的 JSON、退出码及 stdout/stderr 分离；需要更改时明确兼容性影响。模拟后端测试不能替代真实影刀运行验收。

不提交生成的可执行文件、`target/`、`dist/`、个人配置、凭据或运行数据。锁文件需要提交；依赖更新应包含其对应锁文件变更。发布配置、身份绑定和仓库规则由维护者管理，贡献者无需 npm 发布权限。Agent 提交的代码遵守相同约定。
