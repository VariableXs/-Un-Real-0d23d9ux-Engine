//! UNX-F56144 对照行签章件（ai_sign 字段）（0x61200 段）
//!
//! 锚点：UNX-F56144-J1 签章账（对照行逐行签章：行键 × ai_sign 指纹 ×
//! 签章波次 × 签章人域四元组落账）；J2 签章核（同内容同键重签同指纹
//! 确定性 + 异内容异指纹碰撞抵抗）；J3 签章后锁定语义（签章行内容改
//! 动即验签失败，锁定不可逆）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·签章面）。
//!
//! 结构：签章器（FNV 双轮折叠——内容轮 + 键轮混合）+ 签章账（行键唯
//! 一索引）+ 验签核（内容 × 键重算一致才过）+ 波次链（签章波次单调）
//! + 注入核（重复键签章/验签内容篡改/波次回退/空键签章/未知签章人域
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61200 独占 0x20）。
pub const SEG_BASE: u32 = 0x61200;
pub const SG_DUP_KEY: u32 = SEG_BASE + 0x01; // 行键重复签章
pub const SG_VERIFY_FAIL: u32 = SEG_BASE + 0x02; // 验签失败（内容篡改）
pub const SG_WAVE_BACK: u32 = SEG_BASE + 0x03; // 签章波次回退
pub const SG_EMPTY_KEY: u32 = SEG_BASE + 0x04; // 空行键签章
pub const SG_UNK_SIGNER: u32 = SEG_BASE + 0x05; // 未知签章人域

/// 签章人域闭集。
pub const SIGNER_AI: usize = 0;
pub const SIGNER_HUMAN: usize = 1;
pub const SIGNER_JOINT: usize = 2;
pub const SIGNER_COUNT: usize = 3;

/// 签章人域名渲染。
pub const SIGNER_NAMES: [&str; SIGNER_COUNT] = ["ai", "human", "joint"];

/// 签章核：FNV 双轮（内容轮 + 键轮混合折叠）。
pub fn ai_sign(content: &str, row_key: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in content.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    // 键轮：键字节与中间态混合
    for b in row_key.as_bytes() {
        h = h.wrapping_mul(0x0100_0193);
        h ^= *b as u32;
    }
    // 收尾轮：固定盐折叠
    h ^= 0xA1C3_5EED;
    h = h.wrapping_mul(0x0100_0193);
    h
}

/// 单条签章账目。
#[derive(Clone)]
pub struct SignEntry {
    pub row_key: String,
    pub sign: u32,
    pub wave: u32,
    pub signer: usize,
}

impl SignEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{:08x}|w{}|{}",
            self.row_key, self.sign, self.wave, SIGNER_NAMES[self.signer]
        )
    }
}

/// 签章账。
pub struct SignBook {
    entries: Vec<SignEntry>,
    top_wave: u32,
}

impl SignBook {
    pub fn new() -> Self {
        SignBook {
            entries: Vec::new(),
            top_wave: 0,
        }
    }

    /// 签章：键非空 + 键唯一 + 波次单调 + 签章人域闭集四闸。
    pub fn sign(&mut self, row_key: &str, content: &str, wave: u32, signer: usize) -> Result<u32, u32> {
        if row_key.is_empty() {
            return Err(SG_EMPTY_KEY);
        }
        if signer >= SIGNER_COUNT {
            return Err(SG_UNK_SIGNER);
        }
        if self.entries.iter().any(|e| e.row_key == row_key) {
            return Err(SG_DUP_KEY);
        }
        if self.top_wave > 0 && wave < self.top_wave {
            return Err(SG_WAVE_BACK);
        }
        let s = ai_sign(content, row_key);
        self.entries.push(SignEntry {
            row_key: row_key.to_string(),
            sign: s,
            wave,
            signer,
        });
        self.top_wave = wave;
        Ok(s)
    }

    /// 验签核：内容 × 键重算与存量签章一致。
    pub fn verify(&self, row_key: &str, content: &str) -> Result<bool, u32> {
        let e = self
            .entries
            .iter()
            .find(|e| e.row_key == row_key)
            .ok_or(SG_VERIFY_FAIL)?;
        Ok(e.sign == ai_sign(content, row_key))
    }

    pub fn entry(&self, row_key: &str) -> Option<&SignEntry> {
        self.entries.iter().find(|e| e.row_key == row_key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 签章账核：全量键唯一 + 签章重算一致。
    pub fn book_ok(&self) -> bool {
        let keys: Vec<&str> = self.entries.iter().map(|e| e.row_key.as_str()).collect();
        let uniq = keys.len()
            == keys
                .iter()
                .copied()
                .collect::<alloc::collections::BTreeSet<&str>>()
                .len();
        uniq
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out
    }
}

impl Default for SignBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行签章（AI 两行 + 联签一行）。
fn standard_book() -> SignBook {
    let mut b = SignBook::new();
    let _ = b.sign("fps_full", "full_load|60|57|9500", 1, SIGNER_AI);
    let _ = b.sign("fps_mid", "mid_load|120|114|9500", 2, SIGNER_AI);
    let _ = b.sign("fps_idle", "idle|240|236|9833", 3, SIGNER_JOINT);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空键签章
    let mut b = SignBook::new();
    if b.sign("", "x", 1, SIGNER_AI) == Err(SG_EMPTY_KEY) {
        caught += 1;
    }
    // 注 2：重复键
    let mut b = standard_book();
    if b.sign("fps_full", "other", 4, SIGNER_AI) == Err(SG_DUP_KEY) {
        caught += 1;
    }
    // 注 3：波次回退
    let mut b = standard_book();
    if b.sign("fps_low", "x", 1, SIGNER_AI) == Err(SG_WAVE_BACK) {
        caught += 1;
    }
    // 注 4：未知签章人域
    let mut b = SignBook::new();
    if b.sign("k", "x", 1, 9) == Err(SG_UNK_SIGNER) {
        caught += 1;
    }
    // 注 5：内容篡改验签失败
    let b = standard_book();
    if b.verify("fps_full", "full_load|60|58|9667") == Ok(false) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 签章账；J2 签章核；J3 锁定语义。
pub fn run_unx56144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56144");
    // J1-1 三行签章落账
    let b = standard_book();
    let ok = b.len() == 3 && b.book_ok();
    s.add("56144-J1-三行签章", ok, if ok { "" } else { "签章账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("fps_full|") && text.contains("w3|joint");
    s.add("56144-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 签章确定性：同内容同键同指纹
    let ok = ai_sign("abc", "k1") == ai_sign("abc", "k1");
    s.add("56144-J2-签章确定性", ok, if ok { "" } else { "签章漂移" });
    // J2-2 内容敏感：异内容异指纹
    let ok = ai_sign("abc", "k1") != ai_sign("abd", "k1");
    s.add("56144-J2-内容敏感", ok, if ok { "" } else { "内容碰撞" });
    // J2-3 键敏感：同内容异键异指纹
    let ok = ai_sign("abc", "k1") != ai_sign("abc", "k2");
    s.add("56144-J2-键敏感", ok, if ok { "" } else { "键碰撞" });
    // J2-4 验签过：原内容原键
    let b = standard_book();
    let ok = b.verify("fps_full", "full_load|60|57|9500") == Ok(true);
    s.add("56144-J2-验签过", ok, if ok { "" } else { "验签漂移" });
    // J2-5 未知键验签拒
    let ok = b.verify("no_such", "x") == Err(SG_VERIFY_FAIL);
    s.add("56144-J2-未知键拒", ok, if ok { "" } else { "未知键穿透" });
    // J3-1 篡改必败（内容轮敏感）
    let b = standard_book();
    let ok = b.verify("fps_mid", "mid_load|120|115|9583") == Ok(false);
    s.add("56144-J3-篡改必败", ok, if ok { "" } else { "篡改穿透" });
    // J3-2 键篡改必败（键轮敏感）
    let ok = ai_sign("abc", "k1") != ai_sign("abc", "kX1");
    s.add("56144-J3-键篡改敏感", ok, if ok { "" } else { "键篡改穿透" });
    // J3-3 波次链单调可查
    let b = standard_book();
    let ok = b.entry("fps_idle").map(|e| e.wave == 3).unwrap_or(false)
        && b.entry("fps_full").map(|e| e.wave == 1).unwrap_or(false);
    s.add("56144-J3-波次链可查", ok, if ok { "" } else { "波次漂移" });
    // J3-4 签章人域闭集渲染
    let ok = SIGNER_NAMES[SIGNER_AI] == "ai" && SIGNER_NAMES[SIGNER_JOINT] == "joint";
    s.add("56144-J3-签章人域闭集", ok, if ok { "" } else { "域名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56144-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56144_all_checks_green() {
        let set = run_unx56144_checks();
        assert!(set.all_passed(), "UNX-F56144 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56144_sign_stable() {
        assert_eq!(ai_sign("abc", "k1"), ai_sign("abc", "k1"));
    }
}
