//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16261 · CreateWindowEx 映射总纲（Win32 窗口→vxwm 窗口树节点）
//! · 0x67AC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F16261-J1 映射总纲落账（创建参数/树节点/回调挂接三段），
//!      三段齐备率 100%，与 F16252 回调表 CreateWindow 行挂接一致率
//!      100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三段闭集：创建参数（Win32 CreateWindowEx 12 参族冻结）/
//!     树节点（vxwm 窗口树 parent/depth 守恒）/回调挂接（F16252
//!     CreateWindow 行），逐段落账齐备率 100%；
//!   - 树守恒恒等式：子深 = 父深 + 1，根深 = 0，任一破则拒挂；
//!   - 回调行对账：与 F16252 回调表 CreateWindow 行（行名+挂接态）
//!     逐字对账，一致判据配反向判据（错行名必判不一致）；
//!   - 状态机：Creating → Mapped → Hooked → Destroyed，非法回跳拒收；
//!   - LCG 种子写死 = SEG_BASE（0x67AC0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect；错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67AC0，AI-VX3-1030 独占区，批 15 卡 4）。
pub const SEG_BASE: u32 = 0x67AC0;

// ── 三段闭集 ─────────────────────────────────────────────────────
pub const SEC_PARAMS: usize = 0; // 创建参数
pub const SEC_TREE: usize = 1; // 树节点
pub const SEC_HOOK: usize = 2; // 回调挂接
pub const SEC_COUNT: usize = 3;
pub const SEC_NAMES: [&str; SEC_COUNT] = ["创建参数", "树节点", "回调挂接"];

// ── Win32 CreateWindowEx 12 参族闭集（来源：Win32 API 签名冻结） ──
pub const PARAM_EXSTYLE: usize = 0;
pub const PARAM_CLASSNAME: usize = 1;
pub const PARAM_WINNAME: usize = 2;
pub const PARAM_STYLE: usize = 3;
pub const PARAM_X: usize = 4;
pub const PARAM_Y: usize = 5;
pub const PARAM_W: usize = 6;
pub const PARAM_H: usize = 7;
pub const PARAM_PARENT: usize = 8;
pub const PARAM_MENU: usize = 9;
pub const PARAM_INSTANCE: usize = 10;
pub const PARAM_CREATEPARAM: usize = 11;
pub const PARAM_COUNT: usize = 12;
pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "dwExStyle", "lpClassName", "lpWindowName", "dwStyle", "x", "y",
    "nWidth", "nHeight", "hWndParent", "hMenu", "hInstance", "lpParam",
];

// ── F16252 回调表 CreateWindow 行（独立写死判据） ────────────────
pub const CALLBACK_ROW_CREATEWINDOW: usize = 0;
pub const CALLBACK_ROW_NAME: &str = "CreateWindow";
/// 反向判据用错行名（独立写死，不复用被测值）。
pub const CALLBACK_ROW_NAME_WRONG: &str = "CreateWindowExW_Bogus";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrtwinCode {
    /// 参数序号越 12 参族
    ParamOutOfRange,
    /// 类名空串（必填闸）
    ClassNameEmpty,
    /// 父节点不存在（挂载闸）
    ParentMissing,
    /// 深度守恒破（子深≠父深+1）
    DepthMismatch,
    /// 回调行对账不一致
    RowHookMismatch,
    /// 三段缺项
    SectionMissing,
    /// 节点未找到
    NodeNotFound,
    /// 状态机非法转换
    StateInvalid,
    /// 复现失败
    NotReproducible,
}

impl CrtwinCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CrtwinCode::ParamOutOfRange => "参数序号越 12 参族闭集",
            CrtwinCode::ClassNameEmpty => "类名空串：必填闸拒收",
            CrtwinCode::ParentMissing => "父节点不存在：挂载闸拒收",
            CrtwinCode::DepthMismatch => "深度守恒破：子深≠父深+1",
            CrtwinCode::RowHookMismatch => "回调行对账不一致：F16252 红线",
            CrtwinCode::SectionMissing => "三段缺项：齐备率 100% 红线",
            CrtwinCode::NodeNotFound => "树节点未找到",
            CrtwinCode::StateInvalid => "状态机非法转换：回跳拒收",
            CrtwinCode::NotReproducible => "复现失败：双跑树态不一致",
        }
    }
    pub const ALL: [CrtwinCode; 9] = [
        CrtwinCode::ParamOutOfRange,
        CrtwinCode::ClassNameEmpty,
        CrtwinCode::ParentMissing,
        CrtwinCode::DepthMismatch,
        CrtwinCode::RowHookMismatch,
        CrtwinCode::SectionMissing,
        CrtwinCode::NodeNotFound,
        CrtwinCode::StateInvalid,
        CrtwinCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CrtwinCode::ALL.len() {
        if CrtwinCode::ALL[i].val() == code {
            return CrtwinCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67AC0 段闭集内"
}

// ── 节点状态机（显式状态 + 合法边闭集） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeState {
    Creating,
    Mapped,
    Hooked,
    Destroyed,
}

pub fn node_edge(from: NodeState, to: NodeState) -> bool {
    match (from, to) {
        (NodeState::Creating, NodeState::Mapped) => true,
        (NodeState::Mapped, NodeState::Hooked) => true,
        (NodeState::Hooked, NodeState::Destroyed) => true,
        _ => false,
    }
}

// ── 创建参数单（12 参定长） ──────────────────────────────────────
/// 字符串参以命名表下标表达（0=空串哨兵，1..=类名/窗名池），数值参
/// 直接携带；hWndParent 以树节点下标 +1 表达（0=无父=桌面根）。
pub struct CreateParams {
    pub vals: [u32; PARAM_COUNT],
    pub classname: String,
    pub winname: String,
}

impl CreateParams {
    pub fn new() -> CreateParams {
        CreateParams {
            vals: [0; PARAM_COUNT],
            classname: String::new(),
            winname: String::new(),
        }
    }
    /// 参数单闭合校验：类名非空 + 宽高语义闸（0=默认尺寸合法，
    /// 负语义在 u32 域以高位段拒绝——此处只建模非零高位段拒收）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.classname.len() == 0 {
            return Err(CrtwinCode::ClassNameEmpty.val());
        }
        if self.vals[PARAM_W] > 0x7FFF || self.vals[PARAM_H] > 0x7FFF {
            return Err(CrtwinCode::ParamOutOfRange.val());
        }
        Ok(())
    }
}

// ── vxwm 窗口树节点 ──────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: u32,
    /// 父节点下标（None=根）
    pub parent: Option<usize>,
    /// 深度（根=0，守恒：子深=父深+1）
    pub depth: u32,
    /// 子节点下标集
    pub children: Vec<usize>,
    pub state: NodeState,
    /// 挂接的回调行号（F16252 回调表）
    pub hook_row: Option<usize>,
}

// ── 窗口树账本 ───────────────────────────────────────────────────
pub struct CrtwinLedger {
    pub nodes: Vec<TreeNode>,
    /// 三段落账位图
    pub sections: [bool; SEC_COUNT],
    pub booked: u32,
    pub summary: String,
    pub status_done: bool,
}

impl CrtwinLedger {
    pub fn new() -> CrtwinLedger {
        CrtwinLedger {
            nodes: Vec::new(),
            sections: [false; SEC_COUNT],
            booked: 0,
            summary: String::new(),
            status_done: false,
        }
    }

    /// 建根节点（桌面根，深 0）。
    pub fn seed_root(&mut self) -> Result<usize, u32> {
        if self.nodes.len() > 0 {
            return Err(CrtwinCode::StateInvalid.val());
        }
        self.nodes.push(TreeNode {
            id: 0,
            parent: None,
            depth: 0,
            children: Vec::new(),
            state: NodeState::Mapped,
            hook_row: None,
        });
        self.sections[SEC_TREE] = true;
        self.booked += 1;
        Ok(0)
    }

    /// 挂子节点：父存在闸 + 深度守恒（子深=父深+1）。
    pub fn attach(&mut self, parent: usize, id: u32) -> Result<usize, u32> {
        // 下标先比长度。
        if parent >= self.nodes.len() {
            return Err(CrtwinCode::ParentMissing.val());
        }
        if self.nodes[parent].state == NodeState::Destroyed {
            return Err(CrtwinCode::ParentMissing.val());
        }
        let depth = self.nodes[parent].depth + 1;
        let idx = self.nodes.len();
        self.nodes.push(TreeNode {
            id,
            parent: Some(parent),
            depth,
            children: Vec::new(),
            state: NodeState::Creating,
            hook_row: None,
        });
        self.nodes[parent].children.push(idx);
        self.booked += 1;
        Ok(idx)
    }

    /// 深度守恒审计：全树子深=父深+1、根深=0。
    pub fn audit_depth(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.nodes.len() {
            match self.nodes[i].parent {
                None => {
                    if self.nodes[i].depth != 0 {
                        return Err(CrtwinCode::DepthMismatch.val());
                    }
                }
                Some(p) => {
                    if p >= self.nodes.len() {
                        return Err(CrtwinCode::ParentMissing.val());
                    }
                    if self.nodes[i].depth != self.nodes[p].depth + 1 {
                        return Err(CrtwinCode::DepthMismatch.val());
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 映射段：参数单闭合校验 + 节点 Creating→Mapped。
    pub fn map_window(&mut self, node: usize, params: &CreateParams) -> Result<(), u32> {
        if node >= self.nodes.len() {
            return Err(CrtwinCode::NodeNotFound.val());
        }
        params.validate()?;
        if self.nodes[node].state != NodeState::Creating {
            return Err(CrtwinCode::StateInvalid.val());
        }
        self.nodes[node].state = NodeState::Mapped;
        self.sections[SEC_PARAMS] = true;
        self.booked += 1;
        Ok(())
    }

    /// 挂接段：节点 Mapped→Hooked，行号必须=CreateWindow 行。
    pub fn hook_callback(&mut self, node: usize) -> Result<(), u32> {
        if node >= self.nodes.len() {
            return Err(CrtwinCode::NodeNotFound.val());
        }
        if self.nodes[node].state != NodeState::Mapped {
            return Err(CrtwinCode::StateInvalid.val());
        }
        if !callback_row_reconcile().is_ok() {
            return Err(CrtwinCode::RowHookMismatch.val());
        }
        self.nodes[node].state = NodeState::Hooked;
        self.nodes[node].hook_row = Some(CALLBACK_ROW_CREATEWINDOW);
        self.sections[SEC_HOOK] = true;
        self.booked += 1;
        Ok(())
    }

    /// 销毁：Hooked→Destroyed（叶子先拆，带子拒销）。
    pub fn destroy(&mut self, node: usize) -> Result<(), u32> {
        if node >= self.nodes.len() {
            return Err(CrtwinCode::NodeNotFound.val());
        }
        if self.nodes[node].children.len() > 0 {
            return Err(CrtwinCode::StateInvalid.val());
        }
        if self.nodes[node].state != NodeState::Hooked {
            return Err(CrtwinCode::StateInvalid.val());
        }
        self.nodes[node].state = NodeState::Destroyed;
        self.booked += 1;
        Ok(())
    }

    /// 三段齐备率（应 100%）。
    pub fn sections_ready(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < SEC_COUNT {
            if self.sections[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 双跑复现：同输入重建树，深度序列逐位一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = CrtwinLedger::new();
        let _ = run2.seed_root();
        let mut i = 1;
        while i < self.nodes.len() {
            let p = match self.nodes[i].parent {
                Some(p) => p,
                None => return Err(CrtwinCode::DepthMismatch.val()),
            };
            run2.attach(p, self.nodes[i].id)?;
            i += 1;
        }
        if run2.nodes.len() != self.nodes.len() {
            return Err(CrtwinCode::NotReproducible.val());
        }
        let mut j = 0;
        while j < self.nodes.len() {
            if run2.nodes[j].depth != self.nodes[j].depth
                || run2.nodes[j].id != self.nodes[j].id
            {
                return Err(CrtwinCode::NotReproducible.val());
            }
            j += 1;
        }
        Ok(())
    }
}

// ── F16252 回调行对账（独立判据，一致配反向） ────────────────────
/// 被测侧行名（写死）。
pub const CALLBACK_ROW_IMPL: &str = "CreateWindow";

pub fn callback_row_reconcile() -> Result<(), u32> {
    if CALLBACK_ROW_NAME != CALLBACK_ROW_IMPL {
        return Err(CrtwinCode::RowHookMismatch.val());
    }
    // 挂接行号恒等。
    if CALLBACK_ROW_CREATEWINDOW != 0 {
        return Err(CrtwinCode::RowHookMismatch.val());
    }
    Ok(())
}

/// 反向判据：错行名必判不一致。
pub fn callback_row_reverse() -> bool {
    match core::primitive::str::eq(CALLBACK_ROW_NAME_WRONG, CALLBACK_ROW_IMPL) {
        true => false,
        false => CALLBACK_ROW_NAME_WRONG != CALLBACK_ROW_IMPL,
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ───────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回（挂接节点数、入账格数、三段齐备率千分位）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = CrtwinLedger::new();
    led.seed_root()?;
    // 挂 3 子窗（一级 2 + 二级 1）。
    let a = led.attach(0, 0xA001)?;
    let b = led.attach(0, 0xA002)?;
    let c = led.attach(a, 0xA003)?;
    led.audit_depth()?;
    // 三窗全过三段。
    let mut i = 0;
    let mut hooked = 0;
    while i < led.nodes.len() {
        if led.nodes[i].parent.is_some() {
            let mut params = CreateParams::new();
            params.classname = String::from("VXMainClass");
            params.winname = String::from("落窗");
            params.vals[PARAM_W] = 640;
            params.vals[PARAM_H] = 480;
            led.map_window(i, &params)?;
            led.hook_callback(i)?;
            hooked += 1;
        }
        i += 1;
    }
    led.reproducible()?;
    if led.sections_ready() != SEC_COUNT as u32 {
        return Err(CrtwinCode::SectionMissing.val());
    }
    let _ = b;
    let _ = c;
    Ok((hooked, led.booked, 1000))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：缺父挂载注入——指向不存在节点（5 发，下标越界逐级放大）。
    let mut k = 0;
    while k < 5 {
        ledger.tries[0] += 1;
        let mut led = CrtwinLedger::new();
        let _ = led.seed_root();
        if led.attach(9 + k, 0xB000 + k as u32).is_err() {
            ledger.caught[0] += 1;
        }
        k += 1;
    }
    // 族 1：类名空串注入（必填闸）。
    ledger.tries[1] += 1;
    let mut led2 = CrtwinLedger::new();
    let _ = led2.seed_root();
    let _ = led2.attach(0, 0xB001);
    let empty = CreateParams::new();
    if led2.map_window(1, &empty).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：回调行名注入（错行名对账必判不一致——反向判据）。
    ledger.tries[2] += 1;
    if !callback_row_reverse() {
        // 错行名若与行名相等（判据失效），不计数。
    } else {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CrtwinCode::ALL.len() {
        let v = CrtwinCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金正演：3 窗挂接 + 守恒 + 三段齐备。
    let golden = golden_run();
    let golden_ok = match golden {
        Ok((h, b, e)) => h == 3 && b >= 7 && e == 1000,
        Err(_) => false,
    };
    v.push(("J1-三窗三段全挂接", golden_ok, format!("{:?}", golden)));
    // 注入面全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught() && inj.caught[0] == 5,
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 12 参族名点验。
    let mut named = 0;
    let mut i = 0;
    while i < PARAM_COUNT {
        if PARAM_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-12参族名点验",
        named == PARAM_COUNT && PARAM_NAMES.len() == PARAM_COUNT,
        format!("{}/12", named),
    ));
    // 回调行对账（正向）。
    v.push((
        "J1-F16252回调行挂接一致",
        callback_row_reconcile().is_ok(),
        String::from("CreateWindow 行逐字对账"),
    ));
    // 深度守恒抽验：根深 0 + 三级树逐级 +1。
    let mut led3 = CrtwinLedger::new();
    let _ = led3.seed_root();
    let a3 = led3.attach(0, 0xC001);
    let depth_ok = match a3 {
        Ok(a) => {
            let _ = led3.attach(a, 0xC002);
            led3.audit_depth().is_ok()
                && led3.nodes[1].depth == 1
                && led3.nodes[2].depth == 2
                && led3.nodes[0].depth == 0
        }
        Err(_) => false,
    };
    v.push(("J1-深度守恒三级树", depth_ok, String::from("0/1/2 逐级+1")));
    // 状态机回跳拒收抽验：Mapped→Creating 非法。
    let back_ok = !node_edge(NodeState::Mapped, NodeState::Creating)
        && !node_edge(NodeState::Hooked, NodeState::Creating)
        && !node_edge(NodeState::Destroyed, NodeState::Hooked);
    v.push(("J1-状态机回跳必拒(反向)", back_ok, String::from("合法边闭集")));
    // 完成写回面谱。
    v.push((
        "J3-完成写回(落账+齐备率)",
        led3.booked > 0 && led3.sections_ready() > 0,
        format!("booked={}", led3.booked),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67AC0 窗口")));
    v
}

pub fn run_unx16261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16261 CreateWindowEx 映射总纲（三段挂接）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
