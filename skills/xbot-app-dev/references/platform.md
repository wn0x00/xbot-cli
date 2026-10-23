# 开放平台 API 与调度排障

`platform` 直接调用影刀企业开放 API，不依赖本机客户端、Studio 或 `status`。
内置 68 个官方文档索引和 44 个接口契约，包括鉴权。
接口 schema 保存请求/响应字段、枚举和状态码表，原始说明、FAQ、示例及附件见来源链接。
回调文档描述服务端推送的数据格式，不是可以主动调用的 API；
CLI 不启动回调接收服务。

## 配置

企业管理员在影刀控制台的 API 配置中创建密钥，再通过环境变量提供：

```powershell
$env:XBOT_PLATFORM_ACCESS_KEY_ID = '<accessKeyId>'
$env:XBOT_PLATFORM_ACCESS_KEY_SECRET = '<accessKeySecret>'
xbot-cli platform auth status --json
xbot-cli platform auth check --json
```

默认地址为 `https://api.yingdao.com`。专有云通过 `XBOT_PLATFORM_BASE_URL`
设置 HTTPS origin，例如 `https://rpa.example.com`，不要包含路径、查询参数或凭据。
仅本地测试允许 HTTP 的 `localhost`、`127.0.0.1` 或 `[::1]`。

也可由外部系统提供 `XBOT_PLATFORM_ACCESS_TOKEN`，它优先于密钥。
此时 `auth check` 仅报告已配置，`verified=false`，实际调用接口时才验证 token 权限。
密钥模式下每次 CLI 调用先获取 token，同一次异步日志查询复用 token。
不持久化凭据、不输出 token，也不自行延长 `expiresIn`。
不要将凭据写入仓库文件或命令行参数。

## 调度排障

`scheduleUuid` 是常规任务配置；`taskUuid` 是某次任务执行；
`jobUuid` 是其中某个应用运行。它们不同于本机 `task` 的 `task_id`
和 Studio 的 `run_id`，不能互换。

```powershell
xbot-cli platform schedule list --search "报表" --json
xbot-cli platform schedule detail <scheduleUuid> --json
xbot-cli platform schedule runs <scheduleUuid> --status error --json
xbot-cli platform task latest --json
xbot-cli platform task status <taskUuid> --json
xbot-cli platform task jobs <taskUuid> --robot <robotClientUuid> --json
xbot-cli platform job list --schedule <scheduleUuid> --status error --json
xbot-cli platform job status <jobUuid> --json
xbot-cli platform job logs <jobUuid> --search "错误" --page 1 --size 20 --json
```

`task status` 的 `jobDataList` 提供各应用的运行标识。
`job status` 可包含 `remark`、`screenshotUrl` 和参数。
结合任务状态、错误备注和日志定位原因；API 调用成功不等于任务运行成功。
截屏地址作为证据返回，不自动下载。

`job list` 和 `schedule runs` 按游标翻页：
从响应取 `nextId`/`preId` 作为 `--cursor`，用 `--direction next/pre` 指定方向。
游标保留长整数精度。CLI 每次查询一页，不自动遍历所有历史。

## 日志

默认 `job logs` 调用同步查询接口。机器人离线、已删除、历史日志不可用
或服务器超时都可能导致查询失败。
结果按 `page`/`size` 分页，日志文本可能已被服务端截断，单页不能当作完整日志。

需要通知和轮询方式时显式选择：

```powershell
xbot-cli platform job logs <jobUuid> --async --timeout 10 --json
```

通知只发送一次，之后每隔 250ms 查询同一个 `requestId`。
`80204002` 表示处理中，其他错误立即返回。
`--timeout` 是包含鉴权的总预算。
超时或轮询失败时，错误 `data` 保留 `request_id` 和 `job_uuid`，
可用同一请求继续查询：

```powershell
xbot-cli platform api call log.query --query '{"requestId":"<request_id>"}' --json
```

文档说明异步日志结果仅缓存 60 秒，不要长期复用标识。
遇到 `429` 后退避，不并发密集查询；CLI 不自动重试请求。

## 运行数据

```powershell
xbot-cli platform app records --app <appId> --from "2026-10-22 00:00:00" --to "2026-10-23 00:00:00" --json
xbot-cli platform app records --from "2026-10-22 00:00:00" --to "2026-10-23 00:00:00" --mode schedule,Assistant --json
```

时间格式为 `yyyy-MM-dd HH:mm:ss`，按平台时间填写，不隐式转换时区。
起止时间必须完整、顺序正确，跨度不超过 7 天。
记录位于 `response.data.result`，分页位于 `response.data.pageDTO`。
该接口不返回调度 `jobUuid`，不能直接用运行记录标识查询调度日志。
需要旧接口的 `remark` 和游标时查看 `platform api schema app.records-cursor`，
按接口契约提供 `appId` 或时间范围及 `minId`。

## 其余接口

```powershell
xbot-cli platform api list --search queue --json
xbot-cli platform api schema job.status --json
xbot-cli platform api schema 710481967730900992 --json
xbot-cli platform docs --json
xbot-cli platform docs 710460954567454720 --json
xbot-cli platform api call job.status --body '{"jobUuid":"<jobUuid>"}' --json
xbot-cli platform api call queue.items --path '{"queueUuid":"<queueUuid>"}' --query '{"status":"on hold","page":1,"size":10}' --json
```

接口选择支持名称、官方文档 ID 或已登记的 `/oapi/...` 路径模板，不接受任意 URL。
`--body`、`--query`、`--path` 接受 JSON、`@file`、`-` stdin；
一次只能有一个参数读取 stdin。GET 使用 `--query`，
路径占位符使用 `--path`，每个值按独立路径段编码。

支持企业账号、应用、机器人、调度、队列和文件接口。
创建、修改、删除、转移、启动、停止、重试、入队、出队和上传需要 `--yes`；
取得对应动作授权后才执行。先离线预览：

```powershell
xbot-cli platform api call job.stop --body @stop.json --dry-run --json
xbot-cli platform api call file.upload --file .\input.xlsx --dry-run --json
```

上传使用 multipart/form-data，支持 txt/csv/xlsx，文件名最长 100 字符，
本地上限 10MB；服务端还可能施加更严格的容量、配额和有效期限制。

## 输出与验收

成功为 `{"ok":true,"data":{"endpoint":"job.status","response":{...}}}`。
`response` 保留平台封装与分页结构，已知凭据和敏感字段脱敏。
错误 `data` 保留 `endpoint`、`api_code`、平台响应和 `requestId`；
网络结果未知时不自动重放写操作。
退出码遵循 CLI 契约：参数 2、认证/配置 3、网络/限流/超时 4、协议 6、
确认门禁 10，其他业务错误 1。
`retryable` 不表示写操作已确认未生效。

`--dry-run` 使用与执行相同的解析和本地校验，完全离线；
不验证凭据、权限、嵌套业务模型或真实日志可用性。
`schema platform api call` 给出 CLI 输入约定，
`platform api schema <endpoint>` 给出官方字段表。
模拟测试不能代替企业服务验收，未真实调用时明确报告未验证。

维护文档契约时运行 `node scripts/sync-platform-docs.cjs`。
该脚本仅读取登记的公开文档，更新事实字段表和来源索引，
不需要企业凭据，不调用生产 API。
