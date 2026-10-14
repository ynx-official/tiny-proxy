# 模型 API 严格校验契约快照

状态：Superseded\
最后更新：2026-10-14

关联文档：[流式转换](../03-architecture/streaming-protocol.md)、[规则](../01-requirements/business-rules.md)、[验收](../01-requirements/acceptance/mvp.md)

本快照于 2026-10-14 被用户明确要求的“默认信任下游字段”方案替代，权威后继为 [当前 API 契约](../03-architecture/api-contracts.md)。仅用于追溯此前严格校验规则，不表示快照中的新增源码已经运行验证或发布。

## 地址与鉴权

对外 Base URL 形如 http://<本机选定的局域网或 VPN IPv4>:8787/v1。示例是配置模板，不是已经启动的服务。

所有 /v1 路由都要求 Authorization: Bearer <GATEWAY_KEY>。网关密钥只在本应用鉴权；发往上游时用本应用持有的 OAuth Token 替换，不透传客户端 Authorization、Cookie、任意上游 URL 或任意自定义请求头。

UI 管理通过进程内命令完成，网络上没有 /admin、登录、账号导出、文件访问或配置修改接口。首版不启用浏览器 CORS，也不使用 Cookie 登录。

## 路由

| 方法和路径 | 成功响应 | 边界 |
|---|---|---|
| GET /v1/models | OpenAI 风格 object=list、data 中的 model 对象 | 仅活动账号当前允许且网关支持的模型；原始上游目录需转换 |
| POST /v1/chat/completions | chat.completion JSON 或 chat.completion.chunk SSE | n=1、文本、用户 Base64 图片、function tools |
| POST /v1/responses | Response JSON 或 Responses 事件 SSE | 明确的无服务器会话子集，保留原生语义 |
| 其他路由 | 404；已知路径错误方法 405 | 不假装支持整个 OpenAI 平台 |

不提供按 response_id 查询、删除或恢复生成的 HTTP 路由。

模型 ID 初版直接使用上游可见 ID，不用硬编码的虚构模型或自动别名。上游缺少模型创建时间时，目录的 created=0 仅作为“未知”的协议占位，UI 显示未知；不把抓取时间冒充模型创建时间。owned_by 使用明确的来源标识，不推断账号权限。客户端选择必须以实际返回列表为准，账号切换后清除旧目录的活动可见性。

## Chat Completions 请求

| 字段/内容 | 首版处理 |
|---|---|
| model、messages | 必填；model 必须在当前可用目录；messages 为完整历史 |
| stream | 默认 false；true 返回 SSE；false 由网关聚合上游流 |
| user/assistant 文本 | 接受字符串、兼容文本块数组；保留消息顺序 |
| user 图片 | 接受 image_url 块的 Base64 data URL，转换为 input_image；保留图文顺序及 detail，详见下述规则 |
| 前置 system 或 developer 文本 | 按原顺序转入上游 instructions；同一请求混合两种指令层级、或中途插入指令，先返回不支持，避免悄悄改变优先级 |
| assistant.tool_calls | 保留 function 名、ID、JSON 参数字符串；content 可为 null |
| assistant.reasoning_content | 兼容接收字符串或 null；不传给 Codex、不拼入正文/指令；非空字符串通过响应头提示未回放，详见下述规则 |
| role=tool | tool_call_id 必须能对应历史中的调用；输出以文本传回 |
| tools | function 类型与 JSON Schema 参数；保留描述和 strict 语义，不能悄悄改变 strict |
| tool_choice | 计划支持 auto、none、required、指定函数；采用 Codex 平铺 function 定义转换，P0/P3 必须验证；未通过的能力明确拒绝 |
| parallel_tool_calls | 按能力校验后保留；响应端始终能区分交错工具事件 |
| n | 省略或 1；其他值 400 |
| stream_options.include_usage | true 时在终止块后、[DONE] 前发送 usage 块；无统计则不伪造数值 |
| reasoning_effort | 仅接受选定模型声明且实测支持的值 |
| store | 省略或 false；true 返回不支持 |
| max_tokens、max_completion_tokens | 接受正整数（u64 范围）或 null；正整数不传给 Codex、不执行输出上限，通过兼容响应头提示；两个字段同时存在时逐个校验并列出；null 等同未设置 |
| temperature、top_p、stop、seed、logprobs、response_format 等 | 非空值严格拒绝；已列明的 null 占位按下述客户端历史与空值规则处理 |
| 非用户消息图片、远程图片 URL、音频、文件、旧 functions/function_call | 返回不支持内容或字段 |

空值与缺省按字段 schema 区分。未知字段默认 400，返回字段名；有证据需要兼容某客户端时，只能新增有文档和测试的允许规则。

Trae 文档确认自定义模型可使用 Chat Completions，支持 Base URL 或完整 URL；但其预设可能自动增加参数，必须记录实际客户端版本并验证。[Trae 模型配置](https://docs.trae.cn/ide_models)

如果 Trae 必填字段与上游能力冲突，先尝试客户端支持的配置；仍无法消除时记录阻塞并评审具体降级规则，不把静默丢参作为默认实现。

## Responses 请求

- 接受 model、input（字符串可规范化为文本数组）、instructions、stream、支持范围内的 tools/tool_choice、reasoning、include、parallel_tool_calls。
- 外部 stream=false 可用，由网关聚合；上游始终使用该路线要求的流式模式。
- max_output_tokens 按下述兼容规则接收正整数（u64 范围）或 null，不传给 Codex、不执行输出上限。
- store 默认按 false 处理，显式 true 拒绝；无服务端会话保存。
- 历史由客户端完整带入，包括已返回的工具调用、工具结果及允许的 reasoning 回放项。
- user 消息 content 数组支持 input_image，接受 Base64 data URL；规则与 Chat 共用，图片内容不会转换成文本。
- 不接受 previous_response_id、conversation、background、WebSocket、托管工具或客户端指定上游地址。
- function tools 使用参考 Codex 上游的平铺形式，保留名称、参数字符串与 strict；当前不支持 namespace/custom/托管工具。
- 非流式返回真实终态 Response 对象；不把 incomplete 或 failed 写成 completed。
- 原生流保留 item ID、call_id、索引、sequence_number 与已支持回放项，避免先降为纯文本再重造事件。

上游已按用户决定改为 Codex 专用 Responses，使用 store=false、stream=true 和完整上下文。平铺函数工具及请求头按本地参考实现；不再套用 SIWC namespace 规则。见 [当前认证决定](../03-architecture/adr/0003-codex-device-gateway.md)。

## Chat 推理历史兼容规则

客户端历史中的其他已知元数据及可选空值，另见下述“客户端历史与空值兼容规则”。

2026-10-13，用户报告 `Invalid or unsupported field: reasoning_content`。源码定位到本地字段白名单，回归用 assistant 历史消息稳定复现相同 400；未采集真实客户端原始请求，不能据此确认其全部字段形状。本次按已有 Agent 兼容方向新增窄范围例外，不改变 B11 的“Chat 不承诺保留隐藏推理状态”。

只读参考项目 `server/src/api/byok/protocol.rs` 的 `parse_chat_messages` 只从 assistant 读取正文和工具调用，thinking 为空且 replay_state=None。[OpenAI Chat 请求定义](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) 未定义该扩展字段；[Responses 推理文档](https://developers.openai.com/api/docs/guides/reasoning) 使用独立 reasoning item 的 encrypted_content 回放。这些是源码和文档依据，不代表本次真实 Codex 验证。

- 仅 Chat 的 `messages[]` 中 role=assistant 接受 `reasoning_content` 字符串或 null；缺省、null、空字符串不产生提示。
- 非空字符串作为兼容元数据接收后移除；不拼入 instructions、content，不伪造 reasoning item 或 encrypted_content。正文、工具调用 ID、参数原字符串、工具结果和顺序保持。
- 非空字段在 `X-Tiny-Proxy-Ignored-Parameters` 中以 `messages[].reasoning_content` 列出，同一请求只列一次；`X-Tiny-Proxy-Warning` 为 `Assistant reasoning_content is not replayed to the Codex upstream`。
- 同时含输出长度参数时，字段名按“长度参数、推理历史字段”顺序以逗号加空格连接，警告按同一顺序以分号加空格连接。JSON/SSE 及请求校验通过后的生成错误响应均保留提示，状态码和正文语义不变。
- 非字符串/非 null、非 assistant 消息、请求顶层、Responses 消息中的同名字段仍返回 400；其他未知字段和无效工具历史仍拒绝。错误和提示头不包含推理文本。Responses 原生 reasoning item 继续按原规则保留。

## 客户端历史与空值兼容规则

2026-10-13，用户报告 `Invalid or unsupported field: index`，要求参考旧项目成组修复，并指定核对 `D:/study/codex-proxy-dev`。报错来源是本地字段校验；未收集真实客户端原文，工具历史中的 index 为根据协议和源码确定的复现场景。

参考依据：`cursor-byok-main/server/src/api/byok/protocol.rs` 提取正文、工具 ID、名称和参数，再构造上游项；`codex-proxy-dev/src/types/openai.ts` 的 Zod 对象 schema 去除未声明的 index/reasoning_content，`src/translation/openai-to-codex.ts` 显式重建工具调用。后者还接收但不转发多个采样参数，提供结构化输出、旧函数格式和托管工具等额外能力；这些不等于 Tiny Proxy 已支持，也不作为静默丢弃有效语义的依据。[OpenAI 工具调用说明](https://developers.openai.com/api/docs/guides/function-calling)将 index 定义为流式工具增量的定位信息。此次采用显式位置和类型规则，避免递归删除工具参数或 JSON Schema 中的同名字段。

- Chat 的 `assistant.tool_calls[].index` 接受非负整数（u64）或 null；仅作流式聚合遗留元数据，不转发、不排序、不用于工具匹配。调用按数组顺序保留，结果按 call_id 配对；ID、函数名及完整 JSON 参数字符串仍必需。其他位置的 index 仍拒绝。
- assistant 的 `tool_calls: null` 等同无调用；`refusal` 接受字符串/null，非空拒绝文本转成原生 refusal 内容，不丢弃。assistant 的 `audio/function_call` 只接受 null，`annotations` 只接受 null/空数组；非空音频、旧函数调用和引文内容仍拒绝。其他角色携带这些 assistant 专属字段仍拒绝。
- tool 消息 `name` 可省略/null，非空时必须与 tool_call_id 对应调用的函数名一致；不作为新的调用标识，也不转发。其他角色的 name 仍不支持。
- 两个端点下列顶层字段的 null 等同省略：`stream/store/tools/tool_choice/parallel_tool_calls/prompt_cache_key/temperature/top_p/metadata/user/service_tier`。Chat 另含 `stream_options/reasoning_effort/n/stop/seed/logprobs/top_logprobs/frequency_penalty/presence_penalty/logit_bias/response_format`；Responses 另含 `instructions/reasoning/include/text/truncation`。这是没有值的占位兼容，不支持字段的非空值继续报错；任意未知字段即使 null 也报错。model/messages/input 等必需值不放宽。
- `stream_options.include_usage` 与 Responses `reasoning.effort/summary` 的 null 等同缺省。函数工具 description/strict 的 null 等同省略，显式 strict=false/true 保持；parameters 省略时补无参数对象 schema，显式 null/非对象拒绝。工具参数、schema 和密文不递归清洗。
- Responses reasoning item 保留合法 `in_progress/completed/incomplete` status，省略/null 可用，错误类型或未知状态拒绝；保留原有 id/summary/content/encrypted_content。这与 codex-proxy-dev 的合法状态保留方向一致，不采用 cursor-byok 的删除 status 策略。
- 冗余索引、匹配的工具名称与空值不产生“参数未生效”警告；有效长度参数与非空 reasoning_content 的既有警告仍保留。有效采样、停止条件、结构化输出、服务等级和存储语义不静默丢弃。

对应验收 A17g/A17h；自动回归与真实客户端验证分别记录在 [开发验证记录](../06-delivery/implementation-status.md)。

## 图片输入兼容规则

2026-10-13，用户反馈 image_url 被本地文本内容校验拒绝，并批准“按参考项目补齐”。此次追加图片输入范围，替代原先全面拒绝图片的规则。只读参考项目 server/src/api/byok/protocol.rs 的 parse_parts 解码 Base64 图片，server/src/plugin/sdk/protocol/openai_responses.ts 的 contentParts 将其编码为 input_image。标准格式同时核对了 [OpenAI 图片输入文档](https://developers.openai.com/api/docs/guides/images-vision)；这些是源码/文档证据，不能替代真实 Codex 图片测试。

- Chat：仅 role=user 的 content 数组接收 `{"type":"image_url","image_url":{"url":"data:image/png;base64,...","detail":"auto"}}`。转换后为 `{"type":"input_image","image_url":"data:image/png;base64,...","detail":"auto"}`。
- Responses：仅 role=user 的 content 数组接收上述 input_image 形式。单图、多图、纯图片、图文交错均保留顺序。纯文本与函数工具历史继续按原规则校验。
- 接受 image/png、image/jpeg、image/webp、image/gif 的非空标准 Base64 data URL，校验 MIME 和编码格式。图片字节是否能解码、GIF 是否静态及模型视觉能力由上游校验；网关不解码/压缩图片像素。拒绝空值、损坏的 Base64、其他 MIME、HTTP(S)/file URL、file_id 和未知图片字段；错误不包含图片数据。
- detail 省略时补 auto；显式 auto、low、high、original 原样保留，其他值（包括 null）拒绝。未在每个 Codex 模型上实测全部 detail 模式，上游拒绝时保留失败语义。
- system/developer/assistant 与工具结果中的图片仍拒绝；不静默丢弃图片。参考项目外部 Chat/Responses 入口的工具结果只提取文本，本次不扩展工具图片或其他协议别名。
- 2026-10-13 用户确认 413 后明确要求“代码里面改为不限制”，替代此前 2 MiB 总请求限制。Chat/Responses 显式关闭 Axum 默认请求体限制，文本、工具历史和 Base64 图片不按字节数拒绝；带 Content-Length 和分块请求共用此规则。正文仍完整读入内存并执行 JSON、字段、工具历史和图片格式校验；不承诺突破客户端、可用内存或上游自身限制。网关不抓取图片地址、不读取本地图片路径、不存储图片正文。

## 输出长度参数兼容规则

2026-10-13，用户反馈第三方 Agent 自动携带 max_tokens 后收到 400，并明确批准“接受兼容处理，优先让 Agent 接入”。此次仅替代原契约中拒绝输出长度参数的规则。参考项目在外部入口读取长度参数，在 Codex provider 中将 maxOutputTokens 置 null；其测试断言上游请求不存在 max_output_tokens。源码依据为只读参考项目的 server/src/api/byok/protocol.rs、server/plugins/build-in/codex-auth/provider.ts 与 codex_test.ts；这是代码核对，不是本次上游实测。

本网关仅接受各路由上述长度字段。正整数从发往上游的请求中移除，不通过改写提示词、字符截断或截断工具参数模拟 token 上限；实际输出和 usage 仍由上游决定。null 等同未设置，零、负数、小数、字符串、布尔值及非数值结构返回 400。其他未支持字段继续拒绝。

含有效非空长度参数且通过请求校验时，JSON 和 SSE 响应均附加：

- X-Tiny-Proxy-Ignored-Parameters：实际未生效的字段名，多个值以逗号加空格分隔，如 max_tokens, max_completion_tokens。
- X-Tiny-Proxy-Warning：Output token limits are not enforced by the Codex upstream。

这两个头也保留在后续生成失败的 HTTP 响应上，不改变原状态码或错误正文。没有非空长度参数时不添加长度提示；若含非空 Chat 推理历史字段，仍按上述规则添加对应提示。它们是网关扩展，客户端可能不会在界面显示，接入方不能依赖这些参数控制实际生成长度或额度消耗。

## 响应字段与 usage

Chat JSON：id、object、created（真实生成时刻）、model、choices；message 为 assistant，工具参数为字符串，finish_reason 为 stop、tool_calls、length 或 content_filter 的可证明映射。

Chat SSE：全程稳定 id/created/model；先 role，再 content 或 tool_calls 增量，随后 finish_reason；可选 usage；最后 [DONE]。只为正常完成或明确受限结束发送 [DONE]，流内失败不发送成功终止。

usage 只使用上游实际统计。缓存 token 是输入统计的子项、推理 token 是输出统计的子项，不能重复加总；无法获得时省略/置 null（按对应 schema），UI 显示未知。

Responses 保留上游支持范围内的 response 状态和 usage，不声称完整 SDK 所有辅助字段都可用。

## 错误契约

JSON 错误外壳为 error 对象，至少含 message、type、code，可含 param；响应头带本网关生成的 X-Request-Id。错误文本脱敏，保留可公开的上游错误码和请求 ID。

| 状态 | 本机含义/错误码 |
|---|---|
| 400 | invalid_request、unsupported_or_invalid_parameter（字段、内容或工具历史校验失败）、upstream_rejected_request |
| 401 | invalid_gateway_key；仅表示客户端密钥缺失、错误或已撤销并删除 |
| 403 | 撤销与已通过鉴权请求竞争时的 key_revoked，或已明确的模型权限限制 |
| 404 | model_not_found 或未知路径 |
| 415 | invalid_request；Content-Type 必须为 application/json 或受支持的 +json 类型 |
| 429 | gateway_concurrency_limit 或上游限流；有真实 Retry-After 才透传/映射 |
| 502 | upstream_protocol_error、upstream_stream_interrupted、不可直接映射的上游错误 |
| 503 | account_not_ready、upstream_auth_required、gateway_stopping、临时上游不可用 |
| 504 | upstream_timeout |

上游 401 归为 upstream_auth_required，而不是让客户端误以为它的网关密钥错误；保留实际 upstream_status=401 于脱敏诊断。已知上游 400/403/429/503 在含义合适时保留状态和代码。

2026-10-13 按用户要求及 [B20](../01-requirements/business-rules.md) 改为撤销即删除：删除后的新请求统一返回 401 invalid_gateway_key，替代此前为已撤销记录返回 403；仍取消已登记的在途请求，验收 A09、A24、A45—A47。

发出 HTTP 200/SSE 后不能改 HTTP 状态。Chat 用 data 中的 error 对象后关闭；Responses 按是否已建立 response 上下文发送合法 error/response.failed。具体消费者对流内错误的识别在兼容矩阵中验证，不能保证所有 Agent 都正确处理。

### 本地参数校验诊断（2026-10-14）

用户报告请求详情只有“请求含未支持字段或参数格式不正确”，要求优化同类问题。据此完善错误定位，不扩大参数兼容子集，也不采集正文。

- 本地参数失败仍返回 HTTP 400 / `unsupported_or_invalid_parameter`；JSON 的 `error.param` 提供第一个失败位置，数组使用从 0 开始的索引，例如 `messages[1].reasoning_content`、`messages[0].content[1].image_url.url`、`tools[0].function.parameters` 或 `input[1].content[0].text`。
- `error.message` 提供该位置的校验规则和修正建议；未知字段仍严格拒绝，可从发起请求的客户端响应取得原字段名。
- 本地异常表只记录代码内明确列举的协议字段名、程序生成的数组索引及固定规则/建议。任意未知字段名替换为 `<unknown>`，保留所属容器；不使用“只要是短 ASCII 就可保存”的过滤方式。请求值、图片、工具参数和上游原始错误都不落库。
- 本地校验拒绝时不启动生成请求；详情注明“尚未发送到上游”，不会把该 400 解释为上游模型失败。不支持的参数不会自动删除后重试。
- 旧记录缺少具体字段，不能从大小、数量或模型推断原字段；不改写历史原因。展示和复制行为见 [请求诊断](../05-operations/request-diagnostics.md#参数校验失败怎么处理2026-10-14)，新增验收为 A55—A57。

## 支持声明

MVP 通过后仅声明“支持已测试客户端的文本与函数工具调用、Chat Completions / Responses 子集”。不宣称全量 OpenAI 兼容、全量 Codex 模型可用或两种协议无损互转。

## 拒绝请求的 HTTP 连接收尾（2026-10-13）

Windows 回归确认，直接丢弃仍在上传的大请求体可能令 TCP 重置先于 401/415 响应送达。鉴权、Host、Origin 和 Content-Type 拒绝路径现在最多丢弃 8 MiB、等待 2 秒，仅按块读取，不解析或保存正文。超过收尾预算、Expect 头或网关停止时尽快返回错误并关闭连接；客户端可能观察到连接结束。该预算只约束已拒绝请求的传输收尾，不限制已鉴权合法请求的大小，也不转发被拒绝输入。
