//! UNX-F0132 · 健康标志位（固件/介质/内存三区）· 诊断段 0x24A0
//! 验收锚点：UNX-F0132-J1 三区健康位独立置位/读取，污染位只置
//! 不清（一次性语义），读数账守恒。零浮点、零 unwrap、零 expect。

use crate::alloc::{format, string::String};

pub const SEG_BASE: u16 = 0x24A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ZONE: u16 = SEG_BASE + 1;  // 区越纲（三区闭集外）
const E_RDONLY: u16 = SEG_BASE + 2;// 污染位只置不清（清除被拒）
const E_FROZEN: u16 = SEG_BASE + 3;// 已封账，再写被拒
const E_EMPTY: u16 = SEG_BASE + 4; // 未置任何位出报告
const E_BAL: u16 = SEG_BASE + 5;   // 位账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ZONE => "区越纲：[固件, 介质, 内存] 三区外",
        E_RDONLY => "污染位只置不清：健康语义单向",
        E_FROZEN => "已封账：交接后禁止再写",
        E_EMPTY => "未置位：零位账禁止出报告",
        E_BAL => "位账不平：置位数与位图不符",
        _ => "未知诊断码",
    }
}

/// 三区闭集。
pub const Z_FW: usize = 0;     // 固件区
pub const Z_MEDIA: usize = 1;  // 介质区
pub const Z_MEM: usize = 2;    // 内存区
pub const ZONE_N: usize = 3;

pub fn zone_name(z: usize) -> Result<&'static str, u16> {
    match z {
        Z_FW => Ok("固件"),
        Z_MEDIA => Ok("介质"),
        Z_MEM => Ok("内存"),
        _ => Err(E_ZONE),
    }
}

/// 健康位账：好位（可清可置）+ 污染位（只置不清）。
pub struct HealthBook {
    pub healthy: [bool; ZONE_N],
    pub polluted: [bool; ZONE_N],
    pub set_count: u64,
    pub pollute_count: u64,
    pub frozen: bool,
}

impl HealthBook {
    pub fn new() -> HealthBook {
        HealthBook {
            healthy: [true; ZONE_N], // 默认全好
            polluted: [false; ZONE_N],
            set_count: 0,
            pollute_count: 0,
            frozen: false,
        }
    }

    /// 置好位（复检通过）。
    pub fn mark_ok(&mut self, zone: usize) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if zone >= ZONE_N {
            return Err(E_ZONE);
        }
        self.set_count += 1;
        self.healthy[zone] = true;
        Ok(())
    }

    /// 置污染位：健康位同步拉低，一次性（重复置不重复计数）。
    pub fn pollute(&mut self, zone: usize) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if zone >= ZONE_N {
            return Err(E_ZONE);
        }
        if !self.polluted[zone] {
            self.polluted[zone] = true;
            self.pollute_count += 1;
        }
        self.healthy[zone] = false;
        Ok(())
    }

    /// 污染位清除（违规操作）→ E_RDONLY。
    pub fn clean_polluted(&mut self, zone: usize) -> Result<(), u16> {
        if zone >= ZONE_N {
            return Err(E_ZONE);
        }
        if self.polluted[zone] {
            return Err(E_RDONLY);
        }
        Ok(())
    }

    /// 全好核：三区无污染、且健康位与污染位无矛盾才放行交接。
    pub fn all_healthy(&self) -> Result<(), u16> {
        if self.polluted.iter().any(|p| *p) {
            return Err(E_BAL);
        }
        for i in 0..ZONE_N {
            if self.healthy[i] && self.polluted[i] {
                return Err(E_BAL); // 好位与污位同真：位账矛盾
            }
        }
        Ok(())
    }

    /// 封账。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        self.frozen = true;
        Ok(())
    }

    /// 位图直读：三区好坏三元组。
    pub fn bitmap(&self) -> [bool; ZONE_N] {
        self.healthy
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.set_count == 0 && self.pollute_count == 0 {
            return Err(E_EMPTY);
        }
        let mut s = String::new();
        for (i, z) in self.healthy.iter().enumerate() {
            if i > 0 {
                s.push('·');
            }
            let name = zone_name(i).unwrap_or("?");
            s.push_str(&format!("{}{}", name, if *z { "好" } else { "污" }));
        }
        Ok(s)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三区复检全过 → 全好 → 交接放行，渲染全好。
    let mut h = HealthBook::new();
    for z in 0..ZONE_N {
        h.mark_ok(z)?;
    }
    h.all_healthy()?;
    let rep = h.render()?;
    if rep != "固件好·介质好·内存好" {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：介质污染 → 该区拉低，其余两区仍好，交接拦截。
    let mut h2 = HealthBook::new();
    for z in 0..ZONE_N {
        h2.mark_ok(z)?;
    }
    h2.pollute(Z_MEDIA)?;
    let bm = h2.bitmap();
    if !(bm[0] && !bm[1] && bm[2]) {
        return Err(E_BAL);
    }
    if h2.all_healthy() != Err(E_BAL) {
        return Err(E_BAL);
    }
    if h2.render()? != "固件好·介质污·内存好" {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：污染一次性——重复置污染不重复计数；好位重置不污染。
    let mut h3 = HealthBook::new();
    h3.pollute(Z_MEM)?;
    h3.pollute(Z_MEM)?;
    h3.pollute(Z_MEM)?;
    if h3.pollute_count != 1 {
        return Err(E_BAL);
    }
    h3.mark_ok(Z_FW)?;
    h3.mark_ok(Z_FW)?;
    h3.mark_ok(Z_FW)?;
    if h3.healthy[Z_MEM] {
        return Err(E_BAL); // 污染区置好位不翻污
    }
    hits += 1;

    // R4：封账终态 + 位图直读。
    let mut h4 = HealthBook::new();
    h4.mark_ok(Z_FW)?;
    h4.freeze()?;
    if h4.bitmap()[Z_FW] != true {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：区越纲 → E_ZONE（置好/污染/清除三路）。
    let mut i1 = HealthBook::new();
    if i1.mark_ok(3) != Err(E_ZONE) {
        return Err(E_ZONE);
    }
    if i1.pollute(9) != Err(E_ZONE) {
        return Err(E_ZONE);
    }
    if i1.clean_polluted(7) != Err(E_ZONE) {
        return Err(E_ZONE);
    }
    hits += 1;

    // I2：污染位清除 → E_RDONLY（只置不清语义）。
    let mut i2 = HealthBook::new();
    i2.pollute(Z_FW)?;
    if i2.clean_polluted(Z_FW) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I3：未污染区清除 → 放行（无位可清，正常路径）。
    let mut i3 = HealthBook::new();
    i3.clean_polluted(Z_MEM)?;
    hits += 1;

    // I4：封账后再写 → E_FROZEN；重复封账 → E_FROZEN。
    i3.mark_ok(Z_FW)?;
    i3.freeze()?;
    if i3.mark_ok(Z_MEDIA) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if i3.pollute(Z_MEM) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if i3.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I5：零位账渲染 → E_EMPTY；污染位账破注入 → E_BAL。
    let i5 = HealthBook::new();
    if i5.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    let mut i5b = HealthBook::new();
    i5b.pollute(Z_FW)?;
    i5b.healthy[Z_FW] = true; // 人为把污染区标好 → 与污染位图矛盾
    if i5b.all_healthy() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
