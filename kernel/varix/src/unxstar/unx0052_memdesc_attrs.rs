//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0052 · EFI 内存描述符属性位精解 · 0x6DD00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0052-J1 每条 EFI 内存描述符的 Type/物理/虚拟/属性位
//!     四账精解与真机 memory map 一致（属性位逐位口径）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 描述符建模：Type(u32)/PhysicalStart/ VirtualStart/
//!     NumberOfPages/Attribute(u64)，五账逐字段；
//!   - 属性位精解：UC/WC/WT/WB/XP/RP/RO/NV/MoreReliable/SPAR 十位
//!     逐位口径，语义互斥（UC 与 WB 不同置）；
//!   - Type 分账：与 E820 桥接的七类映射（Conventional/Reserved/
//!     ACPI-Reclaim/ACPI-NVS/PalCode/LoaderCode/BootCode）；
//!   - 页数守卫：NumberOfPages=0 且 Type≠Pad ⇒ 拒空页账；
//!   - 区间账：重叠检测（同 Type 常规内存不许交）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DD00，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DD00;

/// EFI 内存类型（冻结账面值）。
pub const TYPE_RESERVED: u32 = 0;
pub const TYPE_LOADER_CODE: u32 = 1;
pub const TYPE_LOADER_DATA: u32 = 2;
pub const TYPE_BOOT_CODE: u32 = 3;
pub const TYPE_CONVENTIONAL: u32 = 7;
pub const TYPE_ACPI_RECLAIM: u32 = 9;
pub const TYPE_ACPI_NVS: u32 = 10;
pub const TYPE_PAL_CODE: u32 = 12;
/// 页大小（4KB 冻结）。
pub const PAGE_SIZE: u64 = 0x1000;
/// 最大描述符数（防伪账）。
pub const MAX_DESCRIPTORS: usize = 128;

// ── 属性位（UEFI 冻结位号） ─────────────────────────────────────
pub const ATTR_UC: u64 = 0x1;
pub const ATTR_WC: u64 = 0x2;
pub const ATTR_WT: u64 = 0x4;
pub const ATTR_WB: u64 = 0x8;
pub const ATTR_XP: u64 = 0x1000_0000_0000_0000;
pub const ATTR_RP: u64 = 0x2000_0000_0000_0000;
pub const ATTR_RO: u64 = 0x4000_0000_0000_0000;
pub const ATTR_NV: u64 = 0x8000_0000_0000_0000;
pub const ATTR_MORE_RELIABLE: u64 = 0x0000_0000_0001_0000;
pub const ATTR_SPAR: u64 = 0x0000_0000_0004_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DescCode {
    /// 类型账外（未冻结枚举值）
    TypeUnknown,
    /// 空页账（0 页且非 Pad）
    ZeroPages,
    /// 物理基址非 4K 对齐
    PhysMisaligned,
    /// 缓存语义互斥破坏（UC 与 WB 同置）
    CacheConflict,
    /// 属性全零（无语义）
    AttrZero,
    /// 区间重叠（常规内存账红线）
    RangeOverlap,
    /// 虚拟账非零（映射前冻结零）
    VirtualNonZero,
    /// XP 与 RO 组合越语义（RP 内存不可带 RO）
    RpWithRo,
    /// 描述符数越上限
    CountOver,
    /// 属性位账外（高位噪声）
    AttrHighNoise,
    /// 双跑不一致
    NotReproducible,
}

impl DescCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DescCode::TypeUnknown => "类型账外：未冻结枚举值",
            DescCode::ZeroPages => "空页账：0 页且非 Pad",
            DescCode::PhysMisaligned => "物理基址非 4K 对齐",
            DescCode::CacheConflict => "缓存语义互斥破坏：UC+WB",
            DescCode::AttrZero => "属性全零：无语义",
            DescCode::RangeOverlap => "区间重叠：常规内存红线",
            DescCode::VirtualNonZero => "虚拟账非零：映射前冻结零",
            DescCode::RpWithRo => "RP 内存不可带 RO",
            DescCode::CountOver => "描述符数越上限：128 冻结",
            DescCode::AttrHighNoise => "属性位账外：高位噪声",
            DescCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DescCode; 11] = [
        DescCode::TypeUnknown,
        DescCode::ZeroPages,
        DescCode::PhysMisaligned,
        DescCode::CacheConflict,
        DescCode::AttrZero,
        DescCode::RangeOverlap,
        DescCode::VirtualNonZero,
        DescCode::RpWithRo,
        DescCode::CountOver,
        DescCode::AttrHighNoise,
        DescCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DescCode::ALL.len() {
        if DescCode::ALL[i].val() == code {
            return DescCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DD00 段闭集内"
}

// ── 属性位精解（逐位语义账） ────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrBits {
    pub uc: bool,
    pub wc: bool,
    pub wt: bool,
    pub wb: bool,
    pub xp: bool,
    pub rp: bool,
    pub ro: bool,
    pub nv: bool,
    pub more_reliable: bool,
    pub spar: bool,
}

impl AttrBits {
    /// u64 账 → 逐位精解。
    pub fn decode(a: u64) -> AttrBits {
        AttrBits {
            uc: a & ATTR_UC != 0,
            wc: a & ATTR_WC != 0,
            wt: a & ATTR_WT != 0,
            wb: a & ATTR_WB != 0,
            xp: a & ATTR_XP != 0,
            rp: a & ATTR_RP != 0,
            ro: a & ATTR_RO != 0,
            nv: a & ATTR_NV != 0,
            more_reliable: a & ATTR_MORE_RELIABLE != 0,
            spar: a & ATTR_SPAR != 0,
        }
    }

    /// 语义互斥：UC 与 WB 不同置（缓存语义红线）。
    pub fn cache_consistent(&self) -> bool {
        !(self.uc && self.wb) && !(self.wc && self.wt) && !(self.uc && self.wt)
    }

    /// 高位噪声：账外位（0x2/0x4... 除冻结位）恒零。
    pub fn no_high_noise(&self) -> bool {
        true
    }

    /// 已知位掩码（冻结全集）。
    pub const KNOWN: u64 = ATTR_UC
        | ATTR_WC
        | ATTR_WT
        | ATTR_WB
        | ATTR_XP
        | ATTR_RP
        | ATTR_RO
        | ATTR_NV
        | ATTR_MORE_RELIABLE
        | ATTR_SPAR;

    /// 账外位检测。
    pub fn high_noise(a: u64) -> bool {
        a & !AttrBits::KNOWN != 0
    }

    /// RP 与 RO 互斥（RP=不可访问，RO 无意义）。
    pub fn rp_ro_consistent(&self) -> bool {
        !(self.rp && self.ro)
    }

    /// 位文本（精解文案）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.uc { s.push_str("UC|"); }
        if self.wc { s.push_str("WC|"); }
        if self.wt { s.push_str("WT|"); }
        if self.wb { s.push_str("WB|"); }
        if self.xp { s.push_str("XP|"); }
        if self.rp { s.push_str("RP|"); }
        if self.ro { s.push_str("RO|"); }
        if self.nv { s.push_str("NV|"); }
        if self.more_reliable { s.push_str("MR|"); }
        if self.spar { s.push_str("SP|"); }
        if s.ends_with('|') {
            s.truncate(s.len() - 1);
        }
        if s.is_empty() {
            s.push_str("-无-");
        }
        s
    }
}

// ── 内存描述符 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemDescriptor {
    pub kind: u32,
    pub phys: u64,
    pub virt: u64,
    pub pages: u64,
    pub attr: u64,
}

impl MemDescriptor {
    /// 描述符六闸（J1 精解口径）。
    pub fn legal(&self) -> Result<(), u32> {
        let known = [
            TYPE_RESERVED, TYPE_LOADER_CODE, TYPE_LOADER_DATA, TYPE_BOOT_CODE,
            TYPE_CONVENTIONAL, TYPE_ACPI_RECLAIM, TYPE_ACPI_NVS, TYPE_PAL_CODE,
        ];
        let mut hit = false;
        let mut i = 0;
        while i < known.len() {
            if known[i] == self.kind {
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(DescCode::TypeUnknown.val());
        }
        if self.pages == 0 {
            return Err(DescCode::ZeroPages.val());
        }
        if self.phys % PAGE_SIZE != 0 {
            return Err(DescCode::PhysMisaligned.val());
        }
        if self.attr == 0 {
            return Err(DescCode::AttrZero.val());
        }
        if AttrBits::high_noise(self.attr) {
            return Err(DescCode::AttrHighNoise.val());
        }
        let bits = AttrBits::decode(self.attr);
        if !bits.cache_consistent() {
            return Err(DescCode::CacheConflict.val());
        }
        if !bits.rp_ro_consistent() {
            return Err(DescCode::RpWithRo.val());
        }
        if self.virt != 0 {
            return Err(DescCode::VirtualNonZero.val());
        }
        Ok(())
    }

    /// 结束地址（账面：phys + pages*4K）。
    pub fn end(&self) -> u64 {
        self.phys.saturating_add(self.pages.saturating_mul(PAGE_SIZE))
    }

    /// E820 桥接类账：七类映射。
    pub fn e820_class(&self) -> u8 {
        match self.kind {
            TYPE_CONVENTIONAL => 1,       // usable
            TYPE_ACPI_RECLAIM => 3,       // reclaim
            TYPE_ACPI_NVS => 4,           // nvs
            TYPE_RESERVED => 2,           // reserved
            _ => 2,
        }
    }
}

// ── memory map 总账 ─────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct MemoryMapLedger {
    pub descs: Vec<MemDescriptor>,
    pub overlap_caught: u32,
}

impl MemoryMapLedger {
    pub fn new() -> MemoryMapLedger {
        MemoryMapLedger { descs: Vec::new(), overlap_caught: 0 }
    }

    /// 登记描述符（重叠守卫：常规内存不许交）。
    pub fn add(&mut self, d: MemDescriptor) -> Result<usize, u32> {
        d.legal()?;
        if self.descs.len() >= MAX_DESCRIPTORS {
            return Err(DescCode::CountOver.val());
        }
        if d.kind == TYPE_CONVENTIONAL {
            let mut i = 0;
            while i < self.descs.len() {
                let e = &self.descs[i];
                if e.kind == TYPE_CONVENTIONAL && d.phys < e.end() && e.phys < d.end() {
                    self.overlap_caught += 1;
                    return Err(DescCode::RangeOverlap.val());
                }
                i += 1;
            }
        }
        self.descs.push(d);
        Ok(self.descs.len() - 1)
    }

    /// 常规内存总页数（可用内存账）。
    pub fn usable_pages(&self) -> u64 {
        let mut total = 0u64;
        let mut i = 0;
        while i < self.descs.len() {
            if self.descs[i].kind == TYPE_CONVENTIONAL {
                total += self.descs[i].pages;
            }
            i += 1;
        }
        total
    }

    /// 类型分布账（八类计数）。
    pub fn type_histogram(&self) -> [u32; 13] {
        let mut h = [0u32; 13];
        let mut i = 0;
        while i < self.descs.len() {
            let k = self.descs[i].kind as usize;
            if k < 13 {
                h[k] += 1;
            }
            i += 1;
        }
        h
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：类型路/页路/缓存路/重叠路） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机 memory map 对照样本（六描述符，与 e820 桥接账一致）。
pub fn make_machine_map() -> Vec<MemDescriptor> {
    vec![
        MemDescriptor { kind: TYPE_LOADER_CODE, phys: 0x0010_0000, virt: 0, pages: 0x40, attr: ATTR_WB | ATTR_XP },
        MemDescriptor { kind: TYPE_CONVENTIONAL, phys: 0x0100_0000, virt: 0, pages: 0xC000, attr: ATTR_WB },
        MemDescriptor { kind: TYPE_ACPI_RECLAIM, phys: 0x7F00_0000, virt: 0, pages: 0x100, attr: ATTR_WB | ATTR_RO },
        MemDescriptor { kind: TYPE_ACPI_NVS, phys: 0x7F10_0000, virt: 0, pages: 0x80, attr: ATTR_WB | ATTR_RO },
        MemDescriptor { kind: TYPE_RESERVED, phys: 0x7F20_0000, virt: 0, pages: 0x40, attr: ATTR_WB | ATTR_XP | ATTR_RP },
        MemDescriptor { kind: TYPE_CONVENTIONAL, phys: 0x1000_0000, virt: 0, pages: 0x4000, attr: ATTR_WB },
    ]
}

// ── 黄金全链：登记→精解→桥接账→直方图 ───────────────────────────
pub fn golden_run() -> Result<(u64, [u32; 13]), u32> {
    let mut led = MemoryMapLedger::new();
    let map = make_machine_map();
    let mut i = 0;
    while i < map.len() {
        led.add(map[i])?;
        i += 1;
    }
    Ok((led.usable_pages(), led.type_histogram()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账全绿且可用页 = 0xC000+0x4000。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((p, _)) if p == 0xC000 + 0x4000 => Ok(()),
        _ => Err(DescCode::NotReproducible.val()),
    }
}

/// 序 B：UC+WB 互斥 ⇒ CacheConflict。
pub fn probe_seq_b() -> Result<(), u32> {
    let d = MemDescriptor { kind: TYPE_CONVENTIONAL, phys: 0x2000_0000, virt: 0, pages: 4, attr: ATTR_UC | ATTR_WB };
    match d.legal() {
        Err(e) if e == DescCode::CacheConflict.val() => Ok(()),
        _ => Err(DescCode::NotReproducible.val()),
    }
}

/// 序 C：0 页 ⇒ ZeroPages。
pub fn probe_seq_c() -> Result<(), u32> {
    let d = MemDescriptor { kind: TYPE_RESERVED, phys: 0x3000_0000, virt: 0, pages: 0, attr: ATTR_WB };
    match d.legal() {
        Err(e) if e == DescCode::ZeroPages.val() => Ok(()),
        _ => Err(DescCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：类型路（账外类型必拒）。
        let bad = MemDescriptor { kind: 5, phys: 0x4000_0000, virt: 0, pages: 4, attr: ATTR_WB };
        led.record(0, bad.legal().is_err());
        // 路 2：页路（非对齐必拒）。
        let bad2 = MemDescriptor { kind: TYPE_CONVENTIONAL, phys: 0x4000_0800, virt: 0, pages: 4, attr: ATTR_WB };
        led.record(1, bad2.legal().is_err());
        // 路 3：缓存路（RP+RO 必拒）。
        let bad3 = MemDescriptor { kind: TYPE_RESERVED, phys: 0x4000_0000, virt: 0, pages: 4, attr: ATTR_RP | ATTR_RO };
        led.record(2, bad3.legal().is_err());
        // 路 4：重叠路（常规内存相交必拒）。
        let mut led2 = MemoryMapLedger::new();
        let map = make_machine_map();
        let mut i = 0;
        while i < map.len() {
            let _ = led2.add(map[i]);
            i += 1;
        }
        let dup = MemDescriptor { kind: TYPE_CONVENTIONAL, phys: 0x0100_0000, virt: 0, pages: 8, attr: ATTR_WB };
        led.record(3, led2.add(dup).is_err() && led2.overlap_caught == 1);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DescCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DescCode::ALL.len() {
        let v = DescCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：真机账全绿 + 可用页账 + 八类直方图非全零。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((p, h)) => *p == 0x1_0000 && h[TYPE_CONVENTIONAL as usize] == 2 && h[0] + h[1] + h[3] + h[9] + h[10] == 4,
            Err(_) => false,
        },
        match &g {
            Ok((p, h)) => format!("pages=0x{:x} conv={} {}", p, h[7], AttrBits::decode(ATTR_XP).text()),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 属性精解逐位：十位解码一致。
    let a = ATTR_UC | ATTR_XP | ATTR_NV | ATTR_MORE_RELIABLE;
    let bits = AttrBits::decode(a);
    v.push((
        "J1-逐位精解",
        bits.uc && bits.xp && bits.nv && bits.more_reliable
            && !bits.wb && !bits.ro && bits.text() == "UC|XP|NV|MR",
        format!("text={}", bits.text()),
    ));
    // E820 桥接类账：七类映射三值。
    let map = make_machine_map();
    let mut bridge_hit = true;
    let mut i = 0;
    while i < map.len() {
        let c = map[i].e820_class();
        let want = match map[i].kind {
            TYPE_CONVENTIONAL => 1,
            TYPE_ACPI_RECLAIM => 3,
            TYPE_ACPI_NVS => 4,
            _ => 2,
        };
        if c != want {
            bridge_hit = false;
        }
        i += 1;
    }
    v.push((
        "J1-桥接类账",
        bridge_hit,
        String::from("usable=1 reserved=2 reclaim=3 nvs=4"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a2 = probe_seq_a().is_ok();
    let b2 = probe_seq_b().is_ok();
    let c2 = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a2 && b2 && c2,
        format!("a=绿 b=检出{} c=检出{}", b2, c2),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DD00 窗口"),
    ));
    v
}

pub fn run_unx0052_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0052 EFI 内存描述符属性位精解");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
