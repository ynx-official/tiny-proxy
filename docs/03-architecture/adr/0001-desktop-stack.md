# ADR 0001：原生桌面技术栈

状态：Review（核心技术栈已由用户确认；进程组织与平台范围待评审）  
最后更新：2026-10-13  
关联文档：[架构](../overview.md)、[决策表](../../01-requirements/assumptions.md)

## 决定与建议

已决定：GPUI + GPUI Kit + Tokio + Axum + SQLite。

建议：单进程、单 Cargo package、Windows x64 首发；GPUI 管理 UI 主循环，Tokio 运行后台，Axum 内嵌提供 API，SQLite 保存本地状态。

## 取舍

- GPUI/Kit 满足原生 Rust 桌面方向，但托盘、输入法、缩放、打包和版本兼容必须实机验证。
- Tokio 和 Axum 分别是异步运行时和 HTTP 层，应一起使用。
- SQLite 避免单用户桌面安装额外数据库服务。
- 单进程减少 IPC、独立服务升级和凭据跨进程所有权问题；进程退出时网关一同退出。

此前讨论的 Tauri/Web 前端和服务端+Web 不再属于当前交付范围。除非用户改变目标，不重新引入。

## 后果

GUI 与网络通过应用命令和状态桥接。核心协议模块保持无 UI 依赖，但不提前拆微服务或发布独立服务端。

P0 若 GPUI 集成不达标，先记录复现、版本和可修复性；更换框架属于重大架构变更，需要用户重新决定。

