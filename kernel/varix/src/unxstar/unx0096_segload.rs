//! UNX-F0096 · 段加载器（载入与权限落位）· 诊断段 0x22C0
//! 验收锚点：UNX-F0096-J1 四段（Text/Rodata/Data/Bss）载入与
//! 权限落位（R-X/R--/RW-/RW-）逐段核账判据通过；
//! 纯功能代码 ≥320 行。零浮点、零 unwrap、零 expect。


use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x22C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ALIGN: u16 = SEG_BASE + 1;   // 载入地址未按页对齐
const E_RANGE: u16 = SEG_BASE + 2;   // 段越出载入窗
const E_OVERLAP: u16 = SEG_BASE + 3; // 段间重叠
const E_PERM: u16 = SEG_BASE + 4;    // 权限与段类不符
const E_ORDER: u16 = SEG_BASE + 5;   // 载入序违例（BSS 先于 Data）
const E_RDONLY: u16 = SEG_BASE + 6;  // 载入窗已封
const E_BAL: u16 = SEG_BASE + 7;     // 载入账守恒不平
const E_EMPTY: u16 = SEG_BASE + 8;   // 空段声明

/// 页粒度。
pub const PAGE: u64 = 4096;

/// 权限位：R=4 W=2 X=1。
pub const P_R: u8 = 4;
pub const P_W: u8 = 2;
pub const P_X: u8 = 1;

/// 段类闭集与期望权限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind {
    Text,
    Rodata,
    Data,
    Bss,
}

impl SegKind {
    pub fn expected_perm(&self) -> u8 {
        match self {
            SegKind::Text => P_R | P_X,
            SegKind::Rodata => P_R,
            SegKind::Data | SegKind::Bss => P_R | P_W,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            SegKind::Text => "代码",
            SegKind::Rodata => "只读数据",
            SegKind::Data => "数据",
            SegKind::Bss => "BSS",
        }
    }
    /// BSS 无文件载荷（清零段）。
    pub fn has_file_image(&self) -> bool {
        !matches!(self, SegKind::Bss)
    }
}

pub fn perm_name(p: u8) -> String {
    let mut s = String::new();
    if p & P_R != 0 {
        s.push('R');
    } else {
        s.push('-');
    }
    if p & P_W != 0 {
        s.push('W');
    } else {
        s.push('-');
    }
    if p & P_X != 0 {
        s.push('X');
    } else {
        s.push('-');
    }
    s
}

/// 载入序：Text(0) < Rodata(1) < Data(2) < Bss(3)。
pub fn seg_order(k: SegKind) -> u8 {
    match k {
        SegKind::Text => 0,
        SegKind::Rodata => 1,
        SegKind::Data => 2,
        SegKind::Bss => 3,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LoadSeg {
    pub kind: SegKind,
    pub va: u64,
    pub pages: u64,
    pub perm: u8,
}

impl LoadSeg {
    pub fn end(&self) -> u64 {
        self.va + self.pages * PAGE
    }
}

// ── 载入账 ───────────────────────────────────────────────────
pub struct SegLoader {
    pub window_base: u64,
    pub window_pages: u64,
    pub segs: Vec<LoadSeg>,
    pub last_order: u8,
    pub loaded_bytes: u64,
    pub sealed: bool,
}

impl SegLoader {
    pub fn new(window_base: u64, window_pages: u64) -> Result<SegLoader, u16> {
        if window_base % PAGE != 0 || window_pages == 0 {
            return Err(E_ALIGN);
        }
        Ok(SegLoader {
            window_base,
            window_pages,
            segs: Vec::new(),
            last_order: 0,
            loaded_bytes: 0,
            sealed: false,
        })
    }

    pub fn window_end(&self) -> u64 {
        self.window_base + self.window_pages * PAGE
    }

    /// 载入一段：对齐 + 窗界 + 重叠 + 权限 + 载入序五重核。
    pub fn load(&mut self, s: LoadSeg) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if s.pages == 0 {
            return Err(E_EMPTY);
        }
        if s.va % PAGE != 0 {
            return Err(E_ALIGN);
        }
        if s.va < self.window_base || s.end() > self.window_end() {
            return Err(E_RANGE);
        }
        if s.perm != s.kind.expected_perm() {
            return Err(E_PERM);
        }
        let o = seg_order(s.kind);
        if o < self.last_order {
            return Err(E_ORDER);
        }
        self.last_order = o;
        // 重叠拦截
        for ex in &self.segs {
            if s.va < ex.end() && ex.va < s.end() {
                return Err(E_OVERLAP);
            }
        }
        self.segs.push(s);
        let bytes = if s.kind.has_file_image() {
            s.pages * PAGE
        } else {
            0
        };
        self.loaded_bytes += bytes;
        Ok(bytes)
    }

    /// 封窗：载入账守恒。
    pub fn seal(&mut self) -> Result<(u64, u64), u16> {
        if self.segs.is_empty() {
            return Err(E_EMPTY);
        }
        let file_bytes: u64 = self
            .segs
            .iter()
            .filter(|s| s.kind.has_file_image())
            .map(|s| s.pages * PAGE)
            .sum();
        if file_bytes != self.loaded_bytes {
            return Err(E_BAL);
        }
        // 全段权限核（终审）
        for s in &self.segs {
            if s.perm != s.kind.expected_perm() {
                return Err(E_PERM);
            }
        }
        self.sealed = true;
        Ok((self.segs.len() as u64, self.loaded_bytes))
    }

    /// 覆盖页数（含 BSS）。
    pub fn covered_pages(&self) -> u64 {
        self.segs.iter().map(|s| s.pages).sum()
    }

    /// 段类直方图（4 桶）。
    pub fn kind_histogram(&self) -> [u64; 4] {
        let mut h = [0u64; 4];
        for s in &self.segs {
            h[seg_order(s.kind) as usize] += 1;
        }
        h
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.window_base.to_le_bytes());
        for s in &self.segs {
            buf.extend_from_slice(&s.va.to_le_bytes());
            buf.extend_from_slice(&s.pages.to_le_bytes());
            buf.push(seg_order(s.kind));
            buf.push(s.perm);
        }
        fnv1a64(&buf)
    }
}

/// FNV-1a 64 位折叠（与 unx0095 指纹同根算法，本地独立副本以保持模块封闭）。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 确定性演练：合法 4 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    const WB: u64 = 0xFFFF_8000_0000_0000;

    // R1：四段标准布局全径载入 + 封窗。
    let mut l = SegLoader::new(WB, 256)?;
    l.load(LoadSeg { kind: SegKind::Text, va: WB, pages: 16, perm: P_R | P_X })?;
    l.load(LoadSeg { kind: SegKind::Rodata, va: WB + 64 * PAGE, pages: 8, perm: P_R })?;
    l.load(LoadSeg { kind: SegKind::Data, va: WB + 128 * PAGE, pages: 8, perm: P_R | P_W })?;
    l.load(LoadSeg { kind: SegKind::Bss, va: WB + 192 * PAGE, pages: 32, perm: P_R | P_W })?;
    let (n, bytes) = l.seal()?;
    if n != 4 || bytes != (16 + 8 + 8) * PAGE {
        return Err(E_BAL);
    }
    if l.covered_pages() != 64 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：直方图桶闭集。
    let h = l.kind_histogram();
    if h != [1, 1, 1, 1] {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：权限名渲染。
    if perm_name(P_R | P_X) != "R-X" || perm_name(P_R) != "R--" || perm_name(P_R | P_W) != "RW-" {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：BSS 零文件载荷——载入字节账只算文件段。
    let mut l4 = SegLoader::new(WB, 64)?;
    l4.load(LoadSeg { kind: SegKind::Text, va: WB, pages: 4, perm: P_R | P_X })?;
    l4.load(LoadSeg { kind: SegKind::Bss, va: WB + 8 * PAGE, pages: 8, perm: P_R | P_W })?;
    let (_, b4) = l4.seal()?;
    if b4 != 4 * PAGE {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：未对齐载入 → E_ALIGN。
    let mut i1 = SegLoader::new(WB, 64)?;
    if i1.load(LoadSeg { kind: SegKind::Text, va: WB + 0x1234, pages: 1, perm: P_R | P_X }) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I2：权限不符 → E_PERM。
    if i1.load(LoadSeg { kind: SegKind::Text, va: WB, pages: 1, perm: P_R | P_W }) != Err(E_PERM) {
        return Err(E_PERM);
    }
    hits += 1;

    // I3：越窗 → E_RANGE。
    if i1.load(LoadSeg { kind: SegKind::Text, va: WB + 64 * PAGE, pages: 8, perm: P_R | P_X }) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I4：载入序倒置（BSS 先于 Text）→ E_ORDER。
    let mut i4 = SegLoader::new(WB, 64)?;
    i4.load(LoadSeg { kind: SegKind::Bss, va: WB, pages: 2, perm: P_R | P_W })?;
    if i4.load(LoadSeg { kind: SegKind::Text, va: WB + 4 * PAGE, pages: 2, perm: P_R | P_X }) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I5：段重叠 → E_OVERLAP。
    let mut i5 = SegLoader::new(WB, 64)?;
    i5.load(LoadSeg { kind: SegKind::Text, va: WB, pages: 4, perm: P_R | P_X })?;
    if i5.load(LoadSeg { kind: SegKind::Rodata, va: WB + 2 * PAGE, pages: 4, perm: P_R }) != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I6：空段 → E_EMPTY。
    if i5.load(LoadSeg { kind: SegKind::Data, va: WB + 8 * PAGE, pages: 0, perm: P_R | P_W }) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
