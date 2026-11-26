# 运行已有应用

确认用户给的是已发布应用 ID 还是 Studio 流程；核对输入、账号和允许的业务动作，不为单纯运行修改工程。

```powershell
xbot-cli version --json
xbot-cli status --json
xbot-cli help task run
xbot-cli schema task run
xbot-cli task run <APP_ID> --json
xbot-cli task status <TASK_ID> --json
xbot-cli task logs <TASK_ID> --json
```

参数传法以现场 schema 为准；示例中的 ID 必须替换为真实返回值。Studio 流程使用 flow run/verify，前者运行，后者先静态检查再运行；运行信息用 status，日志/续等用 flow logs/wait，按命令传入原 run_id。先查对应 help，不把 task_id、run_id 和企业 jobUuid 互换。

运行失败先读 error.code 与部分 data。超时/断线保留原 ID，查询同次执行及业务结果；不能以结果未返回为由再次启动。只汇报实际到达的阶段、业务输出与未完成项。需要修复才进入 [维护流程](../maintenance/workflow.md)，不隐式修改或升级。
