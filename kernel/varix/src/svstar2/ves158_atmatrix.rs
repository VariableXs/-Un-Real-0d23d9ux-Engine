//! VE-F3858 读屏与辅助技术矩阵（SEG 0xC9xx）。
//!
//! 三维矩阵（读屏器三型 × 开关控制/眼动 × 三平台）；公开发布红线：
//! 兼容矩阵不公开=用户自担风险；季度更新频率；组合测试记账。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// 诊断码（SEG 0xC9xx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C9Code {
    /// 矩阵缺格
    MatrixHole = 0xC900,
    /// 未公开
    NotPublic = 0xC901,
    /// 超期未更新
    Overdue = 0xC902,
    /// 组合失配
    ComboMismatch = 0xC903,
    /// 索引越界
    IndexOver = 0xC904,
    /// 厂商对端缺失
    VendorMiss = 0xC905,
    /// 维度非法
    DimInvalid = 0xC906,
    /// 测试未跑
    TestMiss = 0xC907,
    /// 发布证据缺失
    PublishMiss = 0xC908,
    /// 频率违规
    FreqViolate = 0xC909,
    /// 格冲突
    CellConflict = 0xC90A,
    /// 真机源缺失
    DeviceMiss = 0xC90B,
}

impl C9Code {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn explain(self) -> &'static str {
        match self {
            C9Code::MatrixHole => "兼容矩阵缺格",
            C9Code::NotPublic => "矩阵未公开",
            C9Code::Overdue => "季度更新超期",
            C9Code::ComboMismatch => "组合失配",
            C9Code::IndexOver => "矩阵索引越界",
            C9Code::VendorMiss => "厂商对端缺失",
            C9Code::DimInvalid => "维度非法",
            C9Code::TestMiss => "组合测试未跑",
            C9Code::PublishMiss => "发布证据缺失",
            C9Code::FreqViolate => "更新频率违规",
            C9Code::CellConflict => "格记录冲突",
            C9Code::DeviceMiss => "真机源缺失",
        }
    }
}

/// 读屏器三型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtReader {
    Desktop,
    Mobile,
    Lite,
}

impl AtReader {
    pub const ALL: [AtReader; 3] = [AtReader::Desktop, AtReader::Mobile, AtReader::Lite];
    pub fn as_str(self) -> &'static str {
        match self {
            AtReader::Desktop => "桌面读屏",
            AtReader::Mobile => "移动读屏",
            AtReader::Lite => "轻量读屏",
        }
    }
}

/// 辅助技术两轴。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtDevice {
    /// 开关控制
    Switch,
    /// 眼动
    EyeGaze,
}

impl AtDevice {
    pub const ALL: [AtDevice; 2] = [AtDevice::Switch, AtDevice::EyeGaze];
    pub fn as_str(self) -> &'static str {
        match self {
            AtDevice::Switch => "开关控制",
            AtDevice::EyeGaze => "眼动",
        }
    }
}

/// 三平台。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtPlatform {
    Desktop,
    Mobile,
    Web,
}

impl AtPlatform {
    pub const ALL: [AtPlatform; 3] = [AtPlatform::Desktop, AtPlatform::Mobile, AtPlatform::Web];
}

/// 兼容结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// 通过
    Pass,
    /// 有限支持
    Limited,
    /// 不支持
    Fail,
    /// 未测
    Untested,
}

impl Compat {
    pub fn as_str(self) -> &'static str {
        match self {
            Compat::Pass => "通过",
            Compat::Limited => "有限",
            Compat::Fail => "不支持",
            Compat::Untested => "未测",
        }
    }
}

/// 矩阵单元。
#[derive(Clone, Debug)]
pub struct AtCell {
    pub reader: AtReader,
    pub device: AtDevice,
    pub platform: AtPlatform,
    pub compat: Compat,
    /// 组合测试已跑
    pub tested: bool,
    /// 厂商对端确认
    pub vendor_ack: bool,
}

/// 兼容矩阵。
pub struct AtMatrix {
    cells: Vec<AtCell>,
    /// 是否已公开发布
    published: bool,
    /// 发布证据编号
    pub_evidence: u32,
    /// 距上次更新天数
    days_since_update: u32,
    /// 真机源（F3850）绑定
    device_bound: bool,
}

impl AtMatrix {
    pub fn new() -> Self {
        let mut cells = Vec::new();
        for r in AtReader::ALL {
            for d in AtDevice::ALL {
                for p in AtPlatform::ALL {
                    cells.push(AtCell {
                        reader: r,
                        device: d,
                        platform: p,
                        compat: Compat::Untested,
                        tested: false,
                        vendor_ack: false,
                    });
                }
            }
        }
        AtMatrix {
            cells,
            published: false,
            pub_evidence: 0,
            days_since_update: 0,
            device_bound: false,
        }
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    fn idx(&self, r: AtReader, d: AtDevice, p: AtPlatform) -> Option<usize> {
        self.cells
            .iter()
            .position(|c| c.reader == r && c.device == d && c.platform == p)
    }

    /// 记录一格结论。
    pub fn set(
        &mut self,
        r: AtReader,
        d: AtDevice,
        p: AtPlatform,
        compat: Compat,
        tested: bool,
        vendor_ack: bool,
    ) -> Result<(), C9Code> {
        let i = self.idx(r, d, p).ok_or(C9Code::IndexOver)?;
        self.cells[i].compat = compat;
        self.cells[i].tested = tested;
        self.cells[i].vendor_ack = vendor_ack;
        Ok(())
    }

    pub fn untested(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| c.compat == Compat::Untested || !c.tested)
            .count()
    }

    pub fn vendor_missing(&self) -> usize {
        self.cells.iter().filter(|c| !c.vendor_ack).count()
    }

    pub fn publish(&mut self, evidence: u32) -> Result<(), C9Code> {
        if evidence == 0 {
            return Err(C9Code::PublishMiss);
        }
        if self.untested() > 0 {
            return Err(C9Code::TestMiss);
        }
        self.published = true;
        self.pub_evidence = evidence;
        Ok(())
    }

    pub fn is_public(&self) -> bool {
        self.published
    }

    /// 季度更新：90 天内必须更新。
    pub fn tick_day(&mut self) {
        self.days_since_update += 1;
    }

    pub fn note_update(&mut self) {
        self.days_since_update = 0;
    }

    pub fn overdue(&self) -> bool {
        self.days_since_update > 90
    }

    pub fn bind_device_source(&mut self) {
        self.device_bound = true;
    }

    pub fn device_ok(&self) -> bool {
        self.device_bound
    }

    /// 发布红线：未公开即违规。
    pub fn public_redline(&self) -> bool {
        !self.published
    }
}

pub fn run_ves158_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves158_atmatrix");
    {
        let m = AtMatrix::new();
        chk(&mut set, "C3858-01", m.cell_count() == 18, "3×2×3=18格");
    }
    {
        let mut m = AtMatrix::new();
        let r = m.set(AtReader::Desktop, AtDevice::Switch, AtPlatform::Web, Compat::Pass, true, true);
        chk(&mut set, "C3858-02", r.is_ok(), "格写入成功");
    }
    {
        let mut m = AtMatrix::new();
        chk(&mut set, "C3858-03", m.untested() == 18, "初始全未测");
    }
    {
        let mut m = AtMatrix::new();
        let r = m.publish(7);
        chk(
            &mut set,
            "C3858-04",
            r == Err(C9Code::TestMiss),
            "未测不可发布",
        );
    }
    {
        let mut m = AtMatrix::new();
        let r = m.publish(0);
        chk(
            &mut set,
            "C3858-05",
            r == Err(C9Code::PublishMiss),
            "缺证据拒发布",
        );
    }
    {
        let mut m = AtMatrix::new();
        for r in AtReader::ALL {
            for d in AtDevice::ALL {
                for p in AtPlatform::ALL {
                    let _ = m.set(r, d, p, Compat::Pass, true, true);
                }
            }
        }
        let e = m.publish(42);
        chk(&mut set, "C3858-06", e.is_ok() && m.is_public(), "全测后可发布");
    }
    {
        let mut m = AtMatrix::new();
        chk(&mut set, "C3858-07", !m.overdue(), "初始未超期");
    }
    {
        let mut m = AtMatrix::new();
        for _ in 0..91 {
            m.tick_day();
        }
        chk(&mut set, "C3858-08", m.overdue(), "91天超期");
    }
    {
        let mut m = AtMatrix::new();
        for _ in 0..50 {
            m.tick_day();
        }
        m.note_update();
        chk(&mut set, "C3858-09", !m.overdue(), "更新后重置");
    }
    {
        let m = AtMatrix::new();
        chk(&mut set, "C3858-10", m.public_redline(), "未公开触发红线");
    }
    {
        let mut m = AtMatrix::new();
        m.bind_device_source();
        chk(&mut set, "C3858-11", m.device_ok(), "真机源绑定");
    }
    {
        let mut m = AtMatrix::new();
        let _ = m.set(AtReader::Lite, AtDevice::EyeGaze, AtPlatform::Mobile, Compat::Limited, true, false);
        chk(&mut set, "C3858-12", m.vendor_missing() >= 1, "厂商对端缺失计数");
    }
    set
}
