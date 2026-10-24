//! VE-F3748 · 创作辅助技术
//!
//! 锚点：创作辅助技术（辅具×创作工具深度适配（F3149 模式创作版复刻：读屏创作
//! （创作界面语义树完备（复述投影；开关控制创作（扫描节奏创作适配（扫描参数
//! 创作级可调——创作扫描参数红线：默认扫描节奏不适合创作密度（参数适配断言；
//! 眼动创作（注视绘制（眼动画笔（眼动画笔精度断言（创作级精度（绘制精度红线：
//! 眼动画笔精度不足=无法创作（精度线（创作眼动精度≥指针 80%——精度量化声明；
//! 语音创作（语音命令词表（创作命令词表公开。
//!
//! 数据结构：语义复用；扫描参数（适配断言）；眼动画笔（80% 断言）；词表公开。
//! 错误路径与降级矩阵：精度不足→优化+量化立案（红线实测——精度计分）；参数不适配→
//! 断言（红线实测）；语义缺→投影修正（复述）；词表私藏→公开（复述红线）。
//! 跨批对接点：F3149/F3745 单源复用声明；辅具厂商（对端）；R01-R03 对端。
//!
//! 诊断码段：独占 0x8Bxx。

#![allow(dead_code)]

use alloc::string::String;
extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Bxx）
// ─────────────────────────────────────────────────────────────

/// F3748 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T7Code(pub u16);

impl T7Code {
    /// 眼动画笔精度低于 80%——绘制精度红线。
    pub const EYE_PRECISION_LOW: T7Code = T7Code(0x8B01);
    /// 扫描参数不适配——创作密度断言。
    pub const SCAN_NOT_FIT: T7Code = T7Code(0x8B02);
    /// 语义树缺投影——读屏创作。
    pub const SEMTREE_GAP: T7Code = T7Code(0x8B03);
    /// 词表私藏——公开红线。
    pub const WORDLIST_PRIVATE: T7Code = T7Code(0x8B04);
    /// 精度计分缺失——量化立案。
    pub const SCORE_MISSING: T7Code = T7Code(0x8B05);
    /// 扫描节奏越界——参数范围。
    pub const SCAN_OOB: T7Code = T7Code(0x8B06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8B01 => "眼动画笔精度低于指针 80%，绘制精度红线不通过。",
            0x8B02 => "默认扫描节奏未适配创作密度，参数适配断言不通过。",
            0x8B03 => "创作界面语义树缺投影，读屏创作不可用。",
            0x8B04 => "创作命令词表私藏，公开红线不通过。",
            0x8B05 => "精度计分缺失，无法量化立案。",
            0x8B06 => "扫描参数越界，范围断言不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 眼动画笔精度（≥ 指针 80% 量化红线）
// ─────────────────────────────────────────────────────────────

/// 精度计分。
#[derive(Clone, Debug, Default)]
pub struct PrecisionScore {
    /// 指针基准分（固定 100）。
    pointer_baseline: u16,
    /// 眼动画笔实得分。
    eye_score: u16,
    /// 样本数。
    samples: u32,
}

impl PrecisionScore {
    /// 构造。
    pub fn new() -> PrecisionScore {
        PrecisionScore {
            pointer_baseline: 100,
            eye_score: 0,
            samples: 0,
        }
    }

    /// 记录一帧样本（分值 0-100）。
    pub fn sample(&mut self, score: u16) -> bool {
        if score > 100 {
            return false;
        }
        self.eye_score = score;
        self.samples += 1;
        true
    }

    /// 相对指针比值（万分制，避免浮点：10000=100%）。
    pub fn ratio_bp(&self) -> u16 {
        if self.pointer_baseline == 0 {
            return 0;
        }
        ((self.eye_score as u32 * 10000) / self.pointer_baseline as u32) as u16
    }

    /// 80% 红线合规：比值 ≥ 8000（万分制 80%）。
    pub fn meets_80(&self) -> bool {
        self.ratio_bp() >= 8000
    }

    /// 样本数。
    pub fn sample_count(&self) -> u32 {
        self.samples
    }

    /// 重置。
    pub fn reset(&mut self) {
        self.eye_score = 0;
        self.samples = 0;
    }
}

// ─────────────────────────────────────────────────────────────
// 扫描参数（开关控制创作适配）
// ─────────────────────────────────────────────────────────────

/// 扫描参数。
#[derive(Clone, Debug)]
pub struct ScanParams {
    /// 扫描周期（ms，创作级 200-2000）。
    pub cycle_ms: u16,
    /// 驻留阈值（0-100）。
    pub dwell: u8,
    /// 是否已适配创作密度。
    pub adapted: bool,
}

impl ScanParams {
    /// 构造（默认消费级节奏，未适配）。
    pub fn new() -> ScanParams {
        ScanParams {
            cycle_ms: 800,
            dwell: 50,
            adapted: false,
        }
    }

    /// 范围断言（创作级）。
    pub fn in_range(&self) -> bool {
        self.cycle_ms >= 200 && self.cycle_ms <= 2000 && self.dwell <= 100
    }

    /// 适配合规：在范围 + 已适配。
    pub fn fit_ok(&self) -> bool {
        self.in_range() && self.adapted
    }

    /// 适配到创作密度（缩短周期）。
    pub fn adapt_for_creation(&mut self, cycle_ms: u16, dwell: u8) -> bool {
        if cycle_ms < 200 || cycle_ms > 2000 || dwell > 100 {
            return false;
        }
        self.cycle_ms = cycle_ms;
        self.dwell = dwell;
        self.adapted = true;
        true
    }

    /// 注入不适配（演练）。
    pub fn inject_unfit(&mut self) {
        self.adapted = false;
    }
}

impl Default for ScanParams {
    fn default() -> ScanParams {
        ScanParams::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 语义树投影（读屏创作）
// ─────────────────────────────────────────────────────────────

/// 语义节点。
#[derive(Clone, Debug)]
pub struct SemNode {
    /// 节点 id。
    pub id: u32,
    /// 是否有读屏投影。
    pub projected: bool,
    /// 可及名称（空=缺失）。
    pub acc_name: alloc::string::String,
}

impl SemNode {
    /// 构造。
    pub fn new(id: u32) -> SemNode {
        SemNode {
            id,
            projected: false,
            acc_name: alloc::string::String::new(),
        }
    }

    /// 语义完备：有投影 + 有名称。
    pub fn complete(&self) -> bool {
        self.projected && !self.acc_name.is_empty()
    }
}

/// 语义树。
#[derive(Clone, Debug, Default)]
pub struct SemTree {
    nodes: Vec<SemNode>,
}

impl SemTree {
    /// 空树。
    pub fn new() -> SemTree {
        SemTree { nodes: Vec::new() }
    }

    /// 挂节点。
    pub fn attach(&mut self, n: SemNode) {
        self.nodes.push(n);
    }

    /// 投影。
    pub fn project(&mut self, id: u32, name: &str) -> bool {
        for n in self.nodes.iter_mut() {
            if n.id == id {
                n.projected = true;
                n.acc_name = alloc::string::String::from(name);
                return true;
            }
        }
        false
    }

    /// 扫描缺投影。
    pub fn scan_gap(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for n in self.nodes.iter() {
            if !n.complete() {
                bad.push(n.id);
            }
        }
        bad
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 创作命令词表（公开红线）
// ─────────────────────────────────────────────────────────────

/// 命令词表。
#[derive(Clone, Debug, Default)]
pub struct VoiceLexicon {
    /// 是否公开。
    public: bool,
    /// 命令数。
    commands: u32,
}

impl VoiceLexicon {
    /// 构造（私藏态，演练入口）。
    pub fn new() -> VoiceLexicon {
        VoiceLexicon {
            public: false,
            commands: 0,
        }
    }

    /// 公开。
    pub fn publish(&mut self) {
        self.public = true;
    }

    /// 登记命令。
    pub fn add_command(&mut self) -> u32 {
        self.commands += 1;
        self.commands
    }

    /// 公开红线合规：公开 + 有命令。
    pub fn public_ok(&self) -> bool {
        self.public && self.commands > 0
    }

    /// 命令数。
    pub fn command_count(&self) -> u32 {
        self.commands
    }

    /// 是否公开。
    pub fn is_public(&self) -> bool {
        self.public
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作辅具技术审查器。
#[derive(Clone, Debug, Default)]
pub struct AssistTechAuditor {
    precision: PrecisionScore,
    scan: ScanParams,
    tree: SemTree,
    lexicon: VoiceLexicon,
}

impl AssistTechAuditor {
    /// 构造。
    pub fn new() -> AssistTechAuditor {
        AssistTechAuditor {
            precision: PrecisionScore::new(),
            scan: ScanParams::new(),
            tree: SemTree::new(),
            lexicon: VoiceLexicon::new(),
        }
    }

    /// 取精度（可变）。
    pub fn precision_mut(&mut self) -> &mut PrecisionScore {
        &mut self.precision
    }

    /// 取扫描（可变）。
    pub fn scan_mut(&mut self) -> &mut ScanParams {
        &mut self.scan
    }

    /// 取语义树（可变）。
    pub fn tree_mut(&mut self) -> &mut SemTree {
        &mut self.tree
    }

    /// 取词表（可变）。
    pub fn lexicon_mut(&mut self) -> &mut VoiceLexicon {
        &mut self.lexicon
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<T7Code> {
        let mut bad = Vec::new();
        if !self.precision.meets_80() {
            bad.push(T7Code::EYE_PRECISION_LOW);
        }
        if !self.scan.fit_ok() {
            bad.push(T7Code::SCAN_NOT_FIT);
        }
        if !self.tree.scan_gap().is_empty() {
            bad.push(T7Code::SEMTREE_GAP);
        }
        if !self.lexicon.public_ok() {
            bad.push(T7Code::WORDLIST_PRIVATE);
        }
        if self.precision.sample_count() == 0 {
            bad.push(T7Code::SCORE_MISSING);
        }
        if !self.scan.in_range() {
            bad.push(T7Code::SCAN_OOB);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3748 判据集。
pub fn run_vr08h_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3748");

    // 0x8B01 精度 80% 线
    {
        let mut p = PrecisionScore::new();
        out.add("0x8B01-无样本不达标", !p.meets_80(), "");
        p.sample(75);
        out.add("0x8B01-75%不达标", !p.meets_80() && p.ratio_bp() == 7500, "");
        p.sample(80);
        out.add("0x8B01-80%恰达标", p.meets_80() && p.ratio_bp() == 8000, "");
        p.sample(101);
        out.add("0x8B01-越界拒收", p.sample_count() == 2, "");
    }

    // 0x8B02 扫描适配
    {
        let mut s = ScanParams::new();
        out.add("0x8B02-默认不适配", !s.fit_ok() && s.in_range(), "");
        s.adapt_for_creation(400, 40);
        out.add("0x8B02-适配后过", s.fit_ok(), "");
        s.inject_unfit();
        out.add("0x8B02-注入不适配", !s.fit_ok(), "");
        out.add("0x8B06-越界拒收", !s.adapt_for_creation(100, 40), "");
    }

    // 0x8B03 语义投影
    {
        let mut t = SemTree::new();
        t.attach(SemNode::new(1));
        out.add("0x8B03-缺投影检出", t.scan_gap().len() == 1, "");
        t.project(1, "画布");
        out.add("0x8B03-投影后过", t.scan_gap().is_empty(), "");
    }

    // 0x8B04 词表公开
    {
        let mut v = VoiceLexicon::new();
        v.add_command();
        out.add("0x8B04-私藏拦", !v.public_ok(), "");
        v.publish();
        out.add("0x8B04-公开后过", v.public_ok(), "");
    }

    // 判据
    {
        let mut a = AssistTechAuditor::new();
        a.precision_mut().sample(85);
        a.scan_mut().adapt_for_creation(500, 45);
        a.tree_mut().attach(SemNode::new(1));
        a.tree_mut().project(1, "工具栏");
        a.lexicon_mut().add_command();
        a.lexicon_mut().publish();
        out.add("0x8B05-全齐过", a.audit().is_empty(), "");
    }

    out
}
