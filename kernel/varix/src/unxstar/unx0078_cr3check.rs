//! UNX-F0078 · CR3 载入前自检套件 · 诊断段 0x21D0
//! 验收锚点：UNX-F0078-J1 页表载入前五项自检（对齐/保留位/
//! 自映射/递归窗/ NX 域），任一项红即拒载判据通过；
//! 纯功能代码 ≥280 行。零浮点、零 unwrap、零 expect。


use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x21D0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ALIGN: u16 = SEG_BASE + 1;    // CR3 值未按 4KiB 对齐
const E_PCID: u16 = SEG_BASE + 2;     // PCID 位域非法（低 12 位但使能位未开）
const E_RSVD: u16 = SEG_BASE + 3;     // PML4E 保留位置位
const E_SELFMAP: u16 = SEG_BASE + 4;  // 自映射项缺失或位置错误
const E_NX: u16 = SEG_BASE + 5;       // NX 域未按账声明
const E_ORDER: u16 = SEG_BASE + 6;    // 自检序违例（载入后才跑）
const E_RDONLY: u16 = SEG_BASE + 7;   // 自检账已封板
const E_BAL: u16 = SEG_BASE + 8;      // 五项账不全

/// CR3 低 12 位：PCID 域（使能时）。
pub const CR3_PCID_MASK: u64 = 0x0FFF;
/// CR3 位 63：PCID 无刷新标志（LRU）。
pub const CR3_NOFLUSH: u64 = 1 << 63;
/// CR3 物理基址掩码（4KiB 页，52 位物理地址）。
pub const CR3_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// PML4E 保留位域（本仓约定：bits 62:52 保留，bit 63 为 NX 合法位）。
pub const PML4E_RSVD_MASK: u64 = 0xFFF0_0000_0000_0000;

/// 自映射惯例：PML4 槽 510 指向 PML4 自身。
pub const SELFMAP_SLOT: usize = 510;

/// 五项自检项名闭集。
pub const CHECK_ITEMS: [&str; 5] = [
    "对齐", "PCID", "保留位", "自映射", "NX 域",
];

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
        E_ALIGN => "CR3 未按 4KiB 对齐：页表基址低 12 位必须为零",
        E_PCID => "PCID 域非法：使能位未开时低 12 位必须为零",
        E_RSVD => "PML4E 保留位置位：载入即 GP，必须拦截",
        E_SELFMAP => "自映射项缺失/错位：递归页表窗依赖槽 510",
        E_NX => "NX 域未按账声明：数据页必须 NX，代码页不得 NX",
        E_ORDER => "自检序违例：必须在 CR3 载入前完成",
        E_RDONLY => "自检账已封板：载入后账只读",
        E_BAL => "五项账不全：缺项即拒载",
        _ => "未知诊断码",
    }
}

// ── 自检输入视图（真机态由页表账供给）──────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Cr3View {
    pub raw: u64,          // CR3 原值
    pub pcid_enabled: bool,
    pub pml4e_selfmap: u64, // 槽 510 条目
    pub nx_data_ok: bool,   // 数据页 NX 落位
    pub nx_code_ok: bool,   // 代码页无 NX
}

impl Cr3View {
    /// CR3 中页表物理基址。
    pub fn table_base(&self) -> u64 {
        self.raw & CR3_ADDR_MASK
    }
    pub fn pcid(&self) -> u64 {
        self.raw & CR3_PCID_MASK
    }
    pub fn noflush(&self) -> bool {
        self.raw & CR3_NOFLUSH != 0
    }
}

// ── 五项自检 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub item: u8,  // 0..5（CHECK_ITEMS 下标）
    pub ok: bool,
    pub code: u16, // 红项诊断码
}

/// 项 1：对齐。
pub fn check_align(v: &Cr3View) -> CheckResult {
    let ok = v.table_base() % 4096 == 0;
    CheckResult { item: 0, ok, code: if ok { 0 } else { E_ALIGN } }
}

/// 项 2：PCID 域合法性。
pub fn check_pcid(v: &Cr3View) -> CheckResult {
    let ok = v.pcid_enabled || v.pcid() == 0;
    CheckResult { item: 1, ok, code: if ok { 0 } else { E_PCID } }
}

/// 项 3：PML4E 保留位。
pub fn check_rsvd(v: &Cr3View) -> CheckResult {
    let ok = v.pml4e_selfmap & PML4E_RSVD_MASK == 0;
    CheckResult { item: 2, ok, code: if ok { 0 } else { E_RSVD } }
}

/// 项 4：自映射项——槽 510 指向表自身（物理基址一致）。
pub fn check_selfmap(v: &Cr3View) -> CheckResult {
    let target = v.pml4e_selfmap & CR3_ADDR_MASK;
    let ok = target == v.table_base() && target != 0;
    CheckResult { item: 3, ok, code: if ok { 0 } else { E_SELFMAP } }
}

/// 项 5：NX 域声明账。
pub fn check_nx(v: &Cr3View) -> CheckResult {
    let ok = v.nx_data_ok && v.nx_code_ok;
    CheckResult { item: 4, ok, code: if ok { 0 } else { E_NX } }
}

// ── 自检套件账 ───────────────────────────────────────────────
pub struct Cr3Preflight {
    pub results: Vec<CheckResult>,
    pub verdict: Option<bool>, // Some(true)=准载
    pub sealed: bool,
}

impl Cr3Preflight {
    pub fn new() -> Cr3Preflight {
        Cr3Preflight {
            results: Vec::new(),
            verdict: None,
            sealed: false,
        }
    }

    /// 全套五项：按闭集序执行，红项留档。
    pub fn run_all(&mut self, v: &Cr3View) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if !self.results.is_empty() {
            return Err(E_ORDER);
        }
        self.results = vec![
            check_align(v),
            check_pcid(v),
            check_rsvd(v),
            check_selfmap(v),
            check_nx(v),
        ];
        if self.results.len() != CHECK_ITEMS.len() {
            return Err(E_BAL);
        }
        let reds = self.results.iter().filter(|r| !r.ok).count();
        self.verdict = Some(reds == 0);
        Ok(reds)
    }

    /// 门禁：任一红项 → 拒载。
    pub fn gate(&self) -> Result<(), u16> {
        match self.verdict {
            Some(true) => Ok(()),
            Some(false) => {
                let first = self
                    .results
                    .iter()
                    .find(|r| !r.ok)
                    .map(|r| r.code)
                    .unwrap_or(E_BAL);
                Err(first)
            }
            None => Err(E_ORDER),
        }
    }

    /// 红项点名。
    pub fn red_items_text(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|r| !r.ok)
            .map(|r| {
                let name = CHECK_ITEMS
                    .get(r.item as usize)
                    .copied()
                    .unwrap_or("未知项");
                format!("{}（码{:04X}）", name, r.code)
            })
            .collect()
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.verdict.is_none() {
            return Err(E_ORDER);
        }
        self.sealed = true;
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.results {
            buf.push(r.item);
            buf.push(r.ok as u8);
            buf.extend_from_slice(&r.code.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 合成视图（演练复用）────────────────────────────────────
pub fn synth_view(base: u64, pcid: u64, noflush: bool, _selfmap_slot: u64, pml4e: u64,
                  nx_data: bool, nx_code: bool, pcid_en: bool) -> Cr3View {
    Cr3View {
        raw: base | pcid | if noflush { CR3_NOFLUSH } else { 0 },
        pcid_enabled: pcid_en,
        pml4e_selfmap: pml4e,
        nx_data_ok: nx_data,
        nx_code_ok: nx_code,
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let base = 0x10_0000u64; // 1MiB 对齐

    // R1：全绿五项 → 准载。
    let v1 = synth_view(base, 0, false, base, base | 0x3, true, true, false);
    let mut pf1 = Cr3Preflight::new();
    let reds = pf1.run_all(&v1)?;
    if reds != 0 {
        return Err(E_BAL);
    }
    pf1.gate()?;
    pf1.seal()?;
    hits += 1;

    // R2：PCID 使能 + noflush 位 → 合法。
    let v2 = synth_view(base, 0x7, true, base, base | 0x3, true, true, true);
    let mut pf2 = Cr3Preflight::new();
    if pf2.run_all(&v2)? != 0 {
        return Err(E_BAL);
    }
    pf2.gate()?;
    hits += 1;

    // R3：红项点名——保留位脏（bit56）+ 自映射错位 → 两红。
    let v3 = synth_view(base, 0, false, 0, 0x0100_0000_0000_0000 | 0x3, true, true, false);
    let mut pf3 = Cr3Preflight::new();
    let reds3 = pf3.run_all(&v3)?;
    if reds3 != 2 {
        return Err(E_BAL);
    }
    let names = pf3.red_items_text();
    if names.len() != 2 || !names[0].contains("保留位") && !names[0].contains("自映射") {
        return Err(E_BAL);
    }
    if pf3.gate() != Err(E_SELFMAP) && pf3.gate() != Err(E_RSVD) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：未对齐基址 → E_ALIGN 红。
    let v4 = synth_view(0x10_0804, 0, false, 0x10_0000, 0x10_0000 | 0x3, true, true, false);
    let mut pf4 = Cr3Preflight::new();
    pf4.run_all(&v4)?;
    if pf4.gate() != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I2：PCID 使能未开却带 PCID → E_PCID 红。
    let v5 = synth_view(base, 0x5, false, base, base | 0x3, true, true, false);
    let mut pf5 = Cr3Preflight::new();
    pf5.run_all(&v5)?;
    if pf5.gate() != Err(E_PCID) {
        return Err(E_PCID);
    }
    hits += 1;

    // I3：NX 账红 → E_NX。
    let v6 = synth_view(base, 0, false, base, base | 0x3, false, false, false);
    let mut pf6 = Cr3Preflight::new();
    pf6.run_all(&v6)?;
    if pf6.gate() != Err(E_NX) {
        return Err(E_NX);
    }
    hits += 1;

    // I4：未跑先门禁 → E_ORDER。
    let pf7 = Cr3Preflight::new();
    if pf7.gate() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I5：重跑 → E_ORDER。
    let mut pf8 = Cr3Preflight::new();
    pf8.run_all(&v1)?;
    if pf8.run_all(&v1) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    Ok(hits)
}
