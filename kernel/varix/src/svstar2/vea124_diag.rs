//! VE-F0124 · 多适配器诊断报表（VE-A 域 · A05 多 GPU 与异构组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0124`
//!
//! **职责定位（锚点原文）**：多卡系统的诊断报表（每卡健康/负载/错误三层），报表
//! 对比视图（卡间差异一目了然），报表开放导出；含报表与 X08 报告模板的复用声明。
//!
//! **铁律**：
//! 1. **三层报表**——健康/负载/错误三层。
//! 2. **卡间对比**——差异一目了然。
//! 3. **开放导出**——可导出。
//! 4. **缺失标注**——数据缺失显式标注。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C2（诊断报表）。
pub const SEG_BASE: u16 = 0xE8C2;

/// 数据缺失。
pub const XC_DATA_MISSING: u16 = SEG_BASE | 0x01;
/// 对比失真。
pub const XC_COMPARE_SKEW: u16 = SEG_BASE | 0x02;
/// 导出失败。
pub const XC_EXPORT_FAIL: u16 = SEG_BASE | 0x03;

/// 健康层。
#[derive(Clone, Copy, Debug)]
pub struct HealthLayer {
    pub gpu_id: u16,
    /// 健康分（0-100）。
    pub score: u8,
    /// 是否失联。
    pub lost: bool,
}

/// 负载层。
#[derive(Clone, Copy, Debug)]
pub struct LoadLayer {
    pub gpu_id: u16,
    /// 负载千分比。
    pub permille: u32,
}

/// 错误层。
#[derive(Clone, Copy, Debug)]
pub struct ErrorLayer {
    pub gpu_id: u16,
    /// 错误计数。
    pub count: u32,
}

/// 三层报表。
#[derive(Clone, Debug)]
pub struct DiagReport {
    pub health: Vec<HealthLayer>,
    pub load: Vec<LoadLayer>,
    pub errors: Vec<ErrorLayer>,
    /// 缺失标注。
    pub missing: Vec<u16>,
}

impl DiagReport {
    /// 卡间对比：返回 (最优卡, 最差卡)。
    pub fn compare(&self) -> Option<(u16, u16)> {
        if self.health.is_empty() {
            return None;
        }
        let best = self
            .health
            .iter()
            .max_by_key(|h| h.score)
            .map(|h| h.gpu_id)?;
        let worst = self
            .health
            .iter()
            .min_by_key(|h| h.score)
            .map(|h| h.gpu_id)?;
        Some((best, worst))
    }
}

/// 诊断报表生成器。
pub struct DiagReporter {
    health: Vec<HealthLayer>,
    load: Vec<LoadLayer>,
    errors: Vec<ErrorLayer>,
    last_code: u16,
    exports: u32,
}

impl DiagReporter {
    pub fn new() -> Self {
        DiagReporter {
            health: Vec::new(),
            load: Vec::new(),
            errors: Vec::new(),
            last_code: 0,
            exports: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn push_health(&mut self, h: HealthLayer) {
        self.health.retain(|x| x.gpu_id != h.gpu_id);
        self.health.push(h);
    }

    pub fn push_load(&mut self, l: LoadLayer) {
        self.load.retain(|x| x.gpu_id != l.gpu_id);
        self.load.push(l);
    }

    pub fn push_error(&mut self, e: ErrorLayer) {
        self.errors.retain(|x| x.gpu_id != e.gpu_id);
        self.errors.push(e);
    }

    /// 生成报表（缺失标注）。
    pub fn generate(&mut self, expected_gpus: &[u16]) -> Result<DiagReport, u16> {
        let mut missing = Vec::new();
        for &g in expected_gpus {
            if !self.health.iter().any(|h| h.gpu_id == g) {
                missing.push(g);
            }
        }
        let rep = DiagReport {
            health: self.health.clone(),
            load: self.load.clone(),
            errors: self.errors.clone(),
            missing: missing.clone(),
        };
        if !missing.is_empty() {
            self.last_code = XC_DATA_MISSING;
        } else {
            self.last_code = 0;
        }
        Ok(rep)
    }

    /// 对比校准（失真检测）。
    pub fn verify_compare(&mut self, rep: &DiagReport) -> Result<(), u16> {
        for h in &rep.health {
            if h.score > 100 {
                self.last_code = XC_COMPARE_SKEW;
                return Err(XC_COMPARE_SKEW);
            }
        }
        self.last_code = 0;
        Ok(())
    }

    /// 导出（开放导出，计数）。
    pub fn export(&mut self, _rep: &DiagReport) -> Result<u32, u16> {
        self.exports += 1;
        self.last_code = 0;
        Ok(self.exports)
    }

    pub fn exports(&self) -> u32 {
        self.exports
    }
}

impl Default for DiagReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：三层报表、卡间对比、开放导出、缺失标注。
pub fn run_vea124_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea124_diag");

    // C124-01 三层报表
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        d.push_load(LoadLayer { gpu_id: 0, permille: 500 });
        d.push_error(ErrorLayer { gpu_id: 0, count: 1 });
        let r = d.generate(&[0]);
        let ok = r.is_ok() && r.unwrap().health.len() == 1;
        chk(&mut set, "C124-01 三层", ok, "三层缺");
    }

    // C124-02 卡间对比
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        d.push_health(HealthLayer { gpu_id: 1, score: 50, lost: false });
        let r = d.generate(&[0, 1]).unwrap();
        let c = r.compare();
        chk(&mut set, "C124-02 对比", c == Some((0, 1)), "对比错");
    }

    // C124-03 缺失标注
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        let r = d.generate(&[0, 1]).unwrap();
        chk(&mut set, "C124-03 缺失标注", r.missing.contains(&1), "缺失漏标");
    }

    // C124-04 开放导出
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        let r = d.generate(&[0]).unwrap();
        let e = d.export(&r);
        chk(&mut set, "C124-04 导出", e == Ok(1) && d.exports() == 1, "导出失败");
    }

    // C124-05 对比失真检出
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        let mut r = d.generate(&[0]).unwrap();
        r.health[0].score = 150; // 越界
        let v = d.verify_compare(&r);
        chk(
            &mut set,
            "C124-05 失真检出",
            v.is_err() && d.last_code() == XC_COMPARE_SKEW,
            "失真漏检",
        );
    }

    // C124-06 失联标注
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 0, lost: true });
        let r = d.generate(&[0]).unwrap();
        chk(&mut set, "C124-06 失联", r.health[0].lost, "失联漏标");
    }

    // C124-07 覆盖更新
    {
        let mut d = DiagReporter::new();
        d.push_load(LoadLayer { gpu_id: 0, permille: 100 });
        d.push_load(LoadLayer { gpu_id: 0, permille: 200 });
        let r = d.generate(&[0]).unwrap();
        chk(&mut set, "C124-07 覆盖", r.load.len() == 1 && r.load[0].permille == 200, "未覆盖");
    }

    // C124-08 校验通过
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        let r = d.generate(&[0]).unwrap();
        let v = d.verify_compare(&r);
        chk(&mut set, "C124-08 校验过", v.is_ok(), "校验误拒");
    }

    // C124-09 空对比
    {
        let d = DiagReporter::new();
        let r = DiagReport {
            health: vec![],
            load: vec![],
            errors: vec![],
            missing: vec![],
        };
        chk(&mut set, "C124-09 空对比", r.compare().is_none(), "空误判");
    }

    // C124-10 导出计数
    {
        let mut d = DiagReporter::new();
        d.push_health(HealthLayer { gpu_id: 0, score: 90, lost: false });
        let r = d.generate(&[0]).unwrap();
        let _ = d.export(&r);
        let _ = d.export(&r);
        chk(&mut set, "C124-10 导出计数", d.exports() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea124_all_green() {
        let s = run_vea124_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
