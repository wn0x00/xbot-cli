# CodeFlow 开发与维护

## 工作流

Studio 命令建议追加 `--expect-app <app-id>`，它核对当前应用而不切换应用。以下流程名称应替换为实际返回的 `flow_id`。

```bash
xbot-cli status
xbot-cli app open <app-id>                 # 维护已有应用；新建用 app create
xbot-cli flow list
xbot-cli flow create hello --kind code     # 记录返回的 flow_id
xbot-cli flow params list hello
xbot-cli flow params set hello --operations @params.json
xbot-cli flow code write hello --file hello.py  # 新流程全量写
xbot-cli flow check hello                 # 只做静态检查
xbot-cli flow verify hello --timeout 600
```

维护已有代码先读后改，优先精确替换，不整份重写：

```bash
xbot-cli flow get hello
xbot-cli flow code edit hello --old @before.txt --new @after.txt
xbot-cli flow verify hello --timeout 600
```

`params.json` 可为 operations 数组或 `{"operations":[...]}`：

```json
[
  {"op":"upsert","name":"text","direction":"In","type":"str","value":"测试默认值"},
  {"op":"upsert","name":"result","direction":"Out","type":"str"}
]
```

## 源码契约

权威文档：`xbot-cli wiki read wiki/guides/codeflow wiki/guides/flow-parameters`。

- 唯一入口 `def main(args):`，不要增加 `main()` 或 `if __name__ == "__main__":`。
- 默认骨架：

  ```python
  import xbot
  from xbot import print, sleep
  from . import package
  from .package import variables as glv

  def main(args):
      if args is None:
          args = {}
      text = args.get("text", "测试默认值")
      args["result"] = text
      print(text)
  ```

- 日志使用 `from xbot import print`；需要级别时使用 `xbot.logging`。
- Out 必须先声明，再写回同一个 `args["参数名"]`；普通 return 不进入 Flow Out。
- 只在 `args is None` 时初始化，不用 truthiness 替换空容器。
- 默认按 Python 3.7 兼容；确认应用升级后才使用新语法。
- 保留无关行为、命名和局部风格，不借机格式化。
- 日志不输出 Cookie、Token、密码、验证码或完整页面。

## 变量命名

编码版变量与函数参数默认使用英文 `snake_case`，例如 `search_keyword`、`target_element`、`retry_count`；具体规则及既有接口的兼容边界见 [项目规范](project-conventions.md)。避免生成中文变量名或驼峰变量名。

既有流程若使用中文 In/Out 名称，保留已声明的字典键，在边界映射为英文内部变量。以下仅为映射示例，运行前须按既有合同声明对应参数：

```python
search_keyword = args.get("搜索词", "")
search_result = {"keyword": search_keyword}
args["搜索结果"] = search_result
```

## 流程组织与 XPath

流程按 [项目规范](project-conventions.md) 组织为配置、业务、测试和根级主流程。保持既有调用和参数契约，不为一次元素修复迁移整个应用。

编写网页代码前读取 `wiki/guides/web/codeflow`、`wiki/xbot_sdk/web/browser` 和需要的 `wiki/xbot_sdk/web/element`，核对实际签名。本项目默认直接 XPath 定位，覆盖 Wiki 中默认通过 `package.selector()` 获取稳定控件的实现偏好；其他运行环境和 SDK 能力约束仍需遵守。

当前已核验的查找接口：

```python
# browser 为已取得的 WebBrowser；verified_xpath 来自当前页面验证。
target = browser.find_by_xpath(verified_xpath, timeout=10)
# 多候选诊断或新旧版本消歧时读取全部，再按业务条件判断。
candidates = browser.find_all_by_xpath(verified_xpath, timeout=10)
```

- `find_by_xpath` 无匹配或多匹配会报错；`find_all_by_xpath` 返回列表，无匹配时超时后返回空列表。不能直接对结果取 `[0]`。
- 行、卡片或弹窗内的操作从已取得的父 `WebElement` 调用 `find_by_xpath` / `find_all_by_xpath`，相对 XPath 保持在父区域内，不使用会重新扩大到全页的路径。
- 将已验证 XPath 按业务用途组织为常量或配置；不要把暂存 ref、坐标或未经核验的 XPath 写进代码。等待标志和可操作目标分别定义。
- 新旧候选先定位、去重并判断业务状态，唯一目标确定后只执行一次动作；具体分支见 [维护指南](web-maintenance.md)。动作已可能生效时不能因异常自动切换另一候选重做。
- 普通 XPath 无法表达或当前 SDK 不支持的上下文可使用已验证的元素库 selector；名称遵循 `平台名_业务含义_按钮/输入框/标志`。不主动将可直接定位的网页元素转成捕获资产。

## 第三方依赖

依赖只装进应用 venv：

```bash
xbot-cli tool studio call python_pip @pip.json
```

```json
{"operation":"install","packages":[{"package_name":"requests","version":"2.32.3"}]}
```

## 验收

`flow check` 检查整个应用的静态诊断，传入流程时先核验目标（`scope: app`），不启动流程。`flow verify` 检查后实际运行并返回 `run_id` 和 `outputs`。失败时错误 `data` 自动带 `log_tail`；超时用 `flow wait <run-id> --timeout 600` 继续等待，不重新启动。写入成功不代表静态或运行验证通过；未实际运行的路径不能宣称已验证。
