# Windows 原生组件与激活适配

以下用途基于本机 ShadowBot 6.4.900 的 PE、导出与反编译证据；后续版本的 ABI 仍以调用时验证为准。

| 文件（x86/x64，misc 仅 x86） | 用途 | 当前接入 |
| --- | --- | --- |
| RpaBuilder.dll | 应用框架适配和元素树激活 | activate 按目标位数调用 InstallRpaDllSimple；x86 为 stdcall 两参数 |
| misc.exe | wxwork 专用消息钩子与兼容修补 | activate --provider shadowbot 的 x86 企微路径；必须核验厂商消息探针 |
| debugger.dll | DoDllProcAny：远程加载具体 DLL 并调用导出 | 已识别依赖关系；没有独立的 UIA 激活逻辑，不暴露任意 DLL 执行命令 |
| debugger.host.exe | RpaBuilder RPC 服务宿主 | 一次性 activate 无需启动；x86/x64 命令行和就绪事件参数不同 |
| ShadowBotJavaBridgeHook.dll | 引导 JVM 内 Java 桥接 | 尚未接入桥接握手和 Java 树协议；依赖 debugger 及 support_*/java/BridgeLib/V2 的 JAR |
| ShadowBot.Inspectors.dll | AnyCPU CIL 的 WinForms/第三方控件检查器 | 尚未接入目标 CLR 装载及跨进程树协议；独立进程直接引用不能读取远端 Control |
| qt4_support/accessible/qtaccessiblewidgets4.dll | Qt 4.8.7 无障碍插件 | 保留目录，由匹配的 Qt4 / RpaBuilder 支持流程加载；依赖 QtCore4、QtGui4、MSVCR100 |
| vcruntime140.dll / vcruntime140_1.dll | VC++ 异常处理、栈展开等运行库 | 作为同位数原生组件的依赖加载；没有元素树入口，不能替代 MSVCR100 |

Java 的 HookTargetJavaProcess 即使远端调用失败也可能返回 0；LoadJavaBridgeStatic 另启线程后立即返回，不能据返回码或 DLL 出现在模块列表就报告完整激活。Inspectors 返回的是自有 UiTreeEntryRoot，需要目标 WinForms CLR 上下文；Java/WinForms 专用树也不等于 UIA 树，当前尚未接入。

## 企微内置适配

用户提供的 WeWorkUIA-Universal.py 揭示了另一种机制：从 DuiLib 的 CWindowWnd::OnGetObject 导出及活对象虚表找到 WXWork 的 HandleMessage，补上 WM_GETOBJECT/OBJID_CLIENT 分发，使 MSAA/UIA 可以获取控件。

CLI 用 Rust 实现解析和一次性激活，无 Python、pefile 或影刀依赖。相较原脚本，增加进程实例互斥、文件/映像身份校验、唯一导出和虚表校验、完整指令校验、独立代码页、this 指针保存、短暂暂停与线程 EIP 检查、失败回滚，以及完整已知钩子的重复执行核验。没有任意 E9 即认为成功的逻辑，也不扫描全模块后任选相邻数据字节。未验证的布局返回不支持。

成功后代码保留到应用退出，不恢复补丁、不卸载、不常驻。hook_verification 与 accessibility_verification 分别表示代码核验和 WM_GETOBJECT 返回情况；tree_verification 保持 not_checked，实际树用 snapshot 读取。

本机企微 5.0.10.6015（x86）实测：激活前 3 节点，激活后 914 节点，本次有界遍历完成；二次激活 already_active / install_called=false。这个结果只代表该窗口当时暴露的 UIA 树，不保证所有版本、页面或控件都可操作。该版本大量控件没有 RuntimeId：快照保留子树并提示，现有动作定位仍拒绝缺少身份的 ref。

## 自建窗口验证

```powershell
npm run test:win32-collect
npm run test:win32-wework
npm run test:win32-misc
npm run test:win32-activation
```

collect 为离线测试。其余为显式运行的本机测试，编译自有隐藏窗口，不选择业务程序；要求已有 Visual Studio C++ 工具，厂商组件测试还需要本机影刀。Qt 测试通过 XBOT_TEST_QT_X86 / XBOT_TEST_QT_X64 指定已有 Qt 运行库。厂商文件继续使用本机安装路径，不复制到 npm 包。
