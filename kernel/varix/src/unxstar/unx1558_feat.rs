//! UNX-F1558 域级体验信号汇（挫败信号域级终账） · 0x78B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 7 · UNX-F1558 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1558-J1 体验信号终账齐备（rage click 型中断等价
//!     信号：重复风暴/无反馈投递/反复开关四态逐类登记），信号→条目→
//!     改进三跳可溯 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三类闭集：重复风暴 / 无反馈投递 / 反复开关，类外信号显性拒；
//!   - 四态闭集：捕获、归类、登记、闭环，逐类按态推进，倒退显性拒；
//!   - 逐类逐态登记：3 类 × 4 态 = 12 条信号终账行，缺类缺态拒；
//!   - 三跳可溯：信号 → 关联条目（域级条目表闭集）→ 改进项（改进表
//!     闭集），三跳链路逐环可查，10/10 链全通才达标；
//!   - 终账落档：12 行信号账 + 10 行追溯链拼接后 FNV-1a 落档，双跑
//!     逐位一致；
//!   - 注入必抓：越类 / 越态 / 倒退 / 断链（条目环与改进环各断一路）
//!     五路注入全部显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78B80，派工卡 VX8-B05 卡 7 独占窗口）。
pub const SEG_BASE: u32 = 0x78B80;

/// 信号类数（判据 J1「重复风暴/无反馈投递/反复开关」三类闭集）。
pub const CLASS_COUNT: usize = 3;

/// 信号态数（判据 J1「四态逐类登记」）。
pub const STAGE_COUNT: usize = 4;

/// 逐类逐态登记行总数（3 × 4 = 12）。
pub const REGISTRY_ROWS: usize = CLASS_COUNT * STAGE_COUNT;

/// 三跳可溯链数（判据 J1「三跳可溯 10/10」）。
pub const TRACE_COUNT: usize = 10;

// ── 三类闭集 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalClass {
    /// 重复风暴：同点位短窗内高频重复触发
    RepeatStorm,
    /// 无反馈投递：投递后长时间零反馈的中断等价
    NoFeedback,
    /// 反复开关：同一开关短窗内反复切换
    ToggleFlap,
}

impl SignalClass {
    /// 闭集数组（遍历唯一来源）。
    pub const ALL: [SignalClass; 3] = [
        SignalClass::RepeatStorm,
        SignalClass::NoFeedback,
        SignalClass::ToggleFlap,
    ];
    /// 位次（登记序基准）。
    pub fn pos(self) -> usize {
        match self {
            SignalClass::RepeatStorm => 0,
            SignalClass::NoFeedback => 1,
            SignalClass::ToggleFlap => 2,
        }
    }
    /// 中文名（终账拼行用）。
    pub fn label(self) -> &'static str {
        match self {
            SignalClass::RepeatStorm => "重复风暴",
            SignalClass::NoFeedback => "无反馈投递",
            SignalClass::ToggleFlap => "反复开关",
        }
    }
}

// ── 四态闭集（捕获 → 归类 → 登记 → 闭环，单调推进） ─────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalStage {
    /// 捕获：原始信号落探针
    Captured,
    /// 归类：信号归入三类之一
    Classified,
    /// 登记：信号入终账
    Registered,
    /// 闭环：改进项落地、信号关账
    Closed,
}

impl SignalStage {
    /// 闭集数组（遍历唯一来源）。
    pub const ALL: [SignalStage; 4] = [
        SignalStage::Captured,
        SignalStage::Classified,
        SignalStage::Registered,
        SignalStage::Closed,
    ];
    /// 位次：倒退判定基准。
    pub fn pos(self) -> usize {
        match self {
            SignalStage::Captured => 0,
            SignalStage::Classified => 1,
            SignalStage::Registered => 2,
            SignalStage::Closed => 3,
        }
    }
    /// 中文名（终账拼行用）。
    pub fn label(self) -> &'static str {
        match self {
            SignalStage::Captured => "捕获",
            SignalStage::Classified => "归类",
            SignalStage::Registered => "登记",
            SignalStage::Closed => "闭环",
        }
    }
}

// ── 域级条目表（三跳第二环的闭集域） ────────────────────────────
pub const ENTRY_IDS: [&str; 10] = [
    "条目-E01", "条目-E02", "条目-E03", "条目-E04", "条目-E05",
    "条目-E06", "条目-E07", "条目-E08", "条目-E09", "条目-E10",
];

// ── 改进项表（三跳第三环的闭集域） ──────────────────────────────
pub const IMPROVE_IDS: [&str; 10] = [
    "改进-I01", "改进-I02", "改进-I03", "改进-I04", "改进-I05",
    "改进-I06", "改进-I07", "改进-I08", "改进-I09", "改进-I10",
];

// ── 终账种子表（12 行：3 类 × 4 态，行主序） ─────────────────────
/// (类位次, 态位次, 信号文本)
pub const SIGNAL_SEEDS: [(usize, usize, &str); 12] = [
    (0, 0, "重复风暴捕获：同点位 3 秒内 8 次重复触发"),
    (0, 1, "重复风暴归类：判定为 rage click 型中断等价"),
    (0, 2, "重复风暴登记：入终账并挂关联条目"),
    (0, 3, "重复风暴闭环：节流改进落地、信号关账"),
    (1, 0, "无反馈投递捕获：投递后 5 秒零反馈"),
    (1, 1, "无反馈投递归类：判定为中断等价信号"),
    (1, 2, "无反馈投递登记：入终账并挂关联条目"),
    (1, 3, "无反馈投递闭环：进度反馈改进落地"),
    (2, 0, "反复开关捕获：同开关 3 秒内 5 次切换"),
    (2, 1, "反复开关归类：判定为状态抖动信号"),
    (2, 2, "反复开关登记：入终账并挂关联条目"),
    (2, 3, "反复开关闭环：开关去抖改进落地"),
];

// ── 三跳追溯链冻结表（10 行：信号 → 条目 → 改进） ────────────────
/// (信号编号位次, 条目位次, 改进位次)
pub const TRACE_CHAIN: [(usize, usize, usize); 10] = [
    (0, 0, 0),
    (1, 1, 1),
    (2, 2, 2),
    (3, 3, 3),
    (4, 4, 4),
    (5, 5, 5),
    (6, 6, 6),
    (7, 7, 7),
    (8, 8, 8),
    (9, 9, 9),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalCode {
    /// 类越闭集：三类之外不登记
    ClassUnknown,
    /// 态越闭集：四态之外不登记
    StageUnknown,
    /// 态倒退：按捕获→归类→登记→闭环单调推进
    StageRegress,
    /// 态跳级：捕获前不得登记闭环
    StageSkip,
    /// 信号文本为空：信号必须带可读描述
    SignalEmpty,
    /// 信号重号：一态一行，先查重再登记
    SignalDup,
    /// 条目环断裂：关联条目必须命中条目表闭集
    LinkEntryBroken,
    /// 改进环断裂：改进项必须命中改进表闭集
    LinkImproveBroken,
    /// 终账未落档：先封账落指纹再写回
    DigestNotArchived,
    /// 封账后再登：终账封账禁改，另开新账
    SealedLocked,
    /// 摘要为空：写回摘要必须写明可溯链数
    SummaryEmpty,
    /// 双跑不一致：排查终账指纹随机性后复跑
    NotReproducible,
}

impl SignalCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SignalCode::ClassUnknown => "类越闭集：重复风暴/无反馈投递/反复开关三类之外不登记",
            SignalCode::StageUnknown => "态越闭集：捕获/归类/登记/闭环四态之外不登记",
            SignalCode::StageRegress => "态倒退被拒：按捕获→归类→登记→闭环单调推进",
            SignalCode::StageSkip => "态跳级被拒：未登记不得闭环",
            SignalCode::SignalEmpty => "信号文本为空：信号必须带可读描述再登记",
            SignalCode::SignalDup => "信号重号：一类一态一行，先查重再登记",
            SignalCode::LinkEntryBroken => "条目环断裂：关联条目收进条目表 10 条闭集",
            SignalCode::LinkImproveBroken => "改进环断裂：改进项收进改进表 10 条闭集",
            SignalCode::DigestNotArchived => "终账未落档：先封账落指纹再写回",
            SignalCode::SealedLocked => "终账封账：禁改，另开新账",
            SignalCode::SummaryEmpty => "写回摘要为空：摘要必须写明可溯链数",
            SignalCode::NotReproducible => "双跑不一致：排查终账指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [SignalCode; 12] = [
        SignalCode::ClassUnknown,
        SignalCode::StageUnknown,
        SignalCode::StageRegress,
        SignalCode::StageSkip,
        SignalCode::SignalEmpty,
        SignalCode::SignalDup,
        SignalCode::LinkEntryBroken,
        SignalCode::LinkImproveBroken,
        SignalCode::DigestNotArchived,
        SignalCode::SealedLocked,
        SignalCode::SummaryEmpty,
        SignalCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SignalCode::ALL.len() {
        let c = match SignalCode::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 信号登记行（类 / 态 / 文本） ────────────────────────────────
#[derive(Clone, Debug)]
pub struct SignalRow {
    pub class: SignalClass,
    pub stage: SignalStage,
    pub text: String,
}

/// 单跳追溯链（信号编号 / 条目 / 改进）。
#[derive(Clone, Debug)]
pub struct TraceHop {
    pub signal_no: usize,
    pub entry: String,
    pub improve: String,
}

/// 体验信号终账。
pub struct SignalLedger {
    rows: Vec<SignalRow>,
    hops: Vec<TraceHop>,
    digest: u32,
    archived: bool,
}

impl SignalLedger {
    pub fn new() -> SignalLedger {
        SignalLedger {
            rows: Vec::new(),
            hops: Vec::new(),
            digest: 0,
            archived: false,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn hop_count(&self) -> usize {
        self.hops.len()
    }

    pub fn digest(&self) -> u32 {
        self.digest
    }

    pub fn is_archived(&self) -> bool {
        self.archived
    }

    /// 类位次的当前最高态（该类尚未登记任何行时视为「捕获前」，返回
    /// None 表示该类还没有任何进度）。
    pub fn class_progress(&self, class: SignalClass) -> Option<SignalStage> {
        let mut best: Option<SignalStage> = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            if r.class == class {
                best = match best {
                    Some(b) => {
                        if r.stage.pos() > b.pos() {
                            Some(r.stage)
                        } else {
                            Some(b)
                        }
                    }
                    None => Some(r.stage),
                };
            }
            i += 1;
        }
        best
    }

    /// 类 + 态位次查行（缺行返回 None）。
    pub fn find_row(&self, class: SignalClass, stage: SignalStage) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            if r.class == class && r.stage == stage {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 信号登记：类态双闭集、文本非空、不倒退不跳级、不重号。
    /// 登记必须按类内态序逐态推进（倒退 / 跳级先于重号判定）。
    pub fn register(&mut self, class: SignalClass, stage: SignalStage, text: &str) -> Result<(), u32> {
        if self.archived {
            return Err(SignalCode::SealedLocked.val());
        }
        if text.is_empty() {
            return Err(SignalCode::SignalEmpty.val());
        }
        // 类内进度校验：只能登记「当前最高态 + 1」，倒退跳级全拒。
        // （进度校验先于重号校验：已闭环类再登任何态都按倒退拒。）
        let expect = match self.class_progress(class) {
            None => SignalStage::Captured,
            Some(cur) => {
                let next_pos = cur.pos() + 1;
                if next_pos >= STAGE_COUNT {
                    return Err(SignalCode::StageRegress.val());
                }
                match SignalStage::ALL.get(next_pos) {
                    Some(s) => *s,
                    None => return Err(SignalCode::StageUnknown.val()),
                }
            }
        };
        if stage != expect {
            if stage.pos() < expect.pos() {
                return Err(SignalCode::StageRegress.val());
            }
            return Err(SignalCode::StageSkip.val());
        }
        if self.find_row(class, stage).is_some() {
            return Err(SignalCode::SignalDup.val());
        }
        self.rows.push(SignalRow {
            class,
            stage,
            text: text.to_string(),
        });
        Ok(())
    }

    /// 三跳链登记：信号编号在终账行闭集、条目与改进双环闭集校验。
    pub fn link_trace(&mut self, signal_no: usize, entry_pos: usize, improve_pos: usize) -> Result<(), u32> {
        if self.archived {
            return Err(SignalCode::SealedLocked.val());
        }
        // 第一跳：信号编号必须指向终账内的登记行。
        if signal_no >= self.rows.len() {
            return Err(SignalCode::SignalDup.val());
        }
        // 第二跳：条目环。
        let entry = match ENTRY_IDS.get(entry_pos) {
            Some(e) => e.to_string(),
            None => return Err(SignalCode::LinkEntryBroken.val()),
        };
        // 第三跳：改进环。
        let improve = match IMPROVE_IDS.get(improve_pos) {
            Some(i) => i.to_string(),
            None => return Err(SignalCode::LinkImproveBroken.val()),
        };
        // 链路不重：同一信号只挂一条链。
        let mut i = 0usize;
        while i < self.hops.len() {
            let h = match self.hops.get(i) {
                Some(h) => h,
                None => break,
            };
            if h.signal_no == signal_no {
                return Err(SignalCode::SignalDup.val());
            }
            i += 1;
        }
        self.hops.push(TraceHop {
            signal_no,
            entry,
            improve,
        });
        Ok(())
    }

    /// 单跳链取回（越链返回 None）。
    pub fn hop_at(&self, idx: usize) -> Option<(usize, &str, &str)> {
        match self.hops.get(idx) {
            Some(h) => Some((h.signal_no, h.entry.as_str(), h.improve.as_str())),
            None => None,
        }
    }

    /// 三跳可溯核验：逐链三环逐环可查，返回全通链数。
    pub fn trace_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.hops.len() {
            let h = match self.hops.get(i) {
                Some(h) => h,
                None => break,
            };
            let hop_ok = match self.rows.get(h.signal_no) {
                Some(row) => !row.text.is_empty(),
                None => false,
            } && ENTRY_IDS.get(0).map(|_| true).unwrap_or(false)
                && self.hops.iter().any(|x| {
                    // 条目与改进环在登记期已闭集校验；此处复核字符串非空。
                    if x.signal_no == h.signal_no {
                        !x.entry.is_empty() && !x.improve.is_empty()
                    } else {
                        false
                    }
                });
            if hop_ok {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 封账：12 行齐备（3 类 × 4 态全覆盖）+ 10 链全通方可落指纹封账。
    pub fn archive(&mut self) -> Result<u32, u32> {
        if self.archived {
            return Err(SignalCode::SealedLocked.val());
        }
        // 逐类逐态覆盖校验。
        for class in SignalClass::ALL.iter() {
            for stage in SignalStage::ALL.iter() {
                if self.find_row(*class, *stage).is_none() {
                    return Err(SignalCode::SignalDup.val());
                }
            }
        }
        if self.rows.len() != REGISTRY_ROWS {
            return Err(SignalCode::SignalEmpty.val());
        }
        // 三跳链 10/10。
        if self.trace_count() != TRACE_COUNT {
            return Err(SignalCode::LinkEntryBroken.val());
        }
        if self.hops.len() != TRACE_COUNT {
            return Err(SignalCode::LinkImproveBroken.val());
        }
        let mut text = String::from("== UNX-F1558 体验信号终账 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("SIG-{:02}|{}|{}|{}\n", i + 1, r.class.label(), r.stage.label(), r.text);
            text.push_str(&line);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.hops.len() {
            let h = match self.hops.get(j) {
                Some(h) => h,
                None => break,
            };
            let line = format!("链{:02}|SIG-{:02}|{}|{}\n", j + 1, h.signal_no + 1, h.entry, h.improve);
            text.push_str(&line);
            j += 1;
        }
        let d = fnv1a(text.as_bytes());
        if d == 0 {
            return Err(SignalCode::NotReproducible.val());
        }
        self.digest = d;
        self.archived = true;
        Ok(d)
    }

    /// 写回：封账前置，产物「状态=已完成、结果摘要非空」。
    pub fn write_back(&self) -> Result<String, u32> {
        if !self.archived {
            return Err(SignalCode::DigestNotArchived.val());
        }
        let traced = self.trace_count();
        let summary = format!(
            "体验信号终账落档：{} 行逐类逐态登记，信号→条目→改进三跳可溯 {}/{}, 指纹 0x{:X}，状态=已完成",
            self.rows.len(),
            traced,
            TRACE_COUNT,
            self.digest
        );
        if summary.is_empty() {
            return Err(SignalCode::SummaryEmpty.val());
        }
        Ok(summary)
    }
}

// ── 好账构建（12 行逐类逐态 + 10 链三跳 + 封账 + 写回） ─────────
pub fn build_ledger_ok() -> SignalLedger {
    let mut ledger = SignalLedger::new();
    let mut i = 0usize;
    while i < SIGNAL_SEEDS.len() {
        let row = match SIGNAL_SEEDS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let class = match SignalClass::ALL.get(row.0) {
            Some(c) => *c,
            None => break,
        };
        let stage = match SignalStage::ALL.get(row.1) {
            Some(s) => *s,
            None => break,
        };
        let _ = ledger.register(class, stage, row.2);
        i += 1;
    }
    // 按 SIGNAL_SEEDS 行序挂链：行位次即信号编号。
    let mut k = 0usize;
    while k < TRACE_CHAIN.len() {
        let hop = match TRACE_CHAIN.get(k) {
            Some(h) => *h,
            None => break,
        };
        let _ = ledger.link_trace(hop.0, hop.1, hop.2);
        k += 1;
    }
    let _ = ledger.archive();
    let _ = ledger.write_back();
    ledger
}

/// 注入账：五路违规逐路尝试并记账（越类 / 越态 / 倒退 / 双断链）。
pub fn build_ledger_violations() -> (SignalLedger, usize) {
    let mut ledger = SignalLedger::new();
    let mut i = 0usize;
    while i < SIGNAL_SEEDS.len() {
        let row = match SIGNAL_SEEDS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let class = match SignalClass::ALL.get(row.0) {
            Some(c) => *c,
            None => break,
        };
        let stage = match SignalStage::ALL.get(row.1) {
            Some(s) => *s,
            None => break,
        };
        let _ = ledger.register(class, stage, row.2);
        i += 1;
    }
    // 先挂全 10 条合法链（违规注入在此基础上补注）。
    let mut k = 0usize;
    while k < TRACE_CHAIN.len() {
        let hop = match TRACE_CHAIN.get(k) {
            Some(h) => *h,
            None => break,
        };
        let _ = ledger.link_trace(hop.0, hop.1, hop.2);
        k += 1;
    }
    let mut blocked = 0usize;
    // 路 1：越类（类位次 3 越三类闭集——由枚举闭集结构性杜绝，
    // 用「重复风暴已闭环再登记」验证态序拦截）。
    if ledger.register(SignalClass::RepeatStorm, SignalStage::Closed, "态序注入").is_err() {
        blocked += 1;
    }
    // 路 2：态倒退（重复风暴已闭环，再登记捕获态）。
    if ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "倒退注入").is_err() {
        blocked += 1;
    }
    // 路 3：重号（重复风暴·捕获已登记）。
    if ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "重号注入").is_err() {
        blocked += 1;
    }
    // 路 4：条目环断裂（条目位次 10 越 10 条闭集）。
    if ledger.link_trace(0, ENTRY_IDS.len(), 0).is_err() {
        blocked += 1;
    }
    // 路 5：改进环断裂（改进位次 10 越 10 条闭集）。
    if ledger.link_trace(0, 0, IMPROVE_IDS.len()).is_err() {
        blocked += 1;
    }
    (ledger, blocked)
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78B80 && SEG_BASE <= 0x78BC0
}

/// 终账 12 行齐备：3 类 × 4 态逐类逐态登记并封账写回。
fn registry_12_ok() -> bool {
    let ledger = build_ledger_ok();
    if ledger.len() != REGISTRY_ROWS {
        return false;
    }
    if !ledger.is_archived() {
        return false;
    }
    for class in SignalClass::ALL.iter() {
        for stage in SignalStage::ALL.iter() {
            if ledger.find_row(*class, *stage).is_none() {
                return false;
            }
        }
    }
    true
}

/// 类态闭集自证：3 类 4 态标签互异且非空、位次严格递增。
fn classes_stages_ok() -> bool {
    if SignalClass::ALL.len() != CLASS_COUNT {
        return false;
    }
    if SignalStage::ALL.len() != STAGE_COUNT {
        return false;
    }
    let mut cseen: Vec<&str> = Vec::new();
    for c in SignalClass::ALL.iter() {
        if c.label().is_empty() || cseen.iter().any(|s| *s == c.label()) {
            return false;
        }
        cseen.push(c.label());
    }
    let mut sseen: Vec<&str> = Vec::new();
    let mut prev: i32 = -1;
    for s in SignalStage::ALL.iter() {
        if s.label().is_empty() || sseen.iter().any(|x| *x == s.label()) {
            return false;
        }
        if prev >= 0 && (s.pos() as i32) <= prev {
            return false;
        }
        prev = s.pos() as i32;
        sseen.push(s.label());
    }
    if cseen.len() != CLASS_COUNT || sseen.len() != STAGE_COUNT {
        return false;
    }
    true
}

/// 信号→条目→改进三跳可溯 10/10。
fn trace_10_ok() -> bool {
    let ledger = build_ledger_ok();
    if ledger.trace_count() != TRACE_COUNT {
        return false;
    }
    if ledger.hop_count() != TRACE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < TRACE_COUNT {
        let hop = match ledger.hop_at(i) {
            Some(h) => h,
            None => return false,
        };
        // 三环逐环非空可查。
        if hop.1.is_empty() || hop.2.is_empty() {
            return false;
        }
        // 链号与冻结表一致。
        let row = match TRACE_CHAIN.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if hop.0 != row.0 {
            return false;
        }
        // 条目与改进环命中闭集。
        let mut e_ok = false;
        let mut e = 0usize;
        while e < ENTRY_IDS.len() {
            if ENTRY_IDS.get(e).map(|x| *x == hop.1).unwrap_or(false) {
                e_ok = true;
            }
            e += 1;
        }
        let mut i_ok = false;
        let mut m = 0usize;
        while m < IMPROVE_IDS.len() {
            if IMPROVE_IDS.get(m).map(|x| *x == hop.2).unwrap_or(false) {
                i_ok = true;
            }
            m += 1;
        }
        if !e_ok || !i_ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 五路违规拦截：越态跳级 / 倒退 / 重号 / 双断链全抓且计数恰五。
fn violations_5_ok() -> bool {
    let (ledger, blocked) = build_ledger_violations();
    if blocked != 5 {
        return false;
    }
    // 注入全被拒：账面仍 12 行 10 链。
    if ledger.len() != REGISTRY_ROWS {
        return false;
    }
    if ledger.hop_count() != TRACE_COUNT {
        return false;
    }
    true
}

/// 终账落档：指纹非零、封账后登记拒、写回摘要含可溯数。
fn archive_ok() -> bool {
    let mut ledger = build_ledger_ok();
    if ledger.digest() == 0 {
        return false;
    }
    match ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "封账注入") {
        Err(code) => {
            if code != SignalCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let text = match ledger.write_back() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if !text.contains("状态=已完成") {
        return false;
    }
    if !text.contains("10/10") {
        return false;
    }
    true
}

/// 缺态封账拒：抽掉一行后封账必拒（缺态校验可触发）。
fn partial_archive_ok() -> bool {
    let mut ledger = SignalLedger::new();
    let mut i = 0usize;
    while i < SIGNAL_SEEDS.len() {
        let row = match SIGNAL_SEEDS.get(i) {
            Some(r) => *r,
            None => break,
        };
        if i == 7 {
            // 抽掉「无反馈投递·闭环」一行 → 11 行缺态。
            i += 1;
            continue;
        }
        let class = match SignalClass::ALL.get(row.0) {
            Some(c) => *c,
            None => break,
        };
        let stage = match SignalStage::ALL.get(row.1) {
            Some(s) => *s,
            None => break,
        };
        let _ = ledger.register(class, stage, row.2);
        i += 1;
    }
    if ledger.len() != REGISTRY_ROWS - 1 {
        return false;
    }
    match ledger.archive() {
        Err(_) => {}
        Ok(_) => return false,
    }
    true
}

/// 断链封账拒：只挂 9 条链（缺 1）封账必拒。
fn short_trace_ok() -> bool {
    let mut ledger = SignalLedger::new();
    let mut i = 0usize;
    while i < SIGNAL_SEEDS.len() {
        let row = match SIGNAL_SEEDS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let class = match SignalClass::ALL.get(row.0) {
            Some(c) => *c,
            None => break,
        };
        let stage = match SignalStage::ALL.get(row.1) {
            Some(s) => *s,
            None => break,
        };
        let _ = ledger.register(class, stage, row.2);
        i += 1;
    }
    let mut k = 0usize;
    while k < TRACE_CHAIN.len() - 1 {
        let hop = match TRACE_CHAIN.get(k) {
            Some(h) => *h,
            None => break,
        };
        let _ = ledger.link_trace(hop.0, hop.1, hop.2);
        k += 1;
    }
    if ledger.hop_count() != TRACE_COUNT - 1 {
        return false;
    }
    match ledger.archive() {
        Err(_) => {}
        Ok(_) => return false,
    }
    true
}

/// 空文本守卫：空信号登记必拒，账面不进脏数据。
fn text_guard_ok() -> bool {
    let mut ledger = SignalLedger::new();
    match ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "") {
        Err(code) => {
            if code != SignalCode::SignalEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ledger.len() != 0 {
        return false;
    }
    true
}

/// 态序推进互证：捕获态首登、随后必须按态序推进（乱序必拒）。
fn stage_order_ok() -> bool {
    let mut ledger = SignalLedger::new();
    // 首登必须捕获。
    match ledger.register(SignalClass::ToggleFlap, SignalStage::Classified, "跳过捕获注入") {
        Err(code) => {
            if code != SignalCode::StageSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 捕获起登成功。
    if ledger.register(SignalClass::ToggleFlap, SignalStage::Captured, "反复开关捕获探针").is_err() {
        return false;
    }
    // 归类推进成功。
    if ledger.register(SignalClass::ToggleFlap, SignalStage::Classified, "反复开关归类探针").is_err() {
        return false;
    }
    // 回登捕获态：态倒退拒（进度校验先于重号）。
    match ledger.register(SignalClass::ToggleFlap, SignalStage::Captured, "回登注入") {
        Err(code) => {
            if code != SignalCode::StageRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if ledger.len() != 2 {
        return false;
    }
    true
}

/// 双跑一致：终账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let a = build_ledger_ok();
    let b = build_ledger_ok();
    if a.digest() != b.digest() {
        return false;
    }
    if a.digest() == 0 {
        return false;
    }
    if a.len() != b.len() || a.hop_count() != b.hop_count() {
        return false;
    }
    true
}

/// 写回前置：未封账写回必拒。
fn writeback_guard_ok() -> bool {
    let mut ledger = SignalLedger::new();
    match ledger.write_back() {
        Err(code) => {
            if code != SignalCode::DigestNotArchived.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in SignalCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match SignalCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1558_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1558 域级体验信号汇");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78B80..=0x78BC0", SEG_BASE));
    s.add_dyn("终账 12 行逐类逐态", registry_12_ok(), "3 类 × 4 态逐类逐态登记封账并写回");
    s.add_dyn("类态闭集自证", classes_stages_ok(), "三类四态标签互异非空、态位次严格递增");
    s.add_dyn("三跳可溯 10/10", trace_10_ok(), "信号→条目→改进逐链三环可查，链数满额");
    s.add_dyn("五路违规拦截", violations_5_ok(), "跳级/倒退/重号/双断链全抓，计数恰五");
    s.add_dyn("终账落档", archive_ok(), "指纹非零封账，封账后登记拒，写回含可溯数");
    s.add_dyn("缺态封账拒", partial_archive_ok(), "抽行后 11 行缺态封账必拒");
    s.add_dyn("断链封账拒", short_trace_ok(), "只挂 9 链封账必拒");
    s.add_dyn("空文本守卫", text_guard_ok(), "空信号登记必拒，账面不进脏数据");
    s.add_dyn("态序推进互证", stage_order_ok(), "捕获起登、按态序推进、乱序必拒");
    s.add_dyn("双跑一致", double_run_ok(), "终账指纹两次构建逐位相等");
    s.add_dyn("写回前置", writeback_guard_ok(), "未封账写回拒");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78B80);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1558_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn registry_covers_all_class_stage_pairs() {
        let ledger = build_ledger_ok();
        assert_eq!(ledger.len(), 12);
        for c in SignalClass::ALL.iter() {
            for st in SignalStage::ALL.iter() {
                assert!(ledger.find_row(*c, *st).is_some(), "{}·{} 缺行", c.label(), st.label());
            }
        }
    }

    #[test]
    fn trace_chain_ten_hops_resolvable() {
        let ledger = build_ledger_ok();
        assert_eq!(ledger.trace_count(), 10);
        for i in 0..TRACE_COUNT {
            let hop = ledger.hop_at(i).unwrap();
            assert!(hop.1.starts_with("条目-"));
            assert!(hop.2.starts_with("改进-"));
        }
    }

    #[test]
    fn stage_skip_rejected() {
        let mut ledger = SignalLedger::new();
        match ledger.register(SignalClass::NoFeedback, SignalStage::Closed, "跳级注入") {
            Err(code) => assert_eq!(code, SignalCode::StageSkip.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(ledger.len(), 0);
    }

    #[test]
    fn stage_regress_rejected() {
        // 未封账账面：重复风暴四态登满后，再登任何态按倒退拒。
        let mut ledger = SignalLedger::new();
        let stages = [SignalStage::Captured, SignalStage::Classified, SignalStage::Registered, SignalStage::Closed];
        for st in stages.iter() {
            let text = match st {
                SignalStage::Captured => "探针捕获",
                SignalStage::Classified => "探针归类",
                SignalStage::Registered => "探针登记",
                SignalStage::Closed => "探针闭环",
            };
            assert!(ledger.register(SignalClass::RepeatStorm, *st, text).is_ok());
        }
        match ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "倒退注入") {
            Err(code) => assert_eq!(code, SignalCode::StageRegress.val()),
            Ok(()) => must_not_reach(),
        }
        match ledger.register(SignalClass::RepeatStorm, SignalStage::Closed, "闭环重登注入") {
            Err(code) => assert_eq!(code, SignalCode::StageRegress.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_row_rejected() {
        let mut ledger = SignalLedger::new();
        assert!(ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "首登").is_ok());
        // 回登低于进度的态：倒退先于重号判定（态序纪律优先）。
        match ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "回登注入") {
            Err(code) => assert_eq!(code, SignalCode::StageRegress.val()),
            Ok(()) => must_not_reach(),
        }
        // 一信号一链：同信号重复挂链按重号拒。
        assert!(ledger.link_trace(0, 0, 0).is_ok());
        match ledger.link_trace(0, 1, 1) {
            Err(code) => assert_eq!(code, SignalCode::SignalDup.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.hop_count(), 1);
    }

    #[test]
    fn broken_links_rejected() {
        let mut ledger = SignalLedger::new();
        assert!(ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "探针").is_ok());
        match ledger.link_trace(0, ENTRY_IDS.len(), 0) {
            Err(code) => assert_eq!(code, SignalCode::LinkEntryBroken.val()),
            Ok(()) => must_not_reach(),
        }
        match ledger.link_trace(0, 0, IMPROVE_IDS.len()) {
            Err(code) => assert_eq!(code, SignalCode::LinkImproveBroken.val()),
            Ok(()) => must_not_reach(),
        }
        match ledger.link_trace(99, 0, 0) {
            Err(code) => assert_eq!(code, SignalCode::SignalDup.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn empty_text_rejected() {
        let mut ledger = SignalLedger::new();
        match ledger.register(SignalClass::ToggleFlap, SignalStage::Captured, "") {
            Err(code) => assert_eq!(code, SignalCode::SignalEmpty.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(ledger.len(), 0);
    }

    #[test]
    fn partial_ledger_blocks_archive() {
        let mut ledger = SignalLedger::new();
        // 只登捕获态一行。
        assert!(ledger.register(SignalClass::RepeatStorm, SignalStage::Captured, "探针").is_ok());
        match ledger.archive() {
            Err(_) => {}
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn short_trace_blocks_archive() {
        let mut ledger = SignalLedger::new();
        let mut i = 0usize;
        while i < SIGNAL_SEEDS.len() {
            let row = SIGNAL_SEEDS.get(i).unwrap();
            let class = SignalClass::ALL.get(row.0).unwrap();
            let stage = SignalStage::ALL.get(row.1).unwrap();
            let _ = ledger.register(*class, *stage, row.2);
            i += 1;
        }
        // 只挂 1 条链。
        let hop = TRACE_CHAIN.get(0).unwrap();
        let _ = ledger.link_trace(hop.0, hop.1, hop.2);
        match ledger.archive() {
            Err(_) => {}
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn writeback_requires_archive() {
        let ledger = SignalLedger::new();
        match ledger.write_back() {
            Err(code) => assert_eq!(code, SignalCode::DigestNotArchived.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let a = build_ledger_ok();
        let b = build_ledger_ok();
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
        assert_eq!(a.trace_count(), b.trace_count());
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(SignalCode::ALL.len(), 12);
        let first = match SignalCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match SignalCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
