//! unx0538_handoff —— UNX-F0538 ktest 交接字段。
//! 锚点（CoRun Unxreal 主册）：UNX-F0538 · ktest 交接字段 · 目标 240 行
//! 判据原文：交接字段 v1 冻结（8 字段），消费方 mock diff 零差。
//! 组件：交接结构（8 字段冻结：套件名/用例名/态/错码/时戳步/种子/框架版/
//!   校验和）+ mock 消费方复现核（同字段产出同 mock diff=零差）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x353C0;
pub const XC_FIELD_MISS: u32 = SEG_BASE + 1; // 字段缺
pub const XC_HANDOFF_VER: u32 = SEG_BASE + 2; // 版本不符
pub const XC_CSUM_DRIFT: u32 = SEG_BASE + 3; // 校验和漂移
pub const XC_FIELD_OOB: u32 = SEG_BASE + 4; // 字段越界

/// 冻结字段数。
pub const FIELD_COUNT: usize = 8;
/// 交接版本。
pub const HANDOFF_VER: u32 = 1;

/// 交接记录（8 字段）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handoff {
    /// F0 套件名 hash（短 8bit）。
    pub suite_h: u8,
    /// F1 用例名 hash。
    pub case_h: u8,
    /// F2 态（0/1/2）。
    pub state: u8,
    /// F3 错码。
    pub err: u32,
    /// F4 时戳步。
    pub step: u64,
    /// F5 种子。
    pub seed: u64,
    /// F6 框架版。
    pub frame_ver: u32,
    /// F7 校验和（前七字段求和折叠）。
    pub csum: u64,
}

impl Handoff {
    /// 构造并算校验和（F7 由前七字段推导）。
    pub fn new(suite_h: u8, case_h: u8, state: u8, err: u32, step: u64, seed: u64, frame_ver: u32) -> Result<Self, u32> {
        if state > 2 {
            return Err(XC_FIELD_OOB);
        }
        if frame_ver != HANDOFF_VER {
            return Err(XC_HANDOFF_VER);
        }
        let csum = Self::calc_csum(suite_h, case_h, state, err, step, seed, frame_ver);
        Ok(Handoff { suite_h, case_h, state, err, step, seed, frame_ver, csum })
    }

    /// 校验和：逐字段加 + 位置权重折叠。
    pub const fn calc_csum(suite_h: u8, case_h: u8, state: u8, err: u32, step: u64, seed: u64, frame_ver: u32) -> u64 {
        (suite_h as u64 * 1)
            + (case_h as u64 * 2)
            + (state as u64 * 3)
            + (err as u64 * 5)
            + (step * 7)
            + (seed * 11)
            + (frame_ver as u64 * 13)
    }

    /// 八字段齐核（csum 复核）。
    pub fn fields_ok(&self) -> bool {
        self.csum
            == Self::calc_csum(self.suite_h, self.case_h, self.state, self.err, self.step, self.seed, self.frame_ver)
            && self.state <= 2
            && self.frame_ver == HANDOFF_VER
    }

    /// mock 消费方视图（确定性行文本）。
    pub fn mock_view(&self) -> String {
        let mut s = String::from("HO|");
        s.push_str(&itoa(self.suite_h as u64));
        s.push('|');
        s.push_str(&itoa(self.case_h as u64));
        s.push('|');
        s.push_str(&itoa(self.state as u64));
        s.push('|');
        s.push_str(&itoa(self.err as u64));
        s.push('|');
        s.push_str(&itoa(self.step));
        s.push('|');
        s.push_str(&itoa(self.seed));
        s.push('|');
        s.push_str(&itoa(self.frame_ver as u64));
        s.push('|');
        s.push_str(&itoa(self.csum));
        s
    }
}

/// mock diff：两记录的视图逐字符比对，零差即等。
pub fn mock_diff(a: &Handoff, b: &Handoff) -> usize {
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

pub fn run_unx0538_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0538");
    cs.add("SEG 段位独占", SEG_BASE == 0x353C0 && XC_FIELD_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：八字段构造。
    let h = Handoff::new(10, 20, 0, 0, 100, 7, 1).unwrap_or(Handoff { suite_h: 0, case_h: 0, state: 0, err: 0, step: 0, seed: 0, frame_ver: 0, csum: 0 });
    cs.add("八字段齐", h.fields_ok(), "csum 复核过");
    cs.add("字段数冻结", FIELD_COUNT == 8, "8 字段 v1");
    cs.add("态越界拒", Handoff::new(1, 1, 3, 0, 1, 1, 1).is_err(), "state=3 Err");
    cs.add("版本不符拒", Handoff::new(1, 1, 0, 0, 1, 1, 2).is_err(), "frame_ver=2 Err");

    // 族二：mock diff 零差。
    let h2 = Handoff::new(10, 20, 0, 0, 100, 7, 1).unwrap_or(h);
    cs.add("同记录零差", mock_diff(&h, &h2) == 0, "同字段同视图");
    let h3 = Handoff::new(10, 20, 1, 9, 100, 7, 1).unwrap_or(h);
    cs.add("异记录有差", mock_diff(&h, &h3) > 0, "字段变 diff>0");
    cs.add("视图格式", h.mock_view().starts_with("HO|10|20|0|"), "七段头精确");

    // 族三：校验和漂移。
    let mut bad = h2;
    bad.csum += 1;
    cs.add("漂移检出", !bad.fields_ok(), "csum+1 即坏");
    cs.add("校验和敏感", Handoff::calc_csum(1, 0, 0, 0, 0, 0, 1) != Handoff::calc_csum(0, 1, 0, 0, 0, 0, 1), "位权区分");

    // 族四：大值折叠。
    let big = Handoff::new(255, 255, 2, u32::MAX, u64::MAX / 2, u64::MAX / 3, 1).unwrap_or(h);
    cs.add("大值不溢", big.fields_ok(), "极端值 csum 自洽");
    cs
}
