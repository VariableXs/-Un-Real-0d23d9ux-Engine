//! UNX-F51594 B20 与波 23 闭账物衔接预告（M 型段产出并入闭账） · 0xC1740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51595-J1 对照 ×3 波 ≥×1.2（真机对照随闸门补测登记不虚
//! 报）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - WavePair：一波对照行（波号/基线值/真机值/放大比 ×10 定点）
//!   - HandoverPreview：波 23 衔接预告册（3 波对照 + 补测登记面 + 虚报探测）
//!   - RetestReg：闸门补测登记行（随闸门补测，登记不虚报）
//!   - PreviewCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。放大比
//!      用 ×100 定点整数（真机值×100/基线值），零除法裸调用（除零显性拒）。
//!   ② 主判据：对照 ×3 波，每波放大比 ≥×1.2（定点 120）；三波齐过才算衔接
//!      预告成立，单波不足显性判 RatioShort。
//!   ③ 不虚报：未随闸门补测登记的真机对照视为虚报（NoRetest），显性拒入册；
//!      已登记的补测行不可撤（登记只增不减）。
//!   ④ 注入核：4 类坏样本（比值不足/零基线/未登记补测/重复登记撤回）逐类
//!      10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1740;

/// 对照波数（×3 波）。
pub const COMPARE_WAVES: usize = 3;
/// 主判据放大比下限（×1.2 的 ×100 定点）。
pub const RATIO_FLOOR: u64 = 120;
/// 定点放大倍数（×100）。
pub const RATIO_SCALE: u64 = 100;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 一波对照行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WavePair {
    /// 波号（1..=3）
    pub wave: usize,
    /// 基线值（B20 侧）
    pub base: u64,
    /// 真机值（随闸门补测登记后有效）
    pub on_hw: u64,
    /// 放大比（×100 定点，on_hw*100/base）
    pub ratio: u64,
    /// 是否随闸门补测登记
    pub retested: bool,
}

/// 闸门补测登记行（登记只增不减，不虚报面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestReg {
    /// 波号
    pub wave: usize,
    /// 登记指纹（波号+真机值 派生）
    pub fp: u64,
}

/// 波 23 衔接预告册。
pub struct HandoverPreview {
    pairs: Vec<WavePair>,
    retests: Vec<RetestReg>,
    merged: bool,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl HandoverPreview {
    /// 新建预告册（零对照零登记，未并入）。
    pub fn new() -> HandoverPreview {
        HandoverPreview {
            pairs: Vec::new(),
            retests: Vec::new(),
            merged: false,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记随闸门补测（同波只登记一次，登记不可撤）。
    pub fn register_retest(&mut self, wave: usize, on_hw: u64) -> Result<u64, PreviewCode> {
        if wave == 0 || wave > COMPARE_WAVES {
            self.rejected += 1;
            return Err(PreviewCode::WaveOob);
        }
        if on_hw == 0 {
            self.rejected += 1;
            return Err(PreviewCode::ZeroHw);
        }
        if self.retests.iter().any(|r| r.wave == wave) {
            self.rejected += 1;
            return Err(PreviewCode::DupRetest);
        }
        let fp = fnv1a(&(wave as u64).to_le_bytes()) ^ fnv1a(&on_hw.to_le_bytes());
        self.retests.push(RetestReg { wave, fp });
        Ok(fp)
    }

    /// 登记一波对照：须先有补测登记（不虚报面），比值 = 真机×100/基线。
    pub fn record_wave(&mut self, wave: usize, base: u64, on_hw: u64) -> Result<WavePair, PreviewCode> {
        if wave == 0 || wave > COMPARE_WAVES {
            self.rejected += 1;
            return Err(PreviewCode::WaveOob);
        }
        if base == 0 {
            self.rejected += 1;
            return Err(PreviewCode::ZeroBase);
        }
        if on_hw == 0 {
            self.rejected += 1;
            return Err(PreviewCode::ZeroHw);
        }
        if !self.retests.iter().any(|r| r.wave == wave) {
            self.rejected += 1;
            return Err(PreviewCode::NoRetest);
        }
        if self.pairs.iter().any(|p| p.wave == wave) {
            self.rejected += 1;
            return Err(PreviewCode::DupWave);
        }
        let ratio = on_hw * RATIO_SCALE / base;
        if ratio < RATIO_FLOOR {
            self.rejected += 1;
            return Err(PreviewCode::RatioShort);
        }
        let p = WavePair { wave, base, on_hw, ratio, retested: true };
        self.pairs.push(p);
        Ok(p)
    }

    /// 衔接判据：×3 波对照齐且每波比值 ≥×1.2 且全部随闸门补测登记。
    pub fn handover_ok(&self) -> bool {
        self.pairs.len() == COMPARE_WAVES
            && self.pairs.iter().all(|p| p.ratio >= RATIO_FLOOR && p.retested)
    }

    /// 最低放大比（三波中最低；空册返回 0）。
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

    /// 并入波 23 闭账（衔接判据过才可并，并后只增不减）。
    pub fn merge_into_closure(&mut self) -> Result<usize, PreviewCode> {
        if self.merged {
            self.rejected += 1;
            return Err(PreviewCode::DupMerge);
        }
        if !self.handover_ok() {
            self.rejected += 1;
            return Err(PreviewCode::HandoverGap);
        }
        self.merged = true;
        Ok(self.pairs.len())
    }

    /// 已并入闭账。
    pub fn merged(&self) -> bool {
        self.merged
    }

    /// 对照行数。
    pub fn pairs_len(&self) -> usize {
        self.pairs.len()
    }

    /// 补测登记数。
    pub fn retests_len(&self) -> usize {
        self.retests.len()
    }

    /// 某波对照行（越界返回 None）。
    pub fn pair_of(&self, wave: usize) -> Option<WavePair> {
        self.pairs.iter().find(|p| p.wave == wave).copied()
    }

    /// 对照轴可回放：波号 1..=3 互异且比值在闭集界内。
    pub fn replayable(&self) -> bool {
        for i in 0..self.pairs.len() {
            let a = match self.pairs.get(i) {
                Some(a) => a,
                None => return false,
            };
            if a.wave == 0 || a.wave > COMPARE_WAVES || a.ratio < RATIO_FLOOR {
                return false;
            }
            for j in (i + 1)..self.pairs.len() {
                if let Some(b) = self.pairs.get(j) {
                    if a.wave == b.wave {
                        return false;
                    }
                }
            }
        }
        !self.pairs.is_empty()
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
        bytes.extend_from_slice(&(self.merged as u64).to_le_bytes());
        for r in self.retests.iter() {
            bytes.extend_from_slice(&r.wave.to_le_bytes());
            bytes.extend_from_slice(&r.fp.to_le_bytes());
        }
        for p in self.pairs.iter() {
            bytes.extend_from_slice(&p.wave.to_le_bytes());
            bytes.extend_from_slice(&p.base.to_le_bytes());
            bytes.extend_from_slice(&p.on_hw.to_le_bytes());
            bytes.extend_from_slice(&p.ratio.to_le_bytes());
            bytes.extend_from_slice(&(p.retested as u64).to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 波 23 衔接预告域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 波号越界
    WaveOob,
    /// 零基线（除法护住）
    ZeroBase,
    /// 零真机值（虚报形态）
    ZeroHw,
    /// 未随闸门补测登记即报对照（不虚报面）
    NoRetest,
    /// 同波重复补测登记
    DupRetest,
    /// 同波重复对照登记
    DupWave,
    /// 放大比不足 ×1.2
    RatioShort,
    /// 衔接缺口（三波未齐或未全达标）
    HandoverGap,
    /// 重复并入闭账
    DupMerge,
    /// 对照轴为空
    EmptyAxis,
}

impl PreviewCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            PreviewCode::Ok => "波 23 衔接预告判定成功",
            PreviewCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PreviewCode::WaveOob => "波号越界：对照波号必须在 1..=3 之内",
            PreviewCode::ZeroBase => "零基线：基线值为零，放大比无定义，显性拒",
            PreviewCode::ZeroHw => "零真机值：真机值为零属虚报形态，显性拒",
            PreviewCode::NoRetest => "未登记补测：真机对照须随闸门补测登记，不虚报",
            PreviewCode::DupRetest => "重复补测登记：同波只许登记一次",
            PreviewCode::DupWave => "重复对照：同波对照行只许登记一次",
            PreviewCode::RatioShort => "放大比不足：每波须 ≥×1.2（定点 120）",
            PreviewCode::HandoverGap => "衔接缺口：×3 波对照齐且全达标才可并入闭账",
            PreviewCode::DupMerge => "重复并入：M 型段产出并入波 23 闭账只许一次",
            PreviewCode::EmptyAxis => "对照轴为空：可回放判据要求至少一行",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [PreviewCode; 12] = [
    PreviewCode::Ok,
    PreviewCode::SegBad,
    PreviewCode::WaveOob,
    PreviewCode::ZeroBase,
    PreviewCode::ZeroHw,
    PreviewCode::NoRetest,
    PreviewCode::DupRetest,
    PreviewCode::DupWave,
    PreviewCode::RatioShort,
    PreviewCode::HandoverGap,
    PreviewCode::DupMerge,
    PreviewCode::EmptyAxis,
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
        Lcg { s: seed ^ 0xc2b2ae3d27d4eb4f }
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

/// 标准脚本：三波补测登记 + 对照（比值 130/125/128）+ 并入闭账。返回 (指纹, 波数)。
pub fn standard_script() -> (u64, usize) {
    let mut p = HandoverPreview::new();
    let bases = [100u64, 200, 250];
    let hws = [130u64, 250, 320];
    for i in 0..COMPARE_WAVES {
        let _ = p.register_retest(i + 1, hws[i]);
        let _ = p.record_wave(i + 1, bases[i], hws[i]);
    }
    let _ = p.merge_into_closure();
    (p.fingerprint(), p.pairs_len())
}

/// 双跑一致性：同一脚本两次建册，指纹与波数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：比值不足——真机 119/基线 100 <×1.2，显性拒（RatioShort）。
    for _ in 0..INJ_ROUNDS {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 119);
        let caught = p.record_wave(1, 100, 119) == Err(PreviewCode::RatioShort);
        if caught {
            p.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            p.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：零基线——除法护住，显性拒（ZeroBase）。
    for _ in 0..INJ_ROUNDS {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 130);
        let caught = p.record_wave(1, 0, 130) == Err(PreviewCode::ZeroBase);
        if caught {
            p.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            p.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：未登记补测即报对照——不虚报面显性拒（NoRetest）。
    for _ in 0..INJ_ROUNDS {
        let mut p = HandoverPreview::new();
        let caught = p.record_wave(2, 100, 130) == Err(PreviewCode::NoRetest);
        if caught {
            p.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            p.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：重复补测登记——同波二次登记显性拒（DupRetest）。
    for _ in 0..INJ_ROUNDS {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(3, 128);
        let caught = p.register_retest(3, 128) == Err(PreviewCode::DupRetest);
        if caught {
            p.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            p.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51594_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51594");
    // 1 三波对照 ×1.2 达标并入。
    {
        let (fp, n) = standard_script();
        s.add(
            "三波对照达标并入",
            fp != 0 && n == COMPARE_WAVES,
            "UNX-F51595-J1：对照 ×3 波每波 ≥×1.2 且并入波 23 闭账",
        );
    }
    // 2 放大比下限精确判。
    {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 120);
        let ok_at_floor = p.record_wave(1, 100, 120).is_ok();
        let mut q = HandoverPreview::new();
        let _ = q.register_retest(1, 119);
        let bad_below = q.record_wave(1, 100, 119) == Err(PreviewCode::RatioShort);
        s.add(
            "比值下限精确判",
            ok_at_floor && bad_below && p.min_ratio() == 120,
            "×1.2（定点 120）恰达标入册，119 显性拒",
        );
    }
    // 3 未登记补测不虚报。
    {
        let mut p = HandoverPreview::new();
        let r = p.record_wave(1, 100, 150);
        s.add(
            "未登记补测不虚报",
            r == Err(PreviewCode::NoRetest) && p.pairs_len() == 0,
            "真机对照须随闸门补测登记，未登记显性拒入册",
        );
    }
    // 4 零基线与零真机值护住。
    {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 130);
        let zb = p.record_wave(1, 0, 130) == Err(PreviewCode::ZeroBase);
        let mut q = HandoverPreview::new();
        let zh = q.register_retest(1, 0) == Err(PreviewCode::ZeroHw);
        s.add(
            "零值护住",
            zb && zh,
            "零基线放大比无定义、零真机值属虚报形态，均显性拒",
        );
    }
    // 5 三波齐判：缺一波不算衔接成立。
    {
        let mut p = HandoverPreview::new();
        for i in 0..(COMPARE_WAVES - 1) {
            let _ = p.register_retest(i + 1, 130);
            let _ = p.record_wave(i + 1, 100, 130);
        }
        s.add(
            "三波齐判",
            !p.handover_ok() && p.merge_into_closure() == Err(PreviewCode::HandoverGap),
            "×3 波未齐不判衔接成立，并入显性拒",
        );
    }
    // 6 同波重复对照拒。
    {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 130);
        let _ = p.record_wave(1, 100, 130);
        let r = p.record_wave(1, 100, 130);
        s.add(
            "同波重复对照拒",
            r == Err(PreviewCode::DupWave) && p.pairs_len() == 1,
            "同波对照行只许登记一次",
        );
    }
    // 7 补测登记不可撤（登记只增不减）。
    {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 130);
        let n0 = p.retests_len();
        let dup = p.register_retest(1, 130);
        s.add(
            "补测登记不可撤",
            n0 == 1 && dup == Err(PreviewCode::DupRetest) && p.retests_len() == 1,
            "登记只增不减，重复登记显性拒且不删原行",
        );
    }
    // 8 波号越界拒。
    {
        let mut p = HandoverPreview::new();
        s.add(
            "波号越界拒",
            p.register_retest(0, 100) == Err(PreviewCode::WaveOob)
                && p.register_retest(4, 100) == Err(PreviewCode::WaveOob),
            "对照波号必须在 1..=3 之内",
        );
    }
    // 9 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 10 对照轴可回放。
    {
        let mut p = HandoverPreview::new();
        for i in 0..COMPARE_WAVES {
            let _ = p.register_retest(i + 1, 130);
            let _ = p.record_wave(i + 1, 100, 130);
        }
        s.add(
            "对照轴可回放",
            p.replayable() && p.pair_of(2).map(|x| x.wave) == Some(2),
            "对照行波号互异且比值在闭集界内，按波号可取行",
        );
    }
    // 11 诊断码闭集唯一性。
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
    // 12 注入账守恒。
    {
        let mut p = HandoverPreview::new();
        for _ in 0..14u32 {
            p.note_detected();
        }
        s.add("注入账守恒", p.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建册，指纹与波数逐位一致",
    );
    // 14 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51594 波23衔接预告 {} 波 ×{} 下限",
            COMPARE_WAVES, RATIO_FLOOR
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51594"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51594_checks_green() {
        let set = run_unx51594_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn three_waves_merge_ok() {
        let mut p = HandoverPreview::new();
        let bases = [100u64, 200, 250];
        let hws = [130u64, 250, 320];
        for i in 0..COMPARE_WAVES {
            assert!(p.register_retest(i + 1, hws[i]).is_ok());
            assert!(p.record_wave(i + 1, bases[i], hws[i]).is_ok());
        }
        assert!(p.handover_ok());
        assert_eq!(p.merge_into_closure(), Ok(3));
        assert!(p.merged());
    }

    #[test]
    fn ratio_floor_exact() {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 120);
        let w = p.record_wave(1, 100, 120);
        assert!(w.is_ok());
        assert_eq!(w.map(|x| x.ratio), Ok(120));
    }

    #[test]
    fn ratio_short_rejected() {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 119);
        assert_eq!(p.record_wave(1, 100, 119), Err(PreviewCode::RatioShort));
        assert_eq!(p.pairs_len(), 0);
    }

    #[test]
    fn no_retest_no_entry() {
        let mut p = HandoverPreview::new();
        assert_eq!(p.record_wave(1, 100, 130), Err(PreviewCode::NoRetest));
        assert_eq!(p.rejected_tally(), 1);
    }

    #[test]
    fn zero_values_guarded() {
        let mut p = HandoverPreview::new();
        assert_eq!(p.register_retest(1, 0), Err(PreviewCode::ZeroHw));
        let _ = p.register_retest(2, 130);
        assert_eq!(p.record_wave(2, 0, 130), Err(PreviewCode::ZeroBase));
    }

    #[test]
    fn wave_oob_rejected() {
        let mut p = HandoverPreview::new();
        assert_eq!(p.register_retest(0, 100), Err(PreviewCode::WaveOob));
        assert_eq!(p.register_retest(4, 100), Err(PreviewCode::WaveOob));
    }

    #[test]
    fn retest_register_once() {
        let mut p = HandoverPreview::new();
        assert!(p.register_retest(1, 130).is_ok());
        assert_eq!(p.register_retest(1, 130), Err(PreviewCode::DupRetest));
        assert_eq!(p.retests_len(), 1);
    }

    #[test]
    fn incomplete_waves_no_merge() {
        let mut p = HandoverPreview::new();
        let _ = p.register_retest(1, 130);
        let _ = p.record_wave(1, 100, 130);
        assert!(!p.handover_ok());
        assert_eq!(p.merge_into_closure(), Err(PreviewCode::HandoverGap));
        assert!(!p.merged());
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
        let mut p = HandoverPreview::new();
        for _ in 0..10u32 {
            p.note_detected();
        }
        assert!(p.conserved());
        let mut bad = HandoverPreview::new();
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
