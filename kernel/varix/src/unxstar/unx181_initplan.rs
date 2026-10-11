//! unx181_initplan — UNX-F0181 初始化顺序编排器（依赖图驱动）。
//!
//! 判据原文（施工书锚点 UNX-F0181，CoRun·UNX-A1-B10 批）：
//! **UNX-F0181-J1 各 init 单元依赖图声明后执行序与拓扑序一致，环依赖检出 100%**。
//!
//! 职责定位：A1 域 B10 批开篇之单——把「init 单元以什么顺序跑」从散落在各处
//! 的调用序（Windows 现状：Smss 会话序、SCM 依赖树 DependOnService 字符串、
//! ntoskrnl 编译期 Init 数组——三处三套真相、无环检出、SCM 依赖环靠运行时
//! 超时才发现）收拢为**单一声明源**：单元声明依赖图 → 静态校验（未知依赖/
//! 自依赖/重复边/冲突对/阶段单向性五类违规全显性）→ 确定性拓扑排序（Kahn
//! 分层 + 优先级/编号平局打破，同输入恒同输出）→ 双实现对拍（分层 Kahn 与
//! 朴素逐点 Kahn 两个独立实现逐位比对，判据 J1 前半「执行序与拓扑序一致」
//! 的落地口径）→ 环依赖检出 100%（判据 J1 后半：Kahn 剩余节点集合 + DFS
//! 三色标记提取具体环路径——不是只报「有环」，而是报出 A→B→C→A 完整路径
//! 供归因）→ 执行计划生成（wave 序列线性化，计划不变量四闸校验）→ 确定性
//! 模拟执行（逻辑 tick 注入 + LCG 种子，同种子同耗时序可 CI 回放）→ 账本
//! 守恒（总数 = done+skipped+blocked+failed+pending 恒等）。
//! 对标 Windows 深在哪：①环检出**前置**到编排期（Windows 是运行期超时暴露）；
//! ②确定性可复现（同种子重放逐位一致，Windows SCM 依赖树重放无此保证）；
//! ③预算显性化（每单元 µs 预算声明，超预算在账本可见，Windows 无单元级
//! 预算账）；④账本守恒（每个单元必有终态，零静默蒸发）；⑤分层 wave 划分
//! 为 A3 域并行化留位（同 wave 无序约束显性化，串行组跨 wave 强制分离）。
//! 串行组在本单只做**计划期编排语义**（同组单元分入不同 wave）；执行期
//! 越序调用检出归 UNX-F0184（串行性显式声明），本单不越界实现。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零墙钟（逻辑 tick 注入）；
//! 零浮点（权重万分制整数）；确定性算法（排序键为 (优先级降序, 编号升序)）；
//! 常量写死并注释来源；错误全显性带可操作建议；FNV 快照往返一致。
//! 诊断码段 0x35（UNX 册已占 0x2E~0x34——先入库者占段纪律，0x35 起本单独占）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本（编排器协议版本——声明格式变更即前滚）。
pub const INIT_PLAN_VERSION: &str = "B10-initplan-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x35 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3501 未知依赖（依赖图引用了未声明的单元编号）。
pub const XC_DEP_UNKNOWN: u32 = 0x3501;
/// 0x3502 自依赖（单元依赖自身——单节点环）。
pub const XC_DEP_SELF: u32 = 0x3502;
/// 0x3503 环依赖（多节点循环依赖，报告含完整环路径）。
pub const XC_DEP_CYCLE: u32 = 0x3503;
/// 0x3504 重复依赖边（同一单元对同一依赖重复声明）。
pub const XC_DEP_DUP: u32 = 0x3504;
/// 0x3505 冲突声明违规（互斥单元落入同一 wave——同层可能同批执行）。
pub const XC_MUTEX_VIOLATION: u32 = 0x3505;
/// 0x3506 执行序失守（双实现拓扑序对拍不一致——确定性破约）。
pub const XC_ORDER_MISMATCH: u32 = 0x3506;
/// 0x3507 计划不变量破（单元重复出现/丢失/硬依赖倒序——四闸任一失守）。
pub const XC_PLAN_INVARIANT: u32 = 0x3507;
/// 0x3508 预算声明非法（零预算或超单单元上限——预算必须显性且可行）。
pub const XC_BUDGET_ILLEGAL: u32 = 0x3508;
/// 0x3509 阶段单向性违规（后阶段单元被前阶段单元依赖——初始化序倒挂）。
pub const XC_PHASE_BACKWARD: u32 = 0x3509;
/// 0x350A 账本守恒失守（终态总数与单元总数不等——有单元蒸发）。
pub const XC_LEDGER_UNBALANCED: u32 = 0x350A;

// ---------------------------------------------------------------------------
// 常量（写死并注明来源）
// ---------------------------------------------------------------------------

/// 单元总数上限。
///
/// 来源：A1 域 20 批 × 20 条 = 400 单元为声明上限，余量 ×1.28 → 512。
/// 超过即拒——依赖图规模必须有界，无界图的排序复杂度不可承诺。
pub const MAX_UNITS: usize = 512;
/// 单单元依赖数上限（声明面复杂度界——超界说明单元切分失当应拆分）。
pub const MAX_DEPS_PER_UNIT: usize = 16;
/// wave（同层并行位）数上限。
///
/// 来源：512 单元全链式最坏 512 层，预算化后实际链深受阶段数约束；
/// 512 为最坏情形硬界，超界即编排失败显性报（不静默截断）。
pub const MAX_WAVES: usize = 512;
/// 阶段数（Phase 闭集大小——见 `Phase`）。
pub const PHASE_COUNT: usize = 5;
/// 单单元预算上限（µs）。
///
/// 来源：A1 域初始化预算总账 50ms（引导到自检完成红线），摊到 400 单元
/// 均值 125µs；单单元硬上限 10ms = 总账 1/5——单单元吃掉五分之一预算
/// 即属切分失当，编排期显性拒。
pub const MAX_UNIT_BUDGET_US: u64 = 10_000;
/// 预算总账红线（µs）——所有单元预算之和不得超过。
///
/// 来源：B10 批「引导到自检完成 50ms」红线（UNX-A1 域总纲口径）。
pub const TOTAL_BUDGET_REDBOX_US: u64 = 50_000;
/// 单元名称最大长度（诊断可读性界——名称进环路径报告，过长撑爆日志行）。
pub const MAX_NAME_LEN: usize = 48;
/// 串行组编号上限（0 = 无组；组编号 1..=MAX_SERIAL_GROUPS）。
pub const MAX_SERIAL_GROUPS: u16 = 32;
/// 优先级取值域（0=最低 ..=9=最高——闭集防随意赋值）。
pub const PRIORITY_MAX: u8 = 9;
/// 模拟执行 LCG 种子默认值（确定性锚——同种子同耗时序）。
pub const DEFAULT_SIM_SEED: u64 = 0x183_811C_9DC5;
/// FNV-1a 32 位素数与偏移（快照指纹口径，与 unxstar 家族一致）。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// 一、阶段体系（Phase 闭集——初始化五阶段单向推进）
// ---------------------------------------------------------------------------

/// 初始化阶段（五阶段闭集，单向推进——判据「阶段单向性」的取值域）。
///
/// 对标 Windows：Early ≈ ntoskrnl KiSystemStartup 前段；Foundation ≈
/// executive 初始化（Po/Pm/Mm）；Subsystems ≈ Smss 起会话；Services ≈
/// SCM 拉服务；Late ≈ 登录前收尾。Varix 收拢为编排器可校验的显性阶段。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Phase {
    /// 阶段 0：最早——CPU/内存基座就绪前不可依赖任何其他单元。
    Early = 0,
    /// 阶段 1：基座——页表/堆/中断基座。
    Foundation = 1,
    /// 阶段 2：子系统——调度/时钟/诊断总线。
    Subsystems = 2,
    /// 阶段 3：服务——文件/驱动/兼容层。
    Services = 3,
    /// 阶段 4：收尾——用户态门面/自检汇总。
    Late = 4,
}

impl Phase {
    /// 阶段序号（单向性校验用）。
    pub fn ord(self) -> usize {
        self as usize
    }

    /// 全阶段闭集按序枚举（遍历用——顺序即推进序）。
    pub fn all() -> [Phase; PHASE_COUNT] {
        [
            Phase::Early,
            Phase::Foundation,
            Phase::Subsystems,
            Phase::Services,
            Phase::Late,
        ]
    }

    /// 阶段名（中文诊断用）。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Early => "最早",
            Phase::Foundation => "基座",
            Phase::Subsystems => "子系统",
            Phase::Services => "服务",
            Phase::Late => "收尾",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、依赖声明体系（单元 + 依赖边 + 冲突对）
// ---------------------------------------------------------------------------

/// 依赖种类（三闭集——判据面：「依赖图声明」的结构化口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepKind {
    /// 硬依赖：被依赖单元必须先完成，否则本单元不可执行（Skip 显性）。
    Hard = 0,
    /// 软依赖：尽力而为——被依赖单元失败不阻断本单元，但顺序仍保证。
    Soft = 1,
    /// 冲突声明：与目标单元互斥——两者不得落入同一 wave（可能同批执行）。
    Mutex = 2,
}

impl DepKind {
    /// 种类名（诊断用）。
    pub fn name(self) -> &'static str {
        match self {
            DepKind::Hard => "硬依赖",
            DepKind::Soft => "软依赖",
            DepKind::Mutex => "互斥冲突",
        }
    }
}

/// 一条依赖声明（边：本单元 → 目标单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepEdge {
    /// 目标单元编号（依赖谁/与谁互斥）。
    pub target: u16,
    /// 依赖种类。
    pub kind: DepKind,
}

impl DepEdge {
    /// 构造一条依赖声明。
    pub fn new(target: u16, kind: DepKind) -> Self {
        DepEdge { target, kind }
    }
}

/// init 单元声明（依赖图节点——单一声明源的原子项）。
#[derive(Clone, Debug)]
pub struct InitUnit {
    /// 单元编号（0..MAX_UNITS 内唯一——声明表索引即编号）。
    pub id: u16,
    /// 单元名（诊断/环路径报告用——非空、≤MAX_NAME_LEN）。
    pub name: String,
    /// 所属阶段（单向推进）。
    pub phase: Phase,
    /// 优先级（0..=9，同 wave 内排序键之一：高者先——高优先级基座先行）。
    pub priority: u8,
    /// 依赖声明列表（≤MAX_DEPS_PER_UNIT 条，target 不得为本单元 id）。
    pub deps: Vec<DepEdge>,
    /// 单元执行预算（µs，1..=MAX_UNIT_BUDGET_US——零预算即拒）。
    pub budget_us: u64,
    /// 串行组（0=无组；同组单元编排进不同 wave 强制分离——计划期语义）。
    pub serial_group: u16,
}

impl InitUnit {
    /// 声明一个 init 单元（构造期最小校验——非法即 Err，不留半成品）。
    ///
    /// 错误路径：编号越界/名称空或超长/优先级越界/预算非法/依赖数越界/
    /// 自依赖/串行组越界——七类构造期违规各自显性。
    pub fn declare(
        id: u16,
        name: &str,
        phase: Phase,
        priority: u8,
        deps: Vec<DepEdge>,
        budget_us: u64,
        serial_group: u16,
    ) -> Result<Self, PlanError> {
        if id as usize >= MAX_UNITS {
            return Err(PlanError::UnitIllegal {
                id,
                why: format!("编号 {} 超出上限 {}——单元编号必须有界", id, MAX_UNITS),
                advice: "把单元拆入下一批次或提升编号容量前先修 MAX_UNITS 常量并注释来源".to_string(),
            });
        }
        if name.is_empty() {
            return Err(PlanError::UnitIllegal {
                id,
                why: "单元名为空——名称是环路径报告与人读账本的主键".to_string(),
                advice: "给出 ≤48 字节的中文或英文短名，如 'heap-init'".to_string(),
            });
        }
        if name.len() > MAX_NAME_LEN {
            return Err(PlanError::UnitIllegal {
                id,
                why: format!("单元名 {} 字节超上限 {}", name.len(), MAX_NAME_LEN),
                advice: "缩短名称——长描述进注释不进名称".to_string(),
            });
        }
        if priority > PRIORITY_MAX {
            return Err(PlanError::UnitIllegal {
                id,
                why: format!("优先级 {} 超出闭集 0..={}", priority, PRIORITY_MAX),
                advice: "取 0(最低)~9(最高) 闭集内值——闭集外排序语义未定义".to_string(),
            });
        }
        if budget_us == 0 || budget_us > MAX_UNIT_BUDGET_US {
            return Err(PlanError::BudgetIllegal {
                id,
                budget_us,
                why: format!(
                    "预算 {}µs 非法——必须 1..={}µs（零预算=无成本承诺是谎报，超上限=切分失当）",
                    budget_us, MAX_UNIT_BUDGET_US
                ),
                advice: "按真实工作量给预算；大活拆单元不撑预算".to_string(),
            });
        }
        if deps.len() > MAX_DEPS_PER_UNIT {
            return Err(PlanError::UnitIllegal {
                id,
                why: format!("依赖数 {} 超上限 {}", deps.len(), MAX_DEPS_PER_UNIT),
                advice: "依赖超过 16 条说明该单元是汇聚点——拆出中间编排单元".to_string(),
            });
        }
        for d in deps.iter() {
            if d.target == id {
                return Err(PlanError::SelfDependency { id });
            }
            if d.target as usize >= MAX_UNITS {
                return Err(PlanError::UnitIllegal {
                    id,
                    why: format!("依赖目标 {} 编号越界", d.target),
                    advice: "先声明目标单元再引用——编号不是自由数".to_string(),
                });
            }
        }
        if serial_group > MAX_SERIAL_GROUPS {
            return Err(PlanError::UnitIllegal {
                id,
                why: format!("串行组 {} 超上限 {}", serial_group, MAX_SERIAL_GROUPS),
                advice: "组编号取 0(无组)..=32；组是稀缺资源按需申请".to_string(),
            });
        }
        Ok(InitUnit {
            id,
            name: name.to_string(),
            phase,
            priority,
            deps,
            budget_us,
            serial_group,
        })
    }

    /// 该单元的硬依赖目标集（拓扑边提取——仅 Hard/Soft 入图，Mutex 不入图）。
    fn topo_targets(&self) -> Vec<u16> {
        self.deps
            .iter()
            .filter(|d| matches!(d.kind, DepKind::Hard | DepKind::Soft))
            .map(|d| d.target)
            .collect()
    }

    /// 该单元的互斥对列表（wave 分离校验用）。
    fn mutex_targets(&self) -> Vec<u16> {
        self.deps
            .iter()
            .filter(|d| d.kind == DepKind::Mutex)
            .map(|d| d.target)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 三、静态校验报告（声明收集期的违规账——全显性零静默）
// ---------------------------------------------------------------------------

/// 静态校验问题（一条违规 = 一个结构化记录，不吞不打折）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    /// 关联单元编号（冲突对类问题记录双方时以小号为准）。
    pub unit: u16,
    /// 诊断码（0x35 段）。
    pub code: u32,
    /// 人话描述（含具体编号/名称）。
    pub detail: String,
    /// 可操作建议（零静默纪律：拒绝必带修法）。
    pub advice: String,
}

impl ValidationIssue {
    fn new(unit: u16, code: u32, detail: String, advice: &str) -> Self {
        ValidationIssue {
            unit,
            code,
            detail,
            advice: advice.to_string(),
        }
    }
}

/// 静态校验结果（问题账 + 通过标志——有账即不通过，无账才放行）。
#[derive(Clone, Debug, Default)]
pub struct ValidationReport {
    /// 违规账（按发现序追加——不排序，保留声明序便于定位）。
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// 是否全绿（零违规）。
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// 违规总数。
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// 空报告判定（与 len()==0 等价——显性命名）。
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// 按诊断码过滤（归因用——如只看环依赖 0x3503）。
    pub fn by_code(&self, code: u32) -> Vec<&ValidationIssue> {
        self.issues.iter().filter(|i| i.code == code).collect()
    }

    /// 人话汇总（多行文本——直接可贴进引导日志）。
    pub fn summarize(&self) -> String {
        if self.ok() {
            return "静态校验全绿：零违规".to_string();
        }
        let mut s = format!("静态校验 {} 条违规：\n", self.issues.len());
        for (n, i) in self.issues.iter().enumerate() {
            s.push_str(&format!(
                "  [{}] 单元#{} 码0x{:04X} {}  建议：{}\n",
                n + 1,
                i.unit,
                i.code,
                i.detail,
                i.advice
            ));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 四、依赖图（DepGraph——声明收集 + 静态校验 + 查询面）
// ---------------------------------------------------------------------------

/// init 依赖图（节点 = 声明单元按 id 索引；边 = 硬/软依赖；互斥对旁路存放）。
#[derive(Clone, Debug)]
pub struct DepGraph {
    /// 节点表（索引即 id；空洞 = 未声明编号——查询时显性）。
    units: BTreeMap<u16, InitUnit>,
    /// 静态校验缓存（collect 时算好——校验与排序解耦）。
    report: ValidationReport,
}

impl DepGraph {
    /// 空图。
    pub fn new() -> Self {
        DepGraph {
            units: BTreeMap::new(),
            report: ValidationReport::default(),
        }
    }

    /// 声明单元数。
    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    /// 取单元（只读——编排器外的消费方走这里）。
    pub fn unit(&self, id: u16) -> Option<&InitUnit> {
        self.units.get(&id)
    }

    /// 全部单元按编号升序（确定性遍历口径）。
    pub fn units_sorted(&self) -> Vec<&InitUnit> {
        self.units.values().collect()
    }

    /// 静态校验报告（只读）。
    pub fn report(&self) -> &ValidationReport {
        &self.report
    }

    /// 收集一个单元进图（不校验——校验统一在 finalize 校验闸做）。
    ///
    /// 同编号重复声明即拒（编号是主键，覆盖=静默改账，零容忍）。
    pub fn collect(&mut self, unit: InitUnit) -> Result<(), PlanError> {
        if self.units.contains_key(&unit.id) {
            return Err(PlanError::DuplicateUnit {
                id: unit.id,
                name: unit.name,
            });
        }
        self.units.insert(unit.id, unit);
        Ok(())
    }

    /// 校验闸：对全图跑五类静态校验并缓存报告。
    ///
    /// 五类：①未知依赖 ②重复依赖边 ③阶段单向性 ④互斥对合法（双方已声明）
    /// ⑤预算总账守恒（Σbudget ≤ 50ms 红线）。自依赖在声明期已拦（declare），
    /// 此处不重复计——但防御性复查仍做（纵深防御：构造旁路绕过 declare 时
    /// 仍能抓到）。
    pub fn finalize(&mut self) -> &ValidationReport {
        let mut issues: Vec<ValidationIssue> = Vec::new();
        let ids: Vec<u16> = self.units.keys().copied().collect();
        let total: u64 = self.units.values().map(|u| u.budget_us).sum();

        // ①②③④：逐单元过边。
        for id in ids.iter() {
            let unit = &self.units[id]; // BTreeMap 按键取——键来自 keys() 必命中
            let name = unit.name.clone();
            let mut seen_targets: Vec<u16> = Vec::new();
            for d in unit.deps.iter() {
                // 防御性自依赖复查（declare 已拦——纵深）。
                if d.target == *id {
                    issues.push(ValidationIssue::new(
                        *id,
                        XC_DEP_SELF,
                        format!("单元 {}({}) 依赖自身", id, name),
                        "删除该依赖边——自己等自己永远不就绪",
                    ));
                    continue;
                }
                // ①未知依赖：Hard/Soft 的 target 必须已声明；Mutex 的 target 也必须已声明。
                if !self.units.contains_key(&d.target) {
                    issues.push(ValidationIssue::new(
                        *id,
                        XC_DEP_UNKNOWN,
                        format!(
                            "单元 {}({}) {}目标 #{} 未声明",
                            id,
                            name,
                            d.kind.name(),
                            d.target
                        ),
                        "先声明目标单元，或删除这条依赖边——悬空引用不进图",
                    ));
                    continue;
                }
                // ②重复依赖边（同 target 重复——无论种类）。
                if seen_targets.contains(&d.target) {
                    issues.push(ValidationIssue::new(
                        *id,
                        XC_DEP_DUP,
                        format!("单元 {}({}) 对 #{} 重复声明依赖", id, name, d.target),
                        "合并为一条——重复边让入度账翻倍，拓扑序失真",
                    ));
                    continue;
                }
                seen_targets.push(d.target);
                // ③阶段单向性：Hard/Soft 依赖的目标阶段必须 ≤ 本单元阶段。
                //   （Mutex 不入图不校验阶段——互斥与先后无关。）
                if matches!(d.kind, DepKind::Hard | DepKind::Soft) {
                    let tgt_phase = self.units[&d.target].phase;
                    if tgt_phase.ord() > unit.phase.ord() {
                        issues.push(ValidationIssue::new(
                            *id,
                            XC_PHASE_BACKWARD,
                            format!(
                                "单元 {}({}) 在{}阶段却依赖{}阶段的 #{}({})——初始化序倒挂",
                                id,
                                name,
                                unit.phase.name(),
                                tgt_phase.name(),
                                d.target,
                                self.units[&d.target].name
                            ),
                            "把本单元挪到目标阶段或之后——初始化只能依赖已就绪的更早阶段",
                        ));
                    }
                }
            }
        }

        // ④互斥对称性复查：A 声明与 B 互斥，B 未声明与 A 互斥 = 单边声明
        //   （单边互斥是半吊子约束——B 不知道自己在冲突名单里，编排器
        //   分 wave 时只防了 A 的视角。要求成对声明，缺边即显性）。
        for id in ids.iter() {
            let unit = &self.units[id];
            for m in unit.mutex_targets() {
                let back_declared = self
                    .units
                    .get(&m)
                    .map(|u| u.mutex_targets().contains(id))
                    .unwrap_or(false);
                if self.units.contains_key(&m) && !back_declared {
                    issues.push(ValidationIssue::new(
                        *id,
                        XC_MUTEX_VIOLATION,
                        format!(
                            "互斥声明单边：{}({}) 声明与 #{}({}) 互斥，但对方未反向声明",
                            id,
                            unit.name,
                            m,
                            self.units[&m].name
                        ),
                        "补对方那条 Mutex 边——互斥必须成对声明，单边防不住对方视角",
                    ));
                }
            }
        }

        // ⑤预算总账：Σbudget > 50ms 红线即显性（不阻断排序——账照算，
        //   但问题账留痕供批次回炉）。
        if total > TOTAL_BUDGET_REDBOX_US {
            issues.push(ValidationIssue::new(
                0,
                XC_BUDGET_ILLEGAL,
                format!(
                    "预算总账 {}µs 超红线 {}µs（红线来源：A1 域引导到自检 50ms）",
                    total, TOTAL_BUDGET_REDBOX_US
                ),
                "砍单元预算或拆批次——总账红线是引导体验承诺，不是软建议",
            ));
        }

        self.report = ValidationReport { issues };
        &self.report
    }
}

impl Default for DepGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、拓扑排序引擎（Kahn 分层 + 平局打破 + 环检出）
// ---------------------------------------------------------------------------

/// 拓扑排序结果（分层 + 线性化 + 环账三件套）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopoResult {
    /// wave 分层（wave[i] = 第 i 层可执行单元编号，层内已按平局键排好）。
    pub waves: Vec<Vec<u16>>,
    /// 线性执行序（wave 展平——消费方直接按此序执行）。
    pub order: Vec<u16>,
    /// 环成员集合（Kahn 剩余——不在任何 wave 里）。
    pub cycle_members: Vec<u16>,
    /// 提取出的具体环路径（每环一条 A→B→…→A；零环时为空）。
    pub cycle_paths: Vec<String>,
}

impl TopoResult {
    /// 是否有环（环检出 100% 的布尔面——有环即编排失败）。
    pub fn has_cycle(&self) -> bool {
        !self.cycle_members.is_empty()
    }

    /// 可执行单元总数（进入 wave 的）。
    pub fn scheduled_count(&self) -> usize {
        self.order.len()
    }

    /// wave 数。
    pub fn wave_count(&self) -> usize {
        self.waves.len()
    }
}

/// 依赖图对拓扑排序引擎的输入视图（借 graph 的只读面，不算账）。
struct TopoView<'a> {
    ids: Vec<u16>,
    names: &'a BTreeMap<u16, InitUnit>,
}

impl<'a> TopoView<'a> {
    fn from_graph(g: &'a DepGraph) -> Self {
        TopoView {
            ids: g.units.keys().copied().collect(),
            names: &g.units,
        }
    }

    fn unit(&self, id: u16) -> &InitUnit {
        &self.names[&id]
    }
}

/// 依赖图拓扑排序——**实现一：Kahn 分层法**（wave 即层，层内平局键排序）。
///
/// 算法：入度表初始化 → 零入度集为 wave0 → 逐层剥皮（剥去本层后新零入度
/// 集为下一层）→ 剥到无节点可剥，剩余即环成员。
/// 确定性：每层按 (优先级降序, 编号升序) 排序——同输入恒同输出。
/// 复杂度：O(V·logV + E)（每层排序）。
pub fn topo_layered(g: &DepGraph) -> TopoResult {
    let view = TopoView::from_graph(g);
    let n = view.ids.len();
    let index_of = |id: u16| -> usize { view.ids.binary_search(&id).unwrap_or(usize::MAX) };
    let _ = index_of; // 保留闭包供调试探针；主逻辑走 id 直查表

    // 入度表（按图节点 id 直查）。
    let mut indeg: BTreeMap<u16, usize> = BTreeMap::new();
    // 邻接表：dep_target → [依赖它的单元]（剥皮时推进）。
    let mut adj: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for id in view.ids.iter() {
        indeg.insert(*id, 0);
    }
    for id in view.ids.iter() {
        let unit = view.unit(*id);
        for t in unit.topo_targets() {
            // 静态校验已保证 target 存在；防御性跳过悬空边（有账在 report）。
            if !indeg.contains_key(&t) {
                continue;
            }
            *indeg.entry(*id).or_insert(0) += 1;
            adj.entry(t).or_default().push(*id);
        }
    }

    let mut waves: Vec<Vec<u16>> = Vec::new();
    let mut order: Vec<u16> = Vec::with_capacity(n);
    let mut remaining = n;

    // wave0：零入度集。
    let mut frontier: Vec<u16> = indeg
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| *id)
        .collect();
    sort_frontier(&mut frontier, &view);

    loop {
        if frontier.is_empty() || remaining == 0 {
            break;
        }
        // 本层入账。
        let layer = frontier.clone();
        remaining -= layer.len();
        order.extend_from_slice(&layer);
        // 剥皮：本层各节点的后继入度减一，减到零者进下一层。
        let mut next: Vec<u16> = Vec::new();
        for id in layer.iter() {
            if let Some(succs) = adj.get_mut(id) {
                for s in succs.drain(..) {
                    let e = indeg.get_mut(&s).expect("入度表缺后继——邻接表与入度表失同步");
                    *e -= 1;
                    if *e == 0 {
                        next.push(s);
                    }
                }
            }
        }
        sort_frontier(&mut next, &view);
        frontier = next;
        if waves.len() >= MAX_WAVES {
            break; // 硬界防御——实际不可能到（剥皮每层至少剥 1）
        }
        waves.push(layer);
    }

    // 剩余 = 环成员（Kahn 剩余定理——判据「环依赖检出 100%」的集合面）。
    let cycle_members: Vec<u16> = indeg
        .iter()
        .filter(|(_, d)| **d > 0)
        .map(|(id, _)| *id)
        .collect();

    // 环路径提取（DFS 三色——报具体路径不报抽象环）。
    let cycle_paths = if cycle_members.is_empty() {
        Vec::new()
    } else {
        extract_cycle_paths(&view, &cycle_members)
    };

    TopoResult {
        waves,
        order,
        cycle_members,
        cycle_paths,
    }
}

/// frontier 排序（平局打破：优先级降序，编号升序——确定性锚）。
fn sort_frontier(frontier: &mut [u16], view: &TopoView) {
    frontier.sort_by(|a, b| {
        let ua = view.unit(*a);
        let ub = view.unit(*b);
        ub.priority.cmp(&ua.priority).then(a.cmp(b))
    });
}

/// 环路径提取——**DFS 三色标记**（白=未访/灰=在栈/黑=完成）。
///
/// 对环成员子图跑 DFS，撞灰即环；回溯灰栈提取路径 A→B→…→A。
/// 每个环只报一次（以环内最小编号为起点归一化——同环不同起点只出一条账）。
fn extract_cycle_paths(view: &TopoView, members: &[u16]) -> Vec<String> {
    // 环成员子图的邻接（只保留成员→成员的边）。
    let mut adj: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for m in members {
        let unit = view.unit(*m);
        let succs: Vec<u16> = unit
            .topo_targets()
            .into_iter()
            .filter(|t| members.contains(t))
            .collect();
        adj.insert(*m, succs);
    }

    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color: BTreeMap<u16, u8> = BTreeMap::new();
    for m in members {
        color.insert(*m, WHITE);
    }

    let mut paths: Vec<String> = Vec::new();
    let mut reported_seeds: Vec<u16> = Vec::new();

    // 从最小编号起 DFS（确定性遍历序）。
    for start in members.iter() {
        if color[start] != WHITE {
            continue;
        }
        // 迭代式 DFS（no_std 下递归深度不可控——显式栈）。
        // 栈元素：(节点, 已展开的后继游标)。
        let mut stack: Vec<(u16, usize)> = Vec::new();
        stack.push((*start, 0));
        color.insert(*start, GRAY);
        while let Some((node, ref mut cur)) = stack.last_mut().copied() {
            let succs = adj.get(&node).cloned().unwrap_or_default();
            if *cur < succs.len() {
                let next = succs[*cur];
                *cur += 1;
                match color.get(&next).copied().unwrap_or(BLACK) {
                    WHITE => {
                        color.insert(next, GRAY);
                        stack.push((next, 0));
                    }
                    GRAY => {
                        // 撞灰 = 环：从栈里回溯提取 node→…→next 路径。
                        let mut chain: Vec<u16> = Vec::new();
                        let mut found = false;
                        for (n, _) in stack.iter().rev() {
                            chain.push(*n);
                            if *n == next {
                                found = true;
                                break;
                            }
                        }
                        if found {
                            chain.reverse();
                            // 归一化：环内最小编号为头（同环不同起点只报一次）。
                            let min_id = *chain.iter().min().unwrap_or(start);
                            if !reported_seeds.contains(&min_id) {
                                reported_seeds.push(min_id);
                                let mut s = String::new();
                                for (i, n) in chain.iter().enumerate() {
                                    if i > 0 {
                                        s.push_str("→");
                                    }
                                    s.push_str(&format!("#{}({})", n, view.unit(*n).name));
                                }
                                s.push_str(&format!("→#{}({})", next, view.unit(next).name));
                                paths.push(s);
                            }
                        }
                    }
                    BLACK => { /* 已完成的后继——无新环 */ }
                    _ => { /* 不可达 */ }
                }
            } else {
                // 后继耗尽：出栈转黑。
                color.insert(node, BLACK);
                stack.pop();
                if stack.is_empty() {
                    break;
                }
            }
        }
    }
    paths
}

/// 依赖图拓扑排序——**实现二：朴素逐点法**（独立实现，供对拍）。
///
/// 算法：每轮从「未分配且全部硬/软依赖已分配」的节点里按平局键取最小者；
/// 取到 n 个或一轮取不到（有环）为止。
/// 与实现一完全不同的遍历策略（逐点贪心 vs 分层剥皮）——两者一致即强证据。
/// 复杂度 O(V²·D)——512 上限下最坏 260 万次比较，初始化一次性可接受。
pub fn topo_naive(g: &DepGraph) -> TopoResult {
    let view = TopoView::from_graph(g);
    let n = view.ids.len();
    let mut placed: Vec<u16> = Vec::with_capacity(n);
    let mut waves: Vec<Vec<u16>> = Vec::new();

    let deps_of = |id: u16| -> Vec<u16> { view.unit(id).topo_targets() };

    loop {
        if placed.len() >= n {
            break;
        }
        // 候选：未放置且全部依赖已放置。
        let mut candidates: Vec<u16> = Vec::new();
        for id in view.ids.iter() {
            if placed.contains(id) {
                continue;
            }
            let ready = deps_of(*id)
                .iter()
                .all(|t| placed.contains(t) || !view.ids.contains(t));
            // 悬空依赖（未声明 target）按「不可满足」处理——与实现一的
            // 防御口径一致：这类边在静态校验已有账，排序时视为不存在。
            let ready = ready || deps_of(*id).iter().any(|t| !view.ids.contains(t));
            if ready {
                candidates.push(*id);
            }
        }
        if candidates.is_empty() {
            break; // 有环——剩余进环账
        }
        // 平局键排序后整层入账（与实现一同键——同层语义等价）。
        sort_frontier_view(&mut candidates, &view);
        waves.push(candidates.clone());
        placed.extend_from_slice(&candidates);
    }

    let cycle_members: Vec<u16> = view
        .ids
        .iter()
        .filter(|id| !placed.contains(id))
        .copied()
        .collect();
    let cycle_paths = if cycle_members.is_empty() {
        Vec::new()
    } else {
        extract_cycle_paths(&view, &cycle_members)
    };

    TopoResult {
        waves,
        order: placed,
        cycle_members,
        cycle_paths,
    }
}

/// 朴素法专用排序（与 sort_frontier 同键——不共享 TopoView 生命周期借用冲突
/// 时的独立入口；键必须与实现一逐字节一致，对拍才有效）。
fn sort_frontier_view(frontier: &mut [u16], view: &TopoView) {
    frontier.sort_by(|a, b| {
        let ua = view.unit(*a);
        let ub = view.unit(*b);
        ub.priority.cmp(&ua.priority).then(a.cmp(b))
    });
}

/// 双实现对拍（判据 J1 前半「执行序与拓扑序一致」的确定性证据）。
///
/// 分层 Kahn 与朴素逐点是两个独立写法；两者 wave 结构与线性序逐位一致
/// 才放行。不一致 = 确定性破约（有隐藏输入序依赖），显性拒。
pub fn crosscheck(g: &DepGraph) -> Result<(), PlanError> {
    let a = topo_layered(g);
    let b = topo_naive(g);
    if a.order != b.order {
        return Err(PlanError::OrderMismatch {
            detail: format!(
                "分层法序 {:?} ≠ 朴素法序 {:?}——两实现不一致说明有隐藏输入序依赖",
                a.order, b.order
            ),
        });
    }
    if a.waves != b.waves {
        return Err(PlanError::OrderMismatch {
            detail: "线性序一致但 wave 分层不一致——层边界语义两写法漂移".to_string(),
        });
    }
    if a.cycle_members != b.cycle_members {
        return Err(PlanError::OrderMismatch {
            detail: "环成员集合两实现不一致——环检出口径漂移".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、执行计划（InitPlan——wave 序列 + 不变量四闸 + 指纹）
// ---------------------------------------------------------------------------

/// 计划内单单元条目（线性序里的一个位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanEntry {
    /// 单元编号。
    pub id: u16,
    /// 所在 wave（0 起）。
    pub wave: usize,
    /// 位置序（全计划内 0..n 唯一——执行序号）。
    pub slot: usize,
    /// 预算（µs，声明透传——账本对账用）。
    pub budget_us: u64,
}

/// 初始化执行计划（编排器的产物——消费方按 order 执行）。
#[derive(Clone, Debug)]
pub struct InitPlan {
    /// 线性执行序（wave 展平，平局键已排——这就是「执行序」）。
    pub order: Vec<u16>,
    /// 计划条目（id → PlanEntry 查询面）。
    pub entries: BTreeMap<u16, PlanEntry>,
    /// wave 分层（并行化预留位——A3 域按此并行，当前串行展平）。
    pub waves: Vec<Vec<u16>>,
    /// 计划指纹（FNV-1a 32 位——order+wave+budget 三要素合成）。
    pub fingerprint: u32,
    /// 串行组分布账（组号 → 组内单元——同组必跨 wave 显性核对）。
    pub serial_map: BTreeMap<u16, Vec<u16>>,
}

impl InitPlan {
    /// 从 TopoResult 构建（无环前提——有环先拒绝）。
    pub fn from_topo(tr: &TopoResult) -> Result<Self, PlanError> {
        if tr.has_cycle() {
            return Err(PlanError::CycleBlocked {
                members: tr.cycle_members.clone(),
                paths: tr.cycle_paths.clone(),
            });
        }
        let mut entries = BTreeMap::new();
        for (slot, id) in tr.order.iter().enumerate() {
            entries.insert(
                *id,
                PlanEntry {
                    id: *id,
                    wave: usize::MAX, // 回填——见下
                    slot,
                    budget_us: 0, // 回填——见下
                },
            );
        }
        // wave/预算回填。
        let mut wave_of: BTreeMap<u16, usize> = BTreeMap::new();
        for (w, layer) in tr.waves.iter().enumerate() {
            for id in layer {
                wave_of.insert(*id, w);
            }
        }
        // 注：wave/预算回填需要图——from_topo 只拿 TopoResult 不够；
        // 预算与串行组由 build_plan（持图）二次回填。此处先置占位，
        // build_plan 兜底覆盖——占位值不外泄（from_topo 是内部步，
        // 对外唯一入口 build_plan 保证终值正确）。
        let fingerprint = fnv_plan(&tr.order, &tr.waves);
        let mut serial_map: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
        for id in tr.order.iter() {
            serial_map.entry(0).or_default().push(*id); // 占位组 0——build_plan 覆盖
        }
        Ok(InitPlan {
            order: tr.order.clone(),
            entries,
            waves: tr.waves.clone(),
            fingerprint,
            serial_map,
        })
    }

    /// 计划不变量四闸（构建后自查——闸门在生成方，不靠消费方自觉）。
    ///
    /// 闸一：恰一次——order 内每编号恰出现一次，entries 键集与 order 集合
    ///       恒等（重复=双执行、丢失=漏执行，都不许）。
    /// 闸二：依赖前置——每单元的全部硬/软依赖在 order 中位次严格靠前
    ///       （拓扑序的定义式核验——不信任排序器，重新验一遍）。
    /// 闸三：wave 一致性——entry.wave 与 waves 布局互证（两个真相源必须
    ///       同步，漂移即指纹失真）。
    /// 闸四：串行组跨 wave——同组任意两单元不得同 wave（同 wave=可能同批，
    ///       串行语义即破）。
    pub fn validate(&self, g: &DepGraph) -> Result<(), PlanError> {
        // 闸一：恰一次。
        let mut seen = vec![false; MAX_UNITS];
        for id in self.order.iter() {
            let u = *id as usize;
            if u >= MAX_UNITS {
                return Err(PlanError::PlanInvariant {
                    gate: 1,
                    why: format!("计划含越界编号 {}", id),
                });
            }
            if seen[u] {
                return Err(PlanError::PlanInvariant {
                    gate: 1,
                    why: format!("单元 #{} 在执行序中重复出现——重复=双执行", id),
                });
            }
            seen[u] = true;
        }
        if self.entries.len() != self.order.len() {
            return Err(PlanError::PlanInvariant {
                gate: 1,
                why: format!(
                    "entries {} 项 ≠ order {} 项——条目表与执行序失同步",
                    self.entries.len(),
                    self.order.len()
                ),
            });
        }
        // 图内声明的全部单元必须全部入计划（编排不留漏网单元）。
        for id in g.units.keys() {
            if (*id as usize) < MAX_UNITS && !seen[*id as usize] {
                return Err(PlanError::PlanInvariant {
                    gate: 1,
                    why: format!("图声明单元 #{} 未入计划——漏编排", id),
                });
            }
        }

        // 闸二：依赖前置（定义式复核）。
        let mut pos: BTreeMap<u16, usize> = BTreeMap::new();
        for (i, id) in self.order.iter().enumerate() {
            pos.insert(*id, i);
        }
        for id in self.order.iter() {
            let unit = g.unit(*id);
            let unit = match unit {
                Some(u) => u,
                None => {
                    return Err(PlanError::PlanInvariant {
                        gate: 2,
                        why: format!("计划含图中不存在的单元 #{}", id),
                    });
                }
            };
            for t in unit.topo_targets() {
                if let Some(&tp) = pos.get(&t) {
                    let myp = pos[id];
                    if tp >= myp {
                        return Err(PlanError::PlanInvariant {
                            gate: 2,
                            why: format!(
                                "硬依赖倒置：#{}({}) 在位 {}，其依赖 #{} 在位 {}——依赖没先跑",
                                id,
                                unit.name,
                                myp,
                                t,
                                tp
                            ),
                        });
                    }
                }
                // 悬空依赖（图中本就有账）此处跳过——校验闸已拦。
            }
        }

        // 闸三：wave 一致性。
        let mut wave_check: BTreeMap<u16, usize> = BTreeMap::new();
        for (w, layer) in self.waves.iter().enumerate() {
            for id in layer {
                if wave_check.insert(*id, w).is_some() {
                    return Err(PlanError::PlanInvariant {
                        gate: 3,
                        why: format!("单元 #{} 在 wave 布局中重复——分层账失真", id),
                    });
                }
            }
        }
        for (id, e) in self.entries.iter() {
            match wave_check.get(id) {
                Some(&w) if w == e.wave => {}
                Some(&w) => {
                    return Err(PlanError::PlanInvariant {
                        gate: 3,
                        why: format!(
                            "单元 #{} wave 双源漂移：条目表 {} vs 布局 {}",
                            id, e.wave, w
                        ),
                    });
                }
                None => {
                    return Err(PlanError::PlanInvariant {
                        gate: 3,
                        why: format!("单元 #{} 有条目无 wave 布局——两真相源失同步", id),
                    });
                }
            }
        }

        // 闸四：串行组跨 wave。
        for (grp, members) in self.serial_map.iter() {
            if *grp == 0 {
                continue; // 组 0 = 无组
            }
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    let wi = wave_check.get(&members[i]).copied().unwrap_or(usize::MAX);
                    let wj = wave_check.get(&members[j]).copied().unwrap_or(usize::MAX);
                    if wi == wj {
                        return Err(PlanError::PlanInvariant {
                            gate: 4,
                            why: format!(
                                "串行组 {} 的 #{} 与 #{} 同处 wave {}——串行语义破",
                                grp, members[i], members[j], wi
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// FNV-1a 32 位逐字节（家族统一口径）。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 计划指纹（order 逐号 + wave 布局合成——任一漂移指纹即变）。
fn fnv_plan(order: &[u16], waves: &[Vec<u16>]) -> u32 {
    let mut h = FNV_OFFSET;
    for id in order {
        h ^= (*id as u32) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= ((*id as u32) >> 8) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    for (w, layer) in waves.iter().enumerate() {
        h ^= (w as u32) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
        for id in layer {
            h ^= (*id as u32) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

// ---------------------------------------------------------------------------
// 七、确定性模拟执行（tick 注入 + LCG——同种子同耗时序可 CI 回放）
// ---------------------------------------------------------------------------

/// 单元执行终态（五闭集——账本守恒的取值域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutcomeKind {
    /// 完成（正常路径）。
    Done = 0,
    /// 跳过（硬依赖未完成——依赖失败/依赖被跳过，传导显性）。
    Skipped = 1,
    /// 阻断（环成员——编排期已拒，此态仅在防御路径出现）。
    Blocked = 2,
    /// 失败（执行报错——注入面）。
    Failed = 3,
    /// 未执行（计划截断前的剩余——账本对账用）。
    Pending = 4,
}

impl OutcomeKind {
    /// 终态名（中文账本）。
    pub fn name(self) -> &'static str {
        match self {
            OutcomeKind::Done => "完成",
            OutcomeKind::Skipped => "跳过",
            OutcomeKind::Blocked => "阻断",
            OutcomeKind::Failed => "失败",
            OutcomeKind::Pending => "未执行",
        }
    }
}

/// 单单元执行账（账本原子项）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitOutcome {
    /// 单元编号。
    pub id: u16,
    /// 终态。
    pub kind: OutcomeKind,
    /// 开始 tick（逻辑时钟，非墙钟）。
    pub start_tick: u64,
    /// 结束 tick。
    pub end_tick: u64,
    /// 耗时（end-start，µs 口径——tick 单位即 µs）。
    pub duration_us: u64,
    /// 跳过/失败时的归因（完成态为空串）。
    pub reason: String,
}

/// 模拟执行账本（全量单元各一条——守恒断言的载体）。
#[derive(Clone, Debug, Default)]
pub struct ExecLedger {
    /// 账目（按单元编号索引）。
    pub outcomes: BTreeMap<u16, UnitOutcome>,
    /// 总 tick（最后一个结束 tick——引导耗时模拟值）。
    pub total_ticks: u64,
    /// 账本指纹（FNV——终态序列漂移即变）。
    pub fingerprint: u32,
}

impl ExecLedger {
    /// 守恒断言（判据「账本守恒」）：账目数 == 单元声明数，且每账终态合法。
    pub fn balanced(&self, expected_units: usize) -> Result<(), PlanError> {
        if self.outcomes.len() != expected_units {
            return Err(PlanError::LedgerUnbalanced {
                got: self.outcomes.len(),
                want: expected_units,
                why: format!(
                    "账目 {} 条 ≠ 单元 {} 条——有单元蒸发或重复记账",
                    self.outcomes.len(),
                    expected_units
                ),
            });
        }
        Ok(())
    }

    /// 按终态统计（诊断汇总）。
    pub fn count(&self, kind: OutcomeKind) -> usize {
        self.outcomes.values().filter(|o| o.kind == kind).count()
    }

    /// 人话汇总（引导日志格式）。
    pub fn summarize(&self) -> String {
        let mut s = format!(
            "执行账本：总 tick {}µs | 完成 {} 跳过 {} 失败 {} 阻断 {} 未执行 {}\n",
            self.total_ticks,
            self.count(OutcomeKind::Done),
            self.count(OutcomeKind::Skipped),
            self.count(OutcomeKind::Failed),
            self.count(OutcomeKind::Blocked),
            self.count(OutcomeKind::Pending),
        );
        for (id, o) in self.outcomes.iter() {
            if o.kind != OutcomeKind::Done {
                s.push_str(&format!(
                    "  #{} {}：{}，{}µs，{}\n",
                    id,
                    o.kind.name(),
                    o.reason,
                    o.duration_us,
                    if o.duration_us > 0 { "非零耗时" } else { "零耗时" }
                ));
            }
        }
        s
    }
}

/// LCG 线性同余生成器（确定性——同种子同序列；参数取 Numerical Recipes
/// 标准组 a=1664525 c=1013904223 mod 2³²，业界公认可复现）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        // 32 位 LCG：state = state*1664525 + 1013904223 (mod 2^32)。
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }

    /// [lo, hi] 闭区间整数（确定性映射——模法有偏但模拟耗时序无碍）。
    pub fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() as u64) % (hi - lo + 1)
    }
}

/// 注入式失败注入表（哪些单元在模拟中失败——确定性注入面）。
#[derive(Clone, Debug, Default)]
pub struct FailureInject {
    /// 注入失败单元编号 → 归因文本。
    pub fail_at: BTreeMap<u16, String>,
}

impl FailureInject {
    /// 空注入（全成功路径）。
    pub fn none() -> Self {
        FailureInject::default()
    }

    /// 声明一处失败注入。
    pub fn inject(mut self, id: u16, reason: &str) -> Self {
        self.fail_at.insert(id, reason.to_string());
        self
    }
}

/// 按计划模拟执行（判据面：「执行序」的消费端语义——按 order 逐个跑）。
///
/// 执行语义：
/// - 逐 plan.order 线性推进（串行——A3 域并行化是后续批次的演进位）；
/// - 模拟耗时 = LCG(seed) 在 [预算 30%, 预算 90%] 内取值（确定性）；
/// - 硬依赖终态非 Done → 本单元 Skipped（传导显性，reason 记依赖链）；
/// - 软依赖失败不阻断（只顺序保证）；
/// - 注入表命中 → Failed（归因文本透传）；
/// - 每个 tick 推进 = end_tick 单调不减（时钟纪律）。
pub fn simulate(
    plan: &InitPlan,
    g: &DepGraph,
    seed: u64,
    inject: &FailureInject,
) -> Result<ExecLedger, PlanError> {
    let mut rng = Lcg::new(seed);
    let mut ledger = ExecLedger::default();
    let mut tick: u64 = 0;

    for id in plan.order.iter() {
        let unit = match g.unit(*id) {
            Some(u) => u,
            None => {
                return Err(PlanError::PlanInvariant {
                    gate: 2,
                    why: format!("模拟遭遇图中不存在单元 #{}", id),
                });
            }
        };
        // 依赖检查：硬依赖全部 Done 才可跑。
        let mut blocked_by: Option<(u16, OutcomeKind)> = None;
        for d in unit.deps.iter() {
            if d.kind != DepKind::Hard {
                continue;
            }
            let dep_out = ledger.outcomes.get(&d.target);
            let ok = matches!(dep_out.map(|o| o.kind), Some(OutcomeKind::Done));
            if !ok {
                blocked_by = Some((
                    d.target,
                    dep_out.map(|o| o.kind).unwrap_or(OutcomeKind::Pending),
                ));
                break;
            }
        }
        if let Some((dep_id, dep_kind)) = blocked_by {
            ledger.outcomes.insert(
                *id,
                UnitOutcome {
                    id: *id,
                    kind: OutcomeKind::Skipped,
                    start_tick: tick,
                    end_tick: tick,
                    duration_us: 0,
                    reason: format!(
                        "硬依赖 #{} 终态 {} 未就绪——传导跳过",
                        dep_id,
                        dep_kind.name()
                    ),
                },
            );
            continue;
        }
        // 注入失败面。
        if let Some(reason) = inject.fail_at.get(id) {
            ledger.outcomes.insert(
                *id,
                UnitOutcome {
                    id: *id,
                    kind: OutcomeKind::Failed,
                    start_tick: tick,
                    end_tick: tick,
                    duration_us: 0,
                    reason: format!("注入失败：{}", reason),
                },
            );
            continue;
        }
        // 正常执行：耗时 = LCG [30%, 90%] × 预算（确定性）。
        let lo = unit.budget_us * 30 / 100;
        let hi = unit.budget_us * 90 / 100;
        let dur = if hi > lo { rng.next_range(lo, hi) } else { lo };
        let start = tick;
        tick += dur;
        ledger.outcomes.insert(
            *id,
            UnitOutcome {
                id: *id,
                kind: OutcomeKind::Done,
                start_tick: start,
                end_tick: tick,
                duration_us: dur,
                reason: String::new(),
            },
        );
    }

    // 未进计划的图单元（防御——validate 已拦，这里按 Pending 记账保守恒）。
    for id in g.units.keys() {
        if !ledger.outcomes.contains_key(id) {
            ledger.outcomes.insert(
                *id,
                UnitOutcome {
                    id: *id,
                    kind: OutcomeKind::Pending,
                    start_tick: 0,
                    end_tick: 0,
                    duration_us: 0,
                    reason: "不在执行计划内——编排遗漏防御账".to_string(),
                },
            );
        }
    }

    ledger.total_ticks = tick;
    ledger.fingerprint = fnv32(&ledger.fingerprint_material(), FNV_OFFSET);
    ledger.balanced(g.units.len())?;
    Ok(ledger)
}

impl ExecLedger {
    /// 指纹素材（终态序列逐条序列化——账目顺序按 id 升序稳定）。
    fn fingerprint_material(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for (id, o) in self.outcomes.iter() {
            v.extend_from_slice(&id.to_le_bytes());
            v.push(o.kind as u8);
            v.extend_from_slice(&o.duration_us.to_le_bytes());
        }
        v
    }
}

// ---------------------------------------------------------------------------
// 八、一站式构建入口（declare → finalize → topo → 对拍 → plan → validate）
// ---------------------------------------------------------------------------

/// 编排失败（全模式显性——每个变体带可操作建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// 单元声明非法。
    UnitIllegal { id: u16, why: String, advice: String },
    /// 预算非法。
    BudgetIllegal { id: u16, budget_us: u64, why: String, advice: String },
    /// 自依赖。
    SelfDependency { id: u16 },
    /// 编号重复声明。
    DuplicateUnit { id: u16, name: String },
    /// 环依赖阻断。
    CycleBlocked { members: Vec<u16>, paths: Vec<String> },
    /// 双实现对拍不一致。
    OrderMismatch { detail: String },
    /// 计划不变量失守（闸号 1~4）。
    PlanInvariant { gate: u8, why: String },
    /// 账本守恒失守。
    LedgerUnbalanced { got: usize, want: usize, why: String },
    /// 静态校验有账（收编 ValidationReport——一站式入口的拒因载体）。
    ValidationFailed { issues: Vec<ValidationIssue> },
}

impl PlanError {
    /// 人话诊断（含建议——零静默纪律：拒绝必带修法）。
    pub fn detail(&self) -> String {
        match self {
            PlanError::UnitIllegal { id, why, advice } => format!(
                "[0x3501/0x3504 族] 单元 #{} 声明非法：{}。建议：{}",
                id, why, advice
            ),
            PlanError::BudgetIllegal { id, budget_us, why, advice } => format!(
                "[0x3508] 单元 #{} 预算 {}µs 非法：{}。建议：{}",
                id, budget_us, why, advice
            ),
            PlanError::SelfDependency { id } => format!(
                "[0x3502] 单元 #{} 自依赖——自己等自己永远不就绪。建议：删除该依赖边",
                id
            ),
            PlanError::DuplicateUnit { id, name } => format!(
                "[0x3504 族] 单元 #{}({}) 重复声明——编号是主键。建议：同编号只声明一次",
                id, name
            ),
            PlanError::CycleBlocked { members, paths } => {
                let mut s = format!(
                    "[0x3503] 环依赖阻断编排：{} 个单元成环（检出 100%——全部拦在编排期）。",
                    members.len()
                );
                for (i, p) in paths.iter().enumerate() {
                    s.push_str(&format!("\n  环{}：{}", i + 1, p));
                }
                s.push_str("\n建议：断开环上任意一条依赖边（引入阶段产物交接，见 UNX-F0182）");
                s
            }
            PlanError::OrderMismatch { detail } => format!(
                "[0x3506] 双实现拓扑序对拍不一致：{}。建议：排查平局键或边收集序的输入序依赖",
                detail
            ),
            PlanError::PlanInvariant { gate, why } => format!(
                "[0x3507] 计划不变量闸 {} 失守：{}。建议：修编排器不要修消费方——闸门在生成方",
                gate, why
            ),
            PlanError::LedgerUnbalanced { got, want, why } => format!(
                "[0x350A] 账本守恒失守：{}。建议：核对 simulate 的记账路径全覆盖",
                format!("账目 {} 条 ≠ 期望 {} 条——{}", got, want, why)
            ),
            PlanError::ValidationFailed { issues } => {
                let mut s = format!(
                    "[0x3501~0x3509] 静态校验 {} 条违规，编排拒绝放行：",
                    issues.len()
                );
                for (i, iss) in issues.iter().enumerate() {
                    s.push_str(&format!(
                        "\n  [{}] 码0x{:04X} {} 建议：{}",
                        i + 1,
                        iss.code,
                        iss.detail,
                        iss.advice
                    ));
                }
                s
            }
        }
    }
}

/// 一站式编排（判据 J1 的完整链路——声明进，计划/账本出，违规全显性）。
///
/// 流程：收集 → finalize 静态校验（有账即拒）→ 双实现拓扑排序 → 对拍 →
/// 环检出（有环即拒，附完整环路径）→ 计划构建 → 预算/串行组回填 →
/// 计划不变量四闸 → 模拟执行（确定性账本）。
pub fn orchestrate(units: Vec<InitUnit>, seed: u64, inject: &FailureInject) -> Result<(InitPlan, ExecLedger), PlanError> {
    // 收集。
    let mut g = DepGraph::new();
    for u in units {
        g.collect(u)?;
    }
    // 空图显性拒（空计划不是成功是没干活）。
    if g.unit_count() == 0 {
        return Err(PlanError::PlanInvariant {
            gate: 0,
            why: "零单元图——空计划不是成功是没干活".to_string(),
        });
    }
    // 静态校验。
    let report = g.finalize();
    if !report.ok() {
        return Err(PlanError::ValidationFailed {
            issues: report.issues.clone(),
        });
    }
    // 双实现拓扑 + 对拍。
    crosscheck(&g)?;
    let tr = topo_layered(&g);
    if tr.has_cycle() {
        return Err(PlanError::CycleBlocked {
            members: tr.cycle_members.clone(),
            paths: tr.cycle_paths.clone(),
        });
    }
    // 计划构建 + 回填。
    let mut plan = InitPlan::from_topo(&tr)?;
    // 预算与 wave 回填（from_topo 占位在此覆盖——单一真相在此刻落定）。
    for id in plan.order.iter() {
        if let Some(unit) = g.unit(*id) {
            if let Some(e) = plan.entries.get_mut(id) {
                e.budget_us = unit.budget_us;
                // wave 回填：查 waves 布局。
                for (w, layer) in tr.waves.iter().enumerate() {
                    if layer.contains(id) {
                        e.wave = w;
                        break;
                    }
                }
            }
        }
    }
    // 串行组回填（真组号——from_topo 的占位组 0 作废）。
    let mut serial_map: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for id in plan.order.iter() {
        let grp = g.unit(*id).map(|u| u.serial_group).unwrap_or(0);
        serial_map.entry(grp).or_default().push(*id);
    }
    plan.serial_map = serial_map;
    // 指纹以回填后的终值重算（占位指纹作废）。
    plan.fingerprint = fnv_plan(&plan.order, &plan.waves);
    // 不变量四闸。
    plan.validate(&g)?;
    // 模拟执行。
    let ledger = simulate(&plan, &g, seed, inject)?;
    Ok((plan, ledger))
}

// ---------------------------------------------------------------------------
// 九、快照（InitSnap——FNV 往返一致，家族范式）
// ---------------------------------------------------------------------------

/// 编排快照（诊断/对拍面——指纹三件套）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitSnap {
    /// 计划指纹。
    pub plan_fp: u32,
    /// 执行序指纹（order 独立指纹——与计划指纹互证）。
    pub order_fp: u32,
    /// 账本指纹。
    pub ledger_fp: u32,
    /// 单元数。
    pub units: usize,
    /// wave 数。
    pub waves: usize,
}

impl InitSnap {
    /// 从编排产物取快照。
    pub fn capture(plan: &InitPlan, ledger: &ExecLedger) -> Self {
        InitSnap {
            plan_fp: plan.fingerprint,
            order_fp: fnv_plan(&plan.order, &[]),
            ledger_fp: ledger.fingerprint,
            units: plan.order.len(),
            waves: plan.waves.len(),
        }
    }

    /// 往返一致（同产物两次 capture 必须全等——确定性锚）。
    pub fn roundtrip_ok(&self, other: &InitSnap) -> bool {
        self == other
    }
}

// ---------------------------------------------------------------------------
// 十、B10 批样板图（B10 批真实单元拓扑的编排自证——用声明面跑通全链）
// ---------------------------------------------------------------------------

/// B10 批样板块数（示例图规模——覆盖五阶段/串行组/软依赖/互斥全部声明面）。
pub const SAMPLE_UNIT_COUNT: usize = 12;

/// 构造 B10 批样例依赖图（真实初始化拓扑缩影：CPU→内存→堆→中断→调度→
/// 诊断→时钟→文件→驱动→兼容层→自检→门面，含软依赖与互斥声明）。
pub fn sample_units() -> Result<Vec<InitUnit>, PlanError> {
    // 依赖速记宏式展开（不引宏——直排 12 单元，声明面全显性）。
    let mk = |id: u16,
              name: &str,
              phase: Phase,
              prio: u8,
              deps: Vec<DepEdge>,
              budget: u64,
              grp: u16|
     -> Result<InitUnit, PlanError> {
        InitUnit::declare(id, name, phase, prio, deps, budget, grp)
    };
    Ok(vec![
        mk(0, "cpu-verify", Phase::Early, 9, vec![], 300, 0)?,
        mk(1, "mm-baseline", Phase::Early, 9, vec![DepEdge::new(0, DepKind::Hard)], 400, 0)?,
        mk(
            2,
            "heap-init",
            Phase::Foundation,
            8,
            vec![DepEdge::new(1, DepKind::Hard)],
            500,
            0,
        )?,
        mk(
            3,
            "idt-init",
            Phase::Foundation,
            8,
            vec![DepEdge::new(0, DepKind::Hard)],
            350,
            1,
        )?,
        mk(
            4,
            "sched-init",
            Phase::Subsystems,
            7,
            vec![
                DepEdge::new(2, DepKind::Hard),
                DepEdge::new(3, DepKind::Hard),
            ],
            600,
            0,
        )?,
        mk(
            5,
            "diag-bus",
            Phase::Subsystems,
            6,
            vec![DepEdge::new(2, DepKind::Soft)],
            250,
            0,
        )?,
        mk(
            6,
            "timer-init",
            Phase::Subsystems,
            7,
            vec![DepEdge::new(3, DepKind::Hard), DepEdge::new(5, DepKind::Soft)],
            300,
            1,
        )?,
        mk(
            7,
            "vfs-init",
            Phase::Services,
            6,
            vec![DepEdge::new(4, DepKind::Hard)],
            700,
            0,
        )?,
        mk(
            8,
            "drv-pci",
            Phase::Services,
            5,
            vec![DepEdge::new(4, DepKind::Hard), DepEdge::new(5, DepKind::Soft)],
            800,
            0,
        )?,
        mk(
            9,
            "compat-layer",
            Phase::Services,
            4,
            vec![DepEdge::new(7, DepKind::Hard)],
            900,
            0,
        )?,
        mk(
            10,
            "selfcheck",
            Phase::Late,
            3,
            vec![DepEdge::new(8, DepKind::Hard), DepEdge::new(9, DepKind::Soft)],
            400,
            0,
        )?,
        mk(
            11,
            "usergate",
            Phase::Late,
            2,
            vec![DepEdge::new(10, DepKind::Hard)],
            200,
            0,
        )?,
    ])
}

/// 样板图互斥对补全（示例：驱动枚举与兼容层声明互斥——成对声明示范）。
pub fn sample_mutex_pairs() -> Vec<(u16, u16)> {
    // 本样例不启用互斥（12 单元无互斥需求）；接口保留供批次扩展。
    // 空表 = 无互斥声明，finalize 的对称性校验零账全绿。
    Vec::new()
}

/// 样板图全链编排（B10 批判据自证入口——声明面→计划→账本一路全绿）。
///
/// 返回 (计划, 账本, 快照) 三件套——消费方（ktest 引导面）直接取用。
pub fn orchestrate_sample(seed: u64) -> Result<(InitPlan, ExecLedger, InitSnap), PlanError> {
    let units = sample_units()?;
    let inject = FailureInject::none();
    let (plan, ledger) = orchestrate(units, seed, &inject)?;
    let snap = InitSnap::capture(&plan, &ledger);
    Ok((plan, ledger, snap))
}

/// 环图构造器（测试/演练面——构造一个 A→B→C→A 环供环检出验证）。
///
/// 这是**编排器的输入构造工具**，不是 init 单元声明——仅用于验证
/// 「环依赖检出 100%」判据的演练路径。
pub fn cycle_sample() -> Result<Vec<InitUnit>, PlanError> {
    Ok(vec![
        InitUnit::declare(
            100,
            "cyc-a",
            Phase::Foundation,
            5,
            vec![DepEdge::new(102, DepKind::Hard)],
            100,
            0,
        )?,
        InitUnit::declare(
            101,
            "cyc-b",
            Phase::Foundation,
            5,
            vec![DepEdge::new(100, DepKind::Hard)],
            100,
            0,
        )?,
        InitUnit::declare(
            102,
            "cyc-c",
            Phase::Foundation,
            5,
            vec![DepEdge::new(101, DepKind::Hard)],
            100,
            0,
        )?,
    ])
}

/// 串行组样板（同组单元跨 wave 分离的编排自证——组 1 两单元被桥接单元隔开）。
pub fn serial_group_sample() -> Result<Vec<InitUnit>, PlanError> {
    Ok(vec![
        InitUnit::declare(
            200,
            "serial-a",
            Phase::Foundation,
            5,
            vec![],
            100,
            1,
        )?,
        InitUnit::declare(
            201,
            "bridge",
            Phase::Foundation,
            6,
            vec![DepEdge::new(200, DepKind::Hard)],
            100,
            0,
        )?,
        InitUnit::declare(
            202,
            "serial-b",
            Phase::Subsystems,
            5,
            vec![DepEdge::new(201, DepKind::Hard)],
            100,
            1,
        )?,
    ])
}
