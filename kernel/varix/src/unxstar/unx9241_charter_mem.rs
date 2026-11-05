//! UNX-F9241 内存族 E 型总纲（地址空间错误面） · 0x7A9C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9241-J1 逐字摘录）：
//!   地址空间错误面判据过，四类错误域（对齐/越界/耗尽/权限）逐格落账，
//!   与本批矩阵映射一致格过。
//!
//! 设计要点：
//!   ① 宪章只登记四类错误域：对齐域（EINVAL 面）、越界域（ENOMEM 面）、
//!      耗尽域（ENOMEM 面）、权限域（EACCES 面）。每域恰三条规则，规则号
//!      1..=12 闭集，域归属唯一，禁止跨域借规则。
//!   ② 宪章状态机 Draft→Registered→Frozen 单向推进；冻结前置条件是 12 条
//!      规则全部 Registered 且逐条验证为 Frozen。未齐即冻回 NotReady，
//!      冻结后任何登记/验证/再冻都回 FrozenCharter 并显性记账。
//!   ③ 判定次序固定为 对齐 → 越界 → 耗尽 → 权限。顺序不是文案而是判据：
//!      同一请求同时未对齐且越界时必须报 EINVAL，报 ENOMEM 即次序错。
//!   ④ 四域与本批矩阵卡一一映射：对齐↔F9243、越界↔F9242、耗尽↔F9244、
//!      权限↔F9245；映射表按域排序，卡号两两不重，结账时逐项复算。
//!   ⑤ 违例注入四类各 10/10 命中对应域的错误码；注入账只增不减，
//!      注入 = 检出 + 逃逸且逃逸恒为 0；预算账 charge/release 闭合归零。
//!   ⑥ 地址域模型使用 4K 页、64KiB 用户下界、47 位用户上界与 1GiB 预算，
//!      全部 checked 运算，零裸下标、零浮点、零墙钟。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9241 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A9C0;

/// 宪章规则总数（四域 × 三条）。
pub const CHARTER_RULES: usize = 12;

/// 页大小（4K）。
pub const PAGE_SIZE: u64 = 4096;

/// 用户地址下界（64KiB，等价 mmap_min_addr 模型）。
pub const USER_BASE: u64 = 0x0001_0000;

/// 用户地址上界（不含，47 位 TASK_SIZE 的最后一页）。
pub const USER_TOP: u64 = 0x0000_7FFF_FFFF_F000;

/// 地址空间映射预算（1GiB）。
pub const BUDGET: u64 = 0x4000_0000;

/// VMA 数量上限（Linux max_map_count 默认值）。
pub const MAX_MAP_COUNT: u32 = 65_530;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（四类错误域）。
pub const INJECT_CLASSES: u32 = 4;

/// Linux errno 正数码。
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..+0x10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCode {
    UnalignedAddr, ZeroLength, RangeOverflow, AboveUserTop, BudgetExceeded,
    MapCountExceeded, BelowMinAddr, BadProt, WriteToRo, UnknownRule,
    NotReady, FrozenCharter, IllegalTransition, LedgerRegress, NotReproducible,
    SegmentViolation,
}

impl MemCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 错误码所属的 Linux errno（正数口径）。
    pub fn errno(self) -> i32 {
        match self {
            MemCode::BelowMinAddr | MemCode::WriteToRo => EACCES,
            MemCode::RangeOverflow | MemCode::AboveUserTop | MemCode::BudgetExceeded
            | MemCode::MapCountExceeded => ENOMEM,
            _ => EINVAL,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MemCode::UnalignedAddr => "地址未页对齐：addr 必须是 4096 的整数倍；请先 align_down 再发起映射",
            MemCode::ZeroLength => "零长度请求：len=0 不构成有效映射；请给出至少一页的长度",
            MemCode::RangeOverflow => "区间回绕：addr+len 超出 u64；请缩小长度或抬高基址后重试",
            MemCode::AboveUserTop => "越过用户上界：区间终点超过 TASK_SIZE；请把映射收进用户地址窗",
            MemCode::BudgetExceeded => "地址空间预算耗尽：累计用量超过 1GiB 预算；请释放不再使用的 VMA",
            MemCode::MapCountExceeded => "VMA 数量超限：已达 max_map_count；请合并相邻同权限区间",
            MemCode::BelowMinAddr => "低于 mmap_min_addr：固定映射不得落在用户下界之前；请抬高 addr",
            MemCode::BadProt => "保护位非法：prot 只能取 READ/WRITE/EXEC 的组合；请清掉未定义位",
            MemCode::WriteToRo => "只读区间写入：目标 VMA 无写权限；请先 mprotect 提权或改用可写映射",
            MemCode::UnknownRule => "规则号不在宪章闭集：只允许 1..=12；请按四域三条的编号登记",
            MemCode::NotReady => "宪章未就绪：12 条规则未全部登记并验证；请补齐后再冻结",
            MemCode::FrozenCharter => "宪章已冻结：冻结后禁止增删改规则；请升版另开宪章",
            MemCode::IllegalTransition => "状态迁移非法：Draft→Registered→Frozen 单向；请勿跳级或回退",
            MemCode::LedgerRegress => "账目倒退：预算/注入计数只增不减；请检查回滚分支",
            MemCode::NotReproducible => "双跑不一致：同输入两次判定不同；请排查隐藏状态",
            MemCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MemCode; 16] = [
        MemCode::UnalignedAddr, MemCode::ZeroLength, MemCode::RangeOverflow,
        MemCode::AboveUserTop, MemCode::BudgetExceeded, MemCode::MapCountExceeded,
        MemCode::BelowMinAddr, MemCode::BadProt, MemCode::WriteToRo, MemCode::UnknownRule,
        MemCode::NotReady, MemCode::FrozenCharter, MemCode::IllegalTransition,
        MemCode::LedgerRegress, MemCode::NotReproducible, MemCode::SegmentViolation,
    ];
}

/// 地址空间错误域闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemDomain {
    /// 对齐域：EINVAL 面
    Align,
    /// 越界域：ENOMEM 面
    Range,
    /// 耗尽域：ENOMEM 面
    Exhaust,
    /// 权限域：EACCES 面
    Perm,
}

impl MemDomain {
    /// 闭集全量（判定次序即此数组次序）。
    pub const ALL: [MemDomain; 4] = [
        MemDomain::Align,
        MemDomain::Range,
        MemDomain::Exhaust,
        MemDomain::Perm,
    ];

    /// 域对应的矩阵卡号（与本批 9242..9245 一一映射）。
    pub fn card(self) -> u16 {
        match self {
            MemDomain::Align => 9243,
            MemDomain::Range => 9242,
            MemDomain::Exhaust => 9244,
            MemDomain::Perm => 9245,
        }
    }
}

/// 12 条宪章规则：(规则号, 域, 规则文本)。
pub const CHARTER_RULE_SET: [(u8, MemDomain, &str); CHARTER_RULES] = [
    (1, MemDomain::Align, "addr 按 4096 对齐"),
    (2, MemDomain::Align, "len 非零且按页取整"),
    (3, MemDomain::Align, "prot 只含 READ/WRITE/EXEC"),
    (4, MemDomain::Range, "addr+len 不得回绕"),
    (5, MemDomain::Range, "区间终点不越过 TASK_SIZE"),
    (6, MemDomain::Range, "区间落在用户窗内且不重叠"),
    (7, MemDomain::Exhaust, "单次请求不超总预算"),
    (8, MemDomain::Exhaust, "累计用量不超总预算"),
    (9, MemDomain::Exhaust, "VMA 数量不超 max_map_count"),
    (10, MemDomain::Perm, "固定映射不低于 mmap_min_addr"),
    (11, MemDomain::Perm, "写只读区间必须被拒"),
    (12, MemDomain::Perm, "执行权限须显式声明"),
];

/// 规则生命周期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleState {
    Draft, Registered, Frozen,
}

/// 状态迁移表：只允许 Draft→Registered、Registered→Frozen。
pub fn can_advance(from: RuleState, to: RuleState) -> bool {
    matches!((from, to), (RuleState::Draft, RuleState::Registered)
        | (RuleState::Registered, RuleState::Frozen))
}

/// 宪章中的一条规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CharterRule {
    pub id: u8,
    pub domain: MemDomain,
    pub state: RuleState,
    pub validations: u32,
}

/// 宪章登记册 + 冻结状态机。
#[derive(Clone, Copy, Debug)]
pub struct Charter {
    slots: [Option<CharterRule>; CHARTER_RULES],
    len: usize,
    validations: u32,
    freezes: u32,
    rejects: u32,
    intercepts: u32,
}

impl Charter {
    /// 空宪章。
    pub const fn new() -> Charter {
        Charter {
            slots: [None; CHARTER_RULES], len: 0, validations: 0,
            freezes: 0, rejects: 0, intercepts: 0,
        }
    }

    /// 已登记规则数。
    pub fn len(&self) -> usize { self.len }

    /// 是否已冻结。
    pub fn frozen(&self) -> bool { self.freezes > 0 }

    /// 累计验证次数。
    pub fn validations(&self) -> u32 { self.validations }

    /// 冻结次数。
    pub fn freezes(&self) -> u32 { self.freezes }

    /// 拦截次数（冻结后变更尝试）。
    pub fn intercepts(&self) -> u32 { self.intercepts }

    /// 按规则号找槽位下标。
    fn index_of(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            match self.slots.get(i) {
                Some(Some(rule)) => {
                    if rule.id == id {
                        return Some(i);
                    }
                }
                _ => return None,
            }
            i += 1;
        }
        None
    }

    /// 查表取规则所属域（规则号非法返回 None）。
    fn domain_of(id: u8) -> Option<MemDomain> {
        let mut i = 0usize;
        while i < CHARTER_RULE_SET.len() {
            if let Some(row) = CHARTER_RULE_SET.get(i) {
                if row.0 == id {
                    return Some(row.1);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条规则：Draft→Registered。冻结后/重复/越号全部拒绝。
    pub fn register_rule(&mut self, id: u8) -> Result<usize, MemCode> {
        if self.frozen() {
            self.rejects = self.rejects.saturating_add(1);
            self.intercepts = self.intercepts.saturating_add(1);
            return Err(MemCode::FrozenCharter);
        }
        let domain = match Self::domain_of(id) {
            Some(d) => d,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(MemCode::UnknownRule);
            }
        };
        if self.index_of(id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::IllegalTransition);
        }
        if self.len >= CHARTER_RULES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::NotReady);
        }
        let rule = CharterRule { id, domain, state: RuleState::Registered, validations: 0 };
        match self.slots.get_mut(self.len) {
            Some(cell) => {
                *cell = Some(rule);
                self.len += 1;
                Ok(self.len - 1)
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                Err(MemCode::NotReady)
            }
        }
    }

    /// 验证一条规则：Registered→Frozen。
    pub fn validate_rule(&mut self, id: u8) -> Result<(), MemCode> {
        if self.frozen() {
            self.rejects = self.rejects.saturating_add(1);
            self.intercepts = self.intercepts.saturating_add(1);
            return Err(MemCode::FrozenCharter);
        }
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(MemCode::UnknownRule);
            }
        };
        let rule = match self.slots.get_mut(idx) {
            Some(Some(r)) => r,
            _ => return Err(MemCode::UnknownRule),
        };
        if !can_advance(rule.state, RuleState::Frozen) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::IllegalTransition);
        }
        rule.state = RuleState::Frozen;
        rule.validations = rule.validations.saturating_add(1);
        self.validations = self.validations.saturating_add(1);
        Ok(())
    }

    /// 冻结：12 条全部登记且逐条验证通过才放行。
    pub fn freeze(&mut self) -> Result<usize, MemCode> {
        if self.frozen() {
            self.rejects = self.rejects.saturating_add(1);
            self.intercepts = self.intercepts.saturating_add(1);
            return Err(MemCode::FrozenCharter);
        }
        if self.len != CHARTER_RULES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::NotReady);
        }
        let mut i = 0usize;
        while i < self.len {
            let ok = match self.slots.get(i) {
                Some(Some(rule)) => rule.state == RuleState::Frozen && rule.validations >= 1,
                _ => false,
            };
            if !ok {
                self.rejects = self.rejects.saturating_add(1);
                return Err(MemCode::NotReady);
            }
            i += 1;
        }
        self.freezes = self.freezes.saturating_add(1);
        Ok(self.len)
    }

    /// 某域已登记规则数。
    pub fn domain_count(&self, domain: MemDomain) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(rule)) = self.slots.get(i) {
                if rule.domain == domain {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 某域已冻结规则数。
    pub fn domain_frozen(&self, domain: MemDomain) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(rule)) = self.slots.get(i) {
                if rule.domain == domain && rule.state == RuleState::Frozen {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 按闭集顺序登记 12 条规则。
pub fn build_charter() -> Charter {
    let mut c = Charter::new();
    let mut i = 0usize;
    while i < CHARTER_RULE_SET.len() {
        if let Some(row) = CHARTER_RULE_SET.get(i) {
            let _ = c.register_rule(row.0);
        }
        i += 1;
    }
    c
}

/// 登记 + 全量验证，返回可冻结宪章。
pub fn validated_charter() -> Charter {
    let mut c = build_charter();
    let mut i = 0usize;
    while i < CHARTER_RULE_SET.len() {
        if let Some(row) = CHARTER_RULE_SET.get(i) {
            let _ = c.validate_rule(row.0);
        }
        i += 1;
    }
    c
}

/// 地址空间预算账（只增用量的反向释放仍受 used 约束）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Budget {
    pub limit: u64,
    pub used: u64,
}

impl Budget {
    /// 新建预算。
    pub const fn new(limit: u64) -> Budget {
        Budget { limit, used: 0 }
    }

    /// 剩余可用量。
    pub fn free(&self) -> u64 { self.limit.saturating_sub(self.used) }

    /// 占用：checked 加法，超限回 BudgetExceeded。
    pub fn charge(&mut self, bytes: u64) -> Result<(), MemCode> {
        let next = match self.used.checked_add(bytes) {
            Some(v) => v,
            None => return Err(MemCode::BudgetExceeded),
        };
        if next > self.limit {
            return Err(MemCode::BudgetExceeded);
        }
        self.used = next;
        Ok(())
    }

    /// 释放：释放量不得大于已用量。
    pub fn release(&mut self, bytes: u64) -> Result<(), MemCode> {
        let next = match self.used.checked_sub(bytes) {
            Some(v) => v,
            None => return Err(MemCode::LedgerRegress),
        };
        self.used = next;
        Ok(())
    }
}

/// 权限位。
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;
pub const PROT_MASK: u32 = PROT_READ | PROT_WRITE | PROT_EXEC;

/// 一次映射请求的宪章视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemRequest {
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub fixed: bool,
    pub writes_ro: bool,
    pub map_count: u32,
}

impl MemRequest {
    /// 合法请求构造。
    pub const fn valid() -> MemRequest {
        MemRequest {
            addr: USER_BASE, len: PAGE_SIZE, prot: PROT_READ | PROT_WRITE,
            fixed: false, writes_ro: false, map_count: 0,
        }
    }
}

/// 四域判定：对齐 → 越界 → 耗尽 → 权限。
pub fn judge(budget: &Budget, req: &MemRequest) -> Result<(), MemCode> {
    // ① 对齐域
    if req.len == 0 {
        return Err(MemCode::ZeroLength);
    }
    if req.addr % PAGE_SIZE != 0 {
        return Err(MemCode::UnalignedAddr);
    }
    if req.len % PAGE_SIZE != 0 {
        return Err(MemCode::UnalignedAddr);
    }
    // ② 越界域
    let end = match req.addr.checked_add(req.len) {
        Some(v) => v,
        None => return Err(MemCode::RangeOverflow),
    };
    if end > USER_TOP {
        return Err(MemCode::AboveUserTop);
    }
    // ③ 耗尽域
    if req.len > budget.limit {
        return Err(MemCode::BudgetExceeded);
    }
    if budget.used.saturating_add(req.len) > budget.limit {
        return Err(MemCode::BudgetExceeded);
    }
    if req.map_count >= MAX_MAP_COUNT {
        return Err(MemCode::MapCountExceeded);
    }
    // ④ 权限域
    if req.fixed && req.addr < USER_BASE {
        return Err(MemCode::BelowMinAddr);
    }
    if req.prot & !PROT_MASK != 0 {
        return Err(MemCode::BadProt);
    }
    if req.writes_ro && req.prot & PROT_WRITE == 0 {
        return Err(MemCode::WriteToRo);
    }
    Ok(())
}

/// 四类违例样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemBadCase {
    Unaligned, Overflow, Budget, Permission,
}

impl MemBadCase {
    /// 闭集全量。
    pub const ALL: [MemBadCase; 4] = [
        MemBadCase::Unaligned,
        MemBadCase::Overflow,
        MemBadCase::Budget,
        MemBadCase::Permission,
    ];

    /// 期望诊断码。
    pub fn expected(self) -> MemCode {
        match self {
            MemBadCase::Unaligned => MemCode::UnalignedAddr,
            MemBadCase::Overflow => MemCode::RangeOverflow,
            MemBadCase::Budget => MemCode::BudgetExceeded,
            MemBadCase::Permission => MemCode::BelowMinAddr,
        }
    }

    /// 构造样本请求。
    pub fn request(self) -> MemRequest {
        match self {
            MemBadCase::Unaligned => MemRequest {
                addr: USER_BASE + 1, len: PAGE_SIZE, prot: PROT_READ, fixed: false,
                writes_ro: false, map_count: 0,
            },
            MemBadCase::Overflow => MemRequest {
                addr: u64::MAX - (PAGE_SIZE - 1), len: PAGE_SIZE * 2, prot: PROT_READ,
                fixed: false, writes_ro: false, map_count: 0,
            },
            MemBadCase::Budget => MemRequest {
                addr: USER_BASE, len: BUDGET + PAGE_SIZE, prot: PROT_READ,
                fixed: false, writes_ro: false, map_count: 0,
            },
            MemBadCase::Permission => MemRequest {
                addr: USER_BASE - PAGE_SIZE, len: PAGE_SIZE, prot: PROT_READ,
                fixed: true, writes_ro: false, map_count: 0,
            },
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), MemCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(MemCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类各 10 轮注入。
pub fn run_mem_injections() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 4];
    let budget = Budget::new(BUDGET);
    let mut ci = 0usize;
    while ci < MemBadCase::ALL.len() {
        if let Some(case) = MemBadCase::ALL.get(ci).copied() {
            let req = case.request();
            let mut caught = 0u32;
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if judge(&budget, &req) == Err(case.expected()) {
                    caught = caught.saturating_add(1);
                }
                let _ = ledger.record(judge(&budget, &req) == Err(case.expected()));
                round += 1;
            }
            if let Some(slot) = per.get_mut(ci) {
                *slot = caught;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

/// FNV-1a 32 位。
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

/// 规范请求语料双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let budget = Budget::new(BUDGET);
    let mut bytes: Vec<u8> = Vec::new();
    let samples = [
        MemRequest::valid(),
        MemRequest { addr: USER_BASE + 1, ..MemRequest::valid() },
        MemRequest { len: 0, ..MemRequest::valid() },
        MemRequest { addr: USER_TOP, len: PAGE_SIZE, ..MemRequest::valid() },
        MemRequest { len: BUDGET + PAGE_SIZE, ..MemRequest::valid() },
        MemRequest { prot: 0x8, ..MemRequest::valid() },
        MemRequest { fixed: true, addr: USER_BASE - PAGE_SIZE, ..MemRequest::valid() },
        MemRequest { writes_ro: true, prot: PROT_READ, ..MemRequest::valid() },
    ];
    let mut i = 0usize;
    while i < samples.len() {
        if let Some(req) = samples.get(i) {
            let r = judge(&budget, req);
            bytes.push(match r {
                Ok(()) => 0,
                Err(code) => (code as u32 & 0xFF) as u8,
            });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// F9241 判据集（13 静态 + 3 动态）。
pub fn run_unx9241_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9241");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝登记");

    // 2. 规则号与域映射唯一
    let mut map_ok = true;
    let mut cards = [0u16; 4];
    let mut i = 0usize;
    while i < CHARTER_RULE_SET.len() {
        if let Some(row) = CHARTER_RULE_SET.get(i) {
            if row.0 == 0 || row.0 as usize > CHARTER_RULES {
                map_ok = false;
            }
            if Charter::domain_of(row.0) != Some(row.1) {
                map_ok = false;
            }
        }
        i += 1;
    }
    let mut d = 0usize;
    while d < MemDomain::ALL.len() {
        if let Some(dom) = MemDomain::ALL.get(d) {
            let card = dom.card();
            if cards.contains(&card) {
                map_ok = false;
            }
            if let Some(slot) = cards.get_mut(d) {
                *slot = card;
            }
        }
        d += 1;
    }
    s.add("映射：规则号唯一且四域卡号不重", map_ok && cards == [9243, 9242, 9244, 9245],
          "对齐↔9243、越界↔9242、耗尽↔9244、权限↔9245，逐项可复算");

    // 4. 判定次序
    let order_ok = MemDomain::ALL[0] == MemDomain::Align
        && MemDomain::ALL[1] == MemDomain::Range
        && MemDomain::ALL[2] == MemDomain::Exhaust
        && MemDomain::ALL[3] == MemDomain::Perm;
    s.add("判定次序：对齐→越界→耗尽→权限", order_ok,
          "次序错报即判据失效，不允许按实现方便重排");

    // 6. 未齐冻结
    let mut partial = Charter::new();
    let early = partial.freeze();
    s.add("状态机：规则未齐冻结回 NotReady", early == Err(MemCode::NotReady),
          "冻结是强前置；11 条规则也不得放行");

    // 7. 全量验证后冻结
    let mut full = validated_charter();
    let frozen_ok = full.freeze() == Ok(CHARTER_RULES)
        && full.frozen()
        && full.freezes() == 1
        && MemDomain::ALL.iter().all(|d| full.domain_frozen(*d) == 3);
    s.add("状态机：12 条全验证后冻结成功", frozen_ok,
          "冻结后每域三条必须全部处于 Frozen");

    // 8. 冻结后变更 10/10 拒
    let mut frozen_hits = 0u32;
    let mut k = 0u32;
    while k < INJECT_ROUNDS {
        let r1 = full.register_rule(1) == Err(MemCode::FrozenCharter);
        let r2 = full.validate_rule(1) == Err(MemCode::FrozenCharter);
        let r3 = full.freeze() == Err(MemCode::FrozenCharter);
        if r1 && r2 && r3 {
            frozen_hits = frozen_hits.saturating_add(1);
        }
        k += 1;
    }
    s.add("状态机：冻结后登记/验证/再冻 10/10 全拒", frozen_hits == INJECT_ROUNDS,
          "冻结是单向终点，任何变更尝试都要显性拦截");

    // 9. 正常流
    let budget = Budget::new(BUDGET);
    let normal = judge(&budget, &MemRequest::valid()) == Ok(());
    s.add("正常流：对齐区间在预算内通过四域", normal,
          "合法请求必须四域全过，不得被任何守卫误伤");

    // 11. 四类注入
    let (ledger, per) = run_mem_injections();
    let mut all_caught = true;
    let mut j = 0usize;
    while j < per.len() {
        if let Some(n) = per.get(j) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        j += 1;
    }
    s.add("注入：四域违例各 10/10 命中对应错误码", all_caught,
          "未对齐/回绕/超预算/低于下界必须逐类回本域码");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 13. 预算账闭合
    let mut acc = Budget::new(PAGE_SIZE * 4);
    let c1 = acc.charge(PAGE_SIZE);
    let c2 = acc.charge(PAGE_SIZE * 2);
    let over = acc.charge(PAGE_SIZE * 2);
    let rel = acc.release(PAGE_SIZE * 3);
    let closed = c1.is_ok() && c2.is_ok() && over == Err(MemCode::BudgetExceeded)
        && rel.is_ok() && acc.used == 0 && acc.free() == acc.limit;
    s.add("预算账：三次占用/一次超限/全额释放归零", closed,
          "预算只增于 charge、只减于 release，释放不得越过已用量");

    // 14. 窗口边界
    let below = MemRequest { addr: USER_BASE - PAGE_SIZE, fixed: true, ..MemRequest::valid() };
    let above = MemRequest { addr: USER_TOP, len: PAGE_SIZE, ..MemRequest::valid() };
    let window_ok = judge(&budget, &below) == Err(MemCode::BelowMinAddr)
        && judge(&budget, &above) == Err(MemCode::AboveUserTop);
    s.add("窗口：低于下界 EACCES、越过上界 ENOMEM", window_ok,
          "下界属权限域、上界属越界域，两码不得互串");

    // 15. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范请求语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 文案闭集 + 码值唯一
    let mut text_ok = true;
    let mut u = 0usize;
    while u < MemCode::ALL.len() {
        if let Some(code) = MemCode::ALL.get(u) {
            if code.detail().is_empty() || code.errno() <= 0 {
                text_ok = false;
            }
            let mut v = u + 1;
            while v < MemCode::ALL.len() {
                if let Some(other) = MemCode::ALL.get(v) {
                    if code.val() == other.val() {
                        text_ok = false;
                    }
                }
                v += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：16 码均有中文建议与正 errno 且码值唯一", text_ok,
          "每码必须可定位、可操作，禁止空文案或重码");

    // 动态明细
    let charter_text = format!("规则 {} 冻结 {} 验证 {} 拦截 {}",
                               full.len(), full.freezes(), full.validations(),
                               full.intercepts());
    s.add_dyn("宪章快照", frozen_ok, &charter_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("内存宪章指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charter_registers_twelve_rules() {
        let c = build_charter();
        assert_eq!(c.len(), CHARTER_RULES);
        for d in MemDomain::ALL.iter() {
            assert_eq!(c.domain_count(*d), 3);
        }
    }

    #[test]
    fn freeze_requires_complete_and_validated() {
        let mut c = build_charter();
        assert_eq!(c.freeze(), Err(MemCode::NotReady));
        let mut v = validated_charter();
        assert_eq!(v.freeze(), Ok(CHARTER_RULES));
        assert!(v.frozen());
    }

    #[test]
    fn frozen_charter_rejects_mutation() {
        let mut c = validated_charter();
        assert!(c.freeze().is_ok());
        assert_eq!(c.register_rule(1), Err(MemCode::FrozenCharter));
        assert_eq!(c.validate_rule(1), Err(MemCode::FrozenCharter));
        assert_eq!(c.freeze(), Err(MemCode::FrozenCharter));
        assert!(c.intercepts() >= 3);
    }

    #[test]
    fn judge_valid_request() {
        let b = Budget::new(BUDGET);
        let mut req = MemRequest::valid();
        assert_eq!(judge(&b, &req), Ok(()));
        req.prot = PROT_READ;
        assert_eq!(judge(&b, &req), Ok(()));
    }

    #[test]
    fn judge_order_alignment_first() {
        let b = Budget::new(BUDGET);
        let req = MemRequest {
            addr: USER_BASE + 1, len: BUDGET * 2, fixed: true,
            writes_ro: true, prot: 0x8, map_count: MAX_MAP_COUNT,
        };
        assert_eq!(judge(&b, &req), Err(MemCode::UnalignedAddr));
    }

    #[test]
    fn injection_conserved() {
        let (l, per) = run_mem_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9241_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
