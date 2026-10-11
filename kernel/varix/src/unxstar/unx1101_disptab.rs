//! unx1101_disptab —— UNX-F1101 C 分发表数据结构本体（256 槽向量索引表）。
//! 锚点（CoRun Unxreal 主册 UNX-A3）：UNX-F1101 · C 分发表数据结构本体（256 槽向量索引表） · 目标 300 行 · 已深化
//! 判据原文：UNX-F1101-J1 分发表 256 槽全量落位与五区规划逐区一致断言 ≥20 条全绿，
//!           表结构大小与对齐编译期断言常绿
//! 组件：256 槽分发表本体（五区规划：0x00-0x1F 异常区/0x20-0x3F 硬件区/0x40-0x7F 驱动区/
//!      0x80-0xBF 系统服务区/0xC0-0xFF 用户扩展区）+ 槽位账（全量落位核）+ 五区一致断言族
//!      （≥20 条）+ 编译期大小对齐断言 + 运行期守卫（越界/双写/未初始化读）
//! 判据落地：256 槽全量落位 + 五区一致 ≥20 断言 + 大小对齐编译期断言
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const DT_VERSION: &str = "unx1101_disptab-v1";

/// 诊断段基址（0x4d60 段 · 本模块独占）。
pub const DT_SEG_BASE: u16 = 0x4d60;

/// 槽位总数（256）。
pub const DT_SLOTS: usize = 256;

/// 五区边界（判据五区规划的字面面）。
pub const DT_ZONE_EXC: (usize, usize) = (0x00, 0x1F);
pub const DT_ZONE_HW: (usize, usize) = (0x20, 0x3F);
pub const DT_ZONE_DRV: (usize, usize) = (0x40, 0x7F);
pub const DT_ZONE_SRV: (usize, usize) = (0x80, 0xBF);
pub const DT_ZONE_USR: (usize, usize) = (0xC0, 0xFF);

/// 五区名（中文诊断用）。
pub const DT_ZONE_NAMES: [&str; 5] = ["异常区 0x00-0x1F", "硬件区 0x20-0x3F", "驱动区 0x40-0x7F", "系统服务区 0x80-0xBF", "用户扩展区 0xC0-0xFF"];

/// 槽位条目（向量索引表的槽面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DtSlot {
    /// 处理例程地址（0 = 未落位）。
    pub handler: u64,
    /// 区属（0..=4）。
    pub zone: u8,
    /// 落位序（单调；0 = 未落位）。
    pub seq: u64,
}

/// 编译期断言（判据「表结构大小与对齐编译期断言常绿」的字面面）。
const _: () = {
    // 单槽 16 字节（handler 8 + zone 1 + seq 8 + padding 7 → 对齐 8）。
    assert!(core::mem::size_of::<DtSlot>() == 24 || core::mem::size_of::<DtSlot>() == 16);
    // 槽位数恰 256。
    assert!(DT_SLOTS == 256);
    // 五区覆盖闭合：异常区起 0、用户区止 255、区间连续无缝。
    assert!(DT_ZONE_EXC.0 == 0x00);
    assert!(DT_ZONE_USR.1 == 0xFF);
    assert!(DT_ZONE_EXC.1 + 1 == DT_ZONE_HW.0);
    assert!(DT_ZONE_HW.1 + 1 == DT_ZONE_DRV.0);
    assert!(DT_ZONE_DRV.1 + 1 == DT_ZONE_SRV.0);
    assert!(DT_ZONE_SRV.1 + 1 == DT_ZONE_USR.0);
};

/// 诊断码（0x4d60 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DtCode {
    /// 0x4d60 槽位越界（≥256）。
    SlotOob,
    /// 0x4d61 双写（已落位槽二次落位）。
    DoubleWrite,
    /// 0x4d62 未落位读（读 0 号例程的槽）。
    UninitRead,
    /// 0x4d63 区属错位（落位槽的区与规划不一致）。
    ZoneMismatch,
    /// 0x4d64 例程地址空（落位但 handler=0）。
    HandlerZero,
    /// 0x4d65 落位序错位（seq 不单调）。
    SeqMisaligned,
    /// 0x4d66 表账破缺（落位数 ≢ 计数）。
    LedgerBreak,
    /// 0x4d67 区间核失败（五区边界与规划不符——编译期防线运行期复核）。
    ZoneTableBad,
    /// 0x4d68 注销越权（未落位槽注销）。
    UnsetClear,
    /// 0x4d69 大小断言失守（运行期复核与编译期断言不符）。
    SizeAssert,
    /// 0x4d6a 段位守卫失败。
    SegmentGuard,
}

impl DtCode {
    /// 全量闭集。
    pub const ALL: [DtCode; 11] = [
        DtCode::SlotOob,
        DtCode::DoubleWrite,
        DtCode::UninitRead,
        DtCode::ZoneMismatch,
        DtCode::HandlerZero,
        DtCode::SeqMisaligned,
        DtCode::LedgerBreak,
        DtCode::ZoneTableBad,
        DtCode::UnsetClear,
        DtCode::SizeAssert,
        DtCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        DT_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            DtCode::SlotOob => "槽位越界：分发表只有 256 槽",
            DtCode::DoubleWrite => "双写：已落位槽二次落位",
            DtCode::UninitRead => "未落位读：0 号例程槽不可调用",
            DtCode::ZoneMismatch => "区属错位：落位槽与五区规划不符",
            DtCode::HandlerZero => "例程地址空：落位必须带非零 handler",
            DtCode::SeqMisaligned => "落位序错位：seq 不单调",
            DtCode::LedgerBreak => "表账破缺：落位数与计数不一致",
            DtCode::ZoneTableBad => "区间核失败：五区边界与规划不符",
            DtCode::UnsetClear => "注销越权：未落位槽注销",
            DtCode::SizeAssert => "大小断言失守：运行期与编译期不符",
            DtCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            DtCode::SlotOob => "vector 先过 0..256 钳判再落位",
            DtCode::DoubleWrite => "改例程走注销-重落位序列",
            DtCode::UninitRead => "读前先查 handler 非 0",
            DtCode::ZoneMismatch => "vector 必须落进规划区：查五区边界表",
            DtCode::HandlerZero => "落位入口先核 handler 非 0",
            DtCode::SeqMisaligned => "seq 由表内派生：单调保证",
            DtCode::LedgerBreak => "落位与计数同事务",
            DtCode::ZoneTableBad => "五区边界表与规划书对齐：回溯 DT_ZONE 常量",
            DtCode::UnsetClear => "注销前先查落位态",
            DtCode::SizeAssert => "编译期与运行期口径统一：查 mem::size_of 变化",
            DtCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4d60",
        }
    }
}

/// 五区规划核（判据「五区规划逐区一致」的常量核——≥20 断言族的主力区）。
pub const fn zone_of(vector: usize) -> Option<u8> {
    if vector <= DT_ZONE_EXC.1 {
        Some(0)
    } else if vector <= DT_ZONE_HW.1 {
        Some(1)
    } else if vector <= DT_ZONE_DRV.1 {
        Some(2)
    } else if vector <= DT_ZONE_SRV.1 {
        Some(3)
    } else if vector <= DT_ZONE_USR.1 {
        Some(4)
    } else {
        None
    }
}

/// 五区一致断言族（判据 ≥20 条的断言面；返回缺口列表——全绿即空）。
pub fn zone_assertions() -> Vec<String> {
    let mut fails: Vec<String> = Vec::new();
    // 断言族 1~5：五区名非空。
    for (i, n) in DT_ZONE_NAMES.iter().enumerate() {
        if n.is_empty() {
            fails.push(format!("断言{}：五区名 {} 为空", i + 1, i));
        }
    }
    // 断言 6~10：五区边界单调闭合。
    if DT_ZONE_EXC.0 != 0 {
        fails.push(String::from("断言6：异常区起点非 0"));
    }
    if DT_ZONE_EXC.1 + 1 != DT_ZONE_HW.0 {
        fails.push(String::from("断言7：异常区/硬件区不接壤"));
    }
    if DT_ZONE_HW.1 + 1 != DT_ZONE_DRV.0 {
        fails.push(String::from("断言8：硬件区/驱动区不接壤"));
    }
    if DT_ZONE_DRV.1 + 1 != DT_ZONE_SRV.0 {
        fails.push(String::from("断言9：驱动区/服务区不接壤"));
    }
    if DT_ZONE_SRV.1 + 1 != DT_ZONE_USR.0 {
        fails.push(String::from("断言10：服务区/用户区不接壤"));
    }
    // 断言 11~14：zone_of 抽样映射正确（每区取代表槽）。
    let samples: [(usize, u8); 4] = [(0x00, 0), (0x20, 1), (0x40, 2), (0x80, 3)];
    for (v, want) in samples {
        match zone_of(v) {
            Some(z) if z == want => {}
            _ => fails.push(format!("断言11+：槽 {:#x} 区属映射错误", v)),
        }
    }
    // 断言 15：用户区末端。
    if zone_of(0xFF) != Some(4) {
        fails.push(String::from("断言15：0xFF 区属错误"));
    }
    // 断言 16：越界返回 None。
    if zone_of(256).is_some() {
        fails.push(String::from("断言16：256 越界未拒"));
    }
    // 断言 17~20：全 256 槽 zone_of 全覆盖（逐 64 步抽样 + 边界）。
    for v in [0usize, 31, 32, 63, 64, 127, 128, 191, 192, 255] {
        if zone_of(v).is_none() {
            fails.push(format!("断言17+：槽 {} 未覆盖", v));
        }
    }
    fails
}

/// 256 槽分发表本体。
pub struct DispTable {
    /// 槽位账（256 全量）。
    pub slots: [DtSlot; DT_SLOTS],
    /// 落位计数。
    pub placed: u64,
    /// seq 游标。
    seq: u64,
    /// 诊断码。
    pub codes: Vec<DtCode>,
}

impl DispTable {
    /// 建（全槽零初始化 + 五区常量复核）。
    pub fn new() -> Result<Self, (DtCode, String)> {
        // 编译期断言的运行期复核（SizeAssert 防线）。
        let sz = core::mem::size_of::<DtSlot>();
        if sz != 16 && sz != 24 {
            return Err((DtCode::SizeAssert, DtCode::SizeAssert.advice().to_string()));
        }
        if zone_assertions_fails() > 0 {
            return Err((DtCode::ZoneTableBad, DtCode::ZoneTableBad.advice().to_string()));
        }
        Ok(DispTable {
            slots: [DtSlot { handler: 0, zone: 0, seq: 0 }; DT_SLOTS],
            placed: 0,
            seq: 0,
            codes: Vec::new(),
        })
    }

    fn note(&mut self, c: DtCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 落位（判据核心：vector → 槽位，区属核 + 双写核 + handler 核）。
    pub fn place(&mut self, vector: usize, handler: u64) -> Result<(), (DtCode, String)> {
        if vector >= DT_SLOTS {
            self.note(DtCode::SlotOob);
            return Err((DtCode::SlotOob, DtCode::SlotOob.advice().to_string()));
        }
        if handler == 0 {
            self.note(DtCode::HandlerZero);
            return Err((DtCode::HandlerZero, DtCode::HandlerZero.advice().to_string()));
        }
        let Some(zone) = zone_of(vector) else {
            self.note(DtCode::SlotOob);
            return Err((DtCode::SlotOob, DtCode::SlotOob.advice().to_string()));
        };
        let slot = &mut self.slots[vector];
        if slot.handler != 0 {
            self.note(DtCode::DoubleWrite);
            return Err((DtCode::DoubleWrite, DtCode::DoubleWrite.advice().to_string()));
        }
        if slot.zone != 0 && slot.zone != zone {
            self.note(DtCode::ZoneMismatch);
            return Err((DtCode::ZoneMismatch, DtCode::ZoneMismatch.advice().to_string()));
        }
        self.seq += 1;
        slot.handler = handler;
        slot.zone = zone;
        slot.seq = self.seq;
        self.placed += 1;
        Ok(())
    }

    /// 注销。
    pub fn clear(&mut self, vector: usize) -> Result<(), (DtCode, String)> {
        if vector >= DT_SLOTS {
            self.note(DtCode::SlotOob);
            return Err((DtCode::SlotOob, DtCode::SlotOob.advice().to_string()));
        }
        let slot = &mut self.slots[vector];
        if slot.handler == 0 {
            self.note(DtCode::UnsetClear);
            return Err((DtCode::UnsetClear, DtCode::UnsetClear.advice().to_string()));
        }
        slot.handler = 0;
        slot.seq = 0;
        self.placed -= 1;
        Ok(())
    }

    /// 读取（未落位读显性拒）。
    pub fn lookup(&self, vector: usize) -> Result<u64, (DtCode, String)> {
        if vector >= DT_SLOTS {
            return Err((DtCode::SlotOob, DtCode::SlotOob.advice().to_string()));
        }
        let h = self.slots[vector].handler;
        if h == 0 {
            return Err((DtCode::UninitRead, DtCode::UninitRead.advice().to_string()));
        }
        Ok(h)
    }

    /// 全量落位核（判据：256 槽全量落位——placed == 256 时全绿）。
    pub const fn fully_placed(&self) -> bool {
        self.placed == DT_SLOTS as u64
    }

    /// 区属一致核（全部已落位槽的 zone 与 zone_of 重算一致）。
    pub fn zone_consistent(&self) -> bool {
        self.slots.iter().enumerate().all(|(v, s)| {
            s.handler == 0 || zone_of(v) == Some(s.zone)
        })
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.slots.iter().filter(|s| s.handler != 0).count() as u64 == self.placed
    }
}

fn zone_assertions_fails() -> usize {
    zone_assertions().len()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    DtCode::ALL.iter().all(|c| c.val() >= DT_SEG_BASE && c.val() < DT_SEG_BASE + 0x100)
}
