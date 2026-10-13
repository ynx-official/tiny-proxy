# 在线更新与发版验收清单

状态：Review  
最后更新：2026-10-13  
关联文档：[ADR 0005](../03-architecture/adr/0005-online-update-release.md)、[打包操作](../05-operations/build-and-package.md)、[发版流程](../06-delivery/release-process.md)

## 验证边界

已补 Rust/Python 回归源码，默认不执行测试、编译、构建、打包或应用。允许格式化、脚本语法解析、资料一致性和差异检查；结果记录到 [版本详情](../upgrade/v0.1.0/README.md)，不能称为运行测试通过。

## 获得对应授权后的命令

```powershell
python -m unittest scripts/test_release_notes.py scripts/test_update_manifest.py scripts/test_release_contract.py
cargo test --locked --test updater
cargo test --locked updater::
cargo test --locked --test data_location --test local_security --test runtime --test application_icon
cargo test --locked --test application_version --test codex_refresh --test device_login --test local_state --test accounts --test key_deletion
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

macOS 纯加密测试在 cargo test --locked --lib；真实钥匙串需可解锁的原生环境。CI 创建临时钥匙串。更新安装须隔离测试位置与数据，不能自动替换正在使用的程序。

回归源码覆盖：SemVer/预发布、四种产物选择、清单/页面地址与摘要校验、篡改、取消、独立暂存、ZIP 越界、配置持久化与非法间隔、macOS nonce/密钥/标签/前缀、锁文件/标签/索引/Draft 发版阻止、清单完整集合、两平台矩阵和安装标识一致性。

## 手工场景

| 入口与操作 | 预期 |
|---|---|
| 设置页与主题切换 | 显示 Cargo 版本/平台；更新不阻塞账号、模型、网关和记录，主题正常。 |
| 修改策略、间隔、提醒后重启 | 持久化；非法小时反馈；关闭后不自动请求，手动仍可检查。 |
| 最新、旧版、无 Release、断网、限流 | 不降级安装；旧版有说明；错误可重试，符合条件才降级来源。 |
| 下载取消重试、大小/摘要篡改 | 无安装或旧文件误选，取消与错误有明确状态。 |
| 四种安装方式分别升级 | 对应产物；确认前不替换，确认后停止请求、收尾记录、旧进程退出后启动新版。 |
| Windows 空格/单引号路径、不可写或 EXE 锁定 | 路径无注入；错误/日志可见；便携失败恢复旧版。 |
| macOS ZIP/PKG，拒绝管理员授权 | 图标/权限保留；授权拒绝可恢复，PKG 核对版本，无单实例锁冲突。 |
| macOS 登录、建 Key、重启、切换账号、再更新 | 凭据钥匙串往返正常；SQLite 无明文 Token/Key，升级后状态保留。 |
| 钥匙串锁定/权限拒绝，数据库复制到其他用户或平台 | 保留库、失败、不明文降级或替换加密密钥；需要重新登录。 |
| 手动 Actions | 两个平台 artifacts、四个应用产物，不发布 Release。 |
| 正式/beta 标签 | 正式版四包+清单+说明；beta 为 prerelease、不为 latest；未 Approved/无日期/缺资料阻止发布。 |

本机 Windows；macOS 原生界面、钥匙串、签名权限、最低系统基线与两平台真实更新往返未验证。Actions 未触发，未发布版本。

## v1.0.0 发布验证追加

发布期间的实际结果见 [v1.0.0](../upgrade/v1.0.0/README.md)。Python 工具回归包含 `scripts/test_macos_packaging.py` 的完整脚本 stdout 合约测试，只有原生 macOS 工具边界在该测试中隔离；真实 app/ZIP/PKG 仍由 macOS Actions 打包核验。临时测试钥匙串预置随机合成加密密钥，允许多个 Rust 测试可执行文件访问，避免无人值守权限提示；不访问本机生产钥匙串。
