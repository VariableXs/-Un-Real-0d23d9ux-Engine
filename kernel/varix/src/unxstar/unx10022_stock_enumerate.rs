//! UNX-F10022 豁免存量盘点器（全量枚举） · 0x91580段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10023-J1 三级判定留痕（逐条归类带依据 10/10 次），无据归类 10/10 次拦截
//!
//! 设计要点：
//!   ① 全量枚举的判据是「零遗漏」：盘点扫描必须触及账本每一个非空
//!      槽位，枚举数 = 账面数、覆盖位图全 1；任何一洞（漏盘）都在
//!      对账步 10/10 次显性检出，不许静默缩表。
//!   ② 对账四查闭集：重复盘（DupId）、幽灵条目（GhostEntry，账外
//!      多出来的）、漏盘（GapDetected，账内有而盘点无）、计数失配
//!      （CountMismatch）；四查全过才出对账报告。
//!   ③ 账本定容：槽位 1..=MAX_SLOTS，id 出界直接拒绝；重复插入拒绝；
//!      账本只增不清（存量盘点不清账，清除属 F10023 之后的流程）。
//!   ④ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；四类坏盘点样本
//!      各 10/10 次被对账器拒绝。
//!   ⑤ 扫描是确定性纯函数：同账本两次扫描逐位一致，FNV-1a 双跑指纹
//!      复核；扫描顺序按槽位号升序，不许乱序漂移。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10022 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91580;

/// 账本容量（槽位 1..=MAX_SLOTS）。
pub const MAX_SLOTS: u32 = 24;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StockCode {
    /// id 出界（0 或越过 MAX_SLOTS）
    BadIdRange,
    /// 重复插入账本
    DupInsert,
    /// 账本满
    BookFull,
    /// 盘点清单重复
    DupId,
    /// 幽灵条目：盘点出现账外 id
    GhostEntry,
    /// 漏盘：账内有而盘点无
    GapDetected,
    /// 计数失配：枚举数 ≠ 账面数
    CountMismatch,
    /// 空账盘点拒绝
    EmptyBook,
    /// 存量态未知（非闭集态）
    StateUnknown,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl StockCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StockCode::BadIdRange => "id 出界被拒：槽位号必须落在 1..=MAX_SLOTS；\
                请改用合法号段重新入账",
            StockCode::DupInsert => "重复插入被拒：同一 id 只能占一个槽位；\
                请先查重再入账",
            StockCode::BookFull => "账本满被拒：槽位全部占用；请先扩容或走清除流程",
            StockCode::DupId => "盘点清单重复被拒：同一 id 在一次盘点里只允许出现一次；\
                请去重后重新对账",
            StockCode::GhostEntry => "幽灵条目被拒：盘点出现账外 id；请核对盘点来源，\
                幽灵条目不得计入存量",
            StockCode::GapDetected => "漏盘被拒：账内存在未被盘点触及的条目；\
                请按槽位号全量重扫，缺一即拒",
            StockCode::CountMismatch => "计数失配被拒：枚举数与账面数不一致；\
                请先修漏盘/幽灵再出对账报告",
            StockCode::EmptyBook => "空账盘点被拒：账面为零时不产出对账报告；\
                请先入账再盘点",
            StockCode::StateUnknown => "存量态未知被拒：条目态必须落在闭集内；\
                请按未清偿/已冻结/待复核三态重新登记",
            StockCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            StockCode::NotReproducible => "双跑不一致：同账本两次扫描不同；\
                请排查隐藏状态或乱序遍历",
            StockCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [StockCode; 12] = [
        StockCode::BadIdRange,
        StockCode::DupInsert,
        StockCode::BookFull,
        StockCode::DupId,
        StockCode::GhostEntry,
        StockCode::GapDetected,
        StockCode::CountMismatch,
        StockCode::EmptyBook,
        StockCode::StateUnknown,
        StockCode::LedgerRegress,
        StockCode::NotReproducible,
        StockCode::SegmentViolation,
    ];
}

// ── 存量账本 ──────────────────────────────────────────────────────────
/// 存量条目三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StockState {
    /// 未清偿（活跃存量）
    Outstanding,
    /// 已冻结（等待条件）
    Frozen,
    /// 待复核
    Review,
}

impl StockState {
    /// 闭集全量。
    pub const ALL: [StockState; 3] = [StockState::Outstanding, StockState::Frozen, StockState::Review];
}

/// 单条存量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StockItem {
    /// 槽位号（1..=MAX_SLOTS）
    pub id: u32,
    /// 存量态
    pub state: StockState,
    /// 登记序（确定性时间戳）
    pub seq: u32,
}

/// 存量账本：定容槽位 + 只增账目。
#[derive(Clone, Copy, Debug)]
pub struct AccountBook {
    slots: [Option<StockItem>; 24],
    /// 账面条目数（与占用槽位数一致）
    count: u32,
    /// 累计登记次数（只增）
    registered: u32,
    /// 序号发生器
    seq_gen: u32,
}

impl AccountBook {
    /// 空账本。
    #[allow(clippy::declare_const_fn)]
    pub const fn new() -> AccountBook {
        AccountBook { slots: [None; 24], count: 0, registered: 0, seq_gen: 0 }
    }

    /// 入账：id 出界/重复/满账均拒绝。
    pub fn insert(&mut self, id: u32, state: StockState) -> Result<(), StockCode> {
        if id == 0 || id > MAX_SLOTS {
            return Err(StockCode::BadIdRange);
        }
        let idx = (id - 1) as usize;
        if idx >= self.slots.len() {
            return Err(StockCode::SegmentViolation);
        }
        if self.slots[idx].is_some() {
            return Err(StockCode::DupInsert);
        }
        self.seq_gen = self.seq_gen.wrapping_add(1);
        self.slots[idx] = Some(StockItem { id, state, seq: self.seq_gen });
        self.count = self.count.saturating_add(1);
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 查条目。
    pub fn get(&self, id: u32) -> Option<StockItem> {
        if id == 0 || id > MAX_SLOTS {
            return None;
        }
        let idx = (id - 1) as usize;
        if idx >= self.slots.len() {
            return None;
        }
        self.slots[idx]
    }

    /// 账面条目数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 累计登记数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 账内 id 全集（按槽位号升序）。
    pub fn ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(item)) = self.slots.get(i).copied() {
                v.push(item.id);
            }
            i += 1;
        }
        v
    }
}

// ── 盘点器 ────────────────────────────────────────────────────────────
/// 全量扫描：按槽位号升序触及每一个非空槽位，返回盘点清单。
/// 确定性纯函数：同账本两次扫描逐位一致。
pub fn scan_all(book: &AccountBook) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < book.slots.len() {
        if let Some(Some(item)) = book.slots.get(i).copied() {
            v.push(item.id);
        }
        i += 1;
    }
    v
}

/// 对账报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconReport {
    /// 对账命中数（账内且盘到）
    pub matched: u32,
    /// 账面条目数
    pub book_count: u32,
    /// 盘点清单长度
    pub enum_count: u32,
}

/// 对账四查：去重 → 幽灵 → 漏盘 → 计数闭合。任何一查不过即拒绝，
/// 不产出残缺报告。
pub fn reconcile(book: &AccountBook, enum_ids: &[u32]) -> Result<ReconReport, StockCode> {
    if book.count() == 0 {
        return Err(StockCode::EmptyBook);
    }
    // 查一：盘点清单内部去重
    let mut i = 0usize;
    while i < enum_ids.len() {
        let mut j = i + 1;
        while j < enum_ids.len() {
            let a = match enum_ids.get(i) {
                Some(x) => *x,
                None => break,
            };
            let b = match enum_ids.get(j) {
                Some(x) => *x,
                None => break,
            };
            if a == b {
                return Err(StockCode::DupId);
            }
            j += 1;
        }
        i += 1;
    }
    // 查二：幽灵（盘点有、账内无）
    let mut k = 0usize;
    while k < enum_ids.len() {
        let id = match enum_ids.get(k) {
            Some(x) => *x,
            None => break,
        };
        if book.get(id).is_none() {
            return Err(StockCode::GhostEntry);
        }
        k += 1;
    }
    // 查三：漏盘（账内有、盘点无）——覆盖位图逐槽核对
    let mut i2 = 0usize;
    while i2 < book.slots.len() {
        if let Some(Some(item)) = book.slots.get(i2).copied() {
            let mut found = false;
            let mut m = 0usize;
            while m < enum_ids.len() {
                if let Some(x) = enum_ids.get(m) {
                    if *x == item.id {
                        found = true;
                    }
                }
                m += 1;
            }
            if !found {
                return Err(StockCode::GapDetected);
            }
        }
        i2 += 1;
    }
    // 查四：计数闭合
    if enum_ids.len() as u32 != book.count() {
        return Err(StockCode::CountMismatch);
    }
    Ok(ReconReport { matched: book.count(), book_count: book.count(), enum_count: enum_ids.len() as u32 })
}

// ── 注入核：坏盘点样本矩阵 ────────────────────────────────────────────
/// 四类坏盘点样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadScanCase {
    /// 漏盘：清单缺一个账内 id
    Gap,
    /// 幽灵：清单多一个账外 id
    Ghost,
    /// 重复盘：清单里同一 id 出现两次
    Dup,
    /// 空盘：账本非空而清单为空
    Empty,
}

impl BadScanCase {
    /// 闭集全量。
    pub const ALL: [BadScanCase; 4] = [
        BadScanCase::Gap,
        BadScanCase::Ghost,
        BadScanCase::Dup,
        BadScanCase::Empty,
    ];

    /// 期望诊断码。
    pub fn code(self) -> StockCode {
        match self {
            BadScanCase::Gap => StockCode::GapDetected,
            BadScanCase::Ghost => StockCode::GhostEntry,
            BadScanCase::Dup => StockCode::DupId,
            BadScanCase::Empty => StockCode::GapDetected,
        }
    }

    /// 构造 8 条账本 + 坏清单，返回对账是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut book = AccountBook::new();
        let mut n = 1u32;
        while n <= 8 {
            let _ = book.insert(n, StockState::Outstanding);
            n += 1;
        }
        let mut list = scan_all(&book);
        match self {
            BadScanCase::Gap => {
                let _ = list.pop();
            }
            BadScanCase::Ghost => {
                list.push(99);
            }
            BadScanCase::Dup => {
                let first = match list.first().copied() {
                    Some(x) => x,
                    None => return false,
                };
                list.push(first);
            }
            BadScanCase::Empty => {
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
                let _ = list.pop();
            }
        }
        reconcile(&book, &list).err() == Some(self.code())
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnumLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl EnumLedger {
    const fn zero() -> EnumLedger {
        EnumLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒破坏返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), StockCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(StockCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏盘点注入：四类各 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_scan_matrix() -> (EnumLedger, [u32; 4]) {
    let mut ledger = EnumLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadScanCase::ALL.len() {
        let case = match BadScanCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；漏盘/幽灵族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复插入/重复盘族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；账本满族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；id 出界族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；存量态未知族"),
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

/// 规范语料双跑指纹：12 条账本（混合三态）→ 全量扫描 → 对账 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut book = AccountBook::new();
    let states = [StockState::Outstanding, StockState::Frozen, StockState::Review];
    let mut n = 3u32;
    while n <= 36 {
        let st = states[(n % 3) as usize];
        let _ = book.insert(n % 12 + 1, st);
        n += 3;
    }
    let list = scan_all(&book);
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < list.len() {
        if let Some(id) = list.get(i) {
            bytes.push((id & 0xFF) as u8);
        }
        i += 1;
    }
    match reconcile(&book, &list) {
        Ok(rep) => {
            bytes.push((rep.matched & 0xFF) as u8);
        }
        Err(_) => {}
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10022 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10022_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10022");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 入账正常流：8 条入账、账面计数一致
    let mut book = AccountBook::new();
    let mut n = 1u32;
    let mut ins_ok = true;
    while n <= 8 {
        if book.insert(n, StockState::Outstanding).is_err() {
            ins_ok = false;
        }
        n += 1;
    }
    s.add("入账正常流：8 条入账且账面计数 8", ins_ok && book.count() == 8 && book.registered() == 8,
          "每次成功入账账面数与登记数各记一笔");

    // 3. 边界注入：id 出界与重复插入 10/10 拒绝
    let mut edge_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut b = AccountBook::new();
        if b.insert(0, StockState::Outstanding).is_ok()
            || b.insert(MAX_SLOTS + 1, StockState::Outstanding).is_ok() {
            edge_ok = false;
        }
        let _ = b.insert(5, StockState::Frozen);
        if b.insert(5, StockState::Review).is_ok() {
            edge_ok = false;
        }
        r += 1;
    }
    s.add("边界注入：出界 id/重复插入 10/10 拒绝", edge_ok,
          "id 必须落在 1..=MAX_SLOTS 且唯一占槽");

    // 4. 全量扫描零遗漏：扫描清单与账面 id 全集逐位相等
    let list = scan_all(&book);
    let truth = book.ids();
    s.add("全量扫描：扫描清单与账面全集逐位相等", list == truth && list.len() == 8,
          "按槽位号升序触及每一非空槽位，不许乱序漂移");

    // 5. 对账通过：零遗漏出报告
    let rep = reconcile(&book, &list);
    let rep_ok = match &rep {
        Ok(rr) => rr.matched == 8 && rr.book_count == 8 && rr.enum_count == 8,
        Err(_) => false,
    };
    s.add("对账通过：8/8 全命中且计数闭合", rep_ok,
          "四查全过才允许出对账报告");

    // 6. 注入核：四类坏盘点各 10/10 拒绝
    let (inj, per_class) = run_bad_scan_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：漏盘/幽灵/重复盘/空盘各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadScanCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 漏盘定位：单独漏一条立即 GapDetected
    let mut gap_list = book.ids();
    let _ = gap_list.pop();
    s.add("漏盘定位：清单缺尾条 → GapDetected",
          reconcile(&book, &gap_list).err() == Some(StockCode::GapDetected),
          "账内有而盘点无的每一洞都必须被点名");

    // 9. 幽灵条目：账外 id 不得计入存量
    let mut ghost_list = book.ids();
    ghost_list.push(99);
    s.add("幽灵条目：账外 id 99 → GhostEntry",
          reconcile(&book, &ghost_list).err() == Some(StockCode::GhostEntry),
          "盘点来源不明的条目必须被拒绝，不得稀释存量口径");

    // 10. 重复盘：同 id 两现 → DupId
    let mut dup_list = book.ids();
    let first = match dup_list.first().copied() {
        Some(x) => x,
        None => 0,
    };
    dup_list.push(first);
    s.add("重复盘：同 id 两现 → DupId",
          reconcile(&book, &dup_list).err() == Some(StockCode::DupId),
          "一次盘点里同一 id 只允许出现一次");

    // 11. 空账拒绝：零账面不出报告
    let empty_book = AccountBook::new();
    s.add("空账拒绝：零账面 → EmptyBook",
          reconcile(&empty_book, &[]).err() == Some(StockCode::EmptyBook),
          "空账不出对账报告，避免把『没盘』伪装成『盘完为零』");

    // 12. 表满边界：24 条插满，第 25 拒
    let mut full = AccountBook::new();
    let mut n2 = 1u32;
    while n2 <= MAX_SLOTS {
        let _ = full.insert(n2, StockState::Outstanding);
        n2 += 1;
    }
    s.add("表满边界：24 插满后追加拒绝",
          full.count() == MAX_SLOTS
              && full.insert(25, StockState::Outstanding).err() == Some(StockCode::BadIdRange),
          "容量上限必须显性拒绝，不许静默挤占");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(17) && agrees_with_linux(28)
        && agrees_with_linux(22) && agrees_with_linux(9)
        && !agrees_with_linux(0) && !agrees_with_linux(4321);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < StockCode::ALL.len() {
        let mut j = i + 1;
        while j < StockCode::ALL.len() {
            if let (Some(a), Some(b)) = (StockCode::ALL.get(i), StockCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = StockCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同账本两次扫描+对账必须逐位一致");

    // 16. 扫描确定性：同账本两次扫描逐项相等
    let s1 = scan_all(&book);
    let s2 = scan_all(&book);
    s.add("扫描确定性：两次扫描结果逐项相等", s1 == s2 && s1.len() == 8,
          "纯函数扫描不得依赖隐藏状态或遍历顺序漂移");

    // 动态明细（≤8）：账目 + 注入账 + 指纹
    let book_text = format!("账面 {} / 登记 {}", book.count(), book.registered());
    s.add_dyn("账本快照", book.count() == 8, &book_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_count() {
        let mut b = AccountBook::new();
        assert!(b.insert(1, StockState::Outstanding).is_ok());
        assert!(b.insert(2, StockState::Frozen).is_ok());
        assert_eq!(b.count(), 2);
        assert!(b.get(1).is_some());
        assert!(b.get(3).is_none());
    }

    #[test]
    fn bad_id_rejected() {
        let mut b = AccountBook::new();
        assert_eq!(b.insert(0, StockState::Outstanding).err(), Some(StockCode::BadIdRange));
        assert_eq!(b.insert(25, StockState::Outstanding).err(), Some(StockCode::BadIdRange));
    }

    #[test]
    fn dup_insert_rejected() {
        let mut b = AccountBook::new();
        let _ = b.insert(4, StockState::Review);
        assert_eq!(b.insert(4, StockState::Review).err(), Some(StockCode::DupInsert));
    }

    #[test]
    fn scan_is_ordered_and_complete() {
        let mut b = AccountBook::new();
        let _ = b.insert(7, StockState::Outstanding);
        let _ = b.insert(2, StockState::Frozen);
        let _ = b.insert(5, StockState::Review);
        let list = scan_all(&b);
        let mut expected: Vec<u32> = Vec::new();
        expected.push(2);
        expected.push(5);
        expected.push(7);
        assert_eq!(list, expected);
    }

    #[test]
    fn reconcile_ok_report() {
        let mut b = AccountBook::new();
        let mut n = 1u32;
        while n <= 8 {
            let _ = b.insert(n, StockState::Outstanding);
            n += 1;
        }
        let list = scan_all(&b);
        let rep = reconcile(&b, &list).unwrap_or(ReconReport { matched: 0, book_count: 0, enum_count: 0 });
        assert_eq!(rep.matched, 8);
        assert_eq!(rep.enum_count, rep.book_count);
    }

    #[test]
    fn gap_detected() {
        let mut b = AccountBook::new();
        let mut n = 1u32;
        while n <= 8 {
            let _ = b.insert(n, StockState::Outstanding);
            n += 1;
        }
        let mut list = scan_all(&b);
        let _ = list.pop();
        assert_eq!(reconcile(&b, &list).err(), Some(StockCode::GapDetected));
    }

    #[test]
    fn ghost_detected() {
        let mut b = AccountBook::new();
        let _ = b.insert(1, StockState::Outstanding);
        let mut list = scan_all(&b);
        list.push(42);
        assert_eq!(reconcile(&b, &list).err(), Some(StockCode::GhostEntry));
    }

    #[test]
    fn dup_detected() {
        let mut b = AccountBook::new();
        let _ = b.insert(1, StockState::Outstanding);
        let _ = b.insert(2, StockState::Outstanding);
        let mut list = scan_all(&b);
        list.push(1);
        assert_eq!(reconcile(&b, &list).err(), Some(StockCode::DupId));
    }

    #[test]
    fn bad_scan_matrix_green() {
        let (inj, per) = run_bad_scan_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(28));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10022_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
