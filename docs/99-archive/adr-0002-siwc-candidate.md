# ADR 0002：认证和上游模型路线

状态：Archived  
最后更新：2026-10-13  
关联文档：[认证](../03-architecture/authentication.md)、[参考分析](../03-architecture/reference-analysis.md)、[API](../03-architecture/api-contracts.md)

## 问题

用户要求从零登录并对其他 Agent 提供 API。参考项目的 Codex 凭据与官方应用自身 SIWC 凭据，不是同一种授权；不能只替换端点或复制一个 Bearer 头。

## 候选

| 路线 | 优点 | 约束 | 建议 |
|---|---|---|---|
| 自身 SIWC 注册 + 公共 Responses | 有当前官方接入文档；应用有自己的注册与权限；不依赖 Codex | 账号资格、模型范围、字段和 tools 存在限制；实际可用待验证 | 首版候选 |
| 参考 codex-auth 的设备授权 + Codex 专用端点 | 更接近参考项目已有链路 | 固定客户端身份、专用端点及变化风险；本地源码不是第三方接入保证；不能声称已通过账户实测 | 记录原理，不默认实现 |
| 官方 API Key + 标准模型 API | 授权和计费边界明确 | 与本次从零 ChatGPT 登录复用个人能力的目标不同 | 不作为静默 fallback |

官方对当前 SIWC 推理路线指定公共 Responses，要求按已授权账号的模型目录选择模型；不能把该 Token 发到 ChatGPT backend-api。[模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)

## 推荐决定

先评审并验证自身 SIWC 路线。使用应用真实名称，独立管理注册和凭据。P0 将目标模型和工具调用作为硬门槛；若不满足用户实际需求，报告证据并重新评审路线。

参考代码用于理解授权状态、模型同步、事件转换和外部 API 分层，不照搬 Codex 客户端身份或把它包装成官方通用 SDK。

## 约束

- 不混用注册、刷新端点、模型目录与推理端点。
- 不自动读取/接管现有 Codex 登录态。
- 不以代理实现改变上游许可或能力限制。
- 首版只落地一条获确认的上游路线，不先写通用插件平台。
- “登录成功”“有模型列表”“指定模型实际完成”“工具往返完成”分别验收。

账号资格及本人的跨设备网关拓扑仍需实际接入验证；文档存在不等于已获账号访问权限。



本历史版本已由 [Codex 路线决定](../03-architecture/adr/0003-codex-device-gateway.md) 与当前认证/实现文档替代。
