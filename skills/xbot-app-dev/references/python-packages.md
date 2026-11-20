# 应用 Python 依赖

先打开目标 Studio 应用，再用 `--expect-app` 核对身份。依赖作用域是当前应用工作区，供其中的流程使用；不操作开发机的全局 Python。

```powershell
xbot-cli python package list --expect-app <app-id> --json
xbot-cli python package install openpyxl requests --expect-app <app-id> --dry-run --json
xbot-cli python package install openpyxl --expect-app <app-id> --json
xbot-cli python package install "包名==版本号" --expect-app <app-id> --json
xbot-cli python package uninstall openpyxl --expect-app <app-id> --json
xbot-cli schema python package install
```

- `list` 只读返回工作区可用包；可用不等于该包已经声明为应用的直接依赖。
- `install` 接受发行包名或 `NAME==VERSION`，`uninstall` 仅接受发行包名；一次最多 50 个，不接受同名重复、版本范围、wheel 路径、requirements 文件或任意 pip 参数。模块 import 名不一定等于发行包名。
- 安装或卸载成功由客户端更新应用依赖配置；包兼容性按应用引擎判断。需要固定版本时显式指定，不能用开发机版本代替应用回读。
- 安装可选 `--index-url <HTTP(S)地址>`，省略时使用客户端配置的源；卸载和列表不接受该选项。
- 三个命令均可用 `--verbosity 0..4`（默认 1）和 `--timeout <秒>`（默认 390）。timeout 是 MCP 等待预算，不会终止客户端 pip；旧客户端内部等待 pip 和执行操作各有 180 秒预算。
- 安装/卸载支持离线 `--dry-run`，只验证输入与展示影响，不检查远端包是否存在、版本兼容、当前应用或实际权限。`list` 无需预览。

## 新旧 MCP

CLI 自动把同一命令映射到旧版 `python_pip`（operation 和 package_name/version 对象），或新版 `xbot-bash python package list/install/uninstall`。以 `client info` 的实际 provider 为准；接口缺失时明确报错，不改用系统 pip。安装/卸载拒绝只读 View 应用。

返回 `data.app_id` 和 `data.result`；后者保留客户端原始包信息、日志路径与执行结果。安装/卸载失败不自动重试、不切换协议重放；读取错误中的部分结果和日志，先查依赖状态再决定如何处理尚未完成项。超时或断连时可能已经部分生效，不能整批重装或卸载来“确认”。

不需要 `execute_cell`；它的探索运行时与正式应用依赖管理是不同入口。
