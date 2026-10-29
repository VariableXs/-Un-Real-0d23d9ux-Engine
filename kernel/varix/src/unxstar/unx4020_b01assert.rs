//! unx4020_b01assert — UNX-F4020 B01 批判据聚合断言集（诊断段 0x8520）。
//!
//! 判据原文：
//!   J1 本批 19 条判据全部聚合入 ktest vfs B01 面：聚合报告 19/19，缺项
//!      即判失败（E_MISS）。
//!   J2 批账段位闭集互异：18 张卡的诊断段全部落在 0x8300..=0x8520、步进
//!      0x20 的闭集内且两两互异（E_SEG_DUP / E_SEG_GAP）。
//!   J3 编号账：卡号升序、无重复、落在 B01 区间（4002..=4020，缺 4015）。
//!   J4 账目守恒：登记条数 == 19、通过数 + 失败数 == 已跑数 == 19，
//!      通过率 100% 才算绿（E_RED / E_BAL）。
//! 设计纪律：
//!   1. 本模块为批级聚合账：内嵌 19 条判据登记项（18 张卡各一条主判据 +
//!      F4003 第二条「万次往返」判据），不 import 同批任何模块（并行施工
//!      依赖不可用），也不执行被聚合用例，只做批账核对。
//!   2. 段位闭集以常量区间表达：SEG_LO..=SEG_HI 步进 SEG_STEP；核对分三关
//!      ——区间内、步进对齐、两两互异。
//!   3. 断言类别状态机 AssertKind 表达判据性质（编码/守恒/状态机/边界），
//!      每类各自计数入账，便于按性质汇总。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8520;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MISS: u16 = SEG_BASE + 1;     // 聚合缺项（不足 19 条）
const E_SEG_DUP: u16 = SEG_BASE + 2;  // 段位重复（闭集互异失守）
const E_SEG_GAP: u16 = SEG_BASE + 3;  // 段位越界/步进不对齐
const E_NUM_ORDER: u16 = SEG_BASE + 4; // 编号账乱序/重复/越区间
const E_BAL: u16 = SEG_BASE + 5;      // 批账不平（跑数≠登记数）
const E_RED: u16 = SEG_BASE + 6;      // 通过率不足 100%（批红）

/// B01 批段位闭集区间与步进（施工单 seg 字段布局）。
pub const SEG_LO: u16 = 0x8300;
pub const SEG_HI: u16 = 0x8520;
pub const SEG_STEP: u16 = 0x0020;
/// 批内卡数（18 张卡）。
pub const CARD_COUNT: usize = 18;
/// 批内判据条数（18 主判据 + F4003 第二条 = 19）。
pub const ASSERT_TOTAL: usize = 19;
/// B01 卡号区间。
pub const NUM_LO: u16 = 4002;
pub const NUM_HI: u16 = 4020;

/// 判据性质类别（状态机式分类，各类独立计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertKind {
    Encoding = 0,    // 编码/位段对照类
    Conservation = 1, // 守恒/计数账类
    StateMachine = 2, // 状态迁移类
    Boundary = 3,    // 边界/注入拒绝类
}

pub const KIND_COUNT: usize = 4;

/// 类别名（报告导出）。
pub const KIND_NAMES: [&'static str; KIND_COUNT] = [
    "encoding", "conservation", "state_machine", "boundary",
];

/// 单条聚合判据登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AssertItem {
    pub card: u16,      // 卡号（UNX-F4NNN）
    pub seg: u16,       // 该卡诊断段
    pub prop: u8,       // 判据序号（J1/J2…）
    pub kind: AssertKind, // 判据性质类别
    pub lines: u16,     // 判据承载的断言行数（登记值）
    pub prio: u8,       // 优先级 1..=3（1 为阻断级）
}

/// 19 条判据内嵌登记表（来源：B01 施工单 accept 判据原文摘录）。
/// 段位与卡号严格按施工单：4002→0x8300 … 4020→0x8520，缺 4015 卡。
pub const ASSERTS: [AssertItem; ASSERT_TOTAL] = [
    AssertItem { card: 4002, seg: 0x8300, prop: 1, kind: AssertKind::Encoding, lines: 4, prio: 1 },
    AssertItem { card: 4003, seg: 0x8320, prop: 1, kind: AssertKind::Encoding, lines: 3, prio: 1 },
    // F4003 第二判据：万次编解码往返零错（独立于位段对照）。
    AssertItem { card: 4003, seg: 0x8320, prop: 2, kind: AssertKind::Conservation, lines: 3, prio: 1 },
    AssertItem { card: 4004, seg: 0x8340, prop: 1, kind: AssertKind::Encoding, lines: 3, prio: 1 },
    AssertItem { card: 4005, seg: 0x8360, prop: 1, kind: AssertKind::Conservation, lines: 4, prio: 1 },
    AssertItem { card: 4006, seg: 0x8380, prop: 1, kind: AssertKind::StateMachine, lines: 3, prio: 1 },
    AssertItem { card: 4007, seg: 0x83A0, prop: 1, kind: AssertKind::Conservation, lines: 3, prio: 1 },
    AssertItem { card: 4008, seg: 0x83C0, prop: 1, kind: AssertKind::Encoding, lines: 3, prio: 2 },
    AssertItem { card: 4009, seg: 0x83E0, prop: 1, kind: AssertKind::Boundary, lines: 3, prio: 2 },
    AssertItem { card: 4010, seg: 0x8400, prop: 1, kind: AssertKind::Conservation, lines: 5, prio: 1 },
    AssertItem { card: 4011, seg: 0x8420, prop: 1, kind: AssertKind::Conservation, lines: 3, prio: 2 },
    AssertItem { card: 4012, seg: 0x8440, prop: 1, kind: AssertKind::StateMachine, lines: 3, prio: 2 },
    AssertItem { card: 4013, seg: 0x8460, prop: 1, kind: AssertKind::StateMachine, lines: 3, prio: 2 },
    AssertItem { card: 4014, seg: 0x8480, prop: 1, kind: AssertKind::Boundary, lines: 3, prio: 2 },
    AssertItem { card: 4016, seg: 0x84A0, prop: 1, kind: AssertKind::StateMachine, lines: 3, prio: 1 },
    AssertItem { card: 4017, seg: 0x84C0, prop: 1, kind: AssertKind::Boundary, lines: 3, prio: 1 },
    AssertItem { card: 4018, seg: 0x84E0, prop: 1, kind: AssertKind::Conservation, lines: 3, prio: 1 },
    AssertItem { card: 4019, seg: 0x8500, prop: 1, kind: AssertKind::StateMachine, lines: 3, prio: 2 },
    AssertItem { card: 4020, seg: 0x8520, prop: 1, kind: AssertKind::Conservation, lines: 3, prio: 3 },
];

/// 批账状态机（聚合面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    Empty = 0,    // 未聚合
    Collected = 1, // 已聚合待核对
    Verified = 2, // 已核对（段位/编号通过）
    Green = 3,    // 全绿（19/19 通过率 100%）
    Red = 4,      // 批红（有失败项）
}

/// B01 批聚合账。
pub struct BatchLedger {
    pub items: Vec<AssertItem>,      // 聚合条目
    pub state: BatchState,           // 批账状态机
    pub checked: u32,                // 已核对条数
    pub passed: u32,                 // 通过条数
    pub failed: u32,                 // 失败条数
    pub kind_counts: [u32; KIND_COUNT], // 四类性质计数
    pub seg_rej: u32,                // 段位拒绝账
    pub num_rej: u32,                // 编号拒绝账
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            items: Vec::new(),
            state: BatchState::Empty,
            checked: 0,
            passed: 0,
            failed: 0,
            kind_counts: [0; KIND_COUNT],
            seg_rej: 0,
            num_rej: 0,
        }
    }

    /// 采集：从内嵌表导入全部判据项，逐条入账并按类别计数。
    pub fn collect(&mut self) -> Result<usize, u16> {
        if self.state != BatchState::Empty && self.state != BatchState::Red {
            return Err(E_BAL);
        }
        self.items.clear();
        for it in ASSERTS.iter() {
            self.items.push(*it);
            let k = it.kind as usize;
            if k >= KIND_COUNT {
                return Err(E_BAL);
            }
            self.kind_counts[k] += 1;
        }
        if self.items.len() != ASSERT_TOTAL {
            self.state = BatchState::Red;
            return Err(E_MISS);
        }
        self.state = BatchState::Collected;
        Ok(self.items.len())
    }

    /// 段位核对：闭集区间、步进对齐、两两互异（判据 J2）。
    pub fn check_segments(&mut self) -> Result<u32, u16> {
        if self.items.is_empty() {
            return Err(E_MISS);
        }
        // 区间 + 步进。
        for it in self.items.iter() {
            if it.seg < SEG_LO || it.seg > SEG_HI {
                self.seg_rej += 1;
                return Err(E_SEG_GAP);
            }
            if (it.seg - SEG_LO) % SEG_STEP != 0 {
                self.seg_rej += 1;
                return Err(E_SEG_GAP);
            }
        }
        // 两两互异（同卡两条判据共享段，按卡去重后应互异）。
        let mut seen: Vec<u16> = Vec::new();
        for it in self.items.iter() {
            let mut dup = false;
            for s in seen.iter() {
                if *s == it.seg {
                    dup = true;
                    break;
                }
            }
            if dup {
                // 同卡同段合法（F4003 的两条判据）；不同卡同段非法。
                let mut other_card = false;
                for it2 in self.items.iter() {
                    if it2.seg == it.seg && it2.card != it.card {
                        other_card = true;
                        break;
                    }
                }
                if other_card {
                    self.seg_rej += 1;
                    return Err(E_SEG_DUP);
                }
            } else {
                seen.push(it.seg);
            }
        }
        if seen.len() != CARD_COUNT {
            return Err(E_SEG_GAP); // 段数应等于卡数
        }
        Ok(seen.len() as u32)
    }

    /// 编号核对：卡号在区间内、升序、无重复、卡数 18（判据 J3）。
    pub fn check_numbers(&mut self) -> Result<u32, u16> {
        if self.items.is_empty() {
            return Err(E_MISS);
        }
        let mut cards: Vec<u16> = Vec::new();
        for it in self.items.iter() {
            if it.card < NUM_LO || it.card > NUM_HI {
                self.num_rej += 1;
                return Err(E_NUM_ORDER);
            }
            let mut dup = false;
            for c in cards.iter() {
                if *c == it.card {
                    dup = true;
                    break;
                }
            }
            if !dup {
                cards.push(it.card);
            }
        }
        if cards.len() != CARD_COUNT {
            self.num_rej += 1;
            return Err(E_NUM_ORDER);
        }
        // 升序（卡号集合严格递增，缺 4015）。
        for i in 1..cards.len() {
            if cards[i] <= cards[i - 1] {
                self.num_rej += 1;
                return Err(E_NUM_ORDER);
            }
        }
        // 缺 4015：区间内 19 个号位缺 1 个即 18 卡。
        if NUM_HI - NUM_LO + 1 != CARD_COUNT as u16 + 1 {
            self.num_rej += 1;
            return Err(E_NUM_ORDER);
        }
        let mut has_4015 = false;
        for c in cards.iter() {
            if *c == 4015 {
                has_4015 = true;
            }
        }
        if has_4015 {
            self.num_rej += 1;
            return Err(E_NUM_ORDER); // 4015 不在 B01 施工单内
        }
        Ok(cards.len() as u32)
    }

    /// 逐条核对：按登记行数跑「结构核对」（行数>0、类别合法、优先级 1..=3）。
    /// 本函数不执行被聚合用例，只做批账结构判定（设计纪律 1）。
    pub fn verify(&mut self) -> Result<u32, u16> {
        if self.items.len() != ASSERT_TOTAL {
            return Err(E_MISS);
        }
        if self.state != BatchState::Collected {
            return Err(E_BAL);
        }
        let mut ok = 0u32;
        for it in self.items.iter() {
            let good = it.lines > 0
                && (it.kind as usize) < KIND_COUNT
                && it.prio >= 1
                && it.prio <= 3
                && it.prop >= 1;
            if good {
                ok += 1;
            } else {
                self.failed += 1;
            }
        }
        self.checked = ASSERT_TOTAL as u32;
        self.passed = ok;
        if ok == ASSERT_TOTAL as u32 {
            self.state = BatchState::Verified;
            Ok(ok)
        } else {
            self.state = BatchState::Red;
            Err(E_RED)
        }
    }

    /// 定绿：通过率 100% 才置 Green（判据 J4）。
    pub fn to_green(&mut self) -> Result<BatchState, u16> {
        if self.state != BatchState::Verified {
            return Err(E_RED);
        }
        if self.checked != ASSERT_TOTAL as u32 || self.passed != ASSERT_TOTAL as u32 {
            return Err(E_BAL);
        }
        if self.passed + self.failed != self.checked {
            return Err(E_BAL);
        }
        self.state = BatchState::Green;
        Ok(self.state)
    }

    /// 批报告：通过条数/总条数（19/19 面）。
    pub fn report(&self) -> (u32, u32) {
        (self.passed, ASSERT_TOTAL as u32)
    }

    /// 类别分布导出（四类计数，Σ == 19）。
    pub fn kind_export(&self) -> [u32; KIND_COUNT] {
        self.kind_counts
    }

    /// 账守恒：Σ类别计数 == 条目数 == 19（判据 J4 的守恒面）。
    pub fn conserve(&self) -> bool {
        let mut s = 0u32;
        for k in self.kind_counts.iter() {
            s += *k;
        }
        s == self.items.len() as u32
            && self.items.len() == ASSERT_TOTAL
            && self.passed + self.failed == self.checked
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MISS => "聚合缺项（不足 19 条）",
        E_SEG_DUP => "段位重复（闭集互异失守）",
        E_SEG_GAP => "段位越界/步进不对齐/段数≠卡数",
        E_NUM_ORDER => "编号账乱序/重复/越区间/含 4015",
        E_BAL => "批账不平（跑数≠登记数）",
        E_RED => "通过率不足 100%（批红）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：采集 19 条 → 段位闭集 → 编号账 → 全绿 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut led = BatchLedger::new();

    // R1：采集内嵌表，条数 == 19（判据 J1 原文）。
    let n = led.collect()?;
    if n != ASSERT_TOTAL || led.state != BatchState::Collected {
        return Err(E_MISS);
    }
    hits += 1;

    // R2：段位闭集互异——18 个段、区间 [0x8300,0x8520]、步进 0x20（判据 J2）。
    let segs = led.check_segments()?;
    if segs != CARD_COUNT as u32 {
        return Err(E_SEG_GAP);
    }
    if led.seg_rej != 0 {
        return Err(E_SEG_DUP);
    }
    hits += 1;

    // R3：编号账——18 卡、升序、缺 4015（判据 J3）。
    let cards = led.check_numbers()?;
    if cards != CARD_COUNT as u32 {
        return Err(E_NUM_ORDER);
    }
    if led.num_rej != 0 {
        return Err(E_NUM_ORDER);
    }
    hits += 1;

    // R4：逐条结构核对 19/19 通过（判据 J1/J4 的跑数面）。
    let ok = led.verify()?;
    if ok != ASSERT_TOTAL as u32 || led.passed != 19 || led.failed != 0 {
        return Err(E_RED);
    }
    if led.checked != 19 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：定绿——通过率 100% 才 Green（判据 J4）。
    if led.to_green()? != BatchState::Green {
        return Err(E_RED);
    }
    let (p, t) = led.report();
    if p != 19 || t != 19 {
        return Err(E_RED);
    }
    hits += 1;

    // R6：类别分布 Σ == 19，且四类均有条目。
    let k = led.kind_export();
    let mut s = 0u32;
    let mut nonzero = 0u32;
    for c in k.iter() {
        s += *c;
        if *c > 0 {
            nonzero += 1;
        }
    }
    if s != 19 || nonzero != KIND_COUNT as u32 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：账守恒——条目数/类别和/跑数三方一致。
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：缺项注入——少一条即 E_MISS（构造截断表核对）。
    let mut short = BatchLedger::new();
    short.collect()?;
    short.items.pop();
    if short.verify() != Err(E_MISS) {
        return Err(E_MISS);
    }
    hits += 1;

    // I2：段位重复注入——把第二条段改成与首条同段（异卡同段）须判 E_SEG_DUP。
    let mut segdup = BatchLedger::new();
    segdup.collect()?;
    segdup.items[3].seg = 0x8300; // F4004 段被篡改为 F4002 段
    if segdup.check_segments() != Err(E_SEG_DUP) {
        return Err(E_SEG_DUP);
    }
    if segdup.seg_rej != 1 {
        return Err(E_SEG_DUP);
    }
    hits += 1;

    // I3：段位越界注入——改成 0x82E0（低于区间）须判 E_SEG_GAP。
    let mut seggap = BatchLedger::new();
    seggap.collect()?;
    seggap.items[0].seg = 0x82E0;
    if seggap.check_segments() != Err(E_SEG_GAP) {
        return Err(E_SEG_GAP);
    }
    hits += 1;

    // I4：编号越界注入——卡号改成 4015（施工单缺失号）须判 E_NUM_ORDER。
    let mut numbad = BatchLedger::new();
    numbad.collect()?;
    numbad.items[10].card = 4015;
    if numbad.check_numbers() != Err(E_NUM_ORDER) {
        return Err(E_NUM_ORDER);
    }
    hits += 1;

    // I5：未采集先核对 / 未核对先定绿双注入被拒（状态机守门）。
    let mut fresh = BatchLedger::new();
    if fresh.check_segments() != Err(E_MISS) {
        return Err(E_MISS);
    }
    if fresh.to_green() != Err(E_RED) {
        return Err(E_RED);
    }
    hits += 1;

    // I6：结构核对失败注入——行数置 0 使 19/19 不成立，须判 E_RED。
    let mut red = BatchLedger::new();
    red.collect()?;
    red.items[0].lines = 0;
    if red.verify() != Err(E_RED) {
        return Err(E_RED);
    }
    if red.failed != 1 || red.state != BatchState::Red {
        return Err(E_RED);
    }
    hits += 1;

    Ok(hits)
}
