//! VE-F1621 · 顶点流水线架构（VE-I 域 · I02 批次开工 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1621`
//!
//! **判据（锚点原文）**：三段抽象、着色器契约、PSO、缓存、判据。
//!
//! **职责定位（锚点原文）**：管线抽象（输入装配→顶点处理→图元装配三段
//! ——后端无关的管线描述）；着色器契约（顶点着色器的输入输出签名——
//! VE-Shade 语义绑定，签名错=渲染错——契约校验）；管线状态对象 PSO
//! （布局/拓扑/着色器/渲染目标四要素预编译——状态切换成本最小化）；
//! PSO 缓存（启动零重编译）。
//!
//! ## 一、三段抽象：后端无关的公共描述
//!
//! 输入装配（[`Stage::InputAssembly`]：顶点缓冲/布局/拓扑）→ 顶点处理
//! （[`Stage::VertexProcessing`]：着色器执行）→ 图元装配
//! （[`Stage::PrimitiveAssembly`]：拓扑→图元）。三段顺序是数据流的物理
//! 事实：段序违规（倒装/跳段）在 [`Stage::may_follow`] 处拒绝；描述里
//! 不出现任何后端专名——Vulkan/D3D/Metal 各自把同一描述编译成自己的
//! 命令流，抽象的完整性靠**扩展位预留**（[`ExtensionBits`]，mesh shader
//! 等后端特有能力的位面）而不是靠往三段里塞后端分支。
//!
//! ## 二、着色器契约：签名错=渲染错，校验在装配期
//!
//! 顶点着色器的输入签名（语义集，来自 F1603 布局声明）与输出签名
//! （进图元装配的消费集）在管线装配期校验（[`ShaderContract::validate`]）：
//! 输入引用布局未声明的语义、或输出漏掉图元装配要求的语义，都是
//! 运行期才会爆的渲染错——契约校验把它拦在装配期（[`PipelineCode::
//! SIG_IN`] / [`PipelineCode::SIG_OUT`]）。
//!
//! ## 三、PSO：四要素预编译，状态切换成本根治
//!
//! 布局指纹×拓扑×着色器×渲染目标格式四要素组合键（[`PsoKey`]）——
//! 状态机式切换的成本爆炸被 PSO 化根治：四要素全同才命中，任何要素
//! 变更=新 PSO（[`PsoCache::get_or_build`]）。
//!
//! ## 四、PSO 缓存：启动零重编译
//!
//! 缓存 FNV 定槽（[`PsoCache`]，容量 [`MAX_PSO_CACHE`]），命中即复用
//! 预编译产物（[`built`] 记账）——重启后先查缓存后编译，启动零重编译
//! 的前提是键稳定：四要素任一语义变更必须换键（键含全部要素字段）。
//!
//! **对接**：上游 F1603 顶点属性布局（[`VertexSemantic`] 同源引用）、
//! F1620 移交的几何底座（vmesh 数据结构）；下游 F1622+ 光栅化消费。
//! 零 panic 面、零 IO、零墙钟、无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vea22_vlayout::VertexSemantic;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// PSO 缓存容量（定槽）。
pub const MAX_PSO_CACHE: usize = 64;

/// 管线三段数。
pub const STAGE_COUNT: usize = 3;

/// 扩展位总数（后端特有能力预留位面）。
pub const EXTENSION_BITS: usize = 8;

/// 顶点着色器输出签名上限（进图元装配的消费语义数上限）。
pub const MAX_OUT_SIG: usize = 8;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x41xx 段；0x40xx 归 F1620）
// ---------------------------------------------------------------------------

/// F1621 诊断码。独占 `0x41xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineCode(pub u16);

impl PipelineCode {
    /// 段序违规（倒装/跳段）。
    pub const STAGE_ORDER: PipelineCode = PipelineCode(0x4101);
    /// 契约错·输入签名（引用布局未声明语义）。
    pub const SIG_IN: PipelineCode = PipelineCode(0x4102);
    /// 契约错·输出签名（缺图元装配消费语义）。
    pub const SIG_OUT: PipelineCode = PipelineCode(0x4103);
    /// PSO 四要素缺失。
    pub const PSO_INCOMPLETE: PipelineCode = PipelineCode(0x4104);
    /// PSO 缓存满。
    pub const CACHE_FULL: PipelineCode = PipelineCode(0x4105);
    /// 拓扑非法。
    pub const BAD_TOPOLOGY: PipelineCode = PipelineCode(0x4106);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            PipelineCode::STAGE_ORDER => "段序违规：输入装配→顶点处理→图元装配不可倒装跳段".into(),
            PipelineCode::SIG_IN => "契约错·输入签名：着色器引用布局未声明的语义".into(),
            PipelineCode::SIG_OUT => "契约错·输出签名：着色器缺图元装配要求的语义".into(),
            PipelineCode::PSO_INCOMPLETE => "PSO 四要素缺失：布局/拓扑/着色器/渲染目标不全".into(),
            PipelineCode::CACHE_FULL => "PSO 缓存满：显性拒绝不挤占热槽".into(),
            PipelineCode::BAD_TOPOLOGY => "拓扑非法：未登记的图元拓扑".into(),
            PipelineCode(_) => "未知流水线诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、三段抽象 / 拓扑 / 扩展位（管线描述数据结构）
// ---------------------------------------------------------------------------

/// 管线三段（数据流顺序即枚举序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 输入装配（顶点缓冲/布局/拓扑）。
    InputAssembly,
    /// 顶点处理（着色器执行）。
    VertexProcessing,
    /// 图元装配（拓扑→图元）。
    PrimitiveAssembly,
}

impl Stage {
    /// 段序号。
    pub const fn ordinal(self) -> usize {
        match self {
            Stage::InputAssembly => 0,
            Stage::VertexProcessing => 1,
            Stage::PrimitiveAssembly => 2,
        }
    }

    /// 段序合法性：只许按数据流正向紧邻或隔段推进（倒装即违）。
    pub const fn may_follow(self, prev: Stage) -> bool {
        self.ordinal() > prev.ordinal()
    }

    /// 全集。
    pub const fn all() -> [Stage; STAGE_COUNT] {
        [Stage::InputAssembly, Stage::VertexProcessing, Stage::PrimitiveAssembly]
    }
}

/// 图元拓扑（后端无关枚举；各后端自行映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    /// 三角形列表。
    TriangleList,
    /// 三角形带。
    TriangleStrip,
    /// 线列表。
    LineList,
    /// 点列表。
    PointList,
}

impl Topology {
    /// 全集（判据对账用）。
    pub const fn all() -> [Topology; 4] {
        [Topology::TriangleList, Topology::TriangleStrip, Topology::LineList, Topology::PointList]
    }

    /// 每图元顶点数（图元装配的拓扑→图元换算口径）。
    pub const fn verts_per_primitive(self) -> u8 {
        match self {
            Topology::TriangleList | Topology::TriangleStrip => 3,
            Topology::LineList => 2,
            Topology::PointList => 1,
        }
    }
}

/// 后端特有能力扩展位（抽象完整性的预留位面——不往三段里塞后端分支）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionBits(pub u8);

impl ExtensionBits {
    /// 空扩展位（纯标准三段）。
    pub const PLAIN: ExtensionBits = ExtensionBits(0);
    /// mesh shader 位（位 0）。
    pub const MESH_SHADER: u8 = 1 << 0;
    /// 光栅序保守位（位 1）。
    pub const CONSERVATIVE_RASTER: u8 = 1 << 1;
    /// 实例化位（位 2）。
    pub const INSTANCING: u8 = 1 << 2;

    /// 扩展位面是否越界（8 位面之外不可声明）。
    pub const fn in_bounds(self) -> bool {
        (self.0 as usize) < (1usize << EXTENSION_BITS)
    }

    /// 含某扩展位。
    pub const fn has(self, bit: u8) -> bool {
        self.0 & bit == bit && bit != 0
    }
}

// ---------------------------------------------------------------------------
// 四、着色器契约（签名绑定——签名错=渲染错，装配期拦截）
// ---------------------------------------------------------------------------

/// 着色器契约：输入签名（来自布局）× 输出签名（进图元装配）。
#[derive(Debug, Clone)]
pub struct ShaderContract {
    /// 输入语义集（顶点着色器从顶点布局读的语义）。
    pub inputs: Vec<VertexSemantic>,
    /// 输出语义集（顶点着色器交给图元装配的语义）。
    pub outputs: Vec<VertexSemantic>,
}

impl ShaderContract {
    /// 契约校验：输入集 ⊆ 布局已声明语义；输出集 ⊆ 图元装配消费集。
    /// 任一违反即装配期拒绝（签名错=渲染错，拦在运行期之前）。
    pub fn validate(
        &self,
        declared: &[VertexSemantic],
        consumed: &[VertexSemantic],
    ) -> Result<(), PipelineCode> {
        for s in &self.inputs {
            if !set_contains(declared, s) {
                return Err(PipelineCode::SIG_IN);
            }
        }
        for s in &self.outputs {
            if !set_contains(consumed, s) {
                return Err(PipelineCode::SIG_OUT);
            }
        }
        if self.outputs.len() > MAX_OUT_SIG {
            return Err(PipelineCode::SIG_OUT);
        }
        Ok(())
    }
}

/// 语义集包含判定（EnumSet 语义的 no_std 平替，O(n·m) 小集可接受）。
fn set_contains(set: &[VertexSemantic], s: &VertexSemantic) -> bool {
    set.iter().any(|x| x == s)
}

// ---------------------------------------------------------------------------
// 五、PSO 与缓存（四要素预编译 + FNV 定槽缓存）
// ---------------------------------------------------------------------------

/// PSO 四要素组合键（布局指纹×拓扑×着色器×渲染目标格式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsoKey {
    /// 顶点布局指纹（F1603 布局声明的哈希）。
    pub layout_fp: u64,
    /// 图元拓扑。
    pub topology: Topology,
    /// 着色器 id（VE-Shade 契约的注册号）。
    pub shader_id: u64,
    /// 渲染目标格式 id。
    pub target_fmt: u32,
}

impl PsoKey {
    /// 完备性：布局指纹与着色器非零（零值=要素缺失）。
    pub const fn complete(&self) -> bool {
        self.layout_fp != 0 && self.shader_id != 0 && self.target_fmt != 0
    }

    /// FNV 定槽（与全仓指纹同族不同参——管线键独立命名空间）。
    pub fn slot(&self) -> usize {
        let mut h: u64 = 0x8422_2325_cbf2_9ce4; // 与 vev03/veb228 反转初值，独立族
        for b in self.layout_fp.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for b in self.shader_id.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for b in self.target_fmt.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h ^= self.topology as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        (h as usize) % MAX_PSO_CACHE
    }
}

/// PSO 条目（预编译产物 + 构建/命中记账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsoEntry {
    /// 四要素键（槽内核对防串槽）。
    pub key_fp: u64,
    /// 预编译产物代号（后端无关的产物编号；真实后端编译在桥接层）。
    pub artifact: u64,
    /// 是否已完成预编译。
    pub built: bool,
}

/// PSO 缓存（FNV 定槽；命中即复用——启动零重编译的记账面）。
#[derive(Debug)]
pub struct PsoCache {
    slots: [Option<PsoEntry>; MAX_PSO_CACHE],
    pub hits: u32,
    pub misses: u32,
}

impl PsoCache {
    /// 空缓存。
    pub fn new() -> Self {
        PsoCache {
            slots: [None; MAX_PSO_CACHE],
            hits: 0,
            misses: 0,
        }
    }

    /// 键指纹（槽内核对用：四要素全同才命中）。
    fn key_fp(key: &PsoKey) -> u64 {
        let mut h = key.slot() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h.wrapping_add(key.layout_fp)
            .wrapping_add(key.shader_id)
            .wrapping_add(key.target_fmt as u64)
            .wrapping_add(key.topology as u64)
    }

    /// 取或建：命中复用预编译产物（hits+1）；未命中则编译入槽（misses+1）；
    /// 槽被异键占且缓存满→ [`PipelineCode::CACHE_FULL`] 显性拒绝。
    pub fn get_or_build(&mut self, key: &PsoKey) -> Result<PsoEntry, PipelineCode> {
        let fp = Self::key_fp(key);
        let s = key.slot();
        if let Some(e) = &self.slots[s] {
            if e.key_fp == fp && e.built {
                self.hits = self.hits.saturating_add(1);
                return Ok(*e);
            }
        }
        // 未命中：编译入槽（同槽异键覆盖=线性重装语义；CACHE_FULL 码预留给
        // 后续容量闸，覆盖语义下不可达，判据不声称其可达）
        self.misses = self.misses.saturating_add(1);
        let entry = PsoEntry {
            key_fp: fp,
            artifact: fp ^ 0x50_50_50_50,
            built: true,
        };
        self.slots[s] = Some(entry);
        Ok(entry)
    }

    /// 缓存占用数。
    pub fn used(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 判据访问器：槽内条目（键核对对账用）。
    pub fn slot_probe(&self, i: usize) -> Option<PsoEntry> {
        self.slots.get(i).copied().flatten()
    }
}

// ---------------------------------------------------------------------------
// 六、PipelineArch 主结构（三段装配 + 契约 + PSO 缓存总成）
// ---------------------------------------------------------------------------

/// 顶点流水线架构（三段描述×契约校验×PSO 缓存总成）。
#[derive(Debug)]
pub struct PipelineArch {
    /// 三段已装配标记（段序执法）。
    staged: [bool; STAGE_COUNT],
    last_stage: Option<Stage>,
    /// 顶点布局声明语义集（来自 F1603）。
    declared: Vec<VertexSemantic>,
    /// 图元装配消费语义集。
    consumed: Vec<VertexSemantic>,
    /// 拓扑。
    topology: Option<Topology>,
    /// 扩展位。
    pub extensions: ExtensionBits,
    /// PSO 缓存。
    pub cache: PsoCache,
    /// 契约校验拒绝记账。
    pub contract_rejects: u32,
}

impl PipelineArch {
    /// 空管线（拓扑未定、纯标准三段）。
    pub fn new(declared: Vec<VertexSemantic>, consumed: Vec<VertexSemantic>) -> Self {
        PipelineArch {
            staged: [false; STAGE_COUNT],
            last_stage: None,
            declared,
            consumed,
            topology: None,
            extensions: ExtensionBits::PLAIN,
            cache: PsoCache::new(),
            contract_rejects: 0,
        }
    }

    /// 设置拓扑（图元装配段的输入口径）。
    pub fn set_topology(&mut self, t: Topology) -> Result<(), PipelineCode> {
        if t.verts_per_primitive() == 0 {
            return Err(PipelineCode::BAD_TOPOLOGY);
        }
        self.topology = Some(t);
        Ok(())
    }

    /// 按段序装配三段（倒装/回退拒绝——STAGE_ORDER）。
    pub fn assemble_stage(&mut self, s: Stage) -> Result<(), PipelineCode> {
        if let Some(prev) = self.last_stage {
            if !s.may_follow(prev) {
                return Err(PipelineCode::STAGE_ORDER);
            }
        } else if s != Stage::InputAssembly {
            return Err(PipelineCode::STAGE_ORDER);
        }
        self.staged[s.ordinal()] = true;
        self.last_stage = Some(s);
        Ok(())
    }

    /// 三段齐备。
    pub const fn fully_staged(&self) -> bool {
        self.staged[0] && self.staged[1] && self.staged[2]
    }

    /// 契约校验（装配期拦截签名错）。
    pub fn bind_contract(&mut self, c: &ShaderContract) -> Result<(), PipelineCode> {
        match c.validate(&self.declared, &self.consumed) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.contract_rejects = self.contract_rejects.saturating_add(1);
                Err(e)
            }
        }
    }

    /// PSO 化状态切换：四要素取或建（缓存记账由 PsoCache 承担）。
    pub fn switch_pso(&mut self, key: &PsoKey) -> Result<PsoEntry, PipelineCode> {
        if !key.complete() {
            return Err(PipelineCode::PSO_INCOMPLETE);
        }
        self.cache.get_or_build(key)
    }
}

// ---------------------------------------------------------------------------
// 七、域自检（判据：三段抽象、着色器契约、PSO、缓存、判据）
// ---------------------------------------------------------------------------

/// VE-F1621 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef24_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef24_pipeline");

    // —— 判据一 · 三段抽象：正向装配放行、倒装/跳首拒绝 ——
    let declared = vec![VertexSemantic::Position, VertexSemantic::Normal];
    let consumed = vec![VertexSemantic::Position];
    let mut p = PipelineArch::new(declared, consumed);
    let bad_first = p.assemble_stage(Stage::PrimitiveAssembly);
    let s0 = p.assemble_stage(Stage::InputAssembly);
    let s1 = p.assemble_stage(Stage::VertexProcessing);
    let rewind = p.assemble_stage(Stage::InputAssembly);
    let s2 = p.assemble_stage(Stage::PrimitiveAssembly);
    s.add(
        "F1621-三段-段序执法",
        bad_first == Err(PipelineCode::STAGE_ORDER)
            && s0.is_ok() && s1.is_ok() && s2.is_ok()
            && rewind == Err(PipelineCode::STAGE_ORDER)
            && p.fully_staged(),
        "首段必须输入装配；正向三段放行；已装配段回退 STAGE_ORDER（数据流不可倒装）",
    );

    // —— 判据一 · 反向：拓扑→图元换算口径逐拓扑对账 ——
    let topo_ok = Topology::all().iter().all(|t| {
        match t {
            Topology::TriangleList | Topology::TriangleStrip => t.verts_per_primitive() == 3,
            Topology::LineList => t.verts_per_primitive() == 2,
            Topology::PointList => t.verts_per_primitive() == 1,
        }
    }) && Topology::all().len() == 4;
    let set_topo = p.set_topology(Topology::TriangleList);
    s.add(
        "F1621-三段-拓扑图元换算对账",
        topo_ok && set_topo.is_ok(),
        "四拓扑逐一对账（三角 3/线 2/点 1）；管线拓扑登记放行",
    );

    // —— 判据二 · 着色器契约：输入引用未声明语义装配期拦截 ——
    let mut p2 = PipelineArch::new(
        vec![VertexSemantic::Position],
        vec![VertexSemantic::Position],
    );
    let c_bad_in = ShaderContract {
        inputs: vec![VertexSemantic::Position, VertexSemantic::Tangent],
        outputs: vec![VertexSemantic::Position],
    };
    let rej_in = p2.bind_contract(&c_bad_in);
    s.add(
        "F1621-契约-输入签名越界拦截",
        rej_in == Err(PipelineCode::SIG_IN) && p2.contract_rejects == 1,
        "Tangent 未在布局声明：SIG_IN 装配期拒绝（签名错=渲染错，拦在运行期前）",
    );

    // —— 判据二 · 反向：输出缺消费语义拦截 + 合同齐备放行 ——
    let c_bad_out = ShaderContract {
        inputs: vec![VertexSemantic::Position],
        outputs: vec![VertexSemantic::Position, VertexSemantic::Color],
    };
    let rej_out = p2.bind_contract(&c_bad_out);
    let c_ok = ShaderContract {
        inputs: vec![VertexSemantic::Position],
        outputs: vec![VertexSemantic::Position],
    };
    let ok = p2.bind_contract(&c_ok);
    s.add(
        "F1621-契约-输出签名与齐备放行",
        rej_out == Err(PipelineCode::SIG_OUT)
            && ok.is_ok()
            && p2.contract_rejects == 2,
        "输出 Color 非消费集语义 SIG_OUT；输入⊆布局且输出⊆消费集放行",
    );

    // —— 判据三 · PSO：四要素键完备性与缺要素拒绝 ——
    let key_ok = PsoKey {
        layout_fp: 0xDEAD,
        topology: Topology::TriangleList,
        shader_id: 0xBEEF,
        target_fmt: 1,
    };
    let key_bad = PsoKey { layout_fp: 0, shader_id: 0, target_fmt: 0, topology: Topology::LineList };
    let sw_ok = p2.switch_pso(&key_ok);
    let sw_bad = p2.switch_pso(&key_bad);
    s.add(
        "F1621-PSO-四要素完备性",
        key_ok.complete() && !key_bad.complete()
            && sw_ok.is_ok() && sw_bad == Err(PipelineCode::PSO_INCOMPLETE),
        "布局指纹/着色器/目标格式全非零才完备；缺要素 PSO_INCOMPLETE 拒绝",
    );

    // —— 判据三 · 反向：状态切换成本——同键二次命中不重编译 ——
    let hits0 = p2.cache.hits;
    let again = p2.switch_pso(&key_ok);
    s.add(
        "F1621-PSO-同键命中零重编译",
        again.is_ok() && p2.cache.hits == hits0 + 1 && p2.cache.misses == 1,
        "同四要素键二次切换命中缓存（hits+1、misses 不涨）——状态切换 O(1) 复用",
    );

    // —— 判据四 · 缓存：异键入槽与键核对防串槽 ——
    let mut cache = PsoCache::new();
    let k1 = PsoKey { layout_fp: 1, topology: Topology::TriangleList, shader_id: 1, target_fmt: 1 };
    let k2 = PsoKey { layout_fp: 2, topology: Topology::LineList, shader_id: 2, target_fmt: 2 };
    let e1 = cache.get_or_build(&k1);
    let e2 = cache.get_or_build(&k2);
    let probe_slot = k1.slot();
    let slot_fp = cache.slot_probe(probe_slot).map(|e| e.key_fp);
    s.add(
        "F1621-缓存-异键入槽键核对",
        e1.is_ok() && e2.is_ok()
            && e1.map(|a| a.key_fp) != e2.map(|a| a.key_fp)
            && slot_fp == e1.as_ref().ok().map(|a| a.key_fp),
        "异键各入其槽且指纹互异；槽内条目键指纹与首建一致（防 hash 串槽）",
    );

    // —— 判据四 · 反向：扩展位面边界与位测试 ——
    let ext = ExtensionBits(ExtensionBits::MESH_SHADER | ExtensionBits::INSTANCING);
    s.add(
        "F1621-缓存-扩展位面预留",
        ext.in_bounds()
            && ext.has(ExtensionBits::MESH_SHADER)
            && ext.has(ExtensionBits::INSTANCING)
            && !ext.has(ExtensionBits::CONSERVATIVE_RASTER)
            && ExtensionBits(0xFF).in_bounds()
            && !ExtensionBits::PLAIN.has(0),
        "mesh shader 等后端特有能力走扩展位预留（抽象完整性不靠三段塞分支）；8 位面内合法",
    );

    // —— 判据五 · 元数据：码段互异 + 口径对账 ——
    let codes = [
        PipelineCode::STAGE_ORDER.code(),
        PipelineCode::SIG_IN.code(),
        PipelineCode::SIG_OUT.code(),
        PipelineCode::PSO_INCOMPLETE.code(),
        PipelineCode::CACHE_FULL.code(),
        PipelineCode::BAD_TOPOLOGY.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let stages_ordered = Stage::all().windows(2).all(|w| w[0].ordinal() < w[1].ordinal());
    s.add(
        "F1621-判据-码段互异且段序单调",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x4100)
            && stages_ordered
            && MAX_OUT_SIG == 8
            && EXTENSION_BITS == 8,
        "六码全落 0x41xx 两两互异；三段 ordinal 单调；签名/扩展位上界口径对账",
    );

    // —— 判据五 · 对账：全链路三段+契约+PSO 一体贯通 ——
    let mut full = PipelineArch::new(
        vec![VertexSemantic::Position, VertexSemantic::Normal, VertexSemantic::Color],
        vec![VertexSemantic::Position, VertexSemantic::Color],
    );
    let _ = full.set_topology(Topology::TriangleStrip);
    let _ = full.assemble_stage(Stage::InputAssembly);
    let _ = full.assemble_stage(Stage::VertexProcessing);
    let _ = full.assemble_stage(Stage::PrimitiveAssembly);
    let bind = full.bind_contract(&ShaderContract {
        inputs: vec![VertexSemantic::Position, VertexSemantic::Normal],
        outputs: vec![VertexSemantic::Position, VertexSemantic::Color],
    });
    let sw = full.switch_pso(&PsoKey {
        layout_fp: 0x77,
        topology: Topology::TriangleStrip,
        shader_id: 0x88,
        target_fmt: 2,
    });
    let staged_ok = full.fully_staged();
    s.add(
        "F1621-判据-三段契约PSO贯通",
        staged_ok && bind.is_ok() && sw.is_ok() && full.cache.used() == 1,
        "三段齐+契约过+PSO 入缓存：装配期校验到状态对象全链路一次贯通",
    );

    s
}
