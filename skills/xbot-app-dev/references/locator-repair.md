# XPath 生成与元素失效修复

用于 ref 转长期定位、旧 XPath 零匹配、ROBULA+、iframe/Shadow 适配和修复提议。一般网页操作不必加载本文件。

修复入口先按 [平台识别与模块选择](automation-routing.md) 确认失败调用的实际对象属于 Web；XPath 字符串不能单独作为平台证据。本文件不接收原生 Windows 控件修复。

## 核心原则

先找回业务目标，再生成路径。ROBULA+ 接收已确定的元素，不根据失效 XPath 恢复业务意图；Agent 负责错误分类和步骤编排，不猜 XPath。生成器的输出必须经过当前 DOM 查询及节点身份验证。

只有旧 XPath、没有可恢复的历史身份时，输出 `REVIEW_REQUIRED_NO_IDENTITY_BASELINE`。可以读取流程输入、旧 Selector、历史快照和日志补充真实身份，不能用当前候选反过来编造基线。

“唯一匹配”不等于“业务对象正确”。页面、订单、按钮所有可观察字段不变而后端处理被改写，仍可能误识别；写入动作须有业务对象检查、明确授权、检查点和幂等/对账契约。证据不足时保留提议，不因采用了算法就扩大动作权限。

## 身份契约

正常运行时采集 [repair-contract.json](../templates/repair-contract.json) 描述的字段：

- 顶层页面 ID、记录 ID、origin/path、业务 query/hash；区分业务参数与跟踪参数。
- 元素业务键、tag/role、动作名或预先批准的名称集合。
- 记录行、区域、工作流、活动弹窗的业务身份。
- 每层 iframe 的源站、路径/语义参数和业务键；不能只存 origin/depth。
- 每层 Shadow Host 的业务键与层级；不能只存标签名。
- 页面就绪/登录/权限/验证码/错误状态的证据。
- 已验证的候选及适用条件、资产版本、源文件/块 ID、共享引用范围。
- 动作影响、完成标志、幂等键和允许重试的条件。

没有通用 DOM 属性可以代表所有站点的业务身份。脚本的 `data-contract-v1` 仅适用于明确提供相应数据契约的页面；真实站点须先提供并验证等价适配，不注入属性伪造身份，也不删除检查来强行通过。

## 修复链路

修复提议前先执行 [AI 用途分析](locator-usage.md)。当前 Agent 阅读原流程，脚本校验调用点和原文，交接时回读当前源码。集合查询后选一个依然属于集合用途，不能进入单元素修复器。

集合用途使用独立 [集合契约与兼容验证](collection-repair.md)；只有验证通过才能修改相关业务步骤。修复保留有效旧、新父子定位并调整判断逻辑，不能直接把旧元素替换掉；不可修复或不确定时不改业务流程，交付尝试记录和建议。

1. 读取原错误和 run_id，确认是动作之前的零匹配。XPath 语法、权限、业务、连接错误分别处理。运行未结束继续等待同一 run_id；结果未知先对账。
2. `status` 核对应用；`flow get` 读取现状及相关调用点。取得当前完整快照；页面变化后重新快照，不沿用旧 ref。
3. 诊断有效新旧路径的匹配数和身份；相同 DOM 节点去重。旧路径命中错误对象时不能保留为可执行回退。
4. 先核对页面/记录和就绪状态，再按身份过滤当前候选。loading 可在统一等待预算内重新观察；登录、权限、验证码或错误状态停止改定位。
5. 排除隐藏模板，多个可见目标继续用业务作用域消歧；仍不唯一则拒绝，不取首项、不加无依据的序号。
6. 检查目标及祖先 hidden/display/visibility/opacity、disabled/aria-disabled/inert/pointer-events；检查点击点遮挡。iframe 内点击点逐层映射到父文档，检查父页面遮挡；未适配 frame transform/zoom 时拒绝。离屏先滚动后重新复核，不使用坐标点击。
7. 对唯一已确认目标运行用户加权算法和固定版本 ROBULA+。两者独立返回候选/错误，逐段查询，必须唯一命中同一个实际节点。结构序号、动态 ID、泛化 `//tag` 候选仅供复核。
8. 生成 [repair-plan.json](../templates/repair-plan.json) 提议，包含版本、旧/新路径、身份/状态证据、算法来源、文档代次、调用点及影响。`persisted:false` 不能写成“已修复应用”。
9. 写入前重新核对应用、代码原文和资产版本；执行前重新核对文档代次、记录身份、唯一性和可操作性。reload、候选变化、版本冲突或 ref 失效使原提议作废。
10. 在已有授权内最小修改；共享 Selector 新建版本并替换明确引用。实际写入后回读，静态检查，再在隔离运行中重新确认身份、执行一次并验收业务结果。成功后提升资产版本并保存历史；配置回滚不撤销业务提交。

## 只读脚本

入口：[scripts/locator-repair.cjs](../scripts/locator-repair.cjs)。

```powershell
node <skill目录>/scripts/locator-repair.cjs --help
node <skill目录>/scripts/locator-repair.cjs capture --profile data-contract-v1 --ref <最新ref> --robula-source <已核验的index.ts> --out <新资产.json>
node <skill目录>/scripts/locator-repair.cjs diagnose --asset <资产.json> --robula-source <已核验的index.ts> --out <新诊断.json>
node <skill目录>/scripts/locator-repair.cjs propose --asset <资产.json> --usage-analysis <用途receipt.json> --robula-source <已核验的index.ts> --out <新提议.json>
node <skill目录>/scripts/locator-repair.cjs revalidate --asset <资产.json> --usage-analysis <用途receipt.json> --plan <提议.json> --robula-source <已核验的index.ts> --out <新复核.json>
```

命令是 Skill 内的 Node helper，不是现有 `xbot-cli repair` 子命令。输出文件只允许新建，不覆盖。脚本只调用 `status`、`flow get`、`web snapshot` 和只读 `web_evaluate_playwright`，不会点击、建库、改流程或提交资产。`propose/revalidate` 必须提供用途 receipt；旧 v10 资产可继续诊断，但提议前须补用途分析。`--cli` 可指定实际 CLI 原生可执行文件；客户端路径使用既有 `XBOT_SHELL_CLI/XBOT_LAUNCHER` 环境变量。操作前脚本核对当前 app_id 与资产，跨应用拒绝。

`capture` 要求明确选择 `data-contract-v1`：页面使用 `data-app/data-page-id/data-record-id/data-page-state`，目标使用 `data-business-key`，作用域使用 `data-region/data-record-id/data-workflow-key/data-dialog-id`，frame/host 使用 `data-frame-key/data-host-key`。默认业务 query 键为 `record`，hash 整体参与身份。它是受控页面参考实现，不能直接宣称适配任意网页。

ROBULA+ 不随 Skill 嵌入第三方源码。显式提供本地 `cyluxx/robula-plus` 修订 `5f33589dadba0f71075294611aa79801da123e40` 的 `src/index.ts`，脚本核对 SHA-256：

```text
a674d99763c8045456add711d549f53bfe6103f025b338b30e1cec398b2b29d4
```

不自动联网、不静默更换上游、不降级成 Agent 手写 XPath。缺少依赖或校验失败时明确报错。解析该 TypeScript 源码需要提供 `stripTypeScriptTypes` 的 Node 运行时；不支持时升级运行时或在单独评估中确认新的构建产物及哈希。上游许可状态仍需确认，未并入发行包。

加权算法与 ROBULA+ 保留各自预算，生成时再次核对目标。浏览器后端长字符串可能被压缩截断；脚本用 gzip/base64 返回结果，解析失败时停止，不能将不完整 JSON 修补为成功。单次 CLI 调用有超时，但这不是全链路硬 deadline；生产协调层还需统一等待/生成/回读预算。

密集页可能触发 ROBULA+ 适配器的 `ROBULA_SCOPE_LIMIT`（当前整页预检上限 20,000 个元素，另有属性数与求值预算）。保留具体算法错误；另一生成器仍可给出经同节点验证的路径，但不得写成“两种算法均成功”或因此放松业务身份校验。不自动提高上限或把页面副本裁剪后冒充整页 ROBULA+ 结果；若后续增加限定区域生成，需独立验证区域锚点、相对路径及原始 DOM 身份。

## 状态与操作

下列名称属于 helper 协议，不是影刀原生错误码。

| 状态 | 解释及下一步 |
|---|---|
| `CAPTURED` | 已采集身份和路径；检查强身份标志，不等于运行已通过 |
| `DIAGNOSED` | 已统计旧候选，不写入、不执行 |
| `REPAIR_VALIDATED` | 当前页面候选已验证，仍是提议 |
| `ACTION_REVALIDATED` | 当前页面复核通过，不是动作执行凭证，也不跨导航有效 |
| `WAIT_PAGE_LOADING` | 有界等待后获取新快照；预算耗尽则停止 |
| `REVIEW_REQUIRED_*` | 无基线、身份弱、文案变化、离屏或路径脆弱，先复核 |
| `BLOCKED_*` | 页面不符、歧义、目标缺失、不可操作、旧快照/提议、路径错误或不支持，保持零动作 |

ref 数量不足也可能是可访问性快照未暴露目标，不能直接认定业务元素已删除。需要只读 DOM 证据区分，未能可靠重新识别就停止。

## iframe 与 Shadow DOM

iframe 用路径数组，每段进入下一文档；open ShadowRoot 用段内 `/xbotShadowRoot` 标记。它们是影刀 Selector 协议，不是可拼接后交给普通 XPath API 的表达式。

```javascript
[
  "//iframe[@data-frame-key='order-frame']",
  "//*[local-name()='order-panel'][@data-host-key='order-primary']/xbotShadowRoot//button[@data-automation-id='approve']"
]
```

普通 DOM 优先 `find_by_xpath`；复杂上下文先核验 `web_build_selector_by_xpath` schema，验证分段路径并仅在确需建库时 `web_review` 保存，CodeFlow 使用工程 Selector。closed ShadowRoot 和 ShadowRoot 内 iframe 当前不支持。

路径生成与执行通道分开验收。曾实测嵌套 open Shadow 的路径有效，而 `web click ref` 后端报 `xpathSegments`；同一路径建库后原生执行成功。遇到此类错误先确认零动作，再按 schema 验证支持的 Selector 通道，不无条件重放原动作。

## 验收与剩余风险

按 [web-acceptance.md](web-acceptance.md) 回归正向、拒绝和结果未知三类路径。独立业务回执应校验目标对象、动作次数与完成状态；不能只校验修复器自身的标签。真实断网与注入错误信号分别记录。

Skill 是 Agent 工作规程，helper 是按需运行的只读工具；二者不是常驻监听器、工程事务或恢复执行器。DOM 复核与点击存在竞态窗口，全部身份相同仍可能业务语义漂移；大规模虚拟列表、复杂 CSS、崩溃恢复和多进程修改均需专门适配验证。
