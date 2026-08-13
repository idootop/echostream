//! 插件：控制面的生命周期扩展与处理器打包

use crate::client::ClientBuilder;
use crate::server::ServerBuilder;

/// 服务端插件：打包常用处理器与生命周期钩子
///
/// 插件在 `install` 中通过 Builder 注册自己的 RPC / Event / Stream
/// 处理器和连接钩子，对外只暴露一个可复用的集合。
pub trait ServerPlugin: Send + Sync + 'static {
    /// 插件名称
    fn name(&self) -> &str;

    /// 安装插件（修改 ServerBuilder）
    fn install(self: Box<Self>, builder: ServerBuilder) -> ServerBuilder;
}

/// 客户端插件：打包客户端处理器与重连/认证等基础能力
pub trait ClientPlugin: Send + Sync + 'static {
    /// 插件名称
    fn name(&self) -> &str;

    /// 安装插件（修改 ClientBuilder）
    fn install(self: Box<Self>, builder: ClientBuilder) -> ClientBuilder;
}
