//! unx0830_lvt_errpmu — UNX-F0830 LVT 错误/性能计数/热温三 LVT 挂接（诊断段 0x64E0）。
//!
//! 判据原文：三 LVT 向量定编唯一且挂接成功，ESR 分账联动触发 10/10 次
//!           告警正确。
//! 设计纪律：
//!   1. 三 LVT 按 SDM Vol.3 §10.5.4：LVT Error（xAPIC 0x370）、LVT
//!      Performance-Counter（0x340）、LVT Thermal Monitor（0x330）。
//!      三向量必须互异且非保留区（0..15 禁用），向量定编账唯一性入账。
//!   2. 挂接状态机：Unwired → Wired(vector) → Masked(vector)，三槽独立，
//!      挂接成功须向量登记入闭集账且全表查重通过。
//!   3. ESR 分账联动：错误事件按七类掩码到达时，只有 Error-LVT 未屏蔽时
//!      告警出账；PMU 溢出事件走 Perf-LVT；热温越限事件走 Thermal-LVT。
//!      告警路由一事件一账，三路互不串账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x64E0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x64E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_VEC: u16 = SEG_BASE + 1;    // 三 LVT 向量定编重复
const E_VEC_LOW: u16 = SEG_BASE + 2;    // 向量 0..15 非法
const E_NOT_WIRED: u16 = SEG_BASE + 3;  // 未挂接先收事件
const E_ROUTE_MIX: u16 = SEG_BASE + 4;  // 事件路由串账（错路入账）
const E_BAL: u16 = SEG_BASE + 5;        // 告警账与事件账不平
const E_UNWIRE_BUSY: u16 = SEG_BASE + 6;// 已挂接再挂接（重挂须先解挂）
const E_MASK_PASS: u16 = SEG_BASE + 7;  // masked 期告警仍出账（判据失守）

/// 三 LVT 槽位（SDM xAPIC 偏移锚定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LvtSlot {
    Thermal = 0, // xAPIC 0x330
    PerfCounter = 1, // xAPIC 0x340
    Error = 2,   // xAPIC 0x370
}

pub const SLOT_COUNT: usize = 3;

/// 挂接状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireState {
    Unwired,
    Wired,
    Masked,
}

/// 单槽 LVT：向量 + 状态 + 各路告警账。
pub struct LvtEntry {
    pub slot: LvtSlot,
    pub vector: u16,      // 0x0000 表未定编（u16 承载闭集校验余量）
    pub state: WireState,
    pub alarms: u32,      // 本槽告警出账数
    pub suppressed: u32,  // masked 期事件被压计数
}

impl LvtEntry {
    fn new(slot: LvtSlot) -> LvtEntry {
        LvtEntry {
            slot,
            vector: 0,
            state: WireState::Unwired,
            alarms: 0,
            suppressed: 0,
        }
    }
}

/// 三 LVT 挂接枢纽：向量定编账 + 事件路由 + 告警分账。
pub struct LvtFabric {
    pub slots: [LvtEntry; SLOT_COUNT],
    pub wired_total: u32,   // 挂接成功计数（判据「挂接成功」面）
    pub events_total: u32,  // 到达事件总数
}

impl LvtFabric {
    pub fn new() -> LvtFabric {
        LvtFabric {
            slots: [
                LvtEntry::new(LvtSlot::Thermal),
                LvtEntry::new(LvtSlot::PerfCounter),
                LvtEntry::new(LvtSlot::Error),
            ],
            wired_total: 0,
            events_total: 0,
        }
    }

    fn slot_idx(&self, s: LvtSlot) -> usize {
        s as usize
    }

    /// 挂接：向量定编 + 全表查重 + 状态迁移 Unwired→Wired。
    pub fn wire(&mut self, slot: LvtSlot, vector: u8) -> Result<(), u16> {
        if vector < 0x10 {
            return Err(E_VEC_LOW);
        }
        // 全表查重：三向量互异（判据「定编唯一」）
        for e in &self.slots {
            if e.state != WireState::Unwired && e.vector == vector as u16 {
                return Err(E_DUP_VEC);
            }
        }
        let i = self.slot_idx(slot);
        if self.slots[i].state != WireState::Unwired {
            return Err(E_UNWIRE_BUSY);
        }
        self.slots[i].vector = vector as u16;
        self.slots[i].state = WireState::Wired;
        self.wired_total += 1;
        Ok(())
    }

    /// 解挂/再挂与屏蔽开关：Wired↔Masked 互切。
    pub fn set_masked(&mut self, slot: LvtSlot, masked: bool) -> Result<(), u16> {
        let i = self.slot_idx(slot);
        match self.slots[i].state {
            WireState::Unwired => Err(E_NOT_WIRED),
            WireState::Wired => {
                if masked {
                    self.slots[i].state = WireState::Masked;
                }
                Ok(())
            }
            WireState::Masked => {
                if !masked {
                    self.slots[i].state = WireState::Wired;
                }
                Ok(())
            }
        }
    }

    /// 事件路由：错误类事件（七类掩码非零）→ Error 槽；
    /// PMU 溢出（pmu_over 置位）→ PerfCounter 槽；热温越限 → Thermal 槽。
    /// 三路掩码互斥（一回合一事件），串账即拒。
    pub fn route_event(
        &mut self,
        esr_mask: u8,
        pmu_over: bool,
        thermal_over: bool,
    ) -> Result<(), u16> {
        let mut kinds = 0u32;
        if esr_mask != 0 {
            kinds += 1;
        }
        if pmu_over {
            kinds += 1;
        }
        if thermal_over {
            kinds += 1;
        }
        if kinds != 1 {
            return Err(E_ROUTE_MIX); // 零类或多类同回合属路由面串账
        }
        self.events_total += 1;
        let (idx, alarm) = if esr_mask != 0 {
            (LvtSlot::Error as usize, true)
        } else if pmu_over {
            (LvtSlot::PerfCounter as usize, true)
        } else {
            (LvtSlot::Thermal as usize, true)
        };
        if !alarm {
            return Err(E_ROUTE_MIX);
        }
        match self.slots[idx].state {
            WireState::Wired => {
                self.slots[idx].alarms += 1;
                Ok(())
            }
            WireState::Masked => {
                self.slots[idx].suppressed += 1;
                Ok(()) // masked 期事件入压账，不算告警（判据联动面的负样本）
            }
            WireState::Unwired => Err(E_NOT_WIRED),
        }
    }

    /// 告警守恒：Σ(alarms + suppressed) == events_total（事件不灭失）。
    pub fn alarm_conserved(&self) -> bool {
        let mut s = 0u32;
        for e in &self.slots {
            s += e.alarms + e.suppressed;
        }
        s == self.events_total
    }

    /// 三向量互异复核（判据「定编唯一」终审）。
    pub fn vectors_unique(&self) -> bool {
        for a in 0..SLOT_COUNT {
            for b in (a + 1)..SLOT_COUNT {
                if self.slots[a].vector == self.slots[b].vector {
                    return false;
                }
            }
        }
        true
    }

    /// ESR 分账联动读出口：Error 槽告警数（判据「联动触发 10/10」读面）。
    pub fn error_alarms(&self) -> u32 {
        self.slots[LvtSlot::Error as usize].alarms
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_VEC => "三 LVT 向量定编重复（唯一性失守）",
        E_VEC_LOW => "向量 0..15 为体系结构保留区",
        E_NOT_WIRED => "未挂接先收事件或对未挂接槽操作",
        E_ROUTE_MIX => "事件路由串账（零类或多类同回合）",
        E_BAL => "告警账与事件账不平（事件灭失）",
        E_UNWIRE_BUSY => "已挂接槽重挂（须先解挂）",
        E_MASK_PASS => "masked 期告警仍出账（判据失守）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三槽挂接 + 唯一性 + 10/10 ESR 联动告警 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三槽挂接，向量 0x31/0x32/0x33 定编唯一，挂接成功计数 3。
    let mut fab = LvtFabric::new();
    fab.wire(LvtSlot::Error, 0x31)?;
    fab.wire(LvtSlot::PerfCounter, 0x32)?;
    fab.wire(LvtSlot::Thermal, 0x33)?;
    if fab.wired_total != 3 || !fab.vectors_unique() {
        return Err(E_DUP_VEC);
    }
    hits += 1;

    // R2：ESR 分账联动 10/10——十次错误事件全部进 Error 槽告警（判据原文）。
    for _ in 0..10 {
        fab.route_event(0x7F, false, false)?;
    }
    if fab.error_alarms() != 10 || !fab.alarm_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：三路独立分账——PMU 3 次、热温 2 次各归各槽，Error 账不动。
    fab.route_event(0, true, false)?;
    fab.route_event(0, true, false)?;
    fab.route_event(0, true, false)?;
    fab.route_event(0, false, true)?;
    fab.route_event(0, false, true)?;
    if fab.slots[LvtSlot::PerfCounter as usize].alarms != 3 {
        return Err(E_ROUTE_MIX);
    }
    if fab.slots[LvtSlot::Thermal as usize].alarms != 2 {
        return Err(E_ROUTE_MIX);
    }
    if fab.error_alarms() != 10 {
        return Err(E_ROUTE_MIX); // Error 槽不被邻路串账
    }
    if !fab.alarm_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：masked 压账——Error 槽屏蔽后 5 次错误事件入压账不入告警。
    fab.set_masked(LvtSlot::Error, true)?;
    for _ in 0..5 {
        fab.route_event(0x40, false, false)?;
    }
    if fab.error_alarms() != 10 {
        return Err(E_MASK_PASS);
    }
    if fab.slots[LvtSlot::Error as usize].suppressed != 5 {
        return Err(E_BAL);
    }
    if !fab.alarm_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：解屏蔽恢复告警 2 次（账面联动可续）。
    fab.set_masked(LvtSlot::Error, false)?;
    for _ in 0..2 {
        fab.route_event(0x10, false, false)?;
    }
    if fab.error_alarms() != 12 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：向量定编重复 → E_DUP_VEC。
    if fab.wire(LvtSlot::Thermal, 0x31) != Err(E_DUP_VEC) {
        return Err(E_DUP_VEC);
    }
    hits += 1;

    // I2：保留区向量 → E_VEC_LOW。
    let mut fab2 = LvtFabric::new();
    if fab2.wire(LvtSlot::Error, 0x0F) != Err(E_VEC_LOW) {
        return Err(E_VEC_LOW);
    }
    hits += 1;

    // I3：未挂接先收事件 → E_NOT_WIRED。
    if fab2.route_event(0x7F, false, false) != Err(E_NOT_WIRED) {
        return Err(E_NOT_WIRED);
    }
    hits += 1;

    // I4：路由串账——双类同回合 → E_ROUTE_MIX。
    if fab.route_event(0x7F, true, false) != Err(E_ROUTE_MIX) {
        return Err(E_ROUTE_MIX);
    }
    if fab.route_event(0, false, false) != Err(E_ROUTE_MIX) {
        return Err(E_ROUTE_MIX); // 零事件回合同拒
    }
    hits += 1;

    // I5：重挂 → E_UNWIRE_BUSY。
    if fab.wire(LvtSlot::Error, 0x40) != Err(E_UNWIRE_BUSY) {
        return Err(E_UNWIRE_BUSY);
    }
    hits += 1;

    // I6：对未挂接槽设屏蔽 → E_NOT_WIRED。
    if fab2.set_masked(LvtSlot::Thermal, true) != Err(E_NOT_WIRED) {
        return Err(E_NOT_WIRED);
    }
    hits += 1;

    Ok(hits)
}
