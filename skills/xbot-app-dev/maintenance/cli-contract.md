# 维护命令导航

所有输出、上下文、确认和失败语义统一见 [CLI 契约](../references/cli-contract.md)。本包不维护另一套静态 schema；现场 help/schema 优先。按任务查一个命令，不必全量枚举。

| 目的 | 发现入口 | 进一步资料 |
| --- | --- | --- |
| 版本、客户端连接和诊断 | version / status / doctor | [环境映射](environment-and-paths.md) |
| 应用或指令身份、查看/打开 | app / activity | [资产分类](asset-types.md) |
| 流程、CodeFlow、In/Out | flow / flow code / flow params | [CodeFlow](../references/codeflow.md) |
| VisualFlow 实例、字段、变量 | flow visual | [VisualFlow](../references/visualflow.md) |
| 已发布应用及运行日志 | task run/status/logs | [运行](../references/run-existing.md) |
| Studio 运行及超时续查 | flow run/verify/logs/wait，status | [验证](validation-and-delivery.md) |
| 浏览器、网页、ref、XPath | web browser / web | [Web 模块](../modules/web.md) |
| 窗口搜索、激活、快照与动作 | win32 list/activate/snapshot/inspect/click | [Windows 模块](../modules/win.md) |
| 动态工具字段 | tool studio/browser list/schema/call | [操作方案](cli-operation-playbooks.md) |
| 用户明确选择企业 API | oapi；通用接口另查 oapi api schema | [企业 API](../references/oapi.md) |

例如先 `xbot-cli help flow params set`，再 `xbot-cli schema flow params set`；不要给所有子命令机械添加相同参数。不存在的命令不猜名字、URL 或 flags。
