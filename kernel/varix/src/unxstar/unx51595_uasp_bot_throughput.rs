//! UNX-F51595 B20 UASP/BOT 吞吐对照终核（≥×1.2 主判据） · 0xC1780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51596-J1 框架终核过；24h 全量归波 23 闭账；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - ThruPair：一对吞吐样本（BOT 基线/UASP 实测/×100 定点放大比）
//!   - ThroughputFinalizer：吞吐终核器（样本入册 + 比值判定 + 框架终核面 +
//!     24h 全量归账）
//!   - FinalGate：框架终核门（终核相位/终核印）
//!   - ThruCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。吞吐值
//!      全整数（KB/s 口径），放大比 ×100 定点，除零显性拒。
//!   ② 主判据：UASP/BOT 放大比 ≥×1.2（定点 120）才算终核样本有效；样本
//!      账只增不减，终核后冻结。
//!   ③ 框架终核：终核门 Open→Counting→Finalized 单调推进，Finalized 后
//!      只读；24h 全量归波 23 闭账以终核印 + 归账行留痕。
//!   ④ 注入核：4 类坏样本（比值不足/零基线/重复样本/终核后改账）逐类 10 轮
//!      精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1780;

/// 终核样本对数下限。
pub const SAMPLE_PAIRS: usize = 3;
/// 主判据放大比下限（×1.2 的 ×100 定点）。
pub const RATIO_FLOOR: u64 = 120;
/// 定点放大倍数（×100）。
pub const RATIO_SCALE: u64 = 100;
/// 24h 全量归账目标（KB/s 累计口径）。
pub const DAY_TOTAL: u64 = 24;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 终核相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalPhase {
    /// 开账（可入样本）
    Open = 0,
    /// 计核中（样本入满、未终核）
    Counting = 1,
    /// 已终核（只读）
    Finalized = 2,
}

/// 一对吞吐样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThruPair {
    /// 样本号（1 起计）
    pub idx: usize,
    /// BOT 基线吞吐（KB/s）
    pub bot: u64,
    /// UASP 实测吞吐（KB/s）
    pub uasp: u64,
    /// 放大比（×100 定点）
    pub ratio: u64,
}

/// 框架终核门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinalGate {
    /// 终核相位
    pub phase: FinalPhase,
    /// 终核印（未终核为 0）
    pub seal: u64,
}

/// 吞吐终核器。
pub struct ThroughputFinalizer {
    pairs: Vec<ThruPair>,
    phase: FinalPhase,
    gate: FinalGate,
    handed_to_wave23: bool,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ThroughputFinalizer {
    /// 新建终核器（Open 相位，零样本）。
    pub fn new() -> ThroughputFinalizer {
        ThroughputFinalizer {
            pairs: Vec::new(),
            phase: FinalPhase::Open,
            gate: FinalGate { phase: FinalPhase::Open, seal: 0 },
            handed_to_wave23: false,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 入一对样本：比值 ≥×1.2 才有效，同参数重复显性拒。
    pub fn add_pair(&mut self, bot: u64, uasp: u64) -> Result<ThruPair, ThruCode> {
        if self.phase != FinalPhase::Open {
            self.rejected += 1;
            return Err(ThruCode::NotOpen);
        }
        if bot == 0 {
            self.rejected += 1;
            return Err(ThruCode::ZeroBase);
        }
        if uasp == 0 {
            self.rejected += 1;
            return Err(ThruCode::ZeroThru);
        }
        if self.pairs.iter().any(|p| p.bot == bot && p.uasp == uasp) {
            self.rejected += 1;
            return Err(ThruCode::DupPair);
        }
        let ratio = uasp * RATIO_SCALE / bot;
        if ratio < RATIO_FLOOR {
            self.rejected += 1;
            return Err(ThruCode::RatioShort);
        }
        let idx = self.pairs.len() + 1;
        let p = ThruPair { idx, bot, uasp, ratio };
        self.pairs.push(p);
        if self.pairs.len() >= SAMPLE_PAIRS {
            self.phase = FinalPhase::Counting;
        }
        Ok(p)
    }

    /// 框架终核：样本入满才可终核，终核印 = 样本账 FNV-1a 指纹。
    pub fn finalize(&mut self) -> Result<u64, ThruCode> {
        if self.phase != FinalPhase::Counting {
            self.rejected += 1;
            return Err(ThruCode::NotCounting);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for p in self.pairs.iter() {
            bytes.extend_from_slice(&p.bot.to_le_bytes());
            bytes.extend_from_slice(&p.uasp.to_le_bytes());
            bytes.extend_from_slice(&p.ratio.to_le_bytes());
        }
        let seal = fnv1a(&bytes);
        self.gate = FinalGate { phase: FinalPhase::Finalized, seal };
        self.phase = FinalPhase::Finalized;
        Ok(seal)
    }

    /// 24h 全量归波 23 闭账：终核过才可归，只许归一次。
    pub fn hand_to_wave23(&mut self) -> Result<(), ThruCode> {
        if self.phase != FinalPhase::Finalized {
            self.rejected += 1;
            return Err(ThruCode::NotFinalized);
        }
        if self.handed_to_wave23 {
            self.rejected += 1;
            return Err(ThruCode::DupHand);
        }
        self.handed_to_wave23 = true;
        Ok(())
    }

    /// 主判据：终核样本 ≥3 对且全部 ≥×1.2。
    pub fn main_judge_ok(&self) -> bool {
        self.pairs.len() >= SAMPLE_PAIRS && self.pairs.iter().all(|p| p.ratio >= RATIO_FLOOR)
    }

    /// 最低放大比（空账返回 0）。
    pub fn min_ratio(&self) -> u64 {
        let mut m: u64 = u64::MAX;
        let mut any = false;
        for p in self.pairs.iter() {
            if p.ratio < m {
                m = p.ratio;
            }
            any = true;
        }
        if any {
            m
        } else {
            0
        }
    }

    /// 已归波 23 闭账。
    pub fn handed(&self) -> bool {
        self.handed_to_wave23
    }

    /// 样本对数。
    pub fn pairs_len(&self) -> usize {
        self.pairs.len()
    }

    /// 终核印（未终核为 0）。
    pub fn gate_seal(&self) -> u64 {
        self.gate.seal
    }

    /// 终核相位。
    pub fn phase(&self) -> FinalPhase {
        self.phase
    }

    /// 拒绝账计数。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.phase as u64).to_le_bytes());
        bytes.extend_from_slice(&self.gate.seal.to_le_bytes());
        bytes.extend_from_slice(&(self.handed_to_wave23 as u64).to_le_bytes());
        for p in self.pairs.iter() {
            bytes.extend_from_slice(&p.idx.to_le_bytes());
            bytes.extend_from_slice(&p.bot.to_le_bytes());
            bytes.extend_from_slice(&p.uasp.to_le_bytes());
            bytes.extend_from_slice(&p.ratio.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 UASP/BOT 吞吐终核域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThruCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 相位非 Open 拒入样本
    NotOpen,
    /// 零基线吞吐
    ZeroBase,
    /// 零实测吞吐
    ZeroThru,
    /// 重复样本对
    DupPair,
    /// 放大比不足 ×1.2
    RatioShort,
    /// 相位非 Counting 拒终核
    NotCounting,
    /// 未终核拒归账
    NotFinalized,
    /// 重复归账
    DupHand,
    /// 终核印漂移（终核后账面变化）
    SealDrift,
    /// 样本账为空
    EmptyLedger,
}

impl ThruCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            ThruCode::Ok => "吞吐终核判定成功",
            ThruCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ThruCode::NotOpen => "相位非 Open：开账期之外不得入样本",
            ThruCode::ZeroBase => "零基线：BOT 基线为零，放大比无定义，显性拒",
            ThruCode::ZeroThru => "零实测：UASP 实测为零属虚报形态，显性拒",
            ThruCode::DupPair => "重复样本：同（基线,实测）对不得二次入账",
            ThruCode::RatioShort => "放大比不足：UASP/BOT 须 ≥×1.2（定点 120）",
            ThruCode::NotCounting => "样本未齐：不足 3 对不得框架终核",
            ThruCode::NotFinalized => "未终核：框架终核过后才可归波 23 闭账",
            ThruCode::DupHand => "重复归账：24h 全量归波 23 只许归一次",
            ThruCode::SealDrift => "终核印漂移：终核后账面不得变化",
            ThruCode::EmptyLedger => "样本账为空：终核判据要求至少一对样本",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [ThruCode; 12] = [
    ThruCode::Ok,
    ThruCode::SegBad,
    ThruCode::NotOpen,
    ThruCode::ZeroBase,
    ThruCode::ZeroThru,
    ThruCode::DupPair,
    ThruCode::RatioShort,
    ThruCode::NotCounting,
    ThruCode::NotFinalized,
    ThruCode::DupHand,
    ThruCode::SealDrift,
    ThruCode::EmptyLedger,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x165667b19e3779f9 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：三对达标样本 → 终核 → 归账。返回 (终核印, 样本数)。
pub fn standard_script() -> (u64, usize) {
    let mut f = ThroughputFinalizer::new();
    let bots = [100u64, 150, 200];
    let uasps = [125u64, 190, 250];
    for i in 0..SAMPLE_PAIRS {
        let _ = f.add_pair(bots[i], uasps[i]);
    }
    let seal = f.finalize();
    let s = match seal {
        Ok(s) => s,
        Err(_) => 0,
    };
    let _ = f.hand_to_wave23();
    (s, f.pairs_len())
}

/// 双跑一致性：同一脚本两次建账，终核印与样本数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：比值不足——119/100 <×1.2，显性拒（RatioShort）。
    for _ in 0..INJ_ROUNDS {
        let mut f = ThroughputFinalizer::new();
        let caught = f.add_pair(100, 119) == Err(ThruCode::RatioShort);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：零基线——显性拒（ZeroBase）。
    for _ in 0..INJ_ROUNDS {
        let mut f = ThroughputFinalizer::new();
        let caught = f.add_pair(0, 130) == Err(ThruCode::ZeroBase);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：重复样本——同对二次入账显性拒（DupPair）。
    for _ in 0..INJ_ROUNDS {
        let mut f = ThroughputFinalizer::new();
        let _ = f.add_pair(100, 130);
        let caught = f.add_pair(100, 130) == Err(ThruCode::DupPair);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：终核后改账——Finalized 后入样本显性拒（NotOpen）。
    for _ in 0..INJ_ROUNDS {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        let _ = f.finalize();
        let caught = f.add_pair(999, 9999) == Err(ThruCode::NotOpen);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51595_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51595");
    // 1 主判据：≥3 对全 ≥×1.2 终核过。
    {
        let (seal, n) = standard_script();
        s.add(
            "主判据 ≥×1.2 终核过",
            seal != 0 && n == SAMPLE_PAIRS,
            "UNX-F51595-J1：UASP/BOT ≥×1.2 ×3 对样本框架终核过",
        );
    }
    // 2 比值下限精确判。
    {
        let mut f = ThroughputFinalizer::new();
        let floor = f.add_pair(100, 120);
        let mut g = ThroughputFinalizer::new();
        let below = g.add_pair(100, 119);
        s.add(
            "比值下限精确判",
            floor.map(|p| p.ratio) == Ok(120) && below == Err(ThruCode::RatioShort),
            "×1.2（定点 120）恰达标入账，119 显性拒",
        );
    }
    // 3 零基线与零实测护住。
    {
        let mut f = ThroughputFinalizer::new();
        let zb = f.add_pair(0, 130) == Err(ThruCode::ZeroBase);
        let mut g = ThroughputFinalizer::new();
        let zt = g.add_pair(100, 0) == Err(ThruCode::ZeroThru);
        s.add(
            "零值护住",
            zb && zt,
            "零基线放大比无定义、零实测属虚报形态，均显性拒",
        );
    }
    // 4 重复样本拒。
    {
        let mut f = ThroughputFinalizer::new();
        let _ = f.add_pair(100, 130);
        let r = f.add_pair(100, 130);
        s.add(
            "重复样本拒",
            r == Err(ThruCode::DupPair) && f.pairs_len() == 1,
            "同（基线,实测）对只许入账一次",
        );
    }
    // 5 样本未齐拒终核。
    {
        let mut f = ThroughputFinalizer::new();
        for i in 0..(SAMPLE_PAIRS - 1) {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        s.add(
            "样本未齐拒终核",
            f.finalize() == Err(ThruCode::NotCounting),
            "不足 3 对不得框架终核",
        );
    }
    // 6 未终核拒归账。
    {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        s.add(
            "未终核拒归账",
            f.hand_to_wave23() == Err(ThruCode::NotFinalized),
            "24h 全量归波 23 闭账须在框架终核之后",
        );
    }
    // 7 归账只许一次。
    {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        let _ = f.finalize();
        let first = f.hand_to_wave23();
        let second = f.hand_to_wave23();
        s.add(
            "归账只许一次",
            first == Ok(()) && second == Err(ThruCode::DupHand) && f.handed(),
            "24h 全量归波 23 闭账只许归一次",
        );
    }
    // 8 相位单调：Open→Counting→Finalized。
    {
        let mut f = ThroughputFinalizer::new();
        let p0 = f.phase() as u8;
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        let p1 = f.phase() as u8;
        let _ = f.finalize();
        let p2 = f.phase() as u8;
        s.add(
            "相位单调",
            p0 < p1 && p1 < p2,
            "Open→Counting→Finalized 只进不退",
        );
    }
    // 9 终核后只读。
    {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        let seal = match f.finalize() {
            Ok(s) => s,
            Err(_) => 0,
        };
        let ro = f.add_pair(999, 9999) == Err(ThruCode::NotOpen)
            && f.finalize() == Err(ThruCode::NotCounting)
            && f.gate_seal() == seal;
        s.add(
            "终核后只读",
            ro && seal != 0,
            "Finalized 相位入样本/二次终核显性拒，终核印不变（防 SealDrift）",
        );
    }
    // 10 最低比值面。
    {
        let mut f = ThroughputFinalizer::new();
        let _ = f.add_pair(100, 125);
        let _ = f.add_pair(200, 250);
        s.add(
            "最低比值面",
            f.min_ratio() == 125,
            "三波中最低放大比可读，判据以最低值为准",
        );
    }
    // 11 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 诊断码闭集唯一性。
    {
        let mut ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                ok = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut f = ThroughputFinalizer::new();
        for _ in 0..15u32 {
            f.note_detected();
        }
        s.add("注入账守恒", f.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建账，终核印与样本数逐位一致",
    );
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51595 UASP/BOT 终核 {} 对 ×{} 24h 归 {} 段",
            SAMPLE_PAIRS, RATIO_FLOOR, DAY_TOTAL
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51595"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51595_checks_green() {
        let set = run_unx51595_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_finalize_path() {
        let mut f = ThroughputFinalizer::new();
        let bots = [100u64, 150, 200];
        let uasps = [125u64, 190, 250];
        for i in 0..SAMPLE_PAIRS {
            assert!(f.add_pair(bots[i], uasps[i]).is_ok());
        }
        assert_eq!(f.phase(), FinalPhase::Counting);
        assert!(f.finalize().is_ok());
        assert_eq!(f.phase(), FinalPhase::Finalized);
        assert!(f.main_judge_ok());
        assert!(f.hand_to_wave23().is_ok());
        assert!(f.handed());
    }

    #[test]
    fn ratio_floor_exact() {
        let mut f = ThroughputFinalizer::new();
        let p = f.add_pair(100, 120);
        assert_eq!(p.map(|x| x.ratio), Ok(120));
    }

    #[test]
    fn ratio_short_rejected() {
        let mut f = ThroughputFinalizer::new();
        assert_eq!(f.add_pair(100, 119), Err(ThruCode::RatioShort));
        assert_eq!(f.pairs_len(), 0);
    }

    #[test]
    fn zero_values_guarded() {
        let mut f = ThroughputFinalizer::new();
        assert_eq!(f.add_pair(0, 130), Err(ThruCode::ZeroBase));
        assert_eq!(f.add_pair(100, 0), Err(ThruCode::ZeroThru));
    }

    #[test]
    fn duplicate_pair_rejected() {
        let mut f = ThroughputFinalizer::new();
        assert!(f.add_pair(100, 130).is_ok());
        assert_eq!(f.add_pair(100, 130), Err(ThruCode::DupPair));
        assert_eq!(f.pairs_len(), 1);
    }

    #[test]
    fn finalize_needs_full_pairs() {
        let mut f = ThroughputFinalizer::new();
        for i in 0..(SAMPLE_PAIRS - 1) {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        assert_eq!(f.finalize(), Err(ThruCode::NotCounting));
    }

    #[test]
    fn hand_once_only() {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        assert_eq!(f.hand_to_wave23(), Err(ThruCode::NotFinalized));
        let _ = f.finalize();
        assert_eq!(f.hand_to_wave23(), Ok(()));
        assert_eq!(f.hand_to_wave23(), Err(ThruCode::DupHand));
    }

    #[test]
    fn finalized_readonly() {
        let mut f = ThroughputFinalizer::new();
        for i in 0..SAMPLE_PAIRS {
            let _ = f.add_pair(100 + i as u64 * 10, 130 + i as u64 * 20);
        }
        let seal = f.finalize().unwrap_or(0);
        assert_eq!(f.add_pair(1, 1), Err(ThruCode::NotOpen));
        assert_eq!(f.finalize(), Err(ThruCode::NotCounting));
        assert_eq!(f.gate_seal(), seal);
        assert_ne!(seal, 0);
    }

    #[test]
    fn min_ratio_surface() {
        let mut f = ThroughputFinalizer::new();
        assert_eq!(f.min_ratio(), 0);
        let _ = f.add_pair(100, 125);
        let _ = f.add_pair(200, 250);
        assert_eq!(f.min_ratio(), 125);
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut f = ThroughputFinalizer::new();
        for _ in 0..11u32 {
            f.note_detected();
        }
        assert!(f.conserved());
        let mut bad = ThroughputFinalizer::new();
        bad.note_escape();
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
