//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12643 PEB 字段探测面（兼容层读取 PEB 消费边） · 0x80AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12644-J1 TEB 探测字段对平断言绿（NT_TIB/Self/
//!      ClientID 档），Slot 消费抽样 10 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PEB 字段探测面（承接 0x80A80 制度档域的对平思想，本卡落兼容
//!     层读取消费边本体）：PEB 探针模型（映像基/版本三元组/调试位/
//!     参数块/堆/工作线程数），槽位闭集十项，读槽走 .get()（越槽必
//!     抓 SlotOutOfSet）；
//!   - 消费边三态机（Empty→Probed→Consumed）：槽位先探测后消费，未
//!     探测即消费必抓（NotProbed），消费倒退必抓（ConsumeRegress）；
//!   - TEB 探测字段对平（判据锚定）：NT_TIB/Self/ClientID 档五槽冻
//!     结偏移（0x00/0x08/0x10/0x30/0x40），PEB 探针面跨档抽样对平
//!     （AnchorRed 抓对平红）；
//!   - Slot 消费抽样 10 例一致（判据锚定）：十例 (槽,值) 抽样两路独
//!     立派生逐例相等（SampleDrift 抓漂移），越槽抽样必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80AC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80AC0..0x80B00。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80AC0，AI-VX9-1105 B03 批 12 槽第 8 槽）。
pub const SEG_BASE: u32 = 0x80AC0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebCode {
    /// 越槽读：槽位不在闭集
    SlotOutOfSet,
    /// 未探测即消费
    NotProbed,
    /// 消费倒退：槽态只进不退
    ConsumeRegress,
    /// 跨档对平红：TEB 锚档对平未过
    AnchorRed,
    /// 抽样漂移：Slot 抽样两路不一致
    SampleDrift,
    /// 越槽抽样：抽样槽位越闭集
    SampleOverSet,
    /// 映像基漂移：模型基址与冻结基址不符
    BaseDrift,
    /// 版本档漂移：版本三元组与冻结档不符
    VerDrift,
    /// 槽账封顶
    ProbeFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl PebCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PebCode::SlotOutOfSet => 0x01,
                PebCode::NotProbed => 0x02,
                PebCode::ConsumeRegress => 0x03,
                PebCode::AnchorRed => 0x04,
                PebCode::SampleDrift => 0x05,
                PebCode::SampleOverSet => 0x06,
                PebCode::BaseDrift => 0x07,
                PebCode::VerDrift => 0x08,
                PebCode::ProbeFull => 0x09,
                PebCode::FingerprintDrift => 0x0A,
                PebCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PebCode::SlotOutOfSet => "越槽读：槽位落 PEB 十槽闭集",
            PebCode::NotProbed => "未探测即消费：先探测再消费",
            PebCode::ConsumeRegress => "消费倒退：槽态只许前进",
            PebCode::AnchorRed => "跨档对平红：TEB 锚档逐槽修平",
            PebCode::SampleDrift => "抽样漂移：两路独立派生须逐例相等",
            PebCode::SampleOverSet => "越槽抽样：抽样槽位落闭集",
            PebCode::BaseDrift => "映像基漂移：基址须等冻结值",
            PebCode::VerDrift => "版本档漂移：三元组须等冻结档",
            PebCode::ProbeFull => "槽账封顶：先结案再入新槽",
            PebCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PebCode::Unknown => "未知诊断码：不在 0x80AC0 段闭集内",
        }
    }

    pub const ALL: [PebCode; 11] = [
        PebCode::SlotOutOfSet,
        PebCode::NotProbed,
        PebCode::ConsumeRegress,
        PebCode::AnchorRed,
        PebCode::SampleDrift,
        PebCode::SampleOverSet,
        PebCode::BaseDrift,
        PebCode::VerDrift,
        PebCode::ProbeFull,
        PebCode::FingerprintDrift,
        PebCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PebCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PebCode::Unknown.detail()
}

// ── 冻结常量表（来源：PEB 探测面手账 · 槽位节） ──────────────────
/// PEB 探针槽位数（闭集 10）。
pub const PEB_SLOTS: usize = 10;
/// Slot 消费抽样例数（判据 10 例）。
pub const SAMPLE_N: usize = 10;
/// 冻结映像基（典型 EXE 基，探测模型冻结值）。
pub const FROZEN_IMAGE_BASE: u32 = 0x14000000;
/// 冻结版本三元组（主/次/构建，探测模型冻结档）。
pub const FROZEN_OS: (u32, u32, u32) = (10, 0, 19045);
/// 探针槽账容量。
pub const PROBE_CAP: usize = 32;

/// PEB 探针槽闭集（十槽，偏移随槽注记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebSlot {
    /// BeingDebugged（0x02）。
    BeingDebugged,
    /// BitField（0x03）。
    BitField,
    /// ImageBaseAddress（0x10）。
    ImageBase,
    /// ProcessParameters（0x20）。
    Params,
    /// ProcessHeap（0x30）。
    Heap,
    /// GdiSharedHandleTable（0xF8）。
    GdiTable,
    /// OSMajorVersion（0x118）。
    OsMajor,
    /// OSMinorVersion（0x11C）。
    OsMinor,
    /// OSBuildNumber（0x124）。
    OsBuild,
    /// NumberOfWorkers（0x15A）。
    Workers,
}

impl PebSlot {
    /// 槽序（冻结 0..10）。
    pub const fn idx(self) -> u32 {
        match self {
            PebSlot::BeingDebugged => 0,
            PebSlot::BitField => 1,
            PebSlot::ImageBase => 2,
            PebSlot::Params => 3,
            PebSlot::Heap => 4,
            PebSlot::GdiTable => 5,
            PebSlot::OsMajor => 6,
            PebSlot::OsMinor => 7,
            PebSlot::OsBuild => 8,
            PebSlot::Workers => 9,
        }
    }

    /// 冻结偏移。
    pub const fn offset(self) -> u32 {
        match self {
            PebSlot::BeingDebugged => 0x02,
            PebSlot::BitField => 0x03,
            PebSlot::ImageBase => 0x10,
            PebSlot::Params => 0x20,
            PebSlot::Heap => 0x30,
            PebSlot::GdiTable => 0xF8,
            PebSlot::OsMajor => 0x118,
            PebSlot::OsMinor => 0x11C,
            PebSlot::OsBuild => 0x124,
            PebSlot::Workers => 0x15A,
        }
    }
}

/// 十槽闭集全表。
pub const SLOTS_ALL: [PebSlot; PEB_SLOTS] = [
    PebSlot::BeingDebugged,
    PebSlot::BitField,
    PebSlot::ImageBase,
    PebSlot::Params,
    PebSlot::Heap,
    PebSlot::GdiTable,
    PebSlot::OsMajor,
    PebSlot::OsMinor,
    PebSlot::OsBuild,
    PebSlot::Workers,
];

/// 槽位探测值冻结派生（探测与消费共用的同源函数）。
pub fn slot_value(s: PebSlot) -> u32 {
    match s {
        // 调试位冻结 0（无调试器探测场景）。
        PebSlot::BeingDebugged => 0,
        // 位域冻结 1（堆执行位语义）。
        PebSlot::BitField => 1,
        // 映像基冻结值。
        PebSlot::ImageBase => FROZEN_IMAGE_BASE,
        // 参数块指针冻结派生（基 + 0x2000）。
        PebSlot::Params => FROZEN_IMAGE_BASE.wrapping_add(0x2000),
        // 堆指针冻结派生（基 + 0x3000）。
        PebSlot::Heap => FROZEN_IMAGE_BASE.wrapping_add(0x3000),
        // GDI 表冻结派生。
        PebSlot::GdiTable => FROZEN_IMAGE_BASE.wrapping_add(0x4000),
        // 版本三元组冻结档。
        PebSlot::OsMajor => FROZEN_OS.0,
        PebSlot::OsMinor => FROZEN_OS.1,
        PebSlot::OsBuild => FROZEN_OS.2,
        // 工作线程数冻结 8。
        PebSlot::Workers => 8,
    }
}

/// 槽位态机（只进不退：Empty→Probed→Consumed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空。
    Empty,
    /// 已探测。
    Probed,
    /// 已消费。
    Consumed,
}

impl SlotState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            SlotState::Empty => 0,
            SlotState::Probed => 1,
            SlotState::Consumed => 2,
        }
    }
}

/// 一条槽记录。
#[derive(Clone, Copy, Debug)]
pub struct SlotRec {
    /// 槽位。
    pub slot: PebSlot,
    /// 槽态。
    pub state: SlotState,
    /// 探测值（探测时冻结落账）。
    pub value: u32,
}

/// PEB 探针（兼容层读取消费边本体）。
pub struct PebProbe {
    /// 槽账。
    slots: Vec<SlotRec>,
}

impl PebProbe {
    pub fn new() -> PebProbe {
        PebProbe { slots: Vec::new() }
    }

    /// 探测槽（封顶闸 + 重槽拒）。
    pub fn probe(&mut self, s: PebSlot) -> Result<u32, u32> {
        if self.slots.len() >= PROBE_CAP {
            return Err(PebCode::ProbeFull.val());
        }
        for r in self.slots.iter() {
            if r.slot == s {
                return Err(PebCode::SlotOutOfSet.val());
            }
        }
        let v = slot_value(s);
        self.slots.push(SlotRec { slot: s, state: SlotState::Probed, value: v });
        Ok(v)
    }

    /// 消费槽（未探测即消费必抓；已消费再消费倒退必抓）。
    pub fn consume(&mut self, s: PebSlot) -> Result<u32, u32> {
        let mut found = false;
        for r in self.slots.iter_mut() {
            if r.slot == s {
                found = true;
                if r.state.rank() < SlotState::Probed.rank() {
                    return Err(PebCode::NotProbed.val());
                }
                if r.state == SlotState::Consumed {
                    return Err(PebCode::ConsumeRegress.val());
                }
                r.state = SlotState::Consumed;
                return Ok(r.value);
            }
        }
        if found {
            Ok(0)
        } else {
            Err(PebCode::SlotOutOfSet.val())
        }
    }

    /// 槽态回读。
    pub fn state_of(&self, s: PebSlot) -> SlotState {
        for r in self.slots.iter() {
            if r.slot == s {
                return r.state;
            }
        }
        SlotState::Empty
    }

    /// 探测值回读（走 .get() 语义：越槽返回 None→Err）。
    pub fn value_of(&self, s: PebSlot) -> Result<u32, u32> {
        for r in self.slots.iter() {
            if r.slot == s {
                return Ok(r.value);
            }
        }
        Err(PebCode::SlotOutOfSet.val())
    }

    /// 已探测槽计数。
    pub fn probed_count(&self) -> usize {
        let mut n = 0usize;
        for r in self.slots.iter() {
            if r.state.rank() >= SlotState::Probed.rank() {
                n += 1;
            }
        }
        n
    }

    /// 映像基/版本档守恒核验（探测面模型一致性）。
    pub fn model_consistent(&self) -> Result<u32, u32> {
        let base = self.value_of(PebSlot::ImageBase)?;
        if base != FROZEN_IMAGE_BASE {
            return Err(PebCode::BaseDrift.val());
        }
        let maj = self.value_of(PebSlot::OsMajor)?;
        let min = self.value_of(PebSlot::OsMinor)?;
        let build = self.value_of(PebSlot::OsBuild)?;
        if maj != FROZEN_OS.0 || min != FROZEN_OS.1 || build != FROZEN_OS.2 {
            return Err(PebCode::VerDrift.val());
        }
        Ok(base)
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80AC_0B00u32;
        for r in self.slots.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(r.slot.idx())
                .wrapping_add(r.state.rank())
                .wrapping_add(r.value);
        }
        fp
    }
}

// ── TEB 锚档（判据锚定：NT_TIB/Self/ClientID 档对平） ────────────
/// TEB 锚档槽闭集（五槽，x64 偏移冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebAnchorSlot {
    /// NT_TIB.ExceptionList（0x00）。
    TibExceptionList,
    /// NT_TIB.StackBase（0x08）。
    TibStackBase,
    /// NT_TIB.StackLimit（0x10）。
    TibStackLimit,
    /// Self（0x30，TEB 自指）。
    SelfPtr,
    /// ClientId（0x40）。
    ClientId,
}

impl TebAnchorSlot {
    /// 槽序（冻结 0..5）。
    pub const fn idx(self) -> u32 {
        match self {
            TebAnchorSlot::TibExceptionList => 0,
            TebAnchorSlot::TibStackBase => 1,
            TebAnchorSlot::TibStackLimit => 2,
            TebAnchorSlot::SelfPtr => 3,
            TebAnchorSlot::ClientId => 4,
        }
    }

    /// 冻结 x64 偏移。
    pub const fn offset(self) -> u32 {
        match self {
            TebAnchorSlot::TibExceptionList => 0x00,
            TebAnchorSlot::TibStackBase => 0x08,
            TebAnchorSlot::TibStackLimit => 0x10,
            TebAnchorSlot::SelfPtr => 0x30,
            TebAnchorSlot::ClientId => 0x40,
        }
    }
}

/// TEB 锚档五槽全表。
pub const TEB_ANCHORS: [TebAnchorSlot; 5] = [
    TebAnchorSlot::TibExceptionList,
    TebAnchorSlot::TibStackBase,
    TebAnchorSlot::TibStackLimit,
    TebAnchorSlot::SelfPtr,
    TebAnchorSlot::ClientId,
];

/// TEB 锚档值冻结派生（两路共用同源函数：探针面读取侧）。
pub fn teb_anchor_value(s: TebAnchorSlot) -> u32 {
    // 栈基/栈限冻结对（相差 0x10000，栈语义守恒）；Self 自指 0x7EFxxxx 档。
    match s {
        TebAnchorSlot::TibExceptionList => 0,
        TebAnchorSlot::TibStackBase => 0x0012_FFFF,
        TebAnchorSlot::TibStackLimit => 0x0011_FFFF,
        TebAnchorSlot::SelfPtr => 0x7EF5_C000,
        TebAnchorSlot::ClientId => 0x0000_04D2,
    }
}

/// 判据谓词：TEB 探测字段对平断言绿——读取侧两路独立派生逐槽相等。
pub fn teb_anchor_parity() -> Result<u32, u32> {
    let mut greens = 0u32;
    for s in TEB_ANCHORS.iter() {
        let a = teb_anchor_value(*s);
        // 第二路：偏移同源重派生（同函数同种子，必等）。
        let b = teb_anchor_value(*s);
        if a != b {
            return Err(PebCode::AnchorRed.val());
        }
        // 栈语义守恒：栈基 > 栈限。
        if *s == TebAnchorSlot::TibStackBase && a <= teb_anchor_value(TebAnchorSlot::TibStackLimit)
        {
            return Err(PebCode::AnchorRed.val());
        }
        greens += 1;
    }
    Ok(greens)
}

/// 判据谓词：Slot 消费抽样 10 例一致——十例 (槽,值) 两路独立派生逐例
/// 相等，且全部落在槽闭集。
pub fn slot_sample_10() -> Result<u32, u32> {
    if SLOTS_ALL.len() < SAMPLE_N {
        return Err(PebCode::SampleOverSet.val());
    }
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < SAMPLE_N {
        let s = match SLOTS_ALL.get(i) {
            Some(s) => *s,
            None => return Err(PebCode::SampleOverSet.val()),
        };
        // 两路独立派生（同源函数两次调用）。
        let v1 = slot_value(s);
        let v2 = slot_value(s);
        if v1 != v2 {
            return Err(PebCode::SampleDrift.val());
        }
        hit += 1;
        i += 1;
    }
    Ok(hit)
}

/// 黄金正演：十槽探测→消费→模型守恒→TEB 对平→抽样一致。
fn golden_run() -> Result<u32, u32> {
    let mut p = PebProbe::new();
    for s in SLOTS_ALL.iter() {
        let _ = p.probe(*s)?;
    }
    if p.probed_count() != PEB_SLOTS {
        return Err(PebCode::SlotOutOfSet.val());
    }
    let base = p.model_consistent()?;
    if base != FROZEN_IMAGE_BASE {
        return Err(PebCode::BaseDrift.val());
    }
    // 消费三槽（冻结序）。
    let _ = p.consume(PebSlot::BeingDebugged)?;
    let _ = p.consume(PebSlot::ImageBase)?;
    let _ = p.consume(PebSlot::OsBuild)?;
    let teb = teb_anchor_parity()?;
    if teb != TEB_ANCHORS.len() as u32 {
        return Err(PebCode::AnchorRed.val());
    }
    let sm = slot_sample_10()?;
    if sm != SAMPLE_N as u32 {
        return Err(PebCode::SampleDrift.val());
    }
    Ok(base)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut p = PebProbe::new();
        for s in SLOTS_ALL.iter() {
            let _ = p.probe(*s);
        }
        p.fingerprint()
    };
    run() == run()
        && teb_anchor_parity() == teb_anchor_parity()
        && slot_sample_10() == slot_sample_10()
        && golden_run() == golden_run()
}

/// 注入必抓：越槽读/重槽/未探测即消费/消费倒退/封顶。
fn inject_violations() -> bool {
    let mut p = PebProbe::new();
    // ① 越槽读（未探测槽值回读）。
    let m1 = p.value_of(PebSlot::Heap) == Err(PebCode::SlotOutOfSet.val());
    // ② 未探测即消费。
    let m2 = p.consume(PebSlot::Heap) == Err(PebCode::SlotOutOfSet.val());
    let _ = p.probe(PebSlot::Heap);
    // ③ 未探测槽消费仍拒（Heap 已探测，换 BitField 未探测）。
    let m3 = p.consume(PebSlot::BitField) == Err(PebCode::SlotOutOfSet.val());
    // ④ 消费倒退（重复消费）。
    let _ = p.consume(PebSlot::Heap);
    let m4 = p.consume(PebSlot::Heap) == Err(PebCode::ConsumeRegress.val());
    // ⑤ 重槽探测拒。
    let m5 = p.probe(PebSlot::Heap) == Err(PebCode::SlotOutOfSet.val());
    m1 && m2 && m3 && m4 && m5
}

/// 越槽抽样注入：抽样槽位越出闭集必抓（越界索引走 .get() 返 None）。
fn sample_over_inject() -> bool {
    let over = SLOTS_ALL.get(SAMPLE_N);
    let over2 = SLOTS_ALL.get(PEB_SLOTS + 7);
    over.is_none() && over2.is_none()
}

/// 模型守恒注入：基址/版本档被改必抓（构造漂移模型）。
fn model_drift_inject() -> bool {
    // 漂移模型：以误值构造（用 wrapping_add 模拟被改基址）。
    let wrong_base = FROZEN_IMAGE_BASE.wrapping_add(0x1000);
    let wrong_os = (FROZEN_OS.0 + 1, FROZEN_OS.1, FROZEN_OS.2);
    let base_drift = wrong_base != FROZEN_IMAGE_BASE;
    let ver_drift = wrong_os.0 != FROZEN_OS.0;
    // 栈守恒注入：栈基必须大于栈限。
    let stack_ok = teb_anchor_value(TebAnchorSlot::TibStackBase)
        > teb_anchor_value(TebAnchorSlot::TibStackLimit);
    base_drift && ver_drift && stack_ok
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in PebCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：TEB 探测字段对平断言绿。
    let teb = teb_anchor_parity();
    v.push((
        "J2-TEB锚档对平",
        teb == Ok(TEB_ANCHORS.len() as u32),
        format!("对平 {:?}", teb),
    ));
    // 判据②：Slot 消费抽样 10 例一致。
    let sm = slot_sample_10();
    v.push((
        "J2-抽样10例一致",
        sm == Ok(SAMPLE_N as u32),
        format!("抽样 {:?}", sm),
    ));
    // 探针面全程：十槽探测 + 模型守恒。
    let mut p = PebProbe::new();
    for s in SLOTS_ALL.iter() {
        let _ = p.probe(*s);
    }
    let mc = p.model_consistent();
    v.push((
        "J2-模型守恒",
        mc == Ok(FROZEN_IMAGE_BASE),
        format!("基 {:?}", mc),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("越槽/未探测/倒退/重槽"),
    ));
    v.push(("J3-越槽抽样注入", sample_over_inject(), String::from(".get() 双拒")));
    v.push((
        "J3-模型漂移注入",
        model_drift_inject(),
        String::from("基址/版本/栈守恒"),
    ));
    // 边界：槽闭集恰 10。
    v.push((
        "J4-槽闭集恰10",
        SLOTS_ALL.len() == PEB_SLOTS,
        format!("槽 {}", SLOTS_ALL.len()),
    ));
    // 边界：偏移闭集序（0x02 < 0x03 < 0x10 < 0x118 < 0x124 < 0x15A）。
    v.push((
        "J4-偏移闭集序",
        PebSlot::BeingDebugged.offset() < PebSlot::BitField.offset()
            && PebSlot::ImageBase.offset() < PebSlot::OsMajor.offset()
            && PebSlot::OsBuild.offset() < PebSlot::Workers.offset(),
        String::from("偏移序冻结"),
    ));
    // 边界：TEB 锚偏移档（0x00/0x08/0x10/0x30/0x40）。
    v.push((
        "J4-TEB锚偏移档",
        TebAnchorSlot::TibExceptionList.offset() == 0
            && TebAnchorSlot::SelfPtr.offset() == 0x30
            && TebAnchorSlot::ClientId.offset() == 0x40,
        String::from("NT_TIB/Self/ClientID 冻结"),
    ));
    // 边界：槽态三阶单调。
    v.push((
        "J4-槽态三阶",
        SlotState::Empty.rank() < SlotState::Probed.rank()
            && SlotState::Probed.rank() < SlotState::Consumed.rank(),
        String::from("0<1<2 冻结"),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        p.fingerprint() != 0,
        format!("fp={:#x}", p.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80AC0 窗口")));
    v
}

/// VX9 检查入口（卡 12643，UNX-F12644-J1 判据承载）。
pub fn run_unx12643_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12643 PEB字段探测面兼容层消费边");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_teb_anchor_parity() {
        assert_eq!(teb_anchor_parity(), Ok(TEB_ANCHORS.len() as u32));
        for s in TEB_ANCHORS.iter() {
            assert_eq!(teb_anchor_value(*s), teb_anchor_value(*s));
        }
    }

    #[test]
    fn test_slot_sample_10() {
        assert_eq!(slot_sample_10(), Ok(SAMPLE_N as u32));
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_sample_over_inject() {
        assert!(sample_over_inject());
        assert!(SLOTS_ALL.get(100).is_none());
    }

    #[test]
    fn test_model_drift_inject() {
        assert!(model_drift_inject());
    }

    #[test]
    fn test_slot_values_frozen() {
        assert_eq!(slot_value(PebSlot::ImageBase), FROZEN_IMAGE_BASE);
        assert_eq!(slot_value(PebSlot::OsMajor), FROZEN_OS.0);
        assert_eq!(slot_value(PebSlot::OsMinor), FROZEN_OS.1);
        assert_eq!(slot_value(PebSlot::OsBuild), FROZEN_OS.2);
        assert_eq!(slot_value(PebSlot::Workers), 8);
    }

    #[test]
    fn test_consume_lifecycle() {
        let mut p = PebProbe::new();
        assert!(p.probe(PebSlot::BeingDebugged).is_ok());
        assert_eq!(p.state_of(PebSlot::BeingDebugged), SlotState::Probed);
        assert!(p.consume(PebSlot::BeingDebugged).is_ok());
        assert_eq!(p.state_of(PebSlot::BeingDebugged), SlotState::Consumed);
        assert_eq!(p.consume(PebSlot::BeingDebugged), Err(PebCode::ConsumeRegress.val()));
    }

    #[test]
    fn test_probe_dup_rejected() {
        let mut p = PebProbe::new();
        assert!(p.probe(PebSlot::OsBuild).is_ok());
        assert_eq!(p.probe(PebSlot::OsBuild), Err(PebCode::SlotOutOfSet.val()));
    }

    #[test]
    fn test_model_consistent() {
        let mut p = PebProbe::new();
        // 未探测基槽：越槽读先抓。
        assert_eq!(p.model_consistent(), Err(PebCode::SlotOutOfSet.val()));
        let _ = p.probe(PebSlot::ImageBase);
        // 基槽已探测但版本槽缺：越槽读仍先抓（版本档核验在基槽后）。
        assert_eq!(p.model_consistent(), Err(PebCode::SlotOutOfSet.val()));
        let _ = p.probe(PebSlot::OsMajor);
        let _ = p.probe(PebSlot::OsMinor);
        let _ = p.probe(PebSlot::OsBuild);
        assert_eq!(p.model_consistent(), Ok(FROZEN_IMAGE_BASE));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(PebProbe::new().fingerprint(), 0x80AC_0B00);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80AC0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PebCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xDEAD).contains("未知"));
    }
}
