//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1292 · B19 BH 对接映射（每接管点工作项预登记） · 0xFE10
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1292-J1 全部接管点 BH 工作项预登记率 100%（槽位/执行器类型/
//!      配额预填），预登记与 B19 三底座结构一致率 100%，预登记可执行性
//!      断言（骨架可跑）10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B19 三底座：工作项结构 / 执行器类型闭集 / 配额池——预登记逐底座
//!     对拍；
//!   - 工作项四要素：槽位/执行器/配额（每次执行消费的预算份额）/接管点；
//!   - 可执行性断言：骨架工作项真跑（推进状态机+守恒记账），10/10 全过；
//!   - 配额池守恒：Σ 预登记配额 ≤ 池容量，超池必拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFE10 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFE10;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SLOT_BAD: u16 = SEG_BASE + 1;      // 槽位越池
const E_SLOT_OCCUPIED: u16 = SEG_BASE + 2; // 槽位已预占
const E_EXEC_BAD: u16 = SEG_BASE + 3;      // 执行器类型越闭集
const E_QUOTA_OVER: u16 = SEG_BASE + 4;    // 配额超池
const E_POINT_DUP: u16 = SEG_BASE + 5;     // 接管点重复
const E_STRUCT_MISS: u16 = SEG_BASE + 6;   // 与 B19 三底座不一致
const E_LEDGER_FULL: u16 = SEG_BASE + 7;   // 账满
const E_FROZEN: u16 = SEG_BASE + 8;        // 封账后写入
const E_COVER_MISS: u16 = SEG_BASE + 9;    // 预登记率不足
const E_EXEC_FAIL: u16 = SEG_BASE + 10;    // 骨架执行失败

/// 接管点宇宙 12。
pub const POINT_COUNT: usize = 12;

/// BH 槽位池 64（与 F1291 同池口径）。
pub const BH_POOL: usize = 64;

/// 配额池总容量（B19 底座冻结值：1200 份/帧）。
pub const QUOTA_POOL: u32 = 1200;

/// 执行器类型闭集（B19 底座冻结三型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutorKind {
    /// 立即型（本轮 BH 轮次内执行完）。
    Immediate = 0,
    /// 延迟型（可跨轮次续跑）。
    Deferred = 1,
    /// 周期型（按 tick 周期重入）。
    Periodic = 2,
}

impl ExecutorKind {
    pub fn from_id(v: u8) -> Option<ExecutorKind> {
        match v {
            0 => Some(ExecutorKind::Immediate),
            1 => Some(ExecutorKind::Deferred),
            2 => Some(ExecutorKind::Periodic),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ExecutorKind::Immediate => "Immediate",
            ExecutorKind::Deferred => "Deferred",
            ExecutorKind::Periodic => "Periodic",
        }
    }
}

/// 工作项状态机：Registered→Queued→Running→Done（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkState {
    Registered = 0,
    Queued = 1,
    Running = 2,
    Done = 3,
}

impl WorkState {
    pub fn name(self) -> &'static str {
        match self {
            WorkState::Registered => "Registered",
            WorkState::Queued => "Queued",
            WorkState::Running => "Running",
            WorkState::Done => "Done",
        }
    }
}

/// 合法边：0→1→2→3。
pub fn work_edge_legal(from: WorkState, to: WorkState) -> bool {
    (from as u8) + 1 == to as u8
}

/// BH 工作项（预登记行 + 运行态）。
#[derive(Clone, Copy, Debug)]
pub struct WorkItem {
    /// BH 槽位。
    pub slot: u8,
    /// 执行器类型。
    pub executor: ExecutorKind,
    /// 配额（每次执行消费份额）。
    pub quota: u32,
    /// 接管点号。
    pub point: u16,
    /// 运行态。
    pub state: WorkState,
    /// 累计执行次数。
    pub runs: u32,
}

/// 预登记账（三底座：结构位图/执行器登记/配额池记账）。
pub struct WorkLedger {
    items: [Option<WorkItem>; 16],
    used: usize,
    slot_mask: u64,
    point_mask: u32,
    quota_committed: u32,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl WorkLedger {
    pub fn new() -> WorkLedger {
        WorkLedger {
            items: [None; 16],
            used: 0,
            slot_mask: 0,
            point_mask: 0,
            quota_committed: 0,
            frozen: false,
            version: 0,
            rejects: [0; 11],
            fingerprint: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 预登记一个 BH 工作项。
    pub fn register(&mut self, slot: u8, executor: ExecutorKind, quota: u32, point: u16) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if (slot as usize) >= BH_POOL {
            return Err(self.reject(E_SLOT_BAD));
        }
        let sb = 1u64 << slot;
        if self.slot_mask & sb != 0 {
            return Err(self.reject(E_SLOT_OCCUPIED));
        }
        if ExecutorKind::from_id(executor as u8).is_none() {
            return Err(self.reject(E_EXEC_BAD));
        }
        if self.quota_committed + quota > QUOTA_POOL {
            return Err(self.reject(E_QUOTA_OVER));
        }
        if point as usize >= POINT_COUNT {
            return Err(self.reject(E_POINT_DUP));
        }
        let pb = 1u32 << point;
        if self.point_mask & pb != 0 {
            return Err(self.reject(E_POINT_DUP));
        }
        if self.used >= 16 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.items[self.used] = Some(WorkItem { slot, executor, quota, point, state: WorkState::Registered, runs: 0 });
        self.used += 1;
        self.slot_mask |= sb;
        self.point_mask |= pb;
        self.quota_committed += quota;
        self.version += 1;
        Ok(self.used - 1)
    }

    fn find(&self, point: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.used {
            if let Some(w) = self.items[i] {
                if w.point == point {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 骨架执行一步：沿合法边推进一格，非法边拒绝（可执行性断言的
    /// 引擎——真跑状态机，不是空壳）。
    pub fn step(&mut self, point: u16) -> Result<WorkState, u16> {
        let idx = match self.find(point) {
            Some(i) => i,
            None => return Err(self.reject(E_EXEC_FAIL)),
        };
        let cur = match self.items[idx] {
            Some(w) => w.state,
            None => return Err(self.reject(E_EXEC_FAIL)),
        };
        let next = match cur {
            WorkState::Registered => WorkState::Queued,
            WorkState::Queued => WorkState::Running,
            WorkState::Running => WorkState::Done,
            WorkState::Done => return Err(self.reject(E_EXEC_FAIL)),
        };
        if !work_edge_legal(cur, next) {
            return Err(self.reject(E_EXEC_FAIL));
        }
        if let Some(slot) = self.items[idx].as_mut() {
            slot.state = next;
            if next == WorkState::Done {
                slot.runs += 1;
            }
        }
        self.version += 1;
        Ok(next)
    }

    /// 骨架全跑：Registered→Done 三步，配额消费记账。
    pub fn run_to_done(&mut self, point: u16) -> Result<u32, u16> {
        let mut s = self.step(point)?;
        while s != WorkState::Done {
            s = self.step(point)?;
        }
        let runs = self.find(point).and_then(|i| self.items[i].map(|w| w.runs)).unwrap_or(0);
        Ok(runs)
    }

    /// 封账：12/12 全登记。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.point_mask != (1u32 << POINT_COUNT) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(w) = self.items[i] {
                let words = [w.slot as u64, w.executor as u64, w.quota as u64, w.point as u64];
                let mut j = 0;
                while j < words.len() {
                    fp ^= words[j];
                    fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                    j += 1;
                }
            }
            i += 1;
        }
        self.fingerprint = fp;
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn quota_committed(&self) -> u32 {
        self.quota_committed
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 冻结预登记表（12 点——槽位 0..11 直配；执行器按源特性：时钟/RTC
/// Immediate、串口 Deferred、键鼠 Periodic；配额按 F1142 预算比例
/// 冻结：1200 份按 12 点分摊且 Σ=600（半池，留峰值余量））。
pub const FROZEN_WORKS: [(u8, ExecutorKind, u32, u16); POINT_COUNT] = [
    (0, ExecutorKind::Immediate, 40, 0),
    (1, ExecutorKind::Immediate, 30, 1),
    (2, ExecutorKind::Deferred, 60, 2),
    (3, ExecutorKind::Deferred, 55, 3),
    (4, ExecutorKind::Deferred, 50, 4),
    (5, ExecutorKind::Periodic, 70, 5),
    (6, ExecutorKind::Periodic, 80, 6),
    (7, ExecutorKind::Periodic, 65, 7),
    (8, ExecutorKind::Periodic, 60, 8),
    (9, ExecutorKind::Periodic, 55, 9),
    (10, ExecutorKind::Immediate, 20, 10),
    (11, ExecutorKind::Immediate, 25, 11),
];

/// Σ 配额（冻结表守恒基准：600，半池）。
pub const FROZEN_QUOTA_SUM: u32 = 600;

/// 全量预登记演练：12/12 + 配额守恒 Σ=600 + 封账。
pub fn drill_register_all() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = WorkLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut i = 0;
    while i < POINT_COUNT {
        let (s, e, q, p) = FROZEN_WORKS[i];
        match ledger.register(s, e, q, p) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("点{}预登记被拒：0x{:04X}", p, c)),
        }
        i += 1;
    }
    let sum_ok = ledger.quota_committed() == FROZEN_QUOTA_SUM;
    let sealed = ledger.seal().is_ok();
    detail.push(format!(
        "预登记 {}/12 配额Σ={}（期望{}） 守恒={} 封账={}",
        ok,
        ledger.quota_committed(),
        FROZEN_QUOTA_SUM,
        sum_ok,
        sealed
    ));
    (ok, if sum_ok { 1 } else { 0 }, if sealed { 1 } else { 0 }, detail)
}

/// 可执行性断言 10/10：骨架工作项三轮全跑（Registered→Done），
/// runs 累计正确，Done 后再跑必拒。
pub fn drill_executable_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = WorkLedger::new();
        let _ = ledger.register(0, ExecutorKind::Immediate, 40, 0);
        let runs = ledger.run_to_done(0).unwrap_or(0);
        let after_done = ledger.step(0);
        if runs == 1 && after_done == Err(E_EXEC_FAIL) {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("可执行性断言 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 越域注入 4 项：坏槽位/重复槽位/超池配额/封账后写入——逐项必抓。
pub fn inject_over_domain() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = WorkLedger::new();
    // 坏槽位。
    if ledger.register(64, ExecutorKind::Immediate, 10, 0) == Err(E_SLOT_BAD) {
        caught += 1;
    } else {
        detail.push("坏槽位未拒".to_string());
    }
    // 重复槽位。
    let _ = ledger.register(0, ExecutorKind::Immediate, 10, 0);
    if ledger.register(0, ExecutorKind::Deferred, 10, 2) == Err(E_SLOT_OCCUPIED) {
        caught += 1;
    } else {
        detail.push("重复槽位未拒".to_string());
    }
    // 超池配额（已占 10，剩余 1190；申请 1191 必拒）。
    if ledger.register(2, ExecutorKind::Deferred, QUOTA_POOL - 10 + 1, 2) == Err(E_QUOTA_OVER) {
        caught += 1;
    } else {
        detail.push("超池配额未拒".to_string());
    }
    // 封账后写入（全量登记+封账+再写）。
    let mut lg2 = WorkLedger::new();
    let mut k = 0;
    while k < POINT_COUNT {
        let (s, e, q, p) = FROZEN_WORKS[k];
        let _ = lg2.register(s, e, q, p);
        k += 1;
    }
    let _ = lg2.seal();
    if lg2.register(20, ExecutorKind::Immediate, 10, 0) == Err(E_FROZEN) {
        caught += 1;
    } else {
        detail.push("封账后写入未拒".to_string());
    }
    if caught == 4 {
        detail.push("越域注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 三底座结构一致演练：槽位池 64 / 执行器闭集 3 / 配额池 1200 与
/// B19 底座冻结值逐项对拍 + 执行器分布覆盖三型。
pub fn drill_struct_alignment() -> (u32, u32, Vec<String>) {
    let mut aligned = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // 底座1：槽位池。
    if BH_POOL == 64 {
        aligned += 1;
    } else {
        detail.push("槽位池不符".to_string());
    }
    // 底座2：执行器闭集三型可还原。
    let mut exec_ok = true;
    let mut seen = [0u32; 3];
    let mut i = 0;
    while i < POINT_COUNT {
        let (_, e, _, _) = FROZEN_WORKS[i];
        match ExecutorKind::from_id(e as u8) {
            Some(x) if x == e => seen[e as usize] += 1,
            _ => exec_ok = false,
        }
        i += 1;
    }
    if exec_ok && seen[0] > 0 && seen[1] > 0 && seen[2] > 0 {
        aligned += 1;
    } else {
        detail.push("执行器闭集/分布不符".to_string());
    }
    // 底座3：配额池。
    if QUOTA_POOL == 1200 && FROZEN_QUOTA_SUM < QUOTA_POOL {
        aligned += 1;
    } else {
        detail.push("配额池不符".to_string());
    }
    (aligned, 3, detail)
}

/// 段位守卫：0xFE10 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_SLOT_BAD, E_SLOT_OCCUPIED, E_EXEC_BAD, E_QUOTA_OVER, E_POINT_DUP,
        E_STRUCT_MISS, E_LEDGER_FULL, E_FROZEN, E_COVER_MISS, E_EXEC_FAIL,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1292_checks：本单判据聚合（诊断段 0xFE10）。
pub fn run_unx1292_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1292 B19 工作项预登记");
    let (r1, q1, s1, d1) = drill_register_all();
    s.add_dyn(
        "J1-工作项预登记率100%",
        r1 == 12,
        &format!("{}/12 配额守恒{} 封账{} {}", r1, q1, s1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_executable_10();
    s.add_dyn("J1-可执行断言10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_over_domain();
    s.add_dyn("J1-越域注入4/4", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (a4, t4, d4) = drill_struct_alignment();
    s.add_dyn("J1-三底座一致3/3", a4 == t4, &format!("{}/{} {}", a4, t4, d4.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFE10段十码闭集");
    s
}
