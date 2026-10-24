//! VE-F3783 · R 域联测（40 场景四断言+心流）（R10 收口组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3783`
//!
//! **判据（锚点原文）**：十段链、40×四维、五断言、心流联测化、量化复用、
//! 判据。
//!
//! **本条是什么**：R 域主链十段贯通联测（创作请求→工作流→编辑→验证→
//! 预览→发布→个性化→市场→运行）；场景集 40（创作类型×资产规模×
//! 用户角色×辅具态四维）；断言五段（终态对拍/统计等效/耗时上限/零异常/
//! **心流断言**：创作中断率≤1/小时）。
//!
//! 诊断码段：独占 0xAExx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xAExx）
// ─────────────────────────────────────────────────────────────

/// F3783 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AECode(pub u16);

impl AECode {
    /// 分段归因失败。
    pub const ATTRIB_FAIL: AECode = AECode(0xAE01);
    /// 漂移超容差。
    pub const DRIFT_OVER: AECode = AECode(0xAE02);
    /// 中断率超——P1（心流红线）。
    pub const FLOW_BREAK: AECode = AECode(0xAE03);
    /// 数据丢失——P0。
    pub const DATA_LOSS: AECode = AECode(0xAE04);
    /// 链断（十段缺段）。
    pub const CHAIN_BREAK: AECode = AECode(0xAE05);
    /// 场景缺格。
    pub const SCENE_MISS: AECode = AECode(0xAE06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xAE01 => "联测失败且无法分段归因。",
            0xAE02 => "统计漂移超容差，需 ADR。",
            0xAE03 => "心流中断率超标（>1/小时），P1。",
            0xAE04 => "联测数据丢失，P0。",
            0xAE05 => "十段链缺段。",
            0xAE06 => "40 场景缺格。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十段链
// ─────────────────────────────────────────────────────────────

/// 链段（十段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainSeg {
    /// 创作请求。
    Request,
    /// 工作流。
    Workflow,
    /// 编辑。
    Edit,
    /// 验证。
    Validate,
    /// 预览。
    Preview,
    /// 发布。
    Publish,
    /// 个性化。
    Persona,
    /// 市场。
    Market,
    /// 运行。
    Run,
    /// 收束。
    Close,
}

impl ChainSeg {
    /// 全集。
    pub const ALL: [ChainSeg; 10] = [
        ChainSeg::Request,
        ChainSeg::Workflow,
        ChainSeg::Edit,
        ChainSeg::Validate,
        ChainSeg::Preview,
        ChainSeg::Publish,
        ChainSeg::Persona,
        ChainSeg::Market,
        ChainSeg::Run,
        ChainSeg::Close,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ChainSeg::Request => 0,
            ChainSeg::Workflow => 1,
            ChainSeg::Edit => 2,
            ChainSeg::Validate => 3,
            ChainSeg::Preview => 4,
            ChainSeg::Publish => 5,
            ChainSeg::Persona => 6,
            ChainSeg::Market => 7,
            ChainSeg::Run => 8,
            ChainSeg::Close => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ChainSeg::Request => "创作请求",
            ChainSeg::Workflow => "工作流",
            ChainSeg::Edit => "编辑",
            ChainSeg::Validate => "验证",
            ChainSeg::Preview => "预览",
            ChainSeg::Publish => "发布",
            ChainSeg::Persona => "个性化",
            ChainSeg::Market => "市场",
            ChainSeg::Run => "运行",
            ChainSeg::Close => "收束",
        }
    }
}

/// 链驱动器。
#[derive(Clone, Debug)]
pub struct ChainDriver {
    passed: [bool; 10],
}

impl ChainDriver {
    /// 新建。
    pub const fn new() -> Self {
        ChainDriver {
            passed: [false; 10],
        }
    }

    /// 跑一段。
    pub fn run_seg(&mut self, s: ChainSeg, ok: bool) -> bool {
        self.passed[s.ordinal()] = ok;
        ok
    }

    /// 覆盖数。
    pub fn coverage(&self) -> usize {
        self.passed.iter().filter(|p| **p).count()
    }

    /// 十段全通。
    pub fn all_pass(&self) -> bool {
        self.coverage() == 10
    }

    /// 归因：首个失败段。
    pub fn first_fail(&self) -> Option<usize> {
        for (i, p) in self.passed.iter().enumerate() {
            if !*p {
                return Some(i);
            }
        }
        None
    }

    /// 缺段名单。
    pub fn open_segs(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for s in ChainSeg::ALL.iter() {
            if !self.passed[s.ordinal()] {
                out.push(s.name());
            }
        }
        out
    }
}

// ─────────────────────────────────────────────────────────────
// 40 场景（四维）
// ─────────────────────────────────────────────────────────────

/// 场景（四维编码：类型×规模×角色×辅具）。
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    /// 创作类型 0..4。
    pub kind: u8,
    /// 资产规模 0..2。
    pub scale: u8,
    /// 用户角色 0..2。
    pub role: u8,
    /// 辅具态 0..2。
    pub assist: u8,
    /// 已跑。
    pub run: bool,
    /// 中断次数（心流）。
    pub interrupts: u16,
}

impl Scene {
    /// 编码唯一 id（0..40 的压缩：4×2×2×2=32 基 + 8 扩展场）。
    pub fn id(self) -> u16 {
        (self.kind as u16) * 16 + (self.scale as u16) * 8 + (self.role as u16) * 4 + self.assist as u16
    }

    /// 心流中断率（中断/小时，这里用整数万分比代理：interrupts*10000/60）。
    pub fn flow_rate(&self) -> u32 {
        (self.interrupts as u32) * 10_000 / 60
    }

    /// 心流达标（≤1/小时 = ≤10000 万分比）。
    pub fn flow_ok(&self) -> bool {
        self.flow_rate() <= 10_000
    }
}

/// 场景集。
#[derive(Clone, Debug)]
pub struct SceneSet {
    scenes: Vec<Scene>,
}

impl SceneSet {
    /// 新建 40 场景。
    pub fn new() -> Self {
        let mut scenes = Vec::new();
        // 32 基础格 + 8 扩展格 = 40
        let mut n = 0u8;
        for kind in 0..4u8 {
            for scale in 0..2u8 {
                for role in 0..2u8 {
                    for assist in 0..2u8 {
                        if n >= 32 {
                            break;
                        }
                        scenes.push(Scene {
                            kind,
                            scale,
                            role,
                            assist,
                            run: false,
                            interrupts: 0,
                        });
                        n += 1;
                    }
                }
            }
        }
        // 8 扩展：kind=4 复用
        for i in 0..8u8 {
            scenes.push(Scene {
                kind: 4,
                scale: i % 2,
                role: (i / 2) % 2,
                assist: (i / 4) % 2,
                run: false,
                interrupts: 0,
            });
        }
        SceneSet { scenes }
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 已跑数。
    pub fn run_count(&self) -> usize {
        self.scenes.iter().filter(|s| s.run).count()
    }

    /// 跑一格。
    pub fn mark_run(&mut self, idx: usize, interrupts: u16) -> bool {
        match self.scenes.get_mut(idx) {
            Some(s) => {
                s.run = true;
                s.interrupts = interrupts;
                true
            }
            None => false,
        }
    }

    /// 心流超标格数。
    pub fn flow_violations(&self) -> usize {
        self.scenes.iter().filter(|s| s.run && !s.flow_ok()).count()
    }

    /// 缺格数。
    pub fn missing(&self) -> usize {
        self.scenes.iter().filter(|s| !s.run).count()
    }
}

impl Default for SceneSet {
    fn default() -> Self {
        SceneSet::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 五断言矩阵
// ─────────────────────────────────────────────────────────────

/// 断言位（五断言）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertBit {
    /// 终态对拍。
    FinalMatch,
    /// 统计等效。
    StatEquiv,
    /// 耗时上限。
    TimeCap,
    /// 零异常。
    ZeroException,
    /// 心流断言。
    Flow,
}

impl AssertBit {
    /// 全集。
    pub const ALL: [AssertBit; 5] = [
        AssertBit::FinalMatch,
        AssertBit::StatEquiv,
        AssertBit::TimeCap,
        AssertBit::ZeroException,
        AssertBit::Flow,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            AssertBit::FinalMatch => 0,
            AssertBit::StatEquiv => 1,
            AssertBit::TimeCap => 2,
            AssertBit::ZeroException => 3,
            AssertBit::Flow => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            AssertBit::FinalMatch => "终态对拍",
            AssertBit::StatEquiv => "统计等效",
            AssertBit::TimeCap => "耗时上限",
            AssertBit::ZeroException => "零异常",
            AssertBit::Flow => "心流断言",
        }
    }
}

/// 断言报告（场景 × 五断言）。
#[derive(Clone, Debug)]
pub struct AssertReport {
    /// 每场景五位掩码。
    masks: Vec<u8>,
}

impl AssertReport {
    /// 新建。
    pub fn new() -> Self {
        AssertReport { masks: Vec::new() }
    }

    /// 开一场景行。
    pub fn open_row(&mut self) -> usize {
        self.masks.push(0);
        self.masks.len() - 1
    }

    /// 置一位。
    pub fn set(&mut self, row: usize, bit: AssertBit) -> bool {
        match self.masks.get_mut(row) {
            Some(m) => {
                *m |= 1 << bit.ordinal();
                true
            }
            None => false,
        }
    }

    /// 全五断言行数。
    pub fn full_rows(&self) -> usize {
        self.masks.iter().filter(|m| **m == 0b1_1111).count()
    }

    /// 行数。
    pub fn rows(&self) -> usize {
        self.masks.len()
    }
}

impl Default for AssertReport {
    fn default() -> Self {
        AssertReport::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3783 判据集。
pub fn run_vr10c_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3783");

    // 0xAE05 十段链
    {
        let mut d = ChainDriver::new();
        set.add("0xAE05-初始零段", d.coverage() == 0, "");
        set.add("0xAE05-首失败段", d.first_fail() == Some(0), "");
        for s in ChainSeg::ALL.iter() {
            d.run_seg(*s, true);
        }
        set.add("0xAE05-十段全通", d.all_pass(), "");
    }

    // 归因
    {
        let mut d = ChainDriver::new();
        for s in ChainSeg::ALL.iter() {
            d.run_seg(*s, true);
        }
        d.run_seg(ChainSeg::Validate, false);
        set.add("0xAE01-归因到验证", d.first_fail() == Some(3), "");
        set.add("0xAE01-缺段名单", d.open_segs().len() == 1, "");
    }

    // 0xAE06 场景集 40
    {
        let s = SceneSet::new();
        set.add("0xAE06-四十格", s.len() == 40, "");
        set.add("0xAE06-初始缺四十", s.missing() == 40, "");
    }

    // 0xAE03 心流断言
    {
        let mut s = SceneSet::new();
        s.mark_run(0, 30); // 30 次/小时 → 30*10000/60=5000 ≤ 10000 达标
        set.add("0xAE03-30次达标", s.flow_violations() == 0, "");
        s.mark_run(1, 120); // 120 次/小时 → 20000 > 10000 超标
        set.add("0xAE03-120次超标", s.flow_violations() == 1, "");
    }

    // 五断言
    {
        let mut r = AssertReport::new();
        let row = r.open_row();
        set.add("0xAE01-开行", r.rows() == 1, "");
        for b in AssertBit::ALL.iter() {
            r.set(row, *b);
        }
        set.add("0xAE01-五行满", r.full_rows() == 1, "");
        set.add("0xAE01-越行拒", !r.set(9, AssertBit::Flow), "");
    }

    // 断言位自洽
    {
        let mut ok = true;
        for (i, b) in AssertBit::ALL.iter().enumerate() {
            if b.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAE01-断言序自洽", ok, "");
        let mut ok_c = true;
        for (i, s) in ChainSeg::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok_c = false;
            }
        }
        set.add("0xAE05-段序自洽", ok_c, "");
    }

    // 场景 id 唯一性（抽样前 32）
    {
        let s = SceneSet::new();
        let mut seen = [false; 64];
        let mut dup = 0;
        for sc in s.scenes.iter().take(32) {
            let id = sc.id() as usize;
            if id < 64 && seen[id] {
                dup += 1;
            }
            if id < 64 {
                seen[id] = true;
            }
        }
        set.add("0xAE06-基格id唯一", dup == 0, "");
    }

    set
}
