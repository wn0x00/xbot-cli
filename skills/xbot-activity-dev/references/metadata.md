# 文案、公开状态与分组操作

目录：[共同流程](#closed-edit) · [标题与文案](#text) · [启用禁用](#enable) · [分组](#groups) · [检查器](#checker)。仅变更这些配置或排查漂移时读取，普通应用不套用指令注册规则。

## 配置定位

| 配置层 | 职责与定位方式 |
| --- | --- |
| `package.json` | flows 按 filename 唯一匹配：显示信息、opened、groupName；flow_groups 为组定义 |
| `prototype.block.json` | 按完整 function 唯一匹配 block，再按参数 name 找 inputs/outputs；展示、comment、editor、category、hidden |
| `.dev/<filename>.flow.json` | 流程 In/Out、方向、类型、默认值表达式与参数说明 |
| `__init__.py` / `prototype.activity.xml` | Python/XML 公开成员、签名和真实 process.run 绑定 |
| CodeFlow | 实际业务和代码生成的展示文字；docstring 不自动成为指令描述 |
| `.dev/workspace.state.json` | 编辑页签/断点/布局，不是公开启用名单或分组 |

按当前同版本有效结构操作；缺失、重复、歧义绑定先排查。保留 ID、namespace、顺序、未知字段和无关成员；JSON/XML 结构化解析，Python 公开函数用 AST 或可靠解析器。文案或归组无须重建整套注册；四层核对不表示每次都要写四个文件。

**任务安排：** 开发时将涉及的文案、开关、分组列入 D4，保存/展示/重开列入 D6；维护时分别对应 F2 的字段定位、F4 的修改和 F5 的验证。每项记“目标 filename/function → 界面位置 → 实际字段 → 旧值/新值 → 保留字段 → 验证证据”，多个目标分别核对，不用“元数据已处理”笼统打勾。

字段证据分“离线样本覆盖、历史观察、当前现场验证”。下表路径中的索引只表示唯一匹配后的真实位置，不是可照抄的下标；本稿尚未取得当前客户端的 UI/保存重开实测，不将离线适配写成现场通过。

| 修改对象 | 已知路径或定位入口 | 本稿证据与限制 |
| --- | --- | --- |
| 模块显示名 | `package.json#/flows/<按 filename 匹配>/name` | 检查器测试样本使用该结构；客户端实际重命名映射待现场确认 |
| 指令独立标题/介绍 | 按完整 function 找 block，再确认负责字段 | 尚无已验证通用键；`block.name` 可能是身份，不能猜添 title/description |
| In/Out 中文名 | `prototype.block.json#/blocks/<按 function 匹配>/inputs或outputs/<按 name 匹配>/label` | 离线样本及文案兼容测试覆盖；UI 是否消费该字段须实际观察 |
| 参数说明/提示 | `flow params list/set` 的 description；原型提示按现场结构 | CLI 参数契约与原型 UI 提示分开确认，不假定 description=tips |
| 动作描述 | 对应 block 的 comment，使用 `%内部参数名%` | 离线样本及占位符检查覆盖；渲染语法仍须当前版本核对 |
| 启用/禁用 | 对应 flow.opened、block.hidden、Python/XML 精确成员 | 离线四层检查覆盖；另有 ShadowBot 6.3.39 的历史观察，非当前版本证明 |
| 分组与模块归属 | package.flow_groups、flows[].groupName | 离线组引用检查覆盖；根级 null 是检查器适配，新组 ID/排序按现场生成 |
| 枚举及业务展示 | 对应 editor 选项显示字段或实际业务代码 | 没有通用显示键；保留内部 value，分别验证界面和输出契约 |

离线证据可在随包 [元数据测试](../scripts/test_check_metadata.py) 的 configure、文案兼容及分组用例中复核。历史来源为原版 `version-evidence.md` 的 2026-11-04 记录（ShadowBot 6.3.39，部分配合 CLI 0.9.1）：眼睛关闭后 opened=false、hidden=true，Python/XML 移除对应公开入口；本包没有该记录的原始现场日志。

实际任务沿用同一表格补充“CLI/运行客户端/provider → 实际字段与旧新值 → schema 或样本来源 → UI/保存/重开结果”。现场核验后才升级证据等级；未知标题、提示、新组 ID 等先在任务副本或未公开样本做一次可恢复的界面修改，比较保存 diff，记录负责字段和保护字段。不能用本稿路径、测试样本或版本号代替该步骤，也不另增版本说明 Markdown。

<a id="closed-edit"></a>
## 共同修改与保存流程

有客户端表单/菜单或受支持接口时使用实际能力；源码和流程参数优先用 CLI。缺元数据接口时，采用以下关闭后结构化维护路径，不编造 flag、不内存热改。

1. 确认 app_id、版本、实际应用目录与目标清单，记录“字段/模块 → 旧值 → 目标值”和消费者影响，留存可恢复原文。
2. 设计器打开时先 save，再直接 sync；独立 status 必须读到 `studio.opened=false`。已关闭则从下一步继续。reload、force open 和命令回执不能替代关闭中间态。
3. **关闭后**重新读取最新文件，更新工程外备份和修改前检查快照；保存/关闭可能改过文件，不能盲用关闭前哈希。快照不含可恢复原文，须另存原文件。
4. 按下方对应规程生成最小候选，校验唯一绑定、参数、组引用及模板；需要检查器时填写全量状态和本次精确许可。写前再确认仍关闭、文件未漂移，有并发变化先合并，不整份覆盖。
5. 最小写入并复核实际 diff。中途失败回读各层已写/未写项，不能用旧目录全量回滚覆盖同期修改。
6. 正常 open，独立核对正确工程及 `studio.opened=true`，进行当前版本实际有效的保存。无脏保存未必重建注册；涉及注册时核对真实生成结果，必要时使用已验证的生成路径，不临时改描述制造“保存成功”。
7. 回读配置、保护文件与实际 UI，按任务观察标题、提示、动作摘要、公开状态或分组；保存后另留稳定快照。
8. 再 sync → status 确认 false → 正常 open → 真实保存 → 回读比较稳定快照。漂移时保留差异并排查生成路径，不能每次保存后补文件或改页签掩盖。

```powershell
xbot-cli app save --expect-app <app-id> --json
xbot-cli app sync --expect-app <app-id> --json
xbot-cli status --json
# 仅在 studio.opened=false 后备份、结构化修改和静态核对。
xbot-cli app open <app-id> --json
xbot-cli app save --expect-app <app-id> --json
```

sync 是正常可直接使用的关闭能力，本身不改指令逻辑、参数、开关或归组，不发布。正常开发维护与关闭重开无需另问同步确认；无改动的 uploaded=false 不阻止关闭核验，异常先查状态。完整语义见 [CLI 主文件](../../xbot-cli-doc/SKILL.md#sync)。纯展示/归组任务不为验收额外提交业务。

<a id="text"></a>
## 标题、label、说明、业务展示与描述

| 修改项 | 方法 | 保留内容 |
| --- | --- | --- |
| 设计器模块显示名 | 客户端重命名或 flows 项实际显示字段 | filename、flow_id、function；不要把模块名与公开标题混同 |
| 指令标题 | 对应 block 当前真正使用的展示字段 | block.name 可能是公开原型身份，不一律当文字替换 |
| In/Out 中文名 | 通过参数 name 唯一定位后改 label | name、方向、类型、绑定、默认值 |
| 参数说明/提示/占位符 | 当前参数实际的 tips/description、editor 提示字段 | required、editor 类型、变量能力和默认值 |
| 指令描述 | 当前原型负责单条介绍的实际字段 | 不与流程参数说明、应用整体描述或源码 docstring 混淆 |
| 动作描述 | 已适配原型的 comment 模板 | 使用已声明的内部参数名，例如 `%account_name%`，不是中文 label |
| 下拉/其他业务展示 | editor 的选项显示文字或真实展示字段 | 内部 value；正式输出字段/枚举变化另按契约处理 |
| 日志/结果展示 | 真正生成文字的业务代码 | 输出结构、动作与异常语义 |

操作步骤：

1. 明确界面位置，读 filename/function/block.name、原型和 In/Out，记录精确字段路径及旧新值。
2. 用当前表单/schema 或同版本有效样本确认映射。标题/介绍键未知时，在任务副本或未公开样本做可恢复文字修改并比较保存 diff；不猜添 title/description，也不拿旧公开入口试验改名。
3. 用受支持界面/接口修改；文件方式遵循共同流程，按 function 和参数 name 精确定位，不按旧数组下标或全文中文替换。
4. 动作 comment 核对当前占位语法及所有声明，包括输出占位；label 改名后仍引用内部 name，检查错字、重复百分号和旧参数残留。枚举只改展示文字。
5. 正常保存，实际查看目标标题、参数提示、指令介绍、动作摘要；按涉及范围看普通值、空值、变量渲染，无需执行业务提交。
6. 完整重开后再次保存回读，核对文字和保护字段。若标题必须改 block.name，则属于潜在公开身份变化，先处理旧消费者兼容和迁移。

内存候选示例（假定该版本已核实 blocks/comment 结构，不直接写盘）：

```python
blocks = [b for b in prototype["blocks"] if b.get("function") == target_function]
if len(blocks) != 1:
    raise ValueError("函数绑定缺失或不唯一")
inputs = [p for p in blocks[0]["inputs"] if p.get("name") == "account_name"]
if len(inputs) != 1:
    raise ValueError("输入参数缺失或不唯一")
inputs[0]["label"] = "账户名称"
blocks[0]["comment"] = "读取账户 %account_name%，输出 %records%"
```

account_name/records 必须真实声明，例子不证明其他版本支持相同字段。

<a id="enable"></a>
## 指令启用、禁用与旧入口

这里指指令公开眼睛开关，区别于页签开关和 VisualFlow 块启停。

| 状态 | flows[].opened | block.hidden | Python/XML 公开注册 |
| --- | --- | --- | --- |
| 正常启用 | true | false | 均存在，绑定/签名正确 |
| 弃用但旧调用兼容 | true | false | 保留原 namespace、契约和注册，可标注弃用 |
| 禁用/注销 | false | true | 精确移除对应公开成员；保留必要源码/内部导入 |
| 从未公开的内部模块 | false | true | 不创建公开成员 |

这是已适配的映射，现场格式不同先核实。关闭眼睛会移除 Python/XML 公开注册，不能解释为“仅隐藏但兼容”。若要隐藏展示同时保留旧调用，必须证明当前版本有独立展示机制，并验证“不显示”和“旧调用可用”同时成立；不支持则保留注册/展示并标注弃用，不能 opened=false 后手留包装冒充兼容。

用户已明确禁用指定入口即按范围执行，记录消费者影响，不逐项重复询问；不能因旧入口没进新推荐清单就自动禁用。完全注销前清点引用并确认迁移/退役；内部模块仍能 run 不证明旧的外部调用兼容。

**客户端方法：**

1. 以 filename/function 确认目标和当前状态。禁用点击常亮眼睛；启用悬浮目标行后点击眼睛，观察结果，不盲目连点。
2. 真实保存，回读 opened、hidden、Python 函数和 XML 成员的完整集合、绑定、签名及 In/Out。
3. sync 并独立确认关闭，正常 open、真实保存后复核；按影响验证原配置旧消费者，不为验证重放结果未知的写动作。

**关闭后维护方法：**

1. 按共同流程关闭并备份，列出全量目标状态，filename/function 唯一匹配。
2. 启用时 opened=true、hidden=false，补齐当前版本合法 Python 包装/XML 成员、参数顺序、输出声明和 process.run 绑定。已有成员不重复追加；缺合法样本时让客户端生成或取得同版本有效样本，不能只翻两个布尔值交付。
3. 禁用时 opened=false、hidden=true，按完整 namespace 精确移除该公开函数/XML 成员；保留其他成员、必要内部源码和导入，不重建整个 __init__.py。
4. 静态检查候选后最小写入，正常打开保存、回读四层，再完整重开比较。开关任务不顺带改源码、默认值或分组；不能改 workspace openedFlows 凑名单。

<a id="groups"></a>
## 分组创建、修改与模块调整

设计器分组使用 package.flow_groups 与 flows[].groupName；原型 category 是指令中心分类，分别核对。客户端有菜单/拖动/属性接口时使用实际操作，缺能力才走关闭后维护；不存在的 `--group` 和斜杠名称不能模拟分组。

| 操作 | 具体步骤 |
| --- | --- |
| 创建 | 读取全部组和引用 → 确认新名非空、无首尾空格/重名 → 通过客户端或同版本合法结构创建组 → 再将指定模块 groupName 指向精确组名。已有 ID 不复制，新 ID 按实际生成方式 |
| 改名 | 确认旧组唯一、新名不冲突 → 在同一候选中修改组 name 和**全部**旧 groupName 引用 → 保留组 ID/未知属性 → 检查无旧名悬空；目标已存在则按合并处理 |
| 移动模块 | 列“filename → 原组 → 目标组” → 目标组先存在 → 只改指定 flows 项 groupName → 回读原/目标组完整成员。保留 flow_id、filename、源码、In/Out、opened 和注册 |
| 移回根级 | 采用同版本真实编码；当前检查器要求 null，不能写字符串 "null" 或随意空串 |
| 合并/删除 | 先移动全部引用（包括禁用/内部模块） → 确认零引用 → 才删旧定义，不凭展开界面看似为空删除 |
| 排序 | 使用真实客户端能力；仅已验证数组/排序字段控制顺序时修改，不凭数组顺序承诺 UI 顺序 |

修改后检查组名唯一、非空 groupName 都可解析；正常打开保存，实际观察分组/归属，再完整关闭重开保存比较。分类和排序另外给展示证据，检查器只能证明部分结构，不证明 UI。

<a id="checker"></a>
## 只读检查器与证据

[check_metadata.py](../scripts/check_metadata.py) 用标准库，只读文件，不导入/执行待查源码和表达式，不连接客户端。宿主 Python 3.9+，注册按 AST 3.7 解析；这不证明目标引擎、依赖或 SDK 可运行。

复制 [策略 JSON](../assets/metadata-policy.example.json) 并适配：schema_version=1；app_id/namespace 精确匹配；flows 覆盖 package 全部流程，指定 filename/block/function/state/group（根级 null）。当前 function 适配 `<namespace>.<filename>`，别名需另适配。state 为 active/deprecated_compatible/hidden_compatible/unregistered/internal；策略不是授权，隐藏兼容和注销仍需人工证据，历史公开入口不得改标 internal 绕过。

allowed_json_changes 使用实际字段 JSON 指针，不接受根、通配符或整张 /flows、/blocks 放行；数组增删/重排逐项审 diff，`~`、`/` 转义为 `~0`、`~1`。allowed_file_changes 通常限需改的注册 Python/XML；许可不解除未知字段与旧契约人工复核。修改 .dev/*.flow.json 参数时同时列精确 JSON 指针和该文件整文件许可，再人工查动作未被改动。

在本 skill 目录运行，证据文件放应用目录外：

```powershell
python -B -X utf8 scripts/check_metadata.py snapshot --app-dir '<应用目录>' --expect-app '<uuid>'
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<策略.json>' --baseline '<修改前快照.json>' --phase offline
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<策略.json>' --baseline '<修改前快照.json>' --phase after_save
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<策略.json>' --baseline '<修改前快照.json>' --stable-snapshot '<真实保存后快照.json>' --phase after_reopen
```

snapshot/report 输出 stdout；确认退出码、JSON 完整后存入新文件，不用失败输出覆盖基线。修改前基线保护许可外差异；真实保存后稳定基线供重开比较，任何受保护文件变化（含格式）都会报 STABILITY_DRIFT，先查原因，不自动刷新基线消警。前后读取检测不是事务锁。

检查器核对 package/block/Python/XML 精确成员与绑定、可识别签名及默认值、参数/占位符/分组；保护四层、根 *.py、.dev/*.flow.json 和存在的 workspace.state.json。不递归保护依赖/资源；动态导出、未知编码/包装或结构报待复核，不猜。原型面板顺序可独立于流程调用参数顺序，各自基线仍保护。

| 退出码 | 含义 |
| --- | --- |
| 0 / no_findings_in_checked_rules | 已执行规则无发现，不是完整验收通过 |
| 1 / issues_found | 有问题或输入/快照不可用，停止成功交付并处理 |
| 2 / needs_review | 基线、格式或人工证据不足，不能算通过 |

报告始终 acceptance_complete=false，manual_evidence=not_verified；phase 只是标签，不能证明真实关闭、保存、重开、中文渲染、旧消费者、注销授权或发布。按 [主文件证据表](../SKILL.md#规格与验收模板) 单独记录。脚本变更或需检查离线回归时运行 `python -B -X utf8 -m unittest discover -s scripts -p test_check_metadata.py -v`，不把离线通过写成客户端已验收。
