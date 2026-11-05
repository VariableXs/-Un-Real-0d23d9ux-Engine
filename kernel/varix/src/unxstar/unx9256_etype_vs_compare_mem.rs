//! UNX-F9256 内存族 E 型与 Windows VirtualAlloc 错误码对照格 · 0x7AD80段
//! 判据原文（任务板 F9256 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9256-J1 ERROR_INVALID_ADDRESS 映射
//!   判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 锚点唯一：ERROR_INVALID_ADDRESS = 487 是本对照的锚，Linux 的 EFAULT/EEXIST/
//!      ENOENT 三种语义在 Windows 侧全部折叠到 487，必须逐条记账而不是一句「类似」。
//!   ② 差异账三条：零长度、固定地址冲突、释放粒度。每条同时列 Linux 行为、Windows
//!      行为与本引擎口径，禁止只写一侧；文本开集即 DiffTextOpen。
//!   ③ 可执行对照：不只查表，还用 WinWorld 真跑 VirtualAlloc/VirtualFree 的
//!      reserve→commit→release 生命周期，证明错误码来自状态机而非常量背诵。
//!   ④ 门次序：参数（长度/对齐/类型）→ 地址冲突 → 内存/容量 → 落位；阶段位必须
//!      是自低位起的前缀，零长度与坏指针对同一占用地址必须先回 87 而不是 487。
//!   ⑤ 容量与守恒：8 个保留槽填满即 8（ERROR_NOT_ENOUGH_MEMORY），释放后名额
//!      可复用；对照行 = 去重码 + 折叠数，锚点行数恒为 3。
//!   ⑥ 注入可复测：每轮篡改一条对照行的 Windows 码，10 轮全部检出，
//!      注入 = 检出 + 逃逸且逃逸恒 0。

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9256 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AD80;
pub const WIN_SUCCESS: u32 = 0;
pub const WIN_NOT_ENOUGH_MEMORY: u32 = 8;
pub const WIN_INVALID_PARAMETER: u32 = 87;
pub const WIN_INVALID_ADDRESS: u32 = 487;
pub const PAGE: u32 = 4096;
pub const WIN_MAX_PAGES: u32 = 16;
pub const WIN_SLOTS: usize = 8;
pub const WIN_BASE: u32 = 0x6000_0000;
pub const RESERVE: u32 = 0x2000;
pub const COMMIT: u32 = 0x1000;
pub const DCOMMIT: u32 = 0x4000;
pub const RELEASE: u32 = 0x8000;
pub const INJECT_ROUNDS: u32 = 10;

/// 阶段位：参数门。
pub const ST_PARAM: u8 = 0x01;
/// 阶段位：地址门。
pub const ST_ADDR: u8 = 0x02;
/// 阶段位：内存/容量门。
pub const ST_MEM: u8 = 0x04;
/// 阶段位：落位/释放。
pub const ST_PLACE: u8 = 0x08;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinDiag {
    AnchorDrift, TableLeak, CollapseLeak, DiffMissing, DiffTextOpen, ParamLeak,
    AddressLeak, MemoryLeak, GateOrder, StageSkip, CapacityLeak, LedgerLeak,
}

impl WinDiag {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + 0x01 + self as u32 }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WinDiag::AnchorDrift => "锚点漂移：ERROR_INVALID_ADDRESS 必须恒为 487，改锚等于全表失真",
            WinDiag::TableLeak => "对照表开集：Linux 码必须逐条入表，表外码不得靠默认值蒙混",
            WinDiag::CollapseLeak => "折叠漏记：EFAULT/EEXIST/ENOENT 折叠到 487 必须逐条计数，不能只留一条",
            WinDiag::DiffMissing => "差异账缺行：零长度、固定冲突、释放粒度三条缺一不可",
            WinDiag::DiffTextOpen => "差异文本不闭集：Linux 侧与 Windows 侧都必须有非空可执行描述",
            WinDiag::ParamLeak => "参数门漏判：零长度/未对齐必须先回 87，不得先报地址冲突 487",
            WinDiag::AddressLeak => "地址门漏判：占用地址或未保留地址必须回 487，不得误报 8 或 87",
            WinDiag::MemoryLeak => "内存门漏判：超上限或槽满必须回 8，不得落位后再失败",
            WinDiag::GateOrder => "门次序错位：参数 → 地址 → 内存 → 落位，错误码不得串位",
            WinDiag::StageSkip => "阶段跳级：阶段位必须是自低位起的前缀，跳级说明门被绕过",
            WinDiag::CapacityLeak => "容量账不闭合：8 槽填满必须回 8，释放后名额必须可复用",
            WinDiag::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [WinDiag; 12] = [
        WinDiag::AnchorDrift, WinDiag::TableLeak, WinDiag::CollapseLeak, WinDiag::DiffMissing,
        WinDiag::DiffTextOpen, WinDiag::ParamLeak, WinDiag::AddressLeak, WinDiag::MemoryLeak,
        WinDiag::GateOrder, WinDiag::StageSkip, WinDiag::CapacityLeak, WinDiag::LedgerLeak,
    ];
}

/// Windows 结果码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinCode { Success, InvalidAddress, InvalidParameter, NotEnoughMemory }

impl WinCode {
    /// 数值码。
    pub fn code(self) -> u32 {
        match self {
            WinCode::Success => WIN_SUCCESS,
            WinCode::InvalidAddress => WIN_INVALID_ADDRESS,
            WinCode::InvalidParameter => WIN_INVALID_PARAMETER,
            WinCode::NotEnoughMemory => WIN_NOT_ENOUGH_MEMORY,
        }
    }
    /// 符号名。
    pub fn label(self) -> &'static str {
        match self {
            WinCode::Success => "ERROR_SUCCESS",
            WinCode::InvalidAddress => "ERROR_INVALID_ADDRESS",
            WinCode::InvalidParameter => "ERROR_INVALID_PARAMETER",
            WinCode::NotEnoughMemory => "ERROR_NOT_ENOUGH_MEMORY",
        }
    }
}

/// Linux 错误码闭集（只列内存族实际用到的负值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinuxErr { EInval, ENomem, EFault, EExist, ENoent }

impl LinuxErr {
    /// 负值。
    pub fn code(self) -> i32 {
        match self {
            LinuxErr::EInval => 22,
            LinuxErr::ENomem => 12,
            LinuxErr::EFault => 14,
            LinuxErr::EExist => 17,
            LinuxErr::ENoent => 2,
        }
    }
}

/// 对照行：一个 Linux 码对应一个 Windows 码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableRow { pub linux: LinuxErr, pub win: WinCode }

/// 5 行冻结对照表。
pub const TABLE: [TableRow; 5] = [
    TableRow { linux: LinuxErr::EInval, win: WinCode::InvalidParameter },
    TableRow { linux: LinuxErr::ENomem, win: WinCode::NotEnoughMemory },
    TableRow { linux: LinuxErr::EFault, win: WinCode::InvalidAddress },
    TableRow { linux: LinuxErr::EExist, win: WinCode::InvalidAddress },
    TableRow { linux: LinuxErr::ENoent, win: WinCode::InvalidAddress },
];

/// Linux 码 → Windows 码（唯一映射函数）。
pub fn win_for(linux: LinuxErr) -> WinCode {
    match linux {
        LinuxErr::EInval => WinCode::InvalidParameter,
        LinuxErr::ENomem => WinCode::NotEnoughMemory,
        LinuxErr::EFault | LinuxErr::EExist | LinuxErr::ENoent => WinCode::InvalidAddress,
    }
}

/// 对照表审计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableAudit {
    pub rows: u32,
    pub distinct: u32,
    pub collapsed_extra: u32,
    pub anchor_hits: u32,
}

/// 审计：行数、去重码数、折叠数、锚点命中行数。
pub fn audit_table() -> TableAudit {
    let mut codes = [WinCode::Success; 5];
    let mut n = 0usize;
    let mut anchor = 0u32;
    for row in TABLE.iter() {
        let mut seen = false;
        let mut k = 0usize;
        while k < n {
            if let Some(c) = codes.get(k) {
                if *c == row.win { seen = true; }
            }
            k += 1;
        }
        if !seen {
            if let Some(c) = codes.get_mut(n) { *c = row.win; n += 1; }
        }
        if row.win == WinCode::InvalidAddress { anchor = anchor.saturating_add(1); }
    }
    TableAudit {
        rows: TABLE.len() as u32,
        distinct: n as u32,
        collapsed_extra: (TABLE.len() as u32).saturating_sub(n as u32),
        anchor_hits: anchor,
    }
}

/// 差异账一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRow { pub id: &'static str, pub linux: &'static str, pub windows: &'static str }

/// 差异账三条（缺一即 DiffMissing）。
pub const DIFFS: [DiffRow; 3] = [
    DiffRow {
        id: "D1-zero-length",
        linux: "POSIX mmap(len=0) 返 EINVAL；本引擎 F9252 正判据返回合法地址且零副作用",
        windows: "VirtualAlloc(size=0) 返 ERROR_INVALID_PARAMETER(87)，无成功分支",
    },
    DiffRow {
        id: "D2-fixed-conflict",
        linux: "MAP_FIXED 覆盖既有映射；MAP_FIXED_NOREPLACE 冲突返 EEXIST(17)",
        windows: "MEM_RESERVE 命中占用地址返 ERROR_INVALID_ADDRESS(487)，不覆盖且无 EEXIST",
    },
    DiffRow {
        id: "D3-release-granularity",
        linux: "munmap 可按页部分解除；本引擎要求整段精确命中否则 EINVAL",
        windows: "MEM_RELEASE 必须整块基址且 size=0，部分释放返 ERROR_INVALID_PARAMETER(87)",
    },
];

/// 差异账闭集判定。
pub fn diffs_closed() -> bool {
    let mut ok = DIFFS.len() == 3;
    for d in DIFFS.iter() {
        if d.id.is_empty() || d.linux.is_empty() || d.windows.is_empty() {
            ok = false;
        }
    }
    ok
}

// ── VirtualAlloc 世界 ─────────────────────────────────────────────────
/// 一段保留区。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Res { pub base: u32, pub pages: u32, pub committed: bool }

/// 世界快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinSnap { pub count: u32, pub seq: u32, pub fp: u32 }

/// Windows 侧最小世界：固定 8 槽保留区。
#[derive(Clone, Copy)]
pub struct WinWorld {
    slots: [Option<Res>; WIN_SLOTS],
    count: u32,
    seq: u32,
}

impl WinWorld {
    /// 空世界。
    pub fn new() -> WinWorld {
        WinWorld { slots: [None; WIN_SLOTS], count: 0, seq: 0 }
    }
    /// 快照。
    pub fn snapshot(&self) -> WinSnap {
        WinSnap { count: self.count, seq: self.seq, fp: self.fingerprint() }
    }
    fn fingerprint(&self) -> u32 {
        let mut h = 0x811C_9DC5u32;
        h = fnv_step(h, (self.count & 0xFF) as u8);
        h = fnv_step(h, (self.seq & 0xFF) as u8);
        let mut i = 0usize;
        while i < WIN_SLOTS {
            if let Some(r) = self.slots.get(i).and_then(|x| x.as_ref()) {
                h = fnv_step(h, (r.base & 0xFF) as u8);
                h = fnv_step(h, (r.pages & 0xFF) as u8);
                h = fnv_step(h, if r.committed { 2 } else { 1 });
            }
            i += 1;
        }
        h
    }
    fn locate(&self, base: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < WIN_SLOTS {
            if let Some(r) = self.slots.get(i).and_then(|x| x.as_ref()) {
                if r.base == base { return Some(i); }
            }
            i += 1;
        }
        None
    }
    fn find_containing(&self, addr: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < WIN_SLOTS {
            if let Some(r) = self.slots.get(i).and_then(|x| x.as_ref()) {
                if let Some(end) = span_end(r.base, r.pages) {
                    if addr >= r.base && addr < end { return Some(i); }
                }
            }
            i += 1;
        }
        None
    }
    fn insert(&mut self, base: u32, pages: u32) -> bool {
        let mut idx = WIN_SLOTS;
        let mut i = 0usize;
        while i < WIN_SLOTS {
            if self.slots.get(i).and_then(|x| x.as_ref()).is_none() { idx = i; break; }
            i += 1;
        }
        if idx == WIN_SLOTS { return false; }
        if let Some(cell) = self.slots.get_mut(idx) {
            *cell = Some(Res { base, pages, committed: false });
        }
        self.count = self.count.saturating_add(1);
        self.seq = self.seq.saturating_add(1);
        true
    }
    /// 分配状态：0 未保留 / 1 已保留 / 2 已提交。
    pub fn state_at(&self, base: u32) -> u8 {
        match self.locate(base) {
            Some(idx) => match self.slots.get(idx).and_then(|x| x.as_ref()) {
                Some(r) if r.committed => 2,
                Some(_) => 1,
                None => 0,
            },
            None => 0,
        }
    }
    /// 保留页数；未保留为 0。
    pub fn pages_at(&self, base: u32) -> u32 {
        match self.locate(base) {
            Some(idx) => match self.slots.get(idx).and_then(|x| x.as_ref()) {
                Some(r) => r.pages,
                None => 0,
            },
            None => 0,
        }
    }
    /// VirtualAlloc：参数 → 地址 → 内存 → 落位。
    pub fn alloc(&mut self, base: u32, size: u32, typ: u32, st: &mut u8) -> Result<u32, u32> {
        *st |= ST_PARAM;
        if size == 0 || base % PAGE != 0 { return Err(WIN_INVALID_PARAMETER); }
        *st |= ST_ADDR;
        if typ & RESERVE != 0 && self.locate(base).is_some() {
            return Err(WIN_INVALID_ADDRESS);
        }
        if typ & COMMIT != 0 && typ & RESERVE == 0 && self.find_containing(base).is_none() {
            return Err(WIN_INVALID_ADDRESS);
        }
        *st |= ST_MEM;
        let pages = match pages_for(size) {
            Some(p) if p <= WIN_MAX_PAGES => p,
            _ => return Err(WIN_NOT_ENOUGH_MEMORY),
        };
        *st |= ST_PLACE;
        if typ & RESERVE != 0 && !self.insert(base, pages) {
            return Err(WIN_NOT_ENOUGH_MEMORY);
        }
        if typ & COMMIT != 0 {
            if let Some(idx) = self.find_containing(base) {
                if let Some(r) = self.slots.get_mut(idx).and_then(|x| x.as_mut()) {
                    if pages > r.pages { return Err(WIN_INVALID_ADDRESS); }
                    r.committed = true;
                }
            }
        }
        Ok(base)
    }
    /// VirtualFree：参数 → 地址 → 释放/去提交。
    pub fn free(&mut self, base: u32, size: u32, typ: u32, st: &mut u8) -> Result<(), u32> {
        *st |= ST_PARAM;
        if typ & RELEASE != 0 {
            if size != 0 { return Err(WIN_INVALID_PARAMETER); }
            *st |= ST_ADDR;
            match self.locate(base) {
                Some(idx) => {
                    if let Some(cell) = self.slots.get_mut(idx) { *cell = None; }
                    self.count = self.count.saturating_sub(1);
                    self.seq = self.seq.saturating_add(1);
                    *st |= ST_MEM;
                    *st |= ST_PLACE;
                    Ok(())
                }
                None => Err(WIN_INVALID_ADDRESS),
            }
        } else {
            if base % PAGE != 0 { return Err(WIN_INVALID_PARAMETER); }
            *st |= ST_ADDR;
            match self.find_containing(base) {
                Some(idx) => {
                    *st |= ST_MEM;
                    if let Some(r) = self.slots.get_mut(idx).and_then(|x| x.as_mut()) {
                        r.committed = false;
                    }
                    *st |= ST_PLACE;
                    Ok(())
                }
                None => Err(WIN_INVALID_ADDRESS),
            }
        }
    }
}

/// 页数向上取整（含溢出保护）。
pub fn pages_for(size: u32) -> Option<u32> {
    size.checked_add(PAGE - 1).map(|v| v / PAGE)
}

/// 跨度末端。
pub fn span_end(base: u32, pages: u32) -> Option<u32> {
    pages.checked_mul(PAGE).and_then(|bytes| base.checked_add(bytes))
}

/// 阶段位是否为自低位起的前缀。
pub fn stages_monotonic(st: u8) -> bool {
    let mut gap = false;
    let mut i = 0u8;
    while i < 4 {
        let bit = 1u8 << i;
        if st & bit == 0 { gap = true; } else if gap { return false; }
        i += 1;
    }
    true
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl InjectLedger {
    /// 闭合判定。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// Windows 码轮转（篡改用）。
pub fn next_win(w: WinCode) -> WinCode {
    match w {
        WinCode::Success => WinCode::InvalidAddress,
        WinCode::InvalidAddress => WinCode::InvalidParameter,
        WinCode::InvalidParameter => WinCode::NotEnoughMemory,
        WinCode::NotEnoughMemory => WinCode::Success,
    }
}

/// 篡改一条对照行。
pub fn mutate_row(row: &TableRow) -> TableRow {
    TableRow { linux: row.linux, win: next_win(row.win) }
}

/// 10 轮篡改：每轮换一行，检测器必须全部抓住。
pub fn run_mutation_matrix() -> InjectLedger {
    let mut led = InjectLedger { injected: 0, detected: 0, escaped: 0 };
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if let Some(row) = TABLE.get((round as usize) % TABLE.len()) {
            let mutated = mutate_row(row);
            led.injected = led.injected.saturating_add(1);
            if win_for(mutated.linux) == mutated.win {
                led.escaped = led.escaped.saturating_add(1);
            } else {
                led.detected = led.detected.saturating_add(1);
            }
        }
        round += 1;
    }
    led
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 { (h ^ b as u32).wrapping_mul(0x0100_0193) }

fn hash_str(mut h: u32, s: &'static str) -> u32 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.as_bytes().get(i) { h = fnv_step(h, *b); }
        i += 1;
    }
    h
}

/// 对照表 + 差异账 + 生命周期脚本的指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for row in TABLE.iter() {
        h = fnv_step(h, row.linux.code() as u8);
        h = fnv_step(h, row.win.code() as u8);
    }
    for d in DIFFS.iter() {
        h = hash_str(h, d.id);
    }
    let mut w = WinWorld::new();
    let mut st = 0u8;
    let _ = w.alloc(WIN_BASE, PAGE, RESERVE, &mut st);
    let _ = w.alloc(WIN_BASE, PAGE, COMMIT, &mut st);
    let s = w.snapshot();
    h = fnv_step(h, (s.count & 0xFF) as u8);
    h = fnv_step(h, (s.seq & 0xFF) as u8);
    h = fnv_step(h, (s.fp & 0xFF) as u8);
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9256 判据集（16 条静态 + 1 条动态明细）。
pub fn run_unx9256_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9256");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. J1 锚点
    let anchor_ok = WIN_INVALID_ADDRESS == 487
        && win_for(LinuxErr::EFault).code() == 487
        && win_for(LinuxErr::EExist).code() == 487
        && win_for(LinuxErr::ENoent).code() == 487;
    s.add("J1 锚点：ERROR_INVALID_ADDRESS=487 覆盖 EFAULT/EEXIST/ENOENT", anchor_ok,
          "锚点必须唯一且三种地址语义全部落到 487，不得另立新码");

    // 3. 对照表闭集
    let audit = audit_table();
    let table_ok = audit.rows == 5 && audit.distinct == 3 && TABLE.len() == 5;
    s.add("对照表：5 行冻结、3 个去重 Windows 码", table_ok,
          "Linux 码逐个入表，表外码不得靠默认值蒙混");

    // 4. 折叠账
    s.add("折叠账：487 命中 3 行，折叠冗余 2 行", audit.anchor_hits == 3
          && audit.collapsed_extra == 2 && audit.rows == audit.distinct + audit.collapsed_extra,
          "折叠不是丢行：3 条 Linux 语义必须都看得见");

    // 5. 映射单源
    let mapped_ok = TABLE.iter().all(|r| win_for(r.linux) == r.win);
    s.add("映射单源：逐行 win_for 与冻结表一致", mapped_ok,
          "查表函数与冻结表必须同源，改一处必须两处同漂移");

    // 6. 差异账三条
    let ids_ok = DIFFS.get(0).map_or(false, |d| d.id == "D1-zero-length")
        && DIFFS.get(1).map_or(false, |d| d.id == "D2-fixed-conflict")
        && DIFFS.get(2).map_or(false, |d| d.id == "D3-release-granularity");
    s.add("差异账：零长度/固定冲突/释放粒度三条全列", diffs_closed() && ids_ok,
          "三条缺一即差异账不成立，不能只写 Windows 单侧");

    // 7. 正常流
    let mut w = WinWorld::new();
    let mut st = 0u8;
    let r1 = w.alloc(WIN_BASE, PAGE, RESERVE, &mut st);
    let r2 = w.alloc(WIN_BASE, PAGE, COMMIT, &mut st);
    let r3 = w.free(WIN_BASE, 0, RELEASE, &mut st);
    let normal_ok = r1 == Ok(WIN_BASE) && r2 == Ok(WIN_BASE) && r3 == Ok(())
        && w.state_at(WIN_BASE) == 0;
    s.add("正常流：reserve → commit → release 全成功", normal_ok,
          "合法生命周期必须零错误码，释放后状态必须回到未保留");

    // 8. 边界
    let mut b0 = 0u8;
    let zero = w.alloc(WIN_BASE, 0, RESERVE, &mut b0) == Err(WIN_INVALID_PARAMETER);
    let mut b1 = 0u8;
    let unaligned = w.alloc(WIN_BASE + 1, PAGE, RESERVE, &mut b1) == Err(WIN_INVALID_PARAMETER);
    let mut b2 = 0u8;
    let rounded = w.alloc(WIN_BASE, PAGE + 1, RESERVE, &mut b2) == Ok(WIN_BASE)
        && w.pages_at(WIN_BASE) == 2;
    let mut b3 = 0u8;
    let huge = w.alloc(WIN_BASE + PAGE * 4, WIN_MAX_PAGES * PAGE + 1, RESERVE, &mut b3)
        == Err(WIN_NOT_ENOUGH_MEMORY);
    s.add("边界：零长度/未对齐回 87，4097 字节取 2 页，超上限回 8",
          zero && unaligned && rounded && huge,
          "页数向上取整与上限必须在落位前判完");

    // 9. 地址冲突
    let mut c0 = 0u8;
    let _ = w.alloc(WIN_BASE + PAGE * 2, PAGE, RESERVE, &mut c0);
    let mut c1 = 0u8;
    let dup = w.alloc(WIN_BASE + PAGE * 2, PAGE, RESERVE, &mut c1) == Err(WIN_INVALID_ADDRESS);
    let mut c2 = 0u8;
    let free_bad = w.free(WIN_BASE + PAGE * 6, 0, RELEASE, &mut c2) == Err(WIN_INVALID_ADDRESS);
    let mut c3 = 0u8;
    let commit_bad = w.alloc(WIN_BASE + PAGE * 6, PAGE, COMMIT, &mut c3)
        == Err(WIN_INVALID_ADDRESS);
    s.add("地址门：重复保留/释放未保留/提交未保留全回 487", dup && free_bad && commit_bad,
          "Windows 没有 EEXIST：占用与缺失都折叠为 ERROR_INVALID_ADDRESS");

    // 10. 判定次序
    let mut o0 = 0u8;
    let param_first = w.alloc(WIN_BASE + PAGE * 2, 0, RESERVE, &mut o0)
        == Err(WIN_INVALID_PARAMETER) && o0 & ST_ADDR == 0;
    let mut o1 = 0u8;
    let huge_after_addr = w.alloc(WIN_BASE + PAGE * 8, WIN_MAX_PAGES * PAGE + 1, RESERVE, &mut o1)
        == Err(WIN_NOT_ENOUGH_MEMORY) && o1 & ST_ADDR != 0 && o1 & ST_PLACE == 0;
    s.add("判定次序：参数 87 先于地址 487 先于内存 8", param_first && huge_after_addr,
          "占用地址配零长度必须先回 87，错误码不得串位");

    // 11. 状态机
    let mut m = WinWorld::new();
    let mut ms = 0u8;
    let s0 = m.state_at(WIN_BASE);
    let _ = m.alloc(WIN_BASE, PAGE, RESERVE, &mut ms);
    let s1 = m.state_at(WIN_BASE);
    let _ = m.alloc(WIN_BASE, PAGE, COMMIT, &mut ms);
    let s2 = m.state_at(WIN_BASE);
    let _ = m.free(WIN_BASE, 0, RELEASE, &mut ms);
    let s3 = m.state_at(WIN_BASE);
    let mut bad = 0u8;
    let illegal = m.alloc(WIN_BASE, PAGE, COMMIT, &mut bad) == Err(WIN_INVALID_ADDRESS);
    let state_ok = s0 == 0 && s1 == 1 && s2 == 2 && s3 == 0 && illegal
        && stages_monotonic(ms) && !stages_monotonic(ST_PLACE) && !stages_monotonic(ST_MEM);
    s.add("状态机：0→1→2→0 单向，释放后提交被拒", state_ok,
          "阶段位必须为前缀，已释放地址再提交必须回 487");

    // 12. 容量
    let mut cap = WinWorld::new();
    let mut filled = 0u32;
    let mut i = 0u32;
    while i < WIN_SLOTS as u32 {
        let mut cs = 0u8;
        if cap.alloc(WIN_BASE + i * PAGE, PAGE, RESERVE, &mut cs) == Ok(WIN_BASE + i * PAGE) {
            filled = filled.saturating_add(1);
        }
        i += 1;
    }
    let mut cs = 0u8;
    let ninth = cap.alloc(WIN_BASE + 8 * PAGE, PAGE, RESERVE, &mut cs)
        == Err(WIN_NOT_ENOUGH_MEMORY);
    let mut fs = 0u8;
    let freed = cap.free(WIN_BASE, 0, RELEASE, &mut fs) == Ok(());
    let mut rs = 0u8;
    let reused = cap.alloc(WIN_BASE + 8 * PAGE, PAGE, RESERVE, &mut rs)
        == Ok(WIN_BASE + 8 * PAGE);
    s.add("容量：8 槽填满回 8，释放后名额可复用", filled == 8 && ninth && freed && reused,
          "槽满必须拒绝而不是覆盖，释放名额必须回收");

    // 13. 双跑一致
    s.add("双跑一致：对照表 + 差异账 + 生命周期指纹相等",
          corpus_fingerprint() == corpus_fingerprint(),
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 注入必抓
    let led = run_mutation_matrix();
    s.add("注入必抓：篡改对照行 10/10 检出",
          led.injected == 10 && led.detected == 10 && led.escaped == 0 && led.conserved(),
          "每轮篡改一条 Windows 码，漏抓一次即对照失效");

    // 15. 守恒
    s.add("守恒：对照行 = 去重码 + 折叠数，差异账恒 3 条",
          audit.rows == audit.distinct + audit.collapsed_extra && DIFFS.len() == 3,
          "行不许丢也不许重复，折叠冗余必须显式计数");

    // 16. 文案闭集
    let text_ok = WinDiag::ALL.iter().all(|c| !c.detail().is_empty());
    let uniq = WinDiag::ALL.iter().enumerate().all(|(u, x)| {
        WinDiag::ALL.iter().skip(u + 1).all(|y| x.val() != y.val())
    });
    s.add("文案闭集：12 码均有中文建议且码值唯一", text_ok && uniq,
          "任一条为空或重码，判据都无法定位到具体违约点");

    let text = format!(
        "对照 {} 行 / 去重 {} / 折叠 {} / 锚点 {} / 差异 {} 条 / 注入 {}/{} 逃逸 {}",
        audit.rows, audit.distinct, audit.collapsed_extra, audit.anchor_hits, DIFFS.len(),
        led.detected, led.injected, led.escaped
    );
    s.add_dyn("对照与注入统计", led.conserved() && audit.anchor_hits == 3, &text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_and_table() {
        assert_eq!(WIN_INVALID_ADDRESS, 487);
        assert_eq!(win_for(LinuxErr::EFault), WinCode::InvalidAddress);
        assert_eq!(win_for(LinuxErr::EExist), WinCode::InvalidAddress);
        assert_eq!(win_for(LinuxErr::ENoent), WinCode::InvalidAddress);
        assert_eq!(TABLE.len(), 5);
    }

    #[test]
    fn collapse_groups() {
        let a = audit_table();
        assert_eq!(a.rows, 5);
        assert_eq!(a.distinct, 3);
        assert_eq!(a.collapsed_extra, 2);
        assert_eq!(a.anchor_hits, 3);
    }

    #[test]
    fn three_diffs() {
        assert!(diffs_closed());
        assert_eq!(DIFFS.len(), 3);
        assert_eq!(DIFFS.get(0).map(|d| d.id), Some("D1-zero-length"));
        assert_eq!(DIFFS.get(1).map(|d| d.id), Some("D2-fixed-conflict"));
        assert_eq!(DIFFS.get(2).map(|d| d.id), Some("D3-release-granularity"));
    }

    #[test]
    fn lifecycle() {
        let mut w = WinWorld::new();
        let mut st = 0u8;
        assert_eq!(w.alloc(WIN_BASE, PAGE, RESERVE, &mut st), Ok(WIN_BASE));
        assert_eq!(w.state_at(WIN_BASE), 1);
        assert_eq!(w.alloc(WIN_BASE, PAGE, COMMIT, &mut st), Ok(WIN_BASE));
        assert_eq!(w.state_at(WIN_BASE), 2);
        assert_eq!(w.free(WIN_BASE, 0, RELEASE, &mut st), Ok(()));
        assert_eq!(w.state_at(WIN_BASE), 0);
    }

    #[test]
    fn boundaries() {
        let mut w = WinWorld::new();
        let mut st = 0u8;
        assert_eq!(w.alloc(WIN_BASE, 0, RESERVE, &mut st), Err(WIN_INVALID_PARAMETER));
        assert_eq!(w.alloc(WIN_BASE + 1, PAGE, RESERVE, &mut st), Err(WIN_INVALID_PARAMETER));
        assert_eq!(w.alloc(WIN_BASE, PAGE + 1, RESERVE, &mut st), Ok(WIN_BASE));
        assert_eq!(w.pages_at(WIN_BASE), 2);
    }

    #[test]
    fn gate_order() {
        let mut w = WinWorld::new();
        let mut st = 0u8;
        let _ = w.alloc(WIN_BASE, PAGE, RESERVE, &mut st);
        let mut s1 = 0u8;
        assert_eq!(w.alloc(WIN_BASE, 0, RESERVE, &mut s1), Err(WIN_INVALID_PARAMETER));
        assert_eq!(s1 & ST_ADDR, 0);
        let mut s2 = 0u8;
        assert_eq!(w.alloc(WIN_BASE, PAGE, RESERVE, &mut s2), Err(WIN_INVALID_ADDRESS));
        assert_eq!(s2 & ST_MEM, 0);
    }

    #[test]
    fn mutation_caught_ten_of_ten() {
        let led = run_mutation_matrix();
        assert_eq!(led.injected, 10);
        assert_eq!(led.detected, 10);
        assert_eq!(led.escaped, 0);
        assert!(led.conserved());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9256_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
