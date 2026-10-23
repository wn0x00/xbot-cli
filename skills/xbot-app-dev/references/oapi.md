# 企业版备用入口：开放平台 Open API(oapi)

默认按原有 CLI 方式操作：应用运行及日志使用 `task run/status/logs`，
Studio 流程使用 `flow run/verify/wait`，开发和浏览器操作继续使用 `app/flow/web`。
本指南仅在企业版用户明确选择开放 API 运行应用、调用调度或查询平台日志时读取。
API 凭据存在不会切换通道，普通命令失败也不会自动回退到 `oapi`；
任务结果未知时先查询原任务，避免跨通道重复运行。

选用此备用入口后，`oapi` 直接调用影刀企业开放 API，
不依赖本机客户端、Studio 或 `status`。`platform` 是兼容别名，新脚本一律使用 `oapi`。
内置 68 个官方文档索引和 44 个接口契约，包括鉴权。
接口 schema 保存请求/响应字段、枚举和状态码表，原始说明、FAQ、示例及附件见来源链接。
回调文档描述服务端推送的数据格式，不是可以主动调用的 API；
CLI 不启动回调接收服务。

## 授权

企业管理员在影刀控制台的 API 配置中创建 AccessKey，然后一条命令完成登录：

```powershell
xbot-cli oapi auth <AccessKeyId> <AccessKeySecret>
xbot-cli oapi auth status --json
```

登录会先联网验证密钥，再把密钥和 token 保存到本地凭据文件
（`%LOCALAPPDATA%\xbot-cli\oapi-auth.json`）。token 过期前直接复用，
过期后自动用密钥刷新，无需重复登录。常用子命令：

```powershell
xbot-cli oapi auth login --ak <id> --sk <secret>   # 与位置参数形式等价
xbot-cli oapi auth check --json                    # 立即联网验证密钥
xbot-cli oapi auth logout                          # 清除当前授权(凭据与 token 缓存)
```

输出只显示脱敏后的 AccessKey ID，不回显密钥和 token。

也可用环境变量提供凭据，优先级高于本地配置，适合 CI：

```powershell
$env:XBOT_OAPI_ACCESS_KEY_ID = '<accessKeyId>'
$env:XBOT_OAPI_ACCESS_KEY_SECRET = '<accessKeySecret>'
```

兼容旧变量名 `XBOT_PLATFORM_*`。`XBOT_OAPI_ACCESS_TOKEN` 直接提供 token 时优先于密钥，
此时 `auth check` 仅报告已配置，`verified=false`；环境变量方式不做 token 缓存。

默认地址为 `https://api.yingdao.com`。专有云通过 `XBOT_OAPI_BASE_URL`
设置 HTTPS origin（登录时也可加 `--base-url` 一并保存），不要包含路径、查询参数或凭据。
仅本地测试允许 HTTP 的 `localhost`、`127.0.0.1` 或 `[::1]`。

## 运行应用(原子调度,推荐)

原子调度一次请求即可指定应用和机器人，无需控制台预配置：

```powershell
xbot-cli oapi robot list --json                                    # 找到在线机器人
xbot-cli oapi robot detail --robot <accountName> --json            # 是否正在运行任务
xbot-cli oapi app list --json                                      # 应用列表
xbot-cli oapi app detail <appId> --json                            # 已发布版本与运行参数定义
xbot-cli oapi job start --app <appId> --robot <accountName> --params @params.json --dry-run --json
xbot-cli oapi job start --app <appId> --robot <accountName> --params @params.json --yes --json
xbot-cli oapi job status <jobUuid> --json
xbot-cli oapi job stop <jobUuid> --yes --json
xbot-cli oapi job retry <jobUuid> --yes --json
```

`--robot` 指定单台机器人，`--group <robotClientGroupUuid> --scope any|all` 指定分组。
`--idempotent <uuid>` 保证网络异常后安全重试；`--params` 是
`[{"name":"month","value":"2026-10","type":"str"}]` 形式的 JSON 数组。
文件类型参数先 `oapi api call file.upload --file .\input.xlsx --yes --json` 换取 fileKey。
启动、停止、重试必须 `--yes`；先 `--dry-run` 离线预览再执行。
`app detail` 要求应用已发布；`app detail --dev` 查看开发版本明细。

## 调度排障

`scheduleUuid` 是常规任务配置；`taskUuid` 是某次任务执行；
`jobUuid` 是其中某个应用运行。它们不同于本机 `task` 的 `task_id`
和 Studio 的 `run_id`，不能互换。

```powershell
xbot-cli oapi schedule list --search "报表" --json
xbot-cli oapi schedule detail <scheduleUuid> --json
xbot-cli oapi schedule runs <scheduleUuid> --status error --json
xbot-cli oapi task start <scheduleUuid> --yes --json               # 触发预配置任务
xbot-cli oapi task latest --json
xbot-cli oapi task status <taskUuid> --json
xbot-cli oapi task stop <taskUuid> --yes --json
xbot-cli oapi task jobs <taskUuid> --robot <robotClientUuid> --json
xbot-cli oapi job list --schedule <scheduleUuid> --status error --json
xbot-cli oapi job status <jobUuid> --json
xbot-cli oapi job logs <jobUuid> --search "错误" --page 1 --size 20 --json
```

`task status` 的顶层状态是聚合状态，需展开 `jobDataList` 逐台机器人确认。
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
xbot-cli oapi job logs <jobUuid> --async --timeout 10 --json
```

通知只发送一次，之后每隔 250ms 查询同一个 `requestId`。
`80204002` 表示处理中，其他错误立即返回。
`--timeout` 是包含鉴权的总预算。
超时或轮询失败时，错误 `data` 保留 `request_id` 和 `job_uuid`，
可用同一请求继续查询：

```powershell
xbot-cli oapi api call log.query --query '{"requestId":"<request_id>"}' --json
```

文档说明异步日志结果仅缓存 60 秒，不要长期复用标识。
遇到 `429` 后退避，不并发密集查询；CLI 不自动重试请求。

## 企业账号与工作队列

```powershell
xbot-cli oapi user list --search "张" --json
xbot-cli oapi user create --name 张三 --account zs@flow --phone 13800000000 --account-type basic --role e_user --password '<初始密码>' --yes --json
xbot-cli oapi user modify --account zs@flow --role e_admin --yes --json
xbot-cli oapi user delete --account zs@flow --receive admin@flow --yes --json

xbot-cli oapi queue list --json
xbot-cli oapi queue items <queueUuid> --status queued --json
xbot-cli oapi queue enqueue <queueUuid> --name 工单1 --priority 0 --biz-info <业务信息> --yes --json
xbot-cli oapi queue dequeue <queueUuid> --yes --json
xbot-cli oapi queue modify <itemUuid> --status processed --yes --json
xbot-cli oapi queue count <queueUuid> --json
```

账号写操作需要 `--yes`。删除账号时其应用资产转移给 `--receive` 指定的同企业账号。
`user reset-password` 是自助改密，需提供旧密码。

## 运行数据

```powershell
xbot-cli oapi app records --app <appId> --from "2026-10-22 00:00:00" --to "2026-10-23 00:00:00" --json
xbot-cli oapi app records --from "2026-10-22 00:00:00" --to "2026-10-23 00:00:00" --mode schedule,Assistant --json
```

时间格式为 `yyyy-MM-dd HH:mm:ss`，按平台时间填写，不隐式转换时区。
起止时间必须完整、顺序正确，跨度不超过 7 天。
记录位于 `response.data.result`，分页位于 `response.data.pageDTO`。
该接口不返回调度 `jobUuid`，不能直接用运行记录标识查询调度日志。
需要旧接口的 `remark` 和游标时查看 `oapi api schema app.records-cursor`，
按接口契约提供 `appId` 或时间范围及 `minId`。

## 通用接口调用

友好命令覆盖全部常用场景；其余参数组合用通用入口：

```powershell
xbot-cli oapi api list --search queue --json
xbot-cli oapi api schema job.status --json
xbot-cli oapi api schema 710481967730900992 --json
xbot-cli oapi docs --json
xbot-cli oapi docs 710460954567454720 --json
xbot-cli oapi api call job.status --body '{"jobUuid":"<jobUuid>"}' --json
xbot-cli oapi api call queue.items --path '{"queueUuid":"<queueUuid>"}' --query '{"status":"on hold","page":1,"size":10}' --json
```

接口选择支持名称、官方文档 ID 或已登记的 `/oapi/...` 路径模板，不接受任意 URL。
`--body`、`--query`、`--path` 接受 JSON、`@file`、`-` stdin；
一次只能有一个参数读取 stdin。GET 使用 `--query`，
路径占位符使用 `--path`，每个值按独立路径段编码。

支持企业账号、应用、机器人、调度、队列和文件接口。
创建、修改、删除、转移、启动、停止、重试、入队、出队和上传需要 `--yes`；
取得对应动作授权后才执行。先离线预览：

```powershell
xbot-cli oapi api call job.stop --body @stop.json --dry-run --json
xbot-cli oapi api call file.upload --file .\input.xlsx --dry-run --json
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
`schema oapi api call` 给出 CLI 输入约定，
`oapi api schema <endpoint>` 给出官方字段表。
模拟测试不能代替企业服务验收，未真实调用时明确报告未验证。

维护文档契约时运行 `node scripts/sync-platform-docs.cjs`。
该脚本仅读取登记的公开文档，更新事实字段表和来源索引，
不需要企业凭据，不调用生产 API。
