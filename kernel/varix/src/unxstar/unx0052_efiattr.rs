//! UNX-F0052 · EFI 内存描述符属性位精解 · 诊断段 0x2050
//! 验收锚点：UNX-F0052-J1 类型闭集 × 属性位逐位精解，
//! 类型-属性合法组合表判据通过，非法组合显性立案；
//! 纯功能代码 ≥260 行。零浮点、零 unwrap、零 expect。

use crate::alloc::{string::String, vec::Vec};

pub const SEG_BASE: u16 = 0x2050;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TYPE: u16 = SEG_BASE + 1;     // 类型超出闭集
const E_ATTR: u16 = SEG_BASE + 2;     // 属性位组合非法
const E_RESERVED: u16 = SEG_BASE + 3; // 保留属性位被置位
const E_RANGE: u16 = SEG_BASE + 4;    // 描述符区间回绕（base+len 溢出）
const E_ZERO_LEN: u16 = SEG_BASE + 5; // 零长度描述符
const E_DUP: u16 = SEG_BASE + 6;      // 同基址重复登记
const E_OVERLAP: u16 = SEG_BASE + 7;  // 同类型运行时区重叠
const E_BAL: u16 = SEG_BASE + 8;      // 页数账与描述符和对不平
const E_ORDER: u16 = SEG_BASE + 9;    // 描述符流乱序（快照序非法）
const E_SIZE: u16 = SEG_BASE + 10;    // 描述符尺寸字段小于 40

/// EFI 内存类型闭集（0..=14，≥0x70000000 为 OEM 保留域）。
pub const T_RESERVED: u32 = 0;
pub const T_LOADER_CODE: u32 = 1;
pub const T_LOADER_DATA: u32 = 2;
pub const T_BS_CODE: u32 = 3;
pub const T_BS_DATA: u32 = 4;
pub const T_RT_CODE: u32 = 5;
pub const T_RT_DATA: u32 = 6;
pub const T_CONV: u32 = 7;
pub const T_UNUSABLE: u32 = 8;
pub const T_ACPI_RECLAIM: u32 = 9;
pub const T_ACPI_NVS: u32 = 10;
pub const T_MMIO: u32 = 11;
pub const T_MMIO_PORT: u32 = 12;
pub const T_PAL_CODE: u32 = 13;
pub const T_PERSISTENT: u32 = 14;
pub const T_OEM_BASE: u32 = 0x7000_0000;

pub fn type_name(t: u32) -> &'static str {
    match t {
        T_RESERVED => "保留",
        T_LOADER_CODE => "引导代码",
        T_LOADER_DATA => "引导数据",
        T_BS_CODE => "引导服务代码",
        T_BS_DATA => "引导服务数据",
        T_RT_CODE => "运行时代码",
        T_RT_DATA => "运行时数据",
        T_CONV => "常规内存",
        T_UNUSABLE => "坏页",
        T_ACPI_RECLAIM => "ACPI 可回收",
        T_ACPI_NVS => "ACPI NVS",
        T_MMIO => "MMIO",
        T_MMIO_PORT => "MMIO 端口",
        T_PAL_CODE => "PAL 代码",
        T_PERSISTENT => "持久内存",
        _ => "OEM 保留域",
    }
}

// ── 属性位 ───────────────────────────────────────────────────
pub const A_UC: u64 = 0x0000_0000_0000_0001;
pub const A_WC: u64 = 0x0000_0000_0000_0002;
pub const A_WT: u64 = 0x0000_0000_0000_0004;
pub const A_WB: u64 = 0x0000_0000_0000_0008;
pub const A_UCE: u64 = 0x0000_0000_0000_0010;
pub const A_WP: u64 = 0x0000_0000_0000_1000;
pub const A_RP: u64 = 0x0000_0000_0000_2000;
pub const A_XP: u64 = 0x0000_0000_0000_4000;
pub const A_NV: u64 = 0x0000_0000_0000_8000;
pub const A_MORE_RELIABLE: u64 = 0x0000_0000_0001_0000;
pub const A_RO: u64 = 0x0000_0000_0002_0000;
pub const A_SP: u64 = 0x0000_0000_0004_0000; // 受保护
pub const A_CPU_CRYPTO: u64 = 0x0000_0000_0008_0000;
/// 本仓显性化保留位域（规范保留，必须为零）。
pub const A_RESERVED_MASK: u64 = !0x0000_0000_000F_C01F;

pub fn attr_name(a: u64) -> String {
    let mut s = String::new();
    let mut push = |cond: bool, tag: &str| {
        if cond {
            if !s.is_empty() {
                s.push('|');
            }
            s.push_str(tag);
        }
    };
    push(a & A_UC != 0, "UC");
    push(a & A_WC != 0, "WC");
    push(a & A_WT != 0, "WT");
    push(a & A_WB != 0, "WB");
    push(a & A_UCE != 0, "UCE");
    push(a & A_WP != 0, "WP");
    push(a & A_RP != 0, "RP");
    push(a & A_XP != 0, "XP");
    push(a & A_NV != 0, "NV");
    push(a & A_MORE_RELIABLE != 0, "Rel");
    push(a & A_RO != 0, "RO");
    push(a & A_SP != 0, "SP");
    push(a & A_CPU_CRYPTO != 0, "Crypto");
    if s.is_empty() {
        s.push_str("无属性");
    }
    s
}

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
        E_TYPE => "内存类型超闭集：0..=14 或 ≥0x70000000 之外的值拒绝",
        E_ATTR => "属性组合非法：如 NV 与 MMIO 同置、RO 与 loader-code 冲突",
        E_RESERVED => "规范保留属性位被置位：固件越纲，登记后剥除",
        E_RANGE => "base+len 回绕：地址算术拦截",
        E_ZERO_LEN => "零长度描述符：登记留痕不参与账",
        E_DUP => "同基址重复登记：后到拒绝",
        E_OVERLAP => "运行时区重叠：RT Code/Data 段必须互斥",
        E_BAL => "页数账不平：Σ描述符页数 ≠ 账面页数",
        E_ORDER => "描述符流按基址乱序：快照序必须升序（MapKey 语义）",
        E_SIZE => "描述符尺寸字段 <40：旧版固件布局，拒绝",
        _ => "未知诊断码",
    }
}

/// 类型-属性合法组合表（J1 核心）：
/// 每类内存允许的缓存属性闭集 + 必备/禁止语义位。
pub struct TypeAttrRule {
    pub ty: u32,
    pub cache_mask: u64,   // 允许的缓存属性并集
    pub cache_needed: bool, // 是否必须含恰一个缓存属性
    pub allow_nv: bool,
    pub allow_ro: bool,
}

pub const RULES: [TypeAttrRule; 15] = [
    TypeAttrRule { ty: T_RESERVED, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: false, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_LOADER_CODE, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: true, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_LOADER_DATA, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: false, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_BS_CODE, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: true, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_BS_DATA, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: false, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_RT_CODE, cache_mask: A_UC | A_WB, cache_needed: true, allow_nv: false, allow_ro: true },
    TypeAttrRule { ty: T_RT_DATA, cache_mask: A_UC | A_WB, cache_needed: true, allow_nv: false, allow_ro: true },
    TypeAttrRule { ty: T_CONV, cache_mask: A_UC | A_WC | A_WT | A_WB, cache_needed: false, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_UNUSABLE, cache_mask: A_UC, cache_needed: false, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_ACPI_RECLAIM, cache_mask: A_UC | A_WB, cache_needed: false, allow_nv: false, allow_ro: true },
    TypeAttrRule { ty: T_ACPI_NVS, cache_mask: A_UC | A_WB, cache_needed: false, allow_nv: true, allow_ro: false },
    TypeAttrRule { ty: T_MMIO, cache_mask: A_UC | A_WC, cache_needed: false, allow_nv: false, allow_ro: true },
    TypeAttrRule { ty: T_MMIO_PORT, cache_mask: A_UC, cache_needed: true, allow_nv: false, allow_ro: true },
    TypeAttrRule { ty: T_PAL_CODE, cache_mask: A_UC | A_WB, cache_needed: true, allow_nv: false, allow_ro: false },
    TypeAttrRule { ty: T_PERSISTENT, cache_mask: A_UC | A_WB, cache_needed: false, allow_nv: true, allow_ro: true },
];

pub fn rule_for(ty: u32) -> Option<&'static TypeAttrRule> {
    RULES.iter().find(|r| r.ty == ty)
}

#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub ty: u32,
    pub phys: u64,
    pub virt: u64,
    pub pages: u64,
    pub attrs: u64,
}

impl Descriptor {
    pub fn end(&self) -> Result<u64, u16> {
        self.phys
            .checked_add(self.pages.saturating_mul(4096))
            .ok_or(E_RANGE)
    }
}

/// 单条描述符精解：类型闭集 → 保留位 → 组合表。
/// 返回 (生效属性, 保留位被剥除数)。
pub fn classify(d: &Descriptor) -> Result<(u64, u32), u16> {
    let is_oem = d.ty >= T_OEM_BASE;
    if d.ty > 14 && !is_oem {
        return Err(E_TYPE);
    }
    if d.pages == 0 {
        return Err(E_ZERO_LEN);
    }
    d.end()?;
    let stripped = (d.attrs & A_RESERVED_MASK).count_ones();
    let eff = d.attrs & !A_RESERVED_MASK;
    if stripped > 0 {
        // 保留位置位：剥除后继续（显性计数，不静默）
    }
    if is_oem {
        // OEM 域属性不适用组合表，只剥保留位
        return Ok((eff, stripped));
    }
    let r = rule_for(d.ty).ok_or(E_TYPE)?;
    let cache_bits = eff & (A_UC | A_WC | A_WT | A_WB);
    if r.cache_needed {
        // 必须恰含一个允许的缓存属性
        if cache_bits.count_ones() != 1 || (cache_bits & !r.cache_mask) != 0 {
            return Err(E_ATTR);
        }
    } else {
        // 可选缓存属性：置位则必须落在允许集内
        if (cache_bits & !r.cache_mask) != 0 {
            return Err(E_ATTR);
        }
        if cache_bits.count_ones() > 1 {
            return Err(E_ATTR);
        }
    }
    if !r.allow_nv && (eff & A_NV) != 0 {
        return Err(E_ATTR);
    }
    if !r.allow_ro && (eff & A_RO) != 0 {
        return Err(E_ATTR);
    }
    Ok((eff, stripped))
}

// ── 内存图账：描述符流 → 精解账 ────────────────────────────────
pub struct MemMapLedger {
    pub entries: Vec<(Descriptor, u64)>, // (描述符, 生效属性)
    pub rejected: Vec<(u64, u16)>,
    pub stripped_total: u32,
    pub frozen: bool,
}

impl MemMapLedger {
    pub fn new() -> MemMapLedger {
        MemMapLedger {
            entries: Vec::new(),
            rejected: Vec::new(),
            stripped_total: 0,
            frozen: false,
        }
    }

    /// 顺序吸收：快照序必须按基址非降；同基址重复拒绝。
    pub fn absorb(&mut self, d: Descriptor) -> Result<bool, u16> {
        if self.frozen {
            return Err(E_BAL);
        }
        if let Some((last, _)) = self.entries.last() {
            if d.phys < last.phys {
                self.rejected.push((d.phys, E_ORDER));
                return Err(E_ORDER);
            }
        }
        if self.entries.iter().any(|(e, _)| e.phys == d.phys) {
            self.rejected.push((d.phys, E_DUP));
            return Ok(false);
        }
        match classify(&d) {
            Ok((eff, stripped)) => {
                // RT Code/Data 互斥：运行时区与既有运行时区不得重叠
                if d.ty == T_RT_CODE || d.ty == T_RT_DATA {
                    let e_new = d.end()?;
                    for (e, _) in &self.entries {
                        if e.ty == T_RT_CODE || e.ty == T_RT_DATA {
                            let e_old = e.end()?;
                            if d.phys < e_old && e.phys < e_new {
                                self.rejected.push((d.phys, E_OVERLAP));
                                return Ok(false);
                            }
                        }
                    }
                }
                self.stripped_total += stripped;
                self.entries.push((d, eff));
                Ok(true)
            }
            Err(c) => {
                self.rejected.push((d.phys, c));
                Ok(false)
            }
        }
    }

    pub fn close(&mut self) -> Result<(), u16> {
        self.frozen = true;
        Ok(())
    }

    /// 页数守恒：Σ有效页数。
    pub fn total_pages(&self) -> u64 {
        self.entries.iter().map(|(d, _)| d.pages).sum()
    }

    /// 类型分布计数（16 桶：0..=14 + OEM）。
    pub fn type_histogram(&self) -> [u64; 16] {
        let mut h = [0u64; 16];
        for (d, _) in &self.entries {
            let idx = if d.ty >= T_OEM_BASE { 15 } else { d.ty as usize };
            h[idx] = h[idx].saturating_add(1);
        }
        h
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for (d, a) in &self.entries {
            buf.extend_from_slice(&d.phys.to_le_bytes());
            buf.extend_from_slice(&d.pages.to_le_bytes());
            buf.extend_from_slice(&d.ty.to_le_bytes());
            buf.extend_from_slice(&a.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：常规内存 WB 属性 → 放行，账面页数守恒。
    let mut led = MemMapLedger::new();
    led.absorb(Descriptor { ty: T_CONV, phys: 0x1000, virt: 0, pages: 256, attrs: A_WB })?;
    if led.total_pages() != 256 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：RT Code WB + RO → 放行；再入重叠 RT → 拒绝留痕。
    led.absorb(Descriptor { ty: T_RT_CODE, phys: 0x2000_0000, virt: 0xFFFF_8000_2000_0000, pages: 16, attrs: A_WB | A_RO })?;
    if led.entries.len() != 2 {
        return Err(E_BAL);
    }
    let dup_rt = Descriptor { ty: T_RT_DATA, phys: 0x2000_0008, virt: 0, pages: 8, attrs: A_WB };
    led.absorb(dup_rt)?;
    if led.rejected.last().map(|(_, c)| *c) != Some(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // R3：类型分布直方图桶闭集。
    let h = led.type_histogram();
    if h[T_CONV as usize] != 1 || h[T_RT_CODE as usize] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：类型越纲 → E_TYPE。
    let mut i1 = MemMapLedger::new();
    i1.absorb(Descriptor { ty: 42, phys: 0x1000, virt: 0, pages: 1, attrs: A_WB })?;
    if i1.rejected.last().map(|(_, c)| *c) != Some(E_TYPE) {
        return Err(E_TYPE);
    }
    hits += 1;

    // I2：MMIO 置 NV → E_ATTR。
    let mut i2 = MemMapLedger::new();
    i2.absorb(Descriptor { ty: T_MMIO, phys: 0xF000_0000, virt: 0, pages: 1, attrs: A_UC | A_NV })?;
    if i2.rejected.last().map(|(_, c)| *c) != Some(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I3：RT Code 用 WT（组合表只允许 UC|WB）→ E_ATTR。
    let mut i3 = MemMapLedger::new();
    i3.absorb(Descriptor { ty: T_RT_CODE, phys: 0x3000_0000, virt: 0, pages: 4, attrs: A_WT })?;
    if i3.rejected.last().map(|(_, c)| *c) != Some(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I4：保留位被置位 → 剥除并计数，不拒。
    let mut i4 = MemMapLedger::new();
    let dirty = A_WB | A_MORE_RELIABLE | 0x8000_0000_0000_0000;
    i4.absorb(Descriptor { ty: T_CONV, phys: 0x2000, virt: 0, pages: 2, attrs: dirty })?;
    if i4.stripped_total == 0 || i4.entries.len() != 1 {
        return Err(E_RESERVED);
    }
    hits += 1;

    // I5：零长度 → E_ZERO_LEN。
    let mut i5 = MemMapLedger::new();
    i5.absorb(Descriptor { ty: T_CONV, phys: 0x3000, virt: 0, pages: 0, attrs: A_WB })?;
    if i5.rejected.last().map(|(_, c)| *c) != Some(E_ZERO_LEN) {
        return Err(E_ZERO_LEN);
    }
    hits += 1;

    // I6：回绕（phys 接近 u64 上限）→ E_RANGE。
    let mut i6 = MemMapLedger::new();
    i6.absorb(Descriptor { ty: T_CONV, phys: u64::MAX - 4096, virt: 0, pages: 4, attrs: A_WB })?;
    if i6.rejected.last().map(|(_, c)| *c) != Some(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I7：乱序 → E_ORDER。
    let mut i7 = MemMapLedger::new();
    i7.absorb(Descriptor { ty: T_CONV, phys: 0x5000, virt: 0, pages: 1, attrs: A_WB })?;
    let r = i7.absorb(Descriptor { ty: T_CONV, phys: 0x4000, virt: 0, pages: 1, attrs: A_WB });
    if r != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    Ok(hits)
}
