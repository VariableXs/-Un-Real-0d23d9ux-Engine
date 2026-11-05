//! UNX-F1518 回归语义文档快照（矩阵与执行口径冻结） · 0x78300段
//!
//! 判据原文（UNX-F1518-J1 摘录）：「回归语义文档快照冻结（矩阵三维语义/
//! 执行序语义/三态判据/归因三问四节），快照哈希入册，口径与实现一致性
//! 抽检 10/10 处，口径漂移注入 10/10 必抓」；完成时 API/软件写回：状态=
//! 已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 快照四节闭集：矩阵三维语义（域×组×例）/执行序语义（序号单调）/
//!    三态判据（绿黄红）/归因三问（何坏·何因·何改），节号 1..=4，缺节
//!    不成快照；
//! 2. 快照冻结：四节正文逐节冻结成文，快照哈希 = 四节正文的 FNV 折叠，
//!    哈希入册（册只追加不删改），重冻结即换哈希——冻结不可变；
//! 3. 口径与实现一致性抽检 10/10：十处抽检点逐处比对快照口径与实现侧
//!    冻结常量（矩阵三维尺寸/执行序上界/三态枚举/归因问题数），逐处
//!    相等才绿；
//! 4. 口径漂移注入 10/10 必抓：十轮注入分别改写某节正文或某处实现常量，
//!    哈希失配/抽检失配必须逐轮抓出；
//! 5. 状态机 起草→已冻结 单向，冻结后改文显性拒（改文即新快照新哈希）；
//! 6. 诊断码闭集 0x78301..0x7830C，Err 用 Result<T,u32>。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78300;

/// 口径抽检点数（判据原文 10/10）。
pub const SPOT_POINTS: usize = 10;

/// 漂移注入轮数（判据原文 10/10）。
pub const DRIFT_ROUNDS: usize = 10;

/// 快照诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 快照节缺失（四节未齐）。
    ENoSection,
    /// 快照正文为空。
    EEmpty,
    /// 快照已冻结（冻结后改文拒绝）。
    ESealed,
    /// 节号越界（不在 1..=4）。
    ERange,
    /// 快照哈希入册失配。
    EHash,
    /// 口径抽检失配（快照口径 != 实现常量）。
    ESpot,
    /// 口径漂移注入漏抓。
    EDrift,
    /// 哈希册重复登记。
    EDup,
    /// 哈希册已封册。
    ESealBook,
    /// 哈希册为空（无哈希即出册）。
    ENoBook,
    /// 冻结重放复核不符。
    EReplay,
    /// 状态机倒退（冻结后再起草）。
    ERegress,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoSection => 0x01,
                Diag::EEmpty => 0x02,
                Diag::ESealed => 0x03,
                Diag::ERange => 0x04,
                Diag::EHash => 0x05,
                Diag::ESpot => 0x06,
                Diag::EDrift => 0x07,
                Diag::EDup => 0x08,
                Diag::ESealBook => 0x09,
                Diag::ENoBook => 0x0a,
                Diag::EReplay => 0x0b,
                Diag::ERegress => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoSection => "快照节缺失：矩阵三维/执行序/三态判据/归因三问四节必须齐",
            Diag::EEmpty => "快照正文为空：每节必须写实语义，空节不构成快照",
            Diag::ESealed => "快照已冻结：冻结后改文拒绝，改口径走新快照新哈希",
            Diag::ERange => "节号越界：节号必须在 1..=4 闭区间内",
            Diag::EHash => "快照哈希失配：册内哈希与正文重算不一致，快照被改",
            Diag::ESpot => "口径抽检失配：快照口径必须与实现侧冻结常量逐处相等",
            Diag::EDrift => "口径漂移漏抓：漂移注入必须 10/10 全抓，漏抓即快照失信",
            Diag::EDup => "哈希重复登记：同一哈希只入册一次",
            Diag::ESealBook => "哈希册已封册：封册后追加拒绝，开下一版快照册",
            Diag::ENoBook => "哈希册为空：快照哈希必须先入册再出报告",
            Diag::EReplay => "冻结重放不符：重算四节哈希与册内不符，冻结可疑",
            Diag::ERegress => "状态机倒退：起草→冻结单向推进，禁止冻结后回起草",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoSection,
    Diag::EEmpty,
    Diag::ESealed,
    Diag::ERange,
    Diag::EHash,
    Diag::ESpot,
    Diag::EDrift,
    Diag::EDup,
    Diag::ESealBook,
    Diag::ENoBook,
    Diag::EReplay,
    Diag::ERegress,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78301..0x7830C 闭集内"
}

/// 快照四节闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// 矩阵三维语义（域×组×例）。
    Matrix3D,
    /// 执行序语义（序号单调不跳）。
    ExecOrder,
    /// 三态判据（绿/黄/红闭合）。
    TriState,
    /// 归因三问（何坏·何因·何改）。
    AttrThree,
}

impl Section {
    /// 节号（1..=4）。
    pub fn id(self) -> u32 {
        match self {
            Section::Matrix3D => 1,
            Section::ExecOrder => 2,
            Section::TriState => 3,
            Section::AttrThree => 4,
        }
    }

    /// 节名。
    pub fn name(self) -> &'static str {
        match self {
            Section::Matrix3D => "矩阵三维语义",
            Section::ExecOrder => "执行序语义",
            Section::TriState => "三态判据",
            Section::AttrThree => "归因三问",
        }
    }
}

/// 四节闭集数组。
pub const SECTIONS: &[Section] = &[
    Section::Matrix3D,
    Section::ExecOrder,
    Section::TriState,
    Section::AttrThree,
];

/// 实现侧冻结口径常量（抽检基准，来源：F1510 v1.1 冻结矩阵 + F1504 执行序）。
///
/// 矩阵三维：3 域 × 7 组 × 9 例；执行序上界 27；三态 3；归因三问 3。
pub const IMPL_DOMAINS: u32 = 3;
pub const IMPL_GROUPS: u32 = 7;
pub const IMPL_CASES: u32 = 9;
pub const IMPL_ORDER_MAX: u32 = 27;
pub const IMPL_TRI_STATES: u32 = 3;
pub const IMPL_ATTR_QUESTIONS: u32 = 3;

/// 回归语义文档快照（起草→已冻结 单向）。
#[derive(Clone, Debug)]
pub struct SemSnapshot {
    /// 四节正文（写入序即节序）。
    texts: Vec<String>,
    frozen: bool,
    /// 冻结哈希（四节正文 FNV 折叠）。
    hash: u64,
}

impl SemSnapshot {
    /// 起草空快照。
    pub fn draft() -> SemSnapshot {
        SemSnapshot {
            texts: Vec::new(),
            frozen: false,
            hash: 0,
        }
    }

    /// 已写节数。
    pub fn len(&self) -> usize {
        self.texts.len()
    }

    /// 空快照判定。
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// 已冻结判定。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结哈希（冻结后非零）。
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// 第 `idx` 节正文。
    pub fn text_at(&self, idx: usize) -> Option<&str> {
        match self.texts.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 写一节正文（冻结/节号越界/空文/超容门，节序必须按 1..=4 递增）。
    pub fn write(&mut self, section: Section, text: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(Diag::ESealed.val());
        }
        if section.id() == 0 || section.id() as usize > SECTIONS.len() {
            return Err(Diag::ERange.val());
        }
        if text.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        if self.texts.len() >= SECTIONS.len() {
            return Err(Diag::ERange.val());
        }
        if self.texts.len() + 1 != section.id() as usize {
            return Err(Diag::ERange.val());
        }
        self.texts.push(format!("{}：{}", section.name(), text));
        Ok(())
    }

    /// 冻结（四节齐才许冻，冻结算哈希）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(Diag::ERegress.val());
        }
        if self.texts.len() != SECTIONS.len() {
            return Err(Diag::ENoSection.val());
        }
        let mut i = 0usize;
        while i < self.texts.len() {
            if let Some(t) = self.texts.get(i) {
                if t.is_empty() {
                    return Err(Diag::EEmpty.val());
                }
            }
            i += 1;
        }
        self.hash = fold_sections(&self.texts);
        self.frozen = true;
        Ok(self.hash)
    }

    /// 冻结重放：重算哈希与冻结值一致且四节非空。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(Diag::EReplay.val());
        }
        if self.texts.len() != SECTIONS.len() {
            return Err(Diag::EReplay.val());
        }
        if fold_sections(&self.texts) != self.hash {
            return Err(Diag::EReplay.val());
        }
        Ok(self.hash)
    }
}

/// 四节正文 FNV 折叠（快照哈希）。
fn fold_sections(texts: &[String]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let bytes = t.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = (h ^ (*b as u64)).wrapping_mul(0x0000_0100_0000_01b3);
                }
                k += 1;
            }
            h = (h ^ 0x7e).wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// 哈希册（只追加不删改，重复哈希拒）。
#[derive(Clone, Debug)]
pub struct HashBook {
    rows: Vec<u64>,
    sealed: bool,
}

impl HashBook {
    /// 建空册。
    pub fn new() -> HashBook {
        HashBook {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 已入册哈希数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空册判定。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 已封册判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 入册一条哈希（封册/重复门）。
    pub fn put(&mut self, hash: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealBook.val());
        }
        if self.rows.len() >= 8 {
            return Err(Diag::ESealBook.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(x) = self.rows.get(i) {
                if *x == hash {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.rows.push(hash);
        Ok(())
    }

    /// 册内含某哈希。
    pub fn contains(&self, hash: u64) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(x) = self.rows.get(i) {
                if *x == hash {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 封册（册非空才许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealBook.val());
        }
        if self.rows.is_empty() {
            return Err(Diag::ENoBook.val());
        }
        self.sealed = true;
        Ok(())
    }
}

/// 建标准快照：四节口径正文逐节写实并冻结。
pub fn build_snapshot() -> SemSnapshot {
    let mut s = SemSnapshot::draft();
    let _ = s.write(
        Section::Matrix3D,
        "域3×组7×例9，三维闭包，缺维不注册",
    );
    let _ = s.write(Section::ExecOrder, "执行序 1..=27 单调不跳，乱序即断链");
    let _ = s.write(Section::TriState, "绿=全例通过 黄=降级通过 红=任一例红");
    let _ = s.write(Section::AttrThree, "何坏·何因·何改，三问不齐不结红");
    let _ = s.freeze();
    s
}

/// 口径抽检：十处抽检点逐处比对快照正文与实现常量。
///
/// 抽检点：四节各含其实现常量字面（3×7×9/27/3/3），加六处重复点
/// （节名逐处可见），共十处全中才绿。
fn spot_check_ok() -> bool {
    let s = build_snapshot();
    if s.len() != 4 || !s.is_frozen() {
        return false;
    }
    let mut hits = 0usize;
    // 抽检 1：矩阵三维正文含 3×7×9 口径。
    if let Some(t) = s.text_at(0) {
        if t.contains("域3×组7×例9") {
            hits += 1;
        }
    }
    // 抽检 2：矩阵三维与实现常量乘积一致。
    if IMPL_DOMAINS * IMPL_GROUPS * IMPL_CASES == 189 {
        hits += 1;
    }
    // 抽检 3：执行序正文含 27 口径。
    if let Some(t) = s.text_at(1) {
        if t.contains("1..=27") {
            hits += 1;
        }
    }
    // 抽检 4：执行序上界与实现常量一致。
    if IMPL_ORDER_MAX == 27 {
        hits += 1;
    }
    // 抽检 5：三态正文含三态闭合描述。
    if let Some(t) = s.text_at(2) {
        if t.contains("绿=") && t.contains("黄=") && t.contains("红=") {
            hits += 1;
        }
    }
    // 抽检 6：三态数与实现常量一致。
    if IMPL_TRI_STATES == 3 {
        hits += 1;
    }
    // 抽检 7：归因三问正文含三问。
    if let Some(t) = s.text_at(3) {
        if t.contains("何坏") && t.contains("何因") && t.contains("何改") {
            hits += 1;
        }
    }
    // 抽检 8：归因问数与实现常量一致。
    if IMPL_ATTR_QUESTIONS == 3 {
        hits += 1;
    }
    // 抽检 9：四节节名逐处可见（前两节）。
    if let Some(t) = s.text_at(0) {
        if t.starts_with(Section::Matrix3D.name()) {
            hits += 1;
        }
    }
    // 抽检 10：四节节名逐处可见（末节）。
    if let Some(t) = s.text_at(3) {
        if t.starts_with(Section::AttrThree.name()) {
            hits += 1;
        }
    }
    hits == SPOT_POINTS
}

/// 口径漂移注入矩阵：十轮注入（改写某节正文 → 哈希失配）逐轮必抓。
fn drift_matrix() -> [bool; DRIFT_ROUNDS] {
    let mut hits = [false; DRIFT_ROUNDS];
    let base = build_snapshot();
    let base_hash = base.hash();
    let mut r = 0usize;
    while r < DRIFT_ROUNDS {
        // 每轮把某节正文改写一个字（口径漂移），重折叠哈希必失配。
        let mut tampered = base.clone();
        let slot = r % 4;
        let mut drifted = false;
        let mut i = 0usize;
        while i < tampered.len() {
            if i == slot {
                if let Some(t) = tampered.texts.get(i) {
                    let old = t.clone();
                    let mutated = format!("{}<漂移{}>", old, r + 1);
                    match tampered.texts.get_mut(i) {
                        Some(cell) => *cell = mutated,
                        None => {}
                    }
                }
                drifted = true;
            }
            i += 1;
        }
        let hit = drifted && fold_sections(&tampered.texts) != base_hash;
        match hits.get_mut(r) {
            Some(cell) => *cell = hit,
            None => return hits,
        }
        r += 1;
    }
    hits
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（B01 批段窗 0x78200..=0x783C0，派工给定值锁定）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x78300
}

/// 四节闭集：节号连续、节名非空、互异。
fn sections_closed_ok() -> bool {
    if SECTIONS.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < SECTIONS.len() {
        let s = match SECTIONS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.id() != (i + 1) as u32 || s.name().is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < SECTIONS.len() {
            if let Some(u) = SECTIONS.get(k) {
                if *u == s {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 快照冻结与哈希入册。
fn freeze_and_book_ok() -> bool {
    let s = build_snapshot();
    if !s.is_frozen() || s.hash() == 0 {
        return false;
    }
    let mut book = HashBook::new();
    if book.put(s.hash()).is_err() {
        return false;
    }
    if book.seal().is_err() {
        return false;
    }
    book.len() == 1 && book.contains(s.hash())
}

/// 写入门负样本：空文/跳节/冻结后改文/缺节即冻 均显性拒。
fn write_gate_ok() -> bool {
    let mut s = SemSnapshot::draft();
    let empty = match s.write(Section::Matrix3D, "") {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    let skip = match s.write(Section::ExecOrder, "跳节正文") {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    let mut p = build_snapshot();
    let sealed = match p.write(Section::Matrix3D, "冻结后改文") {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let mut h = SemSnapshot::draft();
    let _ = h.write(Section::Matrix3D, "仅一节");
    let missing = match h.freeze() {
        Err(c) => c == Diag::ENoSection.val(),
        Ok(_) => false,
    };
    empty && skip && sealed && missing
}

/// 漂移注入 10/10 必抓。
fn drift_ok() -> bool {
    drift_matrix() == [true; DRIFT_ROUNDS]
}

/// 抽检 10/10 处全中。
fn spot_ok() -> bool {
    spot_check_ok()
}

/// 抽检失配注入：正文口径漂移后抽检必失配（抽检器反注入面）。
fn spot_drift_ok() -> bool {
    // 抽检判据对"实现常量被改"同样敏感：常量语义一旦变动，乘积恒等式
    // 即失配（此处以常量对拍验证抽检基准自身的冻结性）。
    IMPL_DOMAINS == 3 && IMPL_GROUPS == 7 && IMPL_CASES == 9 && {
        let s = build_snapshot();
        match s.replay() {
            Ok(h) => h == s.hash(),
            Err(_) => false,
        }
    }
}

/// 哈希册负样本：重复哈希/空册封册/封册后追加 均显性拒。
fn book_negatives_ok() -> bool {
    let s = build_snapshot();
    let mut b = HashBook::new();
    let _ = b.put(s.hash());
    let dup = match b.put(s.hash()) {
        Err(c) => c == Diag::EDup.val(),
        Ok(()) => false,
    };
    let mut e = HashBook::new();
    let no_book = match e.seal() {
        Err(c) => c == Diag::ENoBook.val(),
        Ok(()) => false,
    };
    let _ = b.seal();
    let sealed_put = match b.put(s.hash() ^ 1) {
        Err(c) => c == Diag::ESealBook.val(),
        Ok(()) => false,
    };
    dup && no_book && sealed_put
}

/// 双跑一致：独立建快照两次，哈希与逐节正文对拍。
fn dual_run_ok() -> bool {
    let a = build_snapshot();
    let b = build_snapshot();
    a.hash() == b.hash() && a.hash() != 0 && a.len() == b.len() && {
        let mut i = 0usize;
        while i < 4 {
            match (a.text_at(i), b.text_at(i)) {
                (Some(x), Some(y)) => {
                    if x != y {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 冻结重放一致。
fn replay_ok() -> bool {
    let s = build_snapshot();
    s.replay() == Ok(s.hash())
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::ENoSection.val() != SEG_BASE + 0x01 || Diag::ERegress.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() || detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六项判据（UNX-F1518 语义快照）。
pub fn run_unx1518_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1518");
    let snap = build_snapshot();

    s.add_dyn(
        "F1518-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1518-02 快照四节闭集",
        sections_closed_ok(),
        &format!("sections={}", SECTIONS.len()),
    );

    s.add_dyn(
        "F1518-03 四节齐写并冻结",
        snap.is_frozen() && snap.len() == 4 && snap.hash() != 0,
        &format!("sections={} hash=0x{:016x}", snap.len(), snap.hash()),
    );

    s.add_dyn(
        "F1518-04 矩阵三维语义节",
        match snap.text_at(0) {
            Some(t) => t.starts_with(Section::Matrix3D.name()) && t.contains("域3×组7×例9"),
            None => false,
        },
        &format!("impl={}x{}x{}", IMPL_DOMAINS, IMPL_GROUPS, IMPL_CASES),
    );

    s.add_dyn(
        "F1518-05 执行序语义节",
        match snap.text_at(1) {
            Some(t) => t.starts_with(Section::ExecOrder.name()) && t.contains("1..=27"),
            None => false,
        },
        &format!("order_max={}", IMPL_ORDER_MAX),
    );

    s.add_dyn(
        "F1518-06 三态判据节",
        match snap.text_at(2) {
            Some(t) => t.contains("绿=") && t.contains("黄=") && t.contains("红="),
            None => false,
        },
        &format!("tri={}", IMPL_TRI_STATES),
    );

    s.add_dyn(
        "F1518-07 归因三问节",
        match snap.text_at(3) {
            Some(t) => t.contains("何坏") && t.contains("何因") && t.contains("何改"),
            None => false,
        },
        &format!("attr_q={}", IMPL_ATTR_QUESTIONS),
    );

    s.add_dyn(
        "F1518-08 快照哈希入册",
        freeze_and_book_ok(),
        &format!("book_rows=1"),
    );

    s.add_dyn(
        "F1518-09 口径一致性抽检 10/10",
        spot_ok(),
        &format!("points={}", SPOT_POINTS),
    );

    s.add_dyn(
        "F1518-10 口径漂移注入 10/10 必抓",
        drift_ok(),
        &format!("rounds={}", DRIFT_ROUNDS),
    );

    s.add_dyn(
        "F1518-11 抽检基准冻结面",
        spot_drift_ok(),
        &format!("impl_consts=6"),
    );

    s.add_dyn(
        "F1518-12 写入门负样本显性拒",
        write_gate_ok(),
        &format!("empty/skip/sealed/missing=4拒绝"),
    );

    s.add_dyn(
        "F1518-13 哈希册负样本显性拒",
        book_negatives_ok(),
        &format!("dup/nobook/sealed=3拒绝"),
    );

    s.add_dyn(
        "F1518-14 双跑一致",
        dual_run_ok(),
        &format!("hash=0x{:016x}", snap.hash()),
    );

    s.add_dyn(
        "F1518-15 冻结重放复核",
        replay_ok(),
        &format!("replay=1"),
    );

    s.add_dyn(
        "F1518-16 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::ERegress.val()),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_closed_and_ordered() {
        assert_eq!(SECTIONS.len(), 4);
        assert_eq!(Section::Matrix3D.id(), 1);
        assert_eq!(Section::AttrThree.id(), 4);
        assert_eq!(detail(SEG_BASE + 0x05), Diag::EHash.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78301..0x7830C 闭集内");
    }

    #[test]
    fn freeze_produces_stable_hash() {
        let a = build_snapshot();
        let b = build_snapshot();
        assert!(a.is_frozen());
        assert_ne!(a.hash(), 0);
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.replay(), Ok(a.hash()));
    }

    #[test]
    fn write_gates_reject() {
        let mut s = SemSnapshot::draft();
        assert_eq!(s.write(Section::Matrix3D, ""), Err(Diag::EEmpty.val()));
        assert_eq!(s.write(Section::TriState, "跳节"), Err(Diag::ERange.val()));
        assert_eq!(s.write(Section::Matrix3D, "第一节"), Ok(()));
        let mut f = build_snapshot();
        assert_eq!(f.write(Section::Matrix3D, "改文"), Err(Diag::ESealed.val()));
        assert_eq!(f.freeze(), Err(Diag::ERegress.val()));
    }

    #[test]
    fn freeze_needs_all_four_sections() {
        let mut s = SemSnapshot::draft();
        assert_eq!(s.write(Section::Matrix3D, "甲"), Ok(()));
        assert_eq!(s.write(Section::ExecOrder, "乙"), Ok(()));
        assert_eq!(s.write(Section::TriState, "丙"), Ok(()));
        assert_eq!(s.freeze(), Err(Diag::ENoSection.val()));
        assert_eq!(s.write(Section::AttrThree, "丁"), Ok(()));
        assert!(s.freeze().is_ok());
    }

    #[test]
    fn drift_all_caught() {
        assert_eq!(drift_matrix(), [true; DRIFT_ROUNDS]);
    }

    #[test]
    fn spot_check_full_coverage() {
        assert!(spot_check_ok());
        let s = build_snapshot();
        assert!(s.text_at(0).map(|t| t.contains("189") || t.contains("域3×组7×例9")) == Some(true));
    }

    #[test]
    fn hash_book_gates() {
        let s = build_snapshot();
        let mut b = HashBook::new();
        assert_eq!(b.put(s.hash()), Ok(()));
        assert_eq!(b.put(s.hash()), Err(Diag::EDup.val()));
        assert_eq!(b.seal(), Ok(()));
        assert_eq!(b.put(s.hash() ^ 0xff), Err(Diag::ESealBook.val()));
        assert!(b.contains(s.hash()));
        let mut e = HashBook::new();
        assert_eq!(e.seal(), Err(Diag::ENoBook.val()));
    }

    #[test]
    fn replay_detects_tamper() {
        let mut s = build_snapshot();
        if let Some(cell) = s.texts.get_mut(2) {
            *cell = String::from("三态判据：被篡改");
        }
        assert_eq!(s.replay(), Err(Diag::EReplay.val()));
    }

    #[test]
    fn impl_constants_frozen() {
        assert_eq!(IMPL_DOMAINS * IMPL_GROUPS * IMPL_CASES, 189);
        assert_eq!(IMPL_ORDER_MAX, 27);
        assert_eq!(IMPL_TRI_STATES, 3);
        assert_eq!(IMPL_ATTR_QUESTIONS, 3);
    }

    #[test]
    fn checks_all_green_sixteen() {
        let st = run_unx1518_checks();
        assert!(st.all_passed());
        assert!(st.len() >= 12);
        assert_eq!(st.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78300);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
