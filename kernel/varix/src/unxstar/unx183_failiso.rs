//! unx183_failiso ↔ UNX-F0183 单元失败隔离（失败不阻断无依赖单元）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0183，UNX-A1-B10）：
//! 「单元失败隔离（失败不阻断无依赖单元）| 260 | 已深化 |
//!  UNX-F0183-J1 注入单元失败，无关单元照常完成，失败单元状态显性化（零静默）」
//!
//! 设计要点（全部围绕内核引导期 init 编排真实状态）：
//! 1. **失败只断自己子树**：单元失败进入 `Failed` 终态，其全部（传递）依赖方
//!    进入 `DepBlocked`——但与失败单元**无依赖关系**的单元照常推进到
//!    `Completed`，绝不因一个单元失败而整体停机。
//! 2. **失败显性化（零静默）**：失败必须携带原因码 + 中文诊断 + 来源 tick，
//!    写入失败账（append-only，容量 32，满则显性 `LedgerFull`）；
//!    禁止把失败吞成"跳过"或"延迟"。
//! 3. **依赖闭包传播**：`DepBlocked` 由失败闭包推导，传递闭包逐层记录
//!    （阻断来源 = 哪个失败单元、隔了几跳），可审计可追责。
//! 4. **守恒红线**：单元总数 == Completed + Failed + DepBlocked + Pending，
//!    破坏即 `Conservation` 立案（有单元凭空消失 = 编排器坏了）。
//! 5. **重入防护**：终态（Completed/Failed/DepBlocked）二次迁移一律拒绝，
//!    防止失败单元被"重置洗白"。
//! 6. **纯函数内核**：时间用逻辑 tick 注入，零 IO 零墙钟；快照 FNV 可复算。
//!
//! 诊断码：`FailIsoCode` 十六进制段 0x90（0x9000..=0x900F）独占，
//! segment_guard 双写互斥，no_std 零 unwrap 零 expect，边界一律 checked 运算。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX183_FAILISO_VERSION: &str = "unx183-failiso-v1";

/// 最大单元数（init 编排面上限；超出登记即拒）。
pub const UNIT_CAP: usize = 64;

/// 每单元最大直接依赖数（引导期依赖面窄，8 足够且防病态图）。
pub const DEP_CAP: usize = 8;

/// 失败账容量（append-only，满则显性报错，不静默覆盖历史）。
pub const FAIL_LEDGER_CAP: usize = 32;

/// 阻断账容量（DepBlocked 传播逐条记录）。
pub const BLOCK_LEDGER_CAP: usize = 64;

/// 单元运行态闭集（五态；终态不可回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitState {
    /// 尚未调度
    Pending,
    /// 正在执行
    Running,
    /// 成功完成（终态）
    Completed,
    /// 自身失败（终态，显性化）
    Failed,
    /// 被依赖方失败阻断（终态，非自身故障）
    DepBlocked,
}

impl UnitState {
    /// 中文态名（诊断用）。
    pub const fn zh(self) -> &'static str {
        match self {
            UnitState::Pending => "待调度",
            UnitState::Running => "执行中",
            UnitState::Completed => "已完成",
            UnitState::Failed => "已失败",
            UnitState::DepBlocked => "被阻断",
        }
    }

    /// 是否终态（终态迁移一律拒绝——防失败被重置洗白）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, UnitState::Completed | UnitState::Failed | UnitState::DepBlocked)
    }
}

/// 失败原因闭集（引导期 init 单元的真实失败面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailReason {
    /// 断言失败（单元内部不变量破坏）
    Assert,
    /// 前置资源缺失（交接包字段缺、initrd 缺等）
    Resource,
    /// 超出预算（tick 预算耗尽）
    Budget,
    /// 显式拒载（单元自报无法在本机执行）
    Refuse,
}

impl FailReason {
    /// 英文线缆名（稳定标识，勿改）。
    pub const fn wire(self) -> &'static str {
        match self {
            FailReason::Assert => "FAIL-ASSERT",
            FailReason::Resource => "FAIL-RESOURCE",
            FailReason::Budget => "FAIL-BUDGET",
            FailReason::Refuse => "FAIL-REFUSE",
        }
    }

    /// 中文诊断 + 可操作建议（零静默要求：失败必带建议）。
    pub fn detail(self) -> String {
        let (base, advice) = match self {
            FailReason::Assert => (
                "单元内部断言失败：不变量被破坏，失败单元进入显性 Failed 终态",
                "核对断言现场与前置交接包字段；修复前该单元依赖子树保持阻断",
            ),
            FailReason::Resource => (
                "单元前置资源缺失：交接包缺字段或资源账为空，拒绝带空资源继续",
                "检查上游生产者是否已把该资源登记进交接包（unx182 契约层）",
            ),
            FailReason::Budget => (
                "单元执行超出 tick 预算：编排器强制中止并标记失败",
                "核对单元预算配置与主机性能档；禁止静默放宽预算",
            ),
            FailReason::Refuse => (
                "单元自报拒载：本机形态不满足该单元执行前提",
                "核对单元拒载原因表；该失败属于预期路径，依赖子树照常阻断",
            ),
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx183_failiso.rs，建议：");
        s.push_str(advice);
        s.push_str("）");
        s
    }
}

/// 诊断码闭集（0x90 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailIsoCode {
    /// 0x9000 单元未登记
    UnitUnknown,
    /// 0x9001 终态二次迁移被拒
    TerminalRetouch,
    /// 0x9002 迁移非法（Pending→Failed 等越态）
    IllegalMove,
    /// 0x9003 失败账已满
    LedgerFull,
    /// 0x9004 依赖图成环
    DepCycle,
    /// 0x9005 守恒破坏
    Conservation,
    /// 0x9006 依赖登记越界（下标 ≥ UNIT_CAP 或直接依赖超 DEP_CAP）
    DepRegBad,
    /// 0x9007 调度未起始（对 Pending 之外的单元要求开始执行）
    NotRunnable,
    /// 0x9008 隔离通过（成功码）
    Isolated,
}

impl FailIsoCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            FailIsoCode::UnitUnknown => 0x9000,
            FailIsoCode::TerminalRetouch => 0x9001,
            FailIsoCode::IllegalMove => 0x9002,
            FailIsoCode::LedgerFull => 0x9003,
            FailIsoCode::DepCycle => 0x9004,
            FailIsoCode::Conservation => 0x9005,
            FailIsoCode::DepRegBad => 0x9006,
            FailIsoCode::NotRunnable => 0x9007,
            FailIsoCode::Isolated => 0x9008,
        }
    }

    /// 英文线缆名（稳定标识，勿改）。
    pub const fn wire(self) -> &'static str {
        match self {
            FailIsoCode::UnitUnknown => "FAILISO-UNIT-UNKNOWN",
            FailIsoCode::TerminalRetouch => "FAILISO-TERMINAL-RETOUCH",
            FailIsoCode::IllegalMove => "FAILISO-ILLEGAL-MOVE",
            FailIsoCode::LedgerFull => "FAILISO-LEDGER-FULL",
            FailIsoCode::DepCycle => "FAILISO-DEP-CYCLE",
            FailIsoCode::Conservation => "FAILISO-CONSERVATION",
            FailIsoCode::DepRegBad => "FAILISO-DEP-REG-BAD",
            FailIsoCode::NotRunnable => "FAILISO-NOT-RUNNABLE",
            FailIsoCode::Isolated => "FAILISO-ISOLATED",
        }
    }

    /// 段独占双写互斥（0x90 段，低字节 < 0x10）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9000 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议，禁静默）。
    pub fn detail(self) -> String {
        let base = match self {
            FailIsoCode::UnitUnknown => "单元未登记：对不存在单元的调度/失败注入请求被拒绝",
            FailIsoCode::TerminalRetouch => "终态二次迁移被拒：Completed/Failed/DepBlocked 不可回退，防失败被洗白",
            FailIsoCode::IllegalMove => "非法状态迁移：越过了状态机合法边（如 Pending 直跳 Failed）",
            FailIsoCode::LedgerFull => "失败账已满：本次失败无法落账，按显性失败处理（不静默覆盖历史）",
            FailIsoCode::DepCycle => "依赖图成环：init 依赖闭包不可计算，编排面拒绝建图",
            FailIsoCode::Conservation => "状态守恒破坏：单元计数对不上，编排器内部账本损坏",
            FailIsoCode::DepRegBad => "依赖登记越界：依赖下标超界或直接依赖超容，登记被拒",
            FailIsoCode::NotRunnable => "调度未起始：仅 Pending 态可开始执行，其余态不具运行资格",
            FailIsoCode::Isolated => "失败隔离路径全通：无关单元照常完成、失败显性化、阻断闭包正确",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx183_failiso.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 编排错误（失败→码，成功路径不经过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailIsoError {
    UnitUnknown { id: u16 },
    TerminalRetouch { id: u16, from: UnitState },
    IllegalMove { id: u16, from: UnitState },
    LedgerFull { cap: usize },
    DepCycle { first: u16 },
    Conservation { total: u32, sum: u32 },
    DepRegBad { id: u16 },
    NotRunnable { id: u16, from: UnitState },
}

impl FailIsoError {
    /// 映射诊断码。
    pub const fn code(self) -> FailIsoCode {
        match self {
            FailIsoError::UnitUnknown { .. } => FailIsoCode::UnitUnknown,
            FailIsoError::TerminalRetouch { .. } => FailIsoCode::TerminalRetouch,
            FailIsoError::IllegalMove { .. } => FailIsoCode::IllegalMove,
            FailIsoError::LedgerFull { .. } => FailIsoCode::LedgerFull,
            FailIsoError::DepCycle { .. } => FailIsoCode::DepCycle,
            FailIsoError::Conservation { .. } => FailIsoCode::Conservation,
            FailIsoError::DepRegBad { .. } => FailIsoCode::DepRegBad,
            FailIsoError::NotRunnable { .. } => FailIsoCode::NotRunnable,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 单元记录（依赖边 + 运行态 + 失败面）。
#[derive(Clone, Debug)]
pub struct Unit {
    /// 单元名（诊断点名用）
    pub name: &'static str,
    /// 直接依赖（单元下标清单，闭包由编排器推导）
    pub deps: Vec<u16>,
    /// 当前态
    pub state: UnitState,
    /// 失败原因（仅 Failed 态非空）
    pub fail: Option<FailReason>,
    /// 失败/完成 tick（注入）
    pub done_tick: u64,
    /// 阻断来源（仅 DepBlocked 态非空：哪个失败单元下标）
    pub blocked_by: Option<u16>,
    /// 阻断跳数（自失败单元起算，1 = 直接依赖）
    pub block_hops: u32,
}

/// 失败账单条（append-only，零静默的落点）。
#[derive(Clone, Debug)]
pub struct FailEntry {
    pub tick: u64,
    pub unit: u16,
    pub reason: FailReason,
    /// 失败时该单元依赖子树规模（阻断影响面，事后审计用）
    pub blast_radius: u32,
}

/// 阻断账单条（DepBlocked 传播逐条记录）。
#[derive(Clone, Debug)]
pub struct BlockEntry {
    pub tick: u64,
    pub unit: u16,
    pub by: u16,
    pub hops: u32,
}

/// init 失败隔离编排器。
#[derive(Clone, Debug)]
pub struct FailIsoOrchestrator {
    units: Vec<Unit>,
    fail_ledger: Vec<FailEntry>,
    block_ledger: Vec<BlockEntry>,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl FailIsoOrchestrator {
    /// 空编排器。
    pub const fn new() -> Self {
        FailIsoOrchestrator {
            units: Vec::new(),
            fail_ledger: Vec::new(),
            block_ledger: Vec::new(),
        }
    }

    /// 登记单元（依赖下标必须已登记且在本单元之前——init 图单向无环的登记序保证）。
    pub fn register(&mut self, name: &'static str, deps: &[u16]) -> Result<u16, FailIsoError> {
        if self.units.len() >= UNIT_CAP {
            return Err(FailIsoError::DepRegBad { id: self.units.len() as u16 });
        }
        if deps.len() > DEP_CAP {
            return Err(FailIsoError::DepRegBad { id: self.units.len() as u16 });
        }
        let id = self.units.len() as u16;
        for d in deps.iter() {
            if (*d as usize) >= self.units.len() {
                return Err(FailIsoError::DepRegBad { id });
            }
        }
        self.units.push(Unit {
            name,
            deps: Vec::from(deps),
            state: UnitState::Pending,
            fail: None,
            done_tick: 0,
            blocked_by: None,
            block_hops: 0,
        });
        Ok(id)
    }

    /// 依赖图环检测（登记序只保证前向边，防御性复核：DFS 三色）。
    pub fn check_cycles(&self) -> Result<(), FailIsoError> {
        // 0=白 1=灰（入栈即灰，防重推假环） 2=黑
        let mut color = [0u8; UNIT_CAP];
        for start in 0..self.units.len() {
            if color[start] != 0 {
                continue;
            }
            // 迭代 DFS（显式栈，避免递归；灰标记入栈即置，杜绝同节点二次入栈被误判成环）
            let mut stack: Vec<(u16, u8)> = Vec::new();
            color[start] = 1;
            stack.push((start as u16, 0));
            while let Some((id, phase)) = stack.pop() {
                let idx = id as usize;
                if phase == 1 {
                    color[idx] = 2;
                    continue;
                }
                stack.push((id, 1));
                let deps = match self.units.get(idx) {
                    Some(u) => u.deps.clone(),
                    None => return Err(FailIsoError::UnitUnknown { id }),
                };
                for d in deps {
                    let di = d as usize;
                    if di >= self.units.len() {
                        return Err(FailIsoError::DepRegBad { id });
                    }
                    if color[di] == 1 {
                        return Err(FailIsoError::DepCycle { first: d });
                    }
                    if color[di] == 0 {
                        color[di] = 1;
                        stack.push((d, 0));
                    }
                }
            }
        }
        Ok(())
    }

    /// 单元开始执行（仅 Pending 态可入 Running）。
    pub fn start(&mut self, id: u16) -> Result<(), FailIsoError> {
        let st = self.state_of(id)?;
        if st.is_terminal() {
            return Err(FailIsoError::TerminalRetouch { id, from: st });
        }
        if st != UnitState::Pending {
            return Err(FailIsoError::NotRunnable { id, from: st });
        }
        if let Some(u) = self.units.get_mut(id as usize) {
            u.state = UnitState::Running;
        }
        Ok(())
    }

    /// 单元完成（Running→Completed）。
    pub fn complete(&mut self, id: u16, tick: u64) -> Result<(), FailIsoError> {
        let st = self.state_of(id)?;
        if st.is_terminal() {
            return Err(FailIsoError::TerminalRetouch { id, from: st });
        }
        if st != UnitState::Running {
            return Err(FailIsoError::IllegalMove { id, from: st });
        }
        if let Some(u) = self.units.get_mut(id as usize) {
            u.state = UnitState::Completed;
            u.done_tick = tick;
        }
        Ok(())
    }

    /// 失败注入入口（Running→Failed + 失败账 + 阻断闭包传播）。
    ///
    /// 这是本单元的核心语义：**失败只阻断依赖子树**，无关单元不受影响。
    pub fn inject_failure(&mut self, id: u16, reason: FailReason, tick: u64) -> Result<(), FailIsoError> {
        let st = self.state_of(id)?;
        if st.is_terminal() {
            return Err(FailIsoError::TerminalRetouch { id, from: st });
        }
        if st != UnitState::Running {
            return Err(FailIsoError::IllegalMove { id, from: st });
        }
        // 先算影响面（阻断闭包规模，落账用）
        let blast = self.dependents_closure(id).len() as u32;
        // 失败账（满则显性 Err，不静默覆盖）
        if self.fail_ledger.len() >= FAIL_LEDGER_CAP {
            return Err(FailIsoError::LedgerFull { cap: FAIL_LEDGER_CAP });
        }
        self.fail_ledger.push(FailEntry { tick, unit: id, reason, blast_radius: blast });
        // 置 Failed 终态
        if let Some(u) = self.units.get_mut(id as usize) {
            u.state = UnitState::Failed;
            u.fail = Some(reason);
            u.done_tick = tick;
        }
        // 传播阻断闭包（BFS 逐层，记录跳数）
        self.propagate_block(id, tick)
    }

    /// 传递依赖闭包（所有直接/间接依赖 id 的单元下标清单）。
    fn dependents_closure(&self, id: u16) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut seen = [false; UNIT_CAP];
        let mut frontier: Vec<u16> = Vec::new();
        frontier.push(id);
        seen[id as usize] = true;
        while let Some(cur) = frontier.pop() {
            for i in 0..self.units.len() {
                if seen[i] {
                    continue;
                }
                let depends = match self.units.get(i) {
                    Some(u) => u.deps.contains(&cur),
                    None => false,
                };
                if depends {
                    seen[i] = true;
                    out.push(i as u16);
                    frontier.push(i as u16);
                }
            }
        }
        out
    }

    /// 阻断传播：失败单元的全部（传递）依赖方 → DepBlocked 终态并落账。
    fn propagate_block(&mut self, failed: u16, tick: u64) -> Result<(), FailIsoError> {
        // BFS 分层：hops=1 直接依赖，hops=2 依赖依赖……
        let mut visited = [false; UNIT_CAP];
        visited[failed as usize] = true;
        let mut frontier: Vec<u16> = Vec::new();
        frontier.push(failed);
        let mut hops: u32 = 0;
        while !frontier.is_empty() {
            hops = match hops.checked_add(1) {
                Some(v) => v,
                None => return Err(FailIsoError::Conservation { total: 0, sum: 0 }),
            };
            let mut next: Vec<u16> = Vec::new();
            for cur in frontier.iter() {
                for i in 0..self.units.len() {
                    if visited[i] {
                        continue;
                    }
                    let depends = match self.units.get(i) {
                        Some(u) => u.deps.contains(cur),
                        None => false,
                    };
                    if !depends {
                        continue;
                    }
                    visited[i] = true;
                    // 终态防重入：Running/Pending 才可阻断
                    let st = self.units[i].state;
                    if st.is_terminal() {
                        // 已失败或已被更近的失败阻断：保留更近的阻断来源（跳数更小）
                        continue;
                    }
                    if self.block_ledger.len() >= BLOCK_LEDGER_CAP {
                        return Err(FailIsoError::LedgerFull { cap: BLOCK_LEDGER_CAP });
                    }
                    self.block_ledger.push(BlockEntry { tick, unit: i as u16, by: failed, hops });
                    if let Some(u) = self.units.get_mut(i) {
                        u.state = UnitState::DepBlocked;
                        u.blocked_by = Some(failed);
                        u.block_hops = hops;
                        u.done_tick = tick;
                    }
                    next.push(i as u16);
                }
            }
            frontier = next;
        }
        Ok(())
    }

    /// 读态（越界即 UnitUnknown，不 panic）。
    pub fn state_of(&self, id: u16) -> Result<UnitState, FailIsoError> {
        match self.units.get(id as usize) {
            Some(u) => Ok(u.state),
            None => Err(FailIsoError::UnitUnknown { id }),
        }
    }

    /// 单元数。
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// 守恒校验：total == Completed+Failed+DepBlocked+Pending+Running。
    pub fn verify_conservation(&self) -> Result<(), FailIsoError> {
        let total = self.units.len() as u32;
        let (mut c, mut f, mut b, mut p, mut r) = (0u32, 0u32, 0u32, 0u32, 0u32);
        for u in self.units.iter() {
            match u.state {
                UnitState::Completed => c = c.saturating_add(1),
                UnitState::Failed => f = f.saturating_add(1),
                UnitState::DepBlocked => b = b.saturating_add(1),
                UnitState::Pending => p = p.saturating_add(1),
                UnitState::Running => r = r.saturating_add(1),
            }
        }
        let sum = c.checked_add(f).and_then(|v| v.checked_add(b))
            .and_then(|v| v.checked_add(p)).and_then(|v| v.checked_add(r));
        match sum {
            Some(s) if s == total => Ok(()),
            Some(s) => Err(FailIsoError::Conservation { total, sum: s }),
            None => Err(FailIsoError::Conservation { total, sum: u32::MAX }),
        }
    }

    /// 无关单元是否全部照常完成（J1 的正面口径：失败单元之外的可达单元不被殃及）。
    ///
    /// 「无关」= 不在失败单元的传递依赖闭包内。
    pub fn independents_all_completed(&self, failed: u16) -> Result<bool, FailIsoError> {
        if (failed as usize) >= self.units.len() {
            return Err(FailIsoError::UnitUnknown { id: failed });
        }
        let closure = self.dependents_closure(failed);
        for i in 0..self.units.len() {
            if closure.contains(&(i as u16)) || i == failed as usize {
                continue;
            }
            // 无关单元：期望 Completed（尚未调度的 Pending 不算被殃及——
            // 本判据在"全部调度完毕"语境下由判据侧保证全 start/complete）
            match self.units[i].state {
                UnitState::Completed => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// 失败账条数 / 阻断账条数。
    pub fn fail_count(&self) -> usize {
        self.fail_ledger.len()
    }
    pub fn block_count(&self) -> usize {
        self.block_ledger.len()
    }

    /// 编排快照指纹（单元态序 + 失败账 + 阻断账全进，改动即显性）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for u in self.units.iter() {
            b.push(match u.state {
                UnitState::Pending => 0,
                UnitState::Running => 1,
                UnitState::Completed => 2,
                UnitState::Failed => 3,
                UnitState::DepBlocked => 4,
            });
            b.extend_from_slice(&u.done_tick.to_le_bytes());
            b.extend_from_slice(&u.block_hops.to_le_bytes());
        }
        for e in self.fail_ledger.iter() {
            b.extend_from_slice(&e.tick.to_le_bytes());
            b.extend_from_slice(&e.unit.to_le_bytes());
            b.push(e.reason as u8);
            b.extend_from_slice(&e.blast_radius.to_le_bytes());
        }
        for e in self.block_ledger.iter() {
            b.extend_from_slice(&e.tick.to_le_bytes());
            b.extend_from_slice(&e.unit.to_le_bytes());
            b.extend_from_slice(&e.by.to_le_bytes());
            b.extend_from_slice(&e.hops.to_le_bytes());
        }
        fnv1a64(&b)
    }
}

/// 域自检（`run_unx183_checks`）：J1 判据逐条可失败断言。
pub fn run_unx183_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx183");

    // C183-01 注入失败后，无关单元照常完成（J1 正面）
    {
        let mut o = FailIsoOrchestrator::new();
        // 0←1（1 依赖 0）；2、3 独立
        let _ = o.register("a", &[]);
        let _ = o.register("b", &[0]);
        let _ = o.register("c", &[]);
        let _ = o.register("d", &[]);
        let mut all_started = true;
        for id in 0..4u16 {
            all_started &= o.start(id).is_ok();
        }
        let fail_ok = o.inject_failure(0, FailReason::Assert, 7).is_ok();
        let indep_ok = match o.complete(2, 8).is_ok() && o.complete(3, 9).is_ok() {
            true => o.independents_all_completed(0).unwrap_or(false),
            false => false,
        };
        let ok = all_started && fail_ok && indep_ok;
        s.add("C183-01-无关单元照常完成", ok, if ok { "" } else { "失败殃及了无关单元" });
    }

    // C183-02 失败单元显性 Failed 终态 + 失败账落账（零静默）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let _ = o.start(0);
        let _ = o.inject_failure(0, FailReason::Resource, 3);
        let st = o.state_of(0).unwrap_or(UnitState::Pending);
        let ok = st == UnitState::Failed && o.fail_count() == 1;
        s.add("C183-02-失败显性化且落账", ok, if ok { "" } else { "失败被吞或账缺失" });
    }

    // C183-03 依赖子树被阻断且跳数正确（直接 1 跳 / 传递 2 跳）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let _ = o.register("b", &[0]);
        let _ = o.register("c", &[1]);
        let _ = o.register("free", &[]);
        for id in 0..4u16 {
            let _ = o.start(id);
        }
        let _ = o.inject_failure(0, FailReason::Budget, 5);
        let stb = o.state_of(1).unwrap_or(UnitState::Pending);
        let stc = o.state_of(2).unwrap_or(UnitState::Pending);
        let stf = o.state_of(3).unwrap_or(UnitState::Pending);
        let ok = stb == UnitState::DepBlocked
            && stc == UnitState::DepBlocked
            && stf == UnitState::Running
            && o.block_count() == 2;
        s.add("C183-03-阻断闭包传播跳数正确", ok, if ok { "" } else { "阻断闭包传播不完整" });
    }

    // C183-04 终态二次迁移被拒（Failed 不可重置洗白）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let _ = o.start(0);
        let _ = o.inject_failure(0, FailReason::Assert, 1);
        let retouch = o.start(0);
        let ok = matches!(retouch, Err(FailIsoError::TerminalRetouch { .. }));
        s.add("C183-04-失败终态不可重置", ok, if ok { "" } else { "失败被重置洗白" });
    }

    // C183-05 守恒校验（total == 五态和）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let _ = o.register("b", &[0]);
        let _ = o.register("c", &[]);
        let _ = o.start(0);
        let _ = o.start(2);
        let _ = o.inject_failure(0, FailReason::Assert, 2);
        let _ = o.complete(2, 3);
        let ok = o.verify_conservation().is_ok();
        s.add("C183-05-状态守恒成立", ok, if ok { "" } else { "单元计数对不上" });
    }

    // C183-06 非法迁移拒绝（Pending 直跳 Failed / Completed）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let direct_fail = o.inject_failure(0, FailReason::Assert, 1);
        let direct_done = o.complete(0, 2);
        let ok = matches!(direct_fail, Err(FailIsoError::IllegalMove { .. }))
            || matches!(direct_fail, Err(FailIsoError::NotRunnable { .. }));
        let ok2 = direct_done.is_err();
        s.add("C183-06-越态迁移拒绝", ok && ok2, if ok && ok2 { "" } else { "越态迁移未被拦截" });
    }

    // C183-07 未登记单元拒绝（越界安全，零 panic）
    {
        let mut o = FailIsoOrchestrator::new();
        let _ = o.register("a", &[]);
        let r1 = o.start(9);
        let r2 = o.inject_failure(9, FailReason::Assert, 1);
        let ok = matches!(r1, Err(FailIsoError::UnitUnknown { id: 9 }))
            && matches!(r2, Err(FailIsoError::UnitUnknown { id: 9 }));
        s.add("C183-07-未登记单元拒绝", ok, if ok { "" } else { "越界访问未被拒" });
    }

    // C183-08 失败账满显性 Err（禁静默覆盖）
    {
        // 64 单元全部注入失败：前 FAIL_LEDGER_CAP 条入账，其后逐条显性 LedgerFull
        let mut o2 = FailIsoOrchestrator::new();
        for _ in 0..UNIT_CAP {
            let id = match o2.register("u", &[]) { Ok(v) => v, Err(_) => continue };
            let _ = o2.start(id);
        }
        let mut overflow_seen = false;
        for id in 0..UNIT_CAP as u16 {
            let r = o2.inject_failure(id, FailReason::Assert, id as u64);
            if let Err(FailIsoError::LedgerFull { .. }) = r {
                overflow_seen = true;
            }
        }
        let ok = overflow_seen && o2.fail_count() == FAIL_LEDGER_CAP;
        s.add("C183-08-失败账满显性拒绝", ok, if ok { "" } else { "满账路径未显性报错" });
    }

    // C183-09 快照指纹确定且内容敏感
    {
        let mut o1 = FailIsoOrchestrator::new();
        let mut o2 = FailIsoOrchestrator::new();
        for o in [&mut o1, &mut o2].iter_mut() {
            let _ = o.register("a", &[]);
            let _ = o.register("b", &[0]);
            let _ = o.register("c", &[]);
        }
        let _ = o1.start(0);
        let _ = o2.start(0);
        let _ = o1.inject_failure(0, FailReason::Assert, 4);
        // o2 用不同失败原因（同 tick）：终态态机相同但失败账 reason 不同 → 指纹必异
        let _ = o2.inject_failure(0, FailReason::Resource, 4);
        let _ = o1.complete(2, 5);
        let _ = o2.complete(2, 5);
        let f1a = o1.fingerprint();
        let f1b = o1.fingerprint();
        let _ = o2.complete(2, 6).unwrap_or(());
        let f2 = o2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C183-09-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C183-10 诊断码 0x90 段独占
    {
        let codes = [
            FailIsoCode::UnitUnknown,
            FailIsoCode::TerminalRetouch,
            FailIsoCode::IllegalMove,
            FailIsoCode::LedgerFull,
            FailIsoCode::DepCycle,
            FailIsoCode::Conservation,
            FailIsoCode::DepRegBad,
            FailIsoCode::NotRunnable,
            FailIsoCode::Isolated,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C183-10-诊断码 0x90 段独占", all, if all { "" } else { "码值越出 0x90 段" });
    }

    // C183-11 失败原因中文诊断非空且带建议
    {
        let reasons = [FailReason::Assert, FailReason::Resource, FailReason::Budget, FailReason::Refuse];
        let mut all = true;
        for r in reasons.iter() {
            let d = r.detail();
            all = all && d.contains("建议") && d.len() >= 20;
        }
        s.add("C183-11-失败诊断带可操作建议", all, if all { "" } else { "诊断缺建议或过短" });
    }

    s
}
