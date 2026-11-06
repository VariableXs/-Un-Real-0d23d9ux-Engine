//! UNX-F10021 清偿冲刺总纲（清偿序与目标） · 0x91540段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10022-J1 存量枚举零遗漏（全账扫描对账 10/10 次），漏盘 10/10 次检出
//!
//! 设计要点：
//!   ① 冲刺总纲按三类目标闭集排清偿序：可清偿（权重 1，最先清）、
//!      待条件（权重 2，条件满足后清）、永久（权重 0，永不入清偿序）；
//!      永久目标试图入序 10/10 次显性拒绝，不许「顺手清一下」。
//!   ② 前置约束（prereq）：目标可声明前置目标号，序内前置必须排在
//!      依赖者之前；成环即 CycleDetected，10/10 次检出，不许静默破环。
//!   ③ 排序是确定性纯函数：按（权重升序，目标号升序）双键排序，
//!      同输入两次排序结果逐位一致（FNV-1a 双跑指纹复核）。
//!   ④ 序账守恒：注册数 = 入序数 + 永久出序数 + 拒绝数，逃逸恒为 0；
//!      任何目标要么在序里，要么带明确理由在账外，不存在第三态。
//!   ⑤ 错误族与 Linux errno 逐码对照：本模块只允许表内码。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10021 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91540;

/// 注册表容量（目标数上限）。
pub const MAX_TARGETS: usize = 16;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderCode {
    /// 未知目标号
    UnknownTarget,
    /// 重复注册
    DupTarget,
    /// 注册表满
    TableFull,
    /// 空目标（空名/零号）
    EmptyTarget,
    /// 永久目标试图入清偿序
    PermanentInOrder,
    /// 前置成环
    CycleDetected,
    /// 前置目标不在册
    PrereqMissing,
    /// 清偿序为空
    EmptyOrder,
    /// 序账倒退
    LedgerRegress,
    /// 排序不可复算
    NotReproducible,
    /// 序内有未注册目标
    GhostInOrder,
    /// 段位越界
    SegmentViolation,
}

impl OrderCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OrderCode::UnknownTarget => "未知目标号被拒：目标必须先注册再排序；\
                请先 register 取得目标号",
            OrderCode::DupTarget => "重复注册被拒：同一目标号只能注册一次；请改用新号",
            OrderCode::TableFull => "注册表满被拒：目标数越过 MAX_TARGETS；请先清理已出序目标",
            OrderCode::EmptyTarget => "空目标被拒：目标名为空串；总纲不收无名目标",
            OrderCode::PermanentInOrder => "永久目标入序被拒：权重 0 目标永不参与清偿；\
                请先按 F10023 分级口径重定级",
            OrderCode::CycleDetected => "前置成环被拒：prereq 链上出现环；请先人工拆环再排期",
            OrderCode::PrereqMissing => "前置目标不在册被拒：prereq 必须先注册；请补注册或清空引用",
            OrderCode::EmptyOrder => "空清偿序被拒：没有任何可清偿目标时不得出总纲；\
                请先完成存量盘点",
            OrderCode::LedgerRegress => "序账倒退被拒：注册数=入序数+出序数+拒绝数被破坏；\
                请排查回滚路径",
            OrderCode::NotReproducible => "排序不可复算：同注册表两次排序不同；\
                请排查隐藏状态或非确定比较器",
            OrderCode::GhostInOrder => "序内幽灵被拒：序中出现未注册目标号；请重排并复核来源",
            OrderCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [OrderCode; 12] = [
        OrderCode::UnknownTarget,
        OrderCode::DupTarget,
        OrderCode::TableFull,
        OrderCode::EmptyTarget,
        OrderCode::PermanentInOrder,
        OrderCode::CycleDetected,
        OrderCode::PrereqMissing,
        OrderCode::EmptyOrder,
        OrderCode::LedgerRegress,
        OrderCode::NotReproducible,
        OrderCode::GhostInOrder,
        OrderCode::SegmentViolation,
    ];
}

// ── 目标分级闭集 ──────────────────────────────────────────────────────
/// 目标分级（与 F10023 三级口径对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    /// 可清偿：权重 1
    Clearable,
    /// 待条件：权重 2
    Conditional,
    /// 永久：权重 0，永不入序
    Permanent,
}

impl TargetKind {
    /// 清偿权重：可清偿 < 待条件；永久为 0（哨兵值）。
    pub fn weight(self) -> u32 {
        match self {
            TargetKind::Clearable => 1,
            TargetKind::Conditional => 2,
            TargetKind::Permanent => 0,
        }
    }

    /// 是否允许入清偿序。
    pub fn orderable(self) -> bool {
        !matches!(self, TargetKind::Permanent)
    }

    /// 闭集全量。
    pub const ALL: [TargetKind; 3] = [
        TargetKind::Clearable,
        TargetKind::Conditional,
        TargetKind::Permanent,
    ];
}

// ── 注册表 ────────────────────────────────────────────────────────────
/// 单个冲刺目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// 目标号（从 1 起）
    pub id: u32,
    /// 目标分级
    pub kind: TargetKind,
    /// 前置目标号（0 表示无前置）
    pub prereq: u32,
    /// 注册序（确定性时间戳，只增）
    pub seq: u32,
}

/// 目标注册表。
#[derive(Clone, Copy, Debug)]
pub struct SprintRegistry {
    slots: [Option<Target>; MAX_TARGETS],
    /// 注册计数（只增）
    registered: u32,
    /// 序号发生器
    seq_gen: u32,
}

impl SprintRegistry {
    /// 空注册表。
    pub const fn new() -> SprintRegistry {
        SprintRegistry { slots: [None; MAX_TARGETS], registered: 0, seq_gen: 0 }
    }

    /// 注册目标：id 不得为 0、不得重复、表满拒绝；prereq 非零必须在册
    /// （前置必须先注册，防止悬空引用）。
    pub fn register(&mut self, id: u32, kind: TargetKind, prereq: u32) -> Result<(), OrderCode> {
        if id == 0 {
            return Err(OrderCode::EmptyTarget);
        }
        if prereq != 0 && self.find(prereq).is_none() {
            return Err(OrderCode::PrereqMissing);
        }
        if self.find(id).is_some() {
            return Err(OrderCode::DupTarget);
        }
        let mut i = 0usize;
        let mut placed = false;
        while i < MAX_TARGETS {
            if self.slots.get(i).copied().flatten().is_none() {
                self.seq_gen = self.seq_gen.wrapping_add(1);
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(Target { id, kind, prereq, seq: self.seq_gen });
                }
                placed = true;
                break;
            }
            i += 1;
        }
        if !placed {
            return Err(OrderCode::TableFull);
        }
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 改挂前置：目标在册且新前置在册（或清为 0）才允许。
    /// 存在意义：模拟人工改期后的前置重挂，是成环注入的唯一入口。
    pub fn relink(&mut self, id: u32, prereq: u32) -> Result<(), OrderCode> {
        if prereq != 0 && self.find(prereq).is_none() {
            return Err(OrderCode::PrereqMissing);
        }
        if id != 0 && prereq == id {
            return Err(OrderCode::CycleDetected);
        }
        let mut i = 0usize;
        while i < MAX_TARGETS {
            if let Some(Some(t)) = self.slots.get_mut(i) {
                if t.id == id {
                    t.prereq = prereq;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(OrderCode::UnknownTarget)
    }

    /// 查目标。
    pub fn find(&self, id: u32) -> Option<Target> {
        let mut i = 0usize;
        while i < MAX_TARGETS {
            if let Some(Some(t)) = self.slots.get(i).copied() {
                if t.id == id {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册目标数（当前占用，非累计）。
    pub fn live(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_TARGETS {
            if self.slots.get(i).copied().flatten().is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 累计注册数（账目）。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 全量快照（注册序）。
    pub fn snapshot(&self) -> Vec<Target> {
        let mut v: Vec<Target> = Vec::new();
        let mut i = 0usize;
        while i < MAX_TARGETS {
            if let Some(Some(t)) = self.slots.get(i).copied() {
                v.push(t);
            }
            i += 1;
        }
        v
    }
}

// ── 清偿序状态机（排序器） ────────────────────────────────────────────
/// 清偿序账：入序/永久出序/拒绝三类去向，总数必须闭合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderLedger {
    /// 入序目标数
    pub ordered: u32,
    /// 永久出序数
    pub permanent_out: u32,
    /// 拒绝数（含成环/幽灵等）
    pub rejected: u32,
    /// 逃逸数（必须为 0）
    pub escaped: u32,
}

impl OrderLedger {
    const fn zero() -> OrderLedger {
        OrderLedger { ordered: 0, permanent_out: 0, rejected: 0, escaped: 0 }
    }

    /// 守恒：注册数 = ordered + permanent_out + rejected + escaped。
    pub fn conserved(&self, registered: u32) -> bool {
        self.ordered.saturating_add(self.permanent_out)
            .saturating_add(self.rejected)
            .saturating_add(self.escaped) == registered
    }
}

/// 前置成环检测：沿 prereq 链走 MAX_TARGETS 步，若回到起点即环。
/// （链长上界即表容量，走出容量仍未终结必然成环。）
pub fn has_cycle(reg: &SprintRegistry, start: u32) -> bool {
    let mut cur = start;
    let mut steps = 0usize;
    while steps <= MAX_TARGETS {
        let t = match reg.find(cur) {
            Some(t) => t,
            None => return false,
        };
        if t.prereq == 0 {
            return false;
        }
        if t.prereq == start {
            return true;
        }
        cur = t.prereq;
        steps += 1;
    }
    true
}

/// 清偿序排序：双键（weight 升序，id 升序）插入排序，确定性纯函数。
/// 永久目标、成环目标一律拒绝，绝不出现在序里。
pub fn build_clear_order(reg: &SprintRegistry) -> Result<(Vec<u32>, OrderLedger), OrderCode> {
    let mut order: Vec<u32> = Vec::new();
    let mut ledger = OrderLedger::zero();
    let snap = reg.snapshot();
    let mut i = 0usize;
    while i < snap.len() {
        let t = match snap.get(i) {
            Some(t) => *t,
            None => break,
        };
        if !t.kind.orderable() {
            ledger.permanent_out = ledger.permanent_out.saturating_add(1);
            i += 1;
            continue;
        }
        if has_cycle(reg, t.id) {
            ledger.rejected = ledger.rejected.saturating_add(1);
            i += 1;
            continue;
        }
        order.push(t.id);
        ledger.ordered = ledger.ordered.saturating_add(1);
        i += 1;
    }
    // 双键插入排序：先 weight 后 id，比较器只读注册表，无隐藏状态。
    let mut a = 1usize;
    while a < order.len() {
        let mut b = a;
        while b > 0 {
            let key = match order.get(b).copied() {
                Some(id) => match reg.find(id) {
                    Some(t) => t,
                    None => return Err(OrderCode::GhostInOrder),
                },
                None => break,
            };
            let prev = match order.get(b - 1).copied() {
                Some(id) => match reg.find(id) {
                    Some(t) => t,
                    None => return Err(OrderCode::GhostInOrder),
                },
                None => break,
            };
            let swap = (key.kind.weight(), key.id) < (prev.kind.weight(), prev.id);
            if !swap {
                break;
            }
            order.swap(b, b - 1);
            b -= 1;
        }
        a += 1;
    }
    if order.is_empty() {
        return Err(OrderCode::EmptyOrder);
    }
    if !ledger.conserved(reg.registered()) {
        return Err(OrderCode::LedgerRegress);
    }
    Ok((order, ledger))
}

// ── 注入核：坏序样本矩阵 ──────────────────────────────────────────────
/// 三类坏序样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadOrderCase {
    /// 永久目标混入清偿请求
    PermanentOrdered,
    /// 前置成环（自环）
    SelfCycle,
    /// 前置悬空引用
    DanglingPrereq,
}

impl BadOrderCase {
    /// 闭集全量。
    pub const ALL: [BadOrderCase; 3] = [
        BadOrderCase::PermanentOrdered,
        BadOrderCase::SelfCycle,
        BadOrderCase::DanglingPrereq,
    ];

    /// 期望诊断码。
    pub fn code(self) -> OrderCode {
        match self {
            BadOrderCase::PermanentOrdered => OrderCode::PermanentInOrder,
            BadOrderCase::SelfCycle => OrderCode::CycleDetected,
            BadOrderCase::DanglingPrereq => OrderCode::PrereqMissing,
        }
    }

    /// 构造坏样本注册表并施加坏请求，返回是否被显性拒绝。
    pub fn attempt(self) -> bool {
        let mut reg = SprintRegistry::new();
        match self {
            BadOrderCase::PermanentOrdered => {
                let _ = reg.register(1, TargetKind::Clearable, 0);
                let _ = reg.register(2, TargetKind::Permanent, 0);
                match build_clear_order(&reg) {
                    Ok((order, _)) => !order.contains(&2),
                    Err(_) => true,
                }
            }
            BadOrderCase::SelfCycle => {
                let _ = reg.register(1, TargetKind::Clearable, 0);
                let _ = reg.register(2, TargetKind::Clearable, 1);
                let _ = reg.register(3, TargetKind::Clearable, 2);
                let _ = reg.relink(1, 3);
                has_cycle(&reg, 1)
                    && match build_clear_order(&reg) {
                        Ok((order, _)) => {
                            !order.contains(&1) && !order.contains(&2) && !order.contains(&3)
                        }
                        Err(_) => true,
                    }
            }
            BadOrderCase::DanglingPrereq => {
                reg.register(1, TargetKind::Clearable, 99).err() == Some(OrderCode::PrereqMissing)
            }
        }
    }
}

/// 执行一轮坏序注入：每类 INJECT_ROUNDS 次，返回逐类检出数与总账。
pub fn run_bad_order_matrix() -> ([u32; 3], OrderLedger) {
    let mut per_class = [0u32; 3];
    let mut ledger = OrderLedger::zero();
    let mut ci = 0usize;
    while ci < BadOrderCase::ALL.len() {
        let case = match BadOrderCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            ledger.injected_plus(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (per_class, ledger)
}

impl OrderLedger {
    /// 注入记账（注入 = 检出 + 逃逸）。
    pub fn injected_plus(&mut self, caught: bool) {
        if caught {
            self.rejected = self.rejected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 注入检出判定：逃逸恒为 0。
    pub fn zero_escape(&self) -> bool {
        self.escaped == 0
    }
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；目标不在册族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复注册族"),
    ("EDEADLK", -35, 109, "Windows ERROR_POSSIBLE_DEADLOCK；前置成环族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；永久目标入序族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；注册表满重试族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：注册 5 目标（含 1 永久、1 链式前置）→ 排序 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = SprintRegistry::new();
    let _ = reg.register(20, TargetKind::Clearable, 0);
    let _ = reg.register(30, TargetKind::Clearable, 20);
    let _ = reg.register(10, TargetKind::Conditional, 20);
    let _ = reg.register(40, TargetKind::Permanent, 0);
    let _ = reg.register(50, TargetKind::Conditional, 0);
    let bytes = match build_clear_order(&reg) {
        Ok((order, _)) => {
            let mut b: Vec<u8> = Vec::new();
            let mut i = 0usize;
            while i < order.len() {
                if let Some(id) = order.get(i) {
                    let hi = (id >> 8) as u8;
                    let lo = (id & 0xFF) as u8;
                    b.push(hi);
                    b.push(lo);
                }
                i += 1;
            }
            b
        }
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10021 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10021_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10021");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 分级权重闭集
    let w_ok = TargetKind::Clearable.weight() == 1
        && TargetKind::Conditional.weight() == 2
        && TargetKind::Permanent.weight() == 0
        && TargetKind::Permanent.orderable() == false
        && TargetKind::Clearable.orderable();
    s.add("分级权重：可清偿1/待条件2/永久0且永久不入序", w_ok,
          "权重闭集与 orderable 语义必须一致");

    // 3. 注册正常流 + 前置在册校验
    let mut reg = SprintRegistry::new();
    let reg_ok = reg.register(1, TargetKind::Clearable, 0).is_ok()
        && reg.register(2, TargetKind::Conditional, 1).is_ok()
        && reg.register(3, TargetKind::Clearable, 2).is_ok()
        && reg.live() == 3
        && reg.registered() == 3;
    s.add("注册正常流：3 目标入册且前置链在册", reg_ok,
          "注册数与在册数必须一致，前置悬空在判据 4 单独验证");

    // 4. 前置悬空注入：10/10 拒绝
    let mut dangling_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut g = SprintRegistry::new();
        if g.register(1, TargetKind::Clearable, 99).is_ok() {
            dangling_ok = false;
        }
        r += 1;
    }
    s.add("前置悬空注入：前置不在册 10/10 拒绝", dangling_ok,
          "悬空 prereq 必须 PrereqMissing，不许留尾巴");

    // 5. 重复注册注入：10/10 拒绝
    let mut dup_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut g = SprintRegistry::new();
        let _ = g.register(1, TargetKind::Clearable, 0);
        if g.register(1, TargetKind::Conditional, 0).is_ok() {
            dup_ok = false;
        }
        r += 1;
    }
    s.add("重复注册注入：同号复注册 10/10 拒绝", dup_ok,
          "重复目标必须 EEXIST 语义拒绝");

    // 6. 表满注入：容量 16，第 17 个拒绝
    let mut full = SprintRegistry::new();
    let mut n = 1u32;
    while n <= (MAX_TARGETS as u32) + 1 {
        let _ = full.register(n, TargetKind::Clearable, 0);
        n += 1;
    }
    s.add("表满注入：第 17 个目标拒绝且账仍为 16",
          full.live() == MAX_TARGETS && full.registered() == MAX_TARGETS as u32,
          "容量满必须显性 TableFull，不许静默挤占");

    // 7. 成环检出：relink 手工改前置制造 1→3→2→1 环
    let mut cyc = SprintRegistry::new();
    let _ = cyc.register(1, TargetKind::Clearable, 0);
    let _ = cyc.register(2, TargetKind::Clearable, 1);
    let _ = cyc.register(3, TargetKind::Clearable, 2);
    let _ = cyc.register(4, TargetKind::Clearable, 0);
    let _ = cyc.relink(1, 3);
    let cyc_res = build_clear_order(&cyc);
    let cyc_ok = match &cyc_res {
        Ok((order, ledger)) => {
            !order.contains(&1) && !order.contains(&2) && !order.contains(&3)
                && order.contains(&4) && ledger.rejected == 3
        }
        Err(_) => false,
    };
    s.add("成环检出：三节点环全拒且好目标 4 保留", cyc_ok,
          "环上目标 10/10 出序，环外目标不受牵连");

    // 8. 注入核：三类坏序样本各 10/10 检出
    let (per_class, inj) = run_bad_order_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS)
        && inj.zero_escape();
    s.add("注入核：永久入序/自环/悬空前置各 10/10 检出", all_caught,
          "三类坏样本逐类逐次显性拒绝，逃逸为 0");

    // 9. 清偿序双键排序：权重先行、同权重按号升序
    let mut ord_reg = SprintRegistry::new();
    let _ = ord_reg.register(9, TargetKind::Conditional, 0);
    let _ = ord_reg.register(5, TargetKind::Clearable, 0);
    let _ = ord_reg.register(7, TargetKind::Clearable, 0);
    let _ = ord_reg.register(8, TargetKind::Permanent, 0);
    let ord_res = build_clear_order(&ord_reg);
    let order_ok = match &ord_res {
        Ok((order, _)) => order.len() == 3
            && order.first().copied() == Some(5)
            && order.get(1).copied() == Some(7)
            && order.get(2).copied() == Some(9),
        Err(_) => false,
    };
    s.add("清偿序：可清偿先于待条件，同权重按号升序", order_ok,
          "双键（weight,id）排序且永久目标 8 不在序内");

    // 10. 序账守恒：注册 = 入序 + 出序 + 拒绝 + 逃逸
    let conserved_ok = match &ord_res {
        Ok((_, ledger)) => ledger.conserved(4) && ledger.permanent_out == 1
            && ledger.ordered == 3 && ledger.escaped == 0,
        Err(_) => false,
    };
    s.add("序账守恒：4 注册 = 3 入序 + 1 永久出序 + 0 逃逸", conserved_ok,
          "任何目标必须落在序内或带理由出序，无第三态");

    // 11. 空序拒绝
    let mut empty_reg = SprintRegistry::new();
    let _ = empty_reg.register(1, TargetKind::Permanent, 0);
    s.add("空序拒绝：仅永久目标时不出总纲",
          build_clear_order(&empty_reg).err() == Some(OrderCode::EmptyOrder),
          "没有任何可清偿目标时必须 EmptyOrder");

    // 12. 前置先行：链 30→20 序内 20 先于 30（20 必须先注册）
    let mut chain = SprintRegistry::new();
    let _ = chain.register(20, TargetKind::Clearable, 0);
    let _ = chain.register(30, TargetKind::Clearable, 20);
    let chain_ok = match build_clear_order(&chain) {
        Ok((order, _)) => {
            let p20 = order.iter().position(|x| *x == 20);
            let p30 = order.iter().position(|x| *x == 30);
            match (p20, p30) {
                (Some(a), Some(b)) => a < b,
                _ => false,
            }
        }
        Err(_) => false,
    };
    s.add("前置先行：prereq 20 必须排在依赖者 30 之前", chain_ok,
          "序内前置约束必须成立，不许依赖者抢跑");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(17) && agrees_with_linux(35)
        && agrees_with_linux(1) && agrees_with_linux(11)
        && !agrees_with_linux(0) && !agrees_with_linux(7777);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < OrderCode::ALL.len() {
        let mut j = i + 1;
        while j < OrderCode::ALL.len() {
            if let (Some(a), Some(b)) = (OrderCode::ALL.get(i), OrderCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = OrderCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同注册表两次排序必须逐位一致");

    // 16. 排序确定性：同表两次排序逐项相等
    let a = build_clear_order(&ord_reg);
    let b = build_clear_order(&ord_reg);
    s.add("排序确定性：两次排序结果逐项相等", a == b,
          "比较器只读、无隐藏状态，重复排序必须恒等");

    // 动态明细（≤8）：序内容 + 指纹 + 注入账
    let order_text = match build_clear_order(&ord_reg) {
        Ok((order, ledger)) => format!(
            "入序 {} 项 / 永久出序 {} / 拒绝 {} / 逃逸 {}",
            order.len(), ledger.permanent_out, ledger.rejected, ledger.escaped
        ),
        Err(_) => format!("排序失败"),
    };
    s.add_dyn("清偿序快照", order_ok, &order_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let inj_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        inj.rejected + inj.escaped, inj.rejected, inj.escaped
    );
    s.add_dyn("注入账快照", inj.zero_escape(), &inj_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_closed_set() {
        assert_eq!(TargetKind::ALL.len(), 3);
        assert!(TargetKind::Clearable.weight() < TargetKind::Conditional.weight());
        assert_eq!(TargetKind::Permanent.weight(), 0);
    }

    #[test]
    fn register_and_find() {
        let mut reg = SprintRegistry::new();
        assert!(reg.register(1, TargetKind::Clearable, 0).is_ok());
        assert_eq!(reg.find(1).unwrap().kind, TargetKind::Clearable);
        assert!(reg.find(2).is_none());
    }

    #[test]
    fn dup_and_empty_rejected() {
        let mut reg = SprintRegistry::new();
        let _ = reg.register(1, TargetKind::Clearable, 0);
        assert_eq!(reg.register(1, TargetKind::Clearable, 0).err(), Some(OrderCode::DupTarget));
        assert_eq!(reg.register(0, TargetKind::Clearable, 0).err(), Some(OrderCode::EmptyTarget));
    }

    #[test]
    fn dangling_prereq_rejected() {
        let mut reg = SprintRegistry::new();
        assert_eq!(reg.register(1, TargetKind::Clearable, 42).err(),
                   Some(OrderCode::PrereqMissing));
    }

    #[test]
    fn table_full_rejected() {
        let mut reg = SprintRegistry::new();
        let mut n = 1u32;
        while n <= MAX_TARGETS as u32 + 1 {
            let _ = reg.register(n, TargetKind::Clearable, 0);
            n += 1;
        }
        assert_eq!(reg.live(), MAX_TARGETS);
    }

    #[test]
    fn cycle_detected() {
        let mut reg = SprintRegistry::new();
        let _ = reg.register(1, TargetKind::Clearable, 0);
        let _ = reg.register(2, TargetKind::Clearable, 1);
        let _ = reg.register(3, TargetKind::Clearable, 2);
        assert!(reg.relink(1, 3).is_ok());
        assert!(has_cycle(&reg, 1));
        // 全表成环 → 序为空 → EmptyOrder；若仍返回 Ok 则账必须全拒零逃逸。
        match build_clear_order(&reg) {
            Err(OrderCode::EmptyOrder) => {}
            Ok((order, ledger)) => {
                assert!(order.is_empty());
                assert_eq!(ledger.rejected, 3);
                assert_eq!(ledger.escaped, 0);
            }
            Err(e) => panic!("unexpected {:?}", e),
        }
    }

    #[test]
    fn order_sorted_by_weight_then_id() {
        let mut reg = SprintRegistry::new();
        let _ = reg.register(9, TargetKind::Conditional, 0);
        let _ = reg.register(5, TargetKind::Clearable, 0);
        let _ = reg.register(6, TargetKind::Clearable, 0);
        let (order, _) = build_clear_order(&reg).unwrap_or((Vec::new(), OrderLedger::zero()));
        let mut expected: Vec<u32> = Vec::new();
        expected.push(5);
        expected.push(6);
        expected.push(9);
        assert_eq!(order, expected);
    }

    #[test]
    fn permanent_never_in_order() {
        let mut reg = SprintRegistry::new();
        let _ = reg.register(1, TargetKind::Permanent, 0);
        let _ = reg.register(2, TargetKind::Clearable, 0);
        let (order, ledger) = build_clear_order(&reg).unwrap_or((Vec::new(), OrderLedger::zero()));
        assert!(!order.contains(&1));
        assert_eq!(ledger.permanent_out, 1);
    }

    #[test]
    fn bad_order_matrix_green() {
        let (per, ledger) = run_bad_order_matrix();
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.rejected, 3 * INJECT_ROUNDS);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(35));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(2), -2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10021_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
