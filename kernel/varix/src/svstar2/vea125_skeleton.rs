//! VE-F0125 · 异构回归测试骨架（VE-A 域 · A05 多 GPU 与异构组收口 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0125`
//!
//! **职责定位（锚点原文）**：多卡配置的回归测试骨架（单卡/双卡/异构组合的测试矩阵），
//! 骨架的配置注入（拓扑参数化），测试结果的跨配置对比；含骨架的 CI 集成。
//!
//! **铁律**：
//! 1. **配置矩阵**——单卡/双卡/异构全覆盖。
//! 2. **参数注入**——拓扑参数化注入。
//! 3. **跨配置对比**——结果可对比。
//! 4. **分歧归因**——分歧定位到配置。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C3（回归骨架）。
pub const SEG_BASE: u16 = 0xE8C3;

/// 矩阵缺格。
pub const XC_MATRIX_GAP: u16 = SEG_BASE | 0x01;
/// 结果分歧。
pub const XC_DIVERGENCE: u16 = SEG_BASE | 0x02;
/// 骨架失效。
pub const XC_SKELETON_FAULT: u16 = SEG_BASE | 0x03;
/// 注入非法。
pub const XC_INJECT_BAD: u16 = SEG_BASE | 0x04;

/// 测试配置。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TestConfig {
    pub name: u32,
    /// 拓扑参数（卡数, 异构=1）。
    pub gpu_count: u16,
    pub heterogeneous: bool,
    /// 拓扑深度注入。
    pub depth: u32,
}

/// 测试结果。
#[derive(Clone, Debug)]
pub struct TestResult {
    pub config: TestConfig,
    pub passed: bool,
    /// 耗时（tick）。
    pub ticks: u64,
}

/// 回归骨架。
pub struct RegressionSkeleton {
    configs: Vec<TestConfig>,
    results: Vec<TestResult>,
    last_code: u16,
    /// 骨架自检通过。
    self_ok: bool,
    runs: u32,
}

impl RegressionSkeleton {
    pub fn new() -> Self {
        RegressionSkeleton {
            configs: Vec::new(),
            results: Vec::new(),
            last_code: 0,
            self_ok: true,
            runs: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 参数注入（拓扑参数化）。
    pub fn inject(&mut self, cfg: TestConfig) -> Result<(), u16> {
        if cfg.gpu_count == 0 {
            self.last_code = XC_INJECT_BAD;
            return Err(XC_INJECT_BAD);
        }
        self.configs.retain(|c| c.name != cfg.name);
        self.configs.push(cfg);
        self.last_code = 0;
        Ok(())
    }

    /// 矩阵构建（单卡/双卡/异构三类）。
    pub fn build_matrix(&mut self) -> Result<Vec<TestConfig>, u16> {
        if self.configs.is_empty() {
            self.last_code = XC_MATRIX_GAP;
            return Err(XC_MATRIX_GAP);
        }
        // 校验三类俱在。
        let has_single = self.configs.iter().any(|c| c.gpu_count == 1 && !c.heterogeneous);
        let has_dual = self.configs.iter().any(|c| c.gpu_count == 2);
        let has_hetero = self.configs.iter().any(|c| c.heterogeneous);
        if !(has_single && has_dual && has_hetero) {
            self.last_code = XC_MATRIX_GAP;
            return Err(XC_MATRIX_GAP);
        }
        self.last_code = 0;
        Ok(self.configs.clone())
    }

    /// 执行一轮（记录结果）。
    pub fn run(&mut self, name: u32, passed: bool, ticks: u64) -> Result<(), u16> {
        match self.configs.iter().find(|c| c.name == name) {
            Some(cfg) => {
                self.results.push(TestResult {
                    config: cfg.clone(),
                    passed,
                    ticks,
                });
                self.runs += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_SKELETON_FAULT;
                Err(XC_SKELETON_FAULT)
            }
        }
    }

    /// 跨配置对比：同名不同配置结果分歧检测。
    pub fn compare_cross(&mut self) -> Result<Vec<u32>, u16> {
        let mut diverged = Vec::new();
        for i in 0..self.results.len() {
            for j in (i + 1)..self.results.len() {
                if self.results[i].config.name == self.results[j].config.name
                    && self.results[i].passed != self.results[j].passed
                {
                    diverged.push(self.results[i].config.name);
                }
            }
        }
        if !diverged.is_empty() {
            self.last_code = XC_DIVERGENCE;
        }
        Ok(diverged)
    }

    /// 分歧归因：返回 (配置名, gpu_count, heterogeneous)。
    pub fn attribute(&self, name: u32) -> Option<(u32, u16, bool)> {
        self.results
            .iter()
            .find(|r| r.config.name == name)
            .map(|r| (r.config.name, r.config.gpu_count, r.config.heterogeneous))
    }

    /// 骨架自检。
    pub fn self_check(&mut self) -> bool {
        self.self_ok = self.configs.iter().all(|c| c.gpu_count > 0);
        self.self_ok
    }

    pub fn runs(&self) -> u32 {
        self.runs
    }

    pub fn config_count(&self) -> usize {
        self.configs.len()
    }
}

impl Default for RegressionSkeleton {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：配置矩阵、参数注入、跨配置对比、分歧归因。
pub fn run_vea125_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn cfg(name: u32, n: u16, het: bool) -> TestConfig {
        TestConfig {
            name,
            gpu_count: n,
            heterogeneous: het,
            depth: 1,
        }
    }

    fn full() -> RegressionSkeleton {
        let mut s = RegressionSkeleton::new();
        let _ = s.inject(cfg(1, 1, false));
        let _ = s.inject(cfg(2, 2, false));
        let _ = s.inject(cfg(3, 2, true));
        s
    }

    let mut set = CheckSet::new("vea125_skeleton");

    // C125-01 参数注入
    {
        let mut s = RegressionSkeleton::new();
        let r = s.inject(cfg(1, 2, true));
        chk(&mut set, "C125-01 注入", r.is_ok() && s.config_count() == 1, "注入失败");
    }

    // C125-02 配置矩阵三类
    {
        let mut s = full();
        let r = s.build_matrix();
        chk(&mut set, "C125-02 矩阵", r.is_ok() && r.unwrap().len() == 3, "矩阵缺格");
    }

    // C125-03 缺格拒绝
    {
        let mut s = RegressionSkeleton::new();
        let _ = s.inject(cfg(1, 1, false));
        let r = s.build_matrix();
        chk(
            &mut set,
            "C125-03 缺格拒",
            r.is_err() && s.last_code() == XC_MATRIX_GAP,
            "缺格被放",
        );
    }

    // C125-04 执行记录
    {
        let mut s = full();
        let r = s.run(1, true, 100);
        chk(&mut set, "C125-04 执行", r.is_ok() && s.runs() == 1, "执行失败");
    }

    // C125-05 跨配置对比
    {
        let mut s = full();
        let _ = s.run(1, true, 100);
        // 同名重复不同结果（模拟分歧）。
        s.results.push(TestResult {
            config: cfg(1, 2, true),
            passed: false,
            ticks: 200,
        });
        let r = s.compare_cross();
        chk(&mut set, "C125-05 对比", r.unwrap().contains(&1), "分歧漏检");
    }

    // C125-06 分歧归因
    {
        let mut s = full();
        let _ = s.run(2, true, 100);
        let a = s.attribute(2);
        chk(&mut set, "C125-06 归因", a == Some((2, 2, false)), "归因错");
    }

    // C125-07 非法注入拒绝
    {
        let mut s = RegressionSkeleton::new();
        let r = s.inject(cfg(1, 0, false));
        chk(
            &mut set,
            "C125-07 非法拒",
            r.is_err() && s.last_code() == XC_INJECT_BAD,
            "非法被放",
        );
    }

    // C125-08 未知配置执行拒绝
    {
        let mut s = full();
        let r = s.run(99, true, 1);
        chk(
            &mut set,
            "C125-08 未知拒",
            r.is_err() && s.last_code() == XC_SKELETON_FAULT,
            "未知被放",
        );
    }

    // C125-09 骨架自检
    {
        let mut s = full();
        chk(&mut set, "C125-09 自检", s.self_check(), "自检失败");
    }

    // C125-10 无分歧不误报
    {
        let mut s = full();
        let _ = s.run(1, true, 100);
        let r = s.compare_cross();
        chk(&mut set, "C125-10 不误报", r.unwrap().is_empty(), "误报分歧");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea125_all_green() {
        let s = run_vea125_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
