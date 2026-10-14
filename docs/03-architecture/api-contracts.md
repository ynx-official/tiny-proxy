# 模型 API 契约

状态：Review  
最后更新：2026-10-14

关联文档：[流式转换](streaming-protocol.md)、[规则](../01-requirements/business-rules.md)、[验收](../01-requirements/acceptance/mvp.md)

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

## 默认信任下游字段（2026-10-14）

用户明确要求“把请求的校验放松一点，不对下游传过来的字段进行严格校验，默认信任”。本节替代此前字段白名单、类型/枚举/范围、工具配对及元数据位置的严格规则；历史依据保留在 [严格校验快照](../99-archive/api-contracts-strict-validation-2026-10-14.md)，验收 A17/A17a/A17b/A17d/A17f/A17h、A55/A56 已同步更新，新增 A58。

- 不再因未知字段（包括 null）返回本地 400。网关仅提取可映射的顶层选项，未知或用不到的顶层字段跳过；Chat 消息/内容/调用中的扩展元数据不进入提示词。Responses 原生输入及函数定义内部的扩展字段保留，语义由上游判断。
- 本地仅要求能读取 JSON 对象、非空字符串 model，以及协议转换所需的消息/输入结构、文本、函数名和调用 ID。错误保留具体位置与安全建议。已登录账号、独立网关密钥、固定上游、模型目录与请求生命周期仍按现有规则处理。
- 不校验采样/长度参数的类型或范围，不校验工具名称字符/长度、128 项工具数量上限、重复调用 ID、历史配对、参数 JSON 内容或推理状态枚举。Chat 参数字符串原样保留；非字符串参数序列化为 JSON 字符串，缺省为 `{}`；Responses 原生参数不重新解释。
- 不新增托管工具执行或 Agent 循环，定义仍转换为 function；缺少转换所需的函数名/对象仍报错。图像继续走既有 user Base64 入口，不抓取 URL 或读取本地文件。
- 本地接收与上游支持是两件事。可直接映射的非法或新值交给上游判断，真实上游拒绝不改写为成功；SSE/终态、断流、鉴权、撤销和取消的检查保持。
- `X-Tiny-Proxy-Ignored-Parameters` 仅使用固定的已知字段名；未知扩展名及参数值不进入响应提示头、异常表或日志。已知选项被跳过时附提示，未知扩展默认跳过。

## Chat Completions 请求

| 字段/内容 | 处理 |
|---|---|
| model、messages | model 为非空字符串且在当前模型目录；messages 必须是数组，可为空；每条消息需有可转换的对象/role，空输入由上游判断 |
| stream | 默认 false；接受 bool、字符串 true/false/1/0 和整数 1/0；无法识别的可选值使用默认值 |
| user/assistant 文本 | 字符串或带字符串 text 的内容块；额外字段与文本块类型元数据不严格校验；保持正文顺序 |
| user 图片 | 接受 image_url 包装对象或直接 Base64 字符串，以及 input_image 形式；继续按下述图片规则转换 |
| system/developer | 所有前置指令按顺序合并为 instructions，允许混用；中途指令作为原位置的原生消息保留 |
| assistant.tool_calls | 数组中的调用 ID、function 对象和函数名须可转换；保留调用顺序，忽略 index 和多余元数据；不检查 ID 唯一性或结果配对 |
| reasoning_content、name、audio、annotations、旧 function_call 等 | 按上下文作为元数据跳过，不校验值类型/角色；assistant 非空 reasoning_content 用响应头提示未回放 |
| assistant.refusal | 字符串保留为拒绝内容；其他类型跳过，不拒绝整条请求 |
| role=tool | 非空字符串 tool_call_id 用于转换；不要求在同一历史中找到匹配调用，不检查冗余 name；输出转换为文本 |
| tools | 定义数组，读取函数名和定义对象，规范化为 function；缺省 parameters 补无参数 schema，现有 parameters/description/strict 值与扩展字段交给上游 |
| tool_choice | 字符串/原生选择值直接传递；Chat function.name 包装转换为平铺选择；不校验枚举或名称是否在 tools 中 |
| parallel_tool_calls | 同 stream 的宽容布尔解析；不识别的值使用 false |
| stream_options.include_usage | 同上宽容解析；仅 stream=true 时使用，无统计不伪造数值，JSON 请求带此选项不报错 |
| reasoning_effort | 作为 reasoning.effort 发送，不按本地模型目录枚举拒绝；summary 默认 auto |
| n、store | 接收任意值，仍只返回一个结果，上游强制 store=false；与网关固定行为不同的非空值通过响应头提示 |
| max_tokens、max_completion_tokens、max_output_tokens | 任意非空值接收后跳过，不发送上游、不模拟输出上限，按固定字段名提示 |
| temperature、top_p、stop、seed、logprobs、response_format 等 | 接收后跳过，非空值通过固定响应头提示；未知扩展直接跳过 |

## Responses 请求

- model 仍需非空字符串；input 接收字符串、单个原生输入对象或数组，可为空。字符串规范化为 user 消息，单个对象包装为数组。
- 原生输入的角色、内容类型、text 值、工具参数/结果、ID 配对、reasoning status/密文及扩展字段不重新严格校验；原值与顺序保留，user 图片入口按下述规则转换。
- tools 继续转换成 function 定义；tool_choice、instructions、reasoning、include 的非空值直接映射。reasoning.effort/summary 的可选 null 继续视为缺省；不递归修改 schema、参数或密文。
- reasoning_effort 的目录枚举不作为本地拒绝依据，真实上游决定支持情况。prompt_cache_key 原值放入请求；仅当字符串可成为 HTTP HeaderValue 时添加对应 session/thread 请求头，不能编码为请求头的值只保留在 JSON 中。
- stream/parallel_tool_calls 采用宽容布尔解析；外部 JSON 由网关聚合，上游始终 stream=true、store=false。
- 采样、长度、服务端会话控制和用不到的顶层扩展字段跳过，不在本地严格拒绝；已知非空字段用响应头提示。不会因为收到 previous_response_id/conversation/background 而启用服务端会话、恢复路由或后台任务。
- 保留原生终态与流式投影规则，不将 incomplete/failed 写成 completed。

## Chat 推理历史兼容规则

此前 2026-10-13 的字符串/null 和 assistant 角色限制现已被“默认信任”方案替代；原始参考依据见 [快照](../99-archive/api-contracts-strict-validation-2026-10-14.md#chat-推理历史兼容规则)。

reasoning_content 的任意类型/位置不造成本地拒绝。Chat 转换不将其拼入正文、指令或加密推理回放；assistant 非空值仅以固定名称 `messages[].reasoning_content` 提示一次，其他角色/顶层扩展跳过。警告为 `Assistant reasoning_content is not replayed to the Codex upstream`。省略、null、空字符串不产生该提示。Responses 原生 reasoning 数据直接保留。

## 客户端历史与空值兼容规则

此前关于 index 类型/位置、结果 name 配对、未知 null、schema 类型和 reasoning status 的严格规则已替代，原始依据见 [快照](../99-archive/api-contracts-strict-validation-2026-10-14.md#客户端历史与空值兼容规则)。

- Chat index/name 等元数据只作兼容输入，按调用数组顺序和原 ID 转换；重复 ID、未完成/孤立工具结果可发往上游，不由本地修复或丢弃实际工具历史。
- 接收任意未知字段/空值；可选 null 通常不映射。tools、tool_choice、instructions、reasoning、include、缓存标识的 null 视为缺省。description/strict 的 null 从定义中省略，其他显式值和 parameters/schema 内的 null 保留。
- 函数参数字符串保持原文，即使是不完整 JSON；非字符串 Chat 参数序列化，缺省补 `{}`。不执行工具、不递归清洗 schema、原生输入或加密回放。
- Responses 原生 status/角色/类型/元数据原样转发，上游拒绝按真实失败返回。

## 图片输入兼容规则

Base64 图片能力沿用 2026-10-13 的批准边界，新增宽容包装与元数据规则。

- Chat user 的 image_url 接受 `{url, detail}` 包装或 Base64 字符串；也接受 input_image 形状。转换为原生 input_image，保持图文顺序。
- Responses user 的 input_image 继续规范化。PNG / JPEG / WebP / GIF 的 Base64 data URL 需有效且非空；不下载远程 URL、不读取本地路径。错误不回显图片数据。
- 图片包装/内容块中的 file_id 或额外字段不再触发拒绝；有有效内联 URL 时只用于兼容元数据，图像正文仍不持久化。
- detail 缺省/null 补 auto，其他值原样发送，由上游决定是否支持，不在本地校验枚举或类型。
- Chat 非 user 图片因不能转换为文本仍拒绝；Responses 非 user 消息 content 中的 input_image 仍遵守既有角色边界，原生工具结果不重新解析。
- 请求体仍没有本地字节上限；仅保留 JSON、转换所需的形状及图片入口检查，不检查工具历史配对。内存及上游限制仍可能影响真实请求。

## 输出长度参数兼容规则

此前只接收 u64 正整数的规则已替代，原始来源见 [快照](../99-archive/api-contracts-strict-validation-2026-10-14.md#输出长度参数兼容规则)。两个接口均接收 max_tokens/max_completion_tokens/max_output_tokens 的任意类型，非空值全部跳过，不改写提示词或截断内容来模拟上限，null 不产生长度提示。

JSON/SSE（包括后续生成失败）保留固定兼容提示头：

- `X-Tiny-Proxy-Ignored-Parameters` 按“长度、已知跳过选项、Chat 推理元数据”列出固定名称，同一类别按代码顺序；未知名称不列入。
- 有长度字段时包含 `Output token limits are not enforced by the Codex upstream`；有 assistant 推理元数据时包含上述未回放警告；其他已知选项被跳过时包含 `Unsupported client options are ignored; the gateway uses one result and store=false`。
- 多个警告按“长度、推理元数据、其他选项”以分号连接。不能依赖这些跳过的值控制输出、采样、保存或费用。

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

此前根据通用 400 报错补充了字段位置与安全建议；用户随后要求默认信任字段，因此本地错误仅用于必需结构/图片转换失败，扩展字段不作为错误。

HTTP 400 / `unsupported_or_invalid_parameter` 的 `error.param` 标明无法转换的位置（从 0 开始的索引），例如 `messages[0].tool_calls[0].function.name` 或 `input[0].content[0].image_url`。`error.message` 包含固定规则和建议。异常表只记录代码生成的字段路径与文案，不含任意字段名/值。

该阶段拒绝时未开始生成调用；详情注明“尚未发送到上游”。上游不接受已转发的值属于上游失败阶段，保留原失败。旧通用异常不改写，详见 [请求诊断](../05-operations/request-diagnostics.md#参数校验失败怎么处理2026-10-14)。

## 支持声明

MVP 通过后仅声明“支持已测试客户端的文本与函数工具调用、Chat Completions / Responses 子集”。不宣称全量 OpenAI 兼容、全量 Codex 模型可用或两种协议无损互转。

## 拒绝请求的 HTTP 连接收尾（2026-10-13）

Windows 回归确认，直接丢弃仍在上传的大请求体可能令 TCP 重置先于 401/415 响应送达。鉴权、Host、Origin 和 Content-Type 拒绝路径现在最多丢弃 8 MiB、等待 2 秒，仅按块读取，不解析或保存正文。超过收尾预算、Expect 头或网关停止时尽快返回错误并关闭连接；客户端可能观察到连接结束。该预算只约束已拒绝请求的传输收尾，不限制已鉴权合法请求的大小，也不转发被拒绝输入。
