#![allow(dead_code)]
//! cgb766_persec —— CGPU-F0766 感知安全。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0766 感知安全  <SEG 0x6B00>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0766；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面输入安全闸，整数面）：
//!   1. 输入合法域（写死）：长度 1..=64 字节；字符集限
//!      a-z A-Z 0-9 空格 _ - . ；控制字节（<0x20）红码、
//!      高位字节（≥0x80）红码、其余标点红码——分类入隔离账；
//!   2. 速率闸：定窗 100 tick、窗容 16 次；窗内超额 → 打击 +1
//!      并拒收；跨窗自动换窗（窗口起点单调推进）；
//!   3. 打击阶梯：0 打击 Clean / 1..=2 Suspicious / ≥3 Blocked；
//!      Suspicious 可赦免回 Clean，Blocked 不可逆（会话面）；
//!   4. 隔离账：定容 16 槽，存违规（长度, 原因码）；满则逐出最老；
//!      守恒恒等式：尝试数 = 通过数 + 隔离数 + 速率拒数，
//!      隔离账 = 在库 + 逐出；
//!   5. Blocked 态仍记账（隔离/速率拒照常），但通过数冻结；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6B00 段为本模块独占（SEG_BASE=0x6B00），判别式
//!   SEG_BASE+1..+N；与 0x6AE0/0x6B20 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PS_VERSION: &str = "cgb766_persec-v1";

/// 诊断段基址（0x6B00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6B00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6B01 输入超长（>64）。
pub const XC_TOO_LONG: u16 = SEG_BASE + 1;
/// 0x6B02 控制字节（<0x20）。
pub const XC_CTRL_CHAR: u16 = SEG_BASE + 2;
/// 0x6B03 高位字节（≥0x80）。
pub const XC_HIGH_BYTE: u16 = SEG_BASE + 3;
/// 0x6B04 越册标点。
pub const XC_BAD_CHAR: u16 = SEG_BASE + 4;
/// 0x6B05 空输入。
pub const XC_EMPTY: u16 = SEG_BASE + 5;
/// 0x6B06 速率超窗。
pub const XC_RATE_HIT: u16 = SEG_BASE + 6;
/// 0x6B07 会话已封禁。
pub const XC_BLOCKED: u16 = SEG_BASE + 7;
/// 0x6B08 赦免面非法（Blocked 态或 Clean 态）。
pub const XC_PARDON_BAD: u16 = SEG_BASE + 8;
/// 0x6B09 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 9;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 输入长度上限（写死：64）。
pub const LEN_MAX: usize = 64;
/// 速率窗宽（写死：100 tick）。
pub const WINDOW_TICKS: u32 = 100;
/// 窗容（写死：16 次）。
pub const WINDOW_CAP: u32 = 16;
/// 隔离账容量（写死：16）。
pub const QUAR_CAP: usize = 16;
/// 封禁打击线（写死：3）。
pub const BLOCK_LINE: u32 = 3;

/// 会话状态阶梯。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecState {
    /// 无打击。
    Clean,
    /// 1..=2 打击。
    Suspicious,
    /// ≥3 打击（不可逆）。
    Blocked,
}

/// 隔离账条目（违规长度 + 原因码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuarRec {
    pub len: u32,
    pub reason: u16,
}

/// 输入字符合法性（字节级分类）。
pub fn byte_legal(b: u8) -> Result<(), u16> {
    if b < 0x20 {
        return Err(XC_CTRL_CHAR);
    }
    if b >= 0x80 {
        return Err(XC_HIGH_BYTE);
    }
    let ok = (b >= b'a' && b <= b'z')
        || (b >= b'A' && b <= b'Z')
        || (b >= b'0' && b <= b'9')
        || b == b' '
        || b == b'_'
        || b == b'-'
        || b == b'.';
    if !ok {
        return Err(XC_BAD_CHAR);
    }
    Ok(())
}

/// 输入整体合法性（空/长/逐字节三查，返回首个违规原因）。
pub fn sanitize(s: &str) -> Result<(), u16> {
    let bs = s.as_bytes();
    if bs.is_empty() {
        return Err(XC_EMPTY);
    }
    if bs.len() > LEN_MAX {
        return Err(XC_TOO_LONG);
    }
    for b in bs.iter() {
        byte_legal(*b)?;
    }
    Ok(())
}

/// 安全摘要串：状态名 + 打击数。
pub fn banner(state: SecState, strikes: u32) -> String {
    let mut s = String::from("SEC[");
    s.push_str(match state {
        SecState::Clean => "clean",
        SecState::Suspicious => "susp",
        SecState::Blocked => "blocked",
    });
    s.push(':');
    s.push_str(&strikes.to_string());
    s.push(']');
    s
}

/// 感知面安全会话：清洗闸 + 速率闸 + 打击阶梯 + 隔离账。
/// 守恒恒等式：尝试数 = 通过数 + 隔离数 + 速率拒数 + 封禁拒数。
pub struct SecSession {
    state: SecState,
    strikes: u32,
    attempts: u32,
    accepted: u32,
    quarantined: u32,
    rate_rej: u32,
    blocked_rej: u32,
    win_start: u32,
    win_count: u32,
    quar: Vec<QuarRec>,
    quar_evicted: u32,
}

impl SecSession {
    /// 空会话（Clean，窗口起点 0）。
    pub fn new() -> Self {
        SecSession {
            state: SecState::Clean,
            strikes: 0,
            attempts: 0,
            accepted: 0,
            quarantined: 0,
            rate_rej: 0,
            blocked_rej: 0,
            win_start: 0,
            win_count: 0,
            quar: Vec::new(),
            quar_evicted: 0,
        }
    }

    /// 打击记账：1..=2 → Suspicious；≥3 → Blocked（不可逆）。
    fn strike(&mut self) {
        self.strikes += 1;
        if self.strikes >= BLOCK_LINE {
            self.state = SecState::Blocked;
        } else if self.state != SecState::Blocked {
            self.state = SecState::Suspicious;
        }
    }

    /// 赦免：仅 Suspicious 态 → Clean（打击清零）。
    pub fn pardon(&mut self) -> Result<(), u16> {
        if self.state != SecState::Suspicious {
            return Err(XC_PARDON_BAD);
        }
        self.strikes = 0;
        self.state = SecState::Clean;
        Ok(())
    }

    /// 隔离入账（定容 16，满则逐出最老）。
    fn quarantine(&mut self, s: &str, reason: u16) {
        if self.quar.len() >= QUAR_CAP {
            self.quar.remove(0);
            self.quar_evicted += 1;
        }
        self.quar.push(QuarRec {
            len: s.len() as u32,
            reason,
        });
        self.quarantined += 1;
    }

    /// 提交输入：清洗闸（先于封禁，违规照常隔离）→ 封禁闸 →
    /// 速率闸 → 通过记账。
    pub fn submit(&mut self, tick: u32, s: &str) -> Result<(), u16> {
        self.attempts += 1;
        // 清洗闸（Blocked 态照样清洗并隔离）。
        if let Err(reason) = sanitize(s) {
            self.quarantine(s, reason);
            self.strike();
            return Err(reason);
        }
        // 封禁闸：合法输入在 Blocked 态拒收。
        if self.state == SecState::Blocked {
            self.blocked_rej += 1;
            return Err(XC_BLOCKED);
        }
        // 速率闸：跨窗换窗，窗内超额打击。
        if tick < self.win_start + WINDOW_TICKS {
            self.win_count += 1;
            if self.win_count > WINDOW_CAP {
                self.rate_rej += 1;
                self.strike();
                return Err(XC_RATE_HIT);
            }
        } else {
            self.win_start = tick;
            self.win_count = 1;
        }
        self.accepted += 1;
        Ok(())
    }

    /// 状态只读。
    pub fn state(&self) -> SecState {
        self.state
    }

    /// 打击数只读。
    pub fn strike_count(&self) -> u32 {
        self.strikes
    }

    /// 隔离账在库数。
    pub fn quar_len(&self) -> usize {
        self.quar.len()
    }

    /// 隔离账条目只读。
    pub fn quar_at(&self, i: usize) -> Result<QuarRec, u16> {
        if i >= self.quar.len() {
            return Err(XC_UNBALANCED);
        }
        Ok(self.quar[i])
    }

    /// 守恒核：尝试 = 通过 + 隔离 + 速率拒 + 封禁拒；
    /// 隔离账 = 在库 + 逐出；通过数 ≤ 尝试数。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.attempts
            != self.accepted + self.quarantined + self.rate_rej + self.blocked_rej
        {
            return Err(XC_UNBALANCED);
        }
        if (self.quar.len() as u32) + self.quar_evicted != self.quarantined {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 安全摘要位型：打击数(0..8) | 状态(8..10) | 通过数(10..26)。
    pub fn summary(&self) -> u32 {
        let st = match self.state {
            SecState::Clean => 0u32,
            SecState::Suspicious => 1,
            SecState::Blocked => 2,
        };
        (self.strikes & 0xFF)
            | (st << 8)
            | ((self.accepted & 0xFFFF) << 10)
    }
}

/// 自例程：清洗闸全字符类 → 速率闸换窗 → 打击阶梯 → 赦免 →
/// 封禁不可逆 → 隔离账滚动 → 守恒 → 摘要位型。
pub fn drill() -> Result<u32, u16> {
    // 清洗直查：合法面 / 各违规类。
    if sanitize("hello_world-1. ok").is_err() {
        return Err(XC_BAD_CHAR);
    }
    if sanitize("") != Err(XC_EMPTY) {
        return Err(XC_EMPTY);
    }
    let long = alloc::string::String::from("a").repeat(LEN_MAX + 1);
    if sanitize(&long) != Err(XC_TOO_LONG) {
        return Err(XC_TOO_LONG);
    }
    if byte_legal(0x01) != Err(XC_CTRL_CHAR) {
        return Err(XC_CTRL_CHAR);
    }
    if byte_legal(0x80) != Err(XC_HIGH_BYTE) {
        return Err(XC_HIGH_BYTE);
    }
    if byte_legal(b'!') != Err(XC_BAD_CHAR) {
        return Err(XC_BAD_CHAR);
    }
    // 主路径：10 次合法提交（窗容 16 内）。
    let mut s = SecSession::new();
    for i in 0..10u32 {
        s.submit(10 + i, "sample-01")?;
    }
    if s.state() != SecState::Clean {
        return Err(XC_UNBALANCED);
    }
    s.conserved()?;
    // 速率闸：窗内再打 7 次（16 容量：10+7=17 > 16）→ 第 17 次拒。
    let mut s2 = SecSession::new();
    for i in 0..16u32 {
        s2.submit(10 + i, "ok")?;
    }
    if s2.submit(50, "ok") != Err(XC_RATE_HIT) {
        return Err(XC_RATE_HIT);
    }
    // 1 打击 → Suspicious。
    if s2.state() != SecState::Suspicious || s2.strike_count() != 1 {
        return Err(XC_UNBALANCED);
    }
    // 跨窗换窗：tick 110 ≥ 0+100 → 新窗，恢复通过。
    s2.submit(110, "ok")?;
    s2.conserved()?;
    // 违规打击：1（速率）+ 2（违规）= 3 → Blocked。
    if s2.submit(120, "bad!") != Err(XC_BAD_CHAR) {
        return Err(XC_BAD_CHAR);
    }
    if s2.submit(121, "x\x01y") != Err(XC_CTRL_CHAR) {
        return Err(XC_CTRL_CHAR);
    }
    if s2.state() != SecState::Blocked || s2.strike_count() != 3 {
        return Err(XC_UNBALANCED);
    }
    // Blocked 态：合法输入也拒（封禁面）。
    if s2.submit(130, "ok") != Err(XC_BLOCKED) {
        return Err(XC_BLOCKED);
    }
    // Blocked 态赦免非法。
    if s2.pardon() != Err(XC_PARDON_BAD) {
        return Err(XC_PARDON_BAD);
    }
    s2.conserved()?;
    // 赦免路径：单打击 Suspicious → 赦免 → Clean。
    let mut s3 = SecSession::new();
    if s3.submit(0, "bad!") != Err(XC_BAD_CHAR) {
        return Err(XC_BAD_CHAR);
    }
    if s3.state() != SecState::Suspicious {
        return Err(XC_UNBALANCED);
    }
    s3.pardon()?;
    if s3.state() != SecState::Clean || s3.strike_count() != 0 {
        return Err(XC_UNBALANCED);
    }
    // Clean 态赦免非法。
    if s3.pardon() != Err(XC_PARDON_BAD) {
        return Err(XC_PARDON_BAD);
    }
    // 隔离账滚动：20 次违规（>16 → 逐出 4）。
    let mut s4 = SecSession::new();
    for _ in 0..20u32 {
        let _ = s4.submit(0, "bad!");
    }
    s4.conserved()?;
    if s4.quar_len() != QUAR_CAP {
        return Err(XC_UNBALANCED);
    }
    // 隔离条目只读：长度 4，原因 XC_BAD_CHAR。
    let q = s4.quar_at(0)?;
    if q.len != 4 || q.reason != XC_BAD_CHAR {
        return Err(XC_UNBALANCED);
    }
    // 摘要位型：打击 20 & 0xFF = 20，状态 Blocked = 2。
    let sm = s4.summary();
    if sm & 0xFF != 20 {
        return Err(XC_UNBALANCED);
    }
    if (sm >> 8) & 0b11 != 2 {
        return Err(XC_UNBALANCED);
    }
    // 摘要串核对。
    let b = banner(SecState::Blocked, 20);
    if !b.contains("blocked:20") {
        return Err(XC_UNBALANCED);
    }
    let b2 = banner(SecState::Clean, 0);
    if !b2.contains("clean:0") {
        return Err(XC_UNBALANCED);
    }
    Ok(0)
}
