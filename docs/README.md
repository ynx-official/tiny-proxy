# Tiny Proxy 方案与开发计划

状态：Review  
最后更新：2026-10-13  
关联文档：[项目章程](00-project/project-charter.md)、[开发计划](06-delivery/development-plan.md)

## 阅读顺序

1. [PRD：要交付什么](01-requirements/prd.md)
   - [全页面 ASCII 原型：界面如何操作](02-design/ascii-prototype.md)
2. [整体架构：如何组成](03-architecture/overview.md)
3. [认证：从零登录和续期](03-architecture/authentication.md)
4. [协议：标准 API 与流式转换](03-architecture/api-contracts.md)、[流式处理](03-architecture/streaming-protocol.md)
5. [开发计划：如何逐步验收](06-delivery/development-plan.md)

## 当前结论

技术栈、个人使用、纯桌面、局域网及个人 VPN 范围已经确认。推荐单进程模块化应用，内嵌 Axum 网关，由 Tokio 承担后台任务。

用户已明确批准采用参考项目的 Codex 设备码认证与 Codex 专用模型端点。详见 [认证 ADR 0003](03-architecture/adr/0003-codex-device-gateway.md)。

用户于 2026-10-13 要求“开始实行”，项目已进入 P0 开发验证；同日选择 hashicorp 风格。设计文档描述目标能力，不能据此推断全部已经实现。当前代码、自动检查与真实验证的区别集中在 [开发验证记录](06-delivery/implementation-status.md)。Codex 真实账号、模型目录及一次完整文本调用已验证；真实工具往返、Trae 和跨设备验收仍须实测。

## 文档总表

| 类别 | 权威文档 |
|---|---|
| 项目 | [项目章程](00-project/project-charter.md) |
| 需求 | [问题定义](01-requirements/problem-framing.md)、[PRD](01-requirements/prd.md)、[故事地图](01-requirements/story-map.md) |
| 规则与不确定性 | [业务规则](01-requirements/business-rules.md)、[事实、假设及决策](01-requirements/assumptions.md) |
| 验收 | [MVP 验收场景](01-requirements/acceptance/mvp.md) |
| 桌面交互 | [流程、页面和状态](02-design/desktop-flows.md) |
| 视觉系统 | [hashicorp 桌面适配](02-design/visual-system.md) |
| 软件图标 | [中继之门：设计、素材与验证](02-design/application-icon.md) |
| 全页面原型 | [原型入口与覆盖表](02-design/ascii-prototype.md)、[首次使用](02-design/prototypes/onboarding.md)、[主页面与详情](02-design/prototypes/main-pages.md)、[设置与系统](02-design/prototypes/settings-and-system.md)、[弹窗与状态](02-design/prototypes/dialogs-and-states.md) |
| 架构 | [概览](03-architecture/overview.md)、[认证](03-architecture/authentication.md)、[数据模型](03-architecture/data-model.md) |
| 协议 | [API 契约](03-architecture/api-contracts.md)、[流式协议](03-architecture/streaming-protocol.md) |
| 安全 | [权限矩阵与威胁模型](03-architecture/threat-model.md) |
| 参考依据 | [cursor-byok 分析与来源](03-architecture/reference-analysis.md) |
| 决策记录 | [桌面技术栈](03-architecture/adr/0001-desktop-stack.md)、[Codex 路线](03-architecture/adr/0003-codex-device-gateway.md)、[本地账号状态与额度](03-architecture/adr/0004-local-account-state.md)、[历史认证候选](99-archive/adr-0002-siwc-candidate.md) |
| 质量 | [验证计划](04-quality/verification-plan.md) |
| 使用与维护 | [桌面运行、接入和恢复](05-operations/desktop-operations.md) |
| 归档 | [早期认证方案](99-archive/siwc-authentication.md)、[早期验证记录](99-archive/siwc-probe-implementation.md) |
| 交付 | [分阶段开发计划](06-delivery/development-plan.md)、[开发验证记录](06-delivery/implementation-status.md) |

## 评审方式

[决策表](01-requirements/assumptions.md)集中维护待确认事项，其他文档引用。确认技术栈不等于确认所有认证、隐私和产品默认值。后续修改需说明原因及受影响的验收场景。


当前交付：[分阶段开发计划](06-delivery/development-plan.md)、[开发验证记录](06-delivery/implementation-status.md)、[Trae 接入操作](05-operations/trae-connection.md)。

2026-10-13 第三方 Agent 长度参数兼容规则与批准依据见 [API 契约](03-architecture/api-contracts.md#输出长度参数兼容规则)，回归证据及可运行修复版路径见 [开发验证记录](06-delivery/implementation-status.md#第三方-agent-输出长度参数兼容2026-10-13)。

2026-10-13 用户批准按参考项目补齐 Base64 图片输入，范围见 [图片输入契约](03-architecture/api-contracts.md#图片输入兼容规则)，对应验收 A17c/A17d 与 P3/P4 图片切片。验证结果集中维护在 [开发验证记录](06-delivery/implementation-status.md)。

2026-10-13 历史数据恢复与不同启动来源的目录统一见 [数据目录与恢复](05-operations/desktop-operations.md#数据目录统一与历史恢复)，覆盖 A40/A41；默认数据目录现为 `%USERPROFILE%/.tiny-proxy`。

2026-10-13 用户要求打开应用默认启动代理，规则及操作见 [代理自动启动](05-operations/desktop-operations.md#代理自动启动)，对应 B15、A42—A44，验证证据见 [开发验证记录](06-delivery/implementation-status.md)。

2026-10-13 reasoning_content 报错处理见 [Chat 推理历史兼容规则](03-architecture/api-contracts.md#chat-推理历史兼容规则)，对应 A17e/A17f；构建与验证证据见 [开发验证记录](06-delivery/implementation-status.md#chat-reasoning_content-兼容2026-10-13)。

2026-10-13 index 与同类问题成组修复，对照 codex-proxy-dev 后采用 [客户端历史与空值规范化](03-architecture/api-contracts.md#客户端历史与空值兼容规则)，对应 A17g/A17h；[参考方式比较](03-architecture/reference-analysis.md#codex-proxy-dev-兼容方式核对2026-10-13)和 [构建验证](06-delivery/implementation-status.md#客户端历史与空值兼容2026-10-13)分别记录源码证据与运行证据。

2026-10-13 用户要求撤销 Key 后删除、不再展示，权威规则见 [B20](01-requirements/business-rules.md)，验收 A45—A47；[0005 历史清理](03-architecture/data-model.md#0005-已撤销密钥清理)及 [验证记录](06-delivery/implementation-status.md#撤销密钥即删除2026-10-13)说明升级与运行边界。
