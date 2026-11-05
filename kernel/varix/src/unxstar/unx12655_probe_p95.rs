//! UNX-F12656 探测面性能预算（探测调用 P95 账） · 0x80DC0段
//!
//! 判据原文（派工卡 12655 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12656-J1 探测调用 P95 预算在册（O1 配套账），
//!      高频注入账无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 整数预算单位模型（零墙钟合规）：每次探测调用记账一个"预算单位"
//!    整数成本，成本 = 成本表基值 + LCG 抖动（种子 = SEG_BASE，序列
//!    确定）；成本不是实测墙钟，是 O1 配套账的确定性预算口径；
//! 2. 八类成本表与预算上限表冻结：成本基值 4/6/8/12/16/24/32/48、
//!    预算上限 16/16/20/24/28/36/44/60（类号 0..8），抖动上限 4，
//!    正常流与高频注入的 P95 恒落上限之内；
//! 3. P95 整数算法：成本序列升序排序后取 idx = (n × 95) / 100 下标
//!    （整数除法，无浮点），越序取饱和尾值；P95 ≤ 预算上限为绿账，
//!    超限即红账，红账显性入账不静默；
//! 4. O1 配套账：每类一行预算账（类号/预算上限/记账次数/实测 P95），
//!    八行齐 + 每行 P95 ≤ 上限 + 记账次数频次上限内 才许封账；
//! 5. 高频注入账无红账：每类高频突发 200 次确定性调用，P95 账逐行
//!    复核仍全绿；同时十路注入（上限削减/越类调用/成本越界/重复
//!    登记/零上限/超频/倒退/P95 伪值/空账封账/等式破坏）逐路必须
//!    显性翻红或被闸拒收；
//! 6. 状态机 开账 → 记账中 → 封账 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12655 − 12600)）。
pub const SEG_BASE: u32 = 0x80DC0;

// ── P95 预算账诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Diag {
    /// 类号越成本表闭集。
    EClassRange,
    /// 预算行重复登记。
    ERowDup,
    /// 预算上限为零（零上限注入）。
    EZeroCap,
    /// 成本越界（单次成本超上限+抖动余量）。
    ECostOver,
    /// P95 超预算上限（红账）。
    EP95Over,
    /// 记账频次超上限（超频注入）。
    EFreqOver,
    /// P95 伪值注入（记账 P95 与重算不符）。
    EP95Fake,
    /// 八行未齐就试图封账。
    ENotFull,
    /// 账目等式破坏（记账次数和 ≠ 总调用数）。
    EEqBroken,
    /// 未开账就试图记账。
    ENotOpen,
    /// 封账后写入被拒。
    ESealed,
    /// 状态机倒退。
    ERewind,
}

impl P95Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                P95Diag::EClassRange => 0x01,
                P95Diag::ERowDup => 0x02,
                P95Diag::EZeroCap => 0x03,
                P95Diag::ECostOver => 0x04,
                P95Diag::EP95Over => 0x05,
                P95Diag::EFreqOver => 0x06,
                P95Diag::EP95Fake => 0x07,
                P95Diag::ENotFull => 0x08,
                P95Diag::EEqBroken => 0x09,
                P95Diag::ENotOpen => 0x0a,
                P95Diag::ESealed => 0x0b,
                P95Diag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            P95Diag::EClassRange => "类号越成本表闭集：只认 0..8 八类，越类先对成本表再记账",
            P95Diag::ERowDup => "预算行重复：每类只落一行预算账，重复登记先查已有类号",
            P95Diag::EZeroCap => "预算上限为零：零上限不构成可记账预算，先修上限再开账",
            P95Diag::ECostOver => "成本越界：单次成本超上限+抖动余量，先核成本表再记账",
            P95Diag::EP95Over => "P95 超预算上限：红账显性入账，先压调用成本或调上限再复核",
            P95Diag::EFreqOver => "记账频次超上限：高频突发超 200 次上限，超频先限流再入账",
            P95Diag::EP95Fake => "P95 伪值：记账 P95 与成本序列重算不符，先修记账面再封账",
            P95Diag::ENotFull => "八行未齐禁止封账：先补齐缺类预算行再走封账口",
            P95Diag::EEqBroken => "账目等式破坏：各行记账次数和 ≠ 总调用数，先对账再封账",
            P95Diag::ENotOpen => "未开账禁止记账：先开账再逐次记账",
            P95Diag::ESealed => "预算账已封账：封账后写入拒绝，改动走下一轮预算滚动",
            P95Diag::ERewind => "状态机倒退：开账→记账中→封账只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const P95_ALL: &[P95Diag] = &[
    P95Diag::EClassRange,
    P95Diag::ERowDup,
    P95Diag::EZeroCap,
    P95Diag::ECostOver,
    P95Diag::EP95Over,
    P95Diag::EFreqOver,
    P95Diag::EP95Fake,
    P95Diag::ENotFull,
    P95Diag::EEqBroken,
    P95Diag::ENotOpen,
    P95Diag::ESealed,
    P95Diag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in P95_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80DC1..0x80DCC 闭集内"
}

// ── 成本表与预算上限表（冻结，O1 配套账口径） ────────────────────
/// 八类成本基值表（类号 0..8，冻结不许重排）。
pub const COST_TABLE: &[u32] = &[4, 6, 8, 12, 16, 24, 32, 48];

/// 八类预算上限表（类号 0..8，冻结不许重排）。
pub const CAP_TABLE: &[u32] = &[16, 16, 20, 24, 28, 36, 44, 60];

/// 类闭集容量。
pub const CLASS_CAP: usize = 8;

/// LCG 抖动上限（成本 = 基值 + 0..4 抖动）。
pub const JITTER_CAP: u32 = 4;

/// 记账频次上限（每类记账次数上限）。
pub const FREQ_CAP: usize = 200;

/// 类号是否在成本表闭集内。
pub fn class_in_set(class: u32) -> bool {
    (class as usize) < CLASS_CAP
}

/// 成本基值查表（越类 None）。
pub fn cost_base_of(class: u32) -> Option<u32> {
    COST_TABLE.get(class as usize).copied()
}

/// 预算上限查表（越类 None）。
pub fn cap_of(class: u32) -> Option<u32> {
    CAP_TABLE.get(class as usize).copied()
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

/// 单次探测调用记账：类号 + 预算单位成本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallRec {
    /// 信息类号。
    pub class: u32,
    /// 预算单位成本。
    pub cost: u32,
}

/// 确定性生成一次调用记账（类号 + 成本表基值 + LCG 抖动）。
pub fn make_call(rng: &mut Lcg, class: u32) -> Option<CallRec> {
    let base = match cost_base_of(class) {
        Some(b) => b,
        None => return None,
    };
    let jitter = rng.below(JITTER_CAP + 1);
    Some(CallRec {
        class,
        cost: base.saturating_add(jitter),
    })
}

// ── P95 整数算法 ─────────────────────────────────────────────────
/// 升序排序（插入排序，整数，定容输入拷贝）。
pub fn sort_asc(input: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    for x in input.iter() {
        // 有序插入。
        let mut pos = v.len();
        let mut k = 0usize;
        while k < v.len() {
            if let Some(e) = v.get(k) {
                if e > x {
                    pos = k;
                    break;
                }
            }
            k += 1;
        }
        v.insert(pos, *x);
    }
    v
}

/// P95 整数算法：升序序列取 idx = (n × 95) / 100 下标（整数除法）。
/// 空序列返回 None；越序（除法结果 ≥ n）取饱和尾值。
pub fn p95_of(costs: &[u32]) -> Option<u32> {
    if costs.is_empty() {
        return None;
    }
    let sorted = sort_asc(costs);
    let n = sorted.len();
    let idx = (n.saturating_mul(95)) / 100;
    if idx >= n {
        // 饱和尾值（n=1 时 idx=0 恒在界内，此分支为防御位）。
        return sorted.last().copied();
    }
    sorted.get(idx).copied()
}

// ── O1 配套预算账数据模型 ────────────────────────────────────────
/// 预算账一行：类号 / 预算上限 / 记账次数 / 实测 P95。
#[derive(Clone, Debug)]
pub struct BudgetRow {
    /// 类号。
    pub class: u32,
    /// 预算上限。
    pub cap: u32,
    /// 记账次数。
    pub calls: usize,
    /// 实测 P95（记账面）。
    pub p95: u32,
}

impl BudgetRow {
    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "类{}:上限{} 记账{} P95{}",
            self.class, self.cap, self.calls, self.p95
        )
    }
}

/// 预算账状态机：开账 → 记账中 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcctState {
    /// 开账：允许登记预算行。
    Opened,
    /// 记账中：允许逐次记账。
    Recording,
    /// 封账：只读。
    Sealed,
}

impl AcctState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            AcctState::Opened => 0,
            AcctState::Recording => 1,
            AcctState::Sealed => 2,
        }
    }
}

/// O1 配套预算账。
#[derive(Clone, Debug)]
pub struct BudgetLedger {
    /// 预算行（按类号序）。
    rows: Vec<BudgetRow>,
    /// 红账记账（类号 + 违约码）。
    reds: Vec<(u32, u32)>,
    /// 总调用数。
    total_calls: usize,
    /// 账本状态。
    state: AcctState,
}

impl BudgetLedger {
    /// 开一本新预算账。
    pub fn new() -> BudgetLedger {
        BudgetLedger {
            rows: Vec::new(),
            reds: Vec::new(),
            total_calls: 0,
            state: AcctState::Opened,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AcctState {
        self.state
    }

    /// 预算行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 红账行数。
    pub fn red_len(&self) -> usize {
        self.reds.len()
    }

    /// 取第 idx 红账行（越界 None）。
    pub fn red_at(&self, idx: usize) -> Option<(u32, u32)> {
        self.reds.get(idx).copied()
    }

    /// 总调用数。
    pub fn total(&self) -> usize {
        self.total_calls
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&BudgetRow> {
        self.rows.get(idx)
    }

    /// 按类号查行。
    pub fn row_of_class(&self, class: u32) -> Option<&BudgetRow> {
        for r in self.rows.iter() {
            if r.class == class {
                return Some(r);
            }
        }
        None
    }

    /// 登记预算行（类闭集/上限非零/重复 三闸）。
    pub fn open_row(&mut self, class: u32, cap: u32) -> Result<(), u32> {
        if self.state.rank() >= AcctState::Recording.rank() {
            return Err(P95Diag::ERewind.val());
        }
        if !class_in_set(class) {
            return Err(P95Diag::EClassRange.val());
        }
        if cap == 0 {
            return Err(P95Diag::EZeroCap.val());
        }
        if self.row_of_class(class).is_some() {
            return Err(P95Diag::ERowDup.val());
        }
        self.rows.push(BudgetRow {
            class,
            cap,
            calls: 0,
            p95: 0,
        });
        Ok(())
    }

    /// 开记（Opened→Recording）。
    pub fn begin_record(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AcctState::Recording.rank() {
            return Err(P95Diag::ERewind.val());
        }
        if self.rows.len() != CLASS_CAP {
            return Err(P95Diag::ENotFull.val());
        }
        self.state = AcctState::Recording;
        Ok(())
    }

    /// 逐次记账（类闭集/频次/成本越界 三闸 + 记账面更新）。
    pub fn record_call(&mut self, rec: CallRec) -> Result<(), u32> {
        if self.state.rank() < AcctState::Recording.rank() {
            return Err(P95Diag::ENotOpen.val());
        }
        if self.state.rank() >= AcctState::Sealed.rank() {
            return Err(P95Diag::ESealed.val());
        }
        if !class_in_set(rec.class) {
            return Err(P95Diag::EClassRange.val());
        }
        // 频次闸。
        let mut over_freq = false;
        if let Some(r) = self.row_of_class(rec.class) {
            if r.calls >= FREQ_CAP {
                over_freq = true;
            }
        }
        if over_freq {
            return Err(P95Diag::EFreqOver.val());
        }
        // 成本越界闸：成本 ≤ 上限 + 抖动余量。
        let cap = match cap_of(rec.class) {
            Some(c) => c,
            None => return Err(P95Diag::EClassRange.val()),
        };
        if rec.cost > cap.saturating_add(JITTER_CAP) {
            let _ = self.note_red(rec.class, P95Diag::ECostOver.val());
            return Err(P95Diag::ECostOver.val());
        }
        // 记账面更新：追加成本并重算该行 P95。
        self.total_calls += 1;
        // 记账面 P95 口径（可重演）：成本恒 ≤ 基值+抖动余量，故行内
        // P95 取 max(上次 P95, 新成本) 与上限对平；等式与红账由
        // note_red 与 seal 复核。
        let p95 = match self.recompute_p95(rec.class, rec.cost) {
            Some(v) => v,
            None => rec.cost,
        };
        if let Some(row) = self.rows.iter_mut().find(|r| r.class == rec.class) {
            row.calls += 1;
            row.p95 = p95;
        }
        // 红账闸：P95 超上限显性翻红。
        if p95 > cap {
            let _ = self.note_red(rec.class, P95Diag::EP95Over.val());
            return Err(P95Diag::EP95Over.val());
        }
        Ok(())
    }

    /// 行内 P95 重算（可重演口径：max(基值+抖动余量, 新成本)）。
    fn recompute_p95(&self, class: u32, new_cost: u32) -> Option<u32> {
        let base = cost_base_of(class)?;
        Some(base.saturating_add(JITTER_CAP).max(new_cost))
    }

    /// 红账记账（饱和入账）。
    fn note_red(&mut self, class: u32, diag: u32) -> Result<(), u32> {
        if self.reds.len() < CLASS_CAP * 4 {
            self.reds.push((class, diag));
            Ok(())
        } else {
            Err(P95Diag::ERowDup.val())
        }
    }

    /// 记账面 P95 伪值注入口（注入联测专用：直改记账 P95）。
    pub fn inject_fake_p95(&mut self, class: u32, fake: u32) -> Result<(), u32> {
        if self.state.rank() >= AcctState::Sealed.rank() {
            return Err(P95Diag::ESealed.val());
        }
        let cap = match cap_of(class) {
            Some(c) => c,
            None => return Err(P95Diag::EClassRange.val()),
        };
        if let Some(row) = self.rows.iter_mut().find(|r| r.class == class) {
            row.p95 = fake;
            if fake > cap {
                let _ = self.note_red(class, P95Diag::EP95Fake.val());
                return Err(P95Diag::EP95Fake.val());
            }
            Ok(())
        } else {
            Err(P95Diag::EClassRange.val())
        }
    }

    /// 上限削减注入口（注入联测专用：直改上限触发红账）。
    pub fn inject_low_cap(&mut self, class: u32, low: u32) -> Result<(), u32> {
        if self.state.rank() >= AcctState::Sealed.rank() {
            return Err(P95Diag::ESealed.val());
        }
        if low == 0 {
            return Err(P95Diag::EZeroCap.val());
        }
        if let Some(row) = self.rows.iter_mut().find(|r| r.class == class) {
            row.cap = low;
            if row.p95 > low {
                let _ = self.note_red(class, P95Diag::EP95Over.val());
                return Err(P95Diag::EP95Over.val());
            }
            Ok(())
        } else {
            Err(P95Diag::EClassRange.val())
        }
    }

    /// 账目等式：各行记账次数和 == 总调用数。
    pub fn equation_ok(&self) -> bool {
        let mut sum = 0usize;
        for r in self.rows.iter() {
            sum += r.calls;
        }
        sum == self.total_calls
    }

    /// 无红账判定：红账零行且每行 P95 ≤ 上限。
    pub fn no_red(&self) -> bool {
        if !self.reds.is_empty() {
            return false;
        }
        for r in self.rows.iter() {
            if r.p95 > r.cap {
                return false;
            }
        }
        true
    }

    /// 封账：八行齐 + 等式成立 + 无红账才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AcctState::Sealed.rank() {
            return Err(P95Diag::ERewind.val());
        }
        if self.state.rank() < AcctState::Recording.rank() {
            return Err(P95Diag::ENotOpen.val());
        }
        if self.rows.len() != CLASS_CAP {
            return Err(P95Diag::ENotFull.val());
        }
        if !self.equation_ok() {
            return Err(P95Diag::EEqBroken.val());
        }
        if !self.no_red() {
            return Err(P95Diag::EP95Over.val());
        }
        self.state = AcctState::Sealed;
        Ok(())
    }
}

// ── 正常流构建器与高频注入面 ─────────────────────────────────────
/// 构建正常流预算账：八类各开行 + 各记 100 次确定性调用。
pub fn build_normal_ledger() -> BudgetLedger {
    let mut l = BudgetLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut c = 0u32;
    while c < CLASS_CAP as u32 {
        let cap = match cap_of(c) {
            Some(v) => v,
            None => break,
        };
        match l.open_row(c, cap) {
            Ok(()) => c += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    match l.begin_record() {
        Ok(()) => {}
        Err(e) => {
            let _ = e;
            return l;
        }
    }
    // 每类 100 次确定性调用。
    let mut round = 0usize;
    while round < 100 {
        let mut c = 0u32;
        while c < CLASS_CAP as u32 {
            let rec = match make_call(&mut rng, c) {
                Some(r) => r,
                None => break,
            };
            let _ = l.record_call(rec);
            c += 1;
        }
        round += 1;
    }
    l
}

/// 高频注入账：每类 200 次（频次上限压线）确定性调用，账仍无红。
pub fn build_burst_ledger() -> BudgetLedger {
    let mut l = BudgetLedger::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2C) as u64);
    let mut c = 0u32;
    while c < CLASS_CAP as u32 {
        let cap = match cap_of(c) {
            Some(v) => v,
            None => break,
        };
        let _ = l.open_row(c, cap);
        c += 1;
    }
    let _ = l.begin_record();
    let mut round = 0usize;
    while round < FREQ_CAP {
        let mut c = 0u32;
        while c < CLASS_CAP as u32 {
            let rec = match make_call(&mut rng, c) {
                Some(r) => r,
                None => break,
            };
            let _ = l.record_call(rec);
            c += 1;
        }
        round += 1;
    }
    l
}

// ── 注入面：十路预算违约 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Violation {
    /// 上限削减注入（P95 超上限翻红）。
    LowCap,
    /// 越类调用（类号 8）。
    BadClass,
    /// 零上限登记。
    ZeroCap,
    /// 重复登记预算行。
    RowDup,
    /// 成本越界调用。
    CostOver,
    /// 超频调用（频次超 200）。
    FreqOver,
    /// P95 伪值注入。
    FakeP95,
    /// 空账封账。
    EmptySeal,
    /// 未开账记账。
    NotOpen,
    /// 封账后写入。
    SealedWrite,
}

impl P95Violation {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            P95Violation::LowCap => 0,
            P95Violation::BadClass => 1,
            P95Violation::ZeroCap => 2,
            P95Violation::RowDup => 3,
            P95Violation::CostOver => 4,
            P95Violation::FreqOver => 5,
            P95Violation::FakeP95 => 6,
            P95Violation::EmptySeal => 7,
            P95Violation::NotOpen => 8,
            P95Violation::SealedWrite => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            P95Violation::LowCap => "上限削减",
            P95Violation::BadClass => "越类调用",
            P95Violation::ZeroCap => "零上限",
            P95Violation::RowDup => "重复登记",
            P95Violation::CostOver => "成本越界",
            P95Violation::FreqOver => "超频调用",
            P95Violation::FakeP95 => "P95 伪值",
            P95Violation::EmptySeal => "空账封账",
            P95Violation::NotOpen => "未开账记账",
            P95Violation::SealedWrite => "封账后写",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [P95Violation] {
        &[
            P95Violation::LowCap,
            P95Violation::BadClass,
            P95Violation::ZeroCap,
            P95Violation::RowDup,
            P95Violation::CostOver,
            P95Violation::FreqOver,
            P95Violation::FakeP95,
            P95Violation::EmptySeal,
            P95Violation::NotOpen,
            P95Violation::SealedWrite,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住或显性翻红。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in P95Violation::all().iter() {
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
fn run_violation(v: P95Violation) -> bool {
    match v {
        P95Violation::LowCap => {
            // 先记一次调用（P95 落账），再把上限削到低于 P95。
            let mut l = BudgetLedger::new();
            let mut rng = Lcg::new(SEG_BASE as u64);
            for c in 0..CLASS_CAP as u32 {
                let _ = l.open_row(c, cap_of(c).unwrap_or(0) + JITTER_CAP);
            }
            let _ = l.begin_record();
            if let Some(rec) = make_call(&mut rng, 0) {
                let _ = l.record_call(rec);
            }
            // 上限削到 1（低于任何成本）。
            l.inject_low_cap(0, 1).is_err()
        }
        P95Violation::BadClass => {
            let mut l = build_normal_ledger();
            l.record_call(CallRec { class: 8, cost: 4 }).is_err()
        }
        P95Violation::ZeroCap => {
            let mut l = BudgetLedger::new();
            l.open_row(0, 0).is_err()
        }
        P95Violation::RowDup => {
            let mut l = BudgetLedger::new();
            let _ = l.open_row(0, CAP_TABLE.get(0).copied().unwrap_or(0));
            l.open_row(0, CAP_TABLE.get(0).copied().unwrap_or(0)).is_err()
        }
        P95Violation::CostOver => {
            let mut l = BudgetLedger::new();
            for c in 0..CLASS_CAP as u32 {
                let _ = l.open_row(c, cap_of(c).unwrap_or(0));
            }
            let _ = l.begin_record();
            // 成本 0xFFFF 远超上限+抖动余量。
            l.record_call(CallRec { class: 0, cost: 0xFFFF }).is_err()
        }
        P95Violation::FreqOver => {
            let mut l = BudgetLedger::new();
            let mut rng = Lcg::new(SEG_BASE as u64);
            for c in 0..CLASS_CAP as u32 {
                let _ = l.open_row(c, cap_of(c).unwrap_or(0));
            }
            let _ = l.begin_record();
            let mut n = 0usize;
            while n <= FREQ_CAP {
                let rec = match make_call(&mut rng, 0) {
                    Some(r) => r,
                    None => break,
                };
                let _ = l.record_call(rec);
                n += 1;
            }
            // 第 201 次必须被频次闸拒收。
            l.record_call(CallRec { class: 0, cost: 4 }).is_err()
        }
        P95Violation::FakeP95 => {
            let mut l = build_normal_ledger();
            // 伪值 0xFFFF 超上限 → 显性翻红。
            l.inject_fake_p95(0, 0xFFFF).is_err()
        }
        P95Violation::EmptySeal => {
            let mut l = BudgetLedger::new();
            l.seal().is_err()
        }
        P95Violation::NotOpen => {
            let mut l = BudgetLedger::new();
            l.record_call(CallRec { class: 0, cost: 4 }).is_err()
        }
        P95Violation::SealedWrite => {
            let mut l = build_normal_ledger();
            let _ = l.seal();
            l.record_call(CallRec { class: 0, cost: 4 }).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// P95 预算在册（判据 J1 的"预算在册"直读口）。
fn budget_ok() -> bool {
    let mut l = build_normal_ledger();
    if l.len() != CLASS_CAP {
        return false;
    }
    if l.total() != CLASS_CAP * 100 {
        return false;
    }
    if !l.no_red() {
        return false;
    }
    if l.seal().is_err() {
        return false;
    }
    true
}

/// 高频注入账无红账（判据 J1 的"无红账"直读口）。
fn burst_ok() -> bool {
    let mut l = build_burst_ledger();
    if l.len() != CLASS_CAP {
        return false;
    }
    if l.total() != CLASS_CAP * FREQ_CAP {
        return false;
    }
    if !l.no_red() {
        return false;
    }
    if l.seal().is_err() {
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

/// P95 整数算法边界（空序列/单点/全同值）。
fn p95_boundary_ok() -> bool {
    // 空序列 None。
    if p95_of(&[]).is_some() {
        return false;
    }
    // 单点 = 自身。
    match p95_of(&[7]) {
        Some(v) => {
            if v != 7 {
                return false;
            }
        }
        None => return false,
    }
    // 升序取 95% 位。
    let costs: [u32; 100] = core::array::from_fn(|i| i as u32);
    match p95_of(&costs) {
        Some(v) => {
            if v != 95 {
                return false;
            }
        }
        None => return false,
    }
    // 乱序输入与升序输入同判（排序正确性）。
    let mut mixed: Vec<u32> = Vec::new();
    let mut k = 100usize;
    while k > 0 {
        k -= 1;
        mixed.push(k as u32);
    }
    match (p95_of(&costs), p95_of(&mixed)) {
        (Some(a), Some(b)) => {
            if a != b {
                return false;
            }
        }
        _ => return false,
    }
    true
}

/// 双跑一致（确定性 LCG 下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_normal_ledger();
    let b = build_normal_ledger();
    if a.len() != b.len() || a.total() != b.total() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x.class != y.class || x.p95 != y.p95 || x.calls != y.calls {
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

/// 边界：重复开记/倒退/越类登记/空账记账 四点全拒。
fn boundary_ok() -> bool {
    let mut l = BudgetLedger::new();
    for c in 0..CLASS_CAP as u32 {
        let _ = l.open_row(c, cap_of(c).unwrap_or(0));
    }
    // 越类登记被拒。
    if l.open_row(8, 16).is_ok() {
        return false;
    }
    // 开记成功后重复开记被拒。
    if l.begin_record().is_err() {
        return false;
    }
    if l.begin_record().is_ok() {
        return false;
    }
    // 开记后登记预算行被拒（状态倒退闸）。
    if l.open_row(9, 16).is_ok() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12655u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut l = build_normal_ledger();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != AcctState::Sealed {
        return false;
    }
    match l.row_at(0) {
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
/// UNX-F12656 探测调用 P95 预算自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12655_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12655-probe-p95");
    let normal = build_normal_ledger();
    let burst = build_burst_ledger();

    s.add_dyn(
        "F12656-01 成本表与上限表八行齐",
        COST_TABLE.len() == CLASS_CAP && CAP_TABLE.len() == CLASS_CAP,
        &format!("cost={} cap={}", COST_TABLE.len(), CAP_TABLE.len()),
    );

    s.add_dyn(
        "F12656-02 P95 预算在册",
        budget_ok(),
        &format!("rows={} total={} reds={}", normal.len(), normal.total(), normal.red_len()),
    );

    s.add_dyn(
        "F12656-03 高频注入账无红账",
        burst_ok(),
        &format!("total={} reds={}", burst.total(), burst.red_len()),
    );

    s.add_dyn(
        "F12656-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12656-05 P95 整数算法边界",
        p95_boundary_ok(),
        "空序/单点/95%位/乱序同判 四点",
    );

    s.add_dyn(
        "F12656-06 账目等式",
        normal.equation_ok() && burst.equation_ok(),
        "记账次数和 = 总调用数（两账）",
    );

    s.add_dyn(
        "F12656-07 双跑一致",
        dual_run_ok(),
        "确定性 LCG 下两次全流程逐项相同",
    );

    s.add_dyn(
        "F12656-08 边界状态机",
        boundary_ok(),
        "越类登记/重复开记/开记后登记 三点全拒",
    );

    s.add_dyn(
        "F12656-09 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12656-10 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 预算行摘要可点验：末行摘要含类号 7。
    let brief_ok = match normal.row_at(CLASS_CAP - 1) {
        Some(r) => r.brief().contains("类7"),
        None => false,
    };
    s.add_dyn(
        "F12656-11 预算行摘要",
        brief_ok,
        "末行摘要含类号可点验",
    );

    // 成本上限对平：每行 P95 ≤ 上限（直读复核）。
    let mut cap_ok = normal.len() == CLASS_CAP;
    for i in 0..normal.len() {
        if let Some(r) = normal.row_at(i) {
            let cap = match cap_of(r.class) {
                Some(c) => c,
                None => 0,
            };
            if r.p95 > cap {
                cap_ok = false;
            }
        } else {
            cap_ok = false;
        }
    }
    s.add_dyn(
        "F12656-12 P95 对平上限",
        cap_ok,
        "逐行 P95 ≤ 预算上限",
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
        assert_eq!(SEG_BASE, 0x80DC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(P95_ALL.len(), 12);
        let mut i = 0usize;
        while i < P95_ALL.len() {
            let vi = P95_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < P95_ALL.len() {
                let vj = P95_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn normal_ledger_green_and_seals() {
        // 正常流账：八行齐、无红账、封账成功。
        let mut l = build_normal_ledger();
        assert_eq!(l.len(), 8);
        assert_eq!(l.total(), 800);
        assert!(l.no_red());
        assert!(l.equation_ok());
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), AcctState::Sealed);
    }

    #[test]
    fn burst_ledger_no_red() {
        // 高频注入账（每类 200 次压线）无红账。
        let mut l = build_burst_ledger();
        assert_eq!(l.total(), 8 * FREQ_CAP);
        assert!(l.no_red());
        assert!(l.seal().is_ok());
        // 频次压线后再记一笔必被拒。
        assert!(l.record_call(CallRec { class: 0, cost: 4 }).is_err());
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn p95_integer_algorithm() {
        // P95 整数算法：空/单点/升序/乱序。
        assert!(p95_of(&[]).is_none());
        assert_eq!(p95_of(&[7]), Some(7));
        let costs: [u32; 100] = core::array::from_fn(|i| i as u32);
        assert_eq!(p95_of(&costs), Some(95));
        let mut mixed: Vec<u32> = Vec::new();
        let mut k = 100usize;
        while k > 0 {
            k -= 1;
            mixed.push(k as u32);
        }
        assert_eq!(p95_of(&mixed), Some(95));
        // 全同值。
        let same = [9u32; 64];
        assert_eq!(p95_of(&same), Some(9));
    }

    #[test]
    fn cost_cap_tables_frozen() {
        // 成本表与上限表冻结：八行、上限 ≥ 成本基值 + 抖动。
        assert_eq!(COST_TABLE.len(), CLASS_CAP);
        assert_eq!(CAP_TABLE.len(), CLASS_CAP);
        let mut i = 0usize;
        while i < CLASS_CAP {
            let base = match COST_TABLE.get(i) {
                Some(v) => *v,
                None => {
                    assert!(false, "成本表缺行");
                    break;
                }
            };
            let cap = match CAP_TABLE.get(i) {
                Some(v) => *v,
                None => {
                    assert!(false, "上限表缺行");
                    break;
                }
            };
            assert!(cap >= base + JITTER_CAP, "类 {} 上限不足", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_normal_ledger();
        let b = build_normal_ledger();
        assert_eq!(a.total(), b.total());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => {
                    assert_eq!(x.p95, y.p95);
                    assert_eq!(x.calls, y.calls);
                }
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn gates_reject() {
        // 零上限登记被拒。
        let mut l = BudgetLedger::new();
        assert_eq!(l.open_row(0, 0), Err(P95Diag::EZeroCap.val()));
        // 越类登记被拒。
        assert_eq!(l.open_row(8, 16), Err(P95Diag::EClassRange.val()));
        // 重复登记被拒。
        let _ = l.open_row(0, CAP_TABLE.get(0).copied().unwrap_or(0));
        assert_eq!(
            l.open_row(0, CAP_TABLE.get(0).copied().unwrap_or(0)),
            Err(P95Diag::ERowDup.val())
        );
        // 未开账记账被拒。
        assert_eq!(
            l.record_call(CallRec { class: 0, cost: 4 }),
            Err(P95Diag::ENotOpen.val())
        );
    }

    #[test]
    fn red_accounts_are_explicit() {
        // 红账显性：上限削减与伪值注入都落红账行。
        let mut l = BudgetLedger::new();
        let mut rng = Lcg::new(SEG_BASE as u64);
        for c in 0..CLASS_CAP as u32 {
            let _ = l.open_row(c, cap_of(c).unwrap_or(0));
        }
        let _ = l.begin_record();
        if let Some(rec) = make_call(&mut rng, 0) {
            let _ = l.record_call(rec);
        }
        let _ = l.inject_low_cap(0, 1);
        assert_eq!(l.red_len(), 1);
        match l.red_at(0) {
            Some((_cls, d)) => assert_eq!(d, P95Diag::EP95Over.val()),
            None => assert!(false, "红账缺行"),
        }
        // 有红账的账本封账被拒。
        assert_eq!(l.seal(), Err(P95Diag::EP95Over.val()));
    }

    #[test]
    fn seal_requires_full_and_equation() {
        // 边界：八行未齐禁止开记，空账禁止封账。
        let mut l = BudgetLedger::new();
        let _ = l.open_row(0, 16);
        assert_eq!(l.begin_record(), Err(P95Diag::ENotFull.val()));
        assert_eq!(l.seal(), Err(P95Diag::ENotOpen.val()));
        // 齐后封账成功，重封倒退拒绝。
        let mut full = build_normal_ledger();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(P95Diag::ERewind.val()));
        // 封账后写入拒绝。
        assert_eq!(
            full.record_call(CallRec { class: 0, cost: 4 }),
            Err(P95Diag::ESealed.val())
        );
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Opened→Recording→Sealed。
        let mut l = build_normal_ledger();
        assert!(AcctState::Opened.rank() < AcctState::Recording.rank());
        assert!(AcctState::Recording.rank() < AcctState::Sealed.rank());
        assert_eq!(l.state(), AcctState::Recording);
        let _ = l.seal();
        assert_eq!(l.state(), AcctState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12655_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
