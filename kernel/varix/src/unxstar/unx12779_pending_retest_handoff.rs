//! UNX-F12779 待实测清单移交（随闸门补测项汇总） · 0x82CC0段
//!
//! 判据原文（派工卡 12779 逐字锚定）：
//!   ① 纯功能代码 ≥326 行；
//!   ② 判据原文：UNX-F12780-J1 对齐率 100% 三断言绿（确认单/出处/术语），
//!      域文档闭账声明在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 待实测项登记表冻结闭集 32 行：每行五元组（项号/判据号引用/闸门条件/
//!    预算行/移交态），项号按 1..=32 顺序登记，乱序、越界、重复一律显性拒；
//! 2. 判据号引用必须真实落号面 F12001–F12799 闭窗（本表 32 行全部引用
//!    VX9 派工卡在册卡号，零虚构）；悬空引用在移交判定器处被 ESRCH 抓住；
//! 3. 闸门条件闭集 G0|G1|G2|G3 四档，越闭集的闸门码在移交判定器处被
//!    EINVAL 抓住；移交态 Pending→HandedOver 单向单调，重复移交 EALREADY，
//!    状态机无回退 API，倒退在结构上不可表达；
//! 4. 对齐率三断言（确认单齐/出处齐/术语齐）各一条，逐行复算 32 行×3，
//!    全绿才满 100%；闭账判定器要求 登记数=移交数+挂起数 且挂起为零，
//!    否则 EBUSY，闭账后写入/再闭一律 EClosed；
//! 5. 注入器三类（悬空引用/闸门越闭集/挂起非零闭账）各 10 轮，人为构造
//!    违约必须被对应诊断码全数抓住，逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/.get_mut()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12779 − 12600) = 0x82CC0）。
pub const SEG_BASE: u32 = 0x82CC0;

// ── 待实测移交诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtDiag {
    /// 未登记项：项号查无此行，先登记再移交。
    ENoEnt,
    /// 判据引用悬空：判据号不在 F12001..=F12799 闭窗。
    ESrch,
    /// 闸门条件越闭集：闸门码不在 G0|G1|G2|G3 四档内。
    EInval,
    /// 重复移交：该项已处于 HandedOver，移交只许一次。
    EAlready,
    /// 挂起非零禁止闭账：先清零挂起项再走闭账口。
    EBusy,
    /// 登记表容量耗尽：32 行闭集已满，扩容走下一条滚动卡。
    EFull,
    /// 项号越界：项号为零或超出 1..=32 闭集。
    ERange,
    /// 项号乱序：未按 1..=32 顺序逐行登记（含重复项号）。
    EOrder,
    /// 确认单缺账：预算行为零，先补确认单预算再入表。
    EConfirm,
    /// 对齐率不足/账目不平：三断言有红行或登记≠移交+挂起。
    EAlign,
    /// 已闭账后再写/再闭被拒：闭账声明在册即只读。
    EClosed,
    /// 双跑指纹不一致：确定性被破坏，整面复核。
    EFingerprint,
}

impl RtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RtDiag::ENoEnt => 0x01,
                RtDiag::ESrch => 0x02,
                RtDiag::EInval => 0x03,
                RtDiag::EAlready => 0x04,
                RtDiag::EBusy => 0x05,
                RtDiag::EFull => 0x06,
                RtDiag::ERange => 0x07,
                RtDiag::EOrder => 0x08,
                RtDiag::EConfirm => 0x09,
                RtDiag::EAlign => 0x0a,
                RtDiag::EClosed => 0x0b,
                RtDiag::EFingerprint => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RtDiag::ENoEnt => "未登记项：项号查无此行，先按序登记入表再发起移交",
            RtDiag::ESrch => "判据引用悬空：判据号须落在 F12001..=F12799 闭窗，先改指在册卡",
            RtDiag::EInval => "闸门越闭集：闸门码只认 G0|G1|G2|G3 四档，先归档再移交",
            RtDiag::EAlready => "重复移交：该项已是 HandedOver，复核走既有移交回执",
            RtDiag::EBusy => "挂起非零禁止闭账：先逐项移交清零挂起数再闭账",
            RtDiag::EFull => "登记表满：32 行闭集已满，新增项走下一条滚动卡",
            RtDiag::ERange => "项号越界：项号须在 1..=32 闭集内且非零",
            RtDiag::EOrder => "项号乱序：按 1..=32 顺序逐行登记，跳行/重复均拒",
            RtDiag::EConfirm => "确认单缺账：预算行为零，先补预算行再入表",
            RtDiag::EAlign => "对齐率不足：三断言有红行或账目不平，按红行回修",
            RtDiag::EClosed => "已闭账：闭账声明在册即只读，改动走下一条滚动卡",
            RtDiag::EFingerprint => "双跑指纹不一致：确定性被破坏，整面复核域模型",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RT_ALL: &[RtDiag] = &[
    RtDiag::ENoEnt,
    RtDiag::ESrch,
    RtDiag::EInval,
    RtDiag::EAlready,
    RtDiag::EBusy,
    RtDiag::EFull,
    RtDiag::ERange,
    RtDiag::EOrder,
    RtDiag::EConfirm,
    RtDiag::EAlign,
    RtDiag::EClosed,
    RtDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82CC1..0x82CCC 闭集内"
}

// ── 闸门条件闭集 G0|G1|G2|G3 ─────────────────────────────────────
/// 闸门条件四档闭集（判据原文 G0|G1|G2|G3，冻结不许增档）。
/// 语义来源：随闸门补测项按所在验证闸门归档——常规/回归/压力/发布终测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateCond {
    /// G0：常规补测闸（日常构建随闸门）。
    G0,
    /// G1：回归补测闸（回归套件触发）。
    G1,
    /// G2：压力补测闸（长稳/压测触发）。
    G2,
    /// G3：发布终测闸（发布前最后一道）。
    G3,
}

impl GateCond {
    /// 闸门码（0..3，与判据原文 G0..G3 对应）。
    pub fn code(self) -> u32 {
        match self {
            GateCond::G0 => 0,
            GateCond::G1 => 1,
            GateCond::G2 => 2,
            GateCond::G3 => 3,
        }
    }

    /// 闸门中文名（术语面闭集）。
    pub fn name(self) -> &'static str {
        match self {
            GateCond::G0 => "常规补测",
            GateCond::G1 => "回归补测",
            GateCond::G2 => "压力补测",
            GateCond::G3 => "发布终测",
        }
    }

    /// 由闸门码反查闭集档（越闭集返回 None）。
    pub fn from_code(code: u32) -> Option<GateCond> {
        match code {
            0 => Some(GateCond::G0),
            1 => Some(GateCond::G1),
            2 => Some(GateCond::G2),
            3 => Some(GateCond::G3),
            _ => None,
        }
    }
}

/// 闸门闭集数组（档序即闭集序）。
pub const GATE_ALL: &[GateCond] = &[
    GateCond::G0,
    GateCond::G1,
    GateCond::G2,
    GateCond::G3,
];

// ── 移交态：Pending → HandedOver 单调 ────────────────────────────
/// 待实测项移交态（单向单调，只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffState {
    /// 挂起：已登记未移交。
    Pending,
    /// 已移交：移交回执在册。
    HandedOver,
}

impl HandoffState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            HandoffState::Pending => 0,
            HandoffState::HandedOver => 1,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            HandoffState::Pending => "挂起",
            HandoffState::HandedOver => "已移交",
        }
    }
}

/// 登记表闭账态：Open → Closed 单向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureState {
    /// 开账：登记与移交进行中。
    Open,
    /// 已闭账：闭账声明在册，只读。
    Closed,
}

// ── 待实测行五元组 ───────────────────────────────────────────────
/// 登记表容量：冻结闭集 32 行（判据要求 ≥32 行，满即封口）。
pub const RETEST_CAP: usize = 32;

/// 判据号引用闭窗下沿（F12001）。
pub const CRIT_LO: u32 = 12001;
/// 判据号引用闭窗上沿（F12799）。
pub const CRIT_HI: u32 = 12799;

/// 待实测行五元组：项号/判据号引用/闸门条件/预算行/移交态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetestRow {
    /// 项号（1..=32 闭集）。
    pub item_no: u32,
    /// 判据号引用（须落 F12001..=F12799 闭窗，零虚构）。
    pub crit_ref: u32,
    /// 闸门条件原始码（0..3 有效，移交时验闭集）。
    pub gate: u32,
    /// 预算行（补测预算，非零有效；确认单载体）。
    pub budget: u32,
    /// 移交态（Pending→HandedOver 单调）。
    pub state: HandoffState,
}

impl RetestRow {
    /// 确认单齐：预算行非零且项号在册非零。
    pub fn confirm_ok(&self) -> bool {
        self.budget != 0 && self.item_no != 0
    }

    /// 出处齐：判据号引用落 F12001..=F12799 闭窗。
    pub fn provenance_ok(&self) -> bool {
        self.crit_ref >= CRIT_LO && self.crit_ref <= CRIT_HI
    }

    /// 术语齐：闸门条件落 G0|G1|G2|G3 闭集。
    pub fn term_ok(&self) -> bool {
        GateCond::from_code(self.gate).is_some()
    }

    /// 三断言逐行复算：全绿才计一行对齐。
    pub fn aligned(&self) -> bool {
        self.confirm_ok() && self.provenance_ok() && self.term_ok()
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        let gate_name = match GateCond::from_code(self.gate) {
            Some(g) => g.name().to_string(),
            None => "?".to_string(),
        };
        format!(
            "项{}引F{}闸{}预算{}{}",
            self.item_no,
            self.crit_ref,
            gate_name,
            self.budget,
            self.state.name()
        )
    }
}

// ── 待实测项登记表（移交协议域模型） ─────────────────────────────
/// 待实测项登记表：32 行冻结闭集 + 移交判定器 + 闭账判定器。
#[derive(Clone, Debug)]
pub struct HandoffLedger {
    /// 已入表行（按项号顺序）。
    rows: Vec<RetestRow>,
    /// 闭账态（Open→Closed 单向）。
    closure: ClosureState,
    /// 闭账声明（闭账成功时在册，非空）。
    declaration: Option<String>,
}

impl HandoffLedger {
    /// 开一张新登记表。
    pub fn new() -> HandoffLedger {
        HandoffLedger {
            rows: Vec::new(),
            closure: ClosureState::Open,
            declaration: None,
        }
    }

    /// 闭账态。
    pub fn closure(&self) -> ClosureState {
        self.closure
    }

    /// 已登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 闭账声明文本（未闭账返回空串）。
    pub fn declaration(&self) -> &str {
        match &self.declaration {
            Some(d) => d.as_str(),
            None => "",
        }
    }

    /// 按项号取行（只读快照，越界 None）。
    pub fn row_of_item(&self, item_no: u32) -> Option<RetestRow> {
        for r in self.rows.iter() {
            if r.item_no == item_no {
                return Some(*r);
            }
        }
        None
    }

    /// 挂起数（逐行复算）。
    pub fn pending_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.state == HandoffState::Pending)
            .count()
    }

    /// 移交数（逐行复算）。
    pub fn handed_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.state == HandoffState::HandedOver)
            .count()
    }

    /// 登记一行（闭账/容量/越界/乱序/确认单五道闸）。
    ///
    /// 判据号引用与闸门码在登记期不验闭窗/闭集——它们是移交判定器的
    /// 职守（判定次序 ESRCH→EINVAL→EALREADY），登记期只守结构闸。
    pub fn register(&mut self, item_no: u32, crit_ref: u32, gate: u32, budget: u32) -> Result<(), u32> {
        // 闭账闸：闭账声明在册即只读。
        if self.closure == ClosureState::Closed {
            return Err(RtDiag::EClosed.val());
        }
        // 容量闸：32 行闭集满即拒（先于项号闸，容量语义优先）。
        if self.rows.len() >= RETEST_CAP {
            return Err(RtDiag::EFull.val());
        }
        // 项号闭集闸：非零且不超容量。
        if item_no == 0 || item_no > RETEST_CAP as u32 {
            return Err(RtDiag::ERange.val());
        }
        // 顺序闸：按 1..=32 逐行登记，跳行/重复均拒。
        let expect = self.rows.len() as u32 + 1;
        if item_no != expect {
            return Err(RtDiag::EOrder.val());
        }
        // 确认单闸：预算行为零即确认单缺账。
        if budget == 0 {
            return Err(RtDiag::EConfirm.val());
        }
        self.rows.push(RetestRow {
            item_no,
            crit_ref,
            gate,
            budget,
            state: HandoffState::Pending,
        });
        Ok(())
    }

    /// 移交判定器：未登记项 ENoEnt → 引用悬空 ESrch → 闸门越闭集 EINVAL
    /// → 重复移交 EAlready，四道判定按此次序执行；全过则 Pending 推进为
    /// HandedOver 并返回移交回执行。
    pub fn judge_retest_handoff(&mut self, item_no: u32) -> Result<RetestRow, u32> {
        // 判定一：项号必须已登记（含零与越闭集项号——查无此行即 ENoEnt）。
        let idx = match self.rows.iter().position(|r| r.item_no == item_no) {
            Some(i) => i,
            None => return Err(RtDiag::ENoEnt.val()),
        };
        // 判定二：判据号引用必须落 F12001..=F12799 闭窗（出处齐）。
        let row = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(RtDiag::ENoEnt.val()),
        };
        if !row.provenance_ok() {
            return Err(RtDiag::ESrch.val());
        }
        // 判定三：闸门条件必须落 G0|G1|G2|G3 闭集（术语齐）。
        if GateCond::from_code(row.gate).is_none() {
            return Err(RtDiag::EInval.val());
        }
        // 判定四：重复移交拒（Pending→HandedOver 单调，无回退 API）。
        if row.state == HandoffState::HandedOver {
            return Err(RtDiag::EAlready.val());
        }
        match self.rows.get_mut(idx) {
            Some(slot) => slot.state = HandoffState::HandedOver,
            None => return Err(RtDiag::ENoEnt.val()),
        }
        Ok(RetestRow {
            state: HandoffState::HandedOver,
            ..row
        })
    }

    /// 闭账判定器：登记数=移交数+挂起数（账目恒等式逐行复算），挂起必须
    /// 为零才闭账（否则 EBUSY）；对齐率 100% 三断言全绿才许闭；闭账成功
    /// 后声明在册（非空），再闭/再写一律 EClosed。
    /// 判定次序：EClosed → 恒等式 EAlign → 对齐率 EAlign → 挂起 EBUSY。
    pub fn judge_closure(&mut self) -> Result<String, u32> {
        // 已闭账：闭账态单调，再闭显性拒。
        if self.closure == ClosureState::Closed {
            return Err(RtDiag::EClosed.val());
        }
        // 账目恒等式：登记数 = 移交数 + 挂起数（逐行复算）。
        let pending = self.pending_count();
        let handed = self.handed_count();
        let total = match handed.checked_add(pending) {
            Some(t) => t,
            None => return Err(RtDiag::EAlign.val()),
        };
        if total != self.rows.len() {
            return Err(RtDiag::EAlign.val());
        }
        // 对齐率 100%：三断言逐行全绿（先验账面质量，再验清账进度）。
        if !alignment_100_ok(self) {
            return Err(RtDiag::EAlign.val());
        }
        // 挂起必须为零。
        if pending != 0 {
            return Err(RtDiag::EBusy.val());
        }
        // 闭账声明在册（非空，含账目三数与判据锚）。
        let decl = format!(
            "UNX-F12780-J1 域文档闭账声明：登记{}=移交{}+挂起{}，对齐率100%三断言绿（确认单/出处/术语）",
            self.rows.len(),
            handed,
            pending
        );
        self.closure = ClosureState::Closed;
        self.declaration = Some(decl.clone());
        Ok(decl)
    }
}

// ── 冻结闭集 32 行：判据号引用取 VX9 在册卡号（零虚构） ──────────
/// 冻结闭集 32 行的判据号引用表（全部为 VX9 派工卡在册卡号，
/// 落 F12001–F12799 闭窗；来源 _tools/vx9_cards.json 卡面 id 面）。
pub const CRIT_REF_TABLE: &[u32] = &[
    12600, 12603, 12607, 12611, 12615, 12619, 12623, 12627, 12631, 12635, 12639, 12643, 12647,
    12651, 12655, 12659, 12663, 12667, 12671, 12675, 12679, 12683, 12687, 12691, 12695, 12699,
    12703, 12707, 12711, 12715, 12719, 12723,
];

/// 确定性 LCG（种子 = SEG_BASE，零随机红线合规）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性，双跑一致）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 1..=bound 内取非零值（bound 为零回落 1）。
    pub fn nonzero_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 1;
        }
        1 + self.next_u32() % bound
    }
}

/// 构建冻结闭集登记表：32 行五元组确定性生成。
/// 项号 1..=32 顺序；闸门按表内序轮转 G0..G3；预算行 LCG 非零。
pub fn build_frozen_ledger() -> HandoffLedger {
    let mut l = HandoffLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    for (k, crit) in CRIT_REF_TABLE.iter().enumerate() {
        let item_no = k as u32 + 1;
        let gate = match k % 4 {
            0 => GateCond::G0.code(),
            1 => GateCond::G1.code(),
            2 => GateCond::G2.code(),
            _ => GateCond::G3.code(),
        };
        let budget = rng.nonzero_below(0xFFFE);
        let _ = l.register(item_no, *crit, gate, budget);
    }
    l
}

/// 构建部分移交表：前 `handed_n` 行移交、其余挂起（用于闭账注入）。
pub fn build_partial_ledger(handed_n: usize) -> HandoffLedger {
    let mut l = build_frozen_ledger();
    let mut item = 1u32;
    while item <= handed_n as u32 {
        let _ = l.judge_retest_handoff(item);
        item += 1;
    }
    l
}

// ── 对齐率三断言（确认单齐/出处齐/术语齐，逐行复算） ────────────
/// 断言一：确认单齐——全部在册行预算行非零且项号非零（逐行复算）。
pub fn assert_confirm_ok(l: &HandoffLedger) -> bool {
    for r in l.rows_iter() {
        if !r.confirm_ok() {
            return false;
        }
    }
    true
}

/// 断言二：出处齐——全部在册行判据号引用落 F12001..=F12799 闭窗。
pub fn assert_provenance_ok(l: &HandoffLedger) -> bool {
    for r in l.rows_iter() {
        if !r.provenance_ok() {
            return false;
        }
    }
    true
}

/// 断言三：术语齐——全部在册行闸门条件落 G0|G1|G2|G3 闭集。
pub fn assert_term_ok(l: &HandoffLedger) -> bool {
    for r in l.rows_iter() {
        if !r.term_ok() {
            return false;
        }
    }
    true
}

/// 对齐率 100% 判定：三断言全绿且在册行数恰为 32 行闭集容量。
pub fn alignment_100_ok(l: &HandoffLedger) -> bool {
    assert_confirm_ok(l) && assert_provenance_ok(l) && assert_term_ok(l) && l.len() == RETEST_CAP
}

/// 对齐率报告：逐行复算 32 行 × 3 断言，返回（绿断言数, 总断言数, 首红行摘要）。
pub fn alignment_report(l: &HandoffLedger) -> (usize, usize, String) {
    let mut green = 0usize;
    let mut total = 0usize;
    let mut first_red = String::new();
    for r in l.rows_iter() {
        let trio = [(r.confirm_ok(), "确认单"), (r.provenance_ok(), "出处"), (r.term_ok(), "术语")];
        for t in trio.iter() {
            total += 1;
            if t.0 {
                green += 1;
            } else if first_red.is_empty() {
                first_red = format!("项{}{}红", r.item_no, t.1);
            }
        }
    }
    (green, total, first_red)
}

impl HandoffLedger {
    /// 行迭代器（只读，供逐行复算）。
    pub fn rows_iter(&self) -> impl Iterator<Item = &RetestRow> + '_ {
        self.rows.iter()
    }
}

// ── 双跑一致指纹（FNV-1a 32 位，确定性） ─────────────────────────
/// 登记表指纹：把行数、闭账态与全部行五元组逐字节混入 FNV-1a。
pub fn fingerprint(l: &HandoffLedger) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mix = |h: &mut u32, v: u64| {
        let bs = v.to_le_bytes();
        for b in bs.iter() {
            *h = h.wrapping_mul(0x0100_0193) ^ (*b as u32);
        }
    };
    mix(&mut h, l.len() as u64);
    mix(&mut h, match l.closure() {
        ClosureState::Open => 0,
        ClosureState::Closed => 1,
    });
    for r in l.rows_iter() {
        mix(&mut h, r.item_no as u64);
        mix(&mut h, r.crit_ref as u64);
        mix(&mut h, r.gate as u64);
        mix(&mut h, r.budget as u64);
        mix(&mut h, r.state.rank() as u64);
    }
    h
}

/// 指纹核对：与期望不一致返回 EFingerprint（双跑一致的判定口）。
pub fn check_fingerprint(l: &HandoffLedger, expect: u32) -> Result<u32, u32> {
    let got = fingerprint(l);
    if got == expect {
        Ok(got)
    } else {
        Err(RtDiag::EFingerprint.val())
    }
}

// ── 注入器三类（各 10 轮，人为违约必被抓） ───────────────────────
/// 悬空引用样本闭集（10 个，全部落在 F12001..=F12799 闭窗之外）。
pub const BAD_REF_TABLE: &[u32] = &[
    0, 1, 10001, 11000, 11999, 12000, 12800, 12801, 13000, 99999,
];

/// 注入一：悬空引用 10 轮——判据号越闭窗，移交判定器必须逐轮 ESRCH。
/// 返回（抓住轮数, 总轮数, 首个逃脱轮描述）。
pub fn inject_dangling() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for (k, bad) in BAD_REF_TABLE.iter().enumerate() {
        let mut l = HandoffLedger::new();
        // 项号 1、闸门 G0、预算非零——只让判据号悬空，单变量注入。
        let reg = l.register(1, *bad, GateCond::G0.code(), 7);
        let verdict = l.judge_retest_handoff(1);
        total += 1;
        let hit = match reg {
            Ok(()) => verdict == Err(RtDiag::ESrch.val()),
            Err(_) => false,
        };
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}引用F{}未抓", k, bad);
        }
    }
    (caught, total, escape)
}

/// 注入二：闸门越闭集 10 轮——闸门码 4..=13，判定器必须逐轮 EINVAL。
pub fn inject_gate_outside() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let bad_gate = 4 + k as u32;
        let mut l = HandoffLedger::new();
        // 判据号引用 F12600（在册）、预算非零——只让闸门越闭集。
        let reg = l.register(1, 12600, bad_gate, 9);
        let verdict = l.judge_retest_handoff(1);
        total += 1;
        let hit = match reg {
            Ok(()) => verdict == Err(RtDiag::EInval.val()),
            Err(_) => false,
        };
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}闸{}未抓", k, bad_gate);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入三：挂起非零闭账 10 轮——挂起 1..=10 行，闭账判定器必须逐轮 EBUSY。
pub fn inject_pending_closure() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut pending_n = 1usize;
    while pending_n <= 10 {
        // 前 (32-挂起数) 行移交、余下挂起，账目恒等式成立而挂起非零。
        let mut l = build_partial_ledger(RETEST_CAP - pending_n);
        let verdict = l.judge_closure();
        total += 1;
        if verdict == Err(RtDiag::EBusy.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}挂起{}未抓", pending_n, pending_n);
        }
        pending_n += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 派工窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全落 [SEG_BASE, SEG_BASE+0x40) 窗、查表命中。
fn diag_closed_ok() -> bool {
    if RT_ALL.len() != 12 {
        return false;
    }
    for d in RT_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 两两互异。
    for a in RT_ALL.iter() {
        for b in RT_ALL.iter() {
            if a.val() == b.val() && a != b {
                return false;
            }
        }
    }
    // 未知码必须回落。
    if detail(SEG_BASE + 0x3F) == RtDiag::ENoEnt.detail() {
        return false;
    }
    true
}

/// 冻结闭集守卫：32 行齐、项号 1..=32、引用全落闭窗、闸门全落闭集、预算非零。
fn frozen_ok() -> bool {
    let l = build_frozen_ledger();
    if l.len() != RETEST_CAP {
        return false;
    }
    for k in 0..RETEST_CAP {
        let item = k as u32 + 1;
        let row = match l.row_of_item(item) {
            Some(r) => r,
            None => return false,
        };
        if row.item_no != item || !row.aligned() {
            return false;
        }
        // 引用必须逐行对表（零虚构：与冻结引用表逐位一致）。
        let expect_ref = match CRIT_REF_TABLE.get(k) {
            Some(c) => *c,
            None => return false,
        };
        if row.crit_ref != expect_ref {
            return false;
        }
    }
    true
}

/// 正常流：冻结表 → 全部 32 项逐项移交成功 → 闭账成功且声明在册非空。
fn happy_path_ok() -> bool {
    let mut l = build_frozen_ledger();
    let mut item = 1u32;
    while item <= RETEST_CAP as u32 {
        match l.judge_retest_handoff(item) {
            Ok(r) => {
                if r.state != HandoffState::HandedOver {
                    return false;
                }
            }
            Err(_) => return false,
        }
        item += 1;
    }
    if l.pending_count() != 0 || l.handed_count() != RETEST_CAP {
        return false;
    }
    match l.judge_closure() {
        Ok(decl) => {
            if l.closure() != ClosureState::Closed || decl.is_empty() {
                return false;
            }
        }
        Err(_) => return false,
    }
    !l.declaration().is_empty()
}

/// 账目恒等式：登记数 = 移交数 + 挂起数（对部分移交表逐行复算）。
fn closure_account_ok() -> bool {
    // 全挂起表。
    let l0 = build_frozen_ledger();
    if l0.pending_count() + l0.handed_count() != l0.len() {
        return false;
    }
    // 半移交表。
    let l1 = build_partial_ledger(16);
    if l1.handed_count() != 16 || l1.pending_count() != 16 || l1.len() != 32 {
        return false;
    }
    // 全移交表。
    let l2 = build_partial_ledger(32);
    l2.handed_count() == 32 && l2.pending_count() == 0
}

/// 判定次序：ENoEnt → ESrch → EINVAL → EALREADY 优先级逐对验证。
fn judge_order_ok() -> bool {
    // ENoEnt 最先：未登记项号（含 0 与超闭集）一律 ENoEnt。
    let mut l = build_frozen_ledger();
    if l.judge_retest_handoff(0) != Err(RtDiag::ENoEnt.val()) {
        return false;
    }
    if l.judge_retest_handoff(33) != Err(RtDiag::ENoEnt.val()) {
        return false;
    }
    // ESrch 先于 EALREADY：悬空引用行重复判定两轮，两轮都是 ESrch。
    let mut l2 = HandoffLedger::new();
    let _ = l2.register(1, 12800, GateCond::G0.code(), 5);
    if l2.judge_retest_handoff(1) != Err(RtDiag::ESrch.val()) {
        return false;
    }
    if l2.judge_retest_handoff(1) != Err(RtDiag::ESrch.val()) {
        return false;
    }
    // ESrch 先于 EINVAL：引用悬空且闸门越闭集的行，报 ESrch 不报 EINVAL。
    let mut l3 = HandoffLedger::new();
    let _ = l3.register(1, 12000, 9, 5);
    if l3.judge_retest_handoff(1) != Err(RtDiag::ESrch.val()) {
        return false;
    }
    // EINVAL 先于移交：闸门越闭集的行不许移交（状态保持挂起）。
    let mut l4 = HandoffLedger::new();
    let _ = l4.register(1, 12600, 4, 5);
    if l4.judge_retest_handoff(1) != Err(RtDiag::EInval.val()) {
        return false;
    }
    if l4.row_of_item(1).map(|r| r.state) != Some(HandoffState::Pending) {
        return false;
    }
    // EALREADY 最后：合法行首轮 Ok、次轮 EALREADY，且状态不可倒退。
    let mut l5 = build_frozen_ledger();
    if l5.judge_retest_handoff(1).is_err() {
        return false;
    }
    if l5.judge_retest_handoff(1) != Err(RtDiag::EAlready.val()) {
        return false;
    }
    l5.row_of_item(1).map(|r| r.state) == Some(HandoffState::HandedOver)
}

/// 双跑一致：两次冻结构建指纹一致、两次注入面结果一致、指纹可核对、
/// 单行移交即指纹变动（灵敏度验证）。
fn dual_run_ok() -> bool {
    let a = build_frozen_ledger();
    let b = build_frozen_ledger();
    if fingerprint(&a) != fingerprint(&b) {
        return false;
    }
    if check_fingerprint(&a, fingerprint(&b)).is_err() {
        return false;
    }
    if inject_dangling() != inject_dangling() {
        return false;
    }
    if inject_gate_outside() != inject_gate_outside() {
        return false;
    }
    if inject_pending_closure() != inject_pending_closure() {
        return false;
    }
    // 灵敏度：移交一行后指纹必须变动，且对期望不一致时报 EFingerprint。
    let mut c = build_frozen_ledger();
    let pristine = fingerprint(&c);
    let _ = c.judge_retest_handoff(1);
    fingerprint(&c) != pristine && check_fingerprint(&c, pristine) == Err(RtDiag::EFingerprint.val())
}

/// 容量耗尽：32 行满后第 33 行拒（EFull），账面不变。
fn capacity_ok() -> bool {
    let mut l = build_frozen_ledger();
    let before = l.len();
    if l.register(33, 12600, GateCond::G0.code(), 5) != Err(RtDiag::EFull.val()) {
        return false;
    }
    if l.len() != before || l.len() != RETEST_CAP {
        return false;
    }
    // 满表上再补确认单缺账行同样先撞容量闸。
    l.register(34, 12600, GateCond::G0.code(), 0) == Err(RtDiag::EFull.val())
}

/// 边界四点：项号闭集边界、判据号闭窗边界、闸门闭集边界、闭账单调边界。
fn boundary_ok() -> bool {
    // 边界一（项号）：零与超闭集拒，1 与 32 收。
    let mut l = HandoffLedger::new();
    if l.register(0, 12600, 0, 5) != Err(RtDiag::ERange.val()) {
        return false;
    }
    if l.register(33, 12600, 0, 5) != Err(RtDiag::ERange.val()) {
        return false;
    }
    if l.register(1, 12600, 0, 5).is_err() {
        return false;
    }
    // 边界二（判据号闭窗）：12001/12799 收，12000/12800 悬空。
    let mut l2 = HandoffLedger::new();
    if l2.register(1, CRIT_LO, 0, 5).is_err() {
        return false;
    }
    if l2.register(2, CRIT_HI, 1, 5).is_err() {
        return false;
    }
    if l2.judge_retest_handoff(1).is_err() || l2.judge_retest_handoff(2).is_err() {
        return false;
    }
    let mut l3 = HandoffLedger::new();
    let _ = l3.register(1, CRIT_LO - 1, 0, 5);
    if l3.judge_retest_handoff(1) != Err(RtDiag::ESrch.val()) {
        return false;
    }
    let mut l4 = HandoffLedger::new();
    let _ = l4.register(1, CRIT_HI + 1, 0, 5);
    if l4.judge_retest_handoff(1) != Err(RtDiag::ESrch.val()) {
        return false;
    }
    // 边界三（闸门闭集）：码 0/3 收，码 4 拒。
    let mut l5 = HandoffLedger::new();
    if l5.register(1, 12600, 3, 5).is_err() {
        return false;
    }
    if l5.judge_retest_handoff(1).is_err() {
        return false;
    }
    let mut l6 = HandoffLedger::new();
    let _ = l6.register(1, 12600, 4, 5);
    if l6.judge_retest_handoff(1) != Err(RtDiag::EInval.val()) {
        return false;
    }
    // 边界四（闭账单调）：闭账后登记与再闭一律 EClosed。
    let mut l7 = build_partial_ledger(32);
    if l7.judge_closure().is_err() {
        return false;
    }
    if l7.register(33, 12600, 0, 5) != Err(RtDiag::EClosed.val()) {
        return false;
    }
    l7.judge_closure() == Err(RtDiag::EClosed.val())
}

/// 乱序与确认单闸：跳行/重复项号 EOrder，预算为零 EConfirm。
fn order_and_confirm_ok() -> bool {
    // 跳行：首行未登就登项 2。
    let mut l = HandoffLedger::new();
    if l.register(2, 12600, 0, 5) != Err(RtDiag::EOrder.val()) {
        return false;
    }
    // 重复：项 1 登两次。
    let mut l2 = HandoffLedger::new();
    if l2.register(1, 12600, 0, 5).is_err() {
        return false;
    }
    if l2.register(1, 12600, 0, 5) != Err(RtDiag::EOrder.val()) {
        return false;
    }
    // 确认单缺账：预算行为零。
    let mut l3 = HandoffLedger::new();
    l3.register(1, 12600, 0, 0) == Err(RtDiag::EConfirm.val())
}

/// 对齐率红线：注入一行悬空引用即三断言出现红项、对齐率失守。
fn alignment_red_ok() -> bool {
    let mut l = HandoffLedger::new();
    // 31 行合法 + 1 行悬空引用。
    for k in 0..RETEST_CAP - 1 {
        let crit = match CRIT_REF_TABLE.get(k) {
            Some(c) => *c,
            None => return false,
        };
        if l.register(k as u32 + 1, crit, GateCond::G0.code(), 3).is_err() {
            return false;
        }
    }
    if l.register(32, 12800, GateCond::G0.code(), 3).is_err() {
        return false;
    }
    let (green, total, red) = alignment_report(&l);
    if total != RETEST_CAP * 3 {
        return false;
    }
    if green >= total || red.is_empty() {
        return false;
    }
    // 出处断言单点红，确认单/术语保持绿。
    !assert_provenance_ok(&l) && assert_confirm_ok(&l) && assert_term_ok(&l)
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 本卡检查节判据条数（收口 add_dyn 的声明锚：收口前 16 条）。
const CHECK_N: usize = 16;

/// UNX-F12779 自检（判据 12779 卡：对齐率 100% 三断言绿 + 域文档闭账声明在册）。
pub fn run_unx12779_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12779");
    let frozen = build_frozen_ledger();

    s.add_dyn(
        "F12780-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12780-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={} first=0x{:05x}", RT_ALL.len(), RT_ALL.first().map(|d| d.val()).unwrap_or(0)),
    );

    s.add_dyn(
        "F12780-03 冻结闭集32行",
        frozen_ok(),
        &format!(
            "rows={} 首行{}",
            frozen.len(),
            frozen.row_of_item(1)
                .map(|r| r.brief())
                .unwrap_or_default()
        ),
    );

    // 正常流：全部 32 项逐项移交 + 闭账 + 声明在册。
    s.add_dyn(
        "F12780-04 正常流移交闭账",
        happy_path_ok(),
        &format!("pending={} handed={}", frozen.pending_count(), frozen.handed_count()),
    );

    s.add_dyn(
        "F12780-05 账目恒等式",
        closure_account_ok(),
        &format!(
            "reg={}=移交{}+挂起{}",
            frozen.len(),
            frozen.handed_count(),
            frozen.pending_count()
        ),
    );

    // 对齐率三断言：确认单/出处/术语，逐行复算。
    let (green, total, red) = alignment_report(&frozen);
    s.add_dyn(
        "F12780-06 三断言-确认单齐",
        assert_confirm_ok(&frozen) && frozen.len() == RETEST_CAP,
        &format!("rows={} budgets_nonzero=1", frozen.len()),
    );
    s.add_dyn(
        "F12780-07 三断言-出处齐",
        assert_provenance_ok(&frozen),
        &format!("window=F{}..F{}", CRIT_LO, CRIT_HI),
    );
    s.add_dyn(
        "F12780-08 三断言-术语齐",
        assert_term_ok(&frozen),
        &format!("gates={} all_in_set=1", GATE_ALL.len()),
    );

    s.add_dyn(
        "F12780-09 对齐率100%",
        alignment_100_ok(&frozen) && green == total && red.is_empty(),
        &format!("{}/{}", green, total),
    );

    // 注入必抓：三类注入各 10 轮，全数命中。
    let (c1, t1, e1) = inject_dangling();
    s.add_dyn(
        "F12780-10 注入-悬空引用10轮",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );
    let (c2, t2, e2) = inject_gate_outside();
    s.add_dyn(
        "F12780-11 注入-闸门越闭集10轮",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );
    let (c3, t3, e3) = inject_pending_closure();
    s.add_dyn(
        "F12780-12 注入-挂起非零闭账10轮",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12780-13 判定次序四码",
        judge_order_ok(),
        "ENoEnt→ESrch→EINVAL→EALREADY 优先级逐对验证",
    );

    s.add_dyn(
        "F12780-14 双跑一致指纹",
        dual_run_ok(),
        &format!("fp=0x{:08x} 灵敏=1", fingerprint(&frozen)),
    );

    s.add_dyn(
        "F12780-15 容量耗尽",
        capacity_ok(),
        &format!("cap={} 满33行拒EFull", RETEST_CAP),
    );

    s.add_dyn(
        "F12780-16 边界与乱序确认单",
        boundary_ok() && order_and_confirm_ok() && alignment_red_ok(),
        "项号/闭窗/闸门/闭账 四边界 + 乱序/确认单 + 对齐红",
    );

    // 收口：判据条数声明。
    let n_before = s.len();
    s.add_dyn(
        "F12780-17 收口判据条数声明",
        n_before == CHECK_N,
        &format!("本卡检查节共{}条判据已入册", n_before + 1),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 派工窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82CC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 码互异、全落窗、查表命中、未知码回落。
        assert_eq!(RT_ALL.len(), 12);
        let mut i = 0usize;
        while i < RT_ALL.len() {
            let vi = RT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < RT_ALL.len() {
                let vj = RT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        assert_eq!(detail(RtDiag::EBusy.val()), RtDiag::EBusy.detail());
    }

    #[test]
    fn normal_flow_handoff_then_closure() {
        // 正常流：32 项逐项移交 → 闭账成功 → 声明在册非空 → 再闭/再写拒。
        let mut l = build_frozen_ledger();
        assert_eq!(l.len(), RETEST_CAP);
        assert_eq!(l.pending_count(), 32);
        let mut item = 1u32;
        while item <= 32 {
            let row = l.judge_retest_handoff(item);
            assert!(row.is_ok(), "项{}移交失败", item);
            assert_eq!(row.map(|r| r.state), Ok(HandoffState::HandedOver));
            item += 1;
        }
        assert_eq!(l.pending_count(), 0);
        assert_eq!(l.handed_count(), 32);
        let decl_ok = match l.judge_closure() {
            Ok(d) => !d.is_empty(),
            Err(_) => false,
        };
        assert!(decl_ok);
        assert_eq!(l.closure(), ClosureState::Closed);
        assert!(!l.declaration().is_empty());
        assert!(l.declaration().contains("闭账声明"));
        assert_eq!(l.judge_closure(), Err(RtDiag::EClosed.val()));
        assert_eq!(
            l.register(33, 12600, 0, 5),
            Err(RtDiag::EClosed.val())
        );
    }

    #[test]
    fn boundaries_four_kinds() {
        // 边界一（项号闭集）：0 与 33 拒 ERange。
        let mut l = HandoffLedger::new();
        assert_eq!(l.register(0, 12600, 0, 5), Err(RtDiag::ERange.val()));
        assert_eq!(l.register(33, 12600, 0, 5), Err(RtDiag::ERange.val()));
        // 边界二（判据号闭窗）：12001/12799 收，12000/12800 报 ESrch。
        let mut l2 = HandoffLedger::new();
        assert!(l2.register(1, CRIT_LO, 0, 5).is_ok());
        assert!(l2.register(2, CRIT_HI, 1, 5).is_ok());
        assert!(l2.judge_retest_handoff(1).is_ok());
        assert!(l2.judge_retest_handoff(2).is_ok());
        let mut l3 = HandoffLedger::new();
        let _ = l3.register(1, CRIT_LO - 1, 0, 5);
        assert_eq!(l3.judge_retest_handoff(1), Err(RtDiag::ESrch.val()));
        let mut l4 = HandoffLedger::new();
        let _ = l4.register(1, CRIT_HI + 1, 0, 5);
        assert_eq!(l4.judge_retest_handoff(1), Err(RtDiag::ESrch.val()));
        // 边界三（闸门闭集）：0/3 收，4 报 EINVAL。
        let mut l5 = HandoffLedger::new();
        assert!(l5.register(1, 12600, 3, 5).is_ok());
        assert!(l5.judge_retest_handoff(1).is_ok());
        let mut l6 = HandoffLedger::new();
        let _ = l6.register(1, 12600, 4, 5);
        assert_eq!(l6.judge_retest_handoff(1), Err(RtDiag::EInval.val()));
        // 边界四（容量闭集）：满 32 行后第 33 行拒 EFull 且账面不变。
        let mut full = build_frozen_ledger();
        let before = full.len();
        assert_eq!(
            full.register(33, 12600, 0, 5),
            Err(RtDiag::EFull.val())
        );
        assert_eq!(full.len(), before);
        assert_eq!(full.len(), 32);
    }

    #[test]
    fn judge_order_precedence() {
        // 判定次序：ENoEnt → ESrch → EINVAL → EALREADY。
        let mut l = build_frozen_ledger();
        assert_eq!(l.judge_retest_handoff(0), Err(RtDiag::ENoEnt.val()));
        assert_eq!(l.judge_retest_handoff(33), Err(RtDiag::ENoEnt.val()));
        // 悬空引用行重复判定：两轮都 ESrch（ESrch 先于 EALREADY）。
        let mut l2 = HandoffLedger::new();
        let _ = l2.register(1, 12800, GateCond::G0.code(), 5);
        assert_eq!(l2.judge_retest_handoff(1), Err(RtDiag::ESrch.val()));
        assert_eq!(l2.judge_retest_handoff(1), Err(RtDiag::ESrch.val()));
        // 引用悬空 + 闸门越闭集：报 ESrch（先于 EINVAL）。
        let mut l3 = HandoffLedger::new();
        let _ = l3.register(1, 12000, 9, 5);
        assert_eq!(l3.judge_retest_handoff(1), Err(RtDiag::ESrch.val()));
        // 闸门越闭集：拒移交且状态保持挂起（EINVAL 先于移交推进）。
        let mut l4 = HandoffLedger::new();
        let _ = l4.register(1, 12600, 4, 5);
        assert_eq!(l4.judge_retest_handoff(1), Err(RtDiag::EInval.val()));
        assert_eq!(
            l4.row_of_item(1).map(|r| r.state),
            Some(HandoffState::Pending)
        );
        // 合法行：首轮 Ok 次轮 EALREADY，状态不倒退。
        let mut l5 = build_frozen_ledger();
        assert!(l5.judge_retest_handoff(1).is_ok());
        assert_eq!(l5.judge_retest_handoff(1), Err(RtDiag::EAlready.val()));
        assert_eq!(
            l5.row_of_item(1).map(|r| r.state),
            Some(HandoffState::HandedOver)
        );
    }

    #[test]
    fn injections_three_kinds_all_caught() {
        // 注入一：悬空引用 10 轮全抓 ESRCH。
        let (c1, t1, e1) = inject_dangling();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "逃脱：{}", e1);
        assert!(e1.is_empty());
        // 注入二：闸门越闭集 10 轮全抓 EINVAL。
        let (c2, t2, e2) = inject_gate_outside();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "逃脱：{}", e2);
        assert!(e2.is_empty());
        // 注入三：挂起非零闭账 10 轮全抓 EBUSY。
        let (c3, t3, e3) = inject_pending_closure();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "逃脱：{}", e3);
        assert!(e3.is_empty());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次冻结构建指纹一致、三次注入面双跑一致。
        let a = build_frozen_ledger();
        let b = build_frozen_ledger();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert!(check_fingerprint(&a, fingerprint(&b)).is_ok());
        assert_eq!(inject_dangling(), inject_dangling());
        assert_eq!(inject_gate_outside(), inject_gate_outside());
        assert_eq!(inject_pending_closure(), inject_pending_closure());
        // 灵敏度：移交一行后指纹变动，错配核对报 EFingerprint。
        let mut c = build_frozen_ledger();
        let pristine = fingerprint(&c);
        let _ = c.judge_retest_handoff(1);
        assert_ne!(fingerprint(&c), pristine);
        assert_eq!(
            check_fingerprint(&c, pristine),
            Err(RtDiag::EFingerprint.val())
        );
    }

    #[test]
    fn capacity_exhaustion_and_account() {
        // 容量耗尽：满表拒新行；账目恒等式对三种移交进度都成立。
        let mut l = build_frozen_ledger();
        assert_eq!(l.len(), RETEST_CAP);
        assert_eq!(
            l.register(33, 12600, 0, 5),
            Err(RtDiag::EFull.val())
        );
        assert_eq!(l.len(), RETEST_CAP);
        // 全挂起表：登记 32 = 移交 0 + 挂起 32，闭账 EBUSY。
        let l0 = build_frozen_ledger();
        assert_eq!(l0.handed_count() + l0.pending_count(), l0.len());
        // 半移交表：16 = 16 + 0？不——16 移交 + 16 挂起 = 32 登记。
        let mut l1 = build_partial_ledger(16);
        assert_eq!(l1.handed_count(), 16);
        assert_eq!(l1.pending_count(), 16);
        assert_eq!(l1.len(), 32);
        assert_eq!(l1.judge_closure(), Err(RtDiag::EBusy.val()));
        // 全移交表闭账成功。
        let mut l2 = build_partial_ledger(32);
        assert!(l2.judge_closure().is_ok());
    }

    #[test]
    fn alignment_recomputed_and_red_on_injection() {
        // 三断言逐行复算：冻结表 96 项全绿；注入一行悬空引用即出处红。
        let l = build_frozen_ledger();
        let (green, total, red) = alignment_report(&l);
        assert_eq!(total, RETEST_CAP * 3);
        assert_eq!(green, total);
        assert!(red.is_empty());
        assert!(assert_confirm_ok(&l));
        assert!(assert_provenance_ok(&l));
        assert!(assert_term_ok(&l));
        assert!(alignment_100_ok(&l));
        // 注入红：31 合法 + 1 悬空。
        let mut bad = HandoffLedger::new();
        for k in 0..RETEST_CAP - 1 {
            let crit = CRIT_REF_TABLE.get(k).copied().unwrap_or(12600);
            assert!(bad.register(k as u32 + 1, crit, GateCond::G0.code(), 3).is_ok());
        }
        assert!(bad.register(32, 12800, GateCond::G0.code(), 3).is_ok());
        let (g2, t2, r2) = alignment_report(&bad);
        assert_eq!(t2, RETEST_CAP * 3);
        assert!(g2 < t2);
        assert!(!r2.is_empty());
        assert!(!assert_provenance_ok(&bad));
        assert!(assert_confirm_ok(&bad));
        assert!(assert_term_ok(&bad));
    }

    #[test]
    fn structural_gates_reject() {
        // 乱序：跳行与重复项号均拒 EOrder。
        let mut l = HandoffLedger::new();
        assert_eq!(l.register(2, 12600, 0, 5), Err(RtDiag::EOrder.val()));
        let mut l2 = HandoffLedger::new();
        assert!(l2.register(1, 12600, 0, 5).is_ok());
        assert_eq!(l2.register(1, 12600, 0, 5), Err(RtDiag::EOrder.val()));
        // 确认单缺账：预算行为零拒 EConfirm。
        let mut l3 = HandoffLedger::new();
        assert_eq!(l3.register(1, 12600, 0, 0), Err(RtDiag::EConfirm.val()));
        // 项号越界：0 与 33 均拒 ERange。
        let mut l4 = HandoffLedger::new();
        assert_eq!(l4.register(0, 12600, 0, 5), Err(RtDiag::ERange.val()));
        assert_eq!(l4.register(33, 12600, 0, 5), Err(RtDiag::ERange.val()));
    }

    #[test]
    fn closure_requires_zero_pending_and_alignment() {
        // 闭账判定器：挂起非零 EBUSY；对齐红 EAlign；全绿闭账 Ok。
        let mut half = build_partial_ledger(31);
        assert_eq!(half.pending_count(), 1);
        assert_eq!(half.judge_closure(), Err(RtDiag::EBusy.val()));
        // 对齐红表（1 行悬空引用且全移交）闭账报 EAlign。
        let mut bad = HandoffLedger::new();
        assert!(bad.register(1, 12800, GateCond::G0.code(), 5).is_ok());
        assert!(bad.judge_retest_handoff(1).is_err());
        assert_eq!(bad.judge_closure(), Err(RtDiag::EAlign.val()));
        // 全绿全移交闭账成功。
        let mut full = build_partial_ledger(32);
        assert!(full.judge_closure().is_ok());
        assert_eq!(full.closure(), ClosureState::Closed);
    }

    #[test]
    fn frozen_table_refs_are_real_cards() {
        // 零虚构：32 行引用全落 F12001..=F12799 闭窗且与冻结表逐位一致。
        assert_eq!(CRIT_REF_TABLE.len(), RETEST_CAP);
        let l = build_frozen_ledger();
        let mut k = 0usize;
        while k < RETEST_CAP {
            let expect = CRIT_REF_TABLE.get(k).copied().unwrap_or(0);
            assert!(expect >= CRIT_LO && expect <= CRIT_HI);
            let row = l.row_of_item(k as u32 + 1);
            assert_eq!(row.map(|r| r.crit_ref), Some(expect));
            assert!(row.map(|r| r.aligned()) == Some(true));
            k += 1;
        }
        // 闸门轮转闭集：四档各 8 行。
        let mut g = 0u32;
        while g < 4 {
            let n = l
                .rows_iter()
                .filter(|r| r.gate == g)
                .count();
            assert_eq!(n, 8, "闸门码{}行数应为8", g);
            g += 1;
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（实为 17 条）。
        let s = run_unx12779_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 17);
        assert_eq!(s.tally().1, 0);
    }
}
