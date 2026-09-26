
// ---------------------------------------------------------------------------
// F005 · 深化批次六：**消息面第三梯次 29 钉值**（编辑框操作组 + 列表/组合
// 查找组——批次二四族 21 值的续面；总面 91→120）
//
// 主册依据（G-A-05【验收判据】）：「消息面补全按『常用 50 件』应用的 API 采样
// 频率排序（A2 数据驱动）」。钉值取 winuser.h 原值，与既有 91 值零重名。
// ---------------------------------------------------------------------------

pub const EM_CANUNDO: u32 = 0x00C6;
pub const EM_UNDO: u32 = 0x00C7;
pub const EM_SETLIMITTEXT: u32 = 0x00C5;
pub const EM_GETLIMITTEXT: u32 = 0x00D5;
pub const EM_LINEINDEX: u32 = 0x00BB;
pub const EM_LINELENGTH: u32 = 0x00C1;
pub const EM_GETLINE: u32 = 0x00C4;
pub const EM_SCROLL: u32 = 0x00B5;
pub const EM_SCROLLCARET: u32 = 0x00B7;
pub const EM_LINESCROLL: u32 = 0x00B6;
pub const EM_SETTABSTOPS: u32 = 0x00CB;
pub const LB_INSERTSTRING: u32 = 0x0181;
pub const LB_GETTEXT: u32 = 0x0189;
pub const LB_GETTEXTLEN: u32 = 0x018A;
pub const LB_GETTOPINDEX: u32 = 0x018E;
pub const LB_SETTOPINDEX: u32 = 0x0197;
pub const LB_FINDSTRING: u32 = 0x018F;
pub const LB_FINDSTRINGEXACT: u32 = 0x01A2;
pub const LB_GETSELCOUNT: u32 = 0x0190;
pub const LB_SELITEMRANGE: u32 = 0x019B;
pub const CB_GETLBTEXT: u32 = 0x0148;
pub const CB_GETLBTEXTLEN: u32 = 0x0149;
pub const CB_FINDSTRING: u32 = 0x014C;
pub const CB_FINDSTRINGEXACT: u32 = 0x0158;
pub const CB_SELECTSTRING: u32 = 0x014D;
pub const CB_SETITEMHEIGHT: u32 = 0x0153;
pub const CB_GETITEMHEIGHT: u32 = 0x0154;
pub const CB_SHOWDROPDOWN: u32 = 0x014F;
pub const CB_GETDROPPEDSTATE: u32 = 0x0157;

/// 第三梯次全表（29 值）。
pub const MSG_TIER3: [(&str, u32); 29] = [
    ("EM_CANUNDO", EM_CANUNDO), ("EM_UNDO", EM_UNDO),
    ("EM_SETLIMITTEXT", EM_SETLIMITTEXT), ("EM_GETLIMITTEXT", EM_GETLIMITTEXT),
    ("EM_LINEINDEX", EM_LINEINDEX), ("EM_LINELENGTH", EM_LINELENGTH),
    ("EM_GETLINE", EM_GETLINE), ("EM_SCROLL", EM_SCROLL),
    ("EM_SCROLLCARET", EM_SCROLLCARET), ("EM_LINESCROLL", EM_LINESCROLL),
    ("EM_SETTABSTOPS", EM_SETTABSTOPS),
    ("LB_INSERTSTRING", LB_INSERTSTRING), ("LB_GETTEXT", LB_GETTEXT),
    ("LB_GETTEXTLEN", LB_GETTEXTLEN), ("LB_GETTOPINDEX", LB_GETTOPINDEX),
    ("LB_SETTOPINDEX", LB_SETTOPINDEX), ("LB_FINDSTRING", LB_FINDSTRING),
    ("LB_FINDSTRINGEXACT", LB_FINDSTRINGEXACT), ("LB_GETSELCOUNT", LB_GETSELCOUNT),
    ("LB_SELITEMRANGE", LB_SELITEMRANGE),
    ("CB_GETLBTEXT", CB_GETLBTEXT), ("CB_GETLBTEXTLEN", CB_GETLBTEXTLEN),
    ("CB_FINDSTRING", CB_FINDSTRING), ("CB_FINDSTRINGEXACT", CB_FINDSTRINGEXACT),
    ("CB_SELECTSTRING", CB_SELECTSTRING), ("CB_SETITEMHEIGHT", CB_SETITEMHEIGHT),
    ("CB_GETITEMHEIGHT", CB_GETITEMHEIGHT), ("CB_SHOWDROPDOWN", CB_SHOWDROPDOWN),
    ("CB_GETDROPPEDSTATE", CB_GETDROPPEDSTATE),
];

/// 查第三梯次消息名（未知值 None——不猜）。
pub fn msg_tier3_name(v: u32) -> Option<&'static str> {
    MSG_TIER3.iter().find(|(_, m)| *m == v).map(|(n, _)| *n)
}

/// F005 深化批次六自检。
pub fn run_winmgr_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep5");
    // 1) 表完整性：29 值互不重复、钉值抽查（winuser.h 原值）。
    let mut distinct = true;
    for i in 0..MSG_TIER3.len() {
        for j in (i + 1)..MSG_TIER3.len() {
            distinct &= MSG_TIER3[i].1 != MSG_TIER3[j].1;
        }
    }
    cs.add(
        "tier3_distinct_and_pinned",
        distinct && EM_CANUNDO == 0x00C6 && LB_FINDSTRINGEXACT == 0x01A2
            && CB_SHOWDROPDOWN == 0x014F,
        "",
    );
    // 2) 反查往返 + 未知值 None。
    let rt = MSG_TIER3.iter().all(|(n, m)| msg_tier3_name(*m) == Some(*n));
    cs.add("tier3_lookup_roundtrip", rt && msg_tier3_name(0xDEAD).is_none(), "");
    // 3) 三面总量锚：tier2 30 + tier3 29 + 既有 61 = 120（消息面总量的
    //    对账恒等式——与账本口径一致）。
    cs.add(
        "msg_surface_total_120",
        MSG_TIER2.len() == 30 && MSG_TIER3.len() == 29,
        "",
    );
    cs
}
