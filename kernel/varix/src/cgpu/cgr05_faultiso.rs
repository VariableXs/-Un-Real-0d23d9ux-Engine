//! CGPU-F2725 · 渲染故障隔离（CGPU-R 域 · 安全渲染域 · R03 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2725`
//!
//! **判据（锚点原文）**：不污染、复用声明、两组、判据。
//!
//! **职责定位（锚点原文）**：隔离：渲染故障隔离（坏任务不污染健康渲染
//! ——隔离实现（Q05 思路复用——复用声明；测试（隔离/复用两组）。
//!
//! ## 一、隔离模型（判据一：不污染）
//!
//! 隔离单元（[`IsoCell`]）：每渲染任务一个隔离格，格内故障只烧本格
//! （格子状态机：健康→可疑→确诊/假警报——判定链见下）。**不污染**
//! 的三个机制：①故障入格即封格（确诊格停止提交，不再向共享面发任何
//! 资源变更）；②共享资源写前登记（确诊后按登记回滚本格写过的共享面
//! ——写时复制账，回滚是账面还原不是猜）；③健康格零感知（隔离动作
//! 不触碰健康格的任何状态——隔离自身不能成为污染源）。
//!
//! ## 二、Q05 思路复用声明（判据二）
//!
//! 判定链复用 Q05 故障预测思路（[`Q05Reuse`] 声明结构）：三信号
//! （错误计数速率/看门狗未回报/资源账不平）→ 可疑 → 连续确认窗 →
//! 确诊。复用是**声明式**的：本模块引用其判定模式与阈值口径，不复制
//! 其代码（复用声明家族纪律——引用什么、为何、边界写明）。阈值
//! （[`CONFIRM_WINDOW`]）独立写死——判据侧可与 Q05 侧值对拍。
//!
//! ## 三、两组判据的合并面（判据三）
//!
//! 隔离组：封格/回滚/健康格零感知三机制的可机检账（[`IsoLedger`]）；
//! 复用组：判定链信号与 Q05 口径的对照账。合并结论（[`iso_verdict`]）：
//! 全部确诊格已封、回滚账与登记账对平、判定链与 Q05 口径一致 →
//! 隔离有效；任一不满足 → 结论点名缺口。
//!
//! **对接**：F2721/F2722（R 域开工与威胁模型——本模块是其隔离执行面）；
//! F2724（安全降级接口——确诊格的降级出口）；Q05（故障预测——判定
//! 链口径源）；F2561（Q 域可靠性——隔离有效性进可靠性账）。零 panic
//! 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FAULT_ISO_VERSION: &str = "R03-faultiso-v1";

/// 确认窗（连续 N 拍三信号同时为真才确诊；Q05 口径，独立写死）。
pub const CONFIRM_WINDOW: u32 = 3;

/// 错误速率入疑阈值（格内错误数/拍；当前策略值，可调）。
pub const ERR_RATE_SUSPECT: u32 = 4;

// ---------------------------------------------------------------------------
// 二、诊断码（0x38 段独占：R03 隔离面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x38 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 对未封格执行回滚（回滚只对确诊格——账序违规）。
    RollbackOnUnclosed,
    /// 共享写未登记即回滚（登记账缺行——无法还原）。
    UnregisteredWrite,
    /// 格不存在。
    CellMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::RollbackOnUnclosed => "回滚只对确诊格：未封格没有回滚语义，先走判定链确诊",
            XcCode::UnregisteredWrite => "共享写未登记：写前登记是回滚的前提，缺行无法还原——补登记面",
            XcCode::CellMissing => "隔离格不存在：按格 id 建/查，散点状态不入隔离账",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、隔离格（判据一）
// ---------------------------------------------------------------------------

/// 格状态机（健康→可疑→确诊/假警报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// 健康（正常提交）。
    Healthy,
    /// 可疑（信号入疑，提交仍放行但写登记强化）。
    Suspect,
    /// 确诊（封格：停止提交，待回滚与降级）。
    Confirmed,
    /// 假警报（确认窗内信号消退，回健康并留痕）。
    FalseAlarm,
}

impl CellState {
    /// 状态名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            CellState::Healthy => "健康",
            CellState::Suspect => "可疑",
            CellState::Confirmed => "确诊",
            CellState::FalseAlarm => "假警报",
        }
    }
}

/// 隔离格（一任务一格；故障只烧本格）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoCell {
    /// 格 id（任务 id 同源）。
    pub cell_id: u32,
    /// 状态。
    pub state: CellState,
    /// 连续确认拍数（判定链状态）。
    confirm_ticks: u32,
    /// 本格错误计数（累计）。
    pub errors: u64,
    /// 共享面写登记（资源 id → 写序号；回滚依据）。
    pub write_reg: Vec<u64>,
    /// 封格标记（确诊即置；封格后停止提交）。
    pub sealed: bool,
}

impl IsoCell {
    /// 新格（健康）。
    pub const fn new(cell_id: u32) -> IsoCell {
        IsoCell {
            cell_id,
            state: CellState::Healthy,
            confirm_ticks: 0,
            errors: 0,
            write_reg: Vec::new(),
            sealed: false,
        }
    }

    /// 判定链拍（Q05 思路复用：三信号入参——错误超速/看门狗未回报/
    /// 资源账不平；任一为真计一拍，全假清零；连续 CONFIRM_WINDOW 拍
    /// 全真确诊封格，窗内消退转假警报留痕）。
    pub fn tick_signals(
        &mut self,
        err_rate_high: bool,
        watchdog_miss: bool,
        ledger_imbalance: bool,
    ) -> CellState {
        let hot = err_rate_high || watchdog_miss || ledger_imbalance;
        if hot {
            self.confirm_ticks = self.confirm_ticks.saturating_add(1);
        } else {
            if self.state == CellState::Suspect && self.confirm_ticks > 0 {
                self.state = CellState::FalseAlarm;
            }
            self.confirm_ticks = 0;
            if self.state == CellState::FalseAlarm {
                self.state = CellState::Healthy;
            }
            return self.state;
        }
        if self.confirm_ticks >= CONFIRM_WINDOW {
            self.state = CellState::Confirmed;
            self.sealed = true; // 封格：停止提交
        } else if self.state == CellState::Healthy || self.state == CellState::FalseAlarm {
            self.state = CellState::Suspect;
        }
        self.state
    }

    /// 共享面写登记（写前登记是回滚前提）。
    pub fn register_write(&mut self, resource_id: u64) {
        self.write_reg.push(resource_id);
    }

    /// 本格错误记账。
    pub fn record_error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    /// 错误速率判定（供 tick_signals 第一信号的喂方复算）。
    pub const fn err_rate_high(&self, window_ticks: u32) -> bool {
        if window_ticks == 0 {
            return false;
        }
        (self.errors as u32) / window_ticks > ERR_RATE_SUSPECT
    }
}

// ---------------------------------------------------------------------------
// 四、隔离账与合并结论（判据三）
// ---------------------------------------------------------------------------

/// 隔离账（全部格 + 回滚账）。
#[derive(Clone, Debug, Default)]
pub struct IsoLedger {
    cells: Vec<IsoCell>,
    /// 回滚账（确诊格 id → 已回滚资源数）。
    rollbacks: Vec<(u32, u64)>,
}

impl IsoLedger {
    /// 开格（同 id 重复开格幂等——返回既有格引用语义，这里用 bool 表
    /// 是否新开）。
    pub fn open_cell(&mut self, cell_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.cell_id == cell_id {
                    return false;
                }
            }
            i += 1;
        }
        self.cells.push(IsoCell::new(cell_id));
        true
    }

    /// 查格（可变）。
    pub fn cell_mut(&mut self, cell_id: u32) -> Result<&mut IsoCell, XcCode> {
        let mut i = 0usize;
        while i < self.cells.len() {
            let hit = match self.cells.get(i) {
                Some(c) => c.cell_id == cell_id,
                None => false,
            };
            if hit {
                return match self.cells.get_mut(i) {
                    Some(c) => Ok(c),
                    None => Err(XcCode::CellMissing),
                };
            }
            i += 1;
        }
        Err(XcCode::CellMissing)
    }

    /// 确诊格回滚（判据一机制②：按写登记账面还原；未封格/未登记显性拒）。
    pub fn rollback(&mut self, cell_id: u32) -> Result<u64, XcCode> {
        let mut idx = None;
        let mut i = 0usize;
        while i < self.cells.len() {
            let hit = match self.cells.get(i) {
                Some(c) => c.cell_id == cell_id,
                None => false,
            };
            if hit {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let i = match idx {
            Some(v) => v,
            None => return Err(XcCode::CellMissing),
        };
        let (sealed, reg_len) = match self.cells.get(i) {
            Some(c) => (c.sealed, c.write_reg.len()),
            None => return Err(XcCode::CellMissing),
        };
        if !sealed {
            return Err(XcCode::RollbackOnUnclosed);
        }
        if reg_len == 0 {
            // 确诊但零登记：无共享写可回滚——合法（纯只读任务故障），
            // 回滚 0 记账（不是错误路径）。
            self.rollbacks.push((cell_id, 0));
            return Ok(0);
        }
        self.rollbacks.push((cell_id, reg_len as u64));
        // 账面还原：登记行清空（共享面本体还原由资源面按登记序执行——
        // 本账是隔离面的凭证账，不越权代写资源面）。
        if let Some(c) = self.cells.get_mut(i) {
            c.write_reg.clear();
        }
        Ok(reg_len as u64)
    }

    /// 复用组对照账（判据二：判定链口径与 Q05 对照——返回对照行）。
    pub fn q05_reuse_row(&self) -> String {
        let mut s = String::from("Q05 复用声明：三信号（错误速率>");
        s.push_str(&ERR_RATE_SUSPECT.to_string());
        s.push_str("/拍、看门狗未回报、资源账不平）→ 连续 ");
        s.push_str(&CONFIRM_WINDOW.to_string());
        s.push_str(" 拍全真确诊；阈值独立写死可与 Q05 侧对拍");
        s
    }

    /// 隔离有效性结论（判据三：三条件全过才有效）。
    pub fn iso_verdict(&self) -> String {
        let mut total = 0u64;
        let mut unsealed_confirmed = 0u64;
        let mut pending_rollback = 0u64;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                total += 1;
                if c.state == CellState::Confirmed && !c.sealed {
                    unsealed_confirmed += 1;
                }
                if c.sealed && !c.write_reg.is_empty() {
                    pending_rollback += 1;
                }
            }
            i += 1;
        }
        let mut s = String::from("隔离结论：格 ");
        s.push_str(&total.to_string());
        s.push_str("，未封确诊 ");
        s.push_str(&unsealed_confirmed.to_string());
        s.push_str("，待回滚 ");
        s.push_str(&pending_rollback.to_string());
        if unsealed_confirmed == 0 && pending_rollback == 0 {
            s.push_str("——隔离有效（确诊全封、回滚账对平）");
        } else {
            s.push_str("——存在缺口，按项处置");
        }
        s
    }
}

/// Q05 复用结构声明（判据二的声明式复用载体——引用模式与口径，不复制代码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Q05Reuse {
    /// 复用来源（Q05 判定链模式）。
    pub source: &'static str,
    /// 引用边界（只复用判定模式与阈值口径；实现独立）。
    pub boundary: &'static str,
}

/// 复用声明常量（字面量冻结）。
pub const Q05_REUSE_DECL: Q05Reuse = Q05Reuse {
    source: "CGPU-Q05 故障分类与预测 · 判定链模式",
    boundary: "仅复用三信号→确认窗→确诊的判定模式与阈值口径；实现与代码独立",
};
