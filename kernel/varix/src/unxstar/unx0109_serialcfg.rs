//! UNX-F0109 · 串口参数可配置（cmdline）· 诊断段 0x2370
//! 验收锚点：UNX-F0109-J1 波特率/数据位/停止位/校验四参自
//! cmdline 解析落账，非法组合全拒。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2370;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAUD: u16 = SEG_BASE + 1;  // 波特率不在支持闭集
const E_BITS: u16 = SEG_BASE + 2;  // 数据位非法（5~8）
const E_STOP: u16 = SEG_BASE + 3;  // 停止位非法（1/2）
const E_PARITY: u16 = SEG_BASE + 4;// 校验位字符非法（n/e/o）
const E_SYNTAX: u16 = SEG_BASE + 5;// cmdline 语法错（缺键/缺值/多余段）
const E_TWICE: u16 = SEG_BASE + 6; // 同参数重复出现
const E_RDONLY: u16 = SEG_BASE + 7;// 已定型（串口锁定后再改被拒）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BAUD => "波特率非法：支持闭集 [9600,19200,38400,57600,115200]",
        E_BITS => "数据位非法：5~8 闭集",
        E_STOP => "停止位非法：1 或 2",
        E_PARITY => "校验位非法：n/e/o 三态闭集",
        E_SYNTAX => "cmdline 语法错：须为 uart=波特,位,停,校验",
        E_TWICE => "参数重复：同键二次出现",
        E_RDONLY => "串口已定型：锁定后不可再配",
        _ => "未知诊断码",
    }
}

pub const BAUD_SET: [u32; 5] = [9600, 19200, 38400, 57600, 115200];

/// 串口参数四元组。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UartCfg {
    pub baud: u32,
    pub data_bits: u8,  // 5~8
    pub stop_bits: u8,  // 1/2
    pub parity: u8,     // b'n' / b'e' / b'o'
}

impl UartCfg {
    pub fn validate(&self) -> Result<(), u16> {
        if !BAUD_SET.contains(&self.baud) {
            return Err(E_BAUD);
        }
        if self.data_bits < 5 || self.data_bits > 8 {
            return Err(E_BITS);
        }
        if self.stop_bits != 1 && self.stop_bits != 2 {
            return Err(E_STOP);
        }
        if !matches!(self.parity, b'n' | b'e' | b'o') {
            return Err(E_PARITY);
        }
        Ok(())
    }

    /// 渲染为规范串（与 cmdline 回写格式一致）。
    pub fn render(&self) -> String {
        format!(
            "uart={},{}{}{}",
            self.baud,
            self.data_bits,
            self.stop_bits,
            self.parity as char
        )
    }
}

/// cmdline 解析：uart=115200,8,1,n 单键格式。
/// 返回四元组；语法/数值/组合错误逐码回报。
pub fn parse_uart(kv: &str) -> Result<UartCfg, u16> {
    let (key, val) = match kv.split_once('=') {
        Some((k, v)) => (k, v),
        None => return Err(E_SYNTAX),
    };
    if key != "uart" {
        return Err(E_SYNTAX);
    }
    let parts: Vec<&str> = val.split(',').collect();
    if parts.len() != 4 {
        return Err(E_SYNTAX);
    }
    // 波特率：全数字且在闭集。
    let baud_s = parts[0];
    if baud_s.is_empty() || !baud_s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(E_BAUD);
    }
    let mut baud: u32 = 0;
    for b in baud_s.bytes() {
        baud = baud
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
            .ok_or(E_BAUD)?;
    }
    if !BAUD_SET.contains(&baud) {
        return Err(E_BAUD);
    }
    // 数据位：单字符数字 5~8。
    let db = parts[1];
    if db.len() != 1 || !db.as_bytes()[0].is_ascii_digit() {
        return Err(E_BITS);
    }
    let data_bits = db.as_bytes()[0] - b'0';
    if data_bits < 5 || data_bits > 8 {
        return Err(E_BITS);
    }
    // 停止位：1/2。
    let sb = parts[2];
    if sb.len() != 1 || !matches!(sb, "1" | "2") {
        return Err(E_STOP);
    }
    let stop_bits = sb.as_bytes()[0] - b'0';
    // 校验位：n/e/o。
    let pb = parts[3];
    if pb.len() != 1 {
        return Err(E_PARITY);
    }
    let parity = pb.as_bytes()[0];
    if !matches!(parity, b'n' | b'e' | b'o') {
        return Err(E_PARITY);
    }
    let cfg = UartCfg {
        baud,
        data_bits,
        stop_bits,
        parity,
    };
    cfg.validate()?;
    Ok(cfg)
}

/// 串口配置账：接受整条 cmdline（空格分键），uart 键至多一次。
pub struct SerialBook {
    pub cfg: Option<UartCfg>,
    pub locked: bool,
    pub keys_seen: u64,
}

impl SerialBook {
    pub fn new() -> SerialBook {
        SerialBook {
            cfg: None,
            locked: false,
            keys_seen: 0,
        }
    }

    /// 吃一条 cmdline：其余键跳过（归属他模块），uart 键解析定型。
    pub fn feed_cmdline(&mut self, line: &str) -> Result<UartCfg, u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        let mut got: Option<UartCfg> = None;
        for tok in line.split_whitespace() {
            if tok.starts_with("uart=") {
                if got.is_some() || self.cfg.is_some() {
                    return Err(E_TWICE);
                }
                got = Some(parse_uart(tok)?);
            }
            self.keys_seen += 1;
        }
        match got {
            Some(c) => {
                self.cfg = Some(c);
                Ok(c)
            }
            None => Err(E_SYNTAX), // cmdline 里压根没有 uart 键
        }
    }

    /// 定型：锁死后不可再配（引导交接语义）。
    pub fn lock(&mut self) -> Result<UartCfg, u16> {
        match self.cfg {
            Some(c) => {
                self.locked = true;
                Ok(c)
            }
            None => Err(E_SYNTAX),
        }
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：标准串解析 → 115200,8,1,n 落账 + 回写渲染与输入一致。
    let c = parse_uart("uart=115200,8,1,n")?;
    if c.baud != 115200 || c.data_bits != 8 || c.stop_bits != 1 || c.parity != b'n' {
        return Err(E_SYNTAX);
    }
    if c.render() != "uart=115200,8,1,n" {
        return Err(E_SYNTAX);
    }
    hits += 1;

    // R2：cmdline 混排（他键共存）+ 锁定 → 定型后再配被拒。
    let mut b = SerialBook::new();
    let c2 = b.feed_cmdline("console=tty0 uart=57600,7,2,e quiet")?;
    if c2.baud != 57600 || c2.parity != b'e' || c2.stop_bits != 2 {
        return Err(E_SYNTAX);
    }
    b.lock()?;
    if b.feed_cmdline("uart=9600,8,1,n") != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // R3：波特率闭集五点全过（边界 9600 与 115200）。
    for s in ["9600", "19200", "38400", "57600", "115200"] {
        let kv = format!("uart={},8,1,n", s);
        parse_uart(&kv)?;
    }
    hits += 1;

    // I1：波特率非法（闭集外 4800 / 全零串 / 溢出超长）→ E_BAUD。
    if parse_uart("uart=4800,8,1,n") != Err(E_BAUD) {
        return Err(E_BAUD);
    }
    if parse_uart("uart=,8,1,n") != Err(E_BAUD) {
        return Err(E_BAUD);
    }
    if parse_uart("uart=12a4,8,1,n") != Err(E_BAUD) {
        return Err(E_BAUD);
    }
    hits += 1;

    // I2：数据位非法（4 与 9）→ E_BITS。
    if parse_uart("uart=9600,4,1,n") != Err(E_BITS) {
        return Err(E_BITS);
    }
    if parse_uart("uart=9600,9,1,n") != Err(E_BITS) {
        return Err(E_BITS);
    }
    hits += 1;

    // I3：停止位非法（0 与 3）→ E_STOP；校验位非法（x）→ E_PARITY。
    if parse_uart("uart=9600,8,0,n") != Err(E_STOP) {
        return Err(E_STOP);
    }
    if parse_uart("uart=9600,8,3,n") != Err(E_STOP) {
        return Err(E_STOP);
    }
    if parse_uart("uart=9600,8,1,x") != Err(E_PARITY) {
        return Err(E_PARITY);
    }
    hits += 1;

    // I4：语法错族——缺等号 / 键名错 / 段数不对 / 无 uart 键 → E_SYNTAX。
    if parse_uart("115200,8,1,n") != Err(E_SYNTAX) {
        return Err(E_SYNTAX);
    }
    if parse_uart("tty=115200,8,1,n") != Err(E_SYNTAX) {
        return Err(E_SYNTAX);
    }
    if parse_uart("uart=115200,8,1") != Err(E_SYNTAX) {
        return Err(E_SYNTAX);
    }
    let mut i4 = SerialBook::new();
    if i4.feed_cmdline("quiet console=tty0") != Err(E_SYNTAX) {
        return Err(E_SYNTAX);
    }
    hits += 1;

    // I5：uart 键重复（同行两次 / 跨行二次）→ E_TWICE。
    let mut i5 = SerialBook::new();
    if i5.feed_cmdline("uart=9600,8,1,n uart=9600,8,1,n") != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    let mut i5b = SerialBook::new();
    i5b.feed_cmdline("uart=9600,8,1,n")?;
    if i5b.feed_cmdline("uart=9600,8,1,n") != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    Ok(hits)
}
