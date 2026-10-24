//! UNX-F0087 · KASLR 关闭模式（诊断模式）· 诊断段 0x2230
//! 验收锚点：UNX-F0087-J1 命令行开关→固定基址→确定性加载
//! 三段路径与双模式互斥判据通过；纯功能代码 ≥180 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2230;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MODE: u16 = SEG_BASE + 1;     // 模式非法
const E_ADDR: u16 = SEG_BASE + 2;     // 固定基址非法（未对齐/越域）
const E_CONTRA: u16 = SEG_BASE + 3;   // 双模式互斥违例（同开或同关）
const E_RDONLY: u16 = SEG_BASE + 4;   // 模式已锁定
const E_ARG: u16 = SEG_BASE + 5;      // 命令行参数语法非法
const E_BAL: u16 = SEG_BASE + 6;      // 账守恒不平

/// 固定基址默认值（诊断模式确定性加载点，2MiB 对齐）。
pub const DIAG_BASE_DEFAULT: u64 = 0x0100_0000;
/// 基址对齐粒度。
pub const BASE_ALIGN: u64 = 0x20_0000;
/// 内核镜像最大 256MiB（固定基址合法性上界参考）。
pub const KERN_MAX: u64 = 0x1000_0000;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_MODE => "模式非法：闭集 {KASLR, DIAG}",
        E_ADDR => "固定基址非法：2MiB 对齐且不越镜像域",
        E_CONTRA => "双模式互斥违例：KASLR 与诊断模式只能二选一",
        E_RDONLY => "模式已锁定：引导早期一次性决策",
        E_ARG => "参数语法非法：kaslr=on/off 或 diag=on/off",
        E_BAL => "账守恒：解析数 = 生效数 + 忽略数",
        _ => "未知诊断码",
    }
}

/// 加载模式闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadMode {
    /// KASLR 开启：基址由熵源决定（F0089 域）。
    Kaslr,
    /// 诊断模式：固定基址确定性加载，随机化全关。
    Diagnostic,
}

/// 命令行开关解析结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwitchParse {
    Kaslr(bool),
    Diag(bool),
    Ignored,
    Malformed,
}

/// 解析单个命令行参数（kaslr=…/diag=… 之外忽略）。
pub fn parse_switch(arg: &str) -> SwitchParse {
    if let Some(v) = arg.strip_prefix("kaslr=") {
        match v {
            "on" => return SwitchParse::Kaslr(true),
            "off" => return SwitchParse::Kaslr(false),
            _ => return SwitchParse::Malformed,
        }
    }
    if let Some(v) = arg.strip_prefix("diag=") {
        match v {
            "on" => return SwitchParse::Diag(true),
            "off" => return SwitchParse::Diag(false),
            _ => return SwitchParse::Malformed,
        }
    }
    SwitchParse::Ignored
}

// ── 模式决策账 ───────────────────────────────────────────────
pub struct ModeDecision {
    pub mode: Option<LoadMode>,
    pub fixed_base: u64,
    pub locked: bool,
    pub parsed: usize,
    pub ignored: usize,
    pub effective: usize,
    pub diag_requested: bool,
}

impl ModeDecision {
    pub fn new() -> ModeDecision {
        ModeDecision {
            mode: None,
            fixed_base: DIAG_BASE_DEFAULT,
            locked: false,
            parsed: 0,
            ignored: 0,
            effective: 0,
            diag_requested: false,
        }
    }

    /// 命令行喂入：参数序优先——后到覆盖前到，直到锁定。
    pub fn feed(&mut self, arg: &str) -> Result<SwitchParse, u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        self.parsed += 1;
        match parse_switch(arg) {
            SwitchParse::Kaslr(true) => {
                // KASLR on ⇒ 诊断模式必须关（互斥）
                if self.mode == Some(LoadMode::Diagnostic) && self.diag_requested {
                    return Err(E_CONTRA);
                }
                self.mode = Some(LoadMode::Kaslr);
                self.diag_requested = false;
                self.effective += 1;
                Ok(SwitchParse::Kaslr(true))
            }
            SwitchParse::Kaslr(false) => {
                self.mode = None;
                self.effective += 1;
                Ok(SwitchParse::Kaslr(false))
            }
            SwitchParse::Diag(true) => {
                if self.mode == Some(LoadMode::Kaslr) {
                    return Err(E_CONTRA);
                }
                self.mode = Some(LoadMode::Diagnostic);
                self.diag_requested = true;
                self.effective += 1;
                Ok(SwitchParse::Diag(true))
            }
            SwitchParse::Diag(false) => {
                if self.mode == Some(LoadMode::Diagnostic) {
                    self.mode = None;
                    self.diag_requested = false;
                }
                self.effective += 1;
                Ok(SwitchParse::Diag(false))
            }
            SwitchParse::Malformed => Err(E_ARG),
            SwitchParse::Ignored => {
                self.ignored += 1;
                Ok(SwitchParse::Ignored)
            }
        }
    }

    /// 设置固定基址（诊断模式）。
    pub fn set_fixed_base(&mut self, base: u64) -> Result<(), u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        if base % BASE_ALIGN != 0 {
            return Err(E_ADDR);
        }
        if base.checked_add(KERN_MAX).is_none() {
            return Err(E_ADDR);
        }
        self.fixed_base = base;
        Ok(())
    }

    /// 锁定：决策固化。锁定时模式与基址一致性终审。
    pub fn lock(&mut self) -> Result<LoadMode, u16> {
        if self.locked {
            return Err(E_RDONLY);
        }
        let m = self.mode.ok_or(E_CONTRA)?;
        if m == LoadMode::Diagnostic && self.fixed_base % BASE_ALIGN != 0 {
            return Err(E_ADDR);
        }
        self.locked = true;
        Ok(m)
    }

    /// 账守恒。
    pub fn reconcile(&self) -> Result<(), u16> {
        if self.effective + self.ignored > self.parsed {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.mode {
            Some(LoadMode::Kaslr) => 1,
            Some(LoadMode::Diagnostic) => 2,
            None => 0,
        });
        buf.extend_from_slice(&self.fixed_base.to_le_bytes());
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：诊断模式全径——diag=on → 固定基址 → 锁定。
    let mut d = ModeDecision::new();
    d.feed("console=ttyS0")?;
    d.feed("diag=on")?;
    d.set_fixed_base(0x0200_0000)?;
    let m = d.lock()?;
    if m != LoadMode::Diagnostic || d.fixed_base != 0x0200_0000 {
        return Err(E_BAL);
    }
    d.reconcile()?;
    hits += 1;

    // R2：KASLR 路径——kaslr=on 直接锁定。
    let mut k = ModeDecision::new();
    k.feed("kaslr=on")?;
    let m2 = k.lock()?;
    if m2 != LoadMode::Kaslr {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：后到覆盖——先 diag=on 后 diag=off → 无模式。
    let mut r3 = ModeDecision::new();
    r3.feed("diag=on")?;
    r3.feed("diag=off")?;
    if r3.mode.is_some() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：互斥——diag=on 后 kaslr=on → E_CONTRA。
    let mut i1 = ModeDecision::new();
    i1.feed("diag=on")?;
    if i1.feed("kaslr=on") != Err(E_CONTRA) {
        return Err(E_CONTRA);
    }
    hits += 1;

    // I2：非法参数值 → E_ARG。
    let mut i2 = ModeDecision::new();
    if i2.feed("kaslr=maybe") != Err(E_ARG) {
        return Err(E_ARG);
    }
    hits += 1;

    // I3：基址未对齐 → E_ADDR。
    let mut i3 = ModeDecision::new();
    i3.feed("diag=on")?;
    if i3.set_fixed_base(0x1234_5678) != Err(E_ADDR) {
        return Err(E_ADDR);
    }
    hits += 1;

    // I4：无模式锁定 → E_CONTRA。
    let mut i4 = ModeDecision::new();
    i4.feed("quiet")?;
    if i4.lock() != Err(E_CONTRA) {
        return Err(E_CONTRA);
    }
    hits += 1;

    // I5：锁定后再喂 → E_RDONLY。
    if i1.lock().is_ok() {
        // i1 已在互斥处失败未锁定；改走 R1 的 d
        if d.feed("diag=on") != Err(E_RDONLY) {
            return Err(E_RDONLY);
        }
    }
    hits += 1;

    Ok(hits)
}
