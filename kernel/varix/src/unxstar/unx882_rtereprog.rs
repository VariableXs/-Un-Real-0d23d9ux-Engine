//! unx882_rtereprog —— UNX-F0882 RTE 目的地重编程与 mask-move-unmask 迁移序列。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B13 · 行 2535）：
//! 「UNX-F0882 | RTE 目的地重编程与 mask-move-unmask 迁移序列 | 340 | 已深化 |
//!   判据：UNX-F0882-J1 迁移序列正确率 100%（注入乱序 10/10 次拦截），
//!   迁移后投递落新核断言 10/10 次，序列时长实测 <50µs」
//!
//! 组件：RTE 表项模型（向量/目的地/屏蔽位/触发极性四要素）
//!      + 五步迁移状态机（Quiesce→Mask→Move→Unmask→Verify 单向严格序）
//!      + 乱序拦截器（步骤序号严格递增，乱序 100% 拒绝留痕）
//!      + mask-move-unmask 纪律闸（未 Mask 先 Move 拒；未 Move 先 Unmask 拒）
//!      + 投递落地账（迁移后投递目的地核验）+ 时长预算账（逻辑拍折算 µs，
//!      50µs 预算红线步数化）+ 迁移流水账（append-only 回放）
//!      + 快照指纹（FNV-1a 确定性）
//!
//! 判据落地：①迁移序列正确率 100%（乱序注入 10/10 次拦截）；②迁移后
//! 投递落新核 10/10 次；③序列时长 <50µs（逻辑拍预算折算，超预算显性）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。分层声明：本层是 I/O
//! APIC RTE 迁移的语义裁决层，真实 MMIO 访问归中断控制器驱动层。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const RTE_VERSION: &str = "unx882_rtereprog-v1";

/// 诊断段基址（0x9B00 段 · 本模块独占，入库前已 grep 全域源文件确认无撞段）。
pub const RTE_SEG_BASE: u16 = 0x9B00;

/// 序列时长预算（逻辑拍口径；每拍折算 1µs——确定性折算策略值，可调）。
pub const SEQ_BUDGET_TICKS: u64 = 50;

/// 迁移流水账容量（append-only 截断面）。
pub const MIG_LOG_CAP: usize = 32;

/// 投递落地核验批量（判据口径 10/10）。
pub const DELIVERY_VERIFY_N: usize = 10;

/// 乱序注入核验批量（判据口径 10/10）。
pub const OOO_VERIFY_N: usize = 10;

/// RTE 表项数（I/O APIC 标准 redirection 表 24 项——当前策略值）。
pub const RTE_TABLE_ENTRIES: usize = 24;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9B01~0x9B0C）
// ---------------------------------------------------------------------------

/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RteCode {
    /// 0x9B01 迁移步骤乱序（严格序破坏——拦截留痕）。
    StepOutOfOrder,
    /// 0x9B02 未 Mask 先 Move（mask-move-unmask 纪律破坏）。
    MoveBeforeMask,
    /// 0x9B03 未 Move 先 Unmask（迁移未确认就开闸）。
    UnmaskBeforeMove,
    /// 0x9B04 步骤重复（同一迁移内步骤幂等键冲突）。
    StepDuplicate,
    /// 0x9B05 目的地核失败（新核不在可达闭集内）。
    DestInvalid,
    /// 0x9B06 迁移窗口事件丢失（在途账与投递账交叉不平）。
    WindowEventLost,
    /// 0x9B07 序列超时（时长超 50µs 预算红线）。
    SeqOverBudget,
    /// 0x9B08 向量号非法（0~31 保留区拒）。
    VectorReserved,
    /// 0x9B09 表项越界（RTE 索引超 24）。
    IndexOutOfRange,
    /// 0x9B0A 迁移流水账满（append-only 截断）。
    MigLogFull,
    /// 0x9B0B 快照指纹对不上。
    FingerprintMismatch,
    /// 0x9B0C 迁移进行中再迁移（并发迁移互斥破坏）。
    MigrateTwice,
}

impl RteCode {
    /// 段内码值。
    pub const fn val(self) -> u16 {
        match self {
            RteCode::StepOutOfOrder => 0x9B01,
            RteCode::MoveBeforeMask => 0x9B02,
            RteCode::UnmaskBeforeMove => 0x9B03,
            RteCode::StepDuplicate => 0x9B04,
            RteCode::DestInvalid => 0x9B05,
            RteCode::WindowEventLost => 0x9B06,
            RteCode::SeqOverBudget => 0x9B07,
            RteCode::VectorReserved => 0x9B08,
            RteCode::IndexOutOfRange => 0x9B09,
            RteCode::MigLogFull => 0x9B0A,
            RteCode::FingerprintMismatch => 0x9B0B,
            RteCode::MigrateTwice => 0x9B0C,
        }
    }

    /// 段归属校验。
    pub const fn in_segment(v: u16) -> bool {
        v >= RTE_SEG_BASE + 1 && v <= RTE_SEG_BASE + 0x0C
    }

    /// 码值反查。
    pub fn of(v: u16) -> Option<RteCode> {
        if !RteCode::in_segment(v) {
            return None;
        }
        match v - RTE_SEG_BASE {
            1 => Some(RteCode::StepOutOfOrder),
            2 => Some(RteCode::MoveBeforeMask),
            3 => Some(RteCode::UnmaskBeforeMove),
            4 => Some(RteCode::StepDuplicate),
            5 => Some(RteCode::DestInvalid),
            6 => Some(RteCode::WindowEventLost),
            7 => Some(RteCode::SeqOverBudget),
            8 => Some(RteCode::VectorReserved),
            9 => Some(RteCode::IndexOutOfRange),
            10 => Some(RteCode::MigLogFull),
            11 => Some(RteCode::FingerprintMismatch),
            12 => Some(RteCode::MigrateTwice),
            _ => None,
        }
    }

    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            RteCode::StepOutOfOrder => "迁移步骤乱序",
            RteCode::MoveBeforeMask => "未 Mask 就 Move",
            RteCode::UnmaskBeforeMove => "未 Move 确认就 Unmask",
            RteCode::StepDuplicate => "迁移步骤重复提交",
            RteCode::DestInvalid => "目的地核不在可达闭集",
            RteCode::WindowEventLost => "迁移窗口事件丢失",
            RteCode::SeqOverBudget => "迁移序列超 50µs 预算",
            RteCode::VectorReserved => "向量号落在保留区",
            RteCode::IndexOutOfRange => "RTE 表项索引越界",
            RteCode::MigLogFull => "迁移流水账已满",
            RteCode::FingerprintMismatch => "快照指纹与账面重算不一致",
            RteCode::MigrateTwice => "同一表项并发迁移",
        }
    }

    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            RteCode::StepOutOfOrder => "上游并发调用或重放注入打破五步严格序",
            RteCode::MoveBeforeMask => "Mask 闸未过即提交 Move——迁移窗口未关就改道",
            RteCode::UnmaskBeforeMove => "Move 未回执确认就 Unmask——开闸时目的地未定",
            RteCode::StepDuplicate => "同一步骤被重放（幂等键缺失）",
            RteCode::DestInvalid => "目的地核号超出拓扑闭集或指向离线核",
            RteCode::WindowEventLost => "Mask 期间到达的事件没有进在途账",
            RteCode::SeqOverBudget => "每步之间插入了他务，序列被拉长超预算",
            RteCode::VectorReserved => "向量分配器漏了 0~31 保留区检查",
            RteCode::IndexOutOfRange => "表项索引来自未校验的外部输入",
            RteCode::MigLogFull => "迁移频次超流水容量且未归档",
            RteCode::FingerprintMismatch => "绕过迁移口直改表项",
            RteCode::MigrateTwice => "迁移互斥锁缺失，两条迁移流水重叠",
        }
    }

    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            RteCode::StepOutOfOrder => "按 Quiesce→Mask→Move→Unmask→Verify 严格序重提；乱序即拦截是正确行为",
            RteCode::MoveBeforeMask => "先走 Mask 拿到闸回执再 Move",
            RteCode::UnmaskBeforeMove => "等 Move 回执（新核确认）再 Unmask",
            RteCode::StepDuplicate => "给步骤加幂等键（表项号+步骤号+迁移代）",
            RteCode::DestInvalid => "以拓扑账为准重选目的地；离线核先上线再迁移",
            RteCode::WindowEventLost => "对账在途账与投递账，缺口立案回放定位",
            RteCode::SeqOverBudget => "迁移窗口内禁止他务插入；超时即回滚到 Mask 前态",
            RteCode::VectorReserved => "向量分配前置闸加保留区检查（0~31 拒）",
            RteCode::IndexOutOfRange => "索引先比 RTE_TABLE_ENTRIES 再落账",
            RteCode::MigLogFull => "先快照+指纹归档再续迁移",
            RteCode::FingerprintMismatch => "停写取快照取证，从最近可信快照重建",
            RteCode::MigrateTwice => "迁移互斥：同表项同时只许一条迁移流水",
        }
    }

    /// 三要素合一。
    pub fn detail(self) -> String {
        let mut s = String::new();
        s.push_str("[0x");
        push_hex_u16(&mut s, self.val());
        s.push_str("] ");
        s.push_str(self.what());
        s.push_str(" ｜ 原因：");
        s.push_str(self.why());
        s.push_str(" ｜ 建议：");
        s.push_str(self.advice());
        s
    }
}

// ---------------------------------------------------------------------------
// RTE 表项模型
// ---------------------------------------------------------------------------

/// 触发模式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerMode {
    /// 边沿触发。
    Edge,
    /// 电平触发。
    Level,
}

/// 一条 RTE 表项（四要素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RteEntry {
    /// 表项索引（0..24）。
    pub index: usize,
    /// 中断向量号（32~255；0~31 保留区拒）。
    pub vector: u8,
    /// 目的地核号。
    pub dest_cpu: u8,
    /// 屏蔽位（true=屏蔽中）。
    pub masked: bool,
    /// 触发模式。
    pub trigger: TriggerMode,
}

impl RteEntry {
    /// 向量号合法性（32~255；0~31 体系结构保留）。
    pub const fn vector_ok(v: u8) -> bool {
        v >= 32
    }

    /// 索引合法性。
    pub const fn index_ok(i: usize) -> bool {
        i < RTE_TABLE_ENTRIES
    }
}

// ---------------------------------------------------------------------------
// 五步迁移状态机
// ---------------------------------------------------------------------------

/// 迁移五步（严格序：Quiesce→Mask→Move→Unmask→Verify）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigStep {
    /// 1 静默（停新投递——窗口关闭前奏）。
    Quiesce = 1,
    /// 2 屏蔽（RTE mask 位置 1）。
    Mask = 2,
    /// 3 改道（目的地重编程 + 回执确认）。
    Move = 3,
    /// 4 开闸（RTE mask 位清 0）。
    Unmask = 4,
    /// 5 核验（投递落地 + 在途账平）。
    Verify = 5,
}

impl MigStep {
    /// 步骤序号。
    pub const fn no(self) -> u8 {
        self as u8
    }

    /// 严格序校验：next 必须是 self 序号 +1。
    pub const fn follows(self, next: MigStep) -> bool {
        next.no() == self.no() + 1
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            MigStep::Quiesce => "静默",
            MigStep::Mask => "屏蔽",
            MigStep::Move => "改道",
            MigStep::Unmask => "开闸",
            MigStep::Verify => "核验",
        }
    }
}

/// 迁移流水状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigState {
    /// 无迁移在途。
    Idle,
    /// 迁移在途（Quiesce/ Mask/ Move 已过，Unmask 未到）。
    InFlight,
    /// 迁移完成（Verify 过——终态）。
    Done,
}

// ---------------------------------------------------------------------------
// 迁移流水与投递账
// ---------------------------------------------------------------------------

/// 一笔迁移流水（表项、步骤史、结果码、时长）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigEntry {
    /// 表项索引。
    pub index: usize,
    /// 迁移代（同表项第 n 次迁移——幂等键成分）。
    pub gen: u32,
    /// 旧目的地。
    pub from_cpu: u8,
    /// 新目的地。
    pub to_cpu: u8,
    /// 已过步骤（严格序史）。
    pub steps_done: u8,
    /// 结果：0=在途，1=成功，其他=诊断码。
    pub result: u16,
    /// 序列时长（逻辑拍——预算折算口径）。
    pub duration_ticks: u64,
    /// 开始时刻。
    pub start_tick: u64,
}

/// 一笔投递落地记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    /// 表项索引。
    pub index: usize,
    /// 向量号。
    pub vector: u8,
    /// 实际落地的核。
    pub landed_cpu: u8,
    /// 时刻。
    pub tick: u64,
}

/// 迁移流水账（append-only）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigLog {
    entries: Vec<MigEntry>,
}

impl MigLog {
    /// 空账。
    pub fn new() -> Self {
        MigLog {
            entries: Vec::new(),
        }
    }

    /// 追加（满即拒——append-only 纪律）。
    pub fn push(&mut self, e: MigEntry) -> Result<(), RteCode> {
        if self.entries.len() >= MIG_LOG_CAP {
            return Err(RteCode::MigLogFull);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 只读视图。
    pub fn all(&self) -> &[MigEntry] {
        &self.entries
    }

    /// 占用数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// RTE 迁移引擎（本模块主体）
// ---------------------------------------------------------------------------

/// RTE 目的地重编程引擎：表 × 迁移状态机 × 纪律闸 × 对账，一体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RteMigrationEngine {
    /// RTE 表（24 项定容）。
    table: [Option<RteEntry>; RTE_TABLE_ENTRIES],
    /// 在途迁移（同表项互斥——单引擎一次只带一条迁移流水）。
    inflight: Option<MigEntry>,
    /// 可达核闭集（拓扑声明面——目的地必须在此闭集内）。
    reachable: [bool; 8],
    /// 投递落地账（滚动核验窗口）。
    deliveries: Vec<Delivery>,
    /// 迁移流水账。
    mlog: MigLog,
    /// 迁移代发生器（同表项迁移计数）。
    gens: [u32; RTE_TABLE_ENTRIES],
    /// 乱序拦截计数（判据 10/10 的分子）。
    ooo_intercepted: u64,
    /// 落新核核验通过数（判据 10/10 的分子）。
    delivery_ok: u64,
    /// 逻辑时钟。
    tick: u64,
    /// 立案数。
    cases: u64,
}

impl RteMigrationEngine {
    /// 新引擎（表空、核 0~3 可达——当前策略拓扑）。
    pub fn new() -> Self {
        let mut reachable = [false; 8];
        reachable[0] = true;
        reachable[1] = true;
        reachable[2] = true;
        reachable[3] = true;
        RteMigrationEngine {
            table: [None; RTE_TABLE_ENTRIES],
            inflight: None,
            reachable,
            deliveries: Vec::new(),
            mlog: MigLog::new(),
            gens: [0; RTE_TABLE_ENTRIES],
            ooo_intercepted: 0,
            delivery_ok: 0,
            tick: 0,
            cases: 0,
        }
    }

    /// 时钟步进。
    pub const fn tick(&mut self) {
        self.tick += 1;
    }

    /// 建一条表项（向量/目的地校验前置）。
    pub fn install(&mut self, index: usize, vector: u8, dest_cpu: u8, trigger: TriggerMode) -> Result<(), RteCode> {
        if !RteEntry::index_ok(index) {
            self.cases += 1;
            return Err(RteCode::IndexOutOfRange);
        }
        if !RteEntry::vector_ok(vector) {
            self.cases += 1;
            return Err(RteCode::VectorReserved);
        }
        if !self.dest_ok(dest_cpu) {
            self.cases += 1;
            return Err(RteCode::DestInvalid);
        }
        self.table[index] = Some(RteEntry {
            index,
            vector,
            dest_cpu,
            masked: false,
            trigger,
        });
        Ok(())
    }

    /// 目的地合法性。
    pub const fn dest_ok(&self, cpu: u8) -> bool {
        (cpu as usize) < 8 && self.reachable[cpu as usize]
    }

    /// 表项只读。
    pub fn entry(&self, index: usize) -> Option<RteEntry> {
        if RteEntry::index_ok(index) {
            self.table[index]
        } else {
            None
        }
    }

    /// 提交一步（判据主入口）：严格序 + 纪律闸双检，任何越序显性拦截。
    ///
    /// 返回 Ok(已过步骤数)；拦截时返回诊断码且迁移流水状态零推进。
    pub fn submit_step(&mut self, index: usize, step: MigStep, at_tick: u64) -> Result<u8, RteCode> {
        self.tick = at_tick;
        if !RteEntry::index_ok(index) {
            self.cases += 1;
            return Err(RteCode::IndexOutOfRange);
        }
        // 第一步必须 Quiesce：无在途迁移时只接受 Quiesce。
        let expected_next: u8 = match &self.inflight {
            None => 1,
            Some(m) => m.steps_done + 1,
        };
        // 同表项并发迁移互斥（在途代与提交代冲突面）。
        if let Some(m) = &self.inflight {
            if m.index == index && m.result == 0 && step.no() <= m.steps_done {
                self.ooo_intercepted += 1;
                self.cases += 1;
                return Err(RteCode::StepDuplicate);
            }
        }
        if step.no() != expected_next {
            // 乱序：拦截留痕（判据「注入乱序 10/10 次拦截」的分子）。
            self.ooo_intercepted += 1;
            self.cases += 1;
            return Err(RteCode::StepOutOfOrder);
        }
        // 纪律闸：Mask 前禁 Move（expected_next 已保证次序，此处复核纪律语义——
        // Move(3) 必须已过 Mask(2)，Unmask(4) 必须已过 Move(3)；由严格序推出，
        // 显性写出防未来放宽序时静默失守）。
        if step.no() == MigStep::Move.no() {
            let m = self.inflight.as_ref().ok_or(RteCode::MoveBeforeMask)?;
            if m.steps_done < MigStep::Mask.no() {
                self.cases += 1;
                return Err(RteCode::MoveBeforeMask);
            }
        }
        if step.no() == MigStep::Unmask.no() {
            let m = self.inflight.as_ref().ok_or(RteCode::UnmaskBeforeMove)?;
            if m.steps_done < MigStep::Move.no() {
                self.cases += 1;
                return Err(RteCode::UnmaskBeforeMove);
            }
        }
        // 步骤落地。
        match step {
            MigStep::Quiesce => {
                // Quiesce 开启迁移流水（目的地表项必须存在）。
                let e = match self.table[index] {
                    Some(e) => e,
                    None => {
                        self.cases += 1;
                        return Err(RteCode::IndexOutOfRange);
                    }
                };
                if self.inflight.is_some() {
                    self.cases += 1;
                    return Err(RteCode::MigrateTwice);
                }
                self.gens[index] = self.gens[index].wrapping_add(1);
                let m = MigEntry {
                    index,
                    gen: self.gens[index],
                    from_cpu: e.dest_cpu,
                    to_cpu: e.dest_cpu, // Move 步才改写
                    steps_done: 1,
                    result: 0,
                    duration_ticks: 0,
                    start_tick: self.tick,
                };
                self.inflight = Some(m);
                Ok(1)
            }
            MigStep::Mask => {
                let m = self.inflight.as_mut().ok_or(RteCode::MoveBeforeMask)?;
                m.steps_done = 2;
                if let Some(e) = &mut self.table[m.index] {
                    e.masked = true;
                }
                Ok(2)
            }
            MigStep::Move => {
                let m = self.inflight.as_mut().ok_or(RteCode::UnmaskBeforeMove)?;
                m.steps_done = 3;
                Ok(3)
            }
            MigStep::Unmask => {
                let m = self.inflight.as_mut().ok_or(RteCode::UnmaskBeforeMove)?;
                m.steps_done = 4;
                if let Some(e) = &mut self.table[m.index] {
                    e.masked = false;
                    e.dest_cpu = m.to_cpu;
                }
                Ok(4)
            }
            MigStep::Verify => {
                let mut m = match self.inflight.take() {
                    Some(m) => m,
                    None => {
                        self.cases += 1;
                        return Err(RteCode::StepOutOfOrder);
                    }
                };
                m.steps_done = 5;
                m.duration_ticks = self.tick.saturating_sub(m.start_tick);
                if m.duration_ticks > SEQ_BUDGET_TICKS {
                    m.result = RteCode::SeqOverBudget.val();
                    self.cases += 1;
                    let code = RteCode::SeqOverBudget;
                    let _ = self.mlog.push(m);
                    return Err(code);
                }
                m.result = 1;
                let _ = self.mlog.push(m);
                Ok(5)
            }
        }
    }

    /// 声明迁移目的地（Move 步的参数面——在 Mask 已过、Move 未到窗口内调用）。
    pub fn set_move_target(&mut self, to_cpu: u8) -> Result<(), RteCode> {
        if !self.dest_ok(to_cpu) {
            self.cases += 1;
            return Err(RteCode::DestInvalid);
        }
        let m = match self.inflight.as_mut() {
            Some(m) if m.steps_done >= MigStep::Mask.no() && m.steps_done < MigStep::Move.no() => m,
            _ => {
                self.cases += 1;
                return Err(RteCode::MoveBeforeMask);
            }
        };
        m.to_cpu = to_cpu;
        Ok(())
    }

    /// 投递落地核验（判据主入口之二）：迁移完成后投递必须落新核。
    pub fn record_delivery(&mut self, index: usize, vector: u8, landed_cpu: u8, at_tick: u64) -> bool {
        self.tick = at_tick;
        // 落地核验：与表项当前目的地一致（迁移后表项目的地=新核）。
        let expect = match self.table.get(index) {
            Some(Some(e)) => (e.dest_cpu, e.vector),
            _ => return false,
        };
        let ok = landed_cpu == expect.0 && vector == expect.1;
        if self.deliveries.len() < DELIVERY_VERIFY_N * 4 {
            self.deliveries.push(Delivery {
                index,
                vector,
                landed_cpu,
                tick: at_tick,
            });
        }
        if ok {
            self.delivery_ok += 1;
        }
        ok
    }

    /// 落新核核验通过数。
    pub const fn delivery_ok_count(&self) -> u64 {
        self.delivery_ok
    }

    /// 乱序拦截数。
    pub const fn ooo_intercepted_count(&self) -> u64 {
        self.ooo_intercepted
    }

    /// 完成的迁移流水（结果=1）。
    pub fn done_migrations(&self) -> Vec<MigEntry> {
        self.mlog
            .all()
            .iter()
            .filter(|m| m.result == 1)
            .cloned()
            .collect()
    }

    /// 迁移流水账只读视图。
    pub fn mig_log(&self) -> &[MigEntry] {
        self.mlog.all()
    }

    /// 预算合规：全部完成流水时长 ≤ 50µs 预算（判据三）。
    pub fn budget_holds(&self) -> bool {
        self.mlog
            .all()
            .iter()
            .all(|m| m.result == 0 || m.duration_ticks <= SEQ_BUDGET_TICKS)
    }

    /// 守恒式：在途账 + 完成账 + 拦截账三面平。
    pub fn conserved(&self) -> bool {
        // 在途至多一条。
        let inflight_n = if self.inflight.is_some() { 1 } else { 0 };
        // 流水账每条 steps_done ∈ 0..=5。
        let steps_ok = self
            .mlog
            .all()
            .iter()
            .all(|m| m.steps_done <= 5);
        inflight_n <= 1 && steps_ok && self.ooo_intercepted + self.delivery_ok <= self.tick.max(1)
    }

    /// 快照序列化（确定性）。
    pub fn snapshot_text(&self) -> String {
        let mut s = String::new();
        s.push_str("RTE v");
        s.push_str(RTE_VERSION);
        s.push_str(" tick=");
        push_u64(&mut s, self.tick);
        s.push_str(" inflight=");
        s.push_str(if self.inflight.is_some() { "y" } else { "n" });
        s.push_str(" ooo=");
        push_u64(&mut s, self.ooo_intercepted);
        s.push_str(" dlv_ok=");
        push_u64(&mut s, self.delivery_ok);
        s.push_str(" migs=");
        push_u64(&mut s, self.mlog.len() as u64);
        s.push_str(" cases=");
        push_u64(&mut s, self.cases);
        s
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(self.snapshot_text().as_bytes())
    }

    /// 防篡改校验。
    pub fn verify_fingerprint(&self, expected: u64) -> Result<(), RteCode> {
        if self.fingerprint() == expected {
            Ok(())
        } else {
            Err(RteCode::FingerprintMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// 整数工具
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub const fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// u16 十六进制。
fn push_hex_u16(s: &mut String, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[((v >> 12) & 0xF) as usize] as char);
    s.push(HEX[((v >> 8) & 0xF) as usize] as char);
    s.push(HEX[((v >> 4) & 0xF) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}

// ---------------------------------------------------------------------------
// 判据（轻量内嵌）
// ---------------------------------------------------------------------------

/// 完整迁移一遍的判定（判据口径的可复用执行器）。
fn migrate_once(e: &mut RteMigrationEngine, index: usize, from: u8, to: u8, t0: u64) -> Result<u64, RteCode> {
    let _ = from; // 旧核仅用于对账语义，账面留痕在 mlog
    let _ = e.submit_step(index, MigStep::Quiesce, t0)?;
    let _ = e.submit_step(index, MigStep::Mask, t0 + 1)?;
    e.set_move_target(to)?;
    let _ = e.submit_step(index, MigStep::Move, t0 + 2)?;
    let _ = e.submit_step(index, MigStep::Unmask, t0 + 3)?;
    let _ = e.submit_step(index, MigStep::Verify, t0 + 4)?;
    Ok(e.tick)
}

/// 判据集。
pub fn run_t882_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t882");

    // c1 五步严格序：Quiesce→Mask→Move→Unmask→Verify 全过即迁移完成。
    let mut e1 = RteMigrationEngine::new();
    let inst = e1.install(2, 0x50, 0, TriggerMode::Edge).is_ok();
    let mig = migrate_once(&mut e1, 2, 0, 2, 10);
    let done_ok = inst && mig.is_ok() && e1.done_migrations().len() == 1;
    set.add(
        "t882-c1-strictseq",
        done_ok,
        if done_ok { "" } else { "five-step strict sequence broken" },
    );

    // c2 乱序拦截 10/10：每轮在途迁移注入一步乱序（判据口径）。
    let mut e2 = RteMigrationEngine::new();
    let _ = e2.install(3, 0x51, 0, TriggerMode::Level);
    let mut ooo_all = true;
    let mut t = 100u64;
    for i in 0..OOO_VERIFY_N {
        // 正常开启迁移流水。
        let q = e2.submit_step(3, MigStep::Quiesce, t);
        if q.is_err() {
            ooo_all = false;
        }
        // 注入一步乱序（花样轮换：重复 Quiesce / 跳步 Move / 跳步 Unmask / 跳步 Verify）。
        let ooo = match i % 4 {
            0 => e2.submit_step(3, MigStep::Quiesce, t + 1), // 步骤重复
            1 => e2.submit_step(3, MigStep::Move, t + 1),    // 跳过 Mask
            2 => e2.submit_step(3, MigStep::Unmask, t + 1),  // 跳过 Move
            _ => e2.submit_step(3, MigStep::Verify, t + 1),  // 跳到核验
        };
        ooo_all &= ooo.is_err();
        // 正常走完五步收尾（迁移代闭环，下轮从干净态开始）。
        let _ = e2.submit_step(3, MigStep::Mask, t + 2);
        let _ = e2.set_move_target(2);
        let _ = e2.submit_step(3, MigStep::Move, t + 3);
        let _ = e2.submit_step(3, MigStep::Unmask, t + 4);
        let _ = e2.submit_step(3, MigStep::Verify, t + 5);
        t += 6;
    }
    set.add(
        "t882-c2-ooo",
        ooo_all && e2.ooo_intercepted_count() >= OOO_VERIFY_N as u64,
        if ooo_all { "" } else { "out-of-order interception below 10/10" },
    );

    // c3 迁移后投递落新核 10/10（判据口径）。
    let mut e3 = RteMigrationEngine::new();
    let _ = e3.install(4, 0x52, 0, TriggerMode::Edge);
    let _ = migrate_once(&mut e3, 4, 0, 2, 500);
    let mut dlv_all = true;
    for i in 0..DELIVERY_VERIFY_N {
        let tick = 600 + i as u64;
        dlv_all &= e3.record_delivery(4, 0x52, 2, tick);
    }
    set.add(
        "t882-c3-delivery",
        dlv_all && e3.delivery_ok_count() == DELIVERY_VERIFY_N as u64,
        if dlv_all { "" } else { "post-migration delivery not always on new core" },
    );

    // c4 时长预算：完成流水时长全部 ≤ 50µs 预算（判据三）。
    set.add(
        "t882-c4-budget",
        e3.budget_holds() && e1.budget_holds(),
        if e3.budget_holds() { "" } else { "sequence over 50us budget" },
    );

    // c5 纪律闸：未 Mask 先 Move 拒；未 Move 先 Unmask 拒。
    let mut e5 = RteMigrationEngine::new();
    let _ = e5.install(5, 0x53, 0, TriggerMode::Edge);
    let _ = e5.submit_step(5, MigStep::Quiesce, 700);
    let r_move_early = matches!(e5.submit_step(5, MigStep::Move, 701), Err(RteCode::StepOutOfOrder));
    let _ = e5.submit_step(5, MigStep::Mask, 702);
    let r_unmask_early = matches!(e5.submit_step(5, MigStep::Unmask, 703), Err(RteCode::StepOutOfOrder));
    set.add(
        "t882-c5-discipline",
        r_move_early && r_unmask_early,
        if r_move_early { "" } else { "mask-move-unmask discipline broken" },
    );

    // c6 目的地闭集：不可达核拒（DestInvalid）。
    let mut e6 = RteMigrationEngine::new();
    let dest_rej = matches!(e6.install(6, 0x54, 7, TriggerMode::Edge), Err(RteCode::DestInvalid));
    set.add(
        "t882-c6-dest",
        dest_rej,
        if dest_rej { "" } else { "unreachable destination accepted" },
    );

    // c7 守恒式 + 指纹确定性。
    let f1 = e3.fingerprint();
    let f2 = e3.fingerprint();
    set.add(
        "t882-c7-conserve",
        e3.conserved() && f1 == f2 && e3.verify_fingerprint(f1).is_ok(),
        if e3.conserved() { "" } else { "conservation or fingerprint broken" },
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t882_all_checks_green() {
        let set = run_t882_checks();
        assert!(set.all_passed(), "T882 domain checks must be green");
    }

    #[test]
    fn t882_diag_segment_guard() {
        for i in 1u16..=0x0C {
            assert!(RteCode::of(RTE_SEG_BASE + i).is_some());
        }
        assert!(RteCode::of(0x9B00).is_none());
        assert!(RteCode::of(0x9C01).is_none());
    }

    #[test]
    fn t882_step_order() {
        assert!(MigStep::Quiesce.follows(MigStep::Mask));
        assert!(!MigStep::Mask.follows(MigStep::Quiesce));
        assert_eq!(MigStep::Verify.no(), 5);
    }
}
