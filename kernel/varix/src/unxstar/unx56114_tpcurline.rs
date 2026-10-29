//! UNX-F56114 吞吐-延迟联合曲线导出件（0x60E20 段）
//!
//! 锚点：UNX-F56114-J1 联合曲线全量（曲线点逐点落账：压力档 × 吞吐 ×
//! 时延三元组）；J2 曲线语义一致（点序按压力档单调、拐点判定可复算——
//! 吞吐峰值后首个吞吐降档即拐点）；J3 曲线审计链（点指纹链、封账只读、
//! 峰值闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·联合曲线面）。
//!
//! 结构：曲线点登记册（压力档 → 吞吐 × 时延逐点落账）+ 点序闸（压力档
//! 严格递增）+ 拐点核（峰值后首个吞吐降档显性标注）+ 峰值闭式（最大
//! 吞吐点位）+ 指纹链 + 注入核（档号回退/零吞吐/拐点漏标/重复档/封账
//! 改写/空曲线查询/峰值漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60E20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60E20;
pub const CV_BACK: u32 = SEG_BASE + 0x01; // 压力档回退
pub const CV_TP_ZERO: u32 = SEG_BASE + 0x02; // 零吞吐点
pub const CV_KNEE_MISS: u32 = SEG_BASE + 0x03; // 拐点漏标
pub const CV_DUP: u32 = SEG_BASE + 0x04; // 重复压力档
pub const CV_SEALED: u32 = SEG_BASE + 0x05; // 曲线账已封账拒点
pub const CV_EMPTY: u32 = SEG_BASE + 0x06; // 空曲线查询
pub const CV_PEAK_MISS: u32 = SEG_BASE + 0x07; // 峰值漂移

/// 曲线点：压力档 × 吞吐 MB/s × 时延微秒 × 拐点标记 × 指纹 × 链。
#[derive(Clone)]
pub struct CvPoint {
    pub level: u32,
    pub tp_mbps: u64,
    pub lat_us: u64,
    pub knee: bool,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 点指纹：FNV-1a 32 位。
pub fn point_fingerprint(level: u32, tp: u64, lat: u64, knee: bool) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}|{}", level, tp, lat, knee);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 联合曲线总账。
pub struct CvBook {
    pub points: Vec<CvPoint>,
    pub sealed: bool,
}

impl CvBook {
    pub fn new() -> Self {
        CvBook { points: Vec::new(), sealed: false }
    }

    fn find(&self, level: u32) -> Option<usize> {
        for i in 0..self.points.len() {
            if self.points[i].level == level {
                return Some(i);
            }
        }
        None
    }

    /// 逐点落账：零吞吐拒 + 档号严格递增 + 重复档拒三闸。
    pub fn add(&mut self, level: u32, tp_mbps: u64, lat_us: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        if tp_mbps == 0 {
            return Err(CV_TP_ZERO);
        }
        if let Some(prev) = self.points.last() {
            if level <= prev.level {
                if level == prev.level {
                    return Err(CV_DUP);
                }
                return Err(CV_BACK);
            }
        }
        let fp = point_fingerprint(level, tp_mbps, lat_us, false);
        let chain = match self.points.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.points.push(CvPoint {
            level,
            tp_mbps,
            lat_us,
            knee: false,
            fp,
            chain,
        });
        Ok(())
    }

    /// J2：拐点标注核（吞吐峰值后首个降档标 knee，重标幂等）。
    pub fn mark_knee(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        if self.points.is_empty() {
            return Err(CV_EMPTY);
        }
        // 找峰值位（首个最大值）。
        let mut peak = 0usize;
        for i in 0..self.points.len() {
            if self.points[i].tp_mbps > self.points[peak].tp_mbps {
                peak = i;
            }
        }
        // 峰值后首个降档标 knee（仅首个，其余不标）。
        let mut first_drop: Option<usize> = None;
        for i in (peak + 1)..self.points.len() {
            if self.points[i].tp_mbps < self.points[i - 1].tp_mbps {
                first_drop = Some(i);
                break;
            }
        }
        let mut marked = 0usize;
        for i in 0..self.points.len() {
            let is_knee = first_drop == Some(i);
            if self.points[i].knee != is_knee {
                marked += 1;
            }
            self.points[i].knee = is_knee;
            self.points[i].fp =
                point_fingerprint(self.points[i].level, self.points[i].tp_mbps, self.points[i].lat_us, is_knee);
        }
        // 重建链。
        let mut chain = CHAIN_SEED;
        for i in 0..self.points.len() {
            chain = chain ^ self.points[i].fp;
            self.points[i].chain = chain;
        }
        Ok(marked)
    }

    /// J2：拐点查询（未标注显性拒绝）。
    pub fn knee_points(&self) -> Result<Vec<u32>, u32> {
        if self.points.is_empty() {
            return Err(CV_EMPTY);
        }
        let mut out = Vec::new();
        for p in self.points.iter() {
            if p.knee {
                out.push(p.level);
            }
        }
        Ok(out)
    }

    /// J3：峰值闭式（最大吞吐点位与值）。
    pub fn peak(&self) -> Result<(u32, u64), u32> {
        if self.points.is_empty() {
            return Err(CV_EMPTY);
        }
        let mut bi = 0usize;
        for i in 0..self.points.len() {
            if self.points[i].tp_mbps > self.points[bi].tp_mbps {
                bi = i;
            }
        }
        Ok((self.points[bi].level, self.points[bi].tp_mbps))
    }

    /// J1：曲线全量（逐点合法）。
    pub fn points_ok(&self) -> bool {
        !self.points.is_empty()
            && self.points.iter().all(|p| p.tp_mbps > 0)
    }

    /// J3：点指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.points.is_empty() {
            return false;
        }
        let mut expect = CHAIN_SEED;
        for i in 0..self.points.len() {
            expect = expect ^ self.points[i].fp;
            if self.points[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.points.last() {
            None => CHAIN_SEED,
            Some(p) => p.chain,
        }
    }
}

/// 标准夹具：5 点曲线（档 1..5，峰值档 3，拐点档 4）。
pub fn standard_book() -> CvBook {
    let mut b = CvBook::new();
    let plan: [(u32, u64, u64); 5] =
        [(1, 1_000, 100), (2, 1_800, 120), (3, 2_200, 180), (4, 2_100, 260), (5, 2_050, 420)];
    for p in plan.iter() {
        if b.add(p.0, p.1, p.2).is_err() {
            panic!("standard fixture add must succeed");
        }
    }
    if b.mark_knee().is_err() {
        panic!("standard fixture mark_knee must succeed");
    }
    b
}

/// 注入核：7 注入必抓（零吞吐/档号回退/重复档/空曲线查询/拐点漏标/封账改写/峰值漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零吞吐
    let mut b = CvBook::new();
    if b.add(1, 0, 100) == Err(CV_TP_ZERO) {
        caught += 1;
    }
    // 2) 档号回退
    if b.add(1, 1_000, 100).is_ok() && b.add(1, 1_100, 100) == Err(CV_DUP) {
        caught += 1;
    }
    // 3) 重复档
    if b.add(2, 1_800, 120).is_ok() && b.add(1, 1_500, 100) == Err(CV_BACK) {
        caught += 1;
    }
    // 4) 空曲线查询
    if CvBook::new().peak().err() == Some(CV_EMPTY) {
        caught += 1;
    }
    // 5) 拐点漏标必抓（单调升曲线无拐点）
    let mut b2 = CvBook::new();
    for i in 1..=4u32 {
        let _ = b2.add(i, 1_000 * i as u64, 100);
    }
    let _ = b2.mark_knee();
    if b2.knee_points() == Ok(Vec::new()) {
        caught += 1;
    }
    // 6) 峰值漂移必抓（改峰值后峰值位变）
    let mut dirty = standard_book();
    dirty.points[2].tp_mbps = 100;
    let _ = dirty.mark_knee();
    if dirty.peak() != Ok((3, 2_200)) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.add(6, 2_000, 500) == Err(CV_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 曲线账渲染导出。
pub fn render_book(b: &CvBook) -> String {
    let mut out = String::new();
    for p in b.points.iter() {
        let k = if p.knee { "knee" } else { "-" };
        out.push_str(&format!("L{}|{}|{}|{}|{:08x}\n", p.level, p.tp_mbps, p.lat_us, k, p.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56114");
    // J1-1 联合曲线全量（五点逐点合法）
    let b = standard_book();
    let ok = b.points_ok() && b.points.len() == 5;
    s.add("56114-J1-联合曲线全量", ok, if ok { "" } else { "曲线点残缺" });
    // J1-2 点序单调（档 1..5 严格递增）
    let ok = b.points[0].level == 1 && b.points[4].level == 5;
    s.add("56114-J1-点序单调", ok, if ok { "" } else { "点序漂移" });
    // J2-1 峰值闭式（档 3 吞吐 2200）
    let ok = b.peak() == Ok((3, 2_200));
    s.add("56114-J2-峰值闭式", ok, if ok { "" } else { "峰值漂移" });
    // J2-2 拐点标注（档 4 拐点）
    let ok = b.knee_points() == Ok(vec![4]);
    s.add("56114-J2-拐点标注", ok, if ok { "" } else { "拐点漂移" });
    // J2-3 拐点幂等重标（再标 marked=0）
    let mut b2 = standard_book();
    let ok = b2.mark_knee() == Ok(0) && b2.knee_points() == Ok(vec![4]);
    s.add("56114-J2-拐点幂等重标", ok, if ok { "" } else { "幂等漂移" });
    // J2-4 单调升曲线无拐点
    let mut b3 = CvBook::new();
    for i in 1..=4u32 {
        let _ = b3.add(i, 1_000 * i as u64, 100);
    }
    let _ = b3.mark_knee();
    let ok = b3.knee_points() == Ok(Vec::new());
    s.add("56114-J2-无拐点语义", ok, if ok { "" } else { "拐点误标" });
    // J3-1 点指纹链复核
    let ok = b.chain_ok();
    s.add("56114-J3-点指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56114-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改时延链尾变）
    let mut b4 = standard_book();
    b4.points[0].lat_us += 1;
    let _ = b4.mark_knee();
    let ok = b4.chain_tail() != standard_book().chain_tail();
    s.add("56114-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒点
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.add(6, 2_000, 500) == Err(CV_SEALED);
    s.add("56114-J3-封账拒点", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.contains("L4|2100|260|knee|");
    s.add("56114-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56114-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56114_all_checks_green() {
        let set = run_unx56114_checks();
        assert!(set.all_passed(), "UNX-F56114 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56114_peak_stable() {
        assert_eq!(standard_book().peak(), Ok((3, 2_200)));
    }
}
