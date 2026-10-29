//! CGPU-F0710 · 多表面滚动并发（目标 400 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0710，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：多表面滚动并发——多个表面（surface）同帧并发滚动，
//!   需要仲裁、步长钳制、撕裂防护与守恒账。
//!
//! 设计纪律：
//! - 表面账本定容：MAX_SURF=16 槽位定容数组，登记数 = 已占槽位数
//!   （守恒恒等式，drill 显式校验）。
//! - 状态机闭集：Idle→Armed→InFlight→Latched→Idle 单向环，
//!   InFlight 异常时可入 Torn；其余边一律拒绝。
//! - 帧行预算：每帧总滚动行数 ≤ FRAME_ROW_BUDGET，仲裁按表面
//!   登记序（低号优先）分配，超预算部分留待下帧。
//! - 步长钳制：每表面单帧位移 ≤ max_step，防撕裂。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性（同输入必同输出，无随机、无时钟依赖）。
//! - 诊断段 0x4700 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4700，批内 CGPU-F0710 独占）。
pub const SEG_BASE: u16 = 0x4700;

/// 表面账本已满（16 槽全占再登记）。
pub const E_SURF_FULL: u16 = SEG_BASE + 1;
/// 表面重号登记。
pub const E_SURF_DUP: u16 = SEG_BASE + 2;
/// 滚动参数越界（dy 为负语义/超绝对上限）。
pub const E_SCROLL_RANGE: u16 = SEG_BASE + 3;
/// 非法状态迁移（状态机闭集之外）。
pub const E_STATE_EDGE: u16 = SEG_BASE + 4;
/// 未登记表面被引用。
pub const E_SURF_UNKNOWN: u16 = SEG_BASE + 5;
/// 守恒账不平（应用行数与各表面偏移之和不符）。
pub const E_LEDGER: u16 = SEG_BASE + 6;
/// 帧行预算参数非法（0 预算或超上限）。
pub const E_BUDGET: u16 = SEG_BASE + 7;

/// 表面账本容量（定容，杜绝动态扩容）。
pub const MAX_SURF: usize = 16;
/// 单帧全局滚动行预算（行/帧，来源：VE-STAR-II 滚动组规格默认档）。
pub const FRAME_ROW_BUDGET: u32 = 4096;
/// 单表面单帧位移绝对上限（行/帧）。
pub const MAX_STEP_ABS: u32 = 1024;

/// 表面描述：几何与步长上限。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfDesc {
    /// 表面号（0..=65535，批内唯一）。
    pub id: u16,
    /// 宽（像素）。
    pub w: u16,
    /// 高（像素，可滚动内容总高）。
    pub h: u16,
    /// 单帧最大滚动步长（行）。
    pub max_step: u32,
}

/// 表面滚动状态机（合法边闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollState {
    /// 空闲：无待发滚动。
    Idle,
    /// 已武装：命令已入队，等待仲裁。
    Armed,
    /// 推进中：已获预算，正在位移。
    InFlight,
    /// 已锁存：本帧位移已定，等呈现。
    Latched,
    /// 撕裂：位移中途发现越界，需回滚。
    Torn,
}

/// 合法边表：from → to。
/// 闭集注释：Idle→Armed（arm）；Armed→InFlight（tick 获预算）；
/// InFlight→Latched（位移完成）；InFlight→Torn（越界回滚）；
/// Torn→Idle（rollback）；Latched→Idle（present 完成）。
fn edge_legal(from: ScrollState, to: ScrollState) -> bool {
    use ScrollState::*;
    matches!(
        (from, to),
        (Idle, Armed)
            | (Armed, InFlight)
            | (InFlight, Latched)
            | (InFlight, Torn)
            | (Torn, Idle)
            | (Latched, Idle)
    )
}

/// 滚动命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollCmd {
    /// 目标表面号。
    pub surf: u16,
    /// 本帧滚动行数（0 表示空转，仅用于保活）。
    pub dy: u32,
    /// 发起帧号。
    pub frame: u32,
}

/// 单表面运行时状态。
#[derive(Clone, Debug)]
pub struct SurfSlot {
    /// 表面描述。
    pub desc: SurfDesc,
    /// 当前状态机状态。
    pub state: ScrollState,
    /// 已累计滚动偏移（行，永远 ≤ h）。
    pub offset: u32,
    /// 待应用位移（Armed 期间暂存）。
    pub pending: u32,
}

/// 多表面滚动并发仲裁器。
///
/// 守恒账（显式恒等式）：
///   applied_total == Σ slot.offset
///   granted_rows(frame) ≤ FRAME_ROW_BUDGET
///   每帧 granted ≤ Σ min(pending, max_step)
#[derive(Clone, Debug)]
pub struct ScrollCon {
    /// 定容槽位表（None=空槽）。
    slots: [Option<SurfSlot>; MAX_SURF],
    /// 已登记表面数（恒等式：count == 非 None 槽位数）。
    count: usize,
    /// 全局累计已应用行数。
    applied_total: u64,
    /// 当前帧号。
    frame: u32,
}

impl ScrollCon {
    /// 构造：全空账本。
    pub fn new() -> Self {
        Self {
            slots: Default::default(),
            count: 0,
            applied_total: 0,
            frame: 0,
        }
    }

    /// 登记表面：越界/重号拒绝。
    pub fn register(&mut self, desc: SurfDesc) -> Result<(), u16> {
        if desc.max_step == 0 || desc.max_step > MAX_STEP_ABS {
            return Err(E_SCROLL_RANGE);
        }
        if self.count >= MAX_SURF {
            return Err(E_SURF_FULL);
        }
        if self.slots
            .iter()
            .flatten()
            .any(|s| s.desc.id == desc.id)
        {
            return Err(E_SURF_DUP);
        }
        for i in 0..MAX_SURF {
            if self.slots[i].is_none() {
                self.slots[i] = Some(SurfSlot {
                    desc,
                    state: ScrollState::Idle,
                    offset: 0,
                    pending: 0,
                });
                self.count += 1;
                return Ok(());
            }
        }
        Err(E_SURF_FULL)
    }

    /// 按号查表面槽位（只读）。
    pub fn find(&self, id: u16) -> Option<&SurfSlot> {
        self.slots.iter().flatten().find(|s| s.desc.id == id)
    }

    /// 按号取可变槽位。
    fn find_mut(&mut self, id: u16) -> Option<&mut SurfSlot> {
        self.slots
            .iter_mut()
            .flatten()
            .find(|s| s.desc.id == id)
    }

    /// 武装一条滚动命令：Idle→Armed，钳制 dy 到 [0, max_step] 且
    /// 不越过内容高度（offset+dy ≤ h）。
    pub fn arm(&mut self, cmd: ScrollCmd) -> Result<(), u16> {
        if cmd.dy > MAX_STEP_ABS {
            return Err(E_SCROLL_RANGE);
        }
        let slot = match self.find_mut(cmd.surf) {
            Some(s) => s,
            None => return Err(E_SURF_UNKNOWN),
        };
        if slot.state != ScrollState::Idle {
            return Err(E_STATE_EDGE);
        }
        if slot.offset.saturating_add(cmd.dy) > slot.desc.h as u32 {
            return Err(E_SCROLL_RANGE);
        }
        slot.pending = cmd.dy;
        slot.state = ScrollState::Armed;
        Ok(())
    }

    /// 推进一帧：Armed 表面按登记序分配预算（低槽位优先），
    /// InFlight 完成位移后入 Latched；随后 present 清账 Latched→Idle。
    ///
    /// 返回本帧实际应用总行数。
    pub fn tick(&mut self) -> Result<u32, u16> {
        self.frame = self.frame.wrapping_add(1);
        // 1) Armed→InFlight：逐个尝试获得预算。
        let mut budget: u32 = FRAME_ROW_BUDGET;
        let order: Vec<u16> = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.state == ScrollState::Armed)
            .map(|s| s.desc.id)
            .collect();
        for id in order {
            let slot = match self.find_mut(id) {
                Some(s) => s,
                None => return Err(E_SURF_UNKNOWN),
            };
            if slot.state != ScrollState::Armed {
                return Err(E_STATE_EDGE);
            }
            let want = slot.pending.min(slot.desc.max_step);
            if want <= budget {
                budget -= want;
                slot.state = ScrollState::InFlight;
            }
            // 预算不足者留在 Armed，等下一帧（仲裁拒绝不报错）。
        }
        // 2) InFlight→Latched：执行位移并记账。
        let mut granted_rows: u32 = 0;
        let flight: Vec<u16> = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.state == ScrollState::InFlight)
            .map(|s| s.desc.id)
            .collect();
        for id in flight {
            let slot = match self.find_mut(id) {
                Some(s) => s,
                None => return Err(E_SURF_UNKNOWN),
            };
            let step = slot.pending.min(slot.desc.max_step);
            if slot.offset.saturating_add(step) > slot.desc.h as u32 {
                // 越界：入撕裂态，丢弃位移（回滚由 rollback 统一处理）。
                slot.state = ScrollState::Torn;
                continue;
            }
            slot.offset += step;
            slot.pending = 0;
            granted_rows += step;
            slot.state = ScrollState::Latched;
        }
        self.applied_total += granted_rows as u64;
        // 3) Latched→Idle：呈现完成，清状态。
        let latched: Vec<u16> = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.state == ScrollState::Latched)
            .map(|s| s.desc.id)
            .collect();
        for id in latched {
            let slot = match self.find_mut(id) {
                Some(s) => s,
                None => return Err(E_SURF_UNKNOWN),
            };
            slot.state = ScrollState::Idle;
        }
        // 4) Torn→Idle：回滚（pending 清零，无位移发生）。
        let torn: Vec<u16> = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.state == ScrollState::Torn)
            .map(|s| s.desc.id)
            .collect();
        for id in torn {
            let slot = match self.find_mut(id) {
                Some(s) => s,
                None => return Err(E_SURF_UNKNOWN),
            };
            slot.pending = 0;
            slot.state = ScrollState::Idle;
        }
        Ok(granted_rows)
    }

    /// 守恒校验：applied_total == Σ offset 且 count == 占用槽位数。
    pub fn audit(&self) -> Result<(), u16> {
        let mut occupied = 0usize;
        let mut sum: u64 = 0;
        for s in self.slots.iter().flatten() {
            occupied += 1;
            sum += s.offset as u64;
        }
        if occupied != self.count {
            return Err(E_LEDGER);
        }
        if sum != self.applied_total {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 只读快照：全部表面（号，状态，偏移）。
    pub fn snapshot(&self) -> Vec<(u16, ScrollState, u32)> {
        self.slots
            .iter()
            .flatten()
            .map(|s| (s.desc.id, s.state, s.offset))
            .collect()
    }

    /// 当前帧号。
    pub fn frame_no(&self) -> u32 {
        self.frame
    }

    /// 累计已应用行数。
    pub fn applied(&self) -> u64 {
        self.applied_total
    }

    /// 已登记表面数。
    pub fn registered(&self) -> usize {
        self.count
    }
}

/// 预算分配纯函数（仲裁核心的独立可测面）：
/// 输入需求列表（按仲裁序），输出每表面对本帧实际获批行数。
/// 恒等式：Σ granted ≤ budget 且 granted[i] ≤ min(want[i], cap[i])。
pub fn grant_split(wants: &[u32], caps: &[u32], budget: u32) -> Result<Vec<u32>, u16> {
    if wants.len() != caps.len() {
        return Err(E_BUDGET);
    }
    let mut left = budget;
    let mut out = Vec::with_capacity(wants.len());
    for i in 0..wants.len() {
        let want = wants[i].min(caps[i]);
        let g = if want <= left { want } else { left };
        left -= g;
        out.push(g);
    }
    Ok(out)
}

/// 自例程：构造样本数据走主路径 + 错误路径，0=全过。
///
/// 主路径：登记 3 表面 → 武装 → tick 两帧 → 守恒审计。
/// 错误路径：重号登记 / 未登记引用 / 非法边 / 越界 dy / 账本审计。
pub fn drill() -> Result<u32, u16> {
    let mut c = ScrollCon::new();
    c.register(SurfDesc { id: 1, w: 320, h: 4096, max_step: 64 })?;
    c.register(SurfDesc { id: 2, w: 320, h: 2048, max_step: 32 })?;
    c.register(SurfDesc { id: 3, w: 480, h: 1024, max_step: 16 })?;
    // 主路径：三表面并发滚动。
    c.arm(ScrollCmd { surf: 1, dy: 64, frame: 0 })?;
    c.arm(ScrollCmd { surf: 2, dy: 32, frame: 0 })?;
    c.arm(ScrollCmd { surf: 3, dy: 16, frame: 0 })?;
    let g1 = c.tick()?;
    if g1 != 112 {
        return Err(E_LEDGER); // 64+32+16 应全获批
    }
    c.arm(ScrollCmd { surf: 1, dy: 64, frame: 1 })?;
    let g2 = c.tick()?;
    if g2 != 64 {
        return Err(E_LEDGER);
    }
    c.audit()?;
    if c.snapshot().len() != 3 || c.registered() != 3 {
        return Err(E_LEDGER);
    }
    if c.applied() != 176 {
        return Err(E_LEDGER);
    }
    // 错误路径 1：重号登记。
    if c.register(SurfDesc { id: 1, w: 1, h: 1, max_step: 1 }) != Err(E_SURF_DUP) {
        return Err(E_SURF_DUP);
    }
    // 错误路径 2：未登记表面武装。
    if c.arm(ScrollCmd { surf: 99, dy: 1, frame: 2 }) != Err(E_SURF_UNKNOWN) {
        return Err(E_SURF_UNKNOWN);
    }
    // 错误路径 3：非法边（对刚 tick 完处于 Idle 的表面重复 present 语义，
    // 用非 Idle 边探测：InFlight 不可能直接武装，此处用 Armed→Armed 探测
    // ——先武装 2 号，再对同一表面武装应拒绝）。
    c.arm(ScrollCmd { surf: 2, dy: 8, frame: 2 })?;
    if c.arm(ScrollCmd { surf: 2, dy: 8, frame: 2 }) != Err(E_STATE_EDGE) {
        return Err(E_STATE_EDGE);
    }
    let _ = c.tick()?;
    // 错误路径 4：越界 dy（超绝对上限）。
    if c.arm(ScrollCmd { surf: 3, dy: MAX_STEP_ABS + 1, frame: 3 }) != Err(E_SCROLL_RANGE) {
        return Err(E_SCROLL_RANGE);
    }
    // 错误路径 5：offset+dy 超 h。
    c.arm(ScrollCmd { surf: 3, dy: 16, frame: 3 })?;
    let _ = c.tick()?;
    let rest = 1024 - 16; // 已滚 16，剩 1008
    let _ = rest;
    // 连滚到底触发越界：把 3 号滚到边界外。
    loop {
        match c.arm(ScrollCmd { surf: 3, dy: 16, frame: 4 }) {
            Ok(()) => {
                let _ = c.tick()?;
            }
            Err(E_SCROLL_RANGE) => break,
            Err(e) => return Err(e),
        }
    }
    c.audit()?;
    // 预算分配纯函数：3 表面争 64 行预算，cap 依次 32/16/48。
    let g = grant_split(&[64, 64, 64], &[32, 16, 48], 64)?;
    if g != vec![32, 16, 16] {
        return Err(E_BUDGET);
    }
    // 预算不足情形：总预算 40，低号优先。
    let g2 = grant_split(&[16, 16, 16], &[16, 16, 16], 40)?;
    if g2 != vec![16, 16, 8] {
        return Err(E_BUDGET);
    }
    Ok(0)
}
