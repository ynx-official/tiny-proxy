# 开发验证记录

状态：Archived  
最后更新：2026-10-13  
关联文档：[开发计划](../06-delivery/development-plan.md)、[验收场景](../01-requirements/acceptance/mvp.md)、[运行入口](../../README.md)、[视觉系统](../02-design/visual-system.md)

## 本次交付

用户要求“开始实行”后进入 P0。新增单 Cargo package、锁定依赖、Windows 保护层、SQLite 首个迁移、认证验证模块、流式基础模块、后台运行时和六个导航页的 GPUI 桌面壳。用户已选择 hashicorp 风格。

主程序：`target/debug/tiny-proxy.exe`。从仓库根运行 `cargo run --features desktop --bin tiny-proxy --locked` 可构建并启动。它不同于只显示两行文字的 `target/debug/examples/desktop_probe.exe`。

当前是开发验证版，不是完整 MVP。以下状态按实际代码与本次执行证据填写。

| 能力 | 实现与证据 | 尚缺什么 |
|---|---|---|
| 原生框架 | gpui-kit 0.7.3 / GPUI 0.3.9 编译成功；最小窗口及中文显示由用户截图确认 | 新桌面壳视觉检查、真实 IME 组合输入、DPI、托盘、干净机 |
| 桌面壳 | 六个导航、真实后台状态、注册列表、模型目录、文本测试按钮、密钥操作 | 全页面原型中的完整向导、详情、设置与状态尚未全部实现 |
| 从零登录准备 | 系统浏览器入口、127.0.0.1 临时回调、随机 state/nonce/PKCE、回调一次消费、issued client ID 保留；2026-10-13 用户反馈“登录验证成功” | 推理权限与模型可用性验证；用户提出流程与参考项目不同，认证路线尚未定稿 |
| 身份校验 | RS256 JWKS 签名、issuer、audience、exp、nbf、nonce、sub 校验；合成 RSA 签名测试通过 | 真实 OpenAI 返回值的端到端证据 |
| 凭据保护 | 用户级 DPAPI、密文 bundle、SQLite 身份绑定与原子保存；篡改拒绝和身份不匹配回滚测试通过 | 自动续期、16 并发刷新、远端撤销、恢复与迁移验收 |
| 模型探测 | 真实目录及 Responses 代码已接线；点击测试才发送固定短提示 | 真实 response.completed、工具往返、Chat 可见历史续接 |
| 本地密钥 | 256 bit 随机值、摘要入库、常量时间比较、命名、一次展示、撤销 | 对外 API 鉴权中间件、撤销在途请求取消 |
| 流式基础 | UTF-8 跨片、BOM、CR/LF/CRLF、多行 data、事件预算、文本探测终态 | 产品级 Responses/Chat 转换、交错工具、背压、SDK 契约 |
| Tokio 桥接 | 独立后台线程、有界命令/事件队列、任务所有权；满队列时退出回归测试通过 | 完整网关优雅停机和休眠恢复 |
| SQLite | 空库迁移、稳定 host ID、重启后密钥状态、凭据 CAS 代次冲突 | 完整设置、模型缓存、请求记录、升级备份/恢复 |
| CI | 新增 Windows fmt/clippy/test/release build 工作流 | 当前目录不是 Git 仓库，没有远端 CI 执行证据 |

## 本次命令证据

环境：Microsoft Windows 11 专业版，10.0.26200；Rust 1.96.0，Cargo 1.96.0，stable-x86_64-pc-windows-msvc；Visual Studio 2022 Build Tools，MSVC 14.44.35207 可用。

| 命令 | 结果 |
|---|---|
| `cargo build --features desktop --example desktop_probe` | 通过 |
| `cargo fmt --all` | 已执行格式化 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --features desktop --all-targets --locked -- -D warnings` | 通过，零警告 |
| `cargo test --features desktop --all-targets --locked` | 23 项测试通过，无跳过 |
| `cargo build --features desktop --bin tiny-proxy --locked` | 通过，产生主程序 |

测试分布：identity 1、local_security 5、oauth_attempt 4、oauth_loopback 2、response_terminal 4、runtime 2、sse 5。

分帧、认证尝试、回调、身份校验、凭据保存、流终态先建立失败测试后实现。后台满队列无法退出的问题由新增回归测试实际复现，修复后通过。桌面布局不以这 23 项测试代替视觉验收。

原生界面检查途中用户按物理 Esc 停止了 Computer Use，随后停止桌面自动化，仅继续代码、编译与命令行测试。用户反馈未看到新窗口后，已启动主程序；Windows 进程查询确认存在标题为“Tiny Proxy · 开发验证版”的窗口，窗口句柄非零且响应正常。这仅证明窗口已建立，完整视觉检查仍未完成。

## P0 实验与产品代码的边界

`examples/desktop_probe.rs` 是最小框架探测。`src/auth/probe.rs` 是候选认证、目录与固定文本测试实验，不包含产品级自动续期或完整会话服务。它调用公共 `api.openai.com/v1`，没有读取本机 Codex 登录缓存、复制参考客户端身份或使用 backend-api 作为隐式 fallback。

本次以 Axum 实现 OAuth 本机回调；尚未实现任何对外 `/v1` 模型路由。客户端接入页不启动监听，也不把创建密钥显示成网关已可用。请求记录和设置页明确标注待实现。当前关闭窗口退出，重复启动拒绝第二实例；托盘和唤起第一实例仍在计划中。

新增表目前是目标数据模型的基础子集。注册的 subject 初始可空，用于保留已取得但尚未完成交换的 issued client ID；只有通过身份校验才能写入凭据并激活。后续迁移需要保持现有校验与历史可追溯。

## 下一步的放行顺序

1. 用户已反馈当前 SIWC 登录验证成功，同时提出流程与参考项目不同。先说明自身应用授权与参考 Codex 设备授权的差异并确认路线；登录反馈不能视为批准替代参考路线，也不等于模型调用成功。
2. 在“模型”页读取实际目录，选择一个模型执行“测试文本”。记录 completed 结果；仅看到模型目录不放行 P0。
3. 补齐 P0 真实函数工具往返与 Chat 历史续接，记录实际 Trae 版本及其默认字段。
4. 完成新桌面壳、IME/DPI/托盘及干净 Windows 环境验证，再按计划确认平台与认证定稿。
5. 在以上约束明确后推进正式认证续期服务、标准模型 API 与完整流式转换；最后完成所有原型页面与跨设备验收。

若用户暂不进行真实账号验证，可以继续不依赖账号的协议/生命周期测试；但 P0 状态保持“进行中”，不把当前版本发布为已可使用的代理服务。

## 来源核对

- [GPUI Kit 官方仓库](https://github.com/longbridge/gpui-kit)：本次依照 0.7.3 应用入口与匹配 GPUI 快照构建。
- [SIWC 注册与登录](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)：应用自身动态注册、issued client ID、loopback/PKCE/nonce。
- [模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)：公共目录及 Responses，等待 completed。
- [当前限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)：store=false、stream=true、完整上下文及函数命名空间限制。


本历史版本已由 [Codex 路线决定](../03-architecture/adr/0003-codex-device-gateway.md) 与当前认证/实现文档替代。
