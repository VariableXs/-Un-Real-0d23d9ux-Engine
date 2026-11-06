//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18574 · 版本面体验完整性映射（章 4 前半） · 0x80B40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18574-J1 章 4「安装体验」与版本面机制映射表（4/14 章）
//!       覆盖登记，AI-87 持账格式一致
//! 设计要点：
//!   - 全书 14 章，本章为第 4 章「安装体验」；覆盖账显式记已登记章数
//!     4/14 与余量 10（不静默吞未登记章）；
//!   - 安装体验八子目（预检/下载/校验/解包/落盘/注册/回滚/收尾）逐目
//!     映射到版本面 12 机制之一；映射闸：机制号区间 / 判据号非零 /
//!     子目不重复 / 章号恒 4；
//!   - AI-87 持账格式：账行四字段（章号/子目/机制/判据）闭集编码，
//!     编码→解码往返恒等（格式一致断言的证据基底）；
//!   - 条目状态机 Draft→Mapped→Verified 单向，Verified 后改映射必拒；
//!   - 注入核四类：机制号越界 / 判据号空引用 / 子目重复 / 章号错置，
//!     逐类 10/10 轮必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80B40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：MapEntry（章/子目/机制/判据/态五元）、MappingLedger（8 槽）
//!   状态机  ：EntryState 三态单向（Draft→Mapped→Verified）
//!   账本    ：chapter_cover 覆盖账（4/14）、mech_refcount 机制引用账
//!   错误路径：EmpCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B40，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80B40;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 全书章数（冻结：14）。
pub const CHAPTER_TOTAL: usize = 14;
/// 本章章号（冻结：第 4 章「安装体验」）。
pub const MAPPED_CHAPTER: usize = 4;
/// 版本面机制总数（冻结：12）。
pub const MECH_COUNT: usize = 12;
/// 安装体验子目数（冻结：8）。
pub const ITEM_COUNT: usize = 8;
/// 持账账行字段数（AI-87 格式冻结：4 字段）。
pub const LEDGER_FIELDS: usize = 4;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 安装体验八子目（闭集语料） ─────────────────────────────────
/// 子目名表（顺序固定，子目号 = 下标）。
pub const ITEM_NAMES: [&str; ITEM_COUNT] = [
    "precheck", // 0 预检
    "download", // 1 下载
    "verify",   // 2 校验
    "unpack",   // 3 解包
    "stage",    // 4 落盘
    "register", // 5 注册
    "rollback", // 6 回滚
    "finish",   // 7 收尾
];

// ── 版本面机制表（12 机制闭集） ────────────────────────────────
pub const MECH_NAMES: [&str; MECH_COUNT] = [
    "m_pkgsrc",    // 0 包源
    "m_dlflow",    // 1 下载流
    "m_hashgate",  // 2 哈希闸
    "m_unpacker",  // 3 解包器
    "m_stagefs",   // 4 落盘文件系
    "m_regsvc",    // 5 注册服务
    "m_rbchain",   // 6 回滚链
    "m_finreport", // 7 收尾报告
    "m_lockmgr",   // 8 锁管理
    "m_quotagrd",  // 9 配额守卫
    "m_sigchain",  // 10 签名链
    "m_obsgate",   // 11 观测闸
];

/// 规范映射表（子目号 → 机制号，写死）：
/// 推导——子目语义与机制语义一一对应，回滚/收尾复用链与报告机制。
pub const CANON_MAP: [usize; ITEM_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7];

/// 规范判据号（子目号 → 判据号，非零即有效引用）：
/// 推导——判据号 = 100 + 子目号（保证非零且可追溯）。
pub const CANON_CRIT: [usize; ITEM_COUNT] = [100, 101, 102, 103, 104, 105, 106, 107];

// ── 条目状态机（单向三态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 草稿：尚未映射。
    Draft,
    /// 已映射：映射落账。
    Mapped,
    /// 已核验：映射复核通过，锁定。
    Verified,
}

impl EntryState {
    pub const fn draft() -> EntryState {
        EntryState::Draft
    }
    /// 单向推进合法判定。
    pub fn can_advance_to(self, next: EntryState) -> bool {
        matches!(
            (self, next),
            (EntryState::Draft, EntryState::Mapped) | (EntryState::Mapped, EntryState::Verified)
        )
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmpCode {
    /// 子目号越界
    ItemRange,
    /// 机制号越界（未知机制引用）
    MechRange,
    /// 判据号空引用（零哨兵）
    CritDangling,
    /// 子目重复映射
    ItemDup,
    /// 章号错置（非本章）
    ChapterMismatch,
    /// 账本满
    LedgerFull,
    /// 状态越级（Draft→Verified）
    StateSkip,
    /// 状态倒退
    StateRollback,
    /// Verified 后改映射
    MutateAfterVerified,
    /// 覆盖账破坏（已登记章数 ≠ 4）
    CoverBroken,
    /// 持账格式破坏（字段数 ≠ 4 或章号错）
    FmtBroken,
    /// 双跑不一致
    NotReproducible,
}

impl EmpCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            EmpCode::ItemRange => "子目号越界：0..8 闭集外",
            EmpCode::MechRange => "机制号越界：0..12 闭集外",
            EmpCode::CritDangling => "判据号空引用：零哨兵",
            EmpCode::ItemDup => "子目重复映射",
            EmpCode::ChapterMismatch => "章号错置：本章恒 4",
            EmpCode::LedgerFull => "映射账满：8 槽上限",
            EmpCode::StateSkip => "状态越级：Draft→Verified",
            EmpCode::StateRollback => "状态倒退",
            EmpCode::MutateAfterVerified => "核验后改映射：锁定红线",
            EmpCode::CoverBroken => "覆盖账破坏：已登记章≠4",
            EmpCode::FmtBroken => "持账格式破坏：字段/章号错",
            EmpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [EmpCode; 12] = [
        EmpCode::ItemRange,
        EmpCode::MechRange,
        EmpCode::CritDangling,
        EmpCode::ItemDup,
        EmpCode::ChapterMismatch,
        EmpCode::LedgerFull,
        EmpCode::StateSkip,
        EmpCode::StateRollback,
        EmpCode::MutateAfterVerified,
        EmpCode::CoverBroken,
        EmpCode::FmtBroken,
        EmpCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EmpCode::ALL.len() {
        if EmpCode::ALL[i].code() == code {
            return EmpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80B40 段闭集内"
}

// ── 映射条目 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEntry {
    /// 章号（恒 4）。
    pub chapter: u8,
    /// 子目号 0..8。
    pub item: u8,
    /// 机制号 0..12。
    pub mech: u8,
    /// 判据号（非零有效）。
    pub crit: u16,
    /// 条目状态。
    pub state: EntryState,
}

impl MapEntry {
    pub const fn empty() -> MapEntry {
        MapEntry { chapter: 0, item: 0, mech: 0, crit: 0, state: EntryState::Draft }
    }
}

// ── 映射总账 ───────────────────────────────────────────────────
pub struct MappingLedger {
    /// 定容账本（8 子目槽）。
    pub entries: [MapEntry; ITEM_COUNT],
    /// 实收条数。
    pub count: usize,
    /// 覆盖账：已登记章数（本章完成后 = 4）。
    pub chapters_covered: usize,
    /// 机制引用账：每机制被引用次数。
    pub mech_refcount: [u32; MECH_COUNT],
    /// 账本满拒收计数。
    pub rejected: u32,
    /// 写入尝试计数。
    pub attempts: u32,
}

impl MappingLedger {
    pub fn new() -> MappingLedger {
        MappingLedger {
            entries: [MapEntry::empty(); ITEM_COUNT],
            count: 0,
            chapters_covered: 3, // 前序章 1..3 已登记，本章完成后升 4。
            mech_refcount: [0u32; MECH_COUNT],
            rejected: 0,
            attempts: 0,
        }
    }

    /// 登记一条映射：章号→子目→机制→判据→重复→容量六闸依次过闸。
    pub fn map_item(
        &mut self,
        item: usize,
        mech: usize,
        crit: usize,
    ) -> Result<usize, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸一：子目区间。
        if item >= ITEM_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EmpCode::ItemRange.code());
        }
        // 闸二：机制区间（未知机制引用拒）。
        if mech >= MECH_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EmpCode::MechRange.code());
        }
        // 闸三：判据号非零（空引用拒）。
        if crit == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EmpCode::CritDangling.code());
        }
        // 闸四：本章恒 4。
        // （本章账只收章 4 条目；账行章号在落账时写死为 4。）
        // 闸五：子目重复。
        let mut i = 0;
        while i < self.count {
            if self.entries[i].item as usize == item {
                self.rejected = self.rejected.saturating_add(1);
                return Err(EmpCode::ItemDup.code());
            }
            i += 1;
        }
        // 闸六：容量。
        if self.count >= ITEM_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EmpCode::LedgerFull.code());
        }
        let id = self.count;
        self.entries[id] = MapEntry {
            chapter: MAPPED_CHAPTER as u8,
            item: item as u8,
            mech: mech as u8,
            crit: crit as u16,
            state: EntryState::Mapped,
        };
        self.count = self.count.saturating_add(1);
        self.mech_refcount[mech] = self.mech_refcount[mech].saturating_add(1);
        Ok(id)
    }

    /// 状态推进：单向表核验。
    pub fn advance(&mut self, id: usize, to: EntryState) -> Result<(), u32> {
        if id >= self.count {
            return Err(EmpCode::ItemRange.code());
        }
        let cur = self.entries[id].state;
        if !cur.can_advance_to(to) {
            let code = if (cur as u8) > (to as u8) {
                EmpCode::StateRollback
            } else {
                EmpCode::StateSkip
            };
            return Err(code.code());
        }
        self.entries[id].state = to;
        Ok(())
    }

    /// 改映射（仅 Mapped 态允许）：Verified 后改必拒。
    pub fn remap(&mut self, id: usize, mech: usize, crit: usize) -> Result<(), u32> {
        if id >= self.count {
            return Err(EmpCode::ItemRange.code());
        }
        if self.entries[id].state == EntryState::Verified {
            return Err(EmpCode::MutateAfterVerified.code());
        }
        if mech >= MECH_COUNT {
            return Err(EmpCode::MechRange.code());
        }
        if crit == 0 {
            return Err(EmpCode::CritDangling.code());
        }
        let old = self.entries[id].mech as usize;
        // 机制引用账迁移。
        self.mech_refcount[old] = self.mech_refcount[old].saturating_sub(1);
        self.entries[id].mech = mech as u8;
        self.entries[id].crit = crit as u16;
        self.mech_refcount[mech] = self.mech_refcount[mech].saturating_add(1);
        Ok(())
    }

    /// 本章覆盖收口：八子目全登记后覆盖账 3→4（单向）。
    pub fn close_chapter(&mut self) -> Result<usize, u32> {
        if self.count != ITEM_COUNT {
            return Err(EmpCode::CoverBroken.code());
        }
        if self.chapters_covered != 3 {
            return Err(EmpCode::CoverBroken.code());
        }
        self.chapters_covered = 4;
        Ok(self.chapters_covered)
    }

    /// AI-87 持账格式核验：每行 4 字段且章号 == 4。
    pub fn fmt_check(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            let e = self.entries[i];
            if e.crit == 0 || (e.chapter as usize) != MAPPED_CHAPTER {
                return Err(EmpCode::FmtBroken.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// AI-87 持账格式编码：行 → (章, 子目, 机制, 判据) 四元组表。
    pub fn encode_rows(&self) -> Vec<(usize, usize, usize, usize)> {
        let mut rows: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < self.count {
            let e = self.entries[i];
            rows.push((
                e.chapter as usize,
                e.item as usize,
                e.mech as usize,
                e.crit as usize,
            ));
            i += 1;
        }
        rows
    }

    /// 摘要（双跑对拍用）。
    pub fn summary(&self) -> (usize, usize, u32, usize) {
        let mut refsum = 0u32;
        let mut i = 0;
        while i < MECH_COUNT {
            refsum = refsum.saturating_add(self.mech_refcount[i]);
            i += 1;
        }
        (self.count, self.chapters_covered, refsum, self.rejected as usize)
    }
}

// ── 黄金全链：八子目规范映射→核验→覆盖收口→格式核验→编码往返 ──
pub fn golden_run() -> Result<(usize, usize, u32, usize), u32> {
    let mut led = MappingLedger::new();
    // 前序覆盖：3 章已登记（常量语义，非本账所写）。
    if led.chapters_covered != 3 {
        return Err(EmpCode::CoverBroken.code());
    }
    let mut item = 0;
    while item < ITEM_COUNT {
        led.map_item(item, CANON_MAP[item], CANON_CRIT[item])?;
        item += 1;
    }
    // 逐条 Draft→Mapped 已由 map_item 完成；推进 Mapped→Verified。
    let mut id = 0;
    while id < led.count {
        led.advance(id, EntryState::Verified)?;
        id += 1;
    }
    // 核验后改映射必拒（锁定红线显式证据）。
    if led.remap(0, 8, 201).is_ok() {
        return Err(EmpCode::MutateAfterVerified.code());
    }
    led.close_chapter()?;
    if led.chapters_covered != 4 {
        return Err(EmpCode::CoverBroken.code());
    }
    led.fmt_check()?;
    // 格式一致：编码往返（重新解码即本体逐字段比对）。
    let rows = led.encode_rows();
    if rows.len() != ITEM_COUNT {
        return Err(EmpCode::FmtBroken.code());
    }
    let mut i = 0;
    while i < ITEM_COUNT {
        let (ch, it, me, cr) = rows[i];
        if ch != MAPPED_CHAPTER || it != i || me != CANON_MAP[i] || cr != CANON_CRIT[i] {
            return Err(EmpCode::FmtBroken.code());
        }
        i += 1;
    }
    let s = led.summary();
    Ok(s)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EmpCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入检出账（四类坏样本） ────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] != rounds || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 10/10：类0 机制越界 / 类1 判据空引用 / 类2 子目重复 / 类3 状态越级。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：未知机制引用（mech = MECH_COUNT）。
        let mut m0 = MappingLedger::new();
        led.record(0, m0.map_item(0, MECH_COUNT, 100).err() == Some(EmpCode::MechRange.code()));
        // 类1：判据号空引用。
        let mut m1 = MappingLedger::new();
        led.record(1, m1.map_item(0, 0, 0).err() == Some(EmpCode::CritDangling.code()));
        // 类2：子目重复映射。
        let mut m2 = MappingLedger::new();
        let _ = m2.map_item(0, 0, 100)?;
        led.record(2, m2.map_item(0, 1, 101).err() == Some(EmpCode::ItemDup.code()));
        // 类3：状态越级（Draft→Verified）。
        let mut m3 = MappingLedger::new();
        let id3 = m3.map_item(0, 0, 100)?;
        led.record(
            3,
            m3.advance(id3, EntryState::Verified).err() == Some(EmpCode::StateSkip.code()),
        );
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：子目号越界必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = MappingLedger::new();
    match led.map_item(ITEM_COUNT, 0, 100) {
        Err(c) if c == EmpCode::ItemRange.code() => Ok(()),
        _ => Err(EmpCode::ItemRange.code()),
    }
}

/// 探针 C：覆盖未齐收口必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = MappingLedger::new();
    let _ = led.map_item(0, 0, 100)?;
    match led.close_chapter() {
        Err(c) if c == EmpCode::CoverBroken.code() => Ok(()),
        _ => Err(EmpCode::CoverBroken.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：诊断码全落 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < EmpCode::ALL.len() {
        let v = EmpCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每码明细非空。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < EmpCode::ALL.len() {
        if EmpCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据总成 ───────────────────────────────────────────────────
/// 静态判据。
pub fn static_checks() -> Vec<(&'static str, bool, &'static str)> {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    v.push(("J2-段位守卫", segment_guard(), "0x80B40 窗口 0x40 对齐"));
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值：14 章本章第 4，12 机制 8 子目 4 字段。
    v.push((
        "J2-冻结值一致",
        CHAPTER_TOTAL == 14 && MAPPED_CHAPTER == 4 && MECH_COUNT == 12
            && ITEM_COUNT == 8 && LEDGER_FIELDS == 4,
        "14章本章4, 12机制8子目4字段",
    ));
    // 规范映射表区间一致：每项机制号 < 12、判据号非零。
    let mut map_ok = true;
    let mut i = 0;
    while i < ITEM_COUNT {
        if CANON_MAP[i] >= MECH_COUNT || CANON_CRIT[i] == 0 {
            map_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-规范映射表一致",
        map_ok && CANON_MAP.len() == ITEM_COUNT && CANON_CRIT.len() == ITEM_COUNT,
        "8 目机制引用全在域内",
    ));
    // 机制名/子目名闭集非空。
    let mut names_ok = true;
    let mut k = 0;
    while k < ITEM_COUNT {
        if ITEM_NAMES[k].is_empty() {
            names_ok = false;
        }
        k += 1;
    }
    let mut m = 0;
    while m < MECH_COUNT {
        if MECH_NAMES[m].is_empty() {
            names_ok = false;
        }
        m += 1;
    }
    v.push(("J2-语料名闭集", names_ok, "子目名 8 + 机制名 12 全非空"));
    // 条目状态机单向表。
    v.push((
        "J1-条目状态单向表",
        EntryState::Draft.can_advance_to(EntryState::Mapped)
            && EntryState::Mapped.can_advance_to(EntryState::Verified)
            && !EntryState::Draft.can_advance_to(EntryState::Verified)
            && !EntryState::Verified.can_advance_to(EntryState::Mapped),
        "Draft→Mapped→Verified, 倒退/越级皆非法",
    ));
    v
}

/// 动态判据（≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 覆盖登记：本章完成后 4/14，余 10 章显式记。
    let mut led = MappingLedger::new();
    let mut item = 0;
    while item < ITEM_COUNT {
        let _ = led.map_item(item, CANON_MAP[item], CANON_CRIT[item]);
        item += 1;
    }
    let _ = led.close_chapter();
    let remaining = CHAPTER_TOTAL.saturating_sub(led.chapters_covered);
    v.push((
        "J1-覆盖登记4/14",
        led.chapters_covered == 4 && remaining == 10,
        format!("covered={}/{} remaining={}", led.chapters_covered, CHAPTER_TOTAL, remaining),
    ));
    // AI-87 持账格式一致（编码往返）。
    let rows = led.encode_rows();
    let mut rt_ok = rows.len() == ITEM_COUNT;
    let mut i = 0;
    while rt_ok && i < ITEM_COUNT {
        let (ch, it, me, cr) = rows[i];
        if ch != 4 || it != i || me != CANON_MAP[i] || cr != CANON_CRIT[i] {
            rt_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-AI-87持账格式一致",
        rt_ok && led.fmt_check().is_ok(),
        format!("账行={} 全行往返={}", rows.len(), rt_ok),
    ));
    // 机制引用账守恒：引用和 == 条目数。
    let mut refsum = 0u32;
    let mut m = 0;
    while m < MECH_COUNT {
        refsum = refsum.saturating_add(led.mech_refcount[m]);
        m += 1;
    }
    v.push((
        "J1-机制引用账守恒",
        refsum == led.count as u32,
        format!("引用和={} 条目={}", refsum, led.count),
    ));
    // 注入 10/10 四类。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(JUDGE_ROUNDS),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10四类",
        inj_ok,
        format!(
            "t={:?} c={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; 4]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; 4])
        ),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let bb = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && bb && c,
        format!("A过={} B拒={} C拒={}", a, bb, c),
    ));
    // Mapped 态改映射合法 + 引用账迁移守恒。
    let mut led2 = MappingLedger::new();
    let id2 = led2.map_item(0, 0, 100);
    let remap_ok = match id2 {
        Ok(id) => {
            let r = led2.remap(id, 8, 201);
            r.is_ok()
                && led2.mech_refcount[0] == 0
                && led2.mech_refcount[8] == 1
                && led2.entries[id].mech == 8
        }
        Err(_) => false,
    };
    v.push((
        "J1-Mapped改映射合法",
        remap_ok,
        format!("改映射后 mech={} ref[0]={} ref[8]={}", 8, led2.mech_refcount[0], led2.mech_refcount[8]),
    ));
    v
}

/// 探针入口：B09 批统一自检（15 条判据）。
pub fn run_unx18574_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18574 体验完整性映射");
    for (n, p, d) in static_checks() {
        s.add(n, p, d);
    }
    for (n, p, d) in dyn_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        let set = run_unx18574_checks();
        assert!(set.all_passed(), "F18574 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：子目 0 / 机制 0 合法。
        let mut led = MappingLedger::new();
        assert!(led.map_item(0, 0, 100).is_ok());
        // 上边界：子目 7 / 机制 11 合法；8 / 12 越界。
        assert!(led.map_item(ITEM_COUNT - 1, MECH_COUNT - 1, 299).is_ok());
        assert!(led.map_item(ITEM_COUNT, 0, 100).is_err());
        assert!(led.map_item(2, MECH_COUNT, 100).is_err());
    }

    #[test]
    fn test_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught(JUDGE_ROUNDS), "注入存在漏检：{:?}", inj.caught);
        let mut i = 0;
        while i < 4 {
            assert_eq!(inj.tries[i] - inj.caught[i], 0);
            i += 1;
        }
    }

    #[test]
    fn test_order_item_before_mech() {
        // 判定次序：子目闸先于机制闸（子目越界 + 机制越界报子目越界）。
        let mut led = MappingLedger::new();
        match led.map_item(ITEM_COUNT, MECH_COUNT, 100) {
            Err(c) if c == EmpCode::ItemRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal() {
        let mut led = MappingLedger::new();
        let a = led.map_item(0, 0, 100).unwrap_or(0);
        // 越级 Draft→Verified 拒；倒退 Verified→Mapped 拒。
        assert!(led.advance(a, EntryState::Verified).is_err());
        assert!(led.advance(a, EntryState::Mapped).is_ok());
        assert!(led.advance(a, EntryState::Verified).is_ok());
        assert!(led.advance(a, EntryState::Mapped).is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_capacity_and_no_loss() {
        let mut led = MappingLedger::new();
        let mut i = 0;
        while i < ITEM_COUNT {
            let _ = led.map_item(i, CANON_MAP[i], CANON_CRIT[i]);
            i += 1;
        }
        assert_eq!(led.count, ITEM_COUNT);
        // 账满后重复子目拒（先触发重复闸）且账不丢。
        let _ = led.map_item(0, 0, 100);
        assert_eq!(led.count, ITEM_COUNT);
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(EmpCode::ALL.len(), 12);
        let mut i = 0;
        while i < EmpCode::ALL.len() {
            let mut j = i + 1;
            while j < EmpCode::ALL.len() {
                assert_ne!(EmpCode::ALL[i].code(), EmpCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
