# xbot-app-dev v10 升级说明

版本：**v10**。更新日期：2026-10-22。

v10 在 v9 的网页探索、CodeFlow/VisualFlow 开发和流程验收能力上，新增算法生成 XPath、失效定位修复辅助、相似元素组验证，以及 Python Windows 单控件修复模块，形成从报错分析到定位验证、兼容改造和业务验收的维护链路。

## 1. 升级概览

- **新增 XPath 自动生成。** 从当前快照 ref 取得真实 DOM 节点，结合 ROBULA+ 与加权算法生成路径，减少 Agent 手写 XPath。
- **新增修复前用途分析。** AI 阅读原流程，说明失效 XPath 的业务含义、单元素或集合用途，以及相关父子依赖。
- **新增相似元素组验证。** 按区域、成员业务键、字段和独立完整性清单检查候选，支持父子定位联动与新旧结构兼容。
- **增强复杂页面检查。** 增加 iframe/open Shadow 分段路径、页面与记录身份、可操作性、密集干扰和提议过期检查。
- **新增 Web/Windows 模块选择。** 按实际失败步骤的调用和对象来源加载对应模块，支持混合流程分步处理。
- **新增 Python Windows 修复。** 使用 UIA 查询真实业务窗口中的单控件，保留旧定位并追加新定位，可由影刀编码版及可视化流程调用。
- **增强修复验收与结果核对。** 增加隔离测试子流程、源码和资产版本复核、拒绝原因记录，以及微信适配中的本地防重放台账与只读对账。

v9 提供 XPath 使用与维护规范，未配套算法生成器和修复脚本。有效新旧定位兼容、歧义停止、最小修改和结果未知先核对等原则继续沿用，并在 v10 中补充可执行检查。

## 2. 功能对比总表

| 功能 | v9 | v10 | 升级类型 |
|---|---|---|---|
| 网页探索与 ref 操作 | 已有 `web snapshot/click/fill/eval` 等使用指导 | 继续支持，并进入 Web 模块 | 继承 |
| CodeFlow/VisualFlow 开发 | 已有创建、参数、编辑、回读和验收流程 | 继续作为承载方式；修复按实际失败调用接入 | 继承并增强 |
| XPath 用于流程定位 | 已要求从真实 DOM 获得并验证 XPath | 继续使用，增加算法生成与修复辅助 | 继承并增强 |
| ref 转 XPath | 无配套生成器；由 Agent 读取 DOM 后确定路径 | 解析真实节点，运行 ROBULA+ 与加权生成器，并核对同节点 | 新增 |
| 旧 XPath 零匹配 | 有维护指导，无随 Skill 提供的修复脚本 | 历史身份重新识别、诊断、提议和执行前复核 | 新增工具链 |
| 判断单元素/相似组 | 无专门分析器或交接凭证 | AI 读流程，输出用途和证据；支持的代码由 AST 校验 | 新增 |
| 解释失效 XPath 含义 | 无专门结构化输出 | 区分表达式含义、业务目标、字段、筛选和行为要求 | 新增方案与模板 |
| 相似元素组修复 | 有一般定位规范，无专用集合解析器 | 普通 DOM 下按独立清单、成员业务键和字段验证候选组 | 新增参考实现 |
| 父子 XPath 联动 | 无专项工具 | 检查父集合与相关相对查询，验证旧新父子组合 | 新增 |
| 新旧定位同时支持 | 已明确要求兼容、消歧、同节点去重 | 补集合解析、单元素复核和 Windows 变体检查 | 规则继承，执行能力增强 |
| iframe/Shadow DOM | 已指出普通 XPath 不能穿透，要求核对 SDK | 新增分段路径生成、上下文身份检查及部分原生执行验证 | 新增适配，有边界 |
| Web/Windows 自动分流 | 没有对应模块选择机制 | Agent 按失败调用和真实对象来源加载模块 | 新增规程 |
| Windows 定位修复 | 没有独立修复模块或脚本 | Python UIA 单控件修复，真实微信、编码版和可视化调用验证 | 新增实验能力 |
| 不确定结果与防重放 | 已有先核对、不盲目重试原则 | 增加微信适配的本地操作台账、只读对账和未知结果分流 | 继承并实现部分机制 |
| 修复报告 | 已要求记录运行和业务证据 | 增加原含义、合同、依赖、新旧版本、尝试及拒绝原因 | 增强 |

Skill 从 v9 的 14 个文件扩展到 38 个文件，包含 Web/Windows 模块、12 个脚本，以及用途分析、目标约束和修复提议模板。能力按模块和任务加载。

## 3. Web：新增了算法生成 XPath

### 从快照到 XPath 的实际链路

```text
xbot-cli web snapshot 的当前完整快照
 -> 取得实际 ref，例如 e12
 -> activePage.locator("aria-ref=" + ref).elementHandle()
 -> 得到当前真实 DOM 节点及所在上下文
 -> 加权算法、固定版本 ROBULA+ 分别生成候选
 -> 在对应文档/Shadow 上下文逐段回查
 -> 只接受唯一命中同一个真实节点的路径
 -> 输出主路径、备选、算法来源、错误和上下文信息
```

ref 是当前快照中的临时引用，编号本身没有编码 XPath。转换依赖浏览器后端将 ref 解析成节点，再用算法描述节点；不能离开页面，仅凭 `e12` 算出 XPath。

### 两种算法如何结合

| 组件 | 作用 | 不能解决什么 |
|---|---|---|
| 加权算法 | 给真实属性、文本和祖先关系分配成本，优先稳定、可解释的路径；过滤部分动态 ID、样式类和会话 URL | 无法单凭路径确定业务对象 |
| ROBULA+ | 对已经确认的节点生成另一组 XPath 候选，保留上游算法逻辑 | 不理解原流程，不找回失效定位的业务含义 |
| 组合验证 | 两种算法独立运行；回查同一节点后按质量和长度等规则选择，并保留错误 | 不保证页面下次更新后路径必然有效 |

例如原定位依赖变化的样式类，而目标有经过确认的稳定业务属性，算法可以为该属性生成新路径。该路径仍要通过原业务身份和实际节点验证；不能因为表达式更短或只匹配一个，就判定修复正确。

ROBULA+ 适配器限制整页规模、属性数、求值次数和时间。超过整页 20,000 个元素等预检条件时，会明确返回限制错误；另一生成器可以独立返回已验证的结果。每种算法分别记录结果与失败原因，单个生成器通过不会改变另一生成器的失败状态。

### 加权算法的优化规则

权重表示路径成本，数值越低越优先。它用于选择描述当前节点的路径，不是目标业务身份的置信分。

| 候选依据 | 成本 |
|---|---|
| `data-testid`、`data-test-id` | 1 |
| `data-test`、`data-qa`、`data-cy`、`data-automation-id` | 2 |
| 稳定 `id` | 3 |
| 其他符合条件的 `data-*` 属性 | 4 |
| `name` | 5 |
| `aria-label` | 7 |
| `title` / `alt` / `placeholder` | 9 / 10 / 11 |
| 稳定 class token | 12 |
| `href`、`src` | 14 |
| 完整文本 | `16 + 文本长度 / 100` |
| `role` / `type` | 18 / 20 |
| 文本前缀包含 | `30 + 前缀长度 / 100` |
| 唯一标签 | 45 |
| 结构回退 | 1000，并标记为结构型路径 |

具体优化包括：

1. **动态特征过滤。** 排除部分 React ID、UUID、长数字、哈希后缀、工具类样式，以及含会话和签名参数的链接。
2. **属性组合。** 除单属性候选外，尝试低成本属性的两两组合，在相似按钮较多时增加区分条件。
3. **祖先锚点。** 最多检查五层祖先，组合区域锚点与目标条件；分别尝试直接子级和后代关系，并增加层级成本。
4. **特殊字符和节点类型。** 处理 XPath 字符串单双引号混用、class token 边界，以及 SVG/自定义标签的名称匹配。
5. **确定性排序。** 按成本、路径长度和字典序排序，同样输入下保持明确的选择规则。
6. **同节点验证。** 每个采用的候选必须唯一返回原节点，不以“有结果”或“只有一个结果”代替身份检查。
7. **预算与结构回退。** 生成过程设置 1,500 次候选求值、2,500ms 时间检查和 200,000 次 Shadow 遍历访问预算。语义候选未通过或候选搜索预算耗尽时，可生成带位置的结构路径并再次验证；输出标记为 `structural`，交由上层修复规程复核，不能据此自动采用业务补丁。
8. **辅助诊断。** 输出采用原因、成本、求值次数和上下文信息；符合条件时附带经过唯一性验证的 CSS 建议。

这些预算在代码检查点执行，不是可以中断所有浏览器调用的全链路硬超时。完整加权算法见本文第 14 节。

## 4. Web：旧 XPath 零匹配后怎样修复

XPath 生成解决“已经知道元素是谁，怎样描述它”；失效修复还需要解决“现在谁才是原来的目标”。

业务合同是原流程对目标身份、成员范围、字段和动作结果的要求。修复以这些要求为依据，判断当前对象是否仍符合原用途。

```text
旧 XPath 当前零匹配
 -> 确认报错位置及动作是否已发生
 -> 排除未加载、登录、权限、错误页面、网络和通道问题
 -> 从原流程、历史资产和可信业务资料恢复目标合同
 -> 确认原用途是单元素还是集合
 -> 在正确页面/记录/区域内重新识别目标
 -> 唯一且身份充分，才运行生成器
 -> 输出修复提议并重新核对页面、源码和资产版本
 -> 隔离测试通过后，按授权最小修改和回读
 -> 静态检查、真实运行、业务结果验收
```

单元素检查不只看匹配数量，还看页面/订单或记录、区域、动作名称、iframe、Shadow Host，以及可见、启用、遮挡等状态。提议产生后页面刷新、记录切换或源码变化，原提议会失效，需要重新分析和验证。

如果只有一条失效 XPath，没有历史身份、原流程或可靠业务证据，当前方案会停止自动采用。不能把当前“最像”的候选抄成原身份，让它自己证明自己正确。

### 当前实现范围

`locator-repair.cjs` 提供 `capture/diagnose/propose/revalidate`。它们只读采集或输出 JSON 提议，**不会自动改流程、点击或保存元素库**。`REPAIR_VALIDATED` 只表示当前提议通过，不表示应用已修复或业务已完成。

当前可执行参考适配器是 `data-contract-v1`，要求页面具有明确的页面、记录、状态、目标、区域和上下文身份字段。真实网站需要适配自己的可信身份来源；这部分不是任何网站开箱即用。不能给页面注入标记，伪造身份后宣称修复成功。

## 5. Web：修复前分析用途和含义

v10 新增的用途分析由当前 Agent 执行，不要求再接一个模型服务。它读取失败行或块及必要的上下游流程，输出以下信息：

| 输出 | 用途 |
|---|---|
| XPath 字面含义 | 原表达式从哪里查、限制什么、可能返回什么 |
| 目标业务含义 | 实际要读取或操作什么对象 |
| 用途分类 | 单元素、集合、集合后选一个、混合或未知 |
| 消费方式 | 遍历、计数、按业务键筛选、按位置选择等 |
| 成员与字段要求 | 每项必须有哪些子元素、读取什么值 |
| 父子和共享依赖 | 哪些相对查询真正依赖本元素，哪些其他步骤共用它 |
| 行为和后置条件 | 点击、读取或切换后应出现的业务结果 |
| 证据与缺口 | 对应源码行/块、历史事实、推断和仍需核实的内容 |

“当前零匹配”“以前只返回一个”“变量名包含 list”都不能单独判断用途。原流程先 `find_all_by_xpath` 再按订单号筛一个，仍必须修复集合查询，不能当成单元素问题处理。

脚本会用 Python AST 校验支持范围内的实际调用和源码证据，并在交接前回读流程。它不是完整程序分析器：动态封装、别名、复杂跨函数/子流程关系可能保持未知；VisualFlow 尚无经表单/schema 验证的用途自动交接适配器。AI 结论也仍可能出错，必须继续通过业务身份和验收检查。

用途分析支持 `single`、`collection`、`collection_then_single`、`mixed` 和 `unknown`。同一定位在不同调用点用途不同时，按实际失败位置拆分；分类为集合不会进入原单元素修复器，未知或未核实的分析不会放行自动采用。

## 6. Web：相似元素组与父子定位联动

相似元素修复的目标是一组满足原业务要求的成员，不能把路径“修到有结果”就结束。

1. **先确定原组的合同。** 区域、成员业务键、必须读取的字段、筛选顺序、允许空集的条件和点击后的效果。
2. **从独立来源确认完整性。** 依据本次输入、可靠业务清单或已核验历史合同，确认应该有哪些成员；不能把候选 DOM 查出的结果直接当完整清单。
3. **筛选候选区域。** 每组都用同一个业务合同检查；两个不同区域都满足合同则保持歧义，不按相似分或位置选一个。
4. **逐成员核对字段。** 不只看名称是否在允许范围，还按业务键核对名称、账期、租户、动作等必要字段，避免两条记录互换内容仍通过。
5. **检查父子依赖。** 父集合修复后，逐成员检查相关 `./` 子查询；有效的保留，失效的新增版本，再验证实际父子组合。
6. **动作前重新检查并验收业务效果。** 查找成功、`COLLECTION_RESOLVED` 或 `BUSINESS_ACTION_READY` 都不能代替业务完成回执。

ROBULA+ 和加权生成器在这里用于已确认的唯一锚点；集合规则需另行构造和验证。当前没有“ROBULA+ 自动发现整组重复祖先并恢复所有业务逻辑”的实现。

集合参考解析器仅覆盖普通 DOM。它支持旧新变体、成员去重、字段检查、候选区域筛选和动作前复核；仍需要 Agent 建立可信合同及编排修改，并非任意站点自动补丁器。

### 密集页面的验证优化

集合解析增加 `business` 校验模式，按每个业务键核对全部约定字段，减少同名、同键、不同租户/账期等干扰导致的误接受。候选区域只能改变定位，不能改写原业务身份、成员清单或字段预期。

读取时在一次 DOM 任务内采集页面、区域、全部父定位版本、成员和子字段，再与原生查询句柄及后续语义快照交叉核对，降低多次查询读到不同页面状态的风险。解析复用父子索引，减少长表格中的重复遍历和逐字段 SDK 往返。

动作预检额外核对字段所属成员、真实动作节点、启用状态、祖先可见状态、指针事件及中心点遮挡。纯读取集合采用读取验收；点击集合增加动作与结果验收。候选批次结束、实际写入及动作前分别重新检查，页面变化时原结果作废。

## 7. 继续支持旧、新定位，避免不同电脑互相影响

这项原则来自 v9，v10 增加了对应验证逻辑。修复时保留原资产和旧新变体，调整流程判断；原定位已经错误命中时，保留历史记录但停止把它作为可执行回退。

| 当前情况 | 处理 |
|---|---|
| 只有旧版有效且符合原合同 | 使用旧版，继续保留新版 |
| 只有新版有效且符合原合同 | 使用新版，继续保留仍有适用场景的旧版 |
| 两版命中同一实际对象 | 按对象身份去重，只操作一次 |
| 旧隐藏、新版位于当前有效区域 | 验证身份后排除隐藏副本 |
| 两个不同可见对象均有效 | 补区域、记录或版本条件；不能消歧则停止 |
| 同一区域不同成员分别用旧新版结构 | 仅在明确允许并验证混合成员协议时合并，继续检查完整性 |
| 查询异常、身份冲突或动作结果未知 | 分别排查；不把任意异常当零匹配，切到新版重做动作 |

跨电脑适用性需要在相应电脑、客户端版本和页面状态下回归。当前验证范围为单台环境，兼容机制尚未完成跨电脑验收。

## 8. iframe、Shadow DOM 和影刀 Selector

v9 已提醒普通 XPath 不跨 iframe/Shadow；v10 新增了生成和检查链路，并适配影刀的分段表示：

```javascript
[
  "//iframe[@data-frame-key='order-frame']",
  "//*[local-name()='order-panel'][@data-host-key='order-primary']/xbotShadowRoot//button[@data-automation-id='approve']"
]
```

这是格式说明，不是任何页面都能执行的通用路径。数组描述 iframe 链，`/xbotShadowRoot` 描述 open ShadowRoot 边界；不能拼成一条普通 XPath。

普通 DOM 可以使用对应 XPath API；复杂上下文按当前 schema 验证 Selector 通道，只有确需建库时才通过 `web_review` 保存。**生成 XPath、后端暂存 Selector、保存工程元素库、实际动作成功，是不同阶段。** 引入 ROBULA+ 不会让影刀 Selector 引擎自动监控页面或自愈元素。

历史测试验证了部分 iframe、嵌套 open Shadow 和跨域上下文。也发现过路径有效而 `web click ref` 后端报 `xpathSegments`，同路径通过工程 Selector 执行成功；因此生成与执行通道必须分别验收。

closed ShadowRoot、ShadowRoot 内 iframe、未适配 transform/zoom 的 frame 均有限制。普通 DOM 集合验收不能推广为 iframe/Shadow 集合修复已经支持。

## 9. 新增模块分流和 Windows Python 修复

### 自动判断修复的是哪一类步骤

Agent 从报错位置、实际 API、接收对象来源及上下游调用判断平台。DOM 查询走 Web，原生窗口控件走 Windows；网页上传时出现的系统文件弹窗属于 Windows，Electron 内通过网页对象查询的表格可以属于 Web。

不能根据流程名、`.exe`、当前前台窗口或字符串中有 XPath 就决定平台。影刀是运行流程的工具，业务目标可以是其他程序；不会默认拿影刀编辑器作为待修复窗口。证据缺失时输出未知，先调查。

这属于 Agent 按 Skill 执行的选择规程，没有新增常驻类型识别服务或后台错误监听器。

### Windows 当前采用的实现

优先使用 Python + `uiautomation`，由影刀编码版直接运行；可视化流程通过真实 `process.run` 块调用同一编码版，无需影刀元素库或 Agent 逐次回传窗口快照。

```text
确认业务进程和唯一窗口
 -> 检查可信原身份及单控件用途
 -> 查询全部旧新变体并比较当前 UIA 对象
 -> 全部零匹配时，按完整原身份寻找唯一候选
 -> 验证新查询，在新资产副本追加定位
 -> 保留旧资产、旧变体及原身份
 -> 隔离测试和动作前复核
 -> 动作后查客户端结果；未知时只读对账
```

身份包括可执行文件完整路径、窗口类、系统标题、控件类型/名称/ID/类和有序必要祖先。PID、HWND、RuntimeId 只作当前运行核对，不作为跨运行的长期业务身份。不使用模糊名称或最高相似分强改。

当前可修范围是：定位条件被改错，而可信原身份仍完整存在且唯一的单控件。控件改名、必要祖先变化、多个候选、窗口不唯一、树读取不完整、集合用途等会被拒绝；不能把环境或观察故障当元素消失。

微信动作另有群聊检查、同名搜索结果消歧、搜索层检查、显式窗口恢复、本地台账与只读对账。它是具体应用适配，不是通用桌面动作规划器。Windows-MCP、Umbriel 仅调研，未接入本批实测；`win32 snapshot` 是本地构建实验能力，旧 CLI 不会因复制 Skill 自动具备新命令。

### Windows 检查与动作优化

- **窗口作用域。** 使用可执行文件完整路径、窗口类和系统标题确认窗口；最小化、窗口歧义和权限等环境问题单独处理。
- **完整观察。** 节点数、深度、时间或 provider 读取异常导致的树不完整会明确停止，不作为旧定位零匹配。
- **变体一致性。** 查询全部旧新定位；同一当前 UIA 实例可以归并，不同实例不按名称相同合并，旧定位命中错误对象时直接拒绝。
- **原资产保留。** 修复结果写入新的资产副本，保留原身份与旧变体，不放宽原身份来提高通过率。
- **显式窗口恢复。** 默认拒绝对最小化窗口执行后续流程；请求明确允许时恢复并重新核对，避免把窗口状态当定位失效。
- **调用参数回读。** 编码版更新后再次读取可视化调用块，确认实际传入的请求配置和输出，避免测试到错误分支。
- **动作方式独立验收。** `Invoke` 支持情况与实际业务结果分别验证；物理点击额外检查前台和 UIA 命中对象。
- **防止重复动作。** 发送前记录稳定操作标识与本地台账，发送后检查正确群聊、精确消息和输入状态；结果未知时只读对账，已有记录阻止重放。本地台账不提供服务器幂等。

## 10. 测试验证

测试覆盖 Web 受控密集业务页面和本机真实微信客户端，包含正向、拒绝、兼容及结果未知场景。各批次使用不同口径，分别报告结果。

| 测试 | 实际记录 | 能说明什么 |
|---|---|---|
| Web 密集业务页面，2026-10-21 | 43 类场景、两轮共 86 次；84 次达到预期，2 次同内容替换缺口 | 84 次含 18 次正向业务动作及 66 次拒绝/筛选，不是 84 次修复成功 |
| 密集页规模 | 13,778–45,346 个 DOM 元素，最多 1,860 行、5,811 个按钮、26 个区域 | 用密集干扰页经真实浏览器及影刀执行；这些是受控测试页，并非任意生产网站 |
| 额外干扰检查 | 12 次达到预期；最大 51,685 个 DOM 元素、25 个候选，动作数 0 | 这些负样本得到阻止，不代表没有未覆盖的误修场景 |
| Windows Python，2026-10-22 | 真实微信、影刀 Python 3.7.4 与 uiautomation 2.0.29；25 项核心核验符合各自预期 | 真实触发错误、修复和拒绝，包含编码版与可视化调用；不是 25 次自动修复 |
| Python 批次消息发送 | 三次发送动作调用，1 条客户端确认，2 个结果未知，无重放 | 编码版有一次客户端回执；可视化发送完整业务验收未通过，服务器送达未独立验证 |

Web 干扰覆盖了错误租户/账期、同名区域、字段互换、隐藏副本、重复键、缺项、嵌套成员、错误动作及复核期间变化。记录中没有错误业务点击，但两次同内容替换场景仍返回可操作状态，是未解决缺口，不能据此承诺零误修。

Windows 在真实微信中验证了故意改错的定位、旧新共存、三个同名搜索项、错误窗口/群聊、禁用发送、观察预算不足、最小化及防重复请求。发送后的会话变化和观察错误根因尚未完全查清；当前通过未知结果分流和对账阻止重发，完整发送链路仍待进一步验收。

## 11. 不足、影响及后续优先级

| 不足 | 可能影响 | 后续处理 |
|---|---|---|
| 缺历史身份，或显示字段与后台对象绑定不明 | 找到唯一对象仍可能是错误业务目标 | 优先接入可靠业务键、记录/动作绑定和独立回执；证据不足只提议 |
| 同位置同内容替换和复核后的变化 | 节点/字段看似相同，业务对象可能已换 | 引入可验证渲染代次、稳定对象身份、幂等或事务协议并专项回归 |
| 全字段基线过严或业务状态正常更新 | 可能正确拒绝，也可能增加误拒绝 | 从独立业务来源更新合同，不抄候选 DOM 消除报错 |
| 动态封装、跨流程、VisualFlow 用途自动交接不足 | 无法可靠确定原用途和影响范围 | 补对象来源、调用链及经 schema 验证的块适配 |
| 集合无通用发现与全量协议 | 分页/虚拟列表可能漏成员，复杂上下文无法直接修 | 补逐页/虚拟化协议、独立完整性来源和 iframe/Shadow 集合适配 |
| Windows 当前只做单控件，单机单微信 | 无法推广为任意程序或跨电脑自动修复 | 测真实改版、多电脑、多语言、多窗口、虚拟控件和父子集合 |
| UIA/provider 卡住无法被内部预算中断 | 无人值守运行可能长时间阻塞 | 用独立 worker/进程隔离实现硬超时和恢复 |
| Windows 发送完整链路尚不稳定 | 接口返回或运行结束不能证明消息发出 | 查清会话变化，完善动作方式与独立结果核对，再验收可视化发送 |
| 缺少常驻编排、原子写入和跨进程协调 | 无法承诺报错后自动修复并无条件续跑 | 先完善版本冲突、原子保存和运行恢复，再考虑常驻服务 |

可修复性以证据和测试为依据：可修时新增兼容逻辑并验证，不直接覆盖旧元素；不可修或不确定时不改业务流程，仍给出根因、尝试、拒绝理由和建议。当前方案能减少误改机会，尚不能保证零误修。

## 12. 修复链路与交付标准

```text
流程报错
 -> 定位失败行/块，判断动作阶段与真实根因
 -> 按实际对象加载 Web 或 Windows 模块
 -> AI 分析原定位含义、单元素/集合用途和父子依赖
 -> 核对历史身份、页面/窗口状态及独立业务基线
 -> 运行对应平台的候选生成与验证
 -> 保留有效旧新定位及相关子定位
 -> 在可复用的隔离测试子流程中验证正向、负向和共存场景
 -> 证据充分时最小修改，回读并静态检查
 -> 真实运行，验收业务结果或对账
 -> 保存定位资产、测试结果、拒绝原因及分析报告
```

修复工具增加只读提议与多阶段复核，分别标记采集、诊断、提议通过、动作前复核、写入、运行及业务通过。当前应用、源码摘要、资产版本、文档代次或目标记录变化时，需要重新产生相应证据。

共享定位只修改已确认的引用范围；配置回滚不代表已执行业务动作被撤销。修复不可行或证据不足时保持业务流程，交付原因、尝试过的方法、未解决问题和建议。

### 工具与工程优化

| 优化 | 作用 |
|---|---|
| 模块与引用按需加载 | 共享规则保留在入口，Web、Windows 和复杂场景资料按任务读取 |
| 第三方源码固定修订与哈希 | 提高算法输入的可复现性，拒绝未核验的源码替换 |
| 只读浏览器求值与新建输出 | 提议阶段不点击、不改业务流程；输出文件避免覆盖已有证据 |
| 压缩结果与完整性校验 | 缓解浏览器后端长输出截断；解析失败明确停止，不修补残缺 JSON 为成功 |
| 应用、源码及资产复核 | 降低切错应用、代码更新或资产冲突后继续采用旧提议的风险 |
| 清晰的工具边界 | Skill helper 与 CLI 子命令分别提供入口和帮助，模板与运行资产分别校验 |

Skill 由 Agent 按需执行，不是常驻监听器。Web helper 主要产生只读提议；Python Windows helper 可在显式允许的条件下追加兼容资产并执行已适配操作。生产采用仍需任务级授权和业务验收。

## 13. 使用条件

| 能力 | 运行要求 |
|---|---|
| 影刀流程开发与运行 | 可用的 xbot-cli、已启动并登录的影刀客户端，以及相应 Studio 通道 |
| Web 生成与修复辅助 | 可用浏览器通道、当前快照、可靠目标身份；Node 提供 `stripTypeScriptTypes` |
| ROBULA+ 组合生成 | 显式提供固定修订的本地源码，通过 SHA-256 校验 |
| 用途 AST 分析 | Python 3.8+，用于只解析流程代码 |
| Python Windows UIA | Windows 交互桌面及目标程序的 UIA 能力；运行环境安装 `uiautomation==2.0.29` |
| 实验性 `win32 snapshot` | 含该能力的本地 CLI 构建，或按模块使用 Python 快照脚本 |

ROBULA+ 固定修订为 `cyluxx/robula-plus` 的 `5f33589dadba0f71075294611aa79801da123e40`，输入文件为 `src/index.ts`，SHA-256 为 `a674d99763c8045456add711d549f53bfe6103f025b338b30e1cec398b2b29d4`。源码采用外部提供方式，不自动下载；上游许可状态确认前不作为发行依赖打包。

Windows 真实验证环境为影刀 Python 3.7.4；用途分析的 Python 3.8+ 解释器与 Windows 业务运行环境分别配置。复制 Skill 不会自动升级 CLI、安装 Python 依赖或提供第三方源码。

## 14. 加权 XPath 算法源码

下面是 v10 `scripts/locator/weighted.js` 使用的完整实现。输入为支持当前快照 `aria-ref` 的浏览器页面对象和真实 ref；函数只生成定位与诊断，不执行点击或保存元素库。单独的通用 Playwright 页面并不保证支持该 ref 协议。

输出 `xpath` 为影刀上下文路径数组。普通 DOM 可使用适配的 XPath API，iframe/open Shadow 使用已验证的 Selector 通道；`raw_xpath_compatible`、`requires_engine_selector` 和 `quality` 分别说明执行方式及路径质量。

```javascript
async function resolveXbotLocator(activePage, refid) {
  if (!/^(?:f\d+)?e\d+$/.test(refid)) {
    throw new Error("INVALID_REF: expected a ref from the current snapshot");
  }

  function describe(target) {
    if (!target.isConnected) throw new Error("STALE_REF: target node is detached");

    const started = Date.now();
    let evaluations = 0;
    let shadowWalks = 0;
    let usedPosition = false;
    const decisions = [];

    const MAX_EVALS = 1500;
    const MAX_MS = 2500;
    const MAX_SHADOW_WALKS = 200000;

    const attributeCosts = {
      "data-testid": 1,
      "data-test-id": 1,
      "data-test": 2,
      "data-qa": 2,
      "data-cy": 2,
      "data-automation-id": 2,
      id: 3,
      name: 5,
      "aria-label": 7,
      title: 9,
      alt: 10,
      placeholder: 11,
      "class-token": 12,
      href: 14,
      src: 14,
      role: 18,
      type: 20,
    };
    const CUSTOM_DATA_COST = 4;

    // ---------- 基础工具 ----------
    function literal(value) {
      if (!value.includes("'")) return "'" + value + "'";
      if (!value.includes('"')) return '"' + value + '"';
      return "concat(" + value.split("'").map(part => "'" + part + "'").join(',"\'",') + ")";
    }
    function nodeTest(node) {
      const name = node.localName;
      return /^[a-z][a-z0-9]*$/.test(name) && node.namespaceURI === "http://www.w3.org/1999/xhtml"
        ? name
        : "*[local-name()=" + literal(name) + "]";
    }
    function tagInfo(node) {
      return { name: node.localName, ns: node.namespaceURI };
    }
    function dynamic(value) {
      if (!value) return false;
      if (/^:r/.test(value)) return true;                              // React useId
      if (/^[0-9a-f]{8}-[0-9a-f-]{27,}$/i.test(value)) return true;    // UUID
      if (/^\d{6,}$/.test(value)) return true;                         // 纯长数字
      if (/^[a-f0-9]{10,}$/i.test(value)) return true;                 // 纯长 hex
      if (/(?:^|[-_])[a-f0-9]{10,}$/i.test(value)) return true;        // 后缀 hash
      if (/(?:^|[-_])\d{6,}$/.test(value)) return true;                // 后缀长数字
      return false;
    }
    function isSessionUrl(value) {
      return /(?:^|[?&])(token|sid|session|sessionid|auth|signature|sig|expires|exp|nonce|state)=/i.test(value);
    }
    function normalizeText(value) {
      return String(value || "").replace(/[ \t\r\n]+/g, " ").trim();
    }
    function classTokens(node) {
      const raw = node.getAttribute("class");
      return raw ? raw.split(/\s+/).filter(Boolean) : [];
    }
    const UTILITY_PREFIX = /^(?:w|h|p|m|px|py|mx|my|pt|pb|pl|pr|mt|mb|ml|mr|text|bg|border|rounded|flex|grid|gap|font|leading|tracking|shadow|opacity|z|top|left|right|bottom|inset|col|row|order|justify|items|self|overflow|cursor|transition|duration|ease|delay|animate|transform|translate|rotate|scale|filter|ring|divide|outline|fill|stroke|select|pointer|resize|scroll|snap|touch|aspect|columns|break|box|clear|float|isolation|object|visibility|whitespace|word|list|indent|align|vertical|table|caption|empty|sr)-/;
    function stableClassToken(token) {
      if (token.length < 3 || token.length > 40) return false;
      if (/[:\[\]/!]/.test(token)) return false;   // Tailwind 变体 / 任意值
      if (UTILITY_PREFIX.test(token)) return false;
      if (dynamic(token)) return false;
      return true;
    }

    // ---------- ShadowRoot 求值器（受限 XPath 子集） ----------
    function matchesTag(el, tag) {
      return el.localName === tag.name && el.namespaceURI === tag.ns;
    }
    function matchesPred(el, pred) {
      if (pred.type === "attr") return el.getAttribute(pred.name) === pred.value;
      if (pred.type === "class") {
        const cls = el.getAttribute("class") || "";
        return (" " + cls.replace(/[ \t\r\n]+/g, " ").trim() + " ").includes(" " + pred.token + " ");
      }
      if (pred.type === "text") return normalizeText(el.textContent) === pred.value;
      if (pred.type === "textContains") return normalizeText(el.textContent).includes(pred.value);
      return false;
    }
    function allDescendants(root) {
      const out = [];
      const stack = [];
      for (const child of Array.from(root.children || [])) stack.push(child);
      while (stack.length) {
        const n = stack.pop();
        out.push(n);
        if (++shadowWalks > MAX_SHADOW_WALKS) throw new Error("LOCATOR_BUDGET: shadow traversal exceeded its bound");
        if (shadowWalks % 128 === 0 && Date.now() - started > MAX_MS) throw new Error("LOCATOR_BUDGET: shadow traversal time exceeded");
        for (const child of Array.from(n.children || [])) stack.push(child);
      }
      return out;
    }
    function evalShadow(root, struct) {
      let current = [root];
      for (const step of struct) {
        const next = [];
        for (const n of current) {
          if (step.axis === "descendant") {
            for (const el of allDescendants(n)) {
              if (matchesTag(el, step.tag) && step.predicates.every(p => matchesPred(el, p))) next.push(el);
            }
          } else { // child
            const kids = Array.from(n.children || []).filter(el =>
              matchesTag(el, step.tag) && step.predicates.every(p => matchesPred(el, p)));
            if (step.index != null) {
              const hit = kids[step.index - 1];
              if (hit) next.push(hit);
            } else {
              for (const k of kids) next.push(k);
            }
          }
        }
        current = Array.from(new Set(next));
        if (!current.length) break;
      }
      return current;
    }

    // ---------- 验证器 ----------
    function makeVerifier(root, expected) {
      const isDoc = root.nodeType === 9;
      return function check(candidate, ignoreBudget) {
        if (!ignoreBudget && (++evaluations > MAX_EVALS || Date.now() - started > MAX_MS)) {
          throw new Error("LOCATOR_BUDGET: candidate search exceeded its bound");
        }
        if (isDoc) {
          const result = root.evaluate(candidate.path, root, null, 7, null);
          return result.snapshotLength === 1 && result.snapshotItem(0) === expected;
        }
        const nodes = evalShadow(root, candidate.struct);
        return nodes.length === 1 && nodes[0] === expected;
      };
    }

    // ---------- 候选生成（结构化 + 字符串） ----------
    function candidates(node) {
      const tag = nodeTest(node);
      const info = tagInfo(node);
      const result = [{
        path: tag, cost: 45, reason: "unique_tag",
        struct: [{ axis: "descendant", tag: info, predicates: [], index: null }],
      }];
      const predicates = [];

      for (const [name, cost] of Object.entries(attributeCosts)) {
        if (name === "class-token") continue;
        const value = node.getAttribute(name);
        if (value === null || value === "" || value.length > 160) continue;
        if (name === "id" && dynamic(value)) continue;
        if ((name === "href" || name === "src") && isSessionUrl(value)) continue;
        predicates.push({
          text: "@" + name + "=" + literal(value), cost, reason: name,
          pred: { type: "attr", name, value },
        });
      }

      for (const attr of Array.from(node.attributes || [])) {
        const name = attr.name;
        if (!name.startsWith("data-") || attributeCosts[name] !== undefined) continue;
        const value = attr.value;
        if (!value || value.length > 160 || dynamic(value)) continue;
        predicates.push({
          text: "@" + name + "=" + literal(value), cost: CUSTOM_DATA_COST, reason: name,
          pred: { type: "attr", name, value },
        });
      }

      for (const token of classTokens(node).filter(stableClassToken).slice(0, 3)) {
        predicates.push({
          text: "contains(concat(' ', normalize-space(@class), ' '), " + literal(" " + token + " ") + ")",
          cost: attributeCosts["class-token"], reason: "class:" + token,
          pred: { type: "class", token },
        });
      }

      const text = normalizeText(node.textContent);
      if (text && text.length <= 80 && !["script", "style", "input", "textarea"].includes(node.localName)) {
        predicates.push({
          text: "normalize-space(.)=" + literal(text), cost: 16 + text.length / 100, reason: "text",
          pred: { type: "text", value: text },
        });
        if (text.length >= 4) {
          const prefix = text.slice(0, Math.min(40, text.length));
          predicates.push({
            text: "contains(normalize-space(.), " + literal(prefix) + ")",
            cost: 30 + prefix.length / 100, reason: "text:contains",
            pred: { type: "textContains", value: prefix },
          });
        }
      }

      predicates.sort((a, b) => a.cost - b.cost);
      for (const item of predicates) {
        result.push({
          path: tag + "[" + item.text + "]", cost: item.cost, reason: item.reason,
          struct: [{ axis: "descendant", tag: info, predicates: [item.pred], index: null }],
        });
      }

      const comboLimit = Math.min(8, predicates.length);
      for (let a = 0; a < comboLimit; a++) {
        for (let b = a + 1; b < comboLimit; b++) {
          result.push({
            path: tag + "[" + predicates[a].text + " and " + predicates[b].text + "]",
            cost: predicates[a].cost + predicates[b].cost + 3,
            reason: predicates[a].reason + "+" + predicates[b].reason,
            struct: [{
              axis: "descendant", tag: info,
              predicates: [predicates[a].pred, predicates[b].pred], index: null,
            }],
          });
        }
      }
      return result.sort((x, y) => x.cost - y.cost);
    }

    // ---------- 单个 scope 内求解 ----------
    function localPath(node, root) {
      const isDoc = root.nodeType === 9;
      const verify = makeVerifier(root, node);
      const own = candidates(node);
      const options = [];

      // 1) 自身候选：//tag[pred]
      for (const item of own) {
        options.push({ ...item, path: "//" + item.path });
      }

      // 2) 祖先锚点短路径：//anchor//item（descendant）与 //anchor/item（child）
      let ancestor = node.parentElement;
      for (let depth = 1; ancestor && ancestor.getRootNode() === root && depth <= 5;
           depth++, ancestor = ancestor.parentElement) {
        if (["html", "body"].includes(ancestor.localName)) continue;
        const anchors = candidates(ancestor)
          .filter(item => !["unique_tag", "text", "text:contains"].includes(item.reason))
          .slice(0, 5);
        for (const anchor of anchors) {
          for (const item of own.slice(0, 8)) {
            const first = item.struct[0];
            options.push({
              path: "//" + anchor.path + "//" + item.path,
              struct: [...anchor.struct, ...item.struct],
              cost: anchor.cost + item.cost + 8 + depth * 2,
              reason: "ancestor(" + anchor.reason + ")/" + item.reason,
            });
            if (depth === 1) options.push({
              path: "//" + anchor.path + "/" + item.path,
              struct: [...anchor.struct, {
                axis: "child", tag: first.tag, predicates: first.predicates, index: null,
              }],
              cost: anchor.cost + item.cost + 6 + depth * 2,
              reason: "ancestor(" + anchor.reason + ")/child/" + item.reason,
            });
          }
        }
      }

      options.sort((a, b) => a.cost - b.cost || a.path.length - b.path.length ||
        (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));

      const seen = new Set();
      let budgetExhausted = false;
      for (const option of options) {
        if (seen.has(option.path)) continue;
        seen.add(option.path);
        try {
          if (verify(option)) {
            decisions.push({ path: option.path, reason: option.reason, cost: option.cost });
            return option.path;
          }
        } catch (error) {
          if (/^LOCATOR_BUDGET/.test(error.message || "")) { budgetExhausted = true; break; }
          throw error;
        }
      }

      // 3) 结构回退（预算耗尽时也走这里，做一次强制验证）
      const steps = [];
      const structSteps = [];
      for (let current = node; current && current !== root; current = current.parentElement) {
        const parent = current.parentNode;
        if (!parent) break;
        const siblings = Array.from(parent.children || []).filter(item =>
          item.localName === current.localName && item.namespaceURI === current.namespaceURI);
        const index = siblings.indexOf(current) + 1;
        steps.unshift(nodeTest(current) + "[" + index + "]");
        structSteps.unshift({ axis: "child", tag: tagInfo(current), predicates: [], index });
      }
      if (!steps.length) throw new Error("IDENTITY_MISMATCH: empty structural path");
      const fallbackPath = (isDoc ? "" : ".") + "/" + steps.join("/");
      const fallbackCandidate = { path: fallbackPath, struct: structSteps };
      if (!verify(fallbackCandidate, true)) {
        throw new Error("IDENTITY_MISMATCH: structural fallback did not resolve the original node");
      }
      usedPosition = true;
      decisions.push({
        path: fallbackPath,
        reason: budgetExhausted ? "structural_fallback(budget)" : "structural_fallback",
        cost: 1000,
      });
      return fallbackPath;
    }

    // ---------- 沿 ShadowRoot 链向上 ----------
    let node = target;
    let path = "";
    let shadowDepth = 0;
    for (;;) {
      const root = node.getRootNode();
      const segment = localPath(node, root);
      path = segment + (path ? "/xbotShadowRoot" + path.replace(/^\.\//, "/") : "");
      if (!root.host) break;
      if (++shadowDepth > 8) throw new Error("CONTEXT_TOO_DEEP: more than 8 shadow roots");
      node = root.host;
    }

    // ---------- 建议 CSS（best-effort） ----------
    let suggestedCss = null;
    if (shadowDepth === 0) {
      const testName = target.hasAttribute("data-testid") ? "data-testid" : "data-test-id";
      const testid = target.getAttribute(testName);
      if (testid && !dynamic(testid)) {
        suggestedCss = "[" + testName + "=" + CSS.escape(testid) + "]";
      } else {
        const id = target.getAttribute("id");
        if (id && !dynamic(id) && /^[A-Za-z_][\w-]*$/.test(id)) suggestedCss = "#" + id;
      }
      if (suggestedCss) {
        const matches = target.getRootNode().querySelectorAll(suggestedCss);
        if (matches.length !== 1 || matches[0] !== target) suggestedCss = null;
      }
    }

    return {
      path,
      shadow_depth: shadowDepth,
      uses_position: usedPosition,
      decisions,
      evaluations,
      identity_verified: true,
      tag: target.localName,
      suggested_css: suggestedCss,
    };
  }

  // ---------- 取句柄 ----------
  let handle;
  try {
    handle = await activePage.locator("aria-ref=" + refid).elementHandle({ timeout: 3000 });
  } catch (error) {
    if (/Timeout \d+ms exceeded/i.test(error.message || "")) {
      throw new Error("STALE_REF: target is missing; take a new snapshot before resolving again");
    }
    throw error;
  }
  if (!handle) throw new Error("STALE_REF: snapshot again before resolving the target");

  const handles = [handle];
  try {
    const target = await handle.evaluate(describe);
    const contexts = [target];
    let frame = await handle.ownerFrame();
    const targetUrl = frame.url();

    while (frame.parentFrame()) {
      if (contexts.length > 16) throw new Error("CONTEXT_TOO_DEEP: more than 16 iframe levels");
      const frameElement = await frame.frameElement();
      handles.push(frameElement);
      const context = await frameElement.evaluate(describe);
      if (context.shadow_depth) {
        throw new Error("UNSUPPORTED_SHADOW_IFRAME: Xbot does not support a frame hosted inside ShadowRoot");
      }
      contexts.unshift(context);
      frame = frame.parentFrame();
    }

    const frameDepth = contexts.length - 1;
    const shadowDepth = contexts.reduce((sum, ctx) => sum + ctx.shadow_depth, 0);

    return {
      algorithm: "weighted_dom_v2",
      refid,
      xpath: contexts.map(ctx => ctx.path),
      frame_depth: frameDepth,
      shadow_depth: shadowDepth,
      raw_xpath_compatible: frameDepth === 0 && shadowDepth === 0,
      requires_engine_selector: frameDepth > 0 || shadowDepth > 0,
      identity_verified: contexts.every(ctx => ctx.identity_verified),
      quality: contexts.some(ctx => ctx.uses_position) ? "structural" : "semantic",
      persisted: false,
      target_url: targetUrl,
      suggested_css: contexts[contexts.length - 1].suggested_css || null,
      diagnostics: {
        evaluations: contexts.reduce((sum, ctx) => sum + ctx.evaluations, 0),
        decisions: contexts.map(ctx => ctx.decisions),
      },
      contexts,
    };
  } finally {
    for (const item of handles) await item.dispose();
  }
}
```

该函数提供单个已确认节点的路径生成能力。失效修复还需调用上层用途分析、历史身份检查、候选消歧和业务验收；相似元素组采用独立集合解析器。
