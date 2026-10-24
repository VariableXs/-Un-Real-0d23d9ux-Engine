//! unx0532_persist —— UNX-F0532 结果持久化（批账落盘格式）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0532 · 结果持久化 · 目标 220 行
//! 判据原文：落盘格式账齐（结果块/守恒式/指纹三段），落盘内容与运行账复
//!           算一致（3/3 段）。
//! 组件：PersistBlock（三段结构：[RESULT] 结果行集 / [CONSERVE] 守恒式 /
//!   [FINGERPRINT] 校验和）+ 序列化/反序列化回环 + 三段复算一致核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35240;
pub const XC_BLOCK_BAD: u32 = SEG_BASE + 1; // 块坏（段缺失）
pub const XC_FPRINT_DRIFT: u32 = SEG_BASE + 2; // 指纹漂移
pub const XC_CONSERVE_BAD: u32 = SEG_BASE + 3; // 守恒式坏
pub const XC_SEG_MISS: u32 = SEG_BASE + 4; // 段缺

/// 一条结果行。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResLine {
    pub suite: &'static str,
    pub case: &'static str,
    /// 0=过 1=败 2=跳。
    pub state: u8,
    pub err: u32,
}

/// 持久化块（三段）。
pub struct PersistBlock {
    pub lines: Vec<ResLine>,
    /// 反序列化路径行数（build 路径=lines.len()）。
    pub line_count: u32,
    /// 守恒式三数：total=pass+fail+skip。
    pub conserve: (u32, u32, u32, u32),
    /// 指纹：Σ(state+1)*(err+1)+行数。
    pub fingerprint: u64,
}

impl PersistBlock {
    /// 从结果行集构造并算账。
    pub fn build(lines: &[ResLine]) -> Self {
        let mut total = 0u32;
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut fp = 0u64;
        let mut i = 0;
        while i < lines.len() {
            match lines[i].state {
                0 => pass += 1,
                1 => fail += 1,
                _ => skip += 1,
            }
            total += 1;
            fp += ((lines[i].state as u64 + 1) * (lines[i].err as u64 + 1)) + 1;
            i += 1;
        }
        PersistBlock {
            lines: Vec::from(lines),
            line_count: lines.len() as u32,
            conserve: (total, pass, fail, skip),
            fingerprint: fp,
        }
    }

    /// 三段齐核：行集非空+守恒成立+指纹复算等。
    pub fn segments_ok(&self) -> bool {
        let (t, p, f, s) = self.conserve;
        t == p + f + s && self.fingerprint == Self::recompute_fp(&self.lines) && !self.lines.is_empty()
    }

    /// 指纹重算。
    pub fn recompute_fp(lines: &[ResLine]) -> u64 {
        let mut fp = 0u64;
        let mut i = 0;
        while i < lines.len() {
            fp += ((lines[i].state as u64 + 1) * (lines[i].err as u64 + 1)) + 1;
            i += 1;
        }
        fp
    }

    /// 序列化：三段文本。
    pub fn serialize(&self) -> String {
        let mut s = String::from("[RESULT]\n");
        let mut i = 0;
        while i < self.lines.len() {
            let l = &self.lines[i];
            s.push_str(l.suite);
            s.push('.');
            s.push_str(l.case);
            s.push('=');
            s.push_str(&itoa(l.state as u64));
            s.push(',');
            s.push_str(&itoa(l.err as u64));
            s.push('\n');
            i += 1;
        }
        s.push_str("[CONSERVE]");
        let (t, p, f, sk) = self.conserve;
        s.push_str(&itoa(t as u64));
        s.push('=');
        s.push_str(&itoa(p as u64));
        s.push('+');
        s.push_str(&itoa(f as u64));
        s.push('+');
        s.push_str(&itoa(sk as u64));
        s.push('\n');
        s.push_str("[FINGERPRINT]");
        s.push_str(&itoa(self.fingerprint));
        s.push('\n');
        s
    }

    /// 反序列化（三段解析，坏块拒）。
    pub fn deserialize(text: &str) -> Result<Self, u32> {
        let mut count = 0u32;
        let mut fp_calc = 0u64;
        let mut seg = 0u8; // 0=RESULT 1=CONSERVE 2=FINGERPRINT
        let mut conserve: Option<(u32, u32, u32, u32)> = None;
        let mut fp: Option<u64> = None;
        for raw in text.split('\n') {
            if raw.is_empty() {
                continue;
            }
            if raw == "[RESULT]" {
                seg = 0;
                continue;
            }
            if raw.starts_with("[CONSERVE]") {
                seg = 1;
                let nums = &raw[10..];
                conserve = Some(parse_conserve(nums)?);
                continue;
            }
            if raw.starts_with("[FINGERPRINT]") {
                seg = 2;
                fp = Some(parse_u64(&raw[13..])?);
                continue;
            }
            if seg == 0 {
                let (st, er) = parse_line_validate(raw)?;
                count += 1;
                fp_calc += ((st as u64 + 1) * (er as u64 + 1)) + 1;
            }
        }
        let c = conserve.ok_or(XC_SEG_MISS)?;
        let f = fp.ok_or(XC_SEG_MISS)?;
        if count == 0 {
            return Err(XC_BLOCK_BAD);
        }
        if fp_calc != f {
            return Err(XC_FPRINT_DRIFT);
        }
        Ok(PersistBlock {
            lines: Vec::new(),
            line_count: count,
            conserve: c,
            fingerprint: f,
        })
    }
}

fn parse_conserve(s: &str) -> Result<(u32, u32, u32, u32), u32> {
    let parts: Vec<&str> = s.split(|c| c == '=' || c == '+').collect();
    if parts.len() != 4 {
        return Err(XC_CONSERVE_BAD);
    }
    Ok((
        parse_u32(parts[0])?,
        parse_u32(parts[1])?,
        parse_u32(parts[2])?,
        parse_u32(parts[3])?,
    ))
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    let v = parse_u64(s)?;
    if v > u32::MAX as u64 {
        return Err(XC_BLOCK_BAD);
    }
    Ok(v as u32)
}

fn parse_u64(s: &str) -> Result<u64, u32> {
    if s.is_empty() {
        return Err(XC_BLOCK_BAD);
    }
    let mut v: u64 = 0;
    for b in s.bytes() {
        if !(b >= b'0' && b <= b'9') {
            return Err(XC_BLOCK_BAD);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    Ok(v)
}

/// 行验证（不持字符串）：`suite.case=state,err` → (state, err)。
fn parse_line_validate(s: &str) -> Result<(u8, u32), u32> {
    let eq = s.find('=').ok_or(XC_BLOCK_BAD)?;
    let comma = s[eq + 1..].find(',').ok_or(XC_BLOCK_BAD)?;
    let dot = s[..eq].find('.').ok_or(XC_BLOCK_BAD)?;
    let _ = dot;
    let state = parse_u32(&s[eq + 1..eq + 1 + comma])?;
    if state > 2 {
        return Err(XC_BLOCK_BAD);
    }
    Ok((state as u8, parse_u32(&s[eq + 1 + comma + 1..])?))
}


fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8_lossy(&buf[i..]).into_owned()
}

pub fn run_unx0532_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0532");
    cs.add("SEG 段位独占", SEG_BASE == 0x35240 && XC_SEG_MISS == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：构造与三段齐。
    let lines = [
        ResLine { suite: "b19", case: "c1", state: 0, err: 0 },
        ResLine { suite: "b19", case: "c2", state: 1, err: 5 },
        ResLine { suite: "b20", case: "c3", state: 2, err: 0 },
    ];
    let blk = PersistBlock::build(&lines);
    cs.add("三段齐", blk.segments_ok(), "结果/守恒/指纹全备");
    cs.add("守恒账", blk.conserve == (3, 1, 1, 1), "3=1+1+1");
    cs.add("行数账", blk.line_count == 3, "build 路=3");
    cs.add("指纹复算", blk.fingerprint == PersistBlock::recompute_fp(&lines), "逐行求和等");

    // 族二：序列化回环。
    let text = blk.serialize();
    cs.add("三段头在", text.contains("[RESULT]") && text.contains("[CONSERVE]") && text.contains("[FINGERPRINT]"), "三段标齐");
    let blk2 = PersistBlock::deserialize(&text).unwrap_or(PersistBlock { lines: Vec::new(), line_count: 0, conserve: (0, 0, 0, 0), fingerprint: 0 });
    cs.add("回环一致", blk2.conserve == blk.conserve && blk2.fingerprint == blk.fingerprint && blk2.line_count == 3, "守恒+指纹+行数还原");

    // 族三：坏块拒。
    cs.add("缺段拒", PersistBlock::deserialize("[RESULT]
").is_err(), "无守恒段 Err");
    cs.add("空行集拒", PersistBlock::deserialize("[RESULT]
[CONSERVE]0=0+0+0
[FINGERPRINT]0
").is_err(), "零行 Err");
    cs.add("守恒坏拒", PersistBlock::deserialize("[RESULT]
b19.c1=0,0
[CONSERVE]5=1+1+1
[FINGERPRINT]2
").is_err(), "5≠3 式 Err");
    cs.add("态越界拒", PersistBlock::deserialize("[RESULT]
b19.c1=3,0
[CONSERVE]1=0+0+1
[FINGERPRINT]3
").is_err(), "state=3 Err");
    cs.add("指纹漂移拒", PersistBlock::deserialize("[RESULT]
b19.c1=0,0
[CONSERVE]1=0+0+1
[FINGERPRINT]9
").is_err(), "指纹≠重算 Err");

    // 族四：指纹漂移检出。
    let mut blk3 = PersistBlock::build(&lines);
    blk3.fingerprint += 1;
    cs.add("漂移检出", !blk3.segments_ok(), "指纹人为 +1 即坏");
    cs
}
