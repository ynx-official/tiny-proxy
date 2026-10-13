# 双平台构建、打包与在线更新

状态：Review  
最后更新：2026-10-13  
关联文档：[发版流程](../06-delivery/release-process.md)、[ADR 0005](../03-architecture/adr/0005-online-update-release.md)、[测试清单](../04-quality/online-update-checklist.md)

## 产物矩阵

| 平台 | Cargo target | 便携版 | 安装版 |
|---|---|---|---|
| Windows x64 | x86_64-pc-windows-msvc | tiny-proxy-vX.Y.Z-windows-x86_64-portable.zip | tiny-proxy-vX.Y.Z-windows-x86_64-setup.exe |
| macOS arm64 | aarch64-apple-darwin | tiny-proxy-vX.Y.Z-macos-aarch64-portable.zip | tiny-proxy-vX.Y.Z-macos-aarch64-setup.pkg |

另附 update-manifest.json 与 release-notes.md，不构建其他平台。工具链 Rust 1.96.0，最低 macOS 14.0 为配置基线、尚未实测。CI 使用 macos-15 arm64：[runner 文档](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job)、[macos-14 淘汰公告](https://github.blog/changelog/2026-10-01-github-actions-macos-14-runner-image-retirement/)。

## 本地操作

以下命令会制作产物，仅在用户明确要求时执行。Windows 需要 Visual Studio C++ Build Tools、Python 3.11+、Inno Setup 6：

```powershell
./scripts/package-windows.ps1
# 重用当前版本、当前 target 的 release EXE：
./scripts/package-windows.ps1 -SkipBuild
```

脚本校验版本资料和 EXE ProductVersion，生成便携 ZIP 与当前用户安装包，不覆盖同名产物。默认安装到 %LOCALAPPDATA%/Programs/TinyProxy，卸载不删除用户数据。

macOS arm64 需要 Xcode Command Line Tools、Rust、Python 3.11+：

```bash
bash scripts/package-macos-app.sh
bash scripts/package-macos-app.sh --binary target/aarch64-apple-darwin/release/tiny-proxy --skip-build
```

脚本生成 dist/TinyProxy.app，从现有 PNG 生成 ICNS，填充 Info.plist，检查 arm64/动态库并 ad-hoc 签名。stdout 只输出 app 路径。流水线使用 ditto 打 ZIP，pkgbuild 创建安装至 /Applications/TinyProxy.app 的 PKG。

当前未配置 Developer ID、Apple 公证或 Windows 代码签名，macOS app 为 ad-hoc、PKG 未签名；首次打开可能需系统批准。不得声称已经签名公证。新版本钥匙串访问提示和签名后的行为须实测。

## 在线更新

设置 → 在线更新，选择启动/定时/关闭自动检查，间隔 1–8760 小时，提醒可开关。配置及成功检查时间重启后保留，应用关闭时暂停。关闭自动检查仍可手动检查。检查只读，不自动下载/安装。

点击下载后显示进度、可取消重试；大小与 SHA-256 验证失败不安装。校验完成后点安装并重启，再次确认停止网关和在途请求。安装版选择同平台 EXE/PKG，便携版选择 ZIP；Windows 助手备份和替换 EXE，失败回滚，安装器静默执行后核对版本。macOS 便携版交换 app，PKG 请求管理员授权并核对版本。助手等待旧进程退出再启动，避免单实例锁冲突。新版本沿用原数据与监听规则。

Windows 助手日志位于系统临时目录 tiny-proxy-update-*.log；暂存区不含账号凭据。安装失败时可查看设置错误/助手日志，不应删除数据库或钥匙串排障。

## 更新源与信任

优先最新 Release 的清单；缺失/网络/403/429/5xx 时降级 GitHub API，API 可用性错误再降级 latest 重定向及 expanded_assets 页面。页面结构变化导致失败时显示错误，可打开版本页。摘要、仓库、版本和内容不合法不能绕过校验，无 Release 显示尚未发布。

清单 schema=1，四个唯一应用产物记录真实大小、SHA-256 和同仓库同标签 URL，生成拒绝缺失/多余/重复/空产物。下载限 1 GiB，解压限 2 GiB、10,000 文件，拒绝越界/符号链接/重复写入。SHA-256 不等同于发布者数字签名，信任 HTTPS 与固定仓库。凭据备份边界见 ADR 0005。
