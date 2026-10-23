//! Public Open API grammar, independent of the local desktop client.
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub(crate) enum OapiSub {
    /// 登录、查看与验证开放平台凭据;也可直接 oapi auth <ak> <sk> 登录
    Auth(AuthArgs),
    /// RPA 应用资产与版本;records 查询运行数据
    #[command(subcommand)]
    App(AppSub),
    /// 机器人与机器人分组
    #[command(subcommand)]
    Robot(RobotSub),
    /// 原子调度:直接指定应用和机器人运行,使用 jobUuid
    #[command(subcommand)]
    Job(JobSub),
    /// 企业账号管理;写操作需 --yes
    #[command(subcommand)]
    User(UserSub),
    /// 工作队列与队列项;写操作需 --yes
    #[command(subcommand)]
    Queue(QueueSub),
    /// 常规调度任务(控制台预配置),使用 scheduleUuid
    #[command(subcommand)]
    Schedule(ScheduleSub),
    /// 常规任务执行记录,使用 taskUuid
    #[command(subcommand)]
    Task(TaskSub),
    /// 离线接口发现、参数文档和通用调用
    #[command(subcommand)]
    Api(ApiSub),
    /// 离线查看参考文档;省略 ID 列出所有文档
    Docs { id: Option<String> },
}

/// `oapi auth <ak> <sk>` 等价于 `oapi auth login --ak <ak> --sk <sk>`。
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct AuthArgs {
    /// AccessKey ID 和 AccessKey Secret;省略时进入子命令
    #[arg(num_args = 0..=2, value_names = ["ACCESS_KEY_ID", "ACCESS_KEY_SECRET"])]
    pub credentials: Vec<String>,
    #[command(subcommand)]
    pub command: Option<AuthSub>,
}

#[derive(Subcommand)]
pub(crate) enum AuthSub {
    /// 验证并保存 AccessKey 到本地配置;密钥只从参数读取,不回显
    Login {
        /// AccessKey ID,形如 xxxx@platform
        #[arg(long, visible_alias = "access-key-id")]
        ak: String,
        /// AccessKey Secret
        #[arg(long, visible_alias = "access-key-secret")]
        sk: String,
        /// 专有云 HTTPS origin;缺省为 https://api.yingdao.com
        #[arg(long)]
        base_url: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 离线显示地址、凭据来源与 token 缓存状态,隐藏密钥
    Status,
    /// 使用密钥获取 token 并验证响应;直接 token 仅报告已配置
    Check {
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 清除当前授权:删除本地保存的凭据与 token 缓存;不影响环境变量
    Logout,
}

#[derive(Subcommand)]
pub(crate) enum AppSub {
    /// 查询应用列表
    List {
        /// 应用名称模糊匹配
        #[arg(long)]
        search: Option<String>,
        /// 按所有者账号精确过滤
        #[arg(long)]
        owner: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 已发布版本详情及运行参数;触发执行前用它确认参数定义
    Detail {
        app_id: String,
        /// 改查当前开发版本明细
        #[arg(long)]
        dev: bool,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 应用版本历史
    Versions {
        app_id: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 子流程/指令集列表
    Flows {
        app_id: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 转移应用 owner 给其他账号(需 --yes)
    Transfer {
        app_id: String,
        /// 接收人账号,需精确匹配
        #[arg(long)]
        receive: String,
        #[command(flatten)]
        http: HttpArgs,
    },
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

#[derive(Subcommand)]
pub(crate) enum RobotSub {
    /// 机器人列表;调度前用它找到在线机器人
    List {
        /// 机器人名称模糊搜索
        #[arg(long)]
        search: Option<String>,
        /// 按机器人状态过滤
        #[arg(long)]
        status: Option<String>,
        /// 按分组 uuid 过滤
        #[arg(long)]
        group: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 机器人分组列表
    Groups {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 机器人详情,含当前是否在运行任务
    Detail {
        /// 机器人账号名称;与 --uuid 二选一
        #[arg(long, conflicts_with = "uuid")]
        robot: Option<String>,
        /// 机器人 uuid
        #[arg(long)]
        uuid: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum JobSub {
    /// 启动一次执行:指定应用和机器人;需 --yes,支持 --dry-run
    Start {
        /// 应用 uuid(控制台应用详情可复制)
        #[arg(long)]
        app: String,
        /// 机器人账号名称;与 --group 二选一
        #[arg(long, conflicts_with = "group")]
        robot: Option<String>,
        /// 机器人分组 uuid;配合 --scope 使用
        #[arg(long)]
        group: Option<String>,
        /// 分组执行范围:any 任意一台 / all 全部
        #[arg(long, requires = "group", value_parser = ["any", "all"], default_value = "any")]
        scope: String,
        /// 应用运行参数 JSON 数组,支持 @file 和 - stdin
        #[arg(long)]
        params: Option<String>,
        /// 运行超时(秒),60~950400
        #[arg(long, value_parser = clap::value_parser!(u64).range(60..=950400))]
        run_timeout: Option<u64>,
        /// 幂等标识,不超过36字符;重复请求返回原 jobUuid
        #[arg(long)]
        idempotent: Option<String>,
        /// 执行结束后的回调地址
        #[arg(long)]
        callback: Option<String>,
        /// 备注,不超过128字符
        #[arg(long)]
        summary: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 停止执行(需 --yes)
    Stop {
        job_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 重试终态为 error/skipped/cancel/stopped 的执行(需 --yes)
    Retry {
        job_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
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
pub(crate) enum UserSub {
    /// 企业用户列表
    List {
        /// 账号关键词
        #[arg(long)]
        search: Option<String>,
        /// 完整手机号
        #[arg(long)]
        phone: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 创建企业用户(需 --yes)
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        account: String,
        #[arg(long)]
        phone: String,
        #[arg(long)]
        email: Option<String>,
        /// basic 基础账号 / senior 高级账号
        #[arg(long, value_parser = ["basic", "senior"])]
        account_type: String,
        /// e_admin 管理员 / e_user 员工
        #[arg(long, value_parser = ["e_admin", "e_user"])]
        role: String,
        #[arg(long)]
        password: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 修改企业用户;只更新提供的字段(需 --yes)
    Modify {
        #[arg(long)]
        account: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        phone: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long, value_parser = ["e_admin", "e_user"])]
        role: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 删除企业用户,应用资产转移给接收账号(需 --yes)
    Delete {
        #[arg(long)]
        account: String,
        /// 接收被删账号应用的同企业账号
        #[arg(long)]
        receive: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 自助修改密码,需校验旧密码(需 --yes)
    ResetPassword {
        #[arg(long)]
        account: String,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
        #[command(flatten)]
        http: HttpArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum QueueSub {
    /// 队列列表
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(10..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 分页查询队列项
    Items {
        queue_uuid: String,
        /// 状态过滤,如 queued / processing / processed / exception
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(10..=100))]
        size: u32,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 入队(需 --yes)
    Enqueue {
        queue_uuid: String,
        #[arg(long)]
        name: String,
        /// 优先级:0 高 / 100 中 / 200 低
        #[arg(long, value_parser = ["0", "100", "200"])]
        priority: String,
        /// 业务信息,1~1000 字符
        #[arg(long)]
        biz_info: String,
        #[arg(long)]
        description: Option<String>,
        /// 生效时间,秒级时间戳
        #[arg(long)]
        effective_time: Option<i64>,
        /// 过期时间,秒级时间戳
        #[arg(long)]
        expire_time: Option<i64>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 出列一个队列项(需 --yes)
    Dequeue {
        queue_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 修改队列项状态为 processed/exception(需 --yes)
    Modify {
        item_uuid: String,
        #[arg(long, value_parser = ["processed", "exception"])]
        status: String,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 队列项重新排队(需 --yes)
    Reenqueue {
        item_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 队列项计数,可用于监控
    Count {
        queue_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 查询各状态可流转到的目标状态
    Statuses {
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 批量修改队列项状态(需 --yes)
    BatchStatus {
        queue_uuid: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
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
    /// 触发控制台预配置的常规任务(需 --yes)
    Start {
        schedule_uuid: String,
        /// 幂等标识,不超过36字符
        #[arg(long)]
        idempotent: Option<String>,
        /// 覆盖已配置机器人的参数 JSON 数组,支持 @file 和 - stdin
        #[arg(long)]
        params: Option<String>,
        #[command(flatten)]
        http: HttpArgs,
    },
    /// 停止整个任务及其未结束的 Job(需 --yes)
    Stop {
        task_uuid: String,
        #[command(flatten)]
        http: HttpArgs,
    },
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
