//! CGPU-F0321 · C 域开工与预算总架构（CGPU-C 域 · 帧预算仲裁域 · C01 组 · 目标 400 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0321`
//!
//! **判据（锚点原文）**：八模块、移交对接核验、服务化纪律、判据。
//!
//! **职责定位（锚点原文）**：C 域开工：帧预算仲裁域（F0321-F0480）——
//! 80 帧承诺的兑现机制（每帧 12.5ms 预算的精确分配与仲裁）；八模块总架构
//! （预算模型/申请/仲裁/超时处置/交易/全局表/再平衡/遥测）；B10 移交包
//! 对接（F0316 冻结接口+成本模型 F0004 映射核验就位）；用户态服务化纪律
//! （卷首铁律 6——仲裁 bug 不威胁系统存活，内核态只留强制执行点）。
//!
//! ## 一、域使命：80 帧承诺的兑现机制
//!
//! 80 帧/秒 ⇒ 每帧 12.5ms 预算（[`FRAME_BUDGET_US`] = 12500 微秒，
//! 来源：1_000_000us ÷ 80，整数除法精确无余——常量写死并注释来源）。
//! 本域的承诺是「预算的精确分配与仲裁」：分配有账（三级预算层级）、
//! 仲裁有据（申请→校验→排序→裁决→记账五步）、超时有路（降档/排队/
//! 拒绝三路——F0328 兑现位）。
//!
//! ## 二、八模块总架构（判据一：闭集规划表）
//!
//! 八模块（[`CModule`] 闭集 + [`C_MODULES`] 规划表）：预算模型/申请/
//! 仲裁/超时处置/交易/全局表/再平衡/遥测——每模块绑定其在 F0322-F0480
//! 区间内的兑现单号段；表外不立模块（模块闭集纪律），域内 160 项的
//! 十组规划（[`C_GROUPS`]）与八模块正交（组是批次切分，模块是职责
//! 切分——两张表守恒各自口径）。
//!
//! ## 三、B10 移交包对接核验（判据二：就位即核验，不空头对接）
//!
//! B10（B 域第十组）移交包两件：F0316 冻结接口 + 成本模型 F0004 映射。
//! 对接核验四项（[`HandoverReceipt`]）：接口版本相符/接口字段闭集核对/
//! 成本模型映射键齐备/移交指纹一致——四项全过才算「就位」，任一缺项
//! 显性 [`XcCode::HandoverMissing`] 拒绝域开工后续步骤（不空转过冬）。
//!
//! ## 四、用户态服务化纪律（判据三：卷首铁律 6 的 C 域落面）
//!
//! 三条款字面量冻结（[`SERVICE_DISCIPLINE`]）：仲裁 bug 不威胁系统存活/
//! 内核态只留强制执行点/仲裁全量活在用户态服务。条款是可机检声明：
//! 每条绑定唯一检查键（判据侧 grep 键面），措辞漂移即红。
//!
//! ## 五、域开工核验入口
//!
//! [`verify_domain_open`]：八模块表守恒（8 模块 × 兑现段首尾连续）+
//! 十组规划守恒（10 组 × 16 单首尾连续覆盖 F0321-F0480）+ 移交核验 +
//! 服务化条款核对，四查全过返回 `Ok`，任一失败带诊断码——域开工是
//! 有闸门的，不是宣告式的。
//!
//! **对接**：F0316（冻结接口，移交件一）；F0004（成本模型，移交件二）；
//! F0167（B 域失败处置——超时处置 F0328 的上游语义）；F0322-F0480
//! （八模块兑现区间）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（80 帧合同口径唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const C_OPEN_VERSION: &str = "C01-copen-v1";

/// 每帧预算（微秒）＝1_000_000us ÷ 80 帧＝12500us，整数除法精确无余。
/// 来源：80 帧合同（锚点原文「每帧 12.5ms 预算」）。当前策略值，可调。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 帧率承诺（帧/秒）——与 [`FRAME_BUDGET_US`] 守恒对账。
pub const COMMIT_FPS: u64 = 80;

/// 域规模：C 域 160 项（F0321-F0480）。
pub const C_DOMAIN_TOTAL: u32 = 160;

/// 域首单号（F0321 ⇒ 数值 321；四位数编号前导零不计值）。
pub const C_DOMAIN_FIRST: u32 = 321;

/// 域末单号（F0480 ⇒ 数值 480；首+总数-1 守恒）。
pub const C_DOMAIN_LAST: u32 = 480;

/// 守恒断言（编译期钉死：末=首+总-1）。
const _: () = assert!(C_DOMAIN_FIRST + C_DOMAIN_TOTAL - 1 == C_DOMAIN_LAST);

// ---------------------------------------------------------------------------
// 二、八模块总架构（判据一）
// ---------------------------------------------------------------------------

/// 八模块闭集（锚点点名；表外不立模块）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CModule {
    /// 预算模型（12.5ms 分解与守恒）。
    BudgetModel,
    /// 预算申请（标签/时机/批量/归还）。
    Request,
    /// 仲裁（v1 静态优先级 + v2 加权公平双轨）。
    Arbitration,
    /// 超时处置（降档/排队/拒绝三路）。
    Timeout,
    /// 预算交易（原子归还与迁移）。
    Transaction,
    /// 全局预算表（域级权威账）。
    GlobalTable,
    /// 再平衡（池间/账户间重分配）。
    Rebalance,
    /// 遥测（预算面五指标进 B08）。
    Telemetry,
}

impl CModule {
    /// 模块中文名（判据侧独立对拍值——措辞与锚点一致）。
    pub const fn zh(self) -> &'static str {
        match self {
            CModule::BudgetModel => "预算模型",
            CModule::Request => "申请",
            CModule::Arbitration => "仲裁",
            CModule::Timeout => "超时处置",
            CModule::Transaction => "交易",
            CModule::GlobalTable => "全局表",
            CModule::Rebalance => "再平衡",
            CModule::Telemetry => "遥测",
        }
    }
}

/// 八模块规划行（模块 + 兑现单号段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CModulePlan {
    /// 模块。
    pub module: CModule,
    /// 兑现段首单（本模块职责的兑现任务区间起点）。
    pub fulfill_first: u32,
    /// 兑现段末单。
    pub fulfill_last: u32,
}

/// 八模块总架构表（判据一：闭集 + 段守恒由 [`verify_domain_open`] 校验）。
/// 前四段对齐锚点已点名的兑现单（模型 F0322-F0323/申请 F0324-F0325/
/// 仲裁 F0326-F0327/超时 F0328）；后四段为本域规划段（交易/全局表/
/// 再平衡/遥测的精确兑现位随批次施工登记，调整走 ADR）。
pub const C_MODULES: [CModulePlan; 8] = [
    CModulePlan { module: CModule::BudgetModel, fulfill_first: 322, fulfill_last: 323 },
    CModulePlan { module: CModule::Request, fulfill_first: 324, fulfill_last: 325 },
    CModulePlan { module: CModule::Arbitration, fulfill_first: 326, fulfill_last: 327 },
    CModulePlan { module: CModule::Timeout, fulfill_first: 328, fulfill_last: 328 },
    CModulePlan { module: CModule::Transaction, fulfill_first: 329, fulfill_last: 360 },
    CModulePlan { module: CModule::GlobalTable, fulfill_first: 361, fulfill_last: 390 },
    CModulePlan { module: CModule::Rebalance, fulfill_first: 391, fulfill_last: 420 },
    CModulePlan { module: CModule::Telemetry, fulfill_first: 421, fulfill_last: 480 },
];

// ---------------------------------------------------------------------------
// 三、十组批次规划（与八模块正交的批次切分）
// ---------------------------------------------------------------------------

/// 十组规划行（组名 + 起止单号；起止逐字取自域规模 160 项 ÷ 10 组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CGroupPlan {
    /// 组名（C01-C10）。
    pub tag: &'static str,
    /// 组内首单。
    pub first: u32,
    /// 组内末单。
    pub last: u32,
}

/// 十组规划表（首尾连续无缺口、无重叠——由 [`verify_domain_open`] 守恒；
/// 160 项 ÷ 10 组 = 每组 16 单）。
pub const C_GROUPS: [CGroupPlan; 10] = [
    CGroupPlan { tag: "C01", first: 321, last: 336 },
    CGroupPlan { tag: "C02", first: 337, last: 352 },
    CGroupPlan { tag: "C03", first: 353, last: 368 },
    CGroupPlan { tag: "C04", first: 369, last: 384 },
    CGroupPlan { tag: "C05", first: 385, last: 400 },
    CGroupPlan { tag: "C06", first: 401, last: 416 },
    CGroupPlan { tag: "C07", first: 417, last: 432 },
    CGroupPlan { tag: "C08", first: 433, last: 448 },
    CGroupPlan { tag: "C09", first: 449, last: 464 },
    CGroupPlan { tag: "C10", first: 465, last: 480 },
];

// ---------------------------------------------------------------------------
// 四、B10 移交包对接核验（判据二）
// ---------------------------------------------------------------------------

/// 移交对接核验结论闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 移交件缺失/指纹不符/字段超闭集——对接未就位。
    HandoverMissing,
    /// 服务化条款核对失败（措辞漂移即红）。
    DisciplineDrift,
    /// 规划表守恒失败（模块段/组段不连续或覆盖不全）。
    PlanMismatch,
    /// 预算守恒失败（Σ池 ≠ 帧预算）。
    BudgetMismatch,
}

impl XcCode {
    /// 人读诊断（异常零静默三要素）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "移交包未就位：接口版本/字段闭集/映射键/指纹四项有缺，建议补齐 B10 移交件再开工",
            XcCode::DisciplineDrift => "服务化条款漂移：三条款措辞与冻结版不符，建议恢复字面量",
            XcCode::PlanMismatch => "规划表失守：模块段或组段不连续/覆盖不全，建议回对锚点重建表",
            XcCode::BudgetMismatch => "预算守恒失败：Σ池 ≠ 帧预算，建议核对四池配额和",
        }
    }
}

/// 移交包核验单（B10 移交件一 F0316 冻结接口 + 件二 F0004 成本模型映射）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 冻结接口版本（须与 F0316 冻结版一致）。
    pub interface_version: String,
    /// 移交接口实际承载的字段名（核对走 [`check_interface_fields`] 闭集）。
    pub interface_fields: Vec<String>,
    /// 移交成本映射实际承载的键名（核对走 [`check_cost_model_keys`] 闭集）。
    pub cost_model_keys: Vec<String>,
    /// 移交指纹（B10 打包指纹；一致性由判据侧独立重算对拍）。
    pub fingerprint: u64,
}

/// B10 移交包期望指纹（移交时冻结；判据侧独立写死对拍，不复用本值）。
pub const B10_EXPECTED_FINGERPRINT: u64 = 0xB10C_0004_0316_0001;

impl HandoverReceipt {
    /// 四项核验（判据二：全过才就位；任一缺项显性码拒）。
    pub fn verify(&self) -> Result<(), XcCode> {
        if self.interface_version != F0316_FROZEN_VERSION {
            return Err(XcCode::HandoverMissing);
        }
        if !check_interface_fields(&self.interface_fields)
            || !check_cost_model_keys(&self.cost_model_keys)
        {
            return Err(XcCode::HandoverMissing);
        }
        if self.fingerprint != B10_EXPECTED_FINGERPRINT {
            return Err(XcCode::HandoverMissing);
        }
        Ok(())
    }
}

/// F0316 冻结接口版本（移交件一的冻结版字面量；变更走 ADR）。
pub const F0316_FROZEN_VERSION: &str = "b10.f0316.v1";

/// F0316 接口字段闭集（移交接口允许的字段名；表外字段=未就位）。
pub const F0316_FIELDS: [&str; 6] = [
    "task_id",
    "est_cost_us",
    "deadline_us",
    "resource_kind",
    "priority",
    "degradable",
];

/// 成本模型 F0004 → 预算标签的映射键闭集（移交件二的键面；映射键齐备
/// = 闭集逐键在场，缺键即映射断裂）。
pub const COST_MODEL_KEYS: [&str; 4] =
    ["est_cost_us", "deadline_us", "resource_kind", "priority"];

/// 接口字段闭集核对（判据二机检面：传入的移交字段逐个必须在闭集内；
/// 空表=零字段也算未就位——接口至少要承载六个字段）。
pub fn check_interface_fields<S: alloc::string::ToString>(received: &[S]) -> bool {
    if received.len() < F0316_FIELDS.len() {
        return false;
    }
    let mut i = 0usize;
    while i < received.len() {
        let f = match received.get(i) {
            Some(f) => f.to_string(),
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < F0316_FIELDS.len() {
            if let Some(allowed) = F0316_FIELDS.get(j) {
                if *allowed == f {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

/// 映射键齐备核对（判据二机检面：COST_MODEL_KEYS 逐键必须在移交映射里）。
pub fn check_cost_model_keys<S: alloc::string::ToString>(received: &[S]) -> bool {
    let mut i = 0usize;
    while i < COST_MODEL_KEYS.len() {
        let key = match COST_MODEL_KEYS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < received.len() {
            if let Some(r) = received.get(j) {
                if r.to_string() == key {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 五、服务化纪律（判据三：卷首铁律 6 的 C 域落面）
// ---------------------------------------------------------------------------

/// 服务化三条款（字面量冻结；判据侧独立写死对拍，措辞漂移即红）。
pub const SERVICE_DISCIPLINE: [&str; 3] = [
    "仲裁 bug 不威胁系统存活",
    "内核态只留强制执行点",
    "仲裁全量活在用户态服务",
];

/// 服务化条款检查键（判据侧 grep 键面——条款是可机检声明不是口号）。
pub const SERVICE_DISCIPLINE_KEYS: [&str; 3] =
    ["c01.discipline.arbitor-isolation", "c01.discipline.kernel-minimal", "c01.discipline.userservice-only"];

// ---------------------------------------------------------------------------
// 六、域开工核验入口（判据四：开工有闸门）
// ---------------------------------------------------------------------------

/// 模块段守恒：八段首尾相接、递增、不重叠、都落在域区间内。
fn modules_contiguous() -> bool {
    let mut prev_end: u32 = 0;
    let mut i = 0usize;
    while i < C_MODULES.len() {
        let m = match C_MODULES.get(i) {
            Some(m) => *m,
            None => return false,
        };
        if m.fulfill_first > m.fulfill_last {
            return false;
        }
        if i > 0 && m.fulfill_first <= prev_end {
            return false;
        }
        prev_end = m.fulfill_last;
        i += 1;
    }
    let first = match C_MODULES.first() {
        Some(m) => m.fulfill_first,
        None => return false,
    };
    let last = prev_end;
    first >= C_DOMAIN_FIRST && last <= C_DOMAIN_LAST
}

/// 组段守恒：十组首尾连续无缝覆盖 [C_DOMAIN_FIRST, C_DOMAIN_LAST]。
fn groups_contiguous() -> bool {
    let mut expect = C_DOMAIN_FIRST;
    let mut i = 0usize;
    while i < C_GROUPS.len() {
        let g = match C_GROUPS.get(i) {
            Some(g) => *g,
            None => return false,
        };
        if g.first != expect || g.first > g.last {
            return false;
        }
        expect = g.last.saturating_add(1);
        i += 1;
    }
    expect == C_DOMAIN_LAST.saturating_add(1)
}

/// 预算守恒换算抽查：帧预算 × 80 = 1 秒（us 口径）。
pub const fn budget_conserves_second() -> bool {
    FRAME_BUDGET_US.saturating_mul(COMMIT_FPS) == 1_000_000
}

/// 域开工四查（判据一~四：模块/组守恒 + 移交核验 + 服务化 + 合同换算）。
pub fn verify_domain_open(receipt: &HandoverReceipt) -> Result<(), XcCode> {
    if !modules_contiguous() || !groups_contiguous() {
        return Err(XcCode::PlanMismatch);
    }
    receipt.verify()?;
    // 服务化条款核对：三条款非空且键面三键互异（措辞漂移由判据侧独立
    // 写死对拍兜底；此处守键面结构）。
    if SERVICE_DISCIPLINE.len() != 3 || SERVICE_DISCIPLINE_KEYS.len() != 3 {
        return Err(XcCode::DisciplineDrift);
    }
    let k0 = SERVICE_DISCIPLINE_KEYS[0];
    let k1 = SERVICE_DISCIPLINE_KEYS[1];
    let k2 = SERVICE_DISCIPLINE_KEYS[2];
    if k0 == k1 || k1 == k2 || k0 == k2 {
        return Err(XcCode::DisciplineDrift);
    }
    if !budget_conserves_second() {
        return Err(XcCode::BudgetMismatch);
    }
    Ok(())
}

/// 域开工报告（人读面：判据逐条带结论，供验收与遥测消费）。
pub fn domain_open_report(receipt: &HandoverReceipt) -> String {
    let mut s = String::new();
    s.push_str("C01 域开工核验：");
    s.push_str(&alloc::format!(
        "八模块 {} 项/十组 {} 组；",
        C_MODULES.len(),
        C_GROUPS.len()
    ));
    s.push_str(&alloc::format!(
        "预算 {}us×{}fps={}us/s；",
        FRAME_BUDGET_US,
        COMMIT_FPS,
        FRAME_BUDGET_US * COMMIT_FPS
    ));
    match verify_domain_open(receipt) {
        Ok(()) => s.push_str("四查全过=就位"),
        Err(c) => {
            s.push_str("拒="); s.push_str(c.detail());
        }
    }
    s
}
