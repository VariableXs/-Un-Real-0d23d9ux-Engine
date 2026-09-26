
// ---------------------------------------------------------------------------
// F005 · 深化批次八：系统命令 SC_ 组钉值（窗口菜单/标题栏发出的系统命令——
// winuser.h 钉值）+ 派发表（SC_CLOSE 走关闭确认路径，SC_MINIMIZE 走最小化）。
//
// 语义：WM_SYSCOMMAND 的 wParam 低 4 位是内部使用位，取值前须掩掉（0xFFF0）。
// ---------------------------------------------------------------------------

pub const SC_SIZE: u32 = 0xF000;
pub const SC_MOVE: u32 = 0xF010;
pub const SC_MINIMIZE: u32 = 0xF020;
pub const SC_MAXIMIZE: u32 = 0xF030;
pub const SC_CLOSE: u32 = 0xF060;
pub const SC_RESTORE: u32 = 0xF120;

/// 系统命令动作（可观测落点——不直接执行，交窗口管理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysAction {
    StartSizing,
    StartMove,
    Minimize,
    Maximize,
    Restore,
    RequestClose,
    Unknown(u32),
}

/// WM_SYSCOMMAND 派发（先掩内部位再识别——不掩会把 0xF061 之类判 Unknown）。
pub fn syscommand_dispatch(wparam: u32) -> SysAction {
    match wparam & 0xFFF0 {
        SC_SIZE => SysAction::StartSizing,
        SC_MOVE => SysAction::StartMove,
        SC_MINIMIZE => SysAction::Minimize,
        SC_MAXIMIZE => SysAction::Maximize,
        SC_RESTORE => SysAction::Restore,
        SC_CLOSE => SysAction::RequestClose,
        other => SysAction::Unknown(other),
    }
}

/// F005 深化批次八自检。
fn run_winmgr_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep7");
    // 1) 内部位掩码：SC_CLOSE | 1..15 任意内部位仍判 RequestClose。
    let a = syscommand_dispatch(SC_CLOSE);
    let b = syscommand_dispatch(SC_CLOSE | 0x0002);
    let c = syscommand_dispatch(SC_CLOSE | 0x000F);
    cs.add(
        "sc_mask_internal_bits",
        a == SysAction::RequestClose && b == SysAction::RequestClose && c == SysAction::RequestClose,
        "",
    );
    // 2) 六命令钉值全派发正确。
    cs.add(
        "sc_dispatch_table",
        syscommand_dispatch(SC_MINIMIZE) == SysAction::Minimize
            && syscommand_dispatch(SC_MAXIMIZE) == SysAction::Maximize
            && syscommand_dispatch(SC_RESTORE) == SysAction::Restore
            && syscommand_dispatch(SC_SIZE) == SysAction::StartSizing
            && syscommand_dispatch(SC_MOVE) == SysAction::StartMove,
        "",
    );
    // 3) 未登记命令如实 Unknown（携带掩码后的原值——不丢诊断信息）。
    let u = syscommand_dispatch(0xF100);
    cs.add(
        "sc_unknown_honest",
        u == SysAction::Unknown(0xF100),
        "",
    );
    cs
}
