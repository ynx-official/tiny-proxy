# Codex 登录与凭据生命周期

状态：Approved  
最后更新：2026-10-13  
关联文档：[路线决定](adr/0003-codex-device-gateway.md)、[数据模型](data-model.md)

## 用户流程

账号连接 → 获取设备码 → 打开系统浏览器 → 本人在 OpenAI 的 Codex 设备页输入代码并授权 → 应用轮询并交换凭据 → DPAPI 保存 → 模型页验证。取消、超时、拒绝和网络错误均停止本次尝试，允许重新获取代码。

新登录期间原有 Codex 会话保持不变；网关运行时不允许更换账号。登录入口不读取本机 Codex 缓存。

## 固定协议

| 用途 | 地址/参数 |
|---|---|
| 客户端身份 | codex-auth/oauth.ts 的固定 public client ID；不是密钥 |
| 申请设备码 | POST https://auth.openai.com/api/accounts/deviceauth/usercode |
| 用户授权页 | https://auth.openai.com/codex/device |
| 轮询 | POST https://auth.openai.com/api/accounts/deviceauth/token |
| 交换/刷新 Token | POST https://auth.openai.com/oauth/token |
| code exchange redirect_uri | https://auth.openai.com/deviceauth/callback |
| 模型 | GET https://chatgpt.com/backend-api/codex/models?client_version=1.0.0 |
| 推理 | POST https://chatgpt.com/backend-api/codex/responses |

服务固定上游地址，禁重定向；客户端无法设置上游 URL、Cookie 或任意转发头。模型请求使用上游 OAuth Bearer、ChatGPT-Account-Id 和参考的 originator。缓存键提供时同步 session-id、thread-id、x-client-request-id。

模型目录按参考项目接受 models/data/根数组，兼容模型 ID、名称、推理档位和默认模型的 snake_case/camelCase 字段。过滤隐藏或不支持 API 的条目，保持去重和 ID 校验。目录响应限制 1 MiB；无效 JSON、缺少列表、没有可用模型和超限分别提示，不再统一报协议错误。

设备轮询的 403/404 表示等待，兼容 deviceauth_authorization_pending、其他 pending 表达、空响应及非 JSON 响应；明确 access_denied/expired 仍优先终止。slow_down 增加间隔，5xx 明确失败。成功响应仍严格校验 JSON 和凭据。该等待规则只适用于设备轮询，申请设备码或交换凭据的 403 为失败，错误提示包含阶段。

轮询最多 15 分钟，每次 HTTP 最多 30 秒，响应体最多 1 MiB；取消和到期结束等待，不覆盖原连接。错误文本不包含上游响应体或凭据。2026-10-13 根据实际未授权轮询的 403/deviceauth_authorization_pending 修正误判，并增加回归测试，验证记录见 [当前交付](../06-delivery/implementation-status.md)。

## 保存与续期

Codex Token bundle 保存 access_token、refresh_token、账号路由 ID、subject 和有效期；Windows 用户级 DPAPI 加密后存入 codex_session。访问 Token 的 JWT 字段来自固定 HTTPS token endpoint，仅解析为路由、刷新绑定和到期元数据，不声称进行了本地 JWT 签名身份验证；上游 API 执行实际授权检查。

请求前距过期不足 60 秒时刷新。后台会话锁使并发请求共用一次刷新，最新凭据和递增 generation 原子保存。请求取消不会中断已开始的刷新保存；进程终止或异常退出仍可能需要重新登录。刷新返回不同 subject/账号 ID 时拒绝替换。刷新失败不切换账号、不重试生成 POST。

无 refresh_token 的有效会话可以调用至过期，届时提示重新登录。当前不提供远端撤销和多 Codex 账号切换界面。

## 对外凭据

网关客户端密钥与上游 OAuth 分开。256 bit 随机密钥保存摘要用于鉴权，按 [ADR 0004](adr/0004-local-account-state.md) 另存 DPAPI 密文用于本人按需查看/复制。每次 HTTP 请求查验密钥。2026-10-13 用户要求撤销后删除、不再展示，依据 [B20](../01-requirements/business-rules.md) 更新：删除记录提交后，旧值按无效密钥拒绝，已登记的在途请求取消；删除失败保留记录并报告错误。


2026-10-13 多账号保存、切换与旧会话导入按 [ADR 0004](adr/0004-local-account-state.md) 执行。
