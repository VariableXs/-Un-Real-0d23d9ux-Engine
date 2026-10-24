//! UNX-F0073 · 内存空洞检测器 · 诊断段 0x2180
//! 验收锚点：UNX-F0073-J1 可用区之间空洞（未映射/未登记带）逐段
//! 检测入账，空洞分类（保留型/坏页型/未声明型）判据通过；
//! 纯功能代码 ≥220 行。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2180;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // 区间倒序/回绕
const E_OVERLAP: u16 = SEG_BASE + 2;  // 可用区重叠（探测前置违例）
const E_EMPTY: u16 = SEG_BASE + 3;    // 无区可探
const E_RDONLY: u16 = SEG_BASE + 4;   // 探测账已封板
const E_UNDECL: u16 = SEG_BASE + 5;   // 未声明型空洞未立案
const E_BAL: u16 = SEG_BASE + 6;      // 空洞账守恒不平
const E_DUP: u16 = SEG_BASE + 7;      // 重复区登记

/// 空洞分类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleKind {
    /// 保留型：落在保留账内（MMIO、固件区），合法存在。
    Reserved,
    /// 坏页型：落在坏页账内，合法但需页表隔离。
    BadRam,
    /// 未声明型：三账皆不覆盖——内存图缺口，必须立案。
    Undeclared,
}

impl HoleKind {
    pub fn name(&self) -> &'static str {
        match self {
            HoleKind::Reserved => "保留型",
            HoleKind::BadRam => "坏页型",
            HoleKind::Undeclared => "未声明型",
        }
    }
    pub fn legal(&self) -> bool {
        !matches!(self, HoleKind::Undeclared)
    }
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
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_OVERLAP => "可用区重叠：探测前置违例，先修内存图",
        E_EMPTY => "无区可探：先登记可用区",
        E_RDONLY => "探测账已封板：再注入属误用",
        E_UNDECL => "未声明型空洞未立案：三账皆不覆盖的缺口必须补账",
        E_BAL => "空洞账守恒：空洞总字节 = 扫描域 − 可用域",
        E_DUP => "重复区登记：后到拒绝",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub base: u64,
    pub len: u64,
}

impl Span {
    pub fn end(&self) -> u64 {
        self.base + self.len
    }
    pub fn contains(&self, a: u64) -> bool {
        a >= self.base && a < self.end()
    }
    pub fn overlaps(&self, o: &Span) -> bool {
        self.base < o.end() && o.base < self.end()
    }
}

/// 空洞记录。
#[derive(Clone, Copy, Debug)]
pub struct Hole {
    pub base: u64,
    pub len: u64,
    pub kind: HoleKind,
}

// ── 探测账 ───────────────────────────────────────────────────
pub struct HoleDetector {
    pub usable: Vec<Span>,
    pub reserved: Vec<Span>,
    pub bad: Vec<Span>,
    pub holes: Vec<Hole>,
    pub scan_domain: Span,
    pub sealed: bool,
}

impl HoleDetector {
    pub fn new(domain_base: u64, domain_len: u64) -> Result<HoleDetector, u16> {
        if domain_len == 0 {
            return Err(E_EMPTY);
        }
        if domain_base.checked_add(domain_len).is_none() {
            return Err(E_RANGE);
        }
        Ok(HoleDetector {
            usable: Vec::new(),
            reserved: Vec::new(),
            bad: Vec::new(),
            holes: Vec::new(),
            scan_domain: Span { base: domain_base, len: domain_len },
            sealed: false,
        })
    }

    fn push_unique(v: &mut Vec<Span>, s: Span) -> Result<(), u16> {
        if v.iter().any(|x| x.base == s.base) {
            return Err(E_DUP);
        }
        if v.iter().any(|x| x.overlaps(&s)) {
            return Err(E_OVERLAP);
        }
        v.push(s);
        Ok(())
    }

    pub fn add_usable(&mut self, base: u64, len: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        if base < self.scan_domain.base || base + len > self.scan_domain.end() {
            return Err(E_RANGE);
        }
        Self::push_unique(&mut self.usable, Span { base, len })
    }

    pub fn add_reserved(&mut self, base: u64, len: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        Self::push_unique(&mut self.reserved, Span { base, len })
    }

    pub fn add_bad(&mut self, base: u64, len: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        Self::push_unique(&mut self.bad, Span { base, len })
    }

    fn classify(&self, addr: u64) -> HoleKind {
        if self.reserved.iter().any(|s| s.contains(addr)) {
            return HoleKind::Reserved;
        }
        if self.bad.iter().any(|s| s.contains(addr)) {
            return HoleKind::BadRam;
        }
        HoleKind::Undeclared
    }

    /// 扫描：扫描域 − 可用区 = 空洞带；带内分类按首地址（带内同质，
    /// 异质带切分登记为多段）。
    pub fn scan(&mut self) -> Result<(usize, u64), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.usable.is_empty() {
            return Err(E_EMPTY);
        }
        // 前置：可用区互斥（push_unique 保证）且不越域（add_usable 保证）
        let mut zs: Vec<Span> = self.usable.clone();
        zs.sort_by_key(|z| z.base);
        self.holes.clear();
        let mut cur = self.scan_domain.base;
        let mut undeclared = 0u64;
        for z in &zs {
            if z.base > cur {
                let (b, l) = (cur, z.base - cur);
                // 按分类切段
                let mut p = b;
                while p < b + l {
                    let k = self.classify(p);
                    // 同类延伸到分类边界或带尾
                    let mut q = p;
                    while q < b + l && self.classify(q) == k {
                        q += 4096;
                    }
                    let seg_len = q - p;
                    self.holes.push(Hole { base: p, len: seg_len, kind: k });
                    if !k.legal() {
                        undeclared += seg_len;
                    }
                    p = q;
                }
            }
            if z.end() > cur {
                cur = z.end();
            }
        }
        if cur < self.scan_domain.end() {
            let (b, l) = (cur, self.scan_domain.end() - cur);
            let mut p = b;
            while p < b + l {
                let k = self.classify(p);
                let mut q = p;
                while q < b + l && self.classify(q) == k {
                    q += 4096;
                }
                let seg_len = q - p;
                self.holes.push(Hole { base: p, len: seg_len, kind: k });
                if !k.legal() {
                    undeclared += seg_len;
                }
                p = q;
            }
        }
        // 守恒：空洞总字节 = 扫描域 − 可用域
        let usable_total: u64 = self.usable.iter().map(|s| s.len).sum();
        let hole_total: u64 = self.holes.iter().map(|h| h.len).sum();
        if hole_total + usable_total != self.scan_domain.len {
            return Err(E_BAL);
        }
        self.sealed = true;
        Ok((self.holes.len(), undeclared))
    }

    /// 未声明空洞立案清单（门禁用）。
    pub fn undeclared_items(&self) -> Vec<String> {
        self.holes
            .iter()
            .filter(|h| h.kind == HoleKind::Undeclared)
            .map(|h| format!("0x{:X}..0x{:X}（{} 字节）", h.base, h.base + h.len, h.len))
            .collect()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for h in &self.holes {
            buf.extend_from_slice(&h.base.to_le_bytes());
            buf.extend_from_slice(&h.len.to_le_bytes());
            buf.push(match h.kind {
                HoleKind::Reserved => 1,
                HoleKind::BadRam => 2,
                HoleKind::Undeclared => 3,
            });
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三分类空洞（保留/坏页/未声明）段切正确。
    let mut d = HoleDetector::new(0x1000_0000, 0x40_0000)?; // 4MiB 域
    d.add_usable(0x1000_0000, 0x10_0000)?;                  // [0,1MiB)
    d.add_reserved(0x1010_0000, 0x08_0000)?;                // 空洞1 保留型 [1,1.5MiB)
    d.add_usable(0x1018_0000, 0x10_0000)?;                  // [1.5,2.5MiB)
    d.add_bad(0x1028_0000, 0x08_0000)?;                     // 空洞2 坏页型 [2.5,3MiB)
    // 剩余 [3,4MiB) 无账 → 未声明型
    let (n, undecl) = d.scan()?;
    if n != 3 || undecl != 0x10_0000 {
        return Err(E_BAL);
    }
    if d.holes[1].kind != HoleKind::BadRam || d.holes[0].kind != HoleKind::Reserved {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：未声明立案清单——段起点应为坏页账尾 0x1030_0000。
    let items = d.undeclared_items();
    if items.len() != 1 || !items[0].contains("0x1030") {
        return Err(E_UNDECL);
    }
    hits += 1;

    // R3：全零空洞域（可用区铺满）→ 0 洞守恒。
    let mut d2 = HoleDetector::new(0x2000_0000, 0x10_0000)?;
    d2.add_usable(0x2000_0000, 0x10_0000)?;
    let (n2, u2) = d2.scan()?;
    if n2 != 0 || u2 != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：可用区重叠 → E_OVERLAP。
    let mut i1 = HoleDetector::new(0x3000_0000, 0x20_0000)?;
    i1.add_usable(0x3000_0000, 0x10_0000)?;
    if i1.add_usable(0x3000_8000, 0x1000) != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I2：越出扫描域 → E_RANGE。
    let mut i2 = HoleDetector::new(0x3000_0000, 0x10_0000)?;
    if i2.add_usable(0x3010_0000, 0x1000) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I3：无可用区就扫描 → E_EMPTY。
    let mut i3 = HoleDetector::new(0x4000_0000, 0x10_0000)?;
    if i3.scan() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：重复登记 → E_DUP。
    let mut i4 = HoleDetector::new(0x4000_0000, 0x20_0000)?;
    i4.add_reserved(0x4010_0000, 0x1000)?;
    if i4.add_reserved(0x4010_0000, 0x1000) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I5：封板后再扫 → E_RDONLY。
    let mut i5 = HoleDetector::new(0x5000_0000, 0x20_0000)?;
    i5.add_usable(0x5000_0000, 0x10_0000)?;
    i5.scan()?;
    if i5.scan() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
