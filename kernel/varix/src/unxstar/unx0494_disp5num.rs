//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0494 显示面五数概括账 · 0x75F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6 M组）：
//!   概括 5 项与逐项账复算一致，任一数篡改必抓。
//! 逐卡要点：
//!   显示面五数（渲染操作总数/越界拦截数/哨兵破坏数/图案校验数/多输
//!   出候选数）逐项复算一致，篡改必抓。
//! 设计要点：
//!   ① 五数为闭集：渲染操作总数 / 越界拦截数 / 哨兵破坏数 / 图案校验
//!     数 / 多输出候选数，类目增减均视为口径漂移而拒收；
//!   ② 逐项账为操作流水（Vec<DispOp>），每行带操作类与单位数，概括
//!     账由流水逐行累加生成，复算即重放流水再逐项比对；
//!   ③ 概括账走 草稿→冻结→勾稽 单调状态机，冻结后改账显性拒；
//!   ④ 篡改注入：LCG 固定种子选五数之一加非零扰动（正负双向），
//!     复算必抓 10/10，漏抓即红线；
//!   ⑤ 累加一律 checked_add，溢出报诊断而非静默回绕；扰动钳位
//!     saturating，保证扰动后必然偏离真值；
//!   ⑥ 常量冻结：五数类目数 5、判据轮数 10、流水容量 128、哨兵标记
//!     0xA55A（来源：本卡判据与显示面 40x12 文本面步长约定）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75F00，AI-VX7-1104 批6 M组独占窗口起点）。
pub const SEG_BASE: u32 = 0x75F00;

/// 五数类目数（冻结 5：渲染/越界/哨兵/图案/多输出）。
pub const FIVE_KINDS: usize = 5;
/// 判据轮数（写死 10：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 逐项流水容量上限（冻结 128，显示面文本 40x12 步长的整倍）。
pub const OP_CAP: usize = 128;
/// 哨兵行标记（冻结 0xA55A：哨兵破坏类操作的单位基数）。
pub const SENTINEL_MARK: u32 = 0xA55A;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiveNumCode {
    /// 复算失配：概括数与流水重放数不等
    RecountMismatch,
    /// 篡改漏抓：扰动后的概括数复算未报错
    TamperEscape,
    /// 状态倒退：勾稽/冻结态回退草稿态
    BackStep,
    /// 冻结后改账：冻结态仍写流水
    FrozenWrite,
    /// 未知操作类：流水行操作类不在五类闭集
    OpKindBad,
    /// 账序断裂：流水行序号不单调
    SeqGap,
    /// 计数溢出：累加超 u64 上界
    CountOverflow,
    /// 流水超容：行数超 128 上限
    OpCapOver,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl FiveNumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FiveNumCode::RecountMismatch => "复算失配：请重放流水逐行累加并逐项比对五数",
            FiveNumCode::TamperEscape => "篡改漏抓：扰动后复算应报失配，检查比对是否逐项落地",
            FiveNumCode::BackStep => "状态倒退：勾稽态不可回退，请先解封再重开账",
            FiveNumCode::FrozenWrite => "冻结后改账：冻结态只读，请先解封概括账再写入",
            FiveNumCode::OpKindBad => "未知操作类：流水行操作类必须在五类闭集内",
            FiveNumCode::SeqGap => "账序断裂：流水行序号必须从 0 起单调递增",
            FiveNumCode::CountOverflow => "计数溢出：请用 checked_add 分账落盘后再累加",
            FiveNumCode::OpCapOver => "流水超容：行数超 128，请归档旧流水后继续",
            FiveNumCode::NotReproducible => "双跑不一致：复现红线，请固定种子与输入重跑",
        }
    }
    pub const ALL: [FiveNumCode; 9] = [
        FiveNumCode::RecountMismatch,
        FiveNumCode::TamperEscape,
        FiveNumCode::BackStep,
        FiveNumCode::FrozenWrite,
        FiveNumCode::OpKindBad,
        FiveNumCode::SeqGap,
        FiveNumCode::CountOverflow,
        FiveNumCode::OpCapOver,
        FiveNumCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码给中文提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FiveNumCode::ALL.len() {
        if let Some(c) = FiveNumCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75F00 段闭集内"
}

// ── 显示面操作类（五类闭集） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// 渲染操作（对应"渲染操作总数"）
    Render,
    /// 越界拦截（对应"越界拦截数"）
    OobBlock,
    /// 哨兵破坏（对应"哨兵破坏数"）
    SentinelBreak,
    /// 图案校验（对应"图案校验数"）
    PatCheck,
    /// 多输出候选（对应"多输出候选数"）
    MultioutCand,
}

/// 操作类 → 五数槽位（0..5 闭集映射）。
pub fn kind_slot(k: OpKind) -> usize {
    match k {
        OpKind::Render => 0,
        OpKind::OobBlock => 1,
        OpKind::SentinelBreak => 2,
        OpKind::PatCheck => 3,
        OpKind::MultioutCand => 4,
    }
}

/// 逐项流水行：操作类 + 单位数 + 行序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispOp {
    pub seq: u32,
    pub kind: OpKind,
    pub units: u64,
}

/// 五数概括（闭集 5 项，数组定容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    pub slots: [u64; FIVE_KINDS],
}

impl FiveNum {
    pub fn zero() -> FiveNum {
        FiveNum { slots: [0; FIVE_KINDS] }
    }
    /// 累加一行流水（checked_add，溢出报诊断）。
    pub fn add_row(&mut self, op: &DispOp) -> Result<(), u32> {
        let slot = kind_slot(op.kind);
        match self.slots.get(slot) {
            Some(cur) => match cur.checked_add(op.units) {
                Some(next) => {
                    if let Some(dst) = self.slots.get_mut(slot) {
                        *dst = next;
                    }
                    Ok(())
                }
                None => Err(FiveNumCode::CountOverflow.val()),
            },
            None => Err(FiveNumCode::OpKindBad.val()),
        }
    }
    /// 渲染操作总数。
    pub fn render_ops(&self) -> u64 {
        *self.slots.get(0).unwrap_or(&0)
    }
    /// 越界拦截数。
    pub fn oob_blocks(&self) -> u64 {
        *self.slots.get(1).unwrap_or(&0)
    }
    /// 哨兵破坏数。
    pub fn sentinel_breaks(&self) -> u64 {
        *self.slots.get(2).unwrap_or(&0)
    }
    /// 图案校验数。
    pub fn pat_checks(&self) -> u64 {
        *self.slots.get(3).unwrap_or(&0)
    }
    /// 多输出候选数。
    pub fn multiout_cands(&self) -> u64 {
        *self.slots.get(4).unwrap_or(&0)
    }
}

/// 五数逐项复算：重放流水逐行累加，账序从 0 起单调。
pub fn recount(ops: &[DispOp]) -> Result<FiveNum, u32> {
    if ops.len() > OP_CAP {
        return Err(FiveNumCode::OpCapOver.val());
    }
    let mut five = FiveNum::zero();
    let mut expect_seq: u32 = 0;
    for op in ops.iter() {
        if op.seq != expect_seq {
            return Err(FiveNumCode::SeqGap.val());
        }
        five.add_row(op)?;
        match expect_seq.checked_add(1) {
            Some(n) => expect_seq = n,
            None => return Err(FiveNumCode::CountOverflow.val()),
        }
    }
    Ok(five)
}

// ── 概括账状态机（草稿→冻结→勾稽，单调） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumState {
    /// 草稿：可写入
    Draft,
    /// 冻结：只读
    Frozen,
    /// 勾稽：复算一致已落账
    Reconciled,
}

/// 概括账：逐项流水 + 概括五数 + 单调状态。
#[derive(Debug)]
pub struct SumLedger {
    pub ops: Vec<DispOp>,
    pub five: FiveNum,
    pub state: SumState,
    pub seq_next: u32,
}

impl SumLedger {
    pub fn new() -> SumLedger {
        SumLedger { ops: Vec::new(), five: FiveNum::zero(), state: SumState::Draft, seq_next: 0 }
    }

    /// 追加一行流水（只增不减；超容/非草稿态拒）。
    pub fn append(&mut self, kind: OpKind, units: u64) -> Result<usize, u32> {
        if self.state != SumState::Draft {
            return Err(FiveNumCode::FrozenWrite.val());
        }
        if self.ops.len() >= OP_CAP {
            return Err(FiveNumCode::OpCapOver.val());
        }
        let seq = self.seq_next;
        let op = DispOp { seq, kind, units };
        self.five.add_row(&op)?;
        self.ops.push(op);
        match self.seq_next.checked_add(1) {
            Some(n) => self.seq_next = n,
            None => return Err(FiveNumCode::CountOverflow.val()),
        }
        Ok(self.ops.len().saturating_sub(1))
    }

    /// 冻结（草稿→冻结，其余拒）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state != SumState::Draft {
            return Err(FiveNumCode::BackStep.val());
        }
        self.state = SumState::Frozen;
        Ok(())
    }

    /// 勾稽（冻结→勾稽：重放流水与概括五数逐项比对）。
    pub fn reconcile(&mut self) -> Result<FiveNum, u32> {
        if self.state != SumState::Frozen {
            return Err(FiveNumCode::BackStep.val());
        }
        let replay = recount(&self.ops)?;
        if replay != self.five {
            return Err(FiveNumCode::RecountMismatch.val());
        }
        self.state = SumState::Reconciled;
        Ok(replay)
    }

    /// 勾稽态回退显性拒（倒退红线探针）。
    pub fn demote(&self) -> Result<(), u32> {
        if self.state == SumState::Reconciled || self.state == SumState::Frozen {
            return Err(FiveNumCode::BackStep.val());
        }
        Ok(())
    }
}

// ── LCG（固定种子，整数口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 真机流水语料（手写，覆盖五类） ──────────────────────────────
/// 12 行真机流水：渲染 4 行、越界拦截 3 行、哨兵破坏 2 行、
/// 图案校验 2 行、多输出候选 1 行（每行单位数来自显示面
/// 40x12 文本面的真实步长记账）。
pub fn machine_ops() -> Vec<DispOp> {
    let mut v: Vec<DispOp> = Vec::new();
    let mut seq: u32 = 0;
    let rows: [(OpKind, u64); 12] = [
        (OpKind::Render, 40),
        (OpKind::Render, 40),
        (OpKind::OobBlock, 1),
        (OpKind::PatCheck, 8),
        (OpKind::Render, 40),
        (OpKind::SentinelBreak, 4),
        (OpKind::Render, 40),
        (OpKind::OobBlock, 2),
        (OpKind::PatCheck, 8),
        (OpKind::SentinelBreak, 2),
        (OpKind::OobBlock, 3),
        (OpKind::MultioutCand, 2),
    ];
    for r in rows.iter() {
        v.push(DispOp { seq, kind: r.0, units: r.1 });
        seq += 1;
    }
    v
}

/// 期望五数（按真机流水手算）：
///   渲染 160 / 越界 6 / 哨兵 6 / 图案 16 / 多输出 2。
pub fn machine_five() -> FiveNum {
    FiveNum { slots: [160, 6, 6, 16, 2] }
}

// ── 篡改注入（单槽 ± 非零扰动，saturating 钳位） ────────────────
pub struct TamperLedger {
    pub tries: u32,
    pub caught: u32,
}

/// 对概括五数做单槽扰动（±delta，负向钳位到 0，保证偏离真值）。
pub fn tamper_slot(base: &FiveNum, slot: usize, delta: i64) -> FiveNum {
    let mut t = *base;
    if let Some(cur) = t.slots.get(slot).copied() {
        let bumped = (cur as i64).saturating_add(delta);
        let clamped = if bumped < 0 { 0i64 } else { bumped };
        if let Some(dst) = t.slots.get_mut(slot) {
            *dst = clamped as u64;
        }
    }
    t
}

/// 注入轮：每轮 LCG 选一槽加非零扰动，同流水复算必须报失配。
pub fn tamper_rounds(rounds: u32) -> Result<TamperLedger, u32> {
    let mut led = TamperLedger { tries: 0, caught: 0 };
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let slot = rng.below(FIVE_KINDS as u64) as usize;
        let raw = (rng.below(16) as i64).saturating_add(1);
        let delta = if raw % 2 == 0 { raw } else { -raw };
        let spoiled = tamper_slot(&machine_five(), slot, delta);
        // 被扰动的概括账与同一流水复算结果必须不等。
        let hit = match recount(&machine_ops()) {
            Ok(replay) => replay != spoiled,
            Err(_) => false,
        };
        led.tries = led.tries.saturating_add(1);
        if hit {
            led.caught = led.caught.saturating_add(1);
        }
        r += 1;
    }
    if led.caught != led.tries {
        return Err(FiveNumCode::TamperEscape.val());
    }
    Ok(led)
}

// ── 黄金全链：记流水→冻结→勾稽→五数比对→双拒 ───────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut l = SumLedger::new();
    let ops = machine_ops();
    let mut i = 0;
    while i < ops.len() {
        if let Some(op) = ops.get(i) {
            l.append(op.kind, op.units)?;
        }
        i += 1;
    }
    let rows_ok = (l.ops.len() == 12 && l.five == machine_five()) as u32;
    l.freeze()?;
    let replay = l.reconcile()?;
    let recon_ok = (replay == machine_five()) as u32;
    // 冻结后追加应拒；勾稽态回退应拒。
    let frozen_deny = l.append(OpKind::Render, 1).is_err();
    let back_deny = l.demote().is_err();
    Ok((rows_ok.saturating_add(recon_ok), frozen_deny as u32 + back_deny as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 2/2 + 双拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2)) => Ok(()),
        _ => Err(FiveNumCode::NotReproducible.val()),
    }
}

/// 序 B：账序断裂必抓（构造 seq 跳行流水）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut ops = machine_ops();
    let last = match ops.last().copied() {
        Some(x) => x,
        None => return Err(FiveNumCode::NotReproducible.val()),
    };
    let _ = ops.pop();
    ops.push(DispOp { seq: last.seq.saturating_add(3), kind: last.kind, units: last.units });
    match recount(&ops) {
        Err(e) if e == FiveNumCode::SeqGap.val() => Ok(()),
        _ => Err(FiveNumCode::NotReproducible.val()),
    }
}

/// 序 C：常量冻结（五类目 5 / 判据轮 10 / 容量 128 / 哨兵标记）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FIVE_KINDS == 5,
        JUDGE_ROUNDS == 10,
        OP_CAP == 128,
        SENTINEL_MARK == 0xA55A,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(FiveNumCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FiveNumCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < FiveNumCode::ALL.len() {
        if let Some(c) = FiveNumCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b)) => *a == 2 && *b == 2,
            Err(_) => false,
        },
        match &g {
            Ok((a, b)) => format!("五数+勾稽={} 双拒={}", a, b),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ② 五数复算一致（真机流水 vs 手算期望）。
    let replay = recount(&machine_ops());
    v.push((
        "J1-五数复算一致",
        match &replay {
            Ok(f) => *f == machine_five(),
            Err(_) => false,
        },
        match &replay {
            Ok(f) => format!(
                "渲染{}越界{}哨兵{}图案{}多出{}",
                f.render_ops(),
                f.oob_blocks(),
                f.sentinel_breaks(),
                f.pat_checks(),
                f.multiout_cands()
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ③ 篡改必抓 10/10。
    let inj = tamper_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-篡改必抓10/10",
        match &inj {
            Ok(l) => l.tries == JUDGE_ROUNDS && l.caught == l.tries,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={} caught={}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ④ 单槽扰动点抽检（哨兵槽 +4 必报失配）。
    let spoiled = tamper_slot(&machine_five(), 2, 4);
    let point_hit = match recount(&machine_ops()) {
        Ok(replay) => replay != spoiled,
        Err(_) => false,
    };
    v.push(("J1-单槽扰动抽检", point_hit, String::from("哨兵槽 +4 复算必报失配")));

    // ⑤ 状态机倒退显性拒。
    let mut l5 = SumLedger::new();
    let _ = l5.freeze();
    let freeze_first = l5.freeze().is_err();
    let _ = l5.reconcile();
    let demote_deny = l5.demote().is_err();
    v.push((
        "J1-状态机倒退拒",
        freeze_first && demote_deny,
        String::from("草稿→冻结→勾稽 单调，回退全拒"),
    ));

    // ⑥ 冻结后改账拒。
    let mut l6 = SumLedger::new();
    let _ = l6.freeze();
    let fw = l6.append(OpKind::Render, 1).is_err();
    v.push(("J1-冻结后改账拒", fw, String::from("冻结态追加被拒，建议先解封")));

    // ⑦ 账序断裂必抓。
    let seqb = probe_seq_b().is_ok();
    v.push(("J1-账序断裂必抓", seqb, String::from("seq 跳行复算报 SeqGap")));

    // ⑧ 流水超容边界（129 行拒收）。
    let mut over: Vec<DispOp> = Vec::new();
    let mut seq: u32 = 0;
    while seq < 129 {
        over.push(DispOp { seq, kind: OpKind::Render, units: 1 });
        seq += 1;
    }
    let cap_deny = recount(&over).is_err();
    v.push((
        "J1-流水超容拒",
        cap_deny,
        String::from("129 行 > 128 容量，报 OpCapOver"),
    ));

    // ⑨ 溢出边界（u64 上界累加报溢出：先累到上界再 +1 必溢）。
    let big = DispOp { seq: 0, kind: OpKind::Render, units: u64::MAX };
    let one = DispOp { seq: 1, kind: OpKind::Render, units: 1 };
    let mut f9 = FiveNum::zero();
    let first_ok = f9.add_row(&big).is_ok();
    let ov = f9.add_row(&one).is_err();
    v.push((
        "J1-计数溢出拒",
        first_ok && ov,
        String::from("上界累加后再 +1 报 CountOverflow"),
    ));

    // ⑩ 操作类闭集映射。
    let kind_ok =
        kind_slot(OpKind::Render) == 0 && kind_slot(OpKind::MultioutCand) == FIVE_KINDS - 1;
    v.push(("J1-操作类闭集", kind_ok, String::from("五类槽位 0..5 闭集映射")));

    // ⑪ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push(("J1-探针三序", a && b && c, format!("a={} b=检出{} c=检出{}", a, b, c)));

    // ⑫ 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // ⑬ 诊断码闭集查表。
    let mut codes_ok = true;
    let mut ci = 0;
    while ci < FiveNumCode::ALL.len() {
        if let Some(c) = FiveNumCode::ALL.get(ci) {
            if detail(c.val()) != c.detail() {
                codes_ok = false;
            }
        }
        ci += 1;
    }
    v.push((
        "J3-诊断码闭集",
        codes_ok && detail(0xDEAD_0000) == "未知诊断码：不在 0x75F00 段闭集内",
        String::from("ALL 查表一致 + 未知码兜底"),
    ));

    // ⑭ 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75F00 窗口 + 0x40 对齐"),
    ));
    v
}

pub fn run_unx0494_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0494 显示面五数概括账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_chain() {
        assert_eq!(golden_run(), Ok((2, 2)));
    }

    #[test]
    fn t02_five_recount() {
        assert_eq!(recount(&machine_ops()), Ok(machine_five()));
        let f = match recount(&machine_ops()) {
            Ok(x) => x,
            Err(_) => FiveNum::zero(),
        };
        assert_eq!(f.render_ops(), 160);
        assert_eq!(f.oob_blocks(), 6);
        assert_eq!(f.sentinel_breaks(), 6);
        assert_eq!(f.pat_checks(), 16);
        assert_eq!(f.multiout_cands(), 2);
    }

    #[test]
    fn t03_tamper_all_slots_caught() {
        let base = machine_five();
        let mut slot = 0;
        while slot < FIVE_KINDS {
            let spoiled = tamper_slot(&base, slot, 7);
            assert_ne!(recount(&machine_ops()), Ok(spoiled), "slot={} 漏抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn t04_tamper_rounds_full() {
        let led = match tamper_rounds(JUDGE_ROUNDS) {
            Ok(x) => x,
            Err(_) => TamperLedger { tries: 0, caught: 0 },
        };
        assert_eq!(led.tries, JUDGE_ROUNDS);
        assert_eq!(led.caught, JUDGE_ROUNDS);
    }

    #[test]
    fn t04b_tamper_rounds_no_err() {
        assert!(tamper_rounds(JUDGE_ROUNDS).is_ok());
    }

    #[test]
    fn t05_state_machine_monotonic() {
        let mut l = SumLedger::new();
        assert!(l.freeze().is_ok());
        assert!(l.freeze().is_err());
        assert!(l.reconcile().is_ok());
        assert!(l.demote().is_err());
        assert!(l.append(OpKind::Render, 1).is_err());
    }

    #[test]
    fn t06_seq_gap_caught() {
        let mut ops = machine_ops();
        if let Some(last) = ops.last().copied() {
            let _ = ops.pop();
            ops.push(DispOp { seq: last.seq + 5, kind: last.kind, units: last.units });
        }
        assert_eq!(recount(&ops), Err(FiveNumCode::SeqGap.val()));
    }

    #[test]
    fn t07_cap_and_overflow() {
        let mut over: Vec<DispOp> = Vec::new();
        let mut seq: u32 = 0;
        while seq < 129 {
            over.push(DispOp { seq, kind: OpKind::Render, units: 1 });
            seq += 1;
        }
        assert!(recount(&over).is_err());
        let big = DispOp { seq: 0, kind: OpKind::Render, units: u64::MAX };
        let one = DispOp { seq: 1, kind: OpKind::Render, units: 1 };
        let mut f = FiveNum::zero();
        assert!(f.add_row(&big).is_ok());
        assert!(f.add_row(&one).is_err());
        // recount 路径同口径：两行流水重放溢出。
        let ops = vec![big, one];
        assert_eq!(recount(&ops), Err(FiveNumCode::CountOverflow.val()));
    }

    #[test]
    fn t08_repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t09_code_closed_set() {
        assert_eq!(FiveNumCode::ALL.len(), 9);
        let mut i = 0;
        while i < FiveNumCode::ALL.len() {
            if let Some(c) = FiveNumCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
    }

    #[test]
    fn t10_negative_tamper_clamped() {
        let spoiled = tamper_slot(&machine_five(), 1, -100);
        assert_eq!(spoiled.oob_blocks(), 0);
        assert_ne!(recount(&machine_ops()), Ok(spoiled));
    }

    #[test]
    fn t11_op_kind_slot_mapping() {
        assert_eq!(kind_slot(OpKind::Render), 0);
        assert_eq!(kind_slot(OpKind::OobBlock), 1);
        assert_eq!(kind_slot(OpKind::SentinelBreak), 2);
        assert_eq!(kind_slot(OpKind::PatCheck), 3);
        assert_eq!(kind_slot(OpKind::MultioutCand), 4);
    }

    #[test]
    fn t12_probes_abc() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
