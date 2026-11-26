# 原 Skill 备份

`2026-11-26/` 完整保留改版前 `origin/main` 提交 `05a161ae3604d0ad3a7e7725297649e42936aa29` 的 `xbot-app-dev` 与 `xbot-development-standards`。两份目录的文件保持原文，供核对和恢复，不嵌入 CLI 的活动 Skill 安装清单。

当前入口为 `skills/xbot-cli-doc`、`skills/xbot-activity-dev` 和 `skills/xbot-activity-fix`。新版安装器遇到旧的已安装目录时，整体移入安装目标下的 `.agents/skill-backups/<唯一批次>/`，保留用户定制；工作区和 `--global` 分别处理，不主动修改另一处安装。
