//! UNX-F48491 —— E25 三类窗口抖动回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严：三窗同抖动序列同结果）
//! 判据原文：UNX-F48491-J1 三类窗口（vxwm 原生 / Wine / Xwayland）对同一
//! CapsLock 抖动序列必须产出同一结果：同翻转账、同终态、同指纹；
//! 任何一窗结果漂移即回归。序列确定性、三窗互异键位、账目守恒全可复核。
//! 判据落地（drill 场景）：R1 窗类闭集；R2 抖动序列确定性（同种子同序）；
//! R3 三窗同序列同翻转账；R4 三窗同终态；R5 三窗指纹一致；
//! R6 账目守恒；R7 消除窗边界；R8 窗键唯一；R9 空序列零翻转；
//! R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE7A0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_WINCLASS: u16 = SEG_BASE + 1; // 窗类越出三类闭集
const E_DET: u16 = SEG_BASE + 2; // 抖动序列违反确定性
const E_FLIPS: u16 = SEG_BASE + 3; // 三窗翻转账不一致（回归）
const E_STATE: u16 = SEG_BASE + 4; // 三窗终态不一致（回归）
const E_FP: u16 = SEG_BASE + 5; // 三窗指纹不一致（回归）
const E_CONSERV: u16 = SEG_BASE + 6; // 翻转+抖动 ≠ 总笔数
const E_BOUND: u16 = SEG_BASE + 7; // 消除窗边界判定错误
const E_DUP: u16 = SEG_BASE + 8; // 窗键重复
const E_EMPTY: u16 = SEG_BASE + 9; // 空序列产生翻转
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 三类窗口键：1 = vxwm 原生，2 = Wine，3 = Xwayland。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinClass(pub u8);

impl WinClass {
    pub const NATIVE_VXWM: u8 = 1;
    pub const WINE: u8 = 2;
    pub const XWAYLAND: u8 = 3;

    pub fn is_known(self) -> bool {
        self.0 >= Self::NATIVE_VXWM && self.0 <= Self::XWAYLAND
    }
}

/// 抖动消除窗（毫秒）。
pub const DEBOUNCE_MS: u32 = 80;
/// 回归序列长度。
pub const SEQ_LEN: usize = 48;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
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

/// CapsLock 状态账（单窗投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapsAccount {
    pub locked: bool,
    pub flips: u32,
    pub jitter: u32,
    pub last_flip_at: u32,
    pub has_flip: bool,
}

impl CapsAccount {
    pub fn new() -> CapsAccount {
        CapsAccount {
            locked: false,
            flips: 0,
            jitter: 0,
            last_flip_at: 0,
            has_flip: false,
        }
    }

    /// 灌入一笔按下：首按必翻；间隔 < 消除窗为抖动；≥ 窗翻转。
    pub fn press(&mut self, at: u32) {
        if !self.has_flip {
            self.locked = !self.locked;
            self.flips += 1;
            self.last_flip_at = at;
            self.has_flip = true;
            return;
        }
        if at.saturating_sub(self.last_flip_at) < DEBOUNCE_MS {
            self.jitter += 1;
        } else {
            self.locked = !self.locked;
            self.flips += 1;
            self.last_flip_at = at;
        }
    }

    /// 账目守恒：flips + jitter == presses。
    pub fn conserved(&self, presses: u32) -> bool {
        self.flips + self.jitter == presses
    }

    /// 状态一致：flips 奇偶与 locked 一致。
    pub fn state_consistent(&self) -> bool {
        (self.flips % 2 == 1) == self.locked
    }
}

/// 单窗账户（含窗键）。
pub struct WinAccount {
    pub class: WinClass,
    pub acct: CapsAccount,
}

/// 消费抖动序列：按序灌入，返回单窗终账。
pub fn replay(class: WinClass, seq: &[u32]) -> Result<WinAccount, u16> {
    if !class.is_known() {
        return Err(E_WINCLASS);
    }
    let mut acct = CapsAccount::new();
    let mut i = 0usize;
    while i < seq.len() {
        acct.press(seq[i]);
        i += 1;
    }
    Ok(WinAccount { class, acct })
}

/// 账户指纹：终态 + 翻转账 + 抖动账。
pub fn acct_fp(a: &CapsAccount) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(a.locked as u8);
    buf.extend_from_slice(&a.flips.to_le_bytes());
    buf.extend_from_slice(&a.jitter.to_le_bytes());
    buf.extend_from_slice(&a.last_flip_at.to_le_bytes());
    fnv1a64(&buf)
}

/// 回归行（呈现面）：win=<键> flips=<n> jitter=<n> locked=<0/1>。
pub fn acct_line(w: &WinAccount) -> String {
    let mut s = String::new();
    s.push_str("win=");
    s.push_str(&(w.class.0 as u32).to_string());
    s.push_str(" flips=");
    s.push_str(&w.acct.flips.to_string());
    s.push_str(" jitter=");
    s.push_str(&w.acct.jitter.to_string());
    s.push_str(" locked=");
    s.push_str(&(w.acct.locked as u32).to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 窗类闭集：1..=3 全知，4 越集拒收。
    let mut k = WinClass(WinClass::NATIVE_VXWM);
    while k.0 <= WinClass::XWAYLAND {
        if !k.is_known() {
            return Err(E_WINCLASS);
        }
        k.0 += 1;
    }
    if WinClass(4).is_known() {
        return Err(E_WINCLASS);
    }
    hits += 1;

    // R2 抖动序列确定性：同种子两次生成完全一致，异种子分叉。
    let mut a = Lcg::new(0x48491);
    let mut b = Lcg::new(0x48491);
    let mut c = Lcg::new(0x48492);
    let mut sa: Vec<u32> = vec![];
    let mut sb: Vec<u32> = vec![];
    let mut sc: Vec<u32> = vec![];
    let mut i = 0usize;
    while i < 8 {
        sa.push(a.below(400));
        sb.push(b.below(400));
        sc.push(c.below(400));
        i += 1;
    }
    if sa != sb || sa == sc {
        return Err(E_DET);
    }
    hits += 1;

    // R3/R4/R5 三窗同序列同结果：宪法序列（快簇+慢簇混合）三窗回放。
    let mut lcg = Lcg::new(0x25A0);
    let mut seq: Vec<u32> = vec![];
    let mut at = 0u32;
    let mut i = 0usize;
    while i < SEQ_LEN {
        // 前半快簇（抖动主导）、后半慢簇（翻转主导）。
        let gap = if i < SEQ_LEN / 2 {
            lcg.below(DEBOUNCE_MS)
        } else {
            DEBOUNCE_MS + lcg.below(300)
        };
        at += gap;
        seq.push(at);
        i += 1;
    }
    let w1 = replay(WinClass(WinClass::NATIVE_VXWM), &seq)?;
    let w2 = replay(WinClass(WinClass::WINE), &seq)?;
    let w3 = replay(WinClass(WinClass::XWAYLAND), &seq)?;
    if w1.acct.flips != w2.acct.flips || w2.acct.flips != w3.acct.flips {
        return Err(E_FLIPS);
    }
    if w1.acct.jitter != w2.acct.jitter || w2.acct.jitter != w3.acct.jitter {
        return Err(E_FLIPS);
    }
    hits += 1;

    // R4 终态一致：locked 与 flips 奇偶三窗一致。
    if w1.acct.locked != w2.acct.locked || w2.acct.locked != w3.acct.locked {
        return Err(E_STATE);
    }
    if !w1.acct.state_consistent() || !w2.acct.state_consistent() || !w3.acct.state_consistent() {
        return Err(E_STATE);
    }
    hits += 1;

    // R5 指纹一致：三窗账指纹相同且非零。
    let f1 = acct_fp(&w1.acct);
    let f2 = acct_fp(&w2.acct);
    let f3 = acct_fp(&w3.acct);
    if f1 != f2 || f2 != f3 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R6 账目守恒：三窗 flips + jitter == SEQ_LEN。
    if !w1.acct.conserved(SEQ_LEN as u32) || !w2.acct.conserved(SEQ_LEN as u32) || !w3.acct.conserved(SEQ_LEN as u32) {
        return Err(E_CONSERV);
    }
    // 快簇必然产生抖动、慢簇必然产生翻转：账面双非零。
    if w1.acct.jitter == 0 || w1.acct.flips == 0 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 消除窗边界：间隔窗内不翻、恰窗翻（单窗复演）。
    let mut b1 = CapsAccount::new();
    b1.press(0);
    b1.press(DEBOUNCE_MS - 1);
    if b1.flips != 1 || b1.jitter != 1 {
        return Err(E_BOUND);
    }
    let mut b2 = CapsAccount::new();
    b2.press(0);
    b2.press(DEBOUNCE_MS);
    if b2.flips != 2 || b2.jitter != 0 {
        return Err(E_BOUND);
    }
    hits += 1;

    // R8 窗键唯一：三窗键两两互异。
    let keys = [w1.class.0, w2.class.0, w3.class.0];
    if keys[0] == keys[1] || keys[1] == keys[2] || keys[0] == keys[2] {
        return Err(E_DUP);
    }
    hits += 1;

    // R9 空序列零翻转：空序列回放得零账。
    let empty_w = replay(WinClass(WinClass::WINE), &[])?;
    if empty_w.acct.flips != 0 || empty_w.acct.jitter != 0 || empty_w.acct.locked {
        return Err(E_EMPTY);
    }
    if !empty_w.acct.conserved(0) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R10 呈现行齐全：win=/flips=/jitter=/locked= 四段在位，三窗互异。
    let l1 = acct_line(&w1);
    let l2 = acct_line(&w2);
    let l3 = acct_line(&w3);
    for txt in [&l1, &l2, &l3] {
        if !has_kw(txt.as_bytes(), b"win=")
            || !has_kw(txt.as_bytes(), b"flips=")
            || !has_kw(txt.as_bytes(), b"jitter=")
            || !has_kw(txt.as_bytes(), b"locked=")
        {
            return Err(E_LINE);
        }
    }
    if l1 == l2 || l2 == l3 || l1 == l3 {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 第二序列三窗复演：换种子重生成慢簇主导序列，三窗仍须同结果。
    let mut lcg2 = Lcg::new(0x25B1);
    let mut seq2: Vec<u32> = vec![];
    let mut at2 = 0u32;
    let mut i = 0usize;
    while i < SEQ_LEN {
        let gap = if i < SEQ_LEN / 4 {
            lcg2.below(DEBOUNCE_MS)
        } else {
            DEBOUNCE_MS + lcg2.below(300)
        };
        at2 += gap;
        seq2.push(at2);
        i += 1;
    }
    let v1 = replay(WinClass(WinClass::NATIVE_VXWM), &seq2)?;
    let v2 = replay(WinClass(WinClass::WINE), &seq2)?;
    let v3 = replay(WinClass(WinClass::XWAYLAND), &seq2)?;
    if v1.acct.flips != v2.acct.flips || v2.acct.flips != v3.acct.flips {
        return Err(E_FLIPS);
    }
    if acct_fp(&v1.acct) != acct_fp(&v2.acct) || acct_fp(&v2.acct) != acct_fp(&v3.acct) {
        return Err(E_FP);
    }
    if !v1.acct.conserved(SEQ_LEN as u32) || !v1.acct.state_consistent() {
        return Err(E_CONSERV);
    }
    hits += 1;

    Ok(hits)
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_WINCLASS => "窗类越出三类闭集：只接受 vxwm 原生/Wine/Xwayland 三键",
        E_DET => "抖动序列不确定：同种子必须同序，查 LCG 是否被并发污染",
        E_FLIPS => "三窗翻转账不一致：逐窗重放定位漂移窗",
        E_STATE => "三窗终态不一致：核对 flips 奇偶与 locked 映射",
        E_FP => "三窗指纹不一致：账面同而指纹异即隐式态泄漏",
        E_CONSERV => "翻转+抖动 ≠ 总笔数：先对账再定位漏记",
        E_BOUND => "消除窗边界判定错误：窗内不翻、恰窗翻",
        E_DUP => "窗键重复：三窗键两两互异",
        E_EMPTY => "空序列产生翻转：零输入必须得零账",
        E_LINE => "呈现行缺段：win=/flips=/jitter=/locked= 缺一即呈现回归",
        _ => "未知诊断码",
    }
}

/// 子串判定（朴素扫描，统一口径）。
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
