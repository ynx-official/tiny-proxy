//! Windows tray resources belong to the GPUI thread and use its message pump.
//! Native callbacks only enqueue application actions; they never stop Tokio.
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    io,
    path::Path,
    ptr,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
};
use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
                NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::PCWSTR,
};

const WINDOW_CLASS: &str = "TinyProxy.TrayWindow";
const TRAY_CALLBACK: u32 = WM_APP + 1;
const RESTORE_INSTANCE: u32 = WM_APP + 2;
const SHOW_COMMAND: u32 = 1;
const EXIT_COMMAND: u32 = 2;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Restore,
    Exit,
    Unavailable,
}

struct State {
    main_window: HWND,
    icon: HICON,
    taskbar_created: u32,
    available: Cell<bool>,
    notified: Cell<bool>,
    events: Sender<TrayEvent>,
}

impl State {
    fn icon_data(&self, owner: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: TRAY_CALLBACK,
            hIcon: self.icon,
            ..Default::default()
        };
        copy_wide(&mut data.szTip, "Tiny Proxy · 点击恢复窗口，右键退出");
        data
    }

    fn add_icon(&self, owner: HWND) -> bool {
        let mut data = self.icon_data(owner);
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) } != 0;
        if added {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) } == 0 {
                unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
                self.available.set(false);
                return false;
            }
        }
        self.available.set(added);
        added
    }

    fn restore(&self) {
        unsafe {
            ShowWindow(
                self.main_window,
                if IsIconic(self.main_window) != 0 {
                    SW_RESTORE
                } else {
                    SW_SHOW
                },
            );
            SetForegroundWindow(self.main_window);
        }
        let _ = self.events.send(TrayEvent::Restore);
    }

    fn command(&self, command: u32) {
        match command {
            SHOW_COMMAND => self.restore(),
            EXIT_COMMAND => {
                let _ = self.events.send(TrayEvent::Exit);
            }
            _ => {}
        }
    }

    fn menu(&self, owner: HWND) {
        // A foreground owner and a trailing WM_NULL let Windows dismiss the
        // native menu correctly when the user clicks elsewhere.
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            let show = wide("显示 Tiny Proxy");
            let exit = wide("退出 Tiny Proxy");
            if AppendMenuW(menu, MF_STRING, SHOW_COMMAND as usize, show.as_ptr()) == 0
                || AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) == 0
                || AppendMenuW(menu, MF_STRING, EXIT_COMMAND as usize, exit.as_ptr()) == 0
            {
                DestroyMenu(menu);
                return;
            }
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            SetForegroundWindow(owner);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                owner,
                ptr::null(),
            );
            DestroyMenu(menu);
            PostMessageW(owner, WM_NULL, 0, 0);
            self.command(command as u32);
        }
    }
}

pub struct Tray {
    owner: HWND,
    state: Box<State>,
    events: Receiver<TrayEvent>,
}

impl Tray {
    /// Create a tray icon from the executable's embedded icon resource.
    /// Call and drop this on the window's UI thread.
    pub fn new(main_window: HWND, data_directory: &Path) -> io::Result<Self> {
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        let icon = unsafe {
            LoadImageW(
                module,
                1usize as PCWSTR,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_SHARED,
            )
        };
        if icon.is_null() {
            return Err(io::Error::last_os_error());
        }
        // LR_SHARED icons are owned by Windows; never destroy this resource.
        Self::create(main_window, &instance_title(data_directory), icon)
    }

    fn create(main_window: HWND, title: &str, icon: HICON) -> io::Result<Self> {
        let (sender, events) = mpsc::channel();
        let class = wide(WINDOW_CLASS);
        let title = wide(title);
        let taskbar = wide("TaskbarCreated");
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        let taskbar_created = unsafe { RegisterWindowMessageW(taskbar.as_ptr()) };
        if module.is_null() || taskbar_created == 0 {
            return Err(io::Error::last_os_error());
        }
        let registration = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&registration) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(io::Error::last_os_error());
        }
        // A hidden top-level window receives TaskbarCreated after Explorer
        // restarts. A message-only HWND would miss this broadcast.
        let owner = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                module,
                ptr::null(),
            )
        };
        if owner.is_null() {
            return Err(io::Error::last_os_error());
        }
        let tray = Self {
            owner,
            state: Box::new(State {
                main_window,
                icon,
                taskbar_created,
                available: Cell::new(false),
                notified: Cell::new(false),
                events: sender,
            }),
            events,
        };
        // The Box's address remains stable until Drop clears GWLP_USERDATA.
        // All callbacks and resource cleanup run on the same UI thread.
        unsafe {
            SetWindowLongPtrW(
                owner,
                GWLP_USERDATA,
                (&*tray.state as *const State) as isize,
            )
        };
        if !tray.state.add_icon(owner) {
            return Err(io::Error::other("无法创建 Windows 托盘图标"));
        }
        Ok(tray)
    }

    pub fn try_recv(&self) -> Result<TrayEvent, TryRecvError> {
        self.events.try_recv()
    }

    /// Check that the control entry still exists before hiding the window.
    pub fn hide(&self) -> io::Result<()> {
        let mut data = self.state.icon_data(self.owner);
        if !self.state.available.get() || unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } == 0 {
            self.state.available.set(false);
            return Err(io::Error::other(
                "托盘不可用，窗口保持打开；可在设置中退出应用",
            ));
        }
        unsafe { ShowWindow(self.state.main_window, SW_HIDE) };
        if !self.state.notified.replace(true) {
            data.uFlags = NIF_INFO;
            data.dwInfoFlags = NIIF_INFO;
            copy_wide(&mut data.szInfoTitle, "Tiny Proxy 已收起到托盘");
            copy_wide(
                &mut data.szInfo,
                "应用继续在后台运行。点击托盘图标恢复窗口，右键选择“退出 Tiny Proxy”结束运行。",
            );
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
        Ok(())
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.state.icon_data(self.owner));
            SetWindowLongPtrW(self.owner, GWLP_USERDATA, 0);
            DestroyWindow(self.owner);
        }
    }
}

unsafe extern "system" fn window_proc(
    owner: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = unsafe { GetWindowLongPtrW(owner, GWLP_USERDATA) } as *const State;
    if let Some(state) = unsafe { state.as_ref() } {
        match message {
            TRAY_CALLBACK => {
                // Version 4 packs the event in LOWORD(lParam) and icon ID in
                // HIWORD(lParam); keyboard activation is also supported.
                match (lparam as u32) & 0xffff {
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONDBLCLK => state.restore(),
                    WM_CONTEXTMENU => state.menu(owner),
                    _ => {}
                }
                return 0;
            }
            RESTORE_INSTANCE => {
                state.restore();
                return 0;
            }
            WM_COMMAND => {
                state.command((wparam as u32) & 0xffff);
                return 0;
            }
            _ if message == state.taskbar_created => {
                if !state.add_icon(owner) {
                    state.restore();
                    let _ = state.events.send(TrayEvent::Unavailable);
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(owner, message, wparam, lparam) }
}

/// Reopening the same data directory restores its existing hidden instance.
pub fn restore_existing_instance(data_directory: &Path) -> bool {
    let class = wide(WINDOW_CLASS);
    let title = wide(&instance_title(data_directory));
    unsafe {
        let owner = FindWindowW(class.as_ptr(), title.as_ptr());
        !owner.is_null() && PostMessageW(owner, RESTORE_INSTANCE, 0, 0) != 0
    }
}

pub fn show_error(message: &str) {
    let title = wide("Tiny Proxy");
    let message = wide(message);
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

fn instance_title(directory: &Path) -> String {
    let digest = Sha256::digest(directory.as_os_str().to_string_lossy().as_bytes());
    format!("TinyProxy.Tray.{digest:x}")
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn copy_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    for (slot, value) in target.iter_mut().take(N - 1).zip(value.encode_utf16()) {
        *slot = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDI_APPLICATION, IsWindowVisible, LoadIconW, SendMessageW,
    };

    struct TestWindow(HWND);

    impl TestWindow {
        fn new() -> Self {
            let class = wide("STATIC");
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    ptr::null(),
                    0,
                    0,
                    0,
                    100,
                    100,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            assert!(!hwnd.is_null());
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            Self(hwnd)
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }

    fn tray(window: &TestWindow) -> Tray {
        let icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        assert!(!icon.is_null());
        Tray::create(window.0, "TinyProxy.Tray.Test", icon).unwrap()
    }

    #[test]
    fn close_hides_window_without_destroying_it_and_tray_click_restores_it() {
        let window = TestWindow::new();
        let tray = tray(&window);
        tray.hide().unwrap();
        assert_eq!(unsafe { IsWindowVisible(window.0) }, 0);
        assert!(tray.try_recv().is_err(), "closing must not request exit");
        unsafe { SendMessageW(tray.owner, TRAY_CALLBACK, 0, NIN_SELECT as isize) };
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
        assert_eq!(tray.try_recv().unwrap(), TrayEvent::Restore);
    }

    #[test]
    fn keyboard_activation_restores_a_minimized_hidden_window() {
        let window = TestWindow::new();
        let tray = tray(&window);
        unsafe { ShowWindow(window.0, SW_MINIMIZE) };
        tray.hide().unwrap();
        unsafe { SendMessageW(tray.owner, TRAY_CALLBACK, 0, NIN_KEYSELECT as isize) };
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
        assert_eq!(unsafe { IsIconic(window.0) }, 0);
        assert_eq!(tray.try_recv().unwrap(), TrayEvent::Restore);
    }

    #[test]
    fn explicit_exit_requests_shutdown_instead_of_hiding_again() {
        let window = TestWindow::new();
        let tray = tray(&window);
        unsafe { SendMessageW(tray.owner, WM_COMMAND, EXIT_COMMAND as usize, 0) };
        assert_eq!(tray.try_recv().unwrap(), TrayEvent::Exit);
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
    }

    #[test]
    fn missing_tray_keeps_the_main_window_accessible() {
        let window = TestWindow::new();
        let tray = tray(&window);
        unsafe { Shell_NotifyIconW(NIM_DELETE, &tray.state.icon_data(tray.owner)) };
        assert!(tray.hide().is_err());
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
    }

    #[test]
    fn explorer_restart_recreates_the_tray_without_stopping_the_application() {
        let window = TestWindow::new();
        let tray = tray(&window);
        tray.hide().unwrap();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &tray.state.icon_data(tray.owner));
            SendMessageW(tray.owner, tray.state.taskbar_created, 0, 0);
        }
        assert!(tray.state.available.get());
        assert!(tray.try_recv().is_err());
        tray.hide().unwrap();
        assert_eq!(unsafe { IsWindowVisible(window.0) }, 0);
    }

    #[test]
    fn reopening_an_instance_restores_only_the_matching_data_directory() {
        let window = TestWindow::new();
        let directory = tempfile::tempdir().unwrap();
        let other_directory = tempfile::tempdir().unwrap();
        let icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        let tray = Tray::create(window.0, &instance_title(directory.path()), icon).unwrap();
        tray.hide().unwrap();
        assert!(!restore_existing_instance(other_directory.path()));
        assert!(restore_existing_instance(directory.path()));
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(
                &mut message,
                tray.owner,
                RESTORE_INSTANCE,
                RESTORE_INSTANCE,
                PM_REMOVE,
            ) != 0
            {
                DispatchMessageW(&message);
            }
        }
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
        assert_eq!(tray.try_recv().unwrap(), TrayEvent::Restore);
    }

    #[test]
    fn tray_destruction_removes_its_native_owner_window() {
        let window = TestWindow::new();
        let tray = tray(&window);
        let owner = tray.owner;
        drop(tray);
        assert_eq!(unsafe { IsWindow(owner) }, 0);
        assert_ne!(unsafe { IsWindowVisible(window.0) }, 0);
    }
}
