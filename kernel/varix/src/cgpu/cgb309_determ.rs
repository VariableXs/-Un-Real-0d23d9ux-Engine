// CGPU-F0309 域级确定性终审 · cgb309_determ.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：同输入两次执行输出逐位一致（哈希对比）；浮点路径规避验证；
//       哈希表迭代序稳定；随机源受控；确定性违规可定位。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 确定性测试类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetermCategory {
    Layout,
    Render,
    Export,
    Serialization,
    HashIteration,
    SortOrder,
    RandomSource,
    TimeDependent,
}

/// 双跑对比样本。
#[derive(Clone, Debug)]
pub struct DualRunSample {
    pub id: u32,
    pub category: DetermCategory,
    pub run_a_hash: u64,
    pub run_b_hash: u64,
    pub input_fingerprint: u64,
    pub byte_len: u64,
    pub matched: bool,
}

/// 浮点规避审计。
#[derive(Clone, Copy, Default, Debug)]
pub struct FloatAudit {
    pub total_ops: u32,
    pub deterministic_ops: u32,
    pub risky_ops: u32,
    pub fixed_point_ops: u32,
}

/// 确定性终审引擎。
pub struct DeterminismAuditor {
    pub samples: Vec<DualRunSample>,
    pub float_audit: FloatAudit,
    pub violations: Vec<u32>,
    pub seed_controlled: bool,
    pub iter_order_verified: bool,
    pub total_bytes_compared: u64,
}

impl DeterminismAuditor {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            float_audit: FloatAudit::default(),
            violations: Vec::new(),
            seed_controlled: false,
            iter_order_verified: false,
            total_bytes_compared: 0,
        }
    }

    /// FNV-1a 64 位哈希。
    pub fn fnv64(data: &[u8]) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 登记双跑对比。
    pub fn record_dual(&mut self, id: u32, category: DetermCategory, data_a: &[u8], data_b: &[u8], input_fp: u64) {
        let ha = Self::fnv64(data_a);
        let hb = Self::fnv64(data_b);
        let matched = ha == hb && data_a.len() == data_b.len();
        self.total_bytes_compared += data_a.len() as u64;
        if !matched {
            self.violations.push(id);
        }
        self.samples.push(DualRunSample {
            id,
            category,
            run_a_hash: ha,
            run_b_hash: hb,
            input_fingerprint: input_fp,
            byte_len: data_a.len() as u64,
            matched,
        });
    }

    /// 浮点审计记账。
    pub fn audit_float(&mut self, total: u32, fixed_point: u32) {
        self.float_audit.total_ops = total;
        self.float_audit.fixed_point_ops = fixed_point;
        self.float_audit.deterministic_ops = fixed_point; // 定点=确定
        self.float_audit.risky_ops = total.saturating_sub(fixed_point);
    }

    pub fn set_seed_controlled(&mut self, v: bool) {
        self.seed_controlled = v;
    }

    pub fn set_iter_verified(&mut self, v: bool) {
        self.iter_order_verified = v;
    }

    /// 一致率（Q8）。
    pub fn match_rate_q8(&self) -> u16 {
        if self.samples.is_empty() {
            return 256;
        }
        let ok = self.samples.iter().filter(|s| s.matched).count() as u64;
        ((ok * 256) / self.samples.len() as u64) as u16
    }

    /// 浮点风险率（Q8）。
    pub fn float_risk_q8(&self) -> u16 {
        if self.float_audit.total_ops == 0 {
            return 0;
        }
        ((self.float_audit.risky_ops as u64 * 256) / self.float_audit.total_ops as u64) as u16
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0309 域级确定性终审");
        cs.add("双跑样本 id 唯一", {
            let mut ids: Vec<u32> = self.samples.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.samples.len()
        }, "unique");
        cs.add_dyn("一致率 256（100%）", self.match_rate_q8() == 256, &alloc::format!("rate={}", self.match_rate_q8()));
        cs.add("浮点风险可控", self.float_risk_q8() <= 64, "≤25%");
        cs.add("种子受控", self.seed_controlled, "seed");
        cs.add("迭代序已验证", self.iter_order_verified, "iter");
        cs.add("违规列表与样本一致", {
            self.violations.len() == self.samples.iter().filter(|s| !s.matched).count()
        }, "violations match");
        cs.add("比较字节量正或零样本", {
            self.samples.is_empty() || self.total_bytes_compared > 0
        }, "bytes");
        cs.add("定点运算占比 ≥ 75%", {
            self.float_audit.total_ops == 0
                || self.float_audit.fixed_point_ops * 4 >= self.float_audit.total_ops * 3
        }, "≥75% fixed-point");
        cs
    }
}

impl Default for DeterminismAuditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_run_match() {
        let mut d = DeterminismAuditor::new();
        d.record_dual(1, DetermCategory::Layout, b"hello", b"hello", 42);
        assert_eq!(d.match_rate_q8(), 256);
        assert!(d.violations.is_empty());
    }

    #[test]
    fn dual_run_mismatch() {
        let mut d = DeterminismAuditor::new();
        d.record_dual(1, DetermCategory::Export, b"hello", b"world", 42);
        assert_eq!(d.match_rate_q8(), 0);
        assert_eq!(d.violations.len(), 1);
    }

    #[test]
    fn float_risk() {
        let mut d = DeterminismAuditor::new();
        d.audit_float(100, 90);
        assert!(d.float_risk_q8() <= 64);
    }
}
