#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDE80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDE80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDE8Code(pub u16);

impl CDE8Code {
    pub const MAP_MISSING: Self = Self(SEG_BASE);     // 映射缺
    pub const URGENT_LOST: Self = Self(SEG_BASE + 1); // 紧急丢
    pub const DUP_DETECT: Self = Self(SEG_BASE + 2);  // 复制检出
    pub const JOINT_DIVERGE: Self = Self(SEG_BASE + 3); // 联动分歧
    pub const SIGN_FAIL: Self = Self(SEG_BASE + 4);   // 双签失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::MAP_MISSING => "映射缺",
            Self::URGENT_LOST => "紧急丢",
            Self::DUP_DETECT => "复制检出",
            Self::JOINT_DIVERGE => "联动分歧",
            Self::SIGN_FAIL => "双签失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::MAP_MISSING, Self::URGENT_LOST, Self::DUP_DETECT,
        Self::JOINT_DIVERGE, Self::SIGN_FAIL,
    ];
}

// ── 十族签名表 ──────────────────────────────────────────────────────

/// API 族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S04ApiFamily {
    AltEngine, Subtitle, Description, Sonar, Realtime,
    Scale, Clamp, Scan, Control, MediaApi,
}

impl S04ApiFamily {
    pub const ALL: [Self; 10] = [
        Self::AltEngine, Self::Subtitle, Self::Description, Self::Sonar, Self::Realtime,
        Self::Scale, Self::Clamp, Self::Scan, Self::Control, Self::MediaApi,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::AltEngine => "替代引擎",
            Self::Subtitle => "字幕",
            Self::Description => "描述",
            Self::Sonar => "声呐",
            Self::Realtime => "实时",
            Self::Scale => "倍率",
            Self::Clamp => "钳制",
            Self::Scan => "扫描",
            Self::Control => "控制",
            Self::MediaApi => "媒体API",
        }
    }
}

/// 签名条目。
#[derive(Debug, Clone)]
pub struct S04Signature {
    pub family: S04ApiFamily,
    pub name: &'static str,
    pub frozen: bool,
}

// ── 收口清单 ────────────────────────────────────────────────────────

/// 映射断言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapAssert {
    pub key: &'static str,
    pub mapped: bool,
}

/// 证据链。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    /// 紧急保真 P0 零发生。
    pub urgent_p0_zero: bool,
    /// 对齐完整。
    pub align_complete: bool,
    /// 开销节省全绿。
    pub overhead_saved: bool,
    /// 复制审计零重叠。
    pub zero_dup: bool,
    /// 消费双签。
    pub dual_signed: bool,
}

impl Evidence {
    /// 全部证据链。
    pub fn all_present(&self) -> bool {
        self.urgent_p0_zero && self.align_complete && self.overhead_saved && self.zero_dup && self.dual_signed
    }
}

/// 双签结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DualSignOff {
    pub lead_signed: bool,
    pub peer_signed: bool,
}

impl DualSignOff {
    pub fn complete(&self) -> bool { self.lead_signed && self.peer_signed }
}

/// S04 收口包。
#[derive(Debug, Clone)]
pub struct S04Close {
    pub signatures: Vec<S04Signature>,
    pub mappings: Vec<MapAssert>,
    pub evidence: Evidence,
    pub sign_off: DualSignOff,
}

impl S04Close {
    pub fn new() -> Self {
        Self {
            signatures: Vec::new(),
            mappings: Vec::new(),
            evidence: Evidence { urgent_p0_zero: true, align_complete: true, overhead_saved: true, zero_dup: true, dual_signed: true },
            sign_off: DualSignOff { lead_signed: false, peer_signed: false },
        }
    }

    /// 十族签名全冻结。
    pub fn all_frozen(&self) -> bool {
        S04ApiFamily::ALL.iter().all(|f| {
            self.signatures.iter().any(|s| s.family == *f && s.frozen)
        })
    }

    /// 映射完整。
    pub fn mappings_ok(&self) -> bool {
        !self.mappings.is_empty() && self.mappings.iter().all(|m| m.mapped)
    }

    /// 紧急硬门。
    pub fn urgent_gate(&self) -> bool { self.evidence.urgent_p0_zero }

    /// 复制硬门。
    pub fn dup_gate(&self) -> bool { self.evidence.zero_dup }

    /// 综合审计。
    pub fn audit(&self) -> S04Audit {
        S04Audit {
            sigs: self.all_frozen(),
            maps: self.mappings_ok(),
            evidence: self.evidence.all_present(),
            signed: self.sign_off.complete(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S04Audit {
    pub sigs: bool,
    pub maps: bool,
    pub evidence: bool,
    pub signed: bool,
}
impl S04Audit {
    pub fn all_green(&self) -> bool { self.sigs && self.maps && self.evidence && self.signed }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3880_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4r_s04close");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 十族枚举
    {
        chk(&mut set, "C3880-01", S04ApiFamily::ALL.len() == 10, "err");
    }
    // 2 族名
    {
        chk(&mut set, "C3880-02", S04ApiFamily::Sonar.name() == "声呐", "err");
    }
    // 3 签名冻结
    {
        let mut c = S04Close::new();
        for f in S04ApiFamily::ALL {
            c.signatures.push(S04Signature { family: f, name: "api", frozen: true });
        }
        chk(&mut set, "C3880-03", c.all_frozen(), "err");
    }
    // 4 签名缺族
    {
        let mut c = S04Close::new();
        c.signatures.push(S04Signature { family: S04ApiFamily::AltEngine, name: "api", frozen: true });
        chk(&mut set, "C3880-04", !c.all_frozen(), "err");
    }
    // 5 映射完整
    {
        let mut c = S04Close::new();
        c.mappings.push(MapAssert { key: "k1", mapped: true });
        chk(&mut set, "C3880-05", c.mappings_ok(), "err");
    }
    // 6 映射缺
    {
        let c = S04Close::new();
        chk(&mut set, "C3880-06", !c.mappings_ok(), "err");
    }
    // 7 紧急硬门
    {
        let c = S04Close::new();
        chk(&mut set, "C3880-07", c.urgent_gate(), "err");
    }
    // 8 紧急硬门失败
    {
        let mut c = S04Close::new();
        c.evidence.urgent_p0_zero = false;
        chk(&mut set, "C3880-08", !c.urgent_gate(), "err");
    }
    // 9 复制硬门
    {
        let c = S04Close::new();
        chk(&mut set, "C3880-09", c.dup_gate(), "err");
    }
    // 10 复制检出
    {
        let mut c = S04Close::new();
        c.evidence.zero_dup = false;
        chk(&mut set, "C3880-10", !c.dup_gate(), "err");
    }
    // 11 双签
    {
        let d = DualSignOff { lead_signed: true, peer_signed: true };
        chk(&mut set, "C3880-11", d.complete(), "err");
    }
    // 12 双签不全
    {
        let d = DualSignOff { lead_signed: true, peer_signed: false };
        chk(&mut set, "C3880-12", !d.complete(), "err");
    }
    // 13 证据全链
    {
        let e = Evidence { urgent_p0_zero: true, align_complete: true, overhead_saved: true, zero_dup: true, dual_signed: true };
        chk(&mut set, "C3880-13", e.all_present(), "err");
    }
    // 14 证据缺环
    {
        let e = Evidence { urgent_p0_zero: true, align_complete: false, overhead_saved: true, zero_dup: true, dual_signed: true };
        chk(&mut set, "C3880-14", !e.all_present(), "err");
    }
    // 15 审计全绿
    {
        let mut c = S04Close::new();
        for f in S04ApiFamily::ALL {
            c.signatures.push(S04Signature { family: f, name: "api", frozen: true });
        }
        c.mappings.push(MapAssert { key: "k", mapped: true });
        c.sign_off = DualSignOff { lead_signed: true, peer_signed: true };
        chk(&mut set, "C3880-15", c.audit().all_green(), "err");
    }
    // 16 审计签名缺
    {
        let mut c = S04Close::new();
        c.mappings.push(MapAssert { key: "k", mapped: true });
        c.sign_off = DualSignOff { lead_signed: true, peer_signed: true };
        chk(&mut set, "C3880-16", !c.audit().all_green(), "err");
    }
    // 17 审计双签缺
    {
        let mut c = S04Close::new();
        for f in S04ApiFamily::ALL {
            c.signatures.push(S04Signature { family: f, name: "api", frozen: true });
        }
        c.mappings.push(MapAssert { key: "k", mapped: true });
        chk(&mut set, "C3880-17", !c.audit().all_green(), "err");
    }
    // 18 映射多条全过
    {
        let mut c = S04Close::new();
        c.mappings.push(MapAssert { key: "a", mapped: true });
        c.mappings.push(MapAssert { key: "b", mapped: true });
        c.mappings.push(MapAssert { key: "c", mapped: true });
        chk(&mut set, "C3880-18", c.mappings_ok(), "err");
    }

    set
}
