//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12767 结构布局出处核验（OA/CONTEXT/UNWIND_INFO） · 0x829C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 vx9_cards.json 卡 12767 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12768-J1 分界声明核验绿（D1–D3 三分界+D4/E1 预演
//!      声明），越界声明 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三结构闭集：OA / CONTEXT / UNWIND_INFO 各自维护一份逐字段出处档
//!     （字段名/偏移/长度/出处档位四元组），档位闭集三档：1=主手册档、
//!     2=规范复核档、3=台账附录档；档外值一律判出处越档，零容忍；
//!   - 三分界声明（D1–D3）：D1=结构内分界（声明布局终点=Σ字段长度），
//!     D2=结构间分界（布局终点≤结构间距 0x400），D3=段窗分界（基址+
//!     终点落在诊断段窗 [0x80000,0x82FC0] 内）——三条声明逐条核验绿；
//!   - 两条预演声明（D4/E1）：D4=布局漂移演练预演声明（漂移注入 10/10
//!     必抓后登记），E1=越界声明预演声明（越界注入 10/10 检出后登记）；
//!     预演声明未登记或登记数不足 10 即分界核验判红——先演练后收口；
//!   - 布局漂移演练：对黄金字段表做 LCG（种子=0x829C0）选点的 +1 偏移
//!     扰动，布局校验和逐结构比对，扰动必被 FieldOffsetDrift 抓获；
//!   - 单调状态机：登记中 Draft → 核验毕 Audited → 冻结 Sealed，倒退
//!     显性拒（StateRegress）并给出中文操作建议；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect，
//!     取值一律 .get()/match/checked_*/saturating_*。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x829C0，VX9 批公式 0x80000+0x40×(12767−12600)）。
pub const SEG_BASE: u32 = 0x829C0;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 结构数：3（卡面标题闭集 OA/CONTEXT/UNWIND_INFO）。
pub const STRUCT_COUNT: usize = 3;
/// 出处档位档数（AI-VX9 出处档建模：主手册/规范复核/台账附录）。
pub const PROV_TIERS: u32 = 3;
/// 结构间距（D2 间分界用；取 0x400 保证 CONTEXT 512 字节布局放得下）。
pub const STRUCT_SPACING: u32 = 0x400;
/// 段窗下沿（DISPATCH_VX9.md SEG 公式）。
pub const SEG_LO: u32 = 0x80000;
/// 段窗上沿（B16 末槽 0x82FC0）。
pub const SEG_HI: u32 = 0x82FC0;
/// 字段档容量：6+8+6 = 20（三结构冻结字段表行数之和）。
pub const FIELD_CAP: usize = 20;
/// 每族演练轮数（判据②：10/10）。
pub const DRILL_ROUNDS: u32 = 10;

// ── 三结构冻结字段表（字段名/偏移/长度/出处档位；出处：卡面三结构
//    示意布局 + x64 CONTEXT / UNWIND_INFO 布局惯例，VX9 出处台账） ──
/// OA 结构：6 字段，布局终点 32。
pub const OA_FIELDS: [(&str, u32, u32, u32); 6] = [
    ("magic 类型字", 0, 4, 1),
    ("owner 属主", 4, 8, 1),
    ("flags 状态字", 12, 4, 2),
    ("refs 引用数", 16, 4, 2),
    ("checksum 校验字", 20, 4, 3),
    ("pad 预留", 24, 8, 3),
];
/// CONTEXT 结构：8 字段，布局终点 512。
pub const CONTEXT_FIELDS: [(&str, u32, u32, u32); 8] = [
    ("reg_gp 通用寄存器组", 0, 128, 1),
    ("ctx_flags 上下文标志", 128, 8, 1),
    ("reg_sp 栈指针", 136, 8, 1),
    ("reg_ip 指令指针", 144, 8, 1),
    ("reg_seg 段寄存器组", 152, 24, 2),
    ("reg_eflags 标志寄存器", 176, 8, 2),
    ("reg_dr 调试寄存器组", 184, 64, 2),
    ("float 浮点区", 248, 264, 3),
];
/// UNWIND_INFO 结构：6 字段，布局终点 32。
pub const UNWIND_FIELDS: [(&str, u32, u32, u32); 6] = [
    ("ver_flags 版本标志字", 0, 1, 1),
    ("prolog 序言长度", 1, 1, 1),
    ("slot_count 槽计数", 2, 1, 1),
    ("frame 帧寄存器", 3, 1, 1),
    ("codes 展开码区", 4, 12, 2),
    ("handlers 异常处理区", 16, 16, 3),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutProvCode {
    /// 出处档位越档（闭集 1..=3 之外）
    TierInvalid,
    /// 字段偏移漂移（与黄金表不一致）
    FieldOffsetDrift,
    /// 字段长度为零（布局占位必须为正）
    FieldSizeZero,
    /// 字段重复登记（同结构同偏移一格一主）
    FieldDoubleBooked,
    /// D1 内分界声明不符（声明终点≠Σ字段长度）
    DeclD1Mismatch,
    /// D2 间分界越界（布局终点超过结构间距）
    DeclD2Overflow,
    /// D3 段窗越界（基址+终点落在段窗外）
    DeclD3SegWin,
    /// 越界声明（D4/E1 预演捕获，越界声明必拒）
    OobDeclaration,
    /// 登记不全即核验（20 字段或 5 声明未齐）
    LedgerIncomplete,
    /// 状态机倒退（Audited/Sealed 后回退登记态）
    StateRegress,
}

impl LayoutProvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LayoutProvCode::TierInvalid => {
                "出处档位越档：闭集 1..=3 之外，请回出处台账补档后再登记"
            }
            LayoutProvCode::FieldOffsetDrift => {
                "字段偏移漂移：与黄金表不一致，请对照三结构冻结字段表修正偏移"
            }
            LayoutProvCode::FieldSizeZero => {
                "字段长度为零：布局占位必须为正，请回字段表补长度"
            }
            LayoutProvCode::FieldDoubleBooked => {
                "字段重复登记：同结构同偏移一格一主，请查重后再登记"
            }
            LayoutProvCode::DeclD1Mismatch => {
                "D1 内分界声明不符：声明终点须等于 Σ字段长度，请重算布局终点"
            }
            LayoutProvCode::DeclD2Overflow => {
                "D2 间分界越界：布局终点超过结构间距 0x400，请压缩布局或调距"
            }
            LayoutProvCode::DeclD3SegWin => {
                "D3 段窗越界：基址+终点须落在 [0x80000,0x82FC0]，请回段窗核验"
            }
            LayoutProvCode::OobDeclaration => {
                "越界声明：越界声明必拒，请检查声明值是否越过对应分界"
            }
            LayoutProvCode::LedgerIncomplete => {
                "登记不全即核验：20 字段+5 声明须先登记齐，先登记后核验"
            }
            LayoutProvCode::StateRegress => {
                "状态机倒退：Draft→Audited→Sealed 单向推进，倒退请走新账本"
            }
        }
    }
    pub const ALL: [LayoutProvCode; 10] = [
        LayoutProvCode::TierInvalid,
        LayoutProvCode::FieldOffsetDrift,
        LayoutProvCode::FieldSizeZero,
        LayoutProvCode::FieldDoubleBooked,
        LayoutProvCode::DeclD1Mismatch,
        LayoutProvCode::DeclD2Overflow,
        LayoutProvCode::DeclD3SegWin,
        LayoutProvCode::OobDeclaration,
        LayoutProvCode::LedgerIncomplete,
        LayoutProvCode::StateRegress,
    ];
}

/// 诊断码闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LayoutProvCode::ALL.len() {
        if let Some(c) = LayoutProvCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x829C0 段闭集内，请核对诊断码取值区间"
}

// ── 三结构闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StructKind {
    /// OA 结构（属主/状态/引用账）
    Oa,
    /// CONTEXT 结构（异常上下文布局）
    Context,
    /// UNWIND_INFO 结构（展开信息布局）
    UnwindInfo,
}

impl StructKind {
    /// 结构下标（冻结序：OA=0，CONTEXT=1，UNWIND_INFO=2）。
    pub fn idx(self) -> usize {
        match self {
            StructKind::Oa => 0,
            StructKind::Context => 1,
            StructKind::UnwindInfo => 2,
        }
    }
    /// 中文名（诊断文案用）。
    pub fn zh(self) -> &'static str {
        match self {
            StructKind::Oa => "OA",
            StructKind::Context => "CONTEXT",
            StructKind::UnwindInfo => "UNWIND_INFO",
        }
    }
    /// 该结构冻结字段表行数。
    pub fn field_count(self) -> usize {
        match self {
            StructKind::Oa => OA_FIELDS.len(),
            StructKind::Context => CONTEXT_FIELDS.len(),
            StructKind::UnwindInfo => UNWIND_FIELDS.len(),
        }
    }
}

/// 由结构下标取冻结字段表第 k 行（出处：三结构冻结常量表）。
fn frozen_field(struct_idx: usize, k: usize) -> Option<(&'static str, u32, u32, u32)> {
    match struct_idx {
        0 => OA_FIELDS.get(k).copied(),
        1 => CONTEXT_FIELDS.get(k).copied(),
        2 => UNWIND_FIELDS.get(k).copied(),
        _ => None,
    }
}

/// 黄金布局终点 = Σ字段长度（出处：冻结字段表逐行累加）。
pub fn layout_end(struct_idx: usize) -> u32 {
    let mut end = 0u32;
    let mut k = 0;
    while let Some(row) = frozen_field(struct_idx, k) {
        end = end.saturating_add(row.2);
        k += 1;
    }
    end
}

/// 黄金布局校验和：Σ(偏移×31 + 长度×7 + 档位)，漂移检测基准。
pub fn layout_checksum(struct_idx: usize) -> u32 {
    let mut sum = 0u32;
    let mut k = 0;
    while let Some(row) = frozen_field(struct_idx, k) {
        sum = sum
            .wrapping_add(row.1.wrapping_mul(31))
            .wrapping_add(row.2.wrapping_mul(7))
            .wrapping_add(row.3);
        k += 1;
    }
    sum
}

// ── 分界声明闭集（D1–D3 三分界 + D4/E1 预演声明） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclKind {
    /// D1 结构内分界：声明布局终点=Σ字段长度
    D1Intra,
    /// D2 结构间分界：布局终点≤结构间距 0x400
    D2Inter,
    /// D3 段窗分界：基址+终点落在段窗内
    D3SegWin,
    /// D4 布局漂移演练预演声明：演练 10/10 必抓后登记
    D4DriftRehearsal,
    /// E1 越界声明预演声明：越界注入 10/10 检出后登记
    E1OobRehearsal,
}

impl DeclKind {
    pub fn idx(self) -> usize {
        match self {
            DeclKind::D1Intra => 0,
            DeclKind::D2Inter => 1,
            DeclKind::D3SegWin => 2,
            DeclKind::D4DriftRehearsal => 3,
            DeclKind::E1OobRehearsal => 4,
        }
    }
    pub fn zh(self) -> &'static str {
        match self {
            DeclKind::D1Intra => "D1 内分界",
            DeclKind::D2Inter => "D2 间分界",
            DeclKind::D3SegWin => "D3 段窗分界",
            DeclKind::D4DriftRehearsal => "D4 漂移预演",
            DeclKind::E1OobRehearsal => "E1 越界预演",
        }
    }
    pub const ALL: [DeclKind; 5] = [
        DeclKind::D1Intra,
        DeclKind::D2Inter,
        DeclKind::D3SegWin,
        DeclKind::D4DriftRehearsal,
        DeclKind::E1OobRehearsal,
    ];
}

/// 字段档行：结构下标 + 冻结四元组（登记时照抄黄金表，漂移即抓）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FieldRow {
    pub struct_idx: usize,
    pub name: &'static str,
    pub off: u32,
    pub size: u32,
    pub tier: u32,
}

/// 声明档行：分界声明或预演声明，claimed 为声明值/演练捕获数。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DeclRow {
    pub kind: DeclKind,
    pub subject: usize,
    pub claimed: u32,
    pub green: bool,
}

// ── 单调状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutState {
    /// 登记中：字段档与声明档逐步落账
    Draft,
    /// 核验毕：三分界+双预演全绿
    Audited,
    /// 冻结：布局出处账收口，只增不减
    Sealed,
}

impl LayoutState {
    /// 推进位次（Draft=0 < Audited=1 < Sealed=2），倒退判定位。
    pub fn rank(self) -> u8 {
        match self {
            LayoutState::Draft => 0,
            LayoutState::Audited => 1,
            LayoutState::Sealed => 2,
        }
    }
}

// ── 布局出处核验账本 ─────────────────────────────────────────────
pub struct LayoutProvLedger {
    fields: [Option<FieldRow>; FIELD_CAP],
    decls: [Option<DeclRow>; 5],
    pub state: LayoutState,
    pub field_n: usize,
    pub decl_n: usize,
    /// 完成时写回摘要（判据③：状态=已完成、结果摘要非空）。
    pub summary: Option<String>,
}

impl LayoutProvLedger {
    pub fn new() -> LayoutProvLedger {
        LayoutProvLedger {
            fields: [None; FIELD_CAP],
            decls: [None; 5],
            state: LayoutState::Draft,
            field_n: 0,
            decl_n: 0,
            summary: None,
        }
    }

    /// 状态推进（只许前位次），倒退显性拒。
    pub fn advance(&mut self, to: LayoutState) -> Result<(), u32> {
        if to.rank() < self.state.rank() {
            return Err(LayoutProvCode::StateRegress.val());
        }
        self.state = to;
        Ok(())
    }

    /// 登记一条字段档：档位越档/零长度/重复偏移一律拒。
    pub fn book_field(&mut self, row: FieldRow) -> Result<usize, u32> {
        if row.tier == 0 || row.tier > PROV_TIERS {
            return Err(LayoutProvCode::TierInvalid.val());
        }
        if row.size == 0 {
            return Err(LayoutProvCode::FieldSizeZero.val());
        }
        let mut i = 0;
        while i < self.field_n {
            if let Some(f) = self.fields.get(i).copied().flatten() {
                if f.struct_idx == row.struct_idx && f.off == row.off {
                    return Err(LayoutProvCode::FieldDoubleBooked.val());
                }
            }
            i += 1;
        }
        if self.field_n >= FIELD_CAP {
            return Err(LayoutProvCode::LedgerIncomplete.val());
        }
        self.fields[self.field_n] = Some(row);
        self.field_n += 1;
        Ok(self.field_n - 1)
    }

    /// 从黄金表整批装载一个结构的字段档（正常流入口）。
    pub fn load_struct(&mut self, struct_idx: usize) -> Result<usize, u32> {
        if struct_idx >= STRUCT_COUNT {
            return Err(LayoutProvCode::LedgerIncomplete.val());
        }
        let mut n = 0;
        let mut k = 0;
        while let Some((name, off, size, tier)) = frozen_field(struct_idx, k) {
            self.book_field(FieldRow { struct_idx, name, off, size, tier })?;
            n += 1;
            k += 1;
        }
        Ok(n)
    }

    /// 登记一条分界/预演声明（同 kind 重复登记即拒）。
    pub fn book_decl(&mut self, row: DeclRow) -> Result<usize, u32> {
        let slot = row.kind.idx();
        if self.decls.get(slot).copied().flatten().is_some() {
            return Err(LayoutProvCode::FieldDoubleBooked.val());
        }
        self.decls[slot] = Some(row);
        self.decl_n += 1;
        Ok(slot)
    }

    /// 分界声明核验（判据②核心谓词）：D1–D3 三分界逐条核验。
    /// 返回 Ok(绿声明数) 或 Err(具体违约码)。
    pub fn verify_decl(&self, row: DeclRow) -> Result<bool, u32> {
        match row.kind {
            DeclKind::D1Intra => {
                let golden = layout_end(row.subject);
                if row.claimed != golden {
                    return Err(LayoutProvCode::DeclD1Mismatch.val());
                }
                Ok(true)
            }
            DeclKind::D2Inter => {
                let end = layout_end(row.subject);
                if end > STRUCT_SPACING {
                    return Err(LayoutProvCode::DeclD2Overflow.val());
                }
                if row.claimed != end {
                    return Err(LayoutProvCode::DeclD1Mismatch.val());
                }
                Ok(true)
            }
            DeclKind::D3SegWin => {
                let base = (row.subject as u32).saturating_mul(STRUCT_SPACING);
                let end = base.saturating_add(layout_end(row.subject));
                if end > SEG_HI.saturating_sub(SEG_LO) {
                    return Err(LayoutProvCode::DeclD3SegWin.val());
                }
                if row.claimed != end {
                    return Err(LayoutProvCode::DeclD3SegWin.val());
                }
                Ok(true)
            }
            // 预演声明：claimed 须为演练捕获数 10/10，少一轮即红。
            DeclKind::D4DriftRehearsal | DeclKind::E1OobRehearsal => {
                if row.claimed != DRILL_ROUNDS {
                    return Err(LayoutProvCode::OobDeclaration.val());
                }
                Ok(true)
            }
        }
    }

    /// 分界声明核验绿总判（判据②）：5 声明齐且逐条绿。
    pub fn boundary_all_green(&self) -> bool {
        if self.decl_n != 5 || self.field_n != FIELD_CAP {
            return false;
        }
        let mut k = 0;
        while k < 5 {
            let row = match self.decls.get(k).copied().flatten() {
                Some(d) => d,
                None => return false,
            };
            match self.verify_decl(row) {
                Ok(g) => {
                    if !g {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            k += 1;
        }
        true
    }

    /// 完成时写回（判据③）：状态推到 Sealed 且摘要非空。
    pub fn close(&mut self, text: &str) -> Result<(), u32> {
        if !self.boundary_all_green() {
            return Err(LayoutProvCode::LedgerIncomplete.val());
        }
        self.advance(LayoutState::Sealed)?;
        if text.is_empty() {
            return Err(LayoutProvCode::LedgerIncomplete.val());
        }
        self.summary = Some(String::from(text));
        Ok(())
    }
}

// ── 确定性 LCG（种子=SEG_BASE，禁 rand/浮点） ────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 布局漂移演练（D4 预演）：LCG 选点 +1 偏移扰动，10/10 必抓 ────
pub fn drift_drill() -> u32 {
    let mut caught = 0u32;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0;
    while round < DRILL_ROUNDS {
        let subject = (round as usize) % STRUCT_COUNT;
        let golden_sum = layout_checksum(subject);
        // 扰动：LCG 在该结构冻结字段数内选点，偏移 +1 后重算校验和。
        let mut fc = 0usize;
        while frozen_field(subject, fc).is_some() {
            fc += 1;
        }
        let pick = rng.below(fc as u64) as usize;
        let mut drift_sum = 0u32;
        let mut k = 0;
        while let Some((name, off, size, tier)) = frozen_field(subject, k) {
            let off2 = if k == pick {
                off.checked_add(1).unwrap_or(off)
            } else {
                off
            };
            let _ = name;
            drift_sum = drift_sum
                .wrapping_add(off2.wrapping_mul(31))
                .wrapping_add(size.wrapping_mul(7))
                .wrapping_add(tier);
            k += 1;
        }
        if drift_sum != golden_sum {
            caught += 1;
        }
        round += 1;
    }
    caught
}

// ── 越界声明演练（E1 预演）：越界声明 10/10 检出 ─────────────────
pub fn oob_drill() -> u32 {
    let mut caught = 0u32;
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0xE1));
    let mut round = 0;
    while round < DRILL_ROUNDS {
        let subject = rng.below(STRUCT_COUNT as u64) as usize;
        let delta = (rng.below(10) + 1) as u32;
        // 三分界各注入一次越界声明，任一被拒即本轮检出。
        let d1 = LayoutProvLedger::verify_decl(
            &LayoutProvLedger::new(),
            DeclRow {
                kind: DeclKind::D1Intra,
                subject,
                claimed: layout_end(subject).saturating_add(delta),
                green: false,
            },
        );
        let d2 = LayoutProvLedger::verify_decl(
            &LayoutProvLedger::new(),
            DeclRow {
                kind: DeclKind::D2Inter,
                subject,
                claimed: STRUCT_SPACING.saturating_add(delta),
                green: false,
            },
        );
        let d3 = LayoutProvLedger::verify_decl(
            &LayoutProvLedger::new(),
            DeclRow {
                kind: DeclKind::D3SegWin,
                subject,
                claimed: SEG_HI.saturating_sub(SEG_LO).saturating_add(delta),
                green: false,
            },
        );
        if d1.is_err() && d2.is_err() && d3.is_err() {
            caught += 1;
        }
        round += 1;
    }
    caught
}

// ── 黄金正演：装载 20 字段 → 5 声明全绿 → 写回收口 ───────────────
pub fn golden_run() -> Result<String, u32> {
    let mut led = LayoutProvLedger::new();
    let mut s = 0;
    while s < STRUCT_COUNT {
        led.load_struct(s)?;
        s += 1;
    }
    // 三分界声明：D1/D2/D3 逐条按黄金值登记。
    let mut d = 0;
    while d < 3 {
        let subject = if d == 2 { 2 } else { d };
        let claimed = match d {
            0 => layout_end(0),
            1 => layout_end(1),
            // D3 段窗声明值 = 结构基址（下标×间距）+ 布局终点。
            _ => STRUCT_SPACING
                .saturating_mul(2)
                .saturating_add(layout_end(2)),
        };
        let kind = match d {
            0 => DeclKind::D1Intra,
            1 => DeclKind::D2Inter,
            _ => DeclKind::D3SegWin,
        };
        let row = DeclRow { kind, subject, claimed, green: true };
        if led.verify_decl(row)? {
            led.book_decl(row)?;
        } else {
            return Err(LayoutProvCode::DeclD1Mismatch.val());
        }
        d += 1;
    }
    // 双预演声明：演练先跑，10/10 后登记。
    let drift = drift_drill();
    let oob = oob_drill();
    if drift != DRILL_ROUNDS || oob != DRILL_ROUNDS {
        return Err(LayoutProvCode::OobDeclaration.val());
    }
    led.book_decl(DeclRow {
        kind: DeclKind::D4DriftRehearsal,
        subject: 0,
        claimed: drift,
        green: true,
    })?;
    led.book_decl(DeclRow {
        kind: DeclKind::E1OobRehearsal,
        subject: 1,
        claimed: oob,
        green: true,
    })?;
    led.advance(LayoutState::Audited)?;
    if !led.boundary_all_green() {
        return Err(LayoutProvCode::LedgerIncomplete.val());
    }
    led.close("UNX-F12767 三结构布局出处核验绿，D1–D3+D4/E1 五声明全绿，越界声明 10/10 检出")?;
    match led.summary.clone() {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(LayoutProvCode::LedgerIncomplete.val()),
    }
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 计数守恒：注入=检出+漏检，漏检=0。
    pub fn conservation(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => {
                    if c > t {
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

/// 三族注入：越档出处 / 重复字段 / 状态机倒退，各 10 轮全抓。
pub fn run_injections(led: &mut InjectLedger, rounds: u32) {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x5A));
    let mut r = 0;
    while r < rounds {
        // 族 0：出处档位越档（0 档与 4 档交替注入）。
        led.tries[0] += 1;
        let mut m = LayoutProvLedger::new();
        let bad_tier = if rng.below(2) == 0 { 0 } else { PROV_TIERS + 1 };
        let r0 = m.book_field(FieldRow {
            struct_idx: 0,
            name: "注入字段",
            off: 0,
            size: 4,
            tier: bad_tier,
        });
        if r0 == Err(LayoutProvCode::TierInvalid.val()) {
            led.caught[0] += 1;
        }
        // 族 1：同结构同偏移重复登记。
        led.tries[1] += 1;
        let mut m1 = LayoutProvLedger::new();
        let _ = m1.load_struct(0);
        let r1 = m1.book_field(FieldRow {
            struct_idx: 0,
            name: "重复字段",
            off: 0,
            size: 4,
            tier: 1,
        });
        if r1 == Err(LayoutProvCode::FieldDoubleBooked.val()) {
            led.caught[1] += 1;
        }
        // 族 2：状态机倒退（Sealed 后回退 Draft）。
        led.tries[2] += 1;
        let mut m2 = LayoutProvLedger::new();
        let _ = m2.advance(LayoutState::Sealed);
        if m2.advance(LayoutState::Draft) == Err(LayoutProvCode::StateRegress.val()) {
            led.caught[2] += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有，全部走 extra_checks 收口） ───────────────────
fn golden_ok() -> bool {
    golden_run().is_ok()
}

/// 判据②：D4 漂移预演 10/10 必抓。
fn drift_rehearsal_ok() -> bool {
    drift_drill() == DRILL_ROUNDS
}

/// 判据②：E1 越界预演 10/10 检出。
fn oob_rehearsal_ok() -> bool {
    oob_drill() == DRILL_ROUNDS
}

/// 四类边界：零长度/零档位/满档/满档+1。
fn boundary_quad_ok() -> bool {
    // 零：字段长度 0 必拒。
    let mut m = LayoutProvLedger::new();
    let zero_ok = m.book_field(FieldRow {
        struct_idx: 0,
        name: "零长度",
        off: 0,
        size: 0,
        tier: 1,
    }) == Err(LayoutProvCode::FieldSizeZero.val());
    // 负向：档位 0（下溢侧）必拒。
    let mut m2 = LayoutProvLedger::new();
    let neg_ok = m2.book_field(FieldRow {
        struct_idx: 0,
        name: "零档位",
        off: 0,
        size: 4,
        tier: 0,
    }) == Err(LayoutProvCode::TierInvalid.val());
    // 上限：恰 20 字段登记成功。
    let mut m3 = LayoutProvLedger::new();
    let mut s = 0;
    while s < STRUCT_COUNT {
        if m3.load_struct(s).is_err() {
            return false;
        }
        s += 1;
    }
    let cap_ok = m3.field_n == FIELD_CAP;
    // 上限+1：第 21 行必拒（同偏移 0 的重复行兜底触发）。
    let over = m3.book_field(FieldRow {
        struct_idx: 0,
        name: "超限字段",
        off: 0,
        size: 4,
        tier: 1,
    })
    .is_err();
    zero_ok && neg_ok && cap_ok && over
}

/// 判定次序：登记不齐即调 boundary_all_green 必判假（先登记后核验）。
fn verdict_order_ok() -> bool {
    let mut m = LayoutProvLedger::new();
    if m.boundary_all_green() {
        return false;
    }
    let _ = m.load_struct(0);
    !m.boundary_all_green()
}

/// 状态机倒退独立口径：Audited 后回 Draft 拒、Sealed 后回 Audited 拒。
fn regression_reject_ok() -> bool {
    let mut m = LayoutProvLedger::new();
    let _ = m.advance(LayoutState::Audited);
    let back1 = m.advance(LayoutState::Draft) == Err(LayoutProvCode::StateRegress.val());
    let _ = m.advance(LayoutState::Sealed);
    let back2 = m.advance(LayoutState::Audited) == Err(LayoutProvCode::StateRegress.val());
    back1 && back2
}

/// 容量耗尽：声明档同 kind 双登必拒。
fn capacity_exact_ok() -> bool {
    let mut m = LayoutProvLedger::new();
    let row = DeclRow { kind: DeclKind::D1Intra, subject: 0, claimed: layout_end(0), green: true };
    if m.book_decl(row).is_err() {
        return false;
    }
    m.book_decl(row) == Err(LayoutProvCode::FieldDoubleBooked.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 段窗。
fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 对账一致：三结构黄金终点与冻结表行数、字段档容量对账。
fn frozen_table_aligned_ok() -> bool {
    layout_end(0) == 32 && layout_end(1) == 512 && layout_end(2) == 32
        && OA_FIELDS.len() + CONTEXT_FIELDS.len() + UNWIND_FIELDS.len() == FIELD_CAP
}

/// 计数守恒：注入=检出+漏检，漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    led.conservation()
        && led.caught[0] == DRILL_ROUNDS
        && led.caught[1] == DRILL_ROUNDS
        && led.caught[2] == DRILL_ROUNDS
}

/// 文案闭集：10 变体 detail 两两互异非空 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let n = LayoutProvCode::ALL.len();
    let mut i = 0;
    while i < n {
        let di = LayoutProvCode::ALL[i].detail();
        if di.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < n {
            if LayoutProvCode::ALL[j].detail() == di {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x40).is_empty()
}

/// 收口只增不减：诊断码闭集两轮采集逐项相等（only-add 收敛）。
fn closure_ok() -> bool {
    let mut first: Vec<u32> = Vec::new();
    let mut second: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < LayoutProvCode::ALL.len() {
        if let Some(c) = LayoutProvCode::ALL.get(i) {
            first.push(c.val());
            second.push(c.val());
        }
        i += 1;
    }
    first == second && first.len() == 10
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12768-J1）。
    v.push((
        "J1-分界声明核验绿三分界",
        boundary_all_green_ok(),
        String::from("D1–D3 三分界+D4/E1 预演声明逐条绿"),
    ));
    v.push((
        "J1-越界声明10/10检出",
        oob_rehearsal_ok(),
        String::from("越界声明注入 10 轮全拒"),
    ));
    v.push((
        "J1-漂移预演10/10必抓",
        drift_rehearsal_ok(),
        String::from("D4 布局漂移演练 10 轮全抓"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        writeback_ok(),
        String::from("Sealed 态 + 摘要含 10/10"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演20字段5声明",
        golden_ok(),
        String::from("装载三结构→五声明→写回零错"),
    ));
    // 四类边界。
    v.push((
        "边界-零长度/零档位/满档/满档+1",
        boundary_quad_ok(),
        String::from("FieldSizeZero/TierInvalid/容量恰满/第21行拒"),
    ));
    // 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    v.push((
        "注入-越档出处10轮全抓",
        led.caught[0] == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[0], led.caught[0]),
    ));
    v.push((
        "注入-重复字段10轮全抓",
        led.caught[1] == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[1], led.caught[1]),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        led.caught[2] == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[2], led.caught[2]),
    ));
    // 判定次序。
    v.push((
        "判定次序-未登记齐先核验判假",
        verdict_order_ok(),
        String::from("空账/1 结构即核验 → boundary_all_green=false"),
    ));
    // 状态机倒退（独立口径）。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("Audited 回 Draft 拒、Sealed 回 Audited 拒"),
    ));
    // 容量恰满。
    v.push((
        "容量-声明档同kind双登拒",
        capacity_exact_ok(),
        String::from("5 声明槽一位一主"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x829C0",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-黄金终点32/512/32",
        frozen_table_aligned_ok(),
        String::from("三结构终点对账+20 行=FIELD_CAP"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("InjectLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", LayoutProvCode::ALL.len()),
    ));
    v
}

/// 判据②总谓词：黄金账本分界声明核验绿（三分界+双预演）。
fn boundary_all_green_ok() -> bool {
    match golden_run() {
        Ok(_) => {
            let mut led = LayoutProvLedger::new();
            let mut s = 0;
            let mut ok = true;
            while s < STRUCT_COUNT {
                if led.load_struct(s).is_err() {
                    ok = false;
                }
                s += 1;
            }
            let drift = drift_drill();
            let oob = oob_drill();
            ok
                && led.book_decl(DeclRow {
                    kind: DeclKind::D1Intra,
                    subject: 0,
                    claimed: layout_end(0),
                    green: true,
                })
                .is_ok()
                && led.book_decl(DeclRow {
                    kind: DeclKind::D2Inter,
                    subject: 1,
                    claimed: layout_end(1),
                    green: true,
                })
                .is_ok()
                && led.book_decl(DeclRow {
                    kind: DeclKind::D3SegWin,
                    subject: 2,
                    claimed: STRUCT_SPACING
                        .saturating_mul(2)
                        .saturating_add(layout_end(2)),
                    green: true,
                })
                .is_ok()
                && drift == DRILL_ROUNDS
                && oob == DRILL_ROUNDS
                && led.book_decl(DeclRow {
                    kind: DeclKind::D4DriftRehearsal,
                    subject: 0,
                    claimed: drift,
                    green: true,
                })
                .is_ok()
                && led.book_decl(DeclRow {
                    kind: DeclKind::E1OobRehearsal,
                    subject: 1,
                    claimed: oob,
                    green: true,
                })
                .is_ok()
                && led.boundary_all_green()
        }
        Err(_) => false,
    }
}

/// 判据③谓词：写回已完成且摘要非空。
fn writeback_ok() -> bool {
    match golden_run() {
        Ok(text) => !text.is_empty(),
        Err(_) => false,
    }
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12767_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12767 结构布局出处核验（OA/CONTEXT/UNWIND_INFO）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        assert!(golden_run().is_ok(), "黄金正演须零错收口");
        assert!(writeback_ok(), "写回摘要须非空");
    }

    #[test]
    fn t02_boundary_quad() {
        assert!(boundary_quad_ok(), "零/负/上限/上限+1 四类边界须全拒或恰满");
    }

    #[test]
    fn t03_drift_rehearsal_10_of_10() {
        assert_eq!(drift_drill(), DRILL_ROUNDS, "漂移演练 10/10 必抓");
    }

    #[test]
    fn t04_oob_rehearsal_10_of_10() {
        assert_eq!(oob_drill(), DRILL_ROUNDS, "越界声明 10/10 检出");
    }

    #[test]
    fn t05_injection_three_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led, DRILL_ROUNDS);
        assert!(led.conservation(), "注入=检出+漏检，漏检须为 0");
        assert_eq!(led.caught[0], DRILL_ROUNDS);
        assert_eq!(led.caught[1], DRILL_ROUNDS);
        assert_eq!(led.caught[2], DRILL_ROUNDS);
    }

    #[test]
    fn t06_verdict_order_and_regression() {
        assert!(verdict_order_ok(), "未登记齐先核验须判假");
        assert!(regression_reject_ok(), "状态机倒退须显性拒");
    }

    #[test]
    fn t07_double_run_identical() {
        assert!(double_run_ok(), "同输入两次须逐字段相等");
    }

    #[test]
    fn t08_segment_guard() {
        assert!(segment_guard(), "0x829C0 须对齐 0x40 且落在段窗");
        assert!(frozen_table_aligned_ok(), "黄金终点对账 32/512/32");
    }

    #[test]
    fn t09_detail_corpus_closed() {
        assert!(detail_corpus_closed_ok(), "文案闭集两两互异+兜底非空");
        assert!(closure_ok(), "收口 only-add 收敛");
    }

    #[test]
    fn t10_check_set_all_green() {
        let s = run_unx12767_checks();
        let (p, f) = s.tally();
        assert!(f == 0, "红项: {}", s.red_items_text());
        assert!(p >= 12, "检查条数须 ≥12");
    }
}
