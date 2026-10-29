//! UNX-F56192 启动聚合审计快照件（0x61800 段）
//!
//! 锚点：UNX-F56192-J1 快照账（多样本聚合审计快照——快照号 × 样本
//! 数 × 四段均值 × 总均值逐字段落账）；J2 快照语义（样本数一致 + 均
//! 值口径守恒 + 快照只读三性质）；J3 快照指纹（快照折叠指纹）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（启动域·聚合审计面）。
//!
//! 结构：聚合快照结构 + 快照生成核（样本数闸 + 均值口径闸）+ 快照指
//! 纹 + 注入核（空聚合/样本数不符/均值漂移/快照后篡改源/重快照号必
//! 抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61800 独占 0x20）。
pub const SEG_BASE: u32 = 0x61800;
pub const AG_EMPTY_POOL: u32 = SEG_BASE + 0x01; // 空聚合
pub const AG_COUNT_MISS: u32 = SEG_BASE + 0x02; // 样本数不符
pub const AG_MEAN_DRIFT: u32 = SEG_BASE + 0x03; // 均值漂移
pub const AG_MUTATED: u32 = SEG_BASE + 0x04; // 快照后篡改源
pub const AG_DUP_SHOT: u32 = SEG_BASE + 0x05; // 重快照号

/// 聚合快照。
#[derive(Clone)]
pub struct AggSnapshot {
    pub shot_no: u32,
    pub sample_count: usize,
    /// 四段均值。
    pub seg_mean: [u64; 4],
    /// 总时长均值。
    pub total_mean: u64,
    /// 生成时源库指纹（防篡改锚）。
    pub source_fp: u32,
}

impl AggSnapshot {
    /// 均值口径守恒：四段均值和 = 总均值。
    pub fn mean_consistent(&self) -> bool {
        self.seg_mean.iter().sum::<u64>() == self.total_mean
    }

    fn render_line(&self) -> String {
        format!(
            "SHOT{}|n={}|m={},{},{},{}|tot={}",
            self.shot_no,
            self.sample_count,
            self.seg_mean[0],
            self.seg_mean[1],
            self.seg_mean[2],
            self.seg_mean[3],
            self.total_mean
        )
    }
}

/// 聚合样本池：四段时长样本。
pub struct AggPool {
    samples: Vec<[u64; 4]>,
    mutated: bool,
}

impl AggPool {
    pub fn new() -> Self {
        AggPool {
            samples: Vec::new(),
            mutated: false,
        }
    }

    /// 入池。
    pub fn push(&mut self, seg_ms: [u64; 4]) {
        if self.mutated {
            return;
        }
        self.samples.push(seg_ms);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 池指纹。
    pub fn pool_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for s in &self.samples {
            for m in s {
                h = h.wrapping_mul(0x0100_0193) ^ (*m as u32);
            }
        }
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 生成快照：空聚合拒；样本数与均值逐字段核。
    pub fn snapshot(&self, shot_no: u32, issued: &mut Vec<u32>) -> Result<AggSnapshot, u32> {
        if self.samples.is_empty() {
            return Err(AG_EMPTY_POOL);
        }
        if issued.contains(&shot_no) {
            return Err(AG_DUP_SHOT);
        }
        let n = self.samples.len();
        let mut seg_mean = [0u64; 4];
        for s in &self.samples {
            for i in 0..4 {
                seg_mean[i] += s[i];
            }
        }
        for m in seg_mean.iter_mut() {
            *m /= n as u64;
        }
        let total_mean: u64 = seg_mean.iter().sum();
        let shot = AggSnapshot {
            shot_no,
            sample_count: n,
            seg_mean,
            total_mean,
            source_fp: self.pool_fp(),
        };
        issued.push(shot_no);
        Ok(shot)
    }

    /// 快照复核：源池是否被篡改。
    pub fn verify_shot(&self, shot: &AggSnapshot) -> Result<(), u32> {
        if self.pool_fp() != shot.source_fp {
            return Err(AG_MUTATED);
        }
        if shot.sample_count != self.samples.len() {
            return Err(AG_COUNT_MISS);
        }
        let mut seg_mean = [0u64; 4];
        let n = self.samples.len() as u64;
        for s in &self.samples {
            for i in 0..4 {
                seg_mean[i] += s[i];
            }
        }
        for m in seg_mean.iter_mut() {
            *m /= n;
        }
        if seg_mean != shot.seg_mean {
            return Err(AG_MEAN_DRIFT);
        }
        Ok(())
    }

    /// 冻结池（此后不再入池）。
    pub fn freeze(&mut self) {
        self.mutated = true;
    }

    /// 渲染导出。
    pub fn render_pool(&self) -> String {
        let mut out = String::new();
        for (i, s) in self.samples.iter().enumerate() {
            out.push_str(&format!("P{}|{},{},{},{}\n", i, s[0], s[1], s[2], s[3]));
        }
        out
    }
}

impl Default for AggPool {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四样本池。
fn standard_pool() -> AggPool {
    let mut p = AggPool::new();
    p.push([800, 1800, 2600, 800]);
    p.push([790, 1790, 2590, 790]);
    p.push([810, 1810, 2610, 810]);
    p.push([800, 1800, 2600, 800]);
    p
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空聚合
    let p = AggPool::new();
    let mut issued = Vec::new();
    if p.snapshot(1, &mut issued).err() == Some(AG_EMPTY_POOL) {
        caught += 1;
    }
    // 注 2：重快照号
    let p = standard_pool();
    let mut issued = Vec::new();
    let _ = p.snapshot(1, &mut issued);
    if p.snapshot(1, &mut issued).err() == Some(AG_DUP_SHOT) {
        caught += 1;
    }
    // 注 3：快照后篡改源
    let mut p = standard_pool();
    let mut issued = Vec::new();
    let shot = p.snapshot(1, &mut issued).ok().unwrap();
    p.samples[0][0] += 7; // 篡改
    if p.verify_shot(&shot).err() == Some(AG_MUTATED) {
        caught += 1;
    }
    // 注 4：样本数不符（用干净池，避免篡改干扰判定路径）
    let p_clean = standard_pool();
    let mut issued_c = Vec::new();
    let shot_ok = p_clean.snapshot(1, &mut issued_c).ok().unwrap();
    let shot_bad = AggSnapshot {
        sample_count: shot_ok.sample_count + 1,
        ..shot_ok.clone()
    };
    if p_clean.verify_shot(&shot_bad).err() == Some(AG_COUNT_MISS) {
        caught += 1;
    }
    // 注 5：均值漂移
    let shot_bad2 = AggSnapshot {
        seg_mean: [999, 1800, 2600, 800],
        ..shot_ok
    };
    if p_clean.verify_shot(&shot_bad2).err() == Some(AG_MEAN_DRIFT) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 快照账；J2 快照语义；J3 快照指纹。
pub fn run_unx56192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56192");
    // J1-1 四样本池全入
    let p = standard_pool();
    let ok = p.len() == 4 && !p.is_empty();
    s.add("56192-J1-四样本池全入", ok, if ok { "" } else { "池账不全" });
    // J1-2 快照生成与渲染齐
    let mut issued = Vec::new();
    let shot = p.snapshot(1, &mut issued).ok().unwrap();
    let text = shot.render_line();
    let ok = text.contains("SHOT1|n=4|") && text.contains("tot=6000");
    s.add("56192-J1-快照渲染齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 样本数一致
    let ok = shot.sample_count == 4;
    s.add("56192-J2-样本数一致", ok, if ok { "" } else { "样本数漂移" });
    // J2-2 四段均值核（800/1800/2600/800）
    let ok = shot.seg_mean == [800, 1800, 2600, 800];
    s.add("56192-J2-四段均值", ok, if ok { "" } else { "均值漂移" });
    // J2-3 均值口径守恒
    let ok = shot.mean_consistent();
    s.add("56192-J2-口径守恒", ok, if ok { "" } else { "口径漂移" });
    // J2-4 快照复核通过
    let ok = p.verify_shot(&shot).is_ok();
    s.add("56192-J2-快照复核过", ok, if ok { "" } else { "复核漂移" });
    // J2-5 池冻结后不再入池
    let mut p2 = standard_pool();
    p2.freeze();
    let n0 = p2.len();
    p2.push([1, 1, 1, 1]);
    let ok = p2.len() == n0;
    s.add("56192-J2-池冻结", ok, if ok { "" } else { "冻结失效" });
    // J2-6 空聚合拒
    let empty = AggPool::new();
    let mut issued2 = Vec::new();
    let ok = empty.snapshot(9, &mut issued2).err() == Some(AG_EMPTY_POOL);
    s.add("56192-J2-空聚合拒", ok, if ok { "" } else { "空聚合闸失效" });
    // J3-1 池指纹确定
    let ok = standard_pool().pool_fp() == standard_pool().pool_fp();
    s.add("56192-J3-池指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 池变签变
    let mut p3 = standard_pool();
    let fp0 = p3.pool_fp();
    p3.push([1, 2, 3, 4]);
    let ok = p3.pool_fp() != fp0;
    s.add("56192-J3-池变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 快照锚核（篡改必抓由注入核覆盖，此处验干净路径）
    let mut p4 = standard_pool();
    let mut issued3 = Vec::new();
    let shot4 = p4.snapshot(2, &mut issued3).ok().unwrap();
    let ok = p4.verify_shot(&shot4).is_ok() && shot4.source_fp == p4.pool_fp();
    s.add("56192-J3-快照锚核", ok, if ok { "" } else { "锚核漂移" });
    // J3-4 渲染池齐
    let text = p.render_pool();
    let ok = text.contains("P0|800,1800,2600,800") && text.contains("P3|800,1800,2600,800");
    s.add("56192-J3-渲染池齐", ok, if ok { "" } else { "池渲染漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56192-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56192_all_checks_green() {
        let set = run_unx56192_checks();
        assert!(set.all_passed(), "UNX-F56192 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56192_pool_stable() {
        let p = standard_pool();
        let mut issued = Vec::new();
        let shot = p.snapshot(1, &mut issued).unwrap();
        assert!(p.verify_shot(&shot).is_ok());
    }
}
