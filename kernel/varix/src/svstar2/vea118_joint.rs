//! VE-F0118 · 异构预算池联合仲裁（VE-A 域 · A05 多 GPU 与异构组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0118`
//!
//! **职责定位（锚点原文）**：多卡预算池的联合仲裁（每卡独立预算+跨卡共享预算两级），
//! 跨卡申请的仲裁规则，预算加总复核。
//!
//! **铁律**：
//! 1. **两级预算**——每卡独立 + 跨卡共享。
//! 2. **联合仲裁**——跨卡申请按规则裁决。
//! 3. **加重复核**——两级加总不超域级。
//! 4. **超支降级**——超支触发降级不硬拒。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BC（联合仲裁）。
pub const SEG_BASE: u16 = 0xE8BC;

/// 加总超域级。
pub const XC_TOTAL_OVER: u16 = SEG_BASE | 0x01;
/// 两级冲突。
pub const XC_LEVEL_CONFLICT: u16 = SEG_BASE | 0x02;
/// 超支降级。
pub const XC_OVERDRAW: u16 = SEG_BASE | 0x03;
/// 未知池。
pub const XC_NO_POOL: u16 = SEG_BASE | 0x04;

/// 申请单元。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRequest {
    pub gpu_id: u16,
    pub bytes: u64,
    /// true=走共享池。
    pub use_shared: bool,
}

/// 独立池。
#[derive(Clone, Debug)]
pub struct SoloPool {
    pub gpu_id: u16,
    pub limit: u64,
    pub used: u64,
}

/// 共享池。
#[derive(Clone, Debug)]
pub struct SharedPool {
    pub limit: u64,
    pub used: u64,
}

/// 联合仲裁器。
pub struct JointArbiter {
    solos: Vec<SoloPool>,
    shared: SharedPool,
    /// 域级加总上限。
    domain_limit: u64,
    last_code: u16,
    /// 降级次数。
    degradations: u32,
    grants: u32,
}

impl JointArbiter {
    pub fn new(domain_limit: u64, shared_limit: u64) -> Result<Self, u16> {
        if domain_limit == 0 || shared_limit > domain_limit {
            return Err(XC_TOTAL_OVER);
        }
        Ok(JointArbiter {
            solos: Vec::new(),
            shared: SharedPool {
                limit: shared_limit,
                used: 0,
            },
            domain_limit,
            last_code: 0,
            degradations: 0,
            grants: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn add_solo(&mut self, gpu_id: u16, limit: u64) -> Result<(), u16> {
        let total: u64 = self.solos.iter().map(|s| s.limit).sum::<u64>() + limit;
        if total + self.shared.limit > self.domain_limit {
            self.last_code = XC_TOTAL_OVER;
            return Err(XC_TOTAL_OVER);
        }
        self.solos.retain(|s| s.gpu_id != gpu_id);
        self.solos.push(SoloPool {
            gpu_id,
            limit,
            used: 0,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 加总复核。
    pub fn verify_total(&self) -> bool {
        let solo_sum: u64 = self.solos.iter().map(|s| s.limit).sum();
        solo_sum + self.shared.limit <= self.domain_limit
    }

    /// 申请裁决。
    pub fn request(&mut self, req: BudgetRequest) -> Result<bool, u16> {
        if req.use_shared {
            // 共享池。
            if self.shared.used + req.bytes <= self.shared.limit {
                self.shared.used += req.bytes;
                self.grants += 1;
                self.last_code = 0;
                Ok(true)
            } else {
                // 超支降级：给部分。
                let avail = self.shared.limit - self.shared.used;
                if avail > 0 {
                    self.shared.used = self.shared.limit;
                    self.degradations += 1;
                    self.last_code = XC_OVERDRAW;
                    Ok(false) // 降级
                } else {
                    self.last_code = XC_OVERDRAW;
                    Err(XC_OVERDRAW)
                }
            }
        } else {
            // 独立池。
            match self.solos.iter_mut().find(|s| s.gpu_id == req.gpu_id) {
                Some(s) => {
                    if s.used + req.bytes <= s.limit {
                        s.used += req.bytes;
                        self.grants += 1;
                        self.last_code = 0;
                        Ok(true)
                    } else {
                        let avail = s.limit - s.used;
                        if avail > 0 {
                            s.used = s.limit;
                            self.degradations += 1;
                            self.last_code = XC_OVERDRAW;
                            Ok(false)
                        } else {
                            self.last_code = XC_OVERDRAW;
                            Err(XC_OVERDRAW)
                        }
                    }
                }
                None => {
                    self.last_code = XC_NO_POOL;
                    Err(XC_NO_POOL)
                }
            }
        }
    }

    /// 两级冲突检测（共享与独立都申请同一资源）。
    pub fn detect_conflict(&mut self, gpu_id: u16) -> bool {
        let solo_used = self
            .solos
            .iter()
            .find(|s| s.gpu_id == gpu_id)
            .map(|s| s.used > 0)
            .unwrap_or(false);
        if solo_used && self.shared.used > 0 {
            self.last_code = XC_LEVEL_CONFLICT;
            return true;
        }
        false
    }

    pub fn degradations(&self) -> u32 {
        self.degradations
    }

    pub fn grants(&self) -> u32 {
        self.grants
    }

    pub fn shared_used(&self) -> u64 {
        self.shared.used
    }
}

/// 自检：两级预算、联合仲裁、加重复核、超支降级。
pub fn run_vea118_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn req(g: u16, b: u64, sh: bool) -> BudgetRequest {
        BudgetRequest {
            gpu_id: g,
            bytes: b,
            use_shared: sh,
        }
    }

    let mut set = CheckSet::new("vea118_joint");

    // C118-01 两级建池
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let r = j.add_solo(0, 5000);
        chk(&mut set, "C118-01 两级", r.is_ok() && j.verify_total(), "建池失败");
    }

    // C118-02 加总超域拒绝
    {
        let mut j = JointArbiter::new(1000, 300).unwrap();
        let r = j.add_solo(0, 800);
        chk(
            &mut set,
            "C118-02 加总复核",
            r.is_err() && j.last_code() == XC_TOTAL_OVER,
            "超域被放",
        );
    }

    // C118-03 独立池申请
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let _ = j.add_solo(0, 5000);
        let r = j.request(req(0, 1000, false));
        chk(&mut set, "C118-03 独立申请", r == Ok(true) && j.grants() == 1, "申请失败");
    }

    // C118-04 共享池申请
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let r = j.request(req(0, 1000, true));
        chk(&mut set, "C118-04 共享申请", r == Ok(true) && j.shared_used() == 1000, "共享失败");
    }

    // C118-05 超支降级
    {
        let mut j = JointArbiter::new(10000, 1000).unwrap();
        let _ = j.request(req(0, 800, true));
        let r = j.request(req(0, 500, true));
        chk(&mut set, "C118-05 超支降级", r == Ok(false) && j.degradations() == 1, "未降级");
    }

    // C118-06 两级冲突检测
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let _ = j.add_solo(0, 5000);
        let _ = j.request(req(0, 100, false));
        let _ = j.request(req(0, 100, true));
        let c = j.detect_conflict(0);
        chk(&mut set, "C118-06 冲突检测", c, "冲突漏检");
    }

    // C118-07 未知池拒绝
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let r = j.request(req(99, 100, false));
        chk(&mut set, "C118-07 未知拒", r.is_err() && j.last_code() == XC_NO_POOL, "未知被放");
    }

    // C118-08 共享超上限拒绝
    {
        let mut j = JointArbiter::new(10000, 100).unwrap();
        let _ = j.request(req(0, 100, true));
        let r = j.request(req(1, 100, true));
        chk(&mut set, "C118-08 共享上限", r.is_err() && j.last_code() == XC_OVERDRAW, "上限被破");
    }

    // C118-09 零域拒绝
    {
        let r = JointArbiter::new(0, 0);
        chk(&mut set, "C118-09 零域拒", r.is_err(), "零域被放");
    }

    // C118-10 加总自洽
    {
        let mut j = JointArbiter::new(10000, 3000).unwrap();
        let _ = j.add_solo(0, 4000);
        let _ = j.add_solo(1, 3000);
        chk(&mut set, "C118-10 加总自洽", j.verify_total(), "加总失真");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea118_all_green() {
        let s = run_vea118_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
