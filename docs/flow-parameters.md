# 流程参数使用文档

适用于 xbot-cli 0.9.3 及以上版本。CodeFlow 和 VisualFlow 使用同一套参数命令。
完整文档也内置于 `xbot-cli guide flow-parameters`。

## 使用前提

- Windows x64，已安装影刀，安装目录包含类型资源 DLL 和自带的 .NET 运行时。
- 实际修改需要客户端已启动、登录，以编辑模式打开目标应用，且没有运行任务。
- 目标流程已保存到本地；新建但未保存的流程先执行 `xbot-cli app save`。
- 操作期间不要在客户端编辑、切换应用，或从另一进程修改同一应用。
- 示例中的 `<app-id>` 和 `process2` 替换为实际应用 ID、流程 ID。先通过 `status`、`flow list` 确认。

```powershell
xbot-cli status --json
xbot-cli flow list --expect-app <app-id> --json
```

CLI 使用影刀自带运行时，使用者不需要另装 .NET SDK。
安装发现异常时可用 `XBOT_SHELL_CLI` 指向实际版本的 `shadowbot.shell-cli.exe`。
若 Studio MCP 使用非默认地址，可在当前 PowerShell 会话指定实际端点，例如：

```powershell
$env:XBOT_MCP_ENDPOINT = 'http://127.0.0.1:42501/api/v1/mcp'
```

## 查询类型和参数

```powershell
# 不连接 Studio；从本机客户端 DLL 只读提取类型定义，默认终端显示表格
xbot-cli flow params types
xbot-cli flow params types --json

# 读取当前应用指定流程的参数
xbot-cli flow params list process2 --expect-app <app-id> --json
```

`types` 返回 `id/name/scope/base/source`，以及目录数量、源 DLL 路径和 SHA256。
`type` 必须使用返回的完整 `id`，区分大小写；不要将对象类型改写成 `any`。
当前 6.3.39 客户端得到 62 项，数量随本机安装版本而定，不提供 `password` 参数类型。

| 类别 | 常用类型 ID |
|---|---|
| 基础值 | `str`、`int`、`float`、`bool`、`any`、`bytes`、`datetime` |
| 集合 | `list`、`dict`、`table.row`、`table.column` |
| 路径 | `file`、`directory` |
| 网页 | `xbot._web.browser.WebBrowser`、`xbot._web.element.WebElement` |
| Windows | `xbot.win32.window.Win32Window`、`xbot.win32.element.Win32Element` |
| 办公和数据库 | `openpyxl.Workbook`、`Word.Document`、`xbot.database` |
| 其他 | OCR、邮件、FTP、移动端、AI 等对象，完整 ID 以本机 `types` 为准 |

这是静态资源目录，不保证覆盖扩展或动态类型，也不代表客户端每个编辑器都提供对应选项。
新建未知类型会失败；既有未知类型可保持同类型同方向更新，或按名称删除。
`file/directory` 的资源声明为 `appMainFlowParameter`，本次在 6.3.39 子流程验证可用；
CLI 保留该声明并返回 warning，不保证其他版本或界面的选择规则。

## 批量新增参数

准备 UTF-8 JSON 文件，例如 `params.json`：

```json
[
  {
    "op": "upsert",
    "name": "keyword",
    "direction": "In",
    "type": "str",
    "kind": "Text",
    "value": "业务默认值",
    "description": "检索关键词"
  },
  {
    "op": "upsert",
    "name": "items",
    "direction": "In",
    "type": "list",
    "kind": "Expression",
    "value": "['A', 'B']",
    "description": "待处理项目"
  },
  {
    "op": "upsert",
    "name": "options",
    "direction": "In",
    "type": "dict",
    "kind": "Expression",
    "value": "{'enabled': True, 'limit': 10}"
  },
  {
    "op": "upsert",
    "name": "web_page",
    "direction": "In",
    "type": "xbot._web.browser.WebBrowser",
    "description": "由调用方传入网页对象"
  },
  {
    "op": "upsert",
    "name": "workbook",
    "direction": "Out",
    "type": "openpyxl.Workbook",
    "description": "流程产出的工作簿"
  }
]
```

先预览，再实际写入并回读：

```powershell
xbot-cli flow params set process2 --operations @params.json --expect-app <app-id> --dry-run --json
xbot-cli flow params set process2 --operations @params.json --expect-app <app-id> --json
xbot-cli flow params list process2 --expect-app <app-id> --json
```

也接受 `{"operations":[...]}` 包装，或通过 `--operations -` 从标准输入读取。
`--dry-run` 只做离线结构校验，不写文件、不连接客户端；
不验证实时类型目录、目标流程、表达式语法或实际可运行性。

## 字段与默认值

| 字段 | 规则 |
|---|---|
| `op` | `upsert` 为新增或更新；`remove` 为按名称删除 |
| `name` | 参数名；新增/更新须为有效 Python 标识符，不能是关键字或规范化重名 |
| `direction` | `In` 或 `Out`；upsert 必填 |
| `type` | 本机目录中的完整真实 ID；upsert 必填 |
| `kind` | `Text` 或 `Expression`；建议显式填写 |
| `value` | 必须是字符串，包含文本或 Python 表达式源码，不是 JSON 原生布尔值/数组 |
| `description` | 可选字符串说明 |

`Text` 表示字面文本，例如 `"hello"`；`Expression` 表示 Python 表达式源码，
如 `"True"`、`"[]"`、`"{'a': 1}"`、`"None"`。CLI 不求值，不执行表达式。
非空 `any` 初值必须显式提供 `kind`；其他类型省略 kind 时使用下表或保留已有 kind。
In 使用 Expression 时不能填写空白表达式。

| 新增 In 类型 | 省略初值时的 kind / value |
|---|---|
| `str/file/directory` | `Text` / 空字符串 |
| `int` | `Expression` / `0` |
| `float` | `Expression` / `0.0` |
| `bool` | `Expression` / `False` |
| `list` 或目录 `base=list` | `Expression` / `[]` |
| `dict` | `Expression` / `{}` |
| `bytes` | `Expression` / `b''` |
| `any/datetime` 及其他对象 | `Expression` / `None` |
| 新增 Out，任意类型 | `Expression` / 空字符串，表示无初始化表达式 |

对象默认 `None` 不会打开网页、创建工作簿或连接数据库。实际对象须由上游传入，
或在流程中通过对应指令/SDK 创建。Out 需要流程实际赋值；
VisualFlow 的产出变量需与 Out 参数同名，CodeFlow 按对应输出契约返回。

## 更新和删除

```json
{
  "operations": [
    {
      "op": "upsert",
      "name": "items",
      "direction": "In",
      "type": "list",
      "description": "新的业务说明，保留原初值"
    },
    {
      "op": "remove",
      "name": "obsolete_parameter"
    }
  ]
}
```

- 同名称、同类型、同方向更新时，省略 `kind/value/description` 会保留已有值。
- 类型或方向改变后，省略的 `kind/value` 按新契约重新取默认值，避免保留不兼容初值。
- `remove` 只接受 `op/name`。删除不存在的名称没有净变化。
- 整批先校验再写入，任一非法操作使整批停止。多次操作按数组顺序执行。
- 无净变化时不写入、不 reload。删除再新增可能改变参数顺序，不等同原位更新。

## 写入过程与结果

CLI 使用本地元数据适配器，而不是扩展客户端原生 `flow.write_parameters` 白名单：
先核对编辑模式、应用身份和空闲状态，整批校验；存在未保存编辑时先保存，
再检查磁盘与客户端参数一致，仅替换目标 `.dev/*.flow.json` 的参数区域，
调用一次 `app.reload`，最后通过 MCP 回读完整参数契约。

不会调用 sync，不发布应用，不手工修改 Python、PDB、签名或客户端 DLL。
reload 会丢弃未保存的内存状态，因此已有编辑必须先保存；保存可能包含该应用的其他已有修改。
reload 由客户端执行，不需要你手动关闭或重开编辑器。

成功结果的关键字段：

| 字段 | 含义 |
|---|---|
| `saved_existing_changes` | 是否先保存了已有编辑 |
| `written` | 是否有净变化并实际替换了参数文件 |
| `reloaded` | 是否已完成 reload；无变化时为 false |
| `readback_verified` | 客户端参数回读与计划一致 |
| `runtime_verified` | 参数写入命令始终为 false，不替代运行验收 |
| `changes/warnings` | 各项修改及范围提示 |

需要实际运行时另执行：

```powershell
xbot-cli flow check process2 --expect-app <app-id> --json
xbot-cli flow verify process2 --timeout 600 --expect-app <app-id> --json
```

`flow verify` 会真实运行目标流程，须确认其业务副作用。已有运行超时保留 `run_id`，
使用 `flow wait <run-id>` 继续等待，不重复启动。

## 失败处理

失败输出到 stderr，读取 `error` 和 `data.stage/data.progress`。
写入、reload 结果可能为 `true/false/null`，其中 null 表示未知，不代表未生效。

- 目标切换、运行中、View 只读、多账号目录歧义、并发编辑：先处理冲突，再重新读取现状。
- 磁盘参数与客户端内存不同：不会强行覆盖，先确定应保留哪份内容。
- `written:true` 后 reload/回读失败：文件可能已经生效，不要保存旧内存覆盖磁盘，也不要整批重试。
- 结果未知：先核对当前应用和参数，只补未完成的恢复动作。CLI 不自动回滚。
- 当前实现要求已保存的流程元数据和类型资源存在，不修改临时 View 副本。

## 本次验证范围

客户端 6.3.39 的“子流程2”验证了 62 种类型的 In/Out 参数元数据、默认值和输出传递；
对象类只验证空值占位及类型契约，未据此证明真实浏览器、Excel、数据库等对象的业务操作。
CodeFlow 的参数文件格式和 Python 不被改写由离线测试覆盖；
真实 CodeFlow 运行、其他客户端版本及动态扩展类型不在本次验证范围内。
