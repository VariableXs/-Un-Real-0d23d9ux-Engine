//! VE-F0120 · P2P 直接拷贝（VE-A 域 · A05 多 GPU 与异构组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0120`
//!
//! **职责定位（锚点原文）**：卡间 P2P 直接拷贝（不过主存的直通传输），P2P 能力
//! 探测与降级（不支持走常规路径），P2P 性能实测对比。
//!
//! **铁律**：
//! 1. **直通传输**——不过主存。
//! 2. **能力探测**——先探后用。
//! 3. **降级路径**——不支持走常规。
//! 4. **实测弃用**——性能反降即弃用 P2P。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BE（P2P 拷贝）。
pub const SEG_BASE: u16 = 0xE8BE;

/// P2P 不支持。
pub const XC_P2P_UNSUPPORTED: u16 = SEG_BASE | 0x01;
/// 性能反降。
pub const XC_P2P_SLOWER: u16 = SEG_BASE | 0x02;
/// 能力虚报。
pub const XC_CAP_LIE: u16 = SEG_BASE | 0x03;
/// 拷贝失败。
pub const XC_COPY_FAIL: u16 = SEG_BASE | 0x04;

/// 拷贝路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyPath {
    /// P2P 直通。
    P2p,
    /// 常规（经主存）。
    ViaHost,
}

/// P2P 能力记录。
#[derive(Clone, Copy, Debug)]
pub struct P2pCap {
    pub from: u16,
    pub to: u16,
    pub supported: bool,
    /// 实测带宽（MB/s）。
    pub measured_bw: u64,
}

/// P2P 拷贝器。
pub struct P2pCopier {
    caps: Vec<P2pCap>,
    /// 降级路径是否启用。
    fallback_ok: bool,
    last_code: u16,
    /// P2P 拷贝成功。
    p2p_copies: u32,
    /// 降级拷贝。
    fallback_copies: u32,
    /// 弃用对（性能反降）。
    deprecated: Vec<(u16, u16)>,
}

impl P2pCopier {
    pub fn new() -> Self {
        P2pCopier {
            caps: Vec::new(),
            fallback_ok: true,
            last_code: 0,
            p2p_copies: 0,
            fallback_copies: 0,
            deprecated: Vec::new(),
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 能力探测。
    pub fn probe(&mut self, from: u16, to: u16, supported: bool) -> P2pCap {
        let cap = P2pCap {
            from,
            to,
            supported,
            measured_bw: 0,
        };
        self.caps.retain(|c| !(c.from == from && c.to == to));
        self.caps.push(cap);
        cap
    }

    /// 实测带宽记录 + 弃用判定。
    pub fn record_perf(&mut self, from: u16, to: u16, p2p_bw: u64, host_bw: u64) -> Result<(), u16> {
        if let Some(c) = self.caps.iter_mut().find(|c| c.from == from && c.to == to) {
            c.measured_bw = p2p_bw;
            if p2p_bw < host_bw {
                // 性能反降 → 弃用。
                self.deprecated.push((from, to));
                self.last_code = XC_P2P_SLOWER;
                return Err(XC_P2P_SLOWER);
            }
            self.last_code = 0;
            Ok(())
        } else {
            self.last_code = XC_CAP_LIE;
            Err(XC_CAP_LIE)
        }
    }

    /// 拷贝执行。
    pub fn copy(&mut self, from: u16, to: u16, bytes: u64) -> Result<(CopyPath, u64), u16> {
        let _ = bytes;
        // 弃用对强制降级。
        if self.deprecated.contains(&(from, to)) {
            self.fallback_copies += 1;
            self.last_code = 0;
            return Ok((CopyPath::ViaHost, 0));
        }
        match self.caps.iter().find(|c| c.from == from && c.to == to) {
            Some(c) if c.supported => {
                self.p2p_copies += 1;
                self.last_code = 0;
                Ok((CopyPath::P2p, 0))
            }
            Some(_) => {
                if self.fallback_ok {
                    self.fallback_copies += 1;
                    self.last_code = XC_P2P_UNSUPPORTED;
                    Ok((CopyPath::ViaHost, 0))
                } else {
                    self.last_code = XC_P2P_UNSUPPORTED;
                    Err(XC_P2P_UNSUPPORTED)
                }
            }
            None => {
                self.last_code = XC_P2P_UNSUPPORTED;
                Err(XC_P2P_UNSUPPORTED)
            }
        }
    }

    /// 能力虚报检测。
    pub fn verify_caps(&mut self) -> Result<(), u16> {
        for c in &self.caps {
            if c.supported && c.measured_bw == 0 {
                self.last_code = XC_CAP_LIE;
                return Err(XC_CAP_LIE);
            }
        }
        self.last_code = 0;
        Ok(())
    }

    pub fn p2p_copies(&self) -> u32 {
        self.p2p_copies
    }

    pub fn fallback_copies(&self) -> u32 {
        self.fallback_copies
    }

    pub fn deprecated(&self) -> &[(u16, u16)] {
        &self.deprecated
    }
}

impl Default for P2pCopier {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：直通传输、能力探测、降级路径、实测弃用。
pub fn run_vea120_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea120_p2p");

    // C120-01 能力探测
    {
        let mut p = P2pCopier::new();
        let c = p.probe(0, 1, true);
        chk(&mut set, "C120-01 探测", c.supported, "探测失败");
    }

    // C120-02 P2P 直通
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        let r = p.copy(0, 1, 100);
        chk(&mut set, "C120-02 直通", r == Ok((CopyPath::P2p, 0)) && p.p2p_copies() == 1, "未直通");
    }

    // C120-03 降级路径
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, false);
        let r = p.copy(0, 1, 100);
        chk(&mut set, "C120-03 降级", r.unwrap().0 == CopyPath::ViaHost && p.fallback_copies() == 1, "未降级");
    }

    // C120-04 实测弃用
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        let r = p.record_perf(0, 1, 50, 100); // P2P 更慢
        chk(
            &mut set,
            "C120-04 弃用",
            r.is_err() && p.deprecated().contains(&(0, 1)),
            "反降未弃",
        );
    }

    // C120-05 弃用后强制降级
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        let _ = p.record_perf(0, 1, 50, 100);
        let r = p.copy(0, 1, 100);
        chk(&mut set, "C120-05 弃用降级", r.unwrap().0 == CopyPath::ViaHost, "弃用仍 P2P");
    }

    // C120-06 能力虚报检出
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true); // supported 但未实测
        let r = p.verify_caps();
        chk(
            &mut set,
            "C120-06 虚报检出",
            r.is_err() && p.last_code() == XC_CAP_LIE,
            "虚报漏检",
        );
    }

    // C120-07 未知对拒绝
    {
        let mut p = P2pCopier::new();
        let r = p.copy(9, 9, 1);
        chk(&mut set, "C120-07 未知拒", r.is_err(), "未知被放");
    }

    // C120-08 实测通过校验
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        let _ = p.record_perf(0, 1, 200, 100);
        let r = p.verify_caps();
        chk(&mut set, "C120-08 校验过", r.is_ok(), "校验误拒");
    }

    // C120-09 计数自洽
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        p.probe(0, 2, false);
        let _ = p.copy(0, 1, 1);
        let _ = p.copy(0, 2, 1);
        chk(&mut set, "C120-09 计数", p.p2p_copies() == 1 && p.fallback_copies() == 1, "计数错");
    }

    // C120-10 性能优不弃
    {
        let mut p = P2pCopier::new();
        p.probe(0, 1, true);
        let r = p.record_perf(0, 1, 200, 100);
        chk(&mut set, "C120-10 优不弃", r.is_ok() && p.deprecated().is_empty(), "优被弃");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea120_all_green() {
        let s = run_vea120_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
