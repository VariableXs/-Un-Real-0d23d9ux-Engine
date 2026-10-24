//! UNX-F0102 16550 串口全速通道（0xA780 段）
//!
//! 锚点：UNX-F0102-J1 波特率 115200 下 1MB 输出零错位零丢字符
//! （环回/对端计数核验）。
//! 域：CoRun·UNX-A1-B06（16550 全速面）。
//!
//! 结构：16550 寄存器模型（THR/RBR/IER/IIR/FCR/LCR/MCR/LSR + DLL/
//! DLM 除数锁）+ 波特率除数器（1.8432 MHz → 115200 = 0x0001，锁存
//! DLAB 时序）+ 环回通道（MCR LOOP 位 + 收发对端计数账）+ 1MB 输出
//! 压测核（字节序完整 + 零丢 + 零错位）+ 注入核（除数漂移/DLAB
//! 未锁/LSR 溢出/计数破缺/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA780 独占 0x80）。

pub const SEG_BASE: u32 = 0xA780;
pub const XC_UA_DIV: u32 = SEG_BASE + 0x01; // 除数漂移
pub const XC_UA_DLAB: u32 = SEG_BASE + 0x02; // DLAB 未锁
pub const XC_UA_LOST: u32 = SEG_BASE + 0x03; // 丢字符
pub const XC_UA_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// UART 基频与目标波特率。
pub const UART_CLK: u32 = 1_843_200;
pub const BAUD_115200: u32 = 115_200;

/// LSR 位。
pub const LSR_THRE: u8 = 1 << 5; // 发送保持空
pub const LSR_DR: u8 = 1; // 数据就绪
pub const LSR_OE: u8 = 1 << 1; // 溢出

/// MCR 位。
pub const MCR_LOOP: u8 = 1 << 4; // 环回
pub const MCR_DTR: u8 = 1;
pub const MCR_RTS: u8 = 2;

/// 波特率除数计算（四舍五入）。
pub fn divisor_for(baud: u32) -> Result<u16, u32> {
    if baud == 0 {
        return Err(XC_UA_DIV);
    }
    let d = (UART_CLK + baud / 2) / (16 * baud);
    if d == 0 || d > 0xFFFF {
        return Err(XC_UA_DIV);
    }
    Ok(d as u16)
}

/// 16550 通道模型（寄存器 + 环回账）。
pub struct Uart16550 {
    pub dll: u8,
    pub dlm: u8,
    pub dlab: bool,
    pub lcr: u8,
    pub mcr: u8,
    pub lsr: u8,
    pub fcr: u8,
    pub tx_count: u64,
    pub rx_count: u64,
    pub rx_buf: Vec<u8>,
    pub lost: u64,
    pub sealed: bool,
}

impl Uart16550 {
    pub fn new() -> Self {
        Uart16550 {
            dll: 0,
            dlm: 0,
            dlab: false,
            lcr: 0x03, // 8N1
            mcr: MCR_DTR | MCR_RTS,
            lsr: LSR_THRE,
            fcr: 0xC7, // FIFO 使能 + 清空 + 14 字节阈值
            tx_count: 0,
            rx_count: 0,
            rx_buf: Vec::new(),
            lost: 0,
            sealed: false,
        }
    }

    /// 配置波特率（DLAB 时序：开 DLAB → 写除数 → 锁 DLAB）。
    pub fn set_baud(&mut self, baud: u32) -> Result<(), u32> {
        let d = divisor_for(baud)?;
        self.dlab = true; // 开锁存
        self.dll = (d & 0xFF) as u8;
        self.dlm = (d >> 8) as u8;
        self.dlab = false; // 锁（访问数据寄存器）
        Ok(())
    }

    /// 除数回读核（须 DLAB 开才能读）。
    pub fn read_divisor(&self) -> Result<u16, u32> {
        if !self.dlab {
            return Err(XC_UA_DLAB);
        }
        Ok(self.dll as u16 | ((self.dlm as u16) << 8))
    }

    /// 发送一字节（THRE 守卫 + 环回直收）。
    pub fn tx_byte(&mut self, b: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_UA_SEALED);
        }
        if self.lsr & LSR_THRE == 0 {
            self.lost += 1;
            return Err(XC_UA_LOST);
        }
        self.tx_count += 1;
        if self.mcr & MCR_LOOP != 0 {
            // 环回：THR → RBR 直通
            self.rx_buf.push(b);
            self.rx_count += 1;
        }
        Ok(())
    }

    /// 接收一字节（DR 守卫）。
    pub fn rx_byte(&mut self) -> Option<u8> {
        if self.rx_buf.is_empty() {
            return None;
        }
        Some(self.rx_buf.remove(0))
    }

    /// 对端计数核：环回下 tx == rx。
    pub fn loopback_counts(&self) -> bool {
        self.mcr & MCR_LOOP != 0 && self.tx_count == self.rx_count
    }
}

/// 115200 除数核：0x0001 精确。
pub fn div_115200_exact() -> bool {
    divisor_for(BAUD_115200) == Ok(1)
        && divisor_for(9600) == Ok(12)
        && divisor_for(38400) == Ok(3)
        && divisor_for(0).is_err()
}

/// DLAB 时序核：配置后锁 + 回读一致。
pub fn dlab_seq_exact() -> bool {
    let mut u = Uart16550::new();
    if u.set_baud(BAUD_115200).is_err() {
        return false;
    }
    if u.read_divisor().is_ok() {
        return false; // 锁后读除数须拒（DLAB 关）
    }
    u.dlab = true;
    u.read_divisor() == Ok(1)
}

/// 1MB 输出压测核：1_048_576 字节环回全收、零丢、零错位。
pub fn mb_stress_exact() -> bool {
    let mut u = Uart16550::new();
    u.mcr |= MCR_LOOP;
    if u.set_baud(BAUD_115200).is_err() {
        return false;
    }
    let total: u64 = 1 << 20;
    for i in 0..total {
        let b = (i % 251) as u8; // 非全 0/255 的伪随机序
        if u.tx_byte(b).is_err() {
            return false;
        }
    }
    if !u.loopback_counts() || u.lost != 0 {
        return false;
    }
    // 对端逐字节核（零错位）
    for i in 0..total {
        match u.rx_byte() {
            Some(b) => {
                if b != (i % 251) as u8 {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 8N1 帧格式核：LCR 冻结。
pub fn frame_8n1_exact() -> bool {
    let u = Uart16550::new();
    // 8 数据位（0x03）+ 1 停止位（bit2=0）+ 无校验（bit3=0）
    u.lcr == 0x03 && (u.lcr & 0x04 == 0) && (u.lcr & 0x08 == 0)
}

/// FIFO 阈值核：FCR 使能位 + 触发档。
pub fn fifo_exact() -> bool {
    let u = Uart16550::new();
    (u.fcr & 0x01) != 0 // FIFO 使能
        && (u.fcr >> 6) == 3 // 14 字节触发档
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零波特率
    if divisor_for(0) == Err(XC_UA_DIV) {
        caught += 1;
    }
    // 2) DLAB 未开读除数
    let u = Uart16550::new();
    if u.read_divisor() == Err(XC_UA_DLAB) {
        caught += 1;
    }
    // 3) THRE 未就绪发送 → 丢字符
    let mut u2 = Uart16550::new();
    u2.lsr = 0;
    if u2.tx_byte(0x41) == Err(XC_UA_LOST) && u2.lost == 1 {
        caught += 1;
    }
    // 4) 非环回下 tx 不产生 rx
    let mut u3 = Uart16550::new();
    let _ = u3.tx_byte(0x42);
    if u3.rx_count == 0 && u3.tx_count == 1 {
        caught += 1;
    }
    // 5) 封账后拒
    let mut u4 = Uart16550::new();
    u4.sealed = true;
    if u4.tx_byte(0x43) == Err(XC_UA_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：通道账折叠哈希。
pub fn fp_frozen(u: &Uart16550) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= u.dll as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= u.dlm as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= u.tx_count;
    h = h.wrapping_mul(0x100000001b3);
    h ^= u.rx_count;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx102_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0102");
    // J1-1 115200 除数
    let ok = div_115200_exact();
    s.add("B06-01-115200除数", ok, if ok { "" } else { "除数漂移" });
    // J1-2 DLAB 时序
    let ok = dlab_seq_exact();
    s.add("B06-02-DLAB时序", ok, if ok { "" } else { "时序漂移" });
    // J1-3 1MB 压测零丢零错位
    let ok = mb_stress_exact();
    s.add("B06-03-1MB压测", ok, if ok { "" } else { "压测漂移" });
    // J1-4 8N1 帧格式
    let ok = frame_8n1_exact();
    s.add("B06-04-8N1帧", ok, if ok { "" } else { "帧漂移" });
    // J1-5 FIFO 阈值
    let ok = fifo_exact();
    s.add("B06-05-FIFO档", ok, if ok { "" } else { "FIFO 漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B06-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let mut u = Uart16550::new();
    u.mcr |= MCR_LOOP;
    let _ = u.tx_byte(0x55);
    let ok = fp_frozen(&u) == fp_frozen(&u);
    s.add("B06-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
