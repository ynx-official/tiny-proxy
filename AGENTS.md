# Tiny Proxy 项目约定

状态：Review  
最后更新：2026-10-13  
关联文档：[文档索引](docs/README.md)、[实施计划](docs/06-delivery/development-plan.md)

## 已确认边界

- 技术栈：GPUI + GPUI Kit + Tokio + Axum + SQLite。
- 纯桌面应用，供本人在不同设备和 Agent 中使用；访问范围为局域网及个人 VPN。
- 应用需要独立完成从零登录，不要求预装 Codex。
- 提供标准模型 API 的明确兼容子集；不实现 Cursor 证书接管或 Agent 运行循环。
- 2026-10-13 用户明确批准采用参考项目的 Codex 设备码登录、模型目录和专用 Responses 上游。依据 docs/03-architecture/adr/0003-codex-device-gateway.md。
- 2026-10-13 用户批准密钥可恢复、多账号、模型/测试/额度本地持久化及运行期间每小时只读额度查询，依据 docs/03-architecture/adr/0004-local-account-state.md。
- 用户于 2026-10-13 明确要求“开始实行”，当前进入开发，按既有方案先执行 P0，并推进不依赖真实账号验证的基础模块。正式发布仍需单独确认。

## 工程原则

- 首版推荐 Windows x64、单进程、单 Cargo package 内部分模块；均以已批准设计为准。
- UI 不直接发模型请求、读取凭据或执行 SQL；网络和生命周期交给应用服务。
- 外部客户端不能管理账号、监听设置或读取凭据。工具由客户端执行。
- 参考目录 D:/study/cursor-byok-main 为只读参考，不修改。
- 不自动提交、推送、创建 PR、切换分支或清理工作区。
- 测试使用 Rust 生态；未来优先 cargo test、fmt、clippy、build，不能机械套用 pytest/前端 Web 测试。
- 认证、Token 轮换、流终止、工具调用、取消和密钥撤销必须行为测试先行。
- 未实测的事项标注未实测，不把文档核对当作运行验证。

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

