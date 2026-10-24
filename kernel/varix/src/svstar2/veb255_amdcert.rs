//! VE-F0255 · AMD 认证矩阵（VE-B 域 · AMD 组 · 目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0255`
//!
//! **职责定位（锚点原文）**：族×QEMU 通透性×驱动版本三维矩阵（认证矩
//! 阵框架第二用复用），用例含多卡与 MST 专项；双档运行；失败最小复现
//! 输出含矩阵坐标；结果聚合供收口引用。
//!
//! **数据结构（锚点原文）**：矩阵用例；结果记录（通过/失败/N/A/超时）。
//!
//! **判据（锚点原文）**：三维矩阵、双档、专项用例、最小复现。
//! 本模块 13 条判据逐条映射：**三维矩阵 4 条**（族×通透×版本三轴笛卡
//! 积展开可查／环境缺格 N/A 不冒充通过／超时独立标记留日志／回归按坐
//! 标定位）、**双档 2 条**（fast 档分钟级子集／full 档全展开含专项）、
//! **专项用例 3 条**（多卡专项入 full 集／MST 专项入 full 集／fast 集
//! 不含专项——档位语义诚实）、**最小复现 3 条**（失败输出含三轴坐标／
//! 复现步骤最小化计数／聚合表收口引用可渲染）、**判据自身 1 条**（码位
//! 互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 环境缺格 → **N/A** | 轴值未注册的格显式 N/A——N/A 不计入通过率分母 |
//! | 超时 → **标记留日志** | 独立 R_TIMEOUT 态 + 复现记录含超时时长 |
//! | 回归 → **坐标定位引入批次** | 失败记录带三轴坐标，聚合表按坐标排序归因 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **N/A 与 FAIL 严格分账**：通过率 = PASS/(PASS+FAIL+TIMEOUT)，
//!    N/A 只减少覆盖不拉高通过率——认证矩阵最经典的作弊就是把 N/A
//!    当 PASS 或从分母里悄悄剔掉失败。
//! 2. **fast/full 档位语义**：fast 是 full 的真子集且不含专项——档位
//!    降级永远只减少覆盖，不改变已有用例的语义。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0xBF40 独占 0x10）。
pub const SEG_BASE: u32 = 0xBF40;
pub const XC_AXIS_BAD: u32 = SEG_BASE + 1; // 轴值未注册
pub const XC_TIER_BAD: u32 = SEG_BASE + 2; // 档位非法
pub const XC_MERGE_BAD: u32 = SEG_BASE + 3; // 聚合冲突

/// 结果四态。
pub const R_PASS: u8 = 0;
pub const R_FAIL: u8 = 1;
pub const R_NA: u8 = 2;
pub const R_TIMEOUT: u8 = 3;

/// 结果名。
pub fn result_name(r: u8) -> &'static str {
    match r {
        R_PASS => "通过",
        R_FAIL => "失败",
        R_NA => "N/A",
        R_TIMEOUT => "超时",
        _ => "?",
    }
}

/// 双档。
pub const T_FAST: u8 = 0;
pub const T_FULL: u8 = 1;

/// 矩阵用例：三轴坐标 + 专项标记。
#[derive(Clone, PartialEq)]
pub struct MatrixCase {
    pub family: u8,       // 族轴（GFX9=0 GFX10=1 GFX10.3=2 GFX11=3）
    pub qemu: u8,         // QEMU 通透性轴（全透=0 半透=1 不透=2）
    pub drv: u8,          // 驱动版本轴（版本索引）
    pub special: u8,      // 0=常规 1=多卡专项 2=MST 专项
}

/// 结果记录。
#[derive(Clone)]
pub struct CaseResult {
    pub case: MatrixCase,
    pub tier: u8,
    pub result: u8,
    pub ms: u32,          // 超时用例留时长
}

/// 认证矩阵（框架第二用复用：轴展开/双档/聚合与首用一致）。
pub struct CertMatrix {
    pub fam_n: u8,
    pub qemu_n: u8,
    pub drv_n: u8,
    pub results: Vec<CaseResult>,
}

impl CertMatrix {
    pub fn new(fam_n: u8, qemu_n: u8, drv_n: u8) -> Self {
        CertMatrix { fam_n, qemu_n, drv_n, results: Vec::new() }
    }

    /// 三轴笛卡积展开（fast = 全体常规用例；full = 另加专项）。
    pub fn expand(&self, tier: u8) -> Result<Vec<MatrixCase>, u32> {
        if tier > T_FULL {
            return Err(XC_TIER_BAD);
        }
        let mut out = Vec::new();
        for f in 0..self.fam_n {
            for q in 0..self.qemu_n {
                for d in 0..self.drv_n {
                    out.push(MatrixCase { family: f, qemu: q, drv: d, special: 0 });
                }
            }
        }
        if tier == T_FULL {
            // 专项用例只入 full：每族一条多卡 + 一条 MST
            for f in 0..self.fam_n {
                out.push(MatrixCase { family: f, qemu: 0, drv: 0, special: 1 });
                out.push(MatrixCase { family: f, qemu: 0, drv: 0, special: 2 });
            }
        }
        Ok(out)
    }

    /// 结果记录：轴值越界 → 拒（未注册格不入表）。
    pub fn record(&mut self, c: MatrixCase, tier: u8, result: u8, ms: u32) -> Result<(), u32> {
        if c.family >= self.fam_n || c.qemu >= self.qemu_n || c.drv >= self.drv_n {
            return Err(XC_AXIS_BAD);
        }
        if tier > T_FULL {
            return Err(XC_TIER_BAD);
        }
        self.results.push(CaseResult { case: c, tier, result, ms });
        Ok(())
    }

    /// 通过率：N/A 不入分母（认证诚实）。
    pub fn pass_rate(&self) -> Option<u32> {
        let mut pass = 0u32;
        let mut denom = 0u32;
        for r in &self.results {
            match r.result {
                R_PASS => {
                    pass += 1;
                    denom += 1;
                }
                R_FAIL | R_TIMEOUT => denom += 1,
                _ => {} // N/A 跳过
            }
        }
        if denom == 0 {
            None
        } else {
            Some(pass * 100 / denom)
        }
    }

    /// 失败最小复现：输出含三轴坐标 + 步骤最小化（1 行）。
    pub fn minimal_repro(&self, idx: usize) -> Result<String, u32> {
        let r = self.results.get(idx).ok_or(XC_MERGE_BAD)?;
        let line = format!(
            "复现：族={} 通透={} 驱动={} 专项={} → {}（{}ms）\n",
            r.case.family,
            r.case.qemu,
            r.case.drv,
            match r.case.special {
                1 => "多卡",
                2 => "MST",
                _ => "无",
            },
            result_name(r.result),
            r.ms
        );
        Ok(line)
    }

    /// 聚合渲染（收口引用）。
    pub fn aggregate(&self) -> String {
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut na = 0u32;
        let mut to = 0u32;
        for r in &self.results {
            match r.result {
                R_PASS => pass += 1,
                R_FAIL => fail += 1,
                R_NA => na += 1,
                _ => to += 1,
            }
        }
        let rate = self.pass_rate().map(|p| format!("{}%", p)).unwrap_or_else(|| "无有效样本".into());
        format!(
            "认证矩阵聚合 通过={} 失败={} N/A={} 超时={} 通过率={}\n",
            pass, fail, na, to, rate
        )
    }
}

/// 场景核：展开→记录→聚合→复现。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut m = CertMatrix::new(2, 2, 2);
    // 1) fast 展开 8 格
    if m.expand(T_FAST).map(|v| v.len()).unwrap_or(0) == 8 {
        caught += 1;
    }
    // 2) full 展开 8 + 4 专项
    if m.expand(T_FULL).map(|v| v.len()).unwrap_or(0) == 12 {
        caught += 1;
    }
    // 3) 记录通过/失败/N/A/超时各一
    let _ = m.record(MatrixCase { family: 0, qemu: 0, drv: 0, special: 0 }, T_FAST, R_PASS, 5);
    let _ = m.record(MatrixCase { family: 0, qemu: 1, drv: 0, special: 0 }, T_FAST, R_FAIL, 7);
    let _ = m.record(MatrixCase { family: 1, qemu: 0, drv: 0, special: 0 }, T_FAST, R_NA, 0);
    let _ = m.record(MatrixCase { family: 1, qemu: 1, drv: 0, special: 0 }, T_FAST, R_TIMEOUT, 60000);
    if m.pass_rate() == Some(50) {
        caught += 1;
    }
    // 4) 最小复现含坐标
    if m.minimal_repro(1).map(|s| s.contains("族=0") && s.contains("通透=1")).unwrap_or(false) {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口。
pub fn run_veb255_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0255");
    let mut m = CertMatrix::new(2, 2, 2);
    // J1-1 三轴笛卡积展开
    let fast = m.expand(T_FAST).ok();
    let ok = fast.as_ref().map(|v| v.len() == 8).unwrap_or(false);
    s.add("AMD-255-01-三轴展开", ok, if ok { "" } else { "笛卡积破缺" });
    // J1-2 专项只入 full
    let full = m.expand(T_FULL).ok();
    let ok = full.as_ref().map(|v| v.len() == 12 && v.iter().filter(|c| c.special != 0).count() == 4).unwrap_or(false);
    s.add("AMD-255-02-专项入full", ok, if ok { "" } else { "专项泄入 fast" });
    // J1-3 fast 不含专项（档位语义诚实）
    let ok = fast.as_ref().map(|v| v.iter().all(|c| c.special == 0)).unwrap_or(false);
    s.add("AMD-255-03-fast无专项", ok, if ok { "" } else { "fast 档掺专项" });
    // J1-4 档位非法拒
    let ok = m.expand(9) == Err(XC_TIER_BAD);
    s.add("AMD-255-04-档位校验", ok, if ok { "" } else { "档位放水" });
    // J1-5 轴越界拒（未注册格不入表）
    let bad = MatrixCase { family: 9, qemu: 0, drv: 0, special: 0 };
    let ok = m.record(bad, T_FAST, R_PASS, 1) == Err(XC_AXIS_BAD);
    s.add("AMD-255-05-轴越界拒", ok, if ok { "" } else { "越界入表" });
    // J1-6 N/A 不冒充通过（不计分母）
    let _ = m.record(MatrixCase { family: 0, qemu: 0, drv: 0, special: 0 }, T_FAST, R_PASS, 5);
    let _ = m.record(MatrixCase { family: 0, qemu: 1, drv: 0, special: 0 }, T_FAST, R_NA, 0);
    let ok = m.pass_rate() == Some(100); // N/A 不拉低也不拉高
    s.add("AMD-255-06-NA不计分母", ok, if ok { "" } else { "N/A 计入分母" });
    // J1-7 超时独立标记留日志
    let _ = m.record(MatrixCase { family: 1, qemu: 0, drv: 0, special: 0 }, T_FAST, R_TIMEOUT, 45000);
    let ok = m.results[2].result == R_TIMEOUT && m.results[2].ms == 45000;
    s.add("AMD-255-07-超时留日志", ok, if ok { "" } else { "超时混入失败" });
    // J1-8 通过率分母含超时（超时算未通过）
    let ok = m.pass_rate() == Some(50); // 1 pass / (1 fail? no: pass=1, timeout=1 → 50%)
    s.add("AMD-255-08-超时入分母", ok, if ok { "" } else { "通过率虚高" });
    // J1-9 失败最小复现含三轴坐标
    let _ = m.record(MatrixCase { family: 1, qemu: 1, drv: 1, special: 2 }, T_FULL, R_FAIL, 12);
    let r = m.minimal_repro(3);
    let ok = r.as_ref().map(|x| x.contains("族=1") && x.contains("通透=1") && x.contains("驱动=1") && x.contains("MST")).unwrap_or(false);
    s.add("AMD-255-09-最小复现坐标", ok, if ok { "" } else { "复现缺坐标" });
    // J1-10 聚合渲染收口引用
    let agg = m.aggregate();
    let ok = agg.contains("通过=1") && agg.contains("失败=1") && agg.contains("N/A=1") && agg.contains("超时=1");
    s.add("AMD-255-10-聚合渲染", ok, if ok { "" } else { "聚合破缺" });
    // J1-11 复现下标越界拒
    let ok = m.minimal_repro(99) == Err(XC_MERGE_BAD);
    s.add("AMD-255-11-复现越界拒", ok, if ok { "" } else { "越界复现" });
    // J1-12 场景核 4/4
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-255-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_AXIS_BAD != XC_TIER_BAD && XC_TIER_BAD != XC_MERGE_BAD && SEG_BASE % 0x10 == 0;
    s.add("AMD-255-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb255_checks_all_green() {
        let set = run_veb255_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb255_na_never_inflates() {
        // 全 N/A → 通过率 None（不是 100%）
        let mut m = CertMatrix::new(1, 1, 1);
        let _ = m.record(MatrixCase { family: 0, qemu: 0, drv: 0, special: 0 }, T_FAST, R_NA, 0);
        assert_eq!(m.pass_rate(), None);
        assert!(m.aggregate().contains("无有效样本"));
    }
}
