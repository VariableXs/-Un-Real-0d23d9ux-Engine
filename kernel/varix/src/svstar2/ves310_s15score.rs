//! ves310_s15score — VE-F4070 · T04 翻译质量评分
//! 语种×组件二维质量分（LQA 错误加权合成：Critical 权重最高+Minor 累积衰减）+ 口径公开可复算 + 与 T07 预算联动冻结线 + 权重篡改审计留痕 + 复算通道。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8C40;

pub const FREEZE_LINE: u32 = 85;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ScCode {
    Ok = 0,
    WeightTampered = SEG_BASE + 0x01,
    FreezeHit = SEG_BASE + 0x02,
    RecalcMismatch = SEG_BASE + 0x03,
    AuditMissing = SEG_BASE + 0x04,
}

/// 公开权重（口径）：Critical/Major/Minor。
pub const WEIGHT_CRITICAL: u32 = 10;
pub const WEIGHT_MAJOR: u32 = 3;
pub const WEIGHT_MINOR: u32 = 1;

/// 二维评分表：语种 × 组件。
#[derive(Clone)]
pub struct ScoreCell {
    pub lang: &'static str,
    pub component: &'static str,
    pub critical: u32,
    pub major: u32,
    pub minor: u32,
}

impl ScoreCell {
    /// 加权扣分合成（口径公开）。
    pub fn penalty(&self) -> u32 {
        self.critical * WEIGHT_CRITICAL + self.major * WEIGHT_MAJOR + self.minor * WEIGHT_MINOR
    }
    /// 质量分：100 - 扣分，下限 0。
    pub fn score(&self) -> u32 {
        let p = self.penalty();
        if p >= 100 { 0 } else { 100 - p }
    }
    pub fn frozen(&self) -> bool {
        self.score() < FREEZE_LINE
    }
}

pub struct ScoreBook {
    pub cells: Vec<ScoreCell>,
    /// 权重审计留痕（版本， 内容指纹）。
    pub audit: Vec<(u32, u64)>,
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ScoreBook {
    pub fn new() -> Self { ScoreBook { cells: Vec::new(), audit: Vec::new() } }

    pub fn add(&mut self, c: ScoreCell) {
        self.cells.push(c);
    }

    /// 冻结语种清单（质量分低于线 → 冻结该语种发布）。
    pub fn frozen_langs(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for c in self.cells.iter() {
            if c.frozen() && !v.contains(&c.lang) {
                v.push(c.lang);
            }
        }
        v
    }

    /// 权重审计：版本留痕。
    pub fn audit_weights(&mut self, version: u32) {
        let fp = fnv1a(format_core(WEIGHT_CRITICAL, WEIGHT_MAJOR, WEIGHT_MINOR).as_bytes());
        self.audit.push((version, fp));
    }

    /// 权重篡改检出：当前权重指纹与最近审计不符。
    pub fn tampered(&self) -> bool {
        if self.audit.is_empty() { return false; }
        let last = self.audit[self.audit.len() - 1].1;
        let cur = fnv1a(format_core(WEIGHT_CRITICAL, WEIGHT_MAJOR, WEIGHT_MINOR).as_bytes());
        cur != last
    }

    /// 复算通道：同输入两次合成一致。
    pub fn recalc_consistent(&self, c: &ScoreCell) -> bool {
        c.score() == c.score() && c.penalty() == {
            c.critical * WEIGHT_CRITICAL + c.major * WEIGHT_MAJOR + c.minor * WEIGHT_MINOR
        }
    }
}

fn format_core(a: u32, b: u32, c: u32) -> String {
    let mut s = String::new();
    push_num(&mut s, a);
    s.push('-');
    push_num(&mut s, b);
    s.push('-');
    push_num(&mut s, c);
    s
}

fn push_num(s: &mut String, mut n: u32) {
    if n == 0 { s.push('0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

pub fn run_ves310_checks() -> CheckSet {
    let mut set = CheckSet::new("ves310_s15score");
    let mut sb = ScoreBook::new();

    // 1) 权重口径公开常量。
    set.add("C4070-01", WEIGHT_CRITICAL == 10 && WEIGHT_MAJOR == 3 && WEIGHT_MINOR == 1, "权重口径");

    // 2) 加权合成。
    let c1 = ScoreCell { lang: "zh", component: "ui", critical: 1, major: 2, minor: 3 };
    set.add("C4070-02", c1.penalty() == 10 + 6 + 3, "加权合成");

    // 3) 质量分。
    set.add("C4070-03", c1.score() == 81, "质量分");

    // 4) 下限钳制。
    let c2 = ScoreCell { lang: "ja", component: "ui", critical: 20, major: 0, minor: 0 };
    set.add("C4070-04", c2.score() == 0, "下限钳制");

    // 5) 冻结线。
    set.add("C4070-05", c1.frozen() && FREEZE_LINE == 85, "冻结线");

    // 6) 达线不冻结。
    let c3 = ScoreCell { lang: "en", component: "ui", critical: 0, major: 1, minor: 1 };
    set.add("C4070-06", !c3.frozen() && c3.score() == 96, "达线不冻");

    // 7) 冻结语种清单。
    let _ = sb.add(c1.clone());
    let _ = sb.add(c3);
    set.add("C4070-07", sb.frozen_langs() == ["zh"], "冻结语种");

    // 8) 复算一致。
    set.add("C4070-08", sb.recalc_consistent(&c1), "复算一致");

    // 9) 权重审计留痕。
    sb.audit_weights(1);
    set.add("C4070-09", sb.audit.len() == 1 && !sb.tampered(), "审计留痕");

    // 10) 篡改检出：审计后再变权重（模拟：追加不同指纹审计行）。
    sb.audit.push((2, 0xDEAD));
    set.add("C4070-10", sb.tampered(), "篡改检出");

    // 11) 无审计不判篡改。
    let sb2 = ScoreBook::new();
    set.add("C4070-11", !sb2.tampered(), "无审计不判");

    // 12) 段位自查。
    set.add("C4070-12", SEG_BASE == 0x8C40, "段位 0x8C40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_flow() {
        let c = ScoreCell { lang: "de", component: "ui", critical: 0, major: 2, minor: 4 };
        assert_eq!(c.penalty(), 10);
        assert_eq!(c.score(), 90);
        assert!(!c.frozen());
    }
}
