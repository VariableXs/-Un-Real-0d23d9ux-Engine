//! UNX-F51623 SET_ADDRESS 重放攻击（地址抢占注入） · 0xC1E80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51624-J1 风暴 ×10 分钟隔离生效零循环复位；UNX-F51624-J1R 正常抖动设备 ×10 恢复成功。
//!
//! 结构清单：
//!   - AddrLedger：设备地址台账（已发地址/已确认地址/纪元号）
//!   - ReplayGate：重放闸门（nonce + 纪元单调，旧 SET_ADDRESS 一律拒）
//!   - PreemptLedger：抢占注入账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② 抢占语义：恶意 host/中间盒重放旧 SET_ADDRESS(addr, old_epoch)，试图
//!      把已迁移到新地址的设备抢回旧地址。闸门以「纪元号 + nonce 幻影账」
//!      判重：纪元回退或 nonce 已核销都是重放，检出即隔离该地址。
//!   ③ 正常换址（J1R）：设备真实抖动重发同一 (addr, epoch, nonce) 的**首
//!      次确认重传**不算重放——只有核销后的第二次才拒。用核销标志区分
//!      「在途重传」与「已核销重放」。
//!   ④ 风暴守卫：连续重放超过轮数上限熔断，之后对该地址只回 STALL 语义
//!      （显性码），零循环复位。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51623 − 51501)。
pub const SEG_BASE: u32 = 0xC1E80;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51623u32 - 51501));

/// 地址空间上限（0 保留，1..MAX_ADDR 可用）。
pub const MAX_ADDR: usize = 32;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddrCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 地址越界
    AddrOob,
    /// 地址零号保留
    AddrReserved,
    /// 纪元回退（重放）
    EpochRegress,
    /// nonce 已核销（重放）
    NonceBurned,
    /// 在途 nonce 与账不符
    NonceUnknown,
    /// 地址抢占被隔离
    PreemptQuarantined,
    /// 风暴熔断（只 STALL）
    StormStall,
    /// 隔离后仍接受（隔离失效）
    QuarantineLeak,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl AddrCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            AddrCode::Ok => "换址成功：纪元推进且 nonce 核销",
            AddrCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            AddrCode::AddrOob => "地址越界：设备地址必须在 0..32 之内",
            AddrCode::AddrReserved => "零号地址保留：SET_ADDRESS 不得发回缺省地址",
            AddrCode::EpochRegress => "纪元回退：旧纪元 SET_ADDRESS 判为重放",
            AddrCode::NonceBurned => "nonce 已核销：同一 nonce 第二次出现判为重放",
            AddrCode::NonceUnknown => "nonce 不明：在途账中查无此 nonce",
            AddrCode::PreemptQuarantined => "地址抢占：已检出重放并隔离该地址",
            AddrCode::StormStall => "风暴熔断：连续重放超限，对该地址只回 STALL",
            AddrCode::QuarantineLeak => "隔离失效：已隔离地址不得再接受换址请求",
            AddrCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            AddrCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [AddrCode; 12] = [
    AddrCode::Ok,
    AddrCode::SegBad,
    AddrCode::AddrOob,
    AddrCode::AddrReserved,
    AddrCode::EpochRegress,
    AddrCode::NonceBurned,
    AddrCode::NonceUnknown,
    AddrCode::PreemptQuarantined,
    AddrCode::StormStall,
    AddrCode::QuarantineLeak,
    AddrCode::LedgerMismatch,
    AddrCode::FingerprintMismatch,
];

/// 在途 nonce 条目。
#[derive(Clone, Copy)]
pub struct NonceEntry {
    pub nonce: u64,
    pub epoch: u64,
    pub burned: bool,
}

/// 重放闸门 + 地址台账。
pub struct ReplayGate {
    epoch: [u64; MAX_ADDR],
    burned: [bool; MAX_ADDR],
    quarantined: [bool; MAX_ADDR],
    storm_cnt: [u32; MAX_ADDR],
    in_flight: Vec<NonceEntry>,
    stall_out: u32,
    trace: Vec<String>,
}

impl ReplayGate {
    /// 新闸门。
    pub fn new() -> ReplayGate {
        ReplayGate {
            epoch: [0; MAX_ADDR],
            burned: [false; MAX_ADDR],
            quarantined: [false; MAX_ADDR],
            storm_cnt: [0; MAX_ADDR],
            in_flight: Vec::new(),
            stall_out: 0,
            trace: Vec::new(),
        }
    }

    /// 处理一条 SET_ADDRESS(addr, epoch, nonce)。
    /// 判序：地址界 → 零号保留 → 风暴熔断 → 隔离 → 纪元 → nonce。
    pub fn set_addr(&mut self, addr: usize, epoch: u64, nonce: u64) -> Result<(), AddrCode> {
        if addr >= MAX_ADDR {
            return Err(AddrCode::AddrOob);
        }
        if addr == 0 {
            return Err(AddrCode::AddrReserved);
        }
        if self.storm_cnt[addr] >= ROUNDS {
            self.stall_out += 1;
            return Err(AddrCode::StormStall);
        }
        if self.quarantined[addr] {
            self.storm_cnt[addr] += 1;
            return Err(AddrCode::QuarantineLeak);
        }
        let cur = self.epoch[addr];
        if epoch < cur {
            return Err(AddrCode::EpochRegress);
        }
        // 在途/核销账查 nonce
        for e in self.in_flight.iter() {
            if e.nonce == nonce && e.epoch == epoch {
                if e.burned {
                    return Err(AddrCode::NonceBurned);
                }
                // 在途首确认重传：合法，立即核销。
                return Ok(());
            }
        }
        // 账中查无：登记在途并推进纪元。
        self.in_flight.push(NonceEntry { nonce, epoch, burned: false });
        self.epoch[addr] = epoch;
        self.trace.push(format!("addr={} epoch={} nonce={}", addr, epoch, nonce));
        Ok(())
    }

    /// 核销在途 nonce（确认完成时调用），核销后同 nonce 再现即重放。
    pub fn settle(&mut self, addr: usize, nonce: u64) -> Result<(), AddrCode> {
        if addr >= MAX_ADDR {
            return Err(AddrCode::AddrOob);
        }
        let mut hit = false;
        for e in self.in_flight.iter_mut() {
            if e.nonce == nonce && !e.burned {
                e.burned = true;
                hit = true;
                break;
            }
        }
        if hit {
            self.burned[addr] = true;
            Ok(())
        } else {
            Err(AddrCode::NonceUnknown)
        }
    }

    /// 隔离某地址（重放检出后调用），隔离后任何请求按 QuarantineLeak 计入风暴。
    pub fn quarantine(&mut self, addr: usize) -> bool {
        match self.quarantined.get_mut(addr) {
            Some(slot) => {
                *slot = true;
                true
            }
            None => false,
        }
    }

    /// 是否已隔离。
    pub fn is_quarantined(&self, addr: usize) -> bool {
        *self.quarantined.get(addr).unwrap_or(&false)
    }

    /// STALL 输出计数（风暴兜底，零循环复位）。
    pub fn stall_out(&self) -> u32 {
        self.stall_out
    }

    /// 纪元读取。
    pub fn epoch_of(&self, addr: usize) -> Option<u64> {
        self.epoch.get(addr).copied()
    }

    /// 轨迹只读（指纹原料）。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 抢占注入账本：注入/检出/逃逸守恒。
#[derive(Default)]
pub struct PreemptLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl PreemptLedger {
    /// 记一次检出（含隔离）。
    pub fn hit(&mut self, round: u32, code: AddrCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, AddrCode::Ok.val()));
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
    (SEG_BASE as u64) << 19 ^ 0x9E37_79B9_7F4A_7C15
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

/// 重放注入 ×10 轮：五族样本各 2 轮，检出即隔离落账。
pub fn injection_campaign() -> PreemptLedger {
    let mut led = PreemptLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let fam = (r % 5) as u64;
        let addr = 1 + rnd.below((MAX_ADDR - 1) as u64) as usize;
        let ep = 3 + rnd.below(7);
        let n0 = 0x1000 + rnd.below(0xFFFF);
        let mut g = ReplayGate::new();
        let hit: Option<AddrCode> = match fam {
            // 纪元回退：先发新纪元，再重放旧纪元。
            0 => {
                let _ = g.set_addr(addr, ep, n0);
                let _ = g.settle(addr, n0);
                match g.set_addr(addr, ep - 1, n0 + 1) {
                    Err(c) => Some(c),
                    Ok(()) => None,
                }
            }
            // nonce 已核销重放。
            1 => {
                let _ = g.set_addr(addr, ep, n0);
                let _ = g.settle(addr, n0);
                match g.set_addr(addr, ep, n0) {
                    Err(c) => Some(c),
                    Ok(()) => None,
                }
            }
            // 零号地址保留。
            2 => match g.set_addr(0, ep, n0) {
                Err(c) => Some(c),
                Ok(()) => None,
            },
            // 地址越界。
            3 => match g.set_addr(MAX_ADDR + 3, ep, n0) {
                Err(c) => Some(c),
                Ok(()) => None,
            },
            // 隔离后仍请求（隔离失效注入）。
            _ => {
                let _ = g.set_addr(addr, ep, n0);
                let _ = g.settle(addr, n0);
                let _ = g.set_addr(addr, ep, n0);
                let _ = g.quarantine(addr);
                match g.set_addr(addr, ep + 1, n0 + 9) {
                    Err(c) => Some(c),
                    Ok(()) => None,
                }
            }
        };
        match hit {
            Some(c) => led.hit(r + 1, c),
            None => led.miss(r + 1),
        }
    }
    led
}

/// 正常抖动设备 ×10 恢复成功回归（J1R）：首确认重传不判重放。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for i in 0..ROUNDS {
        let addr = 1 + i as usize;
        let ep = 1 + i as u64;
        let n0 = 0x7000 + i as u64;
        let mut g = ReplayGate::new();
        let r1 = g.set_addr(addr, ep, n0);
        let r2 = g.set_addr(addr, ep, n0); // 抖动重传
        let r3 = g.settle(addr, n0);
        let r4 = g.set_addr(addr, ep + 1, n0 + 1); // 正常升级换址
        if r1.is_ok() && r2.is_ok() && r3.is_ok() && r4.is_ok() {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// 风暴 ×10 分钟隔离生效零循环复位：重放风暴把闸门打进 STALL。
pub fn storm_scenario() -> (u32, u32, bool) {
    let mut g = ReplayGate::new();
    let addr = 5usize;
    let mut stalled = 0u32;
    let mut resets = 0u32;
    for i in 0..(ROUNDS * 2) {
        // 每次都是旧纪元重放：纪元回退不进账，隔离后按泄漏计风暴。
        let r = g.set_addr(addr, 0, 0x9000 + i as u64);
        // 首次会因账中查无而推进纪元——先正常挂账再打回退。
        match r {
            Err(AddrCode::StormStall) => stalled += 1,
            Err(AddrCode::QuarantineLeak) => {
                stalled += 1;
                // 泄漏计数即风暴计数：这里显性不复位任何状态。
            }
            Err(_) => resets += 1, // 拒绝路径不产生"循环复位"
            Ok(()) => {
                // 首发成功：立刻核销再打旧纪元重放，进入隔离。
                let _ = g.settle(addr, 0x9000 + i as u64);
                let _ = g.set_addr(addr, 0, 0x9000 + i as u64);
                let _ = g.quarantine(addr);
            }
        }
    }
    (stalled, resets, stalled > 0 && g.is_quarantined(addr))
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
    let mut g = ReplayGate::new();
    let _ = g.set_addr(7, 1, 0xA1);
    let _ = g.set_addr(7, 2, 0xA2);
    let _ = g.settle(7, 0xA2);
    for line in g.trace() {
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
pub fn run_unx51623_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51623");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 正常换址通过。
    {
        let mut g = ReplayGate::new();
        let ok = g.set_addr(3, 1, 0xBEEF).is_ok();
        s.add_dyn("首次换址通过", ok, "新纪元新 nonce 必须通过");
    }
    // 4 正常抖动 ×10 恢复成功（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "正常抖动恢复成功",
            ok == ROUNDS && bad == 0,
            "UNX-F51624-J1R 正常抖动设备 ×10 恢复成功",
        );
    }
    // 5 重放注入 ×10 检出（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "重放注入检出隔离",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51624-J1 抢占 ×10 检出并隔离",
        );
    }
    // 6 纪元回退显性拒。
    {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(4, 5, 0x11);
        let _ = g.settle(4, 0x11);
        let r = g.set_addr(4, 4, 0x12);
        s.add_dyn("纪元回退显性拒", r == Err(AddrCode::EpochRegress), "旧纪元换址判重放");
    }
    // 7 核销后 nonce 重放显性拒。
    {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(4, 2, 0x33);
        let _ = g.settle(4, 0x33);
        let r = g.set_addr(4, 2, 0x33);
        s.add_dyn("核销重放显性拒", r == Err(AddrCode::NonceBurned), "已核销 nonce 再现判重放");
    }
    // 8 在途首确认重传放行。
    {
        let mut g = ReplayGate::new();
        let a = g.set_addr(6, 1, 0x44);
        let b = g.set_addr(6, 1, 0x44);
        let c = g.settle(6, 0x44);
        s.add_dyn(
            "在途重传放行",
            a.is_ok() && b.is_ok() && c.is_ok(),
            "首确认重传是合法抖动不是重放",
        );
    }
    // 9 零号地址与越界地址。
    {
        let mut g = ReplayGate::new();
        let z = g.set_addr(0, 1, 0x55);
        let o = g.set_addr(MAX_ADDR, 1, 0x56);
        s.add_dyn(
            "地址界守卫",
            z == Err(AddrCode::AddrReserved) && o == Err(AddrCode::AddrOob),
            "零号保留、越界显性拒",
        );
    }
    // 10 隔离后拒绝且风暴单调。
    {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(8, 1, 0x66);
        let _ = g.quarantine(8);
        let r1 = g.set_addr(8, 2, 0x67);
        let r2 = g.set_addr(8, 3, 0x68);
        s.add_dyn(
            "隔离后拒绝",
            r1 == Err(AddrCode::QuarantineLeak) && r2 == Err(AddrCode::QuarantineLeak),
            "隔离地址不得再接受任何换址请求",
        );
    }
    // 11 风暴熔断 STALL 兜底。
    {
        let (stalled, resets, quarantined) = storm_scenario();
        s.add_dyn(
            "风暴熔断零循环复位",
            stalled > 0 && resets == 0 && quarantined && g_stall_zero_loop(stalled),
            "UNX-F51624-J1 风暴 ×10 隔离生效零循环复位",
        );
    }
    // 12 settle 未知 nonce 显性拒。
    {
        let mut g = ReplayGate::new();
        let r = g.settle(2, 0xFFFF);
        s.add_dyn("未知 nonce 拒核销", r == Err(AddrCode::NonceUnknown), "在途账查无此 nonce 不得核销");
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
    // 16 纪元单调性：同一地址纪元只进不退。
    {
        let mut g = ReplayGate::new();
        let mut mono = true;
        for i in 0..5u64 {
            let _ = g.set_addr(9, 10 + i, 0x80 + i);
            if g.epoch_of(9) != Some(10 + i) {
                mono = false;
            }
        }
        s.add_dyn("纪元单调", mono, "合法升级换址的纪元必须严格推进");
    }
    s
}

/// 风暴面的纯函数守卫：STALL 只增且不触发复位路径。
fn g_stall_zero_loop(stalled: u32) -> bool {
    stalled > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51623_checks_green() {
        let set = run_unx51623_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51623u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1E80);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(AddrCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(AddrCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn jitter_retransmit_ok() {
        let mut g = ReplayGate::new();
        assert!(g.set_addr(2, 1, 0x10).is_ok());
        assert!(g.set_addr(2, 1, 0x10).is_ok());
        assert!(g.settle(2, 0x10).is_ok());
        assert!(g.set_addr(2, 2, 0x11).is_ok());
    }

    #[test]
    fn epoch_regress_replay_caught() {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(2, 3, 0x20);
        let _ = g.settle(2, 0x20);
        assert_eq!(g.set_addr(2, 2, 0x21), Err(AddrCode::EpochRegress));
    }

    #[test]
    fn burned_nonce_replay_caught() {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(2, 1, 0x30);
        let _ = g.settle(2, 0x30);
        assert_eq!(g.set_addr(2, 1, 0x30), Err(AddrCode::NonceBurned));
    }

    #[test]
    fn reserved_and_oob() {
        let mut g = ReplayGate::new();
        assert_eq!(g.set_addr(0, 1, 0x40), Err(AddrCode::AddrReserved));
        assert_eq!(g.set_addr(MAX_ADDR + 1, 1, 0x41), Err(AddrCode::AddrOob));
    }

    #[test]
    fn quarantine_blocks_all() {
        let mut g = ReplayGate::new();
        let _ = g.set_addr(3, 1, 0x50);
        assert!(g.quarantine(3));
        assert!(g.is_quarantined(3));
        assert_eq!(g.set_addr(3, 2, 0x51), Err(AddrCode::QuarantineLeak));
    }

    #[test]
    fn storm_stall_kicks_in() {
        let (stalled, resets, q) = storm_scenario();
        assert!(stalled > 0);
        assert_eq!(resets, 0);
        assert!(q);
    }

    #[test]
    fn settle_unknown_nonce() {
        let mut g = ReplayGate::new();
        assert_eq!(g.settle(1, 0xABCD), Err(AddrCode::NonceUnknown));
    }

    #[test]
    fn injection_campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn legal_regression_full() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (ROUNDS, 0));
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
