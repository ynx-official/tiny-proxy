# 请求记录测试清单

状态：Review  
最后更新：2026-10-13  
关联文档：[诊断说明](../05-operations/request-diagnostics.md)、[验收](../01-requirements/acceptance/mvp.md)、[验证记录](../06-delivery/implementation-status.md)、[执行限制](../../AGENTS.md)

## 本次验证边界

在用户新增“只改代码，不默认构建打包”限制之前，运行过请求存储 3 项、网关生命周期 7 项测试及包含桌面的 clippy，均通过。完整桌面测试因运行中的旧 exe 被占用而未完成。随后对终态后的分片哨兵、检查点并发和异常恢复耗时做了代码调整，并扩展终态测试；这些最终调整未重新运行编译型验证，GUI 未实测。不得用先前通过结果声称最终版本已经通过。

下列清单供用户选择运行，不会自动执行或生成修复版程序。

2026-10-13 新增“复制明细”及关联异常展示；4 项桌面格式化回归测试已添加，覆盖 413/未知值、已知值与零值、执行中/恢复时间及异常关联隔离。未运行这些测试，也未验证原生窗口剪贴板操作。

同日用户要求取消请求体大小限制。现扩展既有 HTTP 图片回归，覆盖 Chat/Responses × 大文本/大 Base64 图片 × JSON/SSE × 定长/分块的 16 组大请求，以及鉴权/JSON/Content-Type/字段拒绝与实际字节数持久化。测试代码已修改，未运行；历史 413 明细回归保留以支持旧异常排查。

## 建议自动验证

```powershell
# 取消请求体大小限制；本地模拟上游，不使用真实账号额度
cargo test --locked --no-default-features --test gateway_images images_and_large_requests_reach_http_upstream_with_auth

# 详情文案与复制字段；需要桌面编译，用户明确要求测试时执行
cargo test --locked --bin tiny-proxy request_detail_tests

# 元数据持久化、恢复和清理；两个接口的流式/聚合终态、错误与取消
cargo test --locked --no-default-features --test request_records --test gateway_lifecycle

# 既有 HTTP、工具、图片与长度参数兼容行为
cargo test --locked --no-default-features --test gateway_http --test gateway_protocol --test gateway_compatibility --test gateway_reasoning --test gateway_images

# 后台启动/停止、满 UI 队列关闭、历史账号/密钥保持
cargo test --locked --no-default-features --test runtime --test gateway_startup --test local_state --test key_deletion

# 用户需要更广验证时选择；这些命令会触发编译
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

完整桌面测试可能需要写入 `target/debug/tiny-proxy.exe`；若旧版仍运行，会报文件占用。由用户选择退出旧版后执行，不擅自停止程序，也不因此自动创建其他构建目录。测试授权不自动授权打包。

## 手动验收重点

| 测试 | 预期 |
|---|---|
| 发起耗时较长的流式请求，在结束前打开请求记录 | 立刻出现执行中记录；无结束时间；每秒更新耗时和阶段，可查看首批数据、首个输出和最近上游活动 |
| 文本及函数工具分别完成；Chat/Responses 各测流式与非流式 | 保存开始/结束时间、结果及已提供的 tokens；收到合法终态后及时结束，不等待上游关闭连接 |
| 输入不支持参数、错误 JSON、错误密钥，或同时启动超过 4 个请求 | 请求表保留结果；异常表分别显示参数、鉴权、并发原因，可打开关联请求详情 |
| 新版本向两个接口发送超过旧 2 MiB 的有效文本/图片请求，包括分块发送 | 网关不按请求字节数返回 413；上游成功时正常返回，详情/复制显示实际请求大小；上游自身拒绝不冒充网关大小限制 |
| 在失败异常中打开历史 413 的关联请求详情，点击“复制明细”并粘贴到文本编辑器 | 复制标题、面板全部字段及该条异常代码/原因/时间/阶段/HTTP；HTTP 为 413，缺失元数据保持“未提供”；状态栏提示已复制，不只复制 ID |
| 在请求记录中复制完成/执行中/恢复后的详情，再切换异常分页并复制另一条 | 复制与当前面板一致，保留零值及恢复时间语义；不混入其他请求的异常或上一次详情；无关联异常时仍能复制请求字段 |
| 主动中止客户端、撤销在途请求的 Key、停止网关 | 请求进入取消/中断；异常原因可辨别；不伪造完成；其他 Key 的请求继续 |
| 模拟上游无终态断流、失败事件或超时 | 请求失败；异常表显示对应原因；SSE 不产生成功 [DONE] |
| 重启应用，并在代理停止时查看记录 | 历史及异常保留；不依赖监听状态；界面可分页 |
| 强制结束应用后重开 | 未结束记录变为中断；恢复时间明确标注；耗时不包含关闭期间，不能显示成成功 |
| 积累超过 100 条并切换页签、分页及关联详情 | 请求与异常独立分页，执行中置顶；不会打开另一条请求的详情 |
| 7 天/10,000 条清理阈值 | 已结束记录及关联异常按周期清理；运行中保留；其他账号/Key/模型测试数据保持 |
| 使用合成敏感值后检查请求/异常数据 | 聊天、代码、工具参数、回答、Token、密钥、原始 URL 和上游错误正文均不出现 |
| 窗口缩小、高 DPI、长模型 ID、未知 tokens、空列表 | 文案完整可读，详情不溢出；禁用/空/错误状态明确 |

真实账号和客户端场景需用户选择执行，会使用模型额度；本次未执行真实上游联调。
