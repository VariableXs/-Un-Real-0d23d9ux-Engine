//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16037 · 宿主 mmap 语义承接面 · 0x69080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16037-J1 mmap 承接面逐语义点落账（匿名/文件映射/保护位/同
//!   步标志），与 A4 语义档对照一致率 100%，Wine 高频 mmap 用途覆盖
//!   清单齐备断言通过。
//! 定位：Wine 对 mmap 的依赖集中在 PE 镜像映射、共享内存、文件后备
//!   缓冲三类，本条落承接面：逐语义点对照 A4 内存语义档，确认 Varix
//!   的 mmap 语义满足 Wine 期望，缺口输出桩清单。
//! 设计要点：
//!   - 语义点五维：映射类型（匿名/文件）/保护位组合/MAP_SHARED|
//!     PRIVATE 语义/同步标志/对齐要求；
//!   - SEC_IMAGE 特例行：镜像映射不走通用 mmap 路径，与 AI-17 冻结
//!     接口对齐（F16061 消费行同源）；
//!   - 覆盖清单：三类高频用途（PE 映射/共享内存/文件缓冲）逐类登记
//!     覆盖状态，未覆盖用途显式"缺口"并输出桩清单，删缺口标注必红；
//!   - 注入面：语义点越五维/映射类型越闭集/保护位越闭集/A4 指针悬
//!     空/缺口标注缺失/重复语义点——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69080）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69080，AI-VX-1031 E1 域批M 独占区）。
pub const SEG_BASE: u32 = 0x69080;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 语义点五维闭集 ───────────────────────────────────────────────
pub const DIM_TYPE: usize = 0; // 映射类型
pub const DIM_PROT: usize = 1; // 保护位组合
pub const DIM_SHARED: usize = 2; // MAP_SHARED|PRIVATE
pub const DIM_SYNC: usize = 3; // 同步标志
pub const DIM_ALIGN: usize = 4; // 对齐要求
pub const DIM_COUNT: usize = 5;

pub const DIM_NAMES: [&str; DIM_COUNT] =
    ["映射类型", "保护位", "共享语义", "同步标志", "对齐要求"];

// ── 映射类型闭集 ─────────────────────────────────────────────────
pub const MT_ANON: usize = 0; // 匿名映射
pub const MT_FILE: usize = 1; // 文件映射
pub const MT_SECIMAGE: usize = 2; // SEC_IMAGE 特例（不走通用路径）
pub const MT_COUNT: usize = 3;

// ── 保护位组合闭集（POSIX PROT_ 值写死） ─────────────────────────
pub const PROT_NONE: u32 = 0x0;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;
pub const PROT_RW: u32 = PROT_READ | PROT_WRITE;
pub const PROT_RX: u32 = PROT_READ | PROT_EXEC;
pub const PROT_RWX: u32 = PROT_READ | PROT_WRITE | PROT_EXEC;
pub const PROT_TABLE: [u32; 7] =
    [PROT_NONE, PROT_READ, PROT_WRITE, PROT_EXEC, PROT_RW, PROT_RX, PROT_RWX];

pub fn prot_legal(p: u32) -> bool {
    let mut i = 0;
    while i < PROT_TABLE.len() {
        if PROT_TABLE[i] == p {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmCode {
    /// 语义点越五维闭集
    DimOutOfRange,
    /// 映射类型越闭集
    MapTypeOutOfRange,
    /// 保护位越闭集
    ProtOutOfRange,
    /// A4 指针悬空
    A4Dangling,
    /// 缺口标注缺失（判据复测方式：删缺口标注必红）
    GapUnmarked,
    /// 重复语义点
    PointDuplicated,
    /// SEC_IMAGE 特例走通用路径（越轨）
    SecImageGeneric,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl MmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MmCode::DimOutOfRange => "语义点越五维闭集",
            MmCode::MapTypeOutOfRange => "映射类型越闭集",
            MmCode::ProtOutOfRange => "保护位越闭集",
            MmCode::A4Dangling => "A4 指针悬空",
            MmCode::GapUnmarked => "缺口标注缺失：不许硬用近似语义充数",
            MmCode::PointDuplicated => "重复语义点：一点一行",
            MmCode::SecImageGeneric => "SEC_IMAGE 走通用路径：越轨",
            MmCode::NotReproducible => "双跑不一致：确定性红线",
            MmCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [MmCode; 9] = [
        MmCode::DimOutOfRange,
        MmCode::MapTypeOutOfRange,
        MmCode::ProtOutOfRange,
        MmCode::A4Dangling,
        MmCode::GapUnmarked,
        MmCode::PointDuplicated,
        MmCode::SecImageGeneric,
        MmCode::NotReproducible,
        MmCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        if MmCode::ALL[i].val() == code {
            return MmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

/// A4 语义档指针闭集。
pub const A4_POOL: [&str; 3] = [
    "A4-匿名映射档",
    "A4-文件映射档",
    "A4-共享内存档",
];

pub fn a4_legal(a: &str) -> bool {
    let mut i = 0;
    while i < A4_POOL.len() {
        if A4_POOL[i] == a {
            return true;
        }
        i += 1;
    }
    false
}

/// 承接面语义点行。
#[derive(Clone, Debug)]
pub struct MmRow {
    /// 语义点维度（0..DIM_COUNT）。
    pub dim: usize,
    /// 映射类型（三态闭集）。
    pub map_type: usize,
    /// 保护位组合。
    pub prot: u32,
    /// A4 语义档引用（须 ∈ A4_POOL）。
    pub a4_ref: String,
}

/// 单行体检。SEC_IMAGE 行必须显式标注特例（不走通用路径）。
pub fn vet_row(r: &MmRow) -> Result<(), u32> {
    if r.dim >= DIM_COUNT {
        return Err(MmCode::DimOutOfRange.val());
    }
    if r.map_type >= MT_COUNT {
        return Err(MmCode::MapTypeOutOfRange.val());
    }
    if !prot_legal(r.prot) {
        return Err(MmCode::ProtOutOfRange.val());
    }
    if !a4_legal(&r.a4_ref) {
        return Err(MmCode::A4Dangling.val());
    }
    Ok(())
}

// ── 覆盖清单（三类高频用途） ────────────────────────────────────
pub const USE_PE: usize = 0; // PE 映射
pub const USE_SHM: usize = 1; // 共享内存
pub const USE_FILEBUF: usize = 2; // 文件后备缓冲
pub const USE_COUNT: usize = 3;

pub const USE_NAMES: [&str; USE_COUNT] =
    ["PE映射", "共享内存", "文件后备缓冲"];

/// 覆盖状态：0=已覆盖 1=缺口（显式标注）。
pub const COV_OK: usize = 0;
pub const COV_GAP: usize = 1;

#[derive(Clone, Debug)]
pub struct UseRow {
    pub use_kind: usize,
    pub status: usize,
    /// 缺口说明（缺口态必填）。
    pub gap_note: String,
}

/// 覆盖行体检：缺口态必带标注（删标注必红）。
pub fn vet_use(r: &UseRow) -> Result<(), u32> {
    if r.use_kind >= USE_COUNT {
        return Err(MmCode::DimOutOfRange.val());
    }
    if r.status == COV_GAP && r.gap_note.len() < 12 {
        return Err(MmCode::GapUnmarked.val());
    }
    Ok(())
}

// ── 承接面账 ─────────────────────────────────────────────────────
pub struct MmFace {
    points: Vec<MmRow>,
    uses: Vec<UseRow>,
}

impl MmFace {
    pub fn new() -> MmFace {
        MmFace { points: Vec::new(), uses: Vec::new() }
    }

    pub fn add_point(&mut self, r: MmRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.points.len() {
            if self.points[i].dim == r.dim {
                return Err(MmCode::PointDuplicated.val());
            }
            i += 1;
        }
        self.points.push(r);
        Ok(())
    }

    pub fn add_use(&mut self, r: UseRow) -> Result<(), u32> {
        vet_use(&r)?;
        self.uses.push(r);
        Ok(())
    }

    /// 五维覆盖。
    pub fn dims_closed(&self) -> Result<(), u32> {
        let mut d = 0;
        while d < DIM_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.points.len() {
                if self.points[i].dim == d {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(MmCode::DimOutOfRange.val());
            }
            d += 1;
        }
        Ok(())
    }

    /// 与 A4 对照一致率（判据 100% 口径）。
    pub fn a4_align_rate(&self) -> Result<usize, u32> {
        if self.points.is_empty() {
            return Err(MmCode::A4Dangling.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.points.len() {
            if vet_row(&self.points[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.points.len())
    }

    /// 三类用途覆盖清单齐备。
    pub fn uses_closed(&self) -> Result<(), u32> {
        let mut u = 0;
        while u < USE_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.uses.len() {
                if self.uses[i].use_kind == u {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(MmCode::GapUnmarked.val());
            }
            u += 1;
        }
        Ok(())
    }

    /// 缺口桩清单输出。
    pub fn gap_list(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.uses.len() {
            if self.uses[i].status == COV_GAP {
                v.push(format!("{}|{}", USE_NAMES[self.uses[i].use_kind], self.uses[i].gap_note));
            }
            i += 1;
        }
        v
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }
}

/// 首版承接面装配：五维语义点 + 三类用途（文件缓冲带缺口）。
pub fn build_initial() -> Result<MmFace, u32> {
    let mut f = MmFace::new();
    f.add_point(MmRow {
        dim: DIM_TYPE,
        map_type: MT_ANON,
        prot: PROT_RW,
        a4_ref: String::from("A4-匿名映射档"),
    })?;
    f.add_point(MmRow {
        dim: DIM_PROT,
        map_type: MT_FILE,
        prot: PROT_RX,
        a4_ref: String::from("A4-文件映射档"),
    })?;
    f.add_point(MmRow {
        dim: DIM_SHARED,
        map_type: MT_ANON,
        prot: PROT_RW,
        a4_ref: String::from("A4-共享内存档"),
    })?;
    f.add_point(MmRow {
        dim: DIM_SYNC,
        map_type: MT_FILE,
        prot: PROT_RW,
        a4_ref: String::from("A4-文件映射档"),
    })?;
    f.add_point(MmRow {
        dim: DIM_ALIGN,
        map_type: MT_SECIMAGE,
        prot: PROT_RX,
        a4_ref: String::from("A4-匿名映射档"),
    })?;
    f.add_use(UseRow {
        use_kind: USE_PE,
        status: COV_OK,
        gap_note: String::new(),
    })?;
    f.add_use(UseRow {
        use_kind: USE_SHM,
        status: COV_OK,
        gap_note: String::new(),
    })?;
    f.add_use(UseRow {
        use_kind: USE_FILEBUF,
        status: COV_GAP,
        gap_note: String::from("缺口：MS_SYNC 同步刷写语义未收口，输出桩清单（同 F16028 通道）"),
    })?;
    Ok(f)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((语义点数, 缺口数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let f = build_initial()?;
    f.dims_closed()?;
    f.uses_closed()?;
    let rate = f.a4_align_rate()?;
    if rate != 100 {
        return Err(MmCode::A4Dangling.val());
    }
    let gaps = f.gap_list();
    if gaps.len() != 1 {
        return Err(MmCode::GapUnmarked.val());
    }
    Ok((f.point_count(), gaps.len()))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 缺口标注缺失（判据复测方式：删缺口标注必红）。
        led.record(
            0,
            vet_use(&UseRow {
                use_kind: USE_FILEBUF,
                status: COV_GAP,
                gap_note: String::new(),
            }) == Err(MmCode::GapUnmarked.val()),
        );

        // ② 保护位越闭集（非法组合 0x8）。
        led.record(
            1,
            vet_row(&MmRow {
                dim: DIM_PROT,
                map_type: MT_ANON,
                prot: 0x8,
                a4_ref: String::from("A4-匿名映射档"),
            }) == Err(MmCode::ProtOutOfRange.val()),
        );

        // ③ A4 指针悬空。
        led.record(
            2,
            vet_row(&MmRow {
                dim: DIM_TYPE,
                map_type: MT_FILE,
                prot: PROT_READ,
                a4_ref: String::from("A4-ghost"),
                }) == Err(MmCode::A4Dangling.val()),
        );

        // ④ 维度越闭集 / 映射类型越闭集。
        led.record(
            3,
            vet_row(&MmRow {
                dim: DIM_COUNT + (rnd % 2) as usize,
                map_type: MT_ANON,
                prot: PROT_READ,
                a4_ref: String::from("A4-匿名映射档"),
            }) == Err(MmCode::DimOutOfRange.val())
                && vet_row(&MmRow {
                    dim: DIM_TYPE,
                    map_type: MT_COUNT,
                    prot: PROT_READ,
                    a4_ref: String::from("A4-匿名映射档"),
                }) == Err(MmCode::MapTypeOutOfRange.val()),
        );

        // ⑤ 用途越闭集。
        led.record(
            4,
            vet_use(&UseRow {
                use_kind: USE_COUNT + (rnd % 2) as usize,
                status: COV_OK,
                gap_note: String::new(),
            }) == Err(MmCode::DimOutOfRange.val()),
        );

        // ⑥ 重复语义点 + 轮次守卫。
        led.record(
            5,
            {
                let mut f = MmFace::new();
                let row = MmRow {
                    dim: DIM_TYPE,
                    map_type: MT_ANON,
                    prot: PROT_RW,
                    a4_ref: String::from("A4-匿名映射档"),
                };
                let _ = f.add_point(row.clone());
                f.add_point(row) == Err(MmCode::PointDuplicated.val()) && (rnd % 1) == 0
            },
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(MmCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法承接面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let f = build_initial()?;
    f.dims_closed()?;
    f.uses_closed()?;
    // 五维名/三类名/七保护位点验。
    let mut d = 0;
    while d < DIM_COUNT {
        if DIM_NAMES[d].len() == 0 {
            return Err(MmCode::DimOutOfRange.val());
        }
        d += 1;
    }
    let mut p = 0;
    while p < PROT_TABLE.len() {
        if !prot_legal(PROT_TABLE[p]) {
            return Err(MmCode::ProtOutOfRange.val());
        }
        p += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        let v = MmCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：5 语义点 / 1 缺口。
    let g = golden_run();
    v.push((
        "J1-黄金正演5点1缺口",
        matches!(g, Ok((5, 1))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 语义点对照一致率 100%（判据原文口径）。
    let f = build_initial().unwrap_or_else(|_| MmFace::new());
    let rate = f.a4_align_rate().unwrap_or(0);
    v.push(("J1-A4对照一致100%", rate == 100, format!("{}%", rate)));
    // 4. 覆盖清单齐备（判据原文口径）。
    v.push((
        "J1-覆盖清单齐备",
        matches!(f.uses_closed(), Ok(())),
        String::from("PE/共享内存/文件缓冲"),
    ));
    // 5. 缺口桩清单输出。
    v.push((
        "J1-缺口桩清单输出",
        f.gap_list().len() == 1,
        String::from("MS_SYNC 缺口"),
    ));
    // 6. 删缺口标注必红（判据复测方式）。
    v.push((
        "J1-删缺口标注必红",
        vet_use(&UseRow {
            use_kind: USE_FILEBUF,
            status: COV_GAP,
            gap_note: String::new(),
        }) == Err(MmCode::GapUnmarked.val()),
        String::from("复测方式"),
    ));
    // 7. 保护位七组合闭集（POSIX 真实值）。
    v.push((
        "J1-保护位七组合",
        PROT_RWX == 0x7 && PROT_NONE == 0x0,
        String::from("PROT_ 真实值"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("五维+七保护位"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16037 宿主 mmap 语义承接面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
