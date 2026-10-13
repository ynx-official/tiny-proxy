# ADR 0003：Codex 设备码认证与模型网关

状态：Approved  
最后更新：2026-10-13  
关联文档：[认证](../authentication.md)、[API](../api-contracts.md)、[历史候选](../../99-archive/adr-0002-siwc-candidate.md)

## 决策与来源

用户明确要求“按参考项目的 Codex 设备码登录和模型调用方式来改，桌面技术栈不变，继续做成通过 IP:端口给 Trae 调用的模型网关，直接开始”。此指令批准替换先前 SIWC 候选路线。

保留 GPUI + GPUI Kit + Tokio + Axum + SQLite。认证、模型目录与推理采用本地 cursor-byok-main/codex-auth 的协议：固定 Codex public client ID、设备码轮询、OAuth code exchange、Codex 专用模型与 Responses 端点。首版独立实现，无 Codex 安装、登录缓存导入、证书接管或 Agent 循环。

当前仅保留一个活动 Codex 会话。原有 SIWC 注册与 DPAPI 密文保留在原表，新的 codex_session 表只存 Codex 密文和代次；两个凭据域不互相读取。

## 网关切片

- GET /v1/models；POST /v1/chat/completions；POST /v1/responses。
- 请求均需本应用生成的 Bearer 密钥；不暴露任何网络管理接口。
- 文本、函数定义、函数调用及工具结果完整历史；工具在 Trae 等客户端执行。
- 上游始终 stream=true/store=false；对外支持流式和聚合 JSON。
- 原决策为用户在桌面输入具体本机/LAN/VPN IP:端口并手动启动，默认 127.0.0.1:8787。2026-10-13 用户明确要求应用启动时默认启动代理，自动监听部分改按 [B15](../../01-requirements/business-rules.md) 执行；不修改防火墙、不允许 0.0.0.0 或公网地址的边界不变。
- 请求元数据最多 100 条，仅在当前监听会话内存中保存。原计划的持久化日志及删除策略继续待确认。

## 验收与边界

已批准路线不等于已完成真实账号验收。设备授权、真实模型 completed、工具两轮往返、具体 Trae 版本、第二台设备需分别记录证据。

固定客户端身份与 backend-api 契约来自参考源码，不描述为 OpenAI 对第三方通用 API 的兼容承诺。未知错误明确暴露类别，禁止回退到 SIWC、API Key 或另一个账号。

## 实施顺序

1. 设备码状态与错误分类测试，Codex 独立存储迁移。
2. 自动续期、并发串行化、取消后继续提交轮换凭据。
3. 模型目录、Chat/Responses 请求转换与流终态。
4. Axum 鉴权、取消、撤销、限制与桌面接入。
5. 本地模拟上游集成测试、原生页面检查、真实账号与 Trae 联调。
