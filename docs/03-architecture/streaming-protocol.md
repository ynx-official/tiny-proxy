# 流式协议与转换设计

状态：Review  
最后更新：2026-10-13  
关联文档：[API](api-contracts.md)、[参考分析](reference-analysis.md)、[验证计划](../04-quality/verification-plan.md)

## 处理链

```text
客户端请求
  → 鉴权 / 大小限制 / 字段与模型校验
  → 上游请求映射（选定凭据、完整历史、Codex 平铺 function tools）
  → reqwest 请求上游并验证状态及 Content-Type
  → 字节流 → 增量 UTF-8 / SSE 帧解析 → Responses 事件
      ├─ Responses 客户端：校验与必要字段投影后保留原生事件
      └─ Chat 客户端：语义事件 → chat.completion.chunk
  → 有界输出 / 下游写入
  → 可信终态 / 取消 / 错误
  → 单次元数据记录
```

Responses 入口不强制经过信息较少的 Chat 中间模型。Chat 路径使用小型内部语义事件；两条路径共享上游传输、SSE 解析和终态判断。

## SSE 帧解析

不能认为一次 TCP/HTTP chunk 就是一行或一个 JSON。

- 增量处理 UTF-8 跨字节分片；支持 LF/CRLF、空行分帧、多行 data 拼接、注释心跳、event/id 字段。
- 处理单 chunk 多事件、JSON 跨多个 chunk、空 data、BOM 和连接意外结束；P0 使用有大小限制的独立字节分帧器，并以逐字节边界测试核对行为。采用此实现是为了直接约束单帧内存；进入生产转换前继续验证语义事件与背压。
- 上游事件类型优先读取 JSON type；event 名存在时验证一致性，冲突报协议错误。
- malformed JSON、超限事件、必需字段缺失不能跳过后继续报成功。
- EOF 剩余未完成帧不视为完整事件；即便有完整 item.done，也必须等待 response 终态。
- [DONE] 只是部分协议的哨兵，不替代 Responses 的 completed/incomplete/failed。
- 已知非语义事件可忽略并计数；未知有输出/终止语义的事件按不支持失败，避免漏掉内容后宣称完整。
- 网络压缩可正常解码，但不能通过整段缓冲解压实现“伪流式”；拒绝不受支持的编码。

## 每请求状态

Created → OpeningUpstream → Streaming → Completed / Incomplete / Failed / Cancelled。

一个请求只能进入一个终态。终态之后不得继续发 token 或第二个结束块；重复终态和矛盾快照记录为协议异常。

上游 response.completed 才是完整推理成功。response.incomplete 是受限结束，response.failed 是失败；HTTP 200 不改变这些结果。实现依据见 [参考分析](reference-analysis.md)。

2026-10-13 实测 Codex 的成功 SSE 响应可能缺少 Content-Type。缺少该头时允许进入有大小和时间上限的 SSE 解析；显式声明其他内容类型时拒绝。缺失头不豁免 UTF-8、帧、事件、身份及完整终态检查，JSON/HTML、空流和中断仍失败。

completed 的 output 若为空，可从同一响应的 output_item.done 重建聚合输出：必须已收到所有连续索引条目的完整快照，message 状态为 completed，且与已接收增量和函数调用一致；仅有 delta、缺少 done、未完成条目、身份冲突均不满足条件。非空终态快照仍按原规则核对，不能用先前快照掩盖冲突。始终等待 response.completed 和正常流结束，不把 item.done 当作整个请求成功。原生 Responses 流转发上游事件；聚合 Responses 结果可包含上述经过校验的重建输出。

## 内部语义事件（设计名）

Start、TextDelta(item, content_index)、RefusalDelta、ToolStart、ToolArgumentsDelta、ToolDone、Usage、Completed、Incomplete(reason)、Failed(error)。

保留 Responses 原始 envelope 给原生路径；不能把复杂状态统一缩成 Stop/Length/ToolUse 后再声称原生无损。

## Responses → Chat 映射

| 上游事件 | Chat 输出 / 动作 |
|---|---|
| 上游成功建立响应 | role=assistant 首块；稳定生成 chatcmpl ID、时间和模型 |
| response.output_text.delta | choices[0].delta.content |
| response.output_text.done | 校验最终文本与已发增量；只允许补缺失后缀，不重新发送整段 |
| response.output_item.added（function_call） | 创建工具映射；发 index、id、type、function.name |
| response.function_call_arguments.delta | 对应 tool_calls[index].function.arguments 字符串增量 |
| arguments.done / item.done | 校验参数快照、身份和完整性；不重复发送已有前缀 |
| response.refusal.* | 转为 Chat 的 refusal 表达；不伪装普通工具或未知失败 |
| response.completed | 无工具时 stop；完整工具待客户端执行时 tool_calls |
| response.incomplete | 仅明确 token 上限映射 length；明确内容过滤映射 content_filter；其他原因错误化 |
| response.failed / error | 流内错误后关闭；无 stop、无 [DONE] |
| reasoning summary / 加密回放项 | 不拼接进普通 content；Chat 首版不输出私有 reasoning 扩展 |

终止 finish_reason 后按 include_usage 决定是否发送空 choices 的 usage 块，再发送 [DONE]。缺少使用量时不编造 0。

非流式 Chat 使用同一个状态机聚合；Completed 返回 JSON；明确受限文本结果可返回相应 finish_reason；失败则在尚未提交响应的前提下返回非 2xx。工具参数不完整时不返回可执行的 tool_calls 成功结果。

## 工具调用与历史

维护 output_index/item_id → 稳定 Chat tool index 的映射；call_id 是跨轮工具关联标识，不能误用 item_id。

多个工具参数可交错到达；按各自工具独立追加。元数据尚未到达时只做有界等待，不生成虚构工具名或空对象。final snapshot 和已发前缀不一致时失败，不能把不同字符串拼起来。

Chat 输入 function 定义映射为 Codex 平铺 tools，函数名和 call_id 不改名，参数保持原始 JSON 字符串。此处按 ADR 0003 替换先前 SIWC namespace 方案；工具实际执行由外部客户端负责。

历史 assistant.tool_calls → function_call，tool.tool_call_id → function_call_output。校验重复 ID、缺失配对和参数类型；保留参数字符串而不无故重新序列化。

一轮请求结束于模型输出工具调用；外部 Agent 执行并把结果作为下一请求的历史。本应用不运行 shell、文件工具、MCP 或自动生成下一轮。

## 推理回放边界

Responses 客户端可带回本次上游返回、允许保留的加密推理 item，网关按注册及模型策略校验并透传，不解密、不落库、不展示为正文。

标准 Chat 没有同等无损承载能力。首版不使用隐藏的全局会话缓存、不向消息正文塞加密数据。P0/P3 必须验证“只有可见历史+工具结果”能否继续目标模型；若不能，则该模型只开放 Responses 或重新评审兼容扩展。

## 错误、取消、背压

- 请求校验与上游 HTTP 建立失败尽量在下游发头之前返回 JSON 错误；不提前用 role 块掩盖上游 401/429。
- 发送 SSE 后失败必须发可识别错误并结束，不伪造成功。某客户端若忽略流内错误，不对其宣称该场景兼容。
- 下游 Body 被丢弃或写入失败时，取消令牌传递至 reqwest 读取、等待刷新和所有内部生产任务。
- 无界 channel 禁用。自然拉取背压优先；有桥接队列时同时限制事件数与总字节。
- 慢客户端达到写阻塞阈值则取消请求。UI 统计独立合并，不受 token 速率驱动。
- 本地心跳只维持下游连接，不能重置“上游没有事件”的计时器；总时限独立计算。
- 非流式聚合达到内存预算即终止；上游连接必须被取消/释放。
- 生成 POST 默认不自动重试。唯一例外为已明确拒绝且尚无输出的过期授权恢复，最多一次，见认证文档。

## 必测边界

逐字节中文 UTF-8、随机切片、多行 data、交错工具、重复 done、截断参数、拒绝内容、limit/failed、EOF、200 后额度错误、下游取消、慢消费者、终态后多余事件、非流式超限和实际 SDK 行为。完整场景见质量与验收文档。

