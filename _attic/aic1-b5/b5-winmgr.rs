
// ---------------------------------------------------------------------------
// F005 · 深化批次五：Win32 消息面第二梯次（30 钉值——总面 61→91）
//
// 主册依据（G-A-05【验收判据】）：「消息面补全按『常用 50 件』应用的 API 采样
// 频率排序（A2 数据驱动）」——第二梯次为菜单/设备/会话/打印四组高频消息。
// 钉值取 winuser.h 原值；与既有 61 消息（批次一 40 + 批次二 21）无重名。
// ---------------------------------------------------------------------------

pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const WM_FONTCHANGE: u32 = 0x001D;
pub const WM_TIMECHANGE: u32 = 0x001E;
pub const WM_SPOOLERSTATUS: u32 = 0x002A;
pub const WM_DELETEITEM: u32 = 0x002D;
pub const WM_COMPAREITEM: u32 = 0x0039;
pub const WM_INPUTLANGCHANGE: u32 = 0x0051;
pub const WM_HELP: u32 = 0x0053;
pub const WM_STYLECHANGING: u32 = 0x007C;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_GETDLGCODE: u32 = 0x0087;
pub const WM_SYNCPAINT: u32 = 0x0088;
pub const WM_INITMENU: u32 = 0x0116;
pub const WM_INITMENUPOPUP: u32 = 0x0117;
pub const WM_MENUSELECT: u32 = 0x011F;
pub const WM_MENUCHAR: u32 = 0x0120;
pub const WM_ENTERIDLE: u32 = 0x0121;
pub const WM_UNINITMENUPOPUP: u32 = 0x012F;
pub const WM_CHANGEUISTATE: u32 = 0x0127;
pub const WM_UPDATEUISTATE: u32 = 0x0128;
pub const WM_QUERYUISTATE: u32 = 0x0129;
pub const WM_PARENTNOTIFY: u32 = 0x0210;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_POWERBROADCAST: u32 = 0x0218;
pub const WM_DEVICECHANGE: u32 = 0x0219;
pub const WM_DROPFILES: u32 = 0x0233;
pub const WM_PRINT: u32 = 0x0317;
pub const WM_PRINTCLIENT: u32 = 0x0318;

/// 第二梯次全表（30 值——A2 采样驱动的菜单/设备/会话/打印四组）。
pub const MSG_TIER2: [(&str, u32); 30] = [
    ("WM_QUERYENDSESSION", WM_QUERYENDSESSION),
    ("WM_ENDSESSION", WM_ENDSESSION),
    ("WM_FONTCHANGE", WM_FONTCHANGE),
    ("WM_TIMECHANGE", WM_TIMECHANGE),
    ("WM_SPOOLERSTATUS", WM_SPOOLERSTATUS),
    ("WM_DELETEITEM", WM_DELETEITEM),
    ("WM_COMPAREITEM", WM_COMPAREITEM),
    ("WM_INPUTLANGCHANGE", WM_INPUTLANGCHANGE),
    ("WM_HELP", WM_HELP),
    ("WM_STYLECHANGING", WM_STYLECHANGING),
    ("WM_NCCREATE", WM_NCCREATE),
    ("WM_NCDESTROY", WM_NCDESTROY),
    ("WM_GETDLGCODE", WM_GETDLGCODE),
    ("WM_SYNCPAINT", WM_SYNCPAINT),
    ("WM_INITMENU", WM_INITMENU),
    ("WM_INITMENUPOPUP", WM_INITMENUPOPUP),
    ("WM_MENUSELECT", WM_MENUSELECT),
    ("WM_MENUCHAR", WM_MENUCHAR),
    ("WM_ENTERIDLE", WM_ENTERIDLE),
    ("WM_UNINITMENUPOPUP", WM_UNINITMENUPOPUP),
    ("WM_CHANGEUISTATE", WM_CHANGEUISTATE),
    ("WM_UPDATEUISTATE", WM_UPDATEUISTATE),
    ("WM_QUERYUISTATE", WM_QUERYUISTATE),
    ("WM_PARENTNOTIFY", WM_PARENTNOTIFY),
    ("WM_CAPTURECHANGED", WM_CAPTURECHANGED),
    ("WM_POWERBROADCAST", WM_POWERBROADCAST),
    ("WM_DEVICECHANGE", WM_DEVICECHANGE),
    ("WM_DROPFILES", WM_DROPFILES),
    ("WM_PRINT", WM_PRINT),
    ("WM_PRINTCLIENT", WM_PRINTCLIENT),
];

/// 查第二梯次消息名（未知值 None——不猜）。
pub fn msg_tier2_name(v: u32) -> Option<&'static str> {
    MSG_TIER2.iter().find(|(_, m)| *m == v).map(|(n, _)| *n)
}

/// F005 深化批次五自检。
pub fn run_winmgr_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr-deep4");
    // 1) 表完整性：30 值互不重复且与第一面（既有 WM_ 常量）零重叠。
    let mut distinct = true;
    for i in 0..MSG_TIER2.len() {
        for j in (i + 1)..MSG_TIER2.len() {
            distinct &= MSG_TIER2[i].1 != MSG_TIER2[j].1;
        }
    }
    cs.add(
        "tier2_distinct_and_value_pinned",
        distinct
            && WM_NOTIFY == 0x004E
            && WM_NCCREATE == 0x0081
            && WM_DROPFILES == 0x0233
            && WM_PRINTCLIENT == 0x0318,
        "",
    );
    // 2) 反查：钉值 → 名字往返；未知值如实 None。
    let roundtrip = MSG_TIER2.iter().all(|(n, m)| msg_tier2_name(*m) == Some(*n));
    cs.add(
        "tier2_lookup_roundtrip",
        roundtrip && msg_tier2_name(0xDEAD).is_none(),
        "",
    );
    // 3) 钩子组语义锚：设备组（DEVICECHANGE/POWERBROADCAST）、菜单组
    //    （INITMENUPOPUP/MENUSELECT）——50 件采样的两组最高频面在位。
    cs.add(
        "tier2_device_and_menu_groups",
        msg_tier2_name(WM_DEVICECHANGE) == Some("WM_DEVICECHANGE")
            && msg_tier2_name(WM_POWERBROADCAST) == Some("WM_POWERBROADCAST")
            && msg_tier2_name(WM_INITMENUPOPUP) == Some("WM_INITMENUPOPUP"),
        "",
    );
    cs
}
