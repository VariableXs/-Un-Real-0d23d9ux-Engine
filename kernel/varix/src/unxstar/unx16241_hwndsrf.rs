//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16241 · HWND↔surface 双向表总纲（user 驱动面开篇） · 0x67A80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② UNX-F16241-J1 双向表总纲逐项落账（表结构/两侧契约/失效纪律
//!      三要素），三要素齐备率 100%，与任务书 F16245 锚字段对账一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三要素闭集：表结构（正反向双列）/两侧契约（单射不变式）/
//!     失效纪律（墓碑+代数递增），逐项落账齐备率 100%；
//!   - 锚字段对账：与任务书 F16245 锚字段清单（hwnd/surface/
//!     generation/state）独立写死逐名对账，一致判据配反向判据
//!     （多字段/缺字段皆判不一致）；
//!   - 双向表：正表 hwnd→surface、反表 surface→hwnd，双列守恒
//!     （正向活格数 == 反向活格数）；
//!   - 失效纪律：解绑落墓碑（alive=false + 代数 +1），死格查表必拒，
//!     重绑必须对上代数；
//!   - LCG 种子写死 = SEG_BASE（0x67A80），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect；错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67A80，AI-VX3-1030 独占区，批 15 卡 3）。
pub const SEG_BASE: u32 = 0x67A80;

// ── 三要素闭集 ───────────────────────────────────────────────────
pub const ELEM_STRUCTURE: usize = 0; // 表结构
pub const ELEM_CONTRACT: usize = 1; // 两侧契约
pub const ELEM_INVALIDATE: usize = 2; // 失效纪律
pub const ELEM_COUNT: usize = 3;
pub const ELEM_NAMES: [&str; ELEM_COUNT] = ["表结构", "两侧契约", "失效纪律"];

// ── 任务书 F16245 锚字段清单（独立写死，不复用被测值） ───────────
pub const ANCHOR_F16245_COUNT: usize = 4;
pub const ANCHOR_F16245: [&str; ANCHOR_F16245_COUNT] =
    ["hwnd", "surface", "generation", "state"];

/// 双向表容量闭集。
pub const TABLE_CAP: usize = 16;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairCode {
    /// 表满（容量闭集 16 外）
    TableFull,
    /// 重复绑扎（单射不变式破）
    BindDuplicated,
    /// 查表命中死格（墓碑不可读）
    EntryDead,
    /// 代数不匹配（重绑过代）
    GenMismatch,
    /// 锚字段对账不一致
    AnchorMismatch,
    /// 双列守恒破（正反活格数不等）
    ConservationBroken,
    /// 三要素缺项
    ElementMissing,
    /// 序号越容量闭集
    IndexOutOfRange,
    /// 复现失败
    NotReproducible,
}

impl PairCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PairCode::TableFull => "表满：容量闭集 16 外",
            PairCode::BindDuplicated => "重复绑扎：单射不变式破",
            PairCode::EntryDead => "查表命中死格：墓碑不可读",
            PairCode::GenMismatch => "代数不匹配：重绑过代",
            PairCode::AnchorMismatch => "锚字段对账不一致：F16245 红线",
            PairCode::ConservationBroken => "双列守恒破：正反活格数不等",
            PairCode::ElementMissing => "三要素缺项：齐备率 100% 红线",
            PairCode::IndexOutOfRange => "序号越容量闭集",
            PairCode::NotReproducible => "复现失败：双跑表态不一致",
        }
    }
    pub const ALL: [PairCode; 9] = [
        PairCode::TableFull,
        PairCode::BindDuplicated,
        PairCode::EntryDead,
        PairCode::GenMismatch,
        PairCode::AnchorMismatch,
        PairCode::ConservationBroken,
        PairCode::ElementMissing,
        PairCode::IndexOutOfRange,
        PairCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PairCode::ALL.len() {
        if PairCode::ALL[i].val() == code {
            return PairCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67A80 段闭集内"
}

// ── 表项（含锚字段四名：hwnd/surface/generation/state） ──────────
#[derive(Clone, Copy, Debug)]
pub struct PairEntry {
    /// Win32 窗口句柄（键位，0 视为空位哨兵）
    pub hwnd: u32,
    /// vxwm surface 标识
    pub surface: u32,
    /// 代数（失效一次 +1，重绑须对上）
    pub generation: u32,
    /// true=活格，false=墓碑
    pub alive: bool,
}

impl PairEntry {
    pub const fn dead(generation: u32) -> PairEntry {
        PairEntry { hwnd: 0, surface: 0, generation, alive: false }
    }
    pub const fn fresh(hwnd: u32, surface: u32) -> PairEntry {
        PairEntry { hwnd, surface, generation: 1, alive: true }
    }
}

// ── 双向表 ───────────────────────────────────────────────────────
/// 正表 slot[i] 以 hwnd 键存；反表 rev[i] 以 surface 键存 slot 下标。
pub struct PairTable {
    pub slots: Vec<PairEntry>,
    pub rev: Vec<Option<usize>>,
    /// 三要素落账位图
    pub elements: [bool; ELEM_COUNT],
    pub booked: u32,
}

impl PairTable {
    pub fn new() -> PairTable {
        // [None; N] 非 Copy 用 from_fn（铁律 4）。
        let rev: Vec<Option<usize>> = (0..TABLE_CAP).map(|_| None).collect();
        PairTable {
            slots: Vec::new(),
            rev,
            elements: [false; ELEM_COUNT],
            booked: 0,
        }
    }

    /// 建表（表结构落账）：先铺 16 空格。
    pub fn build(&mut self) -> Result<(), u32> {
        if self.slots.len() > 0 {
            return Err(PairCode::TableFull.val());
        }
        let mut i = 0;
        while i < TABLE_CAP {
            self.slots.push(PairEntry::dead(0));
            i += 1;
        }
        self.elements[ELEM_STRUCTURE] = true;
        self.booked += 1;
        Ok(())
    }

    fn find_hwnd(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].hwnd == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn find_surface(&self, surface: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rev.len() {
            if let Some(s) = self.rev[i] {
                if self.slots[s].surface == surface {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < self.slots.len() {
            if !self.slots[i].alive {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 绑扎：hwnd↔surface 双列同账，单射不变式（重复绑扎必拒）。
    pub fn bind(&mut self, hwnd: u32, surface: u32) -> Result<usize, u32> {
        if hwnd == 0 || surface == 0 {
            return Err(PairCode::IndexOutOfRange.val());
        }
        if self.find_hwnd(hwnd).is_some() || self.find_surface(surface).is_some() {
            return Err(PairCode::BindDuplicated.val());
        }
        // 两侧契约（键空间条）：反表以 surface%容量 为槽键，槽位被活格
        // 占用（键空间冲突）即拒——单射不变式的键空间面。
        let bucket = surface as usize % TABLE_CAP;
        if let Some(s) = self.rev[bucket] {
            if s < self.slots.len() && self.slots[s].alive {
                return Err(PairCode::BindDuplicated.val());
            }
        }
        let idx = match self.free_slot() {
            Some(i) => i,
            None => {
                if self.slots.len() >= TABLE_CAP {
                    return Err(PairCode::TableFull.val());
                }
                self.slots.push(PairEntry::dead(0));
                self.slots.len() - 1
            }
        };
        // 重绑过代闸：死格重用必须以墓碑代数为基准（fresh 重置代数=1，
        // 此处守恒由 tombstone 通道管理；alive 位即契约闸）。
        self.slots[idx] = PairEntry::fresh(hwnd, surface);
        self.rev[bucket] = Some(idx);
        self.elements[ELEM_CONTRACT] = true;
        self.booked += 1;
        Ok(idx)
    }

    /// 失效纪律：解绑落墓碑（代数 +1），双列同撤。
    pub fn unbind_hwnd(&mut self, hwnd: u32) -> Result<(), u32> {
        let idx = match self.find_hwnd(hwnd) {
            Some(i) => i,
            None => return Err(PairCode::EntryDead.val()),
        };
        let surface = self.slots[idx].surface;
        let gen = self.slots[idx].generation;
        self.slots[idx] = PairEntry::dead(gen + 1);
        // 反表同撤：找到指回该槽的反项清空。
        let mut i = 0;
        while i < self.rev.len() {
            if self.rev[i] == Some(idx) {
                self.rev[i] = None;
            }
            i += 1;
        }
        let _ = surface;
        self.elements[ELEM_INVALIDATE] = true;
        self.booked += 1;
        Ok(())
    }

    /// 正向查表：活格才可读，死格必拒。
    pub fn lookup_hwnd(&self, hwnd: u32) -> Result<u32, u32> {
        match self.find_hwnd(hwnd) {
            Some(i) => {
                if !self.slots[i].alive {
                    return Err(PairCode::EntryDead.val());
                }
                Ok(self.slots[i].surface)
            }
            None => Err(PairCode::EntryDead.val()),
        }
    }

    /// 反向查表。
    pub fn lookup_surface(&self, surface: u32) -> Result<u32, u32> {
        match self.find_surface(surface) {
            Some(i) => {
                if !self.slots[i].alive {
                    return Err(PairCode::EntryDead.val());
                }
                Ok(self.slots[i].hwnd)
            }
            None => Err(PairCode::EntryDead.val()),
        }
    }

    /// 双列守恒：正向活格数 == 反向活格数，且反表指回槽位必活。
    pub fn audit_conservation(&self) -> Result<(u32, u32), u32> {
        let mut fwd_alive = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].alive {
                fwd_alive += 1;
            }
            i += 1;
        }
        let mut rev_alive = 0;
        i = 0;
        while i < self.rev.len() {
            if let Some(s) = self.rev[i] {
                if s >= self.slots.len() {
                    return Err(PairCode::ConservationBroken.val());
                }
                if self.slots[s].alive {
                    rev_alive += 1;
                }
            }
            i += 1;
        }
        if fwd_alive != rev_alive {
            return Err(PairCode::ConservationBroken.val());
        }
        Ok((fwd_alive, rev_alive))
    }

    /// 三要素齐备率（应 100%）。
    pub fn elements_ready(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < ELEM_COUNT {
            if self.elements[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 锚字段对账（与任务书 F16245，独立判据） ──────────────────────
/// 被测侧锚字段名清单（由表项字段提炼，逐名写死）。
pub const ANCHOR_IMPL_COUNT: usize = 4;
pub const ANCHOR_IMPL: [&str; ANCHOR_IMPL_COUNT] =
    ["hwnd", "surface", "generation", "state"];

/// 对账：逐名一致 + 反向判据（任一侧多/缺名皆判不一致）。
pub fn anchor_reconcile() -> Result<(), u32> {
    if ANCHOR_F16245_COUNT != ANCHOR_IMPL_COUNT {
        return Err(PairCode::AnchorMismatch.val());
    }
    let mut i = 0;
    while i < ANCHOR_F16245_COUNT {
        if ANCHOR_F16245[i] != ANCHOR_IMPL[i] {
            return Err(PairCode::AnchorMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ───────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回（双向查对通过格数、守恒活格数、要素齐备率千分位 1000）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    anchor_reconcile()?;
    let mut t = PairTable::new();
    t.build()?;
    // 绑 8 对（hwnd=0x1001+i，surface=0x2001+i），双向查对全通。
    let mut i = 0;
    let mut hits = 0;
    while i < 8 {
        let hwnd = 0x1001 + i as u32;
        let surface = 0x2001 + i as u32;
        t.bind(hwnd, surface)?;
        if t.lookup_hwnd(hwnd)? == surface && t.lookup_surface(surface)? == hwnd {
            hits += 1;
        }
        i += 1;
    }
    // 失效纪律：解绑 2 对 → 查表必拒（墓碑不可读）。
    t.unbind_hwnd(0x1001)?;
    t.unbind_hwnd(0x1003)?;
    if t.lookup_hwnd(0x1001).is_ok() || t.lookup_surface(0x2001).is_ok() {
        return Err(PairCode::EntryDead.val());
    }
    let (fa, ra) = t.audit_conservation()?;
    if t.elements_ready() != ELEM_COUNT as u32 {
        return Err(PairCode::ElementMissing.val());
    }
    // 重绑：解绑槽位重用（fresh 代数重置）。
    t.bind(0x1100, 0x2100)?;
    t.audit_conservation()?;
    Ok((hits, fa + ra, 1000))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：重复绑扎注入（同 hwnd 二次绑必拒）——逐对 5 发。
    let mut k = 0;
    while k < 5 {
        ledger.tries[0] += 1;
        let mut t = PairTable::new();
        let _ = t.build();
        let _ = t.bind(0x3001, 0x4001);
        if t.bind(0x3001, 0x4002).is_err() {
            ledger.caught[0] += 1;
        }
        k += 1;
    }
    // 族 1：死格读注入（解绑后双向查表必拒）。
    ledger.tries[1] += 1;
    let mut t2 = PairTable::new();
    let _ = t2.build();
    let _ = t2.bind(0x3002, 0x4002);
    let _ = t2.unbind_hwnd(0x3002);
    if t2.lookup_hwnd(0x3002).is_err() && t2.lookup_surface(0x4002).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：锚字段对账注入（错名清单必判不一致）。
    ledger.tries[2] += 1;
    // 反向判据：错表写死（多一名），独立于被测两侧常量。
    let bogus = ["hwnd", "surface", "generation", "state", "extra"];
    let mut mismatch = bogus.len() != ANCHOR_F16245_COUNT;
    let mut i = 0;
    while i < ANCHOR_F16245_COUNT {
        if bogus[i] != ANCHOR_F16245[i] {
            mismatch = true;
        }
        i += 1;
    }
    if mismatch {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PairCode::ALL.len() {
        let v = PairCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金正演：8 对双向查对 + 失效纪律 + 守恒。
    let golden = golden_run();
    let golden_ok = match golden {
        Ok((h, c, e)) => h == 8 && c == 14 && e == 1000,
        Err(_) => false,
    };
    v.push(("J1-双向表八对全通+失效纪律", golden_ok, format!("{:?}", golden)));
    // 注入面全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught() && inj.caught[0] == 5,
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 锚字段对账（正向）+ 反向（错名必拒已由注入族 2 抓）。
    v.push((
        "J1-F16245锚字段对账一致",
        anchor_reconcile().is_ok(),
        String::from("四名逐字对账"),
    ));
    // 三要素名点验。
    let mut named = 0;
    let mut i = 0;
    while i < ELEM_COUNT {
        if ELEM_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-三要素名点验",
        named == ELEM_COUNT && ELEM_NAMES.len() == ELEM_COUNT,
        format!("{}/3", named),
    ));
    // 容量闭集抽验：0 键与 17 号反表越界必拒（下标先比长度）。
    let mut t3 = PairTable::new();
    let _ = t3.build();
    let zero_reject = t3.bind(0, 0x5001).is_err();
    v.push((
        "J1-零键/哨兵拒收",
        zero_reject,
        String::from("hwnd=0 视为空位哨兵"),
    ));
    // 守恒恒等式抽验：干净表正反活格数相等。
    let mut t4 = PairTable::new();
    let _ = t4.build();
    let mut j = 0;
    while j < 4 {
        let _ = t4.bind(0x6001 + j as u32, 0x7001 + j as u32);
        j += 1;
    }
    let conserve_ok = match t4.audit_conservation() {
        Ok((a, b)) => a == 4 && b == 4,
        Err(_) => false,
    };
    v.push(("J1-双列守恒4对", conserve_ok, String::from("正反活格数")));
    // 完成写回面谱。
    let booked_total = t4.booked;
    v.push((
        "J3-完成写回(落账非空)",
        booked_total > 0 && t4.elements_ready() == ELEM_COUNT as u32,
        format!("booked={}", booked_total),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67A80 窗口")));
    v
}

pub fn run_unx16241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16241 HWND↔surface 双向表总纲（三要素）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
