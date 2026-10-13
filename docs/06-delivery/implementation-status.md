# 开发验证记录

状态：Review  
最后更新：2026-10-13  
关联文档：[路线决定](../03-architecture/adr/0003-codex-device-gateway.md)、[Trae 接入](../05-operations/trae-connection.md)、[开发计划](development-plan.md)

## 当前交付

当前采用参考项目的 Codex 设备码认证与专用模型上游，保留桌面技术栈及 hashicorp 风格。主程序为 target/debug/tiny-proxy.exe，窗口标题“Tiny Proxy · Codex 网关”。

当前撤销即删除修复产物为 `artifacts/key-deletion-fix/tiny-proxy.exe`，运行中的标准主程序未替换；先退出旧程序再打开此新版，首次启动自动清理历史已撤销 Key。最新验证见本文末尾“撤销密钥即删除”。

最新数据恢复修复已构建到上述标准主程序，直接 `cargo run`。默认数据库现为 `%USERPROFILE%/.tiny-proxy/app.db`；下文早先的独立修复产物是历史构建，不作为当前启动入口。目录恢复证据见本文末尾。

| 能力 | 当前实现 | 验证边界 |
|---|---|---|
| 设备码登录 | 显示/复制代码、浏览器入口、轮询、取消、过期、拒绝/slow_down、换 Token | 本地模拟 HTTP 链路通过；用户反馈真实登录成功，保存会话已用于真实目录与文本调用 |
| 凭据 | codex_accounts 多账号、DPAPI、CAS；旧 codex_session 自动导入 | 隔离迁移及刷新身份不匹配测试通过；不读取本机 Codex 缓存 |
| 自动续期 | 到期前 60 秒刷新，同会话互斥 | 16 并发只刷新一次；发起请求取消后仍完成轮换保存 |
| 上游 | Codex 模型目录、账号头、专用 Responses、store=false/stream=true | 本地 HTTP fixture 通过；真实目录 10 个模型，gpt-6.1-sol 文本 completed；未验证全部模型或额度限制 |
| 对外 API | /v1/models、/v1/chat/completions、/v1/responses；独立 Bearer 鉴权 | 真实本机 TCP 集成测试通过 |
| 协议 | 文本、用户 Base64 图片、函数定义与调用/结果历史，参数增量、终态/usage、聚合 JSON | 图片顺序/校验/HTTP、交错工具、中文、截断失败、参数保留测试通过；真实 Codex 图片与 Trae 待测 |
| 生命周期 | 取消/断开、密钥撤销、停止监听释放上游；4 并发 | 本地持续 SSE 的连接释放测试通过 |
| 桌面 | 六个导航，设备码、模型探测、密钥、具体地址启动/停止、Base URL 复制、内存请求记录 | 上次已查看概览原生窗口；其余页面未完成视觉检查，不是全部原型交付 |
| 网络 | 用户手动选择具体本机/LAN/VPN 地址；无通配/公网、无自动防火墙修改 | 地址约束测试通过；第二台设备待验收 |

## 本次自动验证

Windows 11、Rust/Cargo 1.96.0、MSVC。命令均在 D:/study/tiny-proxy 执行。

| 命令 | 结果 |
|---|---|
| cargo fmt --all -- --check | 通过 |
| cargo clippy --features desktop --all-targets --locked -- -D warnings | 通过，零警告 |
| cargo test --features desktop --all-targets --locked | 51 项全目标通过；随后补充旧密钥兼容/取消记录回归，相关 7 项通过（共 53 项） |
| cargo build --features desktop --bin tiny-proxy --locked | 通过 |

初次开发的新增测试先出现缺失模块/接口的编译失败，再完成实现；其后加入刷新取消与 HTTP 生命周期行为测试。存储及既有模块回归不等于 Codex 实网授权成功。

## 设备授权 403 修复（2026-10-13）

用户报告：授权浏览器已打开，但 Tiny Proxy 窗口显示“上游 403”。本机独立申请设备码的诊断返回 HTTP 200，未完成授权的轮询返回 HTTP 403，安全提取的错误标识为 deviceauth_authorization_pending / invalid_request_error；未记录设备码、设备 ID 或凭据。

根因：旧解析器在检查 403/404 等待约定之前，对不认识的 error.code 返回失败；轮询响应还被提前要求必须是 JSON。修复后，支持该实际错误标识、参考项目的其他等待形式及 403/404 空/非 JSON 响应；明确拒绝和过期仍停止，5xx 不被掩盖。其他认证端点的真实 403 带阶段报错，申请、轮询、交换均发送 Accept: application/json。

回归先失败再修复：纯解析测试复现实际 403；本地 HTTP 测试复现“浏览器打开后等待即失败”，现可连续等待后完成交换；另覆盖申请/交换真实 403、请求头、取消、到期及原凭据保留。界面和当前使用说明按用户要求移除早期认证方案提示；未删除数据库凭据或归档依据。检查 36 份文档、298 个本地链接，无失效链接。

## 模型目录与文本探测修复（2026-10-13）

用户反馈登录成功后读目录出现通用协议错误，随后重读已显示列表。本机使用同一存储会话调用生产 Service::models，获得 10 个模型；独立安全诊断观察到 HTTP 200 / application/json、614466 字节。首次目录错误未稳定复现，不归因为凭据或某个已证实的字段。现将 JSON、列表结构、空目录、超限分开提示，并按参考实现补全模型字段别名与默认排序，覆盖回归测试。

文本探测在同一生产调用路径复现失败：成功 HTTP 响应没有 Content-Type，原代码在读 SSE 前就拒绝。诊断确认正文具有 response.created、文本增量、output_item.done 和 response.completed 等事件。部分终态中可提取的重复文本为空，原实现拒绝；另一次终态携带完整输出并通过。补齐缺失响应头处理，并用合成测试验证 completed.output 为空时仅从完整、连续且一致的 done 快照重建，未放宽 EOF、身份或终态要求。

最终运行 `cargo run --locked --example check_models -- --test-text`：目录 10 个模型，gpt-6.1-sol 文本成功完成，返回 24 个字符；该命令经过桌面同一 Service::open 与网关 consume 路径，收到完整 completed 并正常结束流。探测使用固定短提示并消耗账号额度。临时流元数据诊断已移除；保留不输出凭据/正文的显式目录及文本探测命令。真实工具调用、Trae 及跨设备验证仍未完成。

## 第三方 Agent 输出长度参数兼容（2026-10-13）

用户报告第三方 Agent 接入出现 Invalid or unsupported field: max_tokens，并明确批准“接受兼容处理，优先让 Agent 接入”。核对只读参考项目：外部 Chat 入口读取 max_completion_tokens/max_tokens，Codex provider 将统一 maxOutputTokens 置 null，协议构建器因此不发送 max_output_tokens；参考测试明确断言该字段不存在。本次没有对真实 Codex 上游发送诊断请求。

实现与取舍见 [API 契约](../03-architecture/api-contracts.md#输出长度参数兼容规则)：Chat 接收 max_tokens/max_completion_tokens，Responses 接收 max_output_tokens；正整数只兼容接收且通过两个响应头提示未生效，null 等同未设置。未放宽其他参数，未改写提示词或截断输出。README、业务规则 B09、验收 A17a/A17b、P3 计划及客户端接入说明同步更新。

先新增回归并实际观察到纯解析测试报同一 max_tokens 错误、HTTP 测试收到 400 而不是 200，再修改实现。回归覆盖 JSON/SSE、两种 Chat 字段同时存在、上游不带任何长度字段、正文/终态不截断、空值不带警告、非法类型/范围、其他未支持字段仍拒绝，以及模型错误保持 404 且保留兼容提示；拒绝请求未增加 fake 生成上游计数。

| 本次命令 | 实际结果 |
|---|---|
| cargo fmt --all -- --check | 通过 |
| cargo clippy --features desktop --all-targets --locked -- -D warnings | 通过，零警告 |
| cargo test --locked --all-targets | 55 项通过，无失败/跳过 |
| cargo test --features desktop --all-targets --locked | 构建阶段受阻：正在运行的旧进程锁住 target/debug/tiny-proxy.exe，Windows 拒绝覆盖；不能记为通过 |
| cargo rustc --features desktop --bin tiny-proxy --locked -- -o D:/study/tiny-proxy/artifacts/max-tokens-fix/tiny-proxy.exe | 桌面构建通过，产出独立新版；仅有自定义 -o 与 Cargo 输出路径有关的两条编译器提示 |

当前可重试的新版位于 artifacts/max-tokens-fix/tiny-proxy.exe，原 target/debug/tiny-proxy.exe 未替换。需本人退出正在运行的旧版，再启动该新版并手动开启网关；仍使用原默认应用数据目录。真实第三方 Agent 重试、工具往返及完整桌面运行验证尚未执行，不以本地 HTTP fixture 代替。

## 当前限制

- 支持多账号保存与手动切换；远端撤销、托盘、自启、完整设置和安装包尚未实现。
- 文本/用户 Base64 图片/函数工具子集；图片网址、非 user 图片、托管工具、不支持参数明确报错。图片请求仍受 2 MiB 总请求限制；真实 Codex 图片待测。Chat 无法携带的加密推理历史不会偷偷加进正文；真实模型多轮能力待测。
- 请求记录只有当前监听会话内存的 100 条元数据。未新增持久化日志或替用户批准删除策略。
- 进程退出可能中断正在进行的 Token 刷新，恢复时若上游已轮换需重新登录。
- 固定 Codex public client ID 与 backend-api 契约来自参考代码；按实际返回判定可用性，不以本地测试替代账号授权和上游资格。

## 下一步实测

1. 已完成用户本人 Codex 设备码授权与当前会话目录读取。
2. 已完成 gpt-6.1-sol 一次真实文本测试；其他目标模型按需要验证。
3. 使用本地网关密钥，在 Trae 发起文本及函数工具两轮请求，记录客户端版本和实际字段。
4. 选择真实 LAN/VPN 地址，用第二台本人设备验证连接与撤销。
5. 使用图片追加构建，在实际客户端发送一张 Base64 图片，确认目标模型的图片识别与 detail 能力；本地模拟上游只证明协议转发。

真实授权、模型目录和一次完整文本探测已有证据；不能据此宣称全部模型、工具往返或 Trae 接入均已通过。


## 本地持久化与多账号额度（2026-10-13）

实现 ADR 0004：账号逐条保存/选择，新密钥 DPAPI 保存并可查看/复制，模型目录和每次固定文本测试按账号落库，账号额度每小时及手动只读查询。后台任务串行查询账号，同账号排队去重；时间以最后尝试为准，扫描最多约一分钟延迟。次数子查询失败保留旧次数及单独更新时间。全局普通请求日志仍为内存元数据。

先写存储/迁移/解析回归，编译失败后实现；额度 HTTP 回归曾稳定复现“次数子查询失败清空历史次数”，修复后通过。自动验证覆盖旧账号导入、两个账号隔离/活动选择、重启恢复、旧摘要密钥仍有效、新密钥无明文、小时到期边界、取消/异常退出测试记录、真实本机 HTTP 的按账号额度鉴权与失败保留。未等待真实一小时，不将边界测试描述为一小时实测。

本次先通过 51 项全目标测试，再新增旧摘要密钥和测试取消两项、加强测试结果落库断言；相关 local_state/model_transport 共 7 项通过。fmt、desktop clippy 零警告及 desktop build 通过。工作区期间另有协议参数兼容改动，保留其行为，仅修正等价 Clippy 布尔表达式；对应 gateway_http/gateway_protocol 共 9 项回归通过。

升级前使用 SQLite backup API 生成本机一致性备份，未删除任何原账号或密钥。执行 `check_models --refresh-usage --test-text`：真实目录 10 个模型，额度 1 个窗口且重置次数查询成功；gpt-6.1-sol 完整文本 24 字符。独立重新打开 SQLite 确认 1 个已保存账号、10 个模型、1 条成功测试及额度快照。未把升级前纯内存历史伪造为已恢复。

新版原生窗口已打开：已查看账号额度页和模型页，模型页显示缓存读取时间与成功测试时间。视觉查看不等于全部页面交互验收；多账号生产授权、Trae 和跨设备仍需后续实测。新版新增密钥恢复能力已用临时数据库验证，真实旧密钥没有密文，无法还原且没有自动撤销。

## Base64 图片输入追加（2026-10-13）

用户反馈 `Invalid or unsupported field: image_url` 后明确批准“按参考项目补齐”。实现范围与批准依据见 [图片输入契约](../03-architecture/api-contracts.md#图片输入兼容规则)。此次只修改协议转换和回归测试，不调用真实模型消耗额度，不读取真实图片或改动认证/数据存储逻辑。

新增 tests/gateway_images.rs 的 6 项行为测试，修改实现前实际得到 4 项失败：Chat 报 image_url、Responses 报 input.content，HTTP 收到 400 而非 200。实现后全部通过，原 tests/gateway_protocol.rs 对非法图片的拒绝断言仍保留。已核对单图、多图、图文顺序、detail 默认与保留、MIME/Base64 校验、非 user 图片与未完成工具历史拒绝、JSON/SSE 上游转发、401/413、拒绝请求不上游及错误不回显图片正文。

| 本次验证 | 实际结果 |
|---|---|
| cargo test --locked --no-default-features --test gateway_images --test gateway_http --test gateway_protocol --test gateway_lifecycle --test response_terminal --test sse --test model_transport | 30 项通过，无失败/跳过 |
| rustfmt --edition 2024 --check src/protocol/gateway.rs tests/gateway_images.rs | 本次改动文件通过 |
| cargo clippy --all-targets --locked -- -D warnings | 通过，含桌面目标，零警告 |
| cargo fmt --all -- --check | 全量未通过：检查时工作区另有同时新增的 tests/data_recovery.rs 尚未格式化；未改写该文件 |
| cargo test --locked --no-default-features --all-targets | 全量未通过：检查时另有新增的 data_location::default_location_is_outside_virtualized_app_data 失败，实际仍解析为 AppData/Local/TinyProxy，而测试期待用户目录 .tiny-proxy；未删除/跳过/修改该测试 |
| cargo rustc --locked --bin tiny-proxy -- -o D:/study/tiny-proxy/artifacts/image-input-fix/tiny-proxy.exe | 桌面构建通过，产物已检查存在；两条提示仅涉及 Cargo 输出目录与显式 -o 的组合 |

代码审查确认图片分支仅适用于 user、图片 data URL 不进入 URL 下载或文件读取路径、未知图片字段继续拒绝、工具关联和终态路径保持原校验。未添加依赖、未修改锁文件。

本次独立桌面构建位于 artifacts/image-input-fix/tiny-proxy.exe（约 62 MB）。运行中的旧 target/debug/tiny-proxy.exe 未替换或结束；需退出旧应用，打开上述新版并手动启动网关后再重试图片。该构建采用当时的原数据目录逻辑，本次未完成或打包后来可能发生的数据目录恢复改动。没有启动这个新版窗口、没有真实 Codex 图片调用或 Trae 图片重试证据；不宣称全量工程检查、图片识别或生产发布已完成。全量检查的并行改动状态是上述检查时点记录，后续证据按对应工作记录更新。

## 启动目录重定向与历史数据恢复（2026-10-13）

用户在普通 PowerShell 执行 cargo run，启动快照的账号、密钥、模型和测试数量均为 0；在 Codex 执行的诊断却读取到完整数据。早期仅核对路径字符串及进程打开文件列表，错误地排除了目录差异，不能作为修复证据。

最终通过 Windows GetFinalPathNameByHandleW 查明：Codex 启动环境请求 `%LOCALAPPDATA%/TinyProxy/app.db` 时实际访问 `%LOCALAPPDATA%/Packages/OpenAI.Codex_2p2nqsd0c76g0/LocalCache/Local/TinyProxy/app.db`。该别名与显式包路径的文件标识相同。普通 PowerShell 的启动环境打开另一份空库；原因是 AppData 重定向，历史数据没有被删除。

实现 [目录恢复规则](../05-operations/desktop-operations.md#数据目录统一与历史恢复)：默认改为用户目录 `.tiny-proxy`，首次启动探测并一致性复制唯一有数据的旧库，解析物理路径去重，保留源库；不覆盖目标、不绕过旧版实例锁、不自动选择冲突来源。读取快照失败时停止初始化，不再发送“已就绪”。显式 TINY_PROXY_DATA_DIR 保留隔离语义。去掉临时 SQL/界面接收诊断和临时 examples，仅保留目录与安全数量日志。

回归先实际失败，再实现修复：默认目录仍指向 AppData 的断言失败；WAL 恢复、冲突与锁定回归失败；无效模型缓存触发错误后仍发 Ready 的回归失败。修复后以上通过，并覆盖已有目标不被替换、损坏源拒绝、包目录发现、密钥/账号持久化与正常退出。

真实恢复使用生产 Backend 启动链：3 个密钥、1 个账号、10 个模型、1 条测试记录，connected=true。迁移后全部 8 张表逐行比对相同，quick_check=ok；新文件句柄最终路径就是 `%USERPROFILE%/.tiny-proxy/app.db`，没有重定向。原库保留，新目录另外生成一致性恢复后备份。随后检查用户实际 cargo.exe 子进程，确认其打开新数据库；用户明确反馈“数据已恢复，窗口已关闭”。

| 最后验证命令 | 结果 |
|---|---|
| cargo clippy --locked --all-targets -- -D warnings | 通过，零警告 |
| cargo test --locked --all-targets | 69 项全部通过，无失败或跳过；包含并行完成的图片协议回归 |
| cargo build --locked | 通过，标准 target/debug/tiny-proxy.exe 已更新 |
| cargo fmt --all -- --check | 通过 |

此前全目标构建曾因用户正在运行 exe 被 Windows 锁定而失败；用户关闭窗口后上述最终命令已重新通过。未使用真实模型请求验证恢复，未消耗模型额度，也未把普通请求内存记录伪造为可恢复历史。

## Chat reasoning_content 兼容（2026-10-13）

用户报告 `Invalid or unsupported field: reasoning_content`（客户端显示 HTTP 400、unsupported_or_invalid_parameter、4028）。本地 `normalize_chat` 的消息字段白名单拒绝该字段，HTTP 层将错误映射为上述 400；无需读取真实账号或调用模型即可复现。真实请求原文未采集，本次针对 assistant 历史消息的兼容形状修复，不将客户端包装代码 4028 推断成上游错误。

规则与依据见 [Chat 推理历史契约](../03-architecture/api-contracts.md#chat-推理历史兼容规则)。接收字符串/null，不将推理文本拼进正文、指令或伪造的加密回放项；非空字段通过兼容响应头提示，同一请求只列一次，长度参数提示继续准确保留。Responses 原生 reasoning 回放和工具关联校验保持。同步 B11、A17e/A17f、P3、README、接入说明和文档索引。

新增 `tests/gateway_reasoning.rs` 后先运行回归，实际观察到解析报同一字段错误、HTTP 返回 400 而预期为 200（4 项中 2 失败），再修改实现。修复后覆盖字符串/null/空字符串、多次 assistant 历史、content=null 的工具调用、正文块数组、工具参数原字符串、结果回传；HTTP JSON/SSE、长度提示组合、404 错误保留提示；非法类型/角色/顶层位置/未知字段/无效工具历史以及无密钥请求均不增加模拟生成上游计数，错误不回显推理文本。

| 本次最终验证 | 结果 |
|---|---|
| cargo test --locked --no-default-features --test gateway_reasoning --test gateway_protocol --test gateway_http --test gateway_images | 19 项通过，无失败或跳过；含 4 项新增回归 |
| cargo clippy --locked --no-default-features --all-targets -- -D warnings | 通过，零警告 |
| cargo clippy --all-targets --locked -- -D warnings | 通过，零警告；首次检查曾遇到另一聊天正在修改的 GatewayBind 桌面事件未接完，待共享文件更新后重跑通过，本次未修改那部分代码 |
| cargo fmt --all -- --check | 通过 |
| cargo rustc --features desktop --bin tiny-proxy --locked -- -o D:/study/tiny-proxy/artifacts/reasoning-content-fix/tiny-proxy.exe | 通过，产出桌面 EXE；仅有自定义 -o 对输出路径的两条编译器提示 |

可重试构建为 `artifacts/reasoning-content-fix/tiny-proxy.exe`（62,435,840 字节），包含构建时共享工作区已有修改。运行中的旧进程未结束、标准 EXE 未由本次替换；退出旧应用后打开该新版，确认网关已监听，再重试原会话。未运行此新版 GUI，未执行真实 Codex 或第三方客户端重试，未消耗模型额度；上述模拟 HTTP 测试不能代替真实工具往返验收。本次未新增依赖或修改锁文件。

## 客户端历史与空值兼容（2026-10-13）

用户报告 `Invalid or unsupported field: index`，要求按旧项目成组处理，后指定比较 codex-proxy-dev。只读检查两个参考项目后，采用已知元数据/空值规范化及严格上游重建；具体规则集中在 [API 契约](../03-architecture/api-contracts.md#客户端历史与空值兼容规则)，取舍和参考源码位置见 [参考分析](../03-architecture/reference-analysis.md#codex-proxy-dev-兼容方式核对2026-10-13)。未采集真实客户端请求，源码定位与合成回归确认的是 assistant.tool_calls[].index 场景，不推断所有客户端 4028 都有相同根因。

新增 `tests/gateway_compatibility.rs`：实现前实际运行 8 项，7 失败、1 通过，包含 `index` 同文案错误及 HTTP 400/预期 200。按 codex-proxy-dev 对照结果，将推理状态用例明确为“合法状态保留”，重新运行仍因 reasoning 空值失败，然后实现修复。未删除或放宽原有回归断言。

覆盖内容：流式工具增量合并保留 index 后回传、两次调用和乱序工具结果、首轮/后续 JSON/SSE 四种组合；保持函数参数原字符串、call_id 和正文，剔除冗余元数据；assistant 拒绝文本转换，函数工具缺省 schema、可选空值及嵌套 schema 内容保护；非法类型/位置、未知字段、工具名称不符、重复/缺失调用和损坏 JSON 拒绝。HTTP 非法 Chat 历史均无新增模拟生成调用；既有长度/推理警告保留。

| 本次最终验证 | 结果 |
|---|---|
| cargo test --locked --no-default-features --test gateway_compatibility --test gateway_protocol --test gateway_reasoning --test gateway_images --test gateway_http | 27 项通过，包含新增 8 项；之后补充边界断言并由下述整体回归再次验证 |
| cargo test --locked --no-default-features --all-targets | 87 项通过，零失败；未启用 desktop，桌面图标测试因 feature 条件不参与，不宣称全桌面测试通过 |
| cargo clippy --all-targets --locked -- -D warnings | 包含 desktop，检查通过，零警告 |
| cargo fmt --all -- --check | 通过 |
| cargo rustc --locked --features desktop --bin tiny-proxy -- -o D:/study/tiny-proxy/artifacts/client-compatibility-fix/tiny-proxy.exe | 通过；两条警告仅与自定义 -o 输出路径和 Cargo 默认输出参数组合有关 |

桌面产物：`artifacts/client-compatibility-fix/tiny-proxy.exe`（62,548,992 字节）。运行中的旧 `target/debug/tiny-proxy.exe` 未结束或替换。退出旧应用后打开这份最新构建，确认代理已自动监听，再重试原会话；不能继续启动前面历史记录中的旧修复文件。

本次未使用真实账号/模型额度，未运行新 GUI，也未执行真实 Trae/Codex 工具往返。测试上游为本地 Axum fixture，模拟客户端工具执行；真实联调仍待验收。未新增依赖或修改锁文件、未修改两个参考项目、未提交或发布。修改范围为请求规范化模块、新回归测试及相应契约/验收/计划/接入文档；工作目录当前没有 Git 元数据，未声称与提交基线做过 diff。

## 撤销密钥即删除（2026-10-13）

用户明确要求“撤销了的 key 就彻底删除不展示”，据此更新 R06/B20、A09 及追加 A45—A47。此前 `revoke_key` 只写 revoked_at，列表保留已撤销行且 reveal_key 仍能读取其密文。现在以 DELETE 删除整条记录，包括鉴权摘要与 DPAPI 密文；提交成功后取消对应在途请求并发送最新桌面快照，移除行及正在显示的该密钥明文。删除失败保留记录并显示错误，其他 Key 不受影响。删除后的新请求统一返回 401 invalid_gateway_key。

新增 0005_delete_revoked_keys 迁移，升级时清理旧版已撤销记录，保留有效密钥及旧摘要密钥；原迁移文件和 revoked_at 兼容列保持。迁移与删除边界见 [数据模型](../03-architecture/data-model.md#0005-已撤销密钥清理)。同步 PRD、业务规则、验收、认证/API、安全、原型、操作说明、实施计划、README 和文档索引。

`tests/key_deletion.rs` 的 4 项回归在实现前实际全部失败，分别覆盖删除后仍留列表、升级仍留历史已撤销行、失败行为未执行删除、桌面快照仍包含 Key。随后实现修复并通过。另有 2 项桌面状态单测覆盖对应明文与确认状态清除、失败或其他 Key 变化时保留有效预览；初次运行因新增函数尚未实现而编译失败，完成实现后通过。HTTP 集成覆盖三个 /v1 路由拒绝旧值且不发上游请求；持续 SSE 测试覆盖撤销后的上游释放、另一 Key 的流继续和停止服务。

| 本次最终验证 | 结果 |
|---|---|
| cargo test --locked --no-default-features --all-targets | 92 项通过，零失败；未启用 desktop |
| cargo test --locked --bin tiny-proxy removed_key_display | 2 项桌面状态测试通过；不依赖原生窗口点击 |
| cargo clippy --all-targets --locked -- -D warnings | 包含 desktop，通过、零警告；测试模块位置告警修正后重新通过 |
| cargo fmt --all -- --check | 通过 |
| cargo rustc --locked --features desktop --bin tiny-proxy -- -o D:/study/tiny-proxy/artifacts/key-deletion-fix/tiny-proxy.exe | 通过；两条编译器警告仅涉及自定义 -o 与 Cargo 输出参数组合 |

桌面产物为 `artifacts/key-deletion-fix/tiny-proxy.exe`（62,554,624 字节）。标准 `target/debug/tiny-proxy.exe` 当时仍由用户运行，未结束或替换；新版本未启动，真实数据库的历史 Key 尚未由本次测试清理，待用户重开新版执行迁移。所有删除测试使用隔离临时 SQLite，网络测试使用本地模拟上游；未消耗真实模型额度，未完成新版窗口点击/视觉验收。未新增依赖、修改锁文件、提交或发布。
