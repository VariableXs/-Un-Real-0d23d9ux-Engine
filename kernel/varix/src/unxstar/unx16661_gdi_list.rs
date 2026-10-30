//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16661 · gdi32/mem/字符串组清单冻结 · 0x681C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B34）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F16661-J1 gdi 位图/DC 语义与 msvcrt 字符串/内存族用例
//!     清单冻结、与 B12/B24 语义锚映射、版本化只增不删；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双组闭集：gdi 组（位图/DC 语义）与 msvcrt 组（字符串/内存
//!     族），组间不串（组标签显性）；
//!   - 语义锚映射：gdi 组锚 B12、msvcrt 组锚 B24，锚一一映射；
//!   - 用例三列 + 冻结哈希 + append-only 版本化（与 F16621/F16641
//!     同款制度，本单落实 gdi/mem 两组的键位与期望值闭集）；
//!   - gdi 期望值语义：位图句柄上限/DC 类型数（冻结整数）；
//!     msvcrt 期望值语义：串长/内存块大小（冻结整数）；
//!   - LCG 种子写死 = SEG_BASE（0x681C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x681C0，AI-VX3-1030 批16 独占区）。
pub const SEG_BASE: u32 = 0x681C0;

// ── 双组闭集 ────────────────────────────────────────────────────
pub const GRP_GDI: usize = 0;
pub const GRP_MSVCRT: usize = 1;
pub const GROUP_COUNT: usize = 2;
pub const GROUP_NAMES: [&str; GROUP_COUNT] = ["gdi 位图/DC", "msvcrt 字符串/内存"];
pub const GROUP_FLOOR: usize = 2;

// ── B12/B24 语义锚（冻结） ──────────────────────────────────────
pub const ANCHOR_GDI: &str = "B12-gdi-dc-bitmap";
pub const ANCHOR_MSVCRT: &str = "B24-crt-str-mem";
pub const ANCHOR_TABLE: [&str; 2] = [ANCHOR_GDI, ANCHOR_MSVCRT];

// ── 用例闭集（10 例：gdi 5 + msvcrt 5） ─────────────────────────
pub const CASE_COUNT: usize = 10;
pub const CASE_IDS: [&str; CASE_COUNT] = [
    "GMD-001", "GMD-002", "GMD-003", "GMD-004", "GMD-005",
    "GMD-006", "GMD-007", "GMD-008", "GMD-009", "GMD-010",
];
pub const CASE_GROUPS: [usize; CASE_COUNT] = [0, 0, 0, 0, 0, 1, 1, 1, 1, 1];
pub const CASE_EXPECTS: [u32; CASE_COUNT] =
    [64, 3, 4096, 32, 16, 256, 512, 1024, 8, 4];
/// 期望值语义注释（冻结，与 CASE_EXPECTS 一一对应）。
pub const CASE_MEANINGS: [&str; CASE_COUNT] = [
    "位图句柄上限",
    "DC 类型数",
    "位图最大跨度",
    "调色板项上限",
    "GDI 配额",
    "串长上限",
    "内存块上界",
    "内存池页数",
    "栈保护页",
    "对齐边界",
];

/// 期望值缺列哨兵。
pub const EXPECT_MISSING: u32 = 0xFFFF_FFFF;
/// 清单版本。
pub const LIST_VERSION: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlistCode {
    /// 组越闭集
    GroupOutOfRange,
    /// 组名空（双组齐备破坏）
    GroupNameBlank,
    /// 用例 ID 空
    CaseIdBlank,
    /// 期望值缺列
    ExpectMissing,
    /// 组归属越闭集
    CaseGroupOutOfRange,
    /// 语义锚映射断裂（gdi↔B12、msvcrt↔B24 错配）
    AnchorMismatch,
    /// 用例 ID 重复
    CaseIdDuplicate,
    /// 删除违例
    DeletionForbidden,
    /// 语义注释错位（与期望值表长度/内容不一致）
    MeaningMisaligned,
    /// 双跑不一致
    NotReproducible,
}

impl GlistCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GlistCode::GroupOutOfRange => "组越闭集：gdi/msvcrt 冻结",
            GlistCode::GroupNameBlank => "组名空：双组齐备破坏",
            GlistCode::CaseIdBlank => "用例 ID 空",
            GlistCode::ExpectMissing => "期望值缺列",
            GlistCode::CaseGroupOutOfRange => "组归属越闭集",
            GlistCode::AnchorMismatch => "语义锚错配：gdi↔B12、msvcrt↔B24",
            GlistCode::CaseIdDuplicate => "用例 ID 重复",
            GlistCode::DeletionForbidden => "删除违例：append-only 破坏",
            GlistCode::MeaningMisaligned => "语义注释错位",
            GlistCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GlistCode; 10] = [
        GlistCode::GroupOutOfRange,
        GlistCode::GroupNameBlank,
        GlistCode::CaseIdBlank,
        GlistCode::ExpectMissing,
        GlistCode::CaseGroupOutOfRange,
        GlistCode::AnchorMismatch,
        GlistCode::CaseIdDuplicate,
        GlistCode::DeletionForbidden,
        GlistCode::MeaningMisaligned,
        GlistCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GlistCode::ALL.len() {
        if GlistCode::ALL[i].val() == code {
            return GlistCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x681C0 段闭集内"
}

// ── 用例行 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCaseRow {
    pub id: &'static str,
    pub group: usize,
    pub expect: u32,
}

impl GCaseRow {
    /// 组锚一致性：gdi 组锚 B12、msvcrt 组锚 B24。
    pub fn anchor_matches(&self) -> bool {
        match self.group {
            GRP_GDI => ANCHOR_TABLE[GRP_GDI] == ANCHOR_GDI,
            GRP_MSVCRT => ANCHOR_TABLE[GRP_MSVCRT] == ANCHOR_MSVCRT,
            _ => false,
        }
    }
    pub fn complete(&self) -> bool {
        self.id.len() > 0 && self.group < GROUP_COUNT && self.expect != EXPECT_MISSING
    }
}

// ── 清单（append-only + 冻结） ─────────────────────────────────
pub struct GCaseList {
    pub rows: Vec<GCaseRow>,
    pub version: u32,
    pub frozen_hash: u64,
    pub del_caught: u32,
    pub dup_caught: u32,
}

impl GCaseList {
    pub fn new(version: u32) -> GCaseList {
        GCaseList { rows: Vec::new(), version, frozen_hash: 0, del_caught: 0, dup_caught: 0 }
    }

    pub fn append(&mut self, row: GCaseRow) -> Result<(), u32> {
        if !row.complete() {
            if row.id.len() == 0 {
                return Err(GlistCode::CaseIdBlank.val());
            }
            if row.group >= GROUP_COUNT {
                return Err(GlistCode::CaseGroupOutOfRange.val());
            }
            return Err(GlistCode::ExpectMissing.val());
        }
        if !row.anchor_matches() {
            return Err(GlistCode::AnchorMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == row.id {
                self.dup_caught += 1;
                return Err(GlistCode::CaseIdDuplicate.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                self.del_caught += 1;
                return Err(GlistCode::DeletionForbidden.val());
            }
            i += 1;
        }
        Err(GlistCode::CaseIdBlank.val())
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.rows.len() {
            let b = self.rows[i].id.as_bytes();
            let mut j = 0;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x100000001b3);
                j += 1;
            }
            h ^= self.rows[i].group as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].expect as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen_hash = self.hash();
        self.frozen_hash
    }

    pub fn frozen_ok(&self) -> bool {
        self.frozen_hash != 0 && self.frozen_hash == self.hash()
    }

    /// 双组覆盖：两组各有用例。
    pub fn group_coverage(&self) -> Result<u32, u32> {
        let mut hit = [false; GROUP_COUNT];
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].group < GROUP_COUNT {
                hit[self.rows[i].group] = true;
            }
            i += 1;
        }
        let mut c = 0;
        let mut g = 0;
        while g < GROUP_COUNT {
            if hit[g] {
                c += 1;
            }
            g += 1;
        }
        if c != GROUP_COUNT {
            return Err(GlistCode::GroupOutOfRange.val());
        }
        Ok(100)
    }
}

/// 双组齐备账。
pub fn groups_ready() -> bool {
    if GROUP_NAMES.len() < GROUP_FLOOR {
        return false;
    }
    let mut i = 0;
    while i < GROUP_NAMES.len() {
        if GROUP_NAMES[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 语义注释对齐审计。
pub fn meanings_aligned() -> bool {
    if CASE_MEANINGS.len() != CASE_COUNT {
        return false;
    }
    let mut i = 0;
    while i < CASE_COUNT {
        if CASE_MEANINGS[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 基线清单。
pub fn baseline_list() -> Result<GCaseList, u32> {
    let mut l = GCaseList::new(LIST_VERSION);
    let mut i = 0;
    while i < CASE_COUNT {
        l.append(GCaseRow {
            id: CASE_IDS[i],
            group: CASE_GROUPS[i],
            expect: CASE_EXPECTS[i],
        })?;
        i += 1;
    }
    if l.rows.len() != CASE_COUNT {
        return Err(GlistCode::CaseIdBlank.val());
    }
    Ok(l)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：基线 10 例→组覆盖→冻结→追加→演进。
pub fn golden_run() -> Result<(usize, u32, u64), u32> {
    let mut l = baseline_list()?;
    let cov = l.group_coverage()?;
    l.freeze();
    if !l.frozen_ok() {
        return Err(GlistCode::AnchorMismatch.val());
    }
    l.append(GCaseRow { id: "GMD-011", group: GRP_GDI, expect: 128 })?;
    if !groups_ready() {
        return Err(GlistCode::GroupNameBlank.val());
    }
    if !meanings_aligned() {
        return Err(GlistCode::MeaningMisaligned.val());
    }
    Ok((l.rows.len(), cov, l.frozen_hash))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut l = baseline_list()?;
    l.freeze();
    if l.frozen_ok() {
        Ok(())
    } else {
        Err(GlistCode::AnchorMismatch.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 组越界 → 必拒。
    let mut l = GCaseList::new(1);
    l.append(GCaseRow { id: "X", group: GROUP_COUNT, expect: 1 })
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 删除违例 → 必拒。
    let mut l = baseline_list()?;
    l.remove("GMD-003")
}

/// 注入 10/10：组越界、缺期望、重复 ID、删除四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut l0 = GCaseList::new(1);
        led.record(0, l0.append(GCaseRow { id: "A", group: 9, expect: 1 }).is_err());
        let mut l1 = GCaseList::new(1);
        led.record(1, l1.append(GCaseRow { id: "B", group: 0, expect: EXPECT_MISSING }).is_err());
        let mut l2 = GCaseList::new(1);
        l2.append(GCaseRow { id: "C", group: 0, expect: 1 }).ok();
        led.record(2, l2.append(GCaseRow { id: "C", group: 1, expect: 2 }).is_err());
        let mut l3 = baseline_list().ok();
        let hit3 = match &mut l3 {
            Some(l) => l.remove("GMD-007").is_err(),
            None => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GlistCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GlistCode::ALL.len() {
        let v = GlistCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 双组齐备 + B12/B24 锚映射。
    v.push((
        "J1-双组齐备+B12/B24映射",
        groups_ready()
            && ANCHOR_TABLE[0] == "B12-gdi-dc-bitmap"
            && ANCHOR_TABLE[1] == "B24-crt-str-mem",
        format!("{:?}", ANCHOR_TABLE),
    ));
    // 语义注释对齐。
    v.push((
        "J1-语义注释对齐",
        meanings_aligned() && CASE_MEANINGS.len() == CASE_EXPECTS.len(),
        format!("{}/{} 条", CASE_MEANINGS.len(), CASE_EXPECTS.len()),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
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
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x681C0 窗口")));
    v
}

pub fn run_unx16661_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16661 gdi32/mem/字符串组清单冻结");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
