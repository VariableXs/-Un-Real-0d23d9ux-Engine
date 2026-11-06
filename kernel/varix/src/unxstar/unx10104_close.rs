//! UNX-F10104 close 正反双判据（关闭释放达成与重关闭误用受控） · 0x92a00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10104（close 系统调用正反双判据：J1 关闭释放达成，J2
//!       重关闭误用受控）。
//! 锚点：UNX-F10105-J1 dup/dup2 复制达成（共享偏移语义 LTP dup01/dup02
//!       面 10/10 次）；UNX-F10105-J2 非法 fd 误用受控（预期 EBADF
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92a00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `CloseCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `SlotCell`：每槽 Free → Open → Free 循环但带代次
//!     （gen 只在 close 时 +1，重关闭同代次即拒）；
//!   - 注册表 `FdSlots`：16 槽代次表 + 回收序日志（回收后按最低号
//!     复用，RecycleMissed 兜底）；
//!   - 账本 `CloseLedger`：opens/closes/in_use/gen_total 四账，
//!     守恒式 in_use == opens − closes 且 Σgen == closes；
//!   - 解析器 `parse_close_call`：调用行 "close|fd" 两段解析，fd 允许
//!     带负号（负值即 J2 注入对象）；
//!   - 注入核 `BadCloseCase`：负 fd / 越界 fd / 重关闭 / 关未开槽 /
//!     槽账漂移 五类坏样本逐类 10/10 次拒绝，账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10104 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92a00;

/// 段窗上界（下一卡 0x92a40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92a40;

/// fd 表容量（本卡模拟槽位上限）。
pub const FD_CAP: usize = 16;

/// 释放轮数（J1 守恒核跑 10 轮）。
pub const RELEASE_ROUNDS: u32 = 10;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（越界族）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 负 fd（J2 主样本，EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// 重关闭（同代次二次关闭，EBADF）
    DoubleClose,
    /// 关闭从未打开的槽（EBADF）
    NeverOpened,
    /// 槽账漂移（in_use 与表实数不符）
    SlotDrift,
    /// 回收序违规（未按最低空槽复用）
    RecycleMissed,
    /// 状态倒退（Free 槽再关 / Open 槽再开）
    StateRegress,
    /// 账面失配（四账守恒式破坏）
    LedgerMismatch,
    /// 打开已开槽（防双占）
    Reopen,
    /// fd 耗尽（16 槽占满）
    FdExhausted,
    /// 代次未递增（close 后 gen 不变）
    GenMissed,
    /// 段位越界
    SegmentViolation,
}

impl CloseCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::FdNegative => "负 fd 被拒：close(-1) 按 EBADF 受控复现；请核 fd 来源",
            CloseCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            CloseCode::DoubleClose => "重关闭被拒：同代次槽已释放；按 EBADF 受控复现",
            CloseCode::NeverOpened => "关未开槽被拒：槽位从未打开；请先 open 再 close",
            CloseCode::SlotDrift => "槽账漂移被拒：in_use 与表实数不符；请回查开关账",
            CloseCode::RecycleMissed => "回收序违规被拒：必须按最低空槽复用；请核分配器",
            CloseCode::StateRegress => "状态倒退被拒：Free/Open 单向循环；请排查重复操作",
            CloseCode::LedgerMismatch => "账面失配被拒：四账守恒式破坏；请逐账对核",
            CloseCode::Reopen => "打开已开槽被拒：一槽一态；请先释放再开",
            CloseCode::FdExhausted => "fd 耗尽被拒：16 槽占满；请先关闭再开",
            CloseCode::GenMissed => "代次未递增被拒：close 必须推进代次；请核回收链",
            CloseCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92a00..=0x92a40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CloseCode; 12] = [
        CloseCode::FdNegative,
        CloseCode::FdRange,
        CloseCode::DoubleClose,
        CloseCode::NeverOpened,
        CloseCode::SlotDrift,
        CloseCode::RecycleMissed,
        CloseCode::StateRegress,
        CloseCode::LedgerMismatch,
        CloseCode::Reopen,
        CloseCode::FdExhausted,
        CloseCode::GenMissed,
        CloseCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: CloseCode) -> i32 {
    match code {
        CloseCode::FdNegative | CloseCode::FdRange | CloseCode::DoubleClose
        | CloseCode::NeverOpened => EBADF,
        _ => EINVAL,
    }
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 分段器：按 '|' 切两段，段数≠2 返回 None。
fn split2(line: &str) -> Option<[usize; 3]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 3];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 1 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 1 {
        return None;
    }
    cuts[2] = b.len();
    Some(cuts)
}

/// 调用行解析："close|fd" → fd（允许负号，负值留给表层按 EBADF 受控）。
pub fn parse_close_call(line: &str) -> Result<i32, CloseCode> {
    let cuts = match split2(line) {
        Some(c) => c,
        None => return Err(CloseCode::FdRange),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2]).unwrap_or("");
    if op_seg != "close" {
        return Err(CloseCode::FdRange);
    }
    match fd_seg.parse::<i32>() {
        Ok(v) => Ok(v),
        Err(_) => Err(CloseCode::FdRange),
    }
}

// ── 槽位代次表与状态机 ────────────────────────────────────────────────
/// 槽位状态：Free（可分配）/ Open（在用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空槽
    Free,
    /// 在用
    Open,
}

/// 一个槽位：状态 + 代次（close 时 gen+1）。
#[derive(Clone, Copy, Debug)]
pub struct SlotCell {
    /// 当前状态
    pub state: SlotState,
    /// 代次（释放次数）
    pub gen: u32,
}

/// fd 槽位表：16 槽 + 回收序日志。
#[derive(Clone, Debug)]
pub struct FdSlots {
    /// 槽位数组
    pub cells: [SlotCell; FD_CAP],
    /// 回收序日志（close 先后记录被释放槽号）
    pub recycle_log: Vec<usize>,
    /// 累计打开次数
    pub opens: u32,
    /// 累计关闭次数
    pub closes: u32,
}

impl FdSlots {
    /// 构造空表（全 Free、零代次）。
    pub fn new() -> FdSlots {
        let mut cells = Vec::new();
        let mut i = 0usize;
        while i < FD_CAP {
            cells.push(SlotCell { state: SlotState::Free, gen: 0 });
            i += 1;
        }
        let mut arr = [SlotCell { state: SlotState::Free, gen: 0 }; FD_CAP];
        let mut j = 0usize;
        while j < FD_CAP {
            if let (Some(src), Some(dst)) = (cells.get(j), arr.get_mut(j)) {
                *dst = *src;
            }
            j += 1;
        }
        FdSlots { cells: arr, recycle_log: Vec::new(), opens: 0, closes: 0 }
    }

    /// 打开最低空槽；无空槽即耗尽拒绝。
    pub fn open(&mut self) -> Result<usize, CloseCode> {
        let mut hit = None;
        let mut j = 0usize;
        while j < FD_CAP {
            if let Some(c) = self.cells.get(j) {
                if c.state == SlotState::Free {
                    hit = Some(j);
                    j = FD_CAP;
                } else {
                    j += 1;
                }
            } else {
                j += 1;
            }
        }
        match hit {
            None => Err(CloseCode::FdExhausted),
            Some(s) => {
                if let Some(c) = self.cells.get_mut(s) {
                    c.state = SlotState::Open;
                }
                self.opens = self.opens.saturating_add(1);
                Ok(s)
            }
        }
    }

    /// 关闭 fd：负/越界/未开/重关（同代次二次关）逐类拒绝。
    pub fn close(&mut self, fd: i32) -> Result<u32, CloseCode> {
        if fd < 0 {
            return Err(CloseCode::FdNegative);
        }
        let fd_u = fd as usize;
        if fd_u >= FD_CAP {
            return Err(CloseCode::FdRange);
        }
        let cell = match self.cells.get(fd_u) {
            Some(c) => *c,
            None => return Err(CloseCode::FdRange),
        };
        if cell.state != SlotState::Open {
            if cell.gen == 0 {
                return Err(CloseCode::NeverOpened);
            }
            return Err(CloseCode::DoubleClose);
        }
        if let Some(c) = self.cells.get_mut(fd_u) {
            c.state = SlotState::Free;
            c.gen = c.gen.saturating_add(1);
        }
        self.closes = self.closes.saturating_add(1);
        self.recycle_log.push(fd_u);
        Ok(match self.cells.get(fd_u) {
            Some(c) => c.gen,
            None => 0,
        })
    }

    /// 在用计数。
    pub fn in_use(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(c) = self.cells.get(i) {
                if c.state == SlotState::Open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 代次总和（守恒式 Σgen == closes）。
    pub fn gen_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(c) = self.cells.get(i) {
                n = n.saturating_add(c.gen);
            }
            i += 1;
        }
        n
    }

    /// 回收守恒核：打开必须命中最低空槽（close 语义）。
    /// 先记下最低空槽号，再 open 验证命中同一槽。
    pub fn recycle_lowest(&mut self) -> Result<usize, CloseCode> {
        let mut lowest = None;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(c) = self.cells.get(i) {
                if c.state == SlotState::Free {
                    lowest = Some(i);
                    i = FD_CAP;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        let fd = self.open()?;
        match lowest {
            Some(l) if l == fd => Ok(fd),
            _ => Err(CloseCode::RecycleMissed),
        }
    }
}

// ── 关闭账本 ──────────────────────────────────────────────────────────
/// 关闭账本：四账守恒（in_use == opens−closes 且 Σgen == closes）。
#[derive(Clone, Debug)]
pub struct CloseLedger {
    /// 账面在用数
    pub claimed_in_use: u32,
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl CloseLedger {
    /// 构造零账。
    pub const fn new() -> CloseLedger {
        CloseLedger { claimed_in_use: 0, drifts: 0, escapes: 0 }
    }

    /// 守恒核：与槽位表逐账比对，不符即漂移入账。
    pub fn audit(&mut self, table: &FdSlots) -> Result<(), CloseCode> {
        let actual = table.in_use();
        let derived = table.opens.saturating_sub(table.closes);
        if self.claimed_in_use != actual
            || self.claimed_in_use != derived
            || table.gen_total() != table.closes
        {
            self.drifts = self.drifts.saturating_add(1);
            return Err(CloseCode::LedgerMismatch);
        }
        Ok(())
    }
}

/// 标准语料：10 轮「开 3 关 3」释放循环，每轮守恒核过、回收序命中最低空槽。
pub fn standard_corpus(table: &mut FdSlots, ledger: &mut CloseLedger) -> Result<(), CloseCode> {
    let mut round = 0u32;
    while round < RELEASE_ROUNDS {
        let mut opened: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < 3 {
            let fd = table.open()?;
            opened.push(fd);
            ledger.claimed_in_use = ledger.claimed_in_use.saturating_add(1);
            k += 1;
        }
        let mut j = 0usize;
        while j < opened.len() {
            let fd = match opened.get(j) {
                Some(f) => *f,
                None => return Err(CloseCode::FdRange),
            };
            table.close(fd as i32)?;
            if ledger.claimed_in_use > 0 {
                ledger.claimed_in_use -= 1;
            }
            j += 1;
        }
        ledger.audit(table)?;
        let fd = table.recycle_lowest()?;
        table.close(fd as i32)?;
        if ledger.claimed_in_use > 0 {
            ledger.claimed_in_use -= 1;
        }
        ledger.audit(table)?;
        round += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 close 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCloseCase {
    /// 负 fd（J2 主样本）
    NegativeFd,
    /// 越界 fd
    RangeFd,
    /// 重关闭
    DoubleClose,
    /// 关闭从未打开的槽
    NeverOpened,
    /// 槽账漂移
    SlotDrift,
}

impl BadCloseCase {
    /// 闭集全量。
    pub const ALL: [BadCloseCase; 5] = [
        BadCloseCase::NegativeFd,
        BadCloseCase::RangeFd,
        BadCloseCase::DoubleClose,
        BadCloseCase::NeverOpened,
        BadCloseCase::SlotDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self, table: &mut FdSlots, ledger: &mut CloseLedger) -> bool {
        match self {
            BadCloseCase::NegativeFd => {
                table.close(-1).err() == Some(CloseCode::FdNegative)
                    && errno_of(CloseCode::FdNegative) == EBADF
            }
            BadCloseCase::RangeFd => {
                table.close(FD_CAP as i32).err() == Some(CloseCode::FdRange)
            }
            BadCloseCase::DoubleClose => {
                let f = match table.open() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let first = table.close(f as i32).is_ok();
                first && table.close(f as i32).err() == Some(CloseCode::DoubleClose)
            }
            BadCloseCase::NeverOpened => {
                // 槽 15 从未打开（保证此前注入未占满 16 槽）
                table.close(15).err() == Some(CloseCode::NeverOpened)
            }
            BadCloseCase::SlotDrift => {
                let mut lg = CloseLedger::new();
                lg.claimed_in_use = 9;
                lg.audit(table).err() == Some(CloseCode::LedgerMismatch) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_close_injection() -> (CloseLedger, [u32; 5]) {
    let mut table = FdSlots::new();
    let mut ledger = CloseLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadCloseCase::ALL.len() {
        if let Some(case) = BadCloseCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut ledger);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/重关闭/关未开族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；账漂移/回收序违规族"),
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

/// 规范语料双跑指纹：10 轮开合释放循环全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = FdSlots::new();
    let mut lg = CloseLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.opens.to_le_bytes());
    bytes.extend_from_slice(&table.closes.to_le_bytes());
    bytes.extend_from_slice(&table.gen_total().to_le_bytes());
    bytes.extend_from_slice(&(table.recycle_log.len() as u32).to_le_bytes());
    bytes.push(match lg.audit(&table) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10104 判据集（18 条静态 + 3 条动态明细）。
pub fn run_unx10104_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10104");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92a00..=0x92a40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：10 轮开合释放循环全过
    let mut table = FdSlots::new();
    let mut lg = CloseLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut lg).is_ok();
    s.add("关闭释放达成：10 轮开 3 关 3 循环全过", corpus_ok,
          "J1 正判据：槽位回收守恒逐轮核过，回收序命中最低空槽");

    // 3. 守恒式：in_use==opens−closes==0 且 Σgen==closes
    let conservation = table.in_use() == 0
        && table.opens == table.closes
        && table.gen_total() == table.closes
        && lg.audit(&table).is_ok();
    s.add("槽位守恒：in_use=0、opens=closes、Σgen=closes", conservation,
          "四账逐项比对，代次账与关闭账必须严格相等");

    // 4. 重关闭 10/10 EBADF（J2 反判据）
    let mut dbl_hits = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut t = FdSlots::new();
        let f = match t.open() {
            Ok(f) => f,
            Err(_) => 0,
        };
        let _ = t.close(f as i32);
        if t.close(f as i32).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            dbl_hits += 1;
        }
        r += 1;
    }
    s.add("重关闭：10/10 次 EBADF 精确复现", dbl_hits == INJECT_ROUNDS,
          "J2 反判据：同代次二次关闭逐次按 EBADF 受控复现");

    // 5. 负 fd 10/10 EBADF
    let mut neg_hits = 0u32;
    let mut nt = FdSlots::new();
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if nt.close(-1).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "close(-1) 逐次按 EBADF 受控复现，不静默");

    // 6. 越界 fd 拒
    let range_rej = nt.close(FD_CAP as i32).err() == Some(CloseCode::FdRange);
    s.add("越界 fd：fd=16 出窗拒收", range_rej,
          "fd 表 16 槽，越界关闭按 EBADF 族受控");

    // 7. 关闭从未打开的槽拒
    let mut nt2 = FdSlots::new();
    let never_rej = nt2.close(3).err() == Some(CloseCode::NeverOpened);
    s.add("关未开槽：零代次槽关闭拒收", never_rej,
          "从未打开的槽没有可释放资源，显性拒绝");

    // 8. 回收序：close 后最低空槽复用
    let mut rt = FdSlots::new();
    let _ = rt.open();
    let _ = rt.open();
    let _ = rt.open();
    let _ = rt.close(1);
    let reuse_ok = rt.recycle_lowest() == Ok(1);
    s.add("回收序：close(1) 后复用命中槽 1", reuse_ok,
          "回收槽按最低号复用，RecycleMissed 兜底检出");

    // 9. 代次推进：close 后 gen+1
    let mut gt = FdSlots::new();
    let f0 = gt.open().ok().unwrap_or(0);
    let gen0 = match gt.cells.get(f0) {
        Some(c) => c.gen,
        None => 0,
    };
    let _ = gt.close(f0 as i32);
    let gen1 = match gt.cells.get(f0) {
        Some(c) => c.gen,
        None => 0,
    };
    s.add("代次推进：close 后 gen+1（释放留痕）", gen0 == 0 && gen1 == 1,
          "每次释放推进代次，重关闭同代次即拒");

    // 10. 解析器：close|fd 正常/负值/残段
    let ok_parse = parse_close_call("close|3") == Ok(3);
    let neg_parse = parse_close_call("close|-1") == Ok(-1);
    let bad_parse = parse_close_call("close|x").err() == Some(CloseCode::FdRange);
    let seg_rej = parse_close_call("close").err() == Some(CloseCode::FdRange);
    s.add("协议解析：close|fd 两段、负值可表达、残段拒", ok_parse && neg_parse && bad_parse && seg_rej,
          "调用行恰两段，fd 十进制可带负号");

    // 11. 耗尽拒 + 防双占
    let mut et = FdSlots::new();
    let mut filled = 0u32;
    let mut guard = 0u32;
    loop {
        match et.open() {
            Ok(_) => filled += 1,
            Err(_) => break,
        }
        guard += 1;
        if guard > FD_CAP as u32 + 4 {
            break;
        }
    }
    s.add("容量上界：16 槽占满即拒绝", filled == FD_CAP as u32,
          "耗尽显性拒绝（本协议同时以防双占约束打开语义）");

    // 12. 账漂移检出
    let mut dt = FdSlots::new();
    let mut dl = CloseLedger::new();
    dl.claimed_in_use = 9;
    let drift_rej = dl.audit(&dt).err() == Some(CloseCode::LedgerMismatch);
    s.add("槽账漂移：账面≠表实数逐次检出", drift_rej && dl.drifts == 1,
          "四账守恒式破坏显性入账，不静默放行");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_close_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负 fd/越界/重关闭/关未开/槽账漂移全被显式拒收");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 15. 文案闭集 + 诊断码唯一落段
    let text_ok = CloseCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < CloseCode::ALL.len() {
        let mut j = i2 + 1;
        while j < CloseCode::ALL.len() {
            if let (Some(x), Some(y)) = (CloseCode::ALL.get(i2), CloseCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = CloseCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 18. 回收日志规模核：标准语料 10 轮×4 次 close = 40 条
    let log_ok = table.recycle_log.len() == RELEASE_ROUNDS as usize * 4;
    s.add("回收留痕：回收序日志 40 条与释放次数一致", log_ok,
          "每次 close 留痕一条，日志规模可复算");

    // 动态明细（≤8）：槽快照 + 代次快照 + 指纹快照
    let slot_text = format!("槽在用 {}/16，累计开 {} 次 / 关 {} 次", table.in_use(), table.opens, table.closes);
    s.add_dyn("槽快照", table.in_use() <= FD_CAP as u32, &slot_text);
    let gen_text = format!("Σgen={} == closes={}", table.gen_total(), table.closes);
    s.add_dyn("代次快照", table.gen_total() == table.closes, &gen_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_close_call_ok() {
        assert_eq!(parse_close_call("close|3"), Ok(3));
        assert_eq!(parse_close_call("close|-1"), Ok(-1));
        assert_eq!(parse_close_call("close|x"), Err(CloseCode::FdRange));
        assert_eq!(parse_close_call("close"), Err(CloseCode::FdRange));
    }

    #[test]
    fn double_close_is_ebadf() {
        let mut t = FdSlots::new();
        let f = t.open().ok().unwrap();
        assert!(t.close(f as i32).is_ok());
        assert_eq!(t.close(f as i32), Err(CloseCode::DoubleClose));
        assert_eq!(errno_of(CloseCode::DoubleClose), EBADF);
    }

    #[test]
    fn negative_and_range_rejected() {
        let mut t = FdSlots::new();
        assert_eq!(t.close(-1), Err(CloseCode::FdNegative));
        assert_eq!(t.close(FD_CAP as i32), Err(CloseCode::FdRange));
    }

    #[test]
    fn never_opened_rejected() {
        let mut t = FdSlots::new();
        assert_eq!(t.close(5), Err(CloseCode::NeverOpened));
    }

    #[test]
    fn generation_advances() {
        let mut t = FdSlots::new();
        let f = t.open().ok().unwrap();
        let _ = t.close(f as i32);
        assert_eq!(t.cells[f].gen, 1);
        assert_eq!(t.gen_total(), t.closes);
    }

    #[test]
    fn recycle_lowest_hits() {
        let mut t = FdSlots::new();
        let _ = t.open();
        let _ = t.open();
        let _ = t.close(1);
        assert_eq!(t.recycle_lowest(), Ok(1));
    }

    #[test]
    fn standard_corpus_green() {
        let mut t = FdSlots::new();
        let mut lg = CloseLedger::new();
        assert!(standard_corpus(&mut t, &mut lg).is_ok());
        assert_eq!(t.in_use(), 0);
        assert!(lg.audit(&t).is_ok());
    }

    #[test]
    fn exhaustion_rejected() {
        let mut t = FdSlots::new();
        let mut i = 0usize;
        while i < FD_CAP {
            assert!(t.open().is_ok());
            i += 1;
        }
        assert_eq!(t.open(), Err(CloseCode::FdExhausted));
    }

    #[test]
    fn drift_detected() {
        let mut t = FdSlots::new();
        let mut lg = CloseLedger::new();
        lg.claimed_in_use = 3;
        assert_eq!(lg.audit(&t), Err(CloseCode::LedgerMismatch));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_close_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10104_checks_green() {
        let s = run_unx10104_checks();
        assert!(s.all_passed(), "UNX-F10104 red: {}", s.red_items_text());
    }
}
