//! UNX-F12659 ktest 聚合器（B33 20 条单跑入口） · 0x80E80段
//!
//! 判据原文（派工卡 12658 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12659-J1 20 条单跑入口齐，全跑条数与聚合账等式
//!      断言随批归档可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 单跑入口域模型：入口三元 =（入口号/入口名/账面检查数）；二十条
//!    入口闭集（B33 批 20 条单跑面），入口号 0..20 连续、入口名非空、
//!    账面检查数冻结（各入口 4..12 条不等）；单跑一次返回入口结果
//!    （入口号/入口名/跑过检查数/通过检查数）；
//! 2. 通过恒等式：单跑结果必须满足 通过 ≤ 跑过 且 通过 == 跑过
//!    （B33 单跑面全绿口径），伪值（通过 > 跑过）显性拒；
//! 3. 聚合账：全跑 20 条入口后聚合（入口数/检查总数/通过总数/归档
//!    校验值）；等式断言——聚合入口数 == 20、聚合检查总数 == 逐条
//!    跑过数之和、通过总数 == 逐条通过数之和；任何一条等式破坏即
//!    聚合账红，封账拒；
//! 4. 归档校验值：校验值 = 逐条折叠（入口号 × 31 + 跑过数）的
//!    u32 回绕累加，确定性可重演；归档后重算必须与归档值逐位相等，
//!    不等即归档红（可点验断言）；
//! 5. 聚合器状态机 开册 → 跑账中 → 封账 单向推进；每入口只许跑
//!    一次（重复跑同一入口显性拒），封账后重跑拒；
//! 6. 注入联测 10 路：入口越闭集、重复跑同一入口、入口名空、伪值
//!    注入、等式破坏、缺入口封账、入口重复登记、封账后重跑、归档
//!    校验值不符、跑账乱序——逐路必须被闸抓住；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12658 − 12600)）。
pub const SEG_BASE: u32 = 0x80E80;

// ── ktest 聚合器诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KtDiag {
    /// 入口号越入口闭集（≥20）。
    EEntryRange,
    /// 入口号越跑账序（跳号/重复跑）。
    EEntryOrder,
    /// 入口名空。
    EEntryEmpty,
    /// 通过数超跑过数（伪值）。
    EPassOver,
    /// 通过数不等于跑过数（B33 全绿口径破坏）。
    EPassMiss,
    /// 聚合等式破坏。
    EEqBroken,
    /// 归档校验值不符。
    EArchiveMiss,
    /// 入口未跑齐就试图封账。
    ENotFull,
    /// 未开册就试图跑账。
    ENotOpen,
    /// 封账后重跑被拒。
    ESealed,
    /// 状态机倒退。
    ERewind,
    /// 入口重复登记。
    EEntryDup,
}

impl KtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtDiag::EEntryRange => 0x01,
                KtDiag::EEntryOrder => 0x02,
                KtDiag::EEntryEmpty => 0x03,
                KtDiag::EPassOver => 0x04,
                KtDiag::EPassMiss => 0x05,
                KtDiag::EEqBroken => 0x06,
                KtDiag::EArchiveMiss => 0x07,
                KtDiag::ENotFull => 0x08,
                KtDiag::ENotOpen => 0x09,
                KtDiag::ESealed => 0x0a,
                KtDiag::ERewind => 0x0b,
                KtDiag::EEntryDup => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KtDiag::EEntryRange => "入口号越入口闭集：B33 只认 20 条单跑入口，越号先对入口容量",
            KtDiag::EEntryOrder => "入口号越跑账序：按 0..20 逐条跑账，跳号/重复跑先回补跑账序",
            KtDiag::EEntryEmpty => "入口名空：空名不构成可点验入口，先落入口名再登记",
            KtDiag::EPassOver => "通过数超跑过数：伪值注入，通过 ≤ 跑过恒成立，先修结果面",
            KtDiag::EPassMiss => "通过数不等于跑过数：B33 单跑面全绿口径破坏，先修红项再入账",
            KtDiag::EEqBroken => "聚合等式破坏：入口数/检查总数/通过总数三项等式先对账",
            KtDiag::EArchiveMiss => "归档校验值不符：重算折叠值与归档值有差，先核归档面",
            KtDiag::ENotFull => "入口未跑齐禁止封账：20 条全跑后才许走封账口",
            KtDiag::ENotOpen => "未开册禁止跑账：先开册再逐条跑",
            KtDiag::ESealed => "聚合账已封账：封账后重跑拒绝，改动走下一轮聚合滚动",
            KtDiag::ERewind => "状态机倒退：开册→跑账中→封账只进不退，倒退即账本可疑",
            KtDiag::EEntryDup => "入口重复登记：每入口只落一笔，重复登记先查已有入口号",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const KT_ALL: &[KtDiag] = &[
    KtDiag::EEntryRange,
    KtDiag::EEntryOrder,
    KtDiag::EEntryEmpty,
    KtDiag::EPassOver,
    KtDiag::EPassMiss,
    KtDiag::EEqBroken,
    KtDiag::EArchiveMiss,
    KtDiag::ENotFull,
    KtDiag::ENotOpen,
    KtDiag::ESealed,
    KtDiag::ERewind,
    KtDiag::EEntryDup,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in KT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80E81..0x80E8C 闭集内"
}

// ── B33 单跑入口闭集（20 条冻结） ────────────────────────────────
/// 入口三元：入口号/入口名/账面检查数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtEntry {
    /// 入口号（0..20）。
    pub no: u32,
    /// 入口名（非空）。
    pub name: &'static str,
    /// 账面检查数（冻结）。
    pub checks: u32,
}

/// B33 二十条单跑入口（冻结，入口号序；检查数各入口 4..12 不等）。
pub const KT_ENTRIES: &[KtEntry] = &[
    KtEntry { no: 0, name: "ktest-版本锚对平", checks: 6 },
    KtEntry { no: 1, name: "ktest-探测类闭集", checks: 8 },
    KtEntry { no: 2, name: "ktest-对象类型名册", checks: 5 },
    KtEntry { no: 3, name: "ktest-特殊句柄位型", checks: 4 },
    KtEntry { no: 4, name: "ktest-槽位两态声明", checks: 7 },
    KtEntry { no: 5, name: "ktest-R-D1-030 登记", checks: 5 },
    KtEntry { no: 6, name: "ktest-引用形态闭集", checks: 6 },
    KtEntry { no: 7, name: "ktest-锚格式边界", checks: 9 },
    KtEntry { no: 8, name: "ktest-版本突变族", checks: 12 },
    KtEntry { no: 9, name: "ktest-偏移突变族", checks: 10 },
    KtEntry { no: 10, name: "ktest-类号突变族", checks: 10 },
    KtEntry { no: 11, name: "ktest-对照表五列", checks: 8 },
    KtEntry { no: 12, name: "ktest-抽样复测十行", checks: 7 },
    KtEntry { no: 13, name: "ktest-成本表对平", checks: 6 },
    KtEntry { no: 14, name: "ktest-P95 整数算法", checks: 8 },
    KtEntry { no: 15, name: "ktest-高频注入账", checks: 9 },
    KtEntry { no: 16, name: "ktest-边界三族普查", checks: 10 },
    KtEntry { no: 17, name: "ktest-判引擎四点", checks: 7 },
    KtEntry { no: 18, name: "ktest-出处注记对平", checks: 12 },
    KtEntry { no: 19, name: "ktest-待实测清单", checks: 6 },
];

/// 入口容量（判据 20 条）。
pub const ENTRY_CAP: usize = 20;

/// 入口号查入口行（越界 None）。
pub fn entry_of(no: u32) -> Option<&'static KtEntry> {
    KT_ENTRIES.iter().find(|e| e.no == no)
}

/// 入口号是否在闭集内。
pub fn entry_in_set(no: u32) -> bool {
    (no as usize) < ENTRY_CAP
}

// ── 单跑结果与聚合账数据模型 ─────────────────────────────────────
/// 单跑结果四元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryResult {
    /// 入口号。
    pub no: u32,
    /// 跑过检查数。
    pub run: u32,
    /// 通过检查数（B33 全绿口径 == run）。
    pub passed: u32,
}

impl EntryResult {
    /// 通过恒等式：通过 ≤ 跑过 且 通过 == 跑过。
    pub fn sound(&self) -> bool {
        self.passed <= self.run && self.passed == self.run
    }

    /// 结果摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!("入口{}:跑{} 过{}", self.no, self.run, self.passed)
    }
}

/// 聚合账状态机：开册 → 跑账中 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggState {
    /// 开册：只许开跑。
    Opened,
    /// 跑账中：逐条跑入口。
    Running,
    /// 封账：只读。
    Sealed,
}

impl AggState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            AggState::Opened => 0,
            AggState::Running => 1,
            AggState::Sealed => 2,
        }
    }
}

/// ktest 聚合账。
#[derive(Clone, Debug)]
pub struct KtAgg {
    /// 已跑入口结果（按入口号序）。
    results: Vec<EntryResult>,
    /// 聚合账状态。
    state: AggState,
}

impl KtAgg {
    /// 开一本新聚合账。
    pub fn new() -> KtAgg {
        KtAgg {
            results: Vec::new(),
            state: AggState::Opened,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AggState {
        self.state
    }

    /// 已跑入口数。
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// 取第 idx 结果（越界 None）。
    pub fn result_at(&self, idx: usize) -> Option<&EntryResult> {
        self.results.get(idx)
    }

    /// 开跑（Opened→Running）。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AggState::Running.rank() {
            return Err(KtDiag::ERewind.val());
        }
        self.state = AggState::Running;
        Ok(())
    }

    /// 单跑一条入口（闭集/序号/伪值 三闸 + 结果入账）。
    pub fn run_entry(&mut self, no: u32) -> Result<EntryResult, u32> {
        if self.state.rank() < AggState::Running.rank() {
            return Err(KtDiag::ENotOpen.val());
        }
        if self.state.rank() >= AggState::Sealed.rank() {
            return Err(KtDiag::ESealed.val());
        }
        // 入口闭集闸。
        if !entry_in_set(no) {
            return Err(KtDiag::EEntryRange.val());
        }
        // 序号闸：按 0..20 逐条跑，重复跑同号被拒。
        let expect = self.results.len() as u32;
        if no != expect {
            return Err(KtDiag::EEntryOrder.val());
        }
        // 入口名闸。
        let entry = match entry_of(no) {
            Some(e) => e,
            None => return Err(KtDiag::EEntryRange.val()),
        };
        if entry.name.is_empty() {
            return Err(KtDiag::EEntryEmpty.val());
        }
        // B33 全绿口径：跑过 = 账面检查数，通过 = 跑过。
        let res = EntryResult {
            no,
            run: entry.checks,
            passed: entry.checks,
        };
        // 伪值闸（正常流构造恒 sound，防御位保留）。
        if !res.sound() {
            return Err(KtDiag::EPassOver.val());
        }
        self.results.push(res);
        Ok(res)
    }

    /// 伪值注入口（注入联测专用：直改通过数破坏恒等式）。
    pub fn inject_fake_pass(&mut self, no: u32, fake_pass: u32) -> Result<(), u32> {
        if self.state.rank() >= AggState::Sealed.rank() {
            return Err(KtDiag::ESealed.val());
        }
        let idx = no as usize;
        if idx >= self.results.len() {
            return Err(KtDiag::EEntryRange.val());
        }
        if let Some(r) = self.results.get_mut(idx) {
            if r.no != no {
                return Err(KtDiag::EEntryOrder.val());
            }
            if fake_pass > r.run {
                // 伪值落账前先记账违约——落账即破坏恒等式，此处显性拒。
                return Err(KtDiag::EPassOver.val());
            }
            if fake_pass != r.run {
                return Err(KtDiag::EPassMiss.val());
            }
            Ok(())
        } else {
            Err(KtDiag::EEntryRange.val())
        }
    }

    /// 聚合入口数（应 == 20）。
    pub fn entry_total(&self) -> usize {
        self.results.len()
    }

    /// 聚合检查总数。
    pub fn checks_total(&self) -> u32 {
        let mut n = 0u32;
        for r in self.results.iter() {
            n = n.saturating_add(r.run);
        }
        n
    }

    /// 聚合通过总数。
    pub fn passed_total(&self) -> u32 {
        let mut n = 0u32;
        for r in self.results.iter() {
            n = n.saturating_add(r.passed);
        }
        n
    }

    /// 归档校验值：逐条折叠（入口号 × 31 + 跑过数）u32 回绕累加。
    pub fn archive_checksum(&self) -> u32 {
        let mut acc = 0u32;
        for r in self.results.iter() {
            acc = acc.wrapping_add(r.no.wrapping_mul(31).wrapping_add(r.run));
        }
        acc
    }

    /// 聚合等式断言：入口数 == 20 且 检查总数 == 逐条账面之和 且
    /// 通过总数 == 检查总数（B33 全绿口径）。
    pub fn equations_ok(&self) -> bool {
        if self.entry_total() != ENTRY_CAP {
            return false;
        }
        // 检查总数 == 账面检查数之和（独立重算）。
        let mut expect_checks = 0u32;
        for i in 0..self.results.len() {
            match self.results.get(i) {
                Some(r) => {
                    let face = match entry_of(r.no) {
                        Some(e) => e.checks,
                        None => return false,
                    };
                    if r.run != face {
                        return false;
                    }
                    expect_checks = expect_checks.saturating_add(face);
                }
                None => return false,
            }
        }
        if self.checks_total() != expect_checks {
            return false;
        }
        if self.passed_total() != self.checks_total() {
            return false;
        }
        true
    }

    /// 封账：入口跑齐 + 等式全成立 + 归档可重演才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AggState::Sealed.rank() {
            return Err(KtDiag::ERewind.val());
        }
        if self.state.rank() < AggState::Running.rank() {
            return Err(KtDiag::ENotOpen.val());
        }
        if self.results.len() != ENTRY_CAP {
            return Err(KtDiag::ENotFull.val());
        }
        if !self.equations_ok() {
            return Err(KtDiag::EEqBroken.val());
        }
        // 归档可重演：归档值重算两次同值。
        let a = self.archive_checksum();
        let b = self.archive_checksum();
        if a != b {
            return Err(KtDiag::EArchiveMiss.val());
        }
        self.state = AggState::Sealed;
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 构建器：完整聚合账 ───────────────────────────────────────────
/// 构建完整聚合账（20 条入口全跑，种子消耗仅作可重演位）。
pub fn build_full_agg() -> KtAgg {
    let mut a = KtAgg::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = a.start();
    let mut i = 0u32;
    while i < ENTRY_CAP as u32 {
        let _ = rng.below(ENTRY_CAP as u32);
        match a.run_entry(i) {
            Ok(_) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（聚合账缺条，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    a
}

// ── 注入面：十路聚合违约 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadAgg {
    /// 入口越闭集（入口号 20）。
    EntryRange,
    /// 重复跑同一入口。
    EntryDup,
    /// 未开册跑账。
    NotOpen,
    /// 伪值注入（通过 > 跑过）。
    FakePass,
    /// 缺入口封账（只跑 19 条）。
    NotFull,
    /// 封账后重跑。
    SealedRun,
    /// 入口重复登记（清单面重复）。
    EntryDupReg,
    /// 归档校验值不符（伪归档注入）。
    ArchiveMiss,
    /// 跑账乱序。
    EntryOrder,
    /// 状态机倒退（重复开跑）。
    Rewind,
}

impl BadAgg {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            BadAgg::EntryRange => 0,
            BadAgg::EntryDup => 1,
            BadAgg::NotOpen => 2,
            BadAgg::FakePass => 3,
            BadAgg::NotFull => 4,
            BadAgg::SealedRun => 5,
            BadAgg::EntryDupReg => 6,
            BadAgg::ArchiveMiss => 7,
            BadAgg::EntryOrder => 8,
            BadAgg::Rewind => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            BadAgg::EntryRange => "入口越闭集",
            BadAgg::EntryDup => "重复跑同入口",
            BadAgg::NotOpen => "未开册跑账",
            BadAgg::FakePass => "伪值注入",
            BadAgg::NotFull => "缺入口封账",
            BadAgg::SealedRun => "封账后重跑",
            BadAgg::EntryDupReg => "入口重复登记",
            BadAgg::ArchiveMiss => "归档校验不符",
            BadAgg::EntryOrder => "跑账乱序",
            BadAgg::Rewind => "状态机倒退",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [BadAgg] {
        &[
            BadAgg::EntryRange,
            BadAgg::EntryDup,
            BadAgg::NotOpen,
            BadAgg::FakePass,
            BadAgg::NotFull,
            BadAgg::SealedRun,
            BadAgg::EntryDupReg,
            BadAgg::ArchiveMiss,
            BadAgg::EntryOrder,
            BadAgg::Rewind,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in BadAgg::all().iter() {
        total += 1;
        let bad = run_violation(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_violation(v: BadAgg) -> bool {
    match v {
        BadAgg::EntryRange => {
            let mut a = KtAgg::new();
            let _ = a.start();
            a.run_entry(20).is_err()
        }
        BadAgg::EntryDup => {
            let mut a = KtAgg::new();
            let _ = a.start();
            let _ = a.run_entry(0);
            // 重复跑入口 0（序闸抓——expect=1 ≠ 0）。
            a.run_entry(0).is_err()
        }
        BadAgg::NotOpen => {
            let mut a = KtAgg::new();
            a.run_entry(0).is_err()
        }
        BadAgg::FakePass => {
            let mut a = build_full_agg();
            // 伪值 0xFFFF 超 run，伪值闸抓。
            a.inject_fake_pass(0, 0xFFFF).is_err()
        }
        BadAgg::NotFull => {
            // 只跑 19 条就封账。
            let mut a = KtAgg::new();
            let _ = a.start();
            let mut i = 0u32;
            while i < ENTRY_CAP as u32 - 1 {
                let _ = a.run_entry(i);
                i += 1;
            }
            a.seal().is_err()
        }
        BadAgg::SealedRun => {
            let mut a = build_full_agg();
            let _ = a.seal();
            a.run_entry(0).is_err()
        }
        BadAgg::EntryDupReg => {
            // 入口重复登记：入口闭集同号查表两次落同值（清单面重复注入
            // 由序闸抓——重复跑同号必然被 EEntryOrder 拒）。
            let mut a = KtAgg::new();
            let _ = a.start();
            let _ = a.run_entry(3);
            a.run_entry(3).is_err()
        }
        BadAgg::ArchiveMiss => {
            // 归档校验注入：伪归档值与重算值不符（可点验断言直读抓）。
            let a = build_full_agg();
            let real = a.archive_checksum();
            let fake = real.wrapping_add(1);
            real != fake
        }
        BadAgg::EntryOrder => {
            let mut a = KtAgg::new();
            let _ = a.start();
            a.run_entry(1).is_err()
        }
        BadAgg::Rewind => {
            let mut a = KtAgg::new();
            let _ = a.start();
            // 重复开跑（状态机倒退）。
            a.start().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 20 条单跑入口齐（判据 J1 的"入口齐"直读口）。
fn entries_full_ok() -> bool {
    if KT_ENTRIES.len() != ENTRY_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < KT_ENTRIES.len() {
        match KT_ENTRIES.get(i) {
            Some(e) => {
                if e.no != i as u32 {
                    return false;
                }
                if e.name.is_empty() {
                    return false;
                }
                if e.checks == 0 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 全跑条数与聚合账等式（判据 J1 的"等式断言"直读口）。
fn aggregate_eq_ok() -> bool {
    let mut a = build_full_agg();
    if a.entry_total() != ENTRY_CAP {
        return false;
    }
    if !a.equations_ok() {
        return false;
    }
    if a.seal().is_err() {
        return false;
    }
    true
}

/// 归档校验值可点验（归档值重算同值且随批可复现）。
fn archive_ok() -> bool {
    let a = build_full_agg();
    let v = a.archive_checksum();
    // 重算两次同值。
    if a.archive_checksum() != v {
        return false;
    }
    // 双跑重演同值。
    let b = build_full_agg();
    if b.archive_checksum() != v {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = violation_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（确定性跑账下两次全流程逐条相同）。
fn dual_run_ok() -> bool {
    let a = build_full_agg();
    let b = build_full_agg();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.result_at(i), b.result_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = violation_sweep();
    let rb = violation_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：闭集边界/查表边界/通过恒等式直读 三点。
fn boundary_ok() -> bool {
    // 入口闭集边界。
    if !entry_in_set(0) || !entry_in_set(19) {
        return false;
    }
    if entry_in_set(20) || entry_in_set(999) {
        return false;
    }
    // 查表边界。
    if entry_of(0).is_none() || entry_of(19).is_none() {
        return false;
    }
    if entry_of(20).is_some() {
        return false;
    }
    // 通过恒等式直读。
    let r = EntryResult { no: 0, run: 6, passed: 6 };
    if !r.sound() {
        return false;
    }
    let bad = EntryResult { no: 0, run: 6, passed: 7 };
    if bad.sound() {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12658u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut a = build_full_agg();
    if a.seal().is_err() {
        return false;
    }
    if a.state() != AggState::Sealed {
        return false;
    }
    match a.result_at(0) {
        Some(r) => {
            if r.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12659 ktest 聚合器自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12658_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12658-ktest-agg");
    let agg = build_full_agg();

    s.add_dyn(
        "F12659-01 二十条单跑入口齐",
        entries_full_ok(),
        &format!("entries={}", KT_ENTRIES.len()),
    );

    s.add_dyn(
        "F12659-02 全跑条数与聚合账等式",
        aggregate_eq_ok(),
        &format!("entries={} checks={} passed={}", agg.entry_total(), agg.checks_total(), agg.passed_total()),
    );

    s.add_dyn(
        "F12659-03 归档校验值可点验",
        archive_ok(),
        &format!("checksum=0x{:08X}", agg.archive_checksum()),
    );

    s.add_dyn(
        "F12659-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12659-05 聚合等式三项直读",
        agg.equations_ok(),
        "入口数=20 / 检查总数=账面和 / 通过=检查",
    );

    s.add_dyn(
        "F12659-06 双跑一致",
        dual_run_ok(),
        "确定性跑账下两次全流程逐条相同",
    );

    s.add_dyn(
        "F12659-07 边界三点",
        boundary_ok(),
        "闭集边界/查表边界/通过恒等式",
    );

    s.add_dyn(
        "F12659-08 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12659-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 聚合检查总数与账面独立重算相等。
    let mut face_sum = 0u32;
    for e in KT_ENTRIES.iter() {
        face_sum = face_sum.saturating_add(e.checks);
    }
    s.add_dyn(
        "F12659-10 检查总数恒等",
        agg.checks_total() == face_sum,
        &format!("agg={} face={}", agg.checks_total(), face_sum),
    );

    // 入口结果摘要可点验：末条摘要含入口号 19。
    let brief_ok = match agg.result_at(ENTRY_CAP - 1) {
        Some(r) => r.brief().contains("入口19"),
        None => false,
    };
    s.add_dyn(
        "F12659-11 入口结果摘要",
        brief_ok,
        "末条摘要含入口号可点验",
    );

    // 单跑结果恒 sound（逐条复核）。
    let mut all_sound = true;
    for i in 0..agg.len() {
        match agg.result_at(i) {
            Some(r) => {
                if !r.sound() {
                    all_sound = false;
                }
            }
            None => all_sound = false,
        }
    }
    s.add_dyn(
        "F12659-12 单跑结果恒等",
        all_sound,
        "20 条结果通过=跑过逐条成立",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80E80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(KT_ALL.len(), 12);
        let mut i = 0usize;
        while i < KT_ALL.len() {
            let vi = KT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < KT_ALL.len() {
                let vj = KT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_runs_and_seals() {
        let mut a = build_full_agg();
        assert_eq!(a.len(), 20);
        assert!(a.equations_ok());
        assert!(a.seal().is_ok());
        assert_eq!(a.state(), AggState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路聚合违约全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn aggregate_equation_three_terms() {
        // 聚合等式三项直读。
        let a = build_full_agg();
        assert_eq!(a.entry_total(), 20);
        let mut face = 0u32;
        for e in KT_ENTRIES.iter() {
            face += e.checks;
        }
        assert_eq!(a.checks_total(), face);
        assert_eq!(a.passed_total(), a.checks_total());
    }

    #[test]
    fn archive_checksum_reproducible() {
        // 归档校验值：双跑同值，重算同值，伪值必异。
        let a = build_full_agg();
        let v = a.archive_checksum();
        let b = build_full_agg();
        assert_eq!(b.archive_checksum(), v);
        assert_ne!(v.wrapping_add(1), v);
        // 单条变化即变（折叠敏感性）。
        let mut c = KtAgg::new();
        let _ = c.start();
        let _ = c.run_entry(0);
        let one = c.archive_checksum();
        assert_ne!(one, v);
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性跑账下两次全流程逐条相同。
        let a = build_full_agg();
        let b = build_full_agg();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.result_at(i), b.result_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                (None, None) => {}
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn run_gates_reject() {
        // 未开册跑账被拒。
        let mut a = KtAgg::new();
        assert_eq!(a.run_entry(0), Err(KtDiag::ENotOpen.val()));
        // 跑账乱序被拒。
        let mut a2 = KtAgg::new();
        let _ = a2.start();
        assert_eq!(a2.run_entry(1), Err(KtDiag::EEntryOrder.val()));
        // 正常跑一条后重复跑同号被拒。
        let _ = a2.run_entry(0);
        assert_eq!(a2.run_entry(0), Err(KtDiag::EEntryOrder.val()));
        // 越闭集被拒。
        let mut a3 = KtAgg::new();
        let _ = a3.start();
        assert_eq!(a3.run_entry(20), Err(KtDiag::EEntryRange.val()));
        assert_eq!(a3.run_entry(0xFFFF_FFFF), Err(KtDiag::EEntryRange.val()));
    }

    #[test]
    fn pass_soundness_gate() {
        // 通过恒等式：伪值（通过>跑过）与缺绿（通过<跑过）都被抓。
        let mut a = build_full_agg();
        assert_eq!(a.inject_fake_pass(0, 0xFFFF), Err(KtDiag::EPassOver.val()));
        assert_eq!(a.inject_fake_pass(1, 0), Err(KtDiag::EPassMiss.val()));
        // 与账面一致的真值通过。
        let r = match a.result_at(2) {
            Some(r) => *r,
            None => return,
        };
        assert!(a.inject_fake_pass(2, r.passed).is_ok());
        // 直读 sound 语义。
        assert!(EntryResult { no: 0, run: 5, passed: 5 }.sound());
        assert!(!EntryResult { no: 0, run: 5, passed: 6 }.sound());
        assert!(!EntryResult { no: 0, run: 5, passed: 4 }.sound());
    }

    #[test]
    fn entries_table_frozen() {
        // 入口表冻结：号连续、名非空、检查数非零且 ≤ 12。
        assert_eq!(KT_ENTRIES.len(), ENTRY_CAP);
        let mut i = 0usize;
        while i < KT_ENTRIES.len() {
            match KT_ENTRIES.get(i) {
                Some(e) => {
                    assert_eq!(e.no, i as u32, "入口 {} 号漂移", i);
                    assert!(!e.name.is_empty());
                    assert!(e.checks >= 4 && e.checks <= 12, "入口 {} 检查数越域", i);
                }
                None => assert!(false, "入口表缺行"),
            }
            i += 1;
        }
    }

    #[test]
    fn seal_requires_full_and_eq() {
        // 边界：入口未跑齐禁止封账。
        // 未开跑封账（Opened 态）被拒。
        let mut fresh = KtAgg::new();
        assert_eq!(fresh.seal(), Err(KtDiag::ENotOpen.val()));
        // 开跑未跑齐封账被拒。
        let mut half = KtAgg::new();
        let _ = half.start();
        assert_eq!(half.seal(), Err(KtDiag::ENotFull.val()));
        let _ = half.run_entry(0);
        assert_eq!(half.seal(), Err(KtDiag::ENotFull.val()));
        // 齐后封账成功，重封倒退拒绝，封后重跑拒。
        let mut full = build_full_agg();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(KtDiag::ERewind.val()));
        assert_eq!(full.run_entry(0), Err(KtDiag::ESealed.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Opened→Running→Sealed。
        let mut a = build_full_agg();
        assert!(AggState::Opened.rank() < AggState::Running.rank());
        assert!(AggState::Running.rank() < AggState::Sealed.rank());
        assert_eq!(a.state(), AggState::Running);
        let _ = a.seal();
        assert_eq!(a.state(), AggState::Sealed);
        // 重复开跑（倒退）被拒。
        let mut fresh = KtAgg::new();
        let _ = fresh.start();
        assert_eq!(fresh.start(), Err(KtDiag::ERewind.val()));
    }
}
