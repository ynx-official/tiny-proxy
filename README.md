# Tiny Proxy

原生桌面 Codex 模型网关，供本人在不同设备和 Agent 中使用。技术栈：GPUI + GPUI Kit + Tokio + Axum + SQLite。

当前是 **Codex 网关开发版**：按 cursor-byok-main 的 Codex 设备码认证、模型目录和 Responses 上游接入，提供带独立密钥的标准模型 API。已验证真实账号登录、模型目录及一次完整文本调用；真实工具往返、Trae 版本及第二台设备尚待联调，不宣称完整 OpenAI 兼容或全部产品原型已实现。

- [文档索引](docs/README.md)
- [Codex 路线决定](docs/03-architecture/adr/0003-codex-device-gateway.md)
- [当前验证记录](docs/06-delivery/implementation-status.md)
- [Trae 接入步骤与排障](docs/05-operations/trae-connection.md)

## 启动

Windows x64，Rust 1.96.0，Visual Studio C++ Build Tools：

```powershell
cargo run
```

也可打开 target/debug/tiny-proxy.exe。desktop_probe.exe 仅是早期最小窗口探测。

1. 账号连接 → 使用 Codex 设备码登录 → 复制设备码，在浏览器授权。
2. 模型 → 读取目录 → 测试文本（会使用当前账号额度）。
3. 客户端接入 → 创建独立密钥；新密钥可随时查看或复制，重启后仍可取回。
4. 首次配置时输入这台电脑的具体 IP:端口并启动。留空默认 127.0.0.1:8787；跨设备应填写本机 LAN/VPN 地址。成功监听后保存地址，下次打开应用自动启动代理。
5. 在 Trae 填写应用显示的 Base URL、网关密钥、模型目录中的准确 ID。

应用启动时，在本地账号和有效客户端密钥齐备后自动监听；无历史地址时默认 127.0.0.1:8787。端口占用或地址不可用会显示错误，可在客户端接入页修改并重试。手动停止只影响本次运行；关闭窗口停止网关并退出，下次启动仍自动监听。不自动修改防火墙，不启用公网/通配监听。

## 已实现的 API 子集

- GET /v1/models
- POST /v1/chat/completions：文本、用户 Base64 图片、函数工具调用和结果历史，SSE/聚合 JSON。
- POST /v1/responses：文本/用户 Base64 图片/函数/推理回放子集，SSE/聚合 JSON。

所有路由要求 Authorization: Bearer <网关密钥>。OAuth Token 仅由本机用于固定 Codex 上游，工具由外部 Agent 执行。Chat 的 max_tokens/max_completion_tokens 和 Responses 的 max_output_tokens 接受正整数或 null；正整数仅作兼容接收，不传给 Codex、不执行输出上限，并通过响应头明确提示。图片仅支持 user 消息的 Base64 PNG/JPEG/WebP/GIF，保留图文顺序与 detail；不下载图片网址，请求体仍限 2 MiB。未知参数、非用户消息图片、托管工具、temperature 等未支持字段的非空值仍明确报错，见 [API 契约](docs/03-architecture/api-contracts.md)。图片转换已有自动测试，真实 Codex 图片调用和客户端重试尚待验证。

默认最多 4 个并发请求、请求体 2 MiB、响应 16 MiB、SSE 单事件 1 MiB、上游空闲 90 秒、慢消费者等待 30 秒。撤销客户端密钥会拒绝新请求并取消该密钥的在途请求。

撤销成功后删除整条 Key 记录，不再显示在列表或明文预览中，重启后也不再出现；新版启动时清理历史已撤销 Key，保留有效 Key。删除后的旧值返回 401 invalid_gateway_key。

Chat assistant 历史消息兼容 `reasoning_content` 字符串/null；推理文本不回放给 Codex，非空值通过响应头提示。正文与工具历史保留，Responses 原生推理回放按原规则处理，详见 [兼容规则](docs/03-architecture/api-contracts.md#chat-推理历史兼容规则)。

兼容客户端合并流式工具后保留的 `tool_calls[].index`、匹配的工具结果 `name`、assistant 拒绝信息及已列明的可选 null 占位；无参数工具可省略 parameters。采用显式规范化后重建上游请求，未知字段与非空未支持参数仍拒绝，详见 [客户端历史与空值规则](docs/03-architecture/api-contracts.md#客户端历史与空值兼容规则)。

## 数据与边界

默认目录 %USERPROFILE%/.tiny-proxy；开发隔离可设置 TINY_PROXY_DATA_DIR。默认首次启动会从旧 AppData/TinyProxy 或打包启动器的 LocalCache 中恢复唯一一份有数据的旧库，保留原库；新库存在后不再迁移。这样从 PowerShell、资源管理器和 Codex 启动都使用同一份数据，详见 [数据目录与恢复](docs/05-operations/desktop-operations.md#数据目录统一与历史恢复)。Codex 凭据与新网关密钥使用 Windows 用户级 DPAPI；密钥另存摘要用于鉴权。旧摘要密钥仍有效，但无法还原明文。

支持多账号保存及手动切换、请求前自动续期；账号各自的模型目录、显式文本测试结果/时间、额度快照持久化。应用运行时每小时查询各账号额度，也可手动刷新；关闭期间不查询，重开补查到期账号。运行网关时先停止再切换账号。远端撤销、托盘、自启、安装包和完整设置尚未实现。最近 100 条生成请求只存内存元数据，不记录提示词、回答、工具参数。

## 验证命令

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --locked
```
