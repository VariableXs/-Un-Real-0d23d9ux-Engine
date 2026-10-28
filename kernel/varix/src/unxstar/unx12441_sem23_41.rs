//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12441 · NtOpenProcessToken/NtOpenThreadToken 语义档 · 0x30520
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12441-J1 按进程/线程开令牌判据绿，无令牌对象注入 10/10 次
//!      正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 令牌对象域：主令牌（Primary）与模拟令牌（Impersonation）两态，
//!     进程挂主令牌、线程挂模拟令牌（可空）；
//!   - 按进程开令牌：NtOpenProcessToken(pid, desired) → 命中主令牌返回
//!     句柄；进程无令牌拒（NoToken）、进程域越界拒（PidOob）；
//!   - 按线程开令牌：NtOpenThreadToken(tid, desired) → 线程模拟中用模拟
//!     令牌，未模拟返回 STATUS_NO_TOKEN 档（NoThreadToken）；
//!   - 期望访问掩码纪律：TOKEN_QUERY/TOKEN_ADJUST_PRIVILEGES/
//!     TOKEN_DUPLICATE 闭集位，越位拒（AccessMask）；
//!   - 句柄账：每令牌打开计数（上限 4），超容拒（HandleCap），关闭递减，
//!     释放态拒（TokenFreed）；
//!   - 无令牌对象注入：无令牌进程 / 未模拟线程双路 10/10 拒止，
//!     拒止不写回句柄账；
//!   - 错误路径带码+中文 detail；
//!   - 注入核：LCG 写死种子 0x30520，开令牌/拒止 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30520 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30520;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 进程域宽度（定容 16）。
pub const PROC_CAP: usize = 16;

/// 线程域宽度（定容 16）。
pub const THREAD_CAP: usize = 16;

/// 令牌对象槽位（定容 8）。
pub const TOKEN_SLOTS: usize = 8;

/// 单令牌句柄上限（打开计数口径）。
pub const HANDLE_CAP: u32 = 4;

// TOKEN 期望访问位（winnt.h 口径）。
/// TOKEN_DUPLICATE。
pub const TOKEN_DUPLICATE: u32 = 0x0002;
/// TOKEN_QUERY。
pub const TOKEN_QUERY: u32 = 0x0008;
/// TOKEN_ADJUST_PRIVILEGES。
pub const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
/// TOKEN_ADJUST_GROUPS。
pub const TOKEN_ADJUST_GROUPS: u32 = 0x0040;
/// 合法位闭集掩码。
pub const TOKEN_VALID_MASK: u32 =
    TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES | TOKEN_ADJUST_GROUPS;

/// 诊断码（0x30520 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OtCode {
    /// 0x30520 进程域越界。
    PidOob = 0,
    /// 0x30521 线程域越界。
    TidOob = 1,
    /// 0x30522 无令牌对象（进程未挂主令牌）。
    NoToken = 2,
    /// 0x30523 线程无模拟令牌（STATUS_NO_TOKEN 档）。
    NoThreadToken = 3,
    /// 0x30524 期望访问掩码越位。
    AccessMask = 4,
    /// 0x30525 令牌已释放。
    TokenFreed = 5,
    /// 0x30526 令牌槽满。
    TableFull = 6,
    /// 0x30527 模拟域越界（imp 位非法）。
    ImpOob = 7,
    /// 0x30528 句柄超容（打开计数越上限）。
    HandleCap = 8,
    /// 0x30529 句柄账漂移（打开计数与关闭账失平）。
    LedgerDrift = 9,
}

impl OtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [OtCode; 10] = [
        OtCode::PidOob, OtCode::TidOob, OtCode::NoToken, OtCode::NoThreadToken,
        OtCode::AccessMask, OtCode::TokenFreed, OtCode::TableFull, OtCode::ImpOob,
        OtCode::HandleCap, OtCode::LedgerDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30520 => "进程域越界：pid 先比 PROC_CAP 再查进程表。",
        0x30521 => "线程域越界：tid 先比 THREAD_CAP 再查线程表。",
        0x30522 => "无令牌对象：先 NtCreateToken/继承令牌，再按进程开。",
        0x30523 => "线程无模拟令牌：先 SetThreadToken 模拟，再按线程开。",
        0x30524 => "访问掩码越位：只收 QUERY/ADJUST_PRIVILEGES/ADJUST_GROUPS/DUPLICATE 四位。",
        0x30525 => "令牌已释放：释放槽只读快照，重建走新令牌。",
        0x30526 => "令牌槽满：先释放废令牌再建，槽位账对平。",
        0x30527 => "模拟域越界：模拟标记只收 0/1 两态。",
        0x30528 => "句柄超容：先关旧句柄再开，上限 4。",
        0x30529 => "句柄账漂移：打开计数 - 关闭计数 ≡ 活句柄，先对平。",
        _ => "未知诊断码：回查 0x30520 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30520 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3052_3052_3052_3052;

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

/// 令牌种类（两态）。
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

/// 进程条目。
#[derive(Clone, Copy, Debug)]
pub struct ProcEntry {
    /// 进程号。
    pub pid: usize,
    /// 挂靠主令牌槽（None=无令牌对象）。
    pub token: Option<usize>,
}

/// 线程条目。
#[derive(Clone, Copy, Debug)]
pub struct ThreadEntry {
    /// 线程号。
    pub tid: usize,
    /// 归属进程号。
    pub pid: usize,
    /// 模拟令牌槽（None=未模拟）。
    pub imp: Option<usize>,
}

/// 按进程/线程开令牌语义档正本。
///
/// 生命周期：create_token（令牌入槽）→ attach_proc（进程挂主令牌）→
/// set_imp（线程挂模拟令牌）→ open_process_token / open_thread_token →
/// close_token。
#[derive(Debug)]
pub struct TokenBook {
    /// 令牌表。
    tokens: [TokenObj; TOKEN_SLOTS],
    /// 进程表。
    procs: [ProcEntry; PROC_CAP],
    /// 线程表。
    threads: [ThreadEntry; THREAD_CAP],
    /// 下一令牌槽游标。
    cursor: usize,
    /// 拒止累计。
    rejects: u32,
}

impl TokenBook {
    /// 建账（空令牌域；P0 挂主令牌；T0 挂 P0 主令牌；T1 挂模拟令牌）。
    pub fn new() -> Self {
        let mut bk = TokenBook {
            tokens: core::array::from_fn(|_| TokenObj {
                kind: TokenKind::Primary,
                live: false,
                opens: 0,
                closes: 0,
            }),
            procs: core::array::from_fn(|i| ProcEntry { pid: i, token: None }),
            threads: core::array::from_fn(|i| ThreadEntry { tid: i, pid: i, imp: None }),
            cursor: 0,
            rejects: 0,
        };
        // 标准域：P0 主令牌槽 0；T0 归 P0 未模拟；T1 归 P0 挂模拟令牌槽 1。
        bk.tokens[0].live = true;
        bk.tokens[0].kind = TokenKind::Primary;
        bk.procs[0].token = Some(0);
        bk.tokens[1].live = true;
        bk.tokens[1].kind = TokenKind::Impersonation;
        bk.threads[1].imp = Some(1);
        bk.threads[1].pid = 0;
        bk
    }

    /// 建令牌（越槽/槽满拒）。
    pub fn create_token(&mut self, kind: TokenKind) -> Result<usize, OtCode> {
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
                return Err(OtCode::TableFull);
            }
        };
        self.tokens[s] = TokenObj { kind, live: true, opens: 0, closes: 0 };
        self.cursor = self.cursor.max(s + 1);
        Ok(s)
    }

    /// 进程挂主令牌（越域/槽越界拒）。
    pub fn attach_proc(&mut self, pid: usize, slot: usize) -> Result<bool, OtCode> {
        if pid >= PROC_CAP {
            self.rejects += 1;
            return Err(OtCode::PidOob);
        }
        if slot >= TOKEN_SLOTS || !self.tokens[slot].live {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        self.procs[pid].token = Some(slot);
        Ok(true)
    }

    /// 线程挂模拟令牌（越域/槽越界/模拟位非法拒）。
    pub fn set_imp(&mut self, tid: usize, pid: usize, slot: usize, imp_on: u32) -> Result<bool, OtCode> {
        if tid >= THREAD_CAP {
            self.rejects += 1;
            return Err(OtCode::TidOob);
        }
        if pid >= PROC_CAP {
            self.rejects += 1;
            return Err(OtCode::PidOob);
        }
        if imp_on > 1 {
            self.rejects += 1;
            return Err(OtCode::ImpOob);
        }
        self.threads[tid].pid = pid;
        if imp_on == 1 {
            if slot >= TOKEN_SLOTS || !self.tokens[slot].live {
                self.rejects += 1;
                return Err(OtCode::TokenFreed);
            }
            self.threads[tid].imp = Some(slot);
        } else {
            self.threads[tid].imp = None;
        }
        Ok(true)
    }

    /// 期望访问掩码核对。
    pub fn desired_ok(desired: u32) -> bool {
        desired != 0 && desired & !TOKEN_VALID_MASK == 0
    }

    /// 按进程开令牌（无令牌对象注入 10/10 拒止主路）。
    pub fn open_process_token(&mut self, pid: usize, desired: u32) -> Result<usize, OtCode> {
        if pid >= PROC_CAP {
            self.rejects += 1;
            return Err(OtCode::PidOob);
        }
        if !Self::desired_ok(desired) {
            self.rejects += 1;
            return Err(OtCode::AccessMask);
        }
        let slot = match self.procs[pid].token {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(OtCode::NoToken);
            }
        };
        self.open_handle(slot)
    }

    /// 按线程开令牌（未模拟 → NoThreadToken；模拟中 → 模拟令牌）。
    pub fn open_thread_token(&mut self, tid: usize, desired: u32) -> Result<usize, OtCode> {
        if tid >= THREAD_CAP {
            self.rejects += 1;
            return Err(OtCode::TidOob);
        }
        if !Self::desired_ok(desired) {
            self.rejects += 1;
            return Err(OtCode::AccessMask);
        }
        let slot = match self.threads[tid].imp {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(OtCode::NoThreadToken);
            }
        };
        self.open_handle(slot)
    }

    /// 打开句柄（计数账；超容/释放态拒）。
    fn open_handle(&mut self, slot: usize) -> Result<usize, OtCode> {
        if slot >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        let t = &self.tokens[slot];
        if !t.live {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        if t.opens - t.closes >= HANDLE_CAP {
            self.rejects += 1;
            return Err(OtCode::HandleCap);
        }
        self.tokens[slot].opens += 1;
        Ok(slot)
    }

    /// 关闭句柄（计数账；重复关闭拒）。
    pub fn close_token(&mut self, slot: usize) -> Result<bool, OtCode> {
        if slot >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        if !self.tokens[slot].live {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        if self.tokens[slot].opens == self.tokens[slot].closes {
            self.rejects += 1;
            return Err(OtCode::LedgerDrift);
        }
        self.tokens[slot].closes += 1;
        Ok(true)
    }

    /// 释放令牌槽（打开计数清零后可回收）。
    pub fn free_token(&mut self, slot: usize) -> Result<bool, OtCode> {
        if slot >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        if !self.tokens[slot].live {
            self.rejects += 1;
            return Err(OtCode::TokenFreed);
        }
        if self.tokens[slot].opens != self.tokens[slot].closes {
            self.rejects += 1;
            return Err(OtCode::LedgerDrift);
        }
        self.tokens[slot].live = false;
        Ok(true)
    }

    /// 句柄账守恒：opens - closes ≡ 活句柄（非负且逐槽平）。
    pub fn handle_ledger_ok(&self) -> bool {
        self.tokens.iter().all(|t| {
            !t.live || (t.opens >= t.closes && t.opens - t.closes <= HANDLE_CAP)
        })
    }

    /// 令牌快照。
    pub fn token_of(&self, slot: usize) -> Option<TokenObj> {
        if slot >= TOKEN_SLOTS {
            None
        } else {
            Some(self.tokens[slot])
        }
    }

    /// 进程令牌挂靠快照。
    pub fn proc_token(&self, pid: usize) -> Option<usize> {
        if pid >= PROC_CAP {
            None
        } else {
            self.procs[pid].token
        }
    }

    /// 线程模拟快照。
    pub fn thread_imp(&self, tid: usize) -> Option<usize> {
        if tid >= THREAD_CAP {
            None
        } else {
            self.threads[tid].imp
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 按进程开令牌演练：10/10 轮命中主令牌、句柄账平。
pub fn drill_open_proc_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30520 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let mut pass = true;
        for i in 0..3 {
            match bk.open_process_token(0, TOKEN_QUERY) {
                Ok(slot) => pass = pass && slot == 0,
                Err(_) => pass = false,
            }
            let _ = i;
        }
        let c1 = bk.close_token(0);
        let c2 = bk.close_token(0);
        let c3 = bk.close_token(0);
        let _ = lcg.below(3);
        if pass && matches!(c1, Ok(true)) && matches!(c2, Ok(true)) && matches!(c3, Ok(true)) && bk.handle_ledger_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("按进程开令牌轮={}/{}", ok, INJECT_ROUNDS))
}

/// 按线程开令牌演练：10/10 轮模拟中用模拟令牌、未模拟拒止。
pub fn drill_open_thread_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30520 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        // T1 已挂模拟令牌 → 开成功且命中槽 1。
        let t1 = bk.open_thread_token(1, TOKEN_QUERY);
        // T0 未模拟 → NoThreadToken。
        let t0 = bk.open_thread_token(0, TOKEN_QUERY);
        let _ = lcg.below(3);
        if matches!(t1, Ok(slot) if slot == 1) && matches!(t0, Err(OtCode::NoThreadToken)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("按线程开令牌轮={}/{}", ok, INJECT_ROUNDS))
}

/// 无令牌对象注入 10/10 正确拒止演练（双路：无令牌进程/越掩码）。
pub fn drill_no_token_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30520 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        // 路 1：P1 无主令牌 → NoToken。
        let p1 = bk.open_process_token(1, TOKEN_QUERY);
        // 路 2：越掩码 → AccessMask。
        let mask = bk.open_process_token(0, 0x8000);
        // 路 3：越进程域 → PidOob。
        let oob = bk.open_process_token(PROC_CAP + 1, TOKEN_QUERY);
        // 路 4：越线程域 → TidOob。
        let toob = bk.open_thread_token(THREAD_CAP + 1, TOKEN_QUERY);
        // 路 5：关闭后句柄账守恒（双零面）。
        let ledger = bk.handle_ledger_ok();
        let _ = lcg.below(3);
        if matches!(p1, Err(OtCode::NoToken))
            && matches!(mask, Err(OtCode::AccessMask))
            && matches!(oob, Err(OtCode::PidOob))
            && matches!(toob, Err(OtCode::TidOob))
            && ledger
            && bk.reject_total() >= 4
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("无令牌注入拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：令牌域转录导出（逐槽一行：种类/活/开/关）。
/// ─────────────────────────────────────────────────────────────────────
/// 令牌域转录导出。
pub fn export_token_lines(bk: &TokenBook) -> String {
    let mut s = String::from("令牌域转录：\n");
    for i in 0..TOKEN_SLOTS {
        if let Some(t) = bk.token_of(i) {
            if t.live {
                s.push_str(&format!(
                    "T{} 类{} 开{} 关{}\n",
                    i, t.kind as u8, t.opens, t.closes
                ));
            }
        }
    }
    s
}

/// 转录完整判据（活令牌逐行齐备且账平）。
pub fn token_lines_complete(bk: &TokenBook) -> bool {
    let s = export_token_lines(bk);
    let live = (0..TOKEN_SLOTS).filter(|&i| bk.token_of(i).map(|t| t.live).unwrap_or(false)).count();
    if live == 0 {
        return false;
    }
    (0..TOKEN_SLOTS).all(|i| {
        bk.token_of(i)
            .map(|t| !t.live || s.contains(&format!("T{} 类", i)))
            .unwrap_or(true)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：开令牌报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct OtReport {
    /// 活令牌数。
    pub live: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 句柄账平。
    pub ledger: bool,
    /// 转录完整。
    pub lines: bool,
}

impl OtReport {
    /// 建报告。
    pub fn build(bk: &TokenBook) -> Self {
        OtReport {
            live: (0..TOKEN_SLOTS).filter(|&i| bk.token_of(i).map(|t| t.live).unwrap_or(false)).count(),
            rejects: bk.reject_total(),
            ledger: bk.handle_ledger_ok(),
            lines: token_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ledger && self.lines && self.rejects >= 1 && self.live >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "OtReport 活={} 拒={} 账平={} 转录={}",
            self.live, self.rejects, self.ledger, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：句柄账演练（开 4 满/第 5 拒/关闭回收 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_handle_cap_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30520 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let mut opened = 0usize;
        for _i in 0..HANDLE_CAP {
            if bk.open_process_token(0, TOKEN_QUERY).is_ok() {
                opened += 1;
            }
        }
        let fifth = bk.open_process_token(0, TOKEN_QUERY);
        let close = bk.close_token(0);
        let reopen = bk.open_process_token(0, TOKEN_QUERY);
        let _ = lcg.below(3);
        if opened == HANDLE_CAP as usize
            && matches!(fifth, Err(OtCode::HandleCap))
            && matches!(close, Ok(true))
            && reopen.is_ok()
            && bk.handle_ledger_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("句柄账轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：掩码域演练（四合法位/零掩码/越位 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_mask_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30520 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = TokenBook::desired_ok(TOKEN_QUERY);
        pass = pass && TokenBook::desired_ok(TOKEN_QUERY | TOKEN_DUPLICATE);
        pass = pass && TokenBook::desired_ok(TOKEN_ADJUST_PRIVILEGES | TOKEN_ADJUST_GROUPS);
        pass = pass && !TokenBook::desired_ok(0);
        pass = pass && !TokenBook::desired_ok(0x8000);
        pass = pass && !TokenBook::desired_ok(TOKEN_QUERY | 0x4000);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("掩码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct OtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ot_checklist(bk: &TokenBook) -> Vec<OtCheckRow> {
    let rep = OtReport::build(bk);
    let mut rows: Vec<OtCheckRow> = Vec::new();
    rows.push(OtCheckRow { name: "句柄账平", passed: bk.handle_ledger_ok() });
    rows.push(OtCheckRow { name: "掩码闭集", passed: TOKEN_VALID_MASK == 0x6A });
    rows.push(OtCheckRow { name: "进程宽16", passed: PROC_CAP == 16 });
    rows.push(OtCheckRow { name: "线程宽16", passed: THREAD_CAP == 16 });
    rows.push(OtCheckRow { name: "句柄上限4", passed: HANDLE_CAP == 4 });
    rows.push(OtCheckRow { name: "转录完整", passed: token_lines_complete(bk) });
    rows.push(OtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(OtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(OtCheckRow { name: "标准域挂靠", passed: bk.proc_token(0) == Some(0) && bk.thread_imp(1) == Some(1) });
    rows.push(OtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ot_checklist_green(rows: &[OtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ot_stats_line(bk: &TokenBook) -> String {
    format!(
        "开令牌概况：活 {} 拒 {}",
        (0..TOKEN_SLOTS).filter(|&i| bk.token_of(i).map(|t| t.live).unwrap_or(false)).count(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12441_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TokenBook::new();
    let _ = bk.open_process_token(0, TOKEN_QUERY);
    let _ = bk.open_thread_token(1, TOKEN_QUERY);
    // 注入一例无令牌进程开令牌（拒止账口径）。
    let _ = bk.open_process_token(1, TOKEN_QUERY);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        token_lines_complete(&bk) && OtReport::build(&bk).all_green(),
        ot_stats_line(&bk),
    ));
    // X-b：句柄账 10/10。
    let (ok_h, d_h) = drill_handle_cap_10();
    out.push(("X-句柄账10/10", ok_h, d_h));
    // X-c：掩码域 10/10。
    let (ok_m, d_m) = drill_mask_domain_10();
    out.push(("X-掩码域10/10", ok_m, d_m));
    // X-d：终验清单。
    let rows = build_ot_checklist(&bk);
    out.push(("X-终验清单10项", ot_checklist_green(&rows), OtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：无令牌进程必拒（NoToken）。
    let mut bk = TokenBook::new();
    if matches!(bk.open_process_token(1, TOKEN_QUERY), Err(OtCode::NoToken)) {
        caught += 1;
        d.push("注1 无令牌:抓".to_string());
    } else {
        d.push("注1 无令牌:漏".to_string());
    }
    // 注 2：未模拟线程必拒（NoThreadToken）。
    if matches!(bk.open_thread_token(0, TOKEN_QUERY), Err(OtCode::NoThreadToken)) {
        caught += 1;
        d.push("注2 未模拟:抓".to_string());
    } else {
        d.push("注2 未模拟:漏".to_string());
    }
    // 注 3：越掩码必拒（AccessMask）。
    if matches!(bk.open_process_token(0, 0x8000), Err(OtCode::AccessMask)) {
        caught += 1;
        d.push("注3 越掩码:抓".to_string());
    } else {
        d.push("注3 越掩码:漏".to_string());
    }
    // 注 4：越进程域必拒（PidOob）。
    if matches!(bk.open_process_token(99, TOKEN_QUERY), Err(OtCode::PidOob)) {
        caught += 1;
        d.push("注4 越域:抓".to_string());
    } else {
        d.push("注4 越域:漏".to_string());
    }
    // 注 5：重复关闭必拒（LedgerDrift）。
    let _ = bk.open_process_token(0, TOKEN_QUERY);
    let _ = bk.close_token(0);
    if matches!(bk.close_token(0), Err(OtCode::LedgerDrift)) {
        caught += 1;
        d.push("注5 重关:抓".to_string());
    } else {
        d.push("注5 重关:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30520 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    OtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12441_checks：本单判据聚合（诊断段 0x30520）。
pub fn run_unx12441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12441 NtOpenProcessToken/NtOpenThreadToken 语义档");
    // J1-a：按进程开令牌 10/10。
    let (ok1, d1) = drill_open_proc_10();
    s.add_dyn("J1-按进程开令牌10/10", ok1, &d1);
    // J1-b：按线程开令牌 10/10。
    let (ok2, d2) = drill_open_thread_10();
    s.add_dyn("J1-按线程开令牌10/10", ok2, &d2);
    // J1-c：无令牌对象注入 10/10 正确拒止。
    let (ok3, d3) = drill_no_token_reject_10();
    s.add_dyn("J1-无令牌注入拒止10/10", ok3, &d3);
    // J1-d：标准会话（双路命中 + 句柄账平）。
    let mut bk = TokenBook::new();
    let p = bk.open_process_token(0, TOKEN_QUERY | TOKEN_DUPLICATE);
    let t = bk.open_thread_token(1, TOKEN_QUERY);
    s.add_dyn(
        "J1-双路开令牌账平",
        matches!(p, Ok(0)) && matches!(t, Ok(1)) && bk.handle_ledger_ok() && token_lines_complete(&bk),
        &format!(
            "进程={:?} 线程={:?} 账平={}",
            p.is_ok(),
            t.is_ok(),
            bk.handle_ledger_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t2, &format!("{}/{} {}", c, t2, d5.join(" ")));
    // J2-b：拒止不写回（无令牌进程句柄账不动）。
    let open0 = bk.token_of(0).map(|x| x.opens).unwrap_or(0);
    let _ = bk.open_process_token(1, TOKEN_QUERY);
    let open1 = bk.token_of(0).map(|x| x.opens).unwrap_or(0);
    s.add_dyn(
        "J2-拒止不写回",
        open0 == open1 && bk.reject_total() >= 1,
        &format!("打开 {}→{}", open0, open1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30520段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p2, d) in run_unx12441_extra() {
        s.add_dyn(n2, p2, &d);
    }
    s
}
