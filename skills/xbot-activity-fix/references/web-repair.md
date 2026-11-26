# Web 定位维护

目录：[诊断与兼容](#diagnosis) · [用途分析](#usage) · [单元素 helper](#single) · [集合 helper](#collection) · [验收](#acceptance)。进入前先按 [主文件](../SKILL.md#元素故障分流) 核实失败接收对象属于 Web；连接、快照/ref 和 CLI 语法见 [操作参考](../../xbot-cli-doc/references/operations.md#web)。

## 先选适用路径

| 当前条件 | 安排的任务与工具 |
| --- | --- |
| 登录/权限/未就绪、参数或普通业务错误 | 执行 F2–F3 常规诊断，按根因修复；不为使用定位 helper 先准备整套资产 |
| 普通站点的 XPath 失效 | 读原调用和历史目标证据，观察真实 DOM，验证候选与旧新兼容后最小修复；缺历史身份时先补证据。data-contract-v1 不能直接套用 |
| 有可信历史资产且页面满足 data-contract-v1 | F3 做用途分析和单元素只读提议；F4/F5 再安排写入、原入口与业务验收 |
| 集合且能取得独立完整键清单与逐键字段 | F3 用集合合同验证；只支持普通 DOM，不能交给单元素引擎 |
| 动作已发出或结果未知 | 先查原执行与业务对账，不安排换定位重试 |

任务清单只加入实际选择路径所需项；helper 不支持不等于整个故障无法维护，可继续已授权的源码分析和常规诊断。候选修复仍须下面的身份、兼容与动作规则，不以手工猜 XPath 绕过证据缺口。

**运行前只检查所选路径：** 普通故障无需准备下表全部材料。在本 skill 目录运行辅助脚本；相对输入/输出路径也以该工作目录为准，证据输出使用工程外新路径。

| 路径 | 必要环境与输入 | 入口和产物 | 缺条件时的下一步 |
| --- | --- | --- | --- |
| 普通常规诊断 | 真实失败调用、原身份与当前对象证据；现场 CLI/SDK 契约 | 原调用/日志和按需 DOM 观察 → 根因或待核实项 | 补调用链、前置和目标证据；不先装 helper 依赖 |
| 用途分析 | Node、Python 3.8+ AST、实际原生 CLI、原流程及历史资产 | locator-usage.cjs capture/validate → input、analysis、receipt | 动态/封装或 VisualFlow 未适配时保留人工分析结论；不能伪造自动放行 |
| 受控单元素提议 | Node 有 `stripTypeScriptTypes`、指定哈希的本地 ROBULA+ 源码、在线 Web 通道、data-contract-v1 及可信历史身份；propose/revalidate 需有效 usage receipt | locator-repair.cjs → 采集/提议/即时复核 JSON，均不动作或持久写入 | 缺源码/能力/页面适配时标该 helper 受阻；继续可靠常规诊断，不伪造属性或更换上游 |
| 普通 DOM 集合 | 应用 xbot SDK、原 browser 对象、独立完整键清单与逐键字段合同 | collection-compat.py 的函数 → 只读证据；传 Flow Out 用 public_result | 无独立覆盖来源先补清单；frame/Shadow 集合需另适配，不改成 single |

`node -p "typeof require('node:module').stripTypeScriptTypes"` 可离线检查所需 Node 能力；`function` 仅证明该 API 存在。`--help` 不验证第三方源码、浏览器在线或目标适配。helper 返回提议后，实际写入、原入口运行及业务验收仍按 F4–F7 安排。

<a id="diagnosis"></a>
## 先判断原因，再修定位

取得错误、run_id、原入口/调用点及前置；先判断动作是否已发出，unknown 只对账。登录/权限/验证码、未就绪、业务数据变化、语法/连接错误不按定位失效处理。快照为空或旧 XPath 零匹配不证明目标已删除。

修复须有可信历史业务身份：原输入、旧资产/Selector、历史快照或日志，明确页面/记录、区域/frame/host、元素含义、动作和后置条件。不能用当前候选反造基线。ROBULA+ 为已确认节点生成路径，不恢复失效 XPath 的业务意图。

| 旧新观测 | 决策 |
| --- | --- |
| 仅旧或仅新有效 | 确认业务身份后使用，保留另一版本的有效历史和适用条件 |
| 两者命中同一实际 DOM 节点 | 按节点身份去重，操作一次，不靠 Python 包装对象 is 或相同文字判断 |
| 隐藏模板/非活动区域副本 | 排除副本；不能绕过目标禁用或业务等待状态 |
| 两个不同可见目标 | 用业务键、区域、弹窗或真实版本条件继续消歧；仍不唯一停止 |
| 均无匹配 | 统一等待预算内观察，到期明确失败，不取相似元素 |

查找、等待和兼容共用有界预算。禁止 XPath union 后取首项、无依据 `[1]`、捕获动作异常后换定位重放。旧定位错误命中时保留历史但停用执行；共享资产新增版本，只替换已确认引用。父子定位仅按真实数据流联动，不按 `./` 前缀全局替换。

目标唯一还需查可见/启用、祖先 hidden/display/visibility/opacity/disabled/aria-disabled/inert/pointer-events 和实际点击点遮挡；iframe 逐层映射和检查父文档，未适配 transform/zoom 拒绝自动采用。离屏按授权滚动后重新快照复核，不用旧坐标。

<a id="usage"></a>
## 先分析定位用途

当前 Agent 读完整调用链并分类，脚本采集/校验，不需新增模型服务。分类对象是**使用位置**，同一字符串可被不同调用点以不同方式消费。

| classification | 处理 |
| --- | --- |
| single | 查询契约为单元素，包含循环中逐行找一个按钮；证据通过后进单元素引擎 |
| collection | 遍历/计数/提取集合，进集合契约 |
| collection_then_single | 先查集合再选一个，仍需修复集合；位置选择另审，参考解析器拒绝 |
| mixed / unknown | 拆分真实失败调用点或补证据，不改共享定位 |

历史匹配数为一、变量名或当前零匹配不能决定用途。Agent 根据 [usage-analysis.json](../assets/usage-analysis.json) 填分类、consumer、相关调用点、逐行原文、理由和真实缺口，不改 observations 迎合结论。

在本 skill 目录运行，输出均用新路径：

```powershell
node scripts/locator-usage.cjs capture --flow <flow-id> --locator '<原XPath>' --asset <历史资产.json> --expect-app <app-id> --out <usage-input.json>
# 读取完整 source/observations 后填写 analysis.json。
node scripts/locator-usage.cjs validate --input <usage-input.json> --analysis <analysis.json> --out <usage-receipt.json>
```

资产须包含失败原表达式的真实已验证关联，无 asset 可分析但不能放行修复。调用点 ID 为 `py:行:列`（行 1 起、列 0 起），只有已对应真实失败调用才用 --callsite 缩小范围，不能排除集合调用取巧。propose/revalidate 必须带 --usage-analysis 并回读原流程；source_sha256、资产或分析变化使旧 receipt 作废。

CodeFlow 适配器需 Python 3.8+ AST，可 --python 指定，只支持直接 find_by_xpath/find_all_by_xpath、字符串及保守单次常量赋值；不执行源码。动态/别名/跨函数封装保留未验证，AST 不证明接收对象平台或全部消费逻辑。VisualFlow 可采集但缺已核验块适配，自动交接 REVIEW_REQUIRED_USAGE_UNVERIFIED，不能猜字段。

USAGE_CONTEXT_CAPTURED 仅有材料，USAGE_ANALYZED 仅证据一致；只有 single_repair_allowed=true 可进入原单元素引擎。COLLECTION_REPAIR_NOT_SUPPORTED 指该引擎不承接集合，不是业务失败或集合已经修复。冲突/材料变化/缺口保留报告重新分析，不能清空 uncertainties 强行通过。

<a id="single"></a>
## 单元素提议与即时复核

原身份契约参考 [repair-contract.json](../assets/repair-contract.json)：页面 origin/path、业务 query/hash 与记录 ID、目标业务键/tag/role/动作、行/区域/工作流/活动弹窗、每层 frame 的路径/业务键、host 层级、就绪/权限状态、历史候选、资产版本及共享引用、动作影响/幂等/对账。origin/depth 或标签名不足以识别上下文。

[locator-repair.cjs](../scripts/locator-repair.cjs) 只读调用 status、flow get、web snapshot/eval，不点击、改流程或建库；不是 xbot-cli repair。--cli 可指定实际原生 CLI，输出不覆盖。受控适配 data-contract-v1 依赖页面 data-app/page-id/record-id/page-state、元素 data-business-key、区域 data-region/record-id/workflow-key/dialog-id、frame-key/host-key，默认 query 的 record 和完整 hash 参与身份。**不适配任意站点，不注入这些属性伪造合同**；实际站点需要可信等价适配。

```powershell
node scripts/locator-repair.cjs --help
node scripts/locator-repair.cjs capture --profile data-contract-v1 --ref <当前ref> --robula-source <index.ts> --out <新资产.json>
node scripts/locator-repair.cjs diagnose --asset <资产.json> --robula-source <index.ts> --out <诊断.json>
node scripts/locator-repair.cjs propose --asset <资产.json> --usage-analysis <usage-receipt.json> --robula-source <index.ts> --out <提议.json>
node scripts/locator-repair.cjs revalidate --asset <资产.json> --usage-analysis <usage-receipt.json> --plan <提议.json> --robula-source <index.ts> --out <复核.json>
```

capture 适用于正常时建立真实基线；已失效且无历史身份时返回 REVIEW_REQUIRED_NO_IDENTITY_BASELINE，不能拿新 capture 冒充历史。计划结构见 [repair-plan.json](../assets/repair-plan.json)，登记结构见 [locator-registry.json](../assets/locator-registry.json)。

加权算法和 ROBULA+ 独立产生候选/错误，逐段验证唯一命中同一真实节点，不用结构序号或泛化路径自动采用。第三方源码不随包附带，须显式提供本地 `cyluxx/robula-plus` 修订 `5f33589dadba0f71075294611aa79801da123e40` 的 src/index.ts，SHA-256：

```text
a674d99763c8045456add711d549f53bfe6103f025b338b30e1cec398b2b29d4
```

运行时须支持 Node stripTypeScriptTypes；缺依赖/哈希不符明确失败，不自动联网、更换上游或降级成手写 XPath。ROBULA_SCOPE_LIMIT 当前预检上限 20,000 元素及其他预算；不自动调大/裁剪页面。另一算法成功仍可作为经验证候选，但不能报告两算法均成功。结果压缩/解析失败不修补 JSON 冒充成功，单调用超时不是全链路硬 deadline。

iframe 使用逐文档 segments，open ShadowRoot 使用 `/xbotShadowRoot` 影刀协议，不能拼成普通 XPath。修复 helper 比 CLI 生成能力更窄：不支持 closed ShadowRoot 或 ShadowRoot 内 iframe；复杂路径生成、ref 点击、Selector 和影刀运行分别验收。

| helper 结果 | 含义 |
| --- | --- |
| CAPTURED / DIAGNOSED | 采集/诊断，未执行或写入 |
| REPAIR_VALIDATED | 当前候选提议通过，persisted=false |
| ACTION_REVALIDATED | 当前文档即时预检通过，不跨导航、不证明授权/业务成功 |
| WAIT_PAGE_LOADING | 在剩余预算内重新观察 |
| REVIEW_REQUIRED_* / BLOCKED_* | 缺基线、身份弱、歧义、不支持、状态/路径变化等，保持零动作 |

写入前再次核对工程、原文、source 摘要、调用点和资产版本；执行前重查文档代次、业务记录与可操作性。reload、ref/候选/资产变化使提议作废。在授权范围最小写入，回读/静态，再按原入口验证；配置回滚不撤销业务提交。

<a id="collection"></a>
## 集合、父子定位与业务完整性

集合用于 collection/collection_then_single，不把集合缩成唯一节点交单元素引擎。先按 [collection-analysis.json](../assets/collection-analysis.json) 从源码反推区域、成员键、字段、过滤/排序/遍历、空集含义及 dependent_locators，区分直接事实、推断和缺口；成员分支名称或历史数量不是全量清单。

[collection-compat.py](../scripts/collection-compat.py) 是普通 DOM 只读参考模块，不是 CLI 或自动补丁器。Agent 在读取真实证据后构造字典并在目标应用用已核实的 browser 对象调用：

| 字段 | 必要内容 |
| --- | --- |
| schema_version / classification | 1，collection 或 collection_then_single，真实缺口解决后才 uncertainties=[] |
| page / region | origin/path/完整 query/fragment、就绪与页面身份；唯一可见区域的定位及业务断言 |
| variants | 旧/新父相对 XPath 和版本；不能因新版存在丢掉有效旧版 |
| coverage | kind=exact_keys，keys 来自本次输入、可靠业务接口或独立清单，不能复制待修 DOM 自证 |
| member | 键属性、身份/启用约束、字段读取及旧新子路径；相对查询不越出成员 |
| selection / allow_empty | all 按明确键顺序，或 business_key；空集须业务允许且独立来源证明确实为空 |
| merge_policy | same_region_union 仅在同一实际区域、已证实允许两版成员混合时使用 |

locator 为 `{kind:"xpath",value:"...",version:"old"}`；原子审计当前只支持普通 XPath，拒绝越界轴/union 和 Selector 集合，iframe/Shadow 集合尚未支持。不能改标志绕过 ATOMIC_SELECTOR_PROFILE_UNSUPPORTED。

业务采用必须 `validation_profile:"business"`，省略时的结构通过不能采用补丁；另需：

- coverage.source 为独立清单出处；member.expected_by_key 覆盖每键**全部读取字段**，member.identity_fields 明确租户、账期、名称等身份；来源不抄当前候选 DOM，脚本不能认证来源真实性。
- page/region 的业务身份断言与登录/权限/就绪证据可靠。名称/允许值集合不替代逐键基线，同名不同租户、字段互换和可见镜像须拒绝。
- 原流程确需点击时，member.action_field 指真实动作字段，member.action 给 kind=click、已核实 tag/role、attributes、prior_outcome=not_attempted 或 confirmed_not_applied；已执行/未知不进预检。纯读取不虚构 action 字段。
- 写动作须核验显示键与后端记录/动作绑定，如真实 href/表单主键；隐藏事件无法核验、虚拟行复用或同内容节点替换而无可靠代次时停在只读提议，不能把结构通过提升为业务授权。

**只读集合最小合同示例：** 下例说明实际解析器字段，业务键/属性/XPath 都是虚构样本，不是对真实站点的已验证定位。使用前用独立来源填入完整 keys 与 expected_by_key，保留真实缺口，不能照抄 uncertainties=[]。

```json
{
  "schema_version": 1,
  "classification": "collection",
  "validation_profile": "business",
  "uncertainties": [],
  "merge_policy": "same_region_union",
  "page": {
    "origin": "https://example.invalid", "path": "/orders", "query": "tenant=T1", "fragment": "",
    "locator": {"kind": "xpath", "value": "//main[@data-page='orders']", "version": "known"},
    "attributes": {"data-tenant": "T1", "data-state": "ready"}
  },
  "region": {
    "locator": {"kind": "xpath", "value": "//main[@data-page='orders']/section[@data-region='orders']", "version": "known"},
    "attributes": {"data-tenant": "T1", "data-region": "orders"}
  },
  "variants": [{"kind": "xpath", "value": ".//article[@data-order-id]", "version": "known"}],
  "coverage": {"kind": "exact_keys", "keys": ["O-001"], "source": "示例：经独立业务台账确认的本次完整清单"},
  "member": {
    "key_attribute": "data-order-id", "attributes": {"data-tenant": "T1"},
    "identity_fields": ["order_id"], "expected_by_key": {"O-001": {"order_id": "O-001"}},
    "fields": [{"name": "order_id", "read": "text", "locators": [{"kind": "xpath", "value": ".//span[@data-field='order-id']", "version": "known"}]}]
  },
  "selection": {"mode": "all"}, "allow_empty": false
}
```

此例无动作字段，仅用于读取。单个版本可以作为最小起点，兼容修复须加入真实验证的旧/新 variants；same_region_union 不能合并不同区域。复用 helper 函数时传入已确认业务页面对象；脚本内演示 main 会新建 edge 页面，不能不经适配拿它代替原流程的登录态和对象来源。

主要 API：

| API | 作用 |
| --- | --- |
| attribute_locator(...) | 为已确认属性生成正确引号的 XPath，不发现目标 |
| resolve_collection(browser, contract, budget_seconds=12) | 解析全部版本/字段和精确覆盖，返回内部句柄与证据 |
| public_result(...) | 移除不能跨 Flow 传递的句柄，供 result_json 等 Out 使用 |
| revalidate_collection(...) | 重新解析并使用新句柄，变化使旧结果作废 |
| screen_collection_candidates(browser, baseline, candidates, budget_seconds=30) | 候选 `{name,region_locator}` 只能改定位；两处不同区域均满足合同则 CANDIDATES_AMBIGUOUS，不按分数/位置选 |
| validate_business_action(...) | 当前仅点击预检，调用前准备视口；不滚动、不点击，BUSINESS_ACTION_READY 不证明执行 |

只有同一实际 DOM 节点去重；两个可见成员同键、字段旧新指向不同节点、缺字段/漏成员/误入或嵌套成员不明均拒绝，不跳过伪造完整。先验证父集合，再逐成员检查旧子路径，有效保留，失效新增版本并测试父子组合。SDK/XPath 异常直接暴露，不当作旧版不存在转用新版。

解析器在 DOM 任务内读语义、核对句柄并复读，降低撕裂；同内容替换仍可能无法辨认，重查与点击之间也没有事务。底层 SDK 阻塞不受步骤间预算硬中断。分页/虚拟列表须独立完整性协议；未知键、动态封装或无可靠绑定时不降级为结构匹配采用。

候选通过不自动写源码。协调 Agent 写前核对 source/usage receipt、资产版本与共享引用；仅改真实失败调用及关联字段，旧新兼容，不全局替换常量。子流程测试失败先处理；不自动发布。

<a id="acceptance"></a>
## 按影响验收与交付

按风险选用例，记录每例预期/实际、页面条件、匹配数、身份、选中版本、动作次数和独立业务回执：

- 旧版、新版、同节点共存、隐藏副本、不同可见目标、均无匹配；歧义/拒绝动作数为 0，唯一成功操作为 1。
- 身份错误（记录/query/hash/frame/host）、登录权限/未就绪、祖先禁用/遮挡、过期 ref、提议后 reload/版本漂移、缺基线及损坏 XPath。
- 集合追加混合成员、父子独立变化、字段缺失/多匹配/互换、漏页、同键错租户/账期、镜像区域、嵌套项、同内容节点替换及后端绑定缺口。记录 DOM/候选/目标规模，负载通过不代表业务正确。
- 动作后观测失败、超时或结果未知：只查原运行/对账，不能换 XPath 重新动作。“当前未见”不证明从未发生。

使用 [acceptance-case.json](../assets/acceptance-case.json) 按需建用例；真实环境与故障注入分开记录，不向生产页面复制旧按钮造兼容场景。分别给探索、写入、静态、原入口运行和业务结论，状态为 passed/failed/not_run/blocked/unknown。无法取得旧页面明确未验证。

可修复需有根因、身份、兼容及依赖验证；不可修复/不确定交付已尝试方法、拒绝原因和下一步，不改生产业务或旧资产。DOM 证据不能证明所有后端语义；helper 不是监听器、事务锁或恢复执行器。
