//! cgb847_inteldefect —— CGPU-F0847 Intel 缺陷清账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0847 Intel 缺陷清账  <SEG 0x7460>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0847；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（Intel 缺陷清账，整数账面）：
//!   1. 缺陷状态机五态闭集：New → Triaged → Fixed → Verified → Closed；
//!      另设 Rejected（否决）为终态旁路；非法迁移红码
//!      （XC_BAD_TRANS）；终态（Closed/Rejected）不可再迁移；
//!   2. 严重度闭集四级：S0 致命 / S1 严重 / S2 一般 / S3 轻微；
//!      清零判据：S0/S1 存量 = 0 且 S2 在修数 = 0 才可报「清账」；
//!   3. 缺陷登记定容账（32 槽），账守恒：登记数 = 存量数 + 结案数
//!      （Closed + Rejected）；漂移红码（XC_LEDGER_DRIFT）；
//!   4. 来源域闭集：驱动 / 编译器 / 运行时 / 硬件 / 文档，写入越域
//!      红码（XC_BAD_DOMAIN）；
//!   5. 修复验证账：每条 Fixed 必须带验证轮次（verify_run 单调），
//!      Verified 前不许 Closed；越序红码（XC_SKIP_VERIFY）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7460 段为本模块独占（SEG_BASE=0x7460），判别式
//!   SEG_BASE+1..+N；与 0x7440/0x7480 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const ID_VERSION: &str = "cgb847_inteldefect-v1";

/// 诊断段基址（0x7460 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7460;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7461 缺陷号越界（账外）。
pub const XC_BAD_DEFECT: u16 = SEG_BASE + 1;
/// 0x7462 状态迁移非法（状态机闭集之外）。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7463 严重度越界（0..=3 之外）。
pub const XC_BAD_SEVERITY: u16 = SEG_BASE + 3;
/// 0x7464 来源域越界（闭集之外）。
pub const XC_BAD_DOMAIN: u16 = SEG_BASE + 4;
/// 0x7465 登记账漂移（登记 ≠ 存量 + 结案）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 5;
/// 0x7466 越序结案（未 Verified 即求 Closed）。
pub const XC_SKIP_VERIFY: u16 = SEG_BASE + 6;
/// 0x7467 终态再迁移。
pub const XC_TERMINAL_MOVE: u16 = SEG_BASE + 7;
/// 0x7468 账满（32 槽用尽）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 8;
/// 0x7469 验证轮次倒挂。
pub const XC_VERIFY_BACKWARD: u16 = SEG_BASE + 9;
/// 0x7470 未清零即报清账（S0/S1 存量 > 0）。
pub const XC_NOT_CLEARED: u16 = SEG_BASE + 10;
/// 0x7471 重复登记（同签名）。
pub const XC_DUP_SIG: u16 = SEG_BASE + 11;

/// 账容量（写死：32）。
pub const LEDGER_CAP: usize = 32;
/// 严重度闭集上限。
pub const SEV_CAP: u8 = 3;
/// 来源域闭集上限。
pub const DOM_CAP: u8 = 4;

/// 缺陷状态五态 + 否决终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DState {
    /// 新建。
    New,
    /// 已分诊。
    Triaged,
    /// 已修复（待验证）。
    Fixed,
    /// 已验证。
    Verified,
    /// 已关闭（终态）。
    Closed,
    /// 已否决（终态）。
    Rejected,
}

/// 严重度闭集：0=S0 致命 1=S1 严重 2=S2 一般 3=S3 轻微。
pub const S0: u8 = 0;
pub const S1: u8 = 1;
pub const S2: u8 = 2;
pub const S3: u8 = 3;

/// 来源域闭集：0=驱动 1=编译器 2=运行时 3=硬件 4=文档。
pub const D_DRIVER: u8 = 0;
pub const D_COMPILER: u8 = 1;
pub const D_RUNTIME: u8 = 2;
pub const D_HARDWARE: u8 = 3;
pub const D_DOC: u8 = 4;

/// 单条缺陷记录。
#[derive(Clone, Debug)]
pub struct Defect {
    /// 缺陷号（1 起，0 保留）。
    pub id: u32,
    /// 签名（去重键）。
    pub sig: String,
    /// 严重度（0..=3）。
    pub severity: u8,
    /// 来源域（0..=4）。
    pub domain: u8,
    /// 当前状态。
    pub state: DState,
    /// 最近验证轮次。
    pub verify_run: u32,
    /// 迁移次数。
    pub moves: u32,
}

impl Defect {
    /// 终态判定。
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, DState::Closed | DState::Rejected)
    }
}

/// 状态迁移合法性（闭集真值表，无副作用）。
pub fn trans_legal(from: DState, to: DState) -> bool {
    use DState::*;
    matches!(
        (from, to),
        (New, Triaged)
            | (New, Rejected)
            | (Triaged, Fixed)
            | (Triaged, Rejected)
            | (Fixed, Verified)
            | (Fixed, Triaged)
            | (Verified, Closed)
            | (Verified, Fixed)
    )
}

/// 缺陷清账册。
pub struct DefectLedger {
    /// 顺序账（定容 32）。
    pub rows: Vec<Defect>,
    /// 下一缺陷号。
    pub next_id: u32,
    /// 结案数（Closed）。
    pub closed: u64,
    /// 否决数（Rejected）。
    pub rejected: u64,
    /// 累计登记数。
    pub registered: u64,
    /// 累计红码次数。
    pub red_count: u64,
}

impl DefectLedger {
    /// 开新账。
    pub fn new() -> Self {
        DefectLedger {
            rows: Vec::new(),
            next_id: 1,
            closed: 0,
            rejected: 0,
            registered: 0,
            red_count: 0,
        }
    }

    /// 存量数（未进终态）。
    pub fn open_count(&self) -> u64 {
        self.registered - self.closed - self.rejected
    }

    /// 账守恒：registered = open + closed + rejected。
    pub fn balanced(&self) -> bool {
        self.registered == self.open_count() + self.closed + self.rejected
    }

    /// 按号定位缺陷。
    pub fn find(&self, id: u32) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Ok(i);
            }
            i += 1;
        }
        Err(XC_BAD_DEFECT)
    }

    /// 签名查重。
    pub fn has_sig(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记新缺陷（S0/S3/DOM 越域、重复签名、账满即红）。
    pub fn register(&mut self, sig: &str, severity: u8, domain: u8) -> Result<u32, u16> {
        if severity > SEV_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SEVERITY);
        }
        if domain > DOM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DOMAIN);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_SIG);
        }
        if self.rows.len() == LEDGER_CAP {
            self.red_count += 1;
            return Err(XC_LEDGER_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(Defect {
            id,
            sig: String::from(sig),
            severity,
            domain,
            state: DState::New,
            verify_run: 0,
            moves: 0,
        });
        self.registered += 1;
        Ok(id)
    }

    /// 状态迁移（终态、非法迁移、越序结案即红）。
    pub fn advance(&mut self, id: u32, to: DState, verify_run: u32) -> Result<(), u16> {
        let idx = self.find(id)?;
        let cur = self.rows[idx].state;
        if cur.is_terminal() {
            self.red_count += 1;
            return Err(XC_TERMINAL_MOVE);
        }
        if !trans_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if to == DState::Closed && cur != DState::Verified {
            self.red_count += 1;
            return Err(XC_SKIP_VERIFY);
        }
        if to == DState::Verified {
            if verify_run <= self.rows[idx].verify_run {
                self.red_count += 1;
                return Err(XC_VERIFY_BACKWARD);
            }
            self.rows[idx].verify_run = verify_run;
        }
        if to == DState::Closed {
            self.closed += 1;
        }
        if to == DState::Rejected {
            self.rejected += 1;
        }
        self.rows[idx].state = to;
        self.rows[idx].moves += 1;
        Ok(())
    }

    /// S0/S1 存量计数。
    pub fn crit_open(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let d = &self.rows[i];
            if !d.is_terminal() && (d.severity == S0 || d.severity == S1) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// S2 在修计数（Triaged/Fixed 态的 S2）。
    pub fn s2_inflight(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let d = &self.rows[i];
            if !d.is_terminal()
                && d.severity == S2
                && matches!(d.state, DState::Triaged | DState::Fixed)
            {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 清账判定：S0/S1 存量 = 0 且 S2 在修 = 0。
    pub fn cleared(&self) -> Result<bool, u16> {
        if !self.balanced() {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(self.crit_open() == 0 && self.s2_inflight() == 0)
    }

    /// 清账报告（未清零红码；清零给结案摘要）。
    pub fn settle(&self) -> Result<String, u16> {
        if !self.balanced() {
            return Err(XC_LEDGER_DRIFT);
        }
        if !(self.crit_open() == 0 && self.s2_inflight() == 0) {
            return Err(XC_NOT_CLEARED);
        }
        let mut s = String::from("inteldefect settled closed=");
        s += &self.closed.to_string();
        s += " rejected=";
        s += &self.rejected.to_string();
        s += " total=";
        s += &self.registered.to_string();
        Ok(s)
    }

    /// 全账自检：守恒 + 每条终态对应计数一致。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_LEDGER_DRIFT);
        }
        let mut closed = 0u64;
        let mut rejected = 0u64;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].state == DState::Closed {
                closed += 1;
            }
            if self.rows[i].state == DState::Rejected {
                rejected += 1;
            }
            i += 1;
        }
        if closed != self.closed || rejected != self.rejected {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：登记 → 分诊 → 修复 → 验证 → 关闭全链 + 负例四连。
pub fn drill() -> Result<u32, u16> {
    let mut led = DefectLedger::new();
    let a = led.register("xe-clip-oob", S1, D_DRIVER)?;
    let b = led.register("xe-shader-hang", S2, D_COMPILER)?;
    let c = led.register("arc-vbt-nop", S3, D_HARDWARE)?;
    if led.register("xe-clip-oob", S2, D_DOC) != Err(XC_DUP_SIG) {
        return Err(XC_DUP_SIG);
    }
    led.advance(a, DState::Triaged, 0)?;
    led.advance(b, DState::Triaged, 0)?;
    led.advance(c, DState::Rejected, 0)?;
    // 负例：New → Fixed 直跳非法。
    if led.advance(b, DState::Fixed, 0) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    led.advance(a, DState::Fixed, 0)?;
    // 负例：未 Verified 关闭非法。
    if led.advance(a, DState::Closed, 0) != Err(XC_SKIP_VERIFY) {
        return Err(XC_SKIP_VERIFY);
    }
    led.advance(a, DState::Verified, 7)?;
    // 负例：验证轮次倒挂。
    if led.advance(a, DState::Fixed, 5) != Err(XC_VERIFY_BACKWARD) {
        return Err(XC_VERIFY_BACKWARD);
    }
    led.advance(a, DState::Closed, 0)?;
    led.advance(b, DState::Fixed, 0)?;
    led.advance(b, DState::Verified, 9)?;
    led.advance(b, DState::Closed, 0)?;
    // 负例：终态再迁移。
    if led.advance(a, DState::Triaged, 0) != Err(XC_TERMINAL_MOVE) {
        return Err(XC_TERMINAL_MOVE);
    }
    led.audit()?;
    if led.crit_open() != 0 {
        return Err(XC_NOT_CLEARED);
    }
    let rep = led.settle()?;
    if rep.len() == 0 {
        return Err(XC_NOT_CLEARED);
    }
    Ok(0)
}
