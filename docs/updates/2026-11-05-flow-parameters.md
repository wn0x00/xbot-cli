# xbot-cli 0.9.3 更新说明

版本：0.9.3。日期：2026-11-05。

本次增强流程参数管理，新增本地类型查询和参数写入适配器，支持集合及对象的真实类型，
并提供完整使用文档。已有浏览器能力保持不变。

## 不受 MCP 六类型白名单限制的参数写入

```powershell
xbot-cli flow params types
xbot-cli flow params types --json
xbot-cli flow params list process2 --expect-app <app-id> --json
xbot-cli flow params set process2 --operations @params.json --expect-app <app-id> --json
```

`types` 直接读取本机影刀客户端 DLL 中的类型资源，不调用客户端 MCP，也不加载或执行该 DLL。
终端以表格展示 ID、中文名称及声明的适用范围，JSON 保留来源、基础类型和源文件摘要。

参数写入不再依赖原生 MCP 的 `flow.write_parameters` 接口，避免该接口仅支持
`str/file/int/bool/float/any`、拒绝其他类型写入的限制。
CLI 本地适配器使用真实类型 ID，支持 `list`、`dict`、网页、Excel、窗口等参数；
不再要求把集合与对象统一声明为 `any`。

CodeFlow、VisualFlow 共用该适配器，支持 In/Out 的批量新增、更新和删除。
先校验完整批次，必要时保存已有编辑，仅写入目标流程的参数元数据区域，
再通过客户端 MCP 执行一次 reload 并回读确认。
不需要手动关闭或重开编辑器，不修改客户端 DLL、生成的 Python 或签名文件。

## 默认值与写入保护

- 同类型、同方向更新时，未提供的字段保留原值；类型或方向改变时重置不兼容的初始值。
- 新增列表、字典分别默认 `[]`、`{}`，对象默认 `None`，Out 默认无初始化表达式。
- 保留客户端未初始化 Out 的 `null` 或缺失值表示，避免无意义改写。
- 无净变化时不写入、不 reload；非法批次在写入前整体拒绝。
- 核对应用身份、编辑模式、运行状态、并发变化及磁盘与客户端内存的一致性。
- 失败结果明确保留阶段和写入状态；结果未知时不自动回滚或整批重试。
- `--dry-run` 仍是离线结构预览，不等同客户端接受性或实际运行验收。

完整字段、默认值、增改删示例与恢复说明见[流程参数使用文档](../flow-parameters.md)，
也可离线查看：

```powershell
xbot-cli guide flow-parameters
```

## Skill 更新

更新应用开发 Skill 的类型选择、参数默认值及写入失败处理说明。
开发规范新增业务步骤注释要求：业务功能函数在关键阶段前适当添加中文业务注释，
便于定位；不加序号、分割线或装饰性标题，不逐行复述代码。
Skill v10 标记保持不变。

## 验证范围与限制

已在影刀 6.3.39 的“子流程2”验证 62 种类型、124 项 In/Out 参数的写入、reload、
回读及默认值输出传递，流程运行通过。无变化更新与非法批次未产生文件修改。

对象类只验证类型契约和空值占位，`None` 不会创建浏览器、Excel 或数据库连接。
真实对象的业务操作、真实 CodeFlow 运行、其他客户端版本及动态扩展类型未在本次验证。
CodeFlow 参数文件适配与 Python 文件不被改写由离线测试覆盖。

类型目录是当前已安装客户端的静态资源，不保证覆盖所有动态类型或所有编辑器的选项。
不提供 `password` 参数类型；`file/directory` 保留资源中的主流程用途声明，
在子流程使用时返回范围 warning。

## 安装与附件

```powershell
npm install -g '@guanzhu.me/xbot-cli@0.9.3'
xbot-cli version
```

GitHub Release 提供 Windows x64 ZIP、同版 npm `.tgz` 和 `SHA256SUMS.txt`，
附件来自通过安装验收的同一份构建。
