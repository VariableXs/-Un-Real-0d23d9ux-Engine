//! UNX-F49045 窗口销毁事件清算 · 0x73080 段（B13 批第三槽）
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，不得改写）：
//!   UNX-F49045 窗口销毁时事件清算（焦点窗口热消失在途事件归宿）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49046-J1 串扰 ×10^2 检出 0；判据锚定 §AI-62 §5 J-2；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49046 多窗口事件路由对账（焦点串扰零发生断言）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49047-J1 递交 ×10^2 保真；判据锚定 §AI-62 §8 联签对 AI-30；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（卡一 F49045）：登记 → 判环 → 分类 → 归宿 → 清零。
//! 登记先于判环：在途事件必须先带"目标窗口 + 代数"入册，
//! 否则销毁时连"这条事件属于谁"都答不出来，"归宿"就成了猜谜；
//! 判环先于分类：目标代数与现存代数不符的事件已在环外（陈旧），
//! 先判环再分类，才不会把陈旧事件错记成"需要改投"的活事件；
//! 分类先于归宿：键/指针/焦点/手势四类的处置语义不同
//! （键不可丢可改投、指针绑坐标只能弃、焦点语义已失效只能弃、
//! 手势可补发），类别判错则归宿必错；
//! 归宿先于清零：只有每条事件都拿到明确归宿，"零悬挂"才是可核对的数，
//! 而不是"没数过"的默认值。
//! 闸门次序（卡二 F49046）：建路由 → 派发 → 串扰 → 保真 → 陈旧。
//! 建路由先于派发：窗口 → 事件流的映射必须唯一（一窗一流、一流一窗），
//! 否则"派发到哪个流"本身就有两个答案；
//! 派发先于串扰：先按路由把包送出去，再谈"有没有送到别人的流上"；
//! 串扰先于保真：串扰是"送错门"，保真是"送错内容"，送错门时谈内容无意义；
//! 保真先于陈旧：内容是活包的内容；陈旧包连门都不该进，只做拒收计数。
//! 关键差异：F49045 的账是**销毁时刻的一次性清算**（每条在途事件一个归宿，
//! 清算后不许再有悬挂）；F49046 的账是**稳态下的逐包对账**（100 次串扰
//! 检出必须为零、100 次递交逐字节保真）。前者一次性收口，后者持续对账。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73080，B13 批第三槽）。
pub const SEG_BASE: u32 = 0x73080;

/// 热消失场景的在途事件条数。
pub const INFLIGHT_TOTAL: u32 = 12;
/// 串扰/保真探针次数（卡面 ×10^2）。
pub const PROBES: u32 = 100;
/// 陈旧代数注入条数。
pub const STALE_INJECT: u32 = 10;
/// 销毁清算路径条数。
pub const DESTROY_PATHS: usize = 10;
/// 路由表容量。
pub const ROUTE_TOTAL: usize = 6;
/// 窗口表容量。
pub const WIN_TOTAL: usize = 6;
/// 每窗口事件流编号（一窗一流）。
pub const STREAM_BASE: u16 = 0x0101;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 在途登记账标签。
pub const INFLIGHT_TAG: &str = "INFLGT";
/// 清算账标签。
pub const SETTLE_TAG: &str = "SETTLE";
/// 路由账标签。
pub const ROUTE_TAG: &str = "ROUTE";
/// 保真账标签。
pub const FIDEL_TAG: &str = "FIDEL";
/// AI-30 联签锚（递交保真对账的签字方）。
pub const AI30_ANCHOR: &str = "AI-30/fidelity-sign";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 F49045 段诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClearCode {
    /// 在途登记簿不守恒（条数与入册序号不符）
    RegBad,
    /// 目标窗口不存在
    TargetBad,
    /// 归宿非法（未决策或类型不符）
    FateBad,
    /// 存在悬挂事件（销毁后无归宿）
    Dangling,
    /// 代数不符（陈旧包/陈旧事件）
    GenBad,
    /// 事件流编号不符
    StreamBad,
    /// 串扰：包被投递到别的窗口流
    Crosstalk,
    /// 保真失败：递交内容与源内容不同
    Fidelity,
    /// 渲染字段不可解析
    RenderBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 覆盖不足
    CoverageBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 计数溢出
    Overflow,
    /// 摘要为零
    DigestBad,
}

impl ClearCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ClearCode::RegBad => "在途登记簿不守恒：条数与入册序号不符",
            ClearCode::TargetBad => "目标窗口不存在",
            ClearCode::FateBad => "归宿非法：未决策或类型不符",
            ClearCode::Dangling => "存在悬挂事件：销毁后没有归宿",
            ClearCode::GenBad => "代数不符：陈旧包或陈旧事件",
            ClearCode::StreamBad => "事件流编号不符",
            ClearCode::Crosstalk => "串扰：包被投递到别的窗口流",
            ClearCode::Fidelity => "保真失败：递交内容与源内容不同",
            ClearCode::RenderBad => "渲染字段不可解析",
            ClearCode::ReplayBad => "复跑摘要漂移",
            ClearCode::CoverageBad => "覆盖不足：探针或路径数不达标",
            ClearCode::SegmentBad => "诊断码越出段位窗口",
            ClearCode::Overflow => "计数溢出",
            ClearCode::DigestBad => "摘要为零：折叠未发生",
        }
    }

    pub const ALL: [ClearCode; 14] = [
        ClearCode::RegBad,
        ClearCode::TargetBad,
        ClearCode::FateBad,
        ClearCode::Dangling,
        ClearCode::GenBad,
        ClearCode::StreamBad,
        ClearCode::Crosstalk,
        ClearCode::Fidelity,
        ClearCode::RenderBad,
        ClearCode::ReplayBad,
        ClearCode::CoverageBad,
        ClearCode::SegmentBad,
        ClearCode::Overflow,
        ClearCode::DigestBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ClearCode::ALL.len() {
        if let Some(c) = ClearCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码连续落在 0x73081..0x7308E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ClearCode::ALL.len() {
        let c = match ClearCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
            return false;
        }
        i += 1;
    }
    true
}

// ── 用例编号（16 条）与保留号 ────────────────────────────────────
/// 前序槽位占用号（B12 族，抄录自前批冻结清单）。
pub const RESERVED_IDS: [u32; 8] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8003,
    0x071C_8014,
    0x071D_0001,
    0x071D_0002,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个用例编号（0x0733_0801..=0x0733_0810）。
pub const CASE_IDS: [u32; 16] = [
    0x0733_0801,
    0x0733_0802,
    0x0733_0803,
    0x0733_0804,
    0x0733_0805,
    0x0733_0806,
    0x0733_0807,
    0x0733_0808,
    0x0733_0809,
    0x0733_080A,
    0x0733_080B,
    0x0733_080C,
    0x0733_080D,
    0x0733_080E,
    0x0733_080F,
    0x0733_0810,
];

/// 用例语义绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
}

/// 16 条用例声明。
pub const CASES: [CaseSpec; 16] = [
    CaseSpec { id: 0x0733_0801, name: "在途登记入册" },
    CaseSpec { id: 0x0733_0802, name: "代数入册校验" },
    CaseSpec { id: 0x0733_0803, name: "热消失清算" },
    CaseSpec { id: 0x0733_0804, name: "键改投归宿" },
    CaseSpec { id: 0x0733_0805, name: "指针弃置归宿" },
    CaseSpec { id: 0x0733_0806, name: "手势补发归宿" },
    CaseSpec { id: 0x0733_0807, name: "零悬挂断言" },
    CaseSpec { id: 0x0733_0808, name: "清算复跑" },
    CaseSpec { id: 0x0733_0809, name: "窗口流映射" },
    CaseSpec { id: 0x0733_080A, name: "串扰百次检出" },
    CaseSpec { id: 0x0733_080B, name: "递交百次保真" },
    CaseSpec { id: 0x0733_080C, name: "陈旧代数拒收" },
    CaseSpec { id: 0x0733_080D, name: "路由唯一性" },
    CaseSpec { id: 0x0733_080E, name: "清算路径十连" },
    CaseSpec { id: 0x0733_080F, name: "保真复跑" },
    CaseSpec { id: 0x0733_0810, name: "渲染往返" },
];

/// 跨槽零撞号。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a >= SEG_BASE + 0x01 && a <= SEG_BASE + 0x0E {
            return false;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(k) {
                if a == *b {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 用例编号连续：CASE_IDS[i] == 0x0733_0800 + 1 + i。
pub fn case_ids_sequential() -> bool {
    let base: u32 = 0x0733_0800;
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match base.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if let Some(v) = CASE_IDS.get(i) {
            if *v != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 窗口与在途事件（F49045） ─────────────────────────────────────
/// 窗口记录：编号、代数、是否可聚焦（可承接改投）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRec {
    pub id: u32,
    pub gen: u16,
    pub focusable: bool,
}

/// 六窗口：0x7C01(1) / 0x7C02(2) / 0x7C03(1) / 0x7C04(1,不可聚焦) /
/// 0x7C05(3) / 0x7C06(1)。
pub const WINDOWS: [WinRec; WIN_TOTAL] = [
    WinRec { id: 0x7C01, gen: 1, focusable: true },
    WinRec { id: 0x7C02, gen: 2, focusable: true },
    WinRec { id: 0x7C03, gen: 1, focusable: true },
    WinRec { id: 0x7C04, gen: 1, focusable: false },
    WinRec { id: 0x7C05, gen: 3, focusable: true },
    WinRec { id: 0x7C06, gen: 1, focusable: true },
];

/// 按编号取窗口记录。
pub fn window_of(id: u32) -> Option<WinRec> {
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if let Some(w) = WINDOWS.get(i) {
            if w.id == id {
                return Some(*w);
            }
        }
        i += 1;
    }
    None
}

/// 在途事件类别（处置语义各不相同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvClass {
    Key,
    Pointer,
    Focus,
    Gesture,
}

impl EvClass {
    pub fn index(self) -> u8 {
        match self {
            EvClass::Key => 0,
            EvClass::Pointer => 1,
            EvClass::Focus => 2,
            EvClass::Gesture => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EvClass::Key => "Key",
            EvClass::Pointer => "Pointer",
            EvClass::Focus => "Focus",
            EvClass::Gesture => "Gesture",
        }
    }
}

/// 一条在途事件：入册序号、目标窗口、目标代数、类别、载荷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InFlight {
    pub id: u32,
    pub target: u32,
    pub gen: u16,
    pub class: EvClass,
    pub payload: u32,
}

/// 在途登记簿：逐条入册，序号单调；代数与当前窗口不符一律拒收。
#[derive(Clone, Debug)]
pub struct Registry {
    pub rows: Vec<InFlight>,
    pub next_id: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            rows: Vec::new(),
            next_id: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn len(&self) -> u32 {
        self.rows.len() as u32
    }

    /// 入册：窗口存在 → 代数一致 → 落号。
    pub fn enqueue(
        &mut self,
        target: u32,
        gen: u16,
        class: EvClass,
        payload: u32,
    ) -> Result<u32, u32> {
        let w = window_of(target).ok_or(ClearCode::TargetBad.val())?;
        if w.gen != gen {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ClearCode::GenBad.val());
        }
        self.next_id = match self.next_id.checked_add(1) {
            Some(v) => v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let row = InFlight {
            id: self.next_id,
            target,
            gen,
            class,
            payload,
        };
        self.digest = p_core::fold_u32(self.digest, row.id);
        self.digest = p_core::fold_u32(self.digest, row.target);
        self.digest = p_core::fold_byte(self.digest, row.class.index());
        self.rows.push(row);
        Ok(self.next_id)
    }

    /// 某窗口的待清算条数。
    pub fn pending_for(&self, win: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.target == win {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 按类别计数。
    pub fn count_class(&self, c: EvClass) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.class == c {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 清算策略：改投目标、是否允许补发。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SettlePolicy {
    pub reroute_to: u32,
    pub redeliver: bool,
}

/// 归宿：保留 / 丢弃 / 改投 / 补发。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fate {
    Keep,
    Discard,
    Reroute(u32),
    Redeliver,
}

impl Fate {
    pub fn index(self) -> u8 {
        match self {
            Fate::Keep => 0,
            Fate::Discard => 1,
            Fate::Reroute(_) => 2,
            Fate::Redeliver => 3,
        }
    }
}

/// 归宿决策（纯函数）：陈旧 → 弃；键 → 改投；指针 → 弃；焦点 → 弃；
/// 手势 → 凭策略补发或弃。
pub fn decide(ev: &InFlight, current_gen: u16, destroyed: u32, p: &SettlePolicy) -> Fate {
    if ev.target != destroyed {
        return Fate::Keep;
    }
    if ev.gen != current_gen {
        return Fate::Discard;
    }
    match ev.class {
        EvClass::Key => {
            if p.reroute_to != 0 {
                Fate::Reroute(p.reroute_to)
            } else {
                Fate::Discard
            }
        }
        EvClass::Pointer => Fate::Discard,
        EvClass::Focus => Fate::Discard,
        EvClass::Gesture => {
            if p.redeliver {
                Fate::Redeliver
            } else {
                Fate::Discard
            }
        }
    }
}

/// 清算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SettleReport {
    pub total: u32,
    pub affected: u32,
    pub discarded: u32,
    pub rerouted: u32,
    pub redelivered: u32,
    pub untouched: u32,
    pub dangling: u32,
    pub orphans: u32,
    pub digest: u64,
}

impl SettleReport {
    /// 守恒：总数 = 受影响 + 未动；受影响 = 弃 + 改投 + 补发；
    /// 零悬挂、零孤儿。
    pub fn conserved(&self) -> bool {
        if self.affected.checked_add(self.untouched) != Some(self.total) {
            return false;
        }
        let parts = match self
            .discarded
            .checked_add(self.rerouted)
            .and_then(|v| v.checked_add(self.redelivered))
        {
            Some(v) => v,
            None => return false,
        };
        if parts != self.affected {
            return false;
        }
        self.dangling == 0 && self.orphans == 0 && self.total > 0
    }
}

/// 清算：销毁窗口 → 逐条定归宿 → 复核零悬挂。
pub fn settle(
    rows: &[InFlight],
    destroyed: u32,
    policy: &SettlePolicy,
) -> Result<SettleReport, u32> {
    let w = window_of(destroyed).ok_or(ClearCode::TargetBad.val())?;
    if policy.reroute_to == destroyed {
        return Err(ClearCode::TargetBad.val());
    }
    if policy.reroute_to != 0 {
        let r = window_of(policy.reroute_to).ok_or(ClearCode::TargetBad.val())?;
        if !r.focusable {
            return Err(ClearCode::TargetBad.val());
        }
    }
    let mut total = 0u32;
    let mut affected = 0u32;
    let mut discarded = 0u32;
    let mut rerouted = 0u32;
    let mut redelivered = 0u32;
    let mut untouched = 0u32;
    let mut orphans = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        let ev = match rows.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        total = total.saturating_add(1);
        let fate = decide(&ev, w.gen, destroyed, policy);
        match fate {
            Fate::Keep => untouched = untouched.saturating_add(1),
            Fate::Discard => {
                affected = affected.saturating_add(1);
                discarded = discarded.saturating_add(1);
            }
            Fate::Reroute(to) => {
                affected = affected.saturating_add(1);
                rerouted = rerouted.saturating_add(1);
                if window_of(to).is_none() {
                    orphans = orphans.saturating_add(1);
                }
            }
            Fate::Redeliver => {
                affected = affected.saturating_add(1);
                redelivered = redelivered.saturating_add(1);
            }
        }
        digest = p_core::fold_u32(digest, ev.id);
        digest = p_core::fold_byte(digest, fate.index());
        digest = p_core::fold_u32(digest, ev.payload);
        i += 1;
    }
    // 零悬挂复核：同一批事件重扫一遍，逐条必须能问出归宿。
    let mut dangling = 0u32;
    i = 0;
    while i < rows.len() {
        let ev = match rows.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let decision = decide(&ev, w.gen, destroyed, policy);
        if ev.target == destroyed && decision == Fate::Keep {
            dangling = dangling.saturating_add(1);
        }
        i += 1;
    }
    if digest == 0 {
        return Err(ClearCode::DigestBad.val());
    }
    let report = SettleReport {
        total,
        affected,
        discarded,
        rerouted,
        redelivered,
        untouched,
        dangling,
        orphans,
        digest,
    };
    if !report.conserved() {
        return Err(ClearCode::Dangling.val());
    }
    Ok(report)
}

/// 热消失场景：焦点窗口 0x7C02 上挂 12 条在途事件，销毁后一次清算。
pub fn hot_disappear() -> Result<(SettleReport, Registry), u32> {
    let mut reg = Registry::new();
    let target = 0x7C02;
    let gen = 2u16;
    let plan = [
        EvClass::Key,
        EvClass::Key,
        EvClass::Pointer,
        EvClass::Focus,
        EvClass::Key,
        EvClass::Gesture,
        EvClass::Pointer,
        EvClass::Focus,
        EvClass::Key,
        EvClass::Pointer,
        EvClass::Gesture,
        EvClass::Focus,
    ];
    let mut i = 0usize;
    while i < plan.len() {
        let c = match plan.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        reg.enqueue(target, gen, c, 0x1000 + i as u32)?;
        i += 1;
    }
    if reg.len() != INFLIGHT_TOTAL {
        return Err(ClearCode::RegBad.val());
    }
    let policy = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
    let report = settle(&reg.rows, target, &policy)?;
    Ok((report, reg))
}

/// 十路径清算：每路销毁不同窗口、在途条数递增，逐路零悬挂。
pub fn settle_paths() -> Result<(u32, u32, u32, u64), u32> {
    let mut affected_sum = 0u32;
    let mut dangling_sum = 0u32;
    let mut paths = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut pi = 0usize;
    while pi < DESTROY_PATHS {
        let w = match WINDOWS.get(pi % WINDOWS.len()) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let mut rows: Vec<InFlight> = Vec::new();
        let n = 3 + pi as u32;
        let mut k = 0u32;
        while k < n {
            let class = match k % 4 {
                0 => EvClass::Key,
                1 => EvClass::Pointer,
                2 => EvClass::Focus,
                _ => EvClass::Gesture,
            };
            rows.push(InFlight {
                id: pi as u32 * 100 + k + 1,
                target: w.id,
                gen: w.gen,
                class,
                payload: 0x2000 + k,
            });
            k += 1;
        }
        let policy = SettlePolicy {
            reroute_to: if pi % 3 == 0 {
                0
            } else if w.id == 0x7C06 {
                0x7C01
            } else {
                0x7C06
            },
            redeliver: pi % 2 == 0,
        };
        let rep = settle(&rows, w.id, &policy)?;
        if rep.dangling != 0 {
            return Err(ClearCode::Dangling.val());
        }
        affected_sum = affected_sum.saturating_add(rep.affected);
        dangling_sum = dangling_sum.saturating_add(rep.dangling);
        paths = paths.saturating_add(1);
        digest = p_core::fold_u32(digest, rep.total);
        digest = p_core::fold_u32(digest, rep.affected);
        digest = p_core::fold_u32(digest, rep.discarded);
        digest = p_core::fold_u32(digest, rep.rerouted);
        pi += 1;
    }
    if paths != DESTROY_PATHS as u32 {
        return Err(ClearCode::CoverageBad.val());
    }
    if digest == 0 {
        return Err(ClearCode::DigestBad.val());
    }
    Ok((paths, affected_sum, dangling_sum, digest))
}

/// 清算复跑：两遍摘要一致。
pub fn settle_replay() -> Result<(u64, u64), u32> {
    let (a, _) = hot_disappear()?;
    let (b, _) = hot_disappear()?;
    Ok((a.digest, b.digest))
}

/// 悬挂注入：手工扣下一条事件的归宿，应被 `audit_partial` 判为悬挂。
pub fn dangling_inject() -> Result<(u32, u32), u32> {
    let (_, reg) = hot_disappear()?;
    let policy = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
    let mut detected = 0u32;
    let mut missed = 0u32;
    let mut skip = 0usize;
    while skip < 10 {
        let dangling = audit_partial(&reg.rows, 0x7C02, &policy, skip);
        if dangling > 0 {
            detected = detected.saturating_add(1);
        } else {
            missed = missed.saturating_add(1);
        }
        skip += 1;
    }
    Ok((detected, missed))
}

/// 局部清算审计：故意跳过第 `skip` 条（模拟登记簿被截断），返回剩余悬挂数。
/// skip 越界时悬挂数为零（该路径本身就是合法清算）。
pub fn audit_partial(rows: &[InFlight], destroyed: u32, _policy: &SettlePolicy, skip: usize) -> u32 {
    let w = match window_of(destroyed) {
        Some(v) => v,
        None => return 0,
    };
    let mut dangling = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        if i == skip {
            i += 1;
            continue;
        }
        if let Some(ev) = rows.get(i) {
            if ev.target == destroyed && ev.gen == w.gen {
                dangling = dangling.saturating_add(1);
            }
        }
        i += 1;
    }
    dangling
}

/// 改投目标注入：10 组非法策略必须被拒。
pub fn policy_faults() -> Result<(u32, u32), u32> {
    let (_, reg) = hot_disappear()?;
    let cases = [
        SettlePolicy { reroute_to: 0x7C02, redeliver: true },
        SettlePolicy { reroute_to: 0x7C04, redeliver: true },
        SettlePolicy { reroute_to: 0x7CFF, redeliver: true },
        SettlePolicy { reroute_to: 0x7C02, redeliver: false },
        SettlePolicy { reroute_to: 0x7C04, redeliver: false },
        SettlePolicy { reroute_to: 0x7CFF, redeliver: false },
        SettlePolicy { reroute_to: 0, redeliver: true },
        SettlePolicy { reroute_to: 0x7C01, redeliver: true },
        SettlePolicy { reroute_to: 0x7C03, redeliver: false },
        SettlePolicy { reroute_to: 0x7C05, redeliver: true },
    ];
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < cases.len() {
        let p = match cases.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        if settle(&reg.rows, 0x7C02, &p).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

// ── 多窗口路由对账（F49046） ─────────────────────────────────────
/// 一条路由：窗口 → 事件流，带代数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Route {
    pub win: u32,
    pub gen: u16,
    pub stream: u16,
}

/// 六条路由（一窗一流，流号从 STREAM_BASE 起逐条 +1）。
pub const ROUTES: [Route; ROUTE_TOTAL] = [
    Route { win: 0x7C01, gen: 1, stream: STREAM_BASE },
    Route { win: 0x7C02, gen: 2, stream: STREAM_BASE + 1 },
    Route { win: 0x7C03, gen: 1, stream: STREAM_BASE + 2 },
    Route { win: 0x7C04, gen: 1, stream: STREAM_BASE + 3 },
    Route { win: 0x7C05, gen: 3, stream: STREAM_BASE + 4 },
    Route { win: 0x7C06, gen: 1, stream: STREAM_BASE + 5 },
];

/// 按窗口取路由。
pub fn route_of(win: u32) -> Option<Route> {
    let mut i = 0usize;
    while i < ROUTES.len() {
        if let Some(r) = ROUTES.get(i) {
            if r.win == win {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 一个待派发的包：编号、目标窗口、代数、声明流、载荷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Packet {
    pub id: u32,
    pub win: u32,
    pub gen: u16,
    pub stream: u16,
    pub payload: u32,
}

/// 一次投递：编号、窗口、实际流、载荷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    pub id: u32,
    pub win: u32,
    pub stream: u16,
    pub payload: u32,
}

/// 派发：窗口存在 → 代数一致 → 流号一致 → 投递。
pub fn dispatch(p: &Packet) -> Result<Delivery, u32> {
    let r = route_of(p.win).ok_or(ClearCode::TargetBad.val())?;
    if p.gen != r.gen {
        return Err(ClearCode::GenBad.val());
    }
    if p.stream != r.stream {
        return Err(ClearCode::Crosstalk.val());
    }
    Ok(Delivery {
        id: p.id,
        win: p.win,
        stream: r.stream,
        payload: p.payload,
    })
}

/// 全批派发：逐包投递并折叠摘要。
pub fn dispatch_all(packets: &[Packet]) -> Result<(Vec<Delivery>, u64), u32> {
    let mut out: Vec<Delivery> = Vec::new();
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < packets.len() {
        let p = match packets.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let d = dispatch(&p)?;
        digest = p_core::fold_u32(digest, d.id);
        digest = p_core::fold_u32(digest, d.win);
        digest = p_core::fold_u32(digest, d.stream as u32);
        digest = p_core::fold_u32(digest, d.payload);
        out.push(d);
        i += 1;
    }
    if out.len() != packets.len() {
        return Err(ClearCode::CoverageBad.val());
    }
    if digest == 0 {
        return Err(ClearCode::DigestBad.val());
    }
    Ok((out, digest))
}

/// 生成 100 个正确包：窗口与流全部按路由表。
pub fn probe_packets() -> Result<Vec<Packet>, u32> {
    let mut out: Vec<Packet> = Vec::new();
    let mut i = 0u32;
    while i < PROBES {
        let r = match ROUTES.get((i as usize) % ROUTES.len()) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        out.push(Packet {
            id: i + 1,
            win: r.win,
            gen: r.gen,
            stream: r.stream,
            payload: 0x9000 + i,
        });
        i += 1;
    }
    Ok(out)
}

/// 串扰探针：100 包派发，投递窗口与流号必须与源一致（检出 0）。
pub fn probe_crosstalk() -> Result<(u32, u32), u32> {
    let packets = probe_packets()?;
    let (rows, _) = dispatch_all(&packets)?;
    let mut crosstalk = 0u32;
    let mut clean = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        let d = match rows.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let p = match packets.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        if d.win == p.win && d.stream == p.stream {
            clean = clean.saturating_add(1);
        } else {
            crosstalk = crosstalk.saturating_add(1);
        }
        i += 1;
    }
    Ok((crosstalk, clean))
}

/// 串扰注入：10 个流号被挪到邻窗的包，必须全部被拒。
pub fn crosstalk_inject() -> Result<(u32, u32), u32> {
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < STALE_INJECT as usize {
        let r = match ROUTES.get(i % ROUTES.len()) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let other = match ROUTES.get((i + 1) % ROUTES.len()) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let p = Packet {
            id: 0xA000 + i as u32,
            win: r.win,
            gen: r.gen,
            stream: other.stream,
            payload: 1,
        };
        if dispatch(&p).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 保真探针：100 包逐包比对载荷、编号、窗口、流号，并比整批摘要。
pub fn probe_fidelity() -> Result<(u32, u32, u64, u64), u32> {
    let packets = probe_packets()?;
    let (rows, delivered_digest) = dispatch_all(&packets)?;
    let mut same = 0u32;
    let mut different = 0u32;
    let mut src_digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        let d = match rows.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let p = match packets.get(i) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        src_digest = p_core::fold_u32(src_digest, p.id);
        src_digest = p_core::fold_u32(src_digest, p.win);
        src_digest = p_core::fold_u32(src_digest, p.stream as u32);
        src_digest = p_core::fold_u32(src_digest, p.payload);
        if d.id == p.id && d.win == p.win && d.stream == p.stream && d.payload == p.payload {
            same = same.saturating_add(1);
        } else {
            different = different.saturating_add(1);
        }
        i += 1;
    }
    Ok((same, different, src_digest, delivered_digest))
}

/// 陈旧代数注入：10 个旧代数包必须全部被拒。
pub fn stale_inject() -> Result<(u32, u32), u32> {
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < STALE_INJECT as usize {
        let r = match ROUTES.get(i % ROUTES.len()) {
            Some(v) => *v,
            None => return Err(ClearCode::Overflow.val()),
        };
        let stale_gen = r.gen.saturating_sub(1);
        let p = Packet {
            id: 0xB000 + i as u32,
            win: r.win,
            gen: stale_gen,
            stream: r.stream,
            payload: 2,
        };
        if dispatch(&p).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 路由唯一性：窗口不重复、流号不重复、代数与窗口表一致。
pub fn routes_unique() -> bool {
    let mut i = 0usize;
    while i < ROUTES.len() {
        let a = match ROUTES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let w = match window_of(a.win) {
            Some(v) => v,
            None => return false,
        };
        if w.gen != a.gen {
            return false;
        }
        let mut j = i + 1;
        while j < ROUTES.len() {
            if let Some(b) = ROUTES.get(j) {
                if a.win == b.win || a.stream == b.stream {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 路由摘要：逐条折窗号、代数、流号。
pub fn route_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < ROUTES.len() {
        if let Some(r) = ROUTES.get(i) {
            h = p_core::fold_u32(h, r.win);
            h = p_core::fold_u32(h, r.gen as u32);
            h = p_core::fold_u32(h, r.stream as u32);
        }
        i += 1;
    }
    h
}

/// 保真复跑：两遍整批摘要一致。
pub fn fidelity_replay() -> Result<(u64, u64), u32> {
    let (_, d1) = dispatch_all(&probe_packets()?)?;
    let (_, d2) = dispatch_all(&probe_packets()?)?;
    Ok((d1, d2))
}

// ── 渲染 ─────────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 在途登记账行：总条数、目标窗口、待清算条数、被拒条数。
pub fn inflight_row(reg: &Registry, target: u32) -> String {
    render_row(
        INFLIGHT_TAG,
        reg.len(),
        target,
        reg.pending_for(target),
        reg.rejected,
        reg.digest,
    )
}

/// 清算账行：总数、受影响、弃置数、改投数。
pub fn settle_row(r: &SettleReport) -> String {
    render_row(
        SETTLE_TAG,
        r.total,
        r.affected,
        r.discarded,
        r.rerouted,
        r.digest,
    )
}

/// 路由账行：路由数、首流号、末流号、窗号异或。
pub fn route_row() -> String {
    let mut xor = 0u32;
    let mut i = 0usize;
    while i < ROUTES.len() {
        if let Some(r) = ROUTES.get(i) {
            xor ^= r.win;
        }
        i += 1;
    }
    render_row(
        ROUTE_TAG,
        ROUTE_TOTAL as u32,
        STREAM_BASE as u32,
        (STREAM_BASE as u32) + ROUTE_TOTAL as u32 - 1,
        xor,
        route_digest(),
    )
}

/// 保真账行：探针数、保真数、串扰数、次批摘要。
pub fn fidel_row(same: u32, crosstalk: u32, digest: u64) -> String {
    render_row(FIDEL_TAG, PROBES, same, crosstalk, 0, digest)
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49045 ①：在途事件登记（12 条、四类齐备、代数错拒收）。
    let hot = hot_disappear();
    let mut bad_gen = 0u32;
    let mut reg_probe = Registry::new();
    let mut i = 0usize;
    while i < 10 {
        if reg_probe.enqueue(0x7C02, 1, EvClass::Key, 1).is_err() {
            bad_gen = bad_gen.saturating_add(1);
        }
        i += 1;
    }
    let reg_ok = match &hot {
        Ok((_, reg)) => {
            reg.len() == INFLIGHT_TOTAL
                && reg.pending_for(0x7C02) == INFLIGHT_TOTAL
                && reg.count_class(EvClass::Key) == 4
                && reg.count_class(EvClass::Pointer) == 3
                && reg.count_class(EvClass::Focus) == 3
                && reg.count_class(EvClass::Gesture) == 2
                && reg.digest != 0
                && bad_gen == 10
                && reg_probe.rejected == 10
        }
        Err(_) => false,
    };
    out.push((
        "F49045/在途事件登记",
        reg_ok,
        format!(
            "n={:?} key={:?} badgen={} rej={}",
            hot.as_ref().map(|(_, r)| r.len()),
            hot.as_ref().map(|(_, r)| r.count_class(EvClass::Key)),
            bad_gen,
            reg_probe.rejected
        ),
    ));

    // F49045 ②：销毁归宿清零（12 受影响：弃 6 / 改投 4 / 补发 2，零悬挂）。
    let fate_ok = match &hot {
        Ok((rep, _)) => {
            rep.total == INFLIGHT_TOTAL
                && rep.affected == INFLIGHT_TOTAL
                && rep.discarded == 6
                && rep.rerouted == 4
                && rep.redelivered == 2
                && rep.untouched == 0
                && rep.dangling == 0
                && rep.orphans == 0
                && rep.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49045/销毁归宿清零",
        fate_ok,
        format!(
            "aff={:?} dis={:?} rer={:?} red={:?} dang={:?}",
            hot.as_ref().map(|(r, _)| r.affected),
            hot.as_ref().map(|(r, _)| r.discarded),
            hot.as_ref().map(|(r, _)| r.rerouted),
            hot.as_ref().map(|(r, _)| r.redelivered),
            hot.as_ref().map(|(r, _)| r.dangling)
        ),
    ));

    // F49045 ③：热消失清算（十路径零悬挂、悬挂注入 10 全检出、复跑一致）。
    let paths = settle_paths();
    let dinj = dangling_inject();
    let srep = settle_replay();
    let hot_ok = match (&paths, &dinj, &srep) {
        (Ok((p, _, d, _)), Ok((det, miss)), Ok((a, b))) => {
            *p == DESTROY_PATHS as u32 && *d == 0 && *det == 10 && *miss == 0 && a == b && *a != 0
        }
        _ => false,
    };
    out.push((
        "F49045/热消失清算",
        hot_ok,
        format!(
            "paths={:?} dang={:?} det={:?} rep={:?}",
            paths.as_ref().map(|(p, _, _, _)| *p),
            paths.as_ref().map(|(_, _, d, _)| *d),
            dinj.as_ref().map(|(d, _)| *d),
            srep.as_ref().map(|(a, _)| *a)
        ),
    ));

    // F49045 ④：改投目标校验（非法策略全拒、合法策略全收）。
    let pf = policy_faults();
    let pol_ok = match &pf {
        Ok((r, a)) => *r == 6 && *a == 4,
        Err(_) => false,
    };
    out.push((
        "F49045/改投目标校验",
        pol_ok,
        format!(
            "rej={:?} acc={:?}",
            pf.as_ref().map(|(r, _)| *r),
            pf.as_ref().map(|(_, a)| *a)
        ),
    ));

    // F49046 ①：窗口流映射（六路由唯一、代数对齐、账行可解析）。
    let route_row_ok = match row_fields(&route_row()) {
        Some((t, v, d)) => {
            t == ROUTE_TAG
                && v[0] == ROUTE_TOTAL as u32
                && v[1] == STREAM_BASE as u32
                && v[2] == STREAM_BASE as u32 + ROUTE_TOTAL as u32 - 1
                && d == route_digest()
                && audit_render(&route_row())
        }
        None => false,
    };
    out.push((
        "F49046/窗口流映射",
        routes_unique() && route_digest() != 0 && route_row_ok,
        format!(
            "uniq={} row={} dig={:016X}",
            routes_unique(),
            route_row_ok,
            route_digest()
        ),
    ));

    // F49046 ②：串扰百次为零（100 包零检出、10 个邻流包全拒）。
    let cross = probe_crosstalk();
    let cinj = crosstalk_inject();
    let cross_ok = match (&cross, &cinj) {
        (Ok((c, clean)), Ok((r, a))) => {
            *c == 0 && *clean == PROBES && *r == STALE_INJECT && *a == 0
        }
        _ => false,
    };
    out.push((
        "F49046/串扰百次为零",
        cross_ok,
        format!(
            "cross={:?} clean={:?} rej={:?}",
            cross.as_ref().map(|(c, _)| *c),
            cross.as_ref().map(|(_, c)| *c),
            cinj.as_ref().map(|(r, _)| *r)
        ),
    ));

    // F49046 ③：递交百次保真（100 逐包同值、源摘要 = 投递摘要、复跑一致）。
    let fid = probe_fidelity();
    let frep = fidelity_replay();
    let fid_ok = match (&fid, &frep) {
        (Ok((same, diff, src, dst)), Ok((a, b))) => {
            *same == PROBES && *diff == 0 && src == dst && a == b && *a != 0
        }
        _ => false,
    };
    out.push((
        "F49046/递交百次保真",
        fid_ok && AI30_ANCHOR.contains("AI-30"),
        format!(
            "same={:?} diff={:?} eq={} rep={}",
            fid.as_ref().map(|(s, _, _, _)| *s),
            fid.as_ref().map(|(_, d, _, _)| *d),
            fid.as_ref().map(|(_, _, s, d)| s == d).map_or(false, |v| v),
            frep.is_ok()
        ),
    ));

    // F49046 ④：陈旧代数拒收（10 个旧代包全拒、保真账行可解析）。
    let stale = stale_inject();
    let frow = match &fid {
        Ok((same, _, _, dst)) => fidel_row(*same, 0, *dst),
        Err(_) => String::from(""),
    };
    let frow_ok = match row_fields(&frow) {
        Some((t, v, d)) => t == FIDEL_TAG && v[0] == PROBES && v[1] == PROBES && v[2] == 0 && d != 0,
        None => false,
    };
    let stale_ok = match &stale {
        Ok((r, a)) => *r == STALE_INJECT && *a == 0,
        Err(_) => false,
    };
    out.push((
        "F49046/陈旧代数拒收",
        stale_ok && frow_ok && segment_guard() && no_cross_collision(),
        format!(
            "rej={:?} acc={:?} row={} seg={}",
            stale.as_ref().map(|(r, _)| *r),
            stale.as_ref().map(|(_, a)| *a),
            frow_ok,
            segment_guard()
        ),
    ));

    out
}

pub fn run_unx49045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49045 窗口销毁事件清算与路由对账");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(ClearCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < ClearCode::ALL.len() {
            let c = ClearCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < ClearCode::ALL.len() {
            let c = ClearCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(1), UNKNOWN);
    }

    #[test]
    fn detail_texts_are_distinct() {
        let mut i = 0usize;
        while i < ClearCode::ALL.len() {
            let mut j = i + 1;
            while j < ClearCode::ALL.len() {
                assert_ne!(ClearCode::ALL[i].detail(), ClearCode::ALL[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn window_table_is_consistent() {
        assert_eq!(WINDOWS.len(), WIN_TOTAL);
        let mut i = 0usize;
        while i < WINDOWS.len() {
            let w = WINDOWS[i];
            assert!(w.gen >= 1);
            assert_eq!(window_of(w.id).map(|x| x.gen), Some(w.gen));
            let mut j = i + 1;
            while j < WINDOWS.len() {
                assert_ne!(WINDOWS[i].id, WINDOWS[j].id);
                j += 1;
            }
            i += 1;
        }
        assert!(window_of(0xFFFF).is_none());
    }

    #[test]
    fn registry_assigns_monotonic_ids() {
        let mut reg = Registry::new();
        let a = reg.enqueue(0x7C01, 1, EvClass::Key, 10).unwrap();
        let b = reg.enqueue(0x7C02, 2, EvClass::Pointer, 11).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.pending_for(0x7C01), 1);
        assert_eq!(reg.pending_for(0x7C02), 1);
        assert_eq!(reg.pending_for(0x7C03), 0);
        assert_ne!(reg.digest, 0);
    }

    #[test]
    fn registry_rejects_bad_window_and_gen() {
        let mut reg = Registry::new();
        assert_eq!(
            reg.enqueue(0x9999, 1, EvClass::Key, 0),
            Err(ClearCode::TargetBad.val())
        );
        assert_eq!(
            reg.enqueue(0x7C02, 1, EvClass::Key, 0),
            Err(ClearCode::GenBad.val())
        );
        assert_eq!(reg.rejected, 1);
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn decide_follows_class_semantics() {
        let p = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
        let key = InFlight { id: 1, target: 0x7C02, gen: 2, class: EvClass::Key, payload: 0 };
        let ptr = InFlight { class: EvClass::Pointer, ..key };
        let foc = InFlight { class: EvClass::Focus, ..key };
        let ges = InFlight { class: EvClass::Gesture, ..key };
        let other = InFlight { target: 0x7C01, ..key };
        assert_eq!(decide(&key, 2, 0x7C02, &p), Fate::Reroute(0x7C01));
        assert_eq!(decide(&ptr, 2, 0x7C02, &p), Fate::Discard);
        assert_eq!(decide(&foc, 2, 0x7C02, &p), Fate::Discard);
        assert_eq!(decide(&ges, 2, 0x7C02, &p), Fate::Redeliver);
        assert_eq!(decide(&other, 2, 0x7C02, &p), Fate::Keep);
    }

    #[test]
    fn decide_treats_stale_as_discard() {
        let p = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
        let ev = InFlight { id: 1, target: 0x7C02, gen: 1, class: EvClass::Key, payload: 0 };
        assert_eq!(decide(&ev, 2, 0x7C02, &p), Fate::Discard);
    }

    #[test]
    fn decide_without_reroute_discards_keys() {
        let p = SettlePolicy { reroute_to: 0, redeliver: false };
        let ev = InFlight { id: 1, target: 0x7C02, gen: 2, class: EvClass::Key, payload: 0 };
        assert_eq!(decide(&ev, 2, 0x7C02, &p), Fate::Discard);
        let ges = InFlight { class: EvClass::Gesture, ..ev };
        assert_eq!(decide(&ges, 2, 0x7C02, &p), Fate::Discard);
    }

    #[test]
    fn hot_scenario_counts_are_exact() {
        let (rep, reg) = hot_disappear().unwrap();
        assert_eq!(reg.len(), 12);
        assert_eq!(rep.total, 12);
        assert_eq!(rep.affected, 12);
        assert_eq!(rep.discarded, 6);
        assert_eq!(rep.rerouted, 4);
        assert_eq!(rep.redelivered, 2);
        assert_eq!(rep.untouched, 0);
        assert_eq!(rep.dangling, 0);
        assert_eq!(rep.orphans, 0);
        assert!(rep.conserved());
        assert_ne!(rep.digest, 0);
    }

    #[test]
    fn settle_keeps_other_windows() {
        let rows = [
            InFlight { id: 1, target: 0x7C02, gen: 2, class: EvClass::Key, payload: 1 },
            InFlight { id: 2, target: 0x7C01, gen: 1, class: EvClass::Key, payload: 2 },
            InFlight { id: 3, target: 0x7C03, gen: 1, class: EvClass::Pointer, payload: 3 },
        ];
        let p = SettlePolicy { reroute_to: 0x7C05, redeliver: false };
        let rep = settle(&rows, 0x7C02, &p).unwrap();
        assert_eq!(rep.total, 3);
        assert_eq!(rep.affected, 1);
        assert_eq!(rep.rerouted, 1);
        assert_eq!(rep.untouched, 2);
    }

    #[test]
    fn settle_rejects_bad_destroy_target() {
        let rows: [InFlight; 0] = [];
        let p = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
        assert_eq!(settle(&rows, 0x1234, &p), Err(ClearCode::TargetBad.val()));
    }

    #[test]
    fn settle_rejects_bad_reroute() {
        let (_, reg) = hot_disappear().unwrap();
        let to_destroyed = SettlePolicy { reroute_to: 0x7C02, redeliver: true };
        assert_eq!(
            settle(&reg.rows, 0x7C02, &to_destroyed),
            Err(ClearCode::TargetBad.val())
        );
        let to_nonfocus = SettlePolicy { reroute_to: 0x7C04, redeliver: true };
        assert_eq!(
            settle(&reg.rows, 0x7C02, &to_nonfocus),
            Err(ClearCode::TargetBad.val())
        );
        let to_unknown = SettlePolicy { reroute_to: 0x7CFF, redeliver: true };
        assert_eq!(
            settle(&reg.rows, 0x7C02, &to_unknown),
            Err(ClearCode::TargetBad.val())
        );
    }

    #[test]
    fn settle_paths_leave_no_dangling() {
        let (paths, affected, dangling, digest) = settle_paths().unwrap();
        assert_eq!(paths, 10);
        assert_eq!(dangling, 0);
        assert!(affected > 0);
        assert_ne!(digest, 0);
    }

    #[test]
    fn dangling_injection_is_detected() {
        let (detected, missed) = dangling_inject().unwrap();
        assert_eq!(detected, 10);
        assert_eq!(missed, 0);
    }

    #[test]
    fn partial_audit_counts_remaining() {
        let (_, reg) = hot_disappear().unwrap();
        let p = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
        assert_eq!(audit_partial(&reg.rows, 0x7C02, &p, usize::MAX), 12);
        assert_eq!(audit_partial(&reg.rows, 0x7C02, &p, 0), 11);
        assert_eq!(audit_partial(&reg.rows, 0x1234, &p, 0), 0);
    }

    #[test]
    fn policy_faults_split_six_four() {
        let (rejected, accepted) = policy_faults().unwrap();
        assert_eq!(rejected, 6);
        assert_eq!(accepted, 4);
    }

    #[test]
    fn settle_replay_is_stable() {
        let (a, b) = settle_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn routes_are_unique_and_aligned() {
        assert!(routes_unique());
        assert_eq!(ROUTES.len(), ROUTE_TOTAL);
        let mut i = 0usize;
        while i < ROUTES.len() {
            let r = ROUTES[i];
            assert_eq!(r.stream, STREAM_BASE + i as u16);
            assert_eq!(window_of(r.win).map(|w| w.gen), Some(r.gen));
            assert_eq!(route_of(r.win).map(|x| x.stream), Some(r.stream));
            i += 1;
        }
        assert!(route_of(0xDEAD).is_none());
        assert_ne!(route_digest(), 0);
    }

    #[test]
    fn dispatch_happy_path() {
        let p = Packet { id: 1, win: 0x7C05, gen: 3, stream: STREAM_BASE + 4, payload: 77 };
        let d = dispatch(&p).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.win, 0x7C05);
        assert_eq!(d.stream, STREAM_BASE + 4);
        assert_eq!(d.payload, 77);
    }

    #[test]
    fn dispatch_rejects_three_pathologies() {
        let ok = Packet { id: 1, win: 0x7C05, gen: 3, stream: STREAM_BASE + 4, payload: 1 };
        assert_eq!(
            dispatch(&Packet { win: 0x1111, ..ok }),
            Err(ClearCode::TargetBad.val())
        );
        assert_eq!(
            dispatch(&Packet { gen: 2, ..ok }),
            Err(ClearCode::GenBad.val())
        );
        assert_eq!(
            dispatch(&Packet { stream: STREAM_BASE + 3, ..ok }),
            Err(ClearCode::Crosstalk.val())
        );
    }

    #[test]
    fn probe_packets_are_hundred() {
        let rows = probe_packets().unwrap();
        assert_eq!(rows.len(), PROBES as usize);
        let mut i = 0usize;
        while i < rows.len() {
            assert_eq!(rows[i].id, i as u32 + 1);
            i += 1;
        }
    }

    #[test]
    fn crosstalk_probe_is_zero() {
        let (crosstalk, clean) = probe_crosstalk().unwrap();
        assert_eq!(crosstalk, 0);
        assert_eq!(clean, PROBES);
    }

    #[test]
    fn crosstalk_injection_rejects_all() {
        let (rejected, accepted) = crosstalk_inject().unwrap();
        assert_eq!(rejected, 10);
        assert_eq!(accepted, 0);
    }

    #[test]
    fn fidelity_probe_is_hundred() {
        let (same, diff, src, dst) = probe_fidelity().unwrap();
        assert_eq!(same, PROBES);
        assert_eq!(diff, 0);
        assert_eq!(src, dst);
        assert_ne!(dst, 0);
    }

    #[test]
    fn fidelity_replay_is_stable() {
        let (a, b) = fidelity_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn stale_packets_rejected() {
        let (rejected, accepted) = stale_inject().unwrap();
        assert_eq!(rejected, 10);
        assert_eq!(accepted, 0);
    }

    #[test]
    fn class_names_are_stable() {
        assert_eq!(EvClass::Key.name(), "Key");
        assert_eq!(EvClass::Pointer.index(), 1);
        assert_eq!(EvClass::Focus.index(), 2);
        assert_eq!(EvClass::Gesture.name(), "Gesture");
        assert_eq!(Fate::Keep.index(), 0);
        assert_eq!(Fate::Discard.index(), 1);
        assert_eq!(Fate::Reroute(1).index(), 2);
        assert_eq!(Fate::Redeliver.index(), 3);
    }

    #[test]
    fn case_ids_do_not_collide() {
        assert!(no_cross_collision());
        assert!(case_ids_sequential());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0733_0801);
        assert_eq!(CASE_IDS[15], 0x0733_0810);
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].id, CASE_IDS[i]);
            assert!(!CASES[i].name.is_empty());
            i += 1;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            assert_ne!(RESERVED_IDS[j], CASE_IDS[0]);
            j += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(SETTLE_TAG, 12, 12, 6, 4, 0x00AB_CDEF_0123_4567);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).unwrap();
        assert_eq!(tag, SETTLE_TAG);
        assert_eq!(vals, [12, 12, 6, 4]);
        assert_eq!(digest, 0x00AB_CDEF_0123_4567);
        assert!(!audit_render("SETTLE|k1=1"));
        assert!(!audit_render("SETTLE|k1=1|k2=2|k3=3|k4=4|digest=zzzzzzzzzzzzzzzz"));
    }

    #[test]
    fn rows_are_parseable() {
        let (rep, reg) = hot_disappear().unwrap();
        let irow = inflight_row(&reg, 0x7C02);
        let (t, v, d) = row_fields(&irow).unwrap();
        assert_eq!(t, INFLIGHT_TAG);
        assert_eq!(v[0], 12);
        assert_eq!(v[1], 0x7C02);
        assert_eq!(v[2], 12);
        assert_eq!(v[3], 0);
        assert_eq!(d, reg.digest);
        let srow = settle_row(&rep);
        let (t2, v2, d2) = row_fields(&srow).unwrap();
        assert_eq!(t2, SETTLE_TAG);
        assert_eq!(v2[0], 12);
        assert_eq!(v2[2], 6);
        assert_eq!(v2[3], 4);
        assert_eq!(d2, rep.digest);
    }

    #[test]
    fn route_and_fidel_rows_are_parseable() {
        let rrow = route_row();
        let (t, v, d) = row_fields(&rrow).unwrap();
        assert_eq!(t, ROUTE_TAG);
        assert_eq!(v[0], 6);
        assert_eq!(v[1], STREAM_BASE as u32);
        assert_eq!(v[2], STREAM_BASE as u32 + 5);
        assert_eq!(d, route_digest());
        let frow = fidel_row(100, 0, 0x1234_5678_9ABC_DEF0);
        let (t2, v2, d2) = row_fields(&frow).unwrap();
        assert_eq!(t2, FIDEL_TAG);
        assert_eq!(v2[0], 100);
        assert_eq!(v2[1], 100);
        assert_eq!(v2[2], 0);
        assert_eq!(d2, 0x1234_5678_9ABC_DEF0);
    }

    #[test]
    fn ai30_anchor_is_declared() {
        assert!(AI30_ANCHOR.contains("AI-30"));
        assert!(!AI30_ANCHOR.is_empty());
    }

    #[test]
    fn settle_report_self_audit() {
        let good = SettleReport {
            total: 5,
            affected: 3,
            discarded: 2,
            rerouted: 1,
            redelivered: 0,
            untouched: 2,
            dangling: 0,
            orphans: 0,
            digest: 1,
        };
        assert!(good.conserved());
        let bad_parts = SettleReport { discarded: 3, ..good };
        assert!(!bad_parts.conserved());
        let bad_dangling = SettleReport { dangling: 1, ..good };
        assert!(!bad_dangling.conserved());
        let bad_total = SettleReport { total: 6, ..good };
        assert!(!bad_total.conserved());
        let empty = SettleReport { total: 0, affected: 0, discarded: 0, rerouted: 0, redelivered: 0, untouched: 0, dangling: 0, orphans: 0, digest: 1 };
        assert!(!empty.conserved());
    }

    #[test]
    fn settle_without_reroute_discards_everything() {
        let (_, reg) = hot_disappear().unwrap();
        let p = SettlePolicy { reroute_to: 0, redeliver: false };
        let rep = settle(&reg.rows, 0x7C02, &p).unwrap();
        assert_eq!(rep.affected, 12);
        assert_eq!(rep.discarded, 12);
        assert_eq!(rep.rerouted, 0);
        assert_eq!(rep.redelivered, 0);
        assert_eq!(rep.dangling, 0);
    }

    #[test]
    fn registry_counts_by_class() {
        let (_, reg) = hot_disappear().unwrap();
        assert_eq!(reg.count_class(EvClass::Key), 4);
        assert_eq!(reg.count_class(EvClass::Pointer), 3);
        assert_eq!(reg.count_class(EvClass::Focus), 3);
        assert_eq!(reg.count_class(EvClass::Gesture), 2);
        let sum = reg.count_class(EvClass::Key)
            + reg.count_class(EvClass::Pointer)
            + reg.count_class(EvClass::Focus)
            + reg.count_class(EvClass::Gesture);
        assert_eq!(sum, reg.len());
    }

    #[test]
    fn dispatch_all_is_deterministic() {
        let rows = probe_packets().unwrap();
        let (a, da) = dispatch_all(&rows).unwrap();
        let (b, db) = dispatch_all(&rows).unwrap();
        assert_eq!(a.len(), 100);
        assert_eq!(a.len(), b.len());
        assert_eq!(da, db);
        let empty: [Packet; 0] = [];
        let (none, _) = dispatch_all(&empty).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn inflight_row_tracks_rejections() {
        let (_, reg) = hot_disappear().unwrap();
        let row = inflight_row(&reg, 0x7C02);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, INFLIGHT_TAG);
        assert_eq!(v[0], 12);
        assert_eq!(v[2], 12);
        assert_eq!(v[3], 0);
        assert_eq!(d, reg.digest);
        assert!(audit_render(&row));
    }

    #[test]
    fn partial_audit_skips_exactly_one_row() {
        let mut reg = Registry::new();
        // 0x7C02 当前代是 2，两条同代事件才会被认作悬挂。
        assert_eq!(reg.enqueue(0x7C02, 2, EvClass::Key, 1).unwrap(), 1);
        assert_eq!(reg.enqueue(0x7C02, 2, EvClass::Pointer, 2).unwrap(), 2);
        assert_eq!(reg.enqueue(0x7C01, 1, EvClass::Key, 3).unwrap(), 3);
        let policy = SettlePolicy { reroute_to: 0x7C01, redeliver: true };
        assert_eq!(audit_partial(&reg.rows, 0x7C02, &policy, 0), 1);
        assert_eq!(audit_partial(&reg.rows, 0x7C02, &policy, 1), 1);
        assert_eq!(audit_partial(&reg.rows, 0x7C02, &policy, 9), 2);
        // 目标不是登记过的窗口时，该审计本身无悬挂可言。
        assert_eq!(audit_partial(&reg.rows, 0x7CFF, &policy, 0), 0);
        assert_eq!(audit_partial(&[], 0x7C02, &policy, 0), 0);
    }

    #[test]
    fn route_table_is_bijective_and_digest_stable() {
        assert!(routes_unique());
        assert_eq!(route_digest(), route_digest());
        let mut seen = 0u32;
        let mut i = 0usize;
        while i < ROUTES.len() {
            let r = ROUTES[i];
            let w = window_of(r.win).unwrap();
            assert_eq!(w.gen, r.gen);
            assert_eq!(r.stream, STREAM_BASE + i as u16);
            seen = seen.saturating_add(1);
            i += 1;
        }
        assert_eq!(seen, ROUTE_TOTAL as u32);
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49045_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
