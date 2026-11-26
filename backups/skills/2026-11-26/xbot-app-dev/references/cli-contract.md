# CLI 契约与现场发现

本包是操作指引，不冻结 CLI 版本。首次执行确认实际路径、版本；同版本号也可能来自不同本地构建。Windows 说明随本版 CLI 一起维护；其他安装版本以其自身 help/schema 为准。

```powershell
Get-Command xbot-cli | Select-Object Source
xbot-cli version --json
xbot-cli client info --json
xbot-cli
xbot-cli win32
xbot-cli help win32 click
xbot-cli schema win32 click
xbot-cli guide
```

先看当前层级，再查目标叶命令。help/schema/guide/version 可离线；裸根或纯分组显示帮助，不运行业务。需要最新发布版本时重新核对发行来源及日期；版本查询不等于升级授权。

## 输出与失败

业务输出默认终端文本、管道 JSON；机器读取显式 `--json`。schema 默认 JSON，guide/help 默认文本，显式格式优先。

- 成功 stdout：`{"ok":true,"data":...}`。
- 失败 stderr：`{"ok":false,"error":{"type":...,"code":...,"message":...,"hint":...,"retryable":...},"data":...}`。
- 保留 applied、partial、运行 ID 和阶段；非零退出不代表未执行，retryable 不代表可以重放。动作已返回但后续观测失败，只补观测。

| 退出码 | 含义与处理 |
| --- | --- |
| 0 | 命令成功，业务结果另查 |
| 1 | 操作/业务失败，读 error 与部分 data |
| 2 | 参数错误，查现场 schema 后修正 |
| 3 | 认证、权限或配置问题，核对实际缺项 |
| 4 | 网络、超时或离线，先查询已启动的同一次执行 |
| 5 | 上下文冲突或占用，回读后协调 |
| 6 | 协议或内部问题，保留证据 |
| 10 | CLI 确认门槛；已有具体授权才追加 --yes |

## 上下文、预览和副作用

Studio 命令按 schema 的 `requires_studio` 使用 `--expect-app`；它断言应用、不切换应用，不保证业务页面/窗口身份。Web Bridge 与 Win32 独立能力不要求打开 Studio，分别确认页面、窗口及权限。

`--dry-run` 只适用于支持它的命令，做离线输入校验；不访问目标、不检查实时权限或断言、不证明后端接受性。`flow check` 静态，`flow verify` 真实运行。app save 是本地保存，sync 有上传/关闭副作用，publish 才是发布；这些证据不能互换。`--yes` 不是新的用户授权。

复杂 JSON 优先 `@file` 或 stdin，源代码按文件参数传入；一次只用一个 stdin 消费者。保持布尔、数组、对象、表达式语义，不拼字符串载荷。不把密钥放到报告、命令回显或 shell 历史中。

超时单位、范围、默认值逐命令查 schema；Win32 的动作/快照预算为 1–60 秒，不沿用其他命令的长预算。保留正确标识：Studio run_id、客户端 task_id、企业 scheduleUuid/taskUuid/jobUuid、Windows snapshot_id 各有作用域。

## 动态工具及安装

原始工具先 `tool studio list/schema` 或 `tool browser list/schema` 再 call。外层 CLI schema 不代替动态工具 schema，不靠 raw call/eval 绕过作用域、确认或发布边界。Web 常用动作走 Chrome Bridge；原始浏览器工具有独立可用性，不以任一通道状态代替另一通道。

`client info --json` 返回 CLI 自身版本、运行中的影刀进程/文件版本/目录、选中的 shell-cli 路径与来源、客户端状态及实际 MCP provider/endpoint。不打开应用；离线或某项查询失败时保留其他信息，读取 `partial`、`client.detection_complete` 和每个通道的 `ok/error`，不能只看外层 `ok`。多安装同时运行或进程路径不可读时不猜唯一客户端；目录推断版本标为 `installation_directory`。`XBOT_SHELL_CLI` 只覆盖后端文件，不改变运行客户端身份。缺少新命令的旧 CLI，先用 version/status/doctor，并明确客户端版本未直接读取。

Studio MCP 自动适配旧版与 6.4；`client info` 或 `doctor --json` 显示实际 provider/endpoint。6.4 的 VisualFlow 用 `flow source read/write/edit`，旧版块接口继续保留。接口差异、原始 xbot-bash 调用和配置见 [MCP 兼容](mcp-compatibility.md)（`guide mcp`）。

新任务用 flow code / flow visual、task、web、tool；code/visual、app run、browser、mcp、platform 等旧入口只作兼容。没有通用 repair 子命令；Skill 内 helper 须用 Node/Python 显式调用。

`install --skills` 同时安装 xbot-app-dev 和 xbot-development-standards，保留原来的两个名称；本文维护优化已同步到这两份资源。安装、升级 CLI 或覆盖已安装 Skill 按用户任务范围执行，不隐式修改 PATH、系统依赖或现有 Skill。
