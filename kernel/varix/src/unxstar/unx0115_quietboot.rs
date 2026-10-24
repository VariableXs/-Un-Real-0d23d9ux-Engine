//! UNX-F0115 · 静默引导模式（quiet）· 诊断段 0x23D0
//! 验收锚点：UNX-F0115-J1 quiet 下普通行全吞、warn 以上必出，
//! 诊断模式一键开闸。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x23D0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LEVEL: u16 = SEG_BASE + 1;  // 级别越纲（五级外）
const E_MODE: u16 = SEG_BASE + 2;   // 模式越纲（三态外）
const E_RDONLY: u16 = SEG_BASE + 3; // 引导定型后再切模式被拒
const E_LEAK: u16 = SEG_BASE + 4;   // 静默泄漏（warn+ 行被吞）
const E_BAL: u16 = SEG_BASE + 5;    // 吞行账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_LEVEL => "级别越纲：五级闭集外",
        E_MODE => "模式越纲：[常规, 静默, 诊断] 三态外",
        E_RDONLY => "引导已定型：模式锁定不可再切",
        E_LEAK => "静默泄漏：warn 及以上级别被吞属违规",
        E_BAL => "吞行账不平：喂入数 ≠ 出行数 + 吞行数",
        _ => "未知诊断码",
    }
}

// 五级（与 F0105/F0113 闭集对齐）。
pub const L_DEBUG: u8 = 0;
pub const L_INFO: u8 = 1;
pub const L_WARN: u8 = 2;
pub const L_ERROR: u8 = 3;
pub const L_FATAL: u8 = 4;

/// 引导输出模式三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    Normal,  // 常规：全出行
    Quiet,   // 静默：debug/info 吞，warn+ 必出
    Diag,    // 诊断：全出行 + 打标
}

pub fn mode_name(m: BootMode) -> &'static str {
    match m {
        BootMode::Normal => "常规",
        BootMode::Quiet => "静默",
        BootMode::Diag => "诊断",
    }
}

/// 静默闸：喂行 → 按模式过滤，吞行记账。
pub struct QuietGate {
    pub mode: Option<BootMode>,
    pub locked: bool,
    pub fed: u64,
    pub out: u64,
    pub swallowed: u64,
    pub leaked_warn_plus: u64, // 违规吞掉的 warn+ 行（须恒 0）
}

impl QuietGate {
    pub fn new() -> QuietGate {
        QuietGate {
            mode: None,
            locked: false,
            fed: 0,
            out: 0,
            swallowed: 0,
            leaked_warn_plus: 0,
        }
    }

    /// 定模式：锁定前可重设。
    pub fn set_mode(&mut self, m: BootMode) -> Result<(), u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        self.mode = Some(m);
        Ok(())
    }

    /// 定型：锁死后不可再切。
    pub fn finalize(&mut self) -> Result<BootMode, u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        match self.mode {
            Some(m) => {
                self.locked = true;
                Ok(m)
            }
            None => Err(E_MODE),
        }
    }

    /// 喂一行：返回是否出闸。静默下 warn+ 必出（零泄漏判据）。
    pub fn feed(&mut self, level: u8, text: &str) -> Result<bool, u16> {
        if level > L_FATAL {
            return Err(E_LEVEL);
        }
        if !self.locked {
            return Err(E_MODE);
        }
        self.fed += 1;
        let m = self.mode.unwrap_or(BootMode::Normal);
        let show = match m {
            BootMode::Normal | BootMode::Diag => true,
            BootMode::Quiet => level >= L_WARN,
        };
        if show {
            self.out += 1;
        } else {
            self.swallowed += 1;
            if level >= L_WARN {
                self.leaked_warn_plus += 1;
            }
        }
        let _ = text;
        Ok(show)
    }

    /// 零泄漏判据：静默模式下 warn+ 被吞数必须为 0。
    pub fn no_leak(&self) -> Result<(), u16> {
        if self.leaked_warn_plus != 0 {
            return Err(E_LEAK);
        }
        Ok(())
    }

    /// 吞行账守恒：fed = out + swallowed。
    pub fn audit(&self) -> Result<(), u16> {
        if self.fed != self.out + self.swallowed {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        match self.mode {
            None => Err(E_MODE),
            Some(m) => Ok(format!(
                "{}模式：喂 {} 出 {} 吞 {}",
                mode_name(m),
                self.fed,
                self.out,
                self.swallowed
            )),
        }
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：静默模式喂五级各一行 → debug/info 吞，warn/error/fatal 出。
    let mut g = QuietGate::new();
    g.set_mode(BootMode::Quiet)?;
    g.finalize()?;
    let shows: Vec<bool> = (0..=4u8)
        .map(|lv| g.feed(lv, "x").unwrap_or(false))
        .collect();
    // 注：unwrap_or 兜底仅为演示装填，实际 feed 已按序成功。
    if shows != [false, false, true, true, true] {
        return Err(E_LEAK);
    }
    g.no_leak()?;
    g.audit()?;
    hits += 1;

    // R2：常规与诊断模式全出行（诊断打标由渲染层担，闸层只放行）。
    for m in [BootMode::Normal, BootMode::Diag] {
        let mut g2 = QuietGate::new();
        g2.set_mode(m)?;
        g2.finalize()?;
        for lv in 0..=4u8 {
            if !g2.feed(lv, "y")? {
                return Err(E_BAL);
            }
        }
        g2.no_leak()?;
        g2.audit()?;
    }
    hits += 1;

    // R3：渲染账含模式名与三计数。
    let rep = g.render()?;
    if !rep.contains("静默模式") || !rep.contains("喂 5 出 3 吞 2") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：静默下 warn+ 连发全出（零泄漏批量核）。
    let mut g4 = QuietGate::new();
    g4.set_mode(BootMode::Quiet)?;
    g4.finalize()?;
    let mut out_n = 0;
    for _ in 0..10 {
        if g4.feed(L_ERROR, "e")? {
            out_n += 1;
        }
    }
    if out_n != 10 {
        return Err(E_LEAK);
    }
    g4.no_leak()?;
    hits += 1;

    // I1：级别越纲（5 级）→ E_LEVEL。
    let mut i1 = QuietGate::new();
    i1.set_mode(BootMode::Normal)?;
    i1.finalize()?;
    if i1.feed(5, "z") != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    hits += 1;

    // I2：未定型喂行 → E_MODE；未定型渲染 → E_MODE。
    let mut i2 = QuietGate::new();
    if i2.feed(L_INFO, "a") != Err(E_MODE) {
        return Err(E_MODE);
    }
    if i2.render() != Err(E_MODE) {
        return Err(E_MODE);
    }
    hits += 1;

    // I3：定型后再切模式 / 再定型 → E_RDONLY。
    if i1.set_mode(BootMode::Quiet) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i1.finalize() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I4：泄漏注入（人为吞 warn 行）→ no_leak E_LEAK。
    let mut i4 = QuietGate::new();
    i4.set_mode(BootMode::Quiet)?;
    i4.finalize()?;
    i4.fed += 1;
    i4.swallowed += 1;
    i4.leaked_warn_plus += 1; // 人为制造泄漏
    if i4.no_leak() != Err(E_LEAK) {
        return Err(E_LEAK);
    }
    hits += 1;

    // I5：账破注入（喂 3 出 1 吞 1）→ E_BAL。
    let mut i5 = QuietGate::new();
    i5.set_mode(BootMode::Normal)?;
    i5.finalize()?;
    i5.fed = 3;
    i5.out = 1;
    i5.swallowed = 1;
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
