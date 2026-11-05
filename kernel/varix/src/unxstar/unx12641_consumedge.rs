//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12641 D1→E1 探测面消费边总表（VX9 版） · 0x80A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12642-J1 三列制度（互引/冻结/联测）与
//!      F12561/F12581 同构声明在册，制度档版本号入账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - D1（探测域）→E1（执行域）消费边总表（与 0x80A00 开门件域分
//!     工：彼卡承载开门闸，本卡承载边总表本体与制度档）：八条消费边
//!     各带消费点计数与边向（D1→E1 单向，逆向必抓 ReverseEdge）；
//!   - 三列制度（判据锚定）：互引列/冻结列/联测列三列闭集，每条带
//!     条目与列向，列齐断言（TriColShort 抓缺列、TriColDup 抓重条）；
//!   - F12561/F12581 同构声明在册（判据锚定）：同构声明闭表两锚，
//!     逐锚入册（IsoAnchorMissing 抓缺锚、IsoAnchorDangling 抓悬空）；
//!   - 制度档版本号入账（判据锚定）：版本号 u32 入账且单调只升不降
//!     （VersionRegress 抓降版、VersionMissing 抓无版）；
//!   - 边总表行数恒等 8（EdgeShort/EdgeDup），消费点计数守恒（消费
//!     点总数 ≡ 冻结和，守恒破即 PointBroken）；
//!   - LCG 种子写死 = SEG_BASE（0x80A40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80A40..0x80A80。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80A40，AI-VX9-1105 B03 批 12 槽第 6 槽）。
pub const SEG_BASE: u32 = 0x80A40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumCode {
    /// 缺边：消费边总表八行不齐
    EdgeShort,
    /// 重边：同边重复登记
    EdgeDup,
    /// 逆向边：D1→E1 只许单向
    ReverseEdge,
    /// 消费点守恒破：消费点总数 ≠ 冻结和
    PointBroken,
    /// 三列缺列
    TriColShort,
    /// 三列重条
    TriColDup,
    /// 同构锚缺失：F12561/F12581 未全入册
    IsoAnchorMissing,
    /// 同构锚悬空：锚名不在冻结闭表
    IsoAnchorDangling,
    /// 版本缺失：制度档版本号未入账
    VersionMissing,
    /// 版本降级：制度档版本只升不降
    VersionRegress,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl ConsumCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ConsumCode::EdgeShort => 0x01,
                ConsumCode::EdgeDup => 0x02,
                ConsumCode::ReverseEdge => 0x03,
                ConsumCode::PointBroken => 0x04,
                ConsumCode::TriColShort => 0x05,
                ConsumCode::TriColDup => 0x06,
                ConsumCode::IsoAnchorMissing => 0x07,
                ConsumCode::IsoAnchorDangling => 0x08,
                ConsumCode::VersionMissing => 0x09,
                ConsumCode::VersionRegress => 0x0A,
                ConsumCode::FingerprintDrift => 0x0B,
                ConsumCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            ConsumCode::EdgeShort => "缺边：消费边总表补齐八行",
            ConsumCode::EdgeDup => "重边：一边一行不许重复",
            ConsumCode::ReverseEdge => "逆向边：消费边只许 D1→E1 单向",
            ConsumCode::PointBroken => "消费点守恒破：总数须等冻结和",
            ConsumCode::TriColShort => "三列缺列：互引/冻结/联测补齐",
            ConsumCode::TriColDup => "三列重条：同列同号不许重复",
            ConsumCode::IsoAnchorMissing => "同构锚缺失：F12561/F12581 全入册",
            ConsumCode::IsoAnchorDangling => "同构锚悬空：锚名落冻结闭表",
            ConsumCode::VersionMissing => "版本缺失：制度档版本号先入账",
            ConsumCode::VersionRegress => "版本降级：制度档版本只升不降",
            ConsumCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            ConsumCode::Unknown => "未知诊断码：不在 0x80A40 段闭集内",
        }
    }

    pub const ALL: [ConsumCode; 12] = [
        ConsumCode::EdgeShort,
        ConsumCode::EdgeDup,
        ConsumCode::ReverseEdge,
        ConsumCode::PointBroken,
        ConsumCode::TriColShort,
        ConsumCode::TriColDup,
        ConsumCode::IsoAnchorMissing,
        ConsumCode::IsoAnchorDangling,
        ConsumCode::VersionMissing,
        ConsumCode::VersionRegress,
        ConsumCode::FingerprintDrift,
        ConsumCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ConsumCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    ConsumCode::Unknown.detail()
}

// ── 冻结常量表（来源：D1→E1 消费边手账 · 总表节） ────────────────
/// 消费边总表行数（冻结 8）。
pub const EDGE_ROWS: usize = 8;
/// 消费点冻结总仓（八行消费点计数和）。
pub const POINT_TOTAL: u32 = 46;
/// 三列制度条目总数。
pub const TRICOL_TOTAL: usize = 10;
/// 制度档冻结版本号（判据锚定：版本号入账）。
pub const CHARTER_VERSION: u32 = 9;
/// 制度档容量。
pub const CHARTER_CAP: usize = 32;

/// 域对（D1 探测域 → E1 执行域，单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomPair {
    /// D1 探测域。
    D1,
    /// E1 执行域。
    E1,
}

impl DomPair {
    /// 域序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            DomPair::D1 => 0,
            DomPair::E1 => 1,
        }
    }
}

/// 八条消费边闭集（行序冻结，与 0x80A00 八行同源但本卡带消费点计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumEdge {
    /// PEB 消费边（消费点 8）。
    Peb,
    /// TEB 消费边（消费点 6）。
    Teb,
    /// NTSTATUS 消费边（消费点 9）。
    Ntstatus,
    /// Process 查询消费边（消费点 7）。
    ProcessQuery,
    /// Thread 查询消费边（消费点 6）。
    ThreadQuery,
    /// System 查询消费边（消费点 4）。
    SystemQuery,
    /// 版本分档消费边（消费点 3）。
    VersionTier,
    /// 对象枚举消费边（消费点 3）。
    ObjectEnum,
}

impl ConsumEdge {
    /// 行序（冻结 0..8）。
    pub const fn idx(self) -> u32 {
        match self {
            ConsumEdge::Peb => 0,
            ConsumEdge::Teb => 1,
            ConsumEdge::Ntstatus => 2,
            ConsumEdge::ProcessQuery => 3,
            ConsumEdge::ThreadQuery => 4,
            ConsumEdge::SystemQuery => 5,
            ConsumEdge::VersionTier => 6,
            ConsumEdge::ObjectEnum => 7,
        }
    }

    /// 冻结消费点计数（8+6+9+7+6+4+3+3 = 46）。
    pub const fn frozen_points(self) -> u32 {
        match self {
            ConsumEdge::Peb => 8,
            ConsumEdge::Teb => 6,
            ConsumEdge::Ntstatus => 9,
            ConsumEdge::ProcessQuery => 7,
            ConsumEdge::ThreadQuery => 6,
            ConsumEdge::SystemQuery => 4,
            ConsumEdge::VersionTier => 3,
            ConsumEdge::ObjectEnum => 3,
        }
    }
}

/// 八边闭集全表。
pub const CONSUM_ALL: [ConsumEdge; EDGE_ROWS] = [
    ConsumEdge::Peb,
    ConsumEdge::Teb,
    ConsumEdge::Ntstatus,
    ConsumEdge::ProcessQuery,
    ConsumEdge::ThreadQuery,
    ConsumEdge::SystemQuery,
    ConsumEdge::VersionTier,
    ConsumEdge::ObjectEnum,
];

/// 一条边登记（边向冻结 D1→E1）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeRec {
    /// 边行。
    pub edge: ConsumEdge,
    /// 源域（恒 D1）。
    pub from: DomPair,
    /// 目的域（恒 E1）。
    pub to: DomPair,
    /// 消费点计数（= 冻结值才入账）。
    pub points: u32,
}

/// 三列制度列闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCol {
    /// 互引列。
    InterRef,
    /// 冻结列。
    Freeze,
    /// 联测列。
    Cotest,
}

impl CharterCol {
    /// 列序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            CharterCol::InterRef => 0,
            CharterCol::Freeze => 1,
            CharterCol::Cotest => 2,
        }
    }
}

/// 一条三列制度条目（带列向注记文本长度作轻量载荷）。
#[derive(Clone, Debug)]
pub struct CharterItem {
    /// 列。
    pub col: CharterCol,
    /// 列内号。
    pub no: u32,
    /// 条目文本（互引锚名/冻结锚名/联测名）。
    pub label: String,
}

/// F12561/F12581 同构声明闭表（判据锚定）。
pub const ISO_ANCHORS: [(&str, &str); 2] = [
    ("F12561", "D1→E1 同构声明上源（探测对平三列制度先例）"),
    ("F12581", "D1→E1 同构声明下源（消费边制度对平先例）"),
];

/// 同构锚解析（悬空抓）。
pub fn iso_anchor_resolve(name: &str) -> Result<&'static str, u32> {
    for a in ISO_ANCHORS.iter() {
        if a.0 == name {
            return Ok(a.1);
        }
    }
    Err(ConsumCode::IsoAnchorDangling.val())
}

/// 制度档（三列制度 + 同构声明 + 版本号）。
pub struct EdgeCharter {
    /// 消费边登记。
    edges: Vec<EdgeRec>,
    /// 三列制度登记。
    items: Vec<CharterItem>,
    /// 已入册同构锚。
    pub iso_registered: Vec<String>,
    /// 制度档版本号（入账值，单调）。
    pub version: u32,
}

impl EdgeCharter {
    pub fn new() -> EdgeCharter {
        EdgeCharter {
            edges: Vec::new(),
            items: Vec::new(),
            iso_registered: Vec::new(),
            version: 0,
        }
    }

    /// 登记消费边：重边拒 + 逆向拒 + 消费点必须等于冻结值。
    pub fn register_edge(&mut self, e: EdgeRec) -> Result<u32, u32> {
        // 边向闸：只许 D1→E1。
        if e.from != DomPair::D1 || e.to != DomPair::E1 {
            return Err(ConsumCode::ReverseEdge.val());
        }
        // 消费点守恒闸：计数必须等于冻结值。
        if e.points != e.edge.frozen_points() {
            return Err(ConsumCode::PointBroken.val());
        }
        for old in self.edges.iter() {
            if old.edge == e.edge {
                return Err(ConsumCode::EdgeDup.val());
            }
        }
        self.edges.push(e);
        Ok(e.edge.idx())
    }

    /// 三列制度登记（同列同号重复拒，文本非空闸）。
    pub fn register_item(&mut self, item: CharterItem) -> Result<u32, u32> {
        if item.label.is_empty() {
            return Err(ConsumCode::TriColShort.val());
        }
        for old in self.items.iter() {
            if old.col == item.col && old.no == item.no {
                return Err(ConsumCode::TriColDup.val());
            }
        }
        self.items.push(item);
        Ok(1)
    }

    /// 同构声明入册：两锚逐锚解析（缺一即 IsoAnchorMissing）。
    pub fn register_iso(&mut self) -> Result<u32, u32> {
        self.iso_registered.clear();
        for a in ISO_ANCHORS.iter() {
            match iso_anchor_resolve(a.0) {
                Ok(desc) => self.iso_registered.push(String::from(desc)),
                Err(e) => return Err(e),
            }
        }
        if self.iso_registered.len() != ISO_ANCHORS.len() {
            return Err(ConsumCode::IsoAnchorMissing.val());
        }
        Ok(self.iso_registered.len() as u32)
    }

    /// 制度档版本号入账（只升不降）。
    pub fn set_version(&mut self, v: u32) -> Result<u32, u32> {
        if v == 0 {
            return Err(ConsumCode::VersionMissing.val());
        }
        if v < self.version {
            return Err(ConsumCode::VersionRegress.val());
        }
        self.version = v;
        Ok(v)
    }

    /// 边总表八行齐。
    pub fn eight_edges_full(&self) -> bool {
        if self.edges.len() != EDGE_ROWS {
            return false;
        }
        let mut ok = true;
        for r in CONSUM_ALL.iter() {
            let mut hit = false;
            for e in self.edges.iter() {
                if e.edge == *r {
                    hit = true;
                }
            }
            if !hit {
                ok = false;
            }
        }
        ok
    }

    /// 消费点守恒：账内消费点总数 ≡ 冻结和。
    pub fn points_conserved(&self) -> bool {
        let mut sum = 0u32;
        for e in self.edges.iter() {
            sum = sum.saturating_add(e.points);
        }
        sum == POINT_TOTAL
    }

    /// 三列制度齐：三列各至少一条且总数 ≥10。
    pub fn tricol_full(&self) -> bool {
        let mut inter = 0usize;
        let mut freeze = 0usize;
        let mut cotest = 0usize;
        for t in self.items.iter() {
            match t.col {
                CharterCol::InterRef => inter += 1,
                CharterCol::Freeze => freeze += 1,
                CharterCol::Cotest => cotest += 1,
            }
        }
        inter > 0 && freeze > 0 && cotest > 0 && self.items.len() >= TRICOL_TOTAL
    }

    /// 判据谓词：制度档版本号入账（版本 = 冻结版本且已入账）。
    pub fn version_on_ledger(&self) -> bool {
        self.version >= CHARTER_VERSION
    }

    /// 判据谓词：同构声明在册（两锚全解析）。
    pub fn iso_on_ledger(&self) -> bool {
        self.iso_registered.len() == ISO_ANCHORS.len()
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80A4_0F00u32;
        for e in self.edges.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.edge.idx())
                .wrapping_add(e.points)
                .wrapping_add(e.from.idx())
                .wrapping_add(e.to.idx());
        }
        for t in self.items.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(t.col.idx())
                .wrapping_add(t.no)
                .wrapping_add(t.label.chars().count() as u32);
        }
        fp.wrapping_add(self.version.wrapping_mul(3))
    }
}

/// 冻结三列制度表（互引 4 / 冻结 3 / 联测 3 = 10）。
pub fn frozen_charter_items() -> Vec<CharterItem> {
    let mut v: Vec<CharterItem> = Vec::new();
    let inter_labels: [&str; 4] = [
        "互引：F12561 探测对平先例",
        "互引：F12581 消费边先例",
        "互引：F12598 B31 预告锚",
        "互引：F12620 B32 闸锚",
    ];
    let freeze_labels: [&str; 3] = [
        "冻结：消费边八行行序",
        "冻结：消费点计数和 46",
        "冻结：制度档版本 9",
    ];
    let cotest_labels: [&str; 3] = [
        "联测：E1 Wine 探测面联签",
        "联测：B31/B32 40 例聚合",
        "联测：版本分档切换注入",
    ];
    let mut i = 0usize;
    while i < 4 {
        let label = match inter_labels.get(i) {
            Some(s) => String::from(*s),
            None => String::from("互引占位"),
        };
        v.push(CharterItem { col: CharterCol::InterRef, no: (i + 1) as u32, label });
        i += 1;
    }
    i = 0;
    while i < 3 {
        let label = match freeze_labels.get(i) {
            Some(s) => String::from(*s),
            None => String::from("冻结占位"),
        };
        v.push(CharterItem { col: CharterCol::Freeze, no: (i + 1) as u32, label });
        i += 1;
    }
    i = 0;
    while i < 3 {
        let label = match cotest_labels.get(i) {
            Some(s) => String::from(*s),
            None => String::from("联测占位"),
        };
        v.push(CharterItem { col: CharterCol::Cotest, no: (i + 1) as u32, label });
        i += 1;
    }
    v
}

/// 黄金正演：八边登记 + 守恒 + 制度 10 条 + 同构入册 + 版本入账。
fn golden_run() -> Result<u32, u32> {
    let mut c = EdgeCharter::new();
    for r in CONSUM_ALL.iter() {
        let _ = c.register_edge(EdgeRec {
            edge: *r,
            from: DomPair::D1,
            to: DomPair::E1,
            points: r.frozen_points(),
        })?;
    }
    if !c.eight_edges_full() {
        return Err(ConsumCode::EdgeShort.val());
    }
    if !c.points_conserved() {
        return Err(ConsumCode::PointBroken.val());
    }
    for t in frozen_charter_items().iter() {
        let _ = c.register_item(t.clone())?;
    }
    if !c.tricol_full() {
        return Err(ConsumCode::TriColShort.val());
    }
    let iso = c.register_iso()?;
    if iso != ISO_ANCHORS.len() as u32 {
        return Err(ConsumCode::IsoAnchorMissing.val());
    }
    let _ = c.set_version(CHARTER_VERSION)?;
    if !c.version_on_ledger() {
        return Err(ConsumCode::VersionMissing.val());
    }
    Ok(c.version)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut c = EdgeCharter::new();
        for r in CONSUM_ALL.iter() {
            let _ = c.register_edge(EdgeRec {
                edge: *r,
                from: DomPair::D1,
                to: DomPair::E1,
                points: r.frozen_points(),
            });
        }
        for t in frozen_charter_items().iter() {
            let _ = c.register_item(t.clone());
        }
        let _ = c.register_iso();
        let _ = c.set_version(CHARTER_VERSION);
        c.fingerprint()
    };
    run() == run() && golden_run() == golden_run()
}

/// 注入必抓：逆向边/点数漂移/重边/重条/悬空锚/版本降级/版本缺失。
fn inject_violations() -> bool {
    let mut c = EdgeCharter::new();
    // ① 逆向边（E1→D1）。
    let m1 = c.register_edge(EdgeRec {
        edge: ConsumEdge::Peb,
        from: DomPair::E1,
        to: DomPair::D1,
        points: 8,
    }) == Err(ConsumCode::ReverseEdge.val());
    // ② 消费点漂移（8 → 7）。
    let m2 = c.register_edge(EdgeRec {
        edge: ConsumEdge::Peb,
        from: DomPair::D1,
        to: DomPair::E1,
        points: 7,
    }) == Err(ConsumCode::PointBroken.val());
    // ③ 重边。
    let _ = c.register_edge(EdgeRec {
        edge: ConsumEdge::Peb,
        from: DomPair::D1,
        to: DomPair::E1,
        points: 8,
    });
    let m3 = c.register_edge(EdgeRec {
        edge: ConsumEdge::Peb,
        from: DomPair::D1,
        to: DomPair::E1,
        points: 8,
    }) == Err(ConsumCode::EdgeDup.val());
    // ④ 悬空同构锚。
    let m4 = iso_anchor_resolve("F12560") == Err(ConsumCode::IsoAnchorDangling.val());
    // ⑤ 版本降级：先入 9 再入 8。
    let _ = c.set_version(CHARTER_VERSION);
    let m5 = c.set_version(CHARTER_VERSION - 1) == Err(ConsumCode::VersionRegress.val());
    // ⑥ 版本缺失：0 版拒。
    let mut c2 = EdgeCharter::new();
    let m6 = c2.set_version(0) == Err(ConsumCode::VersionMissing.val());
    // ⑦ 重条：同列同号。
    let _ = c.register_item(CharterItem {
        col: CharterCol::InterRef,
        no: 1,
        label: String::from("互引注入"),
    });
    let m7 = c.register_item(CharterItem {
        col: CharterCol::InterRef,
        no: 1,
        label: String::from("互引注入2"),
    }) == Err(ConsumCode::TriColDup.val());
    m1 && m2 && m3 && m4 && m5 && m6 && m7
}

/// 同构声明注入：两锚全解析 + 链外全拒。
fn iso_inject() -> bool {
    let ok1 = iso_anchor_resolve("F12561").is_ok();
    let ok2 = iso_anchor_resolve("F12581").is_ok();
    let bad1 = iso_anchor_resolve("F12562").is_err();
    let bad2 = iso_anchor_resolve("F12580").is_err();
    let bad3 = iso_anchor_resolve("").is_err();
    ok1 && ok2 && bad1 && bad2 && bad3
}

/// 消费点守恒注入：八边冻结计数和 = 46（守恒账）。
fn points_inject() -> bool {
    let mut sum = 0u32;
    for r in CONSUM_ALL.iter() {
        sum = sum.saturating_add(r.frozen_points());
    }
    sum == POINT_TOTAL && POINT_TOTAL == 46
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in ConsumCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据承载：八边齐 + 消费点守恒。
    let mut c = EdgeCharter::new();
    for r in CONSUM_ALL.iter() {
        let _ = c.register_edge(EdgeRec {
            edge: *r,
            from: DomPair::D1,
            to: DomPair::E1,
            points: r.frozen_points(),
        });
    }
    v.push((
        "J2-八边齐",
        c.eight_edges_full() && EDGE_ROWS == 8,
        format!("边数 {}", c.edges.len()),
    ));
    v.push((
        "J2-消费点守恒",
        c.points_conserved(),
        format!("冻结和 {}", POINT_TOTAL),
    ));
    // 判据②：三列制度齐。
    for t in frozen_charter_items().iter() {
        let _ = c.register_item(t.clone());
    }
    v.push((
        "J2-三列制度齐",
        c.tricol_full(),
        format!("制度条 {}", c.items.len()),
    ));
    // 判据②：同构声明在册 + 版本号入账。
    let iso = c.register_iso();
    v.push((
        "J2-同构声明在册",
        iso == Ok(ISO_ANCHORS.len() as u32) && c.iso_on_ledger(),
        format!("锚 {:?}", iso),
    ));
    let _ = c.set_version(CHARTER_VERSION);
    v.push((
        "J2-版本号入账",
        c.version_on_ledger(),
        format!("版本 {}", c.version),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("逆向/漂点/重边/重条/悬空/降版/零版"),
    ));
    v.push(("J3-同构锚注入", iso_inject(), String::from("两绿三拒")));
    v.push(("J3-消费点注入", points_inject(), String::from("46 守恒")));
    // 边界：边序恰 0..8。
    let mut idx_sum = 0u32;
    for r in CONSUM_ALL.iter() {
        idx_sum = idx_sum.wrapping_add(r.idx());
    }
    v.push((
        "J4-边序恰0..8",
        idx_sum == 28,
        format!("边序和 {}", idx_sum),
    ));
    // 边界：域对单向（D1=0 < E1=1）。
    v.push((
        "J4-域对单向",
        DomPair::D1.idx() < DomPair::E1.idx(),
        String::from("D1→E1 冻结"),
    ));
    // 边界：制度配额（4+3+3=10）。
    v.push((
        "J4-制度配额",
        frozen_charter_items().len() == TRICOL_TOTAL,
        format!("制度 {}", frozen_charter_items().len()),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        c.fingerprint() != 0,
        format!("fp={:#x}", c.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80A40 窗口")));
    v
}

/// VX9 检查入口（卡 12641，UNX-F12642-J1 判据承载）。
pub fn run_unx12641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12641 D1到E1探测面消费边总表");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_eight_edges_full() {
        let mut c = EdgeCharter::new();
        for r in CONSUM_ALL.iter() {
            let _ = c.register_edge(EdgeRec {
                edge: *r,
                from: DomPair::D1,
                to: DomPair::E1,
                points: r.frozen_points(),
            });
        }
        assert!(c.eight_edges_full());
        assert!(c.points_conserved());
    }

    #[test]
    fn test_reverse_edge_rejected() {
        let mut c = EdgeCharter::new();
        assert_eq!(
            c.register_edge(EdgeRec {
                edge: ConsumEdge::Teb,
                from: DomPair::E1,
                to: DomPair::D1,
                points: 6,
            }),
            Err(ConsumCode::ReverseEdge.val())
        );
        assert_eq!(
            c.register_edge(EdgeRec {
                edge: ConsumEdge::Teb,
                from: DomPair::D1,
                to: DomPair::D1,
                points: 6,
            }),
            Err(ConsumCode::ReverseEdge.val())
        );
    }

    #[test]
    fn test_points_drift_rejected() {
        let mut c = EdgeCharter::new();
        assert_eq!(
            c.register_edge(EdgeRec {
                edge: ConsumEdge::Ntstatus,
                from: DomPair::D1,
                to: DomPair::E1,
                points: 8,
            }),
            Err(ConsumCode::PointBroken.val())
        );
        assert!(c.register_edge(EdgeRec {
            edge: ConsumEdge::Ntstatus,
            from: DomPair::D1,
            to: DomPair::E1,
            points: 9,
        })
        .is_ok());
    }

    #[test]
    fn test_version_monotone() {
        let mut c = EdgeCharter::new();
        assert_eq!(c.set_version(0), Err(ConsumCode::VersionMissing.val()));
        assert!(c.set_version(5).is_ok());
        assert_eq!(c.set_version(4), Err(ConsumCode::VersionRegress.val()));
        assert!(c.set_version(CHARTER_VERSION).is_ok());
        assert!(c.version_on_ledger());
    }

    #[test]
    fn test_iso_anchors() {
        let mut c = EdgeCharter::new();
        assert_eq!(c.register_iso(), Ok(2));
        assert!(c.iso_on_ledger());
        assert!(iso_anchor_resolve("F12561").is_ok());
        assert_eq!(iso_anchor_resolve("F12563"), Err(ConsumCode::IsoAnchorDangling.val()));
    }

    #[test]
    fn test_tricol_full() {
        let mut c = EdgeCharter::new();
        assert!(!c.tricol_full());
        for t in frozen_charter_items().iter() {
            let _ = c.register_item(t.clone());
        }
        assert!(c.tricol_full());
        assert_eq!(c.items.len(), TRICOL_TOTAL);
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_iso_inject() {
        assert!(iso_inject());
    }

    #[test]
    fn test_points_inject() {
        assert!(points_inject());
        let mut sum = 0u32;
        for r in CONSUM_ALL.iter() {
            sum += r.frozen_points();
        }
        assert_eq!(sum, 46);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80A40);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in ConsumCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xABCD).contains("未知"));
    }
}
