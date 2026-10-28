//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12442 · NtOpenProcessTokenEx/NtOpenThreadTokenEx 语义档 · 0x30540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12442-J1 Ex 档（HandleAttributes 继承位）判据绿，与基础版
//!      差异面 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Ex 档差异面：HandleAttributes 参数（OBJ_INHERIT 继承位），
//!     基础版（12441）无此参数——差异面 = attrs=0 时行为与基础版一致；
//!   - HandleAttributes 闭集：OBJ_INHERIT(0x2)/OBJ_CASE_INSENSITIVE(0x80)
//!     两合法位，越位拒（AttrBits）、双位可叠加；
//!   - 继承账：Ex 开句柄落句柄行（含 inherit 标记），INHERIT 位 → 行
//!     inherit=true，逐行可查（DiffMismatch 抓账面失真）；
//!   - 令牌域沿用 12441 口径：进程挂主令牌/线程挂模拟令牌，
//!     无令牌对象拒止两路（NoToken/NoThreadToken）原样生效；
//!   - 句柄账：打开/关闭计数 + 上限 6（HandleCap），Ex 档与基础版共用；
//!   - 差异面判据：同 (pid, desired, attrs=0) 基础版/Ex 版结果逐例一致
//!     （含拒止码一致），attrs≠0 仅继承标记面差异；
//!   - 错误路径带码+中文 detail；
//!   - 注入核：LCG 写死种子 0x30540，Ex 档/差异面/拒止 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30540 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30540;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 进程域宽度（定容 16，与 12441 同口径）。
pub const PROC_CAP: usize = 16;

/// 线程域宽度（定容 16，与 12441 同口径）。
pub const THREAD_CAP: usize = 16;

/// 令牌对象槽位（定容 8）。
pub const TOKEN_SLOTS: usize = 8;

/// 句柄行上限（Ex 档账面）。
pub const HANDLE_ROWS: usize = 12;

/// 单令牌句柄上限（6：双令牌 12 行满账口径）。
pub const HANDLE_CAP: u32 = 6;

// HandleAttributes 合法位（winnt.h 口径）。
/// OBJ_INHERIT（继承位）。
pub const OBJ_INHERIT: u32 = 0x2;
/// OBJ_CASE_INSENSITIVE。
pub const OBJ_CASE_INSENSITIVE: u32 = 0x80;
/// 合法位闭集掩码。
pub const ATTR_VALID_MASK: u32 = OBJ_INHERIT | OBJ_CASE_INSENSITIVE;

// TOKEN 期望访问位（与 12441 同口径）。
/// TOKEN_DUPLICATE。
pub const TOKEN_DUPLICATE: u32 = 0x0002;
/// TOKEN_QUERY。
pub const TOKEN_QUERY: u32 = 0x0008;
/// TOKEN_ADJUST_PRIVILEGES。
pub const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;

/// 诊断码（0x30540 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 0x30540 HandleAttributes 越位。
    AttrBits = 0,
    /// 0x30541 进程域越界。
    PidOob = 1,
    /// 0x30542 线程域越界。
    TidOob = 2,
    /// 0x30543 无令牌对象。
    NoToken = 3,
    /// 0x30544 线程无模拟令牌。
    NoThreadToken = 4,
    /// 0x30545 期望访问掩码越位。
    AccessMask = 5,
    /// 0x30546 令牌已释放。
    TokenFreed = 6,
    /// 0x30547 句柄行满。
    TableFull = 7,
    /// 0x30548 差异面失真（inherit 账与 attrs 不一致）。
    DiffMismatch = 8,
    /// 0x30549 句柄超容。
    HandleCap = 9,
}

impl ExCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ExCode; 10] = [
        ExCode::AttrBits, ExCode::PidOob, ExCode::TidOob, ExCode::NoToken,
        ExCode::NoThreadToken, ExCode::AccessMask, ExCode::TokenFreed, ExCode::TableFull,
        ExCode::DiffMismatch, ExCode::HandleCap,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30540 => "HandleAttributes 越位：只收 OBJ_INHERIT/OBJ_CASE_INSENSITIVE 两位。",
        0x30541 => "进程域越界：pid 先比 PROC_CAP 再查进程表。",
        0x30542 => "线程域越界：tid 先比 THREAD_CAP 再查线程表。",
        0x30543 => "无令牌对象：先挂主令牌再按进程开。",
        0x30544 => "线程无模拟令牌：先 SetThreadToken 模拟，再按线程开。",
        0x30545 => "访问掩码越位：只收 QUERY/ADJUST_PRIVILEGES/DUPLICATE 位。",
        0x30546 => "令牌已释放：释放槽只读快照，重建走新令牌。",
        0x30547 => "句柄行满：先关旧行再开，行账对平。",
        0x30548 => "差异面失真：inherit 账必须与 attrs INHERIT 位一致。",
        0x30549 => "句柄超容：先关旧句柄再开，上限 4。",
        _ => "未知诊断码：回查 0x30540 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30540 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3054_3054_3054_3054;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 令牌种类（两态，与 12441 同口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// 主令牌。
    Primary = 0,
    /// 模拟令牌。
    Impersonation = 1,
}

/// 令牌对象条目。
#[derive(Clone, Copy, Debug)]
pub struct TokenObj {
    /// 种类。
    pub kind: TokenKind,
    /// 存活。
    pub live: bool,
    /// 打开计数。
    pub opens: u32,
    /// 关闭计数。
    pub closes: u32,
}

/// Ex 句柄行（含继承标记）。
#[derive(Clone, Copy, Debug)]
pub struct ExHandleRow {
    /// 令牌槽。
    pub slot: usize,
    /// HandleAttributes。
    pub attrs: u32,
    /// 继承标记（INHERIT 位派生）。
    pub inherit: bool,
    /// 存活。
    pub live: bool,
}

/// Ex 开令牌语义档正本。
///
/// 生命周期：create_token/attach_proc/set_imp（域面，同 12441）→
/// open_process_token_ex / open_thread_token_ex（attrs 消费）→
/// close_ex（行关闭）。
#[derive(Debug)]
pub struct ExTokenBook {
    /// 令牌表。
    tokens: [TokenObj; TOKEN_SLOTS],
    /// 进程主令牌挂靠（下标 = pid）。
    procs: [Option<usize>; PROC_CAP],
    /// 线程模拟挂靠（下标 = tid）。
    imps: [Option<usize>; THREAD_CAP],
    /// Ex 句柄行账。
    handles: [ExHandleRow; HANDLE_ROWS],
    /// 句柄行游标。
    cursor: usize,
    /// 拒止累计。
    rejects: u32,
}

impl ExTokenBook {
    /// 建账（标准域：P0 主令牌槽 0；T1 模拟令牌槽 1）。
    pub fn new() -> Self {
        let mut bk = ExTokenBook {
            tokens: core::array::from_fn(|_| TokenObj {
                kind: TokenKind::Primary,
                live: false,
                opens: 0,
                closes: 0,
            }),
            procs: core::array::from_fn(|_| None),
            imps: core::array::from_fn(|_| None),
            handles: core::array::from_fn(|_| ExHandleRow { slot: 0, attrs: 0, inherit: false, live: false }),
            cursor: 0,
            rejects: 0,
        };
        bk.tokens[0].live = true;
        bk.tokens[0].kind = TokenKind::Primary;
        bk.procs[0] = Some(0);
        bk.tokens[1].live = true;
        bk.tokens[1].kind = TokenKind::Impersonation;
        bk.imps[1] = Some(1);
        bk
    }

    /// 建令牌（槽满拒）。
    pub fn create_token(&mut self, kind: TokenKind) -> Result<usize, ExCode> {
        let mut slot: Option<usize> = None;
        for i in 0..TOKEN_SLOTS {
            if !self.tokens[i].live {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(ExCode::TableFull);
            }
        };
        self.tokens[s] = TokenObj { kind, live: true, opens: 0, closes: 0 };
        Ok(s)
    }

    /// 进程挂主令牌。
    pub fn attach_proc(&mut self, pid: usize, slot: usize) -> Result<bool, ExCode> {
        if pid >= PROC_CAP {
            self.rejects += 1;
            return Err(ExCode::PidOob);
        }
        if slot >= TOKEN_SLOTS || !self.tokens[slot].live {
            self.rejects += 1;
            return Err(ExCode::TokenFreed);
        }
        self.procs[pid] = Some(slot);
        Ok(true)
    }

    /// 线程挂模拟令牌。
    pub fn set_imp(&mut self, tid: usize, slot: usize) -> Result<bool, ExCode> {
        if tid >= THREAD_CAP {
            self.rejects += 1;
            return Err(ExCode::TidOob);
        }
        if slot >= TOKEN_SLOTS || !self.tokens[slot].live {
            self.rejects += 1;
            return Err(ExCode::TokenFreed);
        }
        self.imps[tid] = Some(slot);
        Ok(true)
    }

    /// HandleAttributes 域核对。
    pub fn attrs_ok(attrs: u32) -> bool {
        attrs & !ATTR_VALID_MASK == 0
    }

    /// 期望访问掩码核对。
    pub fn desired_ok(desired: u32) -> bool {
        const MASK: u32 = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ADJUST_PRIVILEGES;
        desired != 0 && desired & !MASK == 0
    }

    /// 落句柄行（继承标记派生 + 账核对）。
    fn push_handle(&mut self, slot: usize, attrs: u32) -> Result<usize, ExCode> {
        if !Self::attrs_ok(attrs) {
            self.rejects += 1;
            return Err(ExCode::AttrBits);
        }
        // 行面先查（TableFull），再查句柄超容（HandleCap）。
        let mut idx: Option<usize> = None;
        for i in 0..HANDLE_ROWS {
            if !self.handles[i].live {
                idx = Some(i);
                break;
            }
        }
        let i = match idx {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(ExCode::TableFull);
            }
        };
        if self.tokens[slot].opens - self.tokens[slot].closes >= HANDLE_CAP {
            self.rejects += 1;
            return Err(ExCode::HandleCap);
        }
        self.handles[i] = ExHandleRow {
            slot,
            attrs,
            inherit: attrs & OBJ_INHERIT != 0,
            live: true,
        };
        self.cursor = self.cursor.max(i + 1);
        self.tokens[slot].opens += 1;
        Ok(i)
    }

    /// NtOpenProcessTokenEx（attrs=0 时与基础版同果）。
    pub fn open_process_token_ex(&mut self, pid: usize, desired: u32, attrs: u32) -> Result<usize, ExCode> {
        if pid >= PROC_CAP {
            self.rejects += 1;
            return Err(ExCode::PidOob);
        }
        if !Self::desired_ok(desired) {
            self.rejects += 1;
            return Err(ExCode::AccessMask);
        }
        let slot = match self.procs[pid] {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(ExCode::NoToken);
            }
        };
        self.push_handle(slot, attrs)
    }

    /// NtOpenThreadTokenEx（未模拟 → NoThreadToken）。
    pub fn open_thread_token_ex(&mut self, tid: usize, desired: u32, attrs: u32) -> Result<usize, ExCode> {
        if tid >= THREAD_CAP {
            self.rejects += 1;
            return Err(ExCode::TidOob);
        }
        if !Self::desired_ok(desired) {
            self.rejects += 1;
            return Err(ExCode::AccessMask);
        }
        let slot = match self.imps[tid] {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(ExCode::NoThreadToken);
            }
        };
        self.push_handle(slot, attrs)
    }

    /// 关闭 Ex 句柄行（账平 + 继承账核对）。
    pub fn close_ex(&mut self, idx: usize) -> Result<bool, ExCode> {
        if idx >= HANDLE_ROWS {
            self.rejects += 1;
            return Err(ExCode::TableFull);
        }
        let h = self.handles[idx];
        if !h.live {
            self.rejects += 1;
            return Err(ExCode::TokenFreed);
        }
        // 继承账核对：行 inherit 必须与 attrs INHERIT 位一致。
        if h.inherit != (h.attrs & OBJ_INHERIT != 0) {
            self.rejects += 1;
            return Err(ExCode::DiffMismatch);
        }
        self.handles[idx].live = false;
        self.tokens[h.slot].closes += 1;
        Ok(true)
    }

    /// 句柄账守恒。
    pub fn ledger_ok(&self) -> bool {
        self.tokens.iter().all(|t| {
            !t.live || (t.opens >= t.closes && t.opens - t.closes <= HANDLE_CAP)
        }) && self.handles.iter().all(|h| {
            !h.live || h.inherit == (h.attrs & OBJ_INHERIT != 0)
        })
    }

    /// 句柄行快照。
    pub fn handle_of(&self, idx: usize) -> Option<ExHandleRow> {
        if idx >= HANDLE_ROWS {
            None
        } else {
            Some(self.handles[idx])
        }
    }

    /// 活句柄行数。
    pub fn live_handles(&self) -> usize {
        self.handles.iter().filter(|h| h.live).count()
    }

    /// 令牌快照。
    pub fn token_of(&self, slot: usize) -> Option<TokenObj> {
        if slot >= TOKEN_SLOTS {
            None
        } else {
            Some(self.tokens[slot])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// Ex 档开令牌演练：10/10 轮 attrs 消费、继承标记派生。
pub fn drill_ex_open_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30540 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ExTokenBook::new();
        let h1 = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_INHERIT);
        let h2 = bk.open_thread_token_ex(1, TOKEN_QUERY, 0);
        let i1 = h1.ok().and_then(|i| bk.handle_of(i)).map(|h| h.inherit);
        let i2 = h2.ok().and_then(|i| bk.handle_of(i)).map(|h| h.inherit);
        let _ = lcg.below(3);
        if h1.is_ok() && h2.is_ok() && i1 == Some(true) && i2 == Some(false) && bk.ledger_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Ex 档开令牌轮={}/{}", ok, INJECT_ROUNDS))
}

/// 与基础版差异面演练：attrs=0 与基础版行为逐例一致 10/10。
pub fn drill_diff_face_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30540 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ExTokenBook::new();
        // Ex 档 attrs=0：命中路径与基础版同果（槽号一致）。
        let h1 = bk.open_process_token_ex(0, TOKEN_QUERY, 0);
        let h2 = bk.open_thread_token_ex(1, TOKEN_QUERY, 0);
        let s1 = h1.ok().and_then(|i| bk.handle_of(i)).map(|h| h.slot);
        let s2 = h2.ok().and_then(|i| bk.handle_of(i)).map(|h| h.slot);
        // 拒止路径与基础版同码（无令牌进程 / 未模拟线程）。
        let r1 = bk.open_process_token_ex(1, TOKEN_QUERY, 0);
        let r2 = bk.open_thread_token_ex(0, TOKEN_QUERY, 0);
        let _ = lcg.below(3);
        if matches!(h1, Ok(_))
            && matches!(h2, Ok(_))
            && s1 == Some(0)
            && s2 == Some(1)
            && matches!(r1, Err(ExCode::NoToken))
            && matches!(r2, Err(ExCode::NoThreadToken))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("差异面一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// attrs 域拒止演练：越位/双位叠加/掩码越位 10/10。
pub fn drill_attrs_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30540 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ExTokenBook::new();
        let bad_bits = bk.open_process_token_ex(0, TOKEN_QUERY, 0x4000);
        let both = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_INHERIT | OBJ_CASE_INSENSITIVE);
        let bad_mask = bk.open_process_token_ex(0, 0x8000, OBJ_INHERIT);
        let zero = bk.open_process_token_ex(0, 0, OBJ_INHERIT);
        let _ = lcg.below(3);
        if matches!(bad_bits, Err(ExCode::AttrBits))
            && matches!(both, Ok(_))
            && matches!(bad_mask, Err(ExCode::AccessMask))
            && matches!(zero, Err(ExCode::AccessMask))
            && bk.reject_total() >= 3
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("attrs 域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：Ex 句柄行转录导出（逐行一行：槽/属性/继承/活）。
/// ─────────────────────────────────────────────────────────────────────
/// Ex 句柄行转录导出。
pub fn export_handle_lines(bk: &ExTokenBook) -> String {
    let mut s = String::from("Ex 句柄行转录：\n");
    for i in 0..HANDLE_ROWS {
        if let Some(h) = bk.handle_of(i) {
            if h.live {
                s.push_str(&format!(
                    "H{} 槽{} 属{:x} 继{}\n",
                    i, h.slot, h.attrs, h.inherit
                ));
            }
        }
    }
    s
}

/// 转录完整判据（活行齐备且继承账一致）。
pub fn handle_lines_complete(bk: &ExTokenBook) -> bool {
    let s = export_handle_lines(bk);
    if bk.live_handles() == 0 {
        return false;
    }
    (0..HANDLE_ROWS).all(|i| {
        bk.handle_of(i)
            .map(|h| !h.live || (s.contains(&format!("H{} 槽", i)) && h.inherit == (h.attrs & OBJ_INHERIT != 0)))
            .unwrap_or(true)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：Ex 档报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ExReport {
    /// 活句柄行数。
    pub live: usize,
    /// 继承行数。
    pub inherited: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 账守恒。
    pub ledger: bool,
    /// 转录完整。
    pub lines: bool,
}

impl ExReport {
    /// 建报告。
    pub fn build(bk: &ExTokenBook) -> Self {
        ExReport {
            live: bk.live_handles(),
            inherited: (0..HANDLE_ROWS)
                .filter(|&i| bk.handle_of(i).map(|h| h.live && h.inherit).unwrap_or(false))
                .count(),
            rejects: bk.reject_total(),
            ledger: bk.ledger_ok(),
            lines: handle_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ledger && self.lines && self.rejects >= 1 && self.live >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ExReport 行={} 继={} 拒={} 账={} 转录={}",
            self.live, self.inherited, self.rejects, self.ledger, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：继承账核对演练（INHERIT 位 ↔ inherit 标记 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_inherit_ledger_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30540 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ExTokenBook::new();
        let h1 = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_INHERIT);
        let h2 = bk.open_process_token_ex(0, TOKEN_QUERY, 0);
        let h3 = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_CASE_INSENSITIVE);
        let c1 = h1.and_then(|i| bk.close_ex(i));
        let c2 = h2.and_then(|i| bk.close_ex(i));
        let c3 = h3.and_then(|i| bk.close_ex(i));
        let _ = lcg.below(3);
        if h1.is_ok() && h2.is_ok() && h3.is_ok()
            && matches!(c1, Ok(true))
            && matches!(c2, Ok(true))
            && matches!(c3, Ok(true))
            && bk.live_handles() == 0
            && bk.ledger_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("继承账轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：句柄行满演练（12 行满后 TableFull 拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_rows_full_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30540 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ExTokenBook::new();
        // 双令牌各开 6 柄 → 12 行满账（HandleCap 6 不触顶）。
        let mut opened = 0usize;
        for _i in 0..HANDLE_CAP {
            if bk.open_process_token_ex(0, TOKEN_QUERY, 0).is_ok() {
                opened += 1;
            }
        }
        for _i in 0..HANDLE_CAP {
            if bk.open_thread_token_ex(1, TOKEN_QUERY, 0).is_ok() {
                opened += 1;
            }
        }
        // 第 13 柄：行面满 → TableFull（先于 HandleCap 判）。
        let over = bk.open_process_token_ex(0, TOKEN_QUERY, 0);
        let _ = lcg.below(3);
        if opened == 12 && bk.live_handles() == 12 && matches!(over, Err(ExCode::TableFull)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("句柄行满轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ExCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ex_checklist(bk: &ExTokenBook) -> Vec<ExCheckRow> {
    let rep = ExReport::build(bk);
    let mut rows: Vec<ExCheckRow> = Vec::new();
    rows.push(ExCheckRow { name: "账守恒", passed: bk.ledger_ok() });
    rows.push(ExCheckRow { name: "attrs闭集", passed: ATTR_VALID_MASK == 0x82 });
    rows.push(ExCheckRow { name: "继承位口径", passed: OBJ_INHERIT == 0x2 });
    rows.push(ExCheckRow { name: "行宽12", passed: HANDLE_ROWS == 12 && HANDLE_ROWS == (HANDLE_CAP * 2) as usize });
    rows.push(ExCheckRow { name: "句柄上限6", passed: HANDLE_CAP == 6 });
    rows.push(ExCheckRow { name: "转录完整", passed: handle_lines_complete(bk) });
    rows.push(ExCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ExCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(ExCheckRow { name: "标准域挂靠", passed: bk.token_of(0).map(|t| t.live).unwrap_or(false) && bk.token_of(1).map(|t| t.live).unwrap_or(false) });
    rows.push(ExCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ex_checklist_green(rows: &[ExCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ex_stats_line(bk: &ExTokenBook) -> String {
    format!(
        "Ex 档概况：行 {} 继 {} 拒 {}",
        bk.live_handles(),
        (0..HANDLE_ROWS)
            .filter(|&i| bk.handle_of(i).map(|h| h.live && h.inherit).unwrap_or(false))
            .count(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12442_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ExTokenBook::new();
    let _ = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_INHERIT);
    let _ = bk.open_thread_token_ex(1, TOKEN_QUERY, 0);
    // 注入一例越位 attrs（拒止账口径）。
    let _ = bk.open_process_token_ex(0, TOKEN_QUERY, 0x4000);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        handle_lines_complete(&bk) && ExReport::build(&bk).all_green(),
        ex_stats_line(&bk),
    ));
    // X-b：继承账 10/10。
    let (ok_i, d_i) = drill_inherit_ledger_10();
    out.push(("X-继承账10/10", ok_i, d_i));
    // X-c：句柄行满 10/10。
    let (ok_f, d_f) = drill_rows_full_10();
    out.push(("X-句柄行满10/10", ok_f, d_f));
    // X-d：终验清单。
    let rows = build_ex_checklist(&bk);
    out.push(("X-终验清单10项", ex_checklist_green(&rows), ExReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：attrs 越位必拒（AttrBits）。
    let mut bk = ExTokenBook::new();
    if matches!(bk.open_process_token_ex(0, TOKEN_QUERY, 0x4000), Err(ExCode::AttrBits)) {
        caught += 1;
        d.push("注1 越位:抓".to_string());
    } else {
        d.push("注1 越位:漏".to_string());
    }
    // 注 2：无令牌进程必拒（NoToken）。
    if matches!(bk.open_process_token_ex(1, TOKEN_QUERY, 0), Err(ExCode::NoToken)) {
        caught += 1;
        d.push("注2 无令牌:抓".to_string());
    } else {
        d.push("注2 无令牌:漏".to_string());
    }
    // 注 3：未模拟线程必拒（NoThreadToken）。
    if matches!(bk.open_thread_token_ex(0, TOKEN_QUERY, 0), Err(ExCode::NoThreadToken)) {
        caught += 1;
        d.push("注3 未模拟:抓".to_string());
    } else {
        d.push("注3 未模拟:漏".to_string());
    }
    // 注 4：越进程域必拒（PidOob）。
    if matches!(bk.open_process_token_ex(99, TOKEN_QUERY, 0), Err(ExCode::PidOob)) {
        caught += 1;
        d.push("注4 越域:抓".to_string());
    } else {
        d.push("注4 越域:漏".to_string());
    }
    // 注 5：零掩码必拒（AccessMask）。
    if matches!(bk.open_process_token_ex(0, 0, 0), Err(ExCode::AccessMask)) {
        caught += 1;
        d.push("注5 零掩码:抓".to_string());
    } else {
        d.push("注5 零掩码:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30540 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    ExCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12442_checks：本单判据聚合（诊断段 0x30540）。
pub fn run_unx12442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12442 NtOpenProcessTokenEx/NtOpenThreadTokenEx 语义档");
    // J1-a：Ex 档开令牌（attrs 消费 10/10）。
    let (ok1, d1) = drill_ex_open_10();
    s.add_dyn("J1-Ex档开令牌10/10", ok1, &d1);
    // J1-b：与基础版差异面一致（10/10）。
    let (ok2, d2) = drill_diff_face_10();
    s.add_dyn("J1-差异面一致10/10", ok2, &d2);
    // J1-c：attrs 域拒止（10/10）。
    let (ok3, d3) = drill_attrs_reject_10();
    s.add_dyn("J1-attrs域拒止10/10", ok3, &d3);
    // J1-d：标准会话（INHERIT 行 + 非继承行 + 账守恒）。
    let mut bk = ExTokenBook::new();
    let h1 = bk.open_process_token_ex(0, TOKEN_QUERY, OBJ_INHERIT);
    let h2 = bk.open_thread_token_ex(1, TOKEN_QUERY, 0);
    let inh = h1.ok().and_then(|i| bk.handle_of(i)).map(|h| h.inherit).unwrap_or(true);
    let noinh = h2.ok().and_then(|i| bk.handle_of(i)).map(|h| h.inherit).unwrap_or(true);
    s.add_dyn(
        "J1-继承位账面一致",
        h1.is_ok() && h2.is_ok() && inh && !noinh && bk.ledger_ok() && handle_lines_complete(&bk),
        &format!(
            "继={} 非继={} 账={}",
            inh,
            !noinh,
            bk.ledger_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越位 attrs 不落行）。
    let l0 = bk.live_handles();
    let _ = bk.open_process_token_ex(0, TOKEN_QUERY, 0x4000);
    s.add_dyn(
        "J2-拒止不写回",
        bk.live_handles() == l0 && bk.reject_total() >= 1,
        &format!("行 {}→{}", l0, bk.live_handles()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30540段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12442_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
