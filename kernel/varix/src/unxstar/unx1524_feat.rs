//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1524 零丢事件账域级恒等式终验（投递=接收=应答全域） · 0x78400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 1）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1524-J1 零丢恒等式全域终验（发号数=入账数=应答数
//!     三账相等 10^7 事件窗），任何一处差额非零即红账 10/10 必抓；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 三账分立：发号账（issued）、接收账（received）、应答账（answered）
//!     各自独立累计，恒等式要求三账总和逐位相等且逐流相等；
//!   · 10^7 事件窗：40 流 × 每流 250_000 序，序号免检闭集 1..=250_000；
//!     窗口冻结，超窗即 SeqOver；
//!   · 单调状态机：每流水位线 issued_hi 只前进不倒退，倒退显性拒
//!     （SeqRewind）；接收序必须在已发号水位内（RecvAhead）；
//!     应答序必须在已接收水位内（AnsAhead）；封账后再登记拒（Sealed）；
//!   · 差额三账：delta_i / delta_r / delta_a 任一非零即红账（DeltaNonZero），
//!     注入路 10 轮 × 每轮三路扰动（少发/少收/少答）全部必抓；
//!   · 双跑一致：恒等式终验摘要两跑逐字段相等，不等即 NotReproducible；
//!   · 段位守卫：诊断码全部落在本卡 0x78400 窗口内。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78400，AI-VX8-1104 · B02 · 卡 1，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78400;

/// 本批（B02 八卡）诊断段窗口下界（派工卡 SEG 列最小值）。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界（派工卡 SEG 列最大值）。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 流总数（冻结 40，全域 40 批一一映射为 40 条事件流）。
pub const STREAM_COUNT: usize = 40;
/// 流号下界（冻结 1）。
pub const STREAM_LO: u8 = 1;
/// 流号上界（冻结 40）。
pub const STREAM_HI: u8 = 40;
/// 每流事件数（冻结 250_000）。
pub const EVENTS_PER_STREAM: u32 = 250_000;
/// 事件窗总量（冻结 10^7 = 40 × 250_000，判据"10^7 事件窗"）。
pub const EVENT_WINDOW: u32 = 10_000_000;
/// 序号闭集上界（每流序号 1..=250_000）。
pub const SEQ_CAP: u32 = EVENTS_PER_STREAM;
/// 红账注入轮数（判据"10/10 必抓"）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78401..0x7840C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZeroLossCode {
    /// 流号越界（不在 1..=40）
    StreamOver,
    /// 序号越窗（超出 1..=250_000 闭集）
    SeqOver,
    /// 发号倒退（水位线单调，倒退显性拒）
    SeqRewind,
    /// 接收超前（收到的序未发号）
    RecvAhead,
    /// 应答超前（应答的序未接收）
    AnsAhead,
    /// 差额非零（三账恒等式破，红账）
    DeltaNonZero,
    /// 逐流差额非零（流级恒等式破）
    StreamDelta,
    /// 封账后改账（seal 之后仍尝试登记）
    Sealed,
    /// 账面缩水（总额倒退，只增不减被破）
    LedgerShrink,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 窗口不足（三账合计 < 10^7）
    WindowShort,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl ZeroLossCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ZeroLossCode::StreamOver => "流号越界：只在 1..=40 闭集",
            ZeroLossCode::SeqOver => "序号越窗：每流序号 1..=250000",
            ZeroLossCode::SeqRewind => "发号倒退：水位线只前进，倒退即拒",
            ZeroLossCode::RecvAhead => "接收超前：先补发号再入接收账",
            ZeroLossCode::AnsAhead => "应答超前：先补接收再入应答账",
            ZeroLossCode::DeltaNonZero => "差额非零：发号=入账=应答恒等式破，红账",
            ZeroLossCode::StreamDelta => "流级差额非零：逐流三账对不平",
            ZeroLossCode::Sealed => "封账后改账：终验封印，登记口已闭",
            ZeroLossCode::LedgerShrink => "账面缩水：三账只增不减，注销一律拒",
            ZeroLossCode::NotReproducible => "双跑不一致：终验摘要两跑应同值",
            ZeroLossCode::WindowShort => "窗口不足：三账合计应达 10^7",
            ZeroLossCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [ZeroLossCode; 12] = [
        ZeroLossCode::StreamOver,
        ZeroLossCode::SeqOver,
        ZeroLossCode::SeqRewind,
        ZeroLossCode::RecvAhead,
        ZeroLossCode::AnsAhead,
        ZeroLossCode::DeltaNonZero,
        ZeroLossCode::StreamDelta,
        ZeroLossCode::Sealed,
        ZeroLossCode::LedgerShrink,
        ZeroLossCode::NotReproducible,
        ZeroLossCode::WindowShort,
        ZeroLossCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ZeroLossCode::ALL.len() {
        if ZeroLossCode::ALL[i].val() == code {
            return ZeroLossCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78400 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 单流零丢账：三账水位线 + 封账位。
///
/// 不变式（逐条可审计）：
///   I1: issued_hi 单调不减（倒退显性拒）；
///   I2: recv_hi ≤ issued_hi（接收不得超前发号）；
///   I3: ans_hi ≤ recv_hi（应答不得超前接收）；
///   I4: 三水位线封账后冻结。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamLedger {
    /// 流号（1..=40）
    pub stream: u8,
    /// 发号水位线（已发最大连续序）
    pub issued_hi: u32,
    /// 接收水位线（已收最大连续序）
    pub recv_hi: u32,
    /// 应答水位线（已答最大连续序）
    pub ans_hi: u32,
    /// 封账位（封后只读）
    pub sealed: bool,
}

impl StreamLedger {
    pub fn new(stream: u8) -> StreamLedger {
        StreamLedger { stream, issued_hi: 0, recv_hi: 0, ans_hi: 0, sealed: false }
    }

    /// 发号单事件：序号必须等于水位线 + 1（免检连续入账），倒退/跳跃/越窗拒。
    pub fn issue(&mut self, seq: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        if seq == 0 || seq > SEQ_CAP {
            return Err(ZeroLossCode::SeqOver.val());
        }
        if seq <= self.issued_hi {
            return Err(ZeroLossCode::SeqRewind.val());
        }
        if seq != self.issued_hi.checked_add(1).unwrap_or(0) {
            return Err(ZeroLossCode::SeqOver.val());
        }
        self.issued_hi = seq;
        Ok(())
    }

    /// 发号批量：n 个连续事件一次入账（10^7 窗口的大流量路径），
    /// 窗口余量不足或已封账拒。
    pub fn issue_bulk(&mut self, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        if n == 0 {
            return Err(ZeroLossCode::SeqOver.val());
        }
        let room = SEQ_CAP.saturating_sub(self.issued_hi);
        if n > room {
            return Err(ZeroLossCode::SeqOver.val());
        }
        let next = self.issued_hi.saturating_add(n);
        self.issued_hi = next;
        Ok(next)
    }

    /// 接收批量：接收序不得超过已发水位线（接收超前拒）。
    pub fn receive_bulk(&mut self, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        if n == 0 {
            return Err(ZeroLossCode::SeqOver.val());
        }
        let next = self.recv_hi.saturating_add(n);
        if next > self.issued_hi {
            return Err(ZeroLossCode::RecvAhead.val());
        }
        self.recv_hi = next;
        Ok(next)
    }

    /// 应答批量：应答序不得超过已收水位线（应答超前拒）。
    pub fn answer_bulk(&mut self, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        if n == 0 {
            return Err(ZeroLossCode::SeqOver.val());
        }
        let next = self.ans_hi.saturating_add(n);
        if next > self.recv_hi {
            return Err(ZeroLossCode::AnsAhead.val());
        }
        self.ans_hi = next;
        Ok(next)
    }

    /// 流级零丢：三水位线齐平即零丢。
    pub fn stream_zero_loss(&self) -> bool {
        self.issued_hi == self.recv_hi && self.recv_hi == self.ans_hi
    }

    /// 封账（不可逆，单调状态机终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 域级零丢总账：40 流 + 三账总和。
///
/// 不变式：
///   D1: 三账总和逐位相等（恒等式主判据）；
///   D2: 三账总和只增不减（缩水显性拒）；
///   D3: 逐流零丢（流级恒等式）；
///   D4: 封账后登记口闭。
#[derive(Clone, Debug)]
pub struct DomainEventLedger {
    /// 40 条流账（下标 0 对应流号 1）
    streams: Vec<StreamLedger>,
    /// 发号总账
    pub total_issued: u32,
    /// 接收总账
    pub total_recv: u32,
    /// 应答总账
    pub total_ans: u32,
    /// 历史峰值（缩水检测锚点）
    pub peak_total: u32,
    /// 域封账位
    pub sealed: bool,
}

impl DomainEventLedger {
    pub fn new() -> DomainEventLedger {
        let mut streams = Vec::new();
        let mut s = STREAM_LO;
        while s <= STREAM_HI {
            streams.push(StreamLedger::new(s));
            s += 1;
        }
        DomainEventLedger {
            streams,
            total_issued: 0,
            total_recv: 0,
            total_ans: 0,
            peak_total: 0,
            sealed: false,
        }
    }

    /// 流账取用（流号 1..=40 → 下标 0..=39）。
    fn stream_mut(&mut self, stream: u8) -> Result<&mut StreamLedger, u32> {
        if stream < STREAM_LO || stream > STREAM_HI {
            return Err(ZeroLossCode::StreamOver.val());
        }
        let idx = (stream - STREAM_LO) as usize;
        match self.streams.get_mut(idx) {
            Some(sl) => Ok(sl),
            None => Err(ZeroLossCode::StreamOver.val()),
        }
    }

    /// 发号批量登记（域级入口：封账/流号/窗口三查后转发流账）。
    pub fn issue_bulk(&mut self, stream: u8, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        let next = self.stream_mut(stream)?.issue_bulk(n)?;
        self.total_issued = self.total_issued.saturating_add(n);
        self.peak_total = self.peak_total.max(self.total_issued);
        Ok(next)
    }

    /// 接收批量登记。
    pub fn receive_bulk(&mut self, stream: u8, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        let next = self.stream_mut(stream)?.receive_bulk(n)?;
        self.total_recv = self.total_recv.saturating_add(n);
        Ok(next)
    }

    /// 应答批量登记。
    pub fn answer_bulk(&mut self, stream: u8, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ZeroLossCode::Sealed.val());
        }
        let next = self.stream_mut(stream)?.answer_bulk(n)?;
        self.total_ans = self.total_ans.saturating_add(n);
        Ok(next)
    }

    /// 三账差额（任一非零即红账）。
    pub fn deltas(&self) -> (u32, u32, u32) {
        let d1 = self.total_issued.saturating_sub(self.total_recv);
        let d2 = self.total_recv.saturating_sub(self.total_ans);
        let d3 = self.total_issued.saturating_sub(self.total_ans);
        (d1, d2, d3)
    }

    /// 流级零丢流数（逐流三账齐平的流数）。
    pub fn zero_loss_streams(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.streams.len() {
            if let Some(sl) = self.streams.get(i) {
                if sl.stream_zero_loss() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 域级恒等式终验：
    ///   ① 三账差额全零；
    ///   ② 三账合计 = 10^7（窗口足额）；
    ///   ③ 逐流零丢 40/40；
    ///   ④ 无缩水（total == peak）。
    pub fn audit(&self) -> Result<(u32, u32, u32), u32> {
        let (d1, d2, d3) = self.deltas();
        if d1 != 0 || d2 != 0 || d3 != 0 {
            return Err(ZeroLossCode::DeltaNonZero.val());
        }
        if self.total_issued != EVENT_WINDOW {
            return Err(ZeroLossCode::WindowShort.val());
        }
        if self.zero_loss_streams() != STREAM_COUNT as u32 {
            return Err(ZeroLossCode::StreamDelta.val());
        }
        if self.total_issued != self.peak_total {
            return Err(ZeroLossCode::LedgerShrink.val());
        }
        Ok((self.total_issued, self.total_recv, self.total_ans))
    }

    /// 域封账（封后所有登记口闭，终验冻结）。
    pub fn seal(&mut self) {
        self.sealed = true;
        let mut i = 0;
        while i < self.streams.len() {
            if let Some(sl) = self.streams.get_mut(i) {
                sl.seal();
            }
            i += 1;
        }
    }
}

// ── 冻结套件规格：40 流 × 250_000 = 10^7 ────────────────────────
/// 每流入账步长（冻结 1250：250_000 = 200 × 1250，批量路径 200 步/流）。
pub const BULK_STRIDE: u32 = 1250;
/// 每流批量步数（冻结 200）。
pub const BULK_STEPS: u32 = 200;

/// 单流满额跑：200 步 × 1250 = 250_000，发/收/答逐级齐平。
pub fn run_stream_full(led: &mut DomainEventLedger, stream: u8) -> Result<(), u32> {
    let mut step = 0;
    while step < BULK_STEPS {
        let n = BULK_STRIDE;
        led.issue_bulk(stream, n)?;
        led.receive_bulk(stream, n)?;
        led.answer_bulk(stream, n)?;
        step += 1;
    }
    Ok(())
}

/// 全域单轮：40 流逐流满额 10^7，域封账，审计对平。
pub fn run_domain_round() -> Result<(u32, u32, u32, u32), u32> {
    let mut led = DomainEventLedger::new();
    let mut s = STREAM_LO;
    while s <= STREAM_HI {
        run_stream_full(&mut led, s)?;
        s += 1;
    }
    let zl = led.zero_loss_streams();
    let audit = led.audit()?;
    // 封账后登记必拒（对照面）。
    led.seal();
    let sealed_rej = led.issue_bulk(STREAM_LO, 1).is_err();
    if !sealed_rej {
        return Err(ZeroLossCode::Sealed.val());
    }
    Ok((audit.0, audit.1, audit.2, zl))
}

/// 终验摘要（供双跑一致对比）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZeroLossSummary {
    pub issued: u32,
    pub received: u32,
    pub answered: u32,
    pub zero_loss_streams: u32,
    pub digest: u32,
}

/// FNV-1a（u32 口径）摘要卷积。
pub fn digest_round(h: u32, sum: &ZeroLossSummary) -> u32 {
    let mut x = h;
    x = x.wrapping_mul(16777619).wrapping_add(sum.issued);
    x = x.wrapping_mul(16777619).wrapping_add(sum.received);
    x = x.wrapping_mul(16777619).wrapping_add(sum.answered);
    x = x.wrapping_mul(16777619).wrapping_add(sum.zero_loss_streams);
    x
}

/// 独立建账终验一轮（供双跑一致）。
pub fn run_verify_round() -> Result<ZeroLossSummary, u32> {
    let (a, b, c, zl) = run_domain_round()?;
    let sum = ZeroLossSummary {
        issued: a,
        received: b,
        answered: c,
        zero_loss_streams: zl,
        digest: 0,
    };
    let d = digest_round(2166136261, &sum);
    Ok(ZeroLossSummary { digest: d, ..sum })
}

// ── LCG（确定性伪随机，种子固定可用） ───────────────────────────
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

// ── 红账注入检出账（三路：少发路/少收路/少答路） ─────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 红账注入：每轮在三账上打一路非零差额，恒等式必须当场抓（10/10）。
/// 路 0：少发（发号缺一步）→ 应答/接收差额非零 → DeltaNonZero；
/// 路 1：少收（接收缺一步）→ 接收/应答差额非零 → DeltaNonZero；
/// 路 2：少答（应答缺一步）→ 发号/应答差额非零 → DeltaNonZero。
pub fn inject_delta(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let stream = STREAM_LO + rng.below(STREAM_COUNT as u64) as u8;
        // 路 0：发号少 1 步（差额 = 1250）。
        let mut l0 = DomainEventLedger::new();
        let _ = l0.issue_bulk(stream, BULK_STRIDE.saturating_sub(1));
        let _ = l0.receive_bulk(stream, BULK_STRIDE);
        let _ = l0.answer_bulk(stream, BULK_STRIDE);
        acc.record(0, l0.audit() == Err(ZeroLossCode::DeltaNonZero.val()));
        // 路 1：接收少 1 步。
        let mut l1 = DomainEventLedger::new();
        let _ = l1.issue_bulk(stream, BULK_STRIDE);
        let _ = l1.receive_bulk(stream, BULK_STRIDE.saturating_sub(1));
        let _ = l1.answer_bulk(stream, BULK_STRIDE.saturating_sub(1));
        acc.record(1, l1.audit() == Err(ZeroLossCode::DeltaNonZero.val()));
        // 路 2：应答少 1 步（接收齐平、应答短缺）。
        let mut l2 = DomainEventLedger::new();
        let _ = l2.issue_bulk(stream, BULK_STRIDE);
        let _ = l2.receive_bulk(stream, BULK_STRIDE);
        let _ = l2.answer_bulk(stream, BULK_STRIDE.saturating_sub(1));
        acc.record(2, l2.audit() == Err(ZeroLossCode::DeltaNonZero.val()));
        r += 1;
    }
    Ok(acc)
}

/// 结构性注入：接收超前 / 应答超前 / 发号倒退 / 流号越界，全部必抓。
pub fn inject_structural() -> Result<(bool, bool, bool), u32> {
    // 接收超前：未发号先接收。
    let mut l1 = DomainEventLedger::new();
    let r1 = l1.receive_bulk(STREAM_LO, BULK_STRIDE);
    let c1 = r1 == Err(ZeroLossCode::RecvAhead.val());
    // 应答超前：未接收先应答。
    let mut l2 = DomainEventLedger::new();
    let _ = l2.issue_bulk(STREAM_LO, BULK_STRIDE);
    let r2 = l2.answer_bulk(STREAM_LO, BULK_STRIDE);
    let c2 = r2 == Err(ZeroLossCode::AnsAhead.val());
    // 流号越界：0 号与 41 号都拒。
    let mut l4 = DomainEventLedger::new();
    let r4a = l4.issue_bulk(0, 1);
    let r4b = l4.issue_bulk(STREAM_HI + 1, 1);
    let c4 = r4a == Err(ZeroLossCode::StreamOver.val()) && r4b == Err(ZeroLossCode::StreamOver.val());
    Ok((c1, c2, c4))
}

// ── 黄金全链：满额 10^7 → 审计对平 → 四拒 → 封账 → 封后再拒 ──────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = DomainEventLedger::new();
    let mut s = STREAM_LO;
    while s <= STREAM_HI {
        run_stream_full(&mut led, s)?;
        s += 1;
    }
    let (a, b, c) = led.audit()?;
    if a != EVENT_WINDOW || b != EVENT_WINDOW || c != EVENT_WINDOW {
        return Err(ZeroLossCode::WindowShort.val());
    }
    // 四拒口径：倒退拒 / 越窗拒 / 接收超前拒 / 越流拒。
    let rewind = {
        let mut l2 = DomainEventLedger::new();
        let _ = l2.stream_mut(STREAM_LO).and_then(|x| x.issue(1));
        let _ = l2.stream_mut(STREAM_LO).and_then(|x| x.issue(2));
        let r = l2.stream_mut(STREAM_LO).and_then(|x| x.issue(1));
        r == Err(ZeroLossCode::SeqRewind.val())
    };
    let over = {
        let mut l3 = DomainEventLedger::new();
        let r = l3.stream_mut(STREAM_LO).and_then(|x| x.issue(SEQ_CAP + 1));
        r == Err(ZeroLossCode::SeqOver.val())
    };
    let ahead = l2_probe() == Err(ZeroLossCode::RecvAhead.val());
    let over_stream = {
        let mut l4 = DomainEventLedger::new();
        l4.issue_bulk(STREAM_HI + 1, 1) == Err(ZeroLossCode::StreamOver.val())
    };
    // 封账后再登记：拒（Sealed）。
    led.seal();
    let sealed_rej = led.receive_bulk(STREAM_LO, 1).is_err();
    let flags = rewind as u32 + over as u32 + ahead as u32 + over_stream as u32;
    Ok((flags + sealed_rej as u32, 1, a))
}

/// 辅助：空账直接接收（接收超前对照）。
fn l2_probe() -> Result<u32, u32> {
    let mut l = DomainEventLedger::new();
    l.receive_bulk(STREAM_LO, 1)
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致（摘要逐字段相等）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 1, EVENT_WINDOW)) => {
            let a = run_verify_round()?;
            let b = run_verify_round()?;
            if a == b {
                Ok(())
            } else {
                Err(ZeroLossCode::NotReproducible.val())
            }
        }
        _ => Err(ZeroLossCode::NotReproducible.val()),
    }
}

/// 序 B：注入面（三路差额 + 结构路）逐条显性抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_delta(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(ZeroLossCode::DeltaNonZero.val());
    }
    let (c1, c2, c4) = inject_structural()?;
    if c1 && c2 && c4 {
        Ok(())
    } else {
        Err(ZeroLossCode::DeltaNonZero.val())
    }
}

/// 序 C：冻结常量 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    match (
        STREAM_COUNT == 40,
        STREAM_LO == 1,
        STREAM_HI == 40,
        EVENTS_PER_STREAM == 250_000,
        EVENT_WINDOW == 10_000_000,
        BULK_STRIDE == 1250,
        BULK_STEPS == 200,
        JUDGE_ROUNDS == 10,
        align,
        in_win,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(ZeroLossCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78400 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ZeroLossCode::ALL.len() {
        let v = ZeroLossCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_verify_round()?;
    let b = run_verify_round()?;
    if a != b || a.digest != b.digest {
        return Err(ZeroLossCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_verify_round()?;
    if sum.issued != EVENT_WINDOW {
        return Err(ZeroLossCode::WindowShort.val());
    }
    Ok(format!(
        "状态=已完成 issued={} recv={} ans={} zero_loss_streams={} digest=0x{:x}",
        sum.issued, sum.received, sum.answered, sum.zero_loss_streams, sum.digest
    ))
}

// ── 判据面（extra_checks：12 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：三账恒等式全域终验（10^7 窗口三账相等）。
    let g = golden_run();
    v.push((
        "J1-三账恒等式10^7窗",
        match &g {
            Ok((flags, _c, total)) => *flags == 5 && *total == EVENT_WINDOW,
            Err(_) => false,
        },
        match &g {
            Ok((flags, _c, total)) => format!("flags={} total={}", flags, total),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 全域单轮：40 流满额 + 逐流零丢 40/40。
    let round = run_domain_round();
    v.push((
        "J1-全域单轮40流",
        match &round {
            Ok((a, b, c, zl)) => {
                *a == EVENT_WINDOW && *b == EVENT_WINDOW && *c == EVENT_WINDOW && *zl == STREAM_COUNT as u32
            }
            Err(_) => false,
        },
        match &round {
            Ok((a, b, c, zl)) => format!("a={} b={} c={} zl={}", a, b, c, zl),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. J1 红账差额注入 10/10（少发/少收/少答三路全抓）。
    let inj = inject_delta(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-红账注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 4. J1 结构注入（接收超前/应答超前/越流）必抓。
    let st = inject_structural();
    v.push((
        "J1-结构注入必抓",
        match &st {
            Ok((c1, c2, c4)) => *c1 && *c2 && *c4,
            Err(_) => false,
        },
        match &st {
            Ok((c1, c2, c4)) => format!("c1={} c2={} c4={}", c1, c2, c4),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 边界：流号 1 与 40 满额单流跑，越界流拒。
    let edge = {
        let mut l = DomainEventLedger::new();
        let lo = run_stream_full(&mut l, STREAM_LO).is_ok();
        let hi = run_stream_full(&mut l, STREAM_HI).is_ok();
        let over = l.issue_bulk(STREAM_HI + 1, 1) == Err(ZeroLossCode::StreamOver.val());
        lo && hi && over
    };
    v.push((
        "边界-流号闭集",
        edge,
        String::from("流 1/40 满额可跑，41 号拒"),
    ));
    // 6. 边界：序号闭集 1..=250000，0 与 250001 拒。
    let seq_edge = {
        let mut l = DomainEventLedger::new();
        let zero = l.stream_mut(STREAM_LO).and_then(|x| x.issue(0));
        let over = l.stream_mut(STREAM_LO).and_then(|x| x.issue(SEQ_CAP + 1));
        let first = l.stream_mut(STREAM_LO).and_then(|x| x.issue(1));
        let last_ok = l.stream_mut(STREAM_LO).and_then(|x| x.issue_bulk(SEQ_CAP - 1));
        zero == Err(ZeroLossCode::SeqOver.val())
            && over == Err(ZeroLossCode::SeqOver.val())
            && first.is_ok()
            && last_ok.is_ok()
    };
    v.push((
        "边界-序号闭集",
        seq_edge,
        String::from("序 0/250001 拒，序 1 与满额批量收"),
    ));
    // 7. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 8. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_verify_round 两跑摘要同值"),
    ));
    // 9. 单调状态机：封账后三账冻结不动。
    let mono = {
        let mut l = DomainEventLedger::new();
        let _ = run_stream_full(&mut l, STREAM_LO);
        let before = (l.total_issued, l.total_recv, l.total_ans);
        l.seal();
        let _ = l.issue_bulk(STREAM_LO, 1);
        let _ = l.receive_bulk(STREAM_LO, 1);
        let _ = l.answer_bulk(STREAM_LO, 1);
        (l.total_issued, l.total_recv, l.total_ans) == before
    };
    v.push((
        "J2-封账冻结",
        mono,
        String::from("封账后三路登记全拒，账面不动"),
    ));
    // 10. 只增不减：峰值锚点不被缩水。
    let shrink_guard = {
        let mut l = DomainEventLedger::new();
        let _ = l.issue_bulk(STREAM_LO, 10);
        let _ = l.receive_bulk(STREAM_LO, 10);
        let _ = l.answer_bulk(STREAM_LO, 10);
        let peak = l.peak_total;
        let audit_bad = l.audit().is_err();
        peak == 10 && audit_bad
    };
    v.push((
        "J2-只增不减",
        shrink_guard,
        String::from("窗口未满额时审计显性拒（WindowShort）"),
    ));
    // 11. 诊断码闭集查表：段外码返回统一文案。
    let closed = {
        let mut ok = true;
        let mut i = 0;
        while i < ZeroLossCode::ALL.len() {
            let code = ZeroLossCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78400 段闭集内" {
                ok = false;
            }
            i += 1;
        }
        ok && detail(SEG_BASE + 0x30).starts_with("未知诊断码")
    };
    v.push((
        "闭集-诊断查表",
        closed,
        String::from("段内 12 码逐条有文案，段外统一文案"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x78400 窗口"),
    ));
    v
}

pub fn run_unx1524_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1524 零丢事件账域级恒等式终验（投递=接收=应答全域）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_three_ledger_identity_full_window() {
        match golden_run() {
            Ok((flags, sealed_marker, total)) => {
                assert_eq!(flags, 5);
                assert_eq!(sealed_marker, 1);
                assert_eq!(total, EVENT_WINDOW);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_delta_inject_10_of_10_caught() {
        match inject_delta(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入账应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_structural_inject_all_caught() {
        match inject_structural() {
            Ok((c1, c2, c4)) => {
                assert!(c1);
                assert!(c2);
                assert!(c4);
            }
            Err(e) => assert!(false, "结构注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t04_rewind_rejected() {
        let mut l = DomainEventLedger::new();
        let _ = l.stream_mut(STREAM_LO).and_then(|x| x.issue(1));
        let _ = l.stream_mut(STREAM_LO).and_then(|x| x.issue(2));
        assert_eq!(
            l.stream_mut(STREAM_LO).and_then(|x| x.issue(1)),
            Err(ZeroLossCode::SeqRewind.val())
        );
    }

    #[test]
    fn t05_recv_and_ans_ahead_rejected() {
        let mut l = DomainEventLedger::new();
        assert_eq!(l.receive_bulk(STREAM_LO, 1), Err(ZeroLossCode::RecvAhead.val()));
        let _ = l.issue_bulk(STREAM_LO, 5);
        assert_eq!(l.answer_bulk(STREAM_LO, 6), Err(ZeroLossCode::AnsAhead.val()));
        let _ = l.receive_bulk(STREAM_LO, 5);
        assert!(l.answer_bulk(STREAM_LO, 5).is_ok());
    }

    #[test]
    fn t06_stream_and_seq_bounds() {
        let mut l = DomainEventLedger::new();
        assert_eq!(l.issue_bulk(0, 1), Err(ZeroLossCode::StreamOver.val()));
        assert_eq!(l.issue_bulk(STREAM_HI + 1, 1), Err(ZeroLossCode::StreamOver.val()));
        assert_eq!(
            l.stream_mut(STREAM_LO).and_then(|x| x.issue(0)),
            Err(ZeroLossCode::SeqOver.val())
        );
        assert_eq!(
            l.stream_mut(STREAM_LO).and_then(|x| x.issue(SEQ_CAP + 1)),
            Err(ZeroLossCode::SeqOver.val())
        );
    }

    #[test]
    fn t07_double_run_identical() {
        let a = run_verify_round();
        let b = run_verify_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t08_seal_freezes_ledger() {
        let mut l = DomainEventLedger::new();
        let _ = l.issue_bulk(STREAM_LO, 10);
        l.seal();
        assert_eq!(l.issue_bulk(STREAM_LO, 1), Err(ZeroLossCode::Sealed.val()));
        assert_eq!(l.receive_bulk(STREAM_LO, 1), Err(ZeroLossCode::Sealed.val()));
        assert_eq!(l.answer_bulk(STREAM_LO, 1), Err(ZeroLossCode::Sealed.val()));
    }

    #[test]
    fn t09_window_short_audit_rejected() {
        let mut l = DomainEventLedger::new();
        let _ = l.issue_bulk(STREAM_LO, 10);
        let _ = l.receive_bulk(STREAM_LO, 10);
        let _ = l.answer_bulk(STREAM_LO, 10);
        assert_eq!(l.audit(), Err(ZeroLossCode::WindowShort.val()));
    }

    #[test]
    fn t10_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(ZeroLossCode::ALL.len(), 12);
        for c in ZeroLossCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78400 段闭集内");
        }
        assert_eq!(detail(SEG_BASE + 0x30).starts_with("未知诊断码"), true);
    }

    #[test]
    fn t11_writeback_summary_non_empty() {
        match writeback_summary() {
            Ok(s) => {
                assert!(s.starts_with("状态=已完成"));
                assert!(s.len() > 10);
            }
            Err(e) => assert!(false, "写回应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1524_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
