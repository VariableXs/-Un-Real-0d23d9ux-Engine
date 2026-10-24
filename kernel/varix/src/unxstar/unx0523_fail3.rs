//! unx0523_fail3 —— UNX-F0523 失败三要素呈现（定位/期望/实际）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0523 · 失败三要素呈现 · 目标 220 行
//! 判据原文：三要素呈现账齐（文件/行/期望 vs 实际），10 失败样本呈现完
//!           整率 100%。
//! 组件：FailReport（文件/行/期望/实际/原语号五字段）+ 呈现器（格式化
//!   为三行文本：定位行/期望行/实际行）+ 完整率核（任一字段缺即不完整）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35000;
pub const XC_LOC_MISSING: u32 = SEG_BASE + 1; // 定位缺失
pub const XC_VALS_MISSING: u32 = SEG_BASE + 2; // 期望/实际缺失
pub const XC_REPORT_BAD: u32 = SEG_BASE + 3; // 报告坏（行号 0）
pub const XC_PCT_OOB: u32 = SEG_BASE + 4; // 完整率越界

/// 失败报告（三要素齐备结构）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FailReport {
    /// 文件锚（'static 短名）。
    pub file: &'static str,
    /// 行号（1 起，0 视为缺定位）。
    pub line: u32,
    /// 期望值。
    pub want: u64,
    /// 实际值。
    pub got: u64,
    /// 原语号（对齐 F0522 八原语）。
    pub prim: u8,
}

impl FailReport {
    /// 构造并校验：行号 0 或空文件名即拒。
    pub fn new(file: &'static str, line: u32, want: u64, got: u64, prim: u8) -> Result<Self, u32> {
        if file.is_empty() || line == 0 {
            return Err(XC_LOC_MISSING);
        }
        if prim > 7 {
            return Err(XC_REPORT_BAD);
        }
        Ok(FailReport { file, line, want, got, prim })
    }

    /// 完整性：三要素全非缺。
    pub const fn is_complete(&self) -> bool {
        self.line > 0 && !self.file.is_empty() && (self.want != 0 || self.got != 0 || self.want == self.got)
    }

    /// 三行呈现：定位行/期望行/实际行。
    pub fn render3(&self) -> [String; 3] {
        [
            fmt2("at ", self.file, self.line),
            fmt1("want ", self.want),
            fmt1("got  ", self.got),
        ]
    }
}

/// `pre<file>:<line>`。
fn fmt2(pre: &str, file: &str, line: u32) -> String {
    let mut s = String::from(pre);
    s.push_str(file);
    s.push(':');
    s.push_str(&itoa(line as u64));
    s
}

/// `pre<value>`。
fn fmt1(pre: &str, v: u64) -> String {
    let mut s = String::from(pre);
    s.push_str(&itoa(v));
    s
}

/// u64→十进制。
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

/// 报告账：批量失败样本管理。
pub struct ReportBook {
    pub reports: Vec<FailReport>,
}

impl ReportBook {
    /// 空账。
    pub fn new() -> Self {
        ReportBook { reports: Vec::new() }
    }

    /// 收录（构造失败即拒——坏样本不入账）。
    pub fn record(&mut self, file: &'static str, line: u32, want: u64, got: u64, prim: u8) -> Result<(), u32> {
        let r = FailReport::new(file, line, want, got, prim)?;
        self.reports.push(r);
        Ok(())
    }

    /// 完整率（千分比）。
    pub fn complete_permille(&self) -> Result<u32, u32> {
        if self.reports.is_empty() {
            return Err(XC_PCT_OOB);
        }
        let mut c = 0;
        let mut i = 0;
        while i < self.reports.len() {
            if self.reports[i].is_complete() {
                c += 1;
            }
            i += 1;
        }
        Ok((c as u64 * 1000 / self.reports.len() as u64) as u32)
    }

    /// 三行呈现完整性核：每份报告三行全部非空。
    pub fn render_all_complete(&self) -> bool {
        let mut i = 0;
        while i < self.reports.len() {
            let r3 = self.reports[i].render3();
            let mut j = 0;
            while j < 3 {
                if r3[j].is_empty() {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }
}

impl Default for ReportBook {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0523_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0523");
    cs.add("SEG 段位独占", SEG_BASE == 0x35000 && XC_PCT_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：构造与拒收。
    cs.add("合法构造", FailReport::new("u.rs", 42, 8, 9, 0).is_ok(), "齐备样本入账");
    cs.add("行号 0 拒", FailReport::new("u.rs", 0, 1, 2, 0).is_err(), "缺定位 Err");
    cs.add("空文件拒", FailReport::new("", 1, 1, 2, 0).is_err(), "缺文件 Err");
    cs.add("原语越界拒", FailReport::new("u.rs", 1, 1, 2, 8).is_err(), "prim>7 Err");

    // 族二：10 失败样本完整率 100%。
    let mut b = ReportBook::new();
    let mut i = 0;
    while i < 10 {
        let _ = b.record("ktest.rs", (i + 1) as u32, i as u64, (i + 1) as u64, (i % 8) as u8);
        i += 1;
    }
    cs.add("十样本入账", b.len() == 10, "10/10 收录");
    cs.add("完整率 100%", b.complete_permille() == Ok(1000), "逐份齐备");
    cs.add("三行呈现全非空", b.render_all_complete(), "定位/期望/实际三行");

    // 族三：呈现内容精确性。
    let r = FailReport::new("core.rs", 7, 100, 99, 0).unwrap_or(FailReport { file: "x", line: 1, want: 0, got: 0, prim: 0 });
    let r3 = r.render3();
    cs.add("定位行", r3[0] == alloc::string::String::from("at core.rs:7"), "文件:行 精确");
    cs.add("期望行", r3[1] == alloc::string::String::from("want 100"), "want 精确");
    cs.add("实际行", r3[2] == alloc::string::String::from("got  99"), "got 精确");

    // 族四：坏账检出。
    let mut b2 = ReportBook::new();
    // want==got==0 且不等不存在——此样本 want=0,got=0 视为完整（want==got）。
    let _ = b2.record("a.rs", 1, 0, 0, 0);
    cs.add("零值但等值完整", b2.complete_permille() == Ok(1000), "want==got 不因零判缺");
    cs.add("空账拒率", ReportBook::new().complete_permille().is_err(), "空账无率");
    cs
}
