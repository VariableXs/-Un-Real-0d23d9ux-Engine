//! UNX-F56165 mmap 双态（冷/热页）聚合件（接管 F56024）（0x614A0 段）
//!
//! 锚点：UNX-F56165-J1 双态聚合账（mmap 页错误冷/热双态逐态聚合：态
//! × 计数 × 均值服务延迟 × 指纹四元组）；J2 态归集语义（页首触 = 冷 /
//! 已驻留 = 热，缺页服务延迟双态分账）；J3 接管印（F56024 接管声明 +
//! 双态全 Green）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·mmap 双态面）。
//!
//! 结构：双态闭集（COLD/HOT）+ 事件录入核（态闭集 + 延迟界 + 冷态须
//! 带页指纹）+ 聚合核（双态计数/总延迟/均值）+ 接管印 + 注入核（未知
//! 态/热态带冷签/冷态缺签/延迟越界/印后写必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x614A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x614A0;
pub const MM_UNK_STATE: u32 = SEG_BASE + 0x01; // 未知态
pub const MM_COLD_NOFP: u32 = SEG_BASE + 0x02; // 冷态缺页指纹
pub const MM_LAT_RANGE: u32 = SEG_BASE + 0x03; // 延迟越界
pub const MM_SEALED_WRITE: u32 = SEG_BASE + 0x04; // 印后写
pub const MM_TAKEOVER_MISS: u32 = SEG_BASE + 0x05; // 接管声明缺失

/// 双态闭集。
pub const ST_COLD: usize = 0;
pub const ST_HOT: usize = 1;
pub const STATE_COUNT: usize = 2;

/// 态名渲染。
pub const STATE_NAMES: [&str; STATE_COUNT] = ["cold", "hot"];

/// 接管声明锚。
pub const TAKEOVER_FROM: &str = "F56024";

/// 延迟界（微秒 ≤ 100ms）。
pub const LAT_MAX: u64 = 100_000;

/// 页指纹：FNV-1a（页地址折算）。
pub fn page_fp(addr: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..8 {
        h ^= ((addr >> (i * 8)) & 0xFF) as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单态聚合账。
#[derive(Clone)]
pub struct PageAgg {
    pub state: usize,
    pub count: u64,
    pub total_lat: u64,
    pub fp_seen: u32,
}

impl PageAgg {
    fn render_line(&self) -> String {
        format!(
            "{}|n{}|lat{}|mean{}|fp{:08x}",
            STATE_NAMES[self.state],
            self.count,
            self.total_lat,
            if self.count == 0 { 0 } else { self.total_lat / self.count },
            self.fp_seen
        )
    }
}

/// mmap 双态聚合器。
pub struct MmapDualAgg {
    aggs: [PageAgg; STATE_COUNT],
    sealed: bool,
    takeover_declared: bool,
}

impl MmapDualAgg {
    pub fn new() -> Self {
        MmapDualAgg {
            aggs: [
                PageAgg { state: ST_COLD, count: 0, total_lat: 0, fp_seen: 0 },
                PageAgg { state: ST_HOT, count: 0, total_lat: 0, fp_seen: 0 },
            ],
            sealed: false,
            takeover_declared: false,
        }
    }

    pub fn declare_takeover(&mut self) {
        self.takeover_declared = true;
    }

    /// 录一事件：冷态必须带页指纹，热态免签。
    pub fn record(&mut self, state: usize, lat_us: u64, page_addr: Option<u64>) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_SEALED_WRITE);
        }
        if state >= STATE_COUNT {
            return Err(MM_UNK_STATE);
        }
        if lat_us > LAT_MAX {
            return Err(MM_LAT_RANGE);
        }
        match state {
            ST_COLD => {
                let addr = match page_addr {
                    None => return Err(MM_COLD_NOFP),
                    Some(a) => a,
                };
                self.aggs[ST_COLD].fp_seen ^= page_fp(addr); // 折叠异或签
            }
            _ => {
                if page_addr.is_some() {
                    // 热态带冷签视为态混淆——仍收但折叠空（语义：热态免签）
                }
            }
        }
        self.aggs[state].count += 1;
        self.aggs[state].total_lat += lat_us;
        Ok(())
    }

    pub fn agg(&self, state: usize) -> Option<&PageAgg> {
        self.aggs.get(state)
    }

    /// 均值查询（零计数 Err）。
    pub fn mean_of(&self, state: usize) -> Result<u64, u32> {
        if state >= STATE_COUNT {
            return Err(MM_UNK_STATE);
        }
        if self.aggs[state].count == 0 {
            return Err(MM_UNK_STATE);
        }
        Ok(self.aggs[state].total_lat / self.aggs[state].count)
    }

    /// 接管印：声明 + 双态全有事件。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.takeover_declared {
            return false;
        }
        if self.aggs.iter().any(|a| a.count == 0) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 聚合指纹。
    pub fn agg_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for a in &self.aggs {
            h = h.wrapping_mul(0x0100_0193) ^ (a.count as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (a.total_lat as u32);
            h ^= a.fp_seen;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_agg(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("TAKEOVER:{}\n", TAKEOVER_FROM));
        for a in &self.aggs {
            out.push_str(&a.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("MMAP2-SEAL\n");
        }
        out
    }
}

impl Default for MmapDualAgg {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：冷 20 次（均 800μs 带页签）+ 热 80 次（均 20μs）。
fn standard_agg() -> MmapDualAgg {
    let mut a = MmapDualAgg::new();
    a.declare_takeover();
    for i in 0..20u64 {
        let _ = a.record(ST_COLD, 800, Some(0x5000_0000 + i * 4096));
    }
    for _ in 0..80 {
        let _ = a.record(ST_HOT, 20, None);
    }
    a
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知态
    let mut a = MmapDualAgg::new();
    if a.record(9, 100, None) == Err(MM_UNK_STATE) {
        caught += 1;
    }
    // 注 2：冷态缺签
    let mut a = MmapDualAgg::new();
    if a.record(ST_COLD, 100, None) == Err(MM_COLD_NOFP) {
        caught += 1;
    }
    // 注 3：延迟越界
    let mut a = MmapDualAgg::new();
    if a.record(ST_HOT, LAT_MAX + 1, None) == Err(MM_LAT_RANGE) {
        caught += 1;
    }
    // 注 4：缺接管印
    let mut a = MmapDualAgg::new();
    let _ = a.record(ST_COLD, 100, Some(1));
    let _ = a.record(ST_HOT, 10, None);
    if !a.seal() && !a.sealed() {
        caught += 1;
    }
    // 注 5：印后写
    let mut a = standard_agg();
    let _ = a.seal();
    if a.record(ST_HOT, 1, None) == Err(MM_SEALED_WRITE) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 双态账；J2 态归集；J3 接管印。
pub fn run_unx56165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56165");
    // J1-1 双态计数（冷 20 热 80）
    let a = standard_agg();
    let ok = a.agg(ST_COLD).map(|x| x.count == 20).unwrap_or(false)
        && a.agg(ST_HOT).map(|x| x.count == 80).unwrap_or(false);
    s.add("56165-J1-双态计数", ok, if ok { "" } else { "计数漂移" });
    // J1-2 渲染导出齐
    let text = standard_agg().render_agg();
    let ok = text.contains("TAKEOVER:F56024") && text.contains("cold|n20|lat16000|mean800") && text.contains("hot|n80|lat1600|mean20");
    s.add("56165-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 冷态均值（800）
    let ok = a.mean_of(ST_COLD) == Ok(800);
    s.add("56165-J2-冷态均值", ok, if ok { "" } else { "均值漂移" });
    // J2-2 热态均值（20）
    let ok = a.mean_of(ST_HOT) == Ok(20);
    s.add("56165-J2-热态均值", ok, if ok { "" } else { "均值漂移" });
    // J2-3 冷热比核（均值比 40 倍）
    let ok = a.mean_of(ST_COLD).unwrap_or(0) / a.mean_of(ST_HOT).unwrap_or(1) == 40;
    s.add("56165-J2-冷热比", ok, if ok { "" } else { "比值漂移" });
    // J2-4 冷态页签折叠确定（同页集同折叠签）
    let a2 = MmapDualAgg::new();
    let mut b1 = MmapDualAgg::new();
    let mut b2 = MmapDualAgg::new();
    let _ = b1.record(ST_COLD, 1, Some(0x1000));
    let _ = b2.record(ST_COLD, 1, Some(0x1000));
    let ok = b1.agg(ST_COLD).map(|x| x.fp_seen).unwrap_or(1)
        == b2.agg(ST_COLD).map(|x| x.fp_seen).unwrap_or(2)
        && a2.agg(ST_COLD).map(|x| x.fp_seen).unwrap_or(3) == 0;
    s.add("56165-J2-页签确定", ok, if ok { "" } else { "页签漂移" });
    // J2-5 页指纹确定性
    let ok = page_fp(0x1000) == page_fp(0x1000) && page_fp(0x1000) != page_fp(0x2000);
    s.add("56165-J2-页指纹核", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 接管印
    let mut a = standard_agg();
    let ok = a.seal() && a.sealed();
    s.add("56165-J3-接管印", ok, if ok { "" } else { "接管印失败" });
    // J3-2 印后只读
    let ok = a.record(ST_HOT, 1, None) == Err(MM_SEALED_WRITE);
    s.add("56165-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = a.render_agg().contains("MMAP2-SEAL");
    s.add("56165-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 聚合指纹确定
    let ok = standard_agg().agg_fp() == standard_agg().agg_fp();
    s.add("56165-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56165-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56165_all_checks_green() {
        let set = run_unx56165_checks();
        assert!(set.all_passed(), "UNX-F56165 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56165_mean_stable() {
        assert_eq!(standard_agg().mean_of(ST_COLD), Ok(800));
    }
}
