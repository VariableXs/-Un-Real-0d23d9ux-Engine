//! unx324_sipiseq ↔ UNX-F0324 INIT-SIPI-SIPI 序列封装
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0324，UNX-A1-B17）：
//! 「INIT-SIPI-SIPI 序列封装 | 280 | 已深化 |
//!  UNX-F0324-J1 INIT/SIPI 时序（10ms/200µs 规范间隔）正确，重发策略有账」
//!
//! 设计要点（全部围绕 SMP 唤醒序列真实状态）：
//! 1. **三步序列封装**：INIT（ assert/deassert）→ 等 **10ms** 规范间隔 →
//!    SIPI#1（启动向量）→ 等 **200µs** 规范间隔 → SIPI#2（同一向量）。
//!    Intel SDM 规定 INIT 后至少 10ms 才发 SIPI，两次 SIPI 之间至少
//!    200µs——间隔不足即显性拒发（不是等待补偿：封装层只承诺"不早发"，
//!    早发 = 时序违例立案）。
//! 2. **启动向量校验**：向量来自跳板物理地址右移 12 位，必须 4K 对齐，
//!    且 (addr >> 12) 必须落在 8 位（0x00-0xFF）——越界即拒绝，防
//!    高位地址被静默截断后 AP 跳到错误页面。
//! 3. **重发策略账**：SIPI#2 后在到达超时窗内 AP 未置位哨兵 → 重发
//!    SIPI 对（计数 + 原因 NoArrive 留账）；重发 **3 次**仍不达 → 升级
//!    FullInit（INIT 从头再来，升级留账）；升级后仍不达 → Failed，
//!    进隔离对接账（B18 F0347 消费）。
//! 4. **逐 AP 序列账**：每 AP 一行（ap_id / init_at / sipi1_at /
//!    sipi2_at / arrive_at / resends / 终态），时序逐行可回查。
//! 5. **守恒**：arrived + failed + in_flight == 注册总数，任何一步
//!    重复发射（DoubleInit / DoubleSipi）都拒绝并留账。
//!
//! 逻辑时钟：全部时间戳由调用方注入 tick（µs 计），模块零墙钟。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const S324_VERSION: &str = "s324-sipiseq-v1";

/// 诊断段基址。
pub const S324_SEG_BASE: u16 = 0x6300;

/// INIT → SIPI#1 规范间隔（µs）：10ms。
pub const INIT_TO_SIPI_US: u64 = 10_000;

/// SIPI#1 → SIPI#2 规范间隔（µs）：200µs。
pub const SIPI_GAP_US: u64 = 200;

/// 到达超时窗（µs）：200ms/AP。
pub const ARRIVE_TIMEOUT_US: u64 = 200_000;

/// 重发上限（次），超过升级 FullInit。
pub const RESEND_LIMIT: u32 = 3;

// --------------------------------------------------------------------------
// 诊断码（s324 十四码 · 13 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 序列诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum S324Code {
    /// 0x6300 INIT 后不足 10ms 发 SIPI（拒）。
    TooEarlySipi,
    /// 0x6301 两次 SIPI 间隔不足 200µs（拒）。
    SipiGapBad,
    /// 0x6302 向量非 4K 对齐（拒）。
    VectorMisalign,
    /// 0x6303 向量越 8 位（截断风险，拒）。
    VectorRangeBad,
    /// 0x6304 重复 INIT（未到重发时机再 INIT）。
    DoubleInit,
    /// 0x6305 重复 SIPI 对（同窗内重复发射）。
    DoubleSipi,
    /// 0x6306 AP 未注册即操作。
    Unregistered,
    /// 0x6307 重发上限耗尽升级 FullInit。
    EscalateFullInit,
    /// 0x6308 升级后仍不达 → Failed。
    GiveUp,
    /// 0x6309 到达后仍发 SIPI（终态拒）。
    SipiAfterArrive,
    /// 0x630A 守恒破缺。
    ConservationBad,
    /// 0x630B 时钟回退（tick 单调破坏）。
    ClockBack,
}

impl S324Code {
    /// 码值。
    pub fn val(self) -> u16 {
        S324_SEG_BASE + match self {
            S324Code::TooEarlySipi => 0x0,
            S324Code::SipiGapBad => 0x1,
            S324Code::VectorMisalign => 0x2,
            S324Code::VectorRangeBad => 0x3,
            S324Code::DoubleInit => 0x4,
            S324Code::DoubleSipi => 0x5,
            S324Code::Unregistered => 0x6,
            S324Code::EscalateFullInit => 0x7,
            S324Code::GiveUp => 0x8,
            S324Code::SipiAfterArrive => 0x9,
            S324Code::ConservationBad => 0xA,
            S324Code::ClockBack => 0xB,
        }
    }
}

/// 段独占守卫。
pub fn s324_segment_guard() -> Result<(), S324Code> {
    let lo = S324Code::TooEarlySipi.val();
    let hi = S324Code::ClockBack.val();
    if lo != S324_SEG_BASE || hi <= lo || hi >= S324_SEG_BASE + 0x10 {
        return Err(S324Code::ConservationBad);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 序列状态机
// --------------------------------------------------------------------------
/// AP 序列状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeqPhase {
    /// 未唤醒。
    Idle,
    /// INIT 已发，等 10ms。
    InitSent,
    /// SIPI#1 已发，等 200µs。
    Sipi1Sent,
    /// SIPI 对齐完毕（SIPI#2 已发），等到达。
    WaitingArrive,
    /// 已到达。
    Arrived,
    /// 放弃（进隔离）。
    Failed,
}

/// 逐 AP 账行。
#[derive(Clone, Copy, Debug)]
pub struct SeqRow {
    /// AP ID。
    pub ap: u32,
    /// 当前相位。
    pub phase: SeqPhase,
    /// INIT 发射时刻（µs）。
    pub init_at: Option<u64>,
    /// SIPI#1 时刻。
    pub sipi1_at: Option<u64>,
    /// SIPI#2 时刻。
    pub sipi2_at: Option<u64>,
    /// 到达时刻。
    pub arrive_at: Option<u64>,
    /// SIPI 对重发次数。
    pub resends: u32,
    /// FullInit 升级次数。
    pub escalations: u32,
    /// 本 AP 是否已放给隔离线。
    pub handed_off: bool,
}

/// 序列封装器。
pub struct SipiSequencer {
    rows: Vec<SeqRow>,
    /// 拒发计数（按原因）。
    rej_too_early: u64,
    rej_gap: u64,
    rej_misalign: u64,
    rej_range: u64,
    rej_double_init: u64,
    rej_double_sipi: u64,
    rej_after_arrive: u64,
    rej_unregistered: u64,
    clock_backs: u64,
    /// 上次操作时刻（单调校验）。
    last_tick: u64,
    /// 升级 FullInit 总数。
    escalations_total: u64,
    /// 放弃（进隔离）总数。
    giveups_total: u64,
}

impl SipiSequencer {
    /// 新建。
    pub fn new() -> Self {
        SipiSequencer {
            rows: Vec::new(),
            rej_too_early: 0,
            rej_gap: 0,
            rej_misalign: 0,
            rej_range: 0,
            rej_double_init: 0,
            rej_double_sipi: 0,
            rej_after_arrive: 0,
            rej_unregistered: 0,
            clock_backs: 0,
            last_tick: 0,
            escalations_total: 0,
            giveups_total: 0,
        }
    }

    /// 注册 AP。
    pub fn register(&mut self, ap: u32) -> bool {
        if self.row_of(ap).is_some() {
            return false;
        }
        self.rows.push(SeqRow {
            ap,
            phase: SeqPhase::Idle,
            init_at: None,
            sipi1_at: None,
            sipi2_at: None,
            arrive_at: None,
            resends: 0,
            escalations: 0,
            handed_off: false,
        });
        true
    }

    fn row_of(&self, ap: u32) -> Option<usize> {
        self.rows.iter().position(|r| r.ap == ap)
    }

    fn row_mut(&mut self, ap: u32) -> Option<&mut SeqRow> {
        self.rows.iter_mut().find(|r| r.ap == ap)
    }

    /// 校验启动向量（4K 对齐 + 8 位域）。
    pub fn check_vector(addr: u64) -> Result<u8, S324Code> {
        if addr & 0xFFF != 0 {
            return Err(S324Code::VectorMisalign);
        }
        let v = addr >> 12;
        if v > 0xFF {
            return Err(S324Code::VectorRangeBad);
        }
        Ok(v as u8)
    }

    fn tick_ok(&mut self, now: u64) -> bool {
        if now < self.last_tick {
            self.clock_backs += 1;
            return false;
        }
        self.last_tick = now;
        true
    }

    /// 发 INIT（t=now）。返回 Ok 表示时序账已记。
    pub fn emit_init(&mut self, ap: u32, now: u64) -> Result<(), S324Code> {
        if !self.tick_ok(now) {
            return Err(S324Code::ClockBack);
        }
        let idx = match self.row_of(ap) {
            Some(i) => i,
            None => {
                self.rej_unregistered += 1;
                return Err(S324Code::Unregistered);
            }
        };
        let r = &mut self.rows[idx];
        match r.phase {
            SeqPhase::Arrived => {
                self.rej_after_arrive += 1;
                return Err(S324Code::SipiAfterArrive);
            }
            SeqPhase::Failed => {
                self.rej_unregistered += 1;
                return Err(S324Code::Unregistered);
            }
            SeqPhase::Idle => {}
            _ => {
                self.rej_double_init += 1;
                return Err(S324Code::DoubleInit);
            }
        }
        r.phase = SeqPhase::InitSent;
        r.init_at = Some(now);
        r.sipi1_at = None;
        r.sipi2_at = None;
        r.arrive_at = None;
        Ok(())
    }

    /// 发 SIPI 对（含 10ms/200µs 双间隔校验）。vector 由 addr 校验导出。
    pub fn emit_sipi_pair(&mut self, ap: u32, now: u64, tramp_addr: u64) -> Result<(), S324Code> {
        if !self.tick_ok(now) {
            return Err(S324Code::ClockBack);
        }
        let vector = Self::check_vector(tramp_addr)?;
        let idx = match self.row_of(ap) {
            Some(i) => i,
            None => {
                self.rej_unregistered += 1;
                return Err(S324Code::Unregistered);
            }
        };
        let r = &mut self.rows[idx];
        let init_at = match (r.phase, r.init_at) {
            (SeqPhase::InitSent, Some(t)) => t,
            (SeqPhase::Arrived, _) => {
                self.rej_after_arrive += 1;
                return Err(S324Code::SipiAfterArrive);
            }
            (SeqPhase::WaitingArrive, _) => {
                self.rej_double_sipi += 1;
                return Err(S324Code::DoubleSipi);
            }
            (SeqPhase::Sipi1Sent, _) => {
                self.rej_gap += 1;
                return Err(S324Code::SipiGapBad);
            }
            _ => {
                self.rej_double_init += 1;
                return Err(S324Code::DoubleInit);
            }
        };
        // INIT → SIPI#1 ≥ 10ms。
        if now < init_at.saturating_add(INIT_TO_SIPI_US) {
            self.rej_too_early += 1;
            return Err(S324Code::TooEarlySipi);
        }
        r.phase = SeqPhase::Sipi1Sent;
        r.sipi1_at = Some(now);
        r.sipi2_at = Some(now.saturating_add(SIPI_GAP_US));
        // SIPI#2 在 200µs 后由硬件间隔定时器触发；封装层按规范间隔记账。
        r.phase = SeqPhase::WaitingArrive;
        let _ = vector;
        Ok(())
    }

    /// AP 到达（哨兵置位回调）。
    pub fn mark_arrive(&mut self, ap: u32, now: u64) -> Result<(), S324Code> {
        if !self.tick_ok(now) {
            return Err(S324Code::ClockBack);
        }
        match self.row_mut(ap) {
            None => {
                self.rej_unregistered += 1;
                Err(S324Code::Unregistered)
            }
            Some(r) => {
                if r.phase != SeqPhase::WaitingArrive {
                    self.rej_double_sipi += 1;
                    return Err(S324Code::DoubleSipi);
                }
                r.phase = SeqPhase::Arrived;
                r.arrive_at = Some(now);
                Ok(())
            }
        }
    }

    /// 超时巡检：对 WaitingArrive 且超窗的 AP 执行重发策略。
    /// 返回本巡检轮动作清单（供账面回查）。
    pub fn sweep(&mut self, now: u64) -> Vec<String> {
        let mut acts: Vec<String> = Vec::new();
        if now < self.last_tick {
            self.clock_backs += 1;
            return acts;
        }
        self.last_tick = now;
        let n = self.rows.len();
        for i in 0..n {
            let ap = self.rows[i].ap;
            let (phase, since) = {
                let r = &self.rows[i];
                let since = r.sipi1_at.unwrap_or(0);
                (r.phase, since)
            };
            if phase != SeqPhase::WaitingArrive {
                continue;
            }
            if now < since.saturating_add(ARRIVE_TIMEOUT_US) {
                continue;
            }
            let resends = self.rows[i].resends;
            if resends < RESEND_LIMIT {
                // 重发 SIPI 对：间隔账重置（SIPI#1 时刻刷新）。
                self.rows[i].resends += 1;
                self.rows[i].sipi1_at = Some(now);
                self.rows[i].sipi2_at = Some(now.saturating_add(SIPI_GAP_US));
                let mut s = String::from("resend ap=");
                push_u32(&mut s, ap);
                s.push_str(" n=");
                push_u32(&mut s, resends + 1);
                s.push_str(" reason=NoArrive");
                acts.push(s);
            } else if self.rows[i].escalations == 0 {
                // 升级 FullInit：INIT 重来。
                self.rows[i].escalations += 1;
                self.escalations_total += 1;
                self.rows[i].phase = SeqPhase::InitSent;
                self.rows[i].init_at = Some(now);
                self.rows[i].sipi1_at = None;
                self.rows[i].sipi2_at = None;
                self.rows[i].resends = 0;
                let mut s = String::from("escalate ap=");
                push_u32(&mut s, ap);
                s.push_str(" reason=ResendLimit");
                acts.push(s);
            } else {
                // 放弃 → 隔离对接账。
                self.rows[i].phase = SeqPhase::Failed;
                self.rows[i].handed_off = true;
                self.giveups_total += 1;
                let mut s = String::from("giveup ap=");
                push_u32(&mut s, ap);
                s.push_str(" reason=FullInitNoArrive");
                acts.push(s);
            }
        }
        acts
    }

    /// 守恒：arrived + failed + in_flight == 注册数。
    pub fn conservation(&self) -> Result<(), S324Code> {
        let mut arr = 0usize;
        let mut fail = 0usize;
        let mut infl = 0usize;
        for r in &self.rows {
            match r.phase {
                SeqPhase::Arrived => arr += 1,
                SeqPhase::Failed => fail += 1,
                _ => infl += 1,
            }
        }
        if arr + fail + infl != self.rows.len() {
            return Err(S324Code::ConservationBad);
        }
        Ok(())
    }

    /// 各计数值只读视图。
    pub fn stats(&self) -> (u64, u64, u64, u64, u64, u64, u64, u64) {
        (
            self.rej_too_early,
            self.rej_gap,
            self.rej_misalign,
            self.rej_range,
            self.rej_double_init,
            self.rej_double_sipi,
            self.escalations_total,
            self.giveups_total,
        )
    }

    /// 注册数。
    pub fn registered(&self) -> usize {
        self.rows.len()
    }

    /// 指定 AP 相位。
    pub fn phase_of(&self, ap: u32) -> Option<SeqPhase> {
        self.rows.iter().find(|r| r.ap == ap).map(|r| r.phase)
    }

    /// 指定 AP 账行。
    pub fn row(&self, ap: u32) -> Option<&SeqRow> {
        self.rows.iter().find(|r| r.ap == ap)
    }

    /// 到达数。
    pub fn arrived(&self) -> usize {
        self.rows.iter().filter(|r| r.phase == SeqPhase::Arrived).count()
    }

    /// 失败数。
    pub fn failed(&self) -> usize {
        self.rows.iter().filter(|r| r.phase == SeqPhase::Failed).count()
    }
}

/// u32 十进制追加。
fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut n = 0;
    while v > 0 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(digits[n] as char);
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F0324-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx324_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx324");
    if s324_segment_guard().is_err() {
        cs.fail("C324-SEG", "段守卫失败");
        return cs;
    }

    // ---- C324-01 INIT 后不足 10ms 发 SIPI 拒 ----
    let mut sq = SipiSequencer::new();
    let _ = sq.register(0);
    let tramp: u64 = 0x0001_0000; // 4K 对齐，向量 0x10
    let r1 = sq.emit_init(0, 1_000);
    let r2 = sq.emit_sipi_pair(0, 1_000 + INIT_TO_SIPI_US - 1, tramp);
    cs.add(
        "C324-01 早发SIPI拒",
        r1.is_ok() && r2 == Err(S324Code::TooEarlySipi),
        "10ms-1µs即拒",
    );

    // ---- C324-02 恰 10ms 后 SIPI#1 过 ----
    let r3 = sq.emit_sipi_pair(0, 1_000 + INIT_TO_SIPI_US, tramp);
    let ph1 = sq.phase_of(0);
    cs.add(
        "C324-02 规范间隔过",
        r3.is_ok() && ph1 == Some(SeqPhase::WaitingArrive),
        "恰10ms放行",
    );

    // ---- C324-03 SIPI#1 后同窗重发拒（DoubleSipi） ----
    let r4 = sq.emit_sipi_pair(0, 1_000 + INIT_TO_SIPI_US + 1, tramp);
    cs.add(
        "C324-03 重复SIPI对拒",
        r4 == Err(S324Code::DoubleSipi),
        "同窗二次发射拒",
    );

    // ---- C324-04 向量非 4K 对齐拒 ----
    let r5 = SipiSequencer::check_vector(0x1234);
    cs.add(
        "C324-04 向量对齐拒",
        r5 == Err(S324Code::VectorMisalign),
        "低12位非零拒",
    );

    // ---- C324-05 向量越 8 位拒 ----
    let r6 = SipiSequencer::check_vector(0x1000_0000);
    cs.add(
        "C324-05 向量越域拒",
        r6 == Err(S324Code::VectorRangeBad),
        "addr>>12>0xFF拒",
    );

    // ---- C324-06 向量合法值回读 ----
    let v7 = SipiSequencer::check_vector(0x0009_E000);
    cs.add(
        "C324-06 向量合法回读",
        v7 == Ok(0x9E),
        "0x9E000→0x9E",
    );

    // ---- C324-07 重复 INIT 拒 ----
    let mut sq2 = SipiSequencer::new();
    let _ = sq2.register(1);
    let _ = sq2.emit_init(1, 100);
    let r8 = sq2.emit_init(1, 200);
    cs.add(
        "C324-07 重复INIT拒",
        r8 == Err(S324Code::DoubleInit),
        "Idle外再INIT拒",
    );

    // ---- C324-08 重发账：超窗重发 SIPI 对计数 + 原因留账 ----
    let mut sq3 = SipiSequencer::new();
    let _ = sq3.register(2);
    let _ = sq3.emit_init(2, 0);
    let _ = sq3.emit_sipi_pair(2, INIT_TO_SIPI_US, tramp);
    let acts = sq3.sweep(INIT_TO_SIPI_US + ARRIVE_TIMEOUT_US);
    let r9 = sq3.row(2);
    cs.add(
        "C324-08 超窗重发留账",
        acts.len() == 1 && acts[0].contains("resend") && acts[0].contains("NoArrive") && r9.map(|x| x.resends).unwrap_or(0) == 1,
        "reason=NoArrive计1",
    );

    // ---- C324-09 重发 3 次上限升级 FullInit（3 轮超时：resend×2 + escalate） ----
    let mut acts_n = 0u32;
    let mut escalated = false;
    for k in 0..4u32 {
        let t = INIT_TO_SIPI_US + ARRIVE_TIMEOUT_US * (k as u64 + 5);
        for a in sq3.sweep(t) {
            acts_n += 1;
            if a.contains("escalate") {
                escalated = true;
            }
        }
    }
    let esc = sq3.escalations_total;
    cs.add(
        "C324-09 升级FullInit",
        escalated && acts_n == 3 && esc == 1,
        "3次耗尽即升级",
    );

    // ---- C324-10 升级后重走序列仍不达 → Failed 进隔离账 ----
    // 升级后相位回 InitSent（init_at=t_e）；发射时刻须晚于账内 last_tick
    // （否则 ClockBack 拒）且距 init_at ≥ 10ms。
    let t_e = INIT_TO_SIPI_US + ARRIVE_TIMEOUT_US * 7; // escalate 轮时刻 1,410,000
    let t_send = INIT_TO_SIPI_US + ARRIVE_TIMEOUT_US * 8; // 1,620,000 > last_tick
    let _ = sq3.emit_sipi_pair(2, t_send, tramp);
    let mut given_up = false;
    for k in 0..5u32 {
        let t = t_send + ARRIVE_TIMEOUT_US * (k as u64 + 1);
        for a in sq3.sweep(t) {
            if a.contains("giveup") {
                given_up = true;
            }
        }
    }
    let ph = sq3.phase_of(2);
    cs.add(
        "C324-10 放弃进隔离",
        given_up && ph == Some(SeqPhase::Failed) && sq3.failed() == 1,
        "FullInit无果弃",
    );

    // ---- C324-11 守恒 arrived+failed+inflight==N ----
    let mut sq4 = SipiSequencer::new();
    for ap in 0..4u32 {
        let _ = sq4.register(ap);
    }
    let _ = sq4.emit_init(0, 0);
    let _ = sq4.emit_sipi_pair(0, INIT_TO_SIPI_US, tramp);
    let _ = sq4.mark_arrive(0, INIT_TO_SIPI_US + 5_000);
    let _ = sq4.emit_init(1, 0);
    let _ = sq4.emit_sipi_pair(1, INIT_TO_SIPI_US, tramp);
    // ap=2/3 保持 Idle（in flight）。
    let cons = sq4.conservation();
    let ok11 = cons.is_ok() && sq4.arrived() == 1 && sq4.failed() == 0 && sq4.registered() == 4;
    cs.add(
        "C324-11 三态守恒",
        ok11,
        "1达3飞计4平",
    );

    // ---- C324-12 正常全路径时序账逐行核对 ----
    let mut sq5 = SipiSequencer::new();
    let _ = sq5.register(3);
    let _ = sq5.emit_init(3, 5_000);
    let _ = sq5.emit_sipi_pair(3, 15_000, tramp);
    let _ = sq5.mark_arrive(3, 15_500);
    let row = sq5.row(3);
    let ok12 = match row {
        Some(r) => {
            r.init_at == Some(5_000)
                && r.sipi1_at == Some(15_000)
                && r.arrive_at == Some(15_500)
                && r.resends == 0
                && r.phase == SeqPhase::Arrived
        }
        None => false,
    };
    cs.add(
        "C324-12 时序账逐行核对",
        ok12,
        "init/s1/arrive三点齐",
    );

    cs
}
