# cursor-byok-main 参考分析与来源

状态：Review  
最后更新：2026-10-13  
关联文档：[架构](overview.md)、[认证 ADR](adr/0002-auth-provider.md)、[流式方案](streaming-protocol.md)

## 证据范围

已阅读本地 D:/study/cursor-byok-main 的 AGENTS、README、server/Cargo.toml、MIT LICENSE 及下列实现。该目录不是本次项目的基础分支；本次不修改它，也没有运行它的测试或登录请求。

所有结论来自这份本地快照和当日官方文档，不假定 GitHub main 与本地一致。

## 最相关的三段实现

| 本地源码 | 已核对行为 | Tiny Proxy 的采用方式 |
|---|---|---|
| [oauth.ts](D:/study/cursor-byok-main/server/plugins/build-in/codex-auth/oauth.ts)（71、99、132 行附近） | 发起设备授权、轮询、换 Token；不调用本地 Codex 进程 | 按用户批准的 ADR 0003 采用设备码协议与 public client ID；Rust 独立实现状态机 |
| [provider.ts](D:/study/cursor-byok-main/server/plugins/build-in/codex-auth/provider.ts)（18、55、72 行附近） | 使用 Codex 专用 Responses 端点、账号头与订阅特定字段 | 采用该 Codex 端点及账号头；不能与 SIWC 凭据混搭 |
| [openai_responses.ts](D:/study/cursor-byok-main/server/src/plugin/sdk/protocol/openai_responses.ts)（69、231 行附近） | 构造历史与工具、处理文本/参数增量、终态和推理回放 | Rust 中实现有测试的协议模块，补帧解析及严格终态 |
| [byok/mod.rs](D:/study/cursor-byok-main/server/src/api/byok/mod.rs)（37、59、142 行附近） | /byok/v1/models、chat/completions、responses、messages；独立 API Key 鉴权 | 最直接的外部模型 API 参考；本项目简化为 /v1，首版不含 messages |
| [byok/protocol.rs](D:/study/cursor-byok-main/server/src/api/byok/protocol.rs)（35、178、274、430 行附近） | 客户端不同协议解析成内部模型请求 | 借鉴分层；增加能力矩阵与不支持字段显式报错 |
| [byok/output.rs](D:/study/cursor-byok-main/server/src/api/byok/output.rs)（164、189、261 行附近） | 聚合/流式编码、工具参数增量、Drop 取消、[DONE] | 借鉴输出器与取消；补字段、终态和 SDK 契约测试 |
| [provider/event.rs](D:/study/cursor-byok-main/server/src/provider/event.rs) | 统一模型事件及 FinishReason | Chat 路径参考；原生 Responses 不压缩成该最小事件集 |

## 其他可借鉴部分

- [models.ts](D:/study/cursor-byok-main/server/plugins/build-in/codex-auth/models.ts)：按账号拉模型目录，避免静态列表。
- [PluginResourcePanels.tsx](D:/study/cursor-byok-main/apps/desktop/src/features/plugins/PluginResourcePanels.tsx)：开始、等待、成功、失败的登录交互；GPUI 重做界面，不引入其 React/Tauri 前端。
- [desktop.rs](D:/study/cursor-byok-main/apps/desktop/src-tauri/src/desktop.rs) 231—305 行附近：桌面进程内绑定并运行服务，关闭窗口入托盘，显式退出时取消服务。
- [codex_test.ts](D:/study/cursor-byok-main/server/plugins/build-in/codex-auth/codex_test.ts) 344、453 行附近：合成流和工具调用样本；它们证明有测试设计，不证明当前账号可调用。

## 需要修正而非照搬的行为

1. [resources.ts](D:/study/cursor-byok-main/server/plugins/build-in/codex-auth/resources.ts) 491 行的 refreshAccount 查询使用量，并非 OAuth Token 续期。在所查 codex-auth/插件源码中没有 refresh_token grant；新应用必须显式实现轮换所有权。
2. openai_responses.ts 296 行附近按 data 行读取，420—430 行允许某个 item 已完成但无 response 终态时成功。新实现按 SSE 帧解码，EOF 必须依照真实终态判断。
3. 同文件 406—411 行将所有 incomplete 归为 length。新实现区分 incomplete_details，未知原因不能伪报 token 上限。
4. byok/output.rs 的 Responses 聚合对象固定写 completed、created_at=0；Chat created 也为 0。新实现保留真实状态和时间。
5. byok/output.rs 的 Output 聚合不保留 ProviderReplayState；新原生 Responses 路径保留必要回放项，不绕经有损 Chat 投影。
6. byok/output.rs 304 行附近有 usage 即输出；新实现尊重 include_usage，未知统计不编造 0。
7. byok/protocol.rs 的部分字段没有进入统一请求结构；新实现以显式能力表决定接受/拒绝，不声称字段已生效。

这些是当前代码的特定行为及本项目的取舍，不代表对参考项目进行完整缺陷审计。

## 明确不带入的部分

[local_app/proxy.rs](D:/study/cursor-byok-main/server/src/local_app/proxy.rs) 的证书代理、Cursor host/path 接管；[api/cursor/handlers.rs](D:/study/cursor-byok-main/server/src/api/cursor/handlers.rs) 的 RunSSE/Bidi 协议；[run/engine.rs](D:/study/cursor-byok-main/server/src/run/engine.rs) 的 Agent 循环及 tool_round；完整插件运行时、用户/团队计费和项目工作区管理。

这些是另一个产品边界。Tiny Proxy 仅把工具调用数据交回外部客户端。

## 本地源码指纹

供后续确认参考快照，SHA-256：

| 路径（相对参考根目录） | SHA-256 |
|---|---|
| server/plugins/build-in/codex-auth/oauth.ts | 91F5462D61754F55C15882A8A1A9076101168B81F904EC1B52E8184CD190C2B1 |
| server/plugins/build-in/codex-auth/provider.ts | 55D56F96C8EB84CEDE512AEDE5FAEEEEC59F6E8EA43F88773DACB37E21424863 |
| server/src/plugin/sdk/protocol/openai_responses.ts | E9D04F2A1710FFAEEC6D8F56D5925861904659BC53CFFB2D1F48AB722EE16942 |
| server/src/api/byok/mod.rs | 50FD74B246E7437F7EC3BBBE7EFA0B9A237BAF1148779DA2A573894204E30AE4 |
| server/src/api/byok/protocol.rs | 9D7FB7D0F51F8B856B2E9DE99944089D6D9758F43AAED8B0DB4B987DA4896C4F |
| server/src/api/byok/output.rs | C153AD7F0CA08A393B54C04538CFFC16257B69BB4C245C8864E6CED203CCCB0C |

若移植实质代码，按参考仓库 MIT 声明保留署名与许可；当前实现依照本项目方案独立编写，没有复制参考仓库代码。

## codex-proxy-dev 兼容方式核对（2026-10-13）

用户指定只读参考 `D:/study/codex-proxy-dev`，本地 package.json 版本为 2.1.16。未安装其依赖、未执行该项目测试、未修改参考文件；以下是源码核对，不代表其所有能力均已实测。

| 源码 | 行为与启示 |
|---|---|
| src/types/openai.ts：ChatMessageSchema/ChatCompletionRequestObjectSchema | Zod 普通对象 schema 未声明工具 index 或消息 reasoning_content，解析时去除；部分内容/托管工具对象使用 passthrough。不是对所有位置统一宽松透传 |
| src/translation/openai-to-codex.ts：translateToCodexRequest | 显式重建 function_call/function_call_output；接收 temperature/top_p/stop/长度参数但没有将它们写入 Codex 请求；response_format 会实际映射 text.format |
| src/translation/tool-format.ts：openAIToolsToCodex | 平铺工具，缺省 strict=false，规范化 schema；Tiny Proxy 保留自己已有的 strict 缺省语义和 schema，不整体照搬 |
| src/proxy/reasoning-input-sanitizer.ts | 保留合法 reasoning status，筛选已知回放字段；与 cursor-byok 去除 status 的策略不同，Tiny Proxy 保留合法状态并拒绝非法值 |

该方式减少附带元数据导致的 400，但接受不执行的参数会造成设置有效的错觉；且 `tool_calls: null` 等已声明字段的空值仍可能被其 schema 拒绝。推荐 Tiny Proxy 对已知冗余元数据和 null 占位兼容，显式重建上游请求；对会改变生成行为的参数实现语义或报错。已批准降级字段继续给响应头提示。具体实施范围以 [API 契约](api-contracts.md#客户端历史与空值兼容规则) 为唯一权威；结构化输出、旧函数格式、托管工具、跨协议路由不因这次核对自动纳入范围。

## 历史 SIWC 候选与通用资料索引（不作为当前 Codex 上游契约）

| 来源 | 支持的结论 |
|---|---|
| [SIWC 概览](https://developers.openai.com/siwc/token-sharing-open-source) | 历史 SIWC 候选的自身应用注册与 host 区分 |
| [注册与登录](https://developers.openai.com/siwc/token-sharing-open-source/sign-in) | public-client OAuth、loopback、身份与权限校验 |
| [账号与会话](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions) | Token 轮换、会话撤销、注册隔离 |
| [模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference) | 公共 Responses、账号模型目录、完整终态 |
| [预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations) | 参数、历史、tools 和输入限制 |
| [错误恢复](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery) | 授权、额度和临时失败区别 |
| [GPUI](https://gpui.rs/) / [GPUI Kit](https://github.com/longbridge/gpui-kit) | 桌面框架、组件、匹配的依赖组合 |
| [Tokio](https://tokio.rs/tokio/tutorial) / [Axum](https://docs.rs/axum/latest/axum/) | 运行时与 HTTP 框架分工 |
| [sqlx SQLite](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html) | SQLite 连接与运行配置 |
| [DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata) | Windows 用户级秘密保护与边界 |
| [Trae 自定义模型](https://docs.trae.cn/ide_models) | Chat Completions 接入、地址与高级参数 |

官方资料会变化，P0 开始及版本发布前重新核对；没有把任何未运行的流程标记为成功。

