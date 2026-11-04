//! 进程内通道(inproc):官方接口给不了的能力由驻留在 ShadowBot.Shell.exe 内的小组件完成。
//!
//! 组件(payload/injectee + payload/shim)编译时嵌入本 exe;首次使用时按内容哈希释放到
//! `%LOCALAPPDATA%\guanzhu-xbot\payload-<hash>`,经原生 shim 注入影刀进程,
//! 在 127.0.0.1:5057 提供回环 HTTP 控制面(与旧 xbot-cli 的 5051 隔离)。
//! 普通功能继续走官方通道;只有官方接口做不到安全的操作(如不改表单地替换
//! CodeFlow 源码)才经过这里。

pub mod client;
pub mod embed;
pub mod inject;
