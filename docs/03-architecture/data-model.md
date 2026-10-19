# 本地数据模型

状态：Review  
最后更新：2026-10-14  
关联文档：[认证](authentication.md)、[安全](threat-model.md)、[业务规则](../01-requirements/business-rules.md)

## 存储选择

SQLite 当前默认路径为 %USERPROFILE%/.tiny-proxy/app.db，不放项目目录、网络共享盘或同步盘。不需要 MySQL。2026-10-13 修复不同启动来源读到不同数据库的问题：原 AppData 目录会被 MSIX 启动器重定向，默认位置改到用户目录；首次启动一致性复制旧库并保留来源。迁移与冲突处理见 [数据目录统一与历史恢复](../05-operations/desktop-operations.md#数据目录统一与历史恢复)。

推荐 sqlx SQLite，使用版本化 SQL 迁移、foreign_keys、busy_timeout；持久化策略在 P1 验证。可采用 WAL，凭据事务需要可靠提交配置，不能把 WAL 自身当成加密或备份机制。[sqlx SQLite 选项](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)

## 表与字段

| 表 | 关键字段 | 约束与用途 |
|---|---|---|
| app_settings | singleton_id、schema_version、host_id、active_registration_id、network_config、runtime_limits、startup_preferences | 单行配置；host_id 稳定随机生成；更新先校验 |
| account_registrations | id、provider_kind、issuer、subject、client_id、label、permission_state、created_at、updated_at | 唯一键(provider_kind, issuer, subject, client_id)；不以 email 作为唯一键 |
| credential_bundles | registration_id、encrypted_blob、protection_kind、format_version、generation、expires_at、updated_at | 每注册最多一条；外键；所有 Token 同一密文 bundle，原子替换 |
| client_keys | id、label、public_prefix、secret_digest、encrypted_value、created_at、last_used_at；旧 revoked_at 列保留兼容 | 高熵 secret 摘要用于常量时间鉴权，DPAPI 密文用于本人查看；撤销删除整条记录，见 B20 |
| model_catalog | registration_id、model_id、display_name、capability_json、verified_at、fetched_at | 复合主键；按注册隔离；“支持”与“实测”分开 |
| request_records | request_id、key_id、registration_id、model_id、protocol、started_at、duration_ms、ttft_ms、result、error_code、token_counts、upstream_request_id | 仅元数据；usage 不明为 NULL；按日期/数量清理 |

上表为目标模型。首个迁移已实现 app_settings、account_registrations、credential_bundles、client_keys 的基础字段；模型目录由 0003 按账号保存。0006 增加请求与异常表，实际字段及最终验证边界见下文；完整设置仍未实现。时间统一 UTC，UI 转为本地时区。

网关鉴权 digest 针对高熵随机秘密；不用于人类密码。密钥格式建议“非秘密 ID + 随机秘密”，先定位记录再比较秘密摘要；避免遍历所有密钥。

## 0008 外观主题偏好

迁移 0008 在 app_settings 增加 theme_preference，非空 TEXT，默认 system，CHECK 只允许 system/light/dark；现有行自动使用跟随电脑，不更新旧迁移。实际界面颜色由 GPUI 根据偏好及系统外观解析，解析结果不落库。

UI 通过 SetThemePreference 命令交给后台保存，成功提交后以专用结果事件更新界面；保存失败不发成功事件，不影响登录任务状态。启动快照携带已保存偏好。读取异常必须报错，不把损坏值静默视为默认值；更新行缺失也不能报告保存成功。升级/重启测试见 [主题验证清单](../04-quality/theme-checklist.md)。

## 凭据保护

Windows 首版建议用户级 DPAPI；整个 Token bundle 加密为 BLOB，禁止使用允许同机其他用户解密的 machine scope。SQLite 只看到密文和必要元数据。[Microsoft DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)

Token bundle 包含 access/refresh/retained ID Token、有效期、权限信息及身份绑定校验信息。安全存储失败时不降级为明文文件。

本方案不是全库加密；账号标签、使用量、时间等元数据仍受文件权限保护。DPAPI 不能防止同用户恶意进程或管理员在运行时读取内存。

## 事务边界

- 注册完成：先保留已取得的注册身份信息；Token 验证通过后提交凭据及权限状态，再激活连接。
- 刷新：CAS generation + 单注册互斥；新密文与版本一次提交。提交前不把新 Token 广播给请求。
- 撤销密钥：单条 DELETE 提交后再取消该 key 的在途请求；失败保留记录。每次鉴权读取记录，删除后的旧值返回 Invalid；UI 使用提交后的快照移除行和对应明文预览。
- 切换账号：活动注册与配置版本事务更新；在途请求不读取新的活动指针。
- 记录写入：0006 起请求开始即建档，运行中每秒检查点，结束结果和异常一次事务提交；不逐 token 落库。写入失败告警并计数，不伪造完成；具体边界见 [请求诊断](../05-operations/request-diagnostics.md)。

## 迁移、备份、清理

2026-10-14 针对 Windows 安装版与开发库的 LF/CRLF 校验冲突，迁移源文件和新增记录统一采用 LF；读取既有记录时额外认可同一 SQL 的精确 CRLF 校验值，仅调整内存迁移描述，不改写历史校验或重放已完成 SQL。内容差异、未知版本和未完成迁移仍按 SQLx 规则拒绝，证据与待运行回归见 [数据库启动兼容清单](../04-quality/database-startup-checklist.md)。

P1 覆盖空库、旧版本升级、迁移失败回滚和不支持降级。迁移前使用 SQLite 一致性备份机制；不复制正在写入的 app.db 而遗漏 WAL。

默认诊断导出不包含数据库或凭据。用户主动备份的数据库含受保护密文，不保证跨电脑/Windows 用户可恢复；新设备重新登录并创建新密钥。

清理请求记录和删除连接为逻辑删除。SQLite 页、WAL、备份及操作系统存储中可能留有历史密文或元数据，界面和文档不能声称安全擦除。若未来要求不可恢复删除，需单独设计与确认。


## 已实施的 Codex 迁移（0002）

codex_session 是单行活动 Codex 会话：singleton_id=1、encrypted_blob、generation、expires_at、updated_at。bundle 字段和续期规则以当前认证文档为准。

0001 中的 SIWC 表保留，0002 不改写原凭据；通过迁移隔离测试。0003_desktop_state 增加 codex_accounts、model_tests、活动账号指针与密钥密文；模型目录、显式测试、额度按账号持久化。0004_gateway_bind 增加监听地址持久化。请求日志原先只有监听会话内存记录，0006 已增加持久化代码，最终运行验证待执行；完整设置仍是后续目标。


## 0003 当前本地状态

- codex_accounts：以 subject + ChatGPT account ID 的 SHA-256 定位，保存 DPAPI 凭据、CAS 代次、显示标签、目录 JSON/读取时间、最近成功额度 JSON/时间、最近尝试时间/错误。
- app_settings.active_codex_account_id：活动账号，启动恢复；网关运行或前台操作中禁止切换。
- client_keys.encrypted_value：可空 DPAPI 密文；NULL 表示旧摘要密钥不可恢复，不改变鉴权与撤销语义。
- model_tests：账号、模型 ID、开始/结束时间、running/completed/failed/interrupted、固定文本测试结果/错误、tokens。保留全部历史，桌面读取最近 100 条，结果最多预览 4096 字符；普通 Agent 请求正文不进入此表。
- 额度 JSON：套餐、主次窗口时长、剩余百分比、重置时间、可用重置次数、次数更新时间及独立错误。时间为 UTC 秒，界面显示本地时间；次数查询失败保留旧值并显示旧时间。

账号标签、额度、模型与显式测试结果是 SQLite 普通字段，并非全库加密。凭据/密钥仍受 DPAPI 保护。参见 [ADR 0004](adr/0004-local-account-state.md)。

## 0004 监听地址

`app_settings.gateway_bind` 为非空文本，默认 `127.0.0.1:8787`。仅通过地址校验且成功监听后更新；保存失败即停止新监听。旧库添加默认值，不改动账号、密钥或历史。启动仍重新验证地址，不能由数据库配置绕过网络边界。行为及批准依据见 [代理自动启动](../05-operations/desktop-operations.md#代理自动启动)。

## 0005 已撤销密钥清理

2026-10-13 用户明确要求撤销后删除、不再展示，依据 [B20](../01-requirements/business-rules.md)。0005_delete_revoked_keys 仅删除 `revoked_at IS NOT NULL` 的历史记录，摘要和 DPAPI 密文随行移除。保留原列及已有迁移校验值，有效旧摘要密钥不受影响。新撤销不再写 revoked_at；列表、鉴权和密钥读取仍排除旧撤销状态，避免重新使用历史无效记录。删除范围为当前数据库记录，历史页和备份的边界沿用上文清理规则。

## 0006 请求元数据与独立异常

批准范围及时间/保留语义见 [请求诊断](../05-operations/request-diagnostics.md)。迁移只增加两个表与时间索引，不修改旧迁移，不包含正文列，也不迁移真实数据目录。

- request_records：id、method、route、model、key_id、account_id、stream、body_bytes、message_count、tool_count、outcome、phase、http_status、upstream_status、started_at、ended_at、elapsed_ms、first_byte_at、first_output_at、last_activity_at、input_tokens、output_tokens。
- failure_records：request_id、occurred_at、phase、code、message、http_status；request_id 唯一外键，清理请求时级联删除异常。message 仅为网关生成的安全文案。2026-10-14 按用户反馈细化为代码内列举的协议字段路径、数组索引及固定规则/建议，随后用户要求默认信任字段，未知扩展不再生成本地异常，只保留必要转换错误的代码字段路径；不含请求值，规则见 [本地参数诊断](api-contracts.md#本地参数校验诊断2026-10-14)。无需迁移，不补写旧异常。
- 所有请求时间用 UTC 毫秒；不改变其他表原有 UTC 秒字段。运行中 ended_at 为 NULL。正常耗时使用 Instant；中断恢复保留最近检查点耗时，ended_at 是恢复确认时间。
- key_id/account_id 是历史关联值，不向账号和 Key 表建立级联外键；删除 Key 不删除请求历史。元数据为普通 SQLite 字段，不是 DPAPI 或全库加密。
- 结束结果与异常同事务，检查点/重复结束仅更新 ended_at IS NULL 的记录，不能覆盖终态；持久化失败计数并提示。

最终代码验证状态与未运行事项见 [验证记录](../06-delivery/implementation-status.md) 和 [测试清单](../04-quality/request-records-checklist.md)。

## 0007 在线更新偏好

用户于 2026-10-13 批准移植 ashell 更新设置。迁移 0007 新增单行 update_preferences：check_mode（startup/interval/disabled）、interval_hours（1–8760，默认24）、notify（默认1）、last_checked_at（默认0，仅成功检查后更新）。不存安装包、凭据或远端管理接口。迁移与持久化回归代码已加入，未执行。macOS 凭据密文封装及钥匙串边界见 [ADR 0005](adr/0005-online-update-release.md)。

## 0009 缓存输入 Token 元数据

2026-10-14 用户批准[缓存命中率预览](../02-design/overview-statistics.md)并要求使用真实数据，因此新增 `0009_request_cache_tokens.sql`，为 `request_records` 添加可空整数 `cached_input_tokens`。旧迁移与校验值不变，旧行保持 NULL，无默认0，不从已保存的成功状态或总输入量推算缓存。

新请求只保存上游终态提供的非负整数缓存数，兼容 Responses 与转换后 Chat 的明细字段；检查点、结束事务、在途快照和按 ID 查询沿用同一记录类型。负值/缺失/非整数不填零，缓存超过输入的记录不参与缓存比例。该字段是输入用量的子集，不能重复加入总 Token。详情与复制保留已有元数据边界，不新增正文或凭据字段，不改变保留周期。

0009 是追加迁移，当前旧版缺少该迁移的代码不能直接打开升级后的数据库；不删除迁移记录或回退列来绕过校验。内存 SQLite 已检查新建与0008→0009的原值保留，SQLx 升级/重开与历史校验值回归代码未运行；真实数据文件本轮未打开或升级。
