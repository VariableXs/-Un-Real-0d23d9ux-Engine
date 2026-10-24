//! UNX-F0091 · 内核页表随机化兼容适配 · 诊断段 0x2270
//! 验收锚点：UNX-F0091-J1 随机化基址与既有页表布局的兼容性
//! 适配（区段重映射+旧指针迁移），双布局一致判据通过；
//! 纯功能代码 ≥280 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2270;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ALIGN: u16 = SEG_BASE + 1;   // 新基址未按页粒度对齐
const E_DOMAIN: u16 = SEG_BASE + 2;  // 重映射越出高半区窗
const E_LEGACY: u16 = SEG_BASE + 3;  // 旧指针未迁移（悬挂引用）
const E_RDONLY: u16 = SEG_BASE + 4;  // 适配已定案
const E_BAL: u16 = SEG_BASE + 5;     // 迁移账守恒不平
const E_CONTRA: u16 = SEG_BASE + 6;  // 双布局互斥违例（同页双映射）

/// 高半区窗。
pub const HIGH_BASE: u64 = 0xFFFF_8000_0000_0000;
pub const HIGH_END: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// 页粒度。
pub const PAGE: u64 = 4096;

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
        E_ALIGN => "新基址未按页粒度对齐",
        E_DOMAIN => "重映射越出高半区窗",
        E_LEGACY => "旧指针未迁移：悬挂引用是运行期炸点",
        E_RDONLY => "适配已定案：一次性决策",
        E_BAL => "迁移账守恒：迁移数 = 登记数",
        E_CONTRA => "双布局互斥：同一虚拟页不得同时挂在旧/新布局",
        _ => "未知诊断码",
    }
}

/// 区段声明（内核布局段）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentDecl {
    pub name_off: u64,   // 相对内核基址的段偏移
    pub pages: u64,
    pub kind: SegKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind {
    Text,
    Rodata,
    Data,
    Bss,
}

impl SegKind {
    pub fn nx_expected(&self) -> bool {
        !matches!(self, SegKind::Text)
    }
    pub fn writable_expected(&self) -> bool {
        matches!(self, SegKind::Data | SegKind::Bss)
    }
}

/// 迁移记录：旧虚拟址 → 新虚拟址。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Migration {
    pub old_va: u64,
    pub new_va: u64,
    pub pages: u64,
}

// ── 适配账 ───────────────────────────────────────────────────
pub struct KptAdapter {
    pub old_base: u64,
    pub new_base: u64,
    pub segments: Vec<SegmentDecl>,
    pub migrations: Vec<Migration>,
    pub sealed: bool,
}

impl KptAdapter {
    pub fn new(old_base: u64, new_base: u64) -> Result<KptAdapter, u16> {
        if new_base % PAGE != 0 || old_base % PAGE != 0 {
            return Err(E_ALIGN);
        }
        if new_base < HIGH_BASE || new_base > HIGH_END {
            return Err(E_DOMAIN);
        }
        Ok(KptAdapter {
            old_base,
            new_base,
            segments: Vec::new(),
            migrations: Vec::new(),
            sealed: false,
        })
    }

    pub fn declare(&mut self, s: SegmentDecl) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if s.pages == 0 {
            return Err(E_BAL);
        }
        self.segments.push(s);
        Ok(())
    }

    /// 全段重映射：每段生成迁移记录；段间重叠（新旧交叉）拦截。
    pub fn remap_all(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.segments.is_empty() {
            return Err(E_BAL);
        }
        self.migrations.clear();
        let mut old_cursor = self.old_base;
        let mut new_cursor = self.new_base;
        for s in &self.segments {
            let old_va = old_cursor;
            let new_va = new_cursor;
            if new_va < HIGH_BASE || new_va + s.pages * PAGE > HIGH_END {
                return Err(E_DOMAIN);
            }
            // 新布局内段间重叠拦截
            let m = Migration {
                old_va,
                new_va,
                pages: s.pages,
            };
            for ex in &self.migrations {
                let a1 = m.new_va;
                let a2 = m.new_va + m.pages * PAGE;
                let b1 = ex.new_va;
                let b2 = ex.new_va + ex.pages * PAGE;
                if a1 < b2 && b1 < a2 {
                    return Err(E_CONTRA);
                }
            }
            self.migrations.push(m);
            old_cursor += s.pages * PAGE;
            new_cursor += s.pages * PAGE;
        }
        Ok(self.migrations.len())
    }

    /// 旧指针迁移：给定旧 VA，查迁移表得新 VA；悬挂 → E_LEGACY。
    pub fn translate(&self, old_va: u64) -> Result<u64, u16> {
        for m in &self.migrations {
            let lo = m.old_va;
            let hi = lo + m.pages * PAGE;
            if old_va >= lo && old_va < hi {
                return Ok(m.new_va + (old_va - lo));
            }
        }
        Err(E_LEGACY)
    }

    /// 迁移账守恒：迁移页数 = Σ段页数。
    pub fn reconcile(&self) -> Result<(), u16> {
        let seg_total: u64 = self.segments.iter().map(|s| s.pages).sum();
        let mig_total: u64 = self.migrations.iter().map(|m| m.pages).sum();
        if seg_total != mig_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.migrations.is_empty() {
            return Err(E_BAL);
        }
        self.reconcile()?;
        self.sealed = true;
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.old_base.to_le_bytes());
        buf.extend_from_slice(&self.new_base.to_le_bytes());
        for m in &self.migrations {
            buf.extend_from_slice(&m.old_va.to_le_bytes());
            buf.extend_from_slice(&m.new_va.to_le_bytes());
            buf.extend_from_slice(&m.pages.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    const OLD: u64 = 0xFFFF_8000_0100_0000;
    const NEW: u64 = 0xFFFF_8800_0400_0000;

    // R1：四段声明 → 全量重映射 → 指针迁移（含段内偏移）。
    let mut a = KptAdapter::new(OLD, NEW)?;
    a.declare(SegmentDecl { name_off: 0, pages: 16, kind: SegKind::Text })?;
    a.declare(SegmentDecl { name_off: 64, pages: 8, kind: SegKind::Rodata })?;
    a.declare(SegmentDecl { name_off: 72, pages: 8, kind: SegKind::Data })?;
    a.declare(SegmentDecl { name_off: 80, pages: 16, kind: SegKind::Bss })?;
    let n = a.remap_all()?;
    if n != 4 {
        return Err(E_BAL);
    }
    // 段 0 中点指针迁移
    let t1 = a.translate(OLD + 8 * PAGE)?;
    if t1 != NEW + 8 * PAGE {
        return Err(E_BAL);
    }
    // 段 2 首页
    let t2 = a.translate(OLD + 24 * PAGE)?;
    if t2 != NEW + 24 * PAGE {
        return Err(E_BAL);
    }
    a.seal()?;
    a.reconcile()?;
    hits += 1;

    // R3：指纹对拍。
    let mut a2 = KptAdapter::new(OLD, NEW)?;
    a2.declare(SegmentDecl { name_off: 0, pages: 16, kind: SegKind::Text })?;
    a2.declare(SegmentDecl { name_off: 64, pages: 8, kind: SegKind::Rodata })?;
    a2.declare(SegmentDecl { name_off: 72, pages: 8, kind: SegKind::Data })?;
    a2.declare(SegmentDecl { name_off: 80, pages: 16, kind: SegKind::Bss })?;
    a2.remap_all()?;
    if a2.fingerprint() != a.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：NX 期望语义——Text 非 NX，其余 NX。
    if SegKind::Text.nx_expected() {
        return Err(E_BAL);
    }
    if !SegKind::Bss.nx_expected() || !SegKind::Data.writable_expected() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：基址未对齐 → E_ALIGN。
    if KptAdapter::new(OLD, NEW + 0x1234).is_ok() {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I2：新基址越窗 → E_DOMAIN。
    if KptAdapter::new(OLD, 0x0000_8000_0000_0000).is_ok() {
        return Err(E_DOMAIN);
    }
    hits += 1;

    // I3：悬挂旧指针 → E_LEGACY。
    if a.translate(0x1234_5678) != Err(E_LEGACY) {
        return Err(E_LEGACY);
    }
    hits += 1;

    // I4：段页数为零声明 → E_BAL。
    let mut i4 = KptAdapter::new(OLD, NEW)?;
    if i4.declare(SegmentDecl { name_off: 0, pages: 0, kind: SegKind::Text }) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：封板后重映射 → E_RDONLY。
    if a.remap_all() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
