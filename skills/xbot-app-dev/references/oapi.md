# 企业开放 API 备用入口

仅在企业用户明确选择开放 API 运行、调度或查询平台日志时使用。默认仍是 task 运行应用、flow 运行 Studio 流程；凭据存在、客户端离线或命令失败都不自动切换。结果未知先查询原任务，避免跨通道重复执行。

```powershell
xbot-cli help oapi
xbot-cli help oapi auth login
xbot-cli oapi auth status --json
xbot-cli oapi api list --json
xbot-cli oapi api schema job.status --json
```

鉴权按现场 auth login schema 使用受控凭据来源；不在对话、日志或脚本内写真实密钥。环境配置与专有云地址以实际部署为准，不擅自切企业或账号。

运行前查 app/robot 的实际身份、发布版参数及占用，再按 job start 的 schema 预览/执行；状态、日志使用同次 jobUuid。配置 scheduleUuid、一次任务 taskUuid、应用执行 jobUuid 与客户端 task_id、Studio run_id 不可互换。通用 API 用 oapi api schema 明确 endpoint 的 path/query/body，不猜 URL。

原子调度可用的幂等参数应在首次提交前确定并保留，不能拿新的键重试未知动作；请求是否可重放还须看该接口实际契约。异步日志保留 request_id 并续查同一请求，分页、游标及缓存时限按现场 schema/接口说明。

--yes 只是 CLI 确认；启动、停止、账号修改、队列消费/写入等仍需相应授权。--dry-run 不联网、不验证密钥或企业权限。接口返回成功与机器人运行、业务完成分别核验。没有真实调用只报告契约/静态已检查。
