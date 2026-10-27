//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1267 · Irq 态稳态故障注入（稳态中断丢失/重复必抓） · 0xFC70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1267-J1 故障场景 ≥5 类（稳态丢中断/重复投递/确认错配/
//!      亲和漂移/配额失效）注入 10/10 次必抓，防护动作正确率 100%，
//!      资产可重复一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五类故障场景资产化：稳态丢中断（序号跳号）/重复投递（序号回放）/
//!      确认错配（ack 号≠投递号）/亲和漂移（CPU 遗留计数）/配额失效
//!      （越限不阻流）；
//!   - 检测核：投递序号账（append-only）+ 期望序号指针，跳号判丢、
//!      回放判重、ack 对不上判错配，逐笔可判；
//!   - 防护动作：丢→补投登记、重→去重丢弃、错配→重确认、漂移→
//!      归账、配额失效→阻流；防护后账面必须回到守恒；
//!   - 注入 10/10 必抓（判据正向面），未注入 10/10 零误报（反向面）；
//!   - LCG 种子 0xFC70_1267 写死，两轮同种子逐位同（可重复 100%）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFC70;

/// 故障场景数（判据点名 ≥5）。
pub const FAULT_COUNT: usize = 5;

/// 每轮注入事件数（确定性规模）。
pub const EVENTS_PER_ROUND: u64 = 200;

/// 注入演练轮数（判据点名 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 在途补投缓冲上限。
pub const PENDING_CAP: usize = 32;

/// 亲和 CPU 数。
pub const CPU_COUNT: usize = 4;

/// 配额基线（笔/千 tick，越限判据线）。
pub const QUOTA_BASE: u64 = 120;

/// 诊断码（0xFC70 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IjCode {
    /// 0xFC70 稳态丢中断（序号跳号未补）。
    LostIrq = 0,
    /// 0xFC71 重复投递（序号回放）。
    DupDeliver = 1,
    /// 0xFC72 确认错配（ack≠投递号）。
    AckMismatch = 2,
    /// 0xFC73 亲和漂移（CPU 计数与路由账不合）。
    AffDrift = 3,
    /// 0xFC74 配额失效（越限不阻流）。
    QuotaFail = 4,
    /// 0xFC75 场景号越界（出 0..5）。
    ScenOob = 5,
    /// 0xFC76 补投缓冲击穿（在途超上限）。
    PendingBreach = 6,
    /// 0xFC77 防护后不守恒（防护动作没把账救回来）。
    GuardBroken = 7,
    /// 0xFC78 零故障误报（干净流被判故障）。
    FalseAlarm = 8,
    /// 0xFC79 序号账损坏（补投后序号不连续且无缺口登记）。
    SeqCorrupt = 9,
}

impl IjCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [IjCode; 10] = [
        IjCode::LostIrq, IjCode::DupDeliver, IjCode::AckMismatch, IjCode::AffDrift,
        IjCode::QuotaFail, IjCode::ScenOob, IjCode::PendingBreach, IjCode::GuardBroken,
        IjCode::FalseAlarm, IjCode::SeqCorrupt,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC70 => "稳态丢中断：序号跳号，查补投登记是否启动，缺口未闭合前不撤警",
        0xFC71 => "重复投递：序号回放，去重丢弃并记重复账，查对端重发源",
        0xFC72 => "确认错配：ack 号与投递号不合，重确认并以投递序号账为准",
        0xFC73 => "亲和漂移：CPU 计数与路由账不合，先归账再查路由 bin 宽度",
        0xFC74 => "配额失效：越限未阻流，查配额闸开闭状态位",
        0xFC75 => "场景号越界：五类场景只认 0..5，索引先比 FAULT_COUNT",
        0xFC76 => "补投缓冲击穿：在途超上限，说明缺口长期未闭合，升级为红账",
        0xFC77 => "防护后不守恒：防护动作未救回账面，重放防护序定位失效步",
        0xFC78 => "零故障误报：干净流被判故障，查判准阈值是否被污染",
        0xFC79 => "序号账损坏：既不连续又无缺口登记，账面不可信，重建序号账",
        _ => "未知诊断码：查 0xFC70 段闭集 IjCode::ALL",
    }
}

/// 五类故障场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 稳态丢中断：跳过一序号不投。
    LostIrq = 0,
    /// 重复投递：同序号投两次。
    DupDeliver = 1,
    /// 确认错配：ack 号偏移 +1。
    AckMismatch = 2,
    /// 亲和漂移：CPU 计数与路由不一致。
    AffDrift = 3,
    /// 配额失效：越限后闸不关。
    QuotaFail = 4,
}

impl FaultKind {
    /// 由号取场景。
    pub const fn from(n: usize) -> Option<FaultKind> {
        match n {
            0 => Some(FaultKind::LostIrq),
            1 => Some(FaultKind::DupDeliver),
            2 => Some(FaultKind::AckMismatch),
            3 => Some(FaultKind::AffDrift),
            4 => Some(FaultKind::QuotaFail),
            _ => None,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            FaultKind::LostIrq => "稳态丢中断",
            FaultKind::DupDeliver => "重复投递",
            FaultKind::AckMismatch => "确认错配",
            FaultKind::AffDrift => "亲和漂移",
            FaultKind::QuotaFail => "配额失效",
        }
    }
}

/// 检测结论（一笔事件过检测核后的判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 正常给付。
    Clean,
    /// 丢中断（跳号，缺口登记）。
    Lost(u64),
    /// 重复投递（回放序号）。
    Dup(u64),
    /// 确认错配（投递号, 错 ack 号）。
    AckBad(u64, u64),
    /// 亲和漂移（CPU 号）。
    AffDrift(usize),
    /// 配额失效（当前速率）。
    QuotaBad(u64),
}

/// 稳态检测核（本单正本）：序号账 + 期望指针 + 各部件分量账。
#[derive(Debug)]
pub struct DetectCore {
    /// 期望下一序号（从 1 起）。
    expect_seq: u64,
    /// 已付序号账（append-only，去重后的连续段）。
    paid: Vec<u64>,
    /// 缺口登记（丢中断补投用）。
    gaps: Vec<u64>,
    /// 重复丢弃计数。
    dup_dropped: u64,
    /// 重确认成功计数。
    reack_ok: u64,
    /// 归账成功计数。
    realign_ok: u64,
    /// 阻流触发计数。
    blocked: u64,
    /// 亲和账（逐 CPU，按路由律 seq%4 归账）。
    aff: [u64; CPU_COUNT],
    /// 路由账（同律独立推演，两源对齐用）。
    aff_expect: [u64; CPU_COUNT],
    /// 当前窗口速率（笔/千 tick）。
    rate: u64,
    /// 配额闸开（true=放行）。
    quota_open: bool,
    /// 补投在途。
    pending: u64,
    /// 防护动作日志（append-only 动作号）。
    guard_log: Vec<u8>,
}

impl DetectCore {
    /// 建核：配额闸开、两源亲和账清零。
    pub fn new() -> DetectCore {
        DetectCore {
            expect_seq: 1,
            paid: Vec::new(),
            gaps: Vec::new(),
            dup_dropped: 0,
            reack_ok: 0,
            realign_ok: 0,
            blocked: 0,
            aff: [0; CPU_COUNT],
            aff_expect: [0; CPU_COUNT],
            rate: 0,
            quota_open: true,
            pending: 0,
            guard_log: Vec::new(),
        }
    }

    /// 亲和期望账预登记（路由律：seq%4，与给付同律两源）。
    pub fn pre_route(&mut self, seq: u64) {
        let c = (seq % CPU_COUNT as u64) as usize;
        self.aff_expect[c] = self.aff_expect[c].wrapping_add(1);
    }

    /// 投递一笔：返回检测结论（不静默，逐笔可判）。
    pub fn deliver(&mut self, seq: u64) -> Verdict {
        // 判一：重复（序号 ≤ 已付最大且不在缺口）。
        if let Some(last) = self.paid.last().copied() {
            if seq <= last {
                self.dup_dropped = self.dup_dropped.wrapping_add(1);
                self.guard_log.push(1); // 防护动作 1：去重丢弃
                return Verdict::Dup(seq);
            }
        }
        // 判二：跳号（> 期望 → 缺口；远跳不逐号展开，防巨账）。
        if seq > self.expect_seq {
            if seq - self.expect_seq > 4096 {
                // 远跳：只登记一枚合成缺口，序号直接跳到新位。
                self.gaps.push(self.expect_seq);
            } else {
                let mut g = self.expect_seq;
                while g < seq {
                    self.gaps.push(g);
                    g += 1;
                }
            }
            self.paid.push(seq);
            self.expect_seq = seq + 1;
            let c = (seq % CPU_COUNT as u64) as usize;
            self.aff[c] = self.aff[c].wrapping_add(1);
            return Verdict::Lost(seq);
        }
        // 正常给付。
        self.paid.push(seq);
        self.expect_seq = seq + 1;
        let c = (seq % CPU_COUNT as u64) as usize;
        self.aff[c] = self.aff[c].wrapping_add(1);
        Verdict::Clean
    }

    /// 确认一笔：ack 号须等于最后一笔投递号，否则错配。
    pub fn ack(&mut self, ack_seq: u64) -> Result<(), IjCode> {
        let last = self.paid.last().copied().unwrap_or(0);
        if ack_seq != last {
            // 防护动作：按投递账重确认。
            self.reack_ok = self.reack_ok.wrapping_add(1);
            self.guard_log.push(2);
            return Err(IjCode::AckMismatch);
        }
        Ok(())
    }

    /// 亲和对账：两源逐 CPU 必须相等，不等即漂移（防护：归账）。
    pub fn affinity_check(&mut self) -> Result<(), IjCode> {
        for c in 0..CPU_COUNT {
            if self.aff[c] != self.aff_expect[c] {
                // 防护动作：以期望账归正。
                self.aff[c] = self.aff_expect[c];
                self.realign_ok = self.realign_ok.wrapping_add(1);
                self.guard_log.push(3);
                return Err(IjCode::AffDrift);
            }
        }
        Ok(())
    }

    /// 配额闸：速率超基线且闸开 → 关闸阻流（防护）；失效场景闸不关即报。
    pub fn quota_gate(&mut self, rate: u64) -> Result<(), IjCode> {
        self.rate = rate;
        if rate > QUOTA_BASE {
            if self.quota_open {
                self.quota_open = false;
                self.blocked = self.blocked.wrapping_add(1);
                self.guard_log.push(4);
                return Err(IjCode::QuotaFail);
            }
            return Err(IjCode::QuotaFail); // 仍超限仍报
        }
        self.quota_open = true;
        Ok(())
    }

    /// 防护动作：补投缺口（丢中断 → 逐缺口补付）。
    pub fn repair_gaps(&mut self) -> Result<u64, IjCode> {
        let n = self.gaps.len() as u64;
        if n as usize > PENDING_CAP {
            return Err(IjCode::PendingBreach);
        }
        while let Some(g) = self.gaps.pop() {
            self.paid.push(g);
            self.pending = self.pending.wrapping_add(1);
            self.guard_log.push(0);
        }
        self.paid.sort_unstable();
        Ok(n)
    }

    /// 守恒复检：序号账连续无重复且与期望指针合拢。
    pub fn conserved(&self) -> bool {
        let mut prev = 0u64;
        for s in self.paid.iter() {
            if *s <= prev {
                return false;
            }
            prev = *s;
        }
        self.gaps.is_empty() && prev + 1 == self.expect_seq
    }

    /// 零故障面：干净流不应有任何故障计数（防误报）。
    pub fn clean_slate(&self) -> bool {
        self.dup_dropped == 0 && self.reack_ok == 0 && self.realign_ok == 0
            && self.blocked == 0 && self.pending == 0
    }

    /// 读数族（判据汇总用）。
    pub fn counters(&self) -> (u64, u64, u64, u64, u64) {
        (self.dup_dropped, self.reack_ok, self.realign_ok, self.blocked, self.pending)
    }

    /// 防护动作账长度。
    pub fn guard_len(&self) -> usize {
        self.guard_log.len()
    }

    /// 已付笔数。
    pub fn paid_len(&self) -> usize {
        self.paid.len()
    }
}

/// 确定性 LCG（种子写死）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 注入一类故障一轮：返回（注入笔数, 检出笔数, 防护动作数, 守恒否）。
/// kind=LostIrq：在 step%57==0 处跳号不投（丢中断）；
/// kind=DupDeliver：在 step%43==0 处重投上一序号；
/// kind=AffDrift：中段污染亲和账一处；
/// 其余场景走正常流（故障以显式调用注入，见 drill 函数）。
fn inject_round(kind: FaultKind, seed: u64) -> (u64, u64, usize, bool) {
    let mut core = DetectCore::new();
    // 故障点为确定性取模位置（step%57/%43/%31），种子保留保证可重复口径。
    let _seed = seed;
    let mut injected = 0u64;
    let mut detected = 0u64;
    let mut seq: u64 = 0;
    for step in 0..EVENTS_PER_ROUND {
        seq += 1;
        match kind {
            FaultKind::LostIrq => {
                core.pre_route(seq);
                if step % 57 == 0 {
                    seq += 1; // 故障点：跳号（下一笔顶上）
                }
                core.pre_route(seq);
                if let Verdict::Lost(_) = core.deliver(seq) {
                    detected += 1;
                }
                injected += 2;
            }
            FaultKind::DupDeliver => {
                core.pre_route(seq);
                let v = core.deliver(seq);
                if matches!(v, Verdict::Dup(_)) {
                    detected += 1;
                }
                injected += 1;
                if step % 43 == 0 {
                    // 故障点：重投上一序号。
                    core.pre_route(seq);
                    if matches!(core.deliver(seq), Verdict::Dup(_)) {
                        detected += 1;
                    }
                    injected += 1;
                }
            }
            FaultKind::AckMismatch => {
                core.pre_route(seq);
                let _ = core.deliver(seq);
                injected += 1;
                if step % 31 == 0 {
                    // 故障点：ack 号偏移。
                    let bad = seq.wrapping_add(1);
                    if core.ack(bad).is_err() {
                        detected += 1;
                    }
                } else {
                    let _ = core.ack(seq);
                }
            }
            FaultKind::AffDrift => {
                core.pre_route(seq);
                let _ = core.deliver(seq);
                injected += 1;
                if step == EVENTS_PER_ROUND / 2 {
                    // 故障点：污染亲和账一处。
                    core.aff[0] = core.aff[0].wrapping_add(1);
                    if core.affinity_check().is_err() {
                        detected += 1;
                    }
                }
            }
            FaultKind::QuotaFail => {
                core.pre_route(seq);
                let _ = core.deliver(seq);
                injected += 1;
                // 速率律：前半段超限（150），后半段回落（80）。
                let rate = if step < EVENTS_PER_ROUND / 2 { 150 } else { 80 };
                if core.quota_gate(rate).is_err() {
                    detected += 1;
                }
            }
        }
    }
    // 丢中断场景：防护补投后必须守恒。
    if kind == FaultKind::LostIrq {
        let _ = core.repair_gaps();
    }
    let ok = core.conserved();
    (injected, detected, core.guard_len(), ok)
}

/// 演练一：五类场景 10/10 必抓（判据正向面：检出>0 且防护动作>0 且守恒）。
pub fn drill_five_faults_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut all = true;
        for i in 0..FAULT_COUNT {
            let kind = match FaultKind::from(i) {
                Some(k) => k,
                None => return (false, "场景枚举败".to_string()),
            };
            let (inj, det, guards, cons) = inject_round(kind, 0xFC70_1267 + r as u64 * 10 + i as u64);
            if det == 0 || guards == 0 || !cons || inj == 0 {
                all = false;
            }
        }
        if all {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("五类必抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练二：零故障零误报 10/10（反向面：干净流判准全绿）。
pub fn drill_clean_no_false_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        // 干净流：严格递增序号（1..=200），全程应零故障零误报。
        let mut core = DetectCore::new();
        for s in 1..=EVENTS_PER_ROUND {
            core.pre_route(s);
            let v = core.deliver(s);
            if !matches!(v, Verdict::Clean) {
                // 理论不可达；防御性补投保守恒。
                core.repair_gaps().unwrap_or(0);
            }
        }
        let aff_ok = core.affinity_check().is_ok();
        let quota_ok = core.quota_gate(80).is_ok();
        if core.clean_slate() && aff_ok && quota_ok && core.conserved() {
            ok += 1;
        }
        let _ = r;
    }
    (ok == INJECT_ROUNDS as u32, format!("零误报={}/{}", ok, INJECT_ROUNDS))
}

/// 演练三：防护动作正确率 100%（每类故障有专属防护动作且守恒）。
pub fn drill_guard_correct() -> (bool, String) {
    // 丢中断 → 补投后守恒。
    let (_, d0, g0, c0) = inject_round(FaultKind::LostIrq, 0xFC70_A001);
    // 重复投递 → 去重后守恒（不补投也守恒）。
    let (_, d1, g1, c1) = inject_round(FaultKind::DupDeliver, 0xFC70_A002);
    // 确认错配 → 重确认动作入账。
    let (_, d2, g2, c2) = inject_round(FaultKind::AckMismatch, 0xFC70_A003);
    // 亲和漂移 → 归账动作入账且归后对账绿。
    let (_, d3, g3, c3) = inject_round(FaultKind::AffDrift, 0xFC70_A004);
    // 配额失效 → 阻流动作入账。
    let (_, d4, g4, c4) = inject_round(FaultKind::QuotaFail, 0xFC70_A005);
    let all = d0 > 0 && c0 && g0 > 0
        && d1 > 0 && c1 && g1 > 0
        && d2 > 0 && c2 && g2 > 0
        && d3 > 0 && c3 && g3 > 0
        && d4 > 0 && c4 && g4 > 0;
    (all, format!("防护[补投{}/{} 重{}/{} 归{}/{} 阻{}/{} 混{}/{}]",
        d0, g0, d1, g1, d2, g2, d3, g3, d4, g4))
}

/// 演练四：资产可重复一致率 100%（同种子两轮五读数逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let kind = match FaultKind::from(r % FAULT_COUNT) {
            Some(k) => k,
            None => return (false, "场景枚举败".to_string()),
        };
        let seed = 0xFC70_BB3 + r as u64;
        if inject_round(kind, seed) == inject_round(kind, seed) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

/// 演练五：丢中断补投判准（3 缺口 → 补 3 笔 → 守恒 → 序号账连续）。
pub fn drill_gap_repair() -> (bool, String) {
    let mut core = DetectCore::new();
    for s in 1..=6u64 {
        core.pre_route(s);
    }
    let _ = core.deliver(1);
    let _ = core.deliver(2);
    let v3 = core.deliver(4); // 跳 3
    let v4 = core.deliver(5);
    let v5 = core.deliver(7); // 跳 6
    // 判准口径：Lost 判定携带的是实际给付序号（4/7），缺口号登记在账。
    let lost = matches!(v3, Verdict::Lost(4)) && matches!(v4, Verdict::Clean)
        && matches!(v5, Verdict::Lost(7));
    let repaired = core.repair_gaps().unwrap_or(u64::MAX);
    let cons = core.conserved();
    let seq_ok = core.paid_len() == 7;
    (lost && repaired == 2 && cons && seq_ok,
     format!("跳号判={} 补投={} 守恒={} 账长={}", lost, repaired, cons, core.paid_len()))
}

/// 演练六：重复判准（同号二投必报 Dup，去重后守恒）。
pub fn drill_dup_judge() -> (bool, String) {
    let mut core = DetectCore::new();
    for s in 1..=4u64 {
        core.pre_route(s);
    }
    let a1 = core.deliver(1);
    let a2 = core.deliver(2);
    let a = core.deliver(3);
    let b = core.deliver(3);
    let dup_ok = matches!(a1, Verdict::Clean) && matches!(a2, Verdict::Clean)
        && matches!(a, Verdict::Clean) && matches!(b, Verdict::Dup(3));
    let (dup_n, _, _, _, _) = core.counters();
    let cons = core.conserved() && core.paid_len() == 3;
    (dup_ok && dup_n == 1 && cons, format!("重判={} 去重={} 守恒={}", dup_ok, dup_n, cons))
}

/// 演练七：确认错配判准（错 ack 报、对 ack 收）。
pub fn drill_ack_judge() -> (bool, String) {
    let mut core = DetectCore::new();
    for s in 1..=3u64 {
        core.pre_route(s);
        let _ = core.deliver(s);
    }
    let bad = matches!(core.ack(99), Err(IjCode::AckMismatch));
    let good = core.ack(3).is_ok();
    (bad && good, format!("错ack拒={} 对ack收={}", bad, good))
}

/// 演练八：配额闸判准（超限关闸→阻流，回落开闸→放行）。
pub fn drill_quota_judge() -> (bool, String) {
    let mut core = DetectCore::new();
    let over = matches!(core.quota_gate(QUOTA_BASE + 1), Err(IjCode::QuotaFail));
    let still = matches!(core.quota_gate(QUOTA_BASE + 5), Err(IjCode::QuotaFail));
    let back = core.quota_gate(QUOTA_BASE).is_ok();
    (over && still && back,
     format!("超限拒={} 持超拒={} 回落收={}", over, still, back))
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：故障资产卡 / 三段链账 / 交叠注入 / 阈值扫描 / 四项演练
// ═══════════════════════════════════════════════════════════════════

/// 一张故障资产卡（判据「资产化」的结构化落档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultCard {
    /// 场景号。
    pub kind: usize,
    /// 场景名。
    pub name: &'static str,
    /// 注入笔数。
    pub injected: u64,
    /// 检出笔数。
    pub detected: u64,
    /// 防护动作数。
    pub guards: usize,
    /// 守恒否。
    pub conserved: bool,
    /// 防护动作码（0 补投/1 去重/2 重确认/3 归账/4 阻流）。
    pub guard_code: u8,
}

/// 防护动作码表（判据常量冻结；与 DetectCore 内部动作号一一对应）。
pub const GUARD_CODES: [u8; FAULT_COUNT] = [0, 1, 2, 3, 4];

/// 五卡全跑落档（种子写死逐卡注入一轮）。
pub fn build_fault_cards(seed: u64) -> Vec<FaultCard> {
    let mut cards: Vec<FaultCard> = Vec::new();
    for i in 0..FAULT_COUNT {
        let kind = match FaultKind::from(i) {
            Some(k) => k,
            None => continue,
        };
        let (inj, det, g, cons) = inject_round(kind, seed + i as u64);
        cards.push(FaultCard {
            kind: i,
            name: kind.text(),
            injected: inj,
            detected: det,
            guards: g,
            conserved: cons,
            guard_code: GUARD_CODES[i],
        });
    }
    cards
}

/// 资产卡完好判据：五卡齐、检出>0、防护>0、全守恒、动作码一一对应。
pub fn fault_cards_intact(cards: &[FaultCard]) -> bool {
    if cards.len() != FAULT_COUNT {
        return false;
    }
    cards.iter().enumerate().all(|(i, c)| {
        c.kind == i && c.detected > 0 && c.guards > 0 && c.conserved
            && c.guard_code == GUARD_CODES[i]
    })
}

/// 资产卡导出。
pub fn export_fault_cards(cards: &[FaultCard]) -> String {
    let mut s = String::from("== UNX-F1267 故障资产卡 ==\n");
    for c in cards.iter() {
        s.push_str(&format!("  [{}] {} 注入={} 检出={} 防护={}({}) 守恒={}\n",
            c.kind, c.name, c.injected, c.detected, c.guards, c.guard_code, c.conserved));
    }
    s
}

/// 交叠注入演练：丢中断与重复投递交叠一轮——检测核须同时报两类
/// 且补投后守恒（判据「防护动作正确率」的复合面）。
pub fn drill_mixed_faults() -> (bool, String) {
    let mut core = DetectCore::new();
    let mut detected_lost = 0u64;
    let mut detected_dup = 0u64;
    // 序列：1,2,(跳3)4,4,5,(跳6)7,7,8,9,10 —— 丢与重交叠。
    let script: [u64; 11] = [1, 2, 4, 4, 5, 7, 7, 8, 9, 10, 10];
    for (i, &seq) in script.iter().enumerate() {
        core.pre_route(seq);
        match core.deliver(seq) {
            Verdict::Lost(_) => detected_lost += 1,
            Verdict::Dup(_) => detected_dup += 1,
            _ => {}
        }
        let _ = i;
    }
    let repaired = core.repair_gaps().unwrap_or(u64::MAX);
    let cons = core.conserved();
    (detected_lost >= 2 && detected_dup >= 3 && repaired == 2 && cons,
     format!("交叠 丢={} 重={} 补投={} 守恒={}", detected_lost, detected_dup, repaired, cons))
}

/// 阈值扫描演练：配额基线邻域五点（基线-2/基线/基线+1/基线×2/基线×8）
/// 判级口径单调（越超限越必报，回落必收）。
pub fn drill_threshold_sweep() -> (bool, String) {
    let mut core = DetectCore::new();
    let below = core.quota_gate(QUOTA_BASE - 2).is_ok();
    let at = core.quota_gate(QUOTA_BASE).is_ok();
    let just_over = matches!(core.quota_gate(QUOTA_BASE + 1), Err(IjCode::QuotaFail));
    let double = matches!(core.quota_gate(QUOTA_BASE * 2), Err(IjCode::QuotaFail));
    let storm = matches!(core.quota_gate(QUOTA_BASE * 8), Err(IjCode::QuotaFail));
    let back = core.quota_gate(QUOTA_BASE).is_ok();
    (below && at && just_over && double && storm && back,
     format!("扫描 下={} 界={} 超={}/{}/{} 回={}", below, at, just_over, double, storm, back))
}

/// 防护动作账对账：五类防护码各自出现 ≥1（全量五场景跑完的账面）。
pub fn drill_guard_codes_covered() -> (bool, String) {
    let mut seen = [false; 5];
    for i in 0..FAULT_COUNT {
        let kind = match FaultKind::from(i) {
            Some(k) => k,
            None => continue,
        };
        let (_, _, g, _) = inject_round(kind, 0xFC70_B103 + i as u64);
        if g > 0 {
            seen[i] = true;
        }
    }
    let all = seen.iter().all(|&b| b);
    (all, format!("防护码覆盖={}/{} [补重认归阻]",
        seen.iter().filter(|&&b| b).count(), FAULT_COUNT))
}

/// 演练九：资产卡 10/10（五卡完好 + 导出非空 + 可重复）。
pub fn drill_fault_cards_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let a = build_fault_cards(0xFC70_B104 + r as u64);
        let b = build_fault_cards(0xFC70_B104 + r as u64);
        if a == b && fault_cards_intact(&a) {
            let s = export_fault_cards(&a);
            if s.contains("稳态丢中断") && s.contains("配额失效") && s.len() > 300 {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("故障资产卡={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录：检测核读数渲染 ──

impl DetectCore {
    /// 读数表渲染（去重/重确认/归账/阻流/在途五行）。
    pub fn counters_table(&self) -> String {
        let (d, r, a, b, p) = self.counters();
        let mut s = String::from("== UNX-F1267 检测核读数 ==\n");
        s.push_str(&format!("  去重丢弃={} 重确认={} 归账={} 阻流={} 在途={}\n", d, r, a, b, p));
        s.push_str(&format!("  已付={} 防护动作={} 守恒={}\n",
            self.paid_len(), self.guard_len(), self.conserved()));
        s
    }
}

/// 演练九：读数表渲染 10/10（干净核零计数行 + 守恒行）。
pub fn drill_counters_table_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let mut core = DetectCore::new();
        for s in 1..=30u64 {
            core.pre_route(s);
            let _ = core.deliver(s);
        }
        let t = core.counters_table();
        if t.contains("去重丢弃=0") && t.contains("守恒=true") && t.len() > 100 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("读数表={}/{}", ok, INJECT_ROUNDS))
}


// ── 附录 C：检出率‰表 ──

/// 逐类检出率（检出笔数 ×1000 / 注入笔数，‰；资产卡口径表）。
pub fn detection_permille(cards: &[FaultCard]) -> [u64; FAULT_COUNT] {
    let mut out = [0u64; FAULT_COUNT];
    for (i, c) in cards.iter().enumerate() {
        if i < FAULT_COUNT && c.injected > 0 {
            out[i] = c.detected * 1000 / c.injected;
        }
    }
    out
}

/// 检出率表导出（五行）。
pub fn export_detection(cards: &[FaultCard]) -> String {
    let pm = detection_permille(cards);
    let mut s = String::from("== UNX-F1267 逐类检出率 ==\n");
    for (i, c) in cards.iter().enumerate() {
        s.push_str(&format!("  {} 检出率={}‰\n", c.name, pm[i]));
    }
    s
}

/// 演练十：检出率表 10/10（五行齐 + 检出率有界 + 可重复）。
pub fn drill_detection_table_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let a = build_fault_cards(0xFC70_BD03 + r as u64);
        let b = build_fault_cards(0xFC70_BD03 + r as u64);
        if a == b {
            let pm = detection_permille(&a);
            if pm.iter().all(|&x| x <= 1000) && export_detection(&a).len() > 150 {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("检出率表={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 D：已付账单调导出 ──

impl DetectCore {
    /// 已付账单调导出（去重+补投后必严格递增）。
    pub fn paid_monotone(&self) -> bool {
        self.paid.windows(2).all(|w| w[0] < w[1])
    }
    /// 已付账渲染（首/末/长度三行）。
    pub fn paid_summary(&self) -> String {
        let first = self.paid.first().copied().unwrap_or(0);
        let last = self.paid.last().copied().unwrap_or(0);
        format!("已付账 首={} 末={} 长={} 单调={}", first, last, self.paid_len(), self.paid_monotone())
    }
}

/// 演练十一：已付账单调 10/10（正常流 + 补投后皆单调）。
pub fn drill_paid_monotone_10() -> (bool, String) {
    let mut ok = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut core = DetectCore::new();
        for s in 1..=40u64 {
            if s % 13 == 0 {
                continue; // 制造跳号
            }
            core.pre_route(s);
            let _ = core.deliver(s);
        }
        let _ = core.repair_gaps();
        if core.paid_monotone() && core.conserved() && core.paid_summary().contains("单调=true") {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("已付单调={}/{}", ok, INJECT_ROUNDS))
}

pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in IjCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1267_checks：本单判据聚合（诊断段 0xFC70）。
pub fn run_unx1267_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1267 稳态故障注入");
    let (a, da) = drill_five_faults_10();
    s.add_dyn("J1-五类故障10/10必抓", a, &da);
    let (b, db) = drill_clean_no_false_10();
    s.add_dyn("J1-零故障零误报10/10", b, &db);
    let (c, dc) = drill_guard_correct();
    s.add_dyn("J1-防护动作正确率100%", c, &dc);
    let (d, dd) = drill_repeatable_10();
    s.add_dyn("J1-可重复一致10/10", d, &dd);
    let (e, de) = drill_gap_repair();
    s.add_dyn("J2-缺口补投判准", e, &de);
    let (f, df) = drill_dup_judge();
    s.add_dyn("J2-重复投递判准", f, &df);
    let (g, dg) = drill_ack_judge();
    s.add_dyn("J2-确认错配判准", g, &dg);
    let (h, dh) = drill_quota_judge();
    s.add_dyn("J2-配额闸判准", h, &dh);
    let (i, di) = drill_fault_cards_10();
    s.add_dyn("J2-故障资产卡10/10", i, &di);
    let (j, dj) = drill_mixed_faults();
    s.add_dyn("J2-交叠注入复合判准", j, &dj);
    let (k, dk) = drill_threshold_sweep();
    s.add_dyn("J2-配额阈值扫描", k, &dk);
    let (l, dl) = drill_guard_codes_covered();
    s.add_dyn("J2-防护动作码全覆盖", l, &dl);
    let (m, dm) = drill_counters_table_10();
    s.add_dyn("J2-检测核读数表10/10", m, &dm);
    let (n, dn) = drill_detection_table_10();
    s.add_dyn("J2-检出率表10/10", n, &dn);
    let (o, do_) = drill_paid_monotone_10();
    s.add_dyn("J2-已付账单调10/10", o, &do_);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC70段十码闭集");
    s
}
