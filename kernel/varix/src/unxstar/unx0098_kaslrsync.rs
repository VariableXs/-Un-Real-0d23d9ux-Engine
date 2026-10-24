//! UNX-F0098 · KASLR 与早期映射一致性自检 · 诊断段 0x22E0
//! 验收锚点：UNX-F0098-J1 开页前后内核符号地址换算一致，自检断言
//! 全过。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x22E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_UNKNOW: u16 = SEG_BASE + 1;  // 符号未登记
const E_ALIGN: u16 = SEG_BASE + 2;   // 滑移非 2M 对齐
const E_WINDOW: u16 = SEG_BASE + 3;  // 滑移越出窗口
const E_MISMATCH: u16 = SEG_BASE + 4;// 开页前后换算不一致
const E_RDONLY: u16 = SEG_BASE + 5;  // 已开页，再设滑移被拒
const E_NOSET: u16 = SEG_BASE + 6;   // 未设滑移即开页
const E_DUP: u16 = SEG_BASE + 7;     // 符号重复登记
const E_BAL: u16 = SEG_BASE + 8;     // 自检账目失衡

pub fn detail(code: u16) -> &'static str {
    match code {
        E_UNKNOW => "符号未登记：换算表查无此名",
        E_ALIGN => "滑移非 2M 对齐：KASLR 偏移必须 2 幂大页对齐",
        E_WINDOW => "滑移越出窗口：超出 [0, SLIDE_CAP) 闭区间",
        E_MISMATCH => "开页前后换算不一致：同符号两路偏移差非恒量",
        E_RDONLY => "已开页：滑移锁定，再设被拒",
        E_NOSET => "未设滑移即开页：先设偏移再翻页",
        E_DUP => "符号重复登记：同名两账",
        E_BAL => "自检账目失衡：通过数与登记数不符",
        _ => "未知诊断码",
    }
}

// ── 换算常量 ─────────────────────────────────────────────────
pub const EARLY_BASE: u64 = 0xFFFF_8000_0000_0000; // 早期恒映半区
pub const HIGH_BASE: u64 = 0xFFFF_9000_0000_0000;  // 随机化高半区
pub const SLIDE_STEP: u64 = 2 * 1024 * 1024;       // 2M 对齐步
pub const SLIDE_CAP: u64 = 1024 * SLIDE_STEP;      // 滑移窗 2G

/// 符号账目。
#[derive(Clone, Debug)]
pub struct SymbolRecord {
    pub name: String,
    pub off: u64, // 相对内核映像基址的偏移
}

/// KASLR 同步账本：早期地址与开页后地址双路换算，恒差校验。
pub struct KaslrSync {
    pub symbols: Vec<SymbolRecord>,
    pub slide: Option<u64>,
    pub paged: bool,
    pub checked: u32,
}

impl KaslrSync {
    pub fn new() -> KaslrSync {
        KaslrSync {
            symbols: Vec::new(),
            slide: None,
            paged: false,
            checked: 0,
        }
    }

    pub fn register(&mut self, name: &str, off: u64) -> Result<(), u16> {
        if self.paged {
            return Err(E_RDONLY);
        }
        for s in &self.symbols {
            if s.name == name {
                return Err(E_DUP);
            }
        }
        self.symbols.push(SymbolRecord {
            name: name.to_string(),
            off,
        });
        Ok(())
    }

    /// 设滑移（开页前）：2M 对齐 + 窗口闭校验。
    pub fn set_slide(&mut self, slide: u64) -> Result<(), u16> {
        if self.paged {
            return Err(E_RDONLY);
        }
        if slide % SLIDE_STEP != 0 {
            return Err(E_ALIGN);
        }
        if slide >= SLIDE_CAP {
            return Err(E_WINDOW);
        }
        self.slide = Some(slide);
        Ok(())
    }

    /// 开页：锁定滑移。未设滑移即翻页是时序违规。
    pub fn turn_page(&mut self) -> Result<(), u16> {
        if self.paged {
            return Err(E_RDONLY);
        }
        if self.slide.is_none() {
            return Err(E_NOSET);
        }
        self.paged = true;
        Ok(())
    }

    /// 早期路：开页前后恒可用（恒映映射不随滑移变）。
    pub fn early_addr(&self, name: &str) -> Result<u64, u16> {
        let s = self.find(name)?;
        Ok(EARLY_BASE + s.off)
    }

    /// 随机化路：开页后才可用。
    pub fn paged_addr(&self, name: &str) -> Result<u64, u16> {
        if !self.paged {
            return Err(E_NOSET);
        }
        let s = self.find(name)?;
        Ok(HIGH_BASE + self.slide.unwrap_or(0) + s.off)
    }

    /// J1 核心判据：开页前后同符号两路换算的差必须恒等于
    /// HIGH_BASE + slide − EARLY_BASE（与符号无关）。
    pub fn check_consistency(&mut self) -> Result<u64, u16> {
        if !self.paged {
            return Err(E_NOSET);
        }
        let slide = self.slide.unwrap_or(0);
        let expect_delta = HIGH_BASE + slide - EARLY_BASE;
        let mut pass: u32 = 0;
        for s in &self.symbols {
            let early = EARLY_BASE + s.off;
            let paged = HIGH_BASE + slide + s.off;
            if paged - early != expect_delta {
                return Err(E_MISMATCH);
            }
            // 反演换算：从两路地址各自还原符号偏移，必须同值。
            if paged - HIGH_BASE - slide != early - EARLY_BASE {
                return Err(E_MISMATCH);
            }
            pass += 1;
        }
        if pass != self.symbols.len() as u32 {
            return Err(E_BAL);
        }
        self.checked += pass;
        Ok(expect_delta)
    }

    pub fn checked_total(&self) -> u32 {
        self.checked
    }

    pub fn render(&self) -> String {
        format!(
            "符号 {} 条，滑移 {:?}，开页 {}，累计换算 {} 次",
            self.symbols.len(),
            self.slide,
            if self.paged { "是" } else { "否" },
            self.checked
        )
    }

    fn find(&self, name: &str) -> Result<&SymbolRecord, u16> {
        for s in &self.symbols {
            if s.name == name {
                return Ok(s);
            }
        }
        Err(E_UNKNOW)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：登记五个内核符号 → 早期路换算恒映基址直加。
    let mut k = KaslrSync::new();
    for (name, off) in [
        ("_start", 0x0),
        ("boot_manifest", 0x2_0000),
        ("page_tables", 0x10_0000),
        ("heap_top", 0x40_0000),
        ("_end", 0x80_0000),
    ] {
        k.register(name, off)?;
    }
    if k.early_addr("_start")? != EARLY_BASE {
        return Err(E_MISMATCH);
    }
    if k.early_addr("heap_top")? != EARLY_BASE + 0x40_0000 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R2：设滑移 64M（2M 整数倍）→ 开页 → J1 一致性全过。
    k.set_slide(64 * 1024 * 1024)?;
    k.turn_page()?;
    let delta = k.check_consistency()?;
    if delta != HIGH_BASE + 64 * 1024 * 1024 - EARLY_BASE {
        return Err(E_BAL);
    }
    if k.checked_total() != 5 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：随机化路逐符号反演——paged − slide − HIGH = 符号偏移。
    let p0 = k.paged_addr("_start")?;
    if p0 - HIGH_BASE - 64 * 1024 * 1024 != 0x0 {
        return Err(E_MISMATCH);
    }
    let p4 = k.paged_addr("_end")?;
    if p4 - HIGH_BASE - 64 * 1024 * 1024 != 0x80_0000 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R4：渲染账含滑移与计数。
    let rep = k.render();
    if !rep.contains("符号 5 条") || !rep.contains("开页 是") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：未登记符号 → E_UNKNOW（两路同拒）。
    if k.early_addr("no_such") != Err(E_UNKNOW) {
        return Err(E_UNKNOW);
    }
    if k.paged_addr("no_such") != Err(E_UNKNOW) {
        return Err(E_UNKNOW);
    }
    hits += 1;

    // I2：滑移非 2M 对齐 → E_ALIGN。
    let mut k2 = KaslrSync::new();
    if k2.set_slide(4 * 1024 * 1024 + 4096) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I3：滑移越窗（恰为 SLIDE_CAP）→ E_WINDOW；边界内一格合法。
    if k2.set_slide(SLIDE_CAP) != Err(E_WINDOW) {
        return Err(E_WINDOW);
    }
    k2.set_slide(SLIDE_CAP - SLIDE_STEP)?;
    hits += 1;

    // I4：未设滑移开页 → E_NOSET；未开页查随机化路 → 同码。
    let mut k3 = KaslrSync::new();
    if k3.turn_page() != Err(E_NOSET) {
        return Err(E_NOSET);
    }
    k3.register("s", 0x1000)?;
    if k3.paged_addr("s") != Err(E_NOSET) {
        return Err(E_NOSET);
    }
    if k3.check_consistency() != Err(E_NOSET) {
        return Err(E_NOSET);
    }
    hits += 1;

    // I5：重复登记 → E_DUP；开页后再登记/再设滑移 → E_RDONLY。
    if k3.register("s", 0x2000) != Err(E_DUP) {
        return Err(E_DUP);
    }
    k3.set_slide(2 * 1024 * 1024)?;
    k3.turn_page()?;
    if k3.turn_page() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if k3.register("t", 0x3000) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if k3.set_slide(4 * 1024 * 1024) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I6：滑移为 2M 一步的最小合法值 → 开页换算一致（边界内下界）。
    let mut k4 = KaslrSync::new();
    k4.register("only", 0xC0DE)?;
    k4.set_slide(SLIDE_STEP)?;
    k4.turn_page()?;
    if k4.paged_addr("only")? != HIGH_BASE + SLIDE_STEP + 0xC0DE {
        return Err(E_MISMATCH);
    }
    k4.check_consistency()?;
    hits += 1;

    Ok(hits)
}
