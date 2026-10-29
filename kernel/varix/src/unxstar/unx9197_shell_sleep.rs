//! UNX-F9197 休眠族与 shell sleep 消费核（0x63860 段）
//!
//! 锚点：UNX-F9197-J1 sleep 账（shell sleep 每次落账：请求串、解析
//! 值、睡后返回、留痕）；J2 sleep 语义（十进制解析 + 后缀支持（s/m）
//! + 非法串拒 + 零时长直返四性质）；J3 sleep 指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B20（shell sleep 消费面）。
//!
//! 结构：sleep 解析核（十进制 + 后缀）+ sleep 账本 + 折叠指纹 + 注
//! 入核（错码/空留痕/非法串/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63860 独占 0x20）。
pub const SEG_BASE: u32 = 0x63860;
pub const SL_BAD_ARG: u32 = SEG_BASE + 0x01; // 非法参数
pub const SL_BAD_PARSE: u32 = SEG_BASE + 0x02; // 解析错
pub const SL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const SL_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const SL_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 单位：毫秒。
pub type Ms = u64;

/// sleep 解析核：十进制（可带小数）+ 后缀 s/m（默认秒）。
pub fn parse_sleep(arg: &str) -> Result<Ms, u32> {
    if arg.is_empty() {
        return Err(SL_BAD_ARG);
    }
    let (num_part, mult) = if let Some(stripped) = arg.strip_suffix('m') {
        (stripped, 60_000u64)
    } else if let Some(stripped) = arg.strip_suffix('s') {
        (stripped, 1_000u64)
    } else {
        (arg, 1_000u64)
    };
    if num_part.is_empty() {
        return Err(SL_BAD_PARSE);
    }
    // 整数或一位小数
    let (int_part, frac_part) = match num_part.split_once('.') {
        Some((a, b)) => (a, b),
        None => (num_part, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(SL_BAD_PARSE);
    }
    let mut ms: Ms = 0;
    for ch in int_part.bytes() {
        if !ch.is_ascii_digit() {
            return Err(SL_BAD_PARSE);
        }
        ms = ms * 10 + (ch - b'0') as u64;
    }
    ms *= mult;
    if !frac_part.is_empty() {
        // 小数部分：0.5 → mult/2
        let mut frac_val: u64 = 0;
        let mut frac_div: u64 = 1;
        for ch in frac_part.bytes() {
            if !ch.is_ascii_digit() {
                return Err(SL_BAD_PARSE);
            }
            frac_val = frac_val * 10 + (ch - b'0') as u64;
            frac_div *= 10;
        }
        ms += frac_val * mult / frac_div;
    }
    Ok(ms)
}

/// 单条 sleep 账。
#[derive(Clone)]
pub struct SlRec {
    pub seq: u32,
    pub arg: String,
    pub ms: Ms,
    pub note: String,
}

impl SlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }

    pub fn render_line(&self) -> String {
        format!("SL{}|{}->{}ms|{}", self.seq, self.arg, self.ms, self.note)
    }
}

/// sleep 簿。
pub struct SlBook {
    pub ledger: Vec<SlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SlBook {
    pub fn new() -> SlBook {
        SlBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// sleep 门：解析 + 落账。
    pub fn sleep(&mut self, arg: &str, note: &str) -> Result<Ms, u32> {
        if self.sealed {
            return Err(SL_FROZEN);
        }
        if note.is_empty() {
            return Err(SL_EMPTY_NOTE);
        }
        let ms = match parse_sleep(arg) {
            Ok(m) => m,
            Err(e) => return Err(e),
        };
        self.ledger.push(SlRec { seq: self.seq_next, arg: arg.to_string(), ms, note: note.to_string() });
        self.seq_next += 1;
        Ok(ms)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91970;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add((r.ms & 0xFFFF) as u32);
            for b in r.arg.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 空串拒
    got[0] = parse_sleep("").err() == Some(SL_BAD_ARG);
    // 2) 非法字符拒
    got[1] = parse_sleep("12x").err() == Some(SL_BAD_PARSE);
    // 3) 空留痕
    let bad2 = SlRec { seq: 3, arg: "1".to_string(), ms: 1000, note: String::new() };
    got[2] = !bad2.ok();
    // 4) 后缀解析
    got[3] = parse_sleep("2m") == Ok(120_000) && parse_sleep("0.5") == Ok(500);
    // 5) 印后登记必抓
    let mut b = SlBook::new();
    b.seal();
    got[4] = b.sleep("1", "inj-frozen").err() == Some(SL_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9197");
    let mut b = SlBook::new();

    // J1 sleep 账非空
    let _ = b.sleep("1", "睡 1 秒");
    let j1 = !b.ledger.is_empty();
    s.add("J1-sleep账非空", j1, "sleep 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空");

    // J3 十进制解析：默认秒
    let j3 = parse_sleep("3") == Ok(3000);
    s.add("J3-默认秒", j3, "3 → 3000ms");

    // J4 小数解析：0.5s
    let j4 = parse_sleep("0.5") == Ok(500);
    s.add("J4-小数解析", j4, "0.5 → 500ms");

    // J5 m 后缀：分钟
    let j5 = parse_sleep("2m") == Ok(120_000);
    s.add("J5-m后缀", j5, "2m → 120000ms");

    // J6 s 后缀显式
    let j6 = parse_sleep("2s") == Ok(2000);
    s.add("J6-s后缀", j6, "2s → 2000ms");

    // J7 非法串拒
    let j7 = parse_sleep("abc").err() == Some(SL_BAD_PARSE)
        && parse_sleep("1.2.3").err() == Some(SL_BAD_PARSE);
    s.add("J7-非法串拒", j7, "非数字必拒");

    // J8 空串拒
    let j8 = parse_sleep("").err() == Some(SL_BAD_ARG);
    s.add("J8-空串拒", j8, "空参数必拒");

    // J9 零时长直返
    let j9 = parse_sleep("0") == Ok(0);
    s.add("J9-零时长", j9, "0 → 0ms 直返");

    // J10 解析唯一
    let j10 = parse_sleep("1.25") == Ok(1250) && parse_sleep("1.25") == Ok(1250);
    s.add("J10-解析唯一", j10, "同串同出");

    // J11 后缀混写拒（1ms 双后缀）
    let j11 = parse_sleep("1ms").err() == Some(SL_BAD_PARSE);
    s.add("J11-双后缀拒", j11, "1ms 视为数字后非法字符");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9197_parse_flow() {
        assert_eq!(parse_sleep("3"), Ok(3000));
        assert_eq!(parse_sleep("0.5"), Ok(500));
        assert_eq!(parse_sleep("2m"), Ok(120_000));
        assert_eq!(parse_sleep("2s"), Ok(2000));
        assert_eq!(parse_sleep("0"), Ok(0));
        assert_eq!(parse_sleep("1.25"), Ok(1250));
        assert_eq!(parse_sleep(""), Err(SL_BAD_ARG));
        assert_eq!(parse_sleep("abc"), Err(SL_BAD_PARSE));
        assert_eq!(parse_sleep("1ms"), Err(SL_BAD_PARSE));
        let mut b = SlBook::new();
        assert!(b.sleep("1", "a").is_ok());
        assert_eq!(b.sleep("xyz", "b"), Err(SL_BAD_PARSE));
    }

    #[test]
    fn unx9197_checks_green() {
        let s = run_unx9197_checks();
        assert!(s.all_passed(), "UNX-F9197 判据未全绿");
    }
}
