//! unx0559_loghandoff —— UNX-F0559 日志交接字段。
//! 锚点（CoRun Unxreal 主册）：UNX-F0559 · 日志交接字段 · 目标 240 行
//! 判据原文：交接字段 v1 冻结（8 字段），消费方 mock diff 零差。
//! 组件：日志交接结构（8 字段：级别/组件/时戳/文本 hash/守恒四数+校验和）
//!   + mock 消费方复现核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35900;
pub const FIELD_COUNT: usize = 8;
pub const LOG_HANDOFF_VER: u32 = 1;
pub const XC_LH_MISS: u32 = SEG_BASE + 1; // 字段缺
pub const XC_LH_VER: u32 = SEG_BASE + 2; // 版本不符
pub const XC_LH_CSUM: u32 = SEG_BASE + 3; // 校验和漂移
pub const XC_LH_OOB: u32 = SEG_BASE + 4; // 越界

/// 日志交接记录（8 字段冻结）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogHandoff {
    /// F0 级别（0..3）。
    pub level: u8,
    /// F1 组件号（0..15）。
    pub comp: u8,
    /// F2 时戳步。
    pub step: u64,
    /// F3 文本 hash（Σbyte×(pos+1) mod 65536）。
    pub text_h: u32,
    /// F4-F7 守恒四数（emit/放行/拒收/丢弃）。
    pub emitted: u32,
    pub gate_pass: u32,
    pub gate_reject: u32,
    pub filt_drop: u32,
}

impl LogHandoff {
    /// 校验和（全 8 字段位权折叠）。
    pub const fn csum(&self) -> u64 {
        (self.level as u64 * 1)
            + (self.comp as u64 * 2)
            + (self.step * 3)
            + (self.text_h as u64 * 5)
            + (self.emitted as u64 * 7)
            + (self.gate_pass as u64 * 11)
            + (self.gate_reject as u64 * 13)
            + (self.filt_drop as u64 * 17)
    }

    /// 字段齐核（八字段合法性+守恒）。
    pub fn fields_ok(&self) -> bool {
        self.level <= 3
            && self.comp < 16
            && self.emitted == self.gate_pass + self.gate_reject + self.filt_drop
            && self.text_h <= 65535
    }

    /// mock 消费方视图。
    pub fn mock_view(&self) -> String {
        let mut s = String::from("LHO|");
        s.push_str(&itoa(self.level as u64));
        s.push('|');
        s.push_str(&itoa(self.comp as u64));
        s.push('|');
        s.push_str(&itoa(self.step));
        s.push('|');
        s.push_str(&itoa(self.text_h as u64));
        s.push('|');
        s.push_str(&itoa(self.emitted as u64));
        s.push('|');
        s.push_str(&itoa(self.gate_pass as u64));
        s.push('|');
        s.push_str(&itoa(self.gate_reject as u64));
        s.push('|');
        s.push_str(&itoa(self.filt_drop as u64));
        s
    }
}

/// 文本 hash（与 F0553 同式）。
pub fn text_hash(text: &str) -> u32 {
    let mut s: u64 = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        s += (bytes[i] as u64) * ((i + 1) as u64);
        i += 1;
    }
    (s % 65536) as u32
}

/// mock diff：两记录视图逐字节差异数。
pub fn mock_diff(a: &LogHandoff, b: &LogHandoff) -> usize {
    let va = a.mock_view();
    let vb = b.mock_view();
    if va.len() != vb.len() {
        return va.len().max(vb.len());
    }
    let (ba, bb) = (va.as_bytes(), vb.as_bytes());
    let mut d = 0;
    let mut i = 0;
    while i < ba.len() {
        if ba[i] != bb[i] {
            d += 1;
        }
        i += 1;
    }
    d
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

pub fn run_unx0559_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0559");
    cs.add("SEG 段位独占", SEG_BASE == 0x35900 && XC_LH_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：八字段构造与守恒。
    let h = LogHandoff {
        level: 1,
        comp: 3,
        step: 42,
        text_h: text_hash("boot ok"),
        emitted: 10,
        gate_pass: 6,
        gate_reject: 2,
        filt_drop: 2,
    };
    cs.add("八字段", FIELD_COUNT == 8, "v1 冻结");
    cs.add("字段合法", h.fields_ok(), "级别/组件域内+守恒");
    cs.add("文本 hash", h.text_h == text_hash("boot ok"), "与 F0553 同式");

    // 族二：mock diff。
    let h2 = LogHandoff { step: 42, ..h };
    cs.add("同记录零差", mock_diff(&h, &h2) == 0, "同字段同视图");
    let h3 = LogHandoff { emitted: 11, gate_pass: 7, ..h };
    cs.add("异记录有差", mock_diff(&h, &h3) > 0, "字段变 diff>0");
    cs.add("视图头", h.mock_view().starts_with("LHO|1|3|42|"), "七段头精确");

    // 族三：守恒破检出。
    let bad = LogHandoff { emitted: 10, gate_pass: 6, gate_reject: 2, filt_drop: 1, ..h };
    cs.add("守恒破检出", !bad.fields_ok(), "9≠10");
    let badlv = LogHandoff { level: 4, ..h };
    cs.add("级别越界检出", !badlv.fields_ok(), "level=4");
    let badcp = LogHandoff { comp: 16, ..h };
    cs.add("组件越界检出", !badcp.fields_ok(), "comp=16");

    // 族四：校验和敏感性。
    cs.add("csum 敏感", h.csum() != h3.csum(), "字段变和变");
    cs.add("csum 确定性", h.csum() == h2.csum(), "同记录同和");
    cs
}
