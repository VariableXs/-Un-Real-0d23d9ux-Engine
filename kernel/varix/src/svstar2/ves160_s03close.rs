//! VE-F3860 S03 组收口双签（SEG 0xCBxx）。
//!
//! 十二族签名表冻结；映射断言；断流 P0 硬门；真机硬门；三方证据；
//! 双签两轮上限。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// 诊断码（SEG 0xCBxx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CB3Code {
    /// 映射缺
    MapMiss = 0xCB00,
    /// 断流检出（P0 硬门）
    StreamBreak = 0xCB01,
    /// 真机缺失
    DeviceMiss = 0xCB02,
    /// 联动分歧
    LinkDrift = 0xCB03,
    /// 签收不全
    SignIncomplete = 0xCB04,
    /// 两轮超限
    RoundOver = 0xCB05,
    /// 族缺失
    FamilyMiss = 0xCB06,
    /// 证据缺失
    EvidenceMiss = 0xCB07,
    /// 硬门拒
    GateReject = 0xCB08,
    /// 签名漂移
    SignDrift = 0xCB09,
    /// 清单越界
    ListOver = 0xCB0A,
    /// 哈希失配
    HashMismatch = 0xCB0B,
}

impl CB3Code {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn explain(self) -> &'static str {
        match self {
            CB3Code::MapMiss => "映射缺失",
            CB3Code::StreamBreak => "断流P0硬门",
            CB3Code::DeviceMiss => "真机缺失",
            CB3Code::LinkDrift => "联动分歧",
            CB3Code::SignIncomplete => "签收不全",
            CB3Code::RoundOver => "双签两轮超限",
            CB3Code::FamilyMiss => "签名族缺失",
            CB3Code::EvidenceMiss => "证据缺失",
            CB3Code::GateReject => "硬门拒绝",
            CB3Code::SignDrift => "签名漂移",
            CB3Code::ListOver => "清单越界",
            CB3Code::HashMismatch => "哈希失配",
        }
    }
}

/// S03 十二族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigFamily {
    /// 闭环架构
    LoopArch,
    /// 跟随
    Follow,
    /// 标注
    Annot,
    /// 噪音
    Noise,
    /// 公告
    Announce,
    /// 导航
    Nav,
    /// 虚拟化
    Virt,
    /// 表面
    Surface,
    /// 预算
    Budget,
    /// 调试
    Debug,
    /// 矩阵
    Matrix,
    /// 国际化
    I18n,
}

impl SigFamily {
    pub const ALL: [SigFamily; 12] = [
        SigFamily::LoopArch,
        SigFamily::Follow,
        SigFamily::Annot,
        SigFamily::Noise,
        SigFamily::Announce,
        SigFamily::Nav,
        SigFamily::Virt,
        SigFamily::Surface,
        SigFamily::Budget,
        SigFamily::Debug,
        SigFamily::Matrix,
        SigFamily::I18n,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            SigFamily::LoopArch => "闭环架构",
            SigFamily::Follow => "跟随",
            SigFamily::Annot => "标注",
            SigFamily::Noise => "噪音",
            SigFamily::Announce => "公告",
            SigFamily::Nav => "导航",
            SigFamily::Virt => "虚拟化",
            SigFamily::Surface => "表面",
            SigFamily::Budget => "预算",
            SigFamily::Debug => "调试",
            SigFamily::Matrix => "矩阵",
            SigFamily::I18n => "国际化",
        }
    }
}

/// 四联动证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkEvidence {
    /// 断流零发生
    NoBreak,
    /// 真机矩阵公开
    MatrixPublic,
    /// 覆盖 100%
    CoverFull,
    /// 三方对拍终版
    TriCompare,
}

impl LinkEvidence {
    pub const ALL: [LinkEvidence; 4] = [
        LinkEvidence::NoBreak,
        LinkEvidence::MatrixPublic,
        LinkEvidence::CoverFull,
        LinkEvidence::TriCompare,
    ];
}

/// 族签名条目。
#[derive(Clone, Debug)]
pub struct FamilySign {
    pub family: SigFamily,
    /// 签名指纹
    pub fingerprint: u64,
    /// 映射到冻结 API
    pub mapped: bool,
}

/// 收口执行器。
pub struct S03Close {
    signs: Vec<FamilySign>,
    links: Vec<LinkEvidence>,
    /// 双签轮次
    rounds: u8,
    signed_a: bool,
    signed_b: bool,
    /// 断流零发生证据
    no_break_proof: bool,
    /// 真机硬门
    device_ok: bool,
}

impl S03Close {
    pub fn new() -> Self {
        let signs = SigFamily::ALL
            .iter()
            .map(|f| FamilySign {
                family: *f,
                fingerprint: 0,
                mapped: false,
            })
            .collect();
        S03Close {
            signs,
            links: Vec::new(),
            rounds: 0,
            signed_a: false,
            signed_b: false,
            no_break_proof: false,
            device_ok: false,
        }
    }

    fn idx(&self, f: SigFamily) -> Option<usize> {
        self.signs.iter().position(|x| x.family == f)
    }

    pub fn freeze(&mut self, f: SigFamily, fingerprint: u64, mapped: bool) -> Result<(), CB3Code> {
        let i = self.idx(f).ok_or(CB3Code::FamilyMiss)?;
        self.signs[i].fingerprint = fingerprint;
        self.signs[i].mapped = mapped;
        Ok(())
    }

    pub fn family_count(&self) -> usize {
        self.signs.len()
    }

    pub fn unmapped(&self) -> usize {
        self.signs.iter().filter(|x| !x.mapped).count()
    }

    pub fn add_link(&mut self, e: LinkEvidence) {
        if !self.links.contains(&e) {
            self.links.push(e);
        }
    }

    pub fn link_ok(&self) -> bool {
        LinkEvidence::ALL.iter().all(|e| self.links.contains(e))
    }

    pub fn set_no_break_proof(&mut self, v: bool) {
        self.no_break_proof = v;
    }

    pub fn set_device_ok(&mut self, v: bool) {
        self.device_ok = v;
    }

    /// 硬门：断流证据 + 真机。
    pub fn hard_gate(&self) -> Result<(), CB3Code> {
        if !self.no_break_proof {
            return Err(CB3Code::StreamBreak);
        }
        if !self.device_ok {
            return Err(CB3Code::DeviceMiss);
        }
        Ok(())
    }

    /// 双签（两轮上限）。
    pub fn sign(&mut self, who: u8) -> Result<(), CB3Code> {
        if self.rounds >= 2 && !(self.signed_a && self.signed_b) {
            return Err(CB3Code::RoundOver);
        }
        if self.unmapped() > 0 {
            return Err(CB3Code::MapMiss);
        }
        self.hard_gate()?;
        if !self.link_ok() {
            return Err(CB3Code::EvidenceMiss);
        }
        match who {
            0 => self.signed_a = true,
            1 => self.signed_b = true,
            _ => return Err(CB3Code::SignIncomplete),
        }
        self.rounds += 1;
        Ok(())
    }

    pub fn fully_signed(&self) -> bool {
        self.signed_a && self.signed_b
    }
}

pub fn run_ves160_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves160_s03close");
    {
        let c = S03Close::new();
        chk(&mut set, "C3860-01", c.family_count() == 12, "十二族签名");
    }
    {
        let mut c = S03Close::new();
        let r = c.freeze(SigFamily::Follow, 0xABCD, true);
        chk(&mut set, "C3860-02", r.is_ok(), "族冻结成功");
    }
    {
        let mut c = S03Close::new();
        for f in SigFamily::ALL {
            let _ = c.freeze(f, 1, true);
        }
        chk(&mut set, "C3860-03", c.unmapped() == 0, "映射全齐");
    }
    {
        let mut c = S03Close::new();
        c.set_no_break_proof(false);
        let r = c.hard_gate();
        chk(
            &mut set,
            "C3860-04",
            r == Err(CB3Code::StreamBreak),
            "断流硬门",
        );
    }
    {
        let mut c = S03Close::new();
        c.set_no_break_proof(true);
        c.set_device_ok(false);
        let r = c.hard_gate();
        chk(
            &mut set,
            "C3860-05",
            r == Err(CB3Code::DeviceMiss),
            "真机硬门",
        );
    }
    {
        let mut c = S03Close::new();
        for f in SigFamily::ALL {
            let _ = c.freeze(f, 2, true);
        }
        c.set_no_break_proof(true);
        c.set_device_ok(true);
        for e in LinkEvidence::ALL {
            c.add_link(e);
        }
        let r = c.sign(0);
        chk(&mut set, "C3860-06", r.is_ok(), "第一签成功");
    }
    {
        let mut c = S03Close::new();
        for f in SigFamily::ALL {
            let _ = c.freeze(f, 3, true);
        }
        c.set_no_break_proof(true);
        c.set_device_ok(true);
        for e in LinkEvidence::ALL {
            c.add_link(e);
        }
        let _ = c.sign(0);
        let _ = c.sign(1);
        chk(&mut set, "C3860-07", c.fully_signed(), "双签完成");
    }
    {
        let mut c = S03Close::new();
        chk(&mut set, "C3860-08", !c.link_ok(), "联动证据初始缺");
    }
    {
        let mut c = S03Close::new();
        for f in SigFamily::ALL {
            let _ = c.freeze(f, 4, false);
        }
        c.set_no_break_proof(true);
        c.set_device_ok(true);
        for e in LinkEvidence::ALL {
            c.add_link(e);
        }
        let r = c.sign(0);
        chk(
            &mut set,
            "C3860-09",
            r == Err(CB3Code::MapMiss),
            "映射缺拒签",
        );
    }
    {
        let mut c = S03Close::new();
        for f in SigFamily::ALL {
            let _ = c.freeze(f, 5, true);
        }
        c.set_no_break_proof(true);
        c.set_device_ok(true);
        let r = c.sign(0);
        chk(
            &mut set,
            "C3860-10",
            r == Err(CB3Code::EvidenceMiss),
            "证据缺拒签",
        );
    }
    {
        let mut c = S03Close::new();
        let r = c.freeze(SigFamily::I18n, 9, true);
        chk(&mut set, "C3860-11", r.is_ok(), "末族可冻结");
        let c2 = S03Close::new();
        chk(&mut set, "C3860-12", c2.family_count() == 12, "清单边界");
    }
    set
}
