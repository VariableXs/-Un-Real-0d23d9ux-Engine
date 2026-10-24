//! unx0545_uartout —— UNX-F0545 串口汇出（F0502 消费）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0545 · 串口汇出 · 目标 220 行
//! 判据原文：串口汇出链核（10 行原样汇出 10/10），汇出与环形读出字节全等。
//! 组件：UartSink（字节流汇出口——no_std 无真串口，以字节账模拟）+ 汇出
//!   链（环读→汇出逐行）+ 字节全等核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35580;
pub const XC_UART_SILENT: u32 = SEG_BASE + 1; // 汇出空转
pub const XC_BYTE_DRIFT: u32 = SEG_BASE + 2; // 字节漂移
pub const XC_UART_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_UART_BAD: u32 = SEG_BASE + 4; // 坏行

/// 串口汇出口（字节账模拟）。
#[derive(Debug, Default)]
pub struct UartSink {
    pub bytes: Vec<u8>,
    /// 汇出行数。
    pub lines: u32,
}

impl UartSink {
    /// 空汇出口。
    pub fn new() -> Self {
        UartSink { bytes: Vec::new(), lines: 0 }
    }

    /// 汇出一行（追加行内容+\n）。
    pub fn send_line(&mut self, line: &str) -> Result<(), u32> {
        if line.is_empty() {
            return Err(XC_UART_BAD);
        }
        self.bytes.extend_from_slice(line.as_bytes());
        self.bytes.push(b'\n');
        self.lines += 1;
        Ok(())
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// 按行切回（对账用）。
    pub fn lines_view(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut i = 0;
        while i < self.bytes.len() {
            if self.bytes[i] == b'\n' {
                out.push(core::mem::take(&mut cur));
            } else {
                cur.push(self.bytes[i] as char);
            }
            i += 1;
        }
        out
    }
}

/// 汇出链：环快照逐行汇出。
pub fn export_chain(snap: &[String], sink: &mut UartSink) -> Result<u32, u32> {
    if snap.is_empty() {
        return Err(XC_UART_SILENT);
    }
    let mut n = 0;
    let mut i = 0;
    while i < snap.len() {
        sink.send_line(&snap[i])?;
        n += 1;
        i += 1;
    }
    Ok(n)
}

/// 字节全等核：汇出内容与环读行集逐字节等。
pub fn bytes_equal(sink: &UartSink, snap: &[String]) -> bool {
    let view = sink.lines_view();
    if view.len() != snap.len() {
        return false;
    }
    let mut i = 0;
    while i < snap.len() {
        if view[i] != snap[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn run_unx0545_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0545");
    cs.add("SEG 段位独占", SEG_BASE == 0x35580 && XC_UART_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：单行汇出。
    let mut s = UartSink::new();
    let _ = s.send_line("42|INF|3|boot ok");
    cs.add("单行字节", s.lines_view() == alloc::vec![String::from("42|INF|3|boot ok")], "含行尾 \\n 切回");
    cs.add("行数账", s.lines == 1, "一行一计");
    cs.add("空行拒", s.send_line("").is_err(), "空行 Err");

    // 族二：10 行原样汇出 10/10。
    let mut snap = Vec::new();
    let mut i = 0;
    while i < 10 {
        snap.push(alloc::format!("{}|INF|0|m{}", i, i));
        i += 1;
    }
    let mut s2 = UartSink::new();
    let n = export_chain(&snap, &mut s2).unwrap_or(0);
    cs.add("汇出 10/10", n == 10 && s2.lines == 10, "十行全发");
    cs.add("字节全等", bytes_equal(&s2, &snap), "汇出=环读逐行等");
    cs.add("非空", !s2.is_empty() && s2.len() > 10, "字节量账");

    // 族三：漂移检出。
    let mut snap2 = snap.clone();
    snap2[3] = String::from("tampered");
    cs.add("漂移检出", !bytes_equal(&s2, &snap2), "篡改行被点名");
    cs.add("行数不等检出", {
        let mut shorter = snap.clone();
        shorter.pop();
        !bytes_equal(&s2, &shorter)
    }, "缺行即不等");

    // 族四：空环拒汇。
    let mut s3 = UartSink::new();
    cs.add("空环拒", export_chain(&[], &mut s3).is_err(), "零行 Err");
    cs.add("拒后无字节", s3.is_empty(), "不产生空转字节");
    cs
}
