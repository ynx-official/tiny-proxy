# Tiny Proxy v0.1.0

> 发布日期：未发布
> 状态：Draft
> 最后更新：2026-10-13
> 关联文档：[实现状态](../../06-delivery/implementation-status.md)、[更新验收](../../04-quality/online-update-checklist.md)、[发版流程](../../06-delivery/release-process.md)

## 版本概述

个人原生桌面 Codex 网关开发版，提供设备码登录、模型目录、独立客户端密钥及明确兼容子集的 Chat/Responses API。本轮移植 ashell 的在线更新和发布链路，配置仅 Windows x64、macOS arm64，各有安装包与便携包；尚未实际发版或验证安装包。

## 更新与发布能力

- 设置中检查正式版、显示说明、下载进度、取消重试，校验后确认安装重启。
- 启动/定时/关闭自动检查，提醒开关，偏好和成功时间持久化。
- 清单优先、API/页面降级，匹配平台与安装方式，校验大小/SHA-256。
- CHANGELOG、总览、详情、发布资料校验和说明提取；手动 Actions 只上传 artifacts，标签流程生成四包+清单+说明。
- macOS HOME 路径及钥匙串/AES-256-GCM 保护已加源码，原生验证未运行。

## 行为与界面变化

设置页增加在线更新，版本来自 Cargo.toml。自动检查不下载安装；明确确认安装才停止网关与在途请求。原有主题、账号、模型、Key 和请求记录流程保留；并行功能的完整状态见实现状态。

## 配置与数据兼容性

新增更新偏好表，不移动 Windows 既有数据。Windows 为用户级 DPAPI，macOS 为 ~/.tiny-proxy 与钥匙串加密密钥，凭据不跨平台互通。仅复制数据库不能跨设备恢复认证。迁移与旧数据保留须实测，回退前备份数据库。

## 升级说明

无正式前置版本。未来升级前备份数据、退出旧进程、选择正确平台包。应用内需下载、校验后确认安装。Windows 默认当前用户目录，macOS PKG 至 /Applications 且可能请求管理员权限；macOS ZIP 保留完整 TinyProxy.app，不只复制 binary。

## 破坏性变更与已知问题

- 双平台真实更新未验证，当前旧 EXE 不会得到源码改动。
- 未配置 Developer ID、公证或 Windows 代码签名，首启可能需系统批准。
- 无 Release 时检查显示尚未发布，本轮无真实发布附件联调。
- 页面降级受 GitHub HTML 结构影响，失败可手动打开版本页。
- 真实工具往返、Trae、跨设备等既有未验收事项见实现状态。

## 验证结果

未运行 Rust/Python 回归测试、Clippy、编译、原生应用、更新安装、构建、打包或 Actions，没有产物验证通过的结论。回归代码和手工场景见验收清单。

- 使用本机已安装 rustfmt 1.96.1 对本轮 Rust 源码格式化并执行 --check，通过；不是编译或测试。
- Python AST、PowerShell Parser、bash -n 和 plist 语法检查通过；新文档链接、迁移序号和依赖锁文件一致性检查通过。
- python scripts/release_notes.py --check-current：v0.1.0 资料校验通过，状态仍 Draft、日期仍未发布。
- 本轮涉及的代码与配置按 CRLF 规则执行 git diff --check，通过；工作区原有与并行修改保留。
- 本地未提供 YAML 解析器，工作流做了源码审查，语法与真实 runner 行为仍待 Actions 验证。
- 工具链查询触发 rustup 自动下载尝试后已取消，后续直接使用已安装工具检查；未主动运行编译、安装可执行产物或停止现有程序。

## 变更依据

首个代码版本，无前置标签与 compare 区间。参考 D:/study/ashell 的 updater、平台打包、release_notes.py、update_manifest.py 和 Actions；参考目录未改。目标 v0.1.0 未创建、未推送，拟发布链接不能视为已发布证据。

[返回版本总览](../README.md)
