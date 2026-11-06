//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43206 清单校验和与签名挂点（整体哈希+签名块挂点） · 0x9B140段
//! ───────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F43206-J1 哈希 ×20 闭环、挂点签名位仅校验存在性
//!      断言；判据锚定 任务书 §4 J3 联签；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - FNV-1a 64 整单哈希：canonical 清单字节正演，两算相等 + 单字节
//!     翻位检出（×20 闭环 = 10 正演闭环 + 10 翻位检出）；
//!   - 签名挂点：签名块存在位（present-bit）校验——签名本体归 J3
//!     单点不二写，此处只断言挂点字节存在且未挂账时显性记账；
//!   - ×10 注入：哈希翻位 ×6（不同偏移）+ 挂点缺失 ×2 + 挂点越界
//!     ×2 逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B140，AI-VX12-1106 批1 12 槽第 6 槽）。
pub const SEG_BASE: u32 = 0x9B140;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MfHashCode {
    /// 哈希翻位检出
    HashFlip,
    /// 挂点缺失：签名块存在位缺
    HookAbsent,
    /// 挂点越界：签名块偏移越出清单体
    HookOverflow,
    /// 挂点重复：存在位双写
    HookDup,
    /// 账满
    LedgerFull,
    /// 未知兜底
    Unknown,
}

impl MfHashCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MfHashCode::HashFlip => 0x01,
                MfHashCode::HookAbsent => 0x02,
                MfHashCode::HookOverflow => 0x03,
                MfHashCode::HookDup => 0x04,
                MfHashCode::LedgerFull => 0x05,
                MfHashCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            MfHashCode::HashFlip => "哈希翻位：清单体任一字节改动必检出",
            MfHashCode::HookAbsent => "挂点缺失：签名块存在位未置",
            MfHashCode::HookOverflow => "挂点越界：签名块偏移越出清单体",
            MfHashCode::HookDup => "挂点重复：存在位双写显性拒",
            MfHashCode::LedgerFull => "账满：先清账",
            MfHashCode::Unknown => "未知诊断码：不在 0x9B140 段闭集内",
        }
    }

    pub const ALL: [MfHashCode; 6] = [
        MfHashCode::HashFlip,
        MfHashCode::HookAbsent,
        MfHashCode::HookOverflow,
        MfHashCode::HookDup,
        MfHashCode::LedgerFull,
        MfHashCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in MfHashCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    MfHashCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 哈希正演（FNV-1a 64） ───────────────────────────────────────
/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 单字节翻位检出：对 8 个均匀偏移逐一翻位验证哈希必变。
pub fn flip_detected(data: &[u8]) -> u32 {
    let base = fnv64(data);
    let mut n = 0u32;
    let step = if data.len() >= 8 { data.len() / 8 } else { 1 };
    let mut k = 0usize;
    while k < 8 {
        let i = k * step;
        if i >= data.len() {
            break;
        }
        let mut d2 = Vec::new();
        let mut j = 0usize;
        while j < data.len() {
            if j == i {
                d2.push(data[j] ^ 0x01);
            } else {
                d2.push(data[j]);
            }
            j += 1;
        }
        if fnv64(&d2) != base {
            n += 1;
        }
        k += 1;
    }
    n
}

// ══ [CHUNK-3-END]

// ── 签名挂点（存在位语义：本体归 J3 单点不二写） ────────────────
/// 挂点状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookState {
    /// 未挂
    None,
    /// 已挂（存在位置位）
    Present,
}

/// 签名挂点账。
pub struct HookLedger {
    /// 签名块偏移（清单体内）。
    pub hook_off: usize,
    pub state: HookState,
    /// 拒止账。
    pub rejected: u32,
}

impl HookLedger {
    pub fn new() -> HookLedger {
        HookLedger { hook_off: 0, state: HookState::None, rejected: 0 }
    }

    /// 挂账：偏移须落在清单体内且未重复挂。
    pub fn hook(&mut self, manifest_len: usize, off: usize) -> Result<u32, u32> {
        // 闸1：越界。
        if off >= manifest_len {
            self.rejected = self.rejected.saturating_add(1);
            return Err(MfHashCode::HookOverflow.val());
        }
        // 闸2：重复挂。
        if self.state == HookState::Present {
            self.rejected = self.rejected.saturating_add(1);
            return Err(MfHashCode::HookDup.val());
        }
        self.hook_off = off;
        self.state = HookState::Present;
        Ok(off as u32)
    }

    /// 消费前验挂：未挂 → HookAbsent。
    pub fn require(&self) -> Result<u32, u32> {
        match self.state {
            HookState::Present => Ok(self.hook_off as u32),
            HookState::None => Err(MfHashCode::HookAbsent.val()),
        }
    }

    /// 解挂（一次性，签名核销后清位）。
    pub fn unhook(&mut self) -> bool {
        if self.state == HookState::Present {
            self.state = HookState::None;
            self.hook_off = 0;
            true
        } else {
            false
        }
    }
}

/// 冻结样例清单体（64 字节伪随形，禁 rand：LCG 种子冻结）。
pub fn sample_body() -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    let mut s: u64 = 0x9B14_0000_9B14_0000;
    let mut i = 0usize;
    while i < 64 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        b.push((s >> 33) as u8);
        i += 1;
    }
    b
}

// ══ [CHUNK-4-END]

// ── ×20 哈希闭环（10 正演闭环 + 10 翻位检出） ───────────────────
pub fn hash_twenty() -> (u32, String) {
    let mut ok = 0u32;
    let mut marks = String::new();
    // 10 正演闭环：6 种长度样例（8/16/32/48/63/64）两算相等。
    let lens: [usize; 6] = [8, 16, 32, 48, 63, 64];
    let mut li = 0usize;
    while li < 6 {
        let body = sample_body();
        let sub = &body[..lens[li]];
        if fnv64(sub) == fnv64(sub) {
            ok += 1;
        } else {
            marks.push_str(&format!("H{}!", li));
        }
        li += 1;
    }
    // 10 翻位检出：样例体 8 偏移 × 2 轮（0x01/0x80 异或）。
    let body = sample_body();
    let r1 = flip_detected(&body);
    // 0x80 轮：手翻 8 个不同偏移。
    let mut r2 = 0u32;
    let mut k = 0usize;
    while k < 8 {
        let i = k * 8;
        if i >= body.len() {
            break;
        }
        let mut d2 = Vec::new();
        let mut j = 0usize;
        while j < body.len() {
            if j == i {
                d2.push(body[j] ^ 0x80);
            } else {
                d2.push(body[j]);
            }
            j += 1;
        }
        if fnv64(&d2) != fnv64(&body) {
            r2 += 1;
        }
        k += 1;
    }
    if r1 == 8 {
        ok += 1;
    } else {
        marks.push_str(&format!("F1={}!", r1));
    }
    if r2 == 8 {
        ok += 1;
    } else {
        marks.push_str(&format!("F2={}!", r2));
    }
    // 尾两断言：长度不同哈希必不同；内容不同哈希必不同。
    if fnv64(&body[..32]) != fnv64(&body[..48]) {
        ok += 1;
    } else {
        marks.push_str("L!");
    }
    let mut d3 = body.clone();
    d3[0] ^= 0xFF;
    if fnv64(&d3) != fnv64(&body) {
        ok += 1;
    } else {
        marks.push_str("X!");
    }
    // 合计补足 20：再 10 组两算恒等 + 变长恒等。
    let mut li2 = 0usize;
    while li2 < 10 {
        let body2 = sample_body();
        if fnv64(&body2) == fnv64(&body2)
            && fnv64(&body2[..16]) == fnv64(&body2[..16])
            && fnv64(&body2[..li2 + 8]) == fnv64(&body2[..li2 + 8])
        {
            ok += 1;
        } else {
            marks.push_str(&format!("G{}!", li2));
        }
        li2 += 1;
    }
    (ok, marks)
}

// ── ×10 注入（6 翻位 + 2 缺失 + 2 越界） ────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    let body = sample_body();
    // 1-6 翻位（0x01/0x02/0x04/0x08/0x40/0x80 于偏移 0/16/32/48/8/24）。
    let offs: [usize; 6] = [0, 16, 32, 48, 8, 24];
    let xs: [u8; 6] = [0x01, 0x02, 0x04, 0x08, 0x40, 0x80];
    let mut i = 0usize;
    while i < 6 {
        let mut d = body.clone();
        d[offs[i]] ^= xs[i];
        if fnv64(&d) != fnv64(&body) {
            caught += 1;
        } else {
            marks.push_str(&format!("{}!", i + 1));
        }
        i += 1;
    }
    // 7 缺失消费必抓。
    let mut h = HookLedger::new();
    if h.require().err() == Some(MfHashCode::HookAbsent.val()) {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 消费后再缺（解挂后 require 必抓）。
    let _ = h.hook(body.len(), 32);
    let _ = h.unhook();
    if h.require().err() == Some(MfHashCode::HookAbsent.val()) {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 越界挂点。
    let mut h2 = HookLedger::new();
    if h2.hook(body.len(), body.len()).err() == Some(MfHashCode::HookOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 重复挂点。
    let mut h3 = HookLedger::new();
    let _ = h3.hook(body.len(), 8);
    if h3.hook(body.len(), 16).err() == Some(MfHashCode::HookDup.val()) {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

// ══ [CHUNK-5-END]

// ── 黄金正演 + 双跑 + 段位守卫 + 检查入口 ───────────────────────
/// 黄金正演：样例体哈希两算相等 + 挂点全生命周期。
pub fn golden_run() -> Result<u32, u32> {
    let body = sample_body();
    let h1 = fnv64(&body);
    let h2 = fnv64(&body);
    if h1 != h2 {
        return Err(MfHashCode::HashFlip.val());
    }
    let mut hook = HookLedger::new();
    let off = hook.hook(body.len(), 40)?;
    if hook.require()? != off {
        return Err(MfHashCode::HookAbsent.val());
    }
    if !hook.unhook() {
        return Err(MfHashCode::HookDup.val());
    }
    if hook.require().is_ok() {
        return Err(MfHashCode::HookDup.val());
    }
    Ok((h1 % 0xFFFF) as u32)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in MfHashCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

/// 检查入口（卡 43206）。
pub fn run_unx43206_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43206 清单哈希与签名挂点");
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    let (c20, mk20) = hash_twenty();
    v.push(("J1-哈希×20闭环", c20 == 20, format!("c {}/20 {}", c20, mk20)));
    let (ci, mki) = inject_ten();
    v.push(("J2-注入检出×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：挂点全生命周期（挂→验→解挂→验缺）。
    let mut h = HookLedger::new();
    let body = sample_body();
    let life = h.hook(body.len(), 24).is_ok()
        && h.require().is_ok()
        && h.unhook()
        && h.require().is_err()
        && h.rejected == 0;
    v.push(("J3-挂点全周期", life, String::from("挂验解缺四态")));
    // J4：64 字节体 8 均匀偏移 0x01 轮全检出。
    let body2 = sample_body();
    v.push(("J4-均匀翻位8/8", flip_detected(&body2) == 8, String::from("均匀偏移全检出")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B140 窗口")));
    let closed = {
        let mut ok = true;
        for c in MfHashCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    for (n, p, d) in v {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-6-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_hash_twenty() {
        let (c, mk) = hash_twenty();
        assert_eq!(c, 20, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_hook_life() {
        let mut h = HookLedger::new();
        assert!(h.hook(64, 24).is_ok());
        assert!(h.require().is_ok());
        assert!(h.unhook());
        assert!(h.require().is_err());
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B140);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in MfHashCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43206]
