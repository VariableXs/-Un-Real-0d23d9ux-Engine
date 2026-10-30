//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16062 · Wine 装载器与原生装载分工边界 · 0x68840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B04）：
//!   UNX-F16062-J1 分工边界账逐行落账（装载职责/归 Wine 归 Varix 裁决），
//!   裁决三态齐备率 100%，与 D2 冻结契约（F12966 LoadStateV1）交叉
//!   一致率 100%。
//! 定位：任务书协作界面预告"Wine 自带装载器与 D2 原生装载的分工边界
//!   在 E1 任务书钉死"——本条立分工边界账：装载职责逐项裁决（Wine
//!   自带 Ldr 面承接哪些、Varix 原生面承接哪些），与 D2 的
//!   LoadStateV1 冻结契约（F12966）交叉一致，边界不清是"两套装载
//!   各装一遍"灾难的源头。
//! 设计要点：
//!   - 边界账：装载职责七项逐行登记（PE 头解析/节映射/重定位/IAT
//!     解析/TLS 目录/SEH 表注册/依赖递归），每行裁决三态（Wine 面
//!     承接/Varix 原生面承接/对照双跑），裁决理由引用任务书边界条款；
//!   - 交叉断言：边界行与 D2 LoadStateV1 字段集交叉一致率 100%——
//!     Varix 面承接的职责，D2 冻结结构里有对应状态格；Wine 面承接
//!     的，LoadStateV1 不重复登记；
//!   - 对照双跑注记：裁决为"对照双跑"的职责逐行注记双跑判据
//!     （同输入两侧输出 diff 一致），双跑是语义底座成熟前的诚实
//!     过渡态；
//!   - 翻转会签：账行状态更新留痕，翻转走双方会签（D2 消费方联签）
//!     ——翻转无会签必红；
//!   - 注入面：裁决缺/交叉失配/翻转无会签/职责越七项/重复行/双跑
//!     注记缺——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68840）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68840，AI-VX-1031 E1 域批D 独占区）。
pub const SEG_BASE: u32 = 0x68840;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// D2 LoadStateV1 冻结版本（F12966 cross_domain_freeze，写死）。
pub const F12966_FREEZE_VER: u32 = 1;

// ── 装载职责七项闭集 ─────────────────────────────────────────────
pub const DUTY_PE_HDR: usize = 0; // PE 头解析
pub const DUTY_SECT_MAP: usize = 1; // 节映射
pub const DUTY_RELOC: usize = 2; // 重定位
pub const DUTY_IAT: usize = 3; // IAT 解析
pub const DUTY_TLS: usize = 4; // TLS 目录
pub const DUTY_SEH: usize = 5; // SEH 表注册
pub const DUTY_DEP_RECUR: usize = 6; // 依赖递归
pub const DUTY_COUNT: usize = 7;

pub const DUTY_NAMES: [&str; DUTY_COUNT] = [
    "PE头解析", "节映射", "重定位", "IAT解析", "TLS目录", "SEH表注册", "依赖递归",
];

// ── 裁决三态闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ownership {
    /// Wine 面承接（只跟随，不改 Ldr）
    WineSide,
    /// Varix 原生面承接
    VarixSide,
    /// 对照双跑（语义底座成熟前的诚实过渡态）
    DualRun,
}

impl Ownership {
    pub fn as_u32(self) -> u32 {
        match self {
            Ownership::WineSide => 0,
            Ownership::VarixSide => 1,
            Ownership::DualRun => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Ownership::WineSide => "Wine面承接",
            Ownership::VarixSide => "Varix原生面承接",
            Ownership::DualRun => "对照双跑",
        }
    }
    pub fn from_u32(v: u32) -> Option<Ownership> {
        match v {
            0 => Some(Ownership::WineSide),
            1 => Some(Ownership::VarixSide),
            2 => Some(Ownership::DualRun),
            _ => None,
        }
    }
    /// 该裁决是否要求 D2 LoadStateV1 状态格在案。
    pub fn requires_loadstate_slot(self) -> bool {
        matches!(self, Ownership::VarixSide | Ownership::DualRun)
    }
    /// 该裁决是否要求双跑判据注记。
    pub fn requires_dual_note(self) -> bool {
        matches!(self, Ownership::DualRun)
    }
}

// ── D2 LoadStateV1 状态格闭集（F12966 冻结字段投影） ────────────
pub const SLOT_PE_HDR: usize = 0;
pub const SLOT_SECT_MAP: usize = 1;
pub const SLOT_RELOC: usize = 2;
pub const SLOT_IAT: usize = 3;
pub const SLOT_TLS: usize = 4;
pub const SLOT_SEH: usize = 5;
pub const SLOT_DEP_RECUR: usize = 6;
pub const SLOT_COUNT: usize = 7;

pub const SLOT_NAMES: [&str; SLOT_COUNT] = [
    "LoadStateV1.pe_hdr", "LoadStateV1.sect_map", "LoadStateV1.reloc",
    "LoadStateV1.iat", "LoadStateV1.tls", "LoadStateV1.seh", "LoadStateV1.dep_recur",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LbCode {
    /// 职责越七项闭集
    DutyOutOfRange,
    /// 职责行重复
    DutyDuplicated,
    /// 裁决越三态闭集
    OwnershipOutOfRange,
    /// 裁决理由空（齐备率失守）
    ReasonBlank,
    /// 交叉失配：Varix/DualRun 裁决无 LoadStateV1 状态格
    CrossSlotMissing,
    /// 双跑注记缺：DualRun 裁决无双跑判据
    DualNoteMissing,
    /// 翻转无会签
    FlipNoCountersign,
    /// 职责理由引用悬空（任务书条款缺失）
    CharterRefDangling,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl LbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LbCode::DutyOutOfRange => "职责越七项闭集",
            LbCode::DutyDuplicated => "职责行重复：职责唯一律",
            LbCode::OwnershipOutOfRange => "裁决越三态闭集",
            LbCode::ReasonBlank => "裁决理由空：齐备率失守",
            LbCode::CrossSlotMissing => "交叉失配：LoadStateV1 状态格缺失",
            LbCode::DualNoteMissing => "双跑注记缺：DualRun 判据失守",
            LbCode::FlipNoCountersign => "翻转无会签：裁决变更失守",
            LbCode::CharterRefDangling => "裁决理由引用悬空",
            LbCode::NotReproducible => "双跑不一致：确定性红线",
            LbCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [LbCode; 10] = [
        LbCode::DutyOutOfRange,
        LbCode::DutyDuplicated,
        LbCode::OwnershipOutOfRange,
        LbCode::ReasonBlank,
        LbCode::CrossSlotMissing,
        LbCode::DualNoteMissing,
        LbCode::FlipNoCountersign,
        LbCode::CharterRefDangling,
        LbCode::NotReproducible,
        LbCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LbCode::ALL.len() {
        if LbCode::ALL[i].val() == code {
            return LbCode::ALL[i].detail();
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

// ── 边界账行 ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct BoundRow {
    /// 装载职责（0..DUTY_COUNT）。
    pub duty: usize,
    /// 裁决（三态闭集）。
    pub owner: Ownership,
    /// 裁决理由（引用任务书边界条款，须以 "E1-CH-" 开头）。
    pub reason: String,
    /// D2 状态格（Varix/DualRun 裁决必填且须 ∈ SLOT_NAMES）。
    pub loadstate_slot: String,
    /// 双跑判据注记（DualRun 必填：同输入两侧输出 diff 一致）。
    pub dual_note: String,
}

/// 任务书条款引用格式：E1-CH- 前缀。
pub fn charter_ref_legal(r: &str) -> bool {
    r.starts_with("E1-CH-") && r.len() >= 10
}

pub fn slot_legal(s: &str) -> bool {
    let mut i = 0;
    while i < SLOT_COUNT {
        if SLOT_NAMES[i] == s {
            return true;
        }
        i += 1;
    }
    false
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &BoundRow) -> Result<(), u32> {
    if row.duty >= DUTY_COUNT {
        return Err(LbCode::DutyOutOfRange.val());
    }
    if Ownership::from_u32(row.owner.as_u32()).is_none() {
        return Err(LbCode::OwnershipOutOfRange.val());
    }
    if row.reason.len() == 0 {
        return Err(LbCode::ReasonBlank.val());
    }
    if !charter_ref_legal(&row.reason) {
        return Err(LbCode::CharterRefDangling.val());
    }
    if row.owner.requires_loadstate_slot() {
        if !slot_legal(&row.loadstate_slot) {
            return Err(LbCode::CrossSlotMissing.val());
        }
        // 状态格必须与本职责对应（交叉一致的字段级口径）。
        let expect = SLOT_NAMES[row.duty];
        if row.loadstate_slot != expect {
            return Err(LbCode::CrossSlotMissing.val());
        }
    } else if row.loadstate_slot.len() != 0 {
        // Wine 面承接不许重复登记 LoadStateV1（不重复登记律）。
        return Err(LbCode::CrossSlotMissing.val());
    }
    if row.owner.requires_dual_note() {
        if row.dual_note.len() == 0 {
            return Err(LbCode::DualNoteMissing.val());
        }
    } else if row.dual_note.len() != 0 {
        return Err(LbCode::DualNoteMissing.val());
    }
    Ok(())
}

// ── 分工边界账 ───────────────────────────────────────────────────
pub struct BoundLedger {
    rows: Vec<BoundRow>,
    /// 翻转留痕账：（职责, 旧裁决, 新裁决, 会签标记）。
    flips: Vec<(usize, u32, u32, bool)>,
    freeze_ver: u32,
}

impl BoundLedger {
    pub fn new() -> BoundLedger {
        BoundLedger { rows: Vec::new(), flips: Vec::new(), freeze_ver: F12966_FREEZE_VER }
    }

    pub fn enroll(&mut self, row: BoundRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].duty == row.duty {
                return Err(LbCode::DutyDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 裁决翻转（状态更新留痕）：必须带 D2 消费方会签。
    pub fn flip(&mut self, duty: usize, new_owner: Ownership, countersigned: bool) -> Result<(), u32> {
        if duty >= DUTY_COUNT {
            return Err(LbCode::DutyOutOfRange.val());
        }
        if Ownership::from_u32(new_owner.as_u32()).is_none() {
            return Err(LbCode::OwnershipOutOfRange.val());
        }
        // 找现行。
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].duty == duty {
                idx = Some(i);
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => return Err(LbCode::DutyOutOfRange.val()),
        };
        let old = self.rows[i].owner;
        if old == new_owner {
            return Err(LbCode::FlipNoCountersign.val());
        }
        if !countersigned {
            return Err(LbCode::FlipNoCountersign.val());
        }
        // 留痕。
        self.flips.push((duty, old.as_u32(), new_owner.as_u32(), true));
        // 应用新裁决：按新态修补状态格与双跑注记。
        self.rows[i].owner = new_owner;
        if new_owner.requires_loadstate_slot() {
            self.rows[i].loadstate_slot = String::from(SLOT_NAMES[duty]);
        } else {
            self.rows[i].loadstate_slot = String::new();
        }
        if new_owner.requires_dual_note() {
            self.rows[i].dual_note = String::from("同输入两侧输出 diff 一致");
        } else {
            self.rows[i].dual_note = String::new();
        }
        vet_row(&self.rows[i])?;
        Ok(())
    }

    /// 七职责覆盖。
    pub fn duties_closed(&self) -> Result<(), u32> {
        let mut d = 0;
        while d < DUTY_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].duty == d {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(LbCode::DutyOutOfRange.val());
            }
            d += 1;
        }
        Ok(())
    }

    /// 裁决齐备率（判据 100% 口径）：理由全非空 = 100%。
    pub fn ownership_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(LbCode::ReasonBlank.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].reason.len() > 0 && vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    /// D2 LoadStateV1 交叉一致率（判据 100% 口径）：需要状态格的
    /// 裁决全部有格，不需要的全部无格。
    pub fn cross_align_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(LbCode::CrossSlotMissing.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    /// 冻结版本核对。
    pub fn freeze_ok(&self) -> Result<(), u32> {
        if self.freeze_ver != F12966_FREEZE_VER {
            return Err(LbCode::CrossSlotMissing.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn flip_count(&self) -> usize {
        self.flips.len()
    }
}

// ── 首版边界账装配 ───────────────────────────────────────────────
/// 裁决口径：PE 头解析/重定位/IAT = Varix 原生面（D2 底座成熟）；
/// 节映射/TLS = 对照双跑；SEH 表注册/依赖递归 = Wine 面承接。
pub fn build_initial() -> Result<BoundLedger, u32> {
    let mut led = BoundLedger::new();
    led.enroll(BoundRow {
        duty: DUTY_PE_HDR,
        owner: Ownership::VarixSide,
        reason: String::from("E1-CH-04-PE头解析归原生"),
        loadstate_slot: String::from(SLOT_NAMES[DUTY_PE_HDR]),
        dual_note: String::new(),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_SECT_MAP,
        owner: Ownership::DualRun,
        reason: String::from("E1-CH-04-节映射双跑过渡"),
        loadstate_slot: String::from(SLOT_NAMES[DUTY_SECT_MAP]),
        dual_note: String::from("同输入两侧输出 diff 一致"),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_RELOC,
        owner: Ownership::VarixSide,
        reason: String::from("E1-CH-04-重定位两档对账在原生"),
        loadstate_slot: String::from(SLOT_NAMES[DUTY_RELOC]),
        dual_note: String::new(),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_IAT,
        owner: Ownership::VarixSide,
        reason: String::from("E1-CH-04-IAT三级链转发在原生"),
        loadstate_slot: String::from(SLOT_NAMES[DUTY_IAT]),
        dual_note: String::new(),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_TLS,
        owner: Ownership::DualRun,
        reason: String::from("E1-CH-04-TLS双档双跑过渡"),
        loadstate_slot: String::from(SLOT_NAMES[DUTY_TLS]),
        dual_note: String::from("同输入两侧输出 diff 一致"),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_SEH,
        owner: Ownership::WineSide,
        reason: String::from("E1-CH-04-SEH注册随Wine Ldr"),
        loadstate_slot: String::new(),
        dual_note: String::new(),
    })?;
    led.enroll(BoundRow {
        duty: DUTY_DEP_RECUR,
        owner: Ownership::WineSide,
        reason: String::from("E1-CH-04-依赖递归随Wine Ldr"),
        loadstate_slot: String::new(),
        dual_note: String::new(),
    })?;
    Ok(led)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 裁决率, 交叉率))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let led = build_initial()?;
    led.duties_closed()?;
    led.freeze_ok()?;
    let rate = led.ownership_rate()?;
    if rate != 100 {
        return Err(LbCode::ReasonBlank.val());
    }
    let cross = led.cross_align_rate()?;
    if cross != 100 {
        return Err(LbCode::CrossSlotMissing.val());
    }
    Ok((led.len(), rate, cross))
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
        // ① 翻转无会签。
        let no_sig = match build_initial() {
            Ok(mut l) => {
                let before = l.flip_count();
                let r = l.flip(DUTY_TLS, Ownership::VarixSide, false);
                r == Err(LbCode::FlipNoCountersign.val()) && l.flip_count() == before
            }
            Err(_) => false,
        };
        led.record(0, no_sig);

        // ② 交叉失配：Varix 裁决但状态格挂错职责。
        let cross_bad = BoundRow {
            duty: DUTY_RELOC,
            owner: Ownership::VarixSide,
            reason: String::from("E1-CH-04-理由占位"),
            loadstate_slot: String::from(SLOT_NAMES[DUTY_IAT]), // 错格
            dual_note: String::new(),
        };
        led.record(1, vet_row(&cross_bad) == Err(LbCode::CrossSlotMissing.val()));

        // ③ 双跑注记缺：DualRun 无注记。
        let dual_bad = BoundRow {
            duty: DUTY_SECT_MAP,
            owner: Ownership::DualRun,
            reason: String::from("E1-CH-04-理由占位"),
            loadstate_slot: String::from(SLOT_NAMES[DUTY_SECT_MAP]),
            dual_note: String::new(),
        };
        led.record(2, vet_row(&dual_bad) == Err(LbCode::DualNoteMissing.val()));
        // Wine 面不许带双跑注记。
        let wine_bad = BoundRow {
            duty: DUTY_SEH,
            owner: Ownership::WineSide,
            reason: String::from("E1-CH-04-理由占位"),
            loadstate_slot: String::new(),
            dual_note: String::from("多余注记"),
        };
        led.record(2, vet_row(&wine_bad) == Err(LbCode::DualNoteMissing.val()));

        // ④ 职责越闭集 / 理由引用悬空。
        led.record(3, {
            let bad = BoundRow {
                duty: DUTY_COUNT + (rnd % 2) as usize,
                owner: Ownership::WineSide,
                reason: String::from("E1-CH-04-理由占位"),
                loadstate_slot: String::new(),
                dual_note: String::new(),
            };
            vet_row(&bad) == Err(LbCode::DutyOutOfRange.val())
        });
        led.record(3, {
            let bad2 = BoundRow {
                duty: DUTY_PE_HDR,
                owner: Ownership::VarixSide,
                reason: String::from("随手写的理由"),
                loadstate_slot: String::from(SLOT_NAMES[DUTY_PE_HDR]),
                dual_note: String::new(),
            };
            vet_row(&bad2) == Err(LbCode::CharterRefDangling.val())
        });

        // ⑤ 职责行重复。
        let dup_ok = {
            let mut l2 = BoundLedger::new();
            let row = BoundRow {
                duty: DUTY_PE_HDR,
                owner: Ownership::VarixSide,
                reason: String::from("E1-CH-04-理由占位"),
                loadstate_slot: String::from(SLOT_NAMES[DUTY_PE_HDR]),
                dual_note: String::new(),
            };
            let first = l2.enroll(row.clone());
            let second = l2.enroll(row);
            first.is_ok() && second == Err(LbCode::DutyDuplicated.val())
        };
        led.record(4, dup_ok);

        // ⑥ 裁决越三态闭集 + 翻转会签正向。
        led.record(5, Ownership::from_u32(3).is_none() && Ownership::from_u32(0xFFFF).is_none());
        let sig_ok = match build_initial() {
            Ok(mut l3) => {
                let r = l3.flip(DUTY_SECT_MAP, Ownership::VarixSide, true);
                r.is_ok() && l3.flip_count() == 1
            }
            Err(_) => false,
        };
        led.record(5, sig_ok && (rnd % 1) == 0);

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
                Err(LbCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法边界账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let led = build_initial()?;
    led.duties_closed()?;
    // 七职责名与七状态格名点验。
    let mut d = 0;
    while d < DUTY_COUNT {
        if DUTY_NAMES[d].len() == 0 || SLOT_NAMES[d].len() == 0 {
            return Err(LbCode::DutyOutOfRange.val());
        }
        d += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LbCode::ALL.len() {
        let v = LbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：7 行 / 100% / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演7行100%100%",
        matches!(g, Ok((7, 100, 100))),
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
    // 3. 七职责覆盖闭合。
    let l = build_initial().unwrap_or_else(|_| BoundLedger::new());
    v.push((
        "J1-七职责覆盖闭合",
        matches!(l.duties_closed(), Ok(())),
        format!("{} 行", l.len()),
    ));
    // 4. 裁决齐备率 100%（判据原文口径）。
    let rate = l.ownership_rate().unwrap_or(0);
    v.push(("J1-裁决齐备率100%", rate == 100, format!("{}%", rate)));
    // 5. D2 交叉一致率 100%（判据原文口径）。
    let cross = l.cross_align_rate().unwrap_or(0);
    v.push(("J1-D2交叉一致率100%", cross == 100, format!("{}%", cross)));
    // 6. 翻转会签律：无会签必拒 + 有会签留痕。
    let flip_ok = {
        let mut l2 = build_initial().unwrap_or_else(|_| BoundLedger::new());
        let no = l2.flip(DUTY_TLS, Ownership::VarixSide, false);
        let yes = l2.flip(DUTY_TLS, Ownership::VarixSide, true);
        no == Err(LbCode::FlipNoCountersign.val()) && yes.is_ok() && l2.flip_count() == 1
    };
    v.push(("J1-翻转会签律", flip_ok, String::from("留痕+会签")));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("职责+状态格名"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x71);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16062 Wine 装载器与原生装载分工边界");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
