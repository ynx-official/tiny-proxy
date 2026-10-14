# Tiny Proxy Changelog

## [1.1.0] - 2026-10-14

- Windows 新增原生托盘：关闭窗口收起并保持网关，托盘菜单/设置提供显式退出，同目录再次启动恢复窗口。
- Windows EXE 使用 GUI 子系统，启动不附带控制台，启动错误显示可读对话框。
- 统一迁移 SQL 为 LF，兼容 v1.0.0 精确 CRLF 校验值；保留历史记录并细分数据库启动错误。
- 用户明确要求本版跳过 Rust/Python 测试；仍执行资料校验、构建打包与清单生成。

## [1.0.0] - 2026-10-13

- 首个正式版：独立 Codex 登录、多账号、模型目录、客户端密钥与 Chat/Responses API 子集。
- 请求元数据与异常诊断、主题切换、确认式在线更新及持久化检查策略。
- Windows x64 与 macOS arm64，各提供安装版和便携版；macOS 使用钥匙串保护凭据。

## [0.1.0] - 未发布

- 个人 Codex 桌面网关开发版，登录、模型、密钥与请求诊断验收见版本详情及实现状态。
- ashell 同流程在线更新、资料校验、清单与 Actions，仅 Windows x64/macOS arm64，各有安装包与便携包。
- macOS 钥匙串保护已补代码，双平台构建、打包、真实更新待验证。

[1.1.0]: docs/upgrade/v1.1.0/README.md
[1.0.0]: docs/upgrade/v1.0.0/README.md
[0.1.0]: docs/upgrade/v0.1.0/README.md
