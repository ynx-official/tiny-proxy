# 开发验证记录

状态：Review  
最后更新：2026-10-15
关联文档：[路线决定](../03-architecture/adr/0003-codex-device-gateway.md)、[Trae 接入](../05-operations/trae-connection.md)、[开发计划](development-plan.md)

## 当前交付

2026-10-15 用户反馈上游 HTTP 200 后流中断。只读核对本地元数据及源码后，确认原 `upstream_stream_interrupted` 丢失具体错误分支，无法还原该历史请求真实原因。先补 44 组模拟请求的回归代码，再细分安全文案，并统一流内 error / JSON error / 持久化异常的安全原因。成功判定、超时、自动重试和数据库结构不变，旧记录不改写。权威说明见[流中断诊断](../05-operations/request-diagnostics.md#流中断诊断2026-10-15)，待验见[请求记录清单](../04-quality/request-records-checklist.md)。未执行编译型测试、Clippy、构建、打包或真实上游调用；运行中的 EXE 未停止或替换，不能宣称原断流已实测修复。

本次非编译检查：直接使用本机 Rust 1.96.1 的 rustfmt 格式化 `src/gateway.rs` 和 `tests/gateway_lifecycle.rs` 并执行 `--check`，退出码 0；本次涉及源码与文档的 `git diff --check` 退出码 0。这些结果只证明格式与差异检查通过，不证明编译或运行行为。

2026-10-14 用户先要求参数失败原因具体，随后要求“默认信任下游字段”。本次源码交付以后一条明确要求为准：去除字段白名单，采样/长度/历史元数据默认接收，取消工具名称/数量/参数 JSON/历史配对与原生状态枚举的本地检查，reasoning effort 交上游判断；兼容布尔和图片包装，未用到的顶层值跳过，原生值保持。网关固定单结果、stream=true/store=false 与兼容提示保持；缓存 JSON 原值保留，仅安全可编码值转为请求头。

先前添加的路径/建议持久化、跨页异常读取、延迟详情隔离和毫秒显示继续保留，用于确实不能转换的必要结构。现有日志不补采正文，旧异常不改写，无新迁移。当前权威为 [默认信任契约](../03-architecture/api-contracts.md#默认信任下游字段2026-10-14)，历史严格方案在 [归档快照](../99-archive/api-contracts-strict-validation-2026-10-14.md)，对应 A55—A58 及修订后的 A17 系列。

本次先新增默认信任回归代码，再修改实现并同步旧严格用例；遵守项目限制，未运行失败/通过的编译型测试、Clippy、构建、打包或真实客户端联调，也未修改运行中的 EXE。仅允许的格式/差异检查结果随本次交付说明报告，不能视为运行验证。待验命令与场景见 [请求记录清单](../04-quality/request-records-checklist.md)。

2026-10-14 安装版数据库初始化故障：只读完整性检查正常；本地 0001–0008 迁移记录为 LF 校验值，已安装 v1.0.0 EXE 内嵌 CRLF SQL 与校验值，确认存在换行差异导致的迁移校验冲突。源码已固定 LF、兼容精确 CRLF 历史值并细分启动错误；保留真实数据库、迁移记录与现有 EXE。已补回归代码，尚未执行编译型测试、构建或打包，不能声称安装版已实测修复；详见 [数据库启动兼容清单](../04-quality/database-startup-checklist.md)。

2026-10-14 Windows 启动无控制台、关闭默认收起托盘及恢复/退出源码已修改。未重新编译、运行测试或打包，现有 EXE 不变，不能作为新行为的运行证据；待验场景集中在 [Windows 桌面生命周期清单](../04-quality/desktop-lifecycle-checklist.md)。

本次补上 Windows GUI 子系统属性、原生托盘 owner/通知图标/菜单、关闭拦截、按数据目录唤起现有实例、设置退出与可见错误反馈。Win32 回调只发送事件，退出沿用 GPUI 结束后 `Backend::stop` 的收尾路径。`raw-window-handle` 复用锁文件已有版本，未升级依赖或版本号。

本次非编译检查：修改的 Rust 文件使用本机 Rust 1.96.1 的 rustfmt 直接格式化并检查通过；项目固定 1.96.0 的 rustfmt 无法启动（退出码 `0xc0000135`），经 rustup 入口还会触发渠道同步，因此本次未安装/修复工具链。离线 `cargo metadata --locked --no-deps` 成功解析清单；完整离线依赖元数据受未缓存的 `accesskit_atspi_common v0.19.1` 阻挡，不能据此声明完整依赖验证。上述检查均不是编译或运行测试。

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
- 文本/用户 Base64 图片/函数工具子集；图片网址、非 user 图片、托管工具、不支持参数明确报错。2026-10-13 后续按用户要求取消请求体大小限制，新代码尚未运行验证；真实 Codex 图片待测。Chat 无法携带的加密推理历史不会偷偷加进正文；真实模型多轮能力待测。
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

## 请求元数据与长耗时诊断（2026-10-13）

用户要求请求详情、起止时间持久化和异常分开，并明确选择“只保存元数据”。新增 0006、请求/异常独立存储、运行中观察、每秒检查点、重启中断恢复、周期清理、两个分页视图与关联详情。权威行为见 [请求诊断](../05-operations/request-diagnostics.md)。没有请求正文/工具参数/认证信息列；错误文案由网关白名单生成。

本地 fixture 已稳定复现“有效 completed 之后上游连接保持打开，客户端迟迟不结束”：实现前的回归测试在 2 秒超时失败。修正为校验终态后结束，保留缺少终态 EOF 的失败语义，并将失败事件与网络超时区分。源码审查又补上可选 [DONE] 分片、终态/检查点竞争及恢复耗时排除应用关闭期间的问题。

| 已执行检查 | 结果和范围 |
|---|---|
| cargo test --locked --no-default-features --test request_records | 第一版 3 项通过：结果/异常分开、重启恢复、清理；后来恢复耗时语义及断言调整未重新运行 |
| cargo test --locked --no-default-features --test gateway_lifecycle | 第一版 7 项通过：终态不等 EOF、实时元数据/断开、参数/鉴权/上游失败及既有取消/工具行为；后来终态矩阵扩展至两个接口、两种模式及分片尾部，未重新运行 |
| cargo clippy --all-targets --locked -- -D warnings | 当时包含桌面的版本通过；后续收尾代码不以此结果声称最终通过 |
| cargo test --all-targets --locked | 因正在运行的 target/debug/tiny-proxy.exe 无法移除而未完成；没有停止用户的应用 |

用户随后明确要求“除非我说了，否则只需要改代码，告诉我要测试什么”。已同步 [AGENTS.md](../../AGENTS.md)，此后停止编译型验证，不构建/打包、不创建修复版产物、不运行新 GUI、不替换旧程序。最终验收交由 [测试清单](../04-quality/request-records-checklist.md)，当前仅交付源码和回归测试代码；真实上游、GUI 及最终代码测试尚未完成。未新增依赖、改锁文件、提交或发布。

## 请求详情复制明细（2026-10-13）

用户确认失败请求为 HTTP 413，并要求将详情“复制 ID”改为“复制明细”，复制界面可见信息以便排查。按 A53 修改共用详情面板：展示与复制采用相同字段文本，包含起止时间、阶段、HTTP、大小/数量/用量及关联 ID；已加载的对应异常同时展示与复制代码、原因、时间、阶段和异常 HTTP。点击后状态栏反馈“请求明细已复制”。权威行为见 [请求诊断](../05-operations/request-diagnostics.md)。

先添加 4 项格式化回归测试代码，再提取共用详情文本并修改剪贴板按钮。已执行 `cargo fmt --all -- --check`，源码格式检查通过；针对改动文件、按 CRLF 行尾规则进行的差异空白检查通过。这些是源码检查，不代表编译或测试通过。依项目限制未运行测试、clippy、构建或窗口验收；测试命令与粘贴检查见 [测试清单](../04-quality/request-records-checklist.md)。未修改请求体上限、数据库结构、依赖或现有可执行程序。

## 白天、夜间及跟随电脑主题（2026-10-13）

用户要求增加白天模式并默认跟随电脑，权威规则见 [视觉系统](../02-design/visual-system.md#白天模式与跟随电脑)，验收为 A-theme-01—A-theme-04。

代码已修改：主题偏好独立于系统当前外观；GPUI 订阅系统外观通知，仅跟随模式自动切换；所有页面改用统一 Palette，组件默认主题与自定义配色一起刷新。设置提供三种选择，由后台保存成功后应用，失败保留原选择；专用回执不重置登录任务状态。迁移 0008 增加默认 system 的受约束字段，新安装及升级默认跟随，启动快照恢复偏好。保留工作区并行的其他功能改动；本次没有修改 Cargo.toml、锁文件或运行程序。

先编写 tests/theme_preferences.rs，再补实现；包含默认/固定模式、存储校验、旧库升级、重启、后台回执和写入失败的回归代码；桌面主题模块补系统外观映射及文字对比度断言。测试运行时明确关闭更新检查，保持临时库场景不访问外部服务。按项目限制，未运行这些 Rust 测试、clippy、编译、构建、打包或原生窗口检查，不能宣称功能已实测。

源码检查：直接使用已安装的 rustfmt 1.96.1 对本次涉及的六个 Rust 文件检查通过（skip_children=true，未遍历其他模块）。从配色源码进行 sRGB 静态计算，夜间最小文字/按钮/焦点对比度约 5.02:1，白天约 5.10:1；不是屏幕视觉验收。全项目 cargo fmt 检查先前报告了工作区其他变动的格式差异，未将其当作本次通过证据。工作区随后新增锁定 1.96.0 的工具链配置，rustup shim 触发下载尝试后已取消，后续格式检查直接调用已安装工具，不安装工具链。实机及自动验证清单见 [主题验证](../04-quality/theme-checklist.md)。

## 取消请求体大小限制（2026-10-13）

用户确认 HTTP 413 后要求“代码里面改为不限制”，据此追加 B21/A54，替代此前 2 MiB 请求体规则。已核对本地锁定依赖的 axum-core 0.5.6 源码：Bytes 提取器默认限 2 MiB，必须显式调用 `DefaultBodyLimit::disable()`；删除配置不会取消默认限制。两个生成接口现不按请求字节数拒绝；读取错误及异常记录文案去掉旧大小说明。历史 413 异常保留，不重写过去的测试/运行证据。权威规则见 [API 契约](../03-architecture/api-contracts.md#图片输入兼容规则)，输入内存边界见 [威胁模型](../03-architecture/threat-model.md#剩余风险)。

先扩展 `tests/gateway_images.rs` 的 HTTP 行为回归，再修改实现：16 组大文本/大图片、Chat/Responses、JSON/SSE 与定长/分块请求验证完整转发、完成响应及实际字节数持久化；大请求鉴权、JSON、Content-Type 和字段错误仍拒绝且不发生成上游。模拟上游明确关闭其自身的 Axum 默认限制，断言原始大文本/图片内容完整收到；不调用真实模型。新旧测试均未运行，也未执行 clippy、构建、打包或原生窗口验收；用户可选命令见 [测试清单](../04-quality/request-records-checklist.md)。未修改数据库结构、依赖、锁文件或当前运行的程序。

已执行 `cargo fmt --all -- --check`，修正测试文件的两处格式后重新检查通过；按 CRLF 行尾规则执行的差异空白检查通过。上述仅是源码检查，不构成运行验证。

## 在线更新与双平台发版移植（2026-10-13）

用户要求复刻 D:/study/ashell 的在线更新、发版资料与 Actions，明确 Windows x64/macOS arm64 各安装版与便携版，并批准 macOS 钥匙串保护。范围决定见 [ADR 0005](../03-architecture/adr/0005-online-update-release.md)，验收 A-update-01—A-update-05。

源码已加入独立更新服务、迁移0007偏好、清单/API/页面降级、版本/大小/SHA-256验证、安全解压、下载取消重试、设置页与安装确认。安装前停止网关和收尾记录，助手等待旧PID退出再启动；Windows安装器/便携替换、macOS应用/PKG分别处理。macOS使用HOME数据目录、钥匙串密钥和AES-256-GCM密文；本机保护移至blocking worker，需回归验证既有账号续期/密钥读写。没有改变客户端API或正式发布授权边界。

建立平台打包脚本、Windows版本资源与Inno标识、macOS plist/图标生成/签名检查、双平台Actions、四应用产物清单、发版校验和说明提取。保留0.1.0，CHANGELOG、总览和独立详情均标未发布，未创建或推送标签。仅创建流水线，没有触发Actions或Release。

实际源码检查及限制集中在 [v0.1.0验证结果](../upgrade/v0.1.0/README.md#验证结果)。已执行Rust格式、Python/PowerShell/Bash/plist语法、依赖/资料/链接一致性与差异检查；未执行编译、Rust/Python回归、打包、原生UI、钥匙串或真实更新。本机无YAML解析器，Actions语法及runner行为未实测。回归与手工步骤见 [更新验收清单](../04-quality/online-update-checklist.md)。

## v1.0.0 发布与重建验证（2026-10-13）

用户明确要求“开始发第一个版本 v1.0.0”，随后要求排查打包报错并重新构建，授权本次必要测试、两平台打包及正式发布。此前各节中的未运行记录保留为当时事实；本轮双平台 Rust 格式、Clippy、完整测试及四包打包预检均已执行通过，v1.0.0 已于当日正式发布；四包与更新清单的版本、大小、地址和 GitHub SHA-256 一致。失败原因、回归证据、构建来源与实际发布状态集中在 [首版验证记录](../upgrade/v1.0.0/README.md#验证结果)，不以历史成功代替当前结果。

本机依赖编译受 D 盘空间不足影响，Rust 验证使用 GitHub Actions 原生 runner；没有清理工作区、停止或替换当前运行的应用。原生 UI、真实上游及实际安装升级仍按验收清单手工验证。

## v1.1.0 正式发版授权（2026-10-14）

用户要求正式发版并跳过测试，本轮允许原生编译打包、资料和产物核对，跳过 Rust/Python 测试。先前源码阶段的未运行记录保留为当时事实；本轮结果集中在 [v1.1.0 验证记录](../upgrade/v1.1.0/README.md#验证结果)。

## v1.1.1 正式发版授权（2026-10-14）

用户在默认信任字段与诊断代码交付后要求“正式发版跳过测试”，授权本次版本准备、提交推送、新标签和四包发布。测试跳过仅作用于 v1.1.1；格式、Clippy、资料与产物校验仍执行。真实执行证据统一记录在 [v1.1.1 详情](../upgrade/v1.1.1/README.md)，不沿用源码阶段未验证或旧版测试通过记录推断本版状态。

## 双主题页面重构源码交付（2026-10-14）

用户批准四张深色图为夜间，指定浅色桌面参考为白天，再明确要求开始重构。八张预览已保存到 `docs/02-design/previews/`，权威规则见 [视觉系统](../02-design/visual-system.md)。这些图片是此前生图，不是本次程序截图。

将原桌面渲染拆成按页面划分的模块及共用组件。重构六项导航、主题与页头、真实状态/复制反馈、概览、额度/设备码登录抽屉、客户端地址/密钥/指南标签、模型搜索/复制/选择、请求及异常本页搜索/分页/详情时间线；保留原后台命令与凭据、监听、存储、撤销及在线更新边界。初始窗口 1280×820、最小 960×640，窄窗覆盖式抽屉和宽表横向滚动已写入源码。

真实统计替代设计图示例；有凭据不宣称调用验证通过，未知额度/用量不补零。时间线仅用已有字段，恢复确认不伪造真实结束；HTTP 200 后的流失败明确提示。复制明细按面板顺序包含时间线及完整元数据，不读取或保存正文。登录抽屉取消后忽略迟到设备码，后台命令发送失败保留重试入口。

先新增展示规则回归代码，因默认禁止编译型验证，没有运行失败基线或修复后测试。新增 6 项行为回归，并延续主题存储/解析、凭据登录、请求详情和密钥删除用例；扩展已有配色断言。

本轮只执行不编译的格式整理、差异审查、依赖本地源码/API 核对及配色数值检查；没有运行 Rust 测试、Clippy、build/run、真实账号请求或打包，也没有更新、停止或替换现有程序。源码格式和静态检查不证明类型检查、实际布局或交互通过；自动命令和实机项目见 [重构验收清单](../04-quality/desktop-redesign-checklist.md)。

静态核对：`cargo fmt --all -- --check` 通过；两套文字/状态/主按钮/导航配色的数值对比度均至少 4.5；使用的图标已与本机锁定的 GPUI Kit 0.7.3 目录匹配。检查 595 个本地 Markdown 文件/图片链接，无失效路径；非测试桌面源码没有生图示例账号、请求 ID 或设备码。Git 差异检查按 CRLF 行尾规则执行，不修改仓库 Git 配置。

## 用户实机反馈后的页面纠偏（2026-10-14）

用户提供概览运行截图并指出每页与预览相差很大、品牌图标应使用本项目素材。可见问题包括侧栏居中且未铺满、文字/控件偏小、表格尾部留白、操作位置和多余说明偏离预览。上轮只做格式/源码检查，没有发现运行差异，不能据此认定视觉实现完成。

核对锁定 GPUI 源码：`h_flex()` 默认 `items_center()`，无高度侧栏按内容居中；Root 将 `Theme.font_size` 用作 rem，14px 基准叠加 `text_sm` 得到 12.25px。新增共享布局函数显式拉伸，字号基准 16、正文 15/辅助 14/标题 26；按钮/输入统一 36/40px。去除侧栏多余品牌块，窗口沿用既有 ICO，设置内部内嵌本项目 128px PNG；未修改图标图案，也不依赖运行时素材目录。

逐页调整页头、概览地址/指标/表格、账号横向额度与三步登录、接入平面双栏及选择器、请求紧凑时间线/摘要/完整字段展开、模型历史与设置分栏。表头/数据/操作列共用增长比例，普通状态并入一条底栏，错误单独显示。原有后台认证、监听、密钥、数据及更新命令保持；新 UI 状态不新增数据库字段。

先补布局/品牌 3 项回归代码，再修改源码；因禁止默认编译，未运行失败基线或修复后测试。当前只执行 `cargo fmt`、源代码/固定依赖接口核对及差异检查。没有启动界面、构建或更新 EXE，也没有停止当前应用；对照预览、DPI、焦点和所有真实交互仍待 [逐页验收](../04-quality/desktop-redesign-checklist.md)。

## 页面重构编译错误修正（2026-10-14）

用户执行 `cargo run` 后报告 `Size` 通配导入歧义与模型状态标签生命周期错误。输入组件显式使用 `gpui_kit::component::Size` 的别名，避免与 GPUI 几何尺寸重名；`test_status` 仅返回字符串字面量，返回类型改为 `&'static str`，避免列表文字借用桌面状态。

先补状态字符串释放后标签仍有效的回归代码，再修正返回类型；覆盖四种已知状态和未知状态。按默认限制未运行测试或重新编译；`cargo fmt --all -- --check` 与按 CRLF 规则的差异空白检查通过，不代表编译通过。回归命令已加入 [重构验收清单](../04-quality/desktop-redesign-checklist.md)。

## 页头宽度与说明文字精简（2026-10-14）

用户实机截图显示标题/副标题被挤成竖列，整个页头撑高；随后要求去掉非必要说明文字。原页头的文字列只有最小宽度归零，没有分配剩余空间。本次改为单行标题并明确 flex 增长、禁止换行；页头和纵向滚动内容显式约束宽度，滚动内容不沿高度压缩。先添加覆盖六个页面名称的标题宽度/不换行约束回归代码，未运行失败基线或修复后测试。

六页副标题、概览重复监听信息/额外指南卡、接入与设置重复说明、常驻搜索/隐私说明和底栏长状态文本已删减；查询调度与额外额度字段在详情中查看，指南集中于接入指南标签。错误、授权进度、撤销/安装确认和复制反馈保留，短操作提醒使用对应按钮 tooltip。界面规则以 [视觉系统](../02-design/visual-system.md#页头与文字精简2026-10-14) 为准。

按项目默认限制仅修改源码、回归代码和文档，未编译、运行测试或启动应用，未停止/替换现有程序。格式与差异检查不能代表实际布局通过；宽度/DPI/抽屉和所有页面仍待 [验收](../04-quality/desktop-redesign-checklist.md)。

本轮 `cargo fmt --all -- --check`、按 CRLF 规则的差异空白检查和所改 Rust 文件的行尾检查通过；静态确认六页均使用共用页头，已删除的副标题/重复说明未残留。上述均为源码检查，不是类型检查或窗口运行证据。

## 全页按钮与基础操作核查（2026-10-14）

用户指出绿色实心按钮突兀，概览只能停止却不能启动；随后要求统一检查所有页面按钮和交互。据此修订 A-theme-05，追加 A-ui-05/06。白天动作改为白底细边框、灰色 hover/active，设置选项使用中性深色；绿色仅用于状态/额度。夜间沿用单色主动作，所有页面共用按钮尺寸与工厂，未修改本项目图标。权威规则见 [视觉系统](../02-design/visual-system.md#按钮与基础交互统一2026-10-14)。

概览、账号停止入口与接入页共用真实状态的启动/停止控件；启动使用已载入的当前监听配置，空值回退到原有 loopback 默认。缺账号/密钥指向对应配置入口，端口错误仍由既有后台校验并可重试；模型测试期间仍可停止监听。后台的幂等停止命令现在也发送明确停止状态，不改变端口保存或在途请求收尾规则。

逐页核对源码后增加本地等待回执处理：启停、密钥操作、账号切换、额度、模型、目标分页和更新操作在入队成功后防重复提交，失败回到可重试状态。共用错误事件没有操作 ID，沿用其原有全局错误恢复边界，不据此宣称实现了新的后台请求关联协议。密钥撤销等待提交快照、账号切换匹配活动账号、分页匹配目标页，不能由无关状态消息冒充完成。

账号关闭/取消/Escape 共用流程，删除重复浏览器按钮，过期设备码取消等待；模型刷新与指定测试有等待标签。密钥名校验与存储相同，选择器互斥，切页/隐藏/Escape 清理明文展示意图，迟到回执不再重新显示密钥；复制仍仅响应已发出的复制动作。请求详情保留加载/关闭/迟到隔离与复制。设置补更新回执等待、确认/Escape，确认安装后进入等待画面，失败恢复；全页错误提示可关闭，不删除记录。底层 `UpdateStatus::available()` 本来就排除了下载中和已就绪状态，本轮不是修复其可见性，而是完善回执前的等待与防重复点击。

新增纯展示/等待状态/配色回归代码，并在 `tests/gateway_startup.rs` 补同进程连续停止两次后按原地址重新启动的行为用例。未运行这些测试、Clippy、编译、构建、打包或桌面实机检查，未停止/替换正在使用的应用；完整逐页矩阵和建议命令见 [重构验收](../04-quality/desktop-redesign-checklist.md)。

源码审查另核对后台分页会在记录清理后归一到有效页；等待状态接受按新记录总数归一后的目标页，补充回归代码，避免分页按钮因旧目标页不存在而一直禁用。本轮 `cargo fmt --all -- --check`、按 CRLF 规则的差异检查和改动文件行尾检查通过；189 个相关本地 Markdown 链接可解析。静态计算白天按钮普通/hover/active 的最小文字对比度 12.27:1；这些检查不构成编译、测试或实机证据。

## 新首页统计预览落地（2026-10-14）

用户批准新首页图并要求开始重构，追加 A-ui-07—A-ui-09。布局、数据口径与批准图片统一维护在[首页统计设计](../02-design/overview-statistics.md)，替代旧概览布局；品牌、其他页面与原有认证/监听/隐私边界保持既有规则。

新增 `src/overview.rs` 纯统计与 `src/storage/overview.rs` 全区间查询，后台 `LoadOverview` 使用实际 SQLite 元数据及在途记录，按 ID 去重；不依赖已加载的100条诊断列表。统计只包含两个生成端点，按开始时间筛选；完整结果、取消、失败/中断和进行中分开。用量缺失不补零，部分用量展示下限；首个输出只采用真实流式观察时间，P50/P95 按最近秩计算。沿用7天/10000条已结束请求的本地保留规则，没有数据库迁移或新依赖。

桌面新增原生柱图、半圆成功率、四项统计与最近四条；顶部时间筛选、手动/首页5秒刷新使用带 ID/时段的独立回执。切换时段清空旧数字、拒绝迟到结果，同一时段失败保留上次结果并可重试，命令入队失败解除等待，统计错误不清除其他操作状态。布局初始尺寸为1280×850，窄窗保留滚动；Tooltip 显示口径、样本和分桶计数。

启停沿用共用控件，补已运行服务再次收到启动命令时也回传真实监听状态，避免窗口落后的点击一直等待；重复停止仍回传停止状态。先补统计行为和状态回归代码，再实现查询/展示；另补后台筛选刷新与启动幂等回执回归。按根 AGENTS 默认限制，未运行失败基线、修复后测试、类型检查、编译、Clippy、应用或打包，未停止或替换现有程序；建议命令与全部待验场景见[桌面清单](../04-quality/desktop-redesign-checklist.md)。

本轮非编译检查：`cargo fmt --all -- --check`、按 CRLF 规则的差异空白检查通过；36个变更文本文件的行尾检查通过（保留 Markdown 有意的双空格换行），300个相关本地文档/图片链接可解析，批准预览副本的 SHA-256 与原图相同。逐项对照已安装 GPUI Kit 0.7.3 / GPUI 0.3.9 的 Canvas、PathBuilder、Tooltip 和尺寸转换公开接口，修正私有 Pixels 字段访问及 Tooltip 导入；这是源码接口核对，不构成 Rust 类型检查或桌面视觉验证。

## 缓存统计、分组菜单与首页精简（2026-10-14）

用户再次批准生成的缓存仪表与分组菜单图，明确要求开始修改，真实缓存数据、首页删除最近请求、布局按最新图；因此修订首页设计并追加 A-ui-10—A-ui-12。新批准图保存在 `docs/02-design/previews/overview-cache-grouped-light.png`，上一版图片保留追溯，权威说明仍为[首页统计](../02-design/overview-statistics.md)。

新增0009可空缓存字段，Journal 兼容 Responses `input_tokens_details.cached_tokens` 与转换后 Chat `prompt_tokens_details.cached_tokens`，通过原检查点/结束事务保存。聚合只用有效成对样本的缓存/输入合计计算加权命中率，旧数据、缺失或非法数据不补零；零缓存有效，零分母/无样本/溢出为未知，部分覆盖在仪表旁标明样本数。缓存读取显示在 Token 栏，属于输入子集，不重复累加。请求详情及复制可查看缓存元数据。

首页移除最近请求与查看全部，趋势绘图区改为228px、统计条182px，半圆改为缓存命中率，成功率并入请求数；保留时间筛选、独立刷新回执、服务启停与地址操作。新增导航模块按批准分组呈现原六页，页面 ID 保持不变，底部使用本项目内嵌品牌和 Cargo 版本；初始布局尺寸恢复1280×820，最小960×640，长值省略可悬停，窄窗保留滚动。

先补缓存统计/旧库/终态持久化回归，再实现采集与聚合；另补两个端点×两种响应模式×七种缓存明细的28个 fixture 场景及结果聚合、菜单映射、格式和复制回归。为兼容新增迁移，原数据库启动测试继续逐项比较历史校验值，并显式验证新增迁移的规范校验值，未移除校验保护。

已在 Python 标准库 SQLite 内存库检查全量0001—0009新建，以及0008→0009保留原请求字段、缓存旧行NULL、零值可写和外键完整性；未打开或迁移用户真实数据库。按根 AGENTS 默认限制，未运行 Rust 测试/SQLx回归、Clippy、编译、应用、打包，未停止或替换现有程序，失败基线与实机视觉均待执行。建议命令见[桌面清单](../04-quality/desktop-redesign-checklist.md)。

本轮格式与静态检查：`cargo fmt --all -- --check`、按CRLF规则的差异空白检查通过；44个变更文本文件的行尾检查通过（保留Markdown双空格换行），325个相关本地文档/图片链接可解析。批准预览副本SHA-256一致，首页源码未残留最近请求/查看全部组件，旧SQL迁移文件未改动。以上不替代Rust类型检查、SQLx测试或实机截图。

## 侧栏拆分后的底栏主题变量修正（2026-10-14）

用户执行 `cargo run` 报告 `shell.rs` 的 `dark` 未定义（E0425）。侧栏抽取到独立模块时移走了原声明，但底栏仍引用该局部变量；导航模块中的同名变量不在底栏作用域。现已在底栏构造前恢复按当前实际配色判断夜间模式的声明，并核对两处引用各自的作用域，不改动主题保存或布局。

本次仅一行源码修正，用户提供的编译输出为失败证据；格式与差异检查不能作为重新编译通过的证据。按根AGENTS默认限制未运行编译、测试或应用，后续需重新执行 `cargo run --locked`，并检查白天/夜间切换后的底栏文字。

## 自定义监听端口（2026-10-14）

用户要求端口可以自定义以避开占用。客户端接入的连接地址页将原单一地址输入拆为监听 IP 与端口，共用概览/接入的启停操作；端口接受1–65535，空值使用原默认8787，修改端口不改IP。服务运行及启停等待时禁用输入，停止后可以编辑。

后台按系统 `AddrInUse` 错误显示具体占用端口和修改入口，其他错误保留实际原因。成功回执按实际地址回填两项输入，连接URL仍来自真实监听；继续沿用现有成功监听后的SQLite保存与自动恢复，失败不覆盖历史配置，无新增迁移。

先补展示规则与后台占用后换端口/重启恢复回归代码，再修改实现；保留已有失败重试、连续启停和默认地址用例。场景、建议命令和桌面检查见 [自定义端口清单](../04-quality/custom-port-checklist.md)，对应 A-port-01—A-port-03。

已执行 `cargo fmt --all`、`cargo fmt --all -- --check` 和按CRLF规则的差异空白检查。未运行失败基线、Rust测试、Clippy、编译、桌面应用或打包，未停止或替换已有EXE；格式和源码审查不代表端口行为已实测通过。
- 本次 v1.2.0 已正式发布（2026-10-19）：用户要求“开始发版，跳过测试”，授权本版本准备、提交推送、构建打包和新标签正式发布。Rust/Python 测试跳过；标签 Native checks、Release 工作流及六个公开文件校验通过，详情见 [v1.2.0 记录](../upgrade/v1.2.0/README.md)。
