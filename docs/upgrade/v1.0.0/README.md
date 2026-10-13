# Tiny Proxy v1.0.0

> 发布日期：2026-10-13
> 状态：Approved（已发布）
> 最后更新：2026-10-13
> 关联文档：[使用与打包](https://github.com/ynx-official/tiny-proxy/blob/v1.0.0/docs/05-operations/build-and-package.md)、[API 契约](https://github.com/ynx-official/tiny-proxy/blob/v1.0.0/docs/03-architecture/api-contracts.md)、[验证记录](https://github.com/ynx-official/tiny-proxy/blob/main/docs/upgrade/v1.0.0/README.md#验证结果)、[更新验收](https://github.com/ynx-official/tiny-proxy/blob/v1.0.0/docs/04-quality/online-update-checklist.md)

## 版本概述

Tiny Proxy 首个正式版本，为个人设备和 AI Agent 提供原生桌面 Codex 模型网关。应用内完成设备码登录、管理账号与客户端密钥，在本机、局域网或个人 VPN 内提供明确兼容子集的 Chat Completions 和 Responses API。发布平台限定 Windows x64 与 macOS arm64，各提供安装包和便携包。

## 账号与客户端接入

- 独立设备码授权，无需预装 Codex；多账号保存、手动切换、请求前自动续期。
- 按账号保存模型目录、文本测试和额度快照；应用运行期间每小时只读刷新额度。
- 创建、查看、复制独立网关密钥，撤销后删除记录并取消对应在途请求。
- 启动时按上次成功的具体监听地址自动恢复网关；关闭窗口停止监听并退出。

## 模型 API 与诊断

- GET /v1/models、POST /v1/chat/completions、POST /v1/responses，支持 SSE 与聚合 JSON。
- 文本、用户 Base64 图片、函数工具调用和客户端执行的工具结果回放；不下载远程图片或运行 Agent 工具。
- 显式处理已列明的历史 index、reasoning_content、可选 null 和输出长度参数；未知非空参数仍按契约拒绝。
- 请求元数据与失败异常独立分页，可复制详情；不保存提示词、回答、图片、代码、工具参数或认证信息。
- 请求体不设置本地大小上限；并发、响应、事件和超时仍受既有规则限制。

## 桌面与在线更新

- 设置提供跟随系统、白天、夜间主题，选择重启后恢复。
- 检查最新正式版、显示更新说明、下载进度、取消和重试；安装前明确确认停止网关与在途请求。
- 支持启动、定时或关闭自动检查，以及新版本提醒；自动检查不会静默下载安装。
- 更新清单优先，GitHub API 与页面作为可用性降级；匹配平台/安装方式并验证大小和 SHA-256。

## 行为与界面变化

首版整合账号、模型、客户端接入、请求记录和设置。正常启动沿用具体监听地址，缺账号或有效密钥时不监听，错误可在客户端接入页重试。主题和更新策略本地持久化。桌面通过后台服务处理网络、存储和更新，外部客户端无法管理账号或执行安装。

## 配置与数据兼容性

应用版本统一为 1.0.0，保留开发阶段 SQLite 迁移 0001–0008。Windows 默认 %USERPROFILE%/.tiny-proxy，支持既有旧库恢复；macOS 默认 ~/.tiny-proxy，可用 TINY_PROXY_DATA_DIR 隔离。Windows 凭据采用 DPAPI；macOS 钥匙串保存本机 AES-256 密钥，数据库只保存 AES-256-GCM 凭据密文。仅复制数据库不能跨用户、跨设备或跨平台恢复认证。

记录保留规则为七天或最近 10,000 条已结束记录，运行中不清理；重启确认中断时间不冒充真实结束时间。升级开发版前备份数据库，回退前保留备份，不删除钥匙串条目排障。

## 升级说明

这是首个正式发布，无此前正式版本。Windows 可选择当前用户安装 EXE 或解压便携 ZIP；macOS 选择 PKG 安装至 /Applications，或保留完整 TinyProxy.app 便携应用。替换开发版前完全退出旧进程，启动后确认账号和监听地址。应用内更新需手动下载并确认安装重启。

## 破坏性变更与已知问题

- 不宣称完整 OpenAI API 兼容，工具由外部客户端执行；长度参数等限制见 API 契约。
- 没有设置请求体大小上限，超大输入可能增加本机内存消耗；上游仍可能拒绝请求。
- 尚未配置 Windows 代码签名、Developer ID 或 Apple 公证，系统可能提示批准首启；macOS 安装器可能请求管理员权限。
- 云端构建成功不等于原生界面、真实 Codex 图片/工具往返、Trae、第二台设备或实际升级已经验证；这些手工联调保持待验。
- 托盘、开机自启及远端账号撤销尚未实现，关闭窗口即退出。

## 验证结果

2026-10-13 18:57:12（UTC+8）首版正式发布：[GitHub Release v1.0.0](https://github.com/ynx-official/tiny-proxy/releases/tag/v1.0.0)。API 已确认非 Draft、非 prerelease，且为最新正式版。标签提交为 4360bb732925c179953ee59ab9aad58adf35e54d。

标签 [发布流水线 38385905189](https://github.com/ynx-official/tiny-proxy/actions/runs/38385905189) 的资料、两平台编译、打包、上传、清单与正式发布全部成功。标签 [原生检查 38385905602](https://github.com/ynx-official/tiny-proxy/actions/runs/38385905602) 的格式、Clippy 和两平台完整测试均通过；其额外 release 构建还在收尾。

| 已执行验证 | 结果与证据 |
|---|---|
| Python 发布工具 | 本机及两平台 CI 共 15 项通过，包含中文输出编码与 macOS 完整脚本 stdout 回归。 |
| Rust 格式 / Clippy / 完整测试 | 提交 0b79543 在两平台的对应步骤全部通过，见 [原生检查 38384871793](https://github.com/ynx-official/tiny-proxy/actions/runs/38384871793)；包含更新、凭据、续期、密钥删除、主题、记录、取消及大请求真实 HTTP 回归。 |
| release 编译与四包 | 提交 1df005e 的 Windows x64 EXE/ZIP、macOS arm64 PKG/ZIP 均成功并上传，见 [打包预检 38383610395](https://github.com/ynx-official/tiny-proxy/actions/runs/38383610395)。版本、原生架构、Windows ProductVersion、macOS 应用结构与签名检查通过。 |

已核对 1df005e 到 0b79543 的生产代码、依赖、构建和打包脚本没有变化，仅修正回归测试的 Responses 原生字符串预期及更新文档。标签工作流已重新构建、打包、生成说明和清单；发布流水线已完成，后续文档补记不移动标签或替换四个应用包。

已下载并重新计算 release-notes.md 与 update-manifest.json 的大小及 SHA-256，并核对说明与 Release 正文相同、验证章节和本地导航已过滤。四个应用包的清单大小、URL、SHA-256 与 GitHub 上传后计算的 API 元数据逐一一致；不是四包的本地安装验证。

| 应用包 | 字节数 | SHA-256 |
|---|---:|---|
| Windows x64 portable ZIP | 12,780,017 | `0265ec8293a87c3220835ae340dea49816869089b2624a3c22a77b6d2e0933ef` |
| Windows x64 setup EXE | 10,094,485 | `468c2de366f9f4ef9c3a3c6c7472d7e9772adc43d4cc8962da593325d3ba0fbe` |
| macOS arm64 portable ZIP | 13,352,221 | `a51ab217db73b05ab0461b120b7663d59a09df0f7291493e11ee5e865bef883c` |
| macOS arm64 setup PKG | 13,352,705 | `c75b5ed455cea5f98c874b940262a98177ce3e4d98ad37e28490ec984c78ab33` |

不停止或替换正在使用的程序。原生 UI、真实上游及实际更新安装没有在本轮运行，历史验证不能代替当前提交的回归结果。

### 首版预检中的阻塞与修正

- Windows 发版工具输出中文的默认编码失败：增加 UTF-8 输出与 CP1252 回归，已先失败后通过。
- Clippy 重复导入、测试模块顺序、通知赋值格式与 GPUI 测试宏遮蔽均已修正，双平台 Clippy 已在 a80313c 的云端检查中通过。
- Windows x64 的 release 编译、安装 EXE、便携 ZIP 与上传已在 [预检 38380205602](https://github.com/ynx-official/tiny-proxy/actions/runs/38380205602) 成功。macOS release 编译通过，打包因 stdout 混入资料提示失败；修正为诊断写 stderr，新增脚本回归先失败后通过，本机 Python 工具共 15 项通过。
- Windows 完整测试定位到大请求提前鉴权拒绝导致的 TCP 重置，新增定长/分块、两个路由、无密钥/无效密钥的真实 HTTP 回归与拒绝体预算/关停测试。新增有界丢弃传输收尾；[预检 38383574362](https://github.com/ynx-official/tiny-proxy/actions/runs/38383574362) 的双平台真实 HTTP 鉴权回归与预算/关停测试已通过，不弱化原有 401 断言。该轮后续发现大文本转发测试错误地把 Responses 原生字符串预期写成 Chat 部件数组，修正为各协议的正确形状，仍完整比较所有文本与图片内容；修正后两个平台完整测试全部通过，仍完整比较内容而非仅比较大小或放宽结果。
- macOS CI 临时钥匙串配置共享的合成测试密钥访问，避免多个测试进程等待权限提示；该轮 macOS 账号迁移、登录、双续期测试已通过，测试仍使用真实钥匙串读写与加密实现。
- [打包预检 38383610395](https://github.com/ynx-official/tiny-proxy/actions/runs/38383610395) 的两平台 release、四种安装/便携包与上传已全部成功。
- 本地 Rust 回归在依赖编译阶段因 D 盘空间不足失败，测试未执行；未清理工作区，继续使用云端验证。

## 变更依据

首个正式版本，无前置发布标签与 compare 区间。用户于 2026-10-13 明确要求发布 v1.0.0，本版包含现有 Codex 网关、请求诊断、主题、在线更新及双平台交付代码。0.1.0 仅为未发布的开发资料，保留以便追溯。v1.0.0 已在发布预检成功后创建并发布，固定指向上述提交。正式发布后更新本版文档的实际记录与公开说明，保留原标签及四个应用包。

[返回版本总览](../README.md)
