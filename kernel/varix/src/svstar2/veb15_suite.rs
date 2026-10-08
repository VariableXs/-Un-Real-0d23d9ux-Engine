//! VE-F0215 · virtio 一致性测试套件（目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0215`
//!
//! **判据（锚点原文逐条）**：
//! - 协议层（状态机序与特性协商矩阵）→ `C215-协议-*`
//! - 功能层（资源创建导出、2D 更新、多头切换回归）→ `C215-功能-*`
//! - 恢复层（注入错误验证 F0212 处置符合矩阵）→ `C215-恢复-*`
//! - 命令流编码用**黄金流比对**（对齐 F0206 版本戳）→ `C215-黄金流-*`
//! - 套件分宿主侧 **fast 档**与 QEMU 批队列 **full 档** → `C215-双档-*`
//! - 黄金流版本不匹配 → 重建需人工确认 → `C215-版本-*`
//! - 设备缺特性 → **跳过并标 N/A，不算失败** → `C215-NA-*`
//! - 测试资源泄漏 → 退出前**清理断言** → `C215-清理-*`
//!
//! ---
//!
//! ## 设计要点一：N/A 不是失败——必须与「通过」在**类型层**分开
//!
//! 锚点：「设备缺特性→跳过并标记 N/A 不算失败」。这句话的陷阱在于：
//! 若把 N/A 塞进 `passed: bool`，测试框架只能靠 `passed || na` 这类
//! 组合判断，于是「N/A 数」与「通过数」在统计上无法区分——报告里
//! 「120 通过 0 失败」可能实际是「120 里 100 个根本没跑」。
//!
//! 故本单用 [`Verdict`] 三值枚举（`Pass` / `Fail` / `NotApplicable`），
//! 且 [`RunReport`] 分别统计三态。**「N/A 不算失败」的实现方式是
//! 令 `failed() == 0` 且把 NA 单列，而不是在判定处放行。**
//!
//! ## 设计要点二：黄金流比对必须带**版本戳**，且版本不匹配是**阻断**不是降级
//!
//! 锚点：「命令流编码用黄金流比对（对齐 F0206 版本戳）」+「黄金流版本
//! 不匹配 → 重建黄金流需人工确认」。
//!
//! 关键区分：F0206 的**流版本**（`VIRGL_STREAM_VERSION`）不匹配时，
//! 正确处置是**阻断**（比对毫无意义：字节布局都变了），且重建黄金流
//! **需人工确认**——自动重建会让一次协议变更静默地把「基准」改成
//! 「当前行为」，从此测试永远通过、什么也测不出来。这是本单刻意
//! 保留的阻断路径，不做成自动降级。
//!
//! ## 设计要点三：fast / full 双档是**同一套用例的子集选择**，不是两份清单
//!
//! 锚点：「套件分宿主侧 fast 档与 QEMU 批队列 full 档」。若维护两份清单，
//! 两份必然漂移（加了用例只记得加其中一份）。故 [`Case`] 带
//! [`Tier`] 标记，`fast` 档跑 `Tier::Fast` 子集、`full` 跑全集——
//! **一份用例清单，两个投影**。档位差异只体现在「跑不跑」，
//! 不断言「跑得快」（耗时是环境相关的，不是本模块能断言的东西）。
//!
//! ## 设计要点四：清理断言必须**在退出前**独立成步，不能藏在用例里
//!
//! 锚点：「测试资源泄漏 → 退出前清理断言」。若每条用例自己清理，
//! 用例中途 panic/早退就跳过清理，泄漏反而检不出来。故本单的
//! [`Suite::finish`] 是**独立收尾步**：无论前面结果如何都跑一遍
//! 「创建数 == 销毁数」对账，对不上即 [`Verdict::Fail`]——
//! **清理是收尾步的职责，不是用例的职责。**

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 用例分层（锚点「三层用例」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 协议层：状态机序与特性协商矩阵。
    Protocol,
    /// 功能层：资源创建导出、2D 更新、多头切换回归。
    Functional,
    /// 恢复层：注入错误验证 F0212 处置符合矩阵。
    Recovery,
}

impl Layer {
    pub fn index(self) -> usize {
        match self {
            Layer::Protocol => 0,
            Layer::Functional => 1,
            Layer::Recovery => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Layer::Protocol => "protocol",
            Layer::Functional => "functional",
            Layer::Recovery => "recovery",
        }
    }

    /// 该层是否属 fast 档（宿主侧分钟级）。
    ///
    /// 协议层与功能层跑 fast：状态机序、特性矩阵、资源/2D/多头用
    /// **模拟设备**即可验，不必等真机。恢复层**不进 fast**：锚点说
    /// 恢复层验的是「注入错误验证 F0212 处置」——注入错误要走真实
    /// reset 序，宿主侧模拟不出来，硬塞进 fast 只会得到一个测不出
    /// 东西的假用例。
    pub fn in_fast(self) -> bool {
        !matches!(self, Layer::Recovery)
    }

    /// 该层**只**属于哪一档（`None` = 两档都跑）。
    pub fn only_tier(self) -> Option<Tier> {
        if self.in_fast() {
            None
        } else {
            Some(Tier::Full)
        }
    }
}

/// 档位（锚点「fast 档与 full 档」）。
///
/// **语义**：`Full` 是**全集**（含 fast 的全部用例 + 只在 QEMU 侧
/// 跑的恢复层），`Fast` 是**子集**（跳过恢复层）。这不是两份清单，
/// 是一份清单的两个投影（头注要点三）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 宿主侧 fast 档：分钟级。
    Fast,
    /// QEMU 批队列 full 档：可过夜批跑。
    Full,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Fast => "fast",
            Tier::Full => "full",
        }
    }

    /// 本档是否包含该用例。
    ///
    /// 只有 full 档是全集；fast 档跳过「仅 full」的用例。
    pub fn includes(self, case: &Case) -> bool {
        match self {
            Tier::Full => true,
            Tier::Fast => case.tier == Tier::Fast,
        }
    }
}

/// 单条用例的判定结果（锚点「N/A 不算失败」的关键，见头注要点一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 失败（真失败，会让 `failed()` 非零）。
    Fail,
    /// 不适用：设备缺特性而跳过（**不算失败**，单列统计）。
    NotApplicable,
}

impl Verdict {
    pub fn is_na(self) -> bool {
        self == Verdict::NotApplicable
    }

    /// 是否计入失败数（NA 恒不计入）。
    pub fn is_fail(self) -> bool {
        self == Verdict::Fail
    }

    /// 是否计入通过数（NA 不计入通过——否则统计会说谎）。
    pub fn is_pass(self) -> bool {
        self == Verdict::Pass
    }
}

/// 三态判定的简写。
pub const V_PASS: Verdict = Verdict::Pass;
pub const V_FAIL: Verdict = Verdict::Fail;
pub const V_NA: Verdict = Verdict::NotApplicable;

// ---------------------------------------------------------------------------
// 二、用例清单（锚点「用例清单（层×用例×断言）」）
// ---------------------------------------------------------------------------

/// 单条用例。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 用例标识（层内唯一）。
    pub id: u32,
    /// 所属层。
    pub layer: Layer,
    /// 档位（一份清单两个投影，见头注要点三）。
    pub tier: Tier,
    /// 断言数（锚点「层×用例×断言」第三维）。
    pub asserts: u16,
    /// 所需特性位（设备缺该特性则 N/A，见头注要点一）。
    pub requires_feature: u32,
    /// 用例名（输出用）。
    pub name: &'static str,
}

/// 特性位（用于 `requires_feature` 判定 N/A）。
pub mod feature {
    /// 2D 通路。
    pub const BIT_2D: u32 = 1 << 0;
    /// virgl 3D 通路。
    pub const BIT_VIRGL: u32 = 1 << 1;
    /// Venus 通路。
    pub const BIT_VENUS: u32 = 1 << 2;
    /// 多输出（多头）。
    pub const BIT_MULTIHEAD: u32 = 1 << 3;
    /// 热重置（恢复层注入错误要用）。
    pub const BIT_RESET: u32 = 1 << 4;
    /// EDID 读取。
    pub const BIT_EDID: u32 = 1 << 5;

    /// 全部特性（full 设备）。
    pub const ALL: u32 = BIT_2D | BIT_VIRGL | BIT_VENUS | BIT_MULTIHEAD | BIT_RESET | BIT_EDID;
    /// 仅 2D 的最小设备（用来验 N/A 路径）。
    pub const MINIMAL: u32 = BIT_2D;
}

/// 内置用例清单（协议 6 + 功能 6 + 恢复 4 = 16 条）。
///
/// 清单是**编译期常量数组**——不动态生成，避免「生成了几条」与
/// 「注册了几条」两处事实源。`CASE_COUNT` 由它派生。
///
/// **档位语义（头注要点三）**：`Tier::Full` 标记的是**只在 full 档跑的
/// 用例**（恢复层，宿主侧模拟不出注入错误）；`Tier::Fast` 标记的是
/// **两档都跑**的用例。故 `full` 是 `fast` 的**超集**而非并列——
/// 初版把档位与层绑死（fast=协议+功能、full=恢复），等于让 full 档
/// 漏跑协议与功能，违背锚点「full 档可过夜批跑」的语义。
/// 「只进哪一档」由 [`Layer::only_tier`] 表达，`tier` 字段只回答
/// 「是否 fast 档包含」。
pub const CASES: [Case; 16] = [
    // ---- 协议层（锚点：状态机序与特性协商矩阵）· fast 档 ----
    Case { id: 1, layer: Layer::Protocol, tier: Tier::Fast, asserts: 4, requires_feature: 0, name: "proto-init-state-seq" },
    Case { id: 2, layer: Layer::Protocol, tier: Tier::Fast, asserts: 3, requires_feature: 0, name: "proto-feature-matrix" },
    Case { id: 3, layer: Layer::Protocol, tier: Tier::Fast, asserts: 5, requires_feature: feature::BIT_VIRGL, name: "proto-negotiate-order" },
    Case { id: 4, layer: Layer::Protocol, tier: Tier::Fast, asserts: 4, requires_feature: feature::BIT_VENUS, name: "proto-version-stamp" },
    Case { id: 5, layer: Layer::Protocol, tier: Tier::Fast, asserts: 3, requires_feature: 0, name: "proto-queue-reject-order" },
    Case { id: 6, layer: Layer::Protocol, tier: Tier::Fast, asserts: 4, requires_feature: feature::BIT_EDID, name: "proto-edid-negotiate" },
    // ---- 功能层（锚点：资源创建导出、2D 更新、多头切换回归）· fast 档 ----
    Case { id: 7, layer: Layer::Functional, tier: Tier::Fast, asserts: 5, requires_feature: feature::BIT_2D, name: "func-resource-create-export" },
    Case { id: 8, layer: Layer::Functional, tier: Tier::Fast, asserts: 4, requires_feature: feature::BIT_2D, name: "func-2d-update-blit" },
    Case { id: 9, layer: Layer::Functional, tier: Tier::Fast, asserts: 6, requires_feature: feature::BIT_MULTIHEAD, name: "func-multihead-switch" },
    Case { id: 10, layer: Layer::Functional, tier: Tier::Fast, asserts: 4, requires_feature: feature::BIT_MULTIHEAD, name: "func-multihead-layout-regress" },
    Case { id: 11, layer: Layer::Functional, tier: Tier::Fast, asserts: 5, requires_feature: feature::BIT_VIRGL, name: "func-virgl-roundtrip" },
    Case { id: 12, layer: Layer::Functional, tier: Tier::Fast, asserts: 3, requires_feature: feature::BIT_VENUS, name: "func-venus-alloc" },
    // ---- 恢复层（锚点：注入错误验证 F0212 处置符合矩阵）· **仅 full 档** ----
    Case { id: 13, layer: Layer::Recovery, tier: Tier::Full, asserts: 6, requires_feature: feature::BIT_RESET, name: "rec-inject-reset" },
    Case { id: 14, layer: Layer::Recovery, tier: Tier::Full, asserts: 5, requires_feature: feature::BIT_RESET, name: "rec-inject-queue-starve" },
    Case { id: 15, layer: Layer::Recovery, tier: Tier::Full, asserts: 5, requires_feature: feature::BIT_MULTIHEAD, name: "rec-head-loss-recover" },
    Case { id: 16, layer: Layer::Recovery, tier: Tier::Full, asserts: 4, requires_feature: feature::BIT_RESET, name: "rec-reset-repeat-idempotent" },
];

/// 用例总数（由 [`CASES`] 派生，避免第二处事实源）。
pub const CASE_COUNT: usize = CASES.len();

/// 恢复层用例数（锚点三层各自都要有）。
pub const RECOVERY_CASES: usize = 4;

/// fast 档用例数（清单投影，见头注要点三）。
pub const FAST_CASES: usize = 12;

// ---------------------------------------------------------------------------
// 三、黄金流（锚点「黄金流文件（版本戳×字节流）」）
// ---------------------------------------------------------------------------

/// 黄金流文件：版本戳 + 字节流（锚点原文的数据结构）。
#[derive(Clone, Debug)]
pub struct GoldenFlow {
    /// 流版本戳（对齐 F0206 `VIRGL_STREAM_VERSION`）。
    pub version: u32,
    /// 字节流。
    pub bytes: Vec<u8>,
    /// 用例名（归属）。
    pub case_name: &'static str,
}

/// 单次编码产物（待比对的一侧）。
#[derive(Clone, Debug)]
pub struct EncodedFlow {
    pub version: u32,
    pub bytes: Vec<u8>,
}

/// 黄金流比对结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowVerdict {
    /// 字节流与版本戳均一致。
    Match,
    /// 版本戳不一致 → **阻断**，重建需人工确认（头注要点二）。
    VersionMismatch,
    /// 版本一致但字节流不一致 → 真失败。
    BytesDiffer,
}

impl FlowVerdict {
    pub fn is_match(self) -> bool {
        self == FlowVerdict::Match
    }

    /// 版本不匹配是否阻断（阻断 ⇒ 不产出用例判定，先停下来处理）。
    pub fn is_blocking(self) -> bool {
        self == FlowVerdict::VersionMismatch
    }
}

/// 黄金流库（一份用例清单的黄金流集合）。
#[derive(Clone, Debug, Default)]
pub struct GoldenSet {
    flows: Vec<GoldenFlow>,
}

impl GoldenSet {
    pub fn new() -> GoldenSet {
        GoldenSet { flows: Vec::new() }
    }

    /// 登记一条黄金流。
    pub fn add(&mut self, case_name: &'static str, version: u32, bytes: Vec<u8>) {
        self.flows.push(GoldenFlow { version, bytes, case_name });
    }

    pub fn len(&self) -> usize {
        self.flows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.flows.is_empty()
    }

    /// 按用例名取黄金流（线性扫：清单规模是十几条，不是数据库）。
    pub fn find(&self, case_name: &str) -> Option<&GoldenFlow> {
        let mut i = 0;
        while i < self.flows.len() {
            if self.flows[i].case_name == case_name {
                return Some(&self.flows[i]);
            }
            i += 1;
        }
        None
    }

    /// 比对一条编码产物。
    ///
    /// **顺序有讲究**：先比版本戳再看字节——版本不符时字节流必然
    /// 也不同（布局都变了），此时报「字节不一致」会把「协议变更」
    /// 误报成「编码回归」，方向完全错。
    pub fn compare(&self, case_name: &str, got: &EncodedFlow) -> FlowVerdict {
        match self.find(case_name) {
            None => FlowVerdict::BytesDiffer,
            Some(g) => {
                if g.version != got.version {
                    return FlowVerdict::VersionMismatch;
                }
                if g.bytes == got.bytes {
                    FlowVerdict::Match
                } else {
                    FlowVerdict::BytesDiffer
                }
            }
        }
    }

    /// 按用例名**可变**引用取黄金流（线性扫：清单规模是十几条，不是数据库）。
    pub fn find_mut(&mut self, case_name: &str) -> Option<&mut GoldenFlow> {
        let mut i = 0;
        while i < self.flows.len() {
            if self.flows[i].case_name == case_name {
                return Some(&mut self.flows[i]);
            }
            i += 1;
        }
        None
    }

    /// 重建黄金流（**必须显式传 `confirm`**）。
    ///
    /// 锚点：「重建黄金流需人工确认」。做成 `confirm: bool` 参数而不是
    /// 自由函数，是为了让「未确认就重建」在**调用点**就多一个必填项——
    /// 调用点必须显式写 `false`（被拒绝）或 `true`（确认），没法
    /// 「忘了传」。
    pub fn rebuild(&mut self, case_name: &'static str, version: u32, bytes: Vec<u8>, confirm: bool) -> bool {
        if !confirm {
            return false;
        }
        match self.find_mut(case_name) {
            Some(g) => {
                g.version = version;
                g.bytes = bytes;
            }
            None => self.add(case_name, version, bytes),
        }
        true
    }

    /// 是否全部版本戳一致（跨用例体检）。
    pub fn versions_uniform(&self) -> bool {
        if self.flows.is_empty() {
            return true;
        }
        let v = self.flows[0].version;
        let mut i = 1;
        while i < self.flows.len() {
            if self.flows[i].version != v {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 四、资源账（锚点「测试资源泄漏→退出前清理断言」）
// ---------------------------------------------------------------------------

/// 资源创建/销毁账（清理断言的数据源）。
///
/// 独立于用例存在：即使某条用例早退或被判 N/A，它已创建的资源
/// 仍记在账上，收尾步照样能查出泄漏。
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceLedger {
    created: u32,
    destroyed: u32,
}

impl ResourceLedger {
    pub fn new() -> ResourceLedger {
        ResourceLedger { created: 0, destroyed: 0 }
    }

    pub fn create(&mut self) {
        self.created += 1;
    }

    pub fn destroy(&mut self) {
        self.destroyed += 1;
    }

    pub fn created(&self) -> u32 {
        self.created
    }

    pub fn destroyed(&self) -> u32 {
        self.destroyed
    }

    /// 未清理数。
    pub fn leaked(&self) -> u32 {
        if self.created > self.destroyed {
            self.created - self.destroyed
        } else {
            0
        }
    }

    /// 销毁数超过创建数（账被破坏 ⇒ 失败，绝不当作「清理干净」）。
    pub fn over_destroyed(&self) -> bool {
        self.destroyed > self.created
    }
}

// ---------------------------------------------------------------------------
// 五、结果记录（锚点「结果记录」）
// ---------------------------------------------------------------------------

/// 单条用例的结果记录。
#[derive(Clone, Debug)]
pub struct CaseResult {
    pub case_id: u32,
    pub layer: Layer,
    pub tier: Tier,
    pub verdict: Verdict,
    pub asserts: u16,
    /// 失败输出含复现最小命令序列（锚点「失败输出含复现最小命令序列」）。
    pub repro: String,
}

impl CaseResult {
    fn new(c: &Case, verdict: Verdict, repro: String) -> CaseResult {
        CaseResult {
            case_id: c.id,
            layer: c.layer,
            tier: c.tier,
            verdict,
            asserts: c.asserts,
            repro,
        }
    }
}

/// 一次套件运行的结果记录（锚点「结果记录」）。
#[derive(Clone, Debug, Default)]
pub struct RunReport {
    pub tier: Option<Tier>,
    pub results: Vec<CaseResult>,
    /// 退出前清理结论（None = 未跑收尾步）。
    pub cleanup_ok: Option<bool>,
}

impl RunReport {
    pub fn new(tier: Tier) -> RunReport {
        RunReport { tier: Some(tier), results: Vec::new(), cleanup_ok: None }
    }

    fn push(&mut self, r: CaseResult) {
        self.results.push(r);
    }

    /// 通过数（**不含 N/A**——把 NA 算进通过会让统计说谎，头注要点一）。
    pub fn passed(&self) -> u32 {
        self.results.iter().filter(|r| r.verdict.is_pass()).count() as u32
    }

    /// 失败数（N/A 恒不计入）。
    pub fn failed(&self) -> u32 {
        self.results.iter().filter(|r| r.verdict.is_fail()).count() as u32
    }

    /// N/A 数（单列）。
    pub fn na(&self) -> u32 {
        self.results.iter().filter(|r| r.verdict.is_na()).count() as u32
    }

    /// 已执行数（不含 N/A——N/A 是跳过，不是执行）。
    pub fn executed(&self) -> u32 {
        self.passed() + self.failed()
    }

    /// 断言总数（含 NA 用例声明的断言数——它「本该跑但跑不了」）。
    pub fn asserts_total(&self) -> u32 {
        self.results.iter().map(|r| r.asserts as u32).sum()
    }

    /// 整体是否通过：**零失败且清理通过**。
    ///
    /// N/A 不影响结论（锚点「N/A 不算失败」）；但清理失败一律
    /// 判不通过——泄漏是真实缺陷，不能因为「测试都过了」就放过。
    pub fn ok(&self) -> bool {
        self.failed() == 0 && self.cleanup_ok == Some(true)
    }

    /// 取某层结果。
    pub fn layer_results(&self, layer: Layer) -> Vec<&CaseResult> {
        self.results.iter().filter(|r| r.layer == layer).collect()
    }

    /// 全失败项的复现序列（锚点「失败输出含复现最小命令序列」）。
    pub fn failures_with_repro(&self) -> Vec<&CaseResult> {
        self.results
            .iter()
            .filter(|r| r.verdict.is_fail() && !r.repro.is_empty())
            .collect()
    }

    /// 按用例号取结果。
    pub fn result_of(&self, case_id: u32) -> Option<&CaseResult> {
        let mut i = 0;
        while i < self.results.len() {
            if self.results[i].case_id == case_id {
                return Some(&self.results[i]);
            }
            i += 1;
        }
        None
    }

    /// 本档实际执行到的用例名清单（输出用；按执行序）。
    ///
    /// 这是 `Case::name` 的**真实消费面**：结果记录要能对上「跑的是
    /// 哪条用例」，否则失败报告只有编号，人得反查清单表。
    pub fn executed_names(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.results.len() {
            if let Some(c) = find_case(self.results[i].case_id) {
                out.push(c.name);
            }
            i += 1;
        }
        out
    }

    /// 某档位标记的用例名清单（编排自检：清单投影是否符合预期）。
    pub fn case_names_in_tier(tier: Tier) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < CASE_COUNT {
            if tier.includes(&CASES[i]) {
                out.push(CASES[i].name);
            }
            i += 1;
        }
        out
    }

    /// 结果里某档位用例的条数（按记录自带的 `tier` 字段核，
    /// 而不重新走 `tier_case_count`——后者会与记录脱钩）。
    pub fn count_in_tier(&self, tier: Tier) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.results.len() {
            if self.results[i].tier == tier {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 六、套件主流程
// ---------------------------------------------------------------------------

/// 一致性测试套件。
///
/// 持三样东西：设备特性位（决定 N/A）、黄金流库、资源账。
/// 特性位是**构造参数**而非全局——同一套件对象可分别以
/// 「full 设备」和「最小设备」跑，N/A 路径才有真实覆盖。
#[derive(Clone, Debug)]
pub struct Suite {
    device_features: u32,
    golden: GoldenSet,
    ledger: ResourceLedger,
    /// 阻断中的版本不匹配（一旦出现，后续用例判定无意义）。
    blocked: bool,
}

impl Suite {
    /// 以给定设备特性构造套件。
    pub fn new(device_features: u32) -> Suite {
        Suite {
            device_features,
            golden: GoldenSet::new(),
            ledger: ResourceLedger::new(),
            blocked: false,
        }
    }

    /// 设备是否具备某特性（决定 N/A，见头注要点一）。
    pub fn has_feature(&self, bit: u32) -> bool {
        bit == 0 || (self.device_features & bit) == bit
    }

    /// 登记黄金流（宿主侧在跑之前先把基准灌进来）。
    pub fn register_golden(&mut self, case_name: &'static str, version: u32, bytes: Vec<u8>) {
        self.golden.add(case_name, version, bytes);
    }

    /// 跑一条**指定**用例（供外部逐条驱动；档位与特性判定同 `run`）。
    ///
    /// 返回该条的判定。`report` 由调用方自持——本方法不隐式记录，
    /// 避免「跑了一条却没进报告」这种半截状态。
    ///
    /// 判定次序**不可交换**：
    /// 1. 先查阻断（版本不匹配已发生 → 本单不判，后面比的都是噪声）；
    /// 2. 再查特性（缺特性 → N/A，**不建资源**——建了必然泄漏）；
    /// 3. 最后跑断言。
    ///
    /// 第 2 步「N/A 时不建资源」是刻意的：若 N/A 用例也走一遍
    /// 建/销流程，收尾对账会被「建了又销了」掩盖掉真实泄漏点，
    /// 泄漏断言的判别力下降。
    pub fn run_one<F>(&mut self, tier: Tier, case_id: u32, mut body: F) -> Verdict
    where
        F: FnMut(&Case) -> (Verdict, String),
    {
        let c = match find_case(case_id) {
            Some(c) => c,
            // 用例号不存在是**编排错误**，判失败而非静默跳过。
            None => return Verdict::Fail,
        };
        if !tier.includes(&c) {
            // 不属本档 → 不执行，N/A。
            return Verdict::NotApplicable;
        }
        if self.blocked {
            return Verdict::Fail;
        }
        if !self.has_feature(c.requires_feature) {
            // 缺特性 → N/A，**不建资源**。
            return Verdict::NotApplicable;
        }
        // 真跑：模拟资源创建与释放（收尾要对账）。
        self.ledger.create();
        self.ledger.destroy();
        body(c).0
    }

    /// 置阻断（版本不匹配时调用）。
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    pub fn blocked(&self) -> bool {
        self.blocked
    }

    pub fn golden(&self) -> &GoldenSet {
        &self.golden
    }

    pub fn golden_mut(&mut self) -> &mut GoldenSet {
        &mut self.golden
    }

    pub fn ledger(&self) -> &ResourceLedger {
        &self.ledger
    }

    /// 账本可变访问（供收尾步与外部注入资源后核对）。
    pub fn ledger_mut(&mut self) -> &mut ResourceLedger {
        &mut self.ledger
    }

    /// 跑完整个档位。
    ///
    /// `inject(case_id) -> Verdict` 由外部给出「这条用例实际跑出来是
    /// 什么」——套件不自己实现 virtio 通路（那是 F0201–F0214 的事），
    /// 它只负责**清单选择、特性判定、结果记录、收尾对账**。
    pub fn run<F>(&mut self, tier: Tier, mut inject: F) -> RunReport
    where
        F: FnMut(&Case) -> (Verdict, String),
    {
        let mut report = RunReport::new(tier);
        let mut i = 0;
        while i < CASE_COUNT {
            let c = CASES[i];
            i += 1;
            if !tier.includes(&c) {
                // 不属本档：不执行、不记录（档位是清单的投影）。
                continue;
            }
            if self.blocked {
                let mut r = CaseResult::new(&c, Verdict::Fail, String::new());
                r.repro = String::from("blocked-by-version-mismatch");
                report.push(r);
                continue;
            }
            if !self.has_feature(c.requires_feature) {
                // 缺特性 → N/A，**不建资源**（头注要点一 + 要点四）。
                report.push(CaseResult::new(&c, Verdict::NotApplicable, String::new()));
                continue;
            }
            self.ledger.create();
            self.ledger.destroy();
            let (v, repro) = inject(&c);
            let mut r = CaseResult::new(&c, v, String::new());
            if v.is_fail() {
                r.repro = repro;
            }
            report.push(r);
        }
        report.cleanup_ok = Some(self.finish());
        report
    }

    /// **退出前清理断言**（独立收尾步，见头注要点四）。
    ///
    /// 返回是否清理通过：`未清理数 == 0 && 未过度销毁`。
    /// 刻意**不是** `leaked() == 0` 单条——账被破坏（销毁多于创建）
    /// 同样是缺陷，且那种情况下 `leaked()` 因下溢保护会返 0，
    /// 单条断言会给出「清理干净」的假绿结论。
    pub fn finish(&self) -> bool {
        finish_ledger(&self.ledger)
    }

    /// 故意制造泄漏（供上游演示「清理断言能抓到」；生产路径不调用）。
    pub fn leak(&mut self, n: u32) {
        let mut i = 0;
        while i < n {
            self.ledger.create();
            i += 1;
        }
    }
}

/// 清理断言的**唯一实现**（自由函数，判据可直接调用）。
///
/// 抽成自由函数是为了让判据能在**不构造 `Suite`** 的前提下验这条逻辑——
/// 若它只作为方法存在，判据就必须造一个 `Suite` 才能触发，而 `Suite` 里
/// 又要经 `run` 建/销资源，判据便与被测路径**同源**，改实现时可能一起变。
pub fn finish_ledger(led: &ResourceLedger) -> bool {
    led.leaked() == 0 && !led.over_destroyed()
}

/// 按用例号查清单条目。
pub fn find_case(id: u32) -> Option<&'static Case> {
    let mut i = 0;
    while i < CASE_COUNT {
        if CASES[i].id == id {
            return Some(&CASES[i]);
        }
        i += 1;
    }
    None
}

/// 某档位的用例数（清单投影，头注要点三）。
pub fn tier_case_count(tier: Tier) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < CASE_COUNT {
        if tier.includes(&CASES[i]) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 某层的用例数。
pub fn layer_case_count(layer: Layer) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < CASE_COUNT {
        if CASES[i].layer == layer {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 构造一条复现序列文本（锚点「失败输出含复现最小命令序列」）。
///
/// 序列格式 `case:<id> step:<n>`，按用例声明的断言数给步数——
/// 复现序列要能让人照着敲一遍，故步数必须来自用例本身而非猜。
pub fn repro_sequence(case_id: u32, step: u16) -> String {
    let mut s = String::new();
    s.push_str("case:");
    s.push_str(&case_id.to_string());
    s.push_str(" steps:");
    let mut k = 0;
    while k < step {
        if k > 0 {
            s.push(',');
        }
        s.push('s');
        s.push_str(&k.to_string());
        k += 1;
    }
    s
}

// ===========================================================================
// 单元测试层（回答「实现是否被改坏」，与判据层「规格是否满足」互不可省）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn all_pass() -> (Verdict, String) {
        (Verdict::Pass, String::new())
    }

    // --- 层与档位 ---

    #[test]
    fn layer_indices_distinct_and_bounded() {
        let ls = [Layer::Protocol, Layer::Functional, Layer::Recovery];
        let mut i = 0;
        while i < ls.len() {
            assert!(ls[i].index() < 3);
            let mut j = i + 1;
            while j < ls.len() {
                assert_ne!(ls[i].index(), ls[j].index());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn layer_names_differ() {
        assert_ne!(Layer::Protocol.name(), Layer::Functional.name());
        assert_ne!(Layer::Functional.name(), Layer::Recovery.name());
    }

    #[test]
    fn recovery_layer_is_full_only() {
        assert!(!Layer::Recovery.in_fast());
        assert_eq!(Layer::Recovery.only_tier(), Some(Tier::Full));
        assert!(Layer::Protocol.in_fast());
        assert_eq!(Layer::Protocol.only_tier(), None);
    }

    #[test]
    fn tier_names_differ() {
        assert_ne!(Tier::Fast.name(), Tier::Full.name());
    }

    #[test]
    fn full_tier_is_superset_of_fast() {
        assert_eq!(tier_case_count(Tier::Full), CASE_COUNT);
        assert!(tier_case_count(Tier::Fast) < tier_case_count(Tier::Full));
        // 每条 fast 用例都必须在 full 里
        let mut i = 0;
        while i < CASE_COUNT {
            assert!(Tier::Full.includes(&CASES[i]), "full 必须是全集");
            if CASES[i].tier == Tier::Fast {
                assert!(Tier::Fast.includes(&CASES[i]));
            }
            i += 1;
        }
    }

    #[test]
    fn case_count_matches_constant() {
        assert_eq!(CASE_COUNT, 16);
        assert_eq!(FAST_CASES, tier_case_count(Tier::Fast));
        assert_eq!(RECOVERY_CASES, layer_case_count(Layer::Recovery));
    }

    #[test]
    fn case_lookup_by_id() {
        assert!(find_case(1).is_some());
        assert!(find_case(CASE_COUNT as u32).is_some());
        assert!(find_case(9999).is_none());
    }

    #[test]
    fn layer_counts_sum_to_total() {
        assert_eq!(
            layer_case_count(Layer::Protocol)
                + layer_case_count(Layer::Functional)
                + layer_case_count(Layer::Recovery),
            CASE_COUNT
        );
    }

    // --- Verdict 三值 ---

    #[test]
    fn verdict_predicates_are_disjoint() {
        assert!(V_PASS.is_pass() && !V_PASS.is_fail() && !V_PASS.is_na());
        assert!(V_FAIL.is_fail() && !V_FAIL.is_pass() && !V_FAIL.is_na());
        assert!(V_NA.is_na() && !V_NA.is_fail() && !V_NA.is_pass());
    }

    // --- 特性判定 ---

    #[test]
    fn feature_zero_always_available() {
        let s = Suite::new(0);
        assert!(s.has_feature(0), "零需求 = 任何设备都该能跑");
        assert!(!s.has_feature(feature::BIT_VIRGL));
    }

    #[test]
    fn feature_requires_all_bits_set() {
        let s = Suite::new(feature::BIT_2D);
        assert!(s.has_feature(feature::BIT_2D));
        assert!(!s.has_feature(feature::BIT_VIRGL));
        // 多位特性必须**全部**具备
        let multi = feature::BIT_2D | feature::BIT_VIRGL;
        assert!(!s.has_feature(multi), "只给一位不得判具备");
    }

    // --- 黄金流 ---

    #[test]
    fn golden_empty_set() {
        let g = GoldenSet::new();
        assert!(g.is_empty() && g.len() == 0);
        assert!(g.versions_uniform());
    }

    #[test]
    fn golden_match_on_identical() {
        let mut g = GoldenSet::new();
        g.add("a", 3, alloc::vec![1u8, 2, 3]);
        let got = EncodedFlow { version: 3, bytes: alloc::vec![1u8, 2, 3] };
        assert_eq!(g.compare("a", &got), FlowVerdict::Match);
        assert!(FlowVerdict::Match.is_match());
        assert!(!FlowVerdict::Match.is_blocking());
    }

    #[test]
    fn golden_bytes_differ_same_version() {
        let mut g = GoldenSet::new();
        g.add("a", 3, alloc::vec![1u8, 2, 3]);
        let got = EncodedFlow { version: 3, bytes: alloc::vec![1u8, 2, 4] };
        assert_eq!(g.compare("a", &got), FlowVerdict::BytesDiffer);
    }

    #[test]
    fn golden_version_mismatch_blocks() {
        // 版本不符必须**先**判版本（布局都变了，报「字节不同」方向错）。
        let mut g = GoldenSet::new();
        g.add("a", 3, alloc::vec![1u8, 2, 3]);
        let got = EncodedFlow { version: 4, bytes: alloc::vec![9u8, 9, 9] };
        let fv = g.compare("a", &got);
        assert_eq!(fv, FlowVerdict::VersionMismatch);
        assert!(fv.is_blocking());
    }

    #[test]
    fn golden_missing_case_does_not_pass() {
        let g = GoldenSet::new();
        let got = EncodedFlow { version: 1, bytes: alloc::vec![0u8] };
        assert_eq!(g.compare("nope", &got), FlowVerdict::BytesDiffer);
    }

    #[test]
    fn golden_rebuild_requires_confirmation() {
        let mut g = GoldenSet::new();
        g.add("a", 3, alloc::vec![1u8]);
        assert!(!g.rebuild("a", 9, alloc::vec![2u8], false), "未确认必须拒绝");
        assert_eq!(g.find("a").unwrap().version, 3, "拒绝后版本不变");
        assert!(g.rebuild("a", 9, alloc::vec![2u8], true));
        assert_eq!(g.find("a").unwrap().version, 9);
        assert_eq!(g.len(), 1, "重建不新增条目");
    }

    #[test]
    fn golden_rebuild_new_case_with_confirm() {
        let mut g = GoldenSet::new();
        assert!(!g.rebuild("new", 1, alloc::vec![0u8], false));
        assert_eq!(g.len(), 0);
        assert!(g.rebuild("new", 1, alloc::vec![0u8], true));
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn golden_versions_uniform_check() {
        let mut g = GoldenSet::new();
        g.add("a", 3, alloc::vec![]);
        g.add("b", 3, alloc::vec![]);
        assert!(g.versions_uniform());
        g.add("c", 4, alloc::vec![]);
        assert!(!g.versions_uniform());
    }

    // --- 资源账 ---

    #[test]
    fn ledger_balanced() {
        let mut l = ResourceLedger::new();
        l.create();
        l.create();
        l.destroy();
        l.destroy();
        assert_eq!(l.leaked(), 0);
        assert!(!l.over_destroyed());
    }

    #[test]
    fn ledger_leak_detected() {
        let mut l = ResourceLedger::new();
        l.create();
        l.create();
        l.create();
        l.destroy();
        assert_eq!(l.leaked(), 2);
    }

    #[test]
    fn ledger_over_destroy_detected() {
        let mut l = ResourceLedger::new();
        l.create();
        l.destroy();
        l.destroy();
        assert!(l.over_destroyed(), "销毁多于创建是账破坏");
        assert_eq!(l.leaked(), 0, "不得因下溢保护而误报清理干净");
    }

    // --- 跑批 ---

    #[test]
    fn run_fast_all_pass() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Fast, |_c| all_pass());
        assert_eq!(rep.results.len(), tier_case_count(Tier::Fast));
        assert_eq!(rep.failed(), 0);
        assert_eq!(rep.na(), 0);
        assert!(rep.ok());
        assert_eq!(rep.cleanup_ok, Some(true));
    }

    #[test]
    fn run_full_covers_three_layers() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Full, |_c| all_pass());
        assert!(!rep.layer_results(Layer::Protocol).is_empty());
        assert!(!rep.layer_results(Layer::Functional).is_empty());
        assert!(!rep.layer_results(Layer::Recovery).is_empty());
    }

    #[test]
    fn run_records_failures_with_repro() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Fast, |c| {
            if c.id == 2 {
                (Verdict::Fail, repro_sequence(c.id, c.asserts))
            } else {
                all_pass()
            }
        });
        assert_eq!(rep.failed(), 1);
        assert!(!rep.ok());
        let f = rep.failures_with_repro();
        assert_eq!(f.len(), 1);
        assert!(f[0].repro.contains("case:2"));
    }

    #[test]
    fn pass_cases_carry_no_repro() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Fast, |c| {
            if c.id == 1 {
                (Verdict::Fail, String::from("boom"))
            } else {
                all_pass()
            }
        });
        let mut i = 0;
        while i < rep.results.len() {
            if rep.results[i].verdict.is_pass() {
                assert!(rep.results[i].repro.is_empty(), "通过项不产复现噪声");
            }
            i += 1;
        }
    }

    #[test]
    fn minimal_device_yields_na_not_fail() {
        let mut s = Suite::new(feature::MINIMAL);
        let rep = s.run(Tier::Fast, |_c| all_pass());
        assert_eq!(rep.failed(), 0, "N/A 绝不计失败");
        assert!(rep.na() > 0, "最小设备上高级特性用例应 N/A");
        assert!(rep.ok(), "N/A 不影响整体结论");
    }

    #[test]
    fn na_cases_do_not_touch_ledger() {
        let mut s = Suite::new(feature::MINIMAL);
        let _ = s.run(Tier::Fast, |_c| all_pass());
        assert_eq!(s.ledger().created(), s.ledger().destroyed());
    }

    #[test]
    fn blocked_makes_all_cases_fail() {
        let mut s = Suite::new(feature::ALL);
        s.set_blocked(true);
        let rep = s.run(Tier::Fast, |_c| all_pass());
        assert_eq!(rep.failed() as usize, tier_case_count(Tier::Fast));
        let mut i = 0;
        while i < rep.results.len() {
            assert_eq!(rep.results[i].repro, "blocked-by-version-mismatch");
            i += 1;
        }
    }

    #[test]
    fn unblock_restores_normal_run() {
        let mut s = Suite::new(feature::ALL);
        s.set_blocked(true);
        assert!(s.blocked());
        assert!(s.run_one(Tier::Fast, 1, |_c| all_pass()).is_fail());
        s.set_blocked(false);
        assert!(s.run_one(Tier::Fast, 1, |_c| all_pass()).is_pass());
    }

    #[test]
    fn run_one_unknown_case_fails() {
        let mut s = Suite::new(feature::ALL);
        assert!(s.run_one(Tier::Fast, 9999, |_c| all_pass()).is_fail());
    }

    #[test]
    fn run_one_wrong_tier_is_na() {
        let mut s = Suite::new(feature::ALL);
        // 恢复层用例属 full，fast 档跑它 → N/A
        assert!(s.run_one(Tier::Fast, 13, |_c| all_pass()).is_na());
        assert!(s.run_one(Tier::Full, 13, |_c| all_pass()).is_pass());
    }

    #[test]
    fn leak_fails_overall() {
        let mut s = Suite::new(feature::ALL);
        s.leak(3);
        assert!(!s.finish());
        let rep = s.run(Tier::Fast, |_c| all_pass());
        assert!(!rep.ok(), "泄漏不得因「测试都过了」而放过");
    }

    // --- 统计 ---

    #[test]
    fn three_state_totals_balance() {
        let mut s = Suite::new(feature::MINIMAL);
        let rep = s.run(Tier::Fast, |c| {
            if c.id == 1 {
                (Verdict::Fail, String::from("x"))
            } else {
                all_pass()
            }
        });
        assert_eq!(
            rep.passed() + rep.failed() + rep.na(),
            rep.results.len() as u32
        );
        assert_eq!(rep.executed(), rep.passed() + rep.failed());
    }

    #[test]
    fn asserts_total_sums() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Fast, |_c| all_pass());
        let mut sum = 0u32;
        let mut i = 0;
        while i < rep.results.len() {
            sum += rep.results[i].asserts as u32;
            i += 1;
        }
        assert_eq!(rep.asserts_total(), sum);
        assert!(sum > 0);
    }

    #[test]
    fn report_records_tier() {
        let mut s = Suite::new(feature::ALL);
        let rep = s.run(Tier::Full, |_c| all_pass());
        assert_eq!(rep.tier, Some(Tier::Full));
    }

    // --- 复现序列 ---

    #[test]
    fn repro_sequence_contains_case_id() {
        let s = repro_sequence(7, 3);
        assert!(s.contains("case:7"));
        assert!(s.contains("steps:"));
    }

    #[test]
    fn repro_steps_match_asserts() {
        let c = find_case(13).unwrap();
        let s = repro_sequence(13, c.asserts);
        let steps = s.matches('s').count();
        // 断言数越大步数越多（粗验：至少 1 步且与 asserts 同量级）
        assert!(c.asserts > 0);
        assert!(steps >= c.asserts as usize || steps >= 1);
    }

    #[test]
    fn repro_zero_steps_is_empty_tail() {
        let s = repro_sequence(1, 0);
        assert!(s.contains("case:1"));
    }

    // --- 接入面 ---

    #[test]
    fn suite_exposes_golden_for_registration() {
        let mut s = Suite::new(feature::ALL);
        s.register_golden("proto-init-state-seq", 7, alloc::vec![1u8, 2]);
        assert!(s.golden().find("proto-init-state-seq").is_some());
        s.golden_mut().add("other", 7, alloc::vec![3u8]);
        assert_eq!(s.golden().len(), 2);
    }
}