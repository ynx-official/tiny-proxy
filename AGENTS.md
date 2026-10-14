# Tiny Proxy 项目约定

状态：Review  
最后更新：2026-10-14  
关联文档：[文档索引](docs/README.md)、[实施计划](docs/06-delivery/development-plan.md)

## 已确认边界

- 技术栈：GPUI + GPUI Kit + Tokio + Axum + SQLite。
- 纯桌面应用，供本人在不同设备和 Agent 中使用；访问范围为局域网及个人 VPN。
- 应用需要独立完成从零登录，不要求预装 Codex。
- 提供标准模型 API 的明确兼容子集；不实现 Cursor 证书接管或 Agent 运行循环。
- 2026-10-13 用户明确批准采用参考项目的 Codex 设备码登录、模型目录和专用 Responses 上游。依据 docs/03-architecture/adr/0003-codex-device-gateway.md。
- 2026-10-13 用户批准密钥可恢复、多账号、模型/测试/额度本地持久化及运行期间每小时只读额度查询，依据 docs/03-architecture/adr/0004-local-account-state.md。
- 用户于 2026-10-13 明确要求“开始实行”，当前进入开发，按既有方案先执行 P0，并推进不依赖真实账号验证的基础模块。正式发布仍需单独确认。
- 2026-10-14 用户要求 Windows EXE 启动不附带命令行窗口、右上角关闭默认收起到托盘；托盘或设置中的显式退出停止服务，托盘不可用保持窗口可操作。macOS 托盘不在本次修改范围，验收见 docs/04-quality/desktop-lifecycle-checklist.md。

## 工程原则

- 首版推荐 Windows x64、单进程、单 Cargo package 内部分模块；均以已批准设计为准。
- UI 不直接发模型请求、读取凭据或执行 SQL；网络和生命周期交给应用服务。
- 外部客户端不能管理账号、监听设置或读取凭据。工具由客户端执行。
- 参考目录 D:/study/cursor-byok-main 为只读参考，不修改。
- D:/study/ashell 为在线更新和发版的只读参考，不修改。
- 不自动提交、推送、创建 PR、切换分支或清理工作区。
- 测试使用 Rust 生态，不能机械套用 pytest/前端 Web 测试；命令执行遵循下方“验证与构建限制”。
- 认证、Token 轮换、流终止、工具调用、取消和密钥撤销必须先补行为测试；未获授权运行编译型测试时，保留测试代码并明确标记未运行。
- 未实测的事项标注未实测，不把文档核对当作运行验证。

## 验证与构建限制

2026-10-13 用户明确要求：默认只改代码，告知需要测试什么；只有用户明确要求时才重新构建或打包。本节优先于此前全局、项目或 Skill 的默认验证与产物流程。

- 默认交付代码、回归测试代码及必要文档修改，并说明测试场景、建议命令和未验证事项。
- 未明确要求时，不执行编译、构建、打包、安装或更新可执行产物；不自动运行 `cargo build`、`cargo rustc`、`cargo run` 或制作 exe、安装包、压缩包。`cargo test`、`cargo clippy` 等会触发编译的验证也不默认执行。
- 可以执行不触发编译的源码检查、差异审查和 `cargo fmt`，但不能把它们称为测试或构建通过。
- 用户要求“测试”只授权相关测试；要求“构建”只授权相关构建，均不自动扩展为打包、替换正在使用的程序或正式发布。
- 不为构建或测试擅自停止现有应用、替换运行中的 exe，或另建修复版产物目录。
- 完成说明区分代码已修改与运行已验证；未运行的验证由测试清单明确交给用户，不声称问题已实测修复。

## UI design system

- Skill: awesome-design-md
- Source: VoltAgent/awesome-design-md
- Style ID: hashicorp（用户于 2026-10-13 选择）。
- Style reference: C:/Users/Administrator/.codex/skills/awesome-design-md/references/design-md/hashicorp/DESIGN.md
- 候选与选择流程见 [桌面交互](docs/02-design/desktop-flows.md)。
- GPUI 使用设计规则，不引入 Web 前端。桌面适配规则见 docs/02-design/visual-system.md。
- 已有低保真原型：[全页面 ASCII 原型](docs/02-design/ascii-prototype.md)，按启动指令作为实施依据。已选择 hashicorp；实际页面完成情况以 docs/06-delivery/implementation-status.md 为准，不能把原型覆盖等同于实现覆盖。

## 文档与秘密信息

- 正式文档统一放在 docs/，由 docs/README.md 索引；使用小写 kebab-case。
- 文档记录状态、更新时间、关联文档；需求变化同步验收与计划。
- 不写入真实 Token、密钥、数据库密码、账号隐私或生产连接信息。
- 外部 API 密钥与上游 OAuth 凭据属于不同信任边界，不得混用。

## 在线更新与发版规范

- 2026-10-13 用户要求复刻 ashell 的在线更新、发版文档和 Actions，限定 Windows x64/macOS arm64，各有安装版与便携版，共四个应用产物。
- 同日批准 macOS 钥匙串加密密钥、AES-256-GCM 凭据和 HOME 路径，见 [ADR 0005](docs/03-architecture/adr/0005-online-update-release.md)。
- Cargo.toml 是版本唯一来源，Cargo.lock、界面、Windows ProductVersion、macOS 版本资源与 Git 标签一致。
- 根 CHANGELOG 为短摘要，docs/upgrade/README.md 为总览，每版 docs/upgrade/vX.Y.Z/README.md 为权威详情；保留此发布工具要求的目录结构。
- 详情必须有状态/日期、概述、实际变更、行为、数据兼容、升级、破坏性变更与已知问题、实际验证、真实祖先与变更依据、返回总览。禁止虚构发布日期、测试通过或标签。
- 每版同步总览、CHANGELOG、锁文件并校验 python scripts/release_notes.py --check-current；完整规范见 [发版流程](docs/06-delivery/release-process.md)。
- 正式标签需要 Approved、发布日期和单独发布授权，不自动提交或推标签；手动 Actions 只出 artifacts。
- 2026-10-13 用户明确要求“开始发第一个版本 v1.0.0”，授权本次首版准备、必要发布验证、提交/推送及新建版本标签发布；不授权停止现有应用或覆盖运行中的程序。后续版本仍按明确请求处理。
- 更新固定 ynx-official/tiny-proxy，只读检查，下载与安装需用户点击确认。创建流水线不等于已经运行、打包或发布成功。


- 2026-10-14 用户要求“正式发版 跳过测试”，授权本次 v1.1.0 版本准备、提交推送、构建打包和新标签发布。仅本版 Rust/Python 测试跳过，格式/Clippy/资料/产物校验保留；后续无 skip 标记版本恢复默认测试。
