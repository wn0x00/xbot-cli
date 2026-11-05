# 维护、排障与发布边界

## 维护顺序

1. `status` → `app open` → `flow list` → `flow get`，先读源码/块和参数。
2. 使用 `--expect-app <app-id>` 锁定期望的当前应用；最小修改，CodeFlow 用 `flow code edit`，VisualFlow 只改目标块。
3. `flow check` 仅检查静态诊断；需要运行验收时使用 `flow verify` 回归受影响流程。
4. 汇报 flow_id、run_id、Out/日志/快照；未运行的路径不声称已验证。

## 编辑模式

- `app open` 是可保存、同步、发布的编辑模式。
- `app view` / `shadowbot:View` 是只读临时副本，不能用于开发。
- 所有修改通过 CLI/MCP，不直接改影刀应用目录文件。
- `studio_already_open`：先读状态并保存当前工作；不要直接 force 丢改动。

## 常见错误

| 症状 | 处理 |
|---|---|
| `api_4011` | 请用户登录影刀，不自动选择账号 |
| `MCP_OFFLINE` | 检查 Shell 登录和 Studio；首次运行建 venv 可能较慢 |
| `VIEW_MODE_READONLY` | 关闭 View，用 `app open` 重开编辑模式 |
| `flow_kind_mismatch` | CodeFlow 用 `flow code`，VisualFlow 用 `flow visual` |
| `FLOW_STATIC_ERRORS` | 先修 `data.static_errors`，尚未运行 |
| `FLOW_FILL_FAILED` | 只修 `fill_errors` 中的 block_id |
| `FLOW_RUN_TIMEOUT` | 保留 run_id，用 `flow wait <run-id>` 继续等待，不启动第二份流程 |
| `FLOW_RUN_PAUSED/STOPPED` | 停止自动操作，等待用户决定 |
| 退出码 10 | 展示高风险动作，用户同意后追加 `--yes` |

## 保存、发布、回收

开发运行完成不等于必须发布。只有用户要求时才继续：

```bash
xbot-cli app save
xbot-cli app sync
xbot-cli app publish <app-id> --log "版本说明" --dry-run
# 展示预览并取得明确同意后：
xbot-cli app publish <app-id> --log "版本说明" --yes
xbot-cli task run <app-id>
```

`app sync` 对应 `shadowbot.shell-cli.exe studio current sync`：同步当前编辑应用并关闭
Studio，不发布应用。建议传 `--expect-app <app-id>`，可先加 `--dry-run --json` 预览。
JSON 中 `synced` 表示后端报告同步完成，`studio_closed` 表示观察到编辑器已关闭，
`uploaded` 仅表示观察到本次上传日志；无改动时可能不上传。调用失败不会仅凭编辑器已关闭
判定成功，也不会自动重复同步或发布。

`app recycle` 和 `app open --force` 也需要明确确认及 `--yes`。发布只支持影刀控制台“我的应用”，不支持 marketplace 或导出 `.xbot`。

`task status/logs/stop <task-id>` 用于已发布应用的控制台任务，不能传入 Studio 的 run_id。`--dry-run` 仅验证离线输入并预览，不验证当前编辑模式或后端接受性。真实客户端未执行的保存、同步、发布和运行路径，汇报时明确标为未验证。
