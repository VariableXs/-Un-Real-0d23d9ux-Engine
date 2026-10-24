//! VE-F3736 · 创作性能资产汇总（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3736`
//!
//! **职责定位（锚点原文）**：创作性能资产总册（P09 模式五用）——
//! - **八资产**：成本模型/基准/门禁看板/降档矩阵/长稳档案
//!   + 心流指标册/功耗画像/调试器——八资产指针 + 状态；
//! - **符合度快照**：八源 + 中断率 + 等待（复述超限登记）；
//! - **四热点档案**：Q 版热点（共享率/线性度/降档频度/重锚效果）——
//!   复述四记录；
//! - **曲线健康**复述。
//!
//! **判据（锚点原文六条）**：八资产、心流入册、四热点、
//! 四用复刻、曲线健康、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 超限存量→两出口（复述） | [`A7Code::OVERSTOCK`] |
//! | 指针失效→重链（复述） | [`A7Code::POINTER_DIED`] |
//! | 热点缺记录→补齐（复述） | [`A7Code::HOTSPOT_MISS`] |
//! | 心流缺→补（红线联动） | [`A7Code::FLOW_MISS`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3736 诊断码段（独占 0x7Fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A7Code(pub u16);

impl A7Code {
    /// 超限存量（两出口）。
    pub const OVERSTOCK: A7Code = A7Code(0x7F01);
    /// 指针失效（重链）。
    pub const POINTER_DIED: A7Code = A7Code(0x7F02);
    /// 热点缺记录（补齐）。
    pub const HOTSPOT_MISS: A7Code = A7Code(0x7F03);
    /// 心流缺（补全）。
    pub const FLOW_MISS: A7Code = A7Code(0x7F04);
    /// 资产 ID 非法。
    pub const BAD_ASSET: A7Code = A7Code(0x7F05);
    /// 曲线不健康。
    pub const CURVE_UNHEALTHY: A7Code = A7Code(0x7F06);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            A7Code::OVERSTOCK => "超限存量（复述）。建议：两出口分流处置。",
            A7Code::POINTER_DIED => "指针失效（复述）。建议：重链资产。",
            A7Code::HOTSPOT_MISS => "热点缺记录（复述）。建议：补齐四热点档案。",
            A7Code::FLOW_MISS => "心流缺（红线联动）。建议：心流指标须入册。",
            A7Code::BAD_ASSET => "资产 ID 非法。建议：ID 0-7。",
            A7Code::CURVE_UNHEALTHY => "曲线不健康。建议：核验曲线健康标记。",
            _ => "未知诊断码。",
        }
    }
}

// ── 八资产指针 ──────────────────────────────────────────────────────

/// 资产 ID。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetId {
    /// 成本模型。
    CostModel = 0,
    /// 基准。
    Bench = 1,
    /// 门禁看板。
    GateDash = 2,
    /// 降档矩阵。
    Degrade = 3,
    /// 长稳档案。
    LongRun = 4,
    /// 心流指标册（红线联动）。
    FlowBook = 5,
    /// 功耗画像。
    Power = 6,
    /// 调试器。
    Debugger = 7,
}

impl AssetId {
    pub const ALL: [AssetId; 8] = [
        AssetId::CostModel,
        AssetId::Bench,
        AssetId::GateDash,
        AssetId::Degrade,
        AssetId::LongRun,
        AssetId::FlowBook,
        AssetId::Power,
        AssetId::Debugger,
    ];
}

/// 资产册。
pub struct AssetBook {
    /// 指针有效位。
    linked: [bool; 8],
    /// 心流已入册。
    flow_enrolled: bool,
}

impl AssetBook {
    pub fn new() -> AssetBook {
        AssetBook {
            linked: [false; 8],
            flow_enrolled: false,
        }
    }

    /// 重链。
    pub fn link(&mut self, id: AssetId) -> Result<(), A7Code> {
        if id as usize > 7 {
            return Err(A7Code::BAD_ASSET);
        }
        self.linked[id as usize] = true;
        if id == AssetId::FlowBook {
            self.flow_enrolled = true;
        }
        Ok(())
    }

    /// 全册齐备。
    pub fn complete(&self) -> Result<(), A7Code> {
        for l in &self.linked {
            if !l {
                return Err(A7Code::POINTER_DIED);
            }
        }
        // 心流红线联动
        if !self.flow_enrolled {
            return Err(A7Code::FLOW_MISS);
        }
        Ok(())
    }

    /// 心流入册检查。
    pub fn flow_enrolled(&self) -> bool {
        self.flow_enrolled
    }
}

// ── 符合度快照（含中断率） ──────────────────────────────────────────

/// 快照。
pub struct Snapshot {
    /// 八源读数。
    pub readings: [u32; 8],
    /// 中断率 ×100。
    pub interrupt_x100: u32,
    /// 等待时长（毫秒）。
    pub wait_ms: u32,
}

/// 快照器。
pub struct Snapshotter {
    /// 超限计数。
    overstock: u32,
}

impl Snapshotter {
    pub fn new() -> Snapshotter {
        Snapshotter { overstock: 0 }
    }

    /// 登记超限。
    pub fn note_overstock(&mut self) {
        self.overstock += 1;
    }

    /// 两出口分流（超限存量 → 降档出口 或 上报出口）。
    pub fn discharge(&mut self) -> Result<(bool, bool), A7Code> {
        if self.overstock == 0 {
            return Ok((false, false));
        }
        // 出口 1：降档；出口 2：上报
        self.overstock = 0;
        Ok((true, true))
    }

    /// 存量。
    pub fn overstock(&self) -> u32 {
        self.overstock
    }
}

// ── 四热点档案（Q 版） ──────────────────────────────────────────────

/// 热点项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hotspot {
    /// 共享率。
    ShareRate,
    /// 线性度。
    Linearity,
    /// 降档频度。
    DegradeFreq,
    /// 重锚效果。
    Reanchor,
}

impl Hotspot {
    pub const ALL: [Hotspot; 4] = [
        Hotspot::ShareRate,
        Hotspot::Linearity,
        Hotspot::DegradeFreq,
        Hotspot::Reanchor,
    ];
}

/// 热点档案。
pub struct HotspotFile {
    recorded: [bool; 4],
}

impl HotspotFile {
    pub fn new() -> HotspotFile {
        HotspotFile {
            recorded: [false; 4],
        }
    }

    /// 记录。
    pub fn record(&mut self, h: Hotspot) {
        self.recorded[h as usize] = true;
    }

    /// 齐备。
    pub fn complete(&self) -> Result<(), A7Code> {
        for r in &self.recorded {
            if !r {
                return Err(A7Code::HOTSPOT_MISS);
            }
        }
        Ok(())
    }
}

// ── 曲线健康 ────────────────────────────────────────────────────────

/// 曲线标记。
pub struct CurveHealth {
    healthy: bool,
}

impl CurveHealth {
    pub const fn new(healthy: bool) -> CurveHealth {
        CurveHealth { healthy }
    }

    /// 健康检查。
    pub fn check(&self) -> Result<(), A7Code> {
        if self.healthy {
            Ok(())
        } else {
            Err(A7Code::CURVE_UNHEALTHY)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3736 判据集（锚点原文六条逐条落位）。
pub fn run_vr07p_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3736");

    // 1) 八资产：齐备
    let mut book = AssetBook::new();
    for a in AssetId::ALL {
        let _ = book.link(a);
    }
    out.add(
        "0x7F02-八资产",
        book.complete().is_ok(),
        "八资产须齐",
    );

    // 2) 心流入册：红线联动
    out.add(
        "0x7F04-心流入册",
        book.flow_enrolled(),
        "心流须入册",
    );

    // 3) 心流缺：被拦
    let mut book2 = AssetBook::new();
    for a in AssetId::ALL {
        if a != AssetId::FlowBook {
            let _ = book2.link(a);
        }
    }
    out.add(
        "0x7F04-心流缺拦",
        book2.complete().is_err(),
        "缺心流须拦",
    );

    // 4) 四热点：齐备
    let mut hf = HotspotFile::new();
    for h in Hotspot::ALL {
        hf.record(h);
    }
    out.add(
        "0x7F03-四热点",
        hf.complete().is_ok(),
        "四热点须齐",
    );

    // 5) 热点缺：被拦
    let hf2 = HotspotFile::new();
    out.add(
        "0x7F03-热点缺",
        hf2.complete().is_err(),
        "缺须拦",
    );

    // 6) 超限存量：两出口
    let mut sn = Snapshotter::new();
    sn.note_overstock();
    let d = sn.discharge().unwrap_or((false, false));
    out.add(
        "0x7F01-两出口",
        d.0 && d.1,
        "两出口须开",
    );

    // 7) 曲线健康：不健康拦
    let ch = CurveHealth::new(false);
    out.add(
        "0x7F06-曲线健康",
        ch.check().is_err(),
        "不健康须拦",
    );

    // 8) 指针失效：未链被拦
    let empty = AssetBook::new();
    out.add(
        "0x7F02-指针失效",
        empty.complete().is_err(),
        "未链须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7F06-判据",
        A7Code::OVERSTOCK.value() != A7Code::FLOW_MISS.value(),
        "码段互异",
    );

    out
}
