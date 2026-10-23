# Web 自动化模块

状态：从当前 v10 整理，保留原脚本、模板和引用路径；本次模块化不改 XPath 算法。共享操作和授权规则继承 [SKILL.md](../SKILL.md)。

## 进入条件

修复前由 Agent 按 [平台识别与模块选择](../references/automation-routing.md) 确认实际失败调用操作的是网页对象，再进入本模块。桌面程序中的 WebView/Electron DOM 也可属于 Web；网页流程中的系统文件弹窗则按原生调用进入 Windows 模块。XPath 字符串和 `find_by_xpath` 名称本身不证明接收对象类型。

失败调用及对象来源已足以确认 Web 平台时，可以先加载本模块了解能力；实际采集和运行修复脚本前，还须确认对应页面及作用域。对象来源不明则返回平台调查，不能调用 Web 修复脚本猜平台。平台确认后，仍须分别通过用途、业务身份和修复验证。

## 工作阶段与按需读取

| 阶段或触发 | 必读资源 |
|---|---|
| `explore`：页面、登录态、元素、分页、业务路径 | [网页生命周期](../references/web-lifecycle.md)、[浏览器契约](../references/browser.md) |
| `build`：从需求建立流程 | [网页生命周期](../references/web-lifecycle.md)、[CodeFlow](../references/codeflow.md) 或 [VisualFlow](../references/visualflow.md) |
| `verify`：静态检查、真实运行、业务结果 | [验收](../references/web-acceptance.md) |
| `maintain`：失败、超时、选择器漂移 | [维护](../references/web-maintenance.md)、[恢复](../references/recovery.md) |
| ref 转 XPath、旧 XPath 零匹配、ROBULA+、修复提议 | [定位修复](../references/locator-repair.md) |
| 修复前判断单元素/集合、查询后遍历或筛选 | [用途分析](../references/locator-usage.md) |
| 相似元素组、父子 XPath 联动、新旧集合兼容 | [集合修复](../references/collection-repair.md) |
| 副作用与用户确认边界 | [网页风险](../references/web-risk.md) |
| `release`：保存、同步、发布和已发布应用运行 | [恢复](../references/recovery.md)、[CLI 契约](../references/cli-contract.md) |

## 浏览器探索

网页 CLI 适合探索、短流程和一次性取数；重复执行、参数化和长期维护优先沉淀为 CodeFlow。网页流程优先用已验证的 XPath；VisualFlow 用“获取元素对象（web）”搭配 XPath，CodeFlow 用公开 XPath API。

```bash
xbot-cli web open https://example.com --json
xbot-cli web snapshot --json
# e4 仅为语法示例，使用当前完整快照中的实际 ref
xbot-cli web fill e4 "搜索内容" --json
xbot-cli web press Enter --ref e4 --json
```

打开、附着、导航和常用交互后默认返回快照；`--no-snapshot` 跳过，`close` 不取快照。快照可能是增量，保留元数据，不把增量当完整页面。页面变化、导航、提交或弹窗变化后重新 snapshot；ref 不能编造、跨页面复用或替换成 CSS/XPath。

动作成功而后续快照失败时，保留 `applied: true`，只补 `web snapshot`；超时或连接中断先确认动作结果。未知结果不重放。

元素选择器名称先暂存于后端；省略 `--name` 时 CLI 自动生成临时名称。只有 `web_review` 才把选定选择器写入项目元素库，先读当前 `tool browser schema web_review`；不能把“XPath 生成成功”当作元素库已保存。

## 快照到定位和修复

```text
实际报错、run_id、源码/块
  -> 核实失败调用点、页面状态及动作是否已发生
  -> AI 读取上下游代码，输出用途、XPath 含义和证据
  -> 校验 single/collection/collection_then_single/mixed/unknown
  -> 历史业务身份、独立基线、父子依赖合同
  -> 当前 snapshot/ref，确认目标与上下文
  -> 已确认唯一节点：ROBULA+ + 加权算法生成、回查
     集合：候选区域 + 每成员合同 + 完整性清单
  -> 兼容旧/新父定位及各自子定位，负样本验证
  -> 复用测试子流程、动作前复核、独立业务回执
  -> 证据充分时最小改动、回读、静态及真实运行验收
  -> 保留定位资产与分析报告
```

AI 负责根据原流程分析目标用途、业务含义、必要子元素、动作及后置条件；它不凭相似度生成或采用 XPath。当前匹配数为零不能推导原用途。用途未知、混用未拆清、没有可信身份基线时停止自动采用。

单元素修复器只接受已校验的单元素用途。身份包括页面/记录、动作、区域、iframe 和 Shadow Host；唯一匹配仍不证明业务身份正确。

相似元素组不得缩成一个元素。保留旧/新父定位与对应相对 `./` 子定位，按逐成员业务键和字段验证；不同真实区域都满足契约时停止。新旧定位指向同一当前实例才可去重，不能仅按相同文本合并。

密集页采用 `business` 校验：独立完整性清单、逐键业务字段基线、候选区域消歧、动作前复核；必须有会混淆身份的负样本和独立业务回执。结构可解析不等于业务正确。

长期资产不只存临时 ref；记录 XPath、作用域、新旧变体、匹配数、调用点及验收证据。[定位登记模板](../templates/locator-registry.json) 是说明模板，不是执行器。

## 既有脚本入口

| 入口 | 作用及边界 |
|---|---|
| [locator-usage.cjs](../scripts/locator-usage.cjs) | 校验 AI 用途分析和源码证据；交接前回读流程 |
| [locator-repair.cjs](../scripts/locator-repair.cjs) | 当前快照和已确认目标的只读定位修复提议 |
| [collection-compat.py](../scripts/collection-compat.py) | 集合合同、旧新父子兼容、候选筛选与动作前复核 |

具体参数从各 reference 和脚本帮助读取。现有 JSON 模板继续使用原路径。脚本不点击、不写应用或元素库；`REPAIR_VALIDATED` 只是提议，`ACTION_REVALIDATED` 只是当前页面复核。写入按既有授权做最小修改并回读；业务结果通过后才提升定位资产版本。

## 已知边界

现有集合实现的密集页验收覆盖普通 DOM，不将其推广为 iframe/Shadow DOM 集合已通过。单元素上下文链路与集合适配分别验收。ROBULA+ 是已确认 DOM 节点的 XPath 生成器，不负责判断业务身份、集合完整性或自愈。

同位置同内容替换、后端动作绑定、观测到点击之间的变化仍有缺口；字段一致和句柄可用不能完全证明原业务对象。分页/虚拟列表当前可识别不完整而拒绝，尚无通用遍历修复协议。大页 ROBULA+ 适配器有 20,000 元素预算，不能为追求生成成功而直接放宽。

历史密集测试为 43 类两轮、86 次执行，其中 84 次达到预期、2 次同内容替换仍为缺口；达到预期包含正确拒绝。其余压力和独立测试详见桌面交付的历史报告，不把模块化检查记成新增业务修复验收。
