![Tiny Proxy](assets/icons/png/tiny-proxy-128.png)

# Tiny Proxy

**连接你的 Codex 账号、个人设备与 AI Agent。**

![阶段：开发版](https://img.shields.io/badge/status-development-blue)
![平台：Windows x64](https://img.shields.io/badge/platform-Windows_x64-0078D4)
![语言：Rust 1.96.0](https://img.shields.io/badge/Rust-1.96.0-orange)
![API：兼容子集](https://img.shields.io/badge/API-compatible_subset-2E8555)

[快速开始](#-快速开始) · [客户端接入](#-客户端接入) · [API 兼容](#-api-兼容范围) · [当前进度](#-当前进度) · [文档](#-文档)

Tiny Proxy 是供个人在不同设备和 Agent 中使用的原生桌面 Codex 模型网关。通过设备码登录账号，在本机提供带独立密钥的模型 API，供局域网或个人 VPN 内的客户端调用，无需预装 Codex。

> 当前为 **Codex 网关开发版**，提供明确的 API 兼容子集。真实账号登录、模型目录和一次完整文本调用已验证；其余联调进度见下方「当前进度」。

---

## ✨ 特性

🖥️ **原生桌面界面** — 使用 GPUI + GPUI Kit，集中管理账号、模型、客户端接入和请求记录。

🔑 **独立设备码登录** — 在浏览器完成授权，支持多账号保存、手动切换和请求前自动续期。

🤖 **统一模型接口** — 提供模型目录、Chat Completions 与 Responses 子集，支持 SSE 和聚合 JSON。

🌐 **个人设备接入** — 为不同客户端创建独立密钥，使用具体的本机、LAN 或 VPN 地址监听。

📊 **账号状态持久化** — 按账号保存模型目录、文本测试结果与额度快照，运行期间每小时只读查询额度。

🔒 **本地凭据保护** — OAuth 凭据与新网关密钥使用 Windows 用户级 DPAPI；外部客户端只持有网关密钥。

🔄 **在线更新与双平台发版** — 已移植 ashell 的检查、校验、下载与安装重启流程；发布配置仅 Windows x64 和 macOS arm64，各有安装包与便携包。v1.1.1 已完成两平台原生构建与四包发布，按要求跳过本版测试，实际安装升级仍待验。

## 🌻 为什么做 Tiny Proxy

在不同设备和 Agent 之间使用同一套个人模型入口，需要把账号登录、模型选择和客户端配置放在一起管理。

Tiny Proxy 在桌面上管理这些状态，由内嵌网关完成请求转换与转发。客户端负责执行工具，应用负责账号连接和模型 API；访问范围限定为局域网与个人 VPN。

```text
个人设备 / Agent  →  Tiny Proxy  →  Codex 上游
     网关密钥          本机 OAuth 凭据
```

## 🚀 快速开始

### 启动应用

源码运行环境：**Windows x64、Rust 1.96.0、Visual Studio C++ Build Tools**。

在项目根目录执行：

```powershell
cargo run --locked
```

已有构建时，也可打开 `target/debug/tiny-proxy.exe`。当前尚无安装包；`desktop_probe.exe` 仅用于早期最小窗口探测。源码修改不会自动更新已有 EXE，具体构建与验证状态见 [开发验证记录](docs/06-delivery/implementation-status.md)。

### 完成首次配置

1. **账号连接**：选择 Codex 设备码登录，复制设备码，在浏览器完成授权。
2. **模型**：读取当前账号的模型目录，复制所需模型的准确 ID；可执行文本测试，该操作会使用账号额度。
3. **客户端接入**：创建独立网关密钥。新密钥可反复查看、复制，重启后仍可取回。
4. **启动网关**：在“客户端接入 → 连接地址”分别填写监听 IP 和端口（1–65535）。留空使用 `127.0.0.1:8787`；端口可自定义，跨设备填写本机 LAN/VPN 地址。
5. **配置客户端**：填写应用显示的 Base URL、网关密钥和模型 ID，先尝试一次文本调用。

### 后续启动

已有本地账号和有效客户端密钥时，应用打开后自动监听上次成功保存的地址；无历史地址时使用 `127.0.0.1:8787`。

- 修改监听 IP、端口或切换账号前，先停止网关；启动成功后保存新配置，下次打开自动恢复。
- 端口占用时可在“客户端接入 → 连接地址”修改端口后重试，并将新的 Base URL 更新到客户端。独立端口输入和占用提示目前仅修改源码，未重新构建或运行验证，见 [自定义端口清单](docs/04-quality/custom-port-checklist.md)。
- 手动停止只影响本次运行；Windows 关闭窗口默认收起托盘并继续服务，托盘或设置中主动退出才停止网关。此改动目前仅在源码中，现有 EXE 需重新构建才生效，见 [待测清单](docs/04-quality/desktop-lifecycle-checklist.md)。
- 应用不自动修改防火墙，不启用公网或通配监听。

## 🔌 客户端接入

以支持 OpenAI Chat Completions 的客户端为例：

| 配置项 | 填写内容 |
|---|---|
| 接口格式 | OpenAI Chat Completions |
| Base URL | 应用显示的地址，本机默认为 `http://127.0.0.1:8787/v1` |
| API Key | Tiny Proxy 创建的独立网关密钥 |
| 模型 ID | 从当前账号模型目录复制的准确 ID |

跨设备使用具体的 LAN/VPN 地址。若客户端要求完整请求 URL，填写应用展示的 `/v1/chat/completions` 地址，避免重复拼接 `/v1`。

Trae 的详细配置、错误码和排障步骤见 [Trae 接入指南](docs/05-operations/trae-connection.md)。真实 Trae 版本、工具往返和第二台设备仍待联调。

## 🧩 API 兼容范围

### 接口

| 方法 | 路径 | 支持内容 |
|---|---|---|
| `GET` | `/v1/models` | 当前账号的模型目录 |
| `POST` | `/v1/chat/completions` | 文本、用户 Base64 图片、函数工具调用与结果历史；SSE / 聚合 JSON |
| `POST` | `/v1/responses` | 文本、用户 Base64 图片、函数工具与推理回放子集；SSE / 聚合 JSON |

所有路由均要求：

```http
Authorization: Bearer YOUR_GATEWAY_KEY
```

OAuth Token 仅由本机用于固定 Codex 上游。外部客户端不能管理账号、监听设置或读取凭据，工具由客户端执行。

### 兼容规则

| 场景 | 当前处理 |
|---|---|
| 图片输入 | 仅支持 `user` 消息中的 Base64 PNG / JPEG / WebP / GIF，保留图文顺序与 `detail`；不下载图片网址 |
| 输出长度 | 三种长度字段的任意非空值接收后跳过，不限制上游输出；响应头固定提示 |
| Chat 推理历史 | reasoning_content 任意类型接收后跳过，不拼入正文；assistant 非空值固定提示未回放 |
| 工具与空值 | 默认信任历史/元数据，不检查 index、冗余 name、配对或 schema 枚举；转换仍需要消息结构与调用 ID/函数名 |
| 字段信任与边界 | 未知字段与采样参数默认接收；顶层用不到的字段跳过，原生值由上游判断；保留 JSON 与必要协议/图片转换要求 |

2026-10-14 用户要求默认信任下游字段，v1.1.1 按上述规则放松，构建/发布与跳过测试的实际状态见 [版本详情](docs/upgrade/v1.1.1/README.md)。字段定义、响应头与错误契约以 [API 契约](docs/03-architecture/api-contracts.md) 为准。图片转换已有自动测试，真实 Codex 图片调用及客户端重试尚待验证。

### 运行限制

| 项目 | 默认值 |
|---|---|
| 请求体大小 | 不设本地上限；上游规则仍适用 |
| 并发生成请求 | 最多 4 个 |
| 响应大小 | 16 MiB |
| SSE 单事件 | 1 MiB |
| 上游空闲等待 | 90 秒 |
| 慢消费者等待 | 30 秒 |

撤销密钥会拒绝新请求、取消该密钥的在途请求，并删除整条密钥记录及明文预览。重启后不再显示，旧值返回 `401 invalid_gateway_key`；新版启动时清理历史已撤销密钥，有效密钥保留。

## 💾 本地数据与安全

### 数据保存

默认数据目录为 Windows `%USERPROFILE%/.tiny-proxy`、macOS `~/.tiny-proxy`，开发隔离可设置 `TINY_PROXY_DATA_DIR`。

首次使用新默认目录时，可从旧 `AppData/TinyProxy` 或打包启动器的 `LocalCache` 恢复唯一一份有数据的旧库，并保留原库；目标库存在后不再迁移。PowerShell、资源管理器和 Codex 启动使用同一份默认数据，详见 [数据目录与恢复](docs/05-operations/desktop-operations.md#数据目录统一与历史恢复)。

账号各自的模型目录、显式文本测试结果与时间、额度快照保存在 SQLite 中。应用运行时每小时查询各账号额度，也可手动刷新；关闭期间不查询，重开补查到期账号。

### 凭据与隐私

- Codex 凭据与新网关密钥使用 Windows 用户级 DPAPI；网关密钥另存摘要用于鉴权。
- macOS 已补齐钥匙串/AES-256-GCM 保护代码；钥匙串保存加密密钥，SQLite 只保存凭据密文，macOS 原生 CI 的凭据、账号与续期回归已通过，原生界面首启和真实账号登录仍待验。仅复制数据库不能跨平台或跨用户恢复凭据，见 [平台决定](docs/03-architecture/adr/0005-online-update-release.md)。
- 旧版只保存摘要的密钥仍有效，但无法还原明文。
- 请求诊断只保存元数据，不保存提示词、代码、回答、图片、工具参数或认证信息。

### 请求诊断

请求元数据持久化与独立「失败异常」分页视图已加入代码，可查看起止时间、首个输出、请求大小、消息与工具数量、HTTP 状态和用量。

已结束记录按 **7 天或最近 10,000 条** 保留，运行中的记录不清理。异常退出后标记为中断，保留最近已观察耗时，恢复时间不作为真实结束时间。

> 此部分最终代码与桌面界面尚待验证，当前运行的旧版不会自动获得这些改动。使用说明见 [请求诊断](docs/05-operations/request-diagnostics.md)，验收见 [测试清单](docs/04-quality/request-records-checklist.md)。

## 🚧 当前进度

| 状态 | 范围 |
|---|---|
| 已有真实验证 | Codex 账号登录、模型目录、一次完整文本调用 |
| 尚待真实联调 | Codex 图片、完整工具往返、实际 Trae 版本、第二台设备 |
| 最终代码待验证 | 请求元数据持久化、失败异常分页与详情界面；请求体上限取消；在线更新、双平台打包流水线与 macOS 凭据保护 |
| 源码已修改，尚未运行验证 | Windows 启动无控制台、关闭窗口入托盘、恢复及显式退出 |
| 尚未实现 | 远端撤销、macOS 托盘、开机自启、其余完整设置 |

完整实现范围和历史检查证据见 [开发验证记录](docs/06-delivery/implementation-status.md)。当前不宣称完整 OpenAI 兼容，也不将全部产品原型视为已实现页面。

## 外观主题

2026-10-14 已按确认的八张预览重构六个页面的源码：夜间保留深色，白天采用浅灰背景、白色侧栏和绿色状态强调；加入表格、设备码/请求详情抽屉、客户端接入标签及本页搜索。本轮未编译、运行或更新 EXE，当前安装版不会自动获得这些界面；设计依据与待验事项见 [视觉系统](docs/02-design/visual-system.md)、[页面重构清单](docs/04-quality/desktop-redesign-checklist.md)。

设置 → 外观主题提供“跟随电脑 / 白天模式 / 夜间模式”。默认跟随电脑的应用主题，手动选择会本地保存并在重启后恢复；跟随模式响应运行期间的系统外观变化。首版两平台编译与主题回归测试已通过，桌面主题实机检查仍待验；下载新版后手动安装。待验场景与命令见 [主题验证清单](docs/04-quality/theme-checklist.md)。

## 🛠️ 开发与验证

设置 → 在线更新，支持启动/定时/关闭自动检查、更新说明、下载进度与取消重试。只有手动确认才安装并重启。GitHub 手动工作流只上传产物，版本标签才发布 Release；详见 [构建与更新](docs/05-operations/build-and-package.md)、[发版流程](docs/06-delivery/release-process.md)和 [版本总览](docs/upgrade/README.md)。[v1.1.1 已正式发布，可直接下载](https://github.com/ynx-official/tiny-proxy/releases/tag/v1.1.1)，包含 Windows x64 的安装 EXE/便携 ZIP 和 macOS arm64 的安装 PKG/便携 ZIP。四包下载后的 SHA-256 与清单、GitHub 记录均一致；本版按要求跳过测试，实际结果见 [v1.1.1 验证记录](docs/upgrade/v1.1.1/README.md#验证结果)。

技术栈：**Rust · GPUI · GPUI Kit · Tokio · Axum · SQLite**。采用单进程、单 Cargo package 内部分模块，UI 通过应用服务访问网络与存储。

按 [项目约定](AGENTS.md)，默认交付代码与测试清单；只有用户明确要求时才执行相应的编译型验证、构建或打包。以下命令供选择，不是每次修改后的自动步骤。

```powershell
# 源码格式检查，不触发编译
cargo fmt --all -- --check

# 以下命令会触发编译，按明确请求执行
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --locked
```

## 📚 文档

| 想了解什么 | 阅读入口 |
|---|---|
| 全部文档 | [文档索引](docs/README.md) |
| 产品范围与验收 | [PRD](docs/01-requirements/prd.md) · [MVP 验收](docs/01-requirements/acceptance/mvp.md) |
| 页面与交互 | [桌面流程](docs/02-design/desktop-flows.md) · [全页面 ASCII 原型](docs/02-design/ascii-prototype.md) |
| 架构与技术选择 | [架构概览](docs/03-architecture/overview.md) · [Codex 路线决定](docs/03-architecture/adr/0003-codex-device-gateway.md) |
| API 与协议 | [API 契约](docs/03-architecture/api-contracts.md) · [流式协议](docs/03-architecture/streaming-protocol.md) |
| 运行与接入 | [桌面运行与恢复](docs/05-operations/desktop-operations.md) · [Trae 接入](docs/05-operations/trae-connection.md) |
| 请求排障 | [请求诊断](docs/05-operations/request-diagnostics.md) · [测试清单](docs/04-quality/request-records-checklist.md) |
| 开发进度 | [实施计划](docs/06-delivery/development-plan.md) · [验证记录](docs/06-delivery/implementation-status.md) |
| 更新与打包 | [双平台操作](docs/05-operations/build-and-package.md) · [更新验收](docs/04-quality/online-update-checklist.md) |
| 版本与发版 | [流程规范](docs/06-delivery/release-process.md) · [版本总览](docs/upgrade/README.md) · [CHANGELOG](CHANGELOG.md) |

---

文档状态：Review · 最后更新：2026-10-14 · 排版参考：[radioactive-state](https://github.com/MananTank/radioactive-state)
