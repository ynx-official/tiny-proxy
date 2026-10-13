# Trae 与个人设备接入

状态：Review  
最后更新：2026-10-13  
关联文档：[当前状态](../06-delivery/implementation-status.md)、[API 契约](../03-architecture/api-contracts.md)、[认证](../03-architecture/authentication.md)

## 桌面应用

1. 在账号连接页点击“使用 Codex 设备码登录”。系统浏览器打开 OpenAI Codex 设备页后，输入桌面应用显示的代码，由本人完成授权。
2. 在模型页读取真实模型目录，点击“测试文本”，确认收到 response.completed。
3. 在客户端接入页创建密钥；通过该行的查看或复制按钮取回完整值。旧版摘要密钥不可恢复，仍可使用原来保存的值。
4. 输入要监听的具体 IP:端口并启动。仅同机测试使用 127.0.0.1:8787；其他设备使用这台电脑实际 LAN/VPN 地址。不能给其他设备填写 127.0.0.1。
5. 复制运行中的 Base URL。之后打开应用会自动恢复上次成功监听地址；自动启动失败时可手动重试，停止按钮立即取消在途请求。详见 [代理自动启动](desktop-operations.md#代理自动启动)。

HTTP 用于本人可信 LAN/VPN。应用不修改 Windows 防火墙；需要跨设备时，按所选网络检查该程序/端口的入站规则。

## Trae

在当前版本的自定义模型设置中选择 OpenAI 兼容接入，填写：

| 设置 | 内容 |
|---|---|
| Base URL | 应用显示的 http://<本机实际IP>:<端口>/v1 |
| API Key | 本应用创建的 tp_ 前缀网关密钥 |
| Model ID | 模型页/GET /v1/models 返回的准确 ID |

若客户端界面要求完整请求地址，使用 http://<IP>:<端口>/v1/chat/completions。不要重复添加 /v1，也不要填写 OAuth Token。先测试文本，再执行一次实际工具调用与结果回传。

不同 Trae/第三方 Agent 版本可能自动携带 max_tokens 等字段。当前版本兼容 Chat 的 max_tokens/max_completion_tokens 和 Responses 的 max_output_tokens：正整数接收但不执行输出上限，null 等同未设置。非空限制通过 X-Tiny-Proxy-Ignored-Parameters 和 X-Tiny-Proxy-Warning 响应头提示未生效；客户端界面可能不显示这些头，不能将该设置当成实际长度或额度上限。temperature 等其他未支持字段的非空值仍返回 400；完整规则见 [API 契约](../03-architecture/api-contracts.md#输出长度参数兼容规则)。更新程序后需退出旧进程、重新打开 target/debug/tiny-proxy.exe，确认网关自动启动后的 Base URL，再让 Agent 重试。

## 诊断

`Invalid or unsupported field: index` 可由旧版本拒绝客户端保存的流式工具历史导致。兼容版接收 assistant.tool_calls[] 的非负整数/null index，同时处理匹配的工具结果 name、assistant 拒绝信息与已列明的可选空值；参数会显式规范化，工具 ID 和结果配对仍校验。规则见 [客户端历史与空值契约](../03-architecture/api-contracts.md#客户端历史与空值兼容规则)。最新修复构建及验证证据见 [开发验证记录](../06-delivery/implementation-status.md#客户端历史与空值兼容2026-10-13)，不要用下方历史修复构建代替最新构建。

`Invalid or unsupported field: reasoning_content` 可由旧版消息字段校验返回，尚未发往模型。修复版兼容 assistant 历史中的字符串/null；正文和工具历史继续转发，非空推理文本不回放给 Codex，通过兼容响应头提示。顶层、非 assistant 或错误类型仍拒绝，详见 [Chat 推理历史契约](../03-architecture/api-contracts.md#chat-推理历史兼容规则)。构建路径和验证结果见 [开发验证记录](../06-delivery/implementation-status.md#chat-reasoning_content-兼容2026-10-13)；切换新版后重新尝试原会话。

图片输入追加版本支持 user 消息中的 Base64 PNG/JPEG/WebP/GIF：Chat 使用 image_url 对象（含 url），Responses 使用 input_image 的 image_url 字符串。图文顺序及 detail 保留；HTTP(S) 图片网址、本地路径和工具结果图片仍不支持。整个请求（含 Base64 与历史）超过 2 MiB 返回 413，应缩小图片或减少历史后重试。图片格式与示例见 [API 契约](../03-architecture/api-contracts.md#图片输入兼容规则)；实际构建路径和测试证据见 [开发验证记录](../06-delivery/implementation-status.md)。

本次图片追加构建位于 artifacts/image-input-fix/tiny-proxy.exe；退出正在运行的旧应用后打开它，再手动启动网关。它包含已有长度参数兼容与本次图片转换，采用构建时的原数据目录逻辑。真实 Codex 图片和客户端重试仍待验证。

2026-10-13 长度参数修复构建时，原程序处于运行状态，标准输出 exe 无法覆盖。本次新版实际位于 artifacts/max-tokens-fix/tiny-proxy.exe；先退出旧版再启动该文件，数据目录保持默认位置。构建及验证边界见 [开发验证记录](../06-delivery/implementation-status.md#第三方-agent-输出长度参数兼容2026-10-13)。

| 现象 | 检查 |
|---|---|
| 连接拒绝/超时 | 应用是否运行、网关是否启动、IP 是否属于这台电脑、VPN 路由/防火墙 |
| 401 invalid_gateway_key | 使用完整有效的网关密钥；撤销后已删除，需新建或更换 |
| 403 key_revoked | 创建或使用另一个未撤销的密钥 |
| 503 account_not_ready | 在账号连接页完成 Codex 设备码登录 |
| 503 upstream_auth_required | 先停止网关，再重新进行设备码登录 |
| 404 model_not_found | 重新拉取当前 Codex 账号的模型目录 |
| 400 unsupported_or_invalid_parameter | 依据字段名检查客户端请求及支持子集 |
| 429 | 检查本机 4 并发限制或上游额度限制；不会自动重试生成 |
| 流中 error | 本次调用失败，不能把已收到的部分文本当作完整结果 |

设备码授权期间，应用应保持“等待结果”。轮询接口返回的 403/404 是该端点的等待状态，不应结束登录。若仍出现“申请设备码失败”或“换取登录凭据失败”，保留错误阶段和 HTTP 状态码用于排查；这与正常轮询不同。更新程序后需重新获取设备码，再由本人在浏览器输入并授权。

模型目录错误现区分：无效 JSON、缺少 models/data 数组、没有可用模型、超过 1 MiB。文本测试的非 SSE 响应单独提示，错误不输出上游正文或凭据。缺少 Content-Type 的有效 SSE 已兼容，但仍要求完整终态。

开发排障可运行 `cargo run --locked --example check_models`，使用本应用保存的会话读取目录，只打印模型数量及安全错误。显式加 `-- --test-text` 会对目录首个模型发送一次固定短提示并使用账号额度，打印完成状态与字符数；不自动重试生成，不打印 Token 或回复正文。

请求记录页只保存当前监听会话最近 100 条生成请求的模型、结果、耗时和请求 ID；不持久化，不含提示词或 Token。HTTP 200 后仍可能发生流内错误。

## 尚待真实验收

本文是当前实现的配置步骤。真实 Codex 授权与一次文本调用已验证，结果见 [验证记录](../06-delivery/implementation-status.md)。两轮工具回传、实际 Trae 版本、第二台设备仍需单独记录结果；本地模拟上游测试不替代这些证据。
