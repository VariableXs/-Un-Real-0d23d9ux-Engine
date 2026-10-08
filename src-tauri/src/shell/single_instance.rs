//! L3 shell — single_instance.rs：单实例守卫。
//!
//! 动机（实机反馈）：全局快捷键是系统级资源（RegisterHotKey），双开 Variable
//! 时第二实例整表注册全部失败，前端弹出一整排"注册失败（被系统或其他软件占用）"。
//! 命名互斥体最轻量：不引入 tauri-plugin-single-instance 依赖，也不做窗口聚焦
//! 唤起（托盘 V 图标即可回到既有实例）。
//! 仅 Windows 有真实行为；其余平台直接放行。

/// 进程级互斥体句柄持有者：句柄必须与进程同寿，绝不可 Close（见 `enforce`）。
/// 以 AtomicPtr 留存裸 HANDLE —— 只做"留住句柄"这一件事，不参与任何并发读写。
#[cfg(windows)]
static GUARD_HANDLE: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// 已有实例在运行 → 弹系统消息框提示并退出当前进程（exit code 0）；
/// 没有实例在运行 → 创建互斥体并由本实例持有至进程退出。
pub fn enforce() {
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{
            CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
        };
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

        const MUTEX_NAME: windows::core::PCWSTR = w!("Variable-1.0sno9u-SingleInstance");
        let existing = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, MUTEX_NAME) };
        if existing.is_err() {
            // 第十三轮大检查修复：没有既有实例 → 本实例必须**创建**互斥体并
            // 持有到进程退出，后续实例的 OpenMutexW 才能探测到。此前只有
            // Open 探测、从未 Create——守卫完全失效，双开畅通无阻，全局
            // 快捷键整表注册失败（正是本守卫要解决的问题）。
            // 句柄故意不 Close：互斥体生命周期必须与进程一致，进程退出时
            // 由操作系统回收（关闭句柄会立即销毁互斥体，守卫再次失效）。
            // HANDLE 是 Copy 值类型、无 Drop，`mem::forget` 是空操作（编译器已警告），
            // 故改为把句柄存进进程级 static —— 显式表达"生命周期 = 进程生命周期"，
            // 且不随 windows-rs 版本对 HANDLE Drop 语义的差异而改变行为。
            if let Ok(h) = unsafe { CreateMutexW(None, false, MUTEX_NAME) } {
                GUARD_HANDLE.store(h.0, std::sync::atomic::Ordering::Relaxed);
            }
            return; // 正常启动
        }
        unsafe {
            let title = w!("Variable");
            let text = w!("Variable 已在运行（托盘 V 图标可返回）。\r\nVariable is already running.\r\n\r\n重复启动会导致全部全局快捷键注册失败，本次启动已取消。");
            MessageBoxW(HWND::default(), text, title, MB_OK | MB_ICONINFORMATION);
        }
        std::process::exit(0);
    }
    #[cfg(not(windows))]
    {
        // 非 Windows：无 RegisterHotKey 抢占问题，放行
    }
}
