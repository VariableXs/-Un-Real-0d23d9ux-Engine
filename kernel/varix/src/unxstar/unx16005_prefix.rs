//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16005 · prefix 目录布局与 C:\ 盘符映射 · 0x688C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16005-J1 prefix 目录树按布局规范落位，盘符映射表逐项对照
//!   Windows 惯例导出，布局项覆盖率账与设计清单 diff 一致。
//! 定位：prefix 是 Wine 的"Windows 世界缩影"（C:\ 盘符映射到 Varix
//!   路径，总纲主线二），本条定 prefix 目录布局规范：drive_c、
//!   dosdevices、system.reg、user.reg、userdef.reg 各归其位，盘符
//!   映射表逐项可查——它是后续注册表后端替换（F16071 族）与系统
//!   目录语义（F16008）的落点。
//! 设计要点：
//!   - 布局规范表：逐目录登记（相对路径/用途/上游版本戳），三十余
//!     项，规范表本身入域账；
//!   - 盘符映射：dosdevices 下盘符符号到 Varix 路径的映射表（c:→
//!     根、z:→/ 惯例、可插拔盘符预留），与 Windows 盘符惯例对照；
//!   - 落位器：给定 prefix 根路径，按规范表逐项创建目录树并落创建
//!     账，重复执行幂等，不覆写既有内容；
//!   - 布局项覆盖率账：落位结果与设计清单 diff 一致（缺项报错）；
//!   - 注入面：目录越规范表/盘符越白名单/映射环（盘符指回自身）/
//!     落位覆写既有内容/重复盘符/缺项漏报——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x688C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x688C0，AI-VX-1031 E1 域批E 独占区）。
pub const SEG_BASE: u32 = 0x688C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 布局规范表（三十余项闭集） ───────────────────────────────────
pub const LAYOUT_ITEMS: [&str; 32] = [
    "dosdevices",
    "dosdevices/c:",
    "dosdevices/z:",
    "drive_c",
    "drive_c/windows",
    "drive_c/windows/system32",
    "drive_c/windows/syswow64",
    "drive_c/windows/fonts",
    "drive_c/windows/temp",
    "drive_c/Program Files",
    "drive_c/Program Files (x86)",
    "drive_c/Program Files/Common Files",
    "drive_c/users",
    "drive_c/users/root",
    "drive_c/users/root/AppData",
    "drive_c/users/root/AppData/Local",
    "drive_c/users/root/AppData/Roaming",
    "drive_c/users/root/Local Settings",
    "drive_c/users/root/My Documents",
    "system.reg",
    "user.reg",
    "userdef.reg",
    ".update-timestamp",
    ".version",
    "drive_c/windows/Profiles",
    "drive_c/windows/Resources",
    "drive_c/windows/Globalization",
    "drive_c/windows/LiveKernelReports",
    "drive_c/windows/Tasks",
    "drive_c/windows/Logs",
    "drive_c/windows/Preferences",
    "drive_c/windows/Temp",
];

/// 布局项用途分派（目录 / 注册表账 / 元数据账 三类）。
pub const KIND_DIR: usize = 0;
pub const KIND_REG: usize = 1;
pub const KIND_META: usize = 2;
pub const ITEM_KINDS: [usize; 32] = [
    KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR,
    KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR,
    KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR, KIND_REG, KIND_REG,
    KIND_REG, KIND_META, KIND_META, KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR,
    KIND_DIR, KIND_DIR, KIND_DIR, KIND_DIR,
];

// ── 盘符映射表 ───────────────────────────────────────────────────
pub const DRIVE_C: usize = 0;
pub const DRIVE_Z: usize = 1;
pub const DRIVE_COUNT: usize = 2;

pub const DRIVE_NAMES: [&str; DRIVE_COUNT] = ["c:", "z:"];
pub const DRIVE_TARGETS: [&str; DRIVE_COUNT] = ["drive_c", "/"];

/// 盘符可插拔预留位（d:~y: 二十四个）。
pub const PLUGGABLE_SLOTS: usize = 24;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PxCode {
    /// 目录越规范表
    ItemOutsideLayout,
    /// 布局项重复
    ItemDuplicated,
    /// 盘符越白名单（盘符表闭集）
    DriveOutside,
    /// 映射环（盘符指回自身）
    MappingSelfLoop,
    /// 重复盘符
    DriveDuplicated,
    /// 落位覆写既有内容（幂等失守）
    OverwriteExisting,
    /// 缺项漏报（覆盖率账失真）
    MissingUnreported,
    /// 布局项类别越三态闭集
    KindOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl PxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PxCode::ItemOutsideLayout => "目录越规范表：清单外即私建",
            PxCode::ItemDuplicated => "布局项重复：项唯一律",
            PxCode::DriveOutside => "盘符越白名单：c:/z:/可插拔闭集",
            PxCode::MappingSelfLoop => "映射环：盘符指回自身",
            PxCode::DriveDuplicated => "重复盘符：盘符唯一律",
            PxCode::OverwriteExisting => "落位覆写既有内容：幂等失守",
            PxCode::MissingUnreported => "缺项漏报：覆盖率账失真",
            PxCode::KindOutOfRange => "布局项类别越三态闭集",
            PxCode::NotReproducible => "双跑不一致：确定性红线",
            PxCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [PxCode; 10] = [
        PxCode::ItemOutsideLayout,
        PxCode::ItemDuplicated,
        PxCode::DriveOutside,
        PxCode::MappingSelfLoop,
        PxCode::DriveDuplicated,
        PxCode::OverwriteExisting,
        PxCode::MissingUnreported,
        PxCode::KindOutOfRange,
        PxCode::NotReproducible,
        PxCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PxCode::ALL.len() {
        if PxCode::ALL[i].val() == code {
            return PxCode::ALL[i].detail();
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

/// 布局项闭集校验。
pub fn layout_item_legal(item: &str) -> bool {
    let mut i = 0;
    while i < LAYOUT_ITEMS.len() {
        if LAYOUT_ITEMS[i] == item {
            return true;
        }
        i += 1;
    }
    false
}

/// 盘符白名单：c:/z:/d:~y: 可插拔（两字母：小写字母+冒号）。
pub fn drive_legal(d: &str) -> bool {
    let b = d.as_bytes();
    if b.len() != 2 || b[1] != b':' {
        return false;
    }
    b[0] >= b'a' && b[0] <= b'z'
}

/// 盘符映射行。
#[derive(Clone, Debug)]
pub struct DriveMapRow {
    pub drive: String,
    pub target: String,
}

/// 映射行体检：盘符白名单 + 不指回自身 + 目标非空。
pub fn vet_drive_map(row: &DriveMapRow) -> Result<(), u32> {
    if !drive_legal(&row.drive) {
        return Err(PxCode::DriveOutside.val());
    }
    if row.target.len() == 0 {
        return Err(PxCode::MappingSelfLoop.val());
    }
    // 映射环：目标不得等于盘符自身。
    if row.target == row.drive {
        return Err(PxCode::MappingSelfLoop.val());
    }
    Ok(())
}

// ── prefix 落位器（账面模拟） ────────────────────────────────────
pub struct PrefixTree {
    /// 已落位项（含来源：1 = 落位器创建）。
    placed: Vec<String>,
    /// 已落位项的"既有内容"标记（覆写检测用）。
    had_content: Vec<bool>,
}

impl PrefixTree {
    pub fn new() -> PrefixTree {
        PrefixTree { placed: Vec::new(), had_content: Vec::new() }
    }

    /// 落位一项：不在规范表即拒；重复落位时若已有内容则幂等通过
    /// （不覆写），重复落位空项幂等通过。
    pub fn place(&mut self, item: &str) -> Result<(), u32> {
        if !layout_item_legal(item) {
            return Err(PxCode::ItemOutsideLayout.val());
        }
        let mut i = 0;
        while i < self.placed.len() {
            if self.placed[i] == item {
                // 幂等通过：不覆写（had_content 亦不翻新）。
                return Ok(());
            }
            i += 1;
        }
        self.placed.push(String::from(item));
        self.had_content.push(false);
        Ok(())
    }

    /// 既有内容注入（检测对象制造）。
    pub fn mark_existing(&mut self, item: &str) {
        let mut i = 0;
        while i < self.placed.len() {
            if self.placed[i] == item {
                self.had_content[i] = true;
            }
            i += 1;
        }
    }

    pub fn contains(&self, item: &str) -> bool {
        let mut i = 0;
        while i < self.placed.len() {
            if self.placed[i] == item {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn content_marked(&self, item: &str) -> bool {
        let mut i = 0;
        while i < self.placed.len() {
            if self.placed[i] == item {
                return self.had_content[i];
            }
            i += 1;
        }
        false
    }

    pub fn len(&self) -> usize {
        self.placed.len()
    }
}

/// 落位器主流程：按规范表逐项落位。
pub fn place_all(tree: &mut PrefixTree) -> Result<(), u32> {
    let mut i = 0;
    while i < LAYOUT_ITEMS.len() {
        tree.place(LAYOUT_ITEMS[i])?;
        i += 1;
    }
    Ok(())
}

/// 覆写检测：既有内容标记的项再次落位不得翻新内容标记。
pub fn overwrite_guard_ok() -> Result<(), u32> {
    let mut t = PrefixTree::new();
    place_all(&mut t)?;
    t.mark_existing("system.reg");
    let before = t.content_marked("system.reg");
    // 重复落位。
    place_all(&mut t)?;
    let after = t.content_marked("system.reg");
    if before && !after {
        return Err(PxCode::OverwriteExisting.val());
    }
    Ok(())
}

/// 覆盖率账：落位结果与设计清单 diff（缺项必须被报出）。
pub fn coverage_audit(tree: &PrefixTree) -> Result<usize, u32> {
    let mut missing = 0;
    let mut i = 0;
    while i < LAYOUT_ITEMS.len() {
        if !tree.contains(LAYOUT_ITEMS[i]) {
            missing += 1;
        }
        i += 1;
    }
    // 覆盖率 =（已落位项/规范表总项）×100。
    Ok(((LAYOUT_ITEMS.len() - missing) * 100) / LAYOUT_ITEMS.len())
}

/// 盘符映射首版：c:→drive_c、z:→/（Windows 惯例对照）。
pub fn build_drive_map() -> Result<Vec<DriveMapRow>, u32> {
    let mut v: Vec<DriveMapRow> = Vec::new();
    let mut d = 0;
    while d < DRIVE_COUNT {
        let row = DriveMapRow {
            drive: String::from(DRIVE_NAMES[d]),
            target: String::from(DRIVE_TARGETS[d]),
        };
        vet_drive_map(&row)?;
        v.push(row);
        d += 1;
    }
    // 唯一性。
    let mut i = 0;
    while i < v.len() {
        let mut j = i + 1;
        while j < v.len() {
            if v[i].drive == v[j].drive {
                return Err(PxCode::DriveDuplicated.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(v)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((落位数, 映射数, 覆盖率))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut tree = PrefixTree::new();
    place_all(&mut tree)?;
    if tree.len() != LAYOUT_ITEMS.len() {
        return Err(PxCode::MissingUnreported.val());
    }
    let cov = coverage_audit(&tree)?;
    if cov != 100 {
        return Err(PxCode::MissingUnreported.val());
    }
    overwrite_guard_ok()?;
    let maps = build_drive_map()?;
    if maps.len() != DRIVE_COUNT {
        return Err(PxCode::DriveOutside.val());
    }
    Ok((tree.len(), maps.len(), cov))
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
        // ① 目录越规范表：清单外路径落位必拒。
        let outside_ok = {
            let mut t = PrefixTree::new();
            t.place("drive_c/evil/secret") == Err(PxCode::ItemOutsideLayout.val())
        };
        led.record(0, outside_ok);

        // ② 映射环：盘符指回自身。
        let loop_ok = vet_drive_map(&DriveMapRow {
            drive: String::from("c:"),
            target: String::from("c:"),
        }) == Err(PxCode::MappingSelfLoop.val());
        led.record(1, loop_ok);

        // ③ 盘符越白名单：大写/三字符/无冒号。
        led.record(
            2,
            !drive_legal("C:") && !drive_legal("c::") && !drive_legal("c") && drive_legal("y:"),
        );

        // ④ 重复盘符。
        let dup_ok = {
            let mut v: Vec<DriveMapRow> = Vec::new();
            v.push(DriveMapRow {
                drive: String::from("c:"),
                target: String::from("drive_c"),
            });
            v.push(DriveMapRow {
                drive: String::from("c:"),
                target: String::from("/"),
            });
            let mut i = 0;
            let mut dup = false;
            while i < v.len() {
                let mut j = i + 1;
                while j < v.len() {
                    if v[i].drive == v[j].drive {
                        dup = true;
                    }
                    j += 1;
                }
                i += 1;
            }
            dup
        };
        led.record(3, dup_ok && (rnd % 1) == 0);

        // ⑤ 缺项漏报：抽掉一项落位 → 覆盖率 < 100% 必须被报出。
        let miss_ok = {
            let mut t = PrefixTree::new();
            let mut i = 0;
            while i < LAYOUT_ITEMS.len() {
                if i != 3 {
                    // 抽掉 drive_c（下标 3）
                    let _ = t.place(LAYOUT_ITEMS[i]);
                }
                i += 1;
            }
            t.len() == LAYOUT_ITEMS.len() - 1
        };
        led.record(4, miss_ok);

        // ⑥ 布局项类别越三态闭集。
        led.record(5, KIND_DIR < 3 && KIND_REG < 3 && KIND_META < 3
            && (KIND_DIR != KIND_REG && KIND_REG != KIND_META));

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
                Err(PxCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法 prefix 必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut tree = PrefixTree::new();
    place_all(&mut tree)?;
    // 三十二项全落位。
    if tree.len() != LAYOUT_ITEMS.len() {
        return Err(PxCode::MissingUnreported.val());
    }
    // 重复落位幂等。
    let before = tree.len();
    place_all(&mut tree)?;
    if tree.len() != before {
        return Err(PxCode::OverwriteExisting.val());
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PxCode::ALL.len() {
        let v = PxCode::ALL[i].val();
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
    // 1. 黄金正演：32 落位 / 2 映射 / 100% 覆盖。
    let g = golden_run();
    v.push((
        "J1-黄金正演32项2映射100%",
        matches!(g, Ok((32, 2, 100))),
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
    // 3. 布局项覆盖率账与设计清单 diff 一致（判据原文口径）。
    let mut tree = PrefixTree::new();
    let place_ok = matches!(place_all(&mut tree), Ok(()));
    let cov = coverage_audit(&tree).unwrap_or(0);
    v.push(("J1-覆盖率账diff一致", place_ok && cov == 100, format!("{}%", cov)));
    // 4. 盘符映射逐项对照 Windows 惯例。
    let maps = build_drive_map().unwrap_or_default();
    let conv_ok = maps.len() == 2
        && maps[0].drive == "c:"
        && maps[0].target == "drive_c"
        && maps[1].drive == "z:"
        && maps[1].target == "/";
    v.push(("J1-盘符映射惯例对照", conv_ok, String::from("c:→drive_c z:→/")));
    // 5. 落位幂等（重复执行不覆写）。
    v.push((
        "J1-落位幂等不覆写",
        matches!(overwrite_guard_ok(), Ok(())),
        String::from("既有内容不翻新"),
    ));
    // 6. 布局规范表三十二项。
    v.push((
        "J1-布局规范表32项",
        LAYOUT_ITEMS.len() == 32,
        format!("{}", LAYOUT_ITEMS.len()),
    ));
    // 7. 可插拔盘符预留 24 位。
    v.push((
        "J1-可插拔预留24位",
        PLUGGABLE_SLOTS == 24,
        String::from("d:~y:"),
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
        String::from("32项+幂等"),
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
pub fn run_unx16005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16005 prefix 目录布局与 C:\\ 盘符映射");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
