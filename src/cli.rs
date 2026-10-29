//! Command grammar. Runtime work lives in commands and plan.
use clap::{Args, Parser, Subcommand};

/// Every wait is bounded; reject overflow before any backend action.
pub(crate) const MAX_TIMEOUT_SECS: u64 = 86_400;

#[derive(Parser)]
#[command(
    name = "xbot-cli",
    version,
    disable_help_subcommand = true,
    about = "影刀 RPA CLI:应用、流程、任务与工具",
    after_help = "示例:\n  xbot-cli status\n  xbot-cli app open <app-id>\n  xbot-cli flow verify <flow-id>\n  xbot-cli schema flow verify"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub group: Option<Group>,
    /// 结果格式:auto=终端文本/管道JSON,text=文本,json=紧凑JSON,pretty=缩进JSON
    #[arg(long, global = true, value_parser = ["auto", "text", "json", "pretty"], default_value = "auto")]
    pub format: String,
    /// 强制输出 JSON,等价于 --format json
    #[arg(long, global = true, conflicts_with = "format")]
    pub json: bool,
    /// 只预览副作用,不调用后端写入
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// 明确确认高风险操作或覆盖已有 Skill
    #[arg(long, global = true)]
    pub yes: bool,
    /// 断言当前 Studio 应用 ID;不匹配时不执行
    #[arg(long, global = true)]
    pub expect_app: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum Group {
    /// 创建、打开、保存、同步和发布应用
    #[command(subcommand)]
    App(AppSub),
    /// 读取、编辑、检查与运行 Studio 流程
    #[command(subcommand)]
    Flow(FlowSub),
    /// 已发布应用的运行任务,默认使用客户端通道
    #[command(subcommand)]
    Task(TaskSub),
    /// 网页操作:打开、快照、点击、填写与导航
    #[command(name = "web", visible_alias = "browser", subcommand)]
    Browser(BrowserSub),
    /// 实验性 Windows UIA 只读能力,独立于影刀原生选择器引擎
    #[command(subcommand)]
    Win32(Win32Sub),
    /// 底层工具的发现、schema 与调用
    #[command(subcommand)]
    Tool(ToolChannel),
    /// 影刀官方知识库
    #[command(subcommand)]
    Wiki(WikiSub),
    /// 影刀开放平台 Open API:授权、应用、机器人、调度与日志
    #[command(
        name = "oapi",
        alias = "platform",
        subcommand,
        long_about = "影刀开放平台 Open API,独立于本机客户端。\n先用 oapi auth <AccessKeyId> <AccessKeySecret> 保存凭据;普通命令失败不会自动切换到此通道。\nplatform 是兼容别名,新脚本请使用 oapi。"
    )]
    Oapi(crate::oapi_cli::OapiSub),
    /// 查看 CLI 自身的命令与参数 schema
    Schema {
        #[arg(num_args = 0..)]
        path: Vec<String>,
    },
    /// 按需阅读内置开发与排障指南
    Guide { topic: Option<String> },
    /// 安装 Agent Skill 和 references
    Install {
        #[arg(long)]
        skills: bool,
        /// 安装到 ~/.agents;缺省安装到当前工作区 .agents
        #[arg(long)]
        global: bool,
    },
    /// 查看客户端与当前应用状态
    Status,
    /// 深度检查 shell-cli / MCP / View 通道
    Doctor,
    /// 输出 CLI 版本
    Version,
    /// 升级 xbot-cli 到最新版本(由 npm 启动器执行)
    Update {
        /// 只检查是否有新版本,不执行升级
        #[arg(long)]
        check: bool,
    },
    /// 查看命令帮助,例如 help app 或 help flow code write
    Help {
        #[arg(num_args = 0..)]
        path: Vec<String>,
    },
    /// CodeFlow 源码操作
    #[command(subcommand)]
    #[command(hide = true)]
    Code(CodeSub),
    /// VisualFlow 块、变量和快照操作
    #[command(subcommand)]
    #[command(hide = true)]
    Visual(VisualSub),
    /// 原始 studio-mcp:tools/schema/call
    #[command(hide = true)]
    Mcp {
        action: String,
        #[arg(num_args = 0..=2)]
        args: Vec<String>,
        #[arg(long, default_value_t = 130, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
        /// tools 同时返回长描述;默认只返回工具名
        #[arg(long)]
        full: bool,
    },
    /// 兼容入口:仅静态诊断
    #[command(hide = true)]
    Diag,
    /// 兼容入口:仅客户端全局状态
    #[command(hide = true)]
    State,
}

#[derive(Subcommand)]
pub(crate) enum Win32Sub {
    /// 有界读取指定 HWND/PID 的 UIA 树;ref 仅用于本次证据,不支持动作
    Snapshot {
        /// 窗口句柄,十进制或 0x 十六进制;不自动选择活动窗口
        #[arg(long)]
        hwnd: String,
        /// 预期窗口所属进程 ID
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        pid: u32,
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..=5000))]
        max_nodes: u32,
        #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(0..=64))]
        max_depth: u32,
        /// 整个提供器子进程的时间上限,包含启动和导入
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=60))]
        timeout: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum AppSub {
    /// 新建应用(编辑模式,Studio 保持打开)
    #[command(alias = "new")]
    Create {
        name: String,
        #[arg(long)]
        desc: Option<String>,
    },
    /// 应用列表(默认精简字段)
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
        page_size: u32,
    },
    /// 打开应用到 Studio 编辑模式
    Open {
        app_id: String,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
        /// 先关闭当前会话;可能丢失未保存改动,需 --yes
        #[arg(long)]
        force: bool,
    },
    /// URL 协议只读查看
    View {
        app_id: String,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// Studio 当前应用完整信息
    Current,
    /// 保存、编译并静态检查
    Save,
    /// 上传云端并关闭 Studio
    Sync,
    /// 发布并轮询版本确认(需 --yes)
    Publish {
        app_id: String,
        #[arg(long)]
        log: Option<String>,
    },
    /// 版本历史
    Versions { app_id: String },
    /// 应用详情
    Detail { app_id: String },
    /// 运行已发布应用
    #[command(hide = true)]
    Run {
        app_id: String,
        #[arg(long = "input")]
        inputs: Vec<String>,
        #[arg(long)]
        inputs_json: Option<String>,
        #[arg(long)]
        no_wait: bool,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 停止控制台任务
    #[command(hide = true)]
    Stop {
        task_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// 控制台任务状态
    #[command(hide = true)]
    TaskStatus { task_id: String },
    /// 控制台任务日志
    #[command(hide = true)]
    TaskLogs {
        task_id: String,
        #[arg(long)]
        cursor: Option<i64>,
    },
    /// 移入回收站(需 --yes)
    Recycle { app_id: String },
}

#[derive(Subcommand)]
pub(crate) enum FlowSub {
    /// CodeFlow 源码读写
    #[command(subcommand)]
    Code(CodeSub),
    /// VisualFlow 块、字段和变量
    #[command(subcommand)]
    Visual(VisualSub),
    /// 仅静态检查,不会启动流程
    Check { flow_id: Option<String> },
    /// 继续等待已有运行,不会重新启动
    Wait {
        run_id: String,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 列出流程
    List,
    /// 新建 CodeFlow 或 VisualFlow
    #[command(alias = "new")]
    Create {
        name: String,
        #[arg(long, value_parser = ["code", "visual"], default_value = "code")]
        kind: String,
    },
    /// 按类型读取源码或 VisualFlow 详细块
    #[command(alias = "read")]
    Get { flow_id: Option<String> },
    /// 流程 In/Out 参数
    #[command(subcommand)]
    Params(FlowParamsSub),
    /// Studio 内运行并等待终态
    Run {
        flow_id: Option<String>,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 工作流:静态诊断 + 运行 + Out;失败附日志尾部
    #[command(
        after_help = "示例:\n  xbot-cli flow verify main --timeout 600\n  xbot-cli flow verify main --dry-run"
    )]
    Verify {
        flow_id: Option<String>,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 停止 Studio 内运行
    Stop,
    /// Studio 日志(默认最多 50 条)
    Logs {
        #[arg(long)]
        cursor: Option<i64>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        limit: Option<i64>,
    },
}

#[derive(Subcommand)]
pub(crate) enum FlowParamsSub {
    /// 查看流程参数
    List { flow_id: Option<String> },
    /// 添加、更新或删除流程参数
    Set {
        flow_id: Option<String>,
        /// operations 数组或完整 JSON,@文件 / '-' stdin
        #[arg(long)]
        operations: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum CodeSub {
    /// 读取 CodeFlow 源码
    Read { flow_id: Option<String> },
    /// 全量写 CodeFlow 源码
    Write {
        flow_id: Option<String>,
        #[arg(long)]
        file: String,
    },
    /// 唯一精确替换 CodeFlow 原文
    Edit {
        flow_id: Option<String>,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
        #[arg(long)]
        replace_all: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum VisualSub {
    /// 启用指定块
    Enable {
        #[arg(required = true, num_args = 1..)]
        block_ids: Vec<String>,
        #[arg(long = "flow")]
        flow_id: Option<String>,
    },
    /// 禁用指定块
    Disable {
        #[arg(required = true, num_args = 1..)]
        block_ids: Vec<String>,
        #[arg(long = "flow")]
        flow_id: Option<String>,
    },
    /// 分页读取 VisualFlow 块
    List {
        flow_id: Option<String>,
        #[arg(long)]
        detail: bool,
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..))]
        offset: Option<i64>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        limit: Option<i64>,
    },
    /// 插入或删除 VisualFlow 块
    Edit {
        flow_id: Option<String>,
        #[arg(long)]
        file: String,
    },
    /// 填充 VisualFlow 块字段
    Fill {
        flow_id: Option<String>,
        #[arg(long)]
        fills: String,
    },
    /// 禁用或启用块:[flow_id] comment|uncomment <block-id...>
    #[command(hide = true)]
    Toggle {
        #[arg(required = true, num_args = 2..)]
        args: Vec<String>,
    },
    /// 查看 VisualFlow 静态变量及作用域
    Variables {
        flow_id: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..))]
        offset: Option<i64>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        limit: Option<i64>,
    },
    /// 查看 VisualFlow 运行后的块级快照
    Snapshots {
        flow_id: Option<String>,
        #[arg(long)]
        block_id: Option<String>,
    },
}

#[derive(Args)]
pub(crate) struct WebOptions {
    /// 操作成功后不获取页面快照
    #[arg(long)]
    pub no_snapshot: bool,
}

#[derive(Args)]
pub(crate) struct WebElement {
    /// 最近页面快照中的元素编号，例如 e4
    pub refid: String,
    /// 为元素暂存指定的选择器名称；省略时生成唯一临时名称
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum BrowserSub {
    /// 打开 URL 并建立浏览器上下文;缺省打开 about:blank
    Open {
        url: Option<String>,
        #[arg(long, visible_alias = "browser")]
        mode: Option<String>,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 附着到当前活动浏览器页
    #[command(visible_alias = "attach")]
    Active {
        #[command(flatten)]
        options: WebOptions,
    },
    /// 获取带元素编号的页面快照，可保存到文件
    Snapshot {
        #[arg(long, visible_alias = "filename")]
        output: Option<String>,
    },
    /// 点击快照中的元素
    Click {
        #[command(flatten)]
        element: WebElement,
        #[arg(long, value_parser = ["left", "right", "middle"], default_value = "left")]
        button: String,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 双击元素
    Dblclick {
        #[command(flatten)]
        element: WebElement,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 清空并填写输入框
    Fill {
        #[command(flatten)]
        element: WebElement,
        text: String,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 向当前焦点输入文字
    Type {
        text: String,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 按键，例如 Enter、Tab 或 Control+A
    Press {
        key: String,
        #[arg(long = "ref")]
        refid: Option<String>,
        #[arg(long, requires = "refid")]
        name: Option<String>,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 将鼠标悬停在元素上
    Hover {
        #[command(flatten)]
        element: WebElement,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 勾选复选框
    Check {
        #[command(flatten)]
        element: WebElement,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 取消勾选复选框
    Uncheck {
        #[command(flatten)]
        element: WebElement,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 按 option value 选择下拉选项
    Select {
        #[command(flatten)]
        element: WebElement,
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 当前页面跳转到 URL
    Goto {
        url: String,
        #[command(flatten)]
        options: WebOptions,
    },
    /// 刷新当前页面
    Reload {
        #[command(flatten)]
        options: WebOptions,
    },
    /// 返回上一页
    #[command(visible_alias = "back")]
    GoBack {
        #[command(flatten)]
        options: WebOptions,
    },
    /// 前进到下一页
    #[command(visible_alias = "forward")]
    GoForward {
        #[command(flatten)]
        options: WebOptions,
    },
    /// 关闭当前受控页面
    Close,
    /// 使用 JSON 执行底层交互（高级兼容入口）
    #[command(hide = true)]
    Act { args_json: String },
    /// 页面内执行只读 Playwright 代码
    Eval { code: String },
    /// 列出浏览器工具(默认只返回名字)
    #[command(hide = true)]
    Tools {
        #[arg(long)]
        full: bool,
    },
    /// 查看浏览器工具 inputSchema
    #[command(hide = true)]
    Schema { tool: String },
    /// 原始 Chrome Bridge 兼容调用
    #[command(hide = true)]
    Call {
        tool: String,
        args_json: Option<String>,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum WikiSub {
    /// 列出官方知识库文档
    List,
    /// 搜索知识库
    Search { query: String },
    /// 读取一个或多个文档
    Read {
        #[arg(required = true, num_args = 1..)]
        doc_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum TaskSub {
    /// 运行已发布应用
    Run {
        app_id: String,
        #[arg(long = "input")]
        inputs: Vec<String>,
        #[arg(long)]
        inputs_json: Option<String>,
        #[arg(long)]
        no_wait: bool,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 停止控制台任务
    Stop {
        task_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// 查看任务状态
    Status { task_id: String },
    /// 查看任务日志
    Logs {
        task_id: String,
        #[arg(long)]
        cursor: Option<i64>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ToolChannel {
    /// Studio MCP 工具
    #[command(subcommand)]
    Studio(ToolSub),
    /// 浏览器 MCP 工具
    #[command(subcommand)]
    Browser(ToolSub),
}

#[derive(Subcommand)]
pub(crate) enum ToolSub {
    /// 列出工具;缺省只输出名字
    #[command(alias = "tools")]
    List {
        #[arg(long)]
        full: bool,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 获取工具的实时输入 schema
    Schema {
        tool: String,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
    /// 调用工具;JSON 支持 @file 或 - stdin
    Call {
        tool: String,
        args_json: Option<String>,
        #[arg(long, default_value_t = 130, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECS))]
        timeout: u64,
    },
}
