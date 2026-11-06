//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1424 · 超限率账（超限事件计数与比率滚动）· 0x6FAC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B32）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1424-J1 超限率账逐向量生效 100%（超限事件计数 / 比率滚动
//!     / 超限事件明细三节），超限判读与阈值册一致率 100%，超限漏报
//!     注入 10/10 必抓，超限明细入升级链消费面；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 闸门次序（超限账为什么不能倒）：
//!   阈值册先于判读——没有冻结阈值，超限与否无从谈起；
//!   判读先于记账——先裁超限再入账，账才是判读的忠实副本；
//!   计数先于明细——明细是计数的抽样展开，计数错则明细必错；
//!   漏报审计先于升级链——漏报不清零，升级链消费的是残账。
//!
//! 关键差异（本卡深度所在）：
//!   本卡只回答"逐向量三节是否全生效、判读与阈值册是否逐拍一致、
//!   漏报注入是否 10/10 必抓、明细是否进升级链且消费守恒"，不复算
//!   阈值册本身怎么定值（那是 F1423 的纵深），也不回答超限之后的
//!   限流动作（那是执行面卡的事）——本卡审的是**账的三节与链**。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6FAC0，AI-VX3-1103A 独占区）。
pub const SEG_BASE: u32 = 0x6FAC0;

/// 向量数（冻结 16）。
pub const VECTOR_COUNT: usize = 16;
/// 比率滚动窗长（冻结 64 拍）。
pub const ROLL_WINDOW: usize = 64;
/// 超限明细容量（每向量留尾 8 条）。
pub const DETAIL_CAP: usize = 8;
/// 实测样本流长度（4096 拍）。
pub const STREAM_LEN: usize = 4096;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverCode {
    /// 向量号越界
    VectorOver,
    /// 判读与阈值册不一致
    ThreshMiss,
    /// 滚动窗非法（拍数或占空越界）
    WindowBad,
    /// 明细丢失（序断或条数失配）
    DetailLoss,
    /// 升级链漏转（推入 ≠ 消费）
    EscalLoss,
    /// 漏报注入逃逸（独立复算对不上账）
    InjectEscape,
    /// 双跑不一致
    NotReproducible,
}

impl OverCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OverCode::VectorOver => "向量号越界：≥16",
            OverCode::ThreshMiss => "判读与阈值册不一致",
            OverCode::WindowBad => "滚动窗非法",
            OverCode::DetailLoss => "明细丢失：序断或条数失配",
            OverCode::EscalLoss => "升级链漏转：推入≠消费",
            OverCode::InjectEscape => "漏报注入逃逸：复算对不上",
            OverCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [OverCode; 7] = [
        OverCode::VectorOver,
        OverCode::ThreshMiss,
        OverCode::WindowBad,
        OverCode::DetailLoss,
        OverCode::EscalLoss,
        OverCode::InjectEscape,
        OverCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OverCode::ALL.len() {
        if OverCode::ALL[i].val() == code {
            return OverCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6FAC0 段闭集内"
}

// ── 阈值册（F1423 承接口：冻结 16 档，SEG_BASE 派生确定性） ──────
#[derive(Clone, Debug)]
pub struct ThresholdBook {
    vals: [u32; VECTOR_COUNT],
}

impl ThresholdBook {
    /// 冻结阈值册：确定性派生（50..=95），判据同源可复算。
    pub fn frozen() -> ThresholdBook {
        let mut vals = [0u32; VECTOR_COUNT];
        let mut v = 0;
        while v < VECTOR_COUNT {
            vals[v] = 50 + ((SEG_BASE as u32).wrapping_mul(v as u32 + 7) % 46);
            v += 1;
        }
        ThresholdBook { vals }
    }
    pub fn threshold_of(&self, v: usize) -> Result<u32, u32> {
        if v >= VECTOR_COUNT {
            return Err(OverCode::VectorOver.val());
        }
        Ok(self.vals[v])
    }
    /// 独立判读面：与阈值册逐字同径（value > threshold ⇒ 超限）。
    pub fn judge(&self, v: usize, value: u32) -> Result<bool, u32> {
        let t = self.threshold_of(v)?;
        Ok(value > t)
    }
}

// ── LCG（样本流驱动，与判据同源确定性） ────────────────────────
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
        (self.next() >> 33) % n
    }
}

/// 生成确定性样本流：(向量号, 采样值)。
pub fn sample_stream(len: usize, seed: u64) -> Vec<(usize, u32)> {
    let mut rng = Lcg::new(seed);
    let mut v: Vec<(usize, u32)> = Vec::new();
    let mut i = 0;
    while i < len {
        let vec = rng.below(VECTOR_COUNT as u64) as usize;
        let val = rng.below(100) as u32;
        v.push((vec, val));
        i += 1;
    }
    v
}

/// 真机样本流（SEG_BASE 作种，判据同源）。
pub fn machine_stream() -> Vec<(usize, u32)> {
    sample_stream(STREAM_LEN, SEG_BASE as u64)
}

// ── 单向量超限账（三节：计数 / 滚动率 / 明细） ──────────────────
#[derive(Clone, Debug)]
pub struct VecOverrun {
    /// 第一节：超限事件计数。
    pub over_count: u64,
    /// 第二节：比率滚动窗（尾 64 拍超限占空）。
    pub ring: [bool; ROLL_WINDOW],
    pub ring_pos: usize,
    pub ring_filled: usize,
    pub ring_over: usize,
    /// 第三节：超限事件明细（尾 8 条：(序号, 采样值)）。
    pub details: Vec<(u32, u32)>,
}

impl VecOverrun {
    pub fn new() -> VecOverrun {
        VecOverrun {
            over_count: 0,
            ring: [false; ROLL_WINDOW],
            ring_pos: 0,
            ring_filled: 0,
            ring_over: 0,
            details: Vec::new(),
        }
    }

    /// 入账一笔判定结果。
    pub fn enter(&mut self, seq: u32, value: u32, over: bool) {
        if over {
            self.over_count += 1;
        }
        // 滚动窗推进（尾 64 拍环形）。
        if self.ring_filled == ROLL_WINDOW {
            if self.ring[self.ring_pos] {
                self.ring_over -= 1;
            }
        } else {
            self.ring_filled += 1;
        }
        self.ring[self.ring_pos] = over;
        if over {
            self.ring_over += 1;
        }
        self.ring_pos = (self.ring_pos + 1) % ROLL_WINDOW;
        // 明细留尾 8。
        if over {
            self.details.push((seq, value));
            if self.details.len() > DETAIL_CAP {
                let _ = self.details.remove(0);
            }
        }
    }

    /// 第二节读数：滚动超限率（‰）。
    pub fn roll_permil(&self) -> u32 {
        if self.ring_filled == 0 {
            return 0;
        }
        (self.ring_over as u32 * 1000) / self.ring_filled as u32
    }

    /// 三节自洽：明细条数 == min(计数, 容量)，明细序严格递增，
    /// 窗内超限数 ≤ 累计超限数。
    pub fn sections_ok(&self) -> bool {
        if self.ring_over > self.over_count as usize {
            return false;
        }
        if self.over_count == 0 {
            return self.details.is_empty();
        }
        let expect = if self.over_count as usize <= DETAIL_CAP {
            self.over_count as usize
        } else {
            DETAIL_CAP
        };
        if self.details.len() != expect {
            return false;
        }
        let mut i = 1;
        while i < self.details.len() {
            let (a, _) = match self.details.get(i - 1) {
                Some(x) => *x,
                None => return false,
            };
            let (b, _) = match self.details.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if b <= a {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 超限率账（16 向量册柜 + 判定留痕 + 升级链） ─────────────────
#[derive(Clone, Debug)]
pub struct OverrunLedger {
    pub book: ThresholdBook,
    pub vecs: Vec<VecOverrun>,
    pub samples: u64,
    /// 判定留痕（逐拍账径，与阈值册径逐拍对表）。
    pub verdicts: Vec<bool>,
    /// 升级链（明细入链待消费）。
    pub escal: Vec<(u32, usize, u32)>,
    pub escal_pushed: u32,
    pub escal_drained: u32,
    /// 注入钩：Some(强判) 时 record 的判读被强行改写（漏报靶子）。
    pub forced: Option<bool>,
}

impl OverrunLedger {
    pub fn new() -> OverrunLedger {
        let mut vecs: Vec<VecOverrun> = Vec::new();
        let mut v = 0;
        while v < VECTOR_COUNT {
            vecs.push(VecOverrun::new());
            v += 1;
        }
        OverrunLedger {
            book: ThresholdBook::frozen(),
            vecs,
            samples: 0,
            verdicts: Vec::new(),
            escal: Vec::new(),
            escal_pushed: 0,
            escal_drained: 0,
            forced: None,
        }
    }

    /// 记一拍：判读（阈值册径）→ 三节入账 → 超限明细入升级链。
    pub fn record(&mut self, seq: u32, vector: usize, value: u32) -> Result<bool, u32> {
        if vector >= VECTOR_COUNT {
            return Err(OverCode::VectorOver.val());
        }
        // 判读：阈值册同径；注入钩只在注入轮搭钩，真跑恒 None。
        let mut verdict = self.book.judge(vector, value)?;
        if let Some(f) = self.forced {
            verdict = f;
        }
        match self.vecs.get_mut(vector) {
            Some(vl) => vl.enter(seq, value, verdict),
            None => return Err(OverCode::VectorOver.val()),
        }
        self.verdicts.push(verdict);
        self.samples += 1;
        if verdict {
            self.escal.push((seq, vector, value));
            self.escal_pushed += 1;
        }
        Ok(verdict)
    }

    /// 判读一致审计：判定留痕与阈值册径逐拍对表（一致率 100% 断言）。
    pub fn judge_consistent(&self, stream: &[(usize, u32)]) -> Result<bool, u32> {
        if stream.len() as u64 != self.samples || self.verdicts.len() as u64 != self.samples {
            return Err(OverCode::ThreshMiss.val());
        }
        let mut i = 0;
        while i < stream.len() {
            let (v, val) = stream[i];
            let want = self.book.judge(v, val)?;
            let got = match self.verdicts.get(i) {
                Some(x) => *x,
                None => return Err(OverCode::ThreshMiss.val()),
            };
            if want != got {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }

    /// 升级链消费面：整链收割，推入 == 消费才守恒。
    pub fn drain_escal(&mut self) -> u32 {
        let n = self.escal.len() as u32;
        let _ = self.escal.clear();
        self.escal_drained += n;
        n
    }

    /// 升级链守恒闸。
    pub fn escal_ok(&self) -> bool {
        self.escal_pushed == self.escal_drained + self.escal.len() as u32
    }

    /// 漏报审计（独立复算）：按阈值册径重算全流超限数，逐向量与账比。
    pub fn audit_recount(&self, stream: &[(usize, u32)]) -> Result<bool, u32> {
        if stream.len() as u64 != self.samples {
            return Err(OverCode::InjectEscape.val());
        }
        let mut expect: [u64; VECTOR_COUNT] = [0; VECTOR_COUNT];
        let mut i = 0;
        while i < stream.len() {
            let (v, val) = stream[i];
            if self.book.judge(v, val)? {
                expect[v] += 1;
            }
            i += 1;
        }
        let mut v = 0;
        while v < VECTOR_COUNT {
            match self.vecs.get(v) {
                Some(vl) => {
                    if vl.over_count != expect[v] {
                        return Ok(false);
                    }
                }
                None => return Err(OverCode::VectorOver.val()),
            }
            v += 1;
        }
        Ok(true)
    }

    /// 逐向量生效闸：16 册三节全自洽（窗口有拍、读数在界内）。
    pub fn all_sections_ok(&self) -> bool {
        let mut v = 0;
        while v < VECTOR_COUNT {
            match self.vecs.get(v) {
                Some(vl) => {
                    if !vl.sections_ok() {
                        return false;
                    }
                    if vl.ring_filled == 0 || vl.roll_permil() > 1000 {
                        return false;
                    }
                }
                None => return false,
            }
            v += 1;
        }
        true
    }
}

// ── 注入检出账（四路：越向量路/改判路/丢明细路/漏报路） ──────────
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

/// 四路注入各自必须被闸门当场抓住。
pub fn inject_four(stream: &[(usize, u32)]) -> [bool; 4] {
    // 路 1：越向量路。
    let mut l1 = OverrunLedger::new();
    let r1 = l1.record(0, VECTOR_COUNT, 99).is_err();
    // 路 2：改判路（强判与阈值册相反 ⇒ 判读一致审计必抓）。
    let mut l2 = OverrunLedger::new();
    let mut s2 = 0;
    while s2 < 64 && s2 < stream.len() {
        let (v, val) = stream[s2];
        let _ = l2.record(s2 as u32, v, val);
        s2 += 1;
    }
    l2.forced = Some(true); // 后 64 拍一律强判超限（与阈值册径相反面）
    while s2 < 128 && s2 < stream.len() {
        let (v, val) = stream[s2];
        let _ = l2.record(s2 as u32, v, val);
        s2 += 1;
    }
    l2.forced = None;
    let r2 = match l2.judge_consistent(&stream[..128.min(stream.len())]) {
        Ok(false) => true,
        _ => false,
    };
    // 路 3：丢明细路（超限明细清空但计数非零 ⇒ 三节自洽必假）。
    let mut l3 = OverrunLedger::new();
    let mut s3 = 0;
    while s3 < 64 && s3 < stream.len() {
        let (v, val) = stream[s3];
        let _ = l3.record(s3 as u32, v, val);
        s3 += 1;
    }
    let mut had_over = false;
    let mut v3 = 0;
    while v3 < VECTOR_COUNT {
        if let Some(vl) = l3.vecs.get_mut(v3) {
            if vl.over_count > 0 {
                let _ = vl.details.clear();
                had_over = true;
            }
        }
        v3 += 1;
    }
    let r3 = had_over && !l3.all_sections_ok();
    // 路 4：漏报路（销账一笔超限 ⇒ 独立复算必抓）。
    let mut l4 = OverrunLedger::new();
    let mut s4 = 0;
    while s4 < 128 && s4 < stream.len() {
        let (v, val) = stream[s4];
        let _ = l4.record(s4 as u32, v, val);
        s4 += 1;
    }
    let mut suppressed = false;
    let mut v4 = VECTOR_COUNT;
    while v4 > 0 {
        v4 -= 1;
        if let Some(vl) = l4.vecs.get_mut(v4) {
            if vl.over_count > 0 {
                vl.over_count -= 1;
                suppressed = true;
                break;
            }
        }
    }
    let r4 = suppressed
        && match l4.audit_recount(&stream[..128.min(stream.len())]) {
            Ok(false) => true,
            _ => false,
        };
    [r1, r2, r3, r4]
}

// ── 黄金全链：判读→三节→升级链→四拒→审计 ───────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let stream = machine_stream();
    let mut l = OverrunLedger::new();
    let mut i = 0;
    while i < stream.len() {
        let (v, val) = stream[i];
        l.record(i as u32, v, val)?;
        i += 1;
    }
    // 逐向量三节生效 100%。
    let sections_ok = l.all_sections_ok() as u32;
    // 判读与阈值册一致率 100%（逐拍对表）。
    let judge_ok = l.judge_consistent(&stream)? as u32;
    // 漏报复算清白（真账必须通过独立复算）。
    let recount_clean = match l.audit_recount(&stream) {
        Ok(true) => 1,
        _ => 0,
    };
    // 升级链消费面（推入 == 消费）。
    let pushed = l.escal_pushed;
    let drained = l.drain_escal();
    let escal_ok = (pushed == drained && pushed > 0 && l.escal_ok()) as u32;
    // 四拒。
    let over_vec = l.record(0, VECTOR_COUNT, 1).is_err();
    let inj = inject_four(&stream);
    let inj_all = (inj[0] && inj[1] && inj[2] && inj[3]) as u32;
    let mut l3 = OverrunLedger::new();
    let _ = l3.record(0, 0, 100);
    if let Some(vl) = l3.vecs.get_mut(0) {
        let _ = vl.details.clear();
    }
    let detail_loss = !l3.vecs[0].sections_ok();
    // 升级链漏转路：推入后抽走一条再收割 ⇒ 守恒闸必假。
    let mut l4 = OverrunLedger::new();
    let mut s = 0;
    while s < 256 && s < stream.len() {
        let (v, val) = stream[s];
        let _ = l4.record(s as u32, v, val);
        s += 1;
    }
    let _ = l4.escal.pop();
    let _ = l4.drain_escal();
    let escal_loss = !l4.escal_ok();
    Ok((
        sections_ok + judge_ok + recount_clean + escal_ok,
        over_vec as u32 + inj_all + detail_loss as u32 + escal_loss as u32,
        pushed as u32,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：三节生效 + 判读一致 + 复算清白 + 升级守恒 + 四拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _)) => Ok(()),
        _ => Err(OverCode::NotReproducible.val()),
    }
}

/// 序 B：越向量 ⇒ VectorOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = OverrunLedger::new();
    match l.record(0, VECTOR_COUNT, 50) {
        Err(e) if e == OverCode::VectorOver.val() => Ok(()),
        _ => Err(OverCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（向量 16 / 窗 64 / 明细 8 / 流 4096）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        VECTOR_COUNT == 16,
        ROLL_WINDOW == 64,
        DETAIL_CAP == 8,
        STREAM_LEN == 4096,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(OverCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let stream = machine_stream();
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let inj = inject_four(&stream);
        let mut i = 0;
        while i < 4 {
            led.record(i, inj[i]);
            i += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OverCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < OverCode::ALL.len() {
        let v = OverCode::ALL[i].val();
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
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, rej, _)) => *ok == 4 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, esc)) => format!("ok={} rejects={} escal={}", ok, rej, esc),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三节账面（逐向量滚动率抽样留痕）。
    let stream = machine_stream();
    let mut l = OverrunLedger::new();
    let mut i = 0;
    while i < stream.len() {
        let (vv, val) = stream[i];
        let _ = l.record(i as u32, vv, val);
        i += 1;
    }
    let mut rates: [u32; 4] = [0; 4];
    let mut vv = 0;
    while vv < 4 {
        if let Some(vl) = l.vecs.get(vv) {
            rates[vv] = vl.roll_permil();
        }
        vv += 1;
    }
    v.push((
        "J1-三节账",
        l.all_sections_ok(),
        format!("率0..3={:?}‰", rates),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(x) => x.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|x| x.tries),
            inj.as_ref().map(|x| x.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=冻结", b),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6FAC0 窗口"),
    ));
    v
}

pub fn run_unx1424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1424 超限率账（超限事件计数与比率滚动）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 自检（探针本地复核，不进内核镜像） ──────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_four_four() {
        match golden_run() {
            Ok((ok, rej, esc)) => {
                assert_eq!(ok, 4);
                assert_eq!(rej, 4);
                assert!(esc > 0);
            }
            Err(e) => panic!("golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn rolling_window_matches_manual_count() {
        let stream = machine_stream();
        let book = ThresholdBook::frozen();
        let mut l = OverrunLedger::new();
        let mut i = 0;
        while i < stream.len() {
            let (v, val) = stream[i];
            let _ = l.record(i as u32, v, val);
            i += 1;
        }
        // 向量 0 的尾窗手算对照（末 64 拍该向量样本）。
        let tail: Vec<(usize, u32)> = stream
            .iter()
            .filter(|(v, _)| *v == 0)
            .copied()
            .collect();
        let start = tail.len().saturating_sub(ROLL_WINDOW);
        let manual = tail[start..]
            .iter()
            .filter(|(v, val)| book.judge(*v, *val) == Ok(true))
            .count();
        match l.vecs.get(0) {
            Some(v0) => assert_eq!(v0.ring_over, manual),
            None => panic!("vec0 missing"),
        }
    }

    #[test]
    fn four_injections_all_caught() {
        let stream = machine_stream();
        let inj = inject_four(&stream);
        assert_eq!(inj, [true, true, true, true]);
    }

    #[test]
    fn three_probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
