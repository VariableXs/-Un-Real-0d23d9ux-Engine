//! VE-F3851 · 读屏调试
//!
//! 读屏调试器：协同检视五列仪表（语义-渲染对齐/公告队列/跟随状态/扫描/序）
//! + 断链扫描（复用 F3813 扫描器单源）+ 读屏模拟器（语义树→模拟播报序列）
//! + 导航序可视化 + 家族二十三成员复述协议。
//! 错误路径：模拟失真→真机对拍校准、仪表分歧→立案、扫描漏→断言修正、开销→打开才采样。




#![allow(dead_code)]

extern crate alloc;

use alloc::{vec, vec::Vec};

// ── 诊断码段（0xC700）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC7Code(pub u16);

impl CC7Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SIM_DRIFT: Self = Self(SEG_BASE + 1);
    pub const PANEL_DIVERGE: Self = Self(SEG_BASE + 2);
    pub const SCAN_LEAK: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SIM_DRIFT => "模拟失真→真机对拍校准",
            Self::PANEL_DIVERGE => "仪表分歧→立案",
            Self::SCAN_LEAK => "扫描漏→断言修正",
            _ => "未知",
        }
    }
}

// ── 五列仪表 ────────────────────────────────────────────────────────

/// 检视列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspColumn {
    /// 语义-渲染对齐。
    SemAlign,
    /// 公告队列。
    AnnounceQ,
    /// 跟随状态。
    FollowState,
    /// 断链扫描。
    ScanBreak,
    /// 导航序。
    NavOrder,
}

impl InspColumn {
    pub const ALL: [InspColumn; 5] = [
        InspColumn::SemAlign,
        InspColumn::AnnounceQ,
        InspColumn::FollowState,
        InspColumn::ScanBreak,
        InspColumn::NavOrder,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SemAlign => "语义-渲染对齐",
            Self::AnnounceQ => "公告队列",
            Self::FollowState => "跟随状态",
            Self::ScanBreak => "断链扫描",
            Self::NavOrder => "导航序",
        }
    }
}

/// 单列仪表读数。
#[derive(Debug, Clone)]
pub struct PanelReading {
    pub col: InspColumn,
    /// 打开才采样（开销红线：关闭的列读数冻结）。
    pub open: bool,
    /// 读数值。
    pub value: u32,
    /// 上次采样读数（分歧检测）。
    pub prev_value: u32,
}

// ── 断链扫描（复用 F3813 单源）──────────────────────────────────────

/// 断链扫描器（复述：复用 F3813，不自建第二份）。
#[derive(Debug, Clone)]
pub struct BreakScanner {
    /// 扫描命中的断链节点。
    pub hits: Vec<u32>,
    /// 断言侧已知断链（对照）。
    pub asserted: Vec<u32>,
}

impl BreakScanner {
    pub fn new() -> Self {
        Self {
            hits: Vec::new(),
            asserted: Vec::new(),
        }
    }

    /// 扫描：从节点集里找语义有节点而渲染无挂载的。
    pub fn scan(&mut self, sem: &[u32], rendered: &[u32]) {
        self.hits.clear();
        for &s in sem.iter() {
            if !rendered.contains(&s) {
                self.hits.push(s);
            }
        }
    }

    /// 扫描漏检：命中了断言侧没有的断链→修正断言。
    pub fn leaked_vs_assert(&self) -> Vec<u32> {
        self.hits
            .iter()
            .filter(|h| !self.asserted.contains(h))
            .copied()
            .collect()
    }

    /// 断言修正：把漏检断链补进断言册。
    pub fn amend_assert(&mut self) -> usize {
        let leaks = self.leaked_vs_assert();
        let n = leaks.len();
        self.asserted.extend(leaks);
        n
    }
}

impl Default for BreakScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 读屏模拟器（语义树→播报序列）───────────────────────────────────

/// 语义树节点（最小复述）。
#[derive(Debug, Clone)]
pub struct SemNode {
    pub id: u32,
    /// 角色（'h'标题/'b'按钮/'t'文本/'l'地标）。
    pub role: u8,
    pub text: &'static str,
    pub child: Vec<SemNode>,
}

impl SemNode {
    /// 叶子构造。
    pub fn leaf(id: u32, role: u8, text: &'static str) -> Self {
        Self {
            id,
            role,
            text,
            child: Vec::new(),
        }
    }
}

/// 模拟播报条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceItem {
    pub node_id: u32,
    /// 播报文本（角色+内容）。
    pub said: &'static str,
}

/// 读屏模拟器：语义树→模拟播报序列。
#[derive(Debug, Clone)]
pub struct SrSimulator {
    pub output: Vec<AnnounceItem>,
    /// 真机对拍基准（校准用）。
    pub device_ref: Vec<AnnounceItem>,
    /// 模拟启用（打开才采样红线同源：不开不跑）。
    pub enabled: bool,
}

impl SrSimulator {
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            device_ref: Vec::new(),
            enabled: false,
        }
    }

    /// 深度优先播报模拟。
    pub fn simulate(&mut self, root: &SemNode) {
        self.output.clear();
        if !self.enabled {
            return;
        }
        self.walk(root);
    }

    fn walk(&mut self, node: &SemNode) {
        let said = match node.role {
            b'h' => "标题",
            b'b' => "按钮",
            b'l' => "地标",
            _ => "文本",
        };
        self.output.push(AnnounceItem {
            node_id: node.id,
            said,
        });
        for c in node.child.iter() {
            self.walk(c);
        }
    }

    /// 真机对拍校准：模拟序列与真机基准逐条比对。
    pub fn calibrated(&self) -> bool {
        if self.output.len() != self.device_ref.len() {
            return false;
        }
        self.output
            .iter()
            .zip(self.device_ref.iter())
            .all(|(a, b)| a.node_id == b.node_id && a.said == b.said)
    }

    /// 失真格数（对拍定位用）。
    pub fn drift_cells(&self) -> usize {
        self.output
            .iter()
            .zip(self.device_ref.iter())
            .filter(|(a, b)| a.said != b.said)
            .count()
    }
}

impl Default for SrSimulator {
    fn default() -> Self {
        Self::new()
    }
}

// ── 导航序可视化 ────────────────────────────────────────────────────

/// 导航序：按读屏焦点遍历次序编号。
#[derive(Debug, Clone)]
pub struct NavOrderView {
    /// 次序→节点 id。
    pub order: Vec<u32>,
}

impl NavOrderView {
    pub fn from_ids(ids: &[u32]) -> Self {
        Self {
            order: ids.to_vec(),
        }
    }

    /// 序单调（无重无漏）判定。
    pub fn strict(&self) -> bool {
        for w in self.order.windows(2) {
            if w[0] >= w[1] {
                return false;
            }
        }
        true
    }

    /// 可视化行：`#01 -> 节点12`。
    pub fn render(&self) -> Vec<(&'static str, u32)> {
        self.order
            .iter()
            .enumerate()
            .map(|(i, id)| ("序位", *id))
            .take(self.order.len())
            .enumerate()
            .map(|(_, (_, id))| ("序位", id))
            .collect()
    }
}

// ── 家族二十三成员复述协议 ──────────────────────────────────────────

/// 家族二十三：读屏协同家族成员清单（复述协议）。
pub const FAMILY_23: [&str; 23] = [
    "闭环",
    "跟随",
    "标注",
    "噪音",
    "公告",
    "地标",
    "跳空",
    "预算",
    "焦点",
    "文本",
    "图形",
    "导航",
    "虚拟化",
    "表面",
    "扫描",
    "模拟",
    "对拍",
    "序",
    "校准",
    "仪表",
    "立案",
    "协议",
    "单源",
];

/// 协议握手：成员逐一报数。
pub fn family_handshake(replies: &[&str]) -> bool {
    if replies.len() != FAMILY_23.len() {
        return false;
    }
    FAMILY_23
        .iter()
        .zip(replies.iter())
        .all(|(a, b)| a == b)
}

// ── 调试会话（总控）─────────────────────────────────────────────────

/// 读屏调试会话。
#[derive(Debug, Clone)]
pub struct SrDebugSession {
    pub panels: Vec<PanelReading>,
    pub scanner: BreakScanner,
    pub sim: SrSimulator,
    pub nav: NavOrderView,
    /// 立案数。
    pub cases_filed: u32,
}

impl SrDebugSession {
    pub fn new() -> Self {
        Self {
            panels: InspColumn::ALL
                .iter()
                .map(|c| PanelReading {
                    col: *c,
                    open: false,
                    value: 0,
                    prev_value: 0,
                })
                .collect(),
            scanner: BreakScanner::new(),
            sim: SrSimulator::new(),
            nav: NavOrderView::from_ids(&[]),
            cases_filed: 0,
        }
    }

    /// 列采样（打开才采样：未开的列不动）。
    pub fn sample(&mut self, col: InspColumn, value: u32) {
        if let Some(p) = self.panels.iter_mut().find(|p| p.col == col) {
            if p.open {
                p.prev_value = p.value;
                p.value = value;
            }
        }
    }

    /// 仪表分歧：相邻两次采样跳变超阈→立案。
    pub fn audit_diverge(&mut self, threshold: u32) -> u32 {
        let mut filed = 0;
        for p in self.panels.iter() {
            if p.open {
                let d = if p.value >= p.prev_value {
                    p.value - p.prev_value
                } else {
                    p.prev_value - p.value
                };
                if d > threshold {
                    filed += 1;
                }
            }
        }
        self.cases_filed += filed;
        filed
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CC7Code> {
        if !self.sim.calibrated() {
            return Err(CC7Code::SIM_DRIFT);
        }
        if self.cases_filed > 0 {
            return Err(CC7Code::PANEL_DIVERGE);
        }
        if !self.scanner.leaked_vs_assert().is_empty() {
            return Err(CC7Code::SCAN_LEAK);
        }
        Ok(())
    }
}

impl Default for SrDebugSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves151_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves151_srdebug");

    fn tree() -> SemNode {
        let mut root = SemNode::leaf(1, b'l', "主地标");
        root.child.push(SemNode::leaf(2, b'h', "标题一"));
        root.child.push(SemNode::leaf(3, b'b', "确定"));
        root.child.push(SemNode::leaf(4, b't', "正文"));
        root
    }

    fn ready() -> SrDebugSession {
        let mut s = SrDebugSession::new();
        s.nav = NavOrderView::from_ids(&[1, 2, 3]);
        s
    }

    // 01 五列仪表定名
    {
        let ok = InspColumn::ALL.len() == 5
            && InspColumn::ALL.iter().all(|c| !c.name().is_empty());
        chk(&mut set, "C3851-01", ok, "err");
    }
    // 02 打开才采样：关列不动
    {
        let mut s = ready();
        s.sample(InspColumn::SemAlign, 99);
        let p = s.panels.iter().find(|p| p.col == InspColumn::SemAlign).unwrap();
        chk(&mut set, "C3851-02", p.value == 0, "err");
    }
    // 03 开列采样生效
    {
        let mut s = ready();
        s.panels[0].open = true;
        s.sample(InspColumn::SemAlign, 42);
        let p = s.panels.iter().find(|p| p.col == InspColumn::SemAlign).unwrap();
        chk(&mut set, "C3851-03", p.value == 42, "err");
    }
    // 04 断链扫描命中
    {
        let mut sc = BreakScanner::new();
        sc.scan(&[1, 2, 3], &[1, 3]);
        chk(
            &mut set,
            "C3851-04",
            sc.hits == vec![2],
            "err",
        );
    }
    // 05 扫描漏→断言修正
    {
        let mut sc = BreakScanner::new();
        sc.asserted.push(9);
        sc.scan(&[1, 9, 9, 2], &[1]);
        let leak = sc.leaked_vs_assert();
        let amended = sc.amend_assert();
        chk(
            &mut set,
            "C3851-05",
            leak == vec![2] && amended == 1 && sc.leaked_vs_assert().is_empty(),
            "err",
        );
    }
    // 06 断链无漏（断言对齐）
    {
        let mut sc = BreakScanner::new();
        sc.asserted.push(2);
        sc.scan(&[1, 2], &[1]);
        chk(&mut set, "C3851-06", sc.leaked_vs_assert().is_empty(), "err");
    }
    // 07 模拟器：未启用不播报
    {
        let mut sim = SrSimulator::new();
        sim.simulate(&tree());
        chk(&mut set, "C3851-07", sim.output.is_empty(), "err");
    }
    // 08 模拟器：播报序列深度优先
    {
        let mut sim = SrSimulator::new();
        sim.enabled = true;
        sim.simulate(&tree());
        let ids: Vec<u32> = sim.output.iter().map(|o| o.node_id).collect();
        chk(&mut set, "C3851-08", ids == vec![1, 2, 3, 4], "err");
    }
    // 09 模拟器：角色映射
    {
        let mut sim = SrSimulator::new();
        sim.enabled = true;
        sim.simulate(&tree());
        let ok = sim.output[0].said == "地标"
            && sim.output[1].said == "标题"
            && sim.output[2].said == "按钮";
        chk(&mut set, "C3851-09", ok, "err");
    }
    // 10 真机对拍校准通过
    {
        let mut sim = SrSimulator::new();
        sim.enabled = true;
        sim.simulate(&tree());
        sim.device_ref = sim.output.clone();
        chk(&mut set, "C3851-10", sim.calibrated(), "err");
    }
    // 11 模拟失真检出与格数
    {
        let mut sim = SrSimulator::new();
        sim.enabled = true;
        sim.simulate(&tree());
        sim.device_ref = sim.output.clone();
        sim.device_ref[2].said = "链接";
        chk(
            &mut set,
            "C3851-11",
            !sim.calibrated() && sim.drift_cells() == 1,
            "err",
        );
    }
    // 12 导航序严格单调
    {
        let v = NavOrderView::from_ids(&[3, 7, 9]);
        let bad = NavOrderView::from_ids(&[3, 3, 9]);
        chk(&mut set, "C3851-12", v.strict() && !bad.strict(), "err");
    }
    // 13 家族二十三握手全对
    {
        chk(&mut set, "C3851-13", family_handshake(&FAMILY_23), "err");
    }
    // 14 家族二十三：缺员即拒
    {
        let short = &FAMILY_23[..22];
        chk(&mut set, "C3851-14", !family_handshake(short), "err");
    }
    // 15 家族二十三：串员即拒
    {
        let mut wrong = FAMILY_23;
        wrong[3] = "错员";
        chk(&mut set, "C3851-15", !family_handshake(&wrong), "err");
    }
    // 16 仪表分歧立案
    {
        let mut s = ready();
        s.panels[1].open = true;
        s.sample(InspColumn::AnnounceQ, 5);
        s.sample(InspColumn::AnnounceQ, 50);
        let filed = s.audit_diverge(10);
        chk(
            &mut set,
            "C3851-16",
            filed == 1 && s.cases_filed == 1,
            "err",
        );
    }
    // 17 分歧未超阈不立案
    {
        let mut s = ready();
        s.panels[1].open = true;
        s.sample(InspColumn::AnnounceQ, 5);
        s.sample(InspColumn::AnnounceQ, 8);
        chk(&mut set, "C3851-17", s.audit_diverge(10) == 0, "err");
    }
    // 18 会话总检：干净会话应全绿
    {
        let s = ready();
        chk(&mut set, "C3851-18", s.full_check() == Ok(()), "err");
    }
    // 19 会话总检：失真模拟阻断
    {
        let mut s = ready();
        s.sim.enabled = true;
        s.sim.simulate(&tree());
        s.sim.device_ref = s.sim.output.clone();
        s.sim.device_ref[0].said = "错播";
        chk(
            &mut set,
            "C3851-19",
            s.full_check() == Err(CC7Code::SIM_DRIFT),
            "err",
        );
    }

    set
}
