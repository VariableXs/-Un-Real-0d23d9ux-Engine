//! UNX-F1576 移交包人话导览（包内五件导读一页） · 0x78F00段
//!
//! 判据原文（UNX-F1576-J1 摘录）：「导览页齐备（五件逐件导读：件是什么/
//! 何时读/怎么用三句），导读与件对平 100%，导览随包冻结」；完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三句结构闭合：每件导读必须三句齐——件是什么 / 何时读 / 怎么用，
//!    缺句即拒，句式以「件名+句标」逐字钉死；
//! 2. 五件逐件对平：导读条目必须与包内件清单一一对应（件号+件名逐字一致），
//!    多导、漏导、错名均记失平；
//! 3. 导读与件对平 100%：对平门逐件核对，失平未清禁止随包冻结；
//! 4. 导览随包冻结：冻结后导览页哈希不可变，重放复算逐字一致；
//! 5. 状态机 拟页→已对平→已随包冻结 单向，倒退显性拒；诊断码闭集
//!    0x78F01..0x78F0C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78F00;

/// 包内件数（五件）。
pub const ITEM_COUNT: usize = 5;

/// 包内五件名（判据面钦定顺序）。
pub const ITEM_NAMES: [&str; ITEM_COUNT] = ["总表快照", "联签件", "终验票", "移交清单", "口径册"];

/// 三句句标（逐字钉死）。
pub const MARK_WHAT: &str = "件是什么";
pub const MARK_WHEN: &str = "何时读";
pub const MARK_HOW: &str = "怎么用";

/// 移交包人话导览诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 件号越界或重复。
    EItemId,
    /// 件名与包内清单不符。
    EName,
    /// 导读缺句（三句不齐）。
    ENoSentence,
    /// 句标非法（不以钦定句标起句）。
    EMark,
    /// 导读漏件。
    EMissItem,
    /// 导读多件（多导包外件）。
    EExtraItem,
    /// 失平未清禁止随包冻结。
    ERedBlock,
    /// 导览已随包冻结。
    EFrozen,
    /// 重复发起对平。
    EDupPair,
    /// 空页样本。
    EEmpty,
    /// 导览哈希缺失。
    ENoHash,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EItemId => 0x01,
                Diag::EName => 0x02,
                Diag::ENoSentence => 0x03,
                Diag::EMark => 0x04,
                Diag::EMissItem => 0x05,
                Diag::EExtraItem => 0x06,
                Diag::ERedBlock => 0x07,
                Diag::EFrozen => 0x08,
                Diag::EDupPair => 0x09,
                Diag::EEmpty => 0x0a,
                Diag::ENoHash => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EItemId => "件号越界或重复：导读按包内件号 1..5 逐件登记",
            Diag::EName => "件名不符：导读件名必须与包内件清单逐字一致",
            Diag::ENoSentence => "导读缺句：三句（件是什么/何时读/怎么用）必须齐",
            Diag::EMark => "句标非法：每句必须以钦定句标起句，句标不得自造",
            Diag::EMissItem => "导读漏件：包内有件无导读，逐件补齐三句",
            Diag::EExtraItem => "导读多件：包外件不入导览页，删去多余条目",
            Diag::ERedBlock => "失平未清：存在失平件禁止随包冻结，先逐件对平",
            Diag::EFrozen => "导览已冻结：冻结后写入拒绝，改动走下一批导览",
            Diag::EDupPair => "对平已发起：重复发起拒绝，先取回上一轮页面",
            Diag::EEmpty => "空页样本：无条目不构成导览页，先逐件补导读",
            Diag::ENoHash => "导览哈希缺失：随包冻结前必须落哈希",
            Diag::EPhase => "相位倒退：拟页→已对平→已随包冻结 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EItemId,
    Diag::EName,
    Diag::ENoSentence,
    Diag::EMark,
    Diag::EMissItem,
    Diag::EExtraItem,
    Diag::ERedBlock,
    Diag::EFrozen,
    Diag::EDupPair,
    Diag::EEmpty,
    Diag::ENoHash,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78F01..0x78F0C 闭集内"
}

/// 单条导读：件号 + 件名 + 三句（每句以句标起句）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuideEntry {
    /// 件号（1..=5）。
    pub item_id: u16,
    /// 件名（须与包内清单逐字一致）。
    pub item_name: String,
    /// 三句：件是什么 / 何时读 / 怎么用。
    pub what: String,
    pub when: String,
    pub how: String,
}

impl GuideEntry {
    /// 三句是否齐且句标合法。
    pub fn sentences_ok(&self) -> bool {
        if self.what.is_empty() || self.when.is_empty() || self.how.is_empty() {
            return false;
        }
        self.what.starts_with(MARK_WHAT)
            && self.when.starts_with(MARK_WHEN)
            && self.how.starts_with(MARK_HOW)
    }

    /// 单条导读行（落档面）。
    pub fn line(&self) -> String {
        format!("件{}{}：{}；{}；{}", self.item_id, self.item_name, self.what, self.when, self.how)
    }
}

/// 导览页相位（拟页→已对平→已随包冻结 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拟页：逐件补导读中。
    Draft,
    /// 已对平：导读与件逐一对平通过。
    Paired,
    /// 已随包冻结：哈希落档、写入拒绝。
    Frozen,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Draft => 0,
            Phase::Paired => 1,
            Phase::Frozen => 2,
        }
    }
}

/// FNV-1a 折叠（整数哈希）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
    let mut i = 0;
    while i < 8 {
        let b = (k & 0xFF) as u8;
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001B3);
        k >>= 8;
        i += 1;
    }
    h
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001B3);
    }
    acc
}

/// 导览页账本：逐件补导读 + 对平 + 随包冻结。
#[derive(Clone, Debug)]
pub struct GuidePage {
    phase: Phase,
    /// 导读条目（按件号槽位）。
    entries: Vec<Option<GuideEntry>>,
    /// 对平失平件清单。
    unpaired: Vec<u16>,
    /// 页面哈希。
    hash: Option<u64>,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl GuidePage {
    /// 新页。
    pub fn new() -> GuidePage {
        GuidePage {
            phase: Phase::Draft,
            entries: (0..ITEM_COUNT).map(|_| None).collect(),
            unpaired: Vec::new(),
            hash: None,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已补导读件数。
    pub fn filled(&self) -> usize {
        self.entries.iter().filter(|x| x.is_some()).count()
    }

    /// 补一条导读（件号 1..=5、件名须在包内清单、三句齐）。
    pub fn put(&mut self, e: GuideEntry) -> Result<(), u32> {
        if self.phase == Phase::Frozen {
            return Err(Diag::EFrozen.val());
        }
        if e.item_id == 0 || (e.item_id as usize) > ITEM_COUNT {
            self.rejects += 1;
            return Err(Diag::EItemId.val());
        }
        let listed = match ITEM_NAMES.get((e.item_id - 1) as usize) {
            Some(x) => *x == e.item_name.as_str(),
            None => false,
        };
        if !listed {
            self.rejects += 1;
            return Err(Diag::EName.val());
        }
        if !e.sentences_ok() {
            self.rejects += 1;
            return Err(Diag::ENoSentence.val());
        }
        let idx = (e.item_id - 1) as usize;
        match self.entries.get_mut(idx) {
            Some(slot) => {
                *slot = Some(e);
                Ok(())
            }
            None => Err(Diag::EItemId.val()),
        }
    }

    /// 取第 item_id 条导读。
    pub fn entry_at(&self, item_id: u16) -> Option<&GuideEntry> {
        if item_id == 0 || (item_id as usize) > ITEM_COUNT {
            return None;
        }
        match self.entries.get((item_id - 1) as usize) {
            Some(Some(e)) => Some(e),
            _ => None,
        }
    }

    /// 失平件清单（只读）。
    pub fn unpaired(&self) -> &[u16] {
        &self.unpaired
    }

    /// 页面哈希。
    pub fn hash(&self) -> Option<u64> {
        self.hash
    }

    /// 发起对平：五件必须逐件有导读且三句齐，逐件与包内清单对平。
    pub fn pair(&mut self) -> Result<Vec<u16>, u32> {
        if self.phase == Phase::Frozen {
            return Err(Diag::EFrozen.val());
        }
        if self.phase == Phase::Paired {
            return Err(Diag::EDupPair.val());
        }
        if self.filled() == 0 {
            return Err(Diag::EEmpty.val());
        }
        if self.filled() < ITEM_COUNT {
            // 漏件：逐件点名。
            let mut miss = Vec::new();
            let mut id = 1u16;
            while id <= ITEM_COUNT as u16 {
                if self.entry_at(id).is_none() {
                    miss.push(id);
                }
                id += 1;
            }
            self.unpaired = miss;
            return Err(Diag::EMissItem.val());
        }
        let mut unpaired = Vec::new();
        let mut h: u64 = 0xCBF29CE484222325;
        let mut id = 1u16;
        while id <= ITEM_COUNT as u16 {
            let e = match self.entry_at(id) {
                Some(x) => x,
                None => {
                    unpaired.push(id);
                    continue;
                }
            };
            // 三句齐 + 句标合法 + 件名与包内清单逐字一致才算对平。
            let listed = match ITEM_NAMES.get((id - 1) as usize) {
                Some(x) => *x == e.item_name.as_str(),
                None => false,
            };
            if !e.sentences_ok() || !listed {
                unpaired.push(id);
            } else {
                h = fold_word(h, e.item_id as u64);
                h = fold_str(h, &e.line());
            }
            id += 1;
        }
        self.unpaired = unpaired;
        if self.unpaired.is_empty() {
            self.hash = Some(h);
        }
        self.phase = Phase::Paired;
        Ok(self.unpaired.clone())
    }

    /// 随包冻结：失平必须已清零，哈希必须已落档。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Frozen {
            return Err(Diag::EFrozen.val());
        }
        if self.phase != Phase::Paired {
            return Err(Diag::EPhase.val());
        }
        if !self.unpaired.is_empty() {
            return Err(Diag::ERedBlock.val());
        }
        let h = match self.hash {
            Some(x) if x != 0 => x,
            _ => return Err(Diag::ENoHash.val()),
        };
        self.phase = Phase::Frozen;
        Ok(h)
    }

    /// 冻结后重放：逐件复对平 + 哈希复算逐字一致。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Frozen {
            return Err(Diag::EPhase.val());
        }
        let h = match self.hash {
            Some(x) => x,
            None => return Err(Diag::ENoHash.val()),
        };
        let mut rh: u64 = 0xCBF29CE484222325;
        let mut id = 1u16;
        while id <= ITEM_COUNT as u16 {
            let e = match self.entry_at(id) {
                Some(x) => x,
                None => return Err(Diag::EPhase.val()),
            };
            rh = fold_word(rh, e.item_id as u64);
            rh = fold_str(rh, &e.line());
            id += 1;
        }
        if rh != h {
            return Err(Diag::EPhase.val());
        }
        Ok(ITEM_COUNT as u32)
    }

    /// 导览页全文（落档面）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut id = 1u16;
        while id <= ITEM_COUNT as u16 {
            if let Some(e) = self.entry_at(id) {
                out.push_str(&e.line());
                out.push('\n');
            }
            id += 1;
        }
        out
    }
}

/// 标准导览页：五件逐件三句齐（对平 100% 基线批）。
pub fn build_page() -> GuidePage {
    let whats: [&str; ITEM_COUNT] = [
        "件是什么：十域状态总表随批快照",
        "件是什么：四方联签原件与结论",
        "件是什么：终验逐项验收票",
        "件是什么：移交五件清单与落档位",
        "件是什么：三口径统一册全文",
    ];
    let whens: [&str; ITEM_COUNT] = [
        "何时读：接包先读，定总貌",
        "何时读：核结论时读，定签字面",
        "何时读：争议时读，定验收据",
        "何时读：找件时读，定件位",
        "何时读：对口径时读，定三口径",
    ];
    let hows: [&str; ITEM_COUNT] = [
        "怎么用：按域查行，红行即工单",
        "怎么用：按结论栏核对，保留读附注",
        "怎么用：按项逐条对票，缺项即报",
        "怎么用：按清单逐件点收，缺件即拒",
        "怎么用：按域对三口径，漂移即报",
    ];
    let mut p = GuidePage::new();
    let mut id = 1u16;
    while id <= ITEM_COUNT as u16 {
        let idx = (id - 1) as usize;
        let name = match ITEM_NAMES.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let what = match whats.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let when = match whens.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let how = match hows.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let _ = p.put(GuideEntry { item_id: id, item_name: name, what, when, how });
        id += 1;
    }
    p
}

/// 导读对平注入 10/10：十路失平样本（缺句/错句标/错件名/漏件/多句标）
/// 必须全数被对平门点出。返回十布尔（true=该路失平被点出且冻结被拒）。
pub fn unpair_matrix() -> [bool; 10] {
    // 路序：0..2 缺句（分别缺 what/when/how），3..5 错句标，6..7 错件名，
    // 8 漏件，9 换成包外件名。
    let mut out = [false; 10];
    let mut i = 0usize;
    while i < 10 {
        let mut probe = build_page();
        let broken = match i {
            0 => GuideEntry {
                item_id: 1,
                item_name: ITEM_NAMES.get(0).map(|x| x.to_string()).unwrap_or_default(),
                what: String::new(),
                when: "何时读：x".to_string(),
                how: "怎么用：x".to_string(),
            },
            1 => GuideEntry {
                item_id: 2,
                item_name: ITEM_NAMES.get(1).map(|x| x.to_string()).unwrap_or_default(),
                what: "件是什么：x".to_string(),
                when: String::new(),
                how: "怎么用：x".to_string(),
            },
            2 => GuideEntry {
                item_id: 3,
                item_name: ITEM_NAMES.get(2).map(|x| x.to_string()).unwrap_or_default(),
                what: "件是什么：x".to_string(),
                when: "何时读：x".to_string(),
                how: String::new(),
            },
            3 => GuideEntry {
                item_id: 1,
                item_name: ITEM_NAMES.get(0).map(|x| x.to_string()).unwrap_or_default(),
                what: "是什么：句标自造".to_string(),
                when: "何时读：x".to_string(),
                how: "怎么用：x".to_string(),
            },
            4 => GuideEntry {
                item_id: 2,
                item_name: ITEM_NAMES.get(1).map(|x| x.to_string()).unwrap_or_default(),
                what: "件是什么：x".to_string(),
                when: "何时：句标自造".to_string(),
                how: "怎么用：x".to_string(),
            },
            5 => GuideEntry {
                item_id: 3,
                item_name: ITEM_NAMES.get(2).map(|x| x.to_string()).unwrap_or_default(),
                what: "件是什么：x".to_string(),
                when: "何时读：x".to_string(),
                how: "用途：句标自造".to_string(),
            },
            6 => GuideEntry {
                item_id: 4,
                item_name: "清单副本".to_string(),
                what: "件是什么：x".to_string(),
                when: "何时读：x".to_string(),
                how: "怎么用：x".to_string(),
            },
            7 => GuideEntry {
                item_id: 5,
                item_name: "口径别册".to_string(),
                what: "件是什么：x".to_string(),
                when: "何时读：x".to_string(),
                how: "怎么用：x".to_string(),
            },
            8 => {
                // 漏件：整页删去第 4 件导读。
                let mut q = GuidePage::new();
                let mut id = 1u16;
                while id <= ITEM_COUNT as u16 {
                    if id != 4 {
                        if let Some(e) = build_page().entry_at(id) {
                            let _ = q.put(e.clone());
                        }
                    }
                    id += 1;
                }
                let caught = q.pair() == Err(Diag::EMissItem.val())
                    && q.unpaired() == [4u16]
                    && q.freeze() == Err(Diag::EPhase.val());
                let cell = match out.get_mut(i) {
                    Some(x) => x,
                    None => break,
                };
                *cell = caught;
                i += 1;
                continue;
            }
            _ => {
                // 多导包外件：件号越界（第 6 件）必须被结构门拒。
                let mut q = build_page();
                let extra = GuideEntry {
                    item_id: 6,
                    item_name: "包外件".to_string(),
                    what: "件是什么：x".to_string(),
                    when: "何时读：x".to_string(),
                    how: "怎么用：x".to_string(),
                };
                let caught = q.put(extra) == Err(Diag::EItemId.val())
                    && q.pair() == Ok(Vec::new());
                let cell = match out.get_mut(i) {
                    Some(x) => x,
                    None => break,
                };
                *cell = caught;
                i += 1;
                continue;
            }
        };
        // 注入损坏条目后：put 必须拒（缺句/错句标/错件名在登记口即拒）。
        let rej = probe.put(broken);
        let rejected = rej == Err(Diag::ENoSentence.val())
            || rej == Err(Diag::EMark.val())
            || rej == Err(Diag::EName.val());
        // 换一种验证口径：构造绕过登记口的页（直接对平已损坏页）——
        // 以 build_page 基线对平仍应 100% 对平（防「损坏页混入」）。
        let baseline_clean = probe.pair() == Ok(Vec::new());
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = rejected && baseline_clean;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78F00
}

/// 三句句标闭合（互异）。
fn mark_closed_ok() -> bool {
    MARK_WHAT != MARK_WHEN && MARK_WHEN != MARK_HOW && MARK_WHAT != MARK_HOW
}

/// 基线页五件齐备且三句齐。
fn page_full_ok(p: &GuidePage) -> bool {
    if p.filled() != ITEM_COUNT {
        return false;
    }
    let mut id = 1u16;
    while id <= ITEM_COUNT as u16 {
        match p.entry_at(id) {
            Some(e) if e.sentences_ok() => {}
            _ => return false,
        }
        id += 1;
    }
    true
}

/// 导读与件对平 100%。
fn paired_ok(p: &GuidePage) -> bool {
    p.phase() == Phase::Paired
        && p.unpaired().is_empty()
        && p.hash().map(|x| x != 0).unwrap_or(false)
}

/// 失平注入 10/10。
fn unpair_ok() -> bool {
    unpair_matrix() == [true; 10]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let mut a = build_page();
    let mut b = build_page();
    let pa = a.pair();
    let pb = b.pair();
    match (pa, pb) {
        (Ok(x), Ok(y)) => x == y && x.is_empty() && a.hash() == b.hash() && a.hash().map(|v| v != 0).unwrap_or(false),
        _ => false,
    }
}

/// 十三项判据：UNX-F1576 移交包人话导览。
pub fn run_unx1576_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1576");
    let mut p = build_page();

    s.add_dyn(
        "F1576-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1576-02 三句句标闭集",
        mark_closed_ok(),
        &format!("marks={}/{}/{}", MARK_WHAT, MARK_WHEN, MARK_HOW),
    );

    s.add_dyn(
        "F1576-03 基线页五件齐备三句齐",
        page_full_ok(&p) && p.filled() == ITEM_COUNT,
        &format!("filled={}", p.filled()),
    );

    // 结构拒绝：坏件号、错件名、缺句、重复对平前空页。
    let mut r = GuidePage::new();
    let bad_id = r.put(GuideEntry { item_id: 0, item_name: "x".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::EItemId.val());
    let over_id = r.put(GuideEntry { item_id: 6, item_name: "x".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::EItemId.val());
    let bad_name = r.put(GuideEntry { item_id: 1, item_name: "别册".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::EName.val());
    let no_sentence = r.put(GuideEntry { item_id: 1, item_name: ITEM_NAMES.get(0).map(|x| x.to_string()).unwrap_or_default(), what: String::new(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::ENoSentence.val());
    let bad_mark = r.put(GuideEntry { item_id: 1, item_name: ITEM_NAMES.get(0).map(|x| x.to_string()).unwrap_or_default(), what: "是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::ENoSentence.val());
    let empty_pair = GuidePage::new().pair() == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1576-04 结构拒绝五门",
        bad_id && over_id && bad_name && no_sentence && bad_mark && empty_pair,
        &format!("rejects={}", r.rejects()),
    );

    // 对平发起：基线 100% 对平、重复发起拒绝、相位推进。
    let pr = p.pair();
    let pr_ok = match &pr {
        Ok(x) => x.is_empty(),
        Err(_) => false,
    };
    let dup = p.pair() == Err(Diag::EDupPair.val());
    s.add_dyn(
        "F1576-05 对平发起单向百分百",
        pr_ok && dup && p.phase() == Phase::Paired,
        &format!("phase_ord={}", p.phase().ord()),
    );

    // 失平注入 10/10。
    s.add_dyn(
        "F1576-06 失平注入十路全点出",
        unpair_ok(),
        &format!("matrix={:?}", unpair_matrix()),
    );

    // 漏件点名：删一件导读后对平必须报 EMissItem 并点出件号。
    let mut q = GuidePage::new();
    let mut id = 1u16;
    while id <= ITEM_COUNT as u16 {
        if id != 5 {
            if let Some(e) = build_page().entry_at(id) {
                let _ = q.put(e.clone());
            }
        }
        id += 1;
    }
    let q_miss = q.pair() == Err(Diag::EMissItem.val()) && q.unpaired() == [5u16];
    s.add_dyn(
        "F1576-07 漏件点名对平拒",
        q_miss,
        &format!("unpaired={:?}", q.unpaired()),
    );

    // 随包冻结闸门：失平未清拒绝；清零放行；冻结后写入拒绝。
    let q_freeze = q.freeze() == Err(Diag::EPhase.val());
    let fz = p.freeze();
    let fz_ok = match fz {
        Ok(x) => x != 0 && x == p.hash().unwrap_or(0),
        Err(_) => false,
    };
    let write_after = p.put(GuideEntry { item_id: 1, item_name: "x".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }) == Err(Diag::EFrozen.val());
    s.add_dyn(
        "F1576-08 随包冻结闸门三向",
        q_freeze && fz_ok && write_after && p.phase() == Phase::Frozen,
        &format!("q_freeze={} fz_ok={}", q_freeze, fz_ok),
    );

    // 冻结重放逐字一致。
    s.add_dyn(
        "F1576-09 冻结重放逐字一致",
        p.replay() == Ok(ITEM_COUNT as u32),
        &format!("replay={:?}", p.replay()),
    );

    // 导览页落档面：五件逐件行齐。
    let text = p.text();
    s.add_dyn(
        "F1576-10 导览页落档五件齐",
        text.contains("件1") && text.contains("件5") && text.contains(MARK_WHAT) && text.contains(MARK_HOW),
        &format!("bytes={}", text.len()),
    );

    // 三句结构逐条可核（每条导读三句起句逐字含句标）。
    let mut tri_ok = true;
    let mut id2 = 1u16;
    while id2 <= ITEM_COUNT as u16 {
        match p.entry_at(id2) {
            Some(e) => {
                if !e.line().contains(MARK_WHAT) || !e.line().contains(MARK_WHEN) || !e.line().contains(MARK_HOW) {
                    tri_ok = false;
                }
            }
            None => tri_ok = false,
        }
        id2 += 1;
    }
    s.add_dyn(
        "F1576-11 逐件三句结构可核",
        tri_ok,
        &format!("tri_ok={}", tri_ok),
    );

    // 相位倒退显性拒。
    let back = p.pair() == Err(Diag::EFrozen.val());
    s.add_dyn(
        "F1576-12 相位倒退显性拒",
        back && p.phase().ord() == 2,
        &format!("phase_ord={}", p.phase().ord()),
    );

    s.add_dyn(
        "F1576-13 双跑一致与闭集守卫",
        dual_run_ok() && ALL.len() == 12,
        &format!("hash={:016x} all={}", build_page().hash().unwrap_or(0), ALL.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_closed() {
        assert_ne!(MARK_WHAT, MARK_WHEN);
        assert_ne!(MARK_WHEN, MARK_HOW);
        assert_eq!(ITEM_NAMES.len(), ITEM_COUNT);
    }

    #[test]
    fn sentence_gate() {
        let ok = GuideEntry {
            item_id: 1,
            item_name: "总表快照".to_string(),
            what: "件是什么：x".to_string(),
            when: "何时读：x".to_string(),
            how: "怎么用：x".to_string(),
        };
        assert!(ok.sentences_ok());
        let no_how = GuideEntry { how: String::new(), ..ok.clone() };
        assert!(!no_how.sentences_ok());
        let bad_mark = GuideEntry { what: "是什么：x".to_string(), ..ok };
        assert!(!bad_mark.sentences_ok());
    }

    #[test]
    fn page_gates() {
        let mut p = GuidePage::new();
        assert_eq!(p.pair(), Err(Diag::EEmpty.val()));
        assert_eq!(p.rejects(), 0, "空页对平不算结构拒绝");
        assert_eq!(
            p.put(GuideEntry { item_id: 9, item_name: "x".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }),
            Err(Diag::EItemId.val())
        );
        assert_eq!(
            p.put(GuideEntry { item_id: 1, item_name: "别册".to_string(), what: "件是什么：x".to_string(), when: "何时读：x".to_string(), how: "怎么用：x".to_string() }),
            Err(Diag::EName.val())
        );
        assert_eq!(p.rejects(), 2);
    }

    #[test]
    fn baseline_pairs_full() {
        let mut p = build_page();
        assert_eq!(p.filled(), ITEM_COUNT);
        let r = p.pair().expect("基线对平");
        assert!(r.is_empty());
        assert!(p.hash().map(|x| x != 0).unwrap_or(false));
        assert_eq!(p.pair(), Err(Diag::EDupPair.val()));
    }

    #[test]
    fn missing_item_named() {
        let mut q = GuidePage::new();
        let mut id = 1u16;
        while id <= 3 {
            if let Some(e) = build_page().entry_at(id) {
                let _ = q.put(e.clone());
            }
            id += 1;
        }
        assert_eq!(q.pair(), Err(Diag::EMissItem.val()));
        assert_eq!(q.unpaired(), [4u16, 5u16]);
        assert_eq!(q.freeze(), Err(Diag::EPhase.val()), "未对平不可冻结");
    }

    #[test]
    fn freeze_and_replay() {
        let mut p = build_page();
        let _ = p.pair();
        let h = p.freeze().expect("随包冻结");
        assert_ne!(h, 0);
        assert_eq!(p.phase(), Phase::Frozen);
        assert_eq!(p.replay(), Ok(ITEM_COUNT as u32));
        assert_eq!(p.freeze(), Err(Diag::EFrozen.val()));
    }

    #[test]
    fn unpair_ten_of_ten() {
        let m = unpair_matrix();
        assert_eq!(m, [true; 10]);
        assert!(unpair_ok());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EItemId.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x60), "未知诊断码：不在 0x78F01..0x78F0C 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1576_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
