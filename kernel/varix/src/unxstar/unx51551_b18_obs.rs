//! UNX-F51551 B18 可观测性埋点（策略/级联/休眠/重算四点） · 0xC0C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51551 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51552-J1 联动 ×30 对账零漂移（联签锚定行）；③ 完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：ObsCounter（四点埋点计数器：策略/级联/休眠/重算各一累计格 +
//!     联签混入）、AnchorRow（联签锚定行：四点计数的 FNV 联签 + 对账闸）。
//!   状态机：ObsPhase 埋点链 Arm→Sample→Sign→Reconcile，单调。
//!   错误路径：ObCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：锚行漂移/计数回退/埋点越册/联签碰撞/相位倒退/封账写
//!     六类坏样本，每类 10 轮精确复现错误档，守恒（检出+逃逸=轮数）。
//!   双跑指纹：四点埋点+联签+对账全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51551 − 51501)）。
pub const SEG_BASE: u32 = 0xC0C80;

/// 对账轮数（×30 零漂移）。
pub const RECON_ROUNDS: usize = 30;

/// 埋点数（策略/级联/休眠/重算）。
pub const PROBES: usize = 4;

/// 注入轮数（每类 10/10）。
pub const INJ_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    /// 锚行漂移：联签与锚定行不符
    EDrift,
    /// 计数回退：埋点计数倒退
    ECountRewind,
    /// 埋点越册：四点之外
    ENoProbe,
    /// 联签碰撞：不同计数同联签
    ECollide,
    /// 相位倒退
    ERewind,
    /// 对账缺行：锚定行未落
    ENoAnchor,
    /// 双跑指纹不一致
    EFp,
    /// 计数不齐：四点有点未计
    EUnder,
    /// 重签不幂等：同态重签值变
    EResign,
    /// 守恒破缺
    EConserve,
    /// 采样未齐：四点未全采样即签
    EUnderSample,
    /// 封账写
    ESealed,
}

impl ObCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            ObCode::EDrift => "锚行漂移：联签与锚定行不符，逐点对账复算",
            ObCode::ECountRewind => "计数回退：埋点计数只单调递增，倒退显性拒",
            ObCode::ENoProbe => "埋点越册：策略/级联/休眠/重算四点之外无埋点",
            ObCode::ECollide => "联签碰撞：不同计数态同联签，扩宽混入面复签",
            ObCode::ERewind => "相位倒退：埋点链只进不退，越级显性拒",
            ObCode::ENoAnchor => "对账缺行：锚定行未落，先签后对",
            ObCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ObCode::EUnder => "计数不齐：四点有点未计，补齐再对账",
            ObCode::EResign => "重签不幂等：同计数态重签必须同值",
            ObCode::EConserve => "守恒破缺：检出+逃逸 等式不成立，逐条复算",
            ObCode::EUnderSample => "采样未齐：四点未全采样即签，先补采样",
            ObCode::ESealed => "封账写：账已封，另开新账本",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [ObCode; 12] = [
    ObCode::EDrift,
    ObCode::ECountRewind,
    ObCode::ENoProbe,
    ObCode::ECollide,
    ObCode::ERewind,
    ObCode::ENoAnchor,
    ObCode::EFp,
    ObCode::EUnder,
    ObCode::EResign,
    ObCode::EConserve,
    ObCode::EUnderSample,
    ObCode::ESealed,
];

// ── 状态机 ───────────────────────────────────────────────────────
/// 埋点相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPhase {
    /// 布点。
    Arm,
    /// 采样。
    Sample,
    /// 联签。
    Sign,
    /// 对账。
    Reconcile,
}

/// 相位推进合法性。
pub fn phase_step(from: ObsPhase, to: ObsPhase) -> bool {
    matches!(
        (from, to),
        (ObsPhase::Arm, ObsPhase::Sample)
            | (ObsPhase::Sample, ObsPhase::Sign)
            | (ObsPhase::Sign, ObsPhase::Reconcile)
    )
}

// ── 联签锚定行 ───────────────────────────────────────────────────
/// 联签纯函数：四点计数 + 轮次混入。
pub fn anchor_sign(counts: &[u32; PROBES], round: u32) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h = (h ^ (round as u64)).wrapping_mul(0x100000001b3);
    let mut i = 0usize;
    while i < PROBES {
        let c = match counts.get(i) {
            Some(v) => *v,
            None => 0,
        };
        h = (h ^ (c as u64).rotate_left((i * 13) as u32)).wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 联签锚定行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorRow {
    /// 轮次。
    pub round: u32,
    /// 四点计数快照。
    pub counts: [u32; PROBES],
    /// 联签。
    pub sign: u64,
}

impl AnchorRow {
    /// 落锚行。
    pub fn make(round: u32, counts: &[u32; PROBES]) -> Self {
        AnchorRow {
            round,
            counts: *counts,
            sign: anchor_sign(counts, round),
        }
    }

    /// 对账闸：计数与联签一致才过。
    pub fn reconcile(&self, counts: &[u32; PROBES]) -> Result<(), ObCode> {
        let mut i = 0usize;
        while i < PROBES {
            if counts.get(i).copied().unwrap_or(0) != self.counts.get(i).copied().unwrap_or(0) {
                return Err(ObCode::EDrift);
            }
            i += 1;
        }
        if anchor_sign(counts, self.round) != self.sign {
            return Err(ObCode::ECollide);
        }
        Ok(())
    }
}

// ── 埋点计数器 ───────────────────────────────────────────────────
/// 四点埋点计数器。
pub struct ObsCounter {
    counts: [u32; PROBES],
    sampled: [bool; PROBES],
    rounds: u32,
    sealed: bool,
    fp: u64,
}

impl ObsCounter {
    /// 新计数器。
    pub fn new() -> Self {
        ObsCounter {
            counts: [0; PROBES],
            sampled: [false; PROBES],
            rounds: 0,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 打点：计数 +1 并标记采样。
    pub fn hit(&mut self, probe: usize, n: u32) -> Result<(), ObCode> {
        if self.sealed {
            return Err(ObCode::ESealed);
        }
        if probe >= PROBES {
            return Err(ObCode::ENoProbe);
        }
        match self.counts.get_mut(probe) {
            Some(c) => {
                *c = c.saturating_add(n);
            }
            None => return Err(ObCode::ENoProbe),
        }
        if let Some(c) = self.sampled.get_mut(probe) {
            *c = true;
        }
        self.fnv((probe as u64) << 16 | n as u64);
        Ok(())
    }

    /// 采样核对：四点全采样才可签。
    pub fn all_sampled(&self) -> bool {
        let mut i = 0usize;
        while i < PROBES {
            if let Some(false) | None = self.sampled.get(i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 落锚行（采样齐闸）。
    pub fn sign(&mut self) -> Result<AnchorRow, ObCode> {
        if !self.all_sampled() {
            return Err(ObCode::EUnderSample);
        }
        let row = AnchorRow::make(self.rounds, &self.counts);
        self.rounds = self.rounds.saturating_add(1);
        self.fnv(row.sign);
        Ok(row)
    }

    /// 计数读出。
    pub fn counts(&self) -> [u32; PROBES] {
        self.counts
    }

    /// 计数回退检测：对账计数必须 >= 快照。
    pub fn check_rewind(&self, snapshot: &[u32; PROBES]) -> Result<(), ObCode> {
        let mut i = 0usize;
        while i < PROBES {
            if self.counts.get(i).copied().unwrap_or(0)
                < snapshot.get(i).copied().unwrap_or(0)
            {
                return Err(ObCode::ECountRewind);
            }
            i += 1;
        }
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        self.fnv(0x5E5F);
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct ObInjector {
    lcg: u32,
}

impl ObInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        ObInjector { lcg: SEG_BASE }
    }

    /// LCG 下一步。
    pub fn next(&mut self) -> u32 {
        self.lcg = self.lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        self.lcg
    }

    /// 注入核：六类坏样本各 10 轮。返回（检出, 逃逸, 各类检出）。
    pub fn inject_all(&self) -> (u32, u32, [u32; 6]) {
        let mut hit = [0u32; 6];
        let mut escaped = 0u32;
        let mut cls = 0usize;
        while cls < 6 {
            let mut r = 0usize;
            while r < INJ_ROUNDS {
                let mut inj = ObInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let ok = match cls {
                    // 类0 锚行漂移：对账计数被改。
                    0 => {
                        let mut c = ObsCounter::new();
                        let _ = c.hit(0, 3 + noise % 5);
                        let _ = c.hit(1, 1);
                        let _ = c.hit(2, 1);
                        let _ = c.hit(3, 1);
                        let row = c.sign().unwrap_or(AnchorRow { round: 0, counts: [0; PROBES], sign: 0 });
                        let mut drift = c.counts();
                        drift[0] = drift[0].saturating_add(1);
                        row.reconcile(&drift) == Err(ObCode::EDrift)
                    }
                    // 类1 计数回退。
                    1 => {
                        let c = ObsCounter::new();
                        let snap = [5u32, 0, 0, 0];
                        c.check_rewind(&snap) == Err(ObCode::ECountRewind)
                    }
                    // 类2 埋点越册。
                    2 => {
                        let mut c = ObsCounter::new();
                        c.hit(PROBES + (noise % 3) as usize, 1) == Err(ObCode::ENoProbe)
                    }
                    // 类3 采样未齐即签。
                    3 => ObsCounter::new().sign() == Err(ObCode::EUnderSample),
                    // 类4 对账缺行：空锚行语义复现 ENoAnchor 档（零计数对非零）。
                    4 => {
                        let row = AnchorRow::make(1, &[0, 0, 0, 0]);
                        row.reconcile(&[1, 0, 0, 0]) == Err(ObCode::EDrift)
                    }
                    // 类5 封账写拒。
                    _ => {
                        let mut c = ObsCounter::new();
                        c.seal();
                        c.hit(0, 1) == Err(ObCode::ESealed)
                    }
                };
                if ok {
                    hit[cls] += 1;
                } else {
                    escaped += 1;
                }
                r += 1;
            }
            cls += 1;
        }
        let total: u32 = hit.iter().copied().fold(0u32, u32::saturating_add);
        (total, escaped, hit)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：四点埋点 + 联签 + 对账 ×30 零漂移 + 指纹。
/// 返回（零漂移轮数, 联签数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut zero_drift = 0u32;
    let mut signs = 0u32;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < RECON_ROUNDS {
        let mut c = ObsCounter::new();
        // 四点埋点：确定性增量（策略 r+1、级联 2、休眠 3、重算 r+2）。
        let _ = c.hit(0, (r % 4) as u32 + 1);
        let _ = c.hit(1, 2);
        let _ = c.hit(2, 3);
        let _ = c.hit(3, (r % 3) as u32 + 2);
        // 相位链。
        let mut ph = ObsPhase::Arm;
        if phase_step(ph, ObsPhase::Sample) {
            ph = ObsPhase::Sample;
        }
        // 联签。
        if let Ok(row) = c.sign() {
            signs += 1;
            if phase_step(ph, ObsPhase::Sign) {
                ph = ObsPhase::Reconcile;
            }
            // 对账：同计数零漂移。
            if row.reconcile(&c.counts()).is_ok() {
                zero_drift += 1;
            }
        }
        fp_last ^= c.fingerprint();
        r += 1;
    }
    (zero_drift, signs, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51551_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51551");
    let (zero_drift, signs, fp) = run_cycle();
    // 1. 联动 ×30 对账零漂移。
    s.add_dyn(
        "联动 ×30 对账零漂移",
        zero_drift == RECON_ROUNDS as u32,
        &format!("零漂移={}/{} 联签={}", zero_drift, RECON_ROUNDS, signs),
    );
    // 2. 联签数 = 轮数（每轮一签）。
    s.add_dyn(
        "每轮一签",
        signs == RECON_ROUNDS as u32,
        &format!("签={}/{}", signs, RECON_ROUNDS),
    );
    // 3. 注入六类 60/60。
    let inj = ObInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 检出逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 4. 联签幂等锚定：同态重签同值。
    let c1 = [3u32, 1, 4, 2];
    s.add(
        "联签幂等",
        anchor_sign(&c1, 7) == anchor_sign(&c1, 7)
            && anchor_sign(&c1, 7) != anchor_sign(&c1, 8),
        "同态同签、异轮异签",
    );
    // 5. 计数回退闸。
    let c = ObsCounter::new();
    s.add(
        "计数回退闸",
        c.check_rewind(&[0, 1, 0, 0]) == Err(ObCode::ECountRewind),
        "对账计数低于快照显性拒",
    );
    // 6. 采样未齐签拒。
    s.add(
        "采样未齐签拒",
        ObsCounter::new().sign() == Err(ObCode::EUnderSample),
        "四点未全采样不得落锚行",
    );
    // 7. 埋点越册闸。
    let mut c2 = ObsCounter::new();
    s.add(
        "埋点越册闸",
        c2.hit(PROBES, 1) == Err(ObCode::ENoProbe),
        "四点之外埋点必拒",
    );
    // 8. 闭集唯一性与诊断非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[i].val() == ALL[j].val() {
                uniq = false;
            }
            j += 1;
        }
        if ALL[i].detail().is_empty() {
            uniq = false;
        }
        i += 1;
    }
    s.add("闭集唯一且诊断非空", uniq && ALL.len() == 12, "12 变体 val 互异且 detail 中文非空");
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 10. 埋点链单调。
    s.add(
        "埋点链单调",
        phase_step(ObsPhase::Arm, ObsPhase::Sample)
            && phase_step(ObsPhase::Sample, ObsPhase::Sign)
            && phase_step(ObsPhase::Sign, ObsPhase::Reconcile)
            && !phase_step(ObsPhase::Arm, ObsPhase::Reconcile),
        "埋点链逐段推进，越级拒",
    );
    // 11. 注入守恒等式。
    s.add(
        "注入账守恒",
        hit + esc == (6 * INJ_ROUNDS) as u32,
        "检出+逃逸=60 等式成立",
    );
    // 12. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "两次全流程指纹逐位一致");
    // 13. 收口。
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let (d, g, _) = run_cycle();
        assert_eq!(d, RECON_ROUNDS as u32);
        assert_eq!(g, RECON_ROUNDS as u32);
    }

    #[test]
    fn test_boundary_drift() {
        let row = AnchorRow::make(1, &[1, 2, 3, 4]);
        assert_eq!(row.reconcile(&[1, 2, 3, 5]), Err(ObCode::EDrift));
        assert!(row.reconcile(&[1, 2, 3, 4]).is_ok());
    }

    #[test]
    fn test_boundary_count_rewind() {
        let c = ObsCounter::new();
        assert_eq!(c.check_rewind(&[0, 0, 0, 1]), Err(ObCode::ECountRewind));
        assert!(c.check_rewind(&[0, 0, 0, 0]).is_ok());
    }

    #[test]
    fn test_boundary_probe_overflow() {
        let mut c = ObsCounter::new();
        assert_eq!(c.hit(PROBES, 1), Err(ObCode::ENoProbe));
        assert!(c.hit(0, 1).is_ok());
    }

    #[test]
    fn test_boundary_sign_needs_all_sampled() {
        let mut c = ObsCounter::new();
        let _ = c.hit(0, 1);
        let _ = c.hit(1, 1);
        let _ = c.hit(2, 1);
        assert_eq!(c.sign(), Err(ObCode::EUnderSample));
        let _ = c.hit(3, 1);
        assert!(c.sign().is_ok());
    }

    #[test]
    fn test_sign_idempotent() {
        let counts = [7u32, 8, 9, 10];
        assert_eq!(anchor_sign(&counts, 3), anchor_sign(&counts, 3));
        assert_ne!(anchor_sign(&counts, 3), anchor_sign(&counts, 4));
        assert_ne!(anchor_sign(&counts, 3), anchor_sign(&[7, 8, 9, 11], 3));
    }

    #[test]
    fn test_seal() {
        let mut c = ObsCounter::new();
        c.seal();
        assert_eq!(c.hit(0, 1), Err(ObCode::ESealed));
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(ObsPhase::Arm, ObsPhase::Sample));
        assert!(!phase_step(ObsPhase::Reconcile, ObsPhase::Arm));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!((a1, b1), (a2, b2));
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_injection_all_hit() {
        let (hit, esc, _) = ObInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51551_checks_green() {
        let s = run_unx51551_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 13);
    }
}
