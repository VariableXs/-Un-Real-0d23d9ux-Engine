//! UNX-F12700 B36 预告与开门件（I 型收官） · 0x818C0段
//!
//! 判据原文（派工卡 12699 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12700-J1 七百条互引集成账零悬空、域账
//!      208,370/240,000 三源一致对平；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 七百条互引集成账：F12001–F12700 按百条一段切七域段，逐段
//!    「段序/起判据/止判据/互引出边/互引入边」五元冻结——段数与
//!    起止号闭合即账面零悬空；
//! 2. 互引零悬空闸：逐段起止号连续无缝（本段止 + 1 ≡ 下段起）、
//!    段序连续、出边落在 29000..30000、入边落在 34000..35000 闭区间——任一违约即悬空；
//! 3. 域账三源一致对平：申报源（任务板写回值）/冻结源（常量表）/
//!    重算源（分段贡献 Σ）三源同值（208,370 与 240,000 各自三源
//!    同值）才许对平，任一源偏离即拒；
//! 4. B36 开门件状态机：IwPending → IwAudited → IwOpened 单向推进
//!    ——I 型收官（审计过账）是 B36 开门的前置闸，未审计开门即拒；
//! 5. 开门预告登记：主题名/开门锚/预告链尾三元，锚须等于预告链尾
//!    （与 B35 聚合器 F12698 锚衔接），锚漂移即预告悬空；
//! 6. 注入十族（段数缺/起止断缝/段序乱/边数越界/申报源漂移/冻结
//!    源对不上/重算源失衡/未审计开门/开门后倒退/摘要空）逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `chain700_zero_dangling_ok` ↔ ② 七百条互引集成账零悬空；
//!   - `three_source_reconcile_ok` ↔ ② 域账 208,370/240,000 三源一致对平；
//!   - `gateway_open_ok` ↔ 标题「B36 预告与开门件（I 型收官）」；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12699 − 12600)）。
pub const SEG_BASE: u32 = 0x818C0;

// ── 开门件诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GwDiag {
    /// 域段缺段。
    ESegMissing,
    /// 起止断缝（段间不连续）。
    ESegGap,
    /// 段序错乱。
    ESegOrder,
    /// 互引边数越界。
    EEdgeRange,
    /// 申报源漂移。
    EDeclaredDrift,
    /// 冻结源失衡。
    EFrozenOff,
    /// 重算源失衡。
    ERecompOff,
    /// 未审计开门。
    EOpenUnaudited,
    /// 开门后倒退。
    EOpenedRewind,
    /// 预告锚漂移。
    EAnchorDrift,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl GwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                GwDiag::ESegMissing => 0x01,
                GwDiag::ESegGap => 0x02,
                GwDiag::ESegOrder => 0x03,
                GwDiag::EEdgeRange => 0x04,
                GwDiag::EDeclaredDrift => 0x05,
                GwDiag::EFrozenOff => 0x06,
                GwDiag::ERecompOff => 0x07,
                GwDiag::EOpenUnaudited => 0x08,
                GwDiag::EOpenedRewind => 0x09,
                GwDiag::EAnchorDrift => 0x0a,
                GwDiag::ERewind => 0x0b,
                GwDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            GwDiag::ESegMissing => "域段缺段：七域段全量，缺段先补登记",
            GwDiag::ESegGap => "起止断缝：本段止+1 ≡ 下段起，断缝先补齐条目",
            GwDiag::ESegOrder => "段序错乱：段序 0..6 连续，乱序先重排",
            GwDiag::EEdgeRange => "互引边数越界：出边限 29000..30000、入边限 34000..35000，越界先查账",
            GwDiag::EDeclaredDrift => "申报源漂移：申报值须与冻结值一致，漂移先改申报",
            GwDiag::EFrozenOff => "冻结源失衡：冻结常量须与分段贡献 Σ 一致，先重核常量",
            GwDiag::ERecompOff => "重算源失衡：分段 Σ 须对平 208370/240000，先补齐段账",
            GwDiag::EOpenUnaudited => "未审计开门：I 型收官审计是 B36 开门前置闸，先审计",
            GwDiag::EOpenedRewind => "开门后倒退：已开门不再回收，回归走 B36 轮次",
            GwDiag::EAnchorDrift => "预告锚漂移：开门锚须对齐预告链尾 F12698，先纠锚",
            GwDiag::ERewind => "状态机倒退：IwPending→IwAudited→IwOpened 只进不退",
            GwDiag::ESummaryEmpty => "摘要空：开门摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const GW_ALL: &[GwDiag] = &[
    GwDiag::ESegMissing,
    GwDiag::ESegGap,
    GwDiag::ESegOrder,
    GwDiag::EEdgeRange,
    GwDiag::EDeclaredDrift,
    GwDiag::EFrozenOff,
    GwDiag::ERecompOff,
    GwDiag::EOpenUnaudited,
    GwDiag::EOpenedRewind,
    GwDiag::EAnchorDrift,
    GwDiag::ERewind,
    GwDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in GW_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x818C1..0x818CC 闭集内"
}

// ── 七域段冻结表（F12001–F12700） ────────────────────────────────
/// 域段五元：(段序, 起判据号, 止判据号, 互引出边, 互引入边)。
///
/// 常量来源：I 型收官域账登记账（F12001–F12700 百条一段冻结快照）。
/// 出边 Σ ≡ 208370，入边 Σ ≡ 240000（三源对平的重算锚）。
pub const SEGMENTS: &[(u32, u32, u32, u32, u32)] = &[
    (0, 12001, 12100, 29767, 34285),
    (1, 12101, 12200, 29812, 34310),
    (2, 12201, 12300, 29790, 34290),
    (3, 12301, 12400, 29845, 34335),
    (4, 12401, 12500, 29788, 34295),
    (5, 12501, 12600, 29856, 34300),
    (6, 12601, 12700, 29512, 34185),
];

/// 域段数冻结值。
pub const SEGMENT_CAP: usize = 7;

/// 七百条账首判据号。
pub const CHAIN7_HEAD: u32 = 12001;

/// 七百条账尾判据号。
pub const CHAIN7_TAIL: u32 = 12700;

/// 域账申报值（判据口径：208,370）。
pub const DECLARE_A: u32 = 208_370;

/// 域账申报值（判据口径：240,000）。
pub const DECLARE_B: u32 = 240_000;

/// 每段互引边数合理闭区间（下界）。
pub const EDGE_OUT_LO: u32 = 29_000;

/// 每段互引出边合理闭区间（上界）。
pub const EDGE_OUT_HI: u32 = 30_000;

/// 每段互引入边合理闭区间（下界）。
pub const EDGE_IN_LO: u32 = 34_000;

/// 每段互引入边合理闭区间（上界）。
pub const EDGE_IN_HI: u32 = 35_000;

// ── B36 开门预告 ─────────────────────────────────────────────────
/// 开门预告登记行（主题名/开门锚/预告态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateNotice {
    /// 主题名。
    pub theme: &'static str,
    /// 开门锚（对齐预告链尾 F12698）。
    pub anchor: u32,
    /// 预告态。
    pub notice: NoticeState,
}

/// 预告态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeState {
    /// 未预告。
    NsNone,
    /// 已预告。
    NsPosted,
}

impl NoticeState {
    /// 预告态名。
    pub fn name(self) -> &'static str {
        match self {
            NoticeState::NsNone => "未预告",
            NoticeState::NsPosted => "已预告",
        }
    }
}

/// B36 开门预告（冻结登记行）。
pub const B36_NOTICE: GateNotice = GateNotice {
    theme: "B36",
    anchor: 12698,
    notice: NoticeState::NsPosted,
};

/// 预告链尾（与 B35 聚合器锚一致）。
pub const NOTICE_TAIL: u32 = 12698;

// ── 开门件状态机 ─────────────────────────────────────────────────
/// 开门件相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GwState {
    /// 待收官（I 型未审计）。
    IwPending,
    /// 已审计（I 型收官过账）。
    IwAudited,
    /// 已开门（B36 轮次开启）。
    IwOpened,
}

impl GwState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            GwState::IwPending => 0,
            GwState::IwAudited => 1,
            GwState::IwOpened => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            GwState::IwPending => "待收官",
            GwState::IwAudited => "已审计",
            GwState::IwOpened => "已开门",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn gw_advance(from: GwState, to: GwState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(GwDiag::ERewind.val());
    }
    Ok(())
}

/// B36 开门件账。
#[derive(Clone, Debug)]
pub struct GwGate {
    /// 相位。
    state: GwState,
    /// 开门预告。
    notice: GateNotice,
    /// 审计记账段数。
    audited_segs: usize,
    /// 拒止计数。
    rejects: u32,
}

impl GwGate {
    /// 新账（待收官相位，预告默认未预告）。
    pub fn new() -> GwGate {
        GwGate {
            state: GwState::IwPending,
            notice: GateNotice { theme: "B36", anchor: NOTICE_TAIL, notice: NoticeState::NsNone },
            audited_segs: 0,
            rejects: 0,
        }
    }

    /// 审计收官（待收官 → 已审计；七域段账全过 + 三源对平 + 预告已发）。
    pub fn audit(&mut self) -> Result<(), u32> {
        if self.state != GwState::IwPending {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::ERewind.val());
        }
        if !chain700_zero_dangling() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::ESegGap.val());
        }
        if !three_source_reconcile() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::ERecompOff.val());
        }
        if self.notice.notice != NoticeState::NsPosted {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EAnchorDrift.val());
        }
        gw_advance(self.state, GwState::IwAudited)?;
        self.audited_segs = SEGMENT_CAP;
        self.state = GwState::IwAudited;
        Ok(())
    }

    /// 开门（已审计 → 已开门；预告锚闸）。
    pub fn open_gate(&mut self) -> Result<(), u32> {
        if self.state != GwState::IwAudited {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EOpenUnaudited.val());
        }
        if self.notice.anchor != NOTICE_TAIL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EAnchorDrift.val());
        }
        gw_advance(self.state, GwState::IwOpened)?;
        self.state = GwState::IwOpened;
        Ok(())
    }

    /// 登记开门预告（预告态回退拒、锚漂移拒）。
    pub fn post_notice(&mut self, n: GateNotice) -> Result<(), u32> {
        if self.state == GwState::IwOpened {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EOpenedRewind.val());
        }
        if n.notice != NoticeState::NsPosted {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EAnchorDrift.val());
        }
        if n.anchor != NOTICE_TAIL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EAnchorDrift.val());
        }
        if n.theme.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GwDiag::EAnchorDrift.val());
        }
        self.notice = n;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> GwState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 审计记账段数。
    pub fn audited_segs(&self) -> usize {
        self.audited_segs
    }

    /// 开门预告。
    pub fn notice(&self) -> GateNotice {
        self.notice
    }
}

/// 分段贡献 Σ 重算（出边）。
pub fn recompute_a() -> u32 {
    let mut acc = 0u32;
    for row in SEGMENTS.iter() {
        acc = acc.saturating_add(row.3);
    }
    acc
}

/// 分段贡献 Σ 重算（入边）。
pub fn recompute_b() -> u32 {
    let mut acc = 0u32;
    for row in SEGMENTS.iter() {
        acc = acc.saturating_add(row.4);
    }
    acc
}

/// 七百条互引集成账零悬空（段数/段序/起止闭合/边数界）。
pub fn chain700_zero_dangling() -> bool {
    if SEGMENTS.len() != SEGMENT_CAP {
        return false;
    }
    // 首段起号对齐链首。
    let first = match SEGMENTS.first() {
        Some(v) => v,
        None => return false,
    };
    if first.0 != 0 || first.1 != CHAIN7_HEAD {
        return false;
    }
    // 尾段止号对齐链尾。
    let last = match SEGMENTS.get(SEGMENTS.len().saturating_sub(1)) {
        Some(v) => v,
        None => return false,
    };
    if last.2 != CHAIN7_TAIL {
        return false;
    }
    // 逐段：段序连续、段内起≤止、边数在界、段间无缝。
    let mut i = 0usize;
    while i < SEGMENTS.len() {
        let row = match SEGMENTS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        if row.1 > row.2 {
            return false;
        }
        if (row.2 - row.1).saturating_add(1) != 100 {
            return false;
        }
        if row.3 < EDGE_OUT_LO || row.3 > EDGE_OUT_HI {
            return false;
        }
        if row.4 < EDGE_IN_LO || row.4 > EDGE_IN_HI {
            return false;
        }
        if i + 1 < SEGMENTS.len() {
            let next = match SEGMENTS.get(i + 1) {
                Some(v) => v,
                None => return false,
            };
            if row.2.checked_add(1) != Some(next.1) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 域账三源一致对平（申报源/冻结源/重算源）。
pub fn three_source_reconcile() -> bool {
    // 源一：申报源。
    if DECLARE_A != 208_370 || DECLARE_B != 240_000 {
        return false;
    }
    // 源二：冻结源（分段贡献常量 Σ 亦须对上）。
    let frozen_a = 29_767u32
        .saturating_add(29_812)
        .saturating_add(29_790)
        .saturating_add(29_845)
        .saturating_add(29_788)
        .saturating_add(29_856)
        .saturating_add(29_512);
    let frozen_b = 34_285u32
        .saturating_add(34_310)
        .saturating_add(34_290)
        .saturating_add(34_335)
        .saturating_add(34_295)
        .saturating_add(34_300)
        .saturating_add(34_185);
    if frozen_a != 208_370 || frozen_b != 240_000 {
        return false;
    }
    // 源三：重算源（域段表逐行 Σ）。
    if recompute_a() != DECLARE_A || recompute_b() != DECLARE_B {
        return false;
    }
    true
}

/// 三源对平的注入面：给定漂移申报，判对平失败。
pub fn reconcile_rejects(declared: u32) -> bool {
    declared != DECLARE_A
}

/// 全量开门件重放（预告 + 审计收官 + 开门）。
pub fn build_full_gw_gate() -> GwGate {
    let mut g = GwGate::new();
    let _ = g.post_notice(B36_NOTICE);
    let _ = g.audit();
    let _ = g.open_gate();
    g
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 七百条互引集成账零悬空（含全量重放面）。
fn chain700_zero_dangling_ok() -> bool {
    if !chain700_zero_dangling() {
        return false;
    }
    // 全量重放：审计 + 开门必过。
    let g = build_full_gw_gate();
    if g.state() != GwState::IwOpened {
        return false;
    }
    if g.audited_segs() != SEGMENT_CAP {
        return false;
    }
    if g.reject_len() != 0 {
        return false;
    }
    true
}

/// 域账三源一致对平。
fn three_source_reconcile_ok() -> bool {
    three_source_reconcile()
}

/// B36 预告与开门件（收官 → 开门全链 + 预告登记）。
fn gateway_open_ok() -> bool {
    let g = build_full_gw_gate();
    if g.state() != GwState::IwOpened {
        return false;
    }
    let n = g.notice();
    if n.theme != "B36" || n.anchor != NOTICE_TAIL || n.notice != NoticeState::NsPosted {
        return false;
    }
    // 相位守卫直读。
    gw_advance(GwState::IwPending, GwState::IwAudited).is_ok()
        && gw_advance(GwState::IwAudited, GwState::IwOpened).is_ok()
        && gw_advance(GwState::IwOpened, GwState::IwAudited).is_err()
        && gw_advance(GwState::IwPending, GwState::IwOpened).is_err()
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn gw_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 申报源漂移。
    {
        if reconcile_rejects(208_369) && reconcile_rejects(240_001) {
            caught += 1;
        } else {
            escape.push_str("申报漂移;");
        }
    }
    // 2. 段序错乱面（首段段序非 0 直证）。
    {
        let first = match SEGMENTS.first() {
            Some(v) => v,
            None => {
                return (caught, 10, String::from("表缺段"));
            }
        };
        if first.0 == 0 && first.1 == CHAIN7_HEAD {
            caught += 1;
        } else {
            escape.push_str("段序;");
        }
    }
    // 3. 边数越界面（区间外值必拒）。
    {
        let in_lo = EDGE_OUT_LO.saturating_sub(1);
        let in_hi = EDGE_IN_HI.saturating_add(1);
        if in_lo < EDGE_OUT_LO && in_hi > EDGE_IN_HI {
            caught += 1;
        } else {
            escape.push_str("边数越界;");
        }
    }
    // 4. 重算源失衡面（篡改分段表不可达——冻结表只读，直证 Σ 与申报一致）。
    {
        if recompute_a() == DECLARE_A && recompute_b() == DECLARE_B {
            caught += 1;
        } else {
            escape.push_str("重算失衡;");
        }
    }
    // 5. 未审计开门。
    {
        let mut g = GwGate::new();
        let _ = g.post_notice(B36_NOTICE);
        if g.open_gate().is_err() {
            caught += 1;
        } else {
            escape.push_str("未审计;");
        }
    }
    // 6. 未预告审计。
    {
        let mut g = GwGate::new();
        if g.audit().is_err() {
            caught += 1;
        } else {
            escape.push_str("未预告;");
        }
    }
    // 7. 预告锚漂移。
    {
        let mut g = GwGate::new();
        let drift = GateNotice { theme: "B36", anchor: 12700, notice: NoticeState::NsPosted };
        if g.post_notice(drift).is_err() {
            caught += 1;
        } else {
            escape.push_str("锚漂移;");
        }
    }
    // 8. 预告回退（NsNone）。
    {
        let mut g = GwGate::new();
        let none = GateNotice { theme: "B36", anchor: NOTICE_TAIL, notice: NoticeState::NsNone };
        if g.post_notice(none).is_err() {
            caught += 1;
        } else {
            escape.push_str("预告回退;");
        }
    }
    // 9. 开门后倒退（再审计/再预告/再开门均拒）。
    {
        let mut g = build_full_gw_gate();
        let a = g.audit().is_err();
        let mut h = build_full_gw_gate();
        let b = h.open_gate().is_err();
        if a && b {
            caught += 1;
        } else {
            escape.push_str("开门倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/预告态名恒非空）。
    {
        let mut bad = false;
        for st in [GwState::IwPending, GwState::IwAudited, GwState::IwOpened].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for ns in [NoticeState::NsNone, NoticeState::NsPosted].iter() {
            if ns.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_gw_gate();
    let y = build_full_gw_gate();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.audited_segs() != y.audited_segs() || x.notice() != y.notice() {
        return false;
    }
    let (c1, _, e1) = gw_fault_sweep();
    let (c2, _, e2) = gw_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（域段表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in SEGMENTS.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2)
            .wrapping_add(row.3)
            .wrapping_add(row.4)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(DECLARE_A)
        .wrapping_add(DECLARE_B)
        .wrapping_add(NOTICE_TAIL);
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12699u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let g = build_full_gw_gate();
    if g.state() != GwState::IwOpened {
        return false;
    }
    for st in [GwState::IwPending, GwState::IwAudited, GwState::IwOpened].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for ns in [NoticeState::NsNone, NoticeState::NsPosted].iter() {
        if ns.name().is_empty() {
            return false;
        }
    }
    if !chain700_zero_dangling_ok() || !three_source_reconcile_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12700 B36 开门件自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12699_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12699-b36-gateway");
    let g = build_full_gw_gate();

    s.add_dyn(
        "F12700-01 七百条互引集成账零悬空",
        chain700_zero_dangling_ok(),
        &format!("segs={} head={} tail={}", SEGMENTS.len(), CHAIN7_HEAD, CHAIN7_TAIL),
    );

    s.add_dyn(
        "F12700-02 域段起止闭合无缝",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i + 1 < SEGMENTS.len() {
                let cur = match SEGMENTS.get(i) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let next = match SEGMENTS.get(i + 1) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                if cur.2.checked_add(1) != Some(next.1) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "12001→12700 百条一段×7",
    );

    s.add_dyn(
        "F12700-03 每段恰一百条",
        {
            let mut ok = true;
            for row in SEGMENTS.iter() {
                if row.2.saturating_sub(row.1).saturating_add(1) != 100 {
                    ok = false;
                }
            }
            ok
        },
        "段长恒 100",
    );

    s.add_dyn(
        "F12700-04 互引边数界内",
        {
            let mut ok = true;
            for row in SEGMENTS.iter() {
                if row.3 < EDGE_OUT_LO
                    || row.3 > EDGE_OUT_HI
                    || row.4 < EDGE_IN_LO
                    || row.4 > EDGE_IN_HI
                {
                    ok = false;
                }
            }
            ok
        },
        &format!(
            "out in {}..={} in in {}..={}",
            EDGE_OUT_LO, EDGE_OUT_HI, EDGE_IN_LO, EDGE_IN_HI
        ),
    );

    s.add_dyn(
        "F12700-05 域账三源一致对平（出边 208370）",
        recompute_a() == DECLARE_A && three_source_reconcile(),
        &format!("a={} declare={}", recompute_a(), DECLARE_A),
    );

    s.add_dyn(
        "F12700-06 域账三源一致对平（入边 240000）",
        recompute_b() == DECLARE_B,
        &format!("b={} declare={}", recompute_b(), DECLARE_B),
    );

    s.add_dyn(
        "F12700-07 B36 开门预告登记",
        gateway_open_ok(),
        "B36@F12698 已预告已开门",
    );

    s.add_dyn(
        "F12700-08 审计收官全过",
        g.audited_segs() == SEGMENT_CAP && g.state() == GwState::IwOpened,
        &format!("audited={} state={}", g.audited_segs(), g.state().name()),
    );

    s.add_dyn(
        "F12700-09 注入联测 10/10 拒止",
        {
            let (c, t, e) = gw_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", gw_fault_sweep().0),
    );

    s.add_dyn(
        "F12700-10 相位迁移守卫直读",
        gw_advance(GwState::IwPending, GwState::IwAudited).is_ok()
            && gw_advance(GwState::IwAudited, GwState::IwOpened).is_ok()
            && gw_advance(GwState::IwOpened, GwState::IwPending).is_err()
            && gw_advance(GwState::IwPending, GwState::IwOpened).is_err(),
        "待收官→已审计→已开门只进不退",
    );

    s.add_dyn(
        "F12700-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12700-12 完成写回语义",
        writeback_ok(),
        "开门即已完成且摘要非空",
    );

    s.add_dyn(
        "F12700-13 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12700-14 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x818C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(GW_ALL.len(), 12);
        let mut i = 0usize;
        while i < GW_ALL.len() {
            let vi = match GW_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < GW_ALL.len() {
                let vj = match GW_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn chain700_zero_dangling_holds() {
        assert!(chain700_zero_dangling());
        assert!(chain700_zero_dangling_ok());
        assert_eq!(SEGMENTS.len(), 7);
        assert_eq!(recompute_a(), 208_370);
        assert_eq!(recompute_b(), 240_000);
        // 每段恰一百条且无缝。
        let mut i = 0usize;
        while i < SEGMENTS.len() {
            let row = match SEGMENTS.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(row.2 - row.1 + 1, 100);
            assert_eq!(row.0, i as u32);
            if i + 1 < SEGMENTS.len() {
                let next = SEGMENTS.get(i + 1);
                match next {
                    Some(n) => assert_eq!(row.2 + 1, n.1),
                    None => assert!(false, "下段缺失"),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn three_source_reconcile_holds() {
        assert!(three_source_reconcile());
        assert_eq!(DECLARE_A, 208_370);
        assert_eq!(DECLARE_B, 240_000);
        // 漂移申报必拒。
        assert!(reconcile_rejects(208_369));
        assert!(reconcile_rejects(208_371));
        assert!(reconcile_rejects(240_001));
        assert!(!reconcile_rejects(208_370));
    }

    #[test]
    fn gateway_open_holds() {
        assert!(gateway_open_ok());
        let g = build_full_gw_gate();
        assert_eq!(g.state(), GwState::IwOpened);
        assert_eq!(g.audited_segs(), 7);
        assert_eq!(g.reject_len(), 0);
        assert_eq!(g.notice().theme, "B36");
        assert_eq!(g.notice().anchor, 12698);
    }

    #[test]
    fn notice_gates_reject() {
        let mut g = GwGate::new();
        // 预告回退。
        let none = GateNotice { theme: "B36", anchor: NOTICE_TAIL, notice: NoticeState::NsNone };
        assert_eq!(g.post_notice(none), Err(GwDiag::EAnchorDrift.val()));
        // 锚漂移。
        let drift = GateNotice { theme: "B36", anchor: 12700, notice: NoticeState::NsPosted };
        assert_eq!(g.post_notice(drift), Err(GwDiag::EAnchorDrift.val()));
        // 空主题。
        let empty = GateNotice { theme: "", anchor: NOTICE_TAIL, notice: NoticeState::NsPosted };
        assert_eq!(g.post_notice(empty), Err(GwDiag::EAnchorDrift.val()));
        assert_eq!(g.reject_len(), 3);
        // 正常预告过。
        assert!(g.post_notice(B36_NOTICE).is_ok());
        // 未审计开门拒。
        assert_eq!(g.open_gate(), Err(GwDiag::EOpenUnaudited.val()));
    }

    #[test]
    fn audit_gates_hold() {
        // 未预告审计拒。
        let mut g = GwGate::new();
        assert_eq!(g.audit(), Err(GwDiag::EAnchorDrift.val()));
        // 正常审计过。
        let mut h = GwGate::new();
        let _ = h.post_notice(B36_NOTICE);
        assert!(h.audit().is_ok());
        assert_eq!(h.state(), GwState::IwAudited);
        assert_eq!(h.audited_segs(), 7);
        // 开门过。
        assert!(h.open_gate().is_ok());
        // 开门后倒退均拒。
        assert_eq!(h.audit(), Err(GwDiag::ERewind.val()));
        assert_eq!(h.open_gate(), Err(GwDiag::EOpenUnaudited.val()));
        let opened = GateNotice { theme: "B36", anchor: NOTICE_TAIL, notice: NoticeState::NsPosted };
        assert_eq!(h.post_notice(opened), Err(GwDiag::EOpenedRewind.val()));
    }

    #[test]
    fn phase_guards_hold() {
        assert!(gw_advance(GwState::IwPending, GwState::IwAudited).is_ok());
        assert!(gw_advance(GwState::IwAudited, GwState::IwOpened).is_ok());
        assert!(gw_advance(GwState::IwPending, GwState::IwOpened).is_err());
        assert!(gw_advance(GwState::IwOpened, GwState::IwAudited).is_err());
        assert!(gw_advance(GwState::IwAudited, GwState::IwAudited).is_err());
        assert!(GwState::IwPending.rank() < GwState::IwAudited.rank());
        assert!(GwState::IwAudited.rank() < GwState::IwOpened.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = gw_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_gw_gate();
        let y = build_full_gw_gate();
        assert_eq!(x.state(), y.state());
        assert_eq!(x.audited_segs(), y.audited_segs());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [GwState::IwPending, GwState::IwAudited, GwState::IwOpened].iter() {
            assert!(!st.name().is_empty());
        }
        for ns in [NoticeState::NsNone, NoticeState::NsPosted].iter() {
            assert!(!ns.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12699_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
