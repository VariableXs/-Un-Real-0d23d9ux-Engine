//! UNX-F9244 mmap ENOMEM 矩阵（超限/无空间） · 0x7AA80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9244-J1 逐字摘录）：
//!   地址空间耗尽注入判据过，全 ENOMEM 格过，max_map_count 上限格过，
//!   与 RLIMIT_AS 联动格过。
//!
//! 设计要点：
//!   ① ENOMEM 不是单一原因：窗口耗尽、单次超窗、碎片无连续洞、
//!      max_map_count 超限、RLIMIT_AS 超限、地址加法回绕，六类都回
//!      ENOMEM，但诊断码必须区分，禁止把一种原因冒充成另一种。
//!   ② 判定次序固定：参数对齐（EINVAL）→ 地址回绕 → 窗口越界 →
//!      RLIMIT_AS → max_map_count → 连续洞；EINVAL 必须先于任何 ENOMEM。
//!   ③ max_map_count 按「连续已映射页段（run）」计数：相邻扩张会合并，
//!      不增加 run；孤立插入才 +1。默认 65,530，测试可下调到 4 复现上限格。
//!   ④ RLIMIT_AS 联动双向：额度内恰好装满允许，多一页拒绝；运行时抬高
//!      额度后原请求放行，压低后再拒；字节账按页×4096 精确对账。
//!   ⑤ 16 页窗口位图复现首适配/碎片/合并；六类注入各 10 轮，注入账
//!      60 = 60 检出 + 0 逃逸，状态机 Idle→Probed→Sealed 单次封印。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9244 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AA80;

/// 页大小。
pub const PAGE_SIZE: u64 = 4096;

/// 窗口页数与地址边界。
pub const WINDOW_PAGES: usize = 16;
pub const WINDOW_BASE: u64 = 0x1000_0000;
pub const WINDOW_TOP: u64 = WINDOW_BASE + 16 * PAGE_SIZE;

/// 默认 max_map_count 与默认 RLIMIT_AS。
pub const MAX_MAP_COUNT: u32 = 65_530;
pub const DEFAULT_AS_LIMIT: u64 = 0x4000_0000;

/// 测试用低额度与高额度。
pub const TEST_AS_LIMIT: u64 = PAGE_SIZE * 4;
pub const TEST_MAP_LIMIT: u32 = 4;
pub const HIGH_AS_LIMIT: u64 = 0x10_0000_0000;
pub const HIGH_MAP_LIMIT: u32 = 64;

/// 注入类别数与单类复测轮数。
pub const EXHAUST_CLASSES: u32 = 6;
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

/// 诊断码闭集（ENOMEM 六因 + 守卫码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCode {
    LenInvalid = 1,
    AddrInvalid = 2,
    RangeWrapped = 3,
    BeyondWindow = 4,
    NoHole = 5,
    MapCountFull = 6,
    RlimitExceeded = 7,
    SuiteUnprobed = 8,
    SuiteSealed = 9,
    OrderBroken = 10,
    RollbackBroken = 11,
    LedgerRegress = 12,
    SegmentViolation = 14,
}

impl MemCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 映射到 Linux errno。
    pub fn errno(self) -> i32 {
        match self {
            MemCode::RangeWrapped | MemCode::BeyondWindow | MemCode::NoHole
            | MemCode::MapCountFull | MemCode::RlimitExceeded => ENOMEM,
            MemCode::LenInvalid | MemCode::AddrInvalid | MemCode::SuiteUnprobed
            | MemCode::SuiteSealed | MemCode::OrderBroken | MemCode::RollbackBroken
            | MemCode::LedgerRegress
            | MemCode::SegmentViolation => EINVAL,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MemCode::LenInvalid => "长度非法：len==0 或非 4096 倍数回 EINVAL；请先对齐长度",
            MemCode::AddrInvalid => "地址非法：addr 未页对齐回 EINVAL；请先对齐地址",
            MemCode::RangeWrapped => "区间回绕：addr+len 溢出 u64 回 ENOMEM；请缩小长度或降低地址",
            MemCode::BeyondWindow => "越出映射窗：区间不在 [BASE, TOP) 内回 ENOMEM；请检查边界",
            MemCode::NoHole => "无连续空洞：窗口已满或碎片化，首适配失败回 ENOMEM；请先释放",
            MemCode::MapCountFull => "VMA 数超限：max_map_count 已到顶回 ENOMEM；请合并或减少映射",
            MemCode::RlimitExceeded => "RLIMIT_AS 超限：字节额度不足回 ENOMEM；请提高额度或缩容",
            MemCode::SuiteUnprobed => "注入套未探毕：先跑满六类再封印；请补跑探针",
            MemCode::SuiteSealed => "注入套已封印：状态机只允许封印一次；请新建套",
            MemCode::OrderBroken => "判定次序被破坏：对齐→回绕→越界→AS→map_count→洞不可交换",
            MemCode::RollbackBroken => "失败未回滚：被拒映射不得占用页或推进计数；请先判定后落位",
            MemCode::LedgerRegress => "注入账倒退：计数只增不减；请检查逃逸与回滚分支",
            MemCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [MemCode; 13] = [
        MemCode::LenInvalid, MemCode::AddrInvalid, MemCode::RangeWrapped,
        MemCode::BeyondWindow, MemCode::NoHole, MemCode::MapCountFull,
        MemCode::RlimitExceeded, MemCode::SuiteUnprobed, MemCode::SuiteSealed,
        MemCode::OrderBroken, MemCode::RollbackBroken, MemCode::LedgerRegress,
        MemCode::SegmentViolation,
    ];
}

/// 是否页对齐。
pub const fn is_page_aligned(v: u64) -> bool { v & (PAGE_SIZE - 1) == 0 }

/// 窗口内页下标（调用前必须已通过判定）。
fn page_index(addr: u64) -> usize { ((addr - WINDOW_BASE) / PAGE_SIZE) as usize }

/// 连续已映射页段数。
fn count_runs(used: &[bool; WINDOW_PAGES]) -> u32 {
    let mut runs = 0u32;
    let mut prev = false;
    let mut i = 0usize;
    while i < WINDOW_PAGES {
        if let Some(cur) = used.get(i) {
            if *cur && !prev {
                runs = runs.saturating_add(1);
            }
            prev = *cur;
        }
        i += 1;
    }
    runs
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

/// 页窗口 + 双额度模型。
#[derive(Clone, Copy, Debug)]
pub struct VmSpace {
    used: [bool; WINDOW_PAGES],
    map_limit: u32,
    as_limit: u64,
    maps: u32,
    rejects: u32,
}

impl VmSpace {
    /// 新地址空间：RLIMIT_AS 与 max_map_count 双额度。
    pub const fn new(as_limit: u64, map_limit: u32) -> VmSpace {
        VmSpace {
            used: [false; WINDOW_PAGES],
            map_limit,
            as_limit,
            maps: 0,
            rejects: 0,
        }
    }

    /// 已用页数。
    pub fn used_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < WINDOW_PAGES {
            if let Some(true) = self.used.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 空闲页数。
    pub fn free_pages(&self) -> u32 { (WINDOW_PAGES as u32).saturating_sub(self.used_pages()) }

    /// 已用字节数。
    pub fn bytes_used(&self) -> u64 { (self.used_pages() as u64).saturating_mul(PAGE_SIZE) }

    /// 当前 VMA 数（连续 run）。
    pub fn map_count(&self) -> u32 { count_runs(&self.used) }

    /// 成功映射次数。
    pub fn maps(&self) -> u32 { self.maps }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 { self.rejects }

    /// 当前 max_map_count。
    pub fn map_limit(&self) -> u32 { self.map_limit }

    /// 运行时抬高/压低 RLIMIT_AS。
    pub fn set_as_limit(&mut self, as_limit: u64) { self.as_limit = as_limit; }

    /// 运行时调整 max_map_count。
    pub fn set_map_limit(&mut self, map_limit: u32) { self.map_limit = map_limit; }

    /// 位图与计数签名。
    pub fn signature(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < WINDOW_PAGES {
            match self.used.get(i) {
                Some(true) => bytes.push(1),
                _ => bytes.push(0),
            }
            i += 1;
        }
        bytes.push((self.maps & 0xFF) as u8);
        fnv1a(&bytes)
    }

    fn mark(&mut self, addr: u64, pages: usize) {
        let start = page_index(addr);
        let mut k = 0usize;
        while k < pages {
            if let Some(cell) = self.used.get_mut(start + k) {
                *cell = true;
            }
            k += 1;
        }
    }

    fn free_range(&self, addr: u64, pages: usize) -> bool {
        let start = page_index(addr);
        let mut k = 0usize;
        while k < pages {
            match self.used.get(start + k) {
                Some(true) => return false,
                _ => {}
            }
            k += 1;
        }
        true
    }

    fn runs_with(&self, addr: u64, pages: usize) -> u32 {
        let mut sim = self.used;
        let start = page_index(addr);
        let mut k = 0usize;
        while k < pages {
            if let Some(cell) = sim.get_mut(start + k) {
                *cell = true;
            }
            k += 1;
        }
        count_runs(&sim)
    }

    fn charge(&self, pages: usize) -> Result<(), MemCode> {
        let need = (self.used_pages() as u64).saturating_add(pages as u64);
        let need = need.saturating_mul(PAGE_SIZE);
        if need > self.as_limit {
            Err(MemCode::RlimitExceeded)
        } else {
            Ok(())
        }
    }

    /// 参数判定：长度、地址、回绕、窗口边界。
    pub fn judge_addr(&self, addr: u64, len: u64) -> Result<usize, MemCode> {
        if len == 0 || !is_page_aligned(len) {
            return Err(MemCode::LenInvalid);
        }
        if !is_page_aligned(addr) {
            return Err(MemCode::AddrInvalid);
        }
        let end = match addr.checked_add(len) {
            Some(e) => e,
            None => return Err(MemCode::RangeWrapped),
        };
        if addr < WINDOW_BASE || end > WINDOW_TOP {
            return Err(MemCode::BeyondWindow);
        }
        Ok((len / PAGE_SIZE) as usize)
    }

    /// 固定地址映射：对齐→回绕/越界→AS→map_count→空洞。
    pub fn map_at(&mut self, addr: u64, len: u64) -> Result<(), MemCode> {
        let pages = match self.judge_addr(addr, len) {
            Ok(p) => p,
            Err(code) => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(code);
            }
        };
        if let Err(code) = self.charge(pages) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(code);
        }
        if self.runs_with(addr, pages) > self.map_limit {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::MapCountFull);
        }
        if !self.free_range(addr, pages) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::NoHole);
        }
        self.mark(addr, pages);
        self.maps = self.maps.saturating_add(1);
        Ok(())
    }

    /// 首适配自动映射：返回起始地址。
    pub fn map_auto(&mut self, len: u64) -> Result<u64, MemCode> {
        if len == 0 || !is_page_aligned(len) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::LenInvalid);
        }
        let pages = (len / PAGE_SIZE) as usize;
        if pages > WINDOW_PAGES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MemCode::NoHole);
        }
        if let Err(code) = self.charge(pages) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(code);
        }
        let mut i = 0usize;
        while i + pages <= WINDOW_PAGES {
            let mut blocked = false;
            let mut k = 0usize;
            while k < pages {
                match self.used.get(i + k) {
                    Some(true) => blocked = true,
                    _ => {}
                }
                k += 1;
            }
            if !blocked {
                let addr = WINDOW_BASE + (i as u64).saturating_mul(PAGE_SIZE);
                if self.runs_with(addr, pages) > self.map_limit {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(MemCode::MapCountFull);
                }
                self.mark(addr, pages);
                self.maps = self.maps.saturating_add(1);
                return Ok(addr);
            }
            i += 1;
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(MemCode::NoHole)
    }
}

/// 六类 ENOMEM 注入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExhaustCase {
    WindowFull,
    Oversize,
    Fragmented,
    MapCount,
    Rlimit,
    Wrap,
}

impl ExhaustCase {
    /// 闭集全量。
    pub const ALL: [ExhaustCase; 6] = [
        ExhaustCase::WindowFull, ExhaustCase::Oversize, ExhaustCase::Fragmented,
        ExhaustCase::MapCount, ExhaustCase::Rlimit, ExhaustCase::Wrap,
    ];

    /// 该类必须命中的诊断码。
    pub fn want_code(self) -> MemCode {
        match self {
            ExhaustCase::WindowFull | ExhaustCase::Oversize
            | ExhaustCase::Fragmented => MemCode::NoHole,
            ExhaustCase::MapCount => MemCode::MapCountFull,
            ExhaustCase::Rlimit => MemCode::RlimitExceeded,
            ExhaustCase::Wrap => MemCode::RangeWrapped,
        }
    }
}

fn high_space() -> VmSpace {
    VmSpace::new(HIGH_AS_LIMIT, HIGH_MAP_LIMIT)
}

/// 单轮构造并触发某类耗尽，返回是否命中期望码。
pub fn inject_case(case: ExhaustCase) -> bool {
    match case {
        ExhaustCase::WindowFull => {
            let mut v = high_space();
            let full = v.map_at(WINDOW_BASE, (WINDOW_PAGES as u64) * PAGE_SIZE);
            full.is_ok() && v.map_auto(PAGE_SIZE) == Err(MemCode::NoHole)
        }
        ExhaustCase::Oversize => {
            let mut v = high_space();
            v.map_auto((WINDOW_PAGES as u64 + 1) * PAGE_SIZE) == Err(MemCode::NoHole)
        }
        ExhaustCase::Fragmented => {
            let mut v = high_space();
            let mut i = 0usize;
            while i < WINDOW_PAGES {
                if i % 3 != 2 {
                    let addr = WINDOW_BASE + (i as u64) * PAGE_SIZE;
                    let _ = v.map_at(addr, PAGE_SIZE);
                }
                i += 1;
            }
            v.map_auto(PAGE_SIZE * 2) == Err(MemCode::NoHole)
        }
        ExhaustCase::MapCount => {
            let mut v = high_space();
            v.set_map_limit(TEST_MAP_LIMIT);
            let _ = v.map_at(WINDOW_BASE, PAGE_SIZE);
            let _ = v.map_at(WINDOW_BASE + PAGE_SIZE * 2, PAGE_SIZE);
            let _ = v.map_at(WINDOW_BASE + PAGE_SIZE * 4, PAGE_SIZE);
            let _ = v.map_at(WINDOW_BASE + PAGE_SIZE * 6, PAGE_SIZE);
            v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE) == Err(MemCode::MapCountFull)
        }
        ExhaustCase::Rlimit => {
            let mut v = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
            let exact = v.map_at(WINDOW_BASE, TEST_AS_LIMIT);
            exact.is_ok()
                && v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE)
                    == Err(MemCode::RlimitExceeded)
        }
        ExhaustCase::Wrap => {
            let mut v = high_space();
            v.map_at(u64::MAX & !(PAGE_SIZE - 1), PAGE_SIZE * 2)
                == Err(MemCode::RangeWrapped)
        }
    }
}

/// 单类 10 轮检出数。
fn inject_rounds(case: ExhaustCase) -> u32 {
    let mut caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let ok = inject_case(case);
        if ok {
            caught = caught.saturating_add(1);
        }
        round += 1;
    }
    caught
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger { InjectLedger { injected: 0, detected: 0, escaped: 0 } }

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

/// 六类各 10 轮，返回账本与逐类检出数。
pub fn run_enomem_injections() -> (InjectLedger, [u32; 6]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 6];
    let mut i = 0usize;
    while i < ExhaustCase::ALL.len() {
        if let Some(case) = ExhaustCase::ALL.get(i).copied() {
            let n = inject_rounds(case);
            if let Some(slot) = per.get_mut(i) {
                *slot = n;
            }
            let mut k = 0u32;
            while k < INJECT_ROUNDS {
                let _ = ledger.record(k < n);
                k += 1;
            }
        }
        i += 1;
    }
    (ledger, per)
}

/// 注入套状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteState {
    Idle,
    Probed,
    Sealed,
}

/// 注入套状态机：Idle→Probed→Sealed。
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    state: SuiteState,
    rounds: u32,
}

impl Suite {
    /// 空套。
    pub const fn new() -> Suite { Suite { state: SuiteState::Idle, rounds: 0 } }

    /// 当前状态。
    pub fn state(&self) -> SuiteState { self.state }

    /// 跑满六类并登记。
    pub fn probe(&mut self) -> Result<u32, MemCode> {
        match self.state {
            SuiteState::Idle => {}
            SuiteState::Probed => return Err(MemCode::OrderBroken),
            SuiteState::Sealed => return Err(MemCode::SuiteSealed),
        }
        let (ledger, per) = run_enomem_injections();
        if !ledger.conserved() || ledger.escaped != 0
            || ledger.injected != EXHAUST_CLASSES * INJECT_ROUNDS
        {
            return Err(MemCode::LedgerRegress);
        }
        let mut all = true;
        let mut i = 0usize;
        while i < per.len() {
            match per.get(i) {
                Some(n) => {
                    if *n != INJECT_ROUNDS {
                        all = false;
                    }
                }
                None => all = false,
            }
            i += 1;
        }
        if !all {
            return Err(MemCode::LedgerRegress);
        }
        self.rounds = ledger.detected;
        self.state = SuiteState::Probed;
        Ok(self.rounds)
    }

    /// 封印：必须先探毕，且只允许一次。
    pub fn seal(&mut self) -> Result<u32, MemCode> {
        match self.state {
            SuiteState::Idle => Err(MemCode::SuiteUnprobed),
            SuiteState::Probed => {
                self.state = SuiteState::Sealed;
                Ok(self.rounds)
            }
            SuiteState::Sealed => Err(MemCode::SuiteSealed),
        }
    }
}

/// 规范序列双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut a = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
    let exact = a.map_auto(TEST_AS_LIMIT);
    let over = a.map_auto(PAGE_SIZE);
    let mut b = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
    let charged = b.map_at(WINDOW_BASE, PAGE_SIZE * 3).map(|_| 0u64);
    b.set_as_limit(PAGE_SIZE * 5);
    let raised = b.map_auto(PAGE_SIZE * 2);
    let mut c = VmSpace::new(HIGH_AS_LIMIT, TEST_MAP_LIMIT);
    let _ = c.map_at(WINDOW_BASE, PAGE_SIZE);
    let _ = c.map_at(WINDOW_BASE + PAGE_SIZE * 2, PAGE_SIZE);
    let counted = c.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE).map(|_| 0u64);
    let mut d = high_space();
    let wrapped = d.map_at(u64::MAX & !(PAGE_SIZE - 1), PAGE_SIZE * 2).map(|_| 0u64);
    let steps = [exact, over, charged, raised, counted, wrapped];
    let mut i = 0usize;
    while i < steps.len() {
        match steps.get(i) {
            Some(Ok(addr)) => {
                bytes.push(0xA5);
                bytes.push((addr & 0xFF) as u8);
            }
            Some(Err(code)) => bytes.push((*code as u32 & 0xFF) as u8),
            None => {}
        }
        i += 1;
    }
    bytes.push(d.used_pages() as u8);
    fnv1a(&bytes)
}

/// F9244 判据集（13 静态 + 3 动态）。
pub fn run_unx9244_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9244");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流
    let normal = {
        let mut v = high_space();
        match v.map_auto(PAGE_SIZE * 2) {
            Ok(addr) => addr == WINDOW_BASE
                && v.used_pages() == 2 && v.bytes_used() == PAGE_SIZE * 2
                && v.map_count() == 1 && v.maps() == 1
                && v.free_pages() == (WINDOW_PAGES as u32 - 2),
            Err(_) => false,
        }
    };
    s.add("正常流：首适配两页、run=1、字节账精确", normal,
          "成功路径必须落位、计数与字节账同步推进");

    // 3. 恰好装满边界
    let exact = {
        let mut v = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
        let filled = v.map_auto(TEST_AS_LIMIT).is_ok();
        let denied = v.map_auto(PAGE_SIZE) == Err(MemCode::RlimitExceeded);
        filled && denied && v.used_pages() == 4 && v.bytes_used() == TEST_AS_LIMIT
    };
    s.add("RLIMIT_AS 边界：额度内恰好装满，多一页 ENOMEM", exact,
          "比较必须用 > 而非 >=，恰好等于额度是允许的");

    // 4. RLIMIT 抬高压低联动
    let linkage = {
        let mut v = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
        let first = v.map_at(WINDOW_BASE, PAGE_SIZE * 3).is_ok();
        let denied = v.map_auto(PAGE_SIZE * 2) == Err(MemCode::RlimitExceeded);
        v.set_as_limit(PAGE_SIZE * 5);
        let allowed = v.map_auto(PAGE_SIZE * 2).is_ok();
        v.set_as_limit(PAGE_SIZE * 2);
        let denied_again = v.map_auto(PAGE_SIZE) == Err(MemCode::RlimitExceeded);
        first && denied && allowed && denied_again
            && v.used_pages() == 5 && v.bytes_used() == PAGE_SIZE * 5
    };
    s.add("RLIMIT_AS 联动：抬高放行、压低再拒", linkage,
          "额度变更必须即时生效，且不得回退已占用字节");

    // 5. max_map_count 上限
    let map_count = {
        let mut v = high_space();
        let default_ok = VmSpace::new(DEFAULT_AS_LIMIT, MAX_MAP_COUNT).map_limit() == MAX_MAP_COUNT;
        v.set_map_limit(TEST_MAP_LIMIT);
        let mut filled = true;
        let mut i = 0u64;
        while i < 4 {
            let addr = WINDOW_BASE + i * PAGE_SIZE * 2;
            if v.map_at(addr, PAGE_SIZE).is_err() {
                filled = false;
            }
            i += 1;
        }
        let four = v.map_count() == TEST_MAP_LIMIT;
        let denied = v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE)
            == Err(MemCode::MapCountFull);
        let merged = v.map_at(WINDOW_BASE + PAGE_SIZE, PAGE_SIZE).is_ok();
        filled && default_ok && four && denied && merged && v.map_count() == 3
    };
    s.add("max_map_count：4 个 run 后孤立插入被拒，邻接合并放行", map_count,
          "run 计数与合并语义必须同时成立，默认上限 65,530");

    // 6. 各类耗尽边界
    let causes = inject_case(ExhaustCase::WindowFull)
        && inject_case(ExhaustCase::Oversize)
        && inject_case(ExhaustCase::Fragmented)
        && inject_case(ExhaustCase::Wrap);
    s.add("耗尽边界：满窗/超窗/碎片/回绕全部 ENOMEM", causes,
          "四类边界各自命中 NoHole 或 RangeWrapped，且失败零落位");

    // 10. 判定次序
    let order = {
        let mut v = VmSpace::new(PAGE_SIZE, 1);
        let _ = v.map_at(WINDOW_BASE, PAGE_SIZE);
        let len_first = v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE + 1)
            == Err(MemCode::LenInvalid);
        let as_first = v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE)
            == Err(MemCode::RlimitExceeded);
        let addr_first = v.map_at(WINDOW_BASE + 1, PAGE_SIZE)
            == Err(MemCode::AddrInvalid);
        let beyond = v.map_at(WINDOW_TOP, PAGE_SIZE) == Err(MemCode::BeyondWindow);
        len_first && as_first && addr_first && beyond
    };
    s.add("判定次序：EINVAL→回绕→越界→AS→map_count→洞", order,
          "AS 与 map_count 同时超限时必须先报 RLIMIT_AS");

    // 11. 失败回滚
    let rollback = {
        let mut v = high_space();
        v.set_map_limit(1);
        let _ = v.map_at(WINDOW_BASE, PAGE_SIZE);
        let sig = v.signature();
        let denied = v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE)
            == Err(MemCode::MapCountFull);
        let denied2 = v.map_auto(PAGE_SIZE + 1) == Err(MemCode::LenInvalid);
        denied && denied2 && v.used_pages() == 1 && v.maps() == 1
            && v.map_count() == 1 && v.signature() == sig && v.rejects() == 2
    };
    s.add("回滚：ENOMEM/EINVAL 路径零落位、签名不变", rollback,
          "先判定后落位；失败分支不得 mark、不得推进 maps");

    // 12. 对账
    let reconcile = {
        let mut v = high_space();
        let a = v.map_at(WINDOW_BASE, PAGE_SIZE * 3).is_ok();
        let b = v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE).is_ok();
        a && b && v.used_pages() == 4 && v.free_pages() == (WINDOW_PAGES as u32 - 4)
            && v.used_pages().saturating_add(v.free_pages()) == WINDOW_PAGES as u32
            && v.bytes_used() == (v.used_pages() as u64) * PAGE_SIZE
            && v.map_count() == 2
    };
    s.add("对账：页数/字节/run 三项守恒", reconcile,
          "used+free==16、bytes==used×4096、run 等于实际连续段");

    // 13. 状态机
    let machine = {
        let mut suite = Suite::new();
        let early = suite.seal() == Err(MemCode::SuiteUnprobed);
        let probed = suite.probe();
        let sealed = suite.seal();
        let again = suite.seal() == Err(MemCode::SuiteSealed);
        let reprobe = suite.probe() == Err(MemCode::SuiteSealed);
        early && matches!(probed, Ok(n) if n == EXHAUST_CLASSES * INJECT_ROUNDS)
            && matches!(sealed, Ok(n) if n == EXHAUST_CLASSES * INJECT_ROUNDS)
            && again && reprobe && suite.state() == SuiteState::Sealed
    };
    s.add("状态机：Idle→Probed→Sealed，未探/重探/重封全拒", machine,
          "Probed 之前不可封，Sealed 之后不可再探再封");

    // 14. 文案闭集
    let mut text_ok = true;
    let mut u = 0usize;
    while u < MemCode::ALL.len() {
        if let Some(c) = MemCode::ALL.get(u) {
            if c.detail().is_empty() || !matches!(c.errno(), ENOMEM | EINVAL) {
                text_ok = false;
            }
            let mut w = u + 1;
            while w < MemCode::ALL.len() {
                if let Some(d) = MemCode::ALL.get(w) {
                    if c.val() == d.val() {
                        text_ok = false;
                    }
                }
                w += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：13 码中文建议非空、码值唯一、errno 归档", text_ok,
          "每个诊断码必须可定位、可操作，且只允许 ENOMEM/EINVAL");

    // 15. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范调用序列指纹逐字节相等", fp_a == fp_b,
          "同输入两次必须相等，隐藏状态或计数泄漏即失败");

    // 16. 注入账
    let (ledger, per) = run_enomem_injections();
    let all_caught = per.iter().all(|n| *n == INJECT_ROUNDS);
    let enomem_all = ExhaustCase::ALL.iter().all(|c| c.want_code().errno() == ENOMEM);
    s.add("注入账：六类各 10/10、全 ENOMEM、逃逸为 0",
          all_caught && enomem_all && ledger.conserved() && ledger.escaped == 0
              && ledger.injected == EXHAUST_CLASSES * INJECT_ROUNDS,
          "60 次注入必须 60 次检出，六类 errno 全部为 ENOMEM");

    // 动态明细
    let min = match per.iter().copied().min() { Some(n) => n, None => 0 };
    let max = match per.iter().copied().max() { Some(n) => n, None => 0 };
    let space_text = format!("窗口 16 页，六类最小 {} 最大 {}，注入 {}",
                             min, max, ledger.injected);
    s.add_dyn("耗尽矩阵快照", all_caught, &space_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("ENOMEM 指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fit_boundary() {
        let mut v = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
        assert_eq!(v.map_auto(TEST_AS_LIMIT), Ok(WINDOW_BASE));
        assert_eq!(v.used_pages(), 4);
        assert_eq!(v.bytes_used(), TEST_AS_LIMIT);
        assert_eq!(v.map_auto(PAGE_SIZE), Err(MemCode::RlimitExceeded));
    }

    #[test]
    fn rlimit_raise_lower_linkage() {
        let mut v = VmSpace::new(TEST_AS_LIMIT, HIGH_MAP_LIMIT);
        assert!(v.map_at(WINDOW_BASE, PAGE_SIZE * 3).is_ok());
        assert_eq!(v.map_auto(PAGE_SIZE * 2), Err(MemCode::RlimitExceeded));
        v.set_as_limit(PAGE_SIZE * 5);
        assert!(v.map_auto(PAGE_SIZE * 2).is_ok());
        assert_eq!(v.used_pages(), 5);
        v.set_as_limit(PAGE_SIZE * 2);
        assert_eq!(v.map_auto(PAGE_SIZE), Err(MemCode::RlimitExceeded));
    }

    #[test]
    fn max_map_count_boundary() {
        let mut v = high_space();
        v.set_map_limit(TEST_MAP_LIMIT);
        let mut i = 0u64;
        while i < 4 {
            let addr = WINDOW_BASE + i * PAGE_SIZE * 2;
            assert!(v.map_at(addr, PAGE_SIZE).is_ok());
            i += 1;
        }
        assert_eq!(v.map_count(), 4);
        assert_eq!(v.map_at(WINDOW_BASE + PAGE_SIZE * 8, PAGE_SIZE),
                   Err(MemCode::MapCountFull));
        assert!(v.map_at(WINDOW_BASE + PAGE_SIZE, PAGE_SIZE).is_ok());
        assert_eq!(v.map_count(), 3);
    }

    #[test]
    fn window_exhaustion_and_fragmentation() {
        let mut full = high_space();
        assert!(full.map_at(WINDOW_BASE, (WINDOW_PAGES as u64) * PAGE_SIZE).is_ok());
        assert_eq!(full.map_auto(PAGE_SIZE), Err(MemCode::NoHole));
        let mut frag = high_space();
        let mut i = 0usize;
        while i < WINDOW_PAGES {
            if i % 3 != 2 {
                let addr = WINDOW_BASE + (i as u64) * PAGE_SIZE;
                assert!(frag.map_at(addr, PAGE_SIZE).is_ok());
            }
            i += 1;
        }
        assert_eq!(frag.map_auto(PAGE_SIZE * 2), Err(MemCode::NoHole));
        assert!(frag.map_auto(PAGE_SIZE).is_ok());
    }

    #[test]
    fn suite_state_machine() {
        let mut s = Suite::new();
        assert_eq!(s.seal(), Err(MemCode::SuiteUnprobed));
        assert_eq!(s.probe(), Ok(EXHAUST_CLASSES * INJECT_ROUNDS));
        assert_eq!(s.seal(), Ok(EXHAUST_CLASSES * INJECT_ROUNDS));
        assert_eq!(s.seal(), Err(MemCode::SuiteSealed));
        assert_eq!(s.probe(), Err(MemCode::SuiteSealed));
        assert_eq!(s.state(), SuiteState::Sealed);
    }

    #[test]
    fn injection_matrix_enomem() {
        let (l, per) = run_enomem_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, EXHAUST_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for c in ExhaustCase::ALL.iter() {
            assert_eq!(c.want_code().errno(), ENOMEM);
        }
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9244_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
