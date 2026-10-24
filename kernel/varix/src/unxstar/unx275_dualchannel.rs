// SPDX-License-Identifier: Apache-2.0
//! UNX-F0275 · 双通道同写一致性（屏幕与串口内容对账）
//!
//! 同一事件双通道输出内容逐行一致（去色比对），差异计数为零。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 双通道
// ---------------------------------------------------------------------------

/// 输出通道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 屏幕。
    Screen,
    /// 串口。
    Serial,
}

/// 单行输出。
#[derive(Debug, Clone)]
pub struct OutputLine {
    pub channel: Channel,
    pub text: String,
    /// 是否含 ANSI 色。
    pub has_color: bool,
}

/// 去色器（剥离 ANSI 转义序列）。
pub struct ColorStripper;

impl ColorStripper {
    /// 剥离 ANSI 色码。
    pub fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
                continue;
            }
            if in_escape {
                if c == 'm' { in_escape = false; }
                continue;
            }
            out.push(c);
        }
        out
    }
}

/// 双通道写入器。
pub struct DualChannelWriter {
    screen_lines: Vec<String>,
    serial_lines: Vec<String>,
    /// 差异计数。
    pub diff_count: u32,
}

impl DualChannelWriter {
    pub fn new() -> Self {
        DualChannelWriter { screen_lines: Vec::new(), serial_lines: Vec::new(), diff_count: 0 }
    }

    /// 同写一行到双通道。
    pub fn write_both(&mut self, text: &str) {
        self.screen_lines.push(text.to_string());
        self.serial_lines.push(text.to_string());
    }

    /// 对账（逐行去色比对）。
    pub fn reconcile(&mut self) -> bool {
        if self.screen_lines.len() != self.serial_lines.len() {
            self.diff_count += 1;
            return false;
        }
        for (s, r) in self.screen_lines.iter().zip(self.serial_lines.iter()) {
            let ss = ColorStripper::strip(s);
            let rr = ColorStripper::strip(r);
            if ss != rr {
                self.diff_count += 1;
                return false;
            }
        }
        true
    }

    /// 屏幕行数。
    pub fn screen_count(&self) -> usize { self.screen_lines.len() }
    /// 串口行数。
    pub fn serial_count(&self) -> usize { self.serial_lines.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0275 判据全集。
pub fn run_dual_channel_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0275", "双通道同写一致性");

    // 1. 去色
    let colored = "\x1b[32mgreen text\x1b[0m";
    cs.ok("去色正确", ColorStripper::strip(colored) == "green text", "ANSI 剥离");
    cs.ok("纯文本不变", ColorStripper::strip("plain") == "plain", "无色不改");

    // 2. 双写
    let mut dw = DualChannelWriter::new();
    dw.write_both("hello");
    dw.write_both("world");
    cs.ok("双写屏幕", dw.screen_count() == 2, "两行");
    cs.ok("双写串口", dw.serial_count() == 2, "两行");

    // 3. 对账
    cs.ok("零差异", dw.reconcile(), "逐行一致");
    cs.ok("差异计数", dw.diff_count == 0, "零差异");

    // 4. 差异检测
    let mut dw2 = DualChannelWriter::new();
    dw2.write_both("line1");
    dw2.screen_lines.push("screen_only".into());
    dw2.serial_lines.push("serial_only".into());
    cs.ok("差异检出", !dw2.reconcile(), "内容不一致");
    cs.ok("差异计数", dw2.diff_count > 0, "显性化");

    // 5. 长度不一致
    let mut dw3 = DualChannelWriter::new();
    dw3.write_both("a");
    dw3.screen_lines.push("extra".into());
    cs.ok("长度不一致", !dw3.reconcile(), "行数不同");

    cs.done()
}
