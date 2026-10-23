//! VE-F0122 · 异构能力协商 API（VE-A 域 · A05 多 GPU 与异构组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0122`
//!
//! **职责定位（锚点原文）**：多卡能力的统一协商 API（应用一次查询到位），协商结果
//! 的快照语义（协商后能力冻结），API 冻结门面。
//!
//! **铁律**：
//! 1. **统一协商**——一次查询到位。
//! 2. **快照冻结**——协商后能力冻结不变。
//! 3. **单卡降级**——查询失败降级单卡。
//! 4. **版本拦截**——API 版本不符拒绝。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C0（能力协商）。
pub const SEG_BASE: u16 = 0xE8C0;

/// 版本不匹配。
pub const XC_VERSION_MISMATCH: u16 = SEG_BASE | 0x01;
/// 结果漂移（冻结后变更）。
pub const XC_RESULT_DRIFT: u16 = SEG_BASE | 0x02;
/// 查询失败。
pub const XC_QUERY_FAIL: u16 = SEG_BASE | 0x03;
/// 快照未建立。
pub const XC_NO_SNAPSHOT: u16 = SEG_BASE | 0x04;

/// API 版本。
pub const API_VERSION: u16 = 0x0100;

/// 能力项。
#[derive(Clone, Copy, Debug)]
pub struct Capability {
    pub gpu_id: u16,
    /// 能力位。
    pub bits: u64,
    /// 显存（MB）。
    pub vram_mb: u32,
}

/// 协商快照（冻结）。
#[derive(Clone, Debug)]
pub struct NegotiationSnapshot {
    pub version: u16,
    pub caps: Vec<Capability>,
    /// 冻结标记。
    pub frozen: bool,
    /// 快照序号。
    pub seq: u64,
}

/// 协商 API 门面。
pub struct NegotiationApi {
    api_version: u16,
    snapshot: Option<NegotiationSnapshot>,
    seq: u64,
    last_code: u16,
    /// 降级次数。
    degradations: u32,
    queries: u32,
}

impl NegotiationApi {
    pub fn new(api_version: u16) -> Result<Self, u16> {
        if api_version != API_VERSION {
            return Err(XC_VERSION_MISMATCH);
        }
        Ok(NegotiationApi {
            api_version,
            snapshot: None,
            seq: 0,
            last_code: 0,
            degradations: 0,
            queries: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 一次协商（一次查询到位）。
    pub fn negotiate(&mut self, caps: Vec<Capability>) -> Result<u64, u16> {
        if caps.is_empty() {
            // 降级单卡。
            self.degradations += 1;
            self.last_code = XC_QUERY_FAIL;
            return Err(XC_QUERY_FAIL);
        }
        self.seq += 1;
        self.snapshot = Some(NegotiationSnapshot {
            version: self.api_version,
            caps,
            frozen: true,
            seq: self.seq,
        });
        self.last_code = 0;
        Ok(self.seq)
    }

    /// 查询（冻结保护：只读）。
    pub fn query(&mut self, gpu_id: u16) -> Result<Capability, u16> {
        self.queries += 1;
        match &self.snapshot {
            Some(s) if s.frozen => {
                match s.caps.iter().find(|c| c.gpu_id == gpu_id) {
                    Some(c) => {
                        self.last_code = 0;
                        Ok(*c)
                    }
                    None => {
                        self.last_code = XC_QUERY_FAIL;
                        Err(XC_QUERY_FAIL)
                    }
                }
            }
            Some(_) => {
                self.last_code = XC_RESULT_DRIFT;
                Err(XC_RESULT_DRIFT)
            }
            None => {
                self.last_code = XC_NO_SNAPSHOT;
                Err(XC_NO_SNAPSHOT)
            }
        }
    }

    /// 尝试解冻（冻结后拒绝 = 快照保护）。
    pub fn try_unfreeze(&mut self) -> Result<(), u16> {
        match &mut self.snapshot {
            Some(s) if s.frozen => {
                self.last_code = XC_RESULT_DRIFT;
                Err(XC_RESULT_DRIFT)
            }
            Some(_) => Ok(()),
            None => {
                self.last_code = XC_NO_SNAPSHOT;
                Err(XC_NO_SNAPSHOT)
            }
        }
    }

    /// 全量枚举（一次到位）。
    pub fn enumerate(&self) -> Result<&[Capability], u16> {
        match &self.snapshot {
            Some(s) => Ok(&s.caps),
            None => Err(XC_NO_SNAPSHOT),
        }
    }

    pub fn degradations(&self) -> u32 {
        self.degradations
    }

    pub fn queries(&self) -> u32 {
        self.queries
    }

    pub fn api_version(&self) -> u16 {
        self.api_version
    }
}

/// 自检：统一协商、快照冻结、单卡降级、版本拦截。
pub fn run_vea122_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn caps() -> Vec<Capability> {
        vec![
            Capability {
                gpu_id: 0,
                bits: 0xFF,
                vram_mb: 8192,
            },
            Capability {
                gpu_id: 1,
                bits: 0x0F,
                vram_mb: 4096,
            },
        ]
    }

    let mut set = CheckSet::new("vea122_negotiate");

    // C122-01 版本拦截
    {
        let r = NegotiationApi::new(0x9999);
        chk(
            &mut set,
            "C122-01 版本拦截",
            r.err() == Some(XC_VERSION_MISMATCH),
            "版本被放",
        );
    }

    // C122-02 一次协商
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let r = a.negotiate(caps());
        chk(&mut set, "C122-02 协商", r.is_ok() && a.enumerate().unwrap().len() == 2, "协商失败");
    }

    // C122-03 快照冻结
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let _ = a.negotiate(caps());
        let r = a.try_unfreeze();
        chk(
            &mut set,
            "C122-03 冻结",
            r.is_err() && a.last_code() == XC_RESULT_DRIFT,
            "解冻被放",
        );
    }

    // C122-04 冻结后查询稳定
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let _ = a.negotiate(caps());
        let q1 = a.query(0).unwrap();
        let q2 = a.query(0).unwrap();
        chk(&mut set, "C122-04 稳定", q1.bits == q2.bits, "结果漂移");
    }

    // C122-05 空查询降级
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let r = a.negotiate(vec![]);
        chk(
            &mut set,
            "C122-05 降级",
            r.is_err() && a.degradations() == 1,
            "未降级",
        );
    }

    // C122-06 未协商查询拒绝
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let r = a.query(0);
        chk(
            &mut set,
            "C122-06 未协商拒",
            r.is_err() && a.last_code() == XC_NO_SNAPSHOT,
            "未协商被放",
        );
    }

    // C122-07 未知卡查询拒绝
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let _ = a.negotiate(caps());
        let r = a.query(99);
        chk(&mut set, "C122-07 未知拒", r.is_err(), "未知被放");
    }

    // C122-08 重新协商换代
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let s1 = a.negotiate(caps()).unwrap();
        let s2 = a.negotiate(caps()).unwrap();
        chk(&mut set, "C122-08 换代", s2 > s1, "序号未增");
    }

    // C122-09 枚举只读
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let _ = a.negotiate(caps());
        let n = a.enumerate().unwrap().len();
        chk(&mut set, "C122-09 只读枚举", n == 2, "枚举错");
    }

    // C122-10 查询计数
    {
        let mut a = NegotiationApi::new(API_VERSION).unwrap();
        let _ = a.negotiate(caps());
        let _ = a.query(0);
        let _ = a.query(1);
        chk(&mut set, "C122-10 计数", a.queries() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea122_all_green() {
        let s = run_vea122_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
