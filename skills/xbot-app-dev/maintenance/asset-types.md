# 资产分类与修复范围

先看真实调用点及对象来源，不按应用名称、XPath 字符串或错误文字分类。

| 对象 | 修复重点 |
| --- | --- |
| 普通应用 | [应用结构](../../xbot-development-standards/references/application-development.md)；正常主流程与运行参数，不套用指令集注册清单 |
| 指令集 | [开发索引](../../xbot-development-standards/SKILL.md)；In/Out、中文参数面板、启用/展示与旧调用兼容 |
| CodeFlow | [CodeFlow](../references/codeflow.md)；精确替换、同一 args 回写、参数及调用链 |
| VisualFlow | [VisualFlow](../references/visualflow.md)；真实 block_id、原型/字段包装及部分成功 |
| Web DOM / iframe / Shadow DOM | [Web 模块](../modules/web.md)；作用域、原用途、身份、单元素/集合及父子引用 |
| 原生业务窗口 / 系统弹窗 | [Windows 模块](../modules/win.md)；HWND/PID、进程实例、UIA 身份和动作方式 |
| 文件 / API / 数据库等 | 当前 SDK/接口/格式契约；不强行进入元素定位修复 |

对外参数名、方向、类型、返回语义和异常语义保持兼容。新增内部标识优先英文，业务面板与注释用中文。缺陷修复不顺带迁移整份 VisualFlow 或批量重命名。

专用指令注册/展示配置确需直接维护时，先读 [元数据流程](../../xbot-development-standards/references/metadata-delivery.md)，保存原文、关闭设计器、核对四层并真实保存重开；普通源码修改仍通过正常客户端通道。只读检查器的通过不证明发布后消费者可用。
