//! unx984_vecisolate —— UNX-F0984 向量级隔离降级（单向量屏蔽与设备通知）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0984 · 向量级隔离降级 · 目标 320 行 · 已深化
//! 判据原文：UNX-F0984-J1 单向量屏蔽正确（指定向量屏蔽后该向量不再分发 10/10 次），
//!           设备通知送达（被屏蔽向量的设备收到降级通知 10/10 次），恢复解屏蔽 10/10 次。
//! 前驱：UNX-F0983 unx983_stormgrade（风暴分级）。本模块在 L3/L4 时按向量隔离。
//! 组件：向量屏蔽位图（256 向量 → 位图，置位即屏蔽）
//!      + 分发闸门（被屏蔽向量返回 Blocked，不再进 handler 链）
//!      + 设备通知队列（屏蔽/解屏蔽各发一条带向量+原因的降级/恢复通知）
//!      + 屏蔽台账（向量/原因/起 tick/止 tick/状态，闭环可查）
//!      + 恢复评估（解屏蔽双写幂等，位图与台账一致）
//!      + 守恒账（屏蔽数 + 活跃数 ≡ 注册向量总数）
//! 判据落地：单向量屏蔽 10/10 + 通知送达 10/10 + 恢复 10/10 + 守恒。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const VI_VERSION: &str = "unx984_vecisolate-v1";

/// 诊断段基址（0x7A10 段 · 本模块独占，grep 同目录核对无撞）。
pub const VI_SEG_BASE: u16 = 0x7A10;

/// 向量总数（x86 IDT 256 项）。
pub const VI_VECTOR_MAX: usize = 256;

/// 屏蔽台账上限。
pub const VI_MASK_ROWS: usize = 256;

/// 向量屏蔽位图（256 位 = 4×u64）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MaskMap {
    bits: [u64; 4],
}

impl MaskMap {
    /// 空位图。
    pub const fn new() -> Self {
        MaskMap { bits: [0; 4] }
    }

    /// 置位（屏蔽向量 v，越界返回 false）。
    pub fn set(&mut self, v: usize) -> bool {
        if v >= VI_VECTOR_MAX {
            return false;
        }
        self.bits[v / 64] |= 1u64 << (v % 64);
        true
    }

    /// 清位（解屏蔽向量 v，越界返回 false）。
    pub fn clear(&mut self, v: usize) -> bool {
        if v >= VI_VECTOR_MAX {
            return false;
        }
        self.bits[v / 64] &= !(1u64 << (v % 64));
        true
    }

    /// 查询向量是否被屏蔽。
    pub const fn is_set(&self, v: usize) -> bool {
        if v >= VI_VECTOR_MAX {
            return false;
        }
        (self.bits[v / 64] >> (v % 64)) & 1 == 1
    }

    /// 被屏蔽向量计数（popcount 求和）。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < 4 {
            n += self.bits[i].count_ones() as usize;
            i += 1;
        }
        n
    }

    /// 是否含向量（越界恒 false）。
    pub const fn contains(&self, v: usize) -> bool {
        self.is_set(v)
    }
}

impl Default for MaskMap {
    fn default() -> Self {
        MaskMap::new()
    }
}

/// 通知类型（降级/恢复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyKind {
    /// 屏蔽降级通知。
    Isolate,
    /// 恢复解屏蔽通知。
    Restore,
}

impl NotifyKind {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            NotifyKind::Isolate => "屏蔽降级",
            NotifyKind::Restore => "恢复解屏蔽",
        }
    }
}

/// 设备通知（带向量+原因，闭环可查）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notify {
    /// 向量。
    pub vector: usize,
    /// 通知类型。
    pub kind: NotifyKind,
    /// 原因文案。
    pub reason: String,
    /// 发出 tick。
    pub tick: u64,
    /// 是否已送达（幂等去重）。
    pub delivered: bool,
}

/// 屏蔽台账行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MaskRow {
    /// 向量。
    pub vector: usize,
    /// 屏蔽原因。
    pub reason: String,
    /// 起 tick。
    pub since: u64,
    /// 止 tick（未恢复为 0）。
    pub until: u64,
    /// 当前是否屏蔽。
    pub active: bool,
}

/// 分发结果（闸门判定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateOutcome {
    /// 放行（向量未被屏蔽）。
    Pass,
    /// 拦截（向量被屏蔽，不再进 handler 链）。
    Blocked,
}

impl GateOutcome {
    /// 是否放行。
    pub const fn passed(self) -> bool {
        match self {
            GateOutcome::Pass => true,
            GateOutcome::Blocked => false,
        }
    }
}

/// 诊断码（0x7A10 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViCode {
    /// 0x7A10 向量越界（≥ 256）。
    VectorOob,
    /// 0x7A11 重复屏蔽（已屏蔽向量再次屏蔽）。
    DoubleMask,
    /// 0x7A12 解屏蔽未屏蔽向量（状态违例）。
    UnmaskIdle,
    /// 0x7A13 闸门破缺（被屏蔽向量竟放行）。
    GateBreak,
    /// 0x7A14 通知未送达（屏蔽但无通知）。
    NotifyMissing,
    /// 0x7A15 守恒破缺（屏蔽+活跃 ≢ 注册总数）。
    ConserveBreak,
    /// 0x7A16 台账满（屏蔽行超上限）。
    RowFull,
    /// 0x7A17 通知重复送达（幂等破缺）。
    NotifyDup,
    /// 0x7A18 恢复未成（位图与台账状态不一致）。
    RestoreGap,
    /// 0x7A19 向量复用违例（解屏蔽后无间隔再屏蔽）。
    ReuseBad,
    /// 0x7A1a 原因文案空（schema 违例）。
    ReasonEmpty,
    /// 0x7A1b 段位守卫失败。
    SegmentGuard,
}

impl ViCode {
    /// 全量闭集。
    pub const ALL: [ViCode; 12] = [
        ViCode::VectorOob,
        ViCode::DoubleMask,
        ViCode::UnmaskIdle,
        ViCode::GateBreak,
        ViCode::NotifyMissing,
        ViCode::ConserveBreak,
        ViCode::RowFull,
        ViCode::NotifyDup,
        ViCode::RestoreGap,
        ViCode::ReuseBad,
        ViCode::ReasonEmpty,
        ViCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        VI_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ViCode::VectorOob => "向量越界：必须落在 [0, 256)",
            ViCode::DoubleMask => "重复屏蔽：该向量已处于屏蔽态",
            ViCode::UnmaskIdle => "解屏蔽未屏蔽向量：状态违例",
            ViCode::GateBreak => "闸门破缺：被屏蔽向量竟被放行",
            ViCode::NotifyMissing => "通知未送达：屏蔽动作缺少降级通知",
            ViCode::ConserveBreak => "守恒破缺：屏蔽数与活跃数之和不等于注册总数",
            ViCode::RowFull => "台账满：屏蔽行数超上限",
            ViCode::NotifyDup => "通知重复送达：幂等纪律破缺",
            ViCode::RestoreGap => "恢复未成：位图与台账状态不一致",
            ViCode::ReuseBad => "向量复用违例：解屏蔽后须间隔才可再屏蔽",
            ViCode::ReasonEmpty => "原因文案空：schema 要求非空",
            ViCode::SegmentGuard => "段位守卫失败",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ViCode::VectorOob => "向量号限制在 0~255，越界拒收",
            ViCode::DoubleMask => "屏蔽前先查位图 is_set，避免重复",
            ViCode::UnmaskIdle => "仅对已屏蔽向量执行解屏蔽",
            ViCode::GateBreak => "检查分发闸门是否先查位图再进 handler",
            ViCode::NotifyMissing => "每次屏蔽同步入通知队列",
            ViCode::ConserveBreak => "核对注册向量总数与屏蔽/活跃分类",
            ViCode::RowFull => "及时归档旧屏蔽行",
            ViCode::NotifyDup => "通知按 (向量,类型) 去重，幂等送达",
            ViCode::RestoreGap => "解屏蔽须同时清位图与更新台账",
            ViCode::ReuseBad => "解屏蔽后至少隔一个 tick 才可再屏蔽",
            ViCode::ReasonEmpty => "屏蔽必须带非空原因文案",
            ViCode::SegmentGuard => "段位守卫失败：诊断码越界，检查 SEG_BASE",
        }
    }

    /// 诊断元组。
    pub const fn detail(self) -> (u16, &'static str, &'static str) {
        (self.val(), self.text(), self.advice())
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    ViCode::ALL.iter().all(|c| c.val() >= VI_SEG_BASE && c.val() < VI_SEG_BASE + 0x100)
}

/// 向量隔离引擎（F0984 核心）：屏蔽位图 + 台账 + 通知队列 + 守恒账。
pub struct VecIsolate {
    /// 逻辑 tick。
    pub tick: u64,
    /// 屏蔽位图（置位即屏蔽）。
    pub map: MaskMap,
    /// 屏蔽台账（向量/原因/起止 tick/状态）。
    pub rows: Vec<MaskRow>,
    /// 设备通知队列（屏蔽/恢复各一条）。
    pub notifies: Vec<Notify>,
    /// 注册向量总数（守恒账分母）。
    pub registered: usize,
    /// 屏蔽动作计数。
    pub mask_count: u64,
    /// 解屏蔽动作计数。
    pub unmask_count: u64,
    /// 闸门放行计数。
    pub gate_pass: u64,
    /// 闸门拦截计数。
    pub gate_block: u64,
    /// 最近一次诊断码（无则 None）。
    pub last: Option<ViCode>,
}

impl Default for VecIsolate {
    fn default() -> Self {
        VecIsolate::new()
    }
}

impl VecIsolate {
    /// 空引擎。
    pub fn new() -> Self {
        VecIsolate {
            tick: 0,
            map: MaskMap::new(),
            rows: Vec::new(),
            notifies: Vec::new(),
            registered: 0,
            mask_count: 0,
            unmask_count: 0,
            gate_pass: 0,
            gate_block: 0,
            last: None,
        }
    }

    /// 记录诊断码。
    fn note(&mut self, c: ViCode) {
        self.last = Some(c);
    }

    /// 推进逻辑 tick。
    pub fn step(&mut self) {
        self.tick += 1;
    }

    /// 注册向量（纳入守恒账分母，不改变屏蔽态）。
    pub fn register(&mut self, n: usize) -> Result<(), (ViCode, String)> {
        self.registered = self.registered.saturating_add(n);
        Ok(())
    }

    /// 入队一条通知（幂等：同 (向量,类型) 未送达则不重复入）。
    fn enqueue_notify(&mut self, vector: usize, kind: NotifyKind, reason: &str) -> Result<(), (ViCode, String)> {
        let dup = self
            .notifies
            .iter()
            .any(|n| n.vector == vector && n.kind == kind && !n.delivered);
        if dup {
            self.note(ViCode::NotifyDup);
            return Err((ViCode::NotifyDup, ViCode::NotifyDup.advice().to_string()));
        }
        if self.notifies.len() >= VI_MASK_ROWS {
            self.note(ViCode::RowFull);
            return Err((ViCode::RowFull, ViCode::RowFull.advice().to_string()));
        }
    /// 屏蔽单个向量（F0984 主操作）。
    pub fn isolate(&mut self, vector: usize, reason: &str) -> Result<(), (ViCode, String)> {
        if vector >= VI_VECTOR_MAX {
            self.note(ViCode::VectorOob);
            return Err((ViCode::VectorOob, ViCode::VectorOob.advice().to_string()));
        }
        if reason.is_empty() {
            self.note(ViCode::ReasonEmpty);
            return Err((ViCode::ReasonEmpty, ViCode::ReasonEmpty.advice().to_string()));
        }
        if self.map.is_set(vector) {
            self.note(ViCode::DoubleMask);
            return Err((ViCode::DoubleMask, ViCode::DoubleMask.advice().to_string()));
        }
        if self.rows.len() >= VI_MASK_ROWS {
            self.note(ViCode::RowFull);
            return Err((ViCode::RowFull, ViCode::RowFull.advice().to_string()));
        }
        // 置位 + 台账 + 通知（缺一即违例）。
        self.map.set(vector);
        self.rows.push(MaskRow {
            vector,
            reason: reason.to_string(),
            since: self.tick,
            until: 0,
            active: true,
        });
        self.mask_count += 1;
        // 通知必送达：缺通知即 NotifyMissing。
        self.enqueue_notify(vector, NotifyKind::Isolate, reason)?;
        Ok(())
    }

    /// 解屏蔽单个向量（恢复）。
    pub fn restore(&mut self, vector: usize) -> Result<(), (ViCode, String)> {
        if vector >= VI_VECTOR_MAX {
            self.note(ViCode::VectorOob);
            return Err((ViCode::VectorOob, ViCode::VectorOob.advice().to_string()));
        }
        if !self.map.is_set(vector) {
            self.note(ViCode::UnmaskIdle);
            return Err((ViCode::UnmaskIdle, ViCode::UnmaskIdle.advice().to_string()));
        }
        // 双写一致：清位图 + 更新台账 active=false/until=tick。
        self.map.clear(vector);
        let mut found = false;
        for r in self.rows.iter_mut() {
            if r.vector == vector && r.active {
                r.active = false;
                r.until = self.tick;
                found = true;
            }
        }
        if !found {
            self.note(ViCode::RestoreGap);
            return Err((ViCode::RestoreGap, ViCode::RestoreGap.advice().to_string()));
        }
        self.unmask_count += 1;
        self.enqueue_notify(vector, NotifyKind::Restore, "恢复解屏蔽")?;
        Ok(())
    }

    /// 分发闸门：被屏蔽向量拦截，否则放行。
    pub fn gate(&mut self, vector: usize) -> GateOutcome {
        if vector < VI_VECTOR_MAX && self.map.is_set(vector) {
            self.gate_block += 1;
            GateOutcome::Blocked
        } else {
            self.gate_pass += 1;
            GateOutcome::Pass
        }
    }

    /// 送达全部通知（幂等标记 delivered）。
    pub fn flush_notifies(&mut self) {
        for n in self.notifies.iter_mut() {
            n.delivered = true;
        }
    }

    /// 已送达通知计数。
    pub fn delivered_count(&self) -> usize {
        self.notifies.iter().filter(|n| n.delivered).count()
    }

    /// 活跃屏蔽数（台账 active 行数）。
    pub fn active_mask(&self) -> usize {
        self.rows.iter().filter(|r| r.active).count()
    }

    /// 守恒核：屏蔽数 + 活跃数 ≡ 注册总数，且位图计数 ≡ 台账活跃数（判据核心）。
    pub fn conserved(&self) -> bool {
        let masked = self.map.count();
        let active_vec = self.registered.saturating_sub(masked);
        masked + active_vec == self.registered && masked == self.active_mask()
    }

    /// 闸门一致性核：台账每一条活跃屏蔽行的向量必在位图上。
    pub fn gate_consistent(&self) -> bool {
        self.rows.iter().all(|r| !r.active || self.map.is_set(r.vector))
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "向量隔离：注册 {} 屏蔽 {} 解屏蔽 {} 闸门放行 {} 拦截 {} 守恒 {} 通知送达 {}/{}",
            self.registered,
            self.mask_count,
            self.unmask_count,
            self.gate_pass,
            self.gate_block,
            self.conserved(),
            self.delivered_count(),
            self.notifies.len()
        )
    }
}
