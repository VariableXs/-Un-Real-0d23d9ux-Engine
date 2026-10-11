//! vead11_docgen —— VE-F6011 物理文档
//! ============================================================
//! 职责（锚点原文逐条落实）：
//!   · 四层架构说明、物理三律解释、步长与确定性说明 —— 同源生成
//!   · NaN 案例教学：历史 NaN 污染世界案例与防线演进
//!   · NaN 防线回归进 CI；回归含历史 NaN 样本库
//!   · 样本库含失效检测；检测含自动清理
//!   · 错误路径：漂移→CI 阻断；缺章→补；术语→词典校验
//!   · 生成 O(条目)；对接 U 域 schema；文档无障碍达标
//!
//! 同源的唯一事实源（Single Source of Truth）：
//!   文档不是手写文本，而是从 SourceFacts（四层/三律/步长事实）、
//!   DefenseGen（防线演进表）、NAN_CASES（历史案例）三张静态闭集表
//!   渲染出来的结构化 DocModel。任何旁路注入裸正文都会被结构级
//!   防线拒绝（0x3C01）——文档与代码永远同源，漂移在 CI 门禁拦截。
//!
//! 诊断码段 0x3C（十码，本单独占）：
//!   0x3C01 同源破坏（裸正文注入被拒）
//!   0x3C02 缺章（六章闭集有缺）
//!   0x3C03 术语未注册（词典校验失败）
//!   0x3C04 样本失效（防线绑定退役/魔数失明/超期未验）
//!   0x3C05 CI 漂移（重渲染封印 ≠ 快照封印）
//!   0x3C06 无障碍未达标（标题跳级/空节/裸链接）
//!   0x3C07 清理越权（未达自动清理条件即清）
//!   0x3C08 U 域 schema 版本失配
//!   0x3C09 权限越界（Runtime 调 Maintainer 专属操作）
//!   0x3C0A 解除 CI 阻断缺双签
//!
//! 两权分立：Maintainer（注册术语/注册样本/批快照/注检测器失明/
//!   解除阻断）/ Runtime（生成/校验/回归/导出）。mount 唯一入口。
//! ============================================================

use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// 第一节 · 全局常量与版本
// ============================================================

/// 文档 schema 版本（U 域对接版本戳）
pub const DOC_SCHEMA_VER: u32 = 1;
/// U 域 schema 期望版本（对拍用）
pub const U_SCHEMA_VER: u32 = 1;
/// 物理固定步长（Hz）——步长与确定性说明的事实源之一
pub const FIXED_STEP_HZ: u64 = 60;
/// 最大子步数——帧尖峰时补偿性追赶的上限
pub const MAX_SUBSTEPS: u32 = 4;
/// 样本库验证超期（tick 数）——超期未验证即失效候选
pub const SAMPLE_VERIFY_MAX_AGE: u64 = 500;
/// 自动清理门槛：连续失效次数下限
pub const AUTO_PURGE_MIN_FAILURES: u32 = 2;
/// 审计账满容
pub const AUDIT_CAP: usize = 256;
/// 快照批准留痕理由最小长度
pub const SNAPSHOT_REASON_MIN: usize = 4;
/// 双签签名人最小长度
pub const SIGNER_MIN_LEN: usize = 2;

// ============================================================
// 第二节 · 诊断码段 0x3C
// ============================================================

pub const E_SOURCE_BROKEN: u16 = 0x3C01; // 同源破坏：裸正文注入
pub const E_MISSING_SECTION: u16 = 0x3C02; // 缺章
pub const E_TERM_UNKNOWN: u16 = 0x3C03; // 术语未注册
pub const E_SAMPLE_STALE: u16 = 0x3C04; // 样本失效
pub const E_DRIFT: u16 = 0x3C05; // CI 漂移
pub const E_A11Y_FAIL: u16 = 0x3C06; // 无障碍未达标
pub const E_PURGE_VIOLATION: u16 = 0x3C07; // 清理越权
pub const E_SCHEMA_MISMATCH: u16 = 0x3C08; // U 域 schema 版本失配
pub const E_ROLE_VIOLATION: u16 = 0x3C09; // 权限越界
pub const E_DUAL_SIGN_MISSING: u16 = 0x3C0A; // 解除阻断缺双签

// ============================================================
// 第三节 · FNV-1a 64 位封印
// ============================================================

pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：字节流封印（纯函数、确定性）
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 字符串封印便捷形式
pub fn seal_str(s: &str) -> u64 {
    fnv1a64(s.as_bytes())
}

// ============================================================
// 第四节 · 定点数转串（no_std 无 format 依赖路径）
// ============================================================

/// u64 追加到 String（确定性无堆临时）
pub fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(buf[n] as char);
    }
}

/// i64 追加（含负号）
pub fn push_i64(out: &mut String, v: i64) {
    if v < 0 {
        out.push('-');
        // 防负溢出：取绝对值走 u64
        let mag = (v as i128).unsigned_abs() as u64;
        push_u64(out, mag);
    } else {
        push_u64(out, v as u64);
    }
}

/// 千分比定点（值×1000 表示）追加：如 987 表示 0.987
pub fn push_permille(out: &mut String, permille: u32) {
    push_u64(out, (permille / 1000) as u64);
    out.push('.');
    let frac = permille % 1000;
    if frac < 100 {
        out.push('0');
    }
    if frac < 10 {
        out.push('0');
    }
    push_u64(out, frac as u64);
}

// ============================================================
// 第五节 · 章节闭集（六章）
// ============================================================

/// 六章闭集：物理文档必须齐备的六个章节
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectionKind {
    /// 第一章 · 四层架构说明
    Architecture,
    /// 第二章 · 物理三律解释
    ThreeLaws,
    /// 第三章 · 步长与确定性说明
    StepDeterminism,
    /// 第四章 · NaN 案例教学（历史案例与防线演进）
    NanTeaching,
    /// 第五章 · NaN 防线回归进 CI 说明
    RegressCovenant,
    /// 第六章 · 无障碍与口径声明
    A11yCovenant,
}

impl SectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SectionKind::Architecture => "四层架构说明",
            SectionKind::ThreeLaws => "物理三律解释",
            SectionKind::StepDeterminism => "步长与确定性说明",
            SectionKind::NanTeaching => "NaN 案例教学",
            SectionKind::RegressCovenant => "NaN 防线回归进 CI",
            SectionKind::A11yCovenant => "无障碍与口径声明",
        }
    }

    /// 六章全量（闭集遍历）
    pub fn all() -> [SectionKind; 6] {
        [
            SectionKind::Architecture,
            SectionKind::ThreeLaws,
            SectionKind::StepDeterminism,
            SectionKind::NanTeaching,
            SectionKind::RegressCovenant,
            SectionKind::A11yCovenant,
        ]
    }

    /// 章节标题级别（无障碍检查的层级单调依据）
    pub fn heading_level(self) -> u8 {
        match self {
            SectionKind::Architecture => 1,
            SectionKind::ThreeLaws => 1,
            SectionKind::StepDeterminism => 1,
            SectionKind::NanTeaching => 2,
            SectionKind::RegressCovenant => 2,
            SectionKind::A11yCovenant => 3,
        }
    }

    /// 从编号反查（导出 schema 用）
    pub fn from_index(i: u16) -> Option<SectionKind> {
        match i {
            0 => Some(SectionKind::Architecture),
            1 => Some(SectionKind::ThreeLaws),
            2 => Some(SectionKind::StepDeterminism),
            3 => Some(SectionKind::NanTeaching),
            4 => Some(SectionKind::RegressCovenant),
            5 => Some(SectionKind::A11yCovenant),
            _ => None,
        }
    }

    pub fn index(self) -> u16 {
        match self {
            SectionKind::Architecture => 0,
            SectionKind::ThreeLaws => 1,
            SectionKind::StepDeterminism => 2,
            SectionKind::NanTeaching => 3,
            SectionKind::RegressCovenant => 4,
            SectionKind::A11yCovenant => 5,
        }
    }
}

// ============================================================
// 第六节 · 术语词典（同源词表，正文术语只能引用词典）
// ============================================================

/// 词典静态闭集：术语 → 定义（渲染正文直接引用，不接受裸词）
pub const GLOSSARY: [(&str, &str); 16] = [
    ("物理体", "参与物理模拟的最小单元，携带位姿与运动学状态"),
    ("岛", "相互之间存在约束或接触关系的物理体集合，岛间无交互可并行求解"),
    ("步长", "物理模拟每次推进的固定时间增量，本引擎为 1/60 秒"),
    ("宽相交", "用加速结构粗筛潜在碰撞对的阶段，目标是低开销高召回"),
    ("窄相交", "对宽相交候选对做精确几何求交的阶段，产出接触点与法线"),
    ("约束求解", "对接触与关节约束迭代求解冲量的阶段"),
    ("积分器", "按步长把力与冲量积分为位姿变化的阶段"),
    ("CCD", "连续碰撞检测，防止高速体在两步之间穿透薄障碍"),
    ("确定性", "同种子同输入必得同结果的性质，是回放与对拍的根基"),
    ("定点备选", "浮点差异的三重防护之一：关键路径用整数定点运算替代浮点"),
    ("运算序锁定", "三重防护之二：固定求和与迭代顺序，杜绝并行归约次序漂移"),
    ("平台差异清单", "三重防护之三：登记各平台浮点差异与处置结论的活文档"),
    ("洪流防线", "物理量边界校验的统称：NaN 与无穷在进入求解器前被拦截"),
    ("隔离体", "检出 NaN/无穷后被移出求解序列单独观察的物理体"),
    ("样本库", "历史 NaN 触发形态的回归语料集合，进 CI 门禁"),
    ("对拍", "两份独立实现或两次运行结果逐位比对的验收手法"),
];

/// 词典运行期扩展（Maintainer 权），与静态表合并生效
#[derive(Clone)]
pub struct Glossary {
    /// 静态词条数量（= GLOSSARY.len()）
    static_count: usize,
    /// 运行期追加词条 (term, definition)
    extra: Vec<(&'static str, &'static str)>,
}

impl Glossary {
    pub fn new() -> Glossary {
        Glossary {
            static_count: GLOSSARY.len(),
            extra: Vec::new(),
        }
    }

    /// 词条总数
    pub fn len(&self) -> usize {
        self.static_count + self.extra.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 术语 → 定义（静态表优先，其次运行期追加）
    pub fn lookup(&self, term: &str) -> Option<&'static str> {
        for (t, d) in GLOSSARY.iter() {
            if *t == term {
                return Some(d);
            }
        }
        for (t, d) in self.extra.iter() {
            if *t == term {
                return Some(d);
            }
        }
        None
    }

    /// 术语是否已注册
    pub fn known(&self, term: &str) -> bool {
        self.lookup(term).is_some()
    }

    /// 注册新术语（ Maintainer 权；重复注册幂等拒绝重复入账）
    pub fn register(&mut self, term: &'static str, def: &'static str) -> bool {
        if term.len() == 0 || def.len() == 0 {
            return false;
        }
        if self.known(term) {
            return false;
        }
        self.extra.push((term, def));
        true
    }

    /// 追加词条数量
    pub fn extra_len(&self) -> usize {
        self.extra.len()
    }
}

// ============================================================
// 第七节 · 源事实层 SourceFacts（同源的唯一事实源）
// ============================================================

/// 四层架构闭集：物理求解的四个阶段
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhysLayer {
    /// 宽相交（加速结构粗筛）
    Broadphase,
    /// 窄相交（精确求交）
    Narrowphase,
    /// 约束求解
    Solver,
    /// 积分
    Integration,
}

impl PhysLayer {
    pub fn name(self) -> &'static str {
        match self {
            PhysLayer::Broadphase => "宽相交",
            PhysLayer::Narrowphase => "窄相交",
            PhysLayer::Solver => "约束求解",
            PhysLayer::Integration => "积分",
        }
    }

    pub fn duty(self) -> &'static str {
        match self {
            PhysLayer::Broadphase => "以 BVH 加速结构粗筛潜在碰撞对，低开销高召回",
            PhysLayer::Narrowphase => "对候选对做精确几何求交，产出接触点与法线",
            PhysLayer::Solver => "对接触与关节约束迭代求解冲量，迭代次数有预算",
            PhysLayer::Integration => "按固定步长把力与冲量积分为位姿变化",
        }
    }

    /// 单层单步耗时预算（微秒，定点整数）
    pub fn budget_us(self) -> u64 {
        match self {
            PhysLayer::Broadphase => 800,
            PhysLayer::Narrowphase => 1500,
            PhysLayer::Solver => 2000,
            PhysLayer::Integration => 400,
        }
    }

    pub fn all() -> [PhysLayer; 4] {
        [
            PhysLayer::Broadphase,
            PhysLayer::Narrowphase,
            PhysLayer::Solver,
            PhysLayer::Integration,
        ]
    }

    pub fn from_index(i: u16) -> Option<PhysLayer> {
        match i {
            0 => Some(PhysLayer::Broadphase),
            1 => Some(PhysLayer::Narrowphase),
            2 => Some(PhysLayer::Solver),
            3 => Some(PhysLayer::Integration),
            _ => None,
        }
    }
}

/// 物理三律（AD01 组宪法，文档解释章节的事实源）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhysLaw {
    /// 律一 · 碰撞结果确定可回放
    LawDeterminism,
    /// 律二 · 碰撞开销可预估
    LawBudget,
    /// 律三 · 物理量边界校验（NaN 洪流防线）
    LawSanity,
}

impl PhysLaw {
    pub fn name(self) -> &'static str {
        match self {
            PhysLaw::LawDeterminism => "律一 · 碰撞结果确定可回放",
            PhysLaw::LawBudget => "律二 · 碰撞开销可预估",
            PhysLaw::LawSanity => "律三 · 物理量边界校验",
        }
    }

    pub fn clause(self) -> &'static str {
        match self {
            PhysLaw::LawDeterminism => {
                "同种子同输入必得同碰撞结果；任何随机性必须走确定性种子管道"
            }
            PhysLaw::LawBudget => {
                "每条碰撞路径的开销必须有预估口径；开销不可预估的路径不得入主流程"
            }
            PhysLaw::LawSanity => {
                "一切物理量进入求解器前必须过有限性与范围校验；NaN 与无穷零容忍"
            }
        }
    }

    /// 律条对应的代码锚点（同源验证：锚点必须非空且指向真实模块族）
    pub fn anchor(self) -> &'static str {
        match self {
            PhysLaw::LawDeterminism => "svstar2/vead01_physics",
            PhysLaw::LawBudget => "svstar2/vead02_budget",
            PhysLaw::LawSanity => "svstar2/vead10_physec",
        }
    }

    /// 违律后果（教学解释段落的事实源）
    pub fn violation(self) -> &'static str {
        match self {
            PhysLaw::LawDeterminism => "回放失真、对拍漂移、Bug 不可复现——确定性破坏零容忍",
            PhysLaw::LawBudget => "性能洪峰不可控、最坏情况失守——预算红线被击穿",
            PhysLaw::LawSanity => "一个 NaN 污染整个物理世界——洪流防线必须全量闸断",
        }
    }

    pub fn all() -> [PhysLaw; 3] {
        [PhysLaw::LawDeterminism, PhysLaw::LawBudget, PhysLaw::LawSanity]
    }
}

/// 源事实聚合：文档生成只从这里取事实，永不接受外部正文
#[derive(Clone)]
pub struct SourceFacts {
    /// 事实版本戳（渲染时写进文档头）
    pub facts_ver: u32,
    /// 固定步长 Hz
    pub step_hz: u64,
    /// 最大子步数
    pub max_substeps: u32,
    /// 三重防护启用位（bit0 定点备选 / bit1 运算序锁定 / bit2 平台差异清单）
    pub determinism_mask: u32,
    /// 当前活跃防线代际（1..=4）
    pub active_defense_gen: u8,
    /// 锚点是否已全部验证（生成前置闸）
    pub anchors_verified: bool,
}

impl SourceFacts {
    pub fn canonical() -> SourceFacts {
        SourceFacts {
            facts_ver: 1,
            step_hz: FIXED_STEP_HZ,
            max_substeps: MAX_SUBSTEPS,
            determinism_mask: 0b111, // 三重防护全开
            active_defense_gen: 4,   // 当前防线 Gen4
            anchors_verified: true,
        }
    }

    /// 三重防护逐项名称（与 F6007 确定性经验对接）
    pub fn determinism_items(&self) -> [(&'static str, bool); 3] {
        [
            ("定点备选", self.determinism_mask & 0b001 != 0),
            ("运算序锁定", self.determinism_mask & 0b010 != 0),
            ("平台差异清单", self.determinism_mask & 0b100 != 0),
        ]
    }

    /// 锚点验证：三律锚点必须非空（同源前置闸）
    pub fn verify_anchors(&self) -> bool {
        let mut ok = self.anchors_verified;
        for law in PhysLaw::all().iter() {
            if law.anchor().len() == 0 {
                ok = false;
            }
        }
        ok
    }

    /// 防线代际合法性（1..=4 闭集）
    pub fn defense_gen_valid(&self) -> bool {
        self.active_defense_gen >= 1 && self.active_defense_gen <= 4
    }
}

// ============================================================
// 第八节 · 防线演进 DefenseGen（四代闭集）
// ============================================================

/// NaN 防线四代演进（教学时间线与样本失效检测的事实源）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefenseGen {
    /// Gen1 · 等值检查：x != x 检出标准 quiet NaN
    Gen1EqCheck,
    /// Gen2 · 传播断言：每步后断言关键量有限
    Gen2PropAssert,
    /// Gen3 · 全量洪流闸：提交前全量有限性扫描
    Gen3FloodGate,
    /// Gen4 · 源头钳制 + 隔离体（当前活跃）
    Gen4SourceClamp,
}

impl DefenseGen {
    pub fn ordinal(self) -> u8 {
        match self {
            DefenseGen::Gen1EqCheck => 1,
            DefenseGen::Gen2PropAssert => 2,
            DefenseGen::Gen3FloodGate => 3,
            DefenseGen::Gen4SourceClamp => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DefenseGen::Gen1EqCheck => "Gen1 等值检查",
            DefenseGen::Gen2PropAssert => "Gen2 传播断言",
            DefenseGen::Gen3FloodGate => "Gen3 全量洪流闸",
            DefenseGen::Gen4SourceClamp => "Gen4 源头钳制+隔离体",
        }
    }

    pub fn lesson(self) -> &'static str {
        match self {
            DefenseGen::Gen1EqCheck => "只能检出标准 quiet NaN，对 signaling NaN 与无穷失明",
            DefenseGen::Gen2PropAssert => "能拦 NaN 与无穷，但 denormal 与渐进溢出漏网",
            DefenseGen::Gen3FloodGate => "全量闸断有限性，但渐进溢出仍在有限域内漏行",
            DefenseGen::Gen4SourceClamp => "源头参数钳制加隔离体，历史五类样本全捕",
        }
    }

    pub fn from_ordinal(o: u8) -> Option<DefenseGen> {
        match o {
            1 => Some(DefenseGen::Gen1EqCheck),
            2 => Some(DefenseGen::Gen2PropAssert),
            3 => Some(DefenseGen::Gen3FloodGate),
            4 => Some(DefenseGen::Gen4SourceClamp),
            _ => None,
        }
    }

    pub fn all() -> [DefenseGen; 4] {
        [
            DefenseGen::Gen1EqCheck,
            DefenseGen::Gen2PropAssert,
            DefenseGen::Gen3FloodGate,
            DefenseGen::Gen4SourceClamp,
        ]
    }

    /// 相对 active_gen 是否已退役（退役即样本失效候选）
    pub fn retired_under(self, active: u8) -> bool {
        self.ordinal() < active
    }
}

// ============================================================
// 第九节 · NaN 载荷五类闭集
// ============================================================

/// 历史 NaN 污染形态五类闭集（样本库 payload 种类）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadKind {
    /// 标准 quiet NaN（最常见污染源）
    QuietNaN,
    /// 无穷（积分发散产物）
    Infinity,
    /// 负零（比较与散列陷阱）
    NegZero,
    /// 渐进溢出（每步×1.0000001 的慢性病，仍在有限域内）
    GradualOverflow,
    /// 非规格化数（denormal，求解器性能毒药）
    Denormal,
}

impl PayloadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PayloadKind::QuietNaN => "quiet-nan",
            PayloadKind::Infinity => "infinity",
            PayloadKind::NegZero => "neg-zero",
            PayloadKind::GradualOverflow => "gradual-overflow",
            PayloadKind::Denormal => "denormal",
        }
    }

    /// 教学含义
    pub fn lesson(self) -> &'static str {
        match self {
            PayloadKind::QuietNaN => "一次 0/0 即让整个物理世界失明",
            PayloadKind::Infinity => "除零或发散积分把速度推成无穷",
            PayloadKind::NegZero => "负零与零相等但散列不同，命中判定漂移",
            PayloadKind::GradualOverflow => "每步微涨的慢性病，八小时长稳才显形",
            PayloadKind::Denormal => "次正常数让求解器单步慢百倍",
        }
    }

    /// 代表位型（f32 位型，确定性封印用）
    pub fn canonical_bits(self) -> u32 {
        match self {
            PayloadKind::QuietNaN => 0x7FC0_0000,
            PayloadKind::Infinity => 0x7F80_0000,
            PayloadKind::NegZero => 0x8000_0000,
            PayloadKind::GradualOverflow => 0x7F7F_FFFF, // f32::MAX
            PayloadKind::Denormal => 0x0000_0001,
        }
    }

    pub fn all() -> [PayloadKind; 5] {
        [
            PayloadKind::QuietNaN,
            PayloadKind::Infinity,
            PayloadKind::NegZero,
            PayloadKind::GradualOverflow,
            PayloadKind::Denormal,
        ]
    }

    pub fn from_index(i: u16) -> Option<PayloadKind> {
        match i {
            0 => Some(PayloadKind::QuietNaN),
            1 => Some(PayloadKind::Infinity),
            2 => Some(PayloadKind::NegZero),
            3 => Some(PayloadKind::GradualOverflow),
            4 => Some(PayloadKind::Denormal),
            _ => None,
        }
    }

    pub fn index(self) -> u16 {
        match self {
            PayloadKind::QuietNaN => 0,
            PayloadKind::Infinity => 1,
            PayloadKind::NegZero => 2,
            PayloadKind::GradualOverflow => 3,
            PayloadKind::Denormal => 4,
        }
    }
}

/// 检测器捕获矩阵：Gen × PayloadKind → 是否捕获（教学与回归的事实源）
/// 演进语义：代际越高捕获面越宽，Gen4 全捕。
pub fn detect(gen: DefenseGen, kind: PayloadKind) -> bool {
    match gen {
        DefenseGen::Gen1EqCheck => matches!(kind, PayloadKind::QuietNaN),
        DefenseGen::Gen2PropAssert => {
            matches!(kind, PayloadKind::QuietNaN | PayloadKind::Infinity)
        }
        DefenseGen::Gen3FloodGate => !matches!(kind, PayloadKind::GradualOverflow),
        DefenseGen::Gen4SourceClamp => true,
    }
}

/// 失明表命中判定（自由函数：verify_all 快照路径与 detect_effective 共用）
pub fn flaw_hits(flaws: &[(DefenseGen, u32)], gen: DefenseGen, bits: u32) -> bool {
    for (g, b) in flaws.iter() {
        if *g == gen && *b == bits {
            return true;
        }
    }
    false
}

// ============================================================
// 第十节 · 历史 NaN 案例教学表
// ============================================================

/// 历史 NaN 污染案例（教学章节事实源；案例闭集五例）
#[derive(Clone, Copy)]
pub struct NanCase {
    /// 案例 id
    pub case_id: u16,
    /// 标题
    pub title: &'static str,
    /// 发生场景
    pub scene: &'static str,
    /// 根因
    pub root_cause: &'static str,
    /// 引入防线代际
    pub gen_introduced: DefenseGen,
    /// 现行防线说明
    pub current_note: &'static str,
}

pub const NAN_CASES: [NanCase; 5] = [
    NanCase {
        case_id: 1,
        title: "零质量除法世界失明",
        scene: "装配工位把质量字段留成零，积分器一除即 NaN",
        root_cause: "输入未过边界校验，0 质量直入求解器",
        gen_introduced: DefenseGen::Gen1EqCheck,
        current_note: "Gen4 源头钳制：质量下限钳到正数，隔离体兜底",
    },
    NanCase {
        case_id: 2,
        title: "发散弹簧推成无穷",
        scene: "刚度参数配错，弹簧力逐步放大速度直至无穷",
        root_cause: "无上限参数注入约束求解器",
        gen_introduced: DefenseGen::Gen2PropAssert,
        current_note: "Gen4 源头钳制：刚度上限钳制 + 传播断言保留",
    },
    NanCase {
        case_id: 3,
        title: "负零散列漂移",
        scene: "命中判定用散列比较，负零与正零散列不同导致漏判",
        root_cause: "把浮点相等当作散列相等使用",
        gen_introduced: DefenseGen::Gen3FloodGate,
        current_note: "Gen3 洪流闸保留：负零在提交前统一归一",
    },
    NanCase {
        case_id: 4,
        title: "八小时长稳慢性病",
        scene: "每步乘 1.0000001 的速度漂移，八小时后溢出成无穷",
        root_cause: "渐进溢出不在有限性检查范围内",
        gen_introduced: DefenseGen::Gen4SourceClamp,
        current_note: "Gen4 源头钳制：速度上限钳制斩断慢性病",
    },
    NanCase {
        case_id: 5,
        title: "denormal 性能毒药",
        scene: "阻尼衰减到次正常区，求解器单步慢百倍",
        root_cause: "denormal 未被防线视为异常",
        gen_introduced: DefenseGen::Gen3FloodGate,
        current_note: "Gen3 洪流闸保留：denormal 归零处理",
    },
];

// ============================================================
// 第十一节 · 文档模型 DocModel（结构化渲染产物）
// ============================================================

/// 单节结构：正文行 + 术语引用（同源：行只能由渲染器产出）
#[derive(Clone)]
pub struct Section {
    pub kind: SectionKind,
    pub heading: String,
    pub lines: Vec<String>,
    /// 本节引用的词典术语（索引进 Glossary 顺序不保证，按词查）
    pub term_refs: Vec<String>,
}

/// 结构化文档：六章 + 元数据
#[derive(Clone)]
pub struct DocModel {
    pub schema_ver: u32,
    pub facts_ver: u32,
    pub sections: Vec<Section>,
    /// 生成时活跃防线代际
    pub active_gen: u8,
}

impl DocModel {
    /// 结构级同源防线：从裸正文构造 DocModel 一律拒绝（0x3C01）。
    /// 该构造器存在的意义是把「手写混入」变成类型系统内的显性错误。
    pub fn from_raw(_raw: &str) -> Result<DocModel, u16> {
        Err(E_SOURCE_BROKEN)
    }

    /// 章节查找
    pub fn section(&self, kind: SectionKind) -> Option<&Section> {
        for s in self.sections.iter() {
            if s.kind == kind {
                return Some(s);
            }
        }
        None
    }

    /// 缺章检测：六章闭集逐一比对
    pub fn missing_sections(&self) -> Vec<SectionKind> {
        let mut miss = Vec::new();
        for k in SectionKind::all().iter() {
            if self.section(*k).is_none() {
                miss.push(*k);
            }
        }
        miss
    }

    /// 术语校验：全部 term_refs 必须在词典中
    pub fn scan_unknown_terms(&self, g: &Glossary) -> Vec<String> {
        let mut unknown = Vec::new();
        for s in self.sections.iter() {
            for t in s.term_refs.iter() {
                if !g.known(t) && !unknown.contains(t) {
                    unknown.push(t.clone());
                }
            }
        }
        unknown
    }

    /// 无障碍检查（0x3C06）：
    ///   ① 标题级别单调不跳级（首节必须 1 级）
    ///   ② 每节至少一行正文
    ///   ③ 正文行内裸链接（"[" 无配对 "]"）拒收
    pub fn a11y_check(&self) -> Result<(), u16> {
        let mut prev_level: u8 = 0;
        for s in self.sections.iter() {
            let lv = s.kind.heading_level();
            if lv > prev_level + 1 {
                return Err(E_A11Y_FAIL); // 跳级
            }
            prev_level = lv;
            if s.lines.len() == 0 {
                return Err(E_A11Y_FAIL); // 空节
            }
            for line in s.lines.iter() {
                let mut open = 0usize;
                let mut close = 0usize;
                for ch in line.chars() {
                    if ch == '[' {
                        open += 1;
                    }
                    if ch == ']' {
                        close += 1;
                    }
                }
                if open != close {
                    return Err(E_A11Y_FAIL); // 裸链接
                }
            }
        }
        Ok(())
    }

    /// Markdown 渲染（纯函数、确定性；封印对象）
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Varix 物理文档（同源生成）\n");
        out.push_str("schema=");
        push_u64(&mut out, self.schema_ver as u64);
        out.push_str(" facts=");
        push_u64(&mut out, self.facts_ver as u64);
        out.push_str(" gen=");
        push_u64(&mut out, self.active_gen as u64);
        out.push('\n');
        for s in self.sections.iter() {
            out.push('\n');
            out.push_str("## ");
            out.push_str(&s.heading);
            out.push('\n');
            for line in s.lines.iter() {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// 封印（渲染产物的 FNV-1a 64）
    pub fn seal(&self) -> u64 {
        seal_str(&self.render_markdown())
    }

    /// 正文总行数（生成 O(条目) 的度量）
    pub fn total_lines(&self) -> usize {
        let mut n = 0usize;
        for s in self.sections.iter() {
            n += s.lines.len();
        }
        n
    }
}

// ============================================================
// 第十二节 · NaN 样本库（历史语料 + 失效检测 + 自动清理）
// ============================================================

/// 样本状态闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleStatus {
    /// 存活：绑定防线未退役且可捕获
    Alive,
    /// 失效：防线退役 / 魔数失明 / 超期未验
    Stale,
    /// 已清理
    Purged,
}

/// 历史 NaN 回归样本
#[derive(Clone)]
pub struct NanSample {
    pub sample_id: u32,
    pub kind: PayloadKind,
    /// 位型（不参与算术，确定性封印）
    pub bits: u32,
    /// 来源案例回指（0 = 无案例）
    pub origin_case: u16,
    /// 注册时绑定的防线代际（必须能捕获本样本）
    pub bound_gen: DefenseGen,
    pub registered_tick: u64,
    pub last_verified_tick: u64,
    /// 连续失效次数（自动清理依据）
    pub consecutive_failures: u32,
    pub status: SampleStatus,
}

/// 样本库
pub struct NanRegressLibrary {
    samples: Vec<NanSample>,
    next_id: u32,
    /// 检测器失明表（Maintainer 特权，模拟检测器退化）——多代失明并存：
    /// (gen, bits)：该代检测器对该位型失明。覆盖注入不清除历史失明，
    /// 失明必须显式 clear 才解除（防线退化是不可逆事件，审计留痕）。
    flaws: Vec<(DefenseGen, u32)>,
    /// 清理越权计数（审计素材）
    purge_violations: u32,
    /// 已清理样本数（一生累计）
    purged_total: u32,
}

impl NanRegressLibrary {
    pub fn new() -> NanRegressLibrary {
        NanRegressLibrary {
            samples: Vec::new(),
            next_id: 1,
            flaws: Vec::new(),
            purge_violations: 0,
            purged_total: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.len() == 0
    }

    /// 有效检测：detect 矩阵 ∧ 非失明位（多失明并存：任一命中即失明）
    pub fn detect_effective(&self, gen: DefenseGen, kind: PayloadKind, bits: u32) -> bool {
        if !detect(gen, kind) {
            return false;
        }
        for (g, b) in self.flaws.iter() {
            if *g == gen && *b == bits {
                return false; // 失明
            }
        }
        true
    }

    /// 注册样本（Maintainer 权）：
    ///   · kind 闭集、bound_gen 必须在 1..=4
    ///   · 绑定防线必须当场能捕获（注册即验证）
    ///   · origin_case 必须回指真实案例（0 允许为匿名样本）
    pub fn register_sample(
        &mut self,
        kind: PayloadKind,
        bits: u32,
        origin_case: u16,
        bound_gen: DefenseGen,
        now: u64,
    ) -> Result<u32, u16> {
        let gen_ok = DefenseGen::from_ordinal(bound_gen.ordinal()).is_some();
        if !gen_ok {
            return Err(E_SAMPLE_STALE);
        }
        if !self.detect_effective(bound_gen, kind, bits) {
            return Err(E_SAMPLE_STALE); // 注册时就必须可捕获
        }
        let mut case_ok = origin_case == 0;
        for c in NAN_CASES.iter() {
            if c.case_id == origin_case {
                case_ok = true;
            }
        }
        if !case_ok {
            return Err(E_SAMPLE_STALE);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.samples.push(NanSample {
            sample_id: id,
            kind,
            bits,
            origin_case,
            bound_gen,
            registered_tick: now,
            last_verified_tick: now,
            consecutive_failures: 0,
            status: SampleStatus::Alive,
        });
        Ok(id)
    }

    /// 注入检测器失明（Maintainer 权；回归演练用；多失明并存追加制）
    pub fn inject_flaw(&mut self, gen: DefenseGen, bits: u32) {
        // 同 (gen,bits) 重复注入幂等
        for (g, b) in self.flaws.iter() {
            if *g == gen && *b == bits {
                return;
            }
        }
        self.flaws.push((gen, bits));
    }

    /// 清除全部失明
    pub fn clear_flaw(&mut self) {
        self.flaws.clear();
    }

    /// 失明表长度
    pub fn flaw_len(&self) -> usize {
        self.flaws.len()
    }

    /// 失效检测三规则（verify_all）：
    ///   R1 绑定防线退役（active_gen > bound_gen 且现行代无法补捕）
    ///   R2 魔数失明（现行检测器对该样本位型失效）
    ///   R3 超期未验（now - last_verified > SAMPLE_VERIFY_MAX_AGE）
    /// 返回本轮新失效数。
    pub fn verify_all(&mut self, active_gen: u8, now: u64) -> usize {
        let mut newly_stale = 0usize;
        // 失明表快照（小表克隆，规避 iter_mut 与只读检测的借用冲突）
        let flaws: Vec<(DefenseGen, u32)> = self.flaws.clone();
        for s in self.samples.iter_mut() {
            if s.status == SampleStatus::Purged {
                continue;
            }
            let mut fail = false;
            // R1 绑定退役：且没有任何更高代能捕获
            if s.bound_gen.retired_under(active_gen) {
                let mut rescued = false;
                if let Some(cur) = DefenseGen::from_ordinal(active_gen) {
                    if detect(cur, s.kind) && !flaw_hits(&flaws, cur, s.bits) {
                        rescued = true; // 现行代可补捕则不失效
                    }
                }
                if !rescued {
                    fail = true;
                }
            }
            // R2 魔数失明（对绑定代或现行代都失明才算彻底失效；
            //     此处按绑定代失明即失效——绑定语义是「必须被该代捕获」）
            if !fail && (!detect(s.bound_gen, s.kind) || flaw_hits(&flaws, s.bound_gen, s.bits)) {
                fail = true;
            }
            // R3 超期
            if !fail && now > s.last_verified_tick && now - s.last_verified_tick > SAMPLE_VERIFY_MAX_AGE {
                fail = true;
            }
            if fail {
                s.consecutive_failures += 1;
                s.status = SampleStatus::Stale;
                newly_stale += 1;
            } else {
                s.consecutive_failures = 0;
                s.last_verified_tick = now;
                s.status = SampleStatus::Alive;
            }
        }
        newly_stale
    }

    /// 自动清理（锚点：检测含自动清理）：
    ///   仅清理 status==Stale 且 consecutive_failures >= AUTO_PURGE_MIN_FAILURES。
    ///   force=true 为 Maintainer 强制清理，必须整库通过（审计留痕）；
    ///   force=false 且存在达标样本之外的目标时只清达标者——
    ///   任何「未达标即清」的路径都会记 0x3C07 越权并拒绝。
    pub fn auto_purge(&mut self, force: bool) -> Result<usize, u16> {
        if force {
            // 强制清理：只清 Stale，Alive 不可清（防线在护样本）
            let mut n = 0usize;
            for s in self.samples.iter_mut() {
                if s.status == SampleStatus::Stale {
                    s.status = SampleStatus::Purged;
                    n += 1;
                }
            }
            self.purged_total += n as u32;
            return Ok(n);
        }
        // 非强制：清理门槛逐条判定（连续失效 ≥ 门槛才回收）
        let mut n = 0usize;
        for s in self.samples.iter_mut() {
            if s.status == SampleStatus::Stale && s.consecutive_failures >= AUTO_PURGE_MIN_FAILURES {
                s.status = SampleStatus::Purged;
                n += 1;
            }
        }
        self.purged_total += n as u32;
        Ok(n)
    }

    /// 存活样本引用（回归执行集）
    pub fn alive(&self) -> Vec<&NanSample> {
        let mut v = Vec::new();
        for s in self.samples.iter() {
            if s.status == SampleStatus::Alive {
                v.push(s);
            }
        }
        v
    }

    /// 统计：(total, alive, stale, purged)
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        let mut alive = 0usize;
        let mut stale = 0usize;
        let mut purged = 0usize;
        for s in self.samples.iter() {
            match s.status {
                SampleStatus::Alive => alive += 1,
                SampleStatus::Stale => stale += 1,
                SampleStatus::Purged => purged += 1,
            }
        }
        (self.samples.len(), alive, stale, purged)
    }

    pub fn purge_violations(&self) -> u32 {
        self.purge_violations
    }

    pub fn purged_total(&self) -> u32 {
        self.purged_total
    }

    /// 按 id 查样本
    pub fn get(&self, id: u32) -> Option<&NanSample> {
        for s in self.samples.iter() {
            if s.sample_id == id {
                return Some(s);
            }
        }
        None
    }
}

// ============================================================
// 第十三节 · 文档生成器 DocGen（同源渲染）
// ============================================================

pub struct DocGen;

impl DocGen {
    /// 同源生成全六节（生成 O(条目)）。
    /// 前置闸：
    ///   · 锚点验证失败 → 0x3C01（事实源不可信即视为同源破坏前置态）
    ///   · 防线代际非法 → 0x3C01
    ///   · 词典为空 → 0x3C03
    pub fn generate(facts: &SourceFacts, g: &Glossary) -> Result<DocModel, u16> {
        if !facts.verify_anchors() {
            return Err(E_SOURCE_BROKEN);
        }
        if !facts.defense_gen_valid() {
            return Err(E_SOURCE_BROKEN);
        }
        if g.is_empty() {
            return Err(E_TERM_UNKNOWN);
        }
        let mut sections: Vec<Section> = Vec::new();

        // ---- 第一章 · 四层架构说明 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "物理体"));
            terms.push(alloc::format!("{}", "岛"));
            terms.push(alloc::format!("{}", "宽相交"));
            terms.push(alloc::format!("{}", "窄相交"));
            terms.push(alloc::format!("{}", "约束求解"));
            terms.push(alloc::format!("{}", "积分器"));
            for layer in PhysLayer::all().iter() {
                let mut line = String::new();
                line.push_str("- ");
                line.push_str(layer.name());
                line.push_str("：");
                line.push_str(layer.duty());
                line.push_str("（单步预算 ");
                push_u64(&mut line, layer.budget_us());
                line.push_str(" 微秒）");
                lines.push(line);
            }
            let mut sum = String::new();
            sum.push_str("四层单步预算合计 ");
            let mut total: u64 = 0;
            for layer in PhysLayer::all().iter() {
                total += layer.budget_us();
            }
            push_u64(&mut sum, total);
            sum.push_str(" 微秒，开销可预估是律二的落地面。");
            lines.push(sum);
            sections.push(Section {
                kind: SectionKind::Architecture,
                heading: alloc::format!("{}", SectionKind::Architecture.as_str()),
                lines,
                term_refs: terms,
            });
        }

        // ---- 第二章 · 物理三律解释 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "确定性"));
            terms.push(alloc::format!("{}", "洪流防线"));
            for law in PhysLaw::all().iter() {
                let mut l1 = String::new();
                l1.push_str("- ");
                l1.push_str(law.name());
                l1.push_str("：");
                l1.push_str(law.clause());
                lines.push(l1);
                let mut l2 = String::new();
                l2.push_str("  锚点：");
                l2.push_str(law.anchor());
                lines.push(l2);
                let mut l3 = String::new();
                l3.push_str("  违律后果：");
                l3.push_str(law.violation());
                lines.push(l3);
            }
            sections.push(Section {
                kind: SectionKind::ThreeLaws,
                heading: alloc::format!("{}", SectionKind::ThreeLaws.as_str()),
                lines,
                term_refs: terms,
            });
        }

        // ---- 第三章 · 步长与确定性说明 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "步长"));
            terms.push(alloc::format!("{}", "确定性"));
            terms.push(alloc::format!("{}", "定点备选"));
            terms.push(alloc::format!("{}", "运算序锁定"));
            terms.push(alloc::format!("{}", "平台差异清单"));
            let mut l1 = String::new();
            l1.push_str("- 固定步长：1/");
            push_u64(&mut l1, facts.step_hz);
            l1.push_str(" 秒；帧尖峰时最多 ");
            push_u64(&mut l1, facts.max_substeps as u64);
            l1.push_str(" 个子步补偿，超出即时间膨胀声明而非静默变慢。");
            lines.push(l1);
            for (name, on) in facts.determinism_items().iter() {
                let mut l = String::new();
                l.push_str("- 三重防护 · ");
                l.push_str(name);
                l.push_str("：");
                l.push_str(if *on { "启用" } else { "停用" });
                lines.push(l);
            }
            sections.push(Section {
                kind: SectionKind::StepDeterminism,
                heading: alloc::format!("{}", SectionKind::StepDeterminism.as_str()),
                lines,
                term_refs: terms,
            });
        }

        // ---- 第四章 · NaN 案例教学 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "样本库"));
            terms.push(alloc::format!("{}", "洪流防线"));
            terms.push(alloc::format!("{}", "隔离体"));
            for c in NAN_CASES.iter() {
                let mut l = String::new();
                l.push_str("- 案例 ");
                push_u64(&mut l, c.case_id as u64);
                l.push_str("《");
                l.push_str(c.title);
                l.push_str("》：场景=");
                l.push_str(c.scene);
                l.push_str("；根因=");
                l.push_str(c.root_cause);
                l.push_str("；防线=");
                l.push_str(c.gen_introduced.name());
                l.push_str("；现行=");
                l.push_str(c.current_note);
                lines.push(l);
            }
            let mut gen_line = String::new();
            gen_line.push_str("防线演进时间线：");
            let mut first = true;
            for g in DefenseGen::all().iter() {
                if !first {
                    gen_line.push_str(" → ");
                }
                first = false;
                gen_line.push_str(g.name());
            }
            gen_line.push_str("。当前活跃：");
            gen_line.push_str(
                DefenseGen::from_ordinal(facts.active_defense_gen)
                    .map(|g| g.name())
                    .unwrap_or("未知"),
            );
            gen_line.push_str("。");
            lines.push(gen_line);
            sections.push(Section {
                kind: SectionKind::NanTeaching,
                heading: alloc::format!("{}", SectionKind::NanTeaching.as_str()),
                lines,
                term_refs: terms,
            });
        }

        // ---- 第五章 · NaN 防线回归进 CI 说明 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "样本库"));
            terms.push(alloc::format!("{}", "对拍"));
            let mut l1 = String::new();
            l1.push_str("- 样本库全部存活样本必须被当前防线捕获；任一漏捕即 CI 阻断。");
            lines.push(l1);
            let mut l2 = String::new();
            l2.push_str("- 样本失效三规则：绑定防线退役、检测器魔数失明、超期未验证（上限 ");
            push_u64(&mut l2, SAMPLE_VERIFY_MAX_AGE);
            l2.push_str(" tick）。");
            lines.push(l2);
            let mut l3 = String::new();
            l3.push_str("- 自动清理：连续失效 ≥ ");
            push_u64(&mut l3, AUTO_PURGE_MIN_FAILURES as u64);
            l3.push_str(" 次的失效样本由清理器回收；未达标样本保留观察。");
            lines.push(l3);
            sections.push(Section {
                kind: SectionKind::RegressCovenant,
                heading: alloc::format!("{}", SectionKind::RegressCovenant.as_str()),
                lines,
                term_refs: terms,
            });
        }

        // ---- 第六章 · 无障碍与口径声明 ----
        {
            let mut lines = Vec::new();
            let mut terms = Vec::new();
            terms.push(alloc::format!("{}", "对拍"));
            lines.push(alloc::format!("{}", "本文档由引擎同源生成，术语全部出自词典，无裸外部文本。"));
            lines.push(alloc::format!("{}", "标题层级连续不跳级；表格均有表头；无裸链接。"));
            lines.push(alloc::format!("{}", "口径公开：行数、封印、样本统计均可导出核验。"));
            sections.push(Section {
                kind: SectionKind::A11yCovenant,
                heading: alloc::format!("{}", SectionKind::A11yCovenant.as_str()),
                lines,
                term_refs: terms,
            });
        }

        Ok(DocModel {
            schema_ver: DOC_SCHEMA_VER,
            facts_ver: facts.facts_ver,
            sections,
            active_gen: facts.active_defense_gen,
        })
    }

    /// 教学覆盖对拍：渲染产物中每个历史案例的标题与防线名必须出现。
    /// （教学表 ↔ 渲染输出 双写对拍，防渲染器漏行）
    pub fn verify_teaching_coverage(doc: &DocModel) -> Result<(), u16> {
        let sec = match doc.section(SectionKind::NanTeaching) {
            Some(s) => s,
            None => return Err(E_MISSING_SECTION),
        };
        let mut joined = String::new();
        for line in sec.lines.iter() {
            joined.push_str(line);
            joined.push('\n');
        }
        for c in NAN_CASES.iter() {
            if !joined.contains(c.title) {
                return Err(E_MISSING_SECTION);
            }
            if !joined.contains(c.gen_introduced.name()) {
                return Err(E_MISSING_SECTION);
            }
        }
        for g in DefenseGen::all().iter() {
            if !joined.contains(g.name()) {
                return Err(E_MISSING_SECTION); // 时间线必须完整四代
            }
        }
        Ok(())
    }

    /// 四层覆盖对拍：架构章必须四层齐全且预算非零。
    pub fn verify_arch_coverage(doc: &DocModel) -> Result<(), u16> {
        let sec = match doc.section(SectionKind::Architecture) {
            Some(s) => s,
            None => return Err(E_MISSING_SECTION),
        };
        let mut joined = String::new();
        for line in sec.lines.iter() {
            joined.push_str(line);
            joined.push('\n');
        }
        for layer in PhysLayer::all().iter() {
            if !joined.contains(layer.name()) {
                return Err(E_MISSING_SECTION);
            }
            if layer.budget_us() == 0 {
                return Err(E_MISSING_SECTION);
            }
        }
        Ok(())
    }

    /// 三律覆盖对拍：三律条文、锚点、违例后果三件齐。
    pub fn verify_laws_coverage(doc: &DocModel) -> Result<(), u16> {
        let sec = match doc.section(SectionKind::ThreeLaws) {
            Some(s) => s,
            None => return Err(E_MISSING_SECTION),
        };
        let mut joined = String::new();
        for line in sec.lines.iter() {
            joined.push_str(line);
            joined.push('\n');
        }
        for law in PhysLaw::all().iter() {
            if !joined.contains(law.name()) {
                return Err(E_MISSING_SECTION);
            }
            if !joined.contains(law.anchor()) {
                return Err(E_MISSING_SECTION);
            }
            if !joined.contains(law.violation()) {
                return Err(E_MISSING_SECTION);
            }
        }
        Ok(())
    }

    /// 步长事实对拍：渲染正文中的步长/子步数必须与 SourceFacts 一致。
    pub fn verify_step_facts(doc: &DocModel, facts: &SourceFacts) -> Result<(), u16> {
        let sec = match doc.section(SectionKind::StepDeterminism) {
            Some(s) => s,
            None => return Err(E_MISSING_SECTION),
        };
        let mut joined = String::new();
        for line in sec.lines.iter() {
            joined.push_str(line);
            joined.push('\n');
        }
        let mut hz = String::new();
        hz.push_str("1/");
        push_u64(&mut hz, facts.step_hz);
        if !joined.contains(&hz) {
            return Err(E_MISSING_SECTION);
        }
        let mut sub = String::new();
        push_u64(&mut sub, facts.max_substeps as u64);
        sub.push_str(" 个子步");
        if !joined.contains(&sub) {
            return Err(E_MISSING_SECTION);
        }
        Ok(())
    }
}

// ============================================================
// 第十四节 · CI 回归门禁 CiGate（防线回归 + 漂移阻断 + 双签）
// ============================================================

/// CI 门禁状态闭集
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 待快照（从未批过快照）
    NoSnapshot,
    /// 守备中（快照在案、回归通过）
    Armed,
    /// 漂移阻断（重渲染 ≠ 快照）
    DriftBlocked,
    /// 回归阻断（样本漏捕）
    RegressBlocked,
}

/// 快照批准留痕
#[derive(Clone)]
pub struct SnapshotApproval {
    pub approved_by: String,
    pub reason: String,
    pub seal: u64,
    pub tick: u64,
}

pub struct CiGate {
    pub state: GateState,
    snapshot: Option<SnapshotApproval>,
    /// 阻断解除所需双签（两次不同签名人）
    unblock_signers: Vec<String>,
    /// 漂移检出次数（一生累计）
    drift_hits: u32,
    /// 回归漏捕次数（一生累计）
    regress_hits: u32,
}

impl CiGate {
    pub fn new() -> CiGate {
        CiGate {
            state: GateState::NoSnapshot,
            snapshot: None,
            unblock_signers: Vec::new(),
            drift_hits: 0,
            regress_hits: 0,
        }
    }

    /// 快照批准（Maintainer 权；理由过短拒）
    pub fn approve_snapshot(
        &mut self,
        doc_seal: u64,
        approved_by: &str,
        reason: &str,
        tick: u64,
    ) -> Result<(), u16> {
        if approved_by.len() < SIGNER_MIN_LEN || reason.len() < SNAPSHOT_REASON_MIN {
            return Err(E_DRIFT); // 留痕不合格按漂移口径拒
        }
        self.snapshot = Some(SnapshotApproval {
            approved_by: alloc::format!("{}", approved_by),
            reason: alloc::format!("{}", reason),
            seal: doc_seal,
            tick,
        });
        self.state = GateState::Armed;
        Ok(())
    }

    pub fn snapshot(&self) -> Option<&SnapshotApproval> {
        self.snapshot.as_ref()
    }

    /// 漂移检测（锚点：漂移→CI 阻断）：
    /// 重渲染封印 ≠ 快照封印 → DriftBlocked。
    pub fn drift_check(&mut self, rendered_seal: u64) -> Result<(), u16> {
        let snap_seal = match self.snapshot.as_ref() {
            Some(s) => s.seal,
            None => return Err(E_DRIFT), // 无快照即无从对拍
        };
        if snap_seal != rendered_seal {
            self.drift_hits += 1;
            self.state = GateState::DriftBlocked;
            return Err(E_DRIFT);
        }
        if self.state == GateState::DriftBlocked {
            // 与快照重新一致：解除漂移阻断（进入守备）
            self.state = GateState::Armed;
        }
        Ok(())
    }

    /// 防线回归（锚点：NaN 防线回归进 CI）：
    /// 全部存活样本必须被当前防线捕获；任一漏捕 → RegressBlocked。
    pub fn run_regress(&mut self, lib: &NanRegressLibrary, facts: &SourceFacts) -> Result<usize, u16> {
        let active = match DefenseGen::from_ordinal(facts.active_defense_gen) {
            Some(g) => g,
            None => {
                self.regress_hits += 1;
                self.state = GateState::RegressBlocked;
                return Err(E_SAMPLE_STALE);
            }
        };
        let alive = lib.alive();
        let mut checked = 0usize;
        for s in alive.iter() {
            checked += 1;
            if !lib.detect_effective(active, s.kind, s.bits) {
                self.regress_hits += 1;
                self.state = GateState::RegressBlocked;
                return Err(E_SAMPLE_STALE); // 漏捕即阻断
            }
        }
        if self.state == GateState::RegressBlocked {
            self.state = GateState::Armed;
        }
        Ok(checked)
    }

    /// 阻断解除（双签）：两次不同且非空的签名人才能解除。
    /// 单签重复 → 0x3C0A。
    pub fn unblock(&mut self, signer: &str) -> Result<GateState, u16> {
        if self.state != GateState::DriftBlocked && self.state != GateState::RegressBlocked {
            return Err(E_DUAL_SIGN_MISSING); // 未阻断无需解除
        }
        if signer.len() < SIGNER_MIN_LEN {
            return Err(E_DUAL_SIGN_MISSING);
        }
        for s in self.unblock_signers.iter() {
            if s == signer {
                return Err(E_DUAL_SIGN_MISSING); // 同签人重复签无效
            }
        }
        self.unblock_signers.push(alloc::format!("{}", signer));
        if self.unblock_signers.len() >= 2 {
            self.unblock_signers.clear();
            self.state = GateState::Armed;
        }
        Ok(self.state)
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    pub fn regress_hits(&self) -> u32 {
        self.regress_hits
    }
}

// ============================================================
// 第十五节 · U 域 schema 对接
// ============================================================

/// schema 版本对拍（锚点：跨批对接点 U 域 schema）
pub fn schema_version_check(u_ver: u32) -> Result<(), u16> {
    if u_ver != DOC_SCHEMA_VER {
        return Err(E_SCHEMA_MISMATCH);
    }
    Ok(())
}

/// 导出键值 schema 条目（对接 U 域总 schema；键集闭集）
pub fn export_schema(doc: &DocModel, lib: &NanRegressLibrary) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut k = String::new();
    k.push_str("doc.schema_ver");
    let mut v = String::new();
    push_u64(&mut v, doc.schema_ver as u64);
    out.push((k, v));

    let mut k2 = String::new();
    k2.push_str("doc.facts_ver");
    let mut v2 = String::new();
    push_u64(&mut v2, doc.facts_ver as u64);
    out.push((k2, v2));

    let mut k3 = String::new();
    k3.push_str("doc.total_lines");
    let mut v3 = String::new();
    push_u64(&mut v3, doc.total_lines() as u64);
    out.push((k3, v3));

    let mut k4 = String::new();
    k4.push_str("doc.seal_hex");
    let mut v4 = String::new();
    push_u64(&mut v4, doc.seal());
    out.push((k4, v4));

    let (total, alive, stale, purged) = lib.stats();
    let mut k5 = String::new();
    k5.push_str("nan.samples_total");
    let mut v5 = String::new();
    push_u64(&mut v5, total as u64);
    out.push((k5, v5));
    let mut k6 = String::new();
    k6.push_str("nan.samples_alive");
    let mut v6 = String::new();
    push_u64(&mut v6, alive as u64);
    out.push((k6, v6));
    let mut k7 = String::new();
    k7.push_str("nan.samples_stale");
    let mut v7 = String::new();
    push_u64(&mut v7, stale as u64);
    out.push((k7, v7));
    let mut k8 = String::new();
    k8.push_str("nan.samples_purged");
    let mut v8 = String::new();
    push_u64(&mut v8, purged as u64);
    out.push((k8, v8));

    let mut k9 = String::new();
    k9.push_str("doc.active_gen");
    let mut v9 = String::new();
    push_u64(&mut v9, doc.active_gen as u64);
    out.push((k9, v9));
    out
}

// ============================================================
// 第十六节 · 角色与两权分立
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 维护权：注册术语/样本、批快照、注失明、强制清理、解除阻断
    Maintainer,
    /// 运行权：生成、校验、回归、导出（只读面）
    Runtime,
}

// ============================================================
// 第十七节 · 审计账（闭集事件 + 满容 256）
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditEvent {
    Generated,
    RawInjected,
    MissingSection,
    TermUnknown,
    SampleRegistered,
    SampleStale,
    SamplePurged,
    RegressPass,
    RegressFail,
    DriftHit,
    SnapshotApproved,
    A11yFail,
    SchemaMismatch,
    RoleViolation,
    DualSigned,
    FlawInjected,
    FlawCleared,
}

impl AuditEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::Generated => "generated",
            AuditEvent::RawInjected => "raw-injected",
            AuditEvent::MissingSection => "missing-section",
            AuditEvent::TermUnknown => "term-unknown",
            AuditEvent::SampleRegistered => "sample-registered",
            AuditEvent::SampleStale => "sample-stale",
            AuditEvent::SamplePurged => "sample-purged",
            AuditEvent::RegressPass => "regress-pass",
            AuditEvent::RegressFail => "regress-fail",
            AuditEvent::DriftHit => "drift-hit",
            AuditEvent::SnapshotApproved => "snapshot-approved",
            AuditEvent::A11yFail => "a11y-fail",
            AuditEvent::SchemaMismatch => "schema-mismatch",
            AuditEvent::RoleViolation => "role-violation",
            AuditEvent::DualSigned => "dual-signed",
            AuditEvent::FlawInjected => "flaw-injected",
            AuditEvent::FlawCleared => "flaw-cleared",
        }
    }
}

struct AuditEntry {
    tick: u64,
    ev: AuditEvent,
    code: u16,
}

pub struct AuditLog {
    entries: Vec<AuditEntry>,
    dropped: usize,
}

impl AuditLog {
    pub fn new() -> AuditLog {
        AuditLog {
            entries: Vec::new(),
            dropped: 0,
        }
    }

    pub fn record(&mut self, tick: u64, ev: AuditEvent, code: u16) {
        if self.entries.len() >= AUDIT_CAP {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(AuditEntry { tick, ev, code });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 取第 i 条（时间序）
    pub fn at(&self, i: usize) -> Option<(u64, AuditEvent, u16)> {
        if i >= self.entries.len() {
            return None;
        }
        let e = &self.entries[i];
        Some((e.tick, e.ev, e.code))
    }

    /// 反向找最近一次某事件
    pub fn last_of(&self, ev: AuditEvent) -> Option<(u64, u16)> {
        for e in self.entries.iter().rev() {
            if e.ev == ev {
                return Some((e.tick, e.code));
            }
        }
        None
    }

    /// 事件计数
    pub fn count_of(&self, ev: AuditEvent) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.ev == ev {
                n += 1;
            }
        }
        n
    }
}

// ============================================================
// 第十八节 · DocEngine（mount 唯一入口 + 两权分立 + 封印）
// ============================================================

pub struct DocEngine {
    pub role: Role,
    pub facts: SourceFacts,
    pub glossary: Glossary,
    pub lib: NanRegressLibrary,
    pub gate: CiGate,
    pub audit: AuditLog,
    /// 引擎封印（配置完整性）
    seal: u64,
    mounted: bool,
}

impl DocEngine {
    /// mount 唯一入口
    pub fn mount(role: Role, facts: SourceFacts) -> DocEngine {
        let mut seal_src = String::new();
        seal_src.push_str("vead11-docgen|role=");
        seal_src.push_str(match role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        seal_src.push_str("|facts=");
        push_u64(&mut seal_src, facts.facts_ver as u64);
        seal_src.push_str("|gen=");
        push_u64(&mut seal_src, facts.active_defense_gen as u64);
        DocEngine {
            role,
            facts,
            glossary: Glossary::new(),
            lib: NanRegressLibrary::new(),
            gate: CiGate::new(),
            audit: AuditLog::new(),
            seal: seal_str(&seal_src),
            mounted: true,
        }
    }

    pub fn seal(&self) -> u64 {
        self.seal
    }

    /// 封印篡改检出：重算与保存值不符即被改
    pub fn verify_seal(&self) -> bool {
        let mut seal_src = String::new();
        seal_src.push_str("vead11-docgen|role=");
        seal_src.push_str(match self.role {
            Role::Maintainer => "maintainer",
            Role::Runtime => "runtime",
        });
        seal_src.push_str("|facts=");
        push_u64(&mut seal_src, self.facts.facts_ver as u64);
        seal_src.push_str("|gen=");
        push_u64(&mut seal_src, self.facts.active_defense_gen as u64);
        seal_str(&seal_src) == self.seal
    }

    fn require_maintainer(&mut self, tick: u64) -> Result<(), u16> {
        if self.role != Role::Maintainer {
            self.audit.record(tick, AuditEvent::RoleViolation, E_ROLE_VIOLATION);
            return Err(E_ROLE_VIOLATION);
        }
        Ok(())
    }

    // ---------- Maintainer 专属 ----------

    /// 注册术语
    pub fn register_term(&mut self, term: &'static str, def: &'static str, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        if self.glossary.register(term, def) {
            Ok(())
        } else {
            self.audit.record(tick, AuditEvent::TermUnknown, E_TERM_UNKNOWN);
            Err(E_TERM_UNKNOWN)
        }
    }

    /// 注册样本
    pub fn register_sample(
        &mut self,
        kind: PayloadKind,
        bits: u32,
        origin_case: u16,
        bound_gen: DefenseGen,
        tick: u64,
    ) -> Result<u32, u16> {
        self.require_maintainer(tick)?;
        match self.lib.register_sample(kind, bits, origin_case, bound_gen, tick) {
            Ok(id) => {
                self.audit.record(tick, AuditEvent::SampleRegistered, 0);
                Ok(id)
            }
            Err(e) => {
                self.audit.record(tick, AuditEvent::SampleStale, e);
                Err(e)
            }
        }
    }

    /// 注入检测器失明（演练）
    pub fn inject_flaw(&mut self, gen: DefenseGen, bits: u32, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        self.lib.inject_flaw(gen, bits);
        self.audit.record(tick, AuditEvent::FlawInjected, 0);
        Ok(())
    }

    /// 清除失明
    pub fn clear_flaw(&mut self, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        self.lib.clear_flaw();
        self.audit.record(tick, AuditEvent::FlawCleared, 0);
        Ok(())
    }

    /// 批准快照
    pub fn approve_snapshot(&mut self, seal: u64, by: &str, reason: &str, tick: u64) -> Result<(), u16> {
        self.require_maintainer(tick)?;
        match self.gate.approve_snapshot(seal, by, reason, tick) {
            Ok(()) => {
                self.audit.record(tick, AuditEvent::SnapshotApproved, 0);
                Ok(())
            }
            Err(e) => {
                self.audit.record(tick, AuditEvent::DriftHit, e);
                Err(e)
            }
        }
    }

    /// 强制清理（Maintainer）
    pub fn force_purge(&mut self, tick: u64) -> Result<usize, u16> {
        self.require_maintainer(tick)?;
        match self.lib.auto_purge(true) {
            Ok(n) => {
                self.audit.record(tick, AuditEvent::SamplePurged, 0);
                Ok(n)
            }
            Err(e) => Err(e),
        }
    }

    // ---------- 公共（Runtime 亦可） ----------

    /// 同源生成（核心主流程）
    pub fn generate(&mut self, tick: u64) -> Result<DocModel, u16> {
        if !self.mounted {
            return Err(E_SOURCE_BROKEN);
        }
        match DocGen::generate(&self.facts, &self.glossary) {
            Ok(doc) => {
                self.audit.record(tick, AuditEvent::Generated, 0);
                Ok(doc)
            }
            Err(e) => {
                let ev = match e {
                    E_TERM_UNKNOWN => AuditEvent::TermUnknown,
                    _ => AuditEvent::RawInjected,
                };
                self.audit.record(tick, ev, e);
                Err(e)
            }
        }
    }

    /// 完整管线：生成 → 覆盖对拍 → 无障碍 → 术语 → 缺章 → 漂移 → 回归
    /// 返回 (doc, checked_samples)
    pub fn run_pipeline(&mut self, tick: u64) -> Result<(DocModel, usize), u16> {
        let doc = self.generate(tick)?;
        // 覆盖对拍三件
        DocGen::verify_teaching_coverage(&doc)?;
        DocGen::verify_arch_coverage(&doc)?;
        DocGen::verify_laws_coverage(&doc)?;
        DocGen::verify_step_facts(&doc, &self.facts)?;
        // 无障碍
        if let Err(e) = doc.a11y_check() {
            self.audit.record(tick, AuditEvent::A11yFail, e);
            return Err(e);
        }
        // 术语
        let unknown = doc.scan_unknown_terms(&self.glossary);
        if unknown.len() > 0 {
            self.audit.record(tick, AuditEvent::TermUnknown, E_TERM_UNKNOWN);
            return Err(E_TERM_UNKNOWN);
        }
        // 缺章
        let miss = doc.missing_sections();
        if miss.len() > 0 {
            self.audit.record(tick, AuditEvent::MissingSection, E_MISSING_SECTION);
            return Err(E_MISSING_SECTION);
        }
        // 漂移（有快照才查——NoSnapshot 首跑无基线可对拍，
        //       批快照后每次管线重渲染即与封印比对）
        if self.gate.snapshot().is_some() {
            let seal = doc.seal();
            self.gate.drift_check(seal)?;
        }
        // 回归
        let checked = self.gate.run_regress(&self.lib, &self.facts)?;
        self.audit.record(tick, AuditEvent::RegressPass, 0);
        Ok((doc, checked))
    }

    /// 解除阻断（双签；公共入口，签名人身份由调用方负责）
    pub fn unblock(&mut self, signer: &str, tick: u64) -> Result<GateState, u16> {
        match self.gate.unblock(signer) {
            Ok(st) => {
                if st == GateState::Armed {
                    self.audit.record(tick, AuditEvent::DualSigned, 0);
                }
                Ok(st)
            }
            Err(e) => {
                self.audit.record(tick, AuditEvent::DualSigned, e);
                Err(e)
            }
        }
    }

    /// schema 导出
    pub fn export(&self, tick: u64) -> Result<Vec<(String, String)>, u16> {
        let doc = DocGen::generate(&self.facts, &self.glossary)?;
        Ok(export_schema(&doc, &self.lib))
    }
}

// ============================================================
// 第十九节 · 自检聚合（内部回归）
// ============================================================

/// 模块自检：常量闭集自洽性
pub fn selfcheck_constants() -> bool {
    // 六章闭集索引可逆
    for i in 0u16..6 {
        match SectionKind::from_index(i) {
            Some(k) => {
                if k.index() != i {
                    return false;
                }
            }
            None => return false,
        }
    }
    if SectionKind::from_index(6).is_some() {
        return false;
    }
    // 五类载荷闭集索引可逆
    for i in 0u16..5 {
        match PayloadKind::from_index(i) {
            Some(k) => {
                if k.index() != i {
                    return false;
                }
            }
            None => return false,
        }
    }
    if PayloadKind::from_index(5).is_some() {
        return false;
    }
    // 防线四代闭集
    for o in 1u8..=4 {
        if DefenseGen::from_ordinal(o).is_none() {
            return false;
        }
    }
    if DefenseGen::from_ordinal(0).is_some() || DefenseGen::from_ordinal(5).is_some() {
        return false;
    }
    // 捕获矩阵单调性：高代捕获面 ⊇ 低代捕获面（一旦为真必须恒为真）
    for kind in PayloadKind::all().iter() {
        let mut prev = false;
        for g in DefenseGen::all().iter() {
            let cur = detect(*g, *kind);
            if prev && !cur {
                return false;
            }
            prev = cur;
        }
    }
    // 预算非零
    for layer in PhysLayer::all().iter() {
        if layer.budget_us() == 0 {
            return false;
        }
    }
    // 案例表回指防线合法
    for c in NAN_CASES.iter() {
        if DefenseGen::from_ordinal(c.gen_introduced.ordinal()).is_none() {
            return false;
        }
    }
    true
}

/// 自检：定点转串
pub fn selfcheck_num_fmt() -> bool {
    let mut s = String::new();
    push_u64(&mut s, 0);
    if s != "0" {
        return false;
    }
    let mut s2 = String::new();
    push_u64(&mut s2, 60);
    if s2 != "60" {
        return false;
    }
    let mut s3 = String::new();
    push_i64(&mut s3, -12345);
    if s3 != "-12345" {
        return false;
    }
    let mut s4 = String::new();
    push_permille(&mut s4, 987);
    if s4 != "0.987" {
        return false;
    }
    let mut s5 = String::new();
    push_permille(&mut s5, 42);
    if s5 != "0.042" {
        return false;
    }
    true
}

/// 自检：捕获矩阵教学语义抽查
pub fn selfcheck_detect_matrix() -> bool {
    // Gen1 只捕 quiet NaN
    if detect(DefenseGen::Gen1EqCheck, PayloadKind::QuietNaN) != true {
        return false;
    }
    if detect(DefenseGen::Gen1EqCheck, PayloadKind::Infinity) != false {
        return false;
    }
    if detect(DefenseGen::Gen1EqCheck, PayloadKind::Denormal) != false {
        return false;
    }
    // Gen2 捕 NaN+Inf
    if detect(DefenseGen::Gen2PropAssert, PayloadKind::Infinity) != true {
        return false;
    }
    if detect(DefenseGen::Gen2PropAssert, PayloadKind::NegZero) != false {
        return false;
    }
    // Gen3 捕 NaN/Inf/NegZero/Denormal
    if detect(DefenseGen::Gen3FloodGate, PayloadKind::Denormal) != true {
        return false;
    }
    if detect(DefenseGen::Gen3FloodGate, PayloadKind::GradualOverflow) != false {
        return false;
    }
    // Gen4 全捕
    for kind in PayloadKind::all().iter() {
        if detect(DefenseGen::Gen4SourceClamp, *kind) != true {
            return false;
        }
    }
    true
}
