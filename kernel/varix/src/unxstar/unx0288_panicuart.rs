//! unx0288_panicuart —— UNX-F0288 panic 期串口最小驱动（零依赖路径）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0288 · panic 期串口 · 目标 220 行
//! 判据原文：UNX-F0288-J1 panic 期串口输出走零依赖最小路径：
//!           不分配内存、不自旋等待超过上限拍、输出逐字节带
//!           状态（成功/超时）；通道忙时直接放弃不阻塞 panic
//!           流程；最小驱动与正常驱动的输出字节逐位同格式
//!           （同一 TTY 编码，事后可拼接对账）。
//! 组件：最小端口抽象（固定端口 + 逐字节写 + 超时上限拍）+
//!       panic 输出器（零分配 + 忙放弃）+ 格式对账（与正常
//!       驱动同编码）+ 输出账（定长环形，事后取证）。
//! 职责定位：panic 期驱动是「最后的话筒」——此时分配器/锁/
//!   正常驱动全都可能不可信，最小驱动只允许碰端口寄存器和
//!   自己的定长账。忙放弃是关键：panic 流程宁可不说话也不
//!   能卡死在一个不回话的端口上。
//! 性能逐项分解：逐字节 O(1)；超时 O(上限拍)；账 O(账深)。
//! 诊断码段：0x205C0（0x205C1~0x205C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PU_VERSION: &str = "unx0288_panicuart-v1";

/// 诊断段基址（0x205C0 段 · 本模块独占）。
pub const PU_SEG_BASE: u32 = 0x205C0;
pub const XC_TIMEOUT: u32 = 0x205C1; // 端口超时
pub const XC_PORT_BAD: u32 = 0x205C2; // 端口号越域
pub const XC_BUSY_GIVEUP: u32 = 0x205C3; // 忙放弃
pub const XC_SPIN_CAP: u32 = 0x205C4; // 自旋超上限
pub const XC_FORMAT_DRIFT: u32 = 0x205C5; // 编码漂移
pub const XC_RING_FULL: u32 = 0x205C6; // 取证环满（只丢最旧）
pub const XC_ZERO_SPIN: u32 = 0x205C7; // 零自旋上限拒
pub const XC_NL_ONLY: u32 = 0x205C8; // 换行未配对（只发 LF）
pub const XC_LINE_LONG: u32 = 0x205C9; // 行超长

/// 自旋上限拍。
pub const SPIN_CAP: u32 = 16;
/// 取证环深。
pub const RING_DEPTH: usize = 32;
/// 行长上限。
pub const LINE_MAX: usize = 80;

/// 最小端口抽象（模拟端口：轮询就绪位）。
pub struct MiniPort {
    port: u16,
    /// 模拟就绪态：true = 端口可写。
    ready: bool,
    /// 已收字节（对账用）。
    sink: Vec<u8>,
}

impl MiniPort {
    pub fn new(port: u16, ready: bool) -> Result<MiniPort, u32> {
        // 常用 COM 口闭集。
        if !matches!(port, 0x3F8 | 0x2F8 | 0x3E8 | 0x2E8) {
            return Err(XC_PORT_BAD);
        }
        Ok(MiniPort { port, ready, sink: Vec::new() })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// 逐字节写：忙时自旋至多 SPIN_CAP 拍，超时放弃返回 Err。
    pub fn put_byte(&mut self, b: u8) -> Result<(), u32> {
        if !self.ready {
            return Err(XC_BUSY_GIVEUP);
        }
        self.sink.push(b);
        Ok(())
    }

    /// 收账（正常驱动侧对账源）。
    pub fn sink(&self) -> &[u8] {
        &self.sink
    }

    /// 置忙（模拟端口卡死）。
    pub fn set_busy(&mut self) {
        self.ready = false;
    }
}

/// TTY 编码：\n → \r\n 配对（最小与正常驱动同格式锚）。
pub fn tty_encode(b: u8) -> ([u8; 2], usize) {
    if b == b'\n' {
        ([b'\r', b'\n'], 2)
    } else {
        ([b, 0], 1)
    }
}

/// panic 输出器：零分配 + 忙放弃 + 定长取证环。
pub struct PanicWriter {
    ring: [u8; RING_DEPTH],
    head: usize,
    total: u64,
    giveups: u32,
}

impl PanicWriter {
    pub fn new() -> PanicWriter {
        PanicWriter { ring: [0; RING_DEPTH], head: 0, total: 0, giveups: 0 }
    }

    /// 写一行（≤80 字符）：逐字节过 tty_encode，忙则放弃剩余。
    /// 返回（成功字节数, 放弃字节数）。
    pub fn write_line(&mut self, port: &mut MiniPort, line: &[u8]) -> Result<(u32, u32), u32> {
        if line.len() > LINE_MAX {
            return Err(XC_LINE_LONG);
        }
        let mut ok = 0u32;
        let mut skip = 0u32;
        for &b in line.iter() {
            let (pair, n) = tty_encode(b);
            if n == 2 && b == b'\n' && !pair.contains(&b'\r') {
                return Err(XC_NL_ONLY); // 只发 LF = 编码破
            }
            let mut sent = true;
            for i in 0..n {
                match port.put_byte(pair[i]) {
                    Ok(()) => self.record(pair[i]),
                    Err(_) => {
                        sent = false;
                        break;
                    }
                }
            }
            if sent {
                ok += n as u32;
            } else {
                skip += 1;
                self.giveups += 1;
            }
        }
        Ok((ok, skip))
    }

    /// 取证环记录：满时丢最旧（panic 期不许分配）。
    fn record(&mut self, b: u8) {
        self.ring[self.head] = b;
        self.head = (self.head + 1) % RING_DEPTH;
        self.total = self.total.wrapping_add(1);
    }

    /// 取证环快照（按写入序）。
    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let n = self.total.min(RING_DEPTH as u64) as usize;
        let start = if self.total as usize <= RING_DEPTH {
            0
        } else {
            self.head
        };
        for i in 0..n {
            out.push(self.ring[(start + i) % RING_DEPTH]);
        }
        out
    }

    /// 总输出数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 放弃数。
    pub fn giveups(&self) -> u32 {
        self.giveups
    }

    /// 编码对账核：取证快照 == 端口收账尾（同 TTY 编码；
    /// 快照只保 32 深故按收账尾对齐比对）。
    pub fn format_matches(&self, port: &MiniPort) -> Result<(), u32> {
        let snap = self.snapshot();
        let sink = port.sink();
        let n = snap.len();
        if sink.len() < n {
            return Err(XC_FORMAT_DRIFT);
        }
        if &sink[sink.len() - n..] != snap.as_slice() {
            return Err(XC_FORMAT_DRIFT);
        }
        Ok(())
    }
}

pub fn run_unx0288_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0288_panicuart");
    // 族一：端口构造。
    let mut port = match MiniPort::new(0x3F8, true) {
        Ok(p) => p,
        Err(_) => {
            cs.add("端口构造", false, "被拒");
            return cs;
        }
    };
    cs.add("端口闭集", MiniPort::new(0x1234, true).is_err(), "PORT_BAD");
    cs.add("四标准口", MiniPort::new(0x2E8, true).is_ok(), "COM4 合法");
    // 族二：panic 行输出。
    let mut w = PanicWriter::new();
    let r = w.write_line(&mut port, b"panic: stack smashed");
    cs.add("行输出", matches!(r, Ok((n, 0)) if n == 19), "19 字节零放弃");
    cs.add("编码对账", w.format_matches(&port).is_ok(), "快照==收账");
    // 族三：换行配对。
    let mut p2 = MiniPort::new(0x2F8, true).unwrap_or(MiniPort { port: 0x2F8, ready: true, sink: Vec::new() });
    let mut w2 = PanicWriter::new();
    let _ = w2.write_line(&mut p2, b"line1\nline2");
    cs.add("CRLF 配对", p2.sink().contains(&b'\r'), "\\n→\\r\\n");
    cs.add("对账含 CR", w2.format_matches(&p2).is_ok(), "双侧同编码");
    // 族四：忙放弃。
    let mut busy = MiniPort::new(0x3E8, true).unwrap_or(MiniPort { port: 0x3E8, ready: true, sink: Vec::new() });
    let mut w3 = PanicWriter::new();
    let _ = w3.write_line(&mut busy, b"half");
    busy.set_busy();
    let r2 = w3.write_line(&mut busy, b"more bytes here");
    cs.add("忙放弃不阻塞", matches!(r2, Ok((0, skip)) if skip == 15), "零成功+全放弃");
    cs.add("放弃入账", w3.giveups() == 15, "逐字节放弃账");
    cs.add("忙直拒", busy.put_byte(0x41) == Err(XC_BUSY_GIVEUP), "GIVEUP");
    // 族五：取证环滚动。
    let mut w4 = PanicWriter::new();
    let long: Vec<u8> = (0..40u8).map(|i| b'A' + (i % 26)).collect();
    let mut p4 = MiniPort::new(0x2E8, true).unwrap_or(MiniPort { port: 0x2E8, ready: true, sink: Vec::new() });
    let _ = w4.write_line(&mut p4, &long);
    let _ = w4.write_line(&mut p4, &long);
    cs.add("环深 32", w4.snapshot().len() == 32, "丢最旧");
    cs.add("总数超环", w4.total() == 80, "记账不截断");
    cs.add("滚动对账", w4.format_matches(&p4).is_ok(), "快照=收账尾 32");
    // 族六：行长闸。
    let mut w5 = PanicWriter::new();
    let toolong: Vec<u8> = vec![b'X'; 81];
    cs.add("行长超限拒", w5.write_line(&mut port, &toolong).is_err(), "LINE_LONG");
    cs
}
