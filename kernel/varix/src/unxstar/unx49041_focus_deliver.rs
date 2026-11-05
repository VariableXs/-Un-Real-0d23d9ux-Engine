//! UNX-F49041 keyboard 焦点递送总成 · 0x73000 段（B13 批首槽）
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，不得改写）：
//!   UNX-F49041 keyboard 焦点递送总成（按 vxwm 判定递送——判定权在 F1 的分界执行）:
//!          ① 纯功能代码 ≥400 行；
//!          ② 判据原文：UNX-F49042-J1 进出 ×10^2 事件序正确；判据锚定 §AI-62 §2 专题六；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49042 pointer 焦点/进入/离开语义:
//!          ① 纯功能代码 ≥380 行；
//!          ② 判据原文：UNX-F49043-J1 ×10^3 切换零丢复跑；判据锚定 §AI-62 §5 J-2；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（卡一 F49041）：定链 → 判权 → 递送 → 配对 → 复算。
//! 定链先于判权：焦点链必须先能回答「当前是谁、候选是谁」，否则"由谁判"
//! 是无对象的空谈；判权先于递送：判定权在 vxwm 侧、在 F1 分界行上执行，
//! 本总成只搬运，不裁决——先锚定这条边界，递送记录才有"授权来源"字段；
//! 递送先于配对：四条事件（FocusOut/Leave/Enter/FocusIn）先落序号再谈闭合；
//! 配对先于复算：只有成对闭合的序列，重复跑一遍的摘要才可能相同。
//! 闸门次序（卡二 F49042）：分焦 → 建栈 → 悬停 → 点击 → 千次。
//! 分焦先于建栈：pointer 焦点与 keyboard 焦点是两本账，混为一谈则"悬停
//! 不夺焦"无从判定；建栈先于悬停：进入/离开必须落在一条祖先链上（进入
//! 子窗口之前必先进入其父），链不成立时"悬停"只是坐标噪声；悬停先于点击：
//! 悬停改的是指针账、点击才可能改键盘账，次序颠倒就会把悬停误记为夺焦；
//! 点击先于千次：夺焦规则（可聚焦 + 允许夺焦）定稿后，千次混合操作的
//! 零丢断言才有确定的口径。
//! 关键差异：F49041 的账落在"事件序"（一次切换 = 四条带序号的配对事件，
//! 只审顺序与归属，不审坐标）；F49042 的账落在"栈与夺焦权"（进入/离开
//! 的祖先链闭合、悬停与点击分离、千次混合操作零悬挂）。
//! 两卡共用同一个 `FocusChain` 作为键盘焦点唯一事实源：pointer 侧只能
//! 通过 `click` 请求它变更，不得直接写 `current`——这条纪律是"指针焦点
//! 与键盘焦点分离"在代码层面的落点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73000，B13 批首槽）。
pub const SEG_BASE: u32 = 0x73000;

/// F1 分界执行行锚（判定权在 vxwm，执行点固定在这一行）。
pub const BOUNDARY_ANCHOR: u32 = 0x0730_1F01;
/// 判定权归属标签（仅作文字锚定，本域不持有判定权）。
pub const JUDGE_OWNER: &str = "vxwm/F1";
/// 窗口表容量。
pub const WINDOW_TOTAL: usize = 6;
/// 键盘焦点历史栈深度上限。
pub const HISTORY_CAP: usize = 4;
/// 卡一判据的事件序规模（×10^2 次进出）。
pub const HUNDRED: u32 = 100;
/// 每次切换固定产生的事件条数。
pub const EVENTS_PER_SWITCH: usize = 4;
/// 每次切换固定配对的对数。
pub const PAIRS_PER_SWITCH: u32 = 2;
/// 卡二判据的切换规模（×10^3）。
pub const THOUSAND: u32 = 1_000;
/// 指针进入栈深度上限（桌面之下最多四层）。
pub const STACK_CAP: usize = 4;
/// 桌面根窗口编号（0 表示无窗口覆盖）。
pub const ROOT: u32 = 0;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 键盘焦点账标签。
pub const KFOCUS_TAG: &str = "KFOCUS";
/// 指针焦点账标签。
pub const PTRFOC_TAG: &str = "PTRFOC";
/// 窗口声明行标签。
pub const WIN_TAG: &str = "WIN";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 F49041 段诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusCode {
    /// 焦点链不成立（历史栈空或当前窗口不在链上）
    ChainBad,
    /// 目标窗口不存在或不可聚焦
    TargetBad,
    /// 目标与当前相同却走了非幂等路径
    SameBad,
    /// 事件序断裂（序号不连续或种类错位）
    OrderBad,
    /// 进出事件未成对闭合
    PairBad,
    /// 判定权越界（本域不得自行裁决递送目标）
    CourtBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 覆盖不足（样本或事件条数不达标）
    CoverageBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 指针进入栈不是一条祖先链
    StackBad,
    /// 点击语义非法（不可聚焦窗口被记为夺焦）
    ClickBad,
    /// 计数溢出
    Overflow,
    /// 摘要为零（折叠未发生）
    DigestBad,
}

impl FocusCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            FocusCode::ChainBad => "焦点链不成立：历史栈空或当前窗口不在链上",
            FocusCode::TargetBad => "目标窗口不存在或不可聚焦",
            FocusCode::SameBad => "目标与当前相同却走了非幂等路径",
            FocusCode::OrderBad => "事件序断裂：序号不连续或种类错位",
            FocusCode::PairBad => "进出事件未成对闭合",
            FocusCode::CourtBad => "判定权越界：本域不得自行裁决递送目标",
            FocusCode::RenderBad => "渲染字段不可解析",
            FocusCode::ReplayBad => "复跑摘要漂移",
            FocusCode::CoverageBad => "覆盖不足：样本或事件条数不达标",
            FocusCode::SegmentBad => "诊断码越出段位窗口",
            FocusCode::StackBad => "指针进入栈不是一条祖先链",
            FocusCode::ClickBad => "点击语义非法：不可聚焦窗口被记为夺焦",
            FocusCode::Overflow => "计数溢出",
            FocusCode::DigestBad => "摘要为零：折叠未发生",
        }
    }

    pub const ALL: [FocusCode; 14] = [
        FocusCode::ChainBad,
        FocusCode::TargetBad,
        FocusCode::SameBad,
        FocusCode::OrderBad,
        FocusCode::PairBad,
        FocusCode::CourtBad,
        FocusCode::RenderBad,
        FocusCode::ReplayBad,
        FocusCode::CoverageBad,
        FocusCode::SegmentBad,
        FocusCode::StackBad,
        FocusCode::ClickBad,
        FocusCode::Overflow,
        FocusCode::DigestBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FocusCode::ALL.len() {
        if let Some(c) = FocusCode::ALL.get(i) {
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

/// 段位守卫：14 条码必须连续落在 0x73001..0x7300E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < FocusCode::ALL.len() {
        let c = match FocusCode::ALL.get(i) {
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

// ── 窗口表（静态、逐条手写） ─────────────────────────────────────
/// 一个可参与焦点递送的窗口：编号、代数、父窗口、可聚焦、允许夺焦。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinNode {
    pub id: u32,
    pub gen: u16,
    pub parent: u32,
    pub focusable: bool,
    pub take_focus: bool,
}

/// 六窗口静态表：
/// - 0x5A01 shell（根壳，父=桌面）
/// - 0x5A02 editor（父 shell）
/// - 0x5A03 panel（父 shell，不可聚焦：状态面板）
/// - 0x5A04 dialog（父 editor，模态对话框）
/// - 0x5A05 hud（父桌面，不可聚焦：抬头层）
/// - 0x5A06 terminal（父 shell，可聚焦但不接受点击夺焦）
pub const WINDOWS: [WinNode; WINDOW_TOTAL] = [
    WinNode { id: 0x5A01, gen: 1, parent: ROOT, focusable: true, take_focus: true },
    WinNode { id: 0x5A02, gen: 1, parent: 0x5A01, focusable: true, take_focus: true },
    WinNode { id: 0x5A03, gen: 2, parent: 0x5A01, focusable: false, take_focus: false },
    WinNode { id: 0x5A04, gen: 1, parent: 0x5A02, focusable: true, take_focus: true },
    WinNode { id: 0x5A05, gen: 3, parent: ROOT, focusable: false, take_focus: false },
    WinNode { id: 0x5A06, gen: 2, parent: 0x5A01, focusable: true, take_focus: false },
];

/// 按编号取窗口声明。
pub fn lookup(id: u32) -> Option<WinNode> {
    if id == ROOT {
        return None;
    }
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if let Some(n) = WINDOWS.get(i) {
            if n.id == id {
                return Some(*n);
            }
        }
        i += 1;
    }
    None
}

/// 可聚焦窗口编号表（递送候选域）。
pub fn focusable_ids() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if let Some(n) = WINDOWS.get(i) {
            if n.focusable {
                out.push(n.id);
            }
        }
        i += 1;
    }
    out
}

/// 祖先链（根 → 目标窗口，含目标；桌面返回空链）。
pub fn ancestors(id: u32) -> Option<Vec<u32>> {
    if id == ROOT {
        return Some(Vec::new());
    }
    let mut out: Vec<u32> = Vec::new();
    let mut cur = id;
    let mut guard = 0usize;
    while cur != ROOT {
        if guard >= STACK_CAP + 1 {
            return None;
        }
        let node = lookup(cur)?;
        out.push(node.id);
        cur = node.parent;
        guard += 1;
    }
    // 反转成根在前
    let mut rev: Vec<u32> = Vec::new();
    let mut i = out.len();
    while i > 0 {
        i -= 1;
        if let Some(v) = out.get(i) {
            rev.push(*v);
        }
    }
    Some(rev)
}

/// 进入栈合法性：首元素父为桌面、后续每层父为前一层。
pub fn chain_valid(stack: &[u32]) -> bool {
    if stack.len() > STACK_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < stack.len() {
        let node = match stack.get(i).and_then(|v| lookup(*v)) {
            Some(n) => n,
            None => return false,
        };
        let want_parent = if i == 0 {
            ROOT
        } else {
            match stack.get(i - 1) {
                Some(v) => *v,
                None => return false,
            }
        };
        if node.parent != want_parent {
            return false;
        }
        i += 1;
    }
    true
}

// ── 用例编号（16 条，base+0x01..base+0x10）与保留号 ───────────────
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

/// 本模块 16 个用例编号（0x0733_0001..=0x0733_0010）。
pub const CASE_IDS: [u32; 16] = [
    0x0733_0001,
    0x0733_0002,
    0x0733_0003,
    0x0733_0004,
    0x0733_0005,
    0x0733_0006,
    0x0733_0007,
    0x0733_0008,
    0x0733_0009,
    0x0733_000A,
    0x0733_000B,
    0x0733_000C,
    0x0733_000D,
    0x0733_000E,
    0x0733_000F,
    0x0733_0010,
];

/// 用例编号与语义的绑定表（编号不是装饰，逐条挂到功能单元上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
}

/// 16 条用例声明。
pub const CASES: [CaseSpec; 16] = [
    CaseSpec { id: 0x0733_0001, name: "焦点链初建" },
    CaseSpec { id: 0x0733_0002, name: "递送四事件" },
    CaseSpec { id: 0x0733_0003, name: "同目标幂等" },
    CaseSpec { id: 0x0733_0004, name: "不可聚焦拒收" },
    CaseSpec { id: 0x0733_0005, name: "未知窗口拒收" },
    CaseSpec { id: 0x0733_0006, name: "分界锚校验" },
    CaseSpec { id: 0x0733_0007, name: "判定权外置" },
    CaseSpec { id: 0x0733_0008, name: "配对闭合" },
    CaseSpec { id: 0x0733_0009, name: "事件序连续" },
    CaseSpec { id: 0x0733_000A, name: "百次进出审计" },
    CaseSpec { id: 0x0733_000B, name: "递送复跑" },
    CaseSpec { id: 0x0733_000C, name: "指针进入链" },
    CaseSpec { id: 0x0733_000D, name: "指针离开链" },
    CaseSpec { id: 0x0733_000E, name: "悬停不夺焦" },
    CaseSpec { id: 0x0733_000F, name: "点击夺焦" },
    CaseSpec { id: 0x0733_0010, name: "千次切换零丢" },
];

/// 跨槽零撞号：16 条用例号内部互异、与保留号不相交、且不落在诊断码窗口。
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

/// 用例编号连续：CASE_IDS[i] == 0x0733_0000 + 1 + i。
pub fn case_ids_sequential() -> bool {
    let base: u32 = 0x0733_0000;
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

// ── 焦点事件与递送（F49041） ─────────────────────────────────────
/// 焦点事件的四种类别（一次切换固定按此顺序发出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    FocusOut,
    Leave,
    Enter,
    FocusIn,
}

impl EvKind {
    pub fn index(self) -> u8 {
        match self {
            EvKind::FocusOut => 0,
            EvKind::Leave => 1,
            EvKind::Enter => 2,
            EvKind::FocusIn => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EvKind::FocusOut => "FocusOut",
            EvKind::Leave => "Leave",
            EvKind::Enter => "Enter",
            EvKind::FocusIn => "FocusIn",
        }
    }
}

/// 事件授权来源：分界裁决（vxwm 判）还是本域执行（搬运）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    Boundary,
    Execute,
}

impl Cause {
    pub fn index(self) -> u8 {
        match self {
            Cause::Boundary => 0,
            Cause::Execute => 1,
        }
    }
}

/// 一条焦点事件：序号、目标窗口、类别、授权来源、分界锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusEvent {
    pub seq: u32,
    pub win: u32,
    pub kind: EvKind,
    pub cause: Cause,
    pub anchor: u32,
}

/// 键盘焦点链：历史栈（当前在首位）、当前窗口、事件序号水位、摘要。
#[derive(Clone, Debug)]
pub struct FocusChain {
    pub order: Vec<u32>,
    pub current: u32,
    pub serial: u32,
    pub switches: u32,
    pub digest: u64,
}

impl FocusChain {
    /// 初建：焦点落在 editor（0x5A02），历史栈只含当前一项。
    pub fn new() -> FocusChain {
        let mut order: Vec<u32> = Vec::new();
        order.push(0x5A02);
        let mut digest = p_core::FNV_OFF;
        digest = p_core::fold_u32(digest, 0x5A02);
        digest = p_core::fold_byte(digest, 0x01);
        FocusChain {
            order,
            current: 0x5A02,
            serial: 0,
            switches: 0,
            digest,
        }
    }

    /// 当前窗口是否在历史栈内（链完整性旁证）。
    pub fn contains(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.order.len() {
            if let Some(v) = self.order.get(i) {
                if *v == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 链自洽：非空、首元素等于当前、无重复。
    pub fn chain_ok(&self) -> bool {
        if self.order.is_empty() {
            return false;
        }
        match self.order.first() {
            Some(v) => {
                if *v != self.current {
                    return false;
                }
            }
            None => return false,
        }
        let mut i = 0usize;
        while i < self.order.len() {
            let a = match self.order.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if lookup(a).is_none() {
                return false;
            }
            let mut j = i + 1;
            while j < self.order.len() {
                if let Some(b) = self.order.get(j) {
                    if *b == a {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 递送：目标必须存在且可聚焦；同目标返回空序列（幂等，不动水位）。
    pub fn focus(&mut self, target: u32) -> Result<Vec<FocusEvent>, u32> {
        let node = lookup(target).ok_or(FocusCode::TargetBad.val())?;
        if !node.focusable {
            return Err(FocusCode::TargetBad.val());
        }
        let from = self.current;
        if from == target {
            return Ok(Vec::new());
        }
        let kinds = [EvKind::FocusOut, EvKind::Leave, EvKind::Enter, EvKind::FocusIn];
        let wins = [from, from, target, target];
        let causes = [Cause::Boundary, Cause::Execute, Cause::Execute, Cause::Boundary];
        let mut out: Vec<FocusEvent> = Vec::new();
        let mut i = 0usize;
        while i < EVENTS_PER_SWITCH {
            self.serial = match self.serial.checked_add(1) {
                Some(v) => v,
                None => return Err(FocusCode::Overflow.val()),
            };
            let kind = match kinds.get(i) {
                Some(v) => *v,
                None => return Err(FocusCode::Overflow.val()),
            };
            let win = match wins.get(i) {
                Some(v) => *v,
                None => return Err(FocusCode::Overflow.val()),
            };
            let cause = match causes.get(i) {
                Some(v) => *v,
                None => return Err(FocusCode::Overflow.val()),
            };
            out.push(FocusEvent {
                seq: self.serial,
                win,
                kind,
                cause,
                anchor: BOUNDARY_ANCHOR,
            });
            i += 1;
        }
        // 历史栈：目标置顶、去重、截断到 HISTORY_CAP（当前永远保留）。
        let mut next: Vec<u32> = Vec::new();
        next.push(target);
        let mut j = 0usize;
        while j < self.order.len() {
            if let Some(v) = self.order.get(j).copied() {
                if v != target && next.len() < HISTORY_CAP {
                    next.push(v);
                }
            }
            j += 1;
        }
        self.order = next;
        self.current = target;
        self.switches = self.switches.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, from);
        self.digest = p_core::fold_u32(self.digest, target);
        self.digest = p_core::fold_u32(self.digest, self.serial);
        Ok(out)
    }
}

/// 单次切换的四事件审计：次序 → 归属 → 序号连续 → 分界锚 → 授权来源。
pub fn audit_switch(events: &[FocusEvent], from: u32, to: u32) -> Result<(), u32> {
    if events.len() != EVENTS_PER_SWITCH {
        return Err(FocusCode::PairBad.val());
    }
    let want = [EvKind::FocusOut, EvKind::Leave, EvKind::Enter, EvKind::FocusIn];
    let want_win = [from, from, to, to];
    let want_cause = [Cause::Boundary, Cause::Execute, Cause::Execute, Cause::Boundary];
    let mut i = 0usize;
    while i < EVENTS_PER_SWITCH {
        let e = match events.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let k = match want.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let w = match want_win.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let c = match want_cause.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        if e.kind != k || e.win != w {
            return Err(FocusCode::OrderBad.val());
        }
        if e.cause != c {
            return Err(FocusCode::CourtBad.val());
        }
        if e.anchor != BOUNDARY_ANCHOR {
            return Err(FocusCode::CourtBad.val());
        }
        if i > 0 {
            let p = match events.get(i - 1) {
                Some(v) => *v,
                None => return Err(FocusCode::Overflow.val()),
            };
            if match p.seq.checked_add(1) {
                Some(v) => v != e.seq,
                None => true,
            } {
                return Err(FocusCode::OrderBad.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 配对闭合计数：返回 (FocusOut-Leave 对, Enter-FocusIn 对)。
pub fn pair_counts(events: &[FocusEvent]) -> (u32, u32) {
    let mut out_leave = 0u32;
    let mut enter_in = 0u32;
    let mut i = 0usize;
    while i < events.len() {
        if let Some(a) = events.get(i).copied() {
            if a.kind == EvKind::FocusOut {
                let mut j = i + 1;
                while j < events.len() {
                    if let Some(b) = events.get(j).copied() {
                        if b.kind == EvKind::Leave && b.win == a.win && b.seq > a.seq {
                            out_leave = out_leave.saturating_add(1);
                            break;
                        }
                    }
                    j += 1;
                }
            }
            if a.kind == EvKind::Enter {
                let mut j = i + 1;
                while j < events.len() {
                    if let Some(b) = events.get(j).copied() {
                        if b.kind == EvKind::FocusIn && b.win == a.win && b.seq > a.seq {
                            enter_in = enter_in.saturating_add(1);
                            break;
                        }
                    }
                    j += 1;
                }
            }
        }
        i += 1;
    }
    (out_leave, enter_in)
}

/// 判定权申请：本总成一概拒绝（判定权在 vxwm 的 F1 分界，不在本域）。
pub fn claim_judgment() -> Result<(), u32> {
    Err(FocusCode::CourtBad.val())
}

/// 分界锚校验：递送记录携带的锚必须是 F1 分界执行行。
pub fn anchor_ok(anchor: u32) -> bool {
    anchor == BOUNDARY_ANCHOR
}

/// 百次进出审计账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HundredLedger {
    pub transitions: u32,
    pub events: u32,
    pub paired: u32,
    pub order_faults: u32,
    pub court_faults: u32,
    pub first_seq: u32,
    pub last_seq: u32,
    pub digest: u64,
}

impl HundredLedger {
    /// 守恒：事件数 = 切换数 × 4；配对 = 切换数 × 2；序号窗口连续。
    pub fn conserved(&self) -> bool {
        if self.transitions != HUNDRED {
            return false;
        }
        if self.events != self.transitions * EVENTS_PER_SWITCH as u32 {
            return false;
        }
        if self.paired != self.transitions * PAIRS_PER_SWITCH {
            return false;
        }
        if self.first_seq != 1 {
            return false;
        }
        match self.first_seq.checked_add(self.events - 1) {
            Some(v) => v == self.last_seq,
            None => false,
        }
    }
}

/// 百次进出审计：确定性伪随机在四个可聚焦窗口之间切换，逐次审计。
pub fn hundred_switches() -> Result<HundredLedger, u32> {
    let mut chain = FocusChain::new();
    let ids = focusable_ids();
    if ids.is_empty() {
        return Err(FocusCode::CoverageBad.val());
    }
    let mut lcg = p_core::Lcg::new(0x0730_0041);
    let mut transitions = 0u32;
    let mut events = 0u32;
    let mut paired = 0u32;
    let mut order_faults = 0u32;
    let mut court_faults = 0u32;
    let mut first_seq = 0u32;
    let mut last_seq = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut attempts = 0u32;
    let limit = HUNDRED.saturating_mul(4);
    while transitions < HUNDRED && attempts < limit {
        attempts = attempts.saturating_add(1);
        let k = lcg.below(ids.len() as u32) as usize;
        let target = match ids.get(k) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let from = chain.current;
        if target == from {
            continue;
        }
        let evs = chain.focus(target)?;
        if evs.len() != EVENTS_PER_SWITCH {
            return Err(FocusCode::PairBad.val());
        }
        let first = match evs.first() {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let last = match evs.last() {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        if transitions == 0 {
            first_seq = first.seq;
        }
        last_seq = last.seq;
        match audit_switch(&evs, from, target) {
            Ok(()) => paired = paired.saturating_add(PAIRS_PER_SWITCH),
            Err(code) => {
                if code == FocusCode::OrderBad.val() {
                    order_faults = order_faults.saturating_add(1);
                } else {
                    court_faults = court_faults.saturating_add(1);
                }
            }
        }
        transitions = transitions.saturating_add(1);
        events = events.saturating_add(evs.len() as u32);
        digest = p_core::fold_u32(digest, from);
        digest = p_core::fold_u32(digest, target);
        digest = p_core::fold_u32(digest, last.seq);
    }
    if transitions != HUNDRED {
        return Err(FocusCode::CoverageBad.val());
    }
    if digest == 0 {
        return Err(FocusCode::DigestBad.val());
    }
    let ledger = HundredLedger {
        transitions,
        events,
        paired,
        order_faults,
        court_faults,
        first_seq,
        last_seq,
        digest,
    };
    if !ledger.conserved() {
        return Err(FocusCode::CoverageBad.val());
    }
    if !chain.chain_ok() {
        return Err(FocusCode::ChainBad.val());
    }
    Ok(ledger)
}

/// 百次递送复跑：两遍摘要必须一致且非零。
pub fn hundred_replay() -> Result<(u64, u64), u32> {
    let a = hundred_switches()?;
    let b = hundred_switches()?;
    Ok((a.digest, b.digest))
}

/// 单次切换的审计注入：10 种畸形序列全部必须被拒。
pub fn switch_faults() -> Result<(u32, u32), u32> {
    let mut chain = FocusChain::new();
    let from = chain.current;
    let to = 0x5A04;
    let good = chain.focus(to)?;
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < 10 {
        let mut probe: Vec<FocusEvent> = Vec::new();
        let mut j = 0usize;
        while j < good.len() {
            if let Some(e) = good.get(j) {
                probe.push(*e);
            }
            j += 1;
        }
        match i {
            0 => {
                probe.pop();
            }
            1 => {
                if let Some(e) = probe.get_mut(0) {
                    e.kind = EvKind::Enter;
                }
            }
            2 => {
                if let Some(e) = probe.get_mut(1) {
                    e.win = 0x5A05;
                }
            }
            3 => {
                if let Some(e) = probe.get_mut(2) {
                    e.seq = e.seq + 2;
                }
            }
            4 => {
                if let Some(e) = probe.get_mut(0) {
                    e.anchor = 0;
                }
            }
            5 => {
                if let Some(e) = probe.get_mut(3) {
                    e.cause = Cause::Execute;
                }
            }
            6 => {
                if let Some(e) = probe.get_mut(0) {
                    e.seq = 0;
                }
            }
            7 => {
                if let Some(e) = probe.get_mut(2) {
                    e.kind = EvKind::Leave;
                }
            }
            8 => {
                probe.reverse();
            }
            _ => {
                if let Some(e) = probe.get_mut(1) {
                    e.cause = Cause::Boundary;
                }
            }
        }
        if audit_switch(&probe, from, to).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 配对注入：10 种断对序列，配对计数必须严格下降。
pub fn pair_faults() -> Result<(u32, u32), u32> {
    let mut chain = FocusChain::new();
    let evs = chain.focus(0x5A01)?;
    let (ok_ol, ok_ei) = pair_counts(&evs);
    let mut broken = 0u32;
    let mut i = 0usize;
    while i < 10 {
        let mut probe: Vec<FocusEvent> = Vec::new();
        let mut j = 0usize;
        while j < evs.len() {
            if let Some(e) = evs.get(j) {
                probe.push(*e);
            }
            j += 1;
        }
        match i {
            0 => {
                probe.remove(1);
            }
            1 => {
                probe.remove(3);
            }
            2 => {
                if let Some(e) = probe.get_mut(1) {
                    e.win = 0x5A05;
                }
            }
            3 => {
                if let Some(e) = probe.get_mut(3) {
                    e.win = 0x5A05;
                }
            }
            4 => {
                probe.remove(0);
            }
            5 => {
                probe.remove(2);
            }
            6 => {
                if let Some(e) = probe.get_mut(1) {
                    e.seq = 0;
                }
            }
            7 => {
                if let Some(e) = probe.get_mut(3) {
                    e.seq = 0;
                }
            }
            8 => {
                probe.clear();
            }
            _ => {
                if let Some(e) = probe.get_mut(0) {
                    e.kind = EvKind::Enter;
                }
                if let Some(e) = probe.get_mut(2) {
                    e.kind = EvKind::FocusOut;
                }
            }
        }
        let (ol, ei) = pair_counts(&probe);
        if ol < ok_ol || ei < ok_ei {
            broken = broken.saturating_add(1);
        }
        i += 1;
    }
    Ok((broken, ok_ol + ok_ei))
}

// ── 指针焦点/进入/离开（F49042） ─────────────────────────────────
/// 指针进入栈 + 指针焦点 + 计数器 + 摘要。键盘焦点不在此结构内。
#[derive(Clone, Debug)]
pub struct PointerStack {
    pub stack: Vec<u32>,
    pub pointer_focus: u32,
    pub enters: u32,
    pub leaves: u32,
    pub moves: u32,
    pub clicks: u32,
    pub took_focus: u32,
    pub digest: u64,
}

impl PointerStack {
    /// 初建：指针在桌面，指针焦点为桌面根。
    pub fn new() -> PointerStack {
        let mut digest = p_core::FNV_OFF;
        digest = p_core::fold_u32(digest, ROOT);
        digest = p_core::fold_byte(digest, 0x02);
        PointerStack {
            stack: Vec::new(),
            pointer_focus: ROOT,
            enters: 0,
            leaves: 0,
            moves: 0,
            clicks: 0,
            took_focus: 0,
            digest,
        }
    }

    /// 进入栈是否仍是一条祖先链。
    pub fn chain_valid(&self) -> bool {
        chain_valid(&self.stack)
    }

    /// 栈顶（最内层窗口）。
    pub fn top(&self) -> u32 {
        match self.stack.last() {
            Some(v) => *v,
            None => ROOT,
        }
    }

    /// 指针移动到目标（0 = 桌面）：先离开到公共祖先，再逐层进入。
    pub fn move_to(&mut self, target: u32) -> Result<(u32, u32), u32> {
        let want = ancestors(target).ok_or(FocusCode::TargetBad.val())?;
        let mut k = 0usize;
        while k < self.stack.len() && k < want.len() {
            let a = match self.stack.get(k) {
                Some(v) => *v,
                None => break,
            };
            let b = match want.get(k) {
                Some(v) => *v,
                None => break,
            };
            if a != b {
                break;
            }
            k += 1;
        }
        let mut left = 0u32;
        while self.stack.len() > k {
            if let Some(top) = self.stack.pop() {
                left = left.saturating_add(1);
                self.leaves = self.leaves.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, top ^ 0x11);
            }
        }
        let mut entered = 0u32;
        let mut j = k;
        while j < want.len() {
            if let Some(id) = want.get(j).copied() {
                self.stack.push(id);
                entered = entered.saturating_add(1);
                self.enters = self.enters.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, id ^ 0x22);
            }
            j += 1;
        }
        self.moves = self.moves.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, target);
        if !self.chain_valid() {
            return Err(FocusCode::StackBad.val());
        }
        Ok((entered, left))
    }

    /// 点击：设指针焦点；仅当「可聚焦 + 允许夺焦 + 与当前不同」才请求键盘焦点变更。
    pub fn click(&mut self, chain: &mut FocusChain, id: u32) -> Result<ClickOutcome, u32> {
        if id == ROOT {
            self.pointer_focus = ROOT;
            self.clicks = self.clicks.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, 0x0D0D);
            return Ok(ClickOutcome {
                took_focus: false,
                kb_from: chain.current,
                kb_to: chain.current,
                focus_events: 0,
            });
        }
        let node = lookup(id).ok_or(FocusCode::TargetBad.val())?;
        self.pointer_focus = id;
        let before = chain.current;
        let mut events = 0u32;
        let mut took = false;
        if node.focusable && node.take_focus && before != id {
            let evs = chain.focus(id)?;
            events = evs.len() as u32;
            if events != EVENTS_PER_SWITCH as u32 {
                return Err(FocusCode::ClickBad.val());
            }
            took = true;
        }
        self.clicks = self.clicks.saturating_add(1);
        if took {
            self.took_focus = self.took_focus.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, id ^ 0x33);
        Ok(ClickOutcome {
            took_focus: took,
            kb_from: before,
            kb_to: chain.current,
            focus_events: events,
        })
    }
}

/// 一次点击的结果面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClickOutcome {
    pub took_focus: bool,
    pub kb_from: u32,
    pub kb_to: u32,
    pub focus_events: u32,
}

/// 千次切换账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerLedger {
    pub steps: u32,
    pub enters: u32,
    pub leaves: u32,
    pub depth: u32,
    pub clicks: u32,
    pub took_focus: u32,
    pub hover_steals: u32,
    pub stack_faults: u32,
    pub kb_moves: u32,
    pub digest: u64,
}

impl PointerLedger {
    /// 守恒：进入 - 离开 = 当前栈深；千步齐备；零吊栈故障。
    pub fn balanced(&self) -> bool {
        if self.steps != THOUSAND {
            return false;
        }
        if self.stack_faults != 0 {
            return false;
        }
        match self.leaves.checked_add(self.depth) {
            Some(v) => v == self.enters,
            None => false,
        }
    }
}

/// 千次混合切换：移动 + 周期性点击，逐步入账。
pub fn thousand_switches() -> Result<PointerLedger, u32> {
    let mut chain = FocusChain::new();
    let mut ps = PointerStack::new();
    let mut lcg = p_core::Lcg::new(0x0730_0A42);
    let mut clicks = 0u32;
    let mut took = 0u32;
    let mut steals = 0u32;
    let mut faults = 0u32;
    let mut kb_moves = 0u32;
    let mut step = 0u32;
    while step < THOUSAND {
        let target = pick_window(&mut lcg);
        let click_now = lcg.below(5) == 0;
        let before_kb = chain.current;
        ps.move_to(target)?;
        let mut moved = false;
        if click_now {
            let out = ps.click(&mut chain, target)?;
            clicks = clicks.saturating_add(1);
            if out.took_focus {
                took = took.saturating_add(1);
                moved = true;
            } else if out.focus_events != 0 {
                faults = faults.saturating_add(1);
            }
        }
        if chain.current != before_kb {
            kb_moves = kb_moves.saturating_add(1);
            if !moved {
                steals = steals.saturating_add(1);
            }
        }
        if !ps.chain_valid() {
            faults = faults.saturating_add(1);
        }
        if ps.enters < ps.leaves {
            faults = faults.saturating_add(1);
        }
        if ps.stack.len() > STACK_CAP {
            faults = faults.saturating_add(1);
        }
        step = step.saturating_add(1);
    }
    let depth = ps.stack.len() as u32;
    let mut digest = ps.digest;
    digest = p_core::fold_u32(digest, chain.current);
    digest = p_core::fold_u32(digest, chain.serial);
    digest = p_core::fold_u32(digest, ps.enters);
    digest = p_core::fold_u32(digest, ps.leaves);
    if digest == 0 {
        return Err(FocusCode::DigestBad.val());
    }
    let ledger = PointerLedger {
        steps: THOUSAND,
        enters: ps.enters,
        leaves: ps.leaves,
        depth,
        clicks,
        took_focus: took,
        hover_steals: steals,
        stack_faults: faults,
        kb_moves,
        digest,
    };
    if !ledger.balanced() {
        return Err(FocusCode::StackBad.val());
    }
    if !chain.chain_ok() {
        return Err(FocusCode::ChainBad.val());
    }
    Ok(ledger)
}

/// 千次复跑：两遍摘要一致且非零。
pub fn thousand_replay() -> Result<(u64, u64), u32> {
    let a = thousand_switches()?;
    let b = thousand_switches()?;
    Ok((a.digest, b.digest))
}

/// 确定性窗口挑选：0 表示桌面，其余落到六窗口表。
pub fn pick_window(lcg: &mut p_core::Lcg) -> u32 {
    let k = lcg.below(WINDOW_TOTAL as u32 + 1);
    if k == 0 {
        return ROOT;
    }
    match WINDOWS.get((k - 1) as usize) {
        Some(n) => n.id,
        None => ROOT,
    }
}

/// 分离探针：纯悬停不动键盘焦点；点击可夺焦。返回 (初始键盘焦点, 悬停后, 点击后, 是否夺焦)。
pub fn separation_probe() -> Result<(u32, u32, u32, u32), u32> {
    let mut chain = FocusChain::new();
    let mut ps = PointerStack::new();
    let kb0 = chain.current;
    let path = [0x5A03u32, 0x5A05, 0x5A04, 0x5A06, 0x5A01, 0x5A02];
    let mut i = 0usize;
    while i < path.len() {
        let target = match path.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        ps.move_to(target)?;
        i += 1;
    }
    let kb1 = chain.current;
    let out = ps.click(&mut chain, 0x5A04)?;
    let kb2 = chain.current;
    Ok((kb0, kb1, kb2, if out.took_focus { 1 } else { 0 }))
}

/// 悬停夺焦注入：10 次「不可夺焦路径」都必须保持键盘焦点不变。
pub fn hover_inject() -> Result<(u32, u32), u32> {
    let chain = FocusChain::new();
    let mut ps = PointerStack::new();
    let path = [0x5A03u32, 0x5A05, 0x5A06, 0x5A03, 0x5A05, 0x5A06, 0x5A03, 0x5A05, 0x5A06, 0x5A03];
    let mut held = 0u32;
    let mut stolen = 0u32;
    let mut i = 0usize;
    while i < path.len() {
        let target = match path.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let before = chain.current;
        ps.move_to(target)?;
        if chain.current == before {
            held = held.saturating_add(1);
        } else {
            stolen = stolen.saturating_add(1);
        }
        i += 1;
    }
    Ok((held, stolen))
}

/// 点击夺焦注入：10 组 (窗口, 期望是否夺焦)，逐组比对。
pub fn click_matrix() -> Result<(u32, u32), u32> {
    let cases: [(u32, bool); 10] = [
        (0x5A01, true),
        (0x5A04, true),
        (0x5A03, false),
        (0x5A05, false),
        (0x5A06, false),
        (0x5A02, false),
        (0x5A03, false),
        (0x5A05, false),
        (0x5A04, true),
        (0x5A06, false),
    ];
    let mut agreed = 0u32;
    let mut mismatched = 0u32;
    let mut i = 0usize;
    while i < cases.len() {
        let (win, expect) = match cases.get(i) {
            Some(v) => *v,
            None => return Err(FocusCode::Overflow.val()),
        };
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        ps.move_to(win)?;
        let out = ps.click(&mut chain, win)?;
        if out.took_focus == expect {
            agreed = agreed.saturating_add(1);
        } else {
            mismatched = mismatched.saturating_add(1);
        }
        i += 1;
    }
    Ok((agreed, mismatched))
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

/// 键盘焦点账行：切换数、事件数、配对数、序号水位。
pub fn kfocus_row(l: &HundredLedger) -> String {
    render_row(
        KFOCUS_TAG,
        l.transitions,
        l.events,
        l.paired,
        l.last_seq,
        l.digest,
    )
}

/// 指针焦点账行：步数、进入数、离开数、栈深。
pub fn ptrfoc_row(l: &PointerLedger) -> String {
    render_row(
        PTRFOC_TAG,
        l.steps,
        l.enters,
        l.leaves,
        l.depth,
        l.digest,
    )
}

/// 窗口声明行：编号、代数、父窗口、可聚焦位、允许夺焦位。
pub fn win_row(n: &WinNode) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, n.id);
    h = p_core::fold_u32(h, n.gen as u32);
    h = p_core::fold_u32(h, n.parent);
    h = p_core::fold_byte(h, if n.focusable { 1 } else { 0 });
    h = p_core::fold_byte(h, if n.take_focus { 1 } else { 0 });
    render_row(
        WIN_TAG,
        n.id,
        n.gen as u32,
        n.parent,
        (if n.focusable { 1 } else { 0 }) | (if n.take_focus { 2 } else { 0 }),
        h,
    )
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49041 ①：焦点链递送（百次切换、四百事件、历史栈自洽）。
    let hundred = hundred_switches();
    let chain_ok = match &hundred {
        Ok(l) => l.conserved() && l.digest != 0,
        Err(_) => false,
    };
    out.push((
        "F49041/焦点链递送",
        chain_ok,
        format!(
            "tr={:?} ev={:?} dig={:016X}",
            hundred.as_ref().map(|l| l.transitions),
            hundred.as_ref().map(|l| l.events),
            hundred.as_ref().map(|l| l.digest).map_or(0, |v| v)
        ),
    ));

    // F49041 ②：进出事件配对（每切换两对、十种断对全部降配对）。
    let pairs = pair_faults();
    let pair_ok = match (&hundred, &pairs) {
        (Ok(l), Ok((broken, base))) => l.paired == l.transitions * PAIRS_PER_SWITCH && *broken == 10 && *base == 2,
        _ => false,
    };
    out.push((
        "F49041/进出事件配对",
        pair_ok,
        format!(
            "paired={:?} broken={:?} base={:?}",
            hundred.as_ref().map(|l| l.paired),
            pairs.as_ref().map(|(b, _)| *b),
            pairs.as_ref().map(|(_, v)| *v)
        ),
    ));

    // F49041 ③：百次进出审计（零序故障、零判权故障、序号窗口连续）。
    let faults = switch_faults();
    let audit_ok = match (&hundred, &faults) {
        (Ok(l), Ok((r, a))) => {
            l.order_faults == 0 && l.court_faults == 0 && *r == 10 && *a == 0 && l.first_seq == 1
        }
        _ => false,
    };
    out.push((
        "F49041/百次进出审计",
        audit_ok,
        format!(
            "ord={:?} crt={:?} rej={:?} acc={:?}",
            hundred.as_ref().map(|l| l.order_faults),
            hundred.as_ref().map(|l| l.court_faults),
            faults.as_ref().map(|(r, _)| *r),
            faults.as_ref().map(|(_, a)| *a)
        ),
    ));

    // F49041 ④：判定权分界（本域申请判权必拒、锚定行恒定、复跑稳定）。
    let rep = hundred_replay();
    let claim = claim_judgment();
    let court_ok = match &rep {
        Ok((a, b)) => *a == *b && *a != 0,
        Err(_) => false,
    };
    out.push((
        "F49041/判定权分界",
        claim.is_err() && court_ok && anchor_ok(BOUNDARY_ANCHOR) && !anchor_ok(0),
        format!(
            "claim={} anchor={} replay={}",
            claim.is_err(),
            anchor_ok(BOUNDARY_ANCHOR),
            court_ok
        ),
    ));

    // F49042 ①：指针焦点分离（悬停不动键盘焦点、点击才动）。
    let sep = separation_probe();
    let sep_ok = match &sep {
        Ok((kb0, kb1, kb2, took)) => kb0 == kb1 && *kb2 == 0x5A04 && *took == 1 && *kb0 != *kb2,
        Err(_) => false,
    };
    out.push((
        "F49042/指针焦点分离",
        sep_ok,
        format!(
            "kb0={:?} kb1={:?} kb2={:?} took={:?}",
            sep.as_ref().map(|v| v.0),
            sep.as_ref().map(|v| v.1),
            sep.as_ref().map(|v| v.2),
            sep.as_ref().map(|v| v.3)
        ),
    ));

    // F49042 ②：进出栈一致（千步零吊栈、进入-离开=栈深）。
    let thousand = thousand_switches();
    let stack_ok = match &thousand {
        Ok(l) => l.balanced() && l.stack_faults == 0 && l.depth <= STACK_CAP as u32,
        Err(_) => false,
    };
    out.push((
        "F49042/进出栈一致",
        stack_ok,
        format!(
            "steps={:?} in={:?} out={:?} depth={:?}",
            thousand.as_ref().map(|l| l.steps),
            thousand.as_ref().map(|l| l.enters),
            thousand.as_ref().map(|l| l.leaves),
            thousand.as_ref().map(|l| l.depth)
        ),
    ));

    // F49042 ③：悬停不夺焦（十次不可夺焦路径零窃取、十组点击矩阵全一致）。
    let hover = hover_inject();
    let matrix = click_matrix();
    let hover_ok = match (&hover, &matrix, &thousand) {
        (Ok((held, stolen)), Ok((agreed, bad)), Ok(l)) => {
            *held == 10 && *stolen == 0 && *agreed == 10 && *bad == 0 && l.hover_steals == 0
        }
        _ => false,
    };
    out.push((
        "F49042/悬停不夺焦",
        hover_ok,
        format!(
            "held={:?} stolen={:?} agree={:?} bad={:?}",
            hover.as_ref().map(|(h, _)| *h),
            hover.as_ref().map(|(_, s)| *s),
            matrix.as_ref().map(|(a, _)| *a),
            matrix.as_ref().map(|(_, b)| *b)
        ),
    ));

    // F49042 ④：千次切换零丢（千步、复跑一致、渲染往返、段位守卫）。
    let trep = thousand_replay();
    let replay_ok = match &trep {
        Ok((a, b)) => a == b && *a != 0,
        Err(_) => false,
    };
    let row_ok = match &thousand {
        Ok(l) => match row_fields(&ptrfoc_row(l)) {
            Some((t, v, d)) => {
                t == PTRFOC_TAG && v[0] == THOUSAND && v[3] == l.depth && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    let krow_ok = match &hundred {
        Ok(l) => match row_fields(&kfocus_row(l)) {
            Some((t, v, d)) => {
                t == KFOCUS_TAG
                    && v[0] == HUNDRED
                    && v[1] == HUNDRED * EVENTS_PER_SWITCH as u32
                    && v[2] == HUNDRED * PAIRS_PER_SWITCH
                    && d == l.digest
            }
            None => false,
        },
        Err(_) => false,
    };
    let win_ok = match row_fields(&win_row(&WINDOWS[0])) {
        Some((t, v, d)) => t == WIN_TAG && v[0] == 0x5A01 && v[1] == 1 && v[3] == 3 && d != 0,
        None => false,
    };
    let case_ok = no_cross_collision() && case_ids_sequential() && segment_guard();
    out.push((
        "F49042/千次切换零丢",
        replay_ok && row_ok && krow_ok && win_ok && case_ok,
        format!(
            "rep={} prow={} krow={} win={} case={}",
            replay_ok, row_ok, krow_ok, win_ok, case_ok
        ),
    ));

    out
}

pub fn run_unx49041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49041 keyboard 焦点递送总成");
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
        assert_eq!(FocusCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < FocusCode::ALL.len() {
            let c = FocusCode::ALL[i];
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
        while i < FocusCode::ALL.len() {
            let c = FocusCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(0), UNKNOWN);
    }

    #[test]
    fn detail_texts_are_distinct() {
        let mut i = 0usize;
        while i < FocusCode::ALL.len() {
            let mut j = i + 1;
            while j < FocusCode::ALL.len() {
                assert_ne!(FocusCode::ALL[i].detail(), FocusCode::ALL[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn window_table_is_sane() {
        assert_eq!(WINDOWS.len(), WINDOW_TOTAL);
        let mut i = 0usize;
        while i < WINDOWS.len() {
            let n = WINDOWS[i];
            assert!(n.gen >= 1);
            if i > 0 {
                let mut j = 0usize;
                while j < i {
                    assert_ne!(WINDOWS[j].id, n.id);
                    j += 1;
                }
            }
            if n.parent != ROOT {
                assert!(lookup(n.parent).is_some());
            }
            i += 1;
        }
    }

    #[test]
    fn lookup_hits_and_misses() {
        assert_eq!(lookup(0x5A01).map(|n| n.gen), Some(1));
        assert!(lookup(0x5A03).map(|n| !n.focusable).unwrap_or(false));
        assert!(lookup(0x5A06).map(|n| !n.take_focus).unwrap_or(false));
        assert!(lookup(ROOT).is_none());
        assert!(lookup(0xDEAD).is_none());
        assert_eq!(focusable_ids().len(), 4);
    }

    #[test]
    fn ancestors_build_root_first_chain() {
        assert_eq!(ancestors(0x5A04), Some(Vec::from([0x5A01, 0x5A02, 0x5A04])));
        assert_eq!(ancestors(0x5A05), Some(Vec::from([0x5A05])));
        assert_eq!(ancestors(ROOT), Some(Vec::new()));
        assert_eq!(ancestors(0xBEEF), None);
    }

    #[test]
    fn chain_valid_checks_parents() {
        assert!(chain_valid(&[0x5A01, 0x5A02, 0x5A04]));
        assert!(chain_valid(&[]));
        assert!(!chain_valid(&[0x5A02, 0x5A01]));
        assert!(!chain_valid(&[0x5A03]));
        assert!(!chain_valid(&[0x5A01, 0x5A01]));
    }

    #[test]
    fn chain_starts_on_editor() {
        let c = FocusChain::new();
        assert_eq!(c.current, 0x5A02);
        assert_eq!(c.serial, 0);
        assert!(c.chain_ok());
        assert!(c.contains(0x5A02));
        assert_ne!(c.digest, 0);
    }

    #[test]
    fn chain_focus_emits_four_events() {
        let mut c = FocusChain::new();
        let evs = c.focus(0x5A01).unwrap();
        assert_eq!(evs.len(), 4);
        assert_eq!(evs[0].kind, EvKind::FocusOut);
        assert_eq!(evs[0].win, 0x5A02);
        assert_eq!(evs[1].kind, EvKind::Leave);
        assert_eq!(evs[2].kind, EvKind::Enter);
        assert_eq!(evs[2].win, 0x5A01);
        assert_eq!(evs[3].kind, EvKind::FocusIn);
        assert_eq!(audit_switch(&evs, 0x5A02, 0x5A01), Ok(()));
        assert_eq!(c.current, 0x5A01);
        assert_eq!(c.serial, 4);
        assert_eq!(c.switches, 1);
    }

    #[test]
    fn chain_same_target_is_idempotent() {
        let mut c = FocusChain::new();
        let evs = c.focus(0x5A02).unwrap();
        assert!(evs.is_empty());
        assert_eq!(c.serial, 0);
        assert_eq!(c.switches, 0);
    }

    #[test]
    fn chain_rejects_unknown_and_nonfocusable() {
        let mut c = FocusChain::new();
        assert_eq!(c.focus(0x9A9A), Err(FocusCode::TargetBad.val()));
        assert_eq!(c.focus(0x5A03), Err(FocusCode::TargetBad.val()));
        assert_eq!(c.focus(ROOT), Err(FocusCode::TargetBad.val()));
        assert_eq!(c.current, 0x5A02);
    }

    #[test]
    fn history_is_capped_and_deduped() {
        let mut c = FocusChain::new();
        let seq = [0x5A01u32, 0x5A04, 0x5A06, 0x5A01, 0x5A02, 0x5A04];
        let mut i = 0usize;
        while i < seq.len() {
            c.focus(seq[i]).unwrap();
            assert!(c.order.len() <= HISTORY_CAP);
            assert_eq!(c.order[0], c.current);
            assert!(c.chain_ok());
            i += 1;
        }
        assert_eq!(c.order.len(), HISTORY_CAP);
    }

    #[test]
    fn audit_rejects_wrong_length() {
        let mut c = FocusChain::new();
        let evs = c.focus(0x5A01).unwrap();
        assert_eq!(audit_switch(&evs[0..3], 0x5A02, 0x5A01), Err(FocusCode::PairBad.val()));
        assert_eq!(audit_switch(&[], 0x5A02, 0x5A01), Err(FocusCode::PairBad.val()));
    }

    #[test]
    fn audit_rejects_swapped_order() {
        let mut c = FocusChain::new();
        let mut evs = c.focus(0x5A01).unwrap();
        evs.swap(0, 1);
        assert_eq!(audit_switch(&evs, 0x5A02, 0x5A01), Err(FocusCode::OrderBad.val()));
    }

    #[test]
    fn audit_rejects_wrong_anchor() {
        let mut c = FocusChain::new();
        let mut evs = c.focus(0x5A01).unwrap();
        evs[2].anchor = 0xDEAD_BEEF;
        assert_eq!(audit_switch(&evs, 0x5A02, 0x5A01), Err(FocusCode::CourtBad.val()));
    }

    #[test]
    fn audit_rejects_wrong_cause() {
        let mut c = FocusChain::new();
        let mut evs = c.focus(0x5A01).unwrap();
        evs[0].cause = Cause::Execute;
        assert_eq!(audit_switch(&evs, 0x5A02, 0x5A01), Err(FocusCode::CourtBad.val()));
    }

    #[test]
    fn audit_rejects_seq_gap() {
        let mut c = FocusChain::new();
        let mut evs = c.focus(0x5A01).unwrap();
        evs[3].seq += 5;
        assert_eq!(audit_switch(&evs, 0x5A02, 0x5A01), Err(FocusCode::OrderBad.val()));
    }

    #[test]
    fn pair_counts_two_per_switch() {
        let mut c = FocusChain::new();
        let evs = c.focus(0x5A04).unwrap();
        assert_eq!(pair_counts(&evs), (1, 1));
        assert_eq!(evs[0].cause.index(), 0);
        assert_eq!(evs[1].cause.index(), 1);
    }

    #[test]
    fn hundred_is_conserved() {
        let l = hundred_switches().unwrap();
        assert_eq!(l.transitions, HUNDRED);
        assert_eq!(l.events, 400);
        assert_eq!(l.paired, 200);
        assert_eq!(l.first_seq, 1);
        assert_eq!(l.last_seq, 400);
        assert!(l.conserved());
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn hundred_has_no_faults() {
        let l = hundred_switches().unwrap();
        assert_eq!(l.order_faults, 0);
        assert_eq!(l.court_faults, 0);
    }

    #[test]
    fn hundred_replay_is_stable() {
        let (a, b) = hundred_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn switch_injection_rejects_all() {
        let (rejected, accepted) = switch_faults().unwrap();
        assert_eq!(rejected, 10);
        assert_eq!(accepted, 0);
    }

    #[test]
    fn pair_injection_breaks_pairs() {
        let (broken, base) = pair_faults().unwrap();
        assert_eq!(broken, 10);
        assert_eq!(base, 2);
    }

    #[test]
    fn judgment_claim_is_refused() {
        assert_eq!(claim_judgment(), Err(FocusCode::CourtBad.val()));
        assert!(anchor_ok(BOUNDARY_ANCHOR));
        assert!(!anchor_ok(BOUNDARY_ANCHOR + 1));
        assert_eq!(JUDGE_OWNER, "vxwm/F1");
    }

    #[test]
    fn pointer_starts_on_desktop() {
        let ps = PointerStack::new();
        assert_eq!(ps.pointer_focus, ROOT);
        assert!(ps.stack.is_empty());
        assert_eq!(ps.top(), ROOT);
        assert!(ps.chain_valid());
        assert_ne!(ps.digest, 0);
    }

    #[test]
    fn pointer_enter_walks_the_chain() {
        let mut ps = PointerStack::new();
        let (entered, left) = ps.move_to(0x5A04).unwrap();
        assert_eq!((entered, left), (3, 0));
        assert_eq!(&ps.stack[..], &[0x5A01, 0x5A02, 0x5A04][..]);
        assert_eq!(ps.top(), 0x5A04);
    }

    #[test]
    fn pointer_sibling_move_leaves_then_enters() {
        let mut ps = PointerStack::new();
        ps.move_to(0x5A04).unwrap();
        let (entered, left) = ps.move_to(0x5A06).unwrap();
        assert_eq!((entered, left), (1, 2));
        assert_eq!(&ps.stack[..], &[0x5A01, 0x5A06][..]);
        assert!(ps.chain_valid());
    }

    #[test]
    fn pointer_desktop_leaves_everything() {
        let mut ps = PointerStack::new();
        ps.move_to(0x5A04).unwrap();
        let (entered, left) = ps.move_to(ROOT).unwrap();
        assert_eq!((entered, left), (0, 3));
        assert!(ps.stack.is_empty());
        assert_eq!(ps.enters, 3);
        assert_eq!(ps.leaves, 3);
    }

    #[test]
    fn pointer_unknown_target_rejected() {
        let mut ps = PointerStack::new();
        assert_eq!(ps.move_to(0x7777), Err(FocusCode::TargetBad.val()));
        assert!(ps.stack.is_empty());
    }

    #[test]
    fn hover_never_touches_keyboard() {
        let chain = FocusChain::new();
        let mut ps = PointerStack::new();
        let before = chain.current;
        ps.move_to(0x5A03).unwrap();
        ps.move_to(0x5A04).unwrap();
        ps.move_to(0x5A05).unwrap();
        assert_eq!(chain.current, before);
        assert_eq!(chain.serial, 0);
        assert_eq!(ps.pointer_focus, ROOT);
    }

    #[test]
    fn click_takes_focus_when_allowed() {
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        ps.move_to(0x5A04).unwrap();
        let out = ps.click(&mut chain, 0x5A04).unwrap();
        assert!(out.took_focus);
        assert_eq!(out.kb_from, 0x5A02);
        assert_eq!(out.kb_to, 0x5A04);
        assert_eq!(out.focus_events, 4);
        assert_eq!(ps.pointer_focus, 0x5A04);
    }

    #[test]
    fn click_respects_no_take_flag() {
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        ps.move_to(0x5A06).unwrap();
        let out = ps.click(&mut chain, 0x5A06).unwrap();
        assert!(!out.took_focus);
        assert_eq!(out.focus_events, 0);
        assert_eq!(chain.current, 0x5A02);
        assert_eq!(ps.pointer_focus, 0x5A06);
    }

    #[test]
    fn click_on_nonfocusable_keeps_keyboard() {
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        ps.move_to(0x5A03).unwrap();
        let out = ps.click(&mut chain, 0x5A03).unwrap();
        assert!(!out.took_focus);
        assert_eq!(chain.current, 0x5A02);
    }

    #[test]
    fn click_unknown_rejected() {
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        assert_eq!(ps.click(&mut chain, 0x8888), Err(FocusCode::TargetBad.val()));
    }

    #[test]
    fn desktop_click_is_inert() {
        let mut chain = FocusChain::new();
        let mut ps = PointerStack::new();
        let out = ps.click(&mut chain, ROOT).unwrap();
        assert!(!out.took_focus);
        assert_eq!(out.focus_events, 0);
        assert_eq!(ps.pointer_focus, ROOT);
        assert_eq!(ps.clicks, 1);
    }

    #[test]
    fn thousand_is_balanced() {
        let l = thousand_switches().unwrap();
        assert_eq!(l.steps, THOUSAND);
        assert!(l.balanced());
        assert_eq!(l.stack_faults, 0);
        assert_eq!(l.leaves + l.depth, l.enters);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn thousand_has_zero_steals() {
        let l = thousand_switches().unwrap();
        assert_eq!(l.hover_steals, 0);
        assert_eq!(l.kb_moves, l.took_focus);
    }

    #[test]
    fn thousand_replay_is_stable() {
        let (a, b) = thousand_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn separation_probe_shows_split() {
        let (kb0, kb1, kb2, took) = separation_probe().unwrap();
        assert_eq!(kb0, kb1);
        assert_eq!(kb2, 0x5A04);
        assert_eq!(took, 1);
    }

    #[test]
    fn hover_and_click_injections_agree() {
        let (held, stolen) = hover_inject().unwrap();
        assert_eq!((held, stolen), (10, 0));
        let (agreed, bad) = click_matrix().unwrap();
        assert_eq!((agreed, bad), (10, 0));
    }

    #[test]
    fn case_ids_do_not_collide() {
        assert!(no_cross_collision());
        assert!(case_ids_sequential());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0733_0001);
        assert_eq!(CASE_IDS[15], 0x0733_0010);
        assert_eq!(CASES.len(), CASE_IDS.len());
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].id, CASE_IDS[i]);
            assert!(!CASES[i].name.is_empty());
            i += 1;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            assert_ne!(RESERVED_IDS[j], CASE_IDS[0]);
            assert_ne!(RESERVED_IDS[j], CASE_IDS[15]);
            j += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(KFOCUS_TAG, 100, 400, 200, 400, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).unwrap();
        assert_eq!(tag, KFOCUS_TAG);
        assert_eq!(vals, [100, 400, 200, 400]);
        assert_eq!(digest, 0x0123_4567_89AB_CDEF);
        assert!(!audit_render("KFOCUS|k1=1"));
        assert!(!audit_render(""));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFF"), None);
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFG"), None);
    }

    #[test]
    fn rows_are_parseable() {
        let l = hundred_switches().unwrap();
        let krow = kfocus_row(&l);
        let (t, v, d) = row_fields(&krow).unwrap();
        assert_eq!(t, KFOCUS_TAG);
        assert_eq!(v[0], 100);
        assert_eq!(v[1], 400);
        assert_eq!(v[2], 200);
        assert_eq!(d, l.digest);
        let p = thousand_switches().unwrap();
        let prow = ptrfoc_row(&p);
        let (t2, v2, d2) = row_fields(&prow).unwrap();
        assert_eq!(t2, PTRFOC_TAG);
        assert_eq!(v2[0], 1000);
        assert_eq!(d2, p.digest);
        let wrow = win_row(&WINDOWS[2]);
        let (t3, v3, d3) = row_fields(&wrow).unwrap();
        assert_eq!(t3, WIN_TAG);
        assert_eq!(v3[0], 0x5A03);
        assert_eq!(v3[3], 0);
        assert_ne!(d3, 0);
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
        let s = run_unx49041_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
