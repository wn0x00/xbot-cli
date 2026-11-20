# v0.11.2 — Windows 自动化增强与 Skill 维护优化

1. 完善 32 位与 64 位应用激活

   - 自动识别目标架构

2. 新增企业微信专用激活适配

   - 新增 --provider auto|wework|shadowbot。
   - 符合条件的 x86 企业微信可使用内置适配

3. 默认使用物理点击

   - win32 click 默认改为 physical，点击目标控件实时中心。
   - 保留 --method invoke，两种方式不自动回退。
   - 点击、双击和悬停返回目标控件、实时范围、实际方法及坐标。

4. 修复元素树采集不完整

   - 修复空 RuntimeId 被误判为重复节点、导致子树截断的问题。
   - 保留相关节点和子树；身份不足时明确提示动作限制。

5. 完善激活结果与异常处理

   - 校验窗口、进程实例和架构，避免重复安装。
   - 区分架构不支持与 E_NOTIMPL。
   - 分别报告激活核验和元素树核验，结果未知时避免重复注入。

6. 同步优化两份内置 Skill

   - 保留原名称，整合任务分流、故障前置分析、版本环境和输入消费路径核对。
   - 完善临时测试恢复，统一普通应用与指令集交付规则。
   - 更新 Web/Windows 指引，新增 guide maintenance、`guide element-preconditions`、`guide run-existing`。

7. 同步文档与测试

   - 格式检查、Clippy、358 项自动化回归、Windows 构建及 npm 独立安装验包通过。
   - 另有 4 项按默认配置标记跳过。

升级后，依赖 InvokePattern 的调用需显式添加 --method invoke。CLI 仍面向 Windows x64；支持 x86 应用不代表支持 32 位 Windows。
