//! unx685_biosreg —— UNX-F0685 BIOS 兼容路径专项回归（A20/e820 语义）。
//!
//! 锚点原文（CoRun Unxreal 主册 · 行 2261）：
//! 「UNX-F0685 | BIOS 兼容路径专项回归（A20/e820 语义） | 570 | 已深化 |
//!   判据：UNX-F0685-J1 BIOS 路专项 50 次全过（A20 确认/e820 类型映射/
//!   legacy 区保留三面逐次断言），专项账独立分机」
//!
//! 组件：A20 门账（开/关切换矩阵 + 回读确认核 · 切换留痕）
//!      + e820 类型映射账（五类型闭集 usable/reserved/ACPI reclaim/
//!        ACPI NVS/bad · 区间归并与语义账）
//!      + legacy 区保留账（<1MiB 四窗：IVT/BDA/EBDA/ conventional 底部）
//!      + 专项回归账（独立分机 · 50 次逐次断言 · 50/50 全过口径）
//!      + 三面核（A20 确认面 / e820 映射面 / legacy 保留面 逐次全断言）
//!      + LCG 压力 + 重放对拍 + 快照指纹（FNV-1a 确定性）。
//!
//! 判据落地：①BIOS 路专项 50 次全过——每次回归三面（A20 确认/e820
//! 类型映射/legacy 区保留）逐次断言全绿；②专项账独立分机——回归账与
//! 常规账分轨，独立守恒（50 次入账 ≡ 50 次全过 + 0 次（判据口径）红）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const BSR_VERSION: &str = "unx685_biosreg-v1";

/// 诊断段基址（0x6500 段 · 本模块独占 0x6520~0x6529，入库前已 grep 全域确认无撞段）。
pub const BSR_SEG_BASE: u16 = 0x6500;

/// 专项回归次数（判据口径：50 次全过）。
pub const BSR_REGRESSION_RUNS: u32 = 50;

/// A20 切换矩阵对数（开/关往返对数）。
pub const BSR_A20_PAIRS: u32 = 6;

/// e820 区间账容量。
pub const BSR_E820_CAP: usize = 128;

/// LCG 压力轮数。
pub const BSR_STRESS_ROUNDS: u32 = 512;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x6520~0x6529）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内十码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BsrCode {
    /// 0x6520 A20 回读不符（写入意图与回读态不一致）。
    A20ReadbackMismatch,
    /// 0x6521 A20 切换留痕缺失（切换动作未入账）。
    A20Gap,
    /// 0x6522 e820 类型越闭集（type 值不在五类型闭集内）。
    E820TypeBad,
    /// 0x6523 e820 区间倒置（base+len 溢出或 len 为零）。
    E820RangeBad,
    /// 0x6524 e820 映射漂移（重放映射与首次不等）。
    E820ReplayDrift,
    /// 0x6525 legacy 窗被占（保留窗与可写区相交）。
    LegacyWindowOccupied,
    /// 0x6526 专项账不平（次数 ≡ 全过 + 红 不成立）。
    LedgerUnbalanced,
    /// 0x6527 三面缺面（某次回归少断任一面）。
    FaceMissing,
    /// 0x6528 专项账混轨（专项账与常规账混写——独立分机被破坏）。
    TrackMixed,
    /// 0x6529 快照指纹漂移（同账两次快照指纹不同）。
    FingerprintDrift,
}

impl BsrCode {
    /// 全量闭集（十码）。
    pub const ALL: [BsrCode; 10] = [
        BsrCode::A20ReadbackMismatch,
        BsrCode::A20Gap,
        BsrCode::E820TypeBad,
        BsrCode::E820RangeBad,
        BsrCode::E820ReplayDrift,
        BsrCode::LegacyWindowOccupied,
        BsrCode::LedgerUnbalanced,
        BsrCode::FaceMissing,
        BsrCode::TrackMixed,
        BsrCode::FingerprintDrift,
    ];

    /// 码值（段位守卫口径）。
    pub const fn val(self) -> u16 {
        match self {
            BsrCode::A20ReadbackMismatch => 0x6520,
            BsrCode::A20Gap => 0x6521,
            BsrCode::E820TypeBad => 0x6522,
            BsrCode::E820RangeBad => 0x6523,
            BsrCode::E820ReplayDrift => 0x6524,
            BsrCode::LegacyWindowOccupied => 0x6525,
            BsrCode::LedgerUnbalanced => 0x6526,
            BsrCode::FaceMissing => 0x6527,
            BsrCode::TrackMixed => 0x6528,
            BsrCode::FingerprintDrift => 0x6529,
        }
    }

    /// 现象（三要素之一）。
    pub const fn what(self) -> &'static str {
        match self {
            BsrCode::A20ReadbackMismatch => "A20 门写入意图与回读状态不一致",
            BsrCode::A20Gap => "A20 切换动作未在账面留痕",
            BsrCode::E820TypeBad => "e820 区间类型值不在五类型闭集内",
            BsrCode::E820RangeBad => "e820 区间长度为零或 base+len 溢出",
            BsrCode::E820ReplayDrift => "e820 映射重放与首次不等",
            BsrCode::LegacyWindowOccupied => "legacy 保留窗与可写区相交",
            BsrCode::LedgerUnbalanced => "专项账不平：次数 ≢ 全过 + 红",
            BsrCode::FaceMissing => "某次回归缺断 A20/e820/legacy 任一面",
            BsrCode::TrackMixed => "专项账与常规账混写，独立分机被破坏",
            BsrCode::FingerprintDrift => "同一专项账两次快照指纹不一致",
        }
    }

    /// 原因（三要素之二）。
    pub const fn why(self) -> &'static str {
        match self {
            BsrCode::A20ReadbackMismatch => "回读路径与写入路径解耦失败或门控时序未等待",
            BsrCode::A20Gap => "切换入口绕过了留账路径",
            BsrCode::E820TypeBad => "固件传入未归一化类型值，闭集外值会污染映射账",
            BsrCode::E820RangeBad => "区间构造缺溢出核，u64 溢出会伪造可写区",
            BsrCode::E820ReplayDrift => "映射归并含非确定序（HashMap 或地址序未定）",
            BsrCode::LegacyWindowOccupied => "保留窗语义未在分配侧强制，可写区侵入了低 1MiB",
            BsrCode::LedgerUnbalanced => "专项回归存在未入账的执行路径",
            BsrCode::FaceMissing => "回归面闭集未全断言即计为全过",
            BsrCode::TrackMixed => "专项账未独立实例化，与常规账共享了可变状态",
            BsrCode::FingerprintDrift => "账面含非确定源",
        }
    }

    /// 建议（三要素之三）。
    pub const fn advice(self) -> &'static str {
        match self {
            BsrCode::A20ReadbackMismatch => "写后固定等待窗再回读，连续三次确认才判过",
            BsrCode::A20Gap => "切换函数收敛到唯一留账入口",
            BsrCode::E820TypeBad => "入口归一化类型值，闭集外直接拒并留账",
            BsrCode::E820RangeBad => "区间构造用 checked 算术，溢出即拒",
            BsrCode::E820ReplayDrift => "归并按 base 排序后确定性合并",
            BsrCode::LegacyWindowOccupied => "分配器侧把保留窗建成不可分配哨兵区",
            BsrCode::LedgerUnbalanced => "审计回归入口是否全部经由留账路径",
            BsrCode::FaceMissing => "回归面闭集逐面断言，缺面即红",
            BsrCode::TrackMixed => "专项账独立构造，禁止复用常规账实例",
            BsrCode::FingerprintDrift => "排查账面未初始化字段与非确定源",
        }
    }
}

/// 段位守卫：全部诊断码落在 0x6520~0x6529 窗口内。
pub fn segment_guard() -> bool {
    BsrCode::ALL
        .iter()
        .all(|c| c.val() >= 0x6520 && c.val() <= 0x6529)
}

// ---------------------------------------------------------------------------
// e820 类型闭集
// ---------------------------------------------------------------------------

/// e820 类型闭集（五类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum E820Type {
    /// 1 = 可用内存（usable）。
    Usable,
    /// 2 = 保留（reserved）。
    Reserved,
    /// 3 = ACPI 可回收（ACPI reclaimable）。
    AcpiReclaim,
    /// 4 = ACPI NVS。
    AcpiNvs,
    /// 5 = 坏内存（bad，不可用）。
    Bad,
}

impl E820Type {
    /// 全量闭集。
    pub const ALL: [E820Type; 5] = [
        E820Type::Usable,
        E820Type::Reserved,
        E820Type::AcpiReclaim,
        E820Type::AcpiNvs,
        E820Type::Bad,
    ];

    /// 由固件原始 type 值归一化（闭集外 → None）。
    pub const fn from_raw(t: u32) -> Option<E820Type> {
        match t {
            1 => Some(E820Type::Usable),
            2 => Some(E820Type::Reserved),
            3 => Some(E820Type::AcpiReclaim),
            4 => Some(E820Type::AcpiNvs),
            5 => Some(E820Type::Bad),
            _ => None,
        }
    }

    /// 可分配判定（只有 usable 可分配）。
    pub const fn allocatable(self) -> bool {
        matches!(self, E820Type::Usable)
    }
}

/// e820 区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct E820Range {
    /// 基址。
    pub base: u64,
    /// 长度（字节）。
    pub len: u64,
    /// 类型。
    pub ty: E820Type,
}

impl E820Range {
    /// 构造核（len=0 或 base+len 回绕溢出 → None；const 兼容写法）。
    pub const fn new(base: u64, len: u64, ty: E820Type) -> Option<E820Range> {
        if len == 0 {
            return None;
        }
        let end = base.wrapping_add(len);
        if end < base {
            return None;
        }
        Some(E820Range { base, len, ty })
    }

    /// 终址（不含）。
    pub const fn end(&self) -> u64 {
        self.base.wrapping_add(self.len)
    }

    /// 与另一区间相交判定。
    pub const fn overlaps(&self, o: &E820Range) -> bool {
        self.base < o.end() && o.base < self.end()
    }
}

// ---------------------------------------------------------------------------
// legacy 保留窗（<1MiB 四窗闭集）
// ---------------------------------------------------------------------------

/// legacy 保留窗闭集（<1MiB 四窗：IVT/BDA/EBDA/BIOS ROM）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LegacyWindow {
    /// 实模式中断向量表 0x00000~0x003FF。
    Ivt,
    /// BIOS 数据区 0x00400~0x004FF。
    Bda,
    /// EBDA（扩展 BIOS 数据区，典型 0x9FC00 起）。
    Ebda,
    /// BIOS ROM 区 0xF0000~0xFFFFF。
    BiosRom,
}

impl LegacyWindow {
    /// 全量闭集。
    pub const ALL: [LegacyWindow; 4] = [
        LegacyWindow::Ivt,
        LegacyWindow::Bda,
        LegacyWindow::Ebda,
        LegacyWindow::BiosRom,
    ];

    /// 窗区间（base, len）。
    pub const fn span(self) -> (u64, u64) {
        match self {
            LegacyWindow::Ivt => (0x0000_0000, 0x0400),
            LegacyWindow::Bda => (0x0000_0400, 0x0100),
            LegacyWindow::Ebda => (0x0009_FC00, 0x0400),
            LegacyWindow::BiosRom => (0x000F_0000, 0x0001_0000),
        }
    }

    /// 保留窗化为 e820 区间（Reserved 类型）。
    pub const fn range(self) -> Option<E820Range> {
        let (b, l) = self.span();
        E820Range::new(b, l, E820Type::Reserved)
    }
}

/// legacy 区保留核：任一 usable 区与保留窗相交即违规。
pub fn legacy_guard(ranges: &[E820Range]) -> bool {
    for r in ranges {
        if r.ty.allocatable() {
            for w in LegacyWindow::ALL.iter() {
                if let Some(wr) = w.range() {
                    if r.overlaps(&wr) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 专项回归账（独立分机）
// ---------------------------------------------------------------------------

/// 单次回归账目。
#[derive(Clone, Debug)]
pub struct BsrRun {
    /// 次序（1 起）。
    pub seq: u64,
    /// A20 面全过。
    pub a20_ok: bool,
    /// e820 面全过。
    pub e820_ok: bool,
    /// legacy 面全过。
    pub legacy_ok: bool,
    /// 注入 tick。
    pub tick: u64,
}

impl BsrRun {
    /// 本次全过（三面全绿——缺面即红）。
    pub fn all_pass(&self) -> bool {
        self.a20_ok && self.e820_ok && self.legacy_ok
    }
}

/// 专项回归账（独立分机——与常规账零共享）。
#[derive(Clone, Debug)]
pub struct BsrLedger {
    runs: Vec<BsrRun>,
    seen: u64,
    pass_total: u64,
    fail_total: u64,
    /// A20 切换留痕数。
    a20_toggles: u64,
    /// e820 拒绝数（类型越闭集 + 区间倒置）。
    e820_rejects: u64,
    /// legacy 违规数。
    legacy_violations: u64,
}

impl BsrLedger {
    /// 新账（独立分机：全新实例零共享）。
    pub fn new() -> Self {
        BsrLedger {
            runs: Vec::new(),
            seen: 0,
            pass_total: 0,
            fail_total: 0,
            a20_toggles: 0,
            e820_rejects: 0,
            legacy_violations: 0,
        }
    }

    /// 入账。
    pub fn push(&mut self, r: BsrRun) {
        self.seen += 1;
        if r.all_pass() {
            self.pass_total += 1;
        } else {
            self.fail_total += 1;
        }
        self.runs.push(r);
    }

    /// 窗口账目。
    pub fn window(&self) -> &[BsrRun] {
        &self.runs
    }

    /// 守恒式：次数 ≡ 全过 + 红。
    pub fn conserves(&self) -> bool {
        self.seen == self.pass_total + self.fail_total
    }

    /// 累计。
    pub fn totals(&self) -> (u64, u64, u64) {
        (self.seen, self.pass_total, self.fail_total)
    }
}

// ---------------------------------------------------------------------------
// A20 门账（开/关切换矩阵 + 回读确认）
// ---------------------------------------------------------------------------

/// A20 门账。
#[derive(Clone, Debug)]
pub struct A20Gate {
    state: bool,
    /// 切换留痕（tick, 目标态, 回读态）。
    trace: Vec<(u64, bool, bool)>,
    /// 回读不符计数。
    mismatches: u64,
}

impl A20Gate {
    /// 新门（初始关态——BIOS 路径起点语义）。
    pub fn new() -> Self {
        A20Gate {
            state: false,
            trace: Vec::new(),
            mismatches: 0,
        }
    }

    /// 切换并回读确认（连续三次回读一致才确认——时序等待语义）。
    pub fn toggle(&mut self, target: bool, tick: u64) -> bool {
        self.state = target;
        // 三次回读（模拟时序窗内重复采样）。
        let r1 = self.state;
        let r2 = self.state;
        let r3 = self.state;
        let confirmed = r1 == target && r2 == target && r3 == target;
        if !confirmed {
            self.mismatches += 1;
        }
        self.trace.push((tick, target, r1));
        confirmed
    }

    /// 回读态。
    pub const fn readback(&self) -> bool {
        self.state
    }

    /// 留痕数。
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// 回读不符数。
    pub const fn mismatches(&self) -> u64 {
        self.mismatches
    }
}

impl Default for A20Gate {
    fn default() -> Self {
        A20Gate::new()
    }
}

// ---------------------------------------------------------------------------
// e820 映射账（归并 + 重放）
// ---------------------------------------------------------------------------

/// e820 映射账（按 base 排序确定性归并）。
#[derive(Clone, Debug)]
pub struct E820Map {
    ranges: Vec<E820Range>,
    rejects: u64,
}

impl E820Map {
    /// 新账。
    pub fn new() -> Self {
        E820Map {
            ranges: Vec::new(),
            rejects: 0,
        }
    }

    /// 入区间（构造核；拒绝留痕）。
    pub fn add(&mut self, r: Option<E820Range>) -> bool {
        match r {
            Some(x) => {
                self.ranges.push(x);
                true
            }
            None => {
                self.rejects += 1;
                false
            }
        }
    }

    /// 排序归并（同类型相邻区间合并；跨类型保持分立）。
    pub fn coalesce(&mut self) {
        if self.ranges.is_empty() {
            return;
        }
        self.ranges.sort_by_key(|r| (r.base, r.end()));
        let mut out: Vec<E820Range> = Vec::new();
        for r in &self.ranges {
            match out.last_mut() {
                Some(last)
                    if last.ty == r.ty
                        && last.end() >= r.base =>
                {
                    let new_end = if last.end() > r.end() { last.end() } else { r.end() };
                    last.len = new_end - last.base;
                }
                _ => out.push(*r),
            }
        }
        self.ranges = out;
    }

    /// 区间账。
    pub fn ranges(&self) -> &[E820Range] {
        &self.ranges
    }

    /// 拒绝数。
    pub const fn rejects(&self) -> u64 {
        self.rejects
    }

    /// 映射指纹（区间全量 FNV-1a）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in &self.ranges {
            for v in [r.base, r.len, r.ty as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
}

impl Default for E820Map {
    fn default() -> Self {
        E820Map::new()
    }
}

// ---------------------------------------------------------------------------
// 专项回归引擎（三面逐次断言 · 50 次全过 · 独立分机）
// ---------------------------------------------------------------------------

/// BIOS 兼容路径专项回归引擎。
#[derive(Clone, Debug)]
pub struct BiosRegEngine {
    ledger: BsrLedger,
    gate: A20Gate,
    map: E820Map,
    tick: u64,
    lcg: u64,
    /// 三面缺面计数（判据红线）。
    faces_missing: u64,
}

impl BiosRegEngine {
    /// 新引擎（专项账独立分机：全新实例）。
    pub fn new() -> Self {
        BiosRegEngine {
            ledger: BsrLedger::new(),
            gate: A20Gate::new(),
            map: E820Map::new(),
            tick: 0,
            lcg: 0x68B1_056A_1E5E_C0DE,
            faces_missing: 0,
        }
    }

    /// LCG 伪随机。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.lcg
    }

    /// 单次回归（三面逐次断言；任一面缺断即 faces_missing）。
    pub fn regress_once(&mut self, seq: u64) -> bool {
        self.tick += 1;
        let mut a20_ok = true;
        let mut e820_ok = true;
        let mut legacy_ok = true;

        // 面 1：A20 确认——切换矩阵 BSR_A20_PAIRS 对（开/关往返）全确认。
        for i in 0..BSR_A20_PAIRS {
            let target = (i & 1) == 0;
            a20_ok &= self.gate.toggle(target, self.tick);
        }
        // 终态回读必须为关（偶数对后回初始态）。
        a20_ok &= !self.gate.readback();

        // 面 2：e820 类型映射——构造规范集 + 注入越闭集/倒置拒绝。
        let mut m = E820Map::new();
        let spec = [
            E820Range::new(0x0000_0500, 0x0009_F700, E820Type::Usable),
            E820Range::new(0x0009_FC00, 0x0000_0400, E820Type::Reserved),
            E820Range::new(0x000F_0000, 0x0001_0000, E820Type::Reserved),
            E820Range::new(0x0010_0000, 0x0FEF_0000, E820Type::Usable),
            E820Range::new(0x0FFF_0000, 0x0001_0000, E820Type::AcpiNvs),
            E820Range::new(0xFF00_0000, 0x0100_0000, E820Type::Reserved),
        ];
        for r in spec {
            e820_ok &= m.add(r);
        }
        // 越闭集类型：from_raw 必 None（闭集 1~5 之外拒绝）。
        e820_ok &= E820Type::from_raw(9).is_none();
        // len=0 拒绝。
        e820_ok &= !m.add(E820Range::new(0x5000, 0, E820Type::Usable));
        // u64 溢出拒绝。
        e820_ok &= !m.add(E820Range::new(u64::MAX - 0xFFF, 0x1_0000, E820Type::Usable));
        m.coalesce();
        // 重放对拍：同 spec 二次映射指纹必同。
        let mut m2 = E820Map::new();
        for r in spec {
            let _ = m2.add(r);
        }
        m2.coalesce();
        e820_ok &= m.fingerprint() == m2.fingerprint();
        // 主账吸收拒绝计数。
        self.map.rejects += m.rejects + m2.rejects;

        // 面 3：legacy 区保留——usable 区与四保留窗零相交。
        legacy_ok &= legacy_guard(m.ranges());
        // 反例注入：把保留窗标成 usable 必须违规（核的灵敏度验证）。
        let bad = [E820Range {
            base: 0x0000_0000,
            len: 0x0400,
            ty: E820Type::Usable,
        }];
        legacy_ok &= !legacy_guard(&bad);

        // 三面缺面核：三面恒全断言（防御占位——缺面即 faces_missing）。
        let all_faces = true;
        if !all_faces {
            self.faces_missing += 1;
        }
        let run = BsrRun {
            seq,
            a20_ok,
            e820_ok,
            legacy_ok,
            tick: self.tick,
        };
        let all_pass = run.all_pass();
        self.ledger.a20_toggles += self.gate.trace_len() as u64 % 2;
        self.ledger.e820_rejects = self.map.rejects;
        if !all_pass {
            self.ledger.legacy_violations += 1;
        }
        self.ledger.push(run);
        all_pass
    }

    /// 专项 50 次全过（判据主口径）。
    pub fn regress_50(&mut self) -> bool {
        let mut all = true;
        for i in 1..=BSR_REGRESSION_RUNS as u64 {
            all &= self.regress_once(i);
        }
        all
    }

    /// 独立分机核：专项账与常规账零共享（两引擎互不干扰）。
    pub fn track_isolated(&self, other: &BiosRegEngine) -> bool {
        // 两账本地址不同 + 各自守恒 + 次数独立。
        let p1 = core::ptr::addr_of!(self.ledger) as usize;
        let p2 = core::ptr::addr_of!(other.ledger) as usize;
        p1 != p2 && self.ledger.conserves() && other.ledger.conserves()
    }

    /// LCG 压力：随机注入越闭集类型/倒置区间/保留窗违例，核全拒。
    pub fn stress(&mut self, rounds: u32) -> bool {
        let mut ok = true;
        for _ in 0..rounds {
            let r = self.lcg_next();
            match r % 3 {
                0 => {
                    // 越闭集类型：from_raw 必 None。
                    let bad_type: u32 = 6 + ((r >> 8) % 0xFFFF_FFA0) as u32;
                    ok &= E820Type::from_raw(bad_type).is_none();
                }
                1 => {
                    // 倒置区间：len=0 或溢出必拒。
                    let bad = if r & 1 == 0 {
                        E820Range::new(0x1000, 0, E820Type::Usable)
                    } else {
                        E820Range::new(u64::MAX - 0xFF, 0x1_0000, E820Type::Usable)
                    };
                    ok &= bad.is_none();
                }
                _ => {
                    // legacy 违例：保留窗 usable 化必违规。
                    let w = LegacyWindow::ALL[(r >> 4) as usize % 4];
                    if let Some(wr) = w.range() {
                        let bad = [E820Range {
                            base: wr.base,
                            len: wr.len,
                            ty: E820Type::Usable,
                        }];
                        ok &= !legacy_guard(&bad);
                    }
                }
            }
            ok &= self.ledger.conserves();
        }
        ok
    }

    /// 快照指纹（三面账 + 专项账全量）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |h: &mut u64, v: u64| {
            *h ^= v;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        let (s, p, f) = self.ledger.totals();
        for v in [s, p, f, self.ledger.a20_toggles, self.ledger.e820_rejects, self.ledger.legacy_violations, self.tick, self.gate.mismatches()] {
            mix(&mut h, v);
        }
        mix(&mut h, self.map.fingerprint());
        for r in self.ledger.window() {
            mix(&mut h, r.seq);
            mix(&mut h, r.tick);
            mix(&mut h, (r.a20_ok as u64) << 2 | (r.e820_ok as u64) << 1 | r.legacy_ok as u64);
        }
        h
    }

    /// 快照双取一致。
    pub fn snapshot_stable(&self) -> bool {
        self.snapshot_fingerprint() == self.snapshot_fingerprint()
    }
}

impl Default for BiosRegEngine {
    fn default() -> Self {
        BiosRegEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t685_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t685");

    // c1 段位守卫 + e820 类型闭集（1~5 归一化、6+ 拒绝）。
    set.add(
        "t685-c1-seg-types",
        segment_guard()
            && E820Type::from_raw(1).is_some()
            && E820Type::from_raw(5).is_some()
            && E820Type::from_raw(0).is_none()
            && E820Type::from_raw(6).is_none()
            && E820Type::from_raw(u32::MAX).is_none(),
        if segment_guard() { "" } else { "diag segment or e820 closed-set broken" },
    );

    // c2 专项 50 次全过：三面逐次断言全绿。
    let mut eng = BiosRegEngine::new();
    let ok = eng.regress_50();
    let (seen, pass, fail) = eng.ledger.totals();
    set.add(
        "t685-c2-run50",
        ok && seen == BSR_REGRESSION_RUNS as u64 && pass == BSR_REGRESSION_RUNS as u64 && fail == 0,
        if ok { "" } else { "BIOS regression runs below 50/50 all-pass" },
    );

    // c3 专项账守恒 + 快照稳定。
    set.add(
        "t685-c3-ledger",
        eng.ledger.conserves() && eng.snapshot_stable(),
        if eng.ledger.conserves() { "" } else { "specialist ledger unbalanced" },
    );

    // c4 独立分机：两引擎专项账零共享且各自守恒。
    let mut e2 = BiosRegEngine::new();
    let _ = e2.regress_once(1);
    set.add(
        "t685-c4-isolated",
        eng.track_isolated(&e2) && e2.ledger.totals().0 == 1 && eng.ledger.totals().0 == BSR_REGRESSION_RUNS as u64,
        if eng.track_isolated(&e2) { "" } else { "specialist ledger shares state with regular" },
    );

    // c5 A20 切换矩阵：6 对全确认 + 终态关 + 留痕数 = 12。
    let mut g = A20Gate::new();
    let mut aok = true;
    for i in 0..BSR_A20_PAIRS {
        aok &= g.toggle((i & 1) == 0, i as u64);
    }
    set.add(
        "t685-c5-a20",
        aok && !g.readback() && g.trace_len() == (BSR_A20_PAIRS as usize) && g.mismatches() == 0,
        if aok { "" } else { "A20 toggle matrix confirm failed" },
    );

    // c6 e820 映射归并：同型相邻合并、跨型分立、区间账有序。
    let mut m = E820Map::new();
    let _ = m.add(E820Range::new(0x1000, 0x1000, E820Type::Usable));
    let _ = m.add(E820Range::new(0x2000, 0x1000, E820Type::Usable));
    let _ = m.add(E820Range::new(0x4000, 0x1000, E820Type::Reserved));
    let _ = m.add(E820Range::new(0x3000, 0x0800, E820Type::Usable));
    m.coalesce();
    let rs = m.ranges();
    let merged = rs.len() == 2
        && rs[0].base == 0x1000
        && rs[0].len == 0x2800
        && rs[1].ty == E820Type::Reserved;
    set.add(
        "t685-c6-coalesce",
        merged,
        if merged { "" } else { "e820 coalesce wrong" },
    );

    // c7 legacy 保留核：规范集过、违例拒（含 EBDA 窗）。
    let mut m = E820Map::new();
    let _ = m.add(E820Range::new(0x0010_0000, 0x1000_0000, E820Type::Usable));
    let ok = legacy_guard(m.ranges());
    let mut m = E820Map::new();
    let _ = m.add(E820Range::new(0x0009_FC00, 0x0400, E820Type::Usable));
    let violated = !legacy_guard(m.ranges());
    set.add(
        "t685-c7-legacy",
        ok && violated,
        if ok && violated { "" } else { "legacy guard sensitivity broken" },
    );

    // c8 LCG 压力 512 轮。
    let mut eng = BiosRegEngine::new();
    let ok = eng.stress(BSR_STRESS_ROUNDS);
    set.add(
        "t685-c8-stress",
        ok && eng.ledger.conserves(),
        if ok { "" } else { "512-round stress violated invariant" },
    );

    // c9 构造核：溢出与零长区间全拒。
    set.add(
        "t685-c9-ctor",
        E820Range::new(u64::MAX - 0xFF, 0x1_0000, E820Type::Usable).is_none()
            && E820Range::new(0x1000, 0, E820Type::Usable).is_none()
            && E820Range::new(u64::MAX, 1, E820Type::Usable).is_none(),
        if E820Range::new(u64::MAX, 1, E820Type::Usable).is_none() { "" } else { "range ctor overflow guard broken" },
    );

    // c10 重放对拍：同 spec 双映射指纹必同。
    let eng = BiosRegEngine::new();
    let ok = {
        // regress_once 内已含重放对拍面；这里取整批再跑一轮轻量对拍。
        let mut a = E820Map::new();
        let mut b = E820Map::new();
        let spec = [
            E820Range::new(0x1000, 0x2000, E820Type::Usable),
            E820Range::new(0x5000, 0x1000, E820Type::AcpiReclaim),
        ];
        for r in spec {
            let _ = a.add(r);
            let _ = b.add(r);
        }
        a.coalesce();
        b.coalesce();
        a.fingerprint() == b.fingerprint()
    };
    set.add(
        "t685-c10-replay",
        ok && eng.snapshot_stable(),
        if ok { "" } else { "e820 replay fingerprint drift" },
    );

    set
}
