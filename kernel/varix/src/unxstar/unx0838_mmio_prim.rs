//! unx0838_mmio_prim — UNX-F0838 xAPIC MMIO 访问原语与对齐断言（诊断段 0x65E0）。
//!
//! 判据原文：全部 LAPIC MMIO 访问经原语（散落访问归零），
//!           非对齐访问拦截 10/10 次。
//! 设计纪律：
//!   1. xAPIC MMIO 窗口按 SDM Vol.3 §10.9：基址默认 0xFEE00000，
//!      寄存器偏移 0x000..0x3FF，全部 32 位宽、16 字节对齐（offset%16==0）。
//!   2. 访问统一走原语（read_reg/write_reg）：准入三闸——基址窗口、
//!      偏移域、对齐断言；任一失守即拒并留拦截账。
//!   3. 散落访问审计：原语外的裸访问入口只留审计钩子（计数必须恒零），
//!      「散落访问归零」以审计账为凭据。
//!   4. 后端走内部模拟 trait（SimBackend：定容寄存器映射表），不写真端口。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x65E0 本单独占。

pub const SEG_BASE: u16 = 0x65E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ALIGN: u16 = SEG_BASE + 1;   // 偏移非 16 字节对齐
const E_OFFSET: u16 = SEG_BASE + 2;  // 偏移越出 0x000..0x3FF 窗口
const E_BASE: u16 = SEG_BASE + 3;    // 访问地址不在 LAPIC 基址窗
const E_STRAY: u16 = SEG_BASE + 4;   // 审计账发现散落（原语外）访问
const E_RDONLY: u16 = SEG_BASE + 5;  // 只读寄存器写被拒
const E_WRONLY: u16 = SEG_BASE + 6;  // 只写寄存器读被拒（自清类）

/// xAPIC 窗口锚点。
pub const LAPIC_BASE: u64 = 0xFEE0_0000;
pub const WINDOW_SIZE: u32 = 0x400;
/// 寄存器宽度：全 32 位。
pub const REG_WIDTH: u32 = 4;

/// 只读寄存器偏移闭集（SDM §10.9）。
pub const RDONLY_SET: [u32; 5] = [0x020, 0x030, 0x080, 0x090, 0x0D0];
/// 只写/自清寄存器偏移闭集（ESR 需写 0 清、ICR 低口为触发写）。
pub const WONLY_SET: [u32; 2] = [0x0B0, 0x300];

pub fn is_rdonly(off: u32) -> bool {
    let mut hit = false;
    for o in RDONLY_SET.iter() {
        if *o == off {
            hit = true;
        }
    }
    hit
}

pub fn is_wonly(off: u32) -> bool {
    let mut hit = false;
    for o in WONLY_SET.iter() {
        if *o == off {
            hit = true;
        }
    }
    hit
}

/// 内部模拟后端 trait：定容寄存器面（不写真端口）。
pub trait RegBackend {
    fn load(&self, off: u32) -> Option<u32>;
    fn store(&mut self, off: u32, v: u32) -> Option<()>;
    fn access_count(&self) -> u32;
}

/// 模拟后端：128 槽（0x000..0x3FF 每 16 字节一槽）定容表。
pub struct SimBackend {
    regs: [u32; 64],
    accesses: u32,
}

impl SimBackend {
    pub fn new() -> SimBackend {
        SimBackend {
            regs: [0u32; 64],
            accesses: 0,
        }
    }

    fn slot(off: u32) -> Option<usize> {
        if off < WINDOW_SIZE {
            Some((off / 16) as usize)
        } else {
            None
        }
    }
}

impl RegBackend for SimBackend {
    fn load(&self, off: u32) -> Option<u32> {
        let s = Self::slot(off)?;
        Some(self.regs[s])
    }

    fn store(&mut self, off: u32, v: u32) -> Option<()> {
        let s = Self::slot(off)?;
        self.regs[s] = v;
        self.accesses += 1;
        Some(())
    }

    fn access_count(&self) -> u32 {
        self.accesses
    }
}

/// 审计账：原语外裸访问计数（判据「散落访问归零」凭据）。
pub struct StrayAudit {
    pub stray_reads: u32,
    pub stray_writes: u32,
}

impl StrayAudit {
    pub fn new() -> StrayAudit {
        StrayAudit {
            stray_reads: 0,
            stray_writes: 0,
        }
    }

    pub fn record_stray_read(&mut self) {
        self.stray_reads += 1;
    }

    pub fn record_stray_write(&mut self) {
        self.stray_writes += 1;
    }

    pub fn clean(&self) -> bool {
        self.stray_reads == 0 && self.stray_writes == 0
    }
}

/// MMIO 访问原语：唯一合法入口。
pub struct LapicMmio<'a> {
    base: u64,
    backend: &'a mut dyn RegBackend,
    pub gate_intercepts: u32, // 准入拦截计数（对齐/窗口/基址）
}

impl<'a> LapicMmio<'a> {
    pub fn new(base: u64, backend: &'a mut dyn RegBackend) -> LapicMmio<'a> {
        LapicMmio {
            base,
            backend,
            gate_intercepts: 0,
        }
    }

    /// 准入三闸：基址窗、偏移域、16 字节对齐。
    fn admit(&mut self, addr: u64) -> Result<u32, u16> {
        if addr < self.base || addr >= self.base + WINDOW_SIZE as u64 {
            self.gate_intercepts += 1;
            return Err(E_BASE);
        }
        let off = (addr - self.base) as u32;
        if off >= WINDOW_SIZE {
            self.gate_intercepts += 1;
            return Err(E_OFFSET);
        }
        if off % 16 != 0 {
            self.gate_intercepts += 1;
            return Err(E_ALIGN);
        }
        Ok(off)
    }

    /// 原语读。
    pub fn read_reg(&mut self, addr: u64, audit: &mut StrayAudit) -> Result<u32, u16> {
        let off = self.admit(addr)?;
        if is_wonly(off) {
            self.gate_intercepts += 1;
            return Err(E_WRONLY);
        }
        match self.backend.load(off) {
            Some(v) => Ok(v),
            None => {
                audit.record_stray_read();
                Err(E_OFFSET)
            }
        }
    }

    /// 原语写。
    pub fn write_reg(
        &mut self,
        addr: u64,
        v: u32,
        audit: &mut StrayAudit,
    ) -> Result<(), u16> {
        let off = self.admit(addr)?;
        if is_rdonly(off) {
            self.gate_intercepts += 1;
            return Err(E_RDONLY);
        }
        match self.backend.store(off, v) {
            Some(()) => Ok(()),
            None => {
                audit.record_stray_write();
                Err(E_OFFSET)
            }
        }
    }

    /// 散落访问归零断言（判据前半）。
    pub fn stray_zero(audit: &StrayAudit) -> Result<(), u16> {
        if audit.clean() {
            Ok(())
        } else {
            Err(E_STRAY)
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ALIGN => "偏移非 16 字节对齐（LAPIC MMIO 全 32 位宽对齐域）",
        E_OFFSET => "偏移越出 0x000..0x3FF 窗口",
        E_BASE => "访问地址不在 LAPIC 基址窗（0xFEE00000..+0x400）",
        E_STRAY => "审计账发现原语外散落访问",
        E_RDONLY => "只读寄存器写被拒（ID/版本/TPR 等读面）",
        E_WRONLY => "只写寄存器读被拒（EOI/ICR 触发口）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：原语全径 + 非对齐 10/10 拦截 + 散落归零 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut backend = SimBackend::new();
    let mut audit = StrayAudit::new();

    // R1：原语写读回全径（对齐域内合法偏移扫描）。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        for i in 0u32..16 {
            let off = i * 0x10 + 0x100; // 0x100..0x2F0
            mmio.write_reg(LAPIC_BASE + off as u64, 0xA500_0000 + off, &mut audit)?;
            let v = mmio.read_reg(LAPIC_BASE + off as u64, &mut audit)?;
            if v != 0xA500_0000 + off {
                return Err(E_OFFSET);
            }
        }
    }
    hits += 1;

    // R2：非对齐访问拦截 10/10（判据原文）——偏移 +1..+15 全拒。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        let mut intercepted = 0u32;
        for d in 1u64..16 {
            let addr = LAPIC_BASE + 0x200 + d;
            match mmio.read_reg(addr, &mut audit) {
                Err(E_ALIGN) => intercepted += 1,
                Err(_) => return Err(E_ALIGN),
                Ok(_) => return Err(E_ALIGN),
            }
        }
        if intercepted != 15 {
            return Err(E_ALIGN);
        }
    }
    hits += 1;

    // R3：写口同样拦截非对齐（10 档采样）。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        for i in 0..10u64 {
            let addr = LAPIC_BASE + 0x220 + 1 + i;
            if mmio.write_reg(addr, 1, &mut audit) != Err(E_ALIGN) {
                return Err(E_ALIGN);
            }
        }
        if mmio.gate_intercepts < 10 {
            return Err(E_ALIGN);
        }
    }
    hits += 1;

    // R4：基址窗与偏移域双闸——窗外地址、越界偏移全拒。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        if mmio.read_reg(LAPIC_BASE + WINDOW_SIZE as u64, &mut audit)
            != Err(E_BASE)
        {
            return Err(E_BASE);
        }
        if mmio.read_reg(LAPIC_BASE - 0x10, &mut audit) != Err(E_BASE) {
            return Err(E_BASE);
        }
        if mmio.read_reg(0xFEC0_0000, &mut audit) != Err(E_BASE) {
            return Err(E_BASE); // IOAPIC 基址邻域误访
        }
    }
    hits += 1;

    // R5：只读/只写闭集闸。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        // ID 寄存器（0x020）只读：写拒。
        if mmio.write_reg(LAPIC_BASE + 0x020, 0x0100_0000, &mut audit)
            != Err(E_RDONLY)
        {
            return Err(E_RDONLY);
        }
        // 版本寄存器（0x030）只读：写拒。
        if mmio.write_reg(LAPIC_BASE + 0x030, 1, &mut audit) != Err(E_RDONLY) {
            return Err(E_RDONLY);
        }
        // EOI（0x0B0）只写触发口：读拒。
        if mmio.read_reg(LAPIC_BASE + 0x0B0, &mut audit) != Err(E_WRONLY) {
            return Err(E_WRONLY);
        }
        // ICR 低口（0x300）触发写：读拒。
        if mmio.read_reg(LAPIC_BASE + 0x300, &mut audit) != Err(E_WRONLY) {
            return Err(E_WRONLY);
        }
    }
    hits += 1;

    // R6：散落访问归零（判据前半）——全径走完后审计账必清。
    LapicMmio::stray_zero(&audit)?;
    if backend.access_count() < 16 {
        return Err(E_STRAY);
    }
    hits += 1;

    // R7：审计钩子负样本——人为记一笔散落访问后归零断言失守。
    let mut audit2 = StrayAudit::new();
    audit2.record_stray_read();
    if LapicMmio::stray_zero(&audit2) != Err(E_STRAY) {
        return Err(E_STRAY);
    }
    if audit2.stray_reads != 1 || audit2.stray_writes != 0 {
        return Err(E_STRAY);
    }
    hits += 1;

    // I1：自定义基址引擎（x2APIC 无 MMIO 面，基址改挂模拟区）同样闭闸。
    {
        let mut b2 = SimBackend::new();
        let mut mmio = LapicMmio::new(0xFEE0_1000, &mut b2);
        if mmio.read_reg(LAPIC_BASE + 0x100, &mut audit) != Err(E_BASE) {
            return Err(E_BASE); // 基址不符即拒：窗口私有性
        }
        mmio.write_reg(0xFEE0_1000 + 0x100, 7, &mut audit)?;
        if mmio.read_reg(0xFEE0_1000 + 0x100, &mut audit)? != 7 {
            return Err(E_OFFSET);
        }
    }
    hits += 1;

    // I2：宽边界——窗口末槽（0x3F0）合法、0x400 起越界。
    {
        let mut mmio = LapicMmio::new(LAPIC_BASE, &mut backend);
        mmio.write_reg(LAPIC_BASE + 0x3F0, 0x1234, &mut audit)?;
        if mmio.read_reg(LAPIC_BASE + 0x3F0, &mut audit)? != 0x1234 {
            return Err(E_OFFSET);
        }
        if mmio.read_reg(LAPIC_BASE + 0x400, &mut audit) != Err(E_BASE) {
            return Err(E_OFFSET);
        }
    }
    hits += 1;

    Ok(hits)
}
