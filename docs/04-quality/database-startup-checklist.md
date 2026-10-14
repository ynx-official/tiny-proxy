# 数据库启动兼容与错误诊断

状态：Review  
最后更新：2026-10-14  
关联文档：[桌面运行与恢复](../05-operations/desktop-operations.md)、[数据模型](../03-architecture/data-model.md)、[开发验证记录](../06-delivery/implementation-status.md)

## 故障与证据

用户反馈安装版 EXE 启动后提示“本地数据库初始化失败，请检查数据目录权限”。该提示原来覆盖所有打开和迁移错误，不能据此断定目录权限异常。

本次只读核对默认数据库和已安装的 v1.0.0 EXE：

- SQLite `PRAGMA quick_check` 返回 `ok`；迁移 0001–0008 的成功标记均为真。
- 已保存迁移校验值均匹配当前 SQL 文件的 LF 字节内容。
- 安装版 EXE 包含同一组脚本的 CRLF 字节内容及其 SHA-384 校验值，不包含对应的 LF 校验值。
- 已阅读本地锁定版本 SQLx 0.8.6 源码：校验值取自 SQL 原始字节，历史校验不一致会返回 `VersionMismatch`。因此该安装版与现有库存在迁移校验冲突；没有将此只读核对称为修复后的运行验证。

未输出账号、凭据、客户端密钥或请求正文，未修改真实数据库或安装版程序。

## 源码修复范围

- `.gitattributes` 固定 `migrations/*.sql` 使用 LF；运行时也规范化内嵌脚本，使新增迁移统一记录 LF 校验值。
- 已有迁移仅接受当前 SQL 的 LF 校验值，或该 SQL 使用 CRLF 时的精确校验值。针对后者调整本次内存中的迁移校验值，不更新 `_sqlx_migrations`，不重放已完成 SQL。
- 其他校验值、程序不认识的迁移版本及未完成迁移仍交给 SQLx 拒绝；不忽略缺失版本、不跳过校验、不删除或重建原库。
- 启动提示区分迁移校验冲突、未知版本、未完成迁移、无法打开文件、只读、占用、磁盘满和损坏文件，并显示所选数据库路径。只使用固定诊断文案，不回显原始 SQL 错误内容。

## 待运行回归

本次非编译检查：使用本机 Rust 1.96.1 自带 rustfmt 对本次四个 Rust 文件执行语法解析、格式化及 `--check`，结果通过；本次已跟踪 Rust 修改的 `git diff --check` 通过，相关五份文档的 139 个本地文件链接均存在。这些检查不代表类型检查、编译或运行测试通过。

代码位于 `tests/database_startup.rs` 与 `src/storage/migrations.rs` 的单元测试。以下场景尚未执行：

| 场景 | 预期 |
|---|---|
| CRLF 版本 0001–0008 的旧库重开两次 | 正常打开；账号密文、客户端密钥密文、主机 ID、活动账号与监听配置保留，迁移记录不变 |
| LF 与 CRLF 混合历史 | 逐条识别对应校验值，历史不变 |
| CRLF 旧库仅完成到 0007 | 保留旧记录，正常应用 0008，新增校验值为 LF |
| 新建数据库 | 所有迁移使用 LF 校验值 |
| 内嵌源码采用 LF 或 CRLF | 新迁移 SQL 和校验值一致 |
| 非 LF/CRLF 的校验差异 | 返回对应版本的 `VersionMismatch`，不更改历史记录 |
| 未知迁移 99 / 未完成迁移 8 | 分别返回 `VersionMissing` / `Dirty`，不伪装正常启动 |
| 后台遇到真正迁移冲突 | 显示冲突版本与数据库路径，不笼统归因为目录权限，不再发 Ready |
| 目录不存在 / 文件内容无效 | 显示对应原因和路径，不回显文件内容，不覆盖损坏库 |

用户明确要求测试后，可执行（会编译）：

```powershell
cargo test --locked --no-default-features --test database_startup --test data_recovery --test local_state --test theme_preferences --test runtime
cargo test --locked --no-default-features --lib storage::migrations::tests
```

测试仅使用临时 SQLite 文件与合成密文，不需要真实登录或模型调用。运行测试不授权打包、替换正在使用的 EXE 或发布。

## 安装版实机待验

在另行授权构建/打包后，用一致性备份产生的隔离数据库核对新 EXE 的 LF/CRLF 旧库启动、账号和设置加载，以及从不同工作目录启动。干净用户首次启动应建库成功；保留旧库且目录确实只读时，应显示访问原因。不要为规避此故障删除 `app.db` 或修改真实库的迁移校验值。

当前交付只有源码、测试代码及说明；已有安装版不会因源码变化自动更新。
