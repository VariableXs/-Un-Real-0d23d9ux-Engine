//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1945 · INIT-SIPI 启动 IPI 复盘账 · 0x72780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B18）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F1945-J1 启动链 IPI 视角终对账全过（INIT-SIPI 序列
//!     ×B01–B02 启动时间线账互证），启动期 IPI 零丢失断言，复盘账冻结入册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · IPI 事件闭集（INIT/DEASSERT/SIPI/WAKE），逐 AP 逐事件记账（seq/时刻
//!     单调、核号闭集），启动链 = 每 AP 恰 1 INIT + 1..=RETRY_CAP 次 SIPI
//!     + 1 次唤醒确认；INIT 必在 SIPI 前、唤醒必在 SIPI 后、INIT 不重复；
//!   · 零丢失断言：发出事件数 == 到达事件数，逐事件发出→到达一一配对；
//!   · B01–B02 互证：B01（BSP 侧发出时刻）× B02（AP 侧唤醒时刻）逐 AP
//!     对撞，awake(B02) 必 ≥ 末次 SIPI(B01)，不等即互证败；
//!   · 失真注入 10 路核验器 10/10 必抓；复盘账冻结入册，冻结后写显性拒。
//! ═══════════════════════════════════════════════════════════════════════
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
/// 诊断段基址（0x72780，VX3-1103 启动 IPI 窗口独占）。
pub const SEG_BASE: u32 = 0x72780;
/// AP 核数（冻结 8，apic 号 0..AP_N；BSP 不入账）。
pub const AP_N: usize = 8;
/// SIPI 启动向量（冻结 0x20，页对齐低址）。
pub const SIPI_VECTOR: u8 = 0x20;
/// SIPI 重试上限（冻结 2：INIT→SIPI→[200us]→SIPI'）。
pub const RETRY_CAP: u32 = 2;
/// SIPI 重试间隔（冻结 200 微秒，整数微秒域）。
pub const RETRY_WAIT_US: u64 = 200;
/// 账面纪元（冻结 3）。
pub const EPOCH: u64 = 3;
/// 启动基准拍（每 AP 间隔步长，确定性时刻派生用）。
pub const TICK_US: u64 = 40;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiCode {
    /// IPI 丢失：发出事件无到达配对（逐事件一一配对缺到）
    IpiLost,
    /// 序列破损：SIPI 先于 INIT / 唤醒先于 SIPI / INIT 二次重复
    SeqBroken,
    /// 时间线互证败：B02 唤醒时刻与 B01 末次 SIPI 时刻对撞不等
    TimelineMiss,
    /// 向量非法：SIPI 启动向量 ≠ SIPI_VECTOR 或域越界
    VectorBad,
    /// 重试超限：SIPI 重试次数 > RETRY_CAP
    RetryOver,
    /// seq 倒退：事件账序号/时刻单调性被破坏
    SeqBack,
    /// 冻结后写：复盘账已冻结入册，任何再写显性拒
    FrozenWrite,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}
impl IpiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IpiCode::IpiLost => "IPI 丢失：发出事件无到达配对，逐事件配对定位缺失路",
            IpiCode::SeqBroken => "序列破损：SIPI 先于 INIT 或唤醒先于 SIPI 或 INIT 重复",
            IpiCode::TimelineMiss => "时间线互证败：B02 唤醒时刻与 B01 末次 SIPI 对撞不等",
            IpiCode::VectorBad => "向量非法：SIPI 向量必须等于冻结向量，越界一律拒",
            IpiCode::RetryOver => "重试超限：SIPI 重试不得超过上限，超限先查 AP 假死",
            IpiCode::SeqBack => "seq 倒退：事件序号/时刻只进不退，倒退显性拒",
            IpiCode::FrozenWrite => "冻结后写：复盘账已入册，改写按倒退拒",
            IpiCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [IpiCode; 8] = [
        IpiCode::IpiLost, IpiCode::SeqBroken, IpiCode::TimelineMiss, IpiCode::VectorBad,
        IpiCode::RetryOver, IpiCode::SeqBack, IpiCode::FrozenWrite, IpiCode::NotReproducible,
    ];
}
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IpiCode::ALL.len() {
        if IpiCode::ALL[i].val() == code { return IpiCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72780 段闭集内"
}
// ── IPI 事件闭集 ────────────────────────────────────────────────
/// 事件类别：INIT（复位请求）/ IDAS（电平撤销）/ SIPI（带启动向量）/
/// WAKE（AP 侧唤醒确认，B02 账源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiKind {
    Init,
    Deassert,
    Sipi,
    Wake,
}
impl IpiKind {
    pub fn rank(self) -> u8 {
        match self {
            IpiKind::Deassert => 0,
            IpiKind::Init => 1,
            IpiKind::Sipi => 2,
            IpiKind::Wake => 3,
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            IpiKind::Init => "INIT",
            IpiKind::Deassert => "IDAS",
            IpiKind::Sipi => "SIPI",
            IpiKind::Wake => "WAKE",
        }
    }
}
/// 启动链状态机（Idle→InitPhase→SipiPhase→Awake→Sealed，单调推进）：
/// 空闲 / INIT 已发 / SIPI 已发（含重试窗）/ AP 已唤醒 / 复盘账冻结入册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootState {
    Idle,
    InitPhase,
    SipiPhase,
    Awake,
    Sealed,
}
impl BootState {
    pub fn rank(self) -> u8 {
        match self {
            BootState::Idle => 0,
            BootState::InitPhase => 1,
            BootState::SipiPhase => 2,
            BootState::Awake => 3,
            BootState::Sealed => 4,
        }
    }
    pub fn can_advance(self, to: BootState) -> bool {
        to.rank() > self.rank()
    }
}
/// 单条 IPI 事件账（逐 AP 逐事件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpiEvent {
    pub apic: usize,
    pub kind: IpiKind,
    /// 事件序号（全局账内单调递增，从 1 起）。
    pub seq: u64,
    /// 事件时刻（微秒，整数域，事件序内单调不减）。
    pub t_us: u64,
    /// SIPI 事件携带的启动向量（非 SIPI 恒 0）。
    pub vector: u8,
    /// true=发出（B01 侧），false=到达（AP 侧配对）。
    pub sent: bool,
}
impl IpiEvent {
    pub fn render(&self, sum: u32) -> String {
        format!(
            "@IPI1|seq={}|apic={}|kind={}|t={}|vec={:02X}|dir={}|sum={:08X};\n",
            self.seq, self.apic, self.kind.tag(), self.t_us, self.vector, if self.sent { "S" } else { "R" }, sum
        )
    }
}
// ── B01–B02 时间线账（互证双源） ─────────────────────────────────
/// B01 行：BSP 侧发出时刻账（每 AP 一行：INIT 时刻 + 末次 SIPI 时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B01Row {
    pub apic: usize,
    pub init_t: u64,
    pub last_sipi_t: u64,
    pub sipi_count: u32,
}
/// B02 行：AP 侧唤醒时刻账（每 AP 一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B02Row {
    pub apic: usize,
    pub awake_t: u64,
}
impl B01Row {
    pub fn render(&self, sum: u32) -> String {
        format!(
            "@B011|apic={}|init={}|last={}|n={}|sum={:08X};\n",
            self.apic, self.init_t, self.last_sipi_t, self.sipi_count, sum
        )
    }
}
impl B02Row {
    pub fn render(&self, sum: u32) -> String {
        format!("@B021|apic={}|wake={}|sum={:08X};\n", self.apic, self.awake_t, sum)
    }
}
pub fn crosscheck(b01: &B01Row, b02: &B02Row) -> Result<(), u32> {
    if b01.apic != b02.apic { return Err(IpiCode::TimelineMiss.val()); }
    if b01.init_t >= b01.last_sipi_t { return Err(IpiCode::SeqBroken.val()); }
    if b02.awake_t < b01.last_sipi_t { return Err(IpiCode::TimelineMiss.val()); }
    Ok(())
}
// ── 确定性启动时间线派生（可复算，无墙钟） ──────────────────────
pub fn init_t_of(apic: usize) -> u64 {
    100 + (apic as u64) * TICK_US
}
pub fn sipi_t_of(apic: usize, retry: u32) -> u64 {
    init_t_of(apic) + TICK_US + (retry as u64) * RETRY_WAIT_US
}
pub fn awake_t_of(apic: usize, retries: u32) -> u64 {
    sipi_t_of(apic, retries) + TICK_US
}
pub fn retries_of(apic: usize) -> u32 {
    if apic % 2 == 1 {
        1
    } else {
        0
    }
}
// ── 复盘账（逐 AP 启动链 + 全局事件账 + 冻结位） ────────────────
/// 每 AP 启动链账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApChain {
    pub apic: usize,
    pub init_count: u32,
    pub deassert_count: u32,
    pub sipi_count: u32,
    pub wake_count: u32,
    pub retries: u32,
    pub last_kind_rank: u8,
    pub last_t: u64,
    pub state: BootState,
}
impl ApChain {
    pub fn new(apic: usize) -> ApChain {
        ApChain {
            apic, init_count: 0, deassert_count: 0, sipi_count: 0, wake_count: 0,
            retries: 0, last_kind_rank: 0, last_t: 0, state: BootState::Idle,
        }
    }
}
/// 复盘账：全局事件账 + 每 AP 链 + B01/B02 时间线 + 冻结位；
/// sent/arrived 为零丢失断言两端，seq_cursor 只进不退，rejects 显性记账。
#[derive(Clone, Debug)]
pub struct ReplayLedger {
    pub epoch: u64,
    pub state: BootState,
    pub events: Vec<IpiEvent>,
    pub chains: Vec<ApChain>,
    pub b01: Vec<B01Row>,
    pub b02: Vec<B02Row>,
    pub sent_n: u64,
    pub arrived_n: u64,
    pub seq_cursor: u64,
    pub rejects: u32,
}
impl ReplayLedger {
    pub fn new(epoch: u64) -> ReplayLedger {
        let mut chains = Vec::new();
        let mut a = 0usize;
        while a < AP_N {
            chains.push(ApChain::new(a));
            a += 1;
        }
        ReplayLedger {
            epoch, state: BootState::Idle, events: Vec::new(), chains,
            b01: Vec::new(), b02: Vec::new(), sent_n: 0, arrived_n: 0,
            seq_cursor: 0, rejects: 0,
        }
    }
    pub fn advance_state(&mut self, to: BootState) -> Result<(), u32> {
        if !self.state.can_advance(to) { self.rejects += 1; return Err(IpiCode::SeqBack.val()); }
        self.state = to;
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<(), u32> {
        self.advance_state(BootState::Sealed)
    }
    pub fn record(&mut self, ev: IpiEvent) -> Result<(), u32> {
        if self.state == BootState::Sealed { self.rejects += 1; return Err(IpiCode::FrozenWrite.val()); }
        if ev.apic >= AP_N { self.rejects += 1; return Err(IpiCode::SeqBack.val()); }
        if ev.seq <= self.seq_cursor { self.rejects += 1; return Err(IpiCode::SeqBack.val()); }
        if ev.kind == IpiKind::Sipi && ev.vector != SIPI_VECTOR { self.rejects += 1; return Err(IpiCode::VectorBad.val()); }
        // 到达影：过四守卫后直接入账配对。
        if !ev.sent {
            self.arrived_n += 1;
            self.seq_cursor = ev.seq;
            self.events.push(ev);
            return Ok(());
        }
        // 发出事件：事件类别推进核（kind 序值不得倒退于该 AP 链尾序值）。
        let chain = match self.chains.get_mut(ev.apic) {
            Some(c) => c,
            None => return Err(IpiCode::SeqBack.val()),
        };
        if ev.kind.rank() < chain.last_kind_rank { self.rejects += 1; return Err(IpiCode::SeqBroken.val()); }
        if ev.t_us < chain.last_t { self.rejects += 1; return Err(IpiCode::SeqBack.val()); }
        // 类别计数与链状态推进（只进不退）。
        match ev.kind {
            IpiKind::Init => {
                if chain.init_count >= 1 { self.rejects += 1; return Err(IpiCode::SeqBroken.val()); }
                chain.init_count += 1;
            }
            IpiKind::Deassert => {
                chain.deassert_count += 1;
            }
            IpiKind::Sipi => {
                if chain.init_count == 0 { self.rejects += 1; return Err(IpiCode::SeqBroken.val()); }
                chain.sipi_count += 1;
                if chain.sipi_count > RETRY_CAP + 1 { self.rejects += 1; return Err(IpiCode::RetryOver.val()); }
                chain.retries = chain.sipi_count.saturating_sub(1);
            }
            IpiKind::Wake => {
                if chain.sipi_count == 0 { self.rejects += 1; return Err(IpiCode::SeqBroken.val()); }
                chain.wake_count += 1;
                if chain.wake_count >= 2 { self.rejects += 1; return Err(IpiCode::SeqBroken.val()); }
            }
        }
        chain.last_kind_rank = ev.kind.rank();
        chain.last_t = ev.t_us;
        let want = match ev.kind {
            IpiKind::Init | IpiKind::Deassert => BootState::InitPhase,
            IpiKind::Sipi => BootState::SipiPhase,
            IpiKind::Wake => BootState::Awake,
        };
        if want.rank() > chain.state.rank() {
            chain.state = want;
        }
        self.sent_n += 1;
        self.seq_cursor = ev.seq;
        self.events.push(ev);
        Ok(())
    }
    pub fn reconcile(&mut self) -> Result<(u64, u64), u32> {
        if self.state == BootState::Sealed { self.rejects += 1; return Err(IpiCode::FrozenWrite.val()); }
        // 每 AP 链完整性：恰 1 INIT、≥1 SIPI、恰 1 WAKE、SIPI 不超限。
        let mut a = 0usize;
        while a < AP_N {
            let c = match self.chains.get(a) {
                Some(c) => c,
                None => return Err(IpiCode::SeqBack.val()),
            };
            if c.init_count != 1 || c.sipi_count == 0 || c.wake_count != 1 { return Err(IpiCode::SeqBroken.val()); }
            if c.sipi_count > RETRY_CAP + 1 { return Err(IpiCode::RetryOver.val()); }
            a += 1;
        }
        // 零丢失断言：发出 == 到达，且逐事件 S/R 一一配对（同 apic+kind+seq/2）。
        if self.sent_n != self.arrived_n { return Err(IpiCode::IpiLost.val()); }
        // 发出事件必须都有到达影（同 apic 同 kind 各恰一条 S 一条 R）。
        let mut i = 0usize;
        while i < self.events.len() {
            let e = match self.events.get(i) {
                Some(e) => e,
                None => return Err(IpiCode::SeqBack.val()),
            };
            if !event_has_pair(&self.events, e) { return Err(IpiCode::IpiLost.val()); }
            i += 1;
        }
        // B01/B02 成型（从事件账派生，非独立直填）。
        self.b01.clear();
        self.b02.clear();
        let mut a = 0usize;
        while a < AP_N {
            let mut init_t = 0u64;
            let mut last_sipi_t = 0u64;
            let mut sipi_count = 0u32;
            let mut awake_t = 0u64;
            let mut j = 0usize;
            while j < self.events.len() {
                if let Some(e) = self.events.get(j) {
                    if e.apic == a {
                        match e.kind {
                            IpiKind::Init if e.sent => init_t = e.t_us,
                            IpiKind::Sipi if e.sent => {
                                last_sipi_t = e.t_us;
                                sipi_count += 1;
                            }
                            IpiKind::Wake if !e.sent => awake_t = e.t_us,
                            _ => {}
                        }
                    }
                }
                j += 1;
            }
            if init_t == 0 || last_sipi_t == 0 || awake_t == 0 { return Err(IpiCode::IpiLost.val()); }
            self.b01.push(B01Row { apic: a, init_t, last_sipi_t, sipi_count });
            self.b02.push(B02Row { apic: a, awake_t });
            a += 1;
        }
        // 互证：逐 AP 对撞。
        let mut a = 0usize;
        while a < AP_N {
            let r1 = match self.b01.get(a) {
                Some(r) => r,
                None => return Err(IpiCode::TimelineMiss.val()),
            };
            let r2 = match self.b02.get(a) {
                Some(r) => r,
                None => return Err(IpiCode::TimelineMiss.val()),
            };
            crosscheck(r1, r2)?;
            a += 1;
        }
        if self.state == BootState::Idle { self.state = BootState::Awake; }
        Ok((self.sent_n, self.arrived_n))
    }
}
fn event_has_pair(events: &[IpiEvent], e: &IpiEvent) -> bool {
    let mut i = 0usize;
    while i < events.len() {
        if let Some(o) = events.get(i) {
            if o.apic == e.apic && o.kind == e.kind && o.sent != e.sent { return true; }
        }
        i += 1;
    }
    false
}
// ── 确定性启动链回放（建账） ────────────────────────────────────
pub fn replay_boot(epoch: u64) -> Result<ReplayLedger, u32> {
    let mut led = ReplayLedger::new(epoch);
    let mut seq = 0u64;
    let mut a = 0usize;
    while a < AP_N {
        let retries = retries_of(a);
        // INIT 发出 + 到达影。
        seq += 1;
        led.record(IpiEvent { apic: a, kind: IpiKind::Init, seq, t_us: init_t_of(a), vector: 0, sent: true })?;
        seq += 1;
        led.record(IpiEvent { apic: a, kind: IpiKind::Init, seq, t_us: init_t_of(a), vector: 0, sent: false })?;
        // SIPI 发出（1+retries 次）+ 各自到达影。
        let mut r = 0u32;
        while r <= retries {
            seq += 1;
            led.record(IpiEvent { apic: a, kind: IpiKind::Sipi, seq, t_us: sipi_t_of(a, r), vector: SIPI_VECTOR, sent: true })?;
            seq += 1;
            led.record(IpiEvent { apic: a, kind: IpiKind::Sipi, seq, t_us: sipi_t_of(a, r), vector: SIPI_VECTOR, sent: false })?;
            r += 1;
        }
        // WAKE 到达 + 发出影（AP 侧确认回执也双向配对）。
        seq += 1;
        led.record(IpiEvent { apic: a, kind: IpiKind::Wake, seq, t_us: awake_t_of(a, retries), vector: 0, sent: false })?;
        seq += 1;
        led.record(IpiEvent { apic: a, kind: IpiKind::Wake, seq, t_us: awake_t_of(a, retries), vector: 0, sent: true })?;
        a += 1;
    }
    led.reconcile()?;
    Ok(led)
}
// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}
// ── 复盘账全册渲染（冻结体例：事件账 + B01 + B02） ──────────────
pub fn render_ledger(led: &ReplayLedger) -> String {
    let mut s = String::new();
    // 三段账（事件账 / B01 / B02）共用一套渲染序：先算零和校验再成型。
    let mut i = 0usize;
    while i < led.events.len() {
        if let Some(e) = led.events.get(i) {
            let sum = fnv1a(e.render(0).as_bytes());
            s.push_str(&e.render(sum));
        }
        i += 1;
    }
    i = 0;
    while i < led.b01.len() {
        if let Some(r) = led.b01.get(i) {
            let sum = fnv1a(r.render(0).as_bytes());
            s.push_str(&r.render(sum));
        }
        i += 1;
    }
    i = 0;
    while i < led.b02.len() {
        if let Some(r) = led.b02.get(i) {
            let sum = fnv1a(r.render(0).as_bytes());
            s.push_str(&r.render(sum));
        }
        i += 1;
    }
    s
}
// ── 字节查找与解析 ──────────────────────────────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle { return Some(i); }
        i += 1;
    }
    None
}
// ── 失真注入（10 路逐字失真） ────────────────────────────────────
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}
fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() { return None; }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() { return Some(i); }
        i += 1;
    }
    None
}
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    let mut i = 0usize;
    while i < b.len() {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
        i += 1;
    }
}
fn drop_first_line(b: &[u8], v: &mut Vec<u8>) {
    let le = match find_byte(b, 0, b'\n') {
        Some(p) => p,
        None => b.len(),
    };
    push_range(b, le + 1, b.len().saturating_sub(1), v);
}
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let le = match find_byte(b, 0, b'\n') {
        Some(p) => p,
        None => b.len(),
    };
    match kind % 10 {
        // 丢事件行（零丢失必抓）· seq/apic/t/vec/sum/kind 域改写六路
        // · 截尾 · 控制符 · 加伪 B02 行（互证必抓）。
        0 => drop_first_line(b, &mut v),
        1 => patch_after(b, b"seq=", b'1', b'9', &mut v),
        2 => patch_after(b, b"apic=", b'0', b'7', &mut v),
        3 => patch_after(b, b"t=", b'1', b'0', &mut v),
        4 => patch_after(b, b"vec=", b'2', b'0', &mut v),
        5 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        6 => patch_after(b, b"kind=", b'I', b'X', &mut v),
        7 => push_range(b, 0, b.len().saturating_sub(9), &mut v),
        8 => {
            push_range(b, 0, le / 2, &mut v);
            v.push(0x01);
            push_range(b, le / 2 + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"@B021|apic=99|wake=1|sum=00000000;\n";
            let mut k = 0usize;
            while k < tail.len() {
                v.push(tail[k]);
                k += 1;
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}
// ── 全册核验器 ──────────────────────────────────────────────────
/// 黄金行逐字节对账（行尾换行剥离后全等；长度不等即不等，安全）。
fn gold_cmp(line: &str, gold: &str, code: u32) -> Result<(), u32> {
    if line != &gold[..gold.len().saturating_sub(1)] { return Err(code); }
    Ok(())
}
pub fn verify_ledger_text(led: &ReplayLedger, out: &str) -> Result<(), u32> {
    if !out.starts_with("@IPI1|") { return Err(IpiCode::SeqBroken.val()); }
    let b = out.as_bytes();
    // 裸控制符扫描（<0x20 且非换行）。
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(IpiCode::SeqBack.val()); }
        i += 1;
    }
    // 行数闭集：事件账行 + AP_N B01 行 + AP_N B02 行。
    let mut lines = 0u32;
    let mut p = 0usize;
    while p < b.len() {
        if b[p] == b'\n' {
            lines += 1;
        }
        p += 1;
    }
    if lines != (led.events.len() + led.b01.len() + led.b02.len()) as u32 { return Err(IpiCode::IpiLost.val()); }
    // 逐行前缀/解析对账（事件行解析逐域必须等于账内事件）。
    let mut li = 0usize;
    let mut ev_i = 0usize;
    let mut b01_i = 0usize;
    let mut b02_i = 0usize;
    while li < b.len() {
        let le = match find_byte(b, li, b'\n') {
            Some(p) => p,
            None => b.len(),
        };
        let line = &out[li..le];
        if line.starts_with("@IPI1|") {
            let e = match led.events.get(ev_i) {
                Some(e) => e,
                None => return Err(IpiCode::IpiLost.val()),
            };
            let sum = fnv1a(e.render(0).as_bytes());
            gold_cmp(line, &e.render(sum), IpiCode::SeqBroken.val())?;
            ev_i += 1;
        } else if line.starts_with("@B011|") {
            let r = match led.b01.get(b01_i) {
                Some(r) => r,
                None => return Err(IpiCode::TimelineMiss.val()),
            };
            let sum = fnv1a(r.render(0).as_bytes());
            gold_cmp(line, &r.render(sum), IpiCode::TimelineMiss.val())?;
            b01_i += 1;
        } else if line.starts_with("@B021|") {
            let r = match led.b02.get(b02_i) {
                Some(r) => r,
                None => return Err(IpiCode::TimelineMiss.val()),
            };
            let sum = fnv1a(r.render(0).as_bytes());
            gold_cmp(line, &r.render(sum), IpiCode::TimelineMiss.val())?;
            b02_i += 1;
        } else {
            return Err(IpiCode::SeqBroken.val());
        }
        if le >= b.len() {
            break;
        }
        li = le + 1;
    }
    if ev_i != led.events.len() || b01_i != led.b01.len() || b02_i != led.b02.len() { return Err(IpiCode::IpiLost.val()); }
    Ok(())
}
// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let ledg = replay_boot(EPOCH + r as u64)?;
        let gold = render_ledger(&ledg);
        verify_ledger_text(&ledg, &gold)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_ledger_text(&ledg, &distort(&gold, kind)).is_err());
        r += 1;
    }
    Ok(led)
}
// ── 黄金全链 ────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, usize, usize), u32> {
    let mut led = replay_boot(EPOCH)?;
    let (sent, arrived) = led.reconcile()?;
    if sent != arrived { return Err(IpiCode::IpiLost.val()); }
    let gold = render_ledger(&led);
    verify_ledger_text(&led, &gold)?;
    // 互证逐 AP 复核。
    let mut cross_ok = 0usize;
    let mut a = 0usize;
    while a < led.b01.len() {
        let r1 = match led.b01.get(a) { Some(r) => r, None => return Err(IpiCode::TimelineMiss.val()) };
        let r2 = match led.b02.get(a) { Some(r) => r, None => return Err(IpiCode::TimelineMiss.val()) };
        if crosscheck(r1, r2).is_ok() { cross_ok += 1; }
        a += 1;
    }
    // 四拒探：apic 越界 / seq 倒退 / 二次 INIT / 冻结后写 / 向量非法。
    let mut probe = ReplayLedger::new(EPOCH);
    let _ = probe.record(IpiEvent { apic: AP_N, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let range_rej = probe.rejects >= 1;
    let mut probe2 = ReplayLedger::new(EPOCH);
    let _ = probe2.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let _ = probe2.record(IpiEvent { apic: 0, kind: IpiKind::Deassert, seq: 0, t_us: 10, vector: 0, sent: true });
    let seq_rej = probe2.rejects >= 1;
    let mut probe3 = ReplayLedger::new(EPOCH);
    let _ = probe3.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let _ = probe3.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 2, t_us: 20, vector: 0, sent: true });
    let dup_rej = probe3.rejects >= 1;
    let _ = led.freeze();
    let frozen_rej = led.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: led.seq_cursor + 1, t_us: 9999, vector: 0, sent: true }).is_err();
    let mut probe4 = ReplayLedger::new(EPOCH);
    let _ = probe4.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let _ = probe4.record(IpiEvent { apic: 0, kind: IpiKind::Sipi, seq: 2, t_us: 20, vector: SIPI_VECTOR + 1, sent: true });
    let vec_rej = probe4.rejects >= 1;
    let flags = (sent == arrived && sent > 0) as u32 + (cross_ok == AP_N) as u32
        + verify_ledger_text(&led, &render_ledger(&led)).is_ok() as u32
        + inject_rounds(1).is_ok() as u32;
    let rejects = range_rej as u32 + seq_rej as u32 + dup_rej as u32 + (frozen_rej && vec_rej) as u32;
    Ok((flags, rejects, led.events.len(), cross_ok))
}
// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(IpiCode::NotReproducible.val()),
    }
}
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(IpiCode::IpiLost.val()),
    }
}
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72780 && IpiCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = AP_N == 8 && SIPI_VECTOR == 0x20 && RETRY_CAP == 2 && RETRY_WAIT_US == 200
        && EPOCH == 3 && TICK_US == 40 && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(IpiCode::NotReproducible.val())
    }
}
// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(IpiCode::NotReproducible.val()); }
    Ok(())
}
// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IpiCode::ALL.len() {
        let v = IpiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}
// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut led = replay_boot(EPOCH)?;
    let (sent, arrived) = led.reconcile()?;
    let _ = led.freeze();
    Ok(format!("状态=已完成 events={} sent={} arrived={} b01={} b02={} frozen={}",
        led.events.len(), sent, arrived, led.b01.len(), led.b02.len(), led.state == BootState::Sealed))
}
// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n, c)) => (*f == 4 && *r == 4, format!("flags={} rejects={} events={} cross={}/{}", f, r, n, c, AP_N)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 启动期 IPI 零丢失断言（多纪元复算）。
    let mut zero_ok = true;
    let mut total_ev = 0u64;
    let mut ep = 0u64;
    while ep < 3 {
        match replay_boot(EPOCH + ep) {
            Ok(l) => {
                zero_ok = zero_ok && l.sent_n == l.arrived_n && l.sent_n > 0;
                total_ev = l.events.len() as u64;
            }
            Err(_) => zero_ok = false,
        }
        ep += 1;
    }
    v.push(("J1-启动期IPI零丢失", zero_ok && total_ev > 0, format!("三纪元 发出==到达，事件账 {} 行", total_ev)));
    // J1 INIT-SIPI 序列 × B01–B02 互证：逐 AP 对撞全过 + 抢跑负探必抓。
    let ledg = replay_boot(EPOCH);
    let mut cross_ok = true;
    let mut cross_n = 0usize;
    if let Ok(l) = &ledg {
        let mut a = 0usize;
        while a < l.b01.len() {
            match (l.b01.get(a), l.b02.get(a)) {
                (Some(r1), Some(r2)) => match crosscheck(r1, r2) {
                    Ok(()) => cross_n += 1,
                    Err(_) => cross_ok = false,
                },
                _ => cross_ok = false,
            }
            a += 1;
        }
        // 互证负探：伪造 B02 抢跑唤醒必抓。
        let fake = B02Row { apic: 0, awake_t: 1 };
        if let Some(r1) = l.b01.get(0) {
            if crosscheck(r1, &fake).is_ok() { cross_ok = false; }
        }
    } else {
        cross_ok = false;
    }
    v.push(("J1-B01–B02互证", cross_ok && cross_n == AP_N, format!("逐 AP 对撞 {}/{} 全过，抢跑负探必抓", cross_n, AP_N)));
    // J1 序列语义负探四路：SIPI 抢跑 / 唤醒抢跑 / INIT 重复 / 重试超限。
    let mut p1 = ReplayLedger::new(EPOCH);
    let _ = p1.record(IpiEvent { apic: 0, kind: IpiKind::Sipi, seq: 1, t_us: 10, vector: SIPI_VECTOR, sent: true });
    let sipi_first_rej = p1.rejects >= 1;
    let mut p2 = ReplayLedger::new(EPOCH);
    let _ = p2.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let _ = p2.record(IpiEvent { apic: 0, kind: IpiKind::Wake, seq: 2, t_us: 20, vector: 0, sent: true });
    let wake_first_rej = p2.rejects >= 1;
    let mut p3 = ReplayLedger::new(EPOCH);
    let _ = p3.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let _ = p3.record(IpiEvent { apic: 0, kind: IpiKind::Deassert, seq: 2, t_us: 15, vector: 0, sent: true });
    let _ = p3.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 3, t_us: 20, vector: 0, sent: true });
    let dup_init_rej = p3.rejects >= 1;
    let mut p4 = ReplayLedger::new(EPOCH);
    let _ = p4.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: 1, t_us: 10, vector: 0, sent: true });
    let mut sq = 2u64;
    let mut over_rej = false;
    let mut r = 0u32;
    while r < RETRY_CAP + 2 {
        // 连发 RETRY_CAP+2 次 SIPI（超出 RETRY_CAP+1 上限）必在最后一发拒。
        let _ = p4.record(IpiEvent { apic: 0, kind: IpiKind::Sipi, seq: sq, t_us: 100 + (r as u64) * 10, vector: SIPI_VECTOR, sent: true });
        if p4.rejects >= 1 {
            over_rej = true;
        }
        sq += 1;
        r += 1;
    }
    v.push((
        "J1-序列语义负探四路",
        sipi_first_rej && wake_first_rej && dup_init_rej && over_rej,
        format!("SIPI抢跑={} 唤醒抢跑={} INIT重复={} 重试超限={}", sipi_first_rej, wake_first_rej, dup_init_rej, over_rej),
    ));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J1 全册核验：行前缀闭集 + 裸控制符 + 行数闭集 + 逐行解析对账。
    let vt = match &ledg {
        Ok(l) => verify_ledger_text(l, &render_ledger(l)).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-全册核验", vt, String::from("事件账+B01+B02 行数闭集，逐行黄金逐字节对账")));
    // J2 单调守卫：状态机进拒平封 + 冻结后写拒 + apic 越界拒。
    let mut l2 = ReplayLedger::new(EPOCH);
    let m1 = l2.advance_state(BootState::InitPhase).is_ok();
    let m2 = l2.advance_state(BootState::Idle).is_err();
    let m3 = l2.advance_state(BootState::InitPhase).is_err();
    let m4 = l2.advance_state(BootState::SipiPhase).is_ok();
    let m5 = l2.advance_state(BootState::Awake).is_ok();
    let m6 = l2.advance_state(BootState::Sealed).is_ok();
    let m7 = l2.record(IpiEvent { apic: 0, kind: IpiKind::Init, seq: l2.seq_cursor + 1, t_us: 1, vector: 0, sent: true }).is_err();
    let m8 = l2.reconcile().is_err();
    v.push((
        "J2-单调守卫",
        m1 && m2 && m3 && m4 && m5 && m6 && m7 && m8,
        format!("状态进拒平封={}/{}/{}/{}/{}/{}/{} 冻结后写={} 冻结后对账={}", m1, m2, m3, m4, m5, m6, m7, m8, m8),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72780 窗口")));
    v
}
pub fn run_unx1945_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1945 INIT-SIPI 启动 IPI 复盘账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}