//! UNX-F0126 · 引导进度可视化（阶段徽标）· 诊断段 0x2440
//! 验收锚点：UNX-F0126-J1 六阶段徽标进度账，阶段序乱与回退全拦，
//! 渲染含进度条账。零浮点、零 unwrap、零 expect。


use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2440;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAGE: u16 = SEG_BASE + 1; // 阶段越纲（六徽标闭集外）
const E_ORDER: u16 = SEG_BASE + 2; // 阶段回退（进度只进不退）
const E_TWICE: u16 = SEG_BASE + 3; // 徽标重复点亮
const E_RDONLY: u16 = SEG_BASE + 4;// 已定格（引导完成后再点被拒）
const E_EMPTY: u16 = SEG_BASE + 5; // 零徽标渲染
const E_BAL: u16 = SEG_BASE + 6;   // 进度账不平（点亮数 ≠ 条数）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STAGE => "阶段越纲：六徽标闭集外",
        E_ORDER => "阶段回退：进度条只进不退",
        E_TWICE => "徽标重复点亮：一阶段一徽",
        E_RDONLY => "引导已定格：终态后禁止再点",
        E_EMPTY => "零徽标：未点亮任何阶段禁止渲染",
        E_BAL => "进度账不平：徽标数与推进数不符",
        _ => "未知诊断码",
    }
}

/// 六徽标闭集（与 F0119 六阶段同源对齐）。
pub const BADGES: [&str; 6] = ["固件", "页表", "堆", "帧簿", "调度", "交接"];

/// 徽标进度账。
pub struct BootLogo {
    pub lit: [bool; 6],
    pub highest: usize, // 已达最远阶段（进度只进不退的根）
    pub frozen: bool,
    pub marks: u64,
}

impl BootLogo {
    pub fn new() -> BootLogo {
        BootLogo {
            lit: [false; 6],
            highest: 0,
            frozen: false,
            marks: 0,
        }
    }

    /// 点亮徽标：必须严格递进（可跳号，不可回退/重复）。
    pub fn light(&mut self, stage: usize) -> Result<usize, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if stage >= 6 {
            return Err(E_STAGE);
        }
        if self.lit[stage] {
            return Err(E_TWICE);
        }
        if stage < self.highest {
            return Err(E_ORDER);
        }
        self.lit[stage] = true;
        self.highest = stage;
        self.marks += 1;
        Ok(stage)
    }

    /// 定格：引导完成。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if self.marks == 0 {
            return Err(E_EMPTY);
        }
        self.frozen = true;
        Ok(())
    }

    /// 进度条渲染：[■■■□□□] 3/6。
    pub fn bar(&self) -> Result<String, u16> {
        if self.marks == 0 {
            return Err(E_EMPTY);
        }
        let n = self.lit.iter().filter(|b| **b).count();
        let mut s = String::from("[");
        for b in &self.lit {
            s.push_str(if *b { "■" } else { "□" });
        }
        s.push_str(&format!("] {}/6", n));
        Ok(s)
    }

    /// 徽标行渲染：已点亮阶段名 + 点缀。
    pub fn render(&self) -> Result<String, u16> {
        if self.marks == 0 {
            return Err(E_EMPTY);
        }
        let mut s = String::new();
        let mut first = true;
        for (i, b) in self.lit.iter().enumerate() {
            if *b {
                if !first {
                    s.push_str("·");
                }
                first = false;
                s.push_str(BADGES[i]);
            }
        }
        Ok(s)
    }

    /// 进度账守恒：marks == 点亮位数。
    pub fn audit(&self) -> Result<(), u16> {
        let n = self.lit.iter().filter(|b| **b).count() as u64;
        if n != self.marks {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：顺序六连点 → 满条 [■■■■■■] 6/6。
    let mut b = BootLogo::new();
    for s in 0..6 {
        b.light(s)?;
    }
    if b.bar()? != "[■■■■■■] 6/6" {
        return Err(E_BAL);
    }
    b.audit()?;
    b.freeze()?;
    hits += 1;

    // R2：跳号点亮（0→2→5）→ 合法（只进不退），渲染含三徽标。
    let mut b2 = BootLogo::new();
    b2.light(0)?;
    b2.light(2)?;
    b2.light(5)?;
    let rep = b2.render()?;
    if !rep.contains("固件·堆·交接") {
        return Err(E_BAL);
    }
    if b2.bar()? != "[■□■□□■] 3/6" {
        return Err(E_BAL);
    }
    b2.audit()?;
    hits += 1;

    // R3：定格后再点 → E_RDONLY；定格态渲染仍可读。
    b2.freeze()?;
    if b2.light(3) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if b2.render()?.is_empty() {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R4：单徽标边界（只点第 0 段）→ 渲染恰一名。
    let mut b4 = BootLogo::new();
    b4.light(0)?;
    if b4.render()? != "固件" {
        return Err(E_BAL);
    }
    b4.audit()?;
    hits += 1;

    // I1：阶段越纲（6）→ E_STAGE。
    let mut i1 = BootLogo::new();
    if i1.light(6) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I2：阶段回退（点亮 3 后回点 1）→ E_ORDER。
    let mut i2 = BootLogo::new();
    i2.light(3)?;
    if i2.light(1) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I3：重复点亮 → E_TWICE。
    let mut i3 = BootLogo::new();
    i3.light(2)?;
    if i3.light(2) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：零徽标渲染/进度条/定格 → E_EMPTY。
    let i4 = BootLogo::new();
    if i4.bar() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if i4.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    let mut i4b = BootLogo::new();
    if i4b.freeze() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I5：账破注入（marks 与位图不符）→ E_BAL；重复定格 → E_RDONLY。
    let mut i5 = BootLogo::new();
    i5.light(1)?;
    i5.marks = 3;
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut i5b = BootLogo::new();
    i5b.light(0)?;
    i5b.freeze()?;
    if i5b.freeze() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
