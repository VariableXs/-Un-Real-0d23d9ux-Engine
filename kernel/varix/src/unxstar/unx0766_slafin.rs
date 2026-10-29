//! UNX-F0766 · Limine 升级 SLA 账终核（7 日核算复核） · 诊断段 0x5e80
//! 判据原文摘录：UNX-F0766-J1 终核过（SLA 账逐单复核：双源核算再现 +
//! 零未处置违约断言过）且与 F0734 账一致 100%；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 双源核算再现：每张跟随单的违约天数由两个独立来源核算
//!    （账面登记值 / 死线重算值），双源一致才采信；不一致即终核拒绝。
//! 2. 7 日口径复核：>7 日即违约的判定逐单重算，与 F0734 账行逐单比对
//!    （级别/天数/状态三要素一致 100%）。
//! 3. 零未处置违约断言：终核口径下 Unhandled/Escalated 计数必须为 0，
//!    任一未清即终核拒绝。
//! 4. 判定再现链：F0734 侧结论（已清）与终核重算结论等值。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5e80 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5e80;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_TICKET_BAD: u16 = SEG_BASE + 1; // 单号越界
const E_TICKET_DUP: u16 = SEG_BASE + 2; // 单号重复登记
const E_TICKET_FULL: u16 = SEG_BASE + 3; // 终核账已满
const E_DAYS_NEG: u16 = SEG_BASE + 4; // 天数为负（重算源非法）
const E_DUAL_MISMATCH: u16 = SEG_BASE + 5; // 双源核算不一致
const E_LEVEL_MISMATCH: u16 = SEG_BASE + 6; // 级别与 F0734 账不一致
const E_STATE_OUT: u16 = SEG_BASE + 7; // 存在未处置（终核拒绝）
const E_CONSIST: u16 = SEG_BASE + 8; // 与 F0734 账一致 100% 失败
const E_BAL: u16 = SEG_BASE + 9; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 10; // 终核已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 11; // 终核账为空
const E_F0734_EMPTY: u16 = SEG_BASE + 12; // F0734 对侧账为空
const E_TWICE: u16 = SEG_BASE + 13; // 单据重复终核
const E_ALTERED: u16 = SEG_BASE + 14; // 账行被改（登记后禁改）

/// SLA 时限：7 日（与 F0734 同口径）。
pub const SLA_DAYS: u32 = 7;

/// 终核账定容。
pub const MAX_TICKETS: usize = 32;

/// F0734 对侧账定容。
pub const MAX_F0734: usize = 32;

/// 处置状态码（与 F0734 同口径）。
pub const ST_UNHANDLED: u8 = 0;
pub const ST_ESCALATED: u8 = 1;
pub const ST_HANDLED: u8 = 2;

// ── FNV-1a 64（终核哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TICKET_BAD => "单号越界：按登记序 1..N",
        E_TICKET_DUP => "单号重复登记",
        E_TICKET_FULL => "终核账已满 32 单",
        E_DAYS_NEG => "天数为负：重算源非法",
        E_DUAL_MISMATCH => "双源核算不一致：账面值与死线重算值不等",
        E_LEVEL_MISMATCH => "级别不一致：按天数重定级与 F0734 账不符",
        E_STATE_OUT => "存在未处置违约：先处置再终核",
        E_CONSIST => "与 F0734 账一致 100% 失败：逐单核对",
        E_BAL => "守恒破裂：计数账不平",
        E_SEALED => "终核已封：封账后只读",
        E_EMPTY => "终核账为空",
        E_F0734_EMPTY => "F0734 对侧账为空：先装载",
        E_TWICE => "单据重复终核",
        E_ALTERED => "账行被改：登记后禁改",
        _ => "未知诊断码",
    }
}

// ── 级别重定级 ───────────────────────────────────────────────
/// 按违约天数重定级（与 F0734 同口径：≥7 高危、3..=6 中危、<3 低危）。
pub fn regrade(overdue: u32) -> u8 {
    if overdue >= 7 {
        3
    } else if overdue >= 3 {
        2
    } else {
        1
    }
}

/// 级别中文名。
pub fn level_zh(level: u8) -> &'static str {
    match level {
        1 => "低危",
        2 => "中危",
        3 => "高危",
        _ => "非法级",
    }
}

// ── 终核账行 ─────────────────────────────────────────────────
/// 一张跟随单的终核行。
#[derive(Clone, Debug)]
pub struct SlaCheckRow {
    /// 单号（1 起连续）。
    pub ticket_id: u32,
    /// 主题。
    pub subject: String,
    /// 来源一：账面登记违约天数。
    pub recorded_days: u32,
    /// 来源二：死线重算违约天数。
    pub recomputed_days: u32,
    /// 账面级别。
    pub recorded_level: u8,
    /// 账面处置状态（0/1/2）。
    pub state: u8,
    /// 双源核过（终核后落）。
    pub dual_ok: Option<bool>,
}

impl SlaCheckRow {
    /// 双源一致性。
    pub fn dual_agree(&self) -> bool {
        self.recorded_days == self.recomputed_days
    }

    /// 级别重算一致。
    pub fn level_agree(&self) -> bool {
        self.recorded_level == regrade(self.recomputed_days)
    }

    /// 是否已清。
    pub fn cleared(&self) -> bool {
        self.state == ST_HANDLED
    }
}

// ── F0734 对侧行 ─────────────────────────────────────────────
/// F0734 账的一行投影：单号 + 天数 + 级别 + 状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct F0734Row {
    /// 单号。
    pub ticket_id: u32,
    /// 违约天数。
    pub overdue: u32,
    /// 级别。
    pub level: u8,
    /// 状态。
    pub state: u8,
}

// ── SLA 账终核（主体）────────────────────────────────────────
/// Limine 升级 SLA 账终核：双源核算 + 零未处置 + F0734 一致 100%。
pub struct SlaFinal {
    rows: [Option<SlaCheckRow>; MAX_TICKETS],
    row_count: usize,
    /// F0734 对侧账。
    mirror: [Option<F0734Row>; MAX_F0734],
    mirror_count: usize,
    /// 双源核过单数。
    dual_checked: u32,
    dual_passed: u32,
    /// 是否封账。
    sealed: bool,
    /// 终核哈希。
    sealed_hash: u64,
}

impl SlaFinal {
    /// 开终核账。
    pub fn new() -> SlaFinal {
        SlaFinal {
            rows: [const { None }; MAX_TICKETS],
            row_count: 0,
            mirror: [const { None }; MAX_F0734],
            mirror_count: 0,
            dual_checked: 0,
            dual_passed: 0,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 登记终核行：单号合法 + 状态闭集 + 一单一行。
    pub fn put_row(
        &mut self,
        ticket_id: u32,
        subject: &str,
        recorded_days: u32,
        recomputed_days: u32,
        recorded_level: u8,
        state: u8,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.row_count >= MAX_TICKETS {
            return Err(E_TICKET_FULL);
        }
        if subject.is_empty() {
            return Err(E_TICKET_BAD);
        }
        if state > ST_HANDLED {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                if r.ticket_id == ticket_id {
                    return Err(E_TICKET_DUP);
                }
            }
            i += 1;
        }
        self.rows[self.row_count] = Some(SlaCheckRow {
            ticket_id,
            subject: subject.to_string(),
            recorded_days,
            recomputed_days,
            recorded_level,
            state,
            dual_ok: None,
        });
        self.row_count += 1;
        Ok(())
    }

    /// 行只读快照。
    pub fn get_row(&self, ticket_id: u32) -> Result<SlaCheckRow, u16> {
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                if r.ticket_id == ticket_id {
                    return Ok(r.clone());
                }
            }
            i += 1;
        }
        Err(E_TICKET_BAD)
    }

    /// 装载 F0734 对侧账行。
    pub fn put_mirror(&mut self, row: F0734Row) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.mirror_count >= MAX_F0734 {
            return Err(E_TICKET_FULL);
        }
        if row.state > ST_HANDLED {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < self.mirror_count {
            if let Some(m) = &self.mirror[i] {
                if m.ticket_id == row.ticket_id {
                    return Err(E_TICKET_DUP);
                }
            }
            i += 1;
        }
        self.mirror[self.mirror_count] = Some(row);
        self.mirror_count += 1;
        Ok(())
    }

    /// 对侧账行数。
    pub fn mirror_count(&self) -> usize {
        self.mirror_count
    }

    /// 终核行数。
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// 单据双源核算：dual 一致 + 级别重算一致 → 落核结论。
    pub fn check_ticket(&mut self, ticket_id: u32) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut i = 0usize;
        let mut found = false;
        while i < self.row_count {
            let ok = {
                let r = self.rows[i].as_ref().ok_or(E_TICKET_BAD)?;
                r.ticket_id == ticket_id
            };
            if ok {
                let r = self.rows[i].as_mut().ok_or(E_TICKET_BAD)?;
                if r.dual_ok.is_some() {
                    return Err(E_TWICE);
                }
                let agree = r.dual_agree() && r.level_agree();
                r.dual_ok = Some(agree);
                self.dual_checked += 1;
                if agree {
                    self.dual_passed += 1;
                }
                found = true;
                return Ok(agree);
            }
            i += 1;
        }
        if !found {
            return Err(E_TICKET_BAD);
        }
        Ok(false)
    }

    /// 全账双源核算。
    pub fn check_all(&mut self) -> Result<(u32, u32), u16> {
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        // 收集单号（避免借用冲突，先取后核）。
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                ids.push(r.ticket_id);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < ids.len() {
            self.check_ticket(ids[j])?;
            j += 1;
        }
        Ok((self.dual_checked, self.dual_passed))
    }

    /// 零未处置违约断言：全账状态必须全 Handled。
    pub fn zero_outstanding_assert(&self) -> Result<(), u16> {
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                if r.state != ST_HANDLED {
                    return Err(E_STATE_OUT);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 与 F0734 账一致 100% 断言：
    /// 终核行集与对侧账行集逐单比对（天数/级别/状态三要素）。
    pub fn consistency_assert(&self) -> Result<(), u16> {
        if self.mirror_count == 0 {
            return Err(E_F0734_EMPTY);
        }
        if self.row_count != self.mirror_count {
            return Err(E_CONSIST);
        }
        let mut i = 0usize;
        while i < self.row_count {
            let r = self.rows[i].as_ref().ok_or(E_TICKET_BAD)?;
            // 对侧找同单。
            let mut hit = false;
            let mut j = 0usize;
            while j < self.mirror_count {
                if let Some(m) = &self.mirror[j] {
                    if m.ticket_id == r.ticket_id {
                        hit = true;
                        if m.overdue != r.recorded_days
                            || m.level != r.recorded_level
                            || m.state != r.state
                        {
                            return Err(E_CONSIST);
                        }
                    }
                }
                j += 1;
            }
            if !hit {
                return Err(E_CONSIST);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：双源核过数 == 终核行数。
    pub fn conserved(&self) -> bool {
        self.dual_checked == self.row_count as u32
            && self.dual_passed <= self.dual_checked
    }

    /// 终核总断言：双源全过 + 零未处置 + 一致 100% + 守恒。
    pub fn final_assert(&self) -> Result<(), u16> {
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.dual_passed != self.row_count as u32 {
            return Err(E_DUAL_MISMATCH);
        }
        self.zero_outstanding_assert()?;
        self.consistency_assert()?;
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.final_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                buf.extend_from_slice(r.ticket_id.to_le_bytes().as_slice());
                buf.extend_from_slice(r.recorded_days.to_le_bytes().as_slice());
                buf.push(r.recorded_level);
                buf.push(r.state);
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 终核哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 终核报告行。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== SLA 账终核 · 双源核算 ===".to_string());
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                let mut line = String::from("T");
                line.push_str(&itoa(r.ticket_id as u64));
                line.push('|');
                line.push_str(&r.subject);
                line.push_str("|账面 ");
                line.push_str(&itoa(r.recorded_days as u64));
                line.push_str("d|重算 ");
                line.push_str(&itoa(r.recomputed_days as u64));
                line.push_str("d|");
                line.push_str(level_zh(r.recorded_level));
                line.push_str("|核 ");
                line.push_str(match r.dual_ok {
                    Some(true) => "过",
                    Some(false) => "不过",
                    None => "未核",
                });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 处置状态三态计数：(未处置, 已升级, 已处置)。
    pub fn state_tally(&self) -> (u32, u32, u32) {
        let mut n0 = 0u32;
        let mut n1 = 0u32;
        let mut n2 = 0u32;
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                match r.state {
                    ST_UNHANDLED => n0 += 1,
                    ST_ESCALATED => n1 += 1,
                    _ => n2 += 1,
                }
            }
            i += 1;
        }
        (n0, n1, n2)
    }

    /// 最大违约天数（账面口径）。
    pub fn overdue_max(&self) -> Result<u32, u16> {
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        let mut mx = 0u32;
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                if r.recorded_days > mx {
                    mx = r.recorded_days;
                }
            }
            i += 1;
        }
        Ok(mx)
    }
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：双源核算 + 零未处置 + F0734 一致 100% + 封账 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut sf = SlaFinal::new();
    // 三张单：双源一致、全 Handled。
    // T1 违约 9 日（高危）、T2 违约 4 日（中危）、T3 违约 2 日（低危）。
    sf.put_row(1, "哈希先行纪律单", 9, 9, regrade(9), ST_HANDLED)?;
    sf.put_row(2, "定容账本迁移单", 4, 4, regrade(4), ST_HANDLED)?;
    sf.put_row(3, "段位码复用单", 2, 2, regrade(2), ST_HANDLED)?;
    // F0734 对侧账同值装载。
    sf.put_mirror(F0734Row { ticket_id: 1, overdue: 9, level: regrade(9), state: ST_HANDLED })?;
    sf.put_mirror(F0734Row { ticket_id: 2, overdue: 4, level: regrade(4), state: ST_HANDLED })?;
    sf.put_mirror(F0734Row { ticket_id: 3, overdue: 2, level: regrade(2), state: ST_HANDLED })?;
    if sf.row_count() != 3 || sf.mirror_count() != 3 {
        return Err(SEG_BASE);
    }
    let (c, p) = sf.check_all()?;
    if c != 3 || p != 3 {
        return Err(SEG_BASE);
    }
    sf.zero_outstanding_assert()?;
    sf.consistency_assert()?;
    sf.final_assert()?;
    // 级别口径核：regrade 阈值。
    if regrade(7) != 3 || regrade(6) != 2 || regrade(2) != 1 {
        return Err(SEG_BASE);
    }
    if level_zh(3) != "高危" {
        return Err(SEG_BASE);
    }
    // 取数口：三态与最大违约天数。
    let (n0, n1, n2) = sf.state_tally();
    if n0 != 0 || n1 != 0 || n2 != 3 {
        return Err(SEG_BASE);
    }
    if sf.overdue_max()? != 9 {
        return Err(SEG_BASE);
    }
    let h = sf.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封账后写拒绝。
    match sf.put_row(4, "封账后单", 1, 1, 1, ST_HANDLED) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：双源不一致。
    let mut sf2 = SlaFinal::new();
    sf2.put_row(1, "不一致单", 9, 8, regrade(8), ST_HANDLED)?;
    match sf2.check_ticket(1) {
        Ok(false) => {}
        _ => return Err(SEG_BASE),
    }
    match sf2.final_assert() {
        Err(c) if c == E_DUAL_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：存在未处置。
    let mut sf3 = SlaFinal::new();
    sf3.put_row(1, "未处置单", 8, 8, regrade(8), ST_UNHANDLED)?;
    sf3.put_mirror(F0734Row { ticket_id: 1, overdue: 8, level: regrade(8), state: ST_UNHANDLED })?;
    let _ = sf3.check_all();
    match sf3.zero_outstanding_assert() {
        Err(c) if c == E_STATE_OUT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：与对侧不一致（状态漂移）。
    let mut sf4 = SlaFinal::new();
    sf4.put_row(1, "漂移单", 8, 8, regrade(8), ST_HANDLED)?;
    sf4.put_mirror(F0734Row { ticket_id: 1, overdue: 8, level: regrade(8), state: ST_ESCALATED })?;
    let _ = sf4.check_all();
    match sf4.consistency_assert() {
        Err(c) if c == E_CONSIST => {}
        _ => return Err(SEG_BASE),
    }
    let rl = sf.report_lines();
    if rl.len() != 4 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
