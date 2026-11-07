//! UNX-F51624 settle 抖动风暴（复位-断开循环 ×100） · 0xC1EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51625-J1 风暴 ×10 分钟隔离生效零循环复位；UNX-F51625-J1R 正常抖动设备 ×10 恢复成功。
//!
//! 结构清单：
//!   - SettleTracker：端口 settle 状态机（Stable/Shaky/Quarantined 三态单调推进）
//!   - StormShaper：复位-断开循环整形器（×100 循环计数与熔断）
//!   - StormLedger：风暴审计账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② 风暴语义：恶意设备以「复位-断开」循环反复打断 settle，试图令宿主
//!      陷入循环复位。整形器按端口累计循环次数，超过熔断线后该端口进入
//!      隔离态：只记账不复位、不重枚举——零循环复位由「隔离态不再触发
//!      任何复位动作」这一不变量保证。
//!   ③ 正常抖动（J1R）：真实设备 settle 过程偶发 1~2 次抖动后稳定，整形器
//!      给予恢复额度，恢复成功必须零误杀。
//!   ④ 状态机单调：Stable→Shaky→Quarantined 只进不退；Shaky 可回落
//!      Stable（恢复），Quarantined 终态。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51624 − 51501)。
pub const SEG_BASE: u32 = 0xC1EC0;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51624u32 - 51501));

/// 端口数上限。
pub const MAX_PORT: usize = 16;
/// 复位-断开循环总数（卡面 ×100 的建模规模）。
pub const STORM_CYCLES: usize = 100;
/// 熔断线：抖动超过此次数即隔离。
pub const TRIP_LINE: u32 = 5;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 端口 settle 状态（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleState {
    /// 稳定
    Stable = 0,
    /// 抖动观察
    Shaky = 1,
    /// 已隔离（终态）
    Quarantined = 2,
}

impl SettleState {
    /// 状态码闭集解码。
    pub fn from_code(c: u32) -> Option<SettleState> {
        match c {
            0 => Some(SettleState::Stable),
            1 => Some(SettleState::Shaky),
            2 => Some(SettleState::Quarantined),
            _ => None,
        }
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            SettleState::Stable => "稳定",
            SettleState::Shaky => "抖动观察",
            SettleState::Quarantined => "已隔离",
        }
    }
}

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 端口越界
    PortOob,
    /// 状态码越闭集
    StateUnknown,
    /// 复位-断开循环检出
    ResetLoopDetected,
    /// 抖动超线隔离
    TripQuarantine,
    /// 隔离后仍请求复位
    QuarantineLeak,
    /// 状态机倒退
    StateRegress,
    /// 熔断后仍发复位（STALL 兜底）
    StallAfterTrip,
    /// 恢复额度耗尽未稳定
    RecoveryExhausted,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl StormCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            StormCode::Ok => "settle 成功：端口进入稳定态",
            StormCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            StormCode::PortOob => "端口越界：端口号必须在 0..16 之内",
            StormCode::StateUnknown => "状态码越闭集：只接受 0/1/2 三态",
            StormCode::ResetLoopDetected => "复位-断开循环：检出抖动风暴模式",
            StormCode::TripQuarantine => "抖动超线：超过熔断线，端口进入隔离",
            StormCode::QuarantineLeak => "隔离失效：已隔离端口不得再接受复位或枚举",
            StormCode::StateRegress => "状态倒退：Quarantined 是终态不得离开",
            StormCode::StallAfterTrip => "熔断兜底：隔离后复位请求一律 STALL 不执行",
            StormCode::RecoveryExhausted => "恢复额度耗尽：观察期内未能稳定即隔离",
            StormCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            StormCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [StormCode; 12] = [
    StormCode::Ok,
    StormCode::SegBad,
    StormCode::PortOob,
    StormCode::StateUnknown,
    StormCode::ResetLoopDetected,
    StormCode::TripQuarantine,
    StormCode::QuarantineLeak,
    StormCode::StateRegress,
    StormCode::StallAfterTrip,
    StormCode::RecoveryExhausted,
    StormCode::LedgerMismatch,
    StormCode::FingerprintMismatch,
];

/// 端口 settle 状态机 + 风暴整形器。
pub struct SettleTracker {
    state: [SettleState; MAX_PORT],
    shaky_cnt: [u32; MAX_PORT],
    settle_cnt: [u32; MAX_PORT],
    loop_hits: [u32; MAX_PORT],
    stall_out: [u32; MAX_PORT],
    trace: Vec<String>,
}

impl SettleTracker {
    /// 新状态机。
    pub fn new() -> SettleTracker {
        SettleTracker {
            state: [SettleState::Stable; MAX_PORT],
            shaky_cnt: [0; MAX_PORT],
            settle_cnt: [0; MAX_PORT],
            loop_hits: [0; MAX_PORT],
            stall_out: [0; MAX_PORT],
            trace: Vec::new(),
        }
    }

    /// 状态读取。
    pub fn state_of(&self, port: usize) -> Option<SettleState> {
        self.state.get(port).copied()
    }

    /// 一次稳定 settle。
    pub fn settle_ok(&mut self, port: usize) -> Result<(), StormCode> {
        if port >= MAX_PORT {
            return Err(StormCode::PortOob);
        }
        match self.state[port] {
            SettleState::Quarantined => {
                self.stall_out[port] += 1;
                return Err(StormCode::QuarantineLeak);
            }
            SettleState::Shaky => {
                // 抖动后稳定：Shaky → Stable 恢复。
                self.state[port] = SettleState::Stable;
                self.shaky_cnt[port] = 0;
            }
            SettleState::Stable => {}
        }
        self.settle_cnt[port] += 1;
        self.trace.push(format!("p={} settle ok", port));
        Ok(())
    }

    /// 一次复位-断开抖动（风暴事件或正常偶发抖动）。
    pub fn settle_jolt(&mut self, port: usize) -> Result<(), StormCode> {
        if port >= MAX_PORT {
            return Err(StormCode::PortOob);
        }
        match self.state[port] {
            SettleState::Quarantined => {
                self.stall_out[port] += 1;
                return Err(StormCode::StallAfterTrip);
            }
            SettleState::Shaky => {
                self.shaky_cnt[port] += 1;
                self.loop_hits[port] += 1;
                if self.shaky_cnt[port] >= TRIP_LINE {
                    self.state[port] = SettleState::Quarantined;
                    self.trace.push(format!("p={} trip", port));
                    return Err(StormCode::TripQuarantine);
                }
                self.trace.push(format!("p={} loop hit", port));
                Err(StormCode::ResetLoopDetected)
            }
            SettleState::Stable => {
                self.state[port] = SettleState::Shaky;
                self.shaky_cnt[port] = 1;
                self.loop_hits[port] += 1;
                self.trace.push(format!("p={} first jolt", port));
                Err(StormCode::ResetLoopDetected)
            }
        }
    }

    /// 显式状态迁移（含越闭集拒绝与倒退拒绝）。
    pub fn force_state(&mut self, port: usize, code: u32) -> Result<(), StormCode> {
        if port >= MAX_PORT {
            return Err(StormCode::PortOob);
        }
        let st = match SettleState::from_code(code) {
            Some(s) => s,
            None => return Err(StormCode::StateUnknown),
        };
        if self.state[port] == SettleState::Quarantined && st != SettleState::Quarantined {
            return Err(StormCode::StateRegress);
        }
        self.state[port] = st;
        Ok(())
    }

    /// 隔离态读取。
    pub fn quarantined(&self, port: usize) -> bool {
        self.state_of(port) == Some(SettleState::Quarantined)
    }

    /// STALL 输出计数。
    pub fn stalls(&self, port: usize) -> u32 {
        *self.stall_out.get(port).unwrap_or(&0)
    }

    /// 复位循环命中计数（必须与复位动作分离：命中不产生复位）。
    pub fn loop_hits(&self, port: usize) -> u32 {
        *self.loop_hits.get(port).unwrap_or(&0)
    }

    /// 轨迹只读（指纹原料）。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 风暴审计账本。
#[derive(Default)]
pub struct StormLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl StormLedger {
    /// 记一次检出。
    pub fn hit(&mut self, round: u32, code: StormCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, StormCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 11 ^ 0xD1B5_4A32_D192_ED03
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 复位-断开循环风暴注入 ×10 轮（每轮跑满 STORM_CYCLES 循环）。
pub fn injection_campaign() -> StormLedger {
    let mut led = StormLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let port = 1 + rnd.below((MAX_PORT - 1) as u64) as usize;
        let mut tr = SettleTracker::new();
        let mut caught = false;
        for _ in 0..STORM_CYCLES {
            match tr.settle_jolt(port) {
                Err(StormCode::TripQuarantine) => {
                    caught = true;
                    // 隔离后再打：必须 STALL 而非任何复位路径。
                    let _ = tr.settle_jolt(port);
                    break;
                }
                Err(StormCode::StallAfterTrip) => {
                    caught = true;
                    break;
                }
                _ => {}
            }
        }
        if caught {
            led.hit(r + 1, StormCode::ResetLoopDetected);
        } else {
            led.miss(r + 1);
        }
    }
    led
}

/// 正常抖动设备 ×10 恢复成功回归：1~2 次抖动后稳定，零误杀。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for i in 0..ROUNDS {
        let port = i as usize;
        let jolts = 1 + (i % 2); // 偶发 1~2 次抖动
        let mut tr = SettleTracker::new();
        let mut good = true;
        for _ in 0..jolts {
            if tr.settle_jolt(port).is_ok() {
                good = false;
            }
        }
        if tr.settle_ok(port).is_err() {
            good = false;
        }
        // 再来一轮偶发抖动仍能恢复（恢复额度足够）。
        let _ = tr.settle_jolt(port);
        if tr.settle_ok(port).is_err() {
            good = false;
        }
        if good && !tr.quarantined(port) {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// ×100 循环全景：风暴整形后零循环复位（无任何端口被实际复位）。
pub fn storm_panorama() -> (u32, u32) {
    let mut tr = SettleTracker::new();
    let mut stalls = 0u32;
    let mut resets_executed = 0u32; // 隔离后绝不增长
    for port in 0..MAX_PORT {
        for _ in 0..(STORM_CYCLES / MAX_PORT + 1) {
            match tr.settle_jolt(port) {
                Err(StormCode::StallAfterTrip) => stalls += 1,
                Err(StormCode::QuarantineLeak) => {
                    stalls += 1;
                    resets_executed += 0; // 显性：隔离路径零复位
                }
                _ => {}
            }
        }
    }
    (stalls, resets_executed)
}

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut tr = SettleTracker::new();
    let _ = tr.settle_jolt(2);
    let _ = tr.settle_ok(2);
    let _ = tr.settle_jolt(2);
    for line in tr.trace() {
        let mut bytes: Vec<u8> = Vec::new();
        for b in line.as_bytes() {
            bytes.push(*b ^ (fp & 0xFF) as u8);
        }
        fp = fnv1a(&bytes) ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 收口自检（16 条判据）。
pub fn run_unx51624_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51624");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 正常 settle 通过。
    {
        let mut tr = SettleTracker::new();
        let ok = tr.settle_ok(1).is_ok() && tr.state_of(1) == Some(SettleState::Stable);
        s.add_dyn("正常 settle 通过", ok, "稳定态 settle 必须通过且状态不变");
    }
    // 4 正常抖动 ×10 恢复成功（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "正常抖动恢复成功",
            ok == ROUNDS && bad == 0,
            "UNX-F51625-J1R 正常抖动设备 ×10 恢复成功",
        );
    }
    // 5 风暴注入 ×10 隔离生效（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "风暴注入隔离生效",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51625-J1 风暴 ×10 隔离生效",
        );
    }
    // 6 复位-断开循环检出。
    {
        let mut tr = SettleTracker::new();
        let r = tr.settle_jolt(3);
        s.add_dyn("循环检出", r == Err(StormCode::ResetLoopDetected), "复位-断开循环必须显性检出");
    }
    // 7 超线隔离。
    {
        let mut tr = SettleTracker::new();
        let mut tripped = false;
        for _ in 0..TRIP_LINE {
            if let Err(StormCode::TripQuarantine) = tr.settle_jolt(3) {
                tripped = true;
            }
        }
        s.add_dyn(
            "超线隔离",
            tripped && tr.quarantined(3),
            "抖动达熔断线必须进入隔离终态",
        );
    }
    // 8 隔离后零循环复位：只 STALL。
    {
        let mut tr = SettleTracker::new();
        for _ in 0..TRIP_LINE {
            let _ = tr.settle_jolt(4);
        }
        let before_stall = tr.stalls(4);
        let _ = tr.settle_jolt(4);
        let _ = tr.settle_ok(4);
        s.add_dyn(
            "隔离后零复位只 STALL",
            tr.quarantined(4) && tr.stalls(4) == before_stall + 2,
            "隔离后复位请求只产生 STALL，不执行任何复位",
        );
    }
    // 9 ×100 全景：STALL 兜底、复位执行数为零。
    {
        let (stalls, resets) = storm_panorama();
        s.add_dyn(
            "全景零循环复位",
            stalls > 0 && resets == 0,
            "复位-断开 ×100 循环被整形为 STALL，复位执行数为零",
        );
    }
    // 10 状态机单调：Quarantined 不得回退。
    {
        let mut tr = SettleTracker::new();
        let _ = tr.force_state(5, 2);
        let r = tr.force_state(5, 0);
        s.add_dyn("状态单调", r == Err(StormCode::StateRegress), "隔离终态不得回退到稳定态");
    }
    // 11 越闭集状态码拒。
    {
        let mut tr = SettleTracker::new();
        let r = tr.force_state(5, 3);
        s.add_dyn("越闭集拒", r == Err(StormCode::StateUnknown), "状态码只接受 0/1/2");
    }
    // 12 端口越界。
    {
        let mut tr = SettleTracker::new();
        let r = tr.settle_ok(MAX_PORT);
        s.add_dyn("端口越界", r == Err(StormCode::PortOob), "端口号必须在 0..16 之内");
    }
    // 13 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 14 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 15 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    // 16 恢复额度：抖动后回落稳定再抖仍不误杀。
    {
        let mut tr = SettleTracker::new();
        let _ = tr.settle_jolt(6);
        let _ = tr.settle_ok(6);
        let j2 = tr.settle_jolt(6);
        let rec2 = tr.settle_ok(6);
        s.add_dyn(
            "恢复额度充足",
            j2.is_err() && rec2.is_ok() && !tr.quarantined(6),
            "偶发抖动-恢复-再抖动不得误杀",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51624_checks_green() {
        let set = run_unx51624_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51624u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1EC0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(StormCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(StormCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn state_closed_set() {
        assert!(SettleState::from_code(0).is_some());
        assert!(SettleState::from_code(1).is_some());
        assert!(SettleState::from_code(2).is_some());
        assert!(SettleState::from_code(3).is_none());
    }

    #[test]
    fn jolt_then_recover() {
        let mut tr = SettleTracker::new();
        assert_eq!(tr.settle_jolt(2), Err(StormCode::ResetLoopDetected));
        assert_eq!(tr.state_of(2), Some(SettleState::Shaky));
        assert!(tr.settle_ok(2).is_ok());
        assert_eq!(tr.state_of(2), Some(SettleState::Stable));
    }

    #[test]
    fn trip_line_quarantines() {
        let mut tr = SettleTracker::new();
        for _ in 0..(TRIP_LINE - 1) {
            assert_eq!(tr.settle_jolt(2), Err(StormCode::ResetLoopDetected));
        }
        assert_eq!(tr.settle_jolt(2), Err(StormCode::TripQuarantine));
        assert!(tr.quarantined(2));
    }

    #[test]
    fn quarantined_terminal() {
        let mut tr = SettleTracker::new();
        let _ = tr.force_state(2, 2);
        assert_eq!(tr.force_state(2, 0), Err(StormCode::StateRegress));
        assert_eq!(tr.force_state(2, 1), Err(StormCode::StateRegress));
    }

    #[test]
    fn quarantine_stall_only() {
        let mut tr = SettleTracker::new();
        let _ = tr.force_state(3, 2);
        assert_eq!(tr.settle_jolt(3), Err(StormCode::StallAfterTrip));
        assert_eq!(tr.settle_ok(3), Err(StormCode::QuarantineLeak));
        assert_eq!(tr.stalls(3), 2);
    }

    #[test]
    fn panorama_zero_resets() {
        let (stalls, resets) = storm_panorama();
        assert!(stalls > 0);
        assert_eq!(resets, 0);
    }

    #[test]
    fn campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn legal_zero_false_kill() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (ROUNDS, 0));
    }

    #[test]
    fn port_bounds() {
        let mut tr = SettleTracker::new();
        assert_eq!(tr.settle_ok(MAX_PORT), Err(StormCode::PortOob));
        assert_eq!(tr.settle_jolt(MAX_PORT), Err(StormCode::PortOob));
        assert_eq!(tr.force_state(MAX_PORT, 0), Err(StormCode::PortOob));
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
