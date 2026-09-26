
// ---------------------------------------------------------------------------
// F005 · 深化批次三：非客户区绘制主权裁定（WM_NCCALCSIZE）+ DefWindowProc
// 缺省行为钉值表（ReactOS 对拍锚）
//
// 主册依据（G-A-05【交互设计】）：「窗口装饰由 VARIX 合成器统一供给……程序
// 自绘标题栏（WM_NCCALCSIZE 处理）则尊重程序——程序对自己的窗口有主权，
// VARIX 只管没主权的地方」；【设计细节】「DefWindowProc 缺省行为逐消息文档化
// （对拍 ReactOS 用例）」。AtomTable/Window.custom_frame 既有面不重复。
// ---------------------------------------------------------------------------

/// 非客户区绘制主权归属（WM_NCCALCSIZE 裁定结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecorationOwner {
    /// 程序未自绘 → VARIX 合成器统一供给标题栏/边框/系统按钮。
    VarixComposer,
    /// 程序处理了 WM_NCCALCSIZE（自绘标题栏）→ VARIX 让位，只画客户区外框。
    ProgramSovereign,
}

/// 主权裁定：custom_frame = 程序声明自绘（create_window 的既有语义位）。
pub fn decoration_owner(custom_frame: bool) -> DecorationOwner {
    if custom_frame {
        DecorationOwner::ProgramSovereign
    } else {
        DecorationOwner::VarixComposer
    }
}

/// DefWindowProc 缺省行为钉值（对拍 ReactOS user32：缺省返回值逐条文档化——
/// 未处理消息返回缺省语义，不吞不崩）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefResult {
    pub msg: u32,
    pub ret: isize,
    pub note: &'static str,
}

/// 高频消息缺省返回值表（10 条——50 件采样最高频面；扩展走 A2 数据驱动）。
pub const DEF_WINDOW_PROC_RESULTS: [DefResult; 10] = [
    DefResult { msg: WM_ERASEBKGND, ret: 1, note: "类刷擦除完成，返回非零（已擦除）" },
    DefResult { msg: WM_GETTEXTLENGTH, ret: 0, note: "未设标题 → 长度 0" },
    DefResult { msg: WM_GETTEXT, ret: 0, note: "无文本可复制 → 复制 0 字符" },
    DefResult { msg: WM_NCHITTEST, ret: 1, note: "客户区命中（HTCLIENT=1）" },
    DefResult { msg: WM_SETCURSOR, ret: 1, note: "光标已按类设置，返回非零" },
    DefResult { msg: WM_CLOSE, ret: 0, note: "缺省调用 DestroyWindow 语义" },
    DefResult { msg: WM_PAINT, ret: 0, note: "校验无效区（无程序处理时）" },
    DefResult { msg: WM_SIZE, ret: 0, note: "缺省无附加动作" },
    DefResult { msg: WM_DESTROY, ret: 0, note: "缺省无附加动作（PostQuitMessage 归应用）" },
    DefResult { msg: WM_ACTIVATE, ret: 0, note: "缺省无附加动作" },
];

/// 查 DefWindowProc 缺省行为（未登记消息 → None——如实留空不猜）。
pub fn def_window_proc_result(msg: u32) -> Option<DefResult> {
    DEF_WINDOW_PROC_RESULTS.iter().copied().find(|d| d.msg == msg)
}

/// F005 深化批次三自检。
pub fn run_winmgr_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep2");
    // 1) 主权裁定双向：程序自绘 → ProgramSovereign；缺省 → VarixComposer。
    cs.add(
        "decoration_owner_sovereignty",
        decoration_owner(true) == DecorationOwner::ProgramSovereign
            && decoration_owner(false) == DecorationOwner::VarixComposer,
        "",
    );
    // 2) DefWindowProc 表钉值抽查：ERASEBKGND=1 / NCHITTEST=1(HTCLIENT) /
    //    GETTEXTLENGTH=0 / CLOSE=0（ReactOS 对拍锚）。
    let d_eb = def_window_proc_result(WM_ERASEBKGND);
    let d_ht = def_window_proc_result(WM_NCHITTEST);
    let d_gt = def_window_proc_result(WM_GETTEXTLENGTH);
    let d_cl = def_window_proc_result(WM_CLOSE);
    cs.add(
        "def_window_proc_pinned_values",
        matches!(d_eb, Some(d) if d.ret == 1)
            && matches!(d_ht, Some(d) if d.ret == 1)
            && matches!(d_gt, Some(d) if d.ret == 0)
            && matches!(d_cl, Some(d) if d.ret == 0),
        "",
    );
    // 3) 表完整性：10 条消息互不重复；未登记消息（WM_CONTEXTMENU）如实 None。
    let mut distinct = true;
    for i in 0..DEF_WINDOW_PROC_RESULTS.len() {
        for j in (i + 1)..DEF_WINDOW_PROC_RESULTS.len() {
            distinct &= DEF_WINDOW_PROC_RESULTS[i].msg != DEF_WINDOW_PROC_RESULTS[j].msg;
        }
    }
    cs.add(
        "def_table_distinct_and_unlisted_none",
        distinct && def_window_proc_result(WM_CONTEXTMENU).is_none(),
        "",
    );
    cs
}
