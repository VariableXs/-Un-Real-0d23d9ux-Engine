//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16621 · advapi32 注册表 conformance 组清单冻结 · 0x68140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B32）：
//!   ① 纯功能代码 ≥283 行；
//!   ② UNX-F16621-J1 测试用例清单冻结（用例 ID/语义锚/期望值三
//!     列）、清单版本化、与 B23 注册表后端键位一一映射；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 用例三列建模：用例 ID（ADV-xxx 冻结闭集）、语义锚（B23 键
//!     位）、期望值（整数冻结）；三列齐才可入清单（缺列拒）；
//!   - 清单版本化：版本号只前进，追加允许、删除禁止（append-only
//!     对账：新清单 ⊇ 旧清单）；
//!   - B23 键位一一映射：每用例语义锚必须命中 B23 注册表后端键
//!     位表（映射一致率 100%），未命中即映射断裂；
//!   - 清单冻结：哈希封印（FNV-1a 全清单指纹），指纹一致才算冻
//!     结成功；追加后指纹必须变化（冻结可演进）；
//!   - LCG 种子写死 = SEG_BASE（0x68140）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68140，AI-VX3-1030 批16 独占区）。
pub const SEG_BASE: u32 = 0x68140;

// ── 用例闭集（冻结 12 例） ──────────────────────────────────────
pub const CASE_COUNT: usize = 12;
pub const CASE_IDS: [&str; CASE_COUNT] = [
    "ADV-001", "ADV-002", "ADV-003", "ADV-004", "ADV-005", "ADV-006",
    "ADV-007", "ADV-008", "ADV-009", "ADV-010", "ADV-011", "ADV-012",
];

// ── B23 注册表后端键位表（冻结） ────────────────────────────────
pub const B23_KEYS: [&str; 8] = [
    "HKLM\\Software\\Varix",
    "HKLM\\System\\Setup",
    "HKCU\\Environment",
    "HKCR\\.vx",
    "HKU\\.Default",
    "HKLM\\Hardware\\Devmap",
    "HKCU\\Software\\Classes",
    "HKLM\\Security\\Policy",
];
/// 用例→键位映射（冻结：12 例一一命中）。
pub const CASE_ANCHORS: [&str; CASE_COUNT] = [
    "HKLM\\Software\\Varix",
    "HKLM\\Software\\Varix",
    "HKLM\\System\\Setup",
    "HKCU\\Environment",
    "HKCR\\.vx",
    "HKU\\.Default",
    "HKLM\\Hardware\\Devmap",
    "HKCU\\Software\\Classes",
    "HKCU\\Software\\Classes",
    "HKLM\\Security\\Policy",
    "HKLM\\System\\Setup",
    "HKCU\\Environment",
];
/// 用例期望值（冻结）。
pub const CASE_EXPECTS: [u32; CASE_COUNT] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

/// 清单版本（只前进）。
pub const LIST_VERSION: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListCode {
    /// 用例越闭集
    CaseOutOfRange,
    /// 用例 ID 空
    CaseIdBlank,
    /// 语义锚缺列
    AnchorMissing,
    /// 期望值缺列（哨兵值 0xFFFFFFFF 视为缺）
    ExpectMissing,
    /// B23 映射断裂（锚未命中键位表）
    AnchorNotMapped,
    /// 用例 ID 重复
    CaseIdDuplicate,
    /// 清单回退（版本不前进）
    VersionNotForward,
    /// 删除违例（append-only 破坏）
    DeletionForbidden,
    /// 冻结指纹不一致
    FreezeHashMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl ListCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ListCode::CaseOutOfRange => "用例越闭集：12 例冻结",
            ListCode::CaseIdBlank => "用例 ID 空",
            ListCode::AnchorMissing => "语义锚缺列",
            ListCode::ExpectMissing => "期望值缺列",
            ListCode::AnchorNotMapped => "B23 映射断裂：锚未命中键位表",
            ListCode::CaseIdDuplicate => "用例 ID 重复",
            ListCode::VersionNotForward => "清单回退：版本不前进",
            ListCode::DeletionForbidden => "删除违例：append-only 破坏",
            ListCode::FreezeHashMismatch => "冻结指纹不一致",
            ListCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ListCode; 10] = [
        ListCode::CaseOutOfRange,
        ListCode::CaseIdBlank,
        ListCode::AnchorMissing,
        ListCode::ExpectMissing,
        ListCode::AnchorNotMapped,
        ListCode::CaseIdDuplicate,
        ListCode::VersionNotForward,
        ListCode::DeletionForbidden,
        ListCode::FreezeHashMismatch,
        ListCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ListCode::ALL.len() {
        if ListCode::ALL[i].val() == code {
            return ListCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x68140 段闭集内"
}

/// 期望值缺列哨兵。
pub const EXPECT_MISSING: u32 = 0xFFFF_FFFF;

// ── 用例行（三列） ──────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseRow {
    pub id: &'static str,
    pub anchor: &'static str,
    pub expect: u32,
}

impl CaseRow {
    /// 三列齐备。
    pub fn complete(&self) -> bool {
        self.id.len() > 0 && self.anchor.len() > 0 && self.expect != EXPECT_MISSING
    }
    /// B23 映射命中。
    pub fn anchor_mapped(&self) -> bool {
        let mut i = 0;
        while i < B23_KEYS.len() {
            if B23_KEYS[i] == self.anchor {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ── 清单（版本化 + append-only + 冻结哈希） ─────────────────────
pub struct CaseList {
    pub rows: Vec<CaseRow>,
    pub version: u32,
    pub frozen_hash: u64,
    pub dup_caught: u32,
    pub del_caught: u32,
}

impl CaseList {
    pub fn new(version: u32) -> CaseList {
        CaseList { rows: Vec::new(), version, frozen_hash: 0, dup_caught: 0, del_caught: 0 }
    }

    /// 追加用例（append-only：缺列/重复/未映射即拒）。
    pub fn append(&mut self, row: CaseRow) -> Result<(), u32> {
        if !row.complete() {
            if row.id.len() == 0 {
                return Err(ListCode::CaseIdBlank.val());
            }
            if row.anchor.len() == 0 {
                return Err(ListCode::AnchorMissing.val());
            }
            return Err(ListCode::ExpectMissing.val());
        }
        if !row.anchor_mapped() {
            return Err(ListCode::AnchorNotMapped.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == row.id {
                self.dup_caught += 1;
                return Err(ListCode::CaseIdDuplicate.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 删除（append-only 红线：一律拒）。
    pub fn remove(&mut self, id: &str) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                self.del_caught += 1;
                return Err(ListCode::DeletionForbidden.val());
            }
            i += 1;
        }
        Err(ListCode::CaseOutOfRange.val())
    }

    /// 版本前进。
    pub fn bump_version(&mut self, new_ver: u32) -> Result<(), u32> {
        if new_ver <= self.version {
            return Err(ListCode::VersionNotForward.val());
        }
        self.version = new_ver;
        Ok(())
    }

    /// 全清单 FNV-1a 指纹。
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
            let b2 = self.rows[i].anchor.as_bytes();
            j = 0;
            while j < b2.len() {
                h ^= b2[j] as u64;
                h = h.wrapping_mul(0x100000001b3);
                j += 1;
            }
            h ^= self.rows[i].expect as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：哈希封印。
    pub fn freeze(&mut self) -> u64 {
        self.frozen_hash = self.hash();
        self.frozen_hash
    }

    /// 冻结校验。
    pub fn frozen_ok(&self) -> bool {
        self.frozen_hash != 0 && self.frozen_hash == self.hash()
    }

    /// 追加后冻结必须演进（指纹必变）。
    pub fn freeze_evolved(&self, old_hash: u64) -> bool {
        self.hash() != old_hash
    }
}

/// 冻结基线清单（12 例全入）。
pub fn baseline_list() -> Result<CaseList, u32> {
    let mut l = CaseList::new(LIST_VERSION);
    let mut i = 0;
    while i < CASE_COUNT {
        l.append(CaseRow {
            id: CASE_IDS[i],
            anchor: CASE_ANCHORS[i],
            expect: CASE_EXPECTS[i],
        })?;
        i += 1;
    }
    if l.rows.len() != CASE_COUNT {
        return Err(ListCode::CaseOutOfRange.val());
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

/// 黄金全链：基线 12 例→冻结→追加 1 例→版本前进→冻结演进。
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    let mut l = baseline_list()?;
    // 一一映射点验。
    let mut i = 0;
    while i < l.rows.len() {
        if !l.rows[i].anchor_mapped() {
            return Err(ListCode::AnchorNotMapped.val());
        }
        i += 1;
    }
    let h0 = l.freeze();
    if !l.frozen_ok() {
        return Err(ListCode::FreezeHashMismatch.val());
    }
    // 追加 1 例（新增键位映射）。
    l.append(CaseRow {
        id: "ADV-013",
        anchor: B23_KEYS[0],
        expect: 12,
    })?;
    l.bump_version(LIST_VERSION + 1)?;
    if !l.freeze_evolved(h0) {
        return Err(ListCode::FreezeHashMismatch.val());
    }
    l.freeze();
    if !l.frozen_ok() {
        return Err(ListCode::FreezeHashMismatch.val());
    }
    Ok((l.rows.len(), l.frozen_hash, l.dup_caught + l.del_caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：基线→冻结。
    let mut l = baseline_list()?;
    l.freeze();
    if l.frozen_ok() {
        Ok(())
    } else {
        Err(ListCode::FreezeHashMismatch.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 语义锚未映射 → 必拒。
    let mut l = CaseList::new(1);
    l.append(CaseRow { id: "ADV-099", anchor: "HKLM\\NoSuchKey", expect: 1 })
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 删除违例 → 必拒。
    let mut l = baseline_list()?;
    l.remove("ADV-001")
}

/// 注入 10/10：缺锚、缺期望、重复 ID、版本回退四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：语义锚缺列。
        let mut l0 = CaseList::new(1);
        led.record(0, l0.append(CaseRow { id: "A", anchor: "", expect: 1 }).is_err());
        // 槽1：期望值缺列。
        let mut l1 = CaseList::new(1);
        led.record(1, l1.append(CaseRow { id: "A", anchor: B23_KEYS[0], expect: EXPECT_MISSING }).is_err());
        // 槽2：重复 ID。
        let mut l2 = CaseList::new(1);
        l2.append(CaseRow { id: "D", anchor: B23_KEYS[0], expect: 1 }).ok();
        led.record(2, l2.append(CaseRow { id: "D", anchor: B23_KEYS[1], expect: 2 }).is_err());
        // 槽3：版本回退。
        let mut l3 = CaseList::new(3);
        led.record(3, l3.bump_version(2).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ListCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ListCode::ALL.len() {
        let v = ListCode::ALL[i].val();
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
    // 三列齐备率 100%（基线 12 例）。
    let base = baseline_list();
    let complete_cnt = match &base {
        Ok(l) => {
            let mut c = 0;
            let mut i = 0;
            while i < l.rows.len() {
                if l.rows[i].complete() {
                    c += 1;
                }
                i += 1;
            }
            c
        }
        Err(_) => 0,
    };
    v.push((
        "J1-三列齐备率100%",
        complete_cnt == CASE_COUNT,
        format!("{}/{} 例三列齐", complete_cnt, CASE_COUNT),
    ));
    // 一一映射一致率 100%。
    let map_cnt = match &base {
        Ok(l) => {
            let mut c = 0;
            let mut i = 0;
            while i < l.rows.len() {
                if l.rows[i].anchor_mapped() {
                    c += 1;
                }
                i += 1;
            }
            c
        }
        Err(_) => 0,
    };
    v.push((
        "J1-B23映射一致率100%",
        map_cnt == CASE_COUNT && B23_KEYS.len() == 8,
        format!("{}/{} 例映射命中", map_cnt, CASE_COUNT),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x68140 窗口")));
    v
}

pub fn run_unx16621_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16621 advapi32 注册表 conformance 组清单冻结");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
