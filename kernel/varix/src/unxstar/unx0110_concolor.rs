//! UNX-F0110 · 控制台色彩语义 · 诊断段 0x2380
//! 验收锚点：UNX-F0110-J1 五级日志→色彩映射语义化落账，属性
//! 组合（加粗/反显/下划线）与复位闭环，非法码全拒。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2380;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FG: u16 = SEG_BASE + 1;   // 前景色越纲（0~7 闭集外）
const E_BG: u16 = SEG_BASE + 2;   // 背景色越纲
const E_ATTR: u16 = SEG_BASE + 3; // 属性位越纲（保留位被置）
const E_SEQ: u16 = SEG_BASE + 4;  // 转义序残缺（有开无合）
const E_TWICE: u16 = SEG_BASE + 5;// 同帧重复设色（未先复位）
const E_BAL: u16 = SEG_BASE + 6;  // 色账不平（开数 ≠ 合数）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FG => "前景色越纲：0~7 八色闭集外",
        E_BG => "背景色越纲：0~7 八色闭集外",
        E_ATTR => "属性越纲：仅 加粗/反显/下划线 三位，保留位禁置",
        E_SEQ => "转义序残缺：设色后未复位即收笔",
        E_TWICE => "同帧重复设色：须先复位再设新色",
        E_BAL => "色账不平：开色数与复位数不等",
        _ => "未知诊断码",
    }
}

// ── 色彩常量 ─────────────────────────────────────────────────
pub const C_BLACK: u8 = 0;
pub const C_RED: u8 = 1;
pub const C_GREEN: u8 = 2;
pub const C_YELLOW: u8 = 3;
pub const C_BLUE: u8 = 4;
pub const C_MAGENTA: u8 = 5;
pub const C_CYAN: u8 = 6;
pub const C_WHITE: u8 = 7;

pub const A_BOLD: u8 = 0x01;
pub const A_REVERSE: u8 = 0x02;
pub const A_UNDER: u8 = 0x04;
pub const A_MASK: u8 = 0x07; // 属性合法掩码

/// 色名表（诊断渲染用）。
pub fn color_name(c: u8) -> Result<&'static str, u16> {
    match c {
        C_BLACK => Ok("黑"),
        C_RED => Ok("红"),
        C_GREEN => Ok("绿"),
        C_YELLOW => Ok("黄"),
        C_BLUE => Ok("蓝"),
        C_MAGENTA => Ok("品"),
        C_CYAN => Ok("青"),
        C_WHITE => Ok("白"),
        _ => Err(E_FG),
    }
}

/// 日志级别 → 语义色（五级闭集，与 F0105 对齐）。
pub fn level_color(level: u8) -> Result<(u8, u8, u8), u16> {
    match level {
        0 => Ok((C_CYAN, C_BLACK, 0)),               // DEBUG  青底黑字
        1 => Ok((C_WHITE, C_BLACK, 0)),              // INFO   白底黑字
        2 => Ok((C_BLACK, C_YELLOW, 0)),             // WARN   黑字黄底
        3 => Ok((C_WHITE, C_RED, A_BOLD)),           // ERROR  白字红底加粗
        4 => Ok((C_WHITE, C_RED, A_BOLD | A_REVERSE)), // FATAL 反显加粗
        _ => Err(E_FG),
    }
}

/// ANSI 前景/背景/属性三段编码（SGR 参数闭集）。
pub fn sgr_encode(fg: u8, bg: u8, attr: u8) -> Result<String, u16> {
    if fg > 7 {
        return Err(E_FG);
    }
    if bg > 7 {
        return Err(E_BG);
    }
    if attr & !A_MASK != 0 {
        return Err(E_ATTR);
    }
    let mut parts: Vec<String> = Vec::new();
    if attr & A_BOLD != 0 {
        parts.push("1".to_string());
    }
    if attr & A_UNDER != 0 {
        parts.push("4".to_string());
    }
    if attr & A_REVERSE != 0 {
        parts.push("7".to_string());
    }
    parts.push(format!("{}", 30 + fg));
    parts.push(format!("{}", 40 + bg));
    Ok(format!("\\x1b[{}m", parts.join(";")))
}

/// 色账：逐帧设色/复位闭环核。
pub struct ColorBook {
    pub open: u64,
    pub reset: u64,
    pub cur: Option<(u8, u8, u8)>,
    pub lines: Vec<String>,
}

impl ColorBook {
    pub fn new() -> ColorBook {
        ColorBook {
            open: 0,
            reset: 0,
            cur: None,
            lines: Vec::new(),
        }
    }

    /// 设色：已开色未复位再设 → E_TWICE。
    pub fn set(&mut self, fg: u8, bg: u8, attr: u8) -> Result<String, u16> {
        if self.cur.is_some() {
            return Err(E_TWICE);
        }
        let s = sgr_encode(fg, bg, attr)?;
        self.cur = Some((fg, bg, attr));
        self.open += 1;
        self.lines.push(s.clone());
        Ok(s)
    }

    /// 复位：无开色复位 → 残缺序的镜像错误（账不平方向），也归 E_SEQ。
    pub fn reset(&mut self) -> Result<String, u16> {
        if self.cur.is_none() {
            return Err(E_SEQ);
        }
        self.cur = None;
        self.reset += 1;
        Ok("\\x1b[0m".to_string())
    }

    /// 色账守恒：开 = 合 且无未闭色。
    pub fn audit(&self) -> Result<(), u16> {
        if self.open != self.reset || self.cur.is_some() {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 按五级日志语义批量着色：每级一帧，帧帧闭合。
    pub fn paint_levels(&mut self) -> Result<usize, u16> {
        let mut n = 0;
        for lv in 0..5u8 {
            let (fg, bg, attr) = level_color(lv)?;
            self.set(fg, bg, attr)?;
            self.reset()?;
            n += 1;
        }
        Ok(n)
    }

    pub fn render(&self) -> String {
        format!(
            "色账 开 {} 合 {} 未闭 {}",
            self.open,
            self.reset,
            if self.cur.is_some() { 1 } else { 0 }
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：五级→色彩语义映射逐级核（DEBUG 青 / FATAL 反显红）。
    let (fg0, bg0, a0) = level_color(0)?;
    if fg0 != C_CYAN || bg0 != C_BLACK || a0 != 0 {
        return Err(E_FG);
    }
    let (fg4, bg4, a4) = level_color(4)?;
    if fg4 != C_WHITE || bg4 != C_RED || a4 != (A_BOLD | A_REVERSE) {
        return Err(E_FG);
    }
    hits += 1;

    // R2：SGR 编码——ERROR 帧 = "1;37;41"，属性位序确定。
    let s = sgr_encode(C_WHITE, C_RED, A_BOLD)?;
    if s != "\\x1b[1;37;41m" {
        return Err(E_ATTR);
    }
    let s2 = sgr_encode(C_BLACK, C_YELLOW, 0)?;
    if s2 != "\\x1b[30;43m" {
        return Err(E_ATTR);
    }
    hits += 1;

    // R3：色账闭环——批量着色 5 帧，开合相等，审计绿。
    let mut b = ColorBook::new();
    let n = b.paint_levels()?;
    if n != 5 {
        return Err(E_BAL);
    }
    b.audit()?;
    let rep = b.render();
    if !rep.contains("开 5 合 5 未闭 0") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：八色名表闭集全覆盖。
    for c in 0..8u8 {
        if color_name(c)?.is_empty() {
            return Err(E_FG);
        }
    }
    hits += 1;

    // I1：前景越纲（8 / 255）→ E_FG。
    if sgr_encode(8, C_BLACK, 0) != Err(E_FG) {
        return Err(E_FG);
    }
    if sgr_encode(255, C_BLACK, 0) != Err(E_FG) {
        return Err(E_FG);
    }
    if level_color(5).is_err() && level_color(5) != Err(E_FG) {
        return Err(E_FG);
    }
    hits += 1;

    // I2：背景越纲（9）→ E_BG。
    if sgr_encode(C_WHITE, 9, 0) != Err(E_BG) {
        return Err(E_BG);
    }
    hits += 1;

    // I3：属性保留位置位（0x08 / 0xFF）→ E_ATTR。
    if sgr_encode(C_WHITE, C_BLACK, 0x08) != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    if sgr_encode(C_WHITE, C_BLACK, 0xFF) != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I4：同帧重复设色 → E_TWICE；无开复位 → E_SEQ。
    let mut b2 = ColorBook::new();
    b2.set(C_GREEN, C_BLACK, 0)?;
    if b2.set(C_RED, C_BLACK, 0) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    let mut b3 = ColorBook::new();
    if b3.reset() != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I5：色账破——开 1 合 0 → E_BAL（含未闭色）。
    let mut b4 = ColorBook::new();
    b4.set(C_WHITE, C_BLACK, 0)?;
    if b4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
