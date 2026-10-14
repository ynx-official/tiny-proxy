# Windows 启动、托盘与退出验证

状态：Review  
最后更新：2026-10-14  
关联文档：[B13](../01-requirements/business-rules.md)、[A27](../01-requirements/acceptance/mvp.md)、[操作说明](../05-operations/desktop-operations.md#windows-窗口与托盘)、[开发验证记录](../06-delivery/implementation-status.md)

本次用户要求修改代码，未授权重新编译、测试或打包。以下回归代码与实机场景均未运行；已有 EXE 和发布包保持原样。

## 已补回归代码

- `tests/application_windows.rs` 读取桌面 EXE 的 PE Subsystem 字段，要求 debug/release 均为 GUI 子系统，防止双击启动附带控制台。
- `src/platform/tray.rs` 的七项 Windows 行为测试覆盖关闭隐藏且不退出/鼠标恢复、键盘恢复最小化窗口、显式退出事件、托盘丢失时窗口保持可见、Explorer 重启重建图标、按数据目录恢复现有实例、销毁托盘 owner。
- 继续使用 `tests/application_version.rs` 验证 `--version` 在打开窗口和数据库之前返回正确版本；GUI 子系统仍需兼容通过管道读取版本。
- 继续使用 `tests/runtime.rs`、`tests/gateway_lifecycle.rs` 验证后台退出、事件队列不被 UI 消费时的收尾与流取消。

## 明确授权测试后执行

```powershell
cargo test --locked --lib platform::tray::tests -- --test-threads=1
cargo test --locked --test application_windows --test application_version --test application_icon --test runtime --test gateway_lifecycle
```

托盘单测会临时创建 Windows 原生窗口和通知图标，需要交互式 Windows 桌面及 Explorer；使用内置系统图标和临时目录，不接入真实账号，不启动当前用户的网关。PE 检查本身不会启动应用。打包阶段应再次核对 release EXE 的 Subsystem。

## 实机验收

1. 分别双击 Windows 安装版与便携版 EXE，确认仅显示主窗口，无控制台窗口；启动失败时显示可读错误对话框。PowerShell 通过管道读取 `--version`，确认返回正确版本且不打开数据库。
2. 已启动网关时点击右上角关闭和 Alt+F4，确认主窗口及任务栏窗口入口收起，托盘保留，客户端持续可用，在途 SSE 不因收起断开。手动停止后收起再恢复，应仍停止，不能重新自动启动。
3. 点击、双击、键盘激活托盘图标和右键“显示 Tiny Proxy”，确认原窗口恢复并获得焦点；页面、监听状态和输入保留。先最小化再收起也应正确恢复。
4. 每次运行首次收起尝试系统提示，后续收起不重复；在隐藏图标区也可恢复。关闭 Windows 通知时仍可用托盘，不依赖提示完成退出。
5. 收起后再次启动同一数据目录的 EXE，确认恢复原实例，不重复创建监听或凭据刷新任务；使用独立 `TINY_PROXY_DATA_DIR` 时不能唤起其他目录的实例。
6. 分别从托盘“退出 Tiny Proxy”和设置“退出应用”退出，确认图标和进程消失、监听端口释放、在途请求取消、元数据收尾；重新启动仍可恢复已有账号、密钥和地址。
7. 托盘创建失败、图标丢失或 Explorer 重启后无法重建时，主窗口保持/恢复可见并显示说明；设置退出始终可用。Explorer 正常重启后应重新注册托盘图标。此项在专门测试环境执行。
8. 在另行授权在线更新验证后确认“安装并重启”仍退出旧进程并由辅助进程启动新版，不会只收起托盘而卡住替换。

本次 Windows 子系统属性依据 [Rust Reference](https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute)，通知图标及版本化回调依据 [Microsoft 通知区域说明](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area)；官方文档核对不代表上述运行验证已通过。macOS 托盘未在本次实现。
