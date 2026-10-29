//! UNX-F56048 启动分段方差账（0x605E0 段）
//!
//! 锚点：UNX-F56048-J1 同镜像 10 次连测逐段 P50/P95/P99+方差导出；J2
//! 方差超阈值（段 P95 漂移 >15%）触发重测规则落账；J3 连测纪律（同镜
//! 像口径：镜像指纹逐轮一致，跨镜像混账即拒）三判据全绿。判据锚定
//! §AI-71.3 示例条目方差口径。
//! 域：CoRun·UNX-F2-B40（启动分段域·方差统计面）。
//!
//! 结构：连测账（轮 × 镜像指纹 × 段时长逐轮落账）+ 逐段分位核（P50/
//! P95/P99 + 极差方差）+ 重测闸（段 P95 相对 P50 漂移 >1500 万分比触
//! 发）+ 镜像一致性闸（跨指纹混账拒）+ 注入核（连测不足/越闭集段/跨
//! 镜像混账/重测闸失灵/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x605E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x605E0;
pub const VR_RUN_SHORT: u32 = SEG_BASE + 0x01; // 连测不足 10 轮
pub const VR_SEG_UNK: u32 = SEG_BASE + 0x02; // 越闭集段
pub const VR_IMAGE_MIX: u32 = SEG_BASE + 0x03; // 跨镜像混账
pub const VR_RETEST: u32 = SEG_BASE + 0x04; // 方差超阈触发重测
pub const VR_SEALED: u32 = SEG_BASE + 0x05; // 方差账已封账拒写
pub const VR_DUP_RUN: u32 = SEG_BASE + 0x06; // 同轮重复
pub const VR_NS_ZERO: u32 = SEG_BASE + 0x07; // 零段时长

/// 连测轮数（10）与重测阈值（万分比 15% = 1500）。
pub const RUNS_N: usize = 10;
pub const RETEST_PBP: u32 = 1_500;

/// 段数（与 F56041 五段同源）。
pub const G_COUNT: usize = 5;

/// 单轮账：轮号 × 镜像指纹 × 五段时长。
#[derive(Clone)]
pub struct VarRun {
    pub seq: u32,
    pub image_fp: u32,
    pub segs: [u64; G_COUNT],
}

impl VarRun {
    pub fn new(seq: u32, image_fp: u32, segs: [u64; G_COUNT]) -> Result<Self, u32> {
        if seq == 0 {
            return Err(VR_DUP_RUN);
        }
        for g in 0..G_COUNT {
            if segs[g] == 0 {
                return Err(VR_NS_ZERO);
            }
        }
        Ok(VarRun { seq, image_fp, segs })
    }
}

/// 逐段分位与方差出参。
#[derive(Clone)]
pub struct SegVarStats {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    /// 极差（max-min）作方差口径。
    pub range: u64,
    /// P95 相对 P50 漂移（万分比）。
    pub drift_pbp: u32,
}

/// 万分比漂移。
pub fn drift_pbp(base: u64, sample: u64) -> u32 {
    if base == 0 {
        return u32::MAX;
    }
    let d = (sample as i64 - base as i64).abs();
    ((d * 10_000) / base as i64) as u32
}

/// 连测方差账。
pub struct VarBook {
    pub runs: Vec<VarRun>,
    pub retests: Vec<(usize, u32)>,
    pub sealed: bool,
}

impl VarBook {
    pub fn new() -> Self {
        VarBook { runs: Vec::new(), retests: Vec::new(), sealed: false }
    }

    /// 落一轮（同轮拒 + 跨镜像混账拒）。
    pub fn put(&mut self, r: VarRun) -> Result<(), u32> {
        if self.sealed {
            return Err(VR_SEALED);
        }
        for p in self.runs.iter() {
            if p.seq == r.seq {
                return Err(VR_DUP_RUN);
            }
            if p.image_fp != r.image_fp {
                return Err(VR_IMAGE_MIX);
            }
        }
        self.runs.push(r);
        Ok(())
    }

    /// J1：逐段分位导出（连测 ≥10 轮才出）。
    pub fn seg_stats(&self, g: usize) -> Result<SegVarStats, u32> {
        if g >= G_COUNT {
            return Err(VR_SEG_UNK);
        }
        if self.runs.len() < RUNS_N {
            return Err(VR_RUN_SHORT);
        }
        let mut v: Vec<u64> = Vec::new();
        for r in self.runs.iter() {
            v.push(r.segs[g]);
        }
        v.sort();
        let p50 = v[v.len() / 2];
        let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
        let p99 = v[(v.len() * 99 / 100).min(v.len() - 1)];
        let range = v[v.len() - 1] - v[0];
        let drift = drift_pbp(p50, p95);
        Ok(SegVarStats { p50, p95, p99, range, drift_pbp: drift })
    }

    /// J2：重测闸扫描（五段逐段检，超阈落重测账）。
    pub fn scan_retest(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(VR_SEALED);
        }
        let mut n = 0usize;
        for g in 0..G_COUNT {
            let st = self.seg_stats(g)?;
            if st.drift_pbp > RETEST_PBP {
                self.retests.push((g, st.drift_pbp));
                n += 1;
            }
        }
        Ok(n)
    }

    /// 重测账留痕齐。
    pub fn retest_trail_ok(&self) -> bool {
        self.retests.iter().all(|r| r.1 > RETEST_PBP)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：10 轮同镜像（指纹 0x1MAGE→0x0ABC0001），段时长确定性
/// （段 g 基线 (g+1)×100ms + 轮 i × 2ms 微抖动）。
pub fn standard_varbook() -> VarBook {
    let mut b = VarBook::new();
    for i in 0..RUNS_N {
        let mut segs = [0u64; G_COUNT];
        for g in 0..G_COUNT {
            segs[g] = (g as u64 + 1) * 100_000_000 + i as u64 * 2_000_000;
        }
        b.put(VarRun::new(i as u32 + 1, 0x0ABC_0001, segs).expect("run")).expect("put");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 连测不足（9 轮）
    let mut b = standard_varbook();
    b.runs.pop();
    if b.seg_stats(0).err() == Some(VR_RUN_SHORT) {
        caught += 1;
    }
    // 2) 越闭集段（g=9）
    let b2 = standard_varbook();
    if b2.seg_stats(9).err() == Some(VR_SEG_UNK) {
        caught += 1;
    }
    // 3) 跨镜像混账
    let mut b3 = standard_varbook();
    if b3.put(VarRun::new(11, 0x0ABC_0002, [1; 5]).expect("mix")) == Err(VR_IMAGE_MIX) {
        caught += 1;
    }
    // 4) 零段时长
    if VarRun::new(1, 1, [0, 1, 1, 1, 1]).err() == Some(VR_NS_ZERO) {
        caught += 1;
    }
    // 5) 同轮重复
    let mut b4 = standard_varbook();
    if b4.put(VarRun::new(5, 0x0ABC_0001, [1; 5]).expect("dup")) == Err(VR_DUP_RUN) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56048_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56048");
    // J1-1 10 轮同镜像连测齐
    let b = standard_varbook();
    let ok = b.runs.len() == RUNS_N
        && b.runs.iter().all(|r| r.image_fp == 0x0ABC_0001);
    s.add("56048-J1-十轮同镜像齐", ok, if ok { "" } else { "连测账缺" });
    // J1-2 逐段分位导出（段 0 基线 100ms 抖 0..18ms：P50=idx5→110，P95=idx9→118）
    let st0 = b.seg_stats(0).expect("st0");
    let ok = st0.p50 == 110_000_000 && st0.p95 == 118_000_000 && st0.p99 == 118_000_000
        && st0.range == 18_000_000;
    s.add("56048-J1-逐段分位导出", ok, if ok { "" } else { "分位漂移" });
    // J1-3 段梯度（段 4 基线 500ms > 段 0 基线 100ms）
    let st4 = b.seg_stats(4).expect("st4");
    let ok = st4.p50 == 510_000_000 && st4.p50 > st0.p50;
    s.add("56048-J1-段梯度", ok, if ok { "" } else { "梯度漂移" });
    // J2-1 方差口径（P95 相对 P50 漂移：118/110 → 727 万分比）
    let ok = st0.drift_pbp == 727;
    s.add("56048-J2-方差口径", ok, if ok { "" } else { "方差算式漂移" });
    // J2-2 重测闸不误触（727 < 1500 → 零重测）
    let mut b2 = standard_varbook();
    let n = b2.scan_retest().expect("scan");
    let ok = n == 0 && b2.retests.is_empty();
    s.add("56048-J2-常态零重测", ok, if ok { "" } else { "常态误报" });
    // J2-3 超阈触发重测（注入抖动段：P95/P50 漂移 >15%）
    let mut b3 = VarBook::new();
    for i in 0..RUNS_N {
        let mut segs = [100_000_000u64; G_COUNT];
        // 段 0 抖动：6 轮 100ms、4 轮 200ms → P50=100，P95=200 → 漂移 10000。
        segs[0] = if i < 6 { 100_000_000 } else { 200_000_000 };
        b3.put(VarRun::new(i as u32 + 1, 0x0ABC_0001, segs).expect("run")).expect("put");
    }
    let n3 = b3.scan_retest().expect("scan3");
    let ok = n3 == 1 && b3.retests[0].0 == 0 && b3.retests[0].1 == 10_000 && b3.retest_trail_ok();
    s.add("56048-J2-超阈触发重测", ok, if ok { "" } else { "重测闸失灵" });
    // J2-4 阈值语义（恰 1500 不触发 / 1501 触发）
    let ok = drift_pbp(1_000, 1_150) == 1_500 && drift_pbp(1_000, 1_151) == 1_501;
    s.add("56048-J2-阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J3-1 跨镜像混账拒（J3 纪律）
    let ok = standard_varbook().runs.iter().all(|r| r.image_fp == 0x0ABC_0001);
    s.add("56048-J3-镜像一致性", ok, if ok { "" } else { "混账穿透" });
    // J3-2 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56048-J3-注入核必抓", ok, &detail);
    // J1-4 段闭集（5 段同 F56041）
    let ok = G_COUNT == 5;
    s.add("56048-J1-段闭集", ok, if ok { "" } else { "段数漂移" });
    // J2-5 封账冻结
    let mut b4 = standard_varbook();
    b4.seal();
    let ok = b4.put(VarRun::new(99, 0x0ABC_0001, [1; 5]).expect("s")) == Err(VR_SEALED)
        && b4.scan_retest().is_err();
    s.add("56048-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56048_all_checks_green() {
        let set = run_unx56048_checks();
        assert!(set.all_passed(), "UNX-F56048 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56048_drift_math() {
        assert_eq!(drift_pbp(110_000_000, 118_000_000), 727);
        assert_eq!(drift_pbp(150_000_000, 200_000_000), 3_333);
    }
}
