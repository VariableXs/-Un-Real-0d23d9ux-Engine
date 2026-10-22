//! CGPU-F0414 · 集成文档（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 文档：域间集成手册（每对接点的契约图/数据流/失败模式——六域
//!   六节）；核销清单终版（全部预留→已兑现状态表）。
//! - 判据：手册六节、核销终版、判据。
//!
//! 设计纪律：
//! - 手册六节固定坐标（D/E/H/I/J/K），每节三块：契约图（对端+签名对）、
//!   数据流（方向序）、失败模式（失败码+处置）——三块齐才成节。
//! - 契约图：本域签名 ↔ 对端签名对表；对端名缺失即节不完整。
//! - 数据流：方向步序号单调（步 1..N），跳步即流断裂红。
//! - 失败模式：失败码 → 处置动作，处置非空。
//! - 核销清单终版：F0401 九条预留逐一出状态表（Pending/InFlight/
//!   Redeemed），终版要求全 Redeemed 且三证齐——终版是硬闸。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D10 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D10，全仓独占）。
pub const SEG_BASE: u16 = 0x0D10;

pub const E_DOC_SECTION: u16 = SEG_BASE + 1; // 节不完整（三块缺一）
pub const E_DOC_FLOWGAP: u16 = SEG_BASE + 2; // 数据流跳步
pub const E_DOC_XREF: u16 = SEG_BASE + 3; // 契约图对端缺失
pub const E_DOC_FINAL: u16 = SEG_BASE + 4; // 核销终版未全兑
pub const E_DOC_POINT: u16 = SEG_BASE + 5; // 域节号非法
pub const E_DOC_FAILMODE: u16 = SEG_BASE + 6; // 失败模式处置为空

/// 六域节坐标（矩阵同源）。
pub const DOC_POINTS: usize = 6;

pub fn section_title(p: u8) -> &'static str {
    match p {
        0 => "第一章 · 与 D 域 80 帧系统集成",
        1 => "第二章 · 与 E 域表面配额集成",
        2 => "第三章 · 与 H 显存域集成",
        3 => "第四章 · 与 I 带宽域集成",
        4 => "第五章 · 与 J 功耗域集成",
        _ => "第六章 · 与 K 异构域集成",
    }
}

/// 契约图行：本域签名 ↔ 对端签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XrefRow {
    pub local_sig: String,
    pub peer_sig: String,
}

/// 数据流步。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowStep {
    pub step: u32,
    pub desc: String,
}

/// 失败模式行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailMode {
    pub code: u16,
    pub action: String,
}

/// 手册一节。
#[derive(Clone, Debug, Default)]
pub struct ManualSection {
    pub xrefs: Vec<XrefRow>,
    pub flow: Vec<FlowStep>,
    pub fails: Vec<FailMode>,
}

impl ManualSection {
    /// 节完整性：三块齐（图≥1、流≥2 步、失败模式≥1 且处置非空）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.xrefs.is_empty() || self.flow.len() < 2 || self.fails.is_empty() {
            return Err(E_DOC_SECTION);
        }
        for x in &self.xrefs {
            if x.peer_sig.is_empty() {
                return Err(E_DOC_XREF);
            }
        }
        // 流步序号严格递增且从 1 起。
        for (i, s) in self.flow.iter().enumerate() {
            if s.step != (i + 1) as u32 {
                return Err(E_DOC_FLOWGAP);
            }
        }
        for f in &self.fails {
            if f.action.is_empty() {
                return Err(E_DOC_FAILMODE);
            }
        }
        Ok(())
    }
}

/// 集成手册（六节）。
#[derive(Clone, Debug, Default)]
pub struct IntManual {
    pub sections: Vec<(u8, ManualSection)>,
}

impl IntManual {
    pub fn put(&mut self, point: u8, s: ManualSection) -> Result<(), u16> {
        if point as usize >= DOC_POINTS {
            return Err(E_DOC_POINT);
        }
        if self.sections.iter().any(|(p, _)| *p == point) {
            return Err(E_DOC_POINT);
        }
        self.sections.push((point, s));
        Ok(())
    }

    /// 手册完整性：六节齐且逐节绿。
    pub fn validate(&self) -> Result<(), u16> {
        for p in 0..DOC_POINTS as u8 {
            match self.sections.iter().find(|(q, _)| *q == p) {
                None => return Err(E_DOC_SECTION),
                Some((_, s)) => s.validate()?,
            }
        }
        Ok(())
    }

    /// 章名视图（渲染输入）。
    pub fn titles(&self) -> Vec<&'static str> {
        self.sections
            .iter()
            .map(|(p, _)| section_title(*p))
            .collect()
    }
}

/// 核销状态（与 F0401 同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedeemState {
    Pending,
    InFlight,
    Redeemed,
}

/// 核销清单终版行。
#[derive(Clone, Debug)]
pub struct FinalRow {
    pub anchor: u32,
    pub peer: u8,
    pub state: RedeemState,
    /// 兑现三证（终版硬闸要求全非零）。
    pub evid: (u32, u32, u32),
}

impl FinalRow {
    pub fn closed(&self) -> bool {
        self.state == RedeemState::Redeemed
            && self.evid.0 != 0
            && self.evid.1 != 0
            && self.evid.2 != 0
    }
}

/// 核销清单终版。
#[derive(Clone, Debug, Default)]
pub struct FinalChecklist {
    pub rows: Vec<FinalRow>,
}

/// 标准九预留锚点（与 F0401 同源）。
pub const FINAL_ANCHORS: [(u32, u8); 9] = [
    (332, b'D'),
    (342, b'E'),
    (338, b'H'),
    (338, b'I'),
    (338, b'J'),
    (346, b'K'),
    (377, b'P'),
    (378, b'N'),
    (394, b'U'),
];

impl FinalChecklist {
    /// 终版装订：九条逐行入表（锚点+对端唯一）。
    pub fn bind(&mut self, anchor: u32, peer: u8, state: RedeemState, evid: (u32, u32, u32)) -> Result<(), u16> {
        if self
            .rows
            .iter()
            .any(|r| r.anchor == anchor && r.peer == peer)
        {
            return Err(E_DOC_FINAL);
        }
        self.rows.push(FinalRow { anchor, peer, state, evid });
        Ok(())
    }

    /// 终版闸：标准九条齐 + 全部 Redeemed + 三证齐。
    pub fn final_ok(&self) -> Result<(), u16> {
        for (a, p) in FINAL_ANCHORS {
            match self.rows.iter().find(|r| r.anchor == a && r.peer == p) {
                None => return Err(E_DOC_FINAL),
                Some(r) if !r.closed() => return Err(E_DOC_FINAL),
                _ => {}
            }
        }
        Ok(())
    }

    /// 终版表格文本（归档渲染：锚点/对端/状态/三证）。
    pub fn render(&self) -> Vec<(u32, u8, &'static str)> {
        self.rows
            .iter()
            .map(|r| {
                (
                    r.anchor,
                    r.peer,
                    match r.state {
                        RedeemState::Pending => "待领",
                        RedeemState::InFlight => "施工中",
                        RedeemState::Redeemed => "已兑现",
                    },
                )
            })
            .collect()
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> ManualSection {
        ManualSection {
            xrefs: vec![XrefRow {
                local_sig: "int.link.register".to_string(),
                peer_sig: "d08.frame.report".to_string(),
            }],
            flow: vec![
                FlowStep { step: 1, desc: "D 域帧计时上报".to_string() },
                FlowStep { step: 2, desc: "预算归因".to_string() },
                FlowStep { step: 3, desc: "修正回写".to_string() },
            ],
            fails: vec![FailMode { code: 0x0C51, action: "闭环状态机复位".to_string() }],
        }
    }

    #[test]
    fn a01_手册六节_三块齐才成节() {
        let mut m = IntManual::default();
        for p in 0..DOC_POINTS as u8 {
            m.put(p, section()).ok();
        }
        assert_eq!(m.validate(), Ok(()));
        assert_eq!(m.titles().len(), 6);
        assert_eq!(m.titles()[0], section_title(0));
        // 缺节红。
        let mut m2 = m.clone();
        m2.sections.remove(4);
        assert_eq!(m2.validate(), Err(E_DOC_SECTION));
    }

    #[test]
    fn a02_数据流_跳步即断() {
        let mut s = section();
        s.flow.push(FlowStep { step: 5, desc: "跳步".to_string() });
        assert_eq!(s.validate(), Err(E_DOC_FLOWGAP));
        // 步序修正后绿。
        let mut s2 = section();
        s2.flow.push(FlowStep { step: 4, desc: "第四步".to_string() });
        assert_eq!(s2.validate(), Ok(()));
        // 对端缺失红。
        let mut s3 = section();
        s3.xrefs[0].peer_sig = String::new();
        assert_eq!(s3.validate(), Err(E_DOC_XREF));
    }

    #[test]
    fn a03_失败模式_处置非空() {
        let mut s = section();
        s.fails.push(FailMode { code: 9, action: String::new() });
        assert_eq!(s.validate(), Err(E_DOC_FAILMODE));
        // 空失败模式列表也拒。
        let mut s2 = section();
        s2.fails.clear();
        assert_eq!(s2.validate(), Err(E_DOC_SECTION));
    }

    #[test]
    fn a04_核销终版_九条全兑三证齐() {
        let mut f = FinalChecklist::default();
        for (a, p) in FINAL_ANCHORS {
            f.bind(a, p, RedeemState::Redeemed, (1, 2, 3)).ok();
        }
        assert_eq!(f.final_ok(), Ok(()));
        // 重复装订拒。
        assert_eq!(
            f.bind(332, b'D', RedeemState::Redeemed, (1, 1, 1)),
            Err(E_DOC_FINAL)
        );
        // 有未兑行 ⇒ 终版红。
        let mut f2 = FinalChecklist::default();
        for (a, p) in FINAL_ANCHORS {
            f2.bind(a, p, RedeemState::Redeemed, (1, 2, 3)).ok();
        }
        f2.rows[6].state = RedeemState::InFlight; // P 域未兑
        assert_eq!(f2.final_ok(), Err(E_DOC_FINAL));
        // 三证缺一 ⇒ 红。
        let mut f3 = FinalChecklist::default();
        for (a, p) in FINAL_ANCHORS {
            f3.bind(a, p, RedeemState::Redeemed, (1, 2, 3)).ok();
        }
        f3.rows[0].evid = (1, 0, 3);
        assert_eq!(f3.final_ok(), Err(E_DOC_FINAL));
        // 渲染表。
        let table = f.render();
        assert_eq!(table.len(), 9);
        assert!(table.iter().all(|(_, _, s)| *s == "已兑现"));
    }

    #[test]
    fn a05_章名表_六节固化() {
        assert_eq!(section_title(2), "第三章 · 与 H 显存域集成");
        assert_eq!(section_title(5), "第六章 · 与 K 异构域集成");
        // 域节越界拒。
        let mut m = IntManual::default();
        assert_eq!(m.put(9, section()), Err(E_DOC_POINT));
    }
}
