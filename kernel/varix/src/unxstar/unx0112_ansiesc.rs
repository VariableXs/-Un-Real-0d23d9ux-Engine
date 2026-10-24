//! UNX-F0112 · ANSI 转义子集（清屏/定位/颜色）· 诊断段 0x23A0
//! 验收锚点：UNX-F0112-J1 清屏/光标定位/设色三族转义确定性
//! 编解码往返，残缺序与越纲参数全拒。零浮点、零 unwrap、零 expect。


use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x23A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CMD: u16 = SEG_BASE + 1;   // 命令字非法（C/J/H/m 闭集外）
const E_ROW: u16 = SEG_BASE + 2;   // 行越纲（1~200 闭集外）
const E_COL: u16 = SEG_BASE + 3;   // 列越纲（1~80 闭集外）
const E_SGR: u16 = SEG_BASE + 4;   // SGR 参数非法（0/1/7/30-37/40-47 外）
const E_TRUNC: u16 = SEG_BASE + 5; // 序残缺（无终结字母）
const E_TWICE: u16 = SEG_BASE + 6; // 同帧重复清屏
const E_BAL: u16 = SEG_BASE + 7;   // 序账不平（编码数 ≠ 解码数）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_CMD => "命令字非法：C(清屏)/J(定位)/H(设色)/m 闭集外",
        E_ROW => "行越纲：定位行须 1~200",
        E_COL => "列越纲：定位列须 1~80",
        E_SGR => "SGR 参数非法：0 复位/1 加粗/7 反显/30-37/40-47",
        E_TRUNC => "转义序残缺：有 CSI 无终结字母",
        E_TWICE => "同帧重复清屏：一帧一清",
        E_BAL => "序账不平：编码条数与解码条数不等",
        _ => "未知诊断码",
    }
}

pub const ROW_MAX: u16 = 200;
pub const COL_MAX: u16 = 80;

/// 转义指令闭集。
#[derive(Clone, Debug, PartialEq)]
pub enum EscCmd {
    Clear,            // C —— 清屏
    Locate(u16, u16), // J 行,列 —— 光标定位
    Color(Vec<u8>),   // m —— SGR 参数串
}

/// 编码：指令 → ANSI 序。
pub fn encode(cmd: &EscCmd) -> Result<String, u16> {
    match cmd {
        EscCmd::Clear => Ok("\x1b[2J".to_string()),
        EscCmd::Locate(row, col) => {
            if *row == 0 || *row > ROW_MAX {
                return Err(E_ROW);
            }
            if *col == 0 || *col > COL_MAX {
                return Err(E_COL);
            }
            Ok(format!("\x1b[{};{}H", row, col))
        }
        EscCmd::Color(params) => {
            if params.is_empty() {
                return Err(E_SGR);
            }
            for p in params {
                let ok = *p == 0
                    || *p == 1
                    || *p == 7
                    || (30..=37).contains(p)
                    || (40..=47).contains(p);
                if !ok {
                    return Err(E_SGR);
                }
            }
            let strs: Vec<String> = params.iter().map(|p| p.to_string()).collect();
            Ok(format!("\x1b[{}m", strs.join(";")))
        }
    }
}

/// 解码：ANSI 序 → 指令。残缺/越纲逐码拒。
pub fn decode(s: &str) -> Result<EscCmd, u16> {
    let b = s.as_bytes();
    if b.len() < 3 || b[0] != 0x1B || b[1] != b'[' {
        return Err(E_TRUNC);
    }
    let term = b[b.len() - 1];
    let body = &s[2..s.len() - 1];
    match term {
        b'J' => {
            if body != "2" {
                return Err(E_CMD);
            }
            Ok(EscCmd::Clear)
        }
        b'H' => {
            let parts: Vec<&str> = body.split(';').collect();
            if parts.len() != 2 {
                return Err(E_TRUNC);
            }
            let row = parse_u16(parts[0], E_ROW)?;
            let col = parse_u16(parts[1], E_COL)?;
            if row == 0 || row > ROW_MAX {
                return Err(E_ROW);
            }
            if col == 0 || col > COL_MAX {
                return Err(E_COL);
            }
            Ok(EscCmd::Locate(row, col))
        }
        b'm' => {
            let parts: Vec<&str> = body.split(';').collect();
            let mut params: Vec<u8> = Vec::new();
            for p in parts {
                let v = parse_u16(p, E_SGR)?;
                if v > 255 {
                    return Err(E_SGR);
                }
                let ok = v == 0 || v == 1 || v == 7 || (30..=37).contains(&v) || (40..=47).contains(&v);
                if !ok {
                    return Err(E_SGR);
                }
                params.push(v as u8);
            }
            if params.is_empty() {
                return Err(E_SGR);
            }
            Ok(EscCmd::Color(params))
        }
        _ => Err(E_CMD),
    }
}

fn parse_u16(s: &str, err: u16) -> Result<u16, u16> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(err);
    }
    let mut v: u32 = 0;
    for c in s.bytes() {
        v = v * 10 + (c - b'0') as u32;
        if v > 0xFFFF {
            return Err(err);
        }
    }
    Ok(v as u16)
}

/// 序账：编码→解码逐条往返全等。
pub struct SeqBook {
    pub encoded: u64,
    pub decoded: u64,
}

impl SeqBook {
    pub fn new() -> SeqBook {
        SeqBook {
            encoded: 0,
            decoded: 0,
        }
    }

    pub fn round_trip(&mut self, cmd: &EscCmd) -> Result<(), u16> {
        let s = encode(cmd)?;
        self.encoded += 1;
        let back = decode(&s)?;
        self.decoded += 1;
        // 往返等价性核（指令结构级比较）。
        let eq = match (cmd, &back) {
            (EscCmd::Clear, EscCmd::Clear) => true,
            (EscCmd::Locate(r1, c1), EscCmd::Locate(r2, c2)) => r1 == r2 && c1 == c2,
            (EscCmd::Color(a), EscCmd::Color(b2)) => {
                a.len() == b2.len() && a.iter().zip(b2.iter()).all(|(x, y)| x == y)
            }
            _ => false,
        };
        if !eq {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.encoded != self.decoded {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三族编码字面核。
    if encode(&EscCmd::Clear)? != "\x1b[2J" {
        return Err(E_CMD);
    }
    if encode(&EscCmd::Locate(24, 80))? != "\x1b[24;80H" {
        return Err(E_ROW);
    }
    if encode(&EscCmd::Color(vec![1, 37, 41]))? != "\x1b[1;37;41m" {
        return Err(E_SGR);
    }
    hits += 1;

    // R2：解码往返——三族全过 + 序账守恒。
    let mut b = SeqBook::new();
    b.round_trip(&EscCmd::Clear)?;
    b.round_trip(&EscCmd::Locate(1, 1))?;
    b.round_trip(&EscCmd::Locate(ROW_MAX, COL_MAX))?;
    b.round_trip(&EscCmd::Color(vec![0]))?;
    b.round_trip(&EscCmd::Color(vec![7, 40]))?;
    b.audit()?;
    hits += 1;

    // R3：SGR 闭集边界——30/37/40/47 全收。
    for p in [30u8, 37, 40, 47] {
        b.round_trip(&EscCmd::Color(vec![p]))?;
    }
    b.audit()?;
    hits += 1;

    // R4：解码直读——"\x1b[12;34H" → Locate(12,34)。
    let cmd = decode("\x1b[12;34H")?;
    if cmd != EscCmd::Locate(12, 34) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：命令字非法（'X' / 小写 'j'）→ E_CMD。
    if decode("\x1b[2X") != Err(E_CMD) {
        return Err(E_CMD);
    }
    if decode("\x1b[2j") != Err(E_CMD) {
        return Err(E_CMD);
    }
    hits += 1;

    // I2：定位越纲（行 0/201、列 0/81）→ E_ROW/E_COL。
    if decode("\x1b[0;1H") != Err(E_ROW) {
        return Err(E_ROW);
    }
    if decode("\x1b[201;1H") != Err(E_ROW) {
        return Err(E_ROW);
    }
    if decode("\x1b[1;81H") != Err(E_COL) {
        return Err(E_COL);
    }
    hits += 1;

    // I3：SGR 非法（2 / 29 / 38 / 48 / 99）→ E_SGR。
    for bad in ["2", "29", "38", "48", "99"] {
        let s = format!("\x1b[{}m", bad);
        if decode(&s) != Err(E_SGR) {
            return Err(E_SGR);
        }
    }
    hits += 1;

    // I4：残缺序（无 CSI / 无终结字母 / 空体）→ E_TRUNC。
    if decode("x1b[2J") != Err(E_TRUNC) {
        return Err(E_TRUNC);
    }
    if decode("\x1b[2") != Err(E_TRUNC) {
        return Err(E_TRUNC);
    }
    if decode("\x1b[H") != Err(E_TRUNC) {
        return Err(E_TRUNC);
    }
    hits += 1;

    // I5：清屏重复帧 → E_TWICE（帧账语义层）；序账破 → E_BAL。
    let mut frame_clears: u32 = 0;
    for _ in 0..2 {
        let _ = encode(&EscCmd::Clear)?;
        frame_clears += 1;
    }
    if frame_clears > 1 {
        // 帧账规则：一帧至多一清屏，第二清即违规。
        return Err(E_TWICE);
    }
    let sb = SeqBook {
        encoded: 3,
        decoded: 2,
    };
    if sb.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
