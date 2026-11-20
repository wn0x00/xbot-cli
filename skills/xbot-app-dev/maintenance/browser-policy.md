# 浏览器选择与复现条件

仅在网页或浏览器依赖任务读取。优先级：用户明确要求 → 项目/团队已配置规则 → 原流程实际选择与用户故障环境。普通任务不强制 Chrome，亚马逊任务也不自动强制紫鸟。

```powershell
xbot-cli web browser list --json
xbot-cli web active --browser <BROWSER_ID> --json
xbot-cli web snapshot --browser <BROWSER_ID> --json
```

使用 inventory 的真实 ID。installed/registered/webSupported 各指不同状态，不能证明插件在线或目标账号已登录。`web --browser` 只选择 CLI 通道，不更改 Flow 内部的浏览器对象；以 Flow 实际参数和对象来源核对。

维护复现保留浏览器产品/版本、扩展连接、页面及 profile/账号的必要差异。团队如有“普通任务 Chrome、亚马逊紫鸟”规则，仅在该团队任务适用；不将样例默认写成平台要求。

目标浏览器缺失时判断是否为必要复现条件：必须精确复现的情况报告缺口，仍可静态分析；允许代表性环境时明确说明差异。不静默换浏览器，不复制登录态、迁移 profile 或切换账号。启用/安装扩展及系统改动按实际授权处理。
