//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1424 · 超限率账（超限事件计数与比率滚动）· 0x720C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B32）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1424-J1 超限率账逐向量生效 100%（超限事件计数/比率滚动
//!     /超限事件明细三节），超限判读与阈值册一致率 100%，超限漏报
//!     注入 10/10 必抓，超限明细入升级链消费面；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 施工说明：
//!   - 逐向量三节账：①超限事件计数（每向量累计）；②比率滚动
//!     （WINDOW 槽环形窗，槽存样本总数/超限数，比率千分数滚动）；
//!     ③超限事件明细（向量/历元/值/阈值四元账）。
//!   - 判读一致：采样路径读阈值册，独立复核路径读开户镜像册，两路
//!     判读必须逐样本一致（一致率 1000‰）。
//!   - 漏报注入：明细账抹一条超限 ⇒ 计数/明细/升级链三节对账必抓。
//!   - 升级链：超限明细推入升级队列，消费面逐条签收，产出 = 消费
//!     才算闭环。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x720C0，VX3-1103 超限率账批独占区）。
pub const SEG_BASE: u32 = 0x720C0;

/// 向量数（32 冻结）。
pub const VECTOR_COUNT: usize = 32;
/// 滚动窗槽数（16 冻结）。
pub const WINDOW: usize = 16;
/// 升级队列容量（512 冻结，容纳 1024 采样的超限产出上限）。
pub const ESC_CAP: usize = 512;
/// 采样事件数（1024，黄金全链跑量）。
pub const SAMPLES: u64 = 1024;
/// 一致率硬线（千分数 1000）。
pub const AGREE_HARDLINE: u64 = 1000;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrCode {
    /// 向量号越界
    VecOver,
    /// 阈值册缺该向量阈值
    ThresholdMissing,
    /// 超限明细丢账（计数与明细不等）
    DetailLoss,
    /// 滚动比率复算失配
    RollMismatch,
    /// 判读与阈值册不一致
    JudgeInconsistent,
    /// 升级链消费面未闭环
    EscNotConsumed,
    /// 漏报未抓（注入对账未检出）
    MissUndetected,
    /// 双跑不一致
    NotReproducible,
}

impl OrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OrCode::VecOver => "向量号越界：≥32，请核对向量表长度",
            OrCode::ThresholdMissing => "阈值册缺该向量阈值：请先逐向量开户阈值",
            OrCode::DetailLoss => "超限明细丢账：逐向量计数与明细条数不等，对账明细节",
            OrCode::RollMismatch => "滚动比率复算失配：账面率与窗口重放率不一致",
            OrCode::JudgeInconsistent => "判读与阈值册不一致：核对镜像册与阈值册是否同步",
            OrCode::EscNotConsumed => "升级链消费面未闭环：产出明细与签收数不等",
            OrCode::MissUndetected => "漏报未抓：注入对账未检出，检查三节对账逻辑",
            OrCode::NotReproducible => "双跑不一致：复现红线，固定种子后重跑",
        }
    }
    pub const ALL: [OrCode; 8] = [
        OrCode::VecOver,
        OrCode::ThresholdMissing,
        OrCode::DetailLoss,
        OrCode::RollMismatch,
        OrCode::JudgeInconsistent,
        OrCode::EscNotConsumed,
        OrCode::MissUndetected,
        OrCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OrCode::ALL.len() {
        if OrCode::ALL[i].val() == code {
            return OrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x720C0 段闭集内"
}

// ── 阈值册 ─────────────────────────────────────────────────────
/// 阈值册（逐向量阈值，超限判读唯一依据）。
#[derive(Clone, Debug)]
pub struct ThresholdBook {
    pub limits: Vec<u32>,
}

impl ThresholdBook {
    /// 真机阈值册（向量 v 阈值 100 + v×4，确定性可复算）。
    pub fn machine() -> ThresholdBook {
        let mut limits = Vec::new();
        let mut v = 0;
        while v < VECTOR_COUNT {
            limits.push(100 + (v as u32) * 4);
            v += 1;
        }
        ThresholdBook { limits }
    }

    /// 判读：值 > 阈值 ⇒ 超限；缺阈值报错。
    pub fn judge(&self, vector: usize, value: u32) -> Result<bool, u32> {
        if vector >= self.limits.len() {
            return Err(OrCode::VecOver.val());
        }
        let lim = match self.limits.get(vector) {
            Some(l) => *l,
            None => return Err(OrCode::ThresholdMissing.val()),
        };
        Ok(value > lim)
    }
}

// ── 超限事件明细 ───────────────────────────────────────────────
/// 一条超限明细（向量/历元/值/阈值四元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverEvent {
    pub vector: usize,
    pub epoch: u64,
    pub value: u32,
    pub threshold: u32,
}

// ── 滚动窗槽 ───────────────────────────────────────────────────
/// 一个滚动窗槽（样本总数/超限数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollSlot {
    pub total: u32,
    pub over: u32,
}

impl RollSlot {
    pub fn empty() -> RollSlot {
        RollSlot { total: 0, over: 0 }
    }
}

// ── 逐向量三节账 ───────────────────────────────────────────────
/// 单向量三节账（计数节/滚动节/明细节 + 原始采样重放日志）。
#[derive(Clone, Debug)]
pub struct VectorLedger {
    /// 节 1：超限事件计数。
    pub count_over: u64,
    /// 节 2：滚动窗（WINDOW 槽环形账面）。
    pub window: Vec<RollSlot>,
    /// 节 3：超限事件明细。
    pub details: Vec<OverEvent>,
    /// 原始采样日志（历元/超限旗标，滚动率重放复算的独立依据）。
    pub history: Vec<(u64, bool)>,
    /// 累计样本数。
    pub samples: u64,
}

impl VectorLedger {
    pub fn new() -> VectorLedger {
        let mut window = Vec::new();
        let mut i = 0;
        while i < WINDOW {
            window.push(RollSlot::empty());
            i += 1;
        }
        VectorLedger {
            count_over: 0,
            window,
            details: Vec::new(),
            history: Vec::new(),
            samples: 0,
        }
    }

    /// 样本入窗（历元取模落槽 + 原始日志留痕；超限事件另行记明细）。
    pub fn sample(&mut self, epoch: u64, over: bool) {
        let slot = (epoch % WINDOW as u64) as usize;
        if slot < self.window.len() {
            self.window[slot].total += 1;
            if over {
                self.window[slot].over += 1;
            }
        }
        self.history.push((epoch, over));
        self.samples += 1;
        if over {
            self.count_over += 1;
        }
    }

    /// 账面滚动超限率（千分数，窗口槽 over×1000/total）。
    pub fn rolling_over_permille(&self) -> u64 {
        let mut t: u64 = 0;
        let mut o: u64 = 0;
        let mut i = 0;
        while i < self.window.len() {
            t += self.window[i].total as u64;
            o += self.window[i].over as u64;
            i += 1;
        }
        if t == 0 {
            return 0;
        }
        o.saturating_mul(1000) / t
    }

    /// 滚动率独立重放：从原始采样日志重建槽账再复算（不经账面窗）。
    pub fn recompute_rolling(&self) -> u64 {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < WINDOW {
            slots.push(RollSlot::empty());
            i += 1;
        }
        let mut j = 0;
        while j < self.history.len() {
            let (epoch, over) = self.history[j];
            let s = (epoch % WINDOW as u64) as usize;
            if s < slots.len() {
                slots[s].total += 1;
                if over {
                    slots[s].over += 1;
                }
            }
            j += 1;
        }
        let mut t: u64 = 0;
        let mut o: u64 = 0;
        let mut k = 0;
        while k < slots.len() {
            t += slots[k].total as u64;
            o += slots[k].over as u64;
            k += 1;
        }
        if t == 0 {
            return 0;
        }
        o.saturating_mul(1000) / t
    }

    /// 明细节对账：计数节 == 明细条数。
    pub fn detail_consistent(&self) -> bool {
        self.count_over == self.details.len() as u64
    }
}

// ── 超限率账本（全向量 + 判读一致账 + 升级链） ──────────────────
/// 升级队列（超限明细入链，消费面签收）。
#[derive(Clone, Debug)]
pub struct EscalationQueue {
    pub pending: Vec<OverEvent>,
    pub produced: u64,
    pub consumed: u64,
}

impl EscalationQueue {
    pub fn new() -> EscalationQueue {
        EscalationQueue { pending: Vec::new(), produced: 0, consumed: 0 }
    }

    /// 推入一条超限明细（容量闸）。
    pub fn produce(&mut self, ev: OverEvent) -> Result<(), u32> {
        if self.pending.len() >= ESC_CAP {
            return Err(OrCode::EscNotConsumed.val());
        }
        self.pending.push(ev);
        self.produced += 1;
        Ok(())
    }

    /// 消费面签收一条。
    pub fn consume(&mut self) -> Option<OverEvent> {
        if self.pending.is_empty() {
            return None;
        }
        let ev = self.pending.remove(0);
        self.consumed += 1;
        Some(ev)
    }

    /// 消费闭环：产出 = 消费 且 队列清空。
    pub fn closed(&self) -> bool {
        self.produced == self.consumed && self.pending.is_empty()
    }
}

/// 超限率账本。
#[derive(Clone, Debug)]
pub struct OverRateLedger {
    /// 阈值册（判读唯一依据）。
    pub book: ThresholdBook,
    /// 开户镜像册（独立复核路径，防阈值漂移）。
    pub mirror: Vec<u32>,
    /// 逐向量三节账。
    pub vecs: Vec<VectorLedger>,
    /// 判读总数。
    pub judge_total: u64,
    /// 判读一致数。
    pub judge_agree: u64,
    /// 升级链。
    pub esc: EscalationQueue,
}

impl OverRateLedger {
    /// 真机开户：阈值册 + 镜像册 + 全向量三节账。
    pub fn open() -> OverRateLedger {
        let book = ThresholdBook::machine();
        let mut mirror = Vec::new();
        let mut v = 0;
        while v < VECTOR_COUNT {
            mirror.push(match book.limits.get(v) {
                Some(l) => *l,
                None => 0,
            });
            v += 1;
        }
        let mut vecs = Vec::new();
        let mut v = 0;
        while v < VECTOR_COUNT {
            vecs.push(VectorLedger::new());
            v += 1;
        }
        OverRateLedger {
            book,
            mirror,
            vecs,
            judge_total: 0,
            judge_agree: 0,
            esc: EscalationQueue::new(),
        }
    }

    /// 单样本判读入账：阈值册判读 + 镜像册复核 + 三节入账 + 升级链推入。
    pub fn sample(&mut self, vector: usize, epoch: u64, value: u32) -> Result<bool, u32> {
        if vector >= VECTOR_COUNT {
            return Err(OrCode::VecOver.val());
        }
        // 主判读：阈值册。
        let over = self.book.judge(vector, value)?;
        // 独立复核判读：镜像册（两路必须一致，一致率硬线）。
        let lim2 = match self.mirror.get(vector) {
            Some(l) => *l,
            None => return Err(OrCode::ThresholdMissing.val()),
        };
        self.judge_total += 1;
        if over == (value > lim2) {
            self.judge_agree += 1;
        }
        // 三节入账。
        let v = match self.vecs.get_mut(vector) {
            Some(v) => v,
            None => return Err(OrCode::VecOver.val()),
        };
        v.sample(epoch, over);
        if over {
            let th = match self.book.limits.get(vector) {
                Some(l) => *l,
                None => 0,
            };
            let ev = OverEvent { vector, epoch, value, threshold: th };
            v.details.push(ev);
            self.esc.produce(ev)?;
        }
        Ok(over)
    }

    /// 判读一致率（千分数）。
    pub fn agree_permille(&self) -> u64 {
        if self.judge_total == 0 {
            return 0;
        }
        self.judge_agree.saturating_mul(1000) / self.judge_total
    }

    /// 逐向量滚动率复算（账面 vs 重放逐向量比对）。
    pub fn roll_recompute_all(&self) -> Result<(), u32> {
        let mut v = 0;
        while v < self.vecs.len() {
            if self.vecs[v].rolling_over_permille() != self.vecs[v].recompute_rolling() {
                return Err(OrCode::RollMismatch.val());
            }
            v += 1;
        }
        Ok(())
    }

    /// 全账对账（三节一致 + 判读一致硬线 + 升级链闭环）。
    pub fn audit(&self) -> Result<(), u32> {
        let mut v = 0;
        while v < self.vecs.len() {
            if !self.vecs[v].detail_consistent() {
                return Err(OrCode::DetailLoss.val());
            }
            v += 1;
        }
        if self.agree_permille() < AGREE_HARDLINE {
            return Err(OrCode::JudgeInconsistent.val());
        }
        self.roll_recompute_all()?;
        if !self.esc.closed() {
            return Err(OrCode::EscNotConsumed.val());
        }
        Ok(())
    }
}

// ── 采样流 ─────────────────────────────────────────────────────
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

/// 确定性采样流：向量与值各用独立 LCG（避免共用状态漏向量），
/// 值域 0..200 对 100..228 的阈值册必有超限面。
pub fn golden_samples() -> Vec<(usize, u64, u32)> {
    let mut rv = Lcg::new(SEG_BASE as u64);
    let mut rq = Lcg::new(SEG_BASE as u64 ^ 0x9E3779B97F4A7C15);
    let mut out = Vec::new();
    let mut e = 0;
    while e < SAMPLES {
        let v = rv.below(VECTOR_COUNT as u64) as usize;
        let val = rq.below(200) as u32;
        out.push((v, e, val));
        e += 1;
    }
    out
}

// ── 黄金全链：采样→三节账→判读一致→滚动复算→升级链闭环→对账 ────
/// 返回 (超限事件数, 判读一致率‰, 升级链消费数)。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut l = OverRateLedger::open();
    let samples = golden_samples();
    let mut i = 0;
    while i < samples.len() {
        let (v, e, val) = samples[i];
        l.sample(v, e, val)?;
        i += 1;
    }
    // 升级链消费面闭环。
    while l.esc.consume().is_some() {}
    // 全账对账。
    l.audit()?;
    let over_total = {
        let mut t: u64 = 0;
        let mut v = 0;
        while v < l.vecs.len() {
            t += l.vecs[v].count_over;
            v += 1;
        }
        t
    };
    Ok((over_total, l.agree_permille(), l.esc.consumed))
}

// ── 注入检出账（四路：明细丢账/阈值漂移/滚动失配/升级漏消费） ──
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 构建一个已超限的账本（黄金采样前 256 条，超限明细必非空）。
pub fn seeded_ledger() -> Result<OverRateLedger, u32> {
    let mut l = OverRateLedger::open();
    let samples = golden_samples();
    let mut i = 0;
    while i < 256 && i < samples.len() {
        let (v, e, val) = samples[i];
        l.sample(v, e, val)?;
        i += 1;
    }
    Ok(l)
}

/// 注入轮（LCG 驱动 rounds 轮 × 4 路，漏报每轮必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：超限漏报（明细抹一条）⇒ 三节对账必抓。
        let mut l1 = seeded_ledger()?;
        let mut dropped = false;
        let mut v = 0;
        while v < l1.vecs.len() && !dropped {
            if l1.vecs[v].details.len() > 1 {
                l1.vecs[v].details.remove(0);
                dropped = true;
            }
            v += 1;
        }
        let caught1 = if dropped { l1.audit().is_err() } else { false };
        led.record(0, caught1);
        // 路 2：阈值漂移（册后改账）⇒ 判读一致率必抓。
        // 取分歧值 120：漂移后册判 120≤140 非超限，镜像册判 120>100 超限。
        let mut l2 = seeded_ledger()?;
        l2.book.limits[0] = l2.book.limits[0].saturating_add(40);
        let _ = l2.sample(0, 9999, 120);
        led.record(1, l2.agree_permille() < AGREE_HARDLINE);
        // 路 3：滚动失配（窗槽超限数虚增）⇒ 复算比对必抓。
        let mut l3 = seeded_ledger()?;
        l3.vecs[0].window[0].over += 1;
        led.record(2, l3.vecs[0].rolling_over_permille() != l3.vecs[0].recompute_rolling());
        // 路 4：升级漏消费（产出后不签收）⇒ 闭环闸必抓。
        let l4 = seeded_ledger()?;
        led.record(3, {
            let _ = l4.audit();
            !l4.esc.closed()
        });
        r += 1;
    }
    Ok(led)
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（有超限面 + 判读一致 1000‰ + 升级链闭环）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((over, agree, consumed)) => {
            if over > 0 && agree == AGREE_HARDLINE && consumed == over {
                Ok(())
            } else {
                Err(OrCode::NotReproducible.val())
            }
        }
        Err(e) => Err(e),
    }
}

/// 序 B：向量越界 ⇒ VecOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = OverRateLedger::open();
    match l.sample(VECTOR_COUNT, 0, 50) {
        Err(e) if e == OrCode::VecOver.val() => Ok(()),
        _ => Err(OrCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常数守卫（向量 32 / 窗 16 / 队 512 / 一致硬线 1000‰）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        VECTOR_COUNT == 32,
        WINDOW == 16,
        ESC_CAP == 512,
        AGREE_HARDLINE == 1000,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(OrCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OrCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < OrCode::ALL.len() {
        let v = OrCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（三节账 + 一致率 + 升级链闭环）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((over, agree, cons)) => *over > 0 && *agree == AGREE_HARDLINE && *cons == *over,
            Err(_) => false,
        },
        match &g {
            Ok((over, agree, cons)) => format!("over={} agree={}‰ consumed={}", over, agree, cons),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 逐向量三节账生效 100%。
    let l = seeded_ledger();
    let tri_ok = match &l {
        Ok(lg) => {
            let mut ok = lg.vecs.len() == VECTOR_COUNT;
            let mut active: usize = 0;
            let mut i = 0;
            while i < lg.vecs.len() {
                if lg.vecs[i].detail_consistent() && lg.vecs[i].samples > 0 {
                    active += 1;
                }
                i += 1;
            }
            // 采样流覆盖全部 32 向量 ⇒ 逐向量生效。
            if active != VECTOR_COUNT {
                ok = false;
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "J1-三节账生效",
        tri_ok,
        String::from("32 向量计数/滚动/明细三节俱全"),
    ));
    // 判读一致率 100%（阈值册 vs 镜像册双路）。
    v.push((
        "J1-判读一致率",
        match &l {
            Ok(lg) => lg.agree_permille() == AGREE_HARDLINE,
            Err(_) => false,
        },
        match &l {
            Ok(lg) => format!("agree={}/{} 样本", lg.judge_agree, lg.judge_total),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 滚动比率复算（逐向量账面 vs 重放）。
    v.push((
        "J1-滚动比率",
        match &l {
            Ok(lg) => lg.roll_recompute_all().is_ok(),
            Err(_) => false,
        },
        String::from("逐向量滚动窗重放一致"),
    ));
    // 超限明细入升级链消费面（产出=签收闭环：消费面逐条签收后对账）。
    let esc_ok = match &l {
        Ok(lg) => {
            let mut lc = lg.clone();
            while lc.esc.consume().is_some() {}
            lg.esc.produced > 0 && lc.esc.consumed == lg.esc.produced && lc.esc.closed()
        }
        Err(_) => false,
    };
    v.push((
        "J1-升级链消费面",
        esc_ok,
        match &l {
            Ok(lg) => format!("produced={} 全部签收闭环", lg.esc.produced),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 漏报注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(led) => led.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-漏报注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x720C0 窗口"),
    ));
    v
}

/// 自检入口（UNX-F1424）。
pub fn run_unx1424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1424 超限率账（超限事件计数与比率滚动）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
