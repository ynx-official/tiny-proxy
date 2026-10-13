# ADR 0005：在线更新与双平台发布

状态：Approved  
最后更新：2026-10-13  
关联文档：[发版流程](../../06-delivery/release-process.md)、[构建与打包](../../05-operations/build-and-package.md)、[测试清单](../../04-quality/online-update-checklist.md)

## 确认依据

2026-10-13 用户要求从只读参考目录 D:/study/ashell 完整移植在线更新、发版文档与 Actions；随后明确仅 Windows x64、macOS arm64，各有安装包与便携包，共四个应用产物。同日明确批准补齐 macOS 钥匙串保存加密密钥、SQLite 保存加密凭据及 HOME 数据目录。

## 决定

固定发布源为 ynx-official/tiny-proxy。复用 ashell 的 SemVer 比较、清单 → GitHub API → 页面降级、大小/SHA-256 验证、按安装方式匹配、进度、取消重试、安装确认与重启。缺清单、网络错误、403/429/5xx 可降级；内容、摘要、版本或仓库不合法直接失败。页面降级不能借用相邻文件摘要。

后台由现有 Tokio 服务管理，GPUI 只发命令、显示状态。默认启动检查，可设为 1–8760 小时间隔（默认 24）或关闭，默认提醒开启；偏好和成功检查时间存 SQLite。只查询最新正式版，自动检查不下载或安装。明确确认安装后停止网关、取消业务任务并收尾记录；安装助手准备成功才退出 UI。重启助手等待旧进程退出，避免单实例锁冲突。

Windows 安装器使用独立 AppId，便携版替换主 EXE；macOS bundle 为 TinyProxy.app，包标识 dev.tiny-proxy.app。安装版走 EXE/PKG，便携版走 ZIP。不移植 Linux、Intel macOS、RDP 多版本或 MCP helper。下载上限 1 GiB，解压上限 2 GiB、10,000 文件；拒绝越界、符号链接和重复路径。

Cargo.toml 是版本唯一来源，锁文件、标签、CHANGELOG、总览和详情一致；Windows ProductVersion 与 macOS 完整版本资源用于安装校验。手动 Actions 只上传 artifacts，标签才发布 Release，预发布不成为 latest。正式发布仍须用户单独确认。

## macOS 凭据边界

HOME 下保存 ~/.tiny-proxy；本机默认钥匙串保存独立随机 AES-256 密钥，SQLite 保存版本前缀、随机 96-bit nonce、AES-256-GCM 密文和认证标签。条目只新增、不覆盖已有密钥；解密时不创建替代密钥。钥匙串拒绝、缺失或损坏时失败，不降级明文。Windows 仍为 DPAPI。

仅复制数据库不能跨平台/跨用户恢复认证，应重新登录并创建客户端密钥。不得删除钥匙串条目排障，否则旧密文不可恢复。CI macOS 凭据测试使用临时钥匙串。

本机保护调用经 blocking worker 执行，避免钥匙串权限提示阻塞 Tokio 主编排线程。公共同步接口仍保留；账号刷新、凭据恢复与客户端 Key 读写的异步调用保持相同加密/解密语义，需复验既有轮换和撤销测试。

## 未验证事项

代码及配置移植不等于构建、签名或更新成功。Rust 编译、原生 UI、macOS 钥匙串与签名权限提示、两平台更新往返、最低 macOS 14 基线和 Actions 均待实测。SHA-256 匹配发布源不等同于发布者数字签名，信任边界为 HTTPS 和固定 GitHub 仓库。
