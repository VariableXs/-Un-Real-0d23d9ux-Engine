//! UNX-F9246 brk 坏值矩阵（越下界 EINVAL） · 0x7AB00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9246 逐字）：
//!   UNX-F9246-J1 低于初始堆底注入判据过，全 EINVAL 格过，返回新 break
//!   正判据格过，与 B07 brk 档对账一致。
//!
//! 设计要点：
//!   ① 下界判定放在页对齐之后。Linux 的 brk 先把请求地址按页上取整，再与
//!      初始堆底比较；因此 floor-1 上取整后恰等于 floor，是合法请求。真正
//!      「低于初始堆底」的样本必须整页低于 floor（floor-PAGE 起）才有意义。
//!      注入矩阵按整页步进取样，避免把「只差一字节」误判成 EINVAL。
//!   ② brk(0) 是查询语义，不是「下界以下」。查询必须在任何范围校验之前
//!      短路，返回当前 break 且不计入拒绝账；这条与「全 EINVAL」判据不
//!      冲突，因为判据针对的是非零地址注入格。
//!   ③ 上界是保护区而不是错误面：floor..=limit 内一律成功，limit 之上的
//!      第一个整页返回 ENOMEM（Linux do_brk 在 TASK_SIZE 外回 ENOMEM，
//!      不会回 EINVAL），因此「越下界 EINVAL / 越上界 ENOMEM」是两条
//!      互斥的判据面，次序不得对调。
//!   ④ 状态机单调可对账：grows/shrinks/rejects 三个计数只增不减；失败的
//!      brk 不得改动当前 break，也不得让增长/收缩计数漂移。任何负增量在
//!      账本层显性拒绝。
//!   ⑤ 与 B07 brk 档逐行对账：冻结 10 行 (页偏移, 期望结果) 表，本模块的
//!      判定器逐行复算必须与表一致；只要有一行不符，对账判据整条红。
//!   ⑥ 诊断码是段内闭集：12 个码位落在 +0x01..=+0x0C，码值两两唯一，
//!      每个码都带非空中文建议；文案闭集与码值唯一性各有一条判据。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9246 独占 0x40 窗口，0x7AB00..=0x7AB3F）。
pub const SEG_BASE: u32 = 0x7AB00;

/// 4K 基础页大小（本模块只做 4K 粒度建模）。
pub const PAGE_SIZE: u64 = 4096;

/// 初始堆底：进程映像结束后的第一个可增长地址（页对齐）。
pub const HEAP_FLOOR: u64 = 0x0000_0000_1000_0000;

/// 堆上界：本语料区允许增长到的最高地址（64 页）。
pub const HEAP_LIMIT: u64 = HEAP_FLOOR + PAGE_SIZE * 64;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 越下界注入类别数（整页步进的四种深度）。
pub const BELOW_FLOOR_CLASSES: u32 = 4;

/// B07 对账表行数（冻结，不可增删）。
pub const B07_ROWS_TOTAL: u32 = 10;

/// Linux errno 数值（正数口径；系统调用返回负值）。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrkCode {
    /// 目标页低于初始堆底
    BelowFloor,
    /// 目标页高于堆上界
    AboveLimit,
    /// brk(0) 查询语义
    QueryZero,
    /// 请求被页对齐上取整
    AlignedUp,
    /// 请求恰好页对齐
    ExactHit,
    /// 收缩到堆底之下（与 BelowFloor 共用判定但单独记账）
    ShrinkBelow,
    /// 地址加掩码溢出
    Overflow,
    /// 与 B07 冻结表不一致
    ReconcileMismatch,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 状态机计数回退
    StateRegress,
}

impl BrkCode {
    /// 码值（段基址 + 1 + 变体序号，保证落在 +0x01..=+0x0C）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BrkCode::BelowFloor => "低于初始堆底被拒：目标页整体落在 start_brk 之下；\
                请把请求抬到当前 break 与堆底之间，越下界一律 EINVAL",
            BrkCode::AboveLimit => "高于堆上界被拒：目标页超过可增长上限；请缩减\
                请求或先释放，越上界回 ENOMEM 而非 EINVAL",
            BrkCode::QueryZero => "brk(0) 查询合法：只读当前 break，不触碰范围校验，\
                也不计入拒绝账",
            BrkCode::AlignedUp => "请求被上取整：非页对齐地址按 4K 向上取整后再判定\
                上下界，返回的是取整后的新 break",
            BrkCode::ExactHit => "请求恰好页对齐：直接命中目标页，无需取整",
            BrkCode::ShrinkBelow => "收缩越底被拒：收缩后的 break 低于堆底；请保留\
                至少一页堆空间，失败时当前 break 保持不变",
            BrkCode::Overflow => "地址溢出被拒：请求地址加页掩码溢出 u64；请先做\
                checked_add 再提交，溢出按 ENOMEM 处理",
            BrkCode::ReconcileMismatch => "B07 对账不一致：冻结表某行与本判定器结果\
                不同；请核对页偏移与 errno 归属后重新对账",
            BrkCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有\
                回滚路径误改累计值",
            BrkCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定哈希",
            BrkCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
            BrkCode::StateRegress => "状态机计数回退：grows/shrinks/rejects 必须单调，\
                请检查失败路径是否误记账",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [BrkCode; 12] = [
        BrkCode::BelowFloor,
        BrkCode::AboveLimit,
        BrkCode::QueryZero,
        BrkCode::AlignedUp,
        BrkCode::ExactHit,
        BrkCode::ShrinkBelow,
        BrkCode::Overflow,
        BrkCode::ReconcileMismatch,
        BrkCode::LedgerRegress,
        BrkCode::NotReproducible,
        BrkCode::SegmentViolation,
        BrkCode::StateRegress,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type BrkResult = Result<u64, i32>;

// ── 页对齐原语 ────────────────────────────────────────────────────────
/// 上取整到 4K 页；溢出返回 None（调用方转 ENOMEM，不 panic）。
pub fn page_align_up(addr: u64) -> Option<u64> {
    let mask = PAGE_SIZE - 1;
    let padded = addr.checked_add(mask)?;
    Some(padded & !mask)
}

/// 下取整到 4K 页（掩码运算，无溢出可能）。
pub fn page_align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// 地址是否页对齐。
pub fn is_page_aligned(addr: u64) -> bool {
    addr & (PAGE_SIZE - 1) == 0
}

// ── brk 地址空间模型 ──────────────────────────────────────────────────
/// 定容 brk 模型。只保存三个边界与三个账目计数，不持有任何堆内存。
#[derive(Clone, Copy, Debug)]
pub struct BrkSpace {
    /// 初始堆底（start_brk）
    floor: u64,
    /// 可增长上界（本语料区冻结值）
    limit: u64,
    /// 当前 break
    cur: u64,
    /// 成功增长次数
    grows: u32,
    /// 成功收缩次数
    shrinks: u32,
    /// 被拒次数（EINVAL + ENOMEM）
    rejects: u32,
}

impl BrkSpace {
    /// 构造初始状态：break 等于堆底。
    pub const fn new() -> BrkSpace {
        BrkSpace {
            floor: HEAP_FLOOR,
            limit: HEAP_LIMIT,
            cur: HEAP_FLOOR,
            grows: 0,
            shrinks: 0,
            rejects: 0,
        }
    }

    /// 初始堆底。
    pub fn floor(&self) -> u64 {
        self.floor
    }

    /// 堆上界。
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// 当前 break。
    pub fn current(&self) -> u64 {
        self.cur
    }

    /// 成功增长次数。
    pub fn grows(&self) -> u32 {
        self.grows
    }

    /// 成功收缩次数。
    pub fn shrinks(&self) -> u32 {
        self.shrinks
    }

    /// 被拒次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 可用页数（上界与当前 break 的差，按页计）。
    pub fn free_pages(&self) -> u64 {
        match self.limit.checked_sub(self.cur) {
            Some(diff) => diff / PAGE_SIZE,
            None => 0,
        }
    }

    /// Linux 语义的 brk：
    ///   - addr == 0 → 查询，返回当前 break；
    ///   - 其余地址先按页上取整；
    ///   - 取整后低于堆底 → EINVAL；
    ///   - 取整后高于上界 → ENOMEM；
    ///   - 溢出 → ENOMEM；
    ///   - 成功返回取整后的新 break，并推进 grow/shrink 计数。
    ///
    /// 失败路径一律不改 `cur`，只增 `rejects`。
    pub fn brk(&mut self, addr: u64) -> BrkResult {
        if addr == 0 {
            return Ok(self.cur);
        }
        let target = match page_align_up(addr) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
        };
        if target < self.floor {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if target > self.limit {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        if target > self.cur {
            self.grows = self.grows.saturating_add(1);
        } else if target < self.cur {
            self.shrinks = self.shrinks.saturating_add(1);
        }
        self.cur = target;
        Ok(target)
    }

    /// 按地址给出诊断分类（不产生副作用）。
    pub fn classify(&self, addr: u64) -> BrkCode {
        if addr == 0 {
            return BrkCode::QueryZero;
        }
        match page_align_up(addr) {
            Some(target) => {
                if target < self.floor {
                    BrkCode::BelowFloor
                } else if target > self.limit {
                    BrkCode::AboveLimit
                } else if target != addr {
                    BrkCode::AlignedUp
                } else {
                    BrkCode::ExactHit
                }
            }
            None => BrkCode::Overflow,
        }
    }

    /// 收缩专用入口：低于堆底单独分类为 ShrinkBelow。
    pub fn classify_shrink(&self, addr: u64) -> BrkCode {
        if addr != 0 && addr < self.floor {
            BrkCode::ShrinkBelow
        } else {
            self.classify(addr)
        }
    }
}

// ── 越下界注入矩阵 ────────────────────────────────────────────────────
/// 四种整页步进的越下界样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BelowFloorCase {
    /// 堆底低一页
    OnePage,
    /// 堆底低两页
    TwoPages,
    /// 堆底低八页
    EightPages,
    /// 堆底低 1 MiB
    OneMib,
}

impl BelowFloorCase {
    /// 样本地址（都按整页低于 floor）。
    pub fn addr(self) -> u64 {
        match self {
            BelowFloorCase::OnePage => HEAP_FLOOR - PAGE_SIZE,
            BelowFloorCase::TwoPages => HEAP_FLOOR - PAGE_SIZE * 2,
            BelowFloorCase::EightPages => HEAP_FLOOR - PAGE_SIZE * 8,
            BelowFloorCase::OneMib => HEAP_FLOOR - 0x10_0000,
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> BrkCode {
        BrkCode::BelowFloor
    }

    /// 闭集全量。
    pub const ALL: [BelowFloorCase; 4] = [
        BelowFloorCase::OnePage,
        BelowFloorCase::TwoPages,
        BelowFloorCase::EightPages,
        BelowFloorCase::OneMib,
    ];
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账本。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒被破坏时返回 LedgerRegress 语义的 EINVAL。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行越下界注入矩阵：四类各 INJECT_ROUNDS 次，全部必须 EINVAL。
pub fn run_below_floor_matrix() -> (InjectLedger, [u32; 4]) {
    let mut space = BrkSpace::new();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BelowFloorCase::ALL.len() {
        if let Some(case) = BelowFloorCase::ALL.get(ci).copied() {
            let addr = case.addr();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = space.brk(addr) == Err(EINVAL);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── B07 brk 档对账 ────────────────────────────────────────────────────
/// 冻结对账行：(相对堆底的页偏移, 期望 errno)。0 表示期望成功。
pub const B07_ROWS: &[(i32, i32)] = &[
    (-16, EINVAL),
    (-8, EINVAL),
    (-2, EINVAL),
    (-1, EINVAL),
    (0, 0),
    (1, 0),
    (8, 0),
    (63, 0),
    (64, 0),
    (65, ENOMEM),
];

/// 把「页偏移」换算为绝对地址；负数走 checked_sub，避免下溢。
pub fn b07_addr(pages: i32) -> Option<u64> {
    if pages < 0 {
        let mag = (pages as i64).checked_neg()? as u64;
        let bytes = mag.checked_mul(PAGE_SIZE)?;
        HEAP_FLOOR.checked_sub(bytes)
    } else {
        let bytes = (pages as u64).checked_mul(PAGE_SIZE)?;
        HEAP_FLOOR.checked_add(bytes)
    }
}

/// 逐行复算 B07 表：返回 (命中行数, 总行数)。
pub fn reconcile_b07() -> (u32, u32) {
    let mut matched = 0u32;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < B07_ROWS.len() {
        if let Some(row) = B07_ROWS.get(i).copied() {
            total = total.saturating_add(1);
            let mut space = BrkSpace::new();
            let addr = match b07_addr(row.0) {
                Some(v) => v,
                None => 0,
            };
            let got = match space.brk(addr) {
                Ok(_) => 0,
                Err(e) => e,
            };
            if got == row.1 {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    (matched, total)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；权限族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
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
    code > 0 && linux_neg(code) == -code
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 把一次 brk 结果编码为两字节：成功 (1, 低字节)，失败 (2, errno 低字节)。
fn encode_result(out: &mut Vec<u8>, r: BrkResult) {
    match r {
        Ok(v) => {
            out.push(1);
            out.push((v & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}

/// 规范语料双跑指纹：覆盖查询、增长、越下界、越上界与溢出五条路径。
pub fn corpus_fingerprint() -> u32 {
    let base = HEAP_FLOOR;
    let mut space = BrkSpace::new();
    let mut bytes: Vec<u8> = Vec::new();
    let seq = [
        0u64,
        base + PAGE_SIZE,
        base + PAGE_SIZE * 8,
        base - PAGE_SIZE,
        base + PAGE_SIZE * 80,
        u64::MAX,
        base + PAGE_SIZE * 2,
        base + PAGE_SIZE * 64,
    ];
    let mut i = 0usize;
    while i < seq.len() {
        if let Some(addr) = seq.get(i).copied() {
            encode_result(&mut bytes, space.brk(addr));
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9246 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9246_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9246");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：增长到堆底 + 4 页
    let mut space = BrkSpace::new();
    let target = HEAP_FLOOR + PAGE_SIZE * 4;
    let grow_ok = space.brk(target) == Ok(target) && space.current() == target;
    s.add("正常流：增长 4 页返回新 break", grow_ok,
          "合法增长必须返回取整后的新 break，且状态同步推进");

    // 3. 查询语义
    let mut query = BrkSpace::new();
    let q = query.brk(0);
    s.add("正常流：brk(0) 只查询不改状态", q == Ok(HEAP_FLOOR) && query.rejects() == 0,
          "零地址是查询而非越下界，不得计入拒绝账");

    // 4. 边界：差一字节需上取整到堆底
    let mut edge = BrkSpace::new();
    let round_ok = edge.brk(HEAP_FLOOR - 1) == Ok(HEAP_FLOOR);
    s.add("边界：floor-1 上取整后等于 floor 合法", round_ok,
          "下界判定在页对齐之后，floor-1 不是越下界");

    // 5. 边界：整页低于堆底即 EINVAL
    let mut below = BrkSpace::new();
    let below_ok = below.brk(HEAP_FLOOR - PAGE_SIZE) == Err(EINVAL)
        && below.current() == HEAP_FLOOR;
    s.add("边界：整页低于堆底 → EINVAL 且状态不变", below_ok,
          "失败路径不得改动当前 break");

    // 6. 边界：恰好在堆上界成功
    let mut top = BrkSpace::new();
    let top_ok = top.brk(HEAP_LIMIT) == Ok(HEAP_LIMIT);
    s.add("边界：恰好等于堆上界成功", top_ok,
          "上界本身是合法地址，只有超过一页才拒绝");

    // 7. 边界：越上界 → ENOMEM（不是 EINVAL）
    let mut over = BrkSpace::new();
    let over_ok = over.brk(HEAP_LIMIT + PAGE_SIZE) == Err(ENOMEM);
    s.add("边界：越上界 → ENOMEM", over_ok,
          "上下界错误码必须分开，越界不得混报 EINVAL");

    // 8. 越下界注入 4 类 × 10 轮
    let (ledger, per_class) = run_below_floor_matrix();
    let matrix_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入：越下界 4 类各 10/10 全 EINVAL", matrix_ok,
          "四种整页深度必须全部返回 -EINVAL，逃逸即红");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BELOW_FLOOR_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 判定次序：越下界先于越上界（互斥面）
    let mut order = BrkSpace::new();
    let order_ok = order.brk(HEAP_FLOOR - 0x10_0000) == Err(EINVAL)
        && order.brk(HEAP_LIMIT + PAGE_SIZE) == Err(ENOMEM);
    s.add("判定次序：下界 EINVAL 与上界 ENOMEM 互斥", order_ok,
          "同一判定器两个方向必须给出各自的码，不得互换");

    // 11. 状态机：增长/收缩计数单调且同步
    let mut sm = BrkSpace::new();
    let a = sm.brk(HEAP_FLOOR + PAGE_SIZE * 8);
    let b = sm.brk(HEAP_FLOOR + PAGE_SIZE * 2);
    let c = sm.brk(HEAP_FLOOR + PAGE_SIZE * 8);
    let sm_ok = a == Ok(HEAP_FLOOR + PAGE_SIZE * 8)
        && b == Ok(HEAP_FLOOR + PAGE_SIZE * 2)
        && c == Ok(HEAP_FLOOR + PAGE_SIZE * 8)
        && sm.grows() == 2 && sm.shrinks() == 1;
    s.add("状态机：8→2→8 页增长两次收缩一次", sm_ok,
          "相同地址重复提交不改变计数方向");

    // 12. 收缩越底被拒且 break 保持
    let mut shrink = BrkSpace::new();
    let _ = shrink.brk(HEAP_FLOOR + PAGE_SIZE * 8);
    let shrink_ok = shrink.brk(HEAP_FLOOR - PAGE_SIZE * 2) == Err(EINVAL)
        && shrink.current() == HEAP_FLOOR + PAGE_SIZE * 8
        && shrink.classify_shrink(HEAP_FLOOR - PAGE_SIZE) == BrkCode::ShrinkBelow;
    s.add("状态机：收缩越底被拒且 break 保持", shrink_ok,
          "失败收缩既不能改 break，也不能错误推进 shrink 计数");

    // 13. 溢出：最大地址 → ENOMEM 而非 panic
    let mut ovf = BrkSpace::new();
    let ovf_ok = ovf.brk(u64::MAX) == Err(ENOMEM) && ovf.classify(u64::MAX) == BrkCode::Overflow;
    s.add("容量：地址上取整溢出 → ENOMEM", ovf_ok,
          "checked_add 溢出必须显性转错误码，禁止回绕");

    // 14. B07 逐行对账
    let (matched, total) = reconcile_b07();
    s.add("对账：B07 brk 档 10/10 行一致", matched == total && total == B07_ROWS_TOTAL,
          "冻结表的页偏移与 errno 必须逐行复算一致");

    // 15. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 文案闭集
    let text_ok = BrkCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均有中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 17. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < BrkCode::ALL.len() {
        let mut j = i + 1;
        while j < BrkCode::ALL.len() {
            if let (Some(a), Some(b)) = (BrkCode::ALL.get(i), BrkCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）：账目快照 + 指纹快照
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    // 18. 容量与对齐原语自洽：上下界与空闲页数可复算
    let mut cap = BrkSpace::new();
    let _ = cap.brk(HEAP_FLOOR + PAGE_SIZE * 8);
    let cap_ok = cap.floor() == HEAP_FLOOR
        && cap.limit() == HEAP_LIMIT
        && cap.free_pages() == 56
        && is_page_aligned(HEAP_LIMIT)
        && page_align_down(HEAP_FLOOR + 3) == HEAP_FLOOR;
    s.add("容量：上下界与空闲页数可复算", cap_ok,
          "免费页数必须等于上界减当前 break 再除页大小");

    // 19. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && linux_neg(EINVAL) == -EINVAL
        && !agrees_with_linux(0)
        && BelowFloorCase::OnePage.code() == BrkCode::BelowFloor;
    s.add("Linux 逐码一致：表内码可复算、表外码拒绝", codes_ok,
          "EINVAL/ENOMEM 只允许表内正值，负值必须精确匹配");

    // 20. 账目单调：再注入只增不减
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_grow_returns_new_break() {
        let mut s = BrkSpace::new();
        let target = HEAP_FLOOR + PAGE_SIZE * 4;
        assert_eq!(s.brk(target), Ok(target));
        assert_eq!(s.current(), target);
        assert_eq!(s.grows(), 1);
    }

    #[test]
    fn query_zero_never_rejected() {
        let mut s = BrkSpace::new();
        assert_eq!(s.brk(0), Ok(HEAP_FLOOR));
        assert_eq!(s.rejects(), 0);
        assert_eq!(s.classify(0), BrkCode::QueryZero);
    }

    #[test]
    fn floor_minus_one_rounds_up() {
        let mut s = BrkSpace::new();
        assert_eq!(s.brk(HEAP_FLOOR - 1), Ok(HEAP_FLOOR));
        assert_eq!(s.classify(HEAP_FLOOR - 1), BrkCode::AlignedUp);
    }

    #[test]
    fn one_page_below_floor_is_einval() {
        let mut s = BrkSpace::new();
        assert_eq!(s.brk(HEAP_FLOOR - PAGE_SIZE), Err(EINVAL));
        assert_eq!(s.current(), HEAP_FLOOR);
        assert_eq!(s.rejects(), 1);
    }

    #[test]
    fn limit_boundary_split() {
        let mut s = BrkSpace::new();
        assert_eq!(s.brk(HEAP_LIMIT), Ok(HEAP_LIMIT));
        assert_eq!(s.brk(HEAP_LIMIT + PAGE_SIZE), Err(ENOMEM));
    }

    #[test]
    fn shrink_state_machine() {
        let mut s = BrkSpace::new();
        let _ = s.brk(HEAP_FLOOR + PAGE_SIZE * 8);
        let _ = s.brk(HEAP_FLOOR + PAGE_SIZE * 2);
        let _ = s.brk(HEAP_FLOOR + PAGE_SIZE * 8);
        assert_eq!(s.grows(), 2);
        assert_eq!(s.shrinks(), 1);
        assert_eq!(s.current(), HEAP_FLOOR + PAGE_SIZE * 8);
    }

    #[test]
    fn overflow_is_enomem_not_panic() {
        let mut s = BrkSpace::new();
        assert_eq!(s.brk(u64::MAX), Err(ENOMEM));
        assert_eq!(s.classify(u64::MAX), BrkCode::Overflow);
    }

    #[test]
    fn below_floor_matrix_all_caught() {
        let (l, per) = run_below_floor_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BELOW_FLOOR_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn b07_reconcile_full() {
        let (m, t) = reconcile_b07();
        assert_eq!(t, B07_ROWS_TOTAL);
        assert_eq!(m, t);
    }

    #[test]
    fn b07_addr_checked() {
        assert_eq!(b07_addr(0), Some(HEAP_FLOOR));
        assert_eq!(b07_addr(-1), Some(HEAP_FLOOR - PAGE_SIZE));
        assert_eq!(b07_addr(64), Some(HEAP_LIMIT));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn codes_unique_and_textual() {
        assert_eq!(BrkCode::ALL.len(), 12);
        for c in BrkCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(BrkCode::BelowFloor.val(), SEG_BASE + 1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9246_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
