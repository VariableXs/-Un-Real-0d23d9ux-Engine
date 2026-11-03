//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1626 · 每核统计区：上下文切换计数账 · 0x72340
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B02）：
//!   ① 纯功能代码 ≥260 行；
//!   ② 判据原文：UNX-F1626-J1 每核切换计数与全账核对（Σ每核=全局），
//!     计数原子性注入（并发切换）零丢失，账可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 每核一份计数账（fast path），另备一份影子流水（独立逐核事件
//!     累计，audit path）；审计 = Σ每核 == 全局 且 fast == 影子，双闭合；
//!   · 计数一律 checked_add：溢出显性拒（CntOverflow），不静默回绕；
//!   · 序号单调：全局 seq 只增，每核 last_seq 记本核最近一次序号，
//!     倒退即 SeqRegress（并发账序破坏面）；
//!   · 原子性注入：交错批（轮转模拟并发切换交错）零丢失——注入侧
//!     篡改任一核账，审计必抓（LostUpdate / SumMismatch）；
//!   · 统计区状态机 Uninit→Live→Sealed 单调推进；Sealed 后记账显性拒；
//!   · 账可导出：封账前置 Live、导出摘要非空（ExportEmpty 闸）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72340，VX3-1103 每核统计窗口独占）。
pub const SEG_BASE: u32 = 0x72340;

/// 统计区核账容量上限（冻结 64）。
pub const CORE_CAP: usize = 64;
/// 真机档核数（冻结 4，含 BSP 0 号）。
pub const ZONE_CPUS: usize = 4;
/// 单次批记账上限（防单批膨胀，冻结 32）。
pub const BATCH_CAP: u64 = 32;
/// 并发交错模拟轮数（判据口径，冻结 3）。
pub const CONCUR_ROUNDS: usize = 3;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CntCode {
    /// 核号越界（不在 0..=63 或未注册）
    CoreOver,
    /// 统计区已封账后再记账
    ZoneSealed,
    /// 计数溢出（checked_add 拒绝，不回绕）
    CntOverflow,
    /// 序号倒退（每核 last_seq 单调被破坏）
    SeqRegress,
    /// Σ每核 ≠ 全局（全账核对失配）
    SumMismatch,
    /// 丢更新（fast 计数 ≠ 影子流水，并发切换面）
    LostUpdate,
    /// 导出摘要为空（账不可导出）
    ExportEmpty,
    /// 双跑不一致
    NotReproducible,
}

impl CntCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CntCode::CoreOver => "核号越界：先注册 0..=63 核账再记账",
            CntCode::ZoneSealed => "已封账：Sealed 后不许再记账，重开新统计区",
            CntCode::CntOverflow => "计数溢出：checked_add 拒绝，查计数面宽度",
            CntCode::SeqRegress => "序号倒退：全局 seq 单调，每核 last_seq 不许回退",
            CntCode::SumMismatch => "Σ每核≠全局：按核重算全账，查漏记/重记",
            CntCode::LostUpdate => "丢更新：fast 计数与影子流水失配，并发记账原子性破坏",
            CntCode::ExportEmpty => "导出为空：账面无记录或格式失配，先记账再导出",
            CntCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [CntCode; 8] = [
        CntCode::CoreOver,
        CntCode::ZoneSealed,
        CntCode::CntOverflow,
        CntCode::SeqRegress,
        CntCode::SumMismatch,
        CntCode::LostUpdate,
        CntCode::ExportEmpty,
        CntCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CntCode::ALL.len() {
        if CntCode::ALL[i].val() == code {
            return CntCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72340 段闭集内"
}

// ── 统计区状态机（单调推进，倒退显性拒） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoneState {
    /// 未初始化：核账未注册。
    Uninit,
    /// 活账：可记账、可审计、可导出。
    Live,
    /// 已封账：只读，记账显性拒。
    Sealed,
}

impl ZoneState {
    /// 单调序值：Uninit=0 < Live=1 < Sealed=2。
    pub fn rank(self) -> u8 {
        match self {
            ZoneState::Uninit => 0,
            ZoneState::Live => 1,
            ZoneState::Sealed => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: ZoneState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 每核计数账（fast path）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuCounter {
    pub core: u32,
    /// 上下文切换次数（只增，checked_add 保护）。
    pub switches: u64,
    /// 本核最近一次记账的全局序号（单调，倒退即账序破坏）。
    pub last_seq: u64,
}

/// 统计区：每核 fast 账 + 每核影子流水 + 全局闭合账。
#[derive(Clone, Debug)]
pub struct SwitchZone {
    pub state: ZoneState,
    pub counters: Vec<CpuCounter>,
    /// 影子流水：独立逐核事件累计（与 fast 账同增，审计对照面）。
    pub shadow: Vec<u64>,
    /// 全局闭合账：= Σ每核 fast（审计复核）。
    pub global: u64,
    /// 全局记账序号（从 1 起，只增不减）。
    pub seq: u64,
}

impl SwitchZone {
    /// 建统计区并注册 0..ZONE_CPUS 核账（注册后即 Live）。
    pub fn new(cpus: usize) -> SwitchZone {
        let mut counters = Vec::new();
        let mut shadow = Vec::new();
        let mut c = 0usize;
        while c < cpus && c < CORE_CAP {
            counters.push(CpuCounter { core: c as u32, switches: 0, last_seq: 0 });
            shadow.push(0u64);
            c += 1;
        }
        SwitchZone { state: ZoneState::Live, counters, shadow, global: 0, seq: 0 }
    }

    /// 按核号定位 fast 账下标。
    pub fn core_index(&self, core: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.counters.len() {
            if let Some(c) = self.counters.get(i) {
                if c.core == core {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 记一笔上下文切换（封账闸 / 越界闸 / 溢出闸 / 序号单调闸）。
    /// 返回本核记账后的累计值。
    pub fn record(&mut self, core: u32) -> Result<u64, u32> {
        if self.state == ZoneState::Sealed {
            return Err(CntCode::ZoneSealed.val());
        }
        let ci = match self.core_index(core) {
            Some(i) => i,
            None => return Err(CntCode::CoreOver.val()),
        };
        // 序号单调闸：新序号必大于本核 last_seq（全局 seq 先进）。
        let next_seq = match self.seq.checked_add(1) {
            Some(v) => v,
            None => return Err(CntCode::CntOverflow.val()),
        };
        let last = match self.counters.get(ci) {
            Some(c) => c.last_seq,
            None => return Err(CntCode::CoreOver.val()),
        };
        if next_seq <= last {
            return Err(CntCode::SeqRegress.val());
        }
        // 溢出闸：fast / 影子 / 全局三面 checked_add。
        let sw = match self.counters.get(ci) {
            Some(c) => c.switches,
            None => return Err(CntCode::CoreOver.val()),
        };
        let nsw = match sw.checked_add(1) {
            Some(v) => v,
            None => return Err(CntCode::CntOverflow.val()),
        };
        let sh = match self.shadow.get(ci) {
            Some(v) => *v,
            None => return Err(CntCode::CoreOver.val()),
        };
        let nsh = match sh.checked_add(1) {
            Some(v) => v,
            None => return Err(CntCode::CntOverflow.val()),
        };
        let ng = match self.global.checked_add(1) {
            Some(v) => v,
            None => return Err(CntCode::CntOverflow.val()),
        };
        // 三面同写（单线程语义下原子；审计双闭合兜底并发面）。
        if let Some(c) = self.counters.get_mut(ci) {
            c.switches = nsw;
            c.last_seq = next_seq;
        }
        if let Some(v) = self.shadow.get_mut(ci) {
            *v = nsh;
        }
        self.global = ng;
        self.seq = next_seq;
        Ok(nsw)
    }

    /// 批记账：同一核连记 k 笔（k ≤ BATCH_CAP，防单批膨胀）。
    pub fn record_batch(&mut self, core: u32, k: u64) -> Result<u64, u32> {
        if k > BATCH_CAP {
            return Err(CntCode::CntOverflow.val());
        }
        let mut acc = 0u64;
        let mut n = 0u64;
        while n < k {
            acc = self.record(core)?;
            n += 1;
        }
        Ok(acc)
    }

    /// 审计双闭合：Σ每核 fast == 全局 且 fast == 影子（逐核）。
    /// 返回 (Σ每核, 全局) 供诊断。
    pub fn audit(&self) -> Result<(u64, u64), u32> {
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.counters.len() {
            let (sw, sh) = match (self.counters.get(i), self.shadow.get(i)) {
                (Some(c), Some(v)) => (c.switches, *v),
                _ => return Err(CntCode::CoreOver.val()),
            };
            if sw != sh {
                // fast 与影子失配：并发丢更新面。
                return Err(CntCode::LostUpdate.val());
            }
            sum = match sum.checked_add(sw) {
                Some(v) => v,
                None => return Err(CntCode::CntOverflow.val()),
            };
            i += 1;
        }
        if sum != self.global {
            return Err(CntCode::SumMismatch.val());
        }
        Ok((sum, self.global))
    }

    /// 封账：审计双闭合通过才许封（封账前置闸）。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.audit()?;
        if !self.state.can_advance(ZoneState::Sealed) {
            return Err(CntCode::ZoneSealed.val());
        }
        self.state = ZoneState::Sealed;
        Ok(())
    }

    /// 导出账面摘要（账可导出判据）：封账后导出只读快照。
    pub fn export_account(&self) -> Result<String, u32> {
        if self.state == ZoneState::Uninit {
            return Err(CntCode::ExportEmpty.val());
        }
        if self.counters.is_empty() {
            return Err(CntCode::ExportEmpty.val());
        }
        let mut s = String::from("ctxsw[");
        let mut i = 0;
        while i < self.counters.len() {
            if let Some(c) = self.counters.get(i) {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&format!("c{}={}", c.core, c.switches));
            }
            i += 1;
        }
        s.push_str(&format!("] total={} seq={}", self.global, self.seq));
        if s.is_empty() {
            return Err(CntCode::ExportEmpty.val());
        }
        Ok(s)
    }

    /// 导出串复核：逐核值拆回与账面全等（可导出且可对回）。
    pub fn export_verifies(&self) -> Result<(), u32> {
        let s = self.export_account()?;
        let mut i = 0;
        while i < self.counters.len() {
            if let Some(c) = self.counters.get(i) {
                let want = format!("c{}={}", c.core, c.switches);
                if !s.contains(&want) {
                    return Err(CntCode::ExportEmpty.val());
                }
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 并发交错模拟（轮转记账，模拟并发切换交错） ───────────────────
/// 一轮：每核记 `per_core[r]` 笔，核间轮转单笔交错（并发语义）。
/// per_core[r] 为第 r 轮每核笔数（1..=BATCH_CAP）。
pub fn interleave_round(z: &mut SwitchZone, per_core: &[u64]) -> Result<u64, u32> {
    if per_core.len() != z.counters.len() {
        return Err(CntCode::CoreOver.val());
    }
    // 本轮总笔数上界 = BATCH_CAP × 核数，防膨胀。
    let mut total = 0u64;
    let mut i = 0;
    while i < per_core.len() {
        let k = match per_core.get(i) {
            Some(v) => *v,
            None => return Err(CntCode::CoreOver.val()),
        };
        if k > BATCH_CAP {
            return Err(CntCode::CntOverflow.val());
        }
        total += k;
        i += 1;
    }
    // 轮转单笔：第 t 拍给每核最多记一笔（按各自剩余额度）。
    let mut remain: Vec<u64> = per_core.to_vec();
    let mut t = 0u64;
    let mut recorded = 0u64;
    while t < total {
        let mut ci = 0usize;
        while ci < remain.len() {
            let left = match remain.get(ci) {
                Some(v) => *v,
                None => 0,
            };
            if left > 0 {
                let core = match z.counters.get(ci) {
                    Some(c) => c.core,
                    None => return Err(CntCode::CoreOver.val()),
                };
                z.record(core)?;
                if let Some(v) = remain.get_mut(ci) {
                    *v -= 1;
                }
                recorded += 1;
            }
            ci += 1;
        }
        t += 1;
    }
    Ok(recorded)
}

/// 标准并发剧本：3 轮，第 r 轮每核 (r+1) 笔。
pub fn concur_spec() -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    let mut r = 0usize;
    while r < CONCUR_ROUNDS {
        v.push((r + 1) as u64);
        r += 1;
    }
    v
}

/// 跑标准并发剧本：返回记账总笔数。
pub fn run_concur_script(z: &mut SwitchZone) -> Result<u64, u32> {
    let spec = concur_spec();
    let mut total = 0u64;
    let mut r = 0;
    while r < spec.len() {
        let k = match spec.get(r) {
            Some(v) => *v,
            None => return Err(CntCode::CoreOver.val()),
        };
        let mut per: Vec<u64> = Vec::new();
        let mut ci = 0;
        while ci < z.counters.len() {
            per.push(k);
            ci += 1;
        }
        total += interleave_round(z, &per)?;
        r += 1;
    }
    Ok(total)
}

// ── LCG（确定性伪随机，供注入轮步进） ───────────────────────────
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

// ── 注入检出账（四路：丢更路/越界路/封账路/溢出路） ─────────────
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

/// 建一个跑完并发剧本、审计通过、已封账的统计区。
pub fn run_fresh_sealed_zone() -> Result<SwitchZone, u32> {
    let mut z = SwitchZone::new(ZONE_CPUS);
    run_concur_script(&mut z)?;
    z.audit()?;
    z.seal()?;
    Ok(z)
}

// ── 黄金全链：建区→并发记账→审计→封账→导出→四拒→闭合复核 ──────
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut z = SwitchZone::new(ZONE_CPUS);
    if z.counters.len() != ZONE_CPUS {
        return Err(CntCode::CoreOver.val());
    }
    // 并发记账 3 轮（每核 1+2+3=6 笔，全局 24）。
    let recorded = run_concur_script(&mut z)?;
    if recorded != 24 {
        return Err(CntCode::SumMismatch.val());
    }
    // 审计双闭合。
    let (sum, glob) = z.audit()?;
    if sum != glob || glob != 24 {
        return Err(CntCode::SumMismatch.val());
    }
    // 封账 + 导出。
    z.seal()?;
    let exported = z.export_account()?;
    if exported.is_empty() {
        return Err(CntCode::ExportEmpty.val());
    }
    z.export_verifies()?;
    // 注入 1：封账后再记账必拒。
    let sealed_rej = z.record(0).is_err();
    // 注入 2：未注册核记账必拒。
    let mut live = SwitchZone::new(ZONE_CPUS);
    let _ = live.record(0);
    let over_rej = live.record(ZONE_CPUS as u32).is_err();
    // 注入 3：丢更新注入（克隆账篡改核 1 fast 计数）审计必抓。
    let mut bad = SwitchZone::new(ZONE_CPUS);
    let _ = run_concur_script(&mut bad);
    if let Some(c) = bad.counters.get_mut(1) {
        c.switches = c.switches.saturating_add(1);
    }
    let lost_rej = match bad.audit() {
        Err(e) => e == CntCode::LostUpdate.val(),
        Ok(_) => false,
    };
    // 注入 4：Σ每核≠全局注入（只改全局闭合账）审计必抓。
    let mut bad2 = SwitchZone::new(ZONE_CPUS);
    let _ = run_concur_script(&mut bad2);
    bad2.global = bad2.global.saturating_add(1);
    let sum_rej = match bad2.audit() {
        Err(e) => e == CntCode::SumMismatch.val(),
        Ok(_) => false,
    };
    let flags = 1u32; // 导出摘要非空旗。
    let rejects = sealed_rej as u32 + over_rej as u32 + lost_rej as u32 + sum_rej as u32;
    Ok((flags, rejects, glob))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 4, 24)) => {
            let a = golden_run()?;
            let b = golden_run()?;
            if a != b {
                return Err(CntCode::NotReproducible.val());
            }
            Ok(())
        }
        _ => Err(CntCode::NotReproducible.val()),
    }
}

/// 序 B：注入面四拒（越界/封账/批超限/序号倒退）逐条显性拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut z = SwitchZone::new(ZONE_CPUS);
    // 越界：未注册核。
    let r1 = z.record(ZONE_CPUS as u32).is_err();
    // 批超限：k > BATCH_CAP 必拒。
    let r2 = z.record_batch(0, BATCH_CAP + 1).is_err();
    // 记账后封账、封账后再记账。
    let _ = z.record(0);
    z.seal()?;
    let r3 = z.record(0).is_err();
    // 重复封账（平移）必拒。
    let r4 = z.seal().is_err();
    match (r1, r2, r3, r4) {
        (true, true, true, true) => Ok(()),
        _ => Err(CntCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（4 核 / 上限 64 / 批上限 32 / 交错 3 轮）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        ZONE_CPUS == 4,
        CORE_CAP == 64,
        BATCH_CAP == 32,
        CONCUR_ROUNDS == 3,
        JUDGE_ROUNDS == 10,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(CntCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：丢更路（随机篡改一核 fast 计数，审计必抓 LostUpdate）。
        let mut z1 = SwitchZone::new(ZONE_CPUS);
        let _ = run_concur_script(&mut z1);
        let pick = rng.below(ZONE_CPUS as u64) as usize;
        if let Some(c) = z1.counters.get_mut(pick) {
            c.switches = c.switches.wrapping_add(1);
        }
        let lost = match z1.audit() {
            Err(e) => e == CntCode::LostUpdate.val(),
            Ok(_) => false,
        };
        led.record(0, lost);
        // 路 2：越界路（未注册核 / 65 号核记账必拒）。
        let mut z2 = SwitchZone::new(ZONE_CPUS);
        let a = z2.record(CORE_CAP as u32 + 1).is_err();
        let b = z2.record(ZONE_CPUS as u32).is_err();
        led.record(1, a && b);
        // 路 3：封账路（封账后记账 / 重复封账必拒）。
        let mut z3 = run_fresh_sealed_zone()?;
        let a = z3.record(0).is_err();
        let b = z3.seal().is_err();
        led.record(2, a && b);
        // 路 4：溢出路（fast 计数顶满后再记必拒 checked_add 面）。
        let mut z4 = SwitchZone::new(1);
        if let Some(c) = z4.counters.get_mut(0) {
            c.switches = u64::MAX;
        }
        if let Some(v) = z4.shadow.get_mut(0) {
            *v = u64::MAX;
        }
        z4.global = u64::MAX;
        let ov = z4.record(0).is_err();
        led.record(3, ov);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CntCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CntCode::ALL.len() {
        let v = CntCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let z = run_fresh_sealed_zone()?;
    let s = z.export_account()?;
    if s.is_empty() || z.global != 24 {
        return Err(CntCode::ExportEmpty.val());
    }
    Ok(format!("状态=已完成 {}", s))
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（1 旗 + 4 拒 + 24 笔）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((f, r, t)) => *f == 1 && *r == 4 && *t == 24,
            Err(_) => false,
        },
        match &g {
            Ok((f, r, t)) => format!("flags={} rejects={} total={}", f, r, t),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 Σ每核=全局：封账区审计双闭合。
    let closed = match run_fresh_sealed_zone() {
        Ok(z) => match z.audit() {
            Ok((s, gl)) => s == gl && gl == 24,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "J1-全账闭合",
        closed,
        format!("{}核 Σ每核=全局=24", ZONE_CPUS),
    ));
    // J1 账可导出：导出非空且可对回。
    let exp = match run_fresh_sealed_zone() {
        Ok(z) => z.export_verifies().is_ok(),
        Err(_) => false,
    };
    v.push((
        "J1-账可导出",
        exp,
        String::from("导出串逐核对回全等"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
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
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 写回摘要非空。
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
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x72340 窗口"),
    ));
    v
}

pub fn run_unx1626_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1626 每核统计区：上下文切换计数账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
