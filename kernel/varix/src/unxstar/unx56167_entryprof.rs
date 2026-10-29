//! UNX-F56167 syscall 延迟归因分段剖析件（段一：系统调用入口）（0x614E0 段）
//!
//! 锚点：UNX-F56167-J1 入口段剖析账（syscall 入口段延迟剖析——四象限
//! 源（参数校验/句柄查表/权限核/审计钩）逐源延迟聚合落账：源 × 计数
//! × 总延迟 × 均值 × 指纹五元组）；J2 剖析语义（入口段总延迟 = 四源
//! 之和，源占比核）；J3 占比哨兵（单源占比 ≥ 60% 判瓶颈源并留痕）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·入口段剖析面）。
//!
//! 结构：入口段四源闭集 + 延迟录入核（源闭集 + 延迟界）+ 聚合核（计
//! 数/总和/均值/占比四值）+ 瓶颈哨兵（60% 阈判定）+ 剖析指纹 + 注入
//! 核（未知源/延迟越界/总和漂移/占比算术漂移/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x614E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x614E0;
pub const EN_UNK_SRC: u32 = SEG_BASE + 0x01; // 未知源
pub const EN_LAT_RANGE: u32 = SEG_BASE + 0x02; // 延迟越界
pub const EN_SUM_DRIFT: u32 = SEG_BASE + 0x03; // 总和漂移
pub const EN_RATIO_DRIFT: u32 = SEG_BASE + 0x04; // 占比算术漂移
pub const EN_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 入口段四源闭集。
pub const S_ARGCHECK: usize = 0; // 参数校验
pub const S_HANDLE: usize = 1; // 句柄查表
pub const S_PERM: usize = 2; // 权限核
pub const S_AUDIT: usize = 3; // 审计钩
pub const SRC_COUNT: usize = 4;

/// 源名渲染。
pub const SRC_NAMES: [&str; SRC_COUNT] = ["argcheck", "handle", "perm", "audit"];

/// 延迟界（纳秒 ≤ 1ms）。
pub const LAT_MAX: u64 = 1_000_000;

/// 瓶颈占比阈（万分比 6000 = 60%）。
pub const BOTTLENECK_AT: u32 = 6_000;

/// 占比核：万分比整数域。
pub fn ratio_of(part: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((part as u128 * 10_000) / total as u128) as u32
}

/// 瓶颈哨兵：最大源占比 ≥ 阈判瓶颈。
pub fn bottleneck_src(ratios: &[u32; SRC_COUNT]) -> Option<usize> {
    let mut best = 0usize;
    let mut bv = 0u32;
    for (i, r) in ratios.iter().enumerate() {
        if *r > bv {
            bv = *r;
            best = i;
        }
    }
    if bv >= BOTTLENECK_AT {
        Some(best)
    } else {
        None
    }
}

/// 单源剖析账。
#[derive(Clone)]
pub struct EntrySrcAgg {
    pub src: usize,
    pub count: u64,
    pub total_lat: u64,
}

impl EntrySrcAgg {
    fn render_line(&self) -> String {
        format!(
            "{}|n{}|lat{}|mean{}",
            SRC_NAMES[self.src],
            self.count,
            self.total_lat,
            if self.count == 0 { 0 } else { self.total_lat / self.count }
        )
    }
}

/// 入口段剖析器。
pub struct EntryProfile {
    aggs: [EntrySrcAgg; SRC_COUNT],
    sealed: bool,
}

impl EntryProfile {
    pub fn new() -> Self {
        EntryProfile {
            aggs: [
                EntrySrcAgg { src: S_ARGCHECK, count: 0, total_lat: 0 },
                EntrySrcAgg { src: S_HANDLE, count: 0, total_lat: 0 },
                EntrySrcAgg { src: S_PERM, count: 0, total_lat: 0 },
                EntrySrcAgg { src: S_AUDIT, count: 0, total_lat: 0 },
            ],
            sealed: false,
        }
    }

    /// 录一延迟：源闭集 + 延迟界 + 印后拒。
    pub fn record(&mut self, src: usize, lat_ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(EN_EMPTY_SEAL);
        }
        if src >= SRC_COUNT {
            return Err(EN_UNK_SRC);
        }
        if lat_ns > LAT_MAX {
            return Err(EN_LAT_RANGE);
        }
        self.aggs[src].count += 1;
        self.aggs[src].total_lat += lat_ns;
        Ok(())
    }

    pub fn agg(&self, src: usize) -> Option<&EntrySrcAgg> {
        self.aggs.get(src)
    }

    /// 入口段总延迟 = 四源之和。
    pub fn phase_total(&self) -> u64 {
        (0..SRC_COUNT).map(|s| self.aggs[s].total_lat).sum()
    }

    /// 四源占比数组。
    pub fn ratios(&self) -> [u32; SRC_COUNT] {
        let t = self.phase_total();
        [
            ratio_of(self.aggs[0].total_lat, t),
            ratio_of(self.aggs[1].total_lat, t),
            ratio_of(self.aggs[2].total_lat, t),
            ratio_of(self.aggs[3].total_lat, t),
        ]
    }

    /// 瓶颈哨兵查询。
    pub fn bottleneck(&self) -> Option<usize> {
        bottleneck_src(&self.ratios())
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.phase_total() == 0 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 剖析指纹。
    pub fn profile_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for a in &self.aggs {
            h = h.wrapping_mul(0x0100_0193) ^ (a.count as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (a.total_lat as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_profile(&self) -> String {
        let mut out = String::new();
        for a in &self.aggs {
            out.push_str(&a.render_line());
            out.push('\n');
        }
        let b = self.bottleneck();
        if let Some(bi) = b {
            out.push_str(&format!("BOTTLENECK:{}\n", SRC_NAMES[bi]));
        }
        if self.sealed {
            out.push_str("ENTRY-SEAL\n");
        }
        out
    }
}

impl Default for EntryProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：句柄查表主导（argcheck 500×20 / handle 500×700 / perm 500×30 / audit 500×50）。
fn standard_profile() -> EntryProfile {
    let mut p = EntryProfile::new();
    for _ in 0..500 {
        let _ = p.record(S_ARGCHECK, 20);
        let _ = p.record(S_HANDLE, 700);
        let _ = p.record(S_PERM, 30);
        let _ = p.record(S_AUDIT, 50);
    }
    p
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知源
    let mut p = EntryProfile::new();
    if p.record(9, 100) == Err(EN_UNK_SRC) {
        caught += 1;
    }
    // 注 2：延迟越界
    let mut p = EntryProfile::new();
    if p.record(S_HANDLE, LAT_MAX + 1) == Err(EN_LAT_RANGE) {
        caught += 1;
    }
    // 注 3：总和漂移（手改后 phase_total 变）
    let mut p = standard_profile();
    let t0 = p.phase_total();
    p.aggs[0].total_lat += 1;
    if p.phase_total() != t0 {
        caught += 1;
    }
    // 注 4：占比算术核（单源全占比 = 10000）
    let mut p = EntryProfile::new();
    let _ = p.record(S_PERM, 100);
    if p.ratios()[S_PERM] == 10_000 {
        caught += 1;
    }
    // 注 5：空账印
    let mut p = EntryProfile::new();
    if !p.seal() && !p.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 剖析账；J2 剖析语义；J3 占比哨兵。
pub fn run_unx56167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56167");
    let p = standard_profile();
    // J1-1 四源计数齐（各 500）
    let ok = (0..SRC_COUNT).all(|src| p.agg(src).map(|x| x.count == 500).unwrap_or(false));
    s.add("56167-J1-四源计数齐", ok, if ok { "" } else { "计数漂移" });
    // J1-2 渲染导出齐（含瓶颈）
    let text = standard_profile().render_profile();
    let ok = text.contains("handle|n500|lat350000|mean700") && text.contains("BOTTLENECK:handle");
    s.add("56167-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总延迟核（500×800 = 400000）
    let ok = p.phase_total() == 400_000;
    s.add("56167-J2-总延迟核", ok, if ok { "" } else { "总延迟漂移" });
    // J2-2 handle 占比（350000/400000 = 8750）
    let ok = p.ratios()[S_HANDLE] == 8_750;
    s.add("56167-J2-handle占比", ok, if ok { "" } else { "占比漂移" });
    // J2-3 四源占比和 ≈ 10000（整数域容差 3）
    let r = p.ratios();
    let sum: u32 = r[0] + r[1] + r[2] + r[3];
    let ok = sum >= 9_997 && sum <= 10_003;
    s.add("56167-J2-占比和核", ok, if ok { "" } else { "占比和漂移" });
    // J2-4 均值核（handle 700）
    let ok = p.agg(S_HANDLE).map(|x| x.total_lat / x.count == 700).unwrap_or(false);
    s.add("56167-J2-均值核", ok, if ok { "" } else { "均值漂移" });
    // J2-5 占比哨兵零占比守卫
    let ok = ratio_of(1, 0) == 0;
    s.add("56167-J2-零占比守卫", ok, if ok { "" } else { "除零穿透" });
    // J3-1 瓶颈判定 handle
    let ok = p.bottleneck() == Some(S_HANDLE);
    s.add("56167-J3-瓶颈判定", ok, if ok { "" } else { "哨兵漂移" });
    // J3-2 均匀分布无瓶颈（25% ×4 < 60%）
    let mut p2 = EntryProfile::new();
    for _ in 0..100 {
        for src in 0..SRC_COUNT {
            let _ = p2.record(src, 100);
        }
    }
    let ok = p2.bottleneck() == None;
    s.add("56167-J3-均匀无瓶颈", ok, if ok { "" } else { "哨兵误报" });
    // J3-3 阈缘判定（恰 60% 触发）
    let mut p3 = EntryProfile::new();
    let _ = p3.record(S_HANDLE, 6000);
    let _ = p3.record(S_ARGCHECK, 4000);
    let ok = p3.bottleneck() == Some(S_HANDLE);
    s.add("56167-J3-阈缘触发", ok, if ok { "" } else { "阈缘漂移" });
    // J3-4 封印
    let mut p4 = standard_profile();
    let ok = p4.seal() && p4.sealed();
    s.add("56167-J3-封印", ok, if ok { "" } else { "封印失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56167-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56167_all_checks_green() {
        let set = run_unx56167_checks();
        assert!(set.all_passed(), "UNX-F56167 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56167_ratio_stable() {
        assert_eq!(ratio_of(350_000, 400_000), 8_750);
    }
}
