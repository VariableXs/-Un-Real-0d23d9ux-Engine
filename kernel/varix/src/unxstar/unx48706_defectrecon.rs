//! UNX-F48706 —— I·AI-97 缺陷转单对账（本域缺陷账全量对账）
//! 判据原文：UNX-F48706-J1 对账 ×1 全量零悬挂；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 账目登记；R2 转单落痕；R3 关单落痕；
//! R4 对账 ×1 全量扫描；R5 悬挂检出；R6 状态闭集；R7 计数守恒；
//! R8 复账一致；R9 指纹稳定；R10 呈现行；R11 对账留痕签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：缺陷账条目三态——悬挂（未转单未关单）、转单（已移交
//! 责任域）、关单（修复验收）；对账 ×1 = 全量逐条核对三态闭环，
//! 任一条悬挂即红；对账动作本身留签名可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0FA0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_STATE: u16 = SEG_BASE + 1; // 状态越出闭集
const E_HANG: u16 = SEG_BASE + 2; // 对账检出悬挂
const E_DUP: u16 = SEG_BASE + 3; // 缺陷号重复
const E_ID: u16 = SEG_BASE + 4; // 缺陷号非法
const E_COUNT: u16 = SEG_BASE + 5; // 对账计数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复账结果漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 对账签名不符
const E_NOACC: u16 = SEG_BASE + 10; // 对账未执行

/// 缺陷状态闭集。
pub const ST_HANG: u8 = 0; // 悬挂
pub const ST_TRANSFER: u8 = 1; // 转单
pub const ST_CLOSE: u8 = 2; // 关单

/// 本域缺陷账条目数。
pub const DEFECT_N: u32 = 12;

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

/// 一条缺陷账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefectRec {
    pub defect_id: u32,
    pub state: u8,   // ST_HANG / ST_TRANSFER / ST_CLOSE
    pub owner: u32,  // 责任域编号（转单/关单后有效，悬挂为 0）
    pub sig: u64,    // 登记签名
}

impl DefectRec {
    /// 登记签名：缺陷号 + 状态 + 责任域混签。
    pub fn make_sig(defect_id: u32, state: u8, owner: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&defect_id.to_le_bytes());
        buf.push(state);
        buf.extend_from_slice(&owner.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 合法留痕：签名一致 + 责任域口径对。
    pub fn sane(&self) -> Result<(), u16> {
        if self.state > ST_CLOSE {
            return Err(E_STATE);
        }
        let want_owner = if self.state == ST_HANG { 0 } else { self.owner };
        if self.state != ST_HANG && want_owner == 0 {
            return Err(E_STATE);
        }
        if self.sig != DefectRec::make_sig(self.defect_id, self.state, self.owner) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 缺陷账。
pub struct ReconLedger {
    pub recs: Vec<DefectRec>,
}

impl ReconLedger {
    pub fn new() -> ReconLedger {
        ReconLedger { recs: vec![] }
    }

    /// 登记：缺陷号 ≥1 且不重复。
    pub fn register(&mut self, defect_id: u32, state: u8, owner: u32) -> Result<(), u16> {
        if defect_id == 0 {
            return Err(E_ID);
        }
        if state > ST_CLOSE {
            return Err(E_STATE);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].defect_id == defect_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = DefectRec::make_sig(defect_id, state, owner);
        self.recs.push(DefectRec { defect_id, state, owner, sig });
        Ok(())
    }

    /// 转单：悬挂 → 转单。
    pub fn transfer(&mut self, defect_id: u32, owner: u32) -> Result<(), u16> {
        if owner == 0 {
            return Err(E_STATE);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].defect_id == defect_id {
                if self.recs[i].state != ST_HANG {
                    return Err(E_STATE);
                }
                self.recs[i].state = ST_TRANSFER;
                self.recs[i].owner = owner;
                self.recs[i].sig = DefectRec::make_sig(defect_id, ST_TRANSFER, owner);
                return Ok(());
            }
            i += 1;
        }
        Err(E_ID)
    }

    /// 关单：转单 → 关单。
    pub fn close(&mut self, defect_id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].defect_id == defect_id {
                if self.recs[i].state != ST_TRANSFER {
                    return Err(E_STATE);
                }
                let owner = self.recs[i].owner;
                self.recs[i].state = ST_CLOSE;
                self.recs[i].sig = DefectRec::make_sig(defect_id, ST_CLOSE, owner);
                return Ok(());
            }
            i += 1;
        }
        Err(E_ID)
    }

    /// 对账 ×1：全量扫描，零悬挂才算闭环。
    pub fn recon_once(&self) -> Result<(u32, u32, u32), u16> {
        let mut n_hang: u32 = 0;
        let mut n_transfer: u32 = 0;
        let mut n_close: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            self.recs[i].sane()?;
            if self.recs[i].state == ST_HANG {
                n_hang += 1;
            } else if self.recs[i].state == ST_TRANSFER {
                n_transfer += 1;
            } else {
                n_close += 1;
            }
            i += 1;
        }
        if n_hang > 0 {
            return Err(E_HANG);
        }
        Ok((n_transfer, n_close, n_transfer + n_close))
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].defect_id.to_le_bytes());
            buf.push(self.recs[i].state);
            buf.extend_from_slice(&self.recs[i].owner.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 对账留痕签名：条目数 + 转单数 + 关单数混签。
pub fn recon_sig(n_transfer: u32, n_close: u32, n_total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n_transfer.to_le_bytes());
    buf.extend_from_slice(&n_close.to_le_bytes());
    buf.extend_from_slice(&n_total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：t=<转单> c=<关单> hang=0。
pub fn recon_line(t: u32, c: u32) -> String {
    let mut out = String::new();
    out.push_str("t=");
    out.push_str(&t.to_string());
    out.push_str(" c=");
    out.push_str(&c.to_string());
    out.push_str(" hang=0");
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

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_06);

    // R1 账目登记：12 条全量入账（初始悬挂）。
    let mut led = ReconLedger::new();
    let mut id: u32 = 1;
    while id <= DEFECT_N {
        led.register(id * 10, ST_HANG, 0)?;
        id += 1;
    }
    if led.recs.len() as u32 != DEFECT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 转单落痕：前 5 条转责任域 701..705。
    let mut t: u32 = 0;
    while t < 5 {
        led.transfer((t + 1) * 10, 701 + t)?;
        t += 1;
    }
    hits += 1;

    // R3 关单落痕：其中 3 条修复关单。
    led.close(10)?;
    led.close(20)?;
    led.close(30)?;
    hits += 1;

    // R4 对账 ×1 全量扫描：零悬挂、计数守恒。
    let (n_t, n_c, total) = led.recon_once()?;
    if n_t != 2 || n_c != 3 || total != DEFECT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R5 悬挂检出：注入 1 条未转单 → 对账红。
    let mut bad = ReconLedger::new();
    bad.register(900, ST_HANG, 0)?;
    match bad.recon_once() {
        Err(E_HANG) => hits += 1,
        _ => return Err(E_HANG),
    }

    // R6 状态闭集：状态 3 越出闭集拒绝。
    let mut probe = ReconLedger::new();
    match probe.register(901, 3, 1) {
        Err(E_STATE) => hits += 1,
        _ => return Err(E_STATE),
    }

    // R7 计数守恒：重复登记拒绝 + 未知号转单拒绝。
    match led.register(10, ST_CLOSE, 1) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    match led.transfer(999, 1) {
        Err(E_ID) => hits += 1,
        _ => return Err(E_ID),
    }

    // R8 复账一致：再扫一次结果不变。
    let (n_t2, n_c2, _t2) = led.recon_once()?;
    if n_t2 != n_t || n_c2 != n_c {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：t=2 c=3 hang=0。
    let line = recon_line(n_t, n_c);
    if !has_kw(line.as_bytes(), b"t=2") || !has_kw(line.as_bytes(), b"c=3")
        || !has_kw(line.as_bytes(), b"hang=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 对账留痕签名：重算一致且非零。
    let want = recon_sig(n_t, n_c, total);
    if want != recon_sig(2, 3, DEFECT_N) || want == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // 收口：LCG 消费数入账，保证确定性路径全走。
    let _ = g.below(DEFECT_N);

    Ok(hits)
}
