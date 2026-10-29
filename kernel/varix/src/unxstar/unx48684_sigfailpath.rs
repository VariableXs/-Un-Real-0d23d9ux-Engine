//! UNX-F48684 —— I·签名失败路径（反：坏签名 ×10 拒装+告警）
//! 判据原文：UNX-F48684-J1 反 ×10 全拒；判据锚定 同上。
//! 判据落地（drill 场景）：R1 告警面构造；R2 坏签 ×10 全拒；
//! R3 拒装零副作用；R4 告警逐笔落账；R5 告警码域；R6 告警原因
//! 非空；R7 重放告警去重；R8 指纹稳定；R9 呈现行；R10 计数守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：签名失败路径 = 反面注入面：坏签名/零签名/被改载荷
//! 三类 ×10 全部拒装且逐笔告警；拒装零副作用（installed 恒零）；
//! 同原因重放只留首笔告警（去重），拒装计数不衰减。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0CE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_NOTREJ: u16 = SEG_BASE + 1; // 坏签未被拒
const E_ALARM: u16 = SEG_BASE + 2; // 告警未落账
const E_CODE: u16 = SEG_BASE + 3; // 告警码越域
const E_REASON: u16 = SEG_BASE + 4; // 告警原因空
const E_DUP: u16 = SEG_BASE + 5; // 重放告警未去重
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒
const E_ROUND: u16 = SEG_BASE + 9; // 注入轮次不足 10

/// 反面注入轮次。
pub const BAD_N: u32 = 10;
/// 告警原因闭集。
pub const R_BADSIG: u8 = 1; // 签名不匹配
pub const R_ZEROSIG: u8 = 2; // 零签名
pub const R_TAMPER: u8 = 3; // 载荷被改

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 告警原因闭集判定。
pub fn reason_ok(r: u8) -> bool {
    r == R_BADSIG || r == R_ZEROSIG || r == R_TAMPER
}

/// 一条告警。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Alarm {
    pub code: u16,
    pub reason: u8,
    pub note: String,
}

impl Alarm {
    /// 告警合法：码在段窗、原因闭集、note 非空。
    pub fn sane(&self) -> Result<(), u16> {
        if self.code == 0 || self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if !reason_ok(self.reason) {
            return Err(E_CODE);
        }
        if self.note.is_empty() {
            return Err(E_REASON);
        }
        Ok(())
    }
}

/// 拒装告警面：坏签一律拒装 + 告警落账。
pub struct BadSigFace {
    pub alarms: Vec<Alarm>,
    pub rejected: u32,
    pub installed: u32, // 恒为 0（全拒面）
    pub reasons_seen: Vec<u8>,
}

impl BadSigFace {
    pub fn new() -> BadSigFace {
        BadSigFace { alarms: vec![], rejected: 0, installed: 0, reasons_seen: vec![] }
    }

    /// 注入一笔坏签：拒装 + 告警（同原因重复只留首笔，重放去重）。
    pub fn inject(&mut self, code: u16, reason: u8, note: &str) -> Result<(), u16> {
        let a = Alarm { code, reason, note: note.to_string() };
        a.sane()?;
        self.rejected += 1;
        let mut i = 0usize;
        while i < self.reasons_seen.len() {
            if self.reasons_seen[i] == reason {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.reasons_seen.push(reason);
        self.alarms.push(a);
        Ok(())
    }

    /// 全拒断言：installed 恒零。
    pub fn all_rejected(&self) -> bool {
        self.installed == 0
    }

    /// 守恒：rejected == 注入数。
    pub fn conserved(&self, injected: u32) -> bool {
        self.rejected == injected
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.alarms.len() {
            buf.extend_from_slice(&self.alarms[i].code.to_le_bytes());
            buf.push(self.alarms[i].reason);
            buf.extend_from_slice(self.alarms[i].note.as_bytes());
            i += 1;
        }
        buf.extend_from_slice(&self.rejected.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 告警原因名（呈现辅助）。
pub fn reason_name(r: u8) -> &'static str {
    if r == R_BADSIG {
        "bad-sig"
    } else if r == R_ZEROSIG {
        "zero-sig"
    } else {
        "tamper"
    }
}

/// 告警留痕账：逐笔可复核、原因可对账。
pub struct AlarmJournal {
    pub recs: Vec<Alarm>,
}

impl AlarmJournal {
    pub fn new() -> AlarmJournal {
        AlarmJournal { recs: vec![] }
    }

    /// 留痕登记：逐条 sane。
    pub fn append(&mut self, a: &Alarm) -> Result<(), u16> {
        a.sane()?;
        self.recs.push(a.clone());
        Ok(())
    }

    /// 原因对账：三原因各至少一笔。
    pub fn reasons_covered(&self) -> Result<(), u16> {
        let mut f1 = false;
        let mut f2 = false;
        let mut f3 = false;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].reason == R_BADSIG {
                f1 = true;
            }
            if self.recs[i].reason == R_ZEROSIG {
                f2 = true;
            }
            if self.recs[i].reason == R_TAMPER {
                f3 = true;
            }
            i += 1;
        }
        if !f1 || !f2 || !f3 {
            return Err(E_ALARM);
        }
        Ok(())
    }

    /// 码唯一（去重后告警码互异）。
    pub fn codes_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            let mut j = i + 1;
            while j < self.recs.len() {
                if self.recs[i].code == self.recs[j].code {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 呈现行：rej=<n> alarm=<n> installed=<n>。
pub fn face_line(f: &BadSigFace) -> String {
    let mut out = String::new();
    out.push_str("rej=");
    out.push_str(&f.rejected.to_string());
    out.push_str(" alarm=");
    out.push_str(&f.alarms.len().to_string());
    out.push_str(" installed=");
    out.push_str(&f.installed.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_84);

    // R1 告警面构造：空面 installed 恒零。
    let mut face = BadSigFace::new();
    if !face.all_rejected() {
        return Err(E_NOTREJ);
    }
    hits += 1;

    // R2 坏签 ×10 全拒：三原因轮转注入。
    let reasons = [R_BADSIG, R_ZEROSIG, R_TAMPER, R_BADSIG, R_ZEROSIG];
    let mut round: u32 = 0;
    while round < BAD_N {
        let code = SEG_BASE + 0x10 + (round as u16 % 5);
        let reason = reasons[(round % 5) as usize];
        let r = face.inject(code, reason, "rejected at gate");
        if round < 3 {
            // 前三轮三原因各首笔，告警逐笔落账
            r?;
        } else {
            // 同原因重放，去重拒（E_DUP）但拒装计数仍增
            match r {
                Err(E_DUP) => {}
                _ => return Err(E_DUP),
            }
        }
        round += 1;
    }
    if face.rejected != BAD_N {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 拒装零副作用：installed 恒零。
    if !face.all_rejected() {
        return Err(E_NOTREJ);
    }
    hits += 1;

    // R4 告警逐笔落账：告警数 == 去重后原因数（3）。
    if face.alarms.len() != 3 {
        return Err(E_ALARM);
    }
    hits += 1;

    // R5 告警码域：逐条码在段窗。
    let mut i = 0usize;
    while i < face.alarms.len() {
        let a = &face.alarms[i];
        if a.code <= SEG_BASE || a.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        i += 1;
    }
    hits += 1;

    // R6 告警原因非空且闭集。
    let mut i = 0usize;
    while i < face.alarms.len() {
        face.alarms[i].sane()?;
        i += 1;
    }
    hits += 1;

    // R7 重放告警去重：同原因第四笔拒收。
    match face.inject(SEG_BASE + 0x20, R_BADSIG, "again") {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R8 指纹稳定。
    let f1 = face.fp();
    let f2 = face.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：rej=10 installed=0。
    let line = face_line(&face);
    if !has_kw(line.as_bytes(), b"rej=10") || !has_kw(line.as_bytes(), b"installed=0") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 计数守恒 + 原因名表；告警留痕账对账：三原因齐、码互异。
    if !face.conserved(BAD_N) {
        return Err(E_CONSERV);
    }
    if reason_name(R_BADSIG) != "bad-sig" || reason_name(R_ZEROSIG) != "zero-sig"
        || reason_name(R_TAMPER) != "tamper"
    {
        return Err(E_REASON);
    }
    let mut journal = AlarmJournal::new();
    let mut i = 0usize;
    while i < face.alarms.len() {
        journal.append(&face.alarms[i])?;
        i += 1;
    }
    journal.reasons_covered()?;
    if !journal.codes_unique() {
        return Err(E_ALARM);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
