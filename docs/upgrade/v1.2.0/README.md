# Tiny Proxy v1.2.0

> 发布日期：2026-10-19
> 状态：Approved（已发布）
> 最后更新：2026-10-19
> 发布测试策略：skip
> 关联文档：[首页统计设计](../../02-design/overview-statistics.md)、[数据模型](../../03-architecture/data-model.md)、[桌面重构清单](../../04-quality/desktop-redesign-checklist.md)、[自定义端口清单](../../04-quality/custom-port-checklist.md)

## 版本概述

重构桌面导航与页面布局，提供真实的首页请求、Token、缓存命中率和延迟统计，并扩展请求记录诊断及监听端口设置。新增 SQLite 迁移保存缓存输入 Token 元数据。面向 Windows x64 与 macOS arm64 发布安装版和便携版。

## 行为与界面变化

- 首页按照选定时间范围汇总生成请求、输入/输出 Token、真实缓存输入及首个输出延迟，支持刷新和共用的网关启停操作；移除首页最近请求列表。
- 缓存命中率使用有效输入与缓存 Token 成对样本的加权比例。旧记录、缺失或异常值不被当作零命中；部分覆盖显示样本数与下限。
- 左侧导航按数据概览、模型配置和设置分组，六个既有页面身份保持不变；统一浅色/深色桌面布局、状态、品牌和按钮表现。
- 请求详情展示缓存输入 Token，并改善异常关联、时间线、耗时和复制内容。客户端接入可配置监听 IP 与端口；端口占用时给出修改入口。
- 概览和接入页共享真实服务启停状态，重复启动会返回当前监听地址，重复停止仍返回停止状态。

## 配置与数据兼容性

新增迁移 `0009_request_cache_tokens.sql`，为请求元数据添加可空缓存输入 Token。历史记录保持未知，不回填、不推断；新请求从上游终态用量读取有效值。此字段是输入 Token 子集，不重复计入总 Token。升级前建议退出应用并备份数据目录；缺少迁移0009的旧版不应打开已迁移数据库，回退时先恢复对应备份。其他依赖和凭据格式未变。

## 升级说明

从 v1.1.1 升级时使用同平台安装包或便携 ZIP。应用内更新仍需用户确认并按既有流程退出/重启；此发布不会安装到或替换正在运行的应用。首次启动会执行0009迁移。保留数据备份，确认桌面应用可启动且历史请求仍可读取后再移除备份。

## 破坏性变更与已知问题

- 迁移0009后，旧版本缺少对应字段，不能直接复用该数据库；回退需要恢复升级前备份。
- 缓存命中率只覆盖具有有效成对 Token 数据的请求。历史缓存数据未知时不会推算；没有有效样本显示为未知。
- 本版按用户要求跳过 Rust/Python 测试。无真实账号和客户端请求、全页面窗口交互、真实用户数据库迁移及应用内更新往返的实测证据时，不宣称这些行为已实测。
- 全目标 Clippy 会编译桌面二进制的测试目标，并在测试属性宏展开处触发 Rust 递归限制/编译器栈溢出；本版的跳过策略下，CI 与本机 Clippy 只检查生产 library 和 binary 目标，测试目标未编译。
- macOS 仍为 ad-hoc 签名且未公证；其他已知限制沿用 v1.1.1。

## 验证结果

本版 Rust 单元/集成测试和 Python 发布工具测试按用户要求跳过。`cargo +1.96.0 fmt --all -- --check` 通过；`cargo +1.96.0 clippy --lib --bin tiny-proxy --locked -- -D warnings` 通过。跳过测试时 Clippy 覆盖 library 与桌面 binary 生产目标；常规版本仍检查所有目标。`cargo build --locked --release --target x86_64-pc-windows-msvc --bin tiny-proxy` 成功。EXE ProductVersion/FileVersion 均为 1.2.0；Windows 便携 ZIP 与安装 EXE 已生成，安装器编译成功。`python scripts/release_notes.py --tag v1.2.0` 通过。

[标签 Native checks](https://github.com/ynx-official/tiny-proxy/actions/runs/39212212113) 的双平台格式、生产目标 Clippy 与 release 构建成功，测试步骤跳过。[正式 Release 工作流](https://github.com/ynx-official/tiny-proxy/actions/runs/39212212066) 的资料校验、Windows x64/macOS arm64 构建、四包打包、清单生成和上传成功；发布工具测试跳过。v1.2.0 于 2026-10-19 15:04:38（北京时间）公开发布，非 Draft/Prerelease。

已下载并逐一核对公开六个文件的名称、大小和 SHA-256 与 GitHub 资产摘要一致；清单为 v1.2.0、包含四个唯一应用包。Windows 便携 EXE 的 ProductVersion/FileVersion 和安装 EXE 的 ProductVersion 均为 1.2.0；macOS 便携包的 plist 版本为 1.2.0，包内可执行文件存在。对应实际文件及哈希见公开 [Release](https://github.com/ynx-official/tiny-proxy/releases/tag/v1.2.0)。测试及真实客户端、全页面窗口交互、真实用户数据库迁移和应用内更新往返未运行，未替换本机正在运行的程序。

## 变更依据

最近真实祖先正式版本为 v1.1.1，发布提交 `e461e66c89d8ad0007b1961171272a161ed7def3`。本版基于该标签后的本次工作区改动，增加统计、缓存元数据及桌面配置能力，属于兼容新功能，按 MINOR 递增至 1.2.0。

新标签为 v1.2.0，不复用或覆盖旧标签。[v1.1.1...v1.2.0](https://github.com/ynx-official/tiny-proxy/compare/v1.1.1...v1.2.0) 提供本版变更依据。

[返回版本总览](../README.md)
