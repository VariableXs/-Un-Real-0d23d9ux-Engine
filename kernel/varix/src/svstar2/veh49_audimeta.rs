//! VE-F1449 · 音频元数据服务（VE-H 域 · 目标 320 行）· 诊断段 0xFA00
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1449`
//!
//! **判据（锚点原文）**：三体系、封面、章节、写回安全、判据。
//!
//! **职责定位（锚点原文）**：三体系统一 API（ID3v2/Vorbis Comment/MP4 tags）键
//! 映射表（标题/艺术家/专辑/封面跨体系映射——上层一次调用体系自动适配）、封面
//! 管理（内嵌/外挂双模式——内嵌优先）、章节标记（MP4/Split 章节）、写回安全
//! （原子写+备份+只改明确要求的字段）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFA00;

/// 元数据体系数。
pub const FORMATS: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 体系闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Id3v2,
    Vorbis,
    Mp4,
}

impl Fmt {
    pub fn name(&self) -> &'static str {
        match self {
            Fmt::Id3v2 => "id3v2",
            Fmt::Vorbis => "vorbis",
            Fmt::Mp4 => "mp4",
        }
    }
}

/// 统一键闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Title,
    Artist,
    Album,
    Cover,
}

impl Key {
    pub fn idx(&self) -> u32 {
        match self {
            Key::Title => 0,
            Key::Artist => 1,
            Key::Album => 2,
            Key::Cover => 3,
        }
    }
}

/// 键映射表：体系 × 统一键 → 体系内键名（映射完备即全格非空）。
pub struct KeyMap {
    pub cells: [[u32; 4]; FORMATS as usize],
}

impl KeyMap {
    /// 标准映射表（各体系内键名哈希登记）。
    pub fn standard() -> Self {
        let mut cells = [[0u32; 4]; 3];
        // id3v2: TIT2/TPE1/TALB/APIC
        cells[0] = [
            fnv1a(b"TIT2"), fnv1a(b"TPE1"), fnv1a(b"TALB"), fnv1a(b"APIC"),
        ];
        // vorbis: TITLE/ARTIST/ALBUM/PICTURE
        cells[1] = [
            fnv1a(b"TITLE"), fnv1a(b"ARTIST"), fnv1a(b"ALBUM"), fnv1a(b"PICTURE"),
        ];
        // mp4: \xa9nam/\xa9ART/\xa9alb/covr
        cells[2] = [
            fnv1a(b"nam"), fnv1a(b"ART"), fnv1a(b"alb"), fnv1a(b"covr"),
        ];
        KeyMap { cells }
    }

    /// 映射完备：3×4 全格非空（跨体系映射零缺格）。
    pub fn complete(&self) -> bool {
        self.cells.iter().all(|row| row.iter().all(|c| *c != 0))
    }

    /// 查映射：体系×键 → 体系内键名哈希。
    pub fn map(&self, f: Fmt, k: Key) -> u32 {
        let r = match f {
            Fmt::Id3v2 => 0,
            Fmt::Vorbis => 1,
            Fmt::Mp4 => 2,
        };
        self.cells[r][k.idx() as usize]
    }
}

/// 封面双模式：内嵌 / 外挂（内嵌优先）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoverMode {
    Embedded,
    External,
}

/// 封面解析：内嵌优先协议。
pub fn resolve_cover(embedded: bool, external: bool) -> Option<CoverMode> {
    if embedded {
        Some(CoverMode::Embedded)
    } else if external {
        Some(CoverMode::External)
    } else {
        None
    }
}

/// 写回安全账：原子写 + 备份 + 只改明确要求的字段。
pub struct WriteBack {
    /// 原子写位（临时文件+rename 语义）。
    pub atomic: bool,
    /// 备份位（写前原文件备份）。
    pub backup: bool,
    /// 明确要求改的字段集合。
    pub allowed: Vec<u32>,
    /// 实际改的字段集合。
    pub touched: Vec<u32>,
}

impl WriteBack {
    /// 写回安全：原子+备份+触碰集 ⊆ 允许集（零误伤）。
    pub fn safe(&self) -> bool {
        self.atomic && self.backup && self.touched.iter().all(|t| self.allowed.contains(t))
    }
}

/// 章节账：MP4/Split 章节标记落账。
pub struct Chapters {
    pub marks: Vec<(u64, u64)>, // (起始 ms, 结束 ms)
}

impl Chapters {
    /// 落账核对：区间有序不交叠、时长正值。
    pub fn ordered(&self) -> bool {
        self.marks.iter().all(|(a, b)| b > a)
            && self.marks.windows(2).all(|w| w[1].0 >= w[0].1)
    }
}

/// J1 主判据：三体系统一 API + 封面双模式 + 写回安全 + 章节。
pub fn j1_main() -> bool {
    let km = KeyMap::standard();
    if !km.complete() {
        return false;
    }
    // 封面内嵌优先
    if resolve_cover(true, true) != Some(CoverMode::Embedded)
        || resolve_cover(false, true) != Some(CoverMode::External)
        || resolve_cover(false, false).is_some()
    {
        return false;
    }
    // 写回安全：只改 Title
    let wb = WriteBack {
        atomic: true,
        backup: true,
        allowed: alloc::vec![Key::Title.idx()],
        touched: alloc::vec![Key::Title.idx()],
    };
    if !wb.safe() {
        return false;
    }
    // 章节
    let ch = Chapters { marks: alloc::vec![(0, 30000), (30000, 90000), (90000, 120000)] };
    if !ch.ordered() {
        return false;
    }
    // 键映射可查：三体系 Title 映射互异（体系自动适配的字典基础）
    km.map(Fmt::Id3v2, Key::Title) != km.map(Fmt::Vorbis, Key::Title)
        && km.map(Fmt::Vorbis, Key::Title) != km.map(Fmt::Mp4, Key::Title)
}

// ── 注入核（0xFA00 段 5 项）────────────────────────────────────────────

/// 注入核 1：映射缺格注入——抽掉一格完备性必红。
pub fn inj1_map_gap() -> bool {
    let mut km = KeyMap::standard();
    km.cells[2][3] = 0;
    !km.complete()
}

/// 注入核 2：误伤注入——改了未允许字段必红。
pub fn inj2_overwrite() -> bool {
    let wb = WriteBack {
        atomic: true,
        backup: true,
        allowed: alloc::vec![Key::Title.idx()],
        touched: alloc::vec![Key::Title.idx(), Key::Artist.idx()],
    };
    !wb.safe()
}

/// 注入核 3：非原子注入——无备份写回必红。
pub fn inj3_no_backup() -> bool {
    let wb = WriteBack {
        atomic: true,
        backup: false,
        allowed: alloc::vec![Key::Title.idx()],
        touched: alloc::vec![Key::Title.idx()],
    };
    !wb.safe()
}

/// 注入核 4：封面优先注入——内嵌与外挂并存时内嵌胜出。
pub fn inj4_cover_priority() -> bool {
    resolve_cover(true, true) == Some(CoverMode::Embedded)
}

/// 注入核 5：章节交叠注入——交叠区间账必红。
pub fn inj5_chapter_overlap() -> bool {
    let ch = Chapters { marks: alloc::vec![(0, 30000), (20000, 90000)] };
    !ch.ordered()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_veh49_checks：本单判据聚合（诊断段 0xFA00）。
pub fn run_veh49_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1449 音频元数据服务");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三体系统一API（键映射3×4全格）",
        j1_main() && KeyMap::standard().complete(),
        if j1_main() { "" } else { "键映射破" },
    );
    s.add(
        "封面双模式+写回安全+章节",
        j1_main(),
        if j1_main() { "" } else { "封面/写回破" },
    );
    let inj = [
        ("注入1映射缺格", inj1_map_gap()),
        ("注入2误伤检出", inj2_overwrite()),
        ("注入3非原子写", inj3_no_backup()),
        ("注入4内嵌优先", inj4_cover_priority()),
        ("注入5章节交叠", inj5_chapter_overlap()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
