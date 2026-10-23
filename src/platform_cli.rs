//! Public platform API grammar, independent of the local desktop client.
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub(crate) enum PlatformSub {
    /// 离线接口发现、参数文档和通用调用
    #[command(subcommand)]
    Api(ApiSub),
    /// 查看配置或验证密钥;不会输出 token
    #[command(subcommand)]
    Auth(AuthSub),
    /// 调度应用运行与日志,使用 jobUuid
    #[command(subcommand)]
    Job(JobSub),
    /// 常规调度任务,使用 scheduleUuid
    #[command(subcommand)]
    Schedule(ScheduleSub),
    /// 调度任务执行记录,使用 taskUuid
    #[command(subcommand)]
    Task(TaskSub),
    /// 应用运行数据
    #[command(subcommand)]
    App(AppSub),
    /// 离线查看参考文档;省略 ID 列出所有文档
    Docs { id: Option<String> },
}

#[derive(Subcommand)]
pub(crate) enum ApiSub {
    /// 列出可调用接口,无需登录
    List {
        #[arg(long)]
        search: Option<String>,
    },
    /// 接口请求、响应字段和来源;支持名称、文档ID或 /oapi 路径
    Schema { endpoint: String },
    /// 调用已登记接口;写操作需 --yes,支持 --dry-run
    Call {
        endpoint: String,
        /// JSON 请求体,支持 @file 和 - stdin
        #[arg(long)]
        body: Option<String>,
        /// URL 查询参数 JSON 对象
        #[arg(long)]
        query: Option<String>,
        /// 路径占位符 JSON 对象,如 {"queueUuid":"..."}
        #[arg(long)]
        path: Option<String>,
        /// file.upload 的本地文件,以 multipart/form-data 上传
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum AuthSub {
    /// 离线显示地址与配置是否存在,隐藏密钥
    Status,
    /// 使用密钥获取 token 并验证响应;直接 token 仅报告已配置
    Check {
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Args)]
pub(crate) struct HttpArgs {
    /// HTTP 超时(秒);日志异步模式为整个查询的时间上限
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=crate::cli::MAX_TIMEOUT_SECS))]
    pub timeout: u64,
}

#[derive(Args)]
pub(crate) struct CursorArgs {
    #[arg(long)]
    pub cursor: Option<i64>,
    #[arg(long, default_value = "next", value_parser = ["next", "pre"])]
    pub direction: String,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub size: u32,
    #[arg(long = "status", value_delimiter = ',')]
    pub statuses: Vec<String>,
    /// yyyy-MM-dd HH:mm:ss,使用平台时间
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    #[command(flatten)]
    pub http: HttpArgs,
}

#[derive(Subcommand)]
pub(crate) enum JobSub {
    /// 查询调度应用运行列表,返回原始游标
    List {
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        robot: Option<String>,
        #[command(flatten)]
        page: CursorArgs,
    },
    /// 查询运行状态、错误备注与截屏地址
    Status {
        job_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 查询一页日志;--async 采用通知+轮询,超时保留 requestId
    Logs {
        job_uuid: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
        size: u32,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value = "desc", value_parser = ["asc", "desc"])]
        order: String,
        #[arg(long = "async")]
        asynchronous: bool,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum ScheduleSub {
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=500))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    Detail {
        schedule_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 查询指定任务的执行记录;再用 task status 获取 jobUuid
    Runs {
        schedule_uuid: String,
        #[command(flatten)]
        page: CursorArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum TaskSub {
    /// 每条常规任务最新一次执行记录
    Latest {
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 查询任务执行状态及各应用 jobUuid
    Status {
        task_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 查询一次任务执行在指定机器人上的应用运行明细
    Jobs {
        task_uuid: String,
        #[arg(long)]
        robot: String,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum AppSub {
    /// 按时间分页查询应用运行记录;时间跨度不超过7天
    Records {
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long, value_delimiter = ',')]
        mode: Vec<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
}
