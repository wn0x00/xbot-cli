# 统一元数据检查入口

本检查器面向指令集公开注册与展示元数据，不用于普通应用的入口、模块调用或业务运行验收。普通应用没有这里要求的指令启用清单，参见[应用开发差异](application-development.md)。

`scripts/check_metadata.py` 使用标准库，只读应用文件，不连接客户端、不写应用、不导入或执行待查源码及默认值表达式。运行工具需 Python 3.9+；注册文件按 Python 3.7 语法解析，但这不是目标引擎或依赖兼容性验收。

## 准备策略清单

复制并按当前应用填写[策略模板](../assets/metadata-policy.example.json)，不要把示例名单或修改许可直接用于真实应用。

| 字段 | 含义 |
| --- | --- |
| `schema_version` | 当前为 `1` |
| `app_id`、`namespace` | 明确指定应用 `uuid` 和 `xbot_extensions.<activity_code>`，必须与文件一致 |
| `flows` | 覆盖 `package.json` 全部流程；遗漏是错误，不代表自动禁用 |
| `filename` | 当前流程文件标识，不是中文显示名，不含扩展名 |
| `block` | 原型完整 `name`；可以是中文显示名，不能猜成函数名 |
| `function` | 当前适配器要求 `<namespace>.<filename>`；其他别名形式须扩展适配器 |
| `state` | `active`、`deprecated_compatible`、`hidden_compatible`、`unregistered`、`internal` |
| `group` | 精确分组名称，无组明确填 `null` |
| `allowed_file_changes` | 本次允许整文件变化的相对路径；通常仅列注册 Python/XML，不能填 package/block |
| `allowed_json_changes` | 文件到 JSON 指针列表的映射，仅允许本次具体字段或必要子树变化 |

状态含义与前置条件以[旧入口策略](contract-ui.md#旧入口的三种策略)和[当前元数据规范](metadata-delivery.md)为准。清单不是授权证据：`unregistered` 始终要求人工核验授权和消费者迁移；基线中曾公开的函数不能标成 `internal` 绕过注销。`hidden_compatible` 始终待复核，不自动认证当前版本有独立隐藏能力。

许可例：`"/flows/2/opened"` 只允许第 3 个流程的开关变化；`"/blocks/4/inputs/0/label"` 只允许某个 label 变化。索引来自修改前后实际结构，数组增删重排须逐项复核，不能为消除告警直接放开整张 `/flows` 或 `/blocks`。不接受通配符或根对象许可；JSON 指针中的 `~`、`/` 分别转义为 `~0`、`~1`。

`.dev/*.flow.json` 的参数既有字段投影又有全文件保护。获授权修改其中参数时，同时列出具体参数 JSON 指针和该文件的整文件许可，并人工复核流程动作及其他内容；仅有整文件许可不能放过未授权参数变化。Python/XML 允许整文件变化后，无关代码、导入、成员及说明仍需人工 diff。许可不解除弃用兼容入口的原契约保护。

## 执行方式

在 skill 目录运行，替换明确的应用路径、应用 ID 和外部证据目录：

```powershell
python -B -X utf8 scripts/check_metadata.py snapshot --app-dir '<应用目录>' --expect-app '<uuid>'
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<完整策略.json>' --baseline '<修改前快照.json>' --phase offline
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<完整策略.json>' --baseline '<修改前快照.json>' --phase after_save
python -B -X utf8 scripts/check_metadata.py check --app-dir '<应用目录>' --policy '<完整策略.json>' --baseline '<修改前快照.json>' --stable-snapshot '<真实保存后快照.json>' --phase after_reopen
```

快照和报告只输出到 stdout。保存证据时先检查退出码及 JSON 是否完整，再写入应用目录之外的新文件，保留原快照，不用失败输出覆盖基线。例如：

```powershell
$snapshot = & python -B -X utf8 scripts/check_metadata.py snapshot --app-dir '<应用目录>' --expect-app '<uuid>'
if ($LASTEXITCODE -ne 0) { throw ($snapshot -join "`n") }
$snapshot | ConvertFrom-Json | Out-Null
$target = '<外部证据目录>\before.json'
if (Test-Path -LiteralPath $target) { throw '证据文件已存在，停止覆盖' }
[System.IO.File]::WriteAllText($target, ($snapshot -join "`n"), [System.Text.UTF8Encoding]::new($false))
```

两种基线不能混用：

- **修改前基线**：修改前采集，按本次精确许可比较，保护未知字段、源码和原契约。快照只含哈希，不是可恢复备份，另存原文件备份。
- **稳定基线**：正确打开并真实保存后另采一份；再次完整关闭、重开、真实保存后比较。忽略所有修改许可，任何受保护文件变化都报 `STABILITY_DRIFT`，包括仅格式变化；应复核原因而非自动刷新基线消除告警。

每次读取前后重新比较文件集合和内容，发现变化即停止。此机制不是事务锁，不能证明写入前或后续客户端操作没有并发修改。基线检查会拒绝明显缺失、损坏或应用路径不符的证据，但不能认证人为改写过的快照来源；基线保全及时间证据另行记录。

## 覆盖范围

- 核对 package、block、顶层 Python 函数、XML 成员的精确集合与实际绑定，不用数量代替成员核对；重复、缺失、跨命名空间或歧义结构不猜测。
- 核对完整策略覆盖、开关布尔值、分组存在和引用、参数名称/方向/类型、可识别的流程与原型输入默认值、Python/XML 签名，以及已识别包装形式的输入映射、输出声明和 `process.run` 绑定。调用参数顺序按流程 In/Out 核对，原型面板允许独立排布；各自原有顺序仍参与基线保护，不强制两种顺序相等。
- 检查已识别 `%参数名%` 动作占位符和 label 缺失；不证明中文文案质量、编辑器可用、实际渲染或发布后的展示。
- 保护四层文件、根目录 `*.py`、`.dev/*.flow.json` 和存在时的 `.dev/workspace.state.json`。不递归保护依赖、资源和虚拟环境，相关变更需另建清单及 diff。
- package/block 包含未知字段的 JSON 叶节点和容器类型也参与比较；Flow JSON 仅投影参数，其余内容受全文件哈希保护。签名和原契约用结构化指纹记录，不输出默认值正文。

当前只适配已观察到的顶层函数式注册和 XML `item` 成员。导入式导出、动态别名、未知默认值编码、无法识别的包装结构等报告待复核；格式不满足解析前提时返回输入错误。检查器不执行控制流分析，不验证未定义变量、SDK 方法存在、依赖可导入、业务返回或异常语义，也不验证仅隐藏能力、注销授权、真实关闭保存重开及发布消费者。

不要把读取安全误写成“待查模块导入安全”：检查器不会执行源码，因此即使源码 import 时包含动作，静态元数据结果也不能证明该模块可安全导入。

## 结果和验收

| 退出码 | `status` | 含义 |
| --- | --- | --- |
| `0` | `no_findings_in_checked_rules` | 本次已执行规则未发现诊断，不是完整通过 |
| `1` | `issues_found` | 发现错误，或输入/快照不可用；停止写入和成功交付 |
| `2` | `needs_review` | 缺少基线、未知格式或需要人工证据，不能当作通过 |

报告始终为 `acceptance_complete=false`，`manual_evidence` 始终为 `not_verified`。`--phase after_reopen` 只是调用者标签，不能自行变成重开证据；该阶段未提供稳定快照也会列为待复核。按[统一验收报告](../assets/acceptance-report.md)补充独立状态回读、真实保存、旧消费者、授权和发布证据；不手工修改静态报告来补“通过”。

离线回归：

```powershell
python -B -X utf8 -m unittest discover -s scripts -p test_check_metadata.py -v
```

测试只在临时目录构造样本，覆盖策略区分、契约保护、未知格式、快照漂移、并发变化、只读性及 CLI 退出码，不代表真实客户端已验收。
