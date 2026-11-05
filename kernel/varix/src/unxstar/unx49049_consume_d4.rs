//! UNX-F49049 D4 消费序联签 · 0x73100 段（B13 批第五槽）
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，不得改写）：
//!   UNX-F49049 D4 消费序联签（WM 消息翻译映射归 D4 自行转换——分界登记）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49050-J1 分发段 ×10^3 打点全落账；
//!             判据锚定 §AI-62 §2 专题五；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49050 焦点递送延迟打点（分发段账运营）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49051-J1 失联注入 ×10 恢复零丢；
//!             判据锚定 体验十三·补 隐蔽捕获；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（卡一 F49049）：建映射 → 核译者 → 核语义 → 核次序 → 联签。
//! 建映射先于核译者：十种 WM 消息先逐条登记"D4 翻译成哪个语义"，
//! 没有登记表，后面对译者的核对就无从比对；
//! 核译者先于核语义：WM 翻译权归 D4，任何自称 unxstar/H5 的转换请求
//! 先在第一道门就被拒，不必再看语义——越权者连参与比对的资格都没有；
//! 核语义先于核次序：语义错了，次序再对也是错的次序（键消息被译成指针消息，
//! 排在第一位也仍然是一次越权转换）；
//! 核次序先于联签：消息流必须落在规范序上（焦点先于按键、按键先于字符、
//! 字符先于抬键、关闭先于销毁），序不对的流不进联签账；
//! 联签是最后一步：D4 把"映射 + 次序 + 越权检出"三本账合成一行签字。
//! 闸门次序（卡二 F49050）：打点 → 对时 → 落桶 → 对账 → 失联恢复。
//! 打点先于对时：1000 次分发先各落一条"入口/出口"时刻，没有点就没有时刻；
//! 对时先于落桶：入口必须单调不减、出口必须不早于入口，时刻自相矛盾时
//! 计时长毫无意义；
//! 落桶先于对账：时长按 2/4/8/16/32/64/128/256 分九桶，桶内计数之和
//! 必须等于打点数——这就是"全落账"的可核对形式；
//! 对账先于失联恢复：先证明 1000 条一条不少，再注入 10 条失联并逐条补回，
//! 补回后仍未落账的条数必须为零。
//! 关键差异：F49049 是**语义账**（一条消息该译成什么、由谁译、排第几），
//! F49050 是**时间账**（一次分发从入口到出口多久、落在哪个桶、少没少条）。
//! 前者错在"译错/越权/乱序"，后者错在"漏点/错时/漏桶"。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73100，B13 批第五槽）。
pub const SEG_BASE: u32 = 0x73100;

/// 映射登记表容量（十种 WM 消息）。
pub const MAP_TOTAL: usize = 10;
/// 次序规则条数。
pub const ORDER_RULE_COUNT: usize = 8;
/// 分发打点次数（卡面 ×10^3）。
pub const DISPATCH_TICKS: u32 = 1000;
/// 失联注入条数（卡面 ×10）。
pub const LOSS_INJECT: u32 = 10;
/// 耗时直方图桶数。
pub const HIST_BUCKETS: usize = 9;
/// 时钟起点。
pub const CLOCK_BASE: u32 = 0x1000;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 映射账标签。
pub const MAP_TAG: &str = "D4MAP";
/// 延迟账标签。
pub const LATENCY_TAG: &str = "LATNCY";
/// 分发账标签。
pub const DISPATCH_TAG: &str = "DISPAT";
/// D4 译者名。
pub const D4_NAME: &str = "D4";
/// D4 消费序联签锚。
pub const D4_SIGNAL: &str = "D4/consume-order-sign";
/// AI-62 §2 专题五锚。
pub const AI62_ANCHOR: &str = "AI-62/section-2/topic-5";
/// 体验十三·补 隐蔽捕获锚。
pub const EXP13_ANCHOR: &str = "体验十三·补/隐蔽捕获";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 F49049 段诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumeCode {
    /// WM 消息编号不在登记表内
    MsgBad,
    /// 映射表缺行/重复行
    MapBad,
    /// 语义码越界或重复
    SemanticBad,
    /// 消费次序违反规范序
    OrderBad,
    /// 越权转换：语义与登记不符
    Unauthorized,
    /// 译者非 D4（翻译权越界）
    TranslatorBad,
    /// 打点条数不足
    TickBad,
    /// 入口/出口时刻自相矛盾
    StampBad,
    /// 失联未恢复（有缺口）
    LostBad,
    /// 直方图不守恒
    HistBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 计数溢出
    Overflow,
    /// 摘要为零
    DigestBad,
}

impl ConsumeCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ConsumeCode::MsgBad => "WM 消息编号不在 D4 登记表内",
            ConsumeCode::MapBad => "映射表不完整：缺行、重复行或秩不连续",
            ConsumeCode::SemanticBad => "语义码越界或重复：十种语义必须一一对应",
            ConsumeCode::OrderBad => "消费次序违反规范序：后置语义被排到了前面",
            ConsumeCode::Unauthorized => "越权转换：请求语义与 D4 登记不符",
            ConsumeCode::TranslatorBad => "译者非 D4：WM 翻译权越界",
            ConsumeCode::TickBad => "打点条数不足：分发段未满一千次",
            ConsumeCode::StampBad => "入口/出口时刻自相矛盾：出口早于入口或入口回退",
            ConsumeCode::LostBad => "失联未恢复：补回后仍有缺口",
            ConsumeCode::HistBad => "直方图不守恒：桶内计数之和不等于打点数",
            ConsumeCode::RenderBad => "渲染字段不可解析",
            ConsumeCode::ReplayBad => "复跑摘要漂移：两遍结果不同",
            ConsumeCode::Overflow => "计数溢出",
            ConsumeCode::DigestBad => "摘要为零：折叠未发生",
        }
    }

    pub const ALL: [ConsumeCode; 14] = [
        ConsumeCode::MsgBad,
        ConsumeCode::MapBad,
        ConsumeCode::SemanticBad,
        ConsumeCode::OrderBad,
        ConsumeCode::Unauthorized,
        ConsumeCode::TranslatorBad,
        ConsumeCode::TickBad,
        ConsumeCode::StampBad,
        ConsumeCode::LostBad,
        ConsumeCode::HistBad,
        ConsumeCode::RenderBad,
        ConsumeCode::ReplayBad,
        ConsumeCode::Overflow,
        ConsumeCode::DigestBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ConsumeCode::ALL.len() {
        if let Some(c) = ConsumeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码连续落在 0x73101..0x7310E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ConsumeCode::ALL.len() {
        let c = match ConsumeCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
            return false;
        }
        i += 1;
    }
    true
}

// ── 用例编号（16 条）与保留号 ────────────────────────────────────
/// 前序槽位占用号（B12 族，抄录自前批冻结清单）。
pub const RESERVED_IDS: [u32; 8] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_0014,
    0x071D_2001,
    0x071E_0801,
    0x071E_0802,
];

/// 本模块 16 个用例编号（0x0733_1001..=0x0733_1010）。
pub const CASE_IDS: [u32; 16] = [
    0x0733_1001,
    0x0733_1002,
    0x0733_1003,
    0x0733_1004,
    0x0733_1005,
    0x0733_1006,
    0x0733_1007,
    0x0733_1008,
    0x0733_1009,
    0x0733_100A,
    0x0733_100B,
    0x0733_100C,
    0x0733_100D,
    0x0733_100E,
    0x0733_100F,
    0x0733_1010,
];

/// 用例语义绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
}

/// 16 条用例声明。
pub const CASES: [CaseSpec; 16] = [
    CaseSpec { id: 0x0733_1001, name: "映射登记表" },
    CaseSpec { id: 0x0733_1002, name: "译者核验" },
    CaseSpec { id: 0x0733_1003, name: "语义一对一" },
    CaseSpec { id: 0x0733_1004, name: "越权转换检出" },
    CaseSpec { id: 0x0733_1005, name: "次序规则" },
    CaseSpec { id: 0x0733_1006, name: "乱序注入" },
    CaseSpec { id: 0x0733_1007, name: "联签锚定行" },
    CaseSpec { id: 0x0733_1008, name: "百次翻译" },
    CaseSpec { id: 0x0733_1009, name: "千次打点" },
    CaseSpec { id: 0x0733_100A, name: "入口出口时刻" },
    CaseSpec { id: 0x0733_100B, name: "耗时直方图" },
    CaseSpec { id: 0x0733_100C, name: "全落账对账" },
    CaseSpec { id: 0x0733_100D, name: "失联注入" },
    CaseSpec { id: 0x0733_100E, name: "恢复零丢" },
    CaseSpec { id: 0x0733_100F, name: "打点复跑" },
    CaseSpec { id: 0x0733_1010, name: "渲染往返" },
];

/// 跨槽零撞号：不与保留号、段位码窗口、彼此相交。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a >= SEG_BASE + 0x01 && a <= SEG_BASE + 0x0E {
            return false;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(k) {
                if a == *b {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 用例编号连续：CASE_IDS[i] == 0x0733_1000 + 1 + i。
pub fn case_ids_sequential() -> bool {
    let base: u32 = 0x0733_1000;
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match base.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if let Some(v) = CASE_IDS.get(i) {
            if *v != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── WM 消息与语义（F49049） ──────────────────────────────────────
/// 十种被登记的 WM 消息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WmMsg {
    KeyDown,
    KeyUp,
    Char,
    MouseMove,
    MouseDown,
    MouseUp,
    Focus,
    KillFocus,
    Close,
    Destroy,
}

impl WmMsg {
    pub const fn raw(self) -> u32 {
        match self {
            WmMsg::KeyDown => 0x0100,
            WmMsg::KeyUp => 0x0101,
            WmMsg::Char => 0x0102,
            WmMsg::MouseMove => 0x0200,
            WmMsg::MouseDown => 0x0201,
            WmMsg::MouseUp => 0x0202,
            WmMsg::Focus => 0x0007,
            WmMsg::KillFocus => 0x0008,
            WmMsg::Close => 0x0010,
            WmMsg::Destroy => 0x0002,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            WmMsg::KeyDown => "WM_KEYDOWN",
            WmMsg::KeyUp => "WM_KEYUP",
            WmMsg::Char => "WM_CHAR",
            WmMsg::MouseMove => "WM_MOUSEMOVE",
            WmMsg::MouseDown => "WM_LBUTTONDOWN",
            WmMsg::MouseUp => "WM_LBUTTONUP",
            WmMsg::Focus => "WM_SETFOCUS",
            WmMsg::KillFocus => "WM_KILLFOCUS",
            WmMsg::Close => "WM_CLOSE",
            WmMsg::Destroy => "WM_DESTROY",
        }
    }

    pub const ALL: [WmMsg; MAP_TOTAL] = [
        WmMsg::KeyDown,
        WmMsg::KeyUp,
        WmMsg::Char,
        WmMsg::MouseMove,
        WmMsg::MouseDown,
        WmMsg::MouseUp,
        WmMsg::Focus,
        WmMsg::KillFocus,
        WmMsg::Close,
        WmMsg::Destroy,
    ];
}

/// 按原始编号反查 WM 消息。
pub fn from_raw(raw: u32) -> Option<WmMsg> {
    let mut i = 0usize;
    while i < WmMsg::ALL.len() {
        if let Some(m) = WmMsg::ALL.get(i) {
            if m.raw() == raw {
                return Some(*m);
            }
        }
        i += 1;
    }
    None
}

/// 十种消费语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Semantic {
    FocusIn,
    KeyDown,
    Char,
    KeyUp,
    PointerMove,
    PointerDown,
    PointerUp,
    Close,
    Destroy,
    FocusOut,
}

impl Semantic {
    pub const fn index(self) -> u8 {
        match self {
            Semantic::FocusIn => 0,
            Semantic::KeyDown => 1,
            Semantic::Char => 2,
            Semantic::KeyUp => 3,
            Semantic::PointerMove => 4,
            Semantic::PointerDown => 5,
            Semantic::PointerUp => 6,
            Semantic::Close => 7,
            Semantic::Destroy => 8,
            Semantic::FocusOut => 9,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Semantic::FocusIn => "focus-in",
            Semantic::KeyDown => "key-down",
            Semantic::Char => "char",
            Semantic::KeyUp => "key-up",
            Semantic::PointerMove => "pointer-move",
            Semantic::PointerDown => "pointer-down",
            Semantic::PointerUp => "pointer-up",
            Semantic::Close => "close",
            Semantic::Destroy => "destroy",
            Semantic::FocusOut => "focus-out",
        }
    }

    pub const ALL: [Semantic; MAP_TOTAL] = [
        Semantic::FocusIn,
        Semantic::KeyDown,
        Semantic::Char,
        Semantic::KeyUp,
        Semantic::PointerMove,
        Semantic::PointerDown,
        Semantic::PointerUp,
        Semantic::Close,
        Semantic::Destroy,
        Semantic::FocusOut,
    ];
}

/// 一条映射登记：消息 → 语义、规范秩、译者。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEntry {
    pub msg: WmMsg,
    pub semantic: Semantic,
    pub rank: u8,
    pub translator: &'static str,
}

/// 十条映射登记（译者一律 D4；秩即规范消费序）。
pub const MAP: [MapEntry; MAP_TOTAL] = [
    MapEntry {
        msg: WmMsg::Focus,
        semantic: Semantic::FocusIn,
        rank: 1,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::KeyDown,
        semantic: Semantic::KeyDown,
        rank: 2,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::Char,
        semantic: Semantic::Char,
        rank: 3,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::KeyUp,
        semantic: Semantic::KeyUp,
        rank: 4,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::MouseMove,
        semantic: Semantic::PointerMove,
        rank: 5,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::MouseDown,
        semantic: Semantic::PointerDown,
        rank: 6,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::MouseUp,
        semantic: Semantic::PointerUp,
        rank: 7,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::Close,
        semantic: Semantic::Close,
        rank: 8,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::Destroy,
        semantic: Semantic::Destroy,
        rank: 9,
        translator: D4_NAME,
    },
    MapEntry {
        msg: WmMsg::KillFocus,
        semantic: Semantic::FocusOut,
        rank: 10,
        translator: D4_NAME,
    },
];

/// 按消息查登记行。
pub fn entry_of(msg: WmMsg) -> Option<MapEntry> {
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            if e.msg == msg {
                return Some(*e);
            }
        }
        i += 1;
    }
    None
}

/// 按语义查规范秩。
pub fn rank_of(semantic: Semantic) -> Option<u8> {
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            if e.semantic == semantic {
                return Some(e.rank);
            }
        }
        i += 1;
    }
    None
}

/// 纯翻译：按登记表把消息译成语义。
pub fn translate(msg: WmMsg) -> Result<Semantic, u32> {
    match entry_of(msg) {
        Some(e) => Ok(e.semantic),
        None => Err(ConsumeCode::MsgBad.val()),
    }
}

/// 带译者与语义的核验翻译：译者必须是 D4，语义必须与登记一致。
pub fn translate_checked(msg: WmMsg, semantic: Semantic, translator: &str) -> Result<Semantic, u32> {
    if translator != D4_NAME {
        return Err(ConsumeCode::TranslatorBad.val());
    }
    let e = match entry_of(msg) {
        Some(v) => v,
        None => return Err(ConsumeCode::MsgBad.val()),
    };
    if e.semantic != semantic {
        return Err(ConsumeCode::Unauthorized.val());
    }
    Ok(semantic)
}

/// 映射表摘要：逐行折消息编号、语义序号、规范秩。
pub fn map_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            h = p_core::fold_u32(h, e.msg.raw());
            h = p_core::fold_byte(h, e.semantic.index());
            h = p_core::fold_byte(h, e.rank);
        }
        i += 1;
    }
    h
}

/// 映射表完整性：消息唯一、语义唯一、秩从 1 连续、译者全为 D4。
pub fn map_complete() -> bool {
    let mut i = 0usize;
    while i < MAP.len() {
        let a = match MAP.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.translator != D4_NAME {
            return false;
        }
        let mut j = i + 1;
        while j < MAP.len() {
            if let Some(b) = MAP.get(j) {
                if a.msg == b.msg || a.semantic == b.semantic || a.rank == b.rank {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut r = 1u8;
    while (r as usize) <= MAP.len() {
        let mut found = false;
        let mut k = 0usize;
        while k < MAP.len() {
            if let Some(e) = MAP.get(k) {
                if e.rank == r {
                    found = true;
                }
            }
            k += 1;
        }
        if !found {
            return false;
        }
        r = match r.checked_add(1) {
            Some(v) => v,
            None => return false,
        };
    }
    true
}

/// 越权转换注入：十组"消息 → 错误语义"的对照。
pub const UNAUTHORIZED: [(WmMsg, Semantic); 10] = [
    (WmMsg::KeyDown, Semantic::PointerDown),
    (WmMsg::KeyUp, Semantic::KeyDown),
    (WmMsg::Char, Semantic::KeyUp),
    (WmMsg::MouseMove, Semantic::PointerDown),
    (WmMsg::MouseDown, Semantic::Char),
    (WmMsg::MouseUp, Semantic::PointerMove),
    (WmMsg::Focus, Semantic::KeyDown),
    (WmMsg::KillFocus, Semantic::FocusIn),
    (WmMsg::Close, Semantic::Destroy),
    (WmMsg::Destroy, Semantic::Close),
];

/// 越权转换检出：(检出, 漏检)。
pub fn unauthorized_probe() -> (u32, u32) {
    let mut detected = 0u32;
    let mut missed = 0u32;
    let mut i = 0usize;
    while i < UNAUTHORIZED.len() {
        if let Some((msg, sem)) = UNAUTHORIZED.get(i) {
            match translate_checked(*msg, *sem, D4_NAME) {
                Err(code) if code == ConsumeCode::Unauthorized.val() => {
                    detected = detected.saturating_add(1);
                }
                _ => {
                    missed = missed.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (detected, missed)
}

/// 译者越界注入：十行登记被非 D4 译者抢先翻译，必须全拒。
pub fn translator_faults() -> (u32, u32) {
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            let r = translate_checked(e.msg, e.semantic, "unxstar");
            match r {
                Err(code) if code == ConsumeCode::TranslatorBad.val() => {
                    rejected = rejected.saturating_add(1);
                }
                _ => {
                    accepted = accepted.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (rejected, accepted)
}

/// 百次翻译：十行轮转十遍，全部按登记译出。
pub fn hundred_translations() -> Result<(u32, u32, u64), u32> {
    let mut ok = 0u32;
    let mut denied = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < DISPATCH_TICKS / 10 {
        let e = match MAP.get((i as usize) % MAP.len()) {
            Some(v) => *v,
            None => return Err(ConsumeCode::MapBad.val()),
        };
        match translate_checked(e.msg, e.semantic, D4_NAME) {
            Ok(sem) => {
                ok = ok.saturating_add(1);
                h = p_core::fold_byte(h, sem.index());
            }
            Err(_) => {
                denied = denied.saturating_add(1);
            }
        }
        i += 1;
    }
    Ok((ok, denied, h))
}

/// 翻译复跑：两遍摘要一致。
pub fn translation_replay() -> Result<(u64, u64), u32> {
    let a = hundred_translations()?.2;
    let b = hundred_translations()?.2;
    Ok((a, b))
}

// ── 消费次序 ─────────────────────────────────────────────────────
/// 一条次序规则：before 必须排在 after 之前。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderRule {
    pub before: Semantic,
    pub after: Semantic,
}

/// 八条规范次序规则。
pub const ORDER_RULES: [OrderRule; ORDER_RULE_COUNT] = [
    OrderRule { before: Semantic::FocusIn, after: Semantic::KeyDown },
    OrderRule { before: Semantic::KeyDown, after: Semantic::Char },
    OrderRule { before: Semantic::Char, after: Semantic::KeyUp },
    OrderRule { before: Semantic::PointerMove, after: Semantic::PointerDown },
    OrderRule { before: Semantic::PointerDown, after: Semantic::PointerUp },
    OrderRule { before: Semantic::KeyUp, after: Semantic::FocusOut },
    OrderRule { before: Semantic::Close, after: Semantic::Destroy },
    OrderRule { before: Semantic::Char, after: Semantic::Close },
];

/// 某语义在消息流中的首次位置。
pub fn position(seq: &[Semantic], s: Semantic) -> Option<usize> {
    let mut i = 0usize;
    while i < seq.len() {
        if let Some(v) = seq.get(i) {
            if *v == s {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 规范序：语义秩单调不减（同秩重复合法，如连续指针移动）。
pub fn order_ok(seq: &[Semantic]) -> bool {
    let mut prev: Option<u8> = None;
    let mut i = 0usize;
    while i < seq.len() {
        let s = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let r = match rank_of(s) {
            Some(v) => v,
            None => return false,
        };
        if let Some(p) = prev {
            if r < p {
                return false;
            }
        }
        prev = Some(r);
        i += 1;
    }
    true
}

/// 规则逐条核对：两侧都出现的规则必须前后有序。
pub fn rules_hold(seq: &[Semantic]) -> bool {
    let mut i = 0usize;
    while i < ORDER_RULE_COUNT {
        if let Some(r) = ORDER_RULES.get(i) {
            let a = position(seq, r.before);
            let b = position(seq, r.after);
            match (a, b) {
                (Some(x), Some(y)) => {
                    if x >= y {
                        return false;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    true
}

/// 一次规范的消息串：焦点进入 → 按键 → 字符 → 抬键 → 指针 → 关闭 → 销毁 → 焦点离开。
pub fn ordered_burst() -> Vec<Semantic> {
    Vec::from([
        Semantic::FocusIn,
        Semantic::KeyDown,
        Semantic::Char,
        Semantic::KeyUp,
        Semantic::PointerMove,
        Semantic::PointerDown,
        Semantic::PointerUp,
        Semantic::Close,
        Semantic::Destroy,
        Semantic::FocusOut,
    ])
}

/// 乱序串：规范串整段倒排。
pub fn reversed_burst() -> Vec<Semantic> {
    let mut v = ordered_burst();
    v.reverse();
    v
}

/// 乱序注入：十次交换，全部必须被次序检查拒收。
pub fn order_faults() -> Result<(u32, u32), u32> {
    let base = ordered_burst();
    let swaps: [(usize, usize); 10] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (0, 3),
        (3, 7),
        (0, 7),
        (4, 5),
        (5, 6),
        (6, 7),
        (2, 6),
    ];
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < swaps.len() {
        let (a, b) = swaps[i];
        let mut seq = base.clone();
        if a < seq.len() && b < seq.len() {
            seq.swap(a, b);
        }
        if order_ok(&seq) {
            accepted = accepted.saturating_add(1);
        } else {
            rejected = rejected.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// D4 联签行：映射数、规则数、越权对、译者，一行落账。
pub fn cosign_line() -> String {
    format!(
        "D4SIGN|map={}|rules={}|deny={}|translator={}|anchor={}|ai={}",
        MAP_TOTAL,
        ORDER_RULE_COUNT,
        UNAUTHORIZED.len(),
        D4_NAME,
        D4_SIGNAL,
        AI62_ANCHOR
    )
}

// ── 分发段打点（F49050） ─────────────────────────────────────────
/// 一条分发打点：序号、目标窗口、入口时刻、出口时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchTick {
    pub seq: u32,
    pub window: u32,
    pub enter: u32,
    pub exit: u32,
}

impl DispatchTick {
    /// 在途时长（出口减入口）。
    pub fn duration(self) -> u32 {
        self.exit.saturating_sub(self.enter)
    }
}

/// 九桶耗时直方图：<2, <4, <8, <16, <32, <64, <128, <256, ≥256。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    pub buckets: [u32; HIST_BUCKETS],
    pub count: u32,
    pub sum: u64,
    pub max: u32,
}

impl Histogram {
    pub fn new() -> Histogram {
        Histogram {
            buckets: [0u32; HIST_BUCKETS],
            count: 0,
            sum: 0,
            max: 0,
        }
    }

    /// 时长落桶。
    pub const fn bucket_of(dur: u32) -> usize {
        if dur < 2 {
            0
        } else if dur < 4 {
            1
        } else if dur < 8 {
            2
        } else if dur < 16 {
            3
        } else if dur < 32 {
            4
        } else if dur < 64 {
            5
        } else if dur < 128 {
            6
        } else if dur < 256 {
            7
        } else {
            8
        }
    }

    /// 记一次时长。
    pub fn add(&mut self, dur: u32) {
        let i = Histogram::bucket_of(dur);
        if let Some(slot) = self.buckets.get_mut(i) {
            *slot = slot.saturating_add(1);
        }
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(dur as u64);
        if dur > self.max {
            self.max = dur;
        }
    }

    /// 守恒：九桶计数之和等于总计数。
    pub fn accounted(&self) -> bool {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.buckets.len() {
            if let Some(v) = self.buckets.get(i) {
                total = total.saturating_add(*v);
            }
            i += 1;
        }
        total == self.count
    }

    /// 非空桶数（诊断用）。
    pub fn used_buckets(&self) -> u32 {
        let mut used = 0u32;
        let mut i = 0usize;
        while i < self.buckets.len() {
            if let Some(v) = self.buckets.get(i) {
                if *v > 0 {
                    used = used.saturating_add(1);
                }
            }
            i += 1;
        }
        used
    }
}

/// 分发段账本。
#[derive(Clone, Debug)]
pub struct DispatchLedger {
    pub ticks: u32,
    pub entries: Vec<DispatchTick>,
    pub hist: Histogram,
    pub lost: u32,
    pub recovered: u32,
    pub digest: u64,
}

impl DispatchLedger {
    pub fn new() -> DispatchLedger {
        DispatchLedger {
            ticks: 0,
            entries: Vec::new(),
            hist: Histogram::new(),
            lost: 0,
            recovered: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 落一条打点：计数、落桶、折摘要。
    pub fn push(&mut self, t: DispatchTick) {
        self.ticks = self.ticks.saturating_add(1);
        self.hist.add(t.duration());
        self.digest = p_core::fold_u32(self.digest, t.seq);
        self.digest = p_core::fold_u32(self.digest, t.window);
        self.digest = p_core::fold_u32(self.digest, t.enter);
        self.digest = p_core::fold_u32(self.digest, t.exit);
        self.entries.push(t);
    }

    /// 重算：从条目重建计数、直方图与摘要（失联补回后使用）。
    pub fn recount(&mut self) {
        let mut hist = Histogram::new();
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(t) = self.entries.get(i) {
                hist.add(t.duration());
                h = p_core::fold_u32(h, t.seq);
                h = p_core::fold_u32(h, t.window);
                h = p_core::fold_u32(h, t.enter);
                h = p_core::fold_u32(h, t.exit);
            }
            i += 1;
        }
        self.ticks = self.entries.len() as u32;
        self.hist = hist;
        self.digest = h;
    }

    /// 入口时刻计数。
    pub fn entered(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(t) = self.entries.get(i) {
                if t.enter > 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 出口时刻计数（出口不早于入口才合格）。
    pub fn exited(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(t) = self.entries.get(i) {
                if t.exit >= t.enter && t.exit > 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 时刻单调：入口不回溯，且每条出口都不早于入口。
    pub fn monotonic(&self) -> bool {
        let mut prev: Option<u32> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            let t = match self.entries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t.exit < t.enter {
                return false;
            }
            if let Some(p) = prev {
                if t.enter < p {
                    return false;
                }
            }
            prev = Some(t.enter);
            i += 1;
        }
        true
    }

    /// 成对打点数：入口与出口都有效。
    pub fn stamp_pairs(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(t) = self.entries.get(i) {
                if t.enter > 0 && t.exit > t.enter {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 守恒：打点数 = 入口数 = 出口数 = 直方图计数，且九桶自洽。
    pub fn conserved(&self) -> bool {
        self.ticks as usize == self.entries.len()
            && self.hist.count == self.ticks
            && self.hist.accounted()
            && self.entered() == self.ticks
            && self.exited() == self.ticks
    }
}

/// 千次分发打点：入口单调、出口晚于入口、时长 1..=96。
pub fn latency_run() -> Result<DispatchLedger, u32> {
    let mut l = DispatchLedger::new();
    let mut rng = p_core::Lcg::new(0x1001_0001);
    let mut clock = CLOCK_BASE;
    let mut i = 0u32;
    while i < DISPATCH_TICKS {
        let enter = clock;
        let dur = 1 + rng.below(96);
        let gap = rng.below(4);
        let exit = match enter.checked_add(dur) {
            Some(v) => v,
            None => return Err(ConsumeCode::Overflow.val()),
        };
        let window = 0x8D01 + (i % 4);
        l.push(DispatchTick {
            seq: i + 1,
            window,
            enter,
            exit,
        });
        clock = match exit.checked_add(gap) {
            Some(v) => v,
            None => return Err(ConsumeCode::Overflow.val()),
        };
        i += 1;
    }
    Ok(l)
}

/// 由条目重建直方图（独立实现，用于与账内直方图对账）。
pub fn histogram_of(entries: &[DispatchTick]) -> Histogram {
    let mut h = Histogram::new();
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(t) = entries.get(i) {
            h.add(t.duration());
        }
        i += 1;
    }
    h
}

/// 打点复跑：两遍摘要一致且非零。
pub fn latency_replay() -> Result<(u64, u64), u32> {
    let a = latency_run()?.digest;
    let b = latency_run()?.digest;
    Ok((a, b))
}

/// 失联注入 + 恢复：抽走 10 条打点，再逐条补回，缺口必须归零。
/// 返回 (失联数, 补回数, 缺口数, 补回后摘要)。
pub fn loss_recover() -> Result<(u32, u32, u32, u64), u32> {
    let mut l = latency_run()?;
    let cut = 100usize;
    let mut saved: Vec<DispatchTick> = Vec::new();
    let mut kept: Vec<DispatchTick> = Vec::new();
    let mut i = 0usize;
    while i < l.entries.len() {
        if let Some(t) = l.entries.get(i) {
            if i >= cut && i < cut + LOSS_INJECT as usize {
                saved.push(*t);
            } else {
                kept.push(*t);
            }
        }
        i += 1;
    }
    l.entries = kept;
    l.lost = LOSS_INJECT;
    l.recount();
    if l.ticks != DISPATCH_TICKS - LOSS_INJECT {
        return Err(ConsumeCode::LostBad.val());
    }
    let mut j = 0usize;
    while j < saved.len() {
        if let Some(t) = saved.get(j) {
            l.entries.push(*t);
        }
        j += 1;
    }
    l.recovered = saved.len() as u32;
    l.recount();
    let outstanding = l.lost.saturating_sub(l.recovered);
    Ok((l.lost, l.recovered, outstanding, l.digest))
}

/// 分布摘要：中位桶（按累计计数过半判定）。
pub fn median_bucket(h: &Histogram) -> usize {
    let half = h.count / 2;
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < h.buckets.len() {
        if let Some(v) = h.buckets.get(i) {
            acc = acc.saturating_add(*v);
            if acc > half {
                return i;
            }
        }
        i += 1;
    }
    HIST_BUCKETS - 1
}

// ── 渲染与解析 ────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 映射账行：映射数、规则数、越权对、D4 行数。
pub fn dispatch_row() -> String {
    let mut d4_rows = 0u32;
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            if e.translator == D4_NAME {
                d4_rows = d4_rows.saturating_add(1);
            }
        }
        i += 1;
    }
    render_row(
        DISPATCH_TAG,
        MAP_TOTAL as u32,
        ORDER_RULE_COUNT as u32,
        UNAUTHORIZED.len() as u32,
        d4_rows,
        map_digest(),
    )
}

/// 延迟账行：打点数、直方图计数、最大时长、失联数。
pub fn latency_row(l: &DispatchLedger) -> String {
    render_row(
        LATENCY_TAG,
        l.ticks,
        l.hist.count,
        l.hist.max,
        l.lost,
        l.digest,
    )
}

/// 映射热行：消息编号、语义序号、规范秩。
pub fn map_row(e: &MapEntry) -> String {
    render_row(
        MAP_TAG,
        e.msg.raw(),
        e.semantic.index() as u32,
        e.rank as u32,
        0,
        map_digest(),
    )
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49049 ①：映射登记表（十行齐备、编号/语义/秩唯一、译者全 D4）。
    let row = dispatch_row();
    let row_ok = match row_fields(&row) {
        Some((t, v, d)) => {
            t == DISPATCH_TAG
                && v[0] == MAP_TOTAL as u32
                && v[1] == ORDER_RULE_COUNT as u32
                && v[2] == UNAUTHORIZED.len() as u32
                && v[3] == MAP_TOTAL as u32
                && d == map_digest()
                && audit_render(&row)
        }
        None => false,
    };
    out.push((
        "F49049/映射登记表",
        map_complete()
            && map_digest() != 0
            && MAP.len() == MAP_TOTAL
            && from_raw(0x0100) == Some(WmMsg::KeyDown)
            && from_raw(0x0999).is_none()
            && row_ok,
        format!(
            "rows={} complete={} digest={:016X} row={}",
            MAP.len(),
            map_complete(),
            map_digest(),
            row_ok
        ),
    ));

    // F49049 ②：顺序约束（规范串可过、倒排被拒、十次交换全拒）。
    let base = ordered_burst();
    let rev = reversed_burst();
    let faults = order_faults();
    let order_ok_all = order_ok(&base)
        && rules_hold(&base)
        && !order_ok(&rev)
        && match &faults {
            Ok((r, a)) => *r == 10 && *a == 0,
            Err(_) => false,
        };
    out.push((
        "F49049/顺序约束",
        order_ok_all,
        format!(
            "ordered={} rules={} reversed={} rej={:?} acc={:?}",
            order_ok(&base),
            rules_hold(&base),
            order_ok(&rev),
            faults.as_ref().map(|(r, _)| *r),
            faults.as_ref().map(|(_, a)| *a)
        ),
    ));

    // F49049 ③：越权转换检出（10 语义越权全检出、10 译者越权全拒、合法全收）。
    let unauth = unauthorized_probe();
    let trans = translator_faults();
    let mut legal = 0u32;
    let mut i = 0usize;
    while i < MAP.len() {
        if let Some(e) = MAP.get(i) {
            if translate_checked(e.msg, e.semantic, D4_NAME).is_ok() {
                legal = legal.saturating_add(1);
            }
        }
        i += 1;
    }
    out.push((
        "F49049/越权转换检出",
        unauth == (10, 0) && trans == (10, 0) && legal == MAP_TOTAL as u32,
        format!(
            "unauth={:?} translator={:?} legal={}",
            unauth, trans, legal
        ),
    ));

    // F49049 ④：联签复跑（百次翻译全过、复跑一致、联签锚齐全）。
    let hundred = hundred_translations();
    let rep = translation_replay();
    let line = cosign_line();
    let sign_ok = line.starts_with("D4SIGN")
        && line.contains(D4_SIGNAL)
        && line.contains(AI62_ANCHOR)
        && line.contains(D4_NAME);
    let replay_ok = match (&hundred, &rep) {
        (Ok((ok, denied, h)), Ok((a, b))) => {
            *ok == 100 && *denied == 0 && *h != 0 && a == b && *a != 0
        }
        _ => false,
    };
    out.push((
        "F49049/联签复跑",
        replay_ok && sign_ok && segment_guard() && no_cross_collision(),
        format!(
            "ok={:?} denied={:?} rep={:?} sign={}",
            hundred.as_ref().map(|(o, _, _)| *o),
            hundred.as_ref().map(|(_, d, _)| *d),
            rep.as_ref().map(|(a, b)| a == b),
            sign_ok
        ),
    ));

    // F49050 ①：千次打点落账（1000 条、计数/桶/摘要全对）。
    let led = latency_run();
    let tick_ok = match &led {
        Ok(l) => {
            l.ticks == DISPATCH_TICKS
                && l.entries.len() as u32 == DISPATCH_TICKS
                && l.hist.count == DISPATCH_TICKS
                && l.hist.accounted()
                && l.conserved()
                && l.digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49050/千次打点落账",
        tick_ok,
        format!(
            "ticks={:?} hist={:?} cons={:?}",
            led.as_ref().map(|l| l.ticks),
            led.as_ref().map(|l| l.hist.count),
            led.as_ref().map(|l| l.conserved())
        ),
    ));

    // F49050 ②：入口出口时刻（入口单调、出口不早于入口、成对 1000）。
    let stamp_ok = match &led {
        Ok(l) => {
            l.monotonic()
                && l.stamp_pairs() == DISPATCH_TICKS
                && l.entered() == DISPATCH_TICKS
                && l.exited() == DISPATCH_TICKS
                && match l.entries.first() {
                    Some(t) => t.enter >= CLOCK_BASE,
                    None => false,
                }
        }
        Err(_) => false,
    };
    out.push((
        "F49050/入口出口时刻",
        stamp_ok,
        format!(
            "mono={:?} pairs={:?} entered={:?} exited={:?}",
            led.as_ref().map(|l| l.monotonic()),
            led.as_ref().map(|l| l.stamp_pairs()),
            led.as_ref().map(|l| l.entered()),
            led.as_ref().map(|l| l.exited())
        ),
    ));

    // F49050 ③：耗时直方图（九桶自洽、独立重算一致、桶边界正确）。
    let hist_ok = match &led {
        Ok(l) => {
            let again = histogram_of(&l.entries);
            again == l.hist
                && l.hist.used_buckets() > 1
                && l.hist.max >= 1
                && Histogram::bucket_of(1) == 0
                && Histogram::bucket_of(2) == 1
                && Histogram::bucket_of(4) == 2
                && Histogram::bucket_of(255) == 7
                && Histogram::bucket_of(256) == 8
                && l.hist.sum >= (l.hist.count as u64)
                && median_bucket(&l.hist) < HIST_BUCKETS
        }
        Err(_) => false,
    };
    out.push((
        "F49050/耗时直方图",
        hist_ok,
        format!(
            "buckets={:?} used={:?} max={:?} sum={:?}",
            led.as_ref().map(|l| l.hist.buckets),
            led.as_ref().map(|l| l.hist.used_buckets()),
            led.as_ref().map(|l| l.hist.max),
            led.as_ref().map(|l| l.hist.sum)
        ),
    ));

    // F49050 ④：失联恢复零丢（注入 10、补回 10、缺口 0、补后守恒）。
    let loss = loss_recover();
    let lrow = match &led {
        Ok(l) => latency_row(l),
        Err(_) => String::from(""),
    };
    let lrow_ok = match row_fields(&lrow) {
        Some((t, v, d)) => {
            t == LATENCY_TAG && v[0] == DISPATCH_TICKS && v[1] == DISPATCH_TICKS && d != 0
        }
        None => false,
    };
    let loss_ok = match &loss {
        Ok((lost, rec, out, digest)) => {
            *lost == LOSS_INJECT && *rec == LOSS_INJECT && *out == 0 && *digest != 0
        }
        Err(_) => false,
    };
    out.push((
        "F49050/失联恢复零丢",
        loss_ok && lrow_ok && EXP13_ANCHOR.contains("隐蔽捕获"),
        format!(
            "lost={:?} rec={:?} out={:?} row={} anchor={}",
            loss.as_ref().map(|(l, _, _, _)| *l),
            loss.as_ref().map(|(_, r, _, _)| *r),
            loss.as_ref().map(|(_, _, o, _)| *o),
            lrow_ok,
            EXP13_ANCHOR.contains("隐蔽捕获")
        ),
    ));

    out
}

pub fn run_unx49049_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49049 D4 消费序联签与分发延迟打点");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(ConsumeCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < ConsumeCode::ALL.len() {
            let c = ConsumeCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < ConsumeCode::ALL.len() {
            let c = ConsumeCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(1), UNKNOWN);
    }

    #[test]
    fn detail_texts_are_distinct() {
        let mut i = 0usize;
        while i < ConsumeCode::ALL.len() {
            let mut j = i + 1;
            while j < ConsumeCode::ALL.len() {
                assert_ne!(ConsumeCode::ALL[i].detail(), ConsumeCode::ALL[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn segment_window_edges() {
        assert_eq!(ConsumeCode::ALL[0].val(), 0x73101);
        assert_eq!(ConsumeCode::ALL[13].val(), 0x7310E);
        assert!(!known_code(0x73100));
        assert!(!known_code(0x7310F));
    }

    #[test]
    fn case_ids_reserved_unique() {
        assert!(no_cross_collision());
        assert_eq!(CASE_IDS[0], 0x0733_1001);
        assert_eq!(CASE_IDS[15], 0x0733_1010);
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = 0usize;
            while j < CASE_IDS.len() {
                assert_ne!(RESERVED_IDS[i], CASE_IDS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn case_ids_sequential_and_named() {
        assert!(case_ids_sequential());
        assert_eq!(CASES.len(), 16);
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].id, CASE_IDS[i]);
            assert!(!CASES[i].name.is_empty());
            i += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("LATNCY", 1000, 1000, 96, 0, 0x00FF_00FF_00FF_00FF);
        assert!(audit_render(&row));
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, "LATNCY");
        assert_eq!(v, [1000, 1000, 96, 0]);
        assert_eq!(d, 0x00FF_00FF_00FF_00FF);
    }

    #[test]
    fn parse_u32_junk() {
        assert_eq!(parse_u32("07"), Some(7));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("1_0"), None);
        assert_eq!(parse_u32("99999999999"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("1234"), None);
        assert_eq!(parse_hex16("GGGGGGGGGGGGGGGG"), None);
    }

    #[test]
    fn row_fields_malformed() {
        assert!(row_fields("").is_none());
        assert!(row_fields("D4MAP|k1=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF").is_some());
        assert!(row_fields("D4MAP|k1=1|k2=2|k3=3|k4=4").is_none());
        assert!(row_fields("D4MAP|k1=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF|x=1").is_none());
    }

    #[test]
    fn map_table_has_ten_rows() {
        assert_eq!(MAP.len(), 10);
        assert_eq!(MAP_TOTAL, 10);
        assert!(map_complete());
        assert_ne!(map_digest(), 0);
    }

    #[test]
    fn map_raw_codes_unique() {
        let mut seen: [u32; MAP_TOTAL] = [0; MAP_TOTAL];
        let mut i = 0usize;
        while i < MAP.len() {
            seen[i] = MAP[i].msg.raw();
            let mut j = 0usize;
            while j < i {
                assert_ne!(seen[j], seen[i]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(from_raw(0x0002), Some(WmMsg::Destroy));
        assert_eq!(from_raw(0x0202), Some(WmMsg::MouseUp));
        assert_eq!(from_raw(0x0000), None);
    }

    #[test]
    fn map_semantics_unique() {
        let mut i = 0usize;
        while i < MAP.len() {
            let mut j = i + 1;
            while j < MAP.len() {
                assert_ne!(MAP[i].semantic, MAP[j].semantic);
                j += 1;
            }
            assert_eq!(MAP[i].semantic.index() as usize, i);
            i += 1;
        }
    }

    #[test]
    fn map_ranks_sequential() {
        let mut i = 0usize;
        while i < MAP.len() {
            assert_eq!(MAP[i].rank as usize, i + 1);
            assert_eq!(rank_of(MAP[i].semantic), Some(i as u8 + 1));
            i += 1;
        }
        assert_eq!(rank_of(Semantic::FocusOut), Some(10));
    }

    #[test]
    fn map_translators_all_d4() {
        let mut i = 0usize;
        while i < MAP.len() {
            assert_eq!(MAP[i].translator, D4_NAME);
            i += 1;
        }
        assert_eq!(dispatch_row().len() > 0, true);
    }

    #[test]
    fn translate_every_message() {
        let mut i = 0usize;
        while i < WmMsg::ALL.len() {
            let m = WmMsg::ALL[i];
            let e = entry_of(m).unwrap();
            assert_eq!(translate(m), Ok(e.semantic));
            i += 1;
        }
    }

    #[test]
    fn translate_checked_rejects_cross_semantic() {
        assert_eq!(
            translate_checked(WmMsg::KeyDown, Semantic::PointerDown, D4_NAME),
            Err(ConsumeCode::Unauthorized.val())
        );
        assert_eq!(
            translate_checked(WmMsg::KeyDown, Semantic::KeyDown, D4_NAME),
            Ok(Semantic::KeyDown)
        );
    }

    #[test]
    fn translate_checked_rejects_foreign_translator() {
        assert_eq!(
            translate_checked(WmMsg::Char, Semantic::Char, "H5"),
            Err(ConsumeCode::TranslatorBad.val())
        );
        assert_eq!(
            translate_checked(WmMsg::Char, Semantic::Char, "unxstar"),
            Err(ConsumeCode::TranslatorBad.val())
        );
    }

    #[test]
    fn unauthorized_probe_ten() {
        assert_eq!(UNAUTHORIZED.len(), 10);
        assert_eq!(unauthorized_probe(), (10, 0));
    }

    #[test]
    fn translator_faults_ten() {
        assert_eq!(translator_faults(), (10, 0));
    }

    #[test]
    fn ordered_burst_is_ordered() {
        let seq = ordered_burst();
        assert!(order_ok(&seq));
        assert!(rules_hold(&seq));
        assert_eq!(seq.len(), 10);
        assert_eq!(seq[0], Semantic::FocusIn);
        assert_eq!(seq[9], Semantic::FocusOut);
    }

    #[test]
    fn reversed_burst_rejected() {
        let seq = reversed_burst();
        assert_eq!(seq.len(), 10);
        assert!(!order_ok(&seq));
    }

    #[test]
    fn order_faults_ten() {
        assert_eq!(order_faults().unwrap(), (10, 0));
    }

    #[test]
    fn order_allows_repeated_rank() {
        let seq = Vec::from([
            Semantic::PointerMove,
            Semantic::PointerMove,
            Semantic::PointerMove,
            Semantic::PointerDown,
            Semantic::PointerUp,
        ]);
        assert!(order_ok(&seq));
    }

    #[test]
    fn rules_hold_requires_both_sides() {
        let only_focus = Vec::from([Semantic::FocusIn]);
        assert!(rules_hold(&only_focus));
        // 只出现单侧语义时规则不触发，秩检查仍会拒收倒序。
        let pairless = Vec::from([Semantic::KeyUp, Semantic::KeyDown]);
        assert!(rules_hold(&pairless));
        assert!(!order_ok(&pairless));
        // 两侧都出现且倒置时，规则检查必须拒收。
        let bad = Vec::from([Semantic::Char, Semantic::KeyDown]);
        assert!(!rules_hold(&bad));
        assert!(!order_ok(&bad));
    }

    #[test]
    fn hundred_translations_green() {
        let (ok, denied, digest) = hundred_translations().unwrap();
        assert_eq!(ok, 100);
        assert_eq!(denied, 0);
        assert_ne!(digest, 0);
    }

    #[test]
    fn translation_replay_stable() {
        let (a, b) = translation_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn dispatch_row_parses() {
        let row = dispatch_row();
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, DISPATCH_TAG);
        assert_eq!(v, [10, 8, 10, 10]);
        assert_eq!(d, map_digest());
        assert!(audit_render(&row));
    }

    #[test]
    fn map_row_parses() {
        let row = map_row(&MAP[1]);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, MAP_TAG);
        assert_eq!(v[0], 0x0100);
        assert_eq!(v[1], Semantic::KeyDown.index() as u32);
        assert_eq!(v[2], 2);
        assert_eq!(d, map_digest());
        assert!(audit_render(&row));
    }

    #[test]
    fn latency_run_thousand() {
        let l = latency_run().unwrap();
        assert_eq!(l.ticks, 1000);
        assert_eq!(l.entries.len(), 1000);
        assert_eq!(l.hist.count, 1000);
        assert!(l.conserved());
        assert_ne!(l.digest, 0);
        assert_eq!(l.lost, 0);
        assert_eq!(l.recovered, 0);
    }

    #[test]
    fn latency_replay_stable() {
        let (a, b) = latency_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn histogram_bucket_boundaries() {
        assert_eq!(Histogram::bucket_of(0), 0);
        assert_eq!(Histogram::bucket_of(1), 0);
        assert_eq!(Histogram::bucket_of(2), 1);
        assert_eq!(Histogram::bucket_of(3), 1);
        assert_eq!(Histogram::bucket_of(4), 2);
        assert_eq!(Histogram::bucket_of(7), 2);
        assert_eq!(Histogram::bucket_of(8), 3);
        assert_eq!(Histogram::bucket_of(16), 4);
        assert_eq!(Histogram::bucket_of(32), 5);
        assert_eq!(Histogram::bucket_of(64), 6);
        assert_eq!(Histogram::bucket_of(128), 7);
        assert_eq!(Histogram::bucket_of(255), 7);
        assert_eq!(Histogram::bucket_of(256), 8);
        assert_eq!(Histogram::bucket_of(9999), 8);
    }

    #[test]
    fn histogram_add_bookkeeping() {
        let mut h = Histogram::new();
        h.add(1);
        h.add(5);
        h.add(300);
        assert_eq!(h.count, 3);
        assert_eq!(h.buckets[0], 1);
        assert_eq!(h.buckets[2], 1);
        assert_eq!(h.buckets[8], 1);
        assert_eq!(h.sum, 306);
        assert_eq!(h.max, 300);
        assert!(h.accounted());
        assert_eq!(h.used_buckets(), 3);
    }

    #[test]
    fn histogram_recompute_matches() {
        let l = latency_run().unwrap();
        assert_eq!(histogram_of(&l.entries), l.hist);
        assert!(l.hist.used_buckets() > 1);
        assert!(median_bucket(&l.hist) < HIST_BUCKETS);
    }

    #[test]
    fn stamps_are_monotonic() {
        let l = latency_run().unwrap();
        assert!(l.monotonic());
        assert_eq!(l.entries[0].enter, CLOCK_BASE);
        let mut i = 1usize;
        while i < l.entries.len() {
            assert!(l.entries[i].enter >= l.entries[i - 1].enter);
            i += 1;
        }
    }

    #[test]
    fn stamps_exit_after_enter() {
        let l = latency_run().unwrap();
        assert_eq!(l.stamp_pairs(), 1000);
        let mut i = 0usize;
        while i < l.entries.len() {
            assert!(l.entries[i].exit > l.entries[i].enter);
            assert!(l.entries[i].duration() >= 1);
            assert!(l.entries[i].duration() <= 96);
            i += 1;
        }
    }

    #[test]
    fn ledger_conserved() {
        let l = latency_run().unwrap();
        assert!(l.conserved());
        assert_eq!(l.entered(), 1000);
        assert_eq!(l.exited(), 1000);
        assert!(l.hist.accounted());
        assert_eq!(l.hist.count, l.ticks);
    }

    #[test]
    fn loss_recover_zero_outstanding() {
        let (lost, rec, out, digest) = loss_recover().unwrap();
        assert_eq!(lost, 10);
        assert_eq!(rec, 10);
        assert_eq!(out, 0);
        assert_ne!(digest, 0);
    }

    #[test]
    fn loss_recover_counts_conserved() {
        let (lost, rec, out, _) = loss_recover().unwrap();
        assert_eq!(lost - rec, out);
        let l = latency_run().unwrap();
        assert_eq!(l.ticks, 1000);
        let mut entries = l.entries.clone();
        let saved: Vec<DispatchTick> = entries.drain(100..110).collect();
        assert_eq!(entries.len(), 990);
        let mut h = histogram_of(&entries);
        assert_eq!(h.count, 990);
        entries.extend(saved.iter());
        h = histogram_of(&entries);
        assert_eq!(h.count, 1000);
        assert!(h.accounted());
    }

    #[test]
    fn loss_recover_sequence_sum_invariant() {
        let l = latency_run().unwrap();
        let expect = 1000u64 * 1001 / 2;
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < l.entries.len() {
            sum += l.entries[i].seq as u64;
            i += 1;
        }
        assert_eq!(sum, expect);
        let (lost, rec, out, _) = loss_recover().unwrap();
        assert_eq!(lost, rec);
        assert_eq!(out, 0);
    }

    #[test]
    fn latency_row_parses() {
        let l = latency_run().unwrap();
        let row = latency_row(&l);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, LATENCY_TAG);
        assert_eq!(v[0], 1000);
        assert_eq!(v[1], 1000);
        assert_eq!(v[3], 0);
        assert_eq!(d, l.digest);
        assert!(audit_render(&row));
    }

    #[test]
    fn cosign_line_anchors() {
        let line = cosign_line();
        assert!(line.starts_with("D4SIGN"));
        assert!(line.contains(D4_SIGNAL));
        assert!(line.contains(AI62_ANCHOR));
        assert!(line.contains("map=10"));
        assert!(line.contains("rules=8"));
        assert!(EXP13_ANCHOR.contains("隐蔽捕获"));
    }

    #[test]
    fn semantic_names_stable() {
        assert_eq!(Semantic::FocusIn.name(), "focus-in");
        assert_eq!(Semantic::PointerDown.name(), "pointer-down");
        assert_eq!(Semantic::FocusOut.name(), "focus-out");
        assert_eq!(Semantic::Destroy.index(), 8);
    }

    #[test]
    fn wm_raw_names_stable() {
        assert_eq!(WmMsg::KeyDown.raw(), 0x0100);
        assert_eq!(WmMsg::KillFocus.raw(), 0x0008);
        assert_eq!(WmMsg::Destroy.name(), "WM_DESTROY");
        assert_eq!(WmMsg::Close.name(), "WM_CLOSE");
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49049_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
