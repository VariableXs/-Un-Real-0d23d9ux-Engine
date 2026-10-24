//! VE-F3641 · 壁纸工坊总架构（VE-S 域 · 批次 R03 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3641`
//!
//! **职责定位（锚点原文）**：视觉创作工坊（壁纸/图标/光标/锁屏四工坊统一架构）——
//! - **统一架构**：画布引擎/素材库/导出器/模板库四核共享（共享核红线：
//!   各工坊私造画布 = 维护灾难 → 单源画布红线）；
//! - **工坊差异**：各工坊专属工具层（壁纸层级编辑/图标网格/光标帧动画）——
//!   共享核 + 专属层架构；
//! - **产消协议**：E 域壁纸运行时消费工坊产物（F3656 联动前向）。
//!
//! **判据（锚点原文六条）**：单源画布、共享核+专属层、产消协议、四工坊、
//! 差异化声明、判据。
//!
//! **错误路径与降级矩阵（锚点原文三条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 私造画布检出→P1 重构（单源红线实测） | [`W41Code::PRIVATE_CANVAS`] |
//! | 核功能分歧→对拍 | [`W41Code::CORE_DIVERGED`] |
//! | 协议分歧→对齐（复述） | [`W41Code::PROTOCOL_MISMATCH`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3641 诊断码段（独占 0x59xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W41Code(pub u16);

impl W41Code {
    /// 工坊私造画布（单源红线 P1）。
    pub const PRIVATE_CANVAS: W41Code = W41Code(0x5901);
    /// 四核功能分歧。
    pub const CORE_DIVERGED: W41Code = W41Code(0x5902);
    /// 产消协议分歧。
    pub const PROTOCOL_MISMATCH: W41Code = W41Code(0x5903);
    /// 工坊未登记。
    pub const WORKSHOP_UNKNOWN: W41Code = W41Code(0x5904);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            W41Code::PRIVATE_CANVAS => {
                "工坊私造画布（单源红线 P1）。建议：改走共享画布引擎。"
            }
            W41Code::CORE_DIVERGED => "四核功能分歧。建议：以共享核基准对拍修正。",
            W41Code::PROTOCOL_MISMATCH => "产消协议分歧。建议：与 E 域壁纸对齐协议版本。",
            W41Code::WORKSHOP_UNKNOWN => "工坊未登记。建议：先注册工坊再取核。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四工坊 ──────────────────────────────────────────────────────────

/// 工坊类型（四工坊）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Workshop {
    /// 壁纸工坊（层级编辑）。
    Wallpaper,
    /// 图标工坊（像素网格）。
    Icon,
    /// 光标工坊（帧动画）。
    Cursor,
    /// 锁屏工坊。
    LockScreen,
}

impl Workshop {
    /// 全集（判据遍历用）。
    pub const ALL: [Workshop; 4] = [
        Workshop::Wallpaper,
        Workshop::Icon,
        Workshop::Cursor,
        Workshop::LockScreen,
    ];

    /// 专属工具层标识（差异化声明）。
    pub fn tool_layer(self) -> &'static str {
        match self {
            Workshop::Wallpaper => "layer-edit",
            Workshop::Icon => "pixel-grid",
            Workshop::Cursor => "frame-anim",
            Workshop::LockScreen => "lock-compose",
        }
    }
}

// ── 四核共享 ────────────────────────────────────────────────────────

/// 共享核类型（四核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Core {
    /// 画布引擎（单源红线核）。
    Canvas,
    /// 素材库。
    Assets,
    /// 导出器。
    Exporter,
    /// 模板库。
    Templates,
}

impl Core {
    pub const ALL: [Core; 4] = [
        Core::Canvas,
        Core::Assets,
        Core::Exporter,
        Core::Templates,
    ];
}

/// 工坊注册项：专属层 + 自带核清单（合法为全共享）。
#[derive(Clone, Copy)]
pub struct WorkshopReg {
    pub kind: Workshop,
    /// 是否违规私造画布。
    pub private_canvas: bool,
    /// 引用的共享核版本（四核必须一致）。
    pub core_rev: u32,
}

// ── 产消协议 ────────────────────────────────────────────────────────

/// 产消协议：工坊产物 → E 域壁纸运行时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumeKind {
    /// 静态壁纸包。
    StaticPack,
    /// 动态壁纸包。
    MotionPack,
    /// 图标集。
    IconSet,
    /// 光标集。
    CursorSet,
}

/// 协议帧（版本 + 产物类型 + 完整性位）。
#[derive(Clone, Copy)]
pub struct ProtocolFrame {
    pub version: u16,
    pub kind: ConsumeKind,
    pub complete: bool,
}

impl ProtocolFrame {
    /// 当前协议版本（对齐口径）。
    pub const CURRENT: u16 = 1;

    /// 是否可被 E 域消费。
    pub fn consumable(&self) -> bool {
        self.version == Self::CURRENT && self.complete
    }
}

// ── 工坊架构总线 ────────────────────────────────────────────────────

/// 壁纸工坊总架构：四核单源 + 四工坊注册 + 产消协议。
pub struct WorkshopArch {
    regs: Vec<WorkshopReg>,
    core_rev: u32,
    frames: Vec<ProtocolFrame>,
    last_code: W41Code,
    has_error: bool,
}

impl WorkshopArch {
    /// 新建；`core_rev` 为共享核基准版本。
    pub fn new(core_rev: u32) -> Self {
        Self {
            regs: Vec::new(),
            core_rev,
            frames: Vec::new(),
            last_code: W41Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> W41Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: W41Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 注册工坊；私造画布立即红线。
    pub fn register(&mut self, kind: Workshop, private_canvas: bool, core_rev: u32) -> Result<usize, W41Code> {
        if self.regs.iter().any(|r| r.kind == kind) {
            return Ok(self.regs.len());
        }
        if private_canvas {
            self.fail(W41Code::PRIVATE_CANVAS);
            return Err(W41Code::PRIVATE_CANVAS);
        }
        if core_rev != self.core_rev {
            self.fail(W41Code::CORE_DIVERGED);
            return Err(W41Code::CORE_DIVERGED);
        }
        self.regs.push(WorkshopReg { kind, private_canvas, core_rev });
        self.last_code = W41Code(0);
        self.has_error = false;
        Ok(self.regs.len())
    }

    /// 四工坊是否齐装。
    pub fn all_workshops(&self) -> bool {
        Workshop::ALL.iter().all(|k| self.regs.iter().any(|r| r.kind == *k))
    }

    /// 共享核审计：全部注册项 core_rev 一致。
    pub fn audit_shared_cores(&self) -> Result<bool, W41Code> {
        if self.regs.iter().any(|r| r.core_rev != self.core_rev) {
            Err(W41Code::CORE_DIVERGED)
        } else {
            Ok(true)
        }
    }

    /// 差异化声明：专属工具层两两互异。
    pub fn tool_layers_distinct(&self) -> bool {
        let mut seen: Vec<&'static str> = Vec::new();
        for k in Workshop::ALL.iter() {
            let t = k.tool_layer();
            if seen.contains(&t) {
                return false;
            }
            seen.push(t);
        }
        true
    }

    /// 推入产消协议帧。
    pub fn publish(&mut self, kind: ConsumeKind) -> Result<ProtocolFrame, W41Code> {
        let frame = ProtocolFrame { version: ProtocolFrame::CURRENT, kind, complete: true };
        if !frame.consumable() {
            self.fail(W41Code::PROTOCOL_MISMATCH);
            return Err(W41Code::PROTOCOL_MISMATCH);
        }
        self.frames.push(frame);
        self.last_code = W41Code(0);
        self.has_error = false;
        Ok(frame)
    }

    /// 协议帧对拍：版本必须为 CURRENT。
    pub fn verify_protocol(&mut self, version: u16) -> Result<bool, W41Code> {
        if version != ProtocolFrame::CURRENT {
            self.fail(W41Code::PROTOCOL_MISMATCH);
            return Err(W41Code::PROTOCOL_MISMATCH);
        }
        self.last_code = W41Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 取共享核编号（单源出口）。
    pub fn core_rev(&self, core: Core) -> Result<u32, W41Code> {
        let _ = core;
        if self.regs.is_empty() {
            // &self 契约（只读取号）不能走 fail() 遥测——错误码经返回值上报。
            return Err(W41Code::WORKSHOP_UNKNOWN);
        }
        Ok(self.core_rev)
    }

    pub fn workshop_count(&self) -> usize {
        self.regs.len()
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 单源画布总评：无注册项违规且核对拍过。
    pub fn canvas_single_source(&self) -> bool {
        self.regs.iter().all(|r| !r.private_canvas) && self.audit_shared_cores().is_ok()
    }

    /// 已注册工坊种类。
    pub fn has_workshop(&self, kind: Workshop) -> bool {
        self.regs.iter().any(|r| r.kind == kind)
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves41_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves41_wallarch");
    {
        let a = WorkshopArch::new(1);
        chk(&mut set, "C3641-01", a.workshop_count() == 0 && a.core_rev(Core::Canvas) == Err(W41Code::WORKSHOP_UNKNOWN), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        let r = a.register(Workshop::Wallpaper, false, 1);
        chk(&mut set, "C3641-02", r == Ok(1), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        let r = a.register(Workshop::Icon, true, 1);
        chk(&mut set, "C3641-03", r.err() == Some(W41Code::PRIVATE_CANVAS), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        let r = a.register(Workshop::Icon, false, 9);
        chk(&mut set, "C3641-04", r.err() == Some(W41Code::CORE_DIVERGED), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        for k in Workshop::ALL.iter() {
            a.register(*k, false, 1);
        }
        chk(&mut set, "C3641-05", a.all_workshops(), "err");
    }
    {
        let a = WorkshopArch::new(1);
        chk(&mut set, "C3641-06", a.tool_layers_distinct(), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        a.register(Workshop::Wallpaper, false, 1);
        let r = a.audit_shared_cores();
        chk(&mut set, "C3641-07", r == Ok(true), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        let f = a.publish(ConsumeKind::StaticPack);
        chk(&mut set, "C3641-08", f.is_ok() && a.frame_count() == 1, "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        let r = a.verify_protocol(99);
        chk(&mut set, "C3641-09", r.err() == Some(W41Code::PROTOCOL_MISMATCH), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        a.register(Workshop::Wallpaper, false, 1);
        let r = a.core_rev(Core::Canvas);
        chk(&mut set, "C3641-10", r == Ok(1), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        a.register(Workshop::Wallpaper, false, 1);
        a.register(Workshop::Icon, false, 1);
        chk(&mut set, "C3641-11", a.canvas_single_source(), "err");
    }
    {
        let mut a = WorkshopArch::new(1);
        a.register(Workshop::Cursor, false, 1);
        chk(&mut set, "C3641-12", a.has_workshop(Workshop::Cursor) && !a.has_workshop(Workshop::Icon), "err");
    }
    set
}
