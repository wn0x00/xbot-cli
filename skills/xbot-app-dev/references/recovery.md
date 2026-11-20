# 恢复、保存与发布

先核对已发生的动作、现状和任务范围。运行状态未知不热改、不重新执行，不因读取日志或后续快照失败重放业务。

| 情况 | 下一步 |
| --- | --- |
| MCP/客户端通道离线 | 查实际连接、登录与安装；不自动切 OAPI 或换账号 |
| VIEW_MODE_READONLY | 只读查看可继续；需要写入时正常进入编辑模式 |
| studio_already_open / 上下文冲突 | 读取现状并保护草稿，不用 force 抢占 |
| FLOW_FILL_FAILED | 查 fill_errors 与已应用结果，只修失败块 |
| FLOW_STATIC_ERRORS | 静态失败，不宣称业务已运行 |
| FLOW_RUN_TIMEOUT | 保留 run_id，通过 flow wait/logs 续查，并用 status 核对当前运行 |
| 暂停/停止 | 保留状态，按用户后续指示决定，不自动再启动 |
| 动作已返回但 snapshot 失败 | 保留 applied 及原结果，只补观察 |

修改走 CLI/客户端；只有获准的指令元数据任务按 [专用流程](../../xbot-development-standards/references/metadata-delivery.md) 关闭设计器维护。备份与恢复见 [差异备份](../maintenance/application-git-backup.md)。

保存、同步和发布分别处理：app save 是保存；app sync 有同步/上传并关闭 Studio 的副作用，不能拿它作通用无上传关闭；app publish 需要对应发布授权和 CLI 确认。先查现场 help/schema，指定明确资产及候选。回收和强制打开也按具体范围确认，已有明确授权不重复问。

synced、studio_closed、uploaded 各是不同证据；关闭不证明同步成功，无变化也可能不上传。未执行同步或发布就报告本地候选。业务通过不自动授权推送/发布，也不默认再运行一次已发布应用。
