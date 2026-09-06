//! Native Windows System Tray implementation for OOS-Lite.
//! Shows an icon in the Windows notification area (System Tray) with right-click menu
//! to open dashboard, open drive, or cleanly exit.

#[cfg(windows)]
#[allow(non_snake_case, dead_code)]
pub mod windows {
    use std::os::windows::process::CommandExt;
    use std::ptr::{null, null_mut};

    /// Prevents console windows from flashing when spawning child processes
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    /// MAKEINTRESOURCEW(id) — converts a resource ID integer to a pointer for Win32 API
    macro_rules! MAKEINTRESOURCEW {
        ($id:expr) => {
            $id as usize as *const u16
        };
    }

    pub type HWND = *mut std::ffi::c_void;
    pub type HICON = *mut std::ffi::c_void;
    pub type HMENU = *mut std::ffi::c_void;
    pub type HINSTANCE = *mut std::ffi::c_void;
    pub type HCURSOR = *mut std::ffi::c_void;
    pub type HBRUSH = *mut std::ffi::c_void;
    pub type LRESULT = isize;
    pub type WPARAM = usize;
    pub type LPARAM = isize;

    pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

    #[repr(C)]
    pub struct WNDCLASSEXW {
        pub cbSize: u32,
        pub style: u32,
        pub lpfnWndProc: WNDPROC,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: HINSTANCE,
        pub hIcon: HICON,
        pub hCursor: HCURSOR,
        pub hbrBackground: HBRUSH,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
        pub hIconSm: HICON,
    }

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: u32,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: u32,
        pub pt: POINT,
    }

    #[repr(C)]
    pub struct NOTIFYICONDATAW {
        pub cbSize: u32,
        pub hWnd: HWND,
        pub uID: u32,
        pub uFlags: u32,
        pub uCallbackMessage: u32,
        pub hIcon: HICON,
        pub szTip: [u16; 128],
        pub dwState: u32,
        pub dwStateMask: u32,
        pub szInfo: [u16; 256],
        pub uTimeoutOrVersion: u32,
        pub szInfoTitle: [u16; 64],
        pub dwInfoFlags: u32,
        pub guidItem: [u8; 16],
        pub hBalloonIcon: HICON,
    }

    pub const WM_DESTROY: u32 = 0x0002;
    pub const WM_APP: u32 = 0x8000;
    pub const TRAY_MSG: u32 = WM_APP + 100;

    pub const WM_LBUTTONUP: isize = 0x0202;
    pub const WM_RBUTTONUP: isize = 0x0205;
    pub const WM_LBUTTONDBLCLK: isize = 0x0203;
    pub const WM_CONTEXTMENU: isize = 0x007B;

    pub const NIM_ADD: u32 = 0;
    pub const NIM_MODIFY: u32 = 1;
    pub const NIM_DELETE: u32 = 2;

    pub const NIF_MESSAGE: u32 = 0x00000001;
    pub const NIF_ICON: u32 = 0x00000002;
    pub const NIF_TIP: u32 = 0x00000004;

    pub const MF_STRING: u32 = 0x00000000;
    pub const MF_SEPARATOR: u32 = 0x00000800;
    pub const TPM_RETURNCMD: u32 = 0x0100;
    pub const TPM_NONOTIFY: u32 = 0x0080;
    pub const IMAGE_ICON: u32 = 1;
    pub const LR_LOADFROMFILE: u32 = 0x00000010;
    pub const LR_DEFAULTSIZE: u32 = 0x00000040;

    #[link(name = "user32")]
    extern "system" {
        pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> u16;
        pub fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn CreateWindowExW(
            dwExStyle: u32,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: u32,
            X: i32,
            Y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: HWND,
            hMenu: HMENU,
            hInstance: HINSTANCE,
            lpParam: *mut std::ffi::c_void,
        ) -> HWND;
        pub fn DestroyWindow(hWnd: HWND) -> i32;
        pub fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: u32, wMsgFilterMax: u32) -> i32;
        pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn PostQuitMessage(nExitCode: i32);
        pub fn GetCursorPos(lpPoint: *mut POINT) -> i32;
        pub fn SetForegroundWindow(hWnd: HWND) -> i32;
        pub fn CreatePopupMenu() -> HMENU;
        pub fn AppendMenuW(hMenu: HMENU, uFlags: u32, uIDNewItem: usize, lpNewItem: *const u16) -> i32;
        pub fn TrackPopupMenu(
            hMenu: HMENU,
            uFlags: u32,
            x: i32,
            y: i32,
            nReserved: i32,
            hWnd: HWND,
            prcRect: *const std::ffi::c_void,
        ) -> i32;
        pub fn DestroyMenu(hMenu: HMENU) -> i32;
        pub fn LoadIconW(hInstance: HINSTANCE, lpIconName: *const u16) -> HICON;
        pub fn LoadImageW(
            hInst: HINSTANCE,
            name: *const u16,
            type_: u32,
            cx: i32,
            cy: i32,
            fuLoad: u32,
        ) -> *mut std::ffi::c_void;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HINSTANCE;
    }

    #[link(name = "shell32")]
    extern "system" {
        pub fn Shell_NotifyIconW(dwMessage: u32, lpData: *const NOTIFYICONDATAW) -> i32;
    }

    pub fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn open_dashboard() {
        crate::ui::open_desktop_window("http://127.0.0.1:3000");
    }

    static mut GLOBAL_TRAY_NID: Option<NOTIFYICONDATAW> = None;

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == TRAY_MSG {
            if lparam == WM_LBUTTONUP || lparam == WM_LBUTTONDBLCLK {
                open_dashboard();
                return 0;
            } else if lparam == WM_RBUTTONUP || lparam == WM_CONTEXTMENU {
                let hmenu = CreatePopupMenu();
                let m_dash = to_wide("⚡ Open OOS-Lite Dashboard");
                let m_drive = to_wide("📁 Open Virtual Drive (Z:\\)");
                let m_exit = to_wide("❌ Exit OOS-Lite");

                AppendMenuW(hmenu, MF_STRING, 1, m_dash.as_ptr());
                AppendMenuW(hmenu, MF_STRING, 2, m_drive.as_ptr());
                AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
                AppendMenuW(hmenu, MF_STRING, 3, m_exit.as_ptr());

                SetForegroundWindow(hwnd);
                let mut pt: POINT = std::mem::zeroed();
                GetCursorPos(&mut pt);

                let cmd = TrackPopupMenu(
                    hmenu,
                    TPM_RETURNCMD | TPM_NONOTIFY,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    null(),
                );
                DestroyMenu(hmenu);

                match cmd {
                    1 => {
                        open_dashboard();
                    }
                    2 => {
                        let _ = std::process::Command::new("explorer.exe")
                            .arg(r"Z:\")
                            .creation_flags(CREATE_NO_WINDOW)
                            .spawn();
                    }
                    3 => {
                        crate::mount::windows::unmap_drive('Z').ok();
                        if let Some(ref nid) = GLOBAL_TRAY_NID {
                            Shell_NotifyIconW(NIM_DELETE, nid);
                        }
                        std::process::exit(0);
                    }
                    _ => {}
                }
                return 0;
            }
        } else if msg == WM_DESTROY {
            if let Some(ref nid) = GLOBAL_TRAY_NID {
                Shell_NotifyIconW(NIM_DELETE, nid);
            }
            PostQuitMessage(0);
            return 0;
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Spawns the native Windows System Tray thread
    pub fn spawn_system_tray() {
        std::thread::Builder::new()
            .name("oos-system-tray".into())
            .spawn(|| unsafe {
                let class_name = to_wide("OOSLiteTrayWindowClass");
                let hinstance = GetModuleHandleW(null());

                // Try to load embedded resource icon (ID 1 from app.ico via winres)
                // MAKEINTRESOURCEW(1) is used to address icon resource ID 1
                let mut hicon = LoadIconW(hinstance, MAKEINTRESOURCEW!(1));
                if hicon.is_null() {
                    // Try to load app.ico from the directory of the running executable
                    if let Ok(exe_path) = std::env::current_exe() {
                        if let Some(parent) = exe_path.parent() {
                            let ico_path = parent.join("app.ico");
                            if ico_path.is_file() {
                                let ico_wide = to_wide(&ico_path.display().to_string());
                                hicon = LoadImageW(
                                    null_mut(),
                                    ico_wide.as_ptr(),
                                    IMAGE_ICON,
                                    0,
                                    0,
                                    LR_LOADFROMFILE | LR_DEFAULTSIZE,
                                ) as HICON;
                            }
                        }
                    }
                }
                if hicon.is_null() {
                    // Fall back to the default application icon (IDI_APPLICATION = 32512)
                    hicon = LoadIconW(null_mut(), MAKEINTRESOURCEW!(32512));
                }

                let wnd_class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: wndproc,
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: hicon,
                    hCursor: null_mut(),
                    hbrBackground: null_mut(),
                    lpszMenuName: null(),
                    lpszClassName: class_name.as_ptr(),
                    hIconSm: hicon,
                };

                RegisterClassExW(&wnd_class);

                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    to_wide("OOS-Lite Tray").as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    hinstance,
                    null_mut(),
                );

                if hwnd.is_null() {
                    return;
                }

                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1001;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                nid.uCallbackMessage = TRAY_MSG;
                nid.hIcon = hicon;

                let tip = to_wide("OOS-Lite Vault & Drive (Z:\\)");
                for (i, &c) in tip.iter().take(127).enumerate() {
                    nid.szTip[i] = c;
                }

                Shell_NotifyIconW(NIM_ADD, &nid);
                GLOBAL_TRAY_NID = Some(nid);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .expect("Failed to spawn system tray thread");
    }
}

#[cfg(not(windows))]
pub mod windows {
    pub fn spawn_system_tray() {}
}
