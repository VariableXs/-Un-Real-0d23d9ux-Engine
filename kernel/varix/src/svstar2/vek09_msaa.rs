//! VE-F2009 ·抗锯齿四法之 MSAA（VE-K 域 · 后处理架构与 Bloom 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2009`
//!
//! **判据（锚点原文四条+ 判据条）**：
//! - **级别协商**：请求级 vs 实际级的**回退链**（8x→4x→2x→off），能力探测单源
//!   D09；协商结果必须**显性声明实际级别**（不是静默给一个 lesser 值）；
//! - **resolve 序位**：resolve 发生在 I02 主 pass 之后、F2001 管线序之前；
//!   后处理链**全1x**——若后处理节点读到 MSAA 数据即序位断言拦截；
//! - **前向约束声明**：前向路径=完整支持／延迟路径=**不启用**（诚实声明限制，
//!   不是"降级支持"）；每条限制带**理由**与**解锁条件**，不写"暂不支持"这种
//!   没有尽头的句子；
//! - **互斥守卫**：MSAA × TAA 二选一（F2011 联动），裁决带**被丢弃方**的具名
//!   记录与理由，不静默丢弃。
//!
//! **错误路径与降级矩阵（锚点原文四条）**：
//! 1. **请求级别硬件不支持** → 回退链自动降级 + **显性声明实际级别**；
//! 2. **resolve 与后处理序错配**（后处理读 MSAA 数据）→ 序位断言；
//! 3. **深度格式 MSAA 不支持**（特殊格式）→ **该格式降级 off + 告警**
//!    ——注意是降级到 **off** 而不是降到"颜色格式支持的那一级"：没有 MSAA 深度
//!    就无法做深度测试的每样本比较，几何边缘会穿帮成"颜色锯齿+ 深度错"，
//!    那比不开 MSAA 更糟；
//! 4. **MSAA 与 TAA 互斥**（同时启用冲突）→ 互斥守卫二选一。
//!
//! **设计要点（为什么这样写）**：
//! - **回退不是"往下取一个"而是"取≤请求的最大支持级"**：写成链式`while` 逐级
//!   下探在"设备支持集不连续"时会落到一个并不支持的级别上（例如设备只支持
//!   1x 与 8x，从 4x 请求下探到 2x 就停了，而 2x 其实不被支持）。本条用
//!   **支持集求交**而非链式下探，交集为空才落 off。
//! - **深度格式是独立的否决项，不是颜色级别的从属**：颜色支持 8x 而深度不支持
//!   时，正确的最终答案是 **off**。这不是"再降一级"，是"这个组合根本不可用"。
//!   把两者混成一个数字是本条最容易犯的错，且**不报任何错**——画面只是"看着
//!   有点不对"，人肉 review 抓不到。
//! - **路径约束的判定顺序刻意放在格式协商之前**：延迟路径 + 8x 请求，正确答案是
//!   off 且理由是"路径不支持"，而不是"设备只支持 4x"。理由不同，要修的地方
//!   也不同（换设备 vs 换路径）。把设计约束报在能力短板后面，人会去换设备——
//!   而那永远修不好。
//! - **MSAA 赢过 TAA 的理由是"状态性"不是"质量"**：MSAA 是**逐帧无状态**的，
//!   关掉它下一帧立刻干净；TAA 依赖历史缓冲，帧内关掉会让历史与当前错配，
//!   产生**一整段鬼影**。所以互斥裁决里被丢弃的应当是 TAA——丢TAA 的瞬态
//!   代价（首帧轻微鬼影）远小于丢 MSAA 的常态代价（全程几何锯齿）。
//! - **area 守恒是 resolve 的真判据**：只断言"resolve 输出有渐变"是弱门禁——
//!   把resolve 写成"取第一个样本"或"取平均值×0.9"都还能给出渐变。本条用
//!   **解析面积**对账：像素内一条过中心的垂直边缘解析覆盖恰为 1/2，resolve
//!   结果必须**精确等于**前景与背景的中点。取第一个样本 → 错；少算一个样本 →
//!   错；重复计数 → 错。这条断言对上述三种错误全部变红。
//! - **枚举判别值 ≠ 线上编码值**：`MsaaLevel::wire()` 是**显式映射**而不是
//!   `as u8`。判别序恰好是 0/1/2/3 而样本数是 1/2/4/8，两者只在前两项偶然
//!   相同；一旦 `as u8` 化并拿去和样本数比较，X4 会变成 2——静默错一半且类型
//!   检查不报。
//!
//! **跨批对接点**：
//! - **D09 能力单源**（跨域）：`DeviceCaps` 是能力查询的**唯一入口**，本条不
//!   自己探测硬件，只消费探测结果；`origin`字段区分"探测结果"与"默认假设"
//!   （同F2008 `TargetSpaceOrigin` 的纪律：默认不冒充探测）；
//! - **I02 主 pass**（跨域协作点）：resolve 序位声明的**上游锚点**是 I02 主
//!   pass，几何 pass 归I02，本条只声明"我在这之后"；
//! - **F2011 TAA**（互斥守卫的对方）：`guard_aa_mutex` 的 TAA 侧位是 F2011
//!   的挂载位——本条不实现 TAA，只实现"两者不能同时开"这条**关系**；
//! - **F2017 基准**：resolve 成本 0.3ms@1080p 4x 的**预算条目**在 F2017 定标，
//!   本条只提供成本模型与单源常量（见「性能诚实标注」）；
//! - **F1776 配额**：MSAA 显存（颜色 + 深度 × 样本数）入中间缓冲配额，
//!   超配额按三要素（当前/上限/建议）拒绝；
//! - **F2012 四法选型表**：本条提供 `msaa_profile()` 作为选型表的**数据源之一**。
//!
//! **性能诚实标注**：
//! `RESOLVE_COST_MS_1080P_4X` 是**锚点给出的预算值**，不是本机实测——本条在
//! 内核里没有可用的 GPU 计时器，声称"实测"是编数据。能被本条**真实验证**的是
//! 成本模型的**内部一致性**（样本数线性、像素数线性、级别单调），这部分写成
//! 判据；绝对值待F2017 实测后回填，回填前的口径在 `PERF_HONESTY_DECL` 里
//! 写明。**上游未达标不代改**：F2017 未定标前，本条不调常数去迎合任何数字。
//!
//! **无障碍与隐私**：无运行时隐私面。**无障碍侧**有一处真实影响：几何 MSAA
//! 关闭时，高对比度对角边缘（栅栏、屋顶、字形斜笔画）的阶梯在低视力与
//! 色觉障碍用户处更易与背景混淆，故级别降级/关闭在诊断上**显式登记**
//! `a11y_impact`——但同时**诚实限定**：本条的 MSAA 只作用于**几何 pass**，
//! UI 元素在 V 域合成、位于后处理之后，**不受 MSAA 影响**，所以这不是
//! "关了 MSAA 界面就不可读"，而是"关了 MSAA 内容边缘更硬"。把这两件事混为
//! 一谈会导致错误地要求 UI 也走 MSAA。
//!
//! 零外部依赖；零 IO；纯确定性函数 + 注入式状态；回归可复现；零 panic 面。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 一、契约与声明表
// ===========================================================================

/// MSAA 级别协商契约（锚点「交换链创建协商（请求级 vs 实际级回退链）」）。
///
/// **协商是四步而非一步**：① 路径否决 → ② 深度格式否决 → ③ 颜色级别求交 →
/// ④ 配额否决。顺序不可交换，理由写在各条注释里并由`K09-序位-` 与
/// `K09-协商-` 两族判据逐条锁住。
pub const MSAA_NEGOTIATION_DOC: &str = "\
MSAA 级别协商契约（VE-F2009 · 锚点「请求级 vs 实际级回退链8x→4x→2x→off」）：

| 步 | 判据 | 否决时结果 | 理由归属 |
| --- | --- | --- | --- |
| ① 路径 | 延迟路径 → MSAA 不启用 | off | 设计约束（换设备修不好）|
| ② 深度格式 | 该深度格式不支持 MSAA | off | 组合不可用（非「降一级」）|
| ③ 颜色级别 | 支持集∩ 请求级 | 支持集内最大级 | 能力短板（换设备可修）|
| ④ 配额 | MSAA 显存超 F1776 中间池 | 降级至配额内最大级 | 预算约束 |

**求交而非链式下探**：设备支持集**可以不连续**（D3D 规范允许只上报
1x/4x/8x 而不支持 2x）。链式下探在这种情况下会停在一个**未被声明支持**
的级别上——那不是降级不足，那是**编造能力**。故能力以`LevelSet` 位掩码
表达，求交即集合求交。

**显性声明是契约的一部分**：协商结果必须带 `declared_level` 与 `declaration`
文本。静默给一个 lesser 值等于让上层以为拿到了请求级——那会让后续所有
画质预算/账本条目建立在错误前提上。";

/// resolve 序位声明（锚点「序位声明（resolve 在 I02 主 pass 后 F2001 管线序前）」）。
///
/// **序位违规的两种形态**：① 后处理节点声明 sample_count > 1（**读 MSAA 数据**）；
/// ② MSAA 开启但序里没有 resolve 步骤。前者更常见也更隐蔽——后处理节点写
/// `sample_count: 4` 在类型上完全合法，只有本条的序位断言能抓住。
pub const RESOLVE_ORDER_DOC: &str = "\
resolve 序位声明（VE-F2009 · 锚点「resolve 在 I02 主 pass 后 F2001 管线序前」）：

  I02 主 pass（几何 MSAA 渲染）─▶ **resolve** ─▶ 后处理链（全 1x）─▶ V 域

| 序位规则 | 违规判据 | 后果 |
| --- | --- | --- |
| R1 resolve 必须在首个后处理节点之前 | 序中 resolve 下标 > 首个后处理下标 | 断言拦截 |
| R2 后处理节点必须 1x | 任一后处理节点 sample_count > 1 | 断言拦截（读 MSAA 数据）|
| R3 MSAA 开启必须有 resolve | granted>off 且序中无 resolve | 断言拦截（数据未解析）|
| R4 MSAA 关闭必须无 resolve | granted==off 且序中有 resolve | 断言拦截（多余解析）|
| R5 resolve 至多一次 | 序中 resolve 出现 >1 次 | 断言拦截（二次解析）|

R2 是锚点点名的「后处理读 MSAA 数据」。它写起来完全合法、编译通过、
画面也只是“边缘有点脏”，故必须写成断言而非注释。";

/// 路径 × MSAA 支持约束表（锚点「约束声明表（路径×MSAA 支持）」）。
///
/// **「不支持」必须带理由与解锁条件**：一句"暂不支持"没有可执行信息，
/// 读者无法判断这是设计决定还是尚未实现。本表两条限制都给了具体解锁条件。
pub const PATH_CONSTRAINT_TABLE_DOC: &str = "\
路径 × MSAA 支持约束表（VE-F2009 · 锚点「一期 MSAA 用于前向路径、延迟路径不启用」）：

| 渲染路径 | MSAA 支持 | 理由 | 解锁条件 |
| --- | --- | --- | --- |
| 前向（Forward） | **完整** | G-Buffer 不存在，几何 pass 直接产出 MSAA 颜色+深度，可逐样本解析 | 已实现 |
| 延迟（Deferred） | **不启用** | G-Buffer 的每样本可见性合并在几何 pass 之前已完成，边缘多采样在G-Buffer 写回时即被覆盖；MSAA 只能作用于「写 G-Buffer 之前」，而延迟路径没有这个阶段 | 需per-sample G-Buffer 写入路径（不在一期范围）|
| 延迟 + 边缘检测 AA | 不适用 | FXAA(TM 后) / TAA 覆盖该路径的边缘需求 | 见 F2010 / F2011 |

**诚实声明的两条边界**：
1. 「延迟路径不启用」是**设计约束**不是「未实现」——它有具体的物理理由
   （G-Buffer 合并早于解析），所以本条对延迟路径返回 off **且带理由**，
   不是返回一个 lesser 值假装“支持 4x”；
2. 本表**不评价** FXAA/TAA 在延迟路径上的质量——那是 F2010/F2011 的职责，
   本条只声明「不是本条的活」。";

/// 互斥守卫契约（锚点「MSAA 与 TAA 互斥（同时启用冲突）→ 互斥守卫二选一」）。
pub const AA_MUTEX_DOC: &str = "\
抗锯齿互斥守卫（VE-F2009 · 锚点「与 F2011 TAA 互斥守卫」）：

| MSAA | TAA | 裁决 | 被丢弃方 | 理由 |
| --- | --- | --- | --- | --- |
| off | off | 两者皆留 | — | 无 AA（显式接受锯齿）|
| >off | off | MSAA | — | 无冲突 |
| off | on | TAA | — | 无冲突 |
| >off | on | **MSAA** | **TAA** | MSAA 逐帧无状态；TAA 依赖历史缓冲，帧内丢弃产生整段鬼影 |

**被丢弃方必须具名记录**：静默丢弃会让用户以为 TAA 还开着（设置页显示已启用，
实际画面无时域累积），且关闭 TAA 不释放历史缓冲显存——配额账本会持续虚高。
故`MutexVerdict` 强制携带 `dropped` 与 `reason`。";

/// 性能诚实标注（锚点「性能逐项分解」+ 方法学纪律）。
///
/// **区分"预算"与"实测"**：本条给出成本**模型**（可验证）与成本**常量**
/// （未实测，锚点给定）。常量待 F2017 定标后回填；本条**不代改**上游，
/// 也不把预算值写成"实测值"——那会让下游基准拿到一个看起来可信的假数字。
pub const PERF_HONESTY_DECL: &str = "\
MSAA 成本声明（VE-F2009 · 模型可验证，常量待 F2017 定标）：

| 项 | 值 | 性质 | 本条可否验证 |
| --- | --- | --- | --- |
| resolve 1080p 4x | 0.3ms | **预算**（锚点给定）| 否（需GPU 计时器）|
| 成本 ∝ 样本数 | 线性 | 模型性质 | **是**（比值判据）|
| 成本 ∝ 像素数 | 线性 | 模型性质 | **是**（比值判据）|
| 8x/4x 成本比 | 2.0 | 模型性质 | **是**（比值判据）|
| 颜色显存 1080p 4x | 31.6MiB | **实算**（2,073,600 px × 4B × 4）| **是**（精确值）|
| 深度显存 1080p 4x | 31.6MiB | **实算**（同上，D32F 4B）| **是**（精确值）|
| MSAA 总显存倍数 | = 样本数 | 模型性质 | **是**（比值判据）|

**「实算」与「实测」的区别**：显存是纯算术（宽×高×字节×样本数），本条给出
精确值并断言；耗时依赖硬件，本条**只验证模型的一致性**（线性/单调），
绝对值标注为预算而非实测。回填前不得在文档或遥测里称其为实测。";

/// 跨批对接台账（锚点「跨批对接点」六项逐条登记）。
pub const CROSS_DOMAIN_LEDGER: &str = "\
跨批对接台账（VE-F2009）：

| 对接点 | 本条侧| 对方侧 | 关系 |
| --- | --- | --- | --- |
| D09 设备能力单源 | `DeviceCaps::query_format`（消费）| D09 探测器（产出）| 消费，不自探 |
| I02 主 pass | `RESOLVE_ORDER_DOC` 的上游锚 | I02 几何 pass | 协作点显式登记 |
| F2011 TAA | `guard_aa_mutex` 的 TAA 位（F2011 实现）| F2011 互斥守卫 | 关系实现，不实现对方 |
| F2017 基准 | `RESOLVE_COST_MS_1080P_4X`（未实测）| F2017 定标回填 | 预算→实测的待办 |
| F1776 配额 | `msaa_memory_bytes`（产出）| F1776 中间池 K 段（消费）| 显存入账 |
| F2012 选型表 | `msaa_profile()`（数据源）| F2012 决策表（汇总）| 供给数据 |";

/// 调试数据负载契约（锚点无此条，本条自设·理由见下）。
///
/// **为什么自设**：锚点把调试数据归F2013，但F2013 尚未开工；MSAA 协商的
/// 「请求级 vs 实际级」是**画质档位争议的第一现场**（用户说"我开了 4x 怎么
/// 没变"），没有具名负载就得靠猜。本条只提供**状态快照 + 逐字声明文本**，
/// 不开总线、不占用 F2013 的注册位——F2013 开工后本条改为转发。
pub const DEBUG_PAYLOAD_DOC: &str = "\
MSAA 调试负载（VE-F2009 自设·F2013 开工前过渡）：

| 字段 | 含义 |
| --- | --- |
| requested_level | 用户/档位请求的级别 |
| granted_level | 协商后实际生效的级别 |
| downgrade_reason | 降级理由（机器可读）|
| declaration | 人读声明文本（逐字用于设置页与告警）|
| caps_origin | 能力来源：探测/默认假设 |
| path | 当前渲染路径 |
| memory_bytes | MSAA 显存实算值 |

F2013 开工后：以上字段并入其「效果统计流」，本条保留`debug_text()`
作为同一份数据的文本投影，避免两处定义漂移。";

// ===========================================================================
// 二、诊断（本条独立诊断面；不与 F2004/F2006/F2008 重码）
// ===========================================================================

/// MSAA 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsaaDiagCode {
    /// 请求级别不被支持，已按回退链降级（显性声明实际级别）。
    LevelDowngraded,
    /// 延迟路径请求 MSAA → 不启用（设计约束，非能力不足）。
    DeferredPathUnsupported,
    /// 深度格式不支持 MSAA → 该格式降级 off。
    DepthFormatNoMsaa,
    /// 颜色格式支持集与请求级无交集 → 落 off。
    NoColorLevelSupported,
    /// MSAA 显存超配额 → 降级至配额内最大级。
    MemoryOverQuota,
    /// 序位违规：后处理节点声明了sample_count > 1（读 MSAA 数据）。
    PostReadsMsaaData,
    /// 序位违规：resolve 排在后处理之后。
    ResolveAfterPost,
    /// 序位违规：MSAA 开启但序中无 resolve。
    ResolveMissing,
    /// 序位违规：MSAA 关闭但序中有 resolve（多余解析）。
    ResolveRedundant,
    /// 序位违规：resolve 出现多次（二次解析）。
    ResolveDuplicated,
    /// 互斥守卫丢弃 TAA（被丢弃方具名记录，防"设置页说开着但没开"）。
    TaaDroppedByMutex,
    /// resolve 输入样本数与级别不符（实现/调用不一致）。
    SampleCountMismatch,
    /// 能力来源为默认假设而非探测结果。
    CapsOriginDefaulted,
}

impl MsaaDiagCode {
    /// 诊断码短名（遥测聚合键）。
    pub fn code(self) -> &'static str {
        match self {
            MsaaDiagCode::LevelDowngraded => "LEVEL_DOWNGRADED",
            MsaaDiagCode::DeferredPathUnsupported => "DEFERRED_PATH_UNSUPPORTED",
            MsaaDiagCode::DepthFormatNoMsaa => "DEPTH_FORMAT_NO_MSAA",
            MsaaDiagCode::NoColorLevelSupported => "NO_COLOR_LEVEL_SUPPORTED",
            MsaaDiagCode::MemoryOverQuota => "MEMORY_OVER_QUOTA",
            MsaaDiagCode::PostReadsMsaaData => "POST_READS_MSAA_DATA",
            MsaaDiagCode::ResolveAfterPost => "RESOLVE_AFTER_POST",
            MsaaDiagCode::ResolveMissing => "RESOLVE_MISSING",
            MsaaDiagCode::ResolveRedundant => "RESOLVE_REDUNDANT",
            MsaaDiagCode::ResolveDuplicated => "RESOLVE_DUPLICATED",
            MsaaDiagCode::TaaDroppedByMutex => "TAA_DROPPED_BY_MUTEX",
            MsaaDiagCode::SampleCountMismatch => "SAMPLE_COUNT_MISMATCH",
            MsaaDiagCode::CapsOriginDefaulted => "CAPS_ORIGIN_DEFAULTED",
        }
    }

    /// 下一处置提示（诊断要说实话：说清谁来处理、能不能修）。
    pub fn next_hint(self) -> &'static str {
        match self {
            MsaaDiagCode::LevelDowngraded => {
                "设备不支持请求的MSAA 级别：已按支持集降级，实际级别见声明文本；\
                 若画质档位依赖该级别，须同步下调档位表（否则档位与实际不一致）"
            }
            MsaaDiagCode::DeferredPathUnsupported => {
                "延迟路径不启用 MSAA（设计约束：G-Buffer 合并早于解析）：\
                 改用 FXAA(F2010) 或 TAA(F2011)；换设备无效"
            }
            MsaaDiagCode::DepthFormatNoMsaa => {
                "该深度格式不支持 MSAA：已降级 off（不是降一级——无 MSAA 深度则\
                 边缘穿帮）。换深度格式或关闭 MSAA"
            }
            MsaaDiagCode::NoColorLevelSupported => {
                "颜色格式支持集与请求级无交集：MSAA 关闭。换颜色格式"
            }
            MsaaDiagCode::MemoryOverQuota => {
                "MSAA 显存超F1776 中间池配额：已降级至配额内最大级；\
                 三要素见配额拒绝记录"
            }
            MsaaDiagCode::PostReadsMsaaData => {
                "后处理节点读取了 MSAA 数据：resolve 必须排在后处理之前（R2）；\
                 确认节点 sample_count 为 1"
            }
            MsaaDiagCode::ResolveAfterPost => {
                "resolve 排在后处理之后：I02 主 pass 之后、F2001 管线序之前才是\
                 resolve 的合法位置（R1）"
            }
            MsaaDiagCode::ResolveMissing => {
                "MSAA 已开启但序中无 resolve：数据未解析就进后处理，画面为\
                 未解析的多采样噪声（R3）"
            }
            MsaaDiagCode::ResolveRedundant => "MSAA 未开启却有 resolve：多余的一次全屏解析（R4）",
            MsaaDiagCode::ResolveDuplicated => {
                "resolve 出现多次：二次解析会把已解析结果再平均一次，画面整体\
                 对比度下降（R5）"
            }
            MsaaDiagCode::TaaDroppedByMutex => {
                "MSAA 与 TAA 互斥：已丢弃 TAA（MSAA 逐帧无状态，丢TAA 瞬态代价更小）。\
                 注意释放 TAA 历史缓冲，否则显存虚高"
            }
            MsaaDiagCode::SampleCountMismatch => {
                "resolve 输入的样本数与声明级别不符：调用点传错长度，\
                 本条已拒绝并返回 0（不静默用部分样本求平均）"
            }
            MsaaDiagCode::CapsOriginDefaulted => {
                "能力来源为默认假设而非探测结果：实际级别可能与设备不符；\
                 接入 D09 探测后由探测结果替代"
            }
        }
    }

    /// 是否属**可访问性**影响。
    ///
    /// **诚实限定**：只包含"几何 MSAA 被关闭/降级"这一族。UI 在 V 域合成、
    /// 位于后处理之后，**不受 MSAA 影响**——所以这不是"界面不可读"，
    /// 而是"内容的高对比对角边缘更硬、更易与背景混淆"。
    pub fn is_a11y_impact(self) -> bool {
        matches!(
            self,
            MsaaDiagCode::LevelDowngraded
                | MsaaDiagCode::DeferredPathUnsupported
                | MsaaDiagCode::DepthFormatNoMsaa
                | MsaaDiagCode::NoColorLevelSupported
                | MsaaDiagCode::MemoryOverQuota
        )
    }

    /// 是否属**阻断**（必须修，不能带着走）。
    ///
    /// **序位类全部阻断**：序位错配不会崩、不会报错，只让画面"边缘有点脏"——
    /// 这正是它危险的地方，故全部按阻断处理。
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            MsaaDiagCode::PostReadsMsaaData
                | MsaaDiagCode::ResolveAfterPost
                | MsaaDiagCode::ResolveMissing
                | MsaaDiagCode::ResolveRedundant
                | MsaaDiagCode::ResolveDuplicated
                | MsaaDiagCode::SampleCountMismatch
        )
    }
}

/// 单条诊断。
#[derive(Clone, Debug, PartialEq)]
pub struct MsaaDiagnostic {
    /// 诊断码。
    pub code: MsaaDiagCode,
    /// 上下文数值（级别样本数 / 字节数 / 下标，含义随码而定）。
    pub context: f32,
    /// 人类可读描述。
    pub text: String,
    /// 是否影响可访问性。
    pub a11y_impact: bool,
}

impl MsaaDiagnostic {
    /// 构造。
    pub fn new(code: MsaaDiagCode, context: f32) -> Self {
        MsaaDiagnostic {
            code,
            context,
            text: format!("[{}] {}", code.code(), code.next_hint()),
            a11y_impact: code.is_a11y_impact(),
        }
    }
}

/// 诊断袋（本条独立诊断面）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MsaaDiagBag {
    items: Vec<MsaaDiagnostic>,
}

impl MsaaDiagBag {
    /// 空袋。
    pub fn new() -> Self {
        MsaaDiagBag { items: Vec::new() }
    }

    /// 压入一条。
    pub fn push(&mut self, d: MsaaDiagnostic) {
        self.items.push(d);
    }

    /// 记一条。
    pub fn note(&mut self, code: MsaaDiagCode, context: f32) {
        self.push(MsaaDiagnostic::new(code, context));
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否含某码。
    pub fn has(&self, code: MsaaDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 阻断项计数。
    pub fn blocking_count(&self) -> usize {
        self.items.iter().filter(|d| d.code.is_blocking()).count()
    }

    /// 可访问性影响项计数。
    pub fn a11y_count(&self) -> usize {
        self.items.iter().filter(|d| d.a11y_impact).count()
    }

    /// 全部码（去重后按出现顺序）。
    pub fn codes(&self) -> Vec<MsaaDiagCode> {
        let mut out: Vec<MsaaDiagCode> = Vec::new();
        for d in &self.items {
            if !out.contains(&d.code) {
                out.push(d.code);
            }
        }
        out
    }

    /// 可读报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("MSAA 诊断：");
        s.push_str(&self.items.len().to_string());
        s.push_str(" 条（阻断 ");
        s.push_str(&self.blocking_count().to_string());
        s.push_str("，可访问性影响 ");
        s.push_str(&self.a11y_count().to_string());
        s.push_str("）");
        for d in &self.items {
            s.push_str("\n  ");
            s.push_str(&d.text);
        }
        s
    }
}

// ===========================================================================
// 三、渲染路径与级别
// ===========================================================================

/// 渲染路径（锚点「约束声明表（路径 × MSAA 支持）」的枚举化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPath {
    /// 前向路径：MSAA 完整支持。
    Forward,
    /// 延迟路径：MSAA **不启用**（设计约束）。
    Deferred,
}

impl RenderPath {
    /// 全部取值。
    pub fn all() -> [RenderPath; 2] {
        [RenderPath::Forward, RenderPath::Deferred]
    }

    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            RenderPath::Forward => "forward",
            RenderPath::Deferred => "deferred",
        }
    }

    /// 人读标签。
    pub fn label(self) -> &'static str {
        match self {
            RenderPath::Forward => "前向渲染",
            RenderPath::Deferred => "延迟渲染",
        }
    }

    /// 本路径是否支持 MSAA（锚点：前向=完整／延迟=不支持）。
    pub fn supports_msaa(self) -> bool {
        self == RenderPath::Forward
    }

    /// 不支持时的理由（供诊断与声明文本引用，避免两处各写一句）。
    pub fn unsupported_reason(self) -> &'static str {
        match self {
            RenderPath::Forward => "",
            RenderPath::Deferred => {
                "G-Buffer 的每样本可见性合并发生在几何 pass 之前，\
                 MSAA 只能作用于 G-Buffer 写入之前，而延迟路径没有该阶段"
            }
        }
    }

    /// 不支持时的解锁条件（避免写「暂不支持」这种没有尽头的句子）。
    pub fn unlock_condition(self) -> &'static str {
        match self {
            RenderPath::Forward => "已支持",
            RenderPath::Deferred => "需per-sample G-Buffer 写入路径（不在一期范围）",
        }
    }
}

/// MSAA 级别（锚点「2x/4x/8x 与硬件能力探测协商」+ 回退链含 off）。
///
/// **判别值 ≠ 线上编码值**：判别序为 off/x2/x4/x8，样本数为 1/2/4/8，
/// 只有前两项偶然相同。`wire()` 是显式映射，`as u8` 禁用（理由见头注）。
///
/// **不派生 `PartialOrd`**：协商路径需要在 `const fn` 里做大小比较，
/// 而派生比较在 const 上下文不可调用（E0015）。排序语义由显式 `rank()`
/// 提供——它同时是 `bit()` 的定义依据，两者恒等由判据断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsaaLevel {
    /// 关闭。
    Off,
    /// 二倍采样。
    X2,
    /// 四倍采样。
    X4,
    /// 八倍采样。
    X8,
}

impl MsaaLevel {
    /// 级别全集规模。
    pub const TABLE_SIZE: usize = 4;

    /// 全部取值（**降序**——回退链的求交需要降序遍历）。
    pub const fn all_desc() -> [MsaaLevel; 4] {
        [MsaaLevel::X8, MsaaLevel::X4, MsaaLevel::X2, MsaaLevel::Off]
    }

    /// 全部取值（升序）。
    pub const fn all_asc() -> [MsaaLevel; 4] {
        [MsaaLevel::Off, MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8]
    }

    /// 每像素样本数（1 / 2 / 4 / 8）。
    ///
    /// **不是判别值 +1**：`Off as u8 == 0`，`0 + 1 == 1` 恰好对；但
    /// `X2 as u8 == 1`，`1 + 1 == 2` 也恰好对——**两个"恰好"叠在一起就是
    /// 一段没人验证过的巧合**。`X4 as u8 == 2` 而真值是 4，静默错一半。
    pub const fn samples(self) -> u32 {
        match self {
            MsaaLevel::Off => 1,
            MsaaLevel::X2 => 2,
            MsaaLevel::X4 => 4,
            MsaaLevel::X8 => 8,
        }
    }

    /// **线上编码值**（写入交换链描述 / 配置序列化用）。
    ///
    /// 显式映射，与 `samples()` 分开声明。两者必须恒等，但**恒等是断言出来的
    /// 事实**而不是构造上的保证——所以 `K09-协商-编码自洽` 逐级断言。
    pub const fn wire(self) -> u8 {
        match self {
            MsaaLevel::Off => 1,
            MsaaLevel::X2 => 2,
            MsaaLevel::X4 => 4,
            MsaaLevel::X8 => 8,
        }
    }

    /// 由线上编码值反解。
    ///
    /// 返回 `None` 而不是夹取：夹取会让"配置里写了 3x"变成"悄悄按 4x 跑"，
    /// 而配置文件里的错误应该显性失败。
    pub const fn from_wire(w: u8) -> Option<MsaaLevel> {
        match w {
            1 => Some(MsaaLevel::Off),
            2 => Some(MsaaLevel::X2),
            4 => Some(MsaaLevel::X4),
            8 => Some(MsaaLevel::X8),
            _ => None,
        }
    }

    /// 稳定短名（设置页 / 遥测键）。
    pub const fn tag(self) -> &'static str {
        match self {
            MsaaLevel::Off => "off",
            MsaaLevel::X2 => "2x",
            MsaaLevel::X4 => "4x",
            MsaaLevel::X8 => "8x",
        }
    }

    /// 人读标签。
    pub const fn label(self) -> &'static str {
        match self {
            MsaaLevel::Off => "关闭（无几何抗锯齿）",
            MsaaLevel::X2 => "2 倍采样",
            MsaaLevel::X4 => "4 倍采样",
            MsaaLevel::X8 => "8 倍采样",
        }
    }

    /// 是否开启 MSAA。
    pub const fn enabled(self) -> bool {
        !matches!(self, MsaaLevel::Off)
    }

    /// 由样本数反解级别（用于解析 resolve 输出的实际样本数）。
    pub const fn from_samples(n: u32) -> Option<MsaaLevel> {
        match n {
            1 => Some(MsaaLevel::Off),
            2 => Some(MsaaLevel::X2),
            4 => Some(MsaaLevel::X4),
            8 => Some(MsaaLevel::X8),
            _ => None,
        }
    }

    /// 在 `LevelSet` 位掩码中的位序号（`off`=0 … `8x`=3）。
    ///
    /// **显式映射而非 `as u8`**：判别值恰为 0/1/2/3，与位序号一一相同——
    /// 但那是**巧合**：一旦将来在枚举中间插入新级别，`as u8` 给出的位序就变了，
    /// 而位序变化会**静默改变已存配置与已存能力表的解释**（同一个掩码值
    ///  suddenly 指向另一个级别，不报任何错）。显式映射 + `K09-协商-位序自洽`
    /// 断言把这类改动挡住。
    pub const fn bit(self) -> u8 {
        self.rank()
    }

    /// 排序秩（`off`=0 … `8x`=3）。**排序语义的唯一来源**。
    ///
    /// 与 `bit()` 是同一个映射的两种叫法——刻意不分叉：分叉出两个映射后，
    /// 它们总有一天会不一致，而那种不一致表现为「能力表说支持 4x，
    /// 协商却按 8x 处理」，不报任何错。
    pub const fn rank(self) -> u8 {
        match self {
            MsaaLevel::Off => 0,
            MsaaLevel::X2 => 1,
            MsaaLevel::X4 => 2,
            MsaaLevel::X8 => 3,
        }
    }

    /// 是否 ≤ `other`（const 可用的显式比较）。
    pub const fn le(self, other: MsaaLevel) -> bool {
        self.rank() <= other.rank()
    }

    /// 是否 ≥ `other`（const 可用的显式比较）。
    pub const fn ge(self, other: MsaaLevel) -> bool {
        self.rank() >= other.rank()
    }
}

// ===========================================================================
// 四、D09 能力单源（消费侧）
// ===========================================================================

/// 颜色格式（能力查询的键之一）。
///
/// **枚举化而非字符串**：字符串比较会因大小写/别名（"rgba8"/"RGBA8_UNORM"）
/// 静默失配，而失配的表现是"查不到支持 →落off"——一个**看起来合理**的降级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFormat {
    /// RGBA8 UNORM（8 位每通道）。
    Rgba8Unorm,
    /// BGRA8 UNORM（部分后端原生）。
    Bgra8Unorm,
    /// R11G11B10 浮点。
    R11G11B10Float,
    /// R16 浮点（无alpha，常用于 HDR 中间 RT）。
    R16Float,
}

impl ColorFormat {
    /// 全部取值。
    pub const fn all() -> [ColorFormat; 4] {
        [
            ColorFormat::Rgba8Unorm,
            ColorFormat::Bgra8Unorm,
            ColorFormat::R11G11B10Float,
            ColorFormat::R16Float,
        ]
    }

    /// 每像素字节数。
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            ColorFormat::Rgba8Unorm | ColorFormat::Bgra8Unorm => 4,
            ColorFormat::R11G11B10Float => 4,
            ColorFormat::R16Float => 8, //双通道（R16G16）= 8B
        }
    }

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            ColorFormat::Rgba8Unorm => "rgba8unorm",
            ColorFormat::Bgra8Unorm => "bgra8unorm",
            ColorFormat::R11G11B10Float => "r11g11b10float",
            ColorFormat::R16Float => "r16float",
        }
    }
}

/// 深度格式（能力查询的键之二，**独立否决项**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthFormat {
    /// D32 浮点。
    D32Float,
    /// D24 UNORM + S8（打包）。
    D24UnormS8Uint,
    /// D16 UNORM（轻量）。
    D16Unorm,
}

impl DepthFormat {
    /// 全部取值。
    pub const fn all() -> [DepthFormat; 3] {
        [
            DepthFormat::D32Float,
            DepthFormat::D24UnormS8Uint,
            DepthFormat::D16Unorm,
        ]
    }

    /// 每像素字节数。
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            DepthFormat::D32Float => 4,
            DepthFormat::D24UnormS8Uint => 4,
            DepthFormat::D16Unorm => 2,
        }
    }

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            DepthFormat::D32Float => "d32float",
            DepthFormat::D24UnormS8Uint => "d24unorms8",
            DepthFormat::D16Unorm => "d16unorm",
        }
    }
}

/// 能力来源（锚点「D09 联动：设备能力单源」+ 同F2008 的来源纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapsOrigin {
    /// 来自 D09 探测结果——可信。
    Probed,
    /// 默认假设（未探测）——**不冒充探测结果**。
    Defaulted,
}

/// 级别支持位掩码（bit0=off/1x, bit1=2x, bit2=4x, bit3=8x）。
///
/// **为什么是位掩码而不是「最大级别」**：真实硬件的能力集**可以不连续**
/// （某些后端支持 1x/4x/8x 而不支持 2x；D3D 规范确实允许这种上报）。
/// 用 `color_max: MsaaLevel` 表达能力时，非连续支持集**无法表达**——
/// 只能写成max=8x，而求交函数会据此认定 2x 也支持，于是给出一个
/// **设备从未声明支持**的级别。那不是降级不足，那是**编造能力**。
///
/// 位掩码让「支持集」成为一个可精确陈述的集合，求交即真实交集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelSet(pub u8);

impl LevelSet {
    /// 空集（什么都不支持）。
    pub const fn empty() -> LevelSet {
        LevelSet(0)
    }

    /// 由级别构造单元素集。
    pub const fn only(level: MsaaLevel) -> LevelSet {
        LevelSet(1u8 << level.bit())
    }

    /// 原始掩码。
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// 由掩码构造。
    ///
    /// **高位（bit≥4）一律丢弃**：掩码里出现未定义位说明能力表被写坏了，
    /// 悄悄忽略等于让一个错误的位不起作用，而错误位本该被发现。
    /// 丢弃是安全方向（能力只会更少，不会凭空多出支持）。
    pub const fn from_bits(bits: u8) -> LevelSet {
        LevelSet(bits & 0x0F)
    }

    /// 位序号 → 级别（位掩码的反查）。
    ///
    /// **`>= 4` 落到 8x** 是有意的：这类输入来自 `from_bits` 未过滤的脏数据
    /// 或越界移位，落在一个**真实存在**的级别上比落在一个不存在的级别上
    /// 安全——`LevelSet::from_bit(9)` 若返回 `None` 会让调用点多出一堆
    /// `Option` 分支，而脏数据本该在写入侧（`from_bits`）就被拦掉。
    pub const fn from_bit(bit: u8) -> MsaaLevel {
        match bit {
            0 => MsaaLevel::Off,
            1 => MsaaLevel::X2,
            2 => MsaaLevel::X4,
            _ => MsaaLevel::X8,
        }
    }

    /// 是否含某级别。
    pub const fn contains(self, level: MsaaLevel) -> bool {
        (self.0 >> level.bit()) & 1 == 1
    }

    /// 插入某级别。
    pub const fn with(self, level: MsaaLevel) -> LevelSet {
        LevelSet(self.0 | (1u8 << level.bit()))
    }

    /// 是否为空。
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 是否支持任一 MSAA 级别（bit≥1）。
    pub const fn any_msaa(self) -> bool {
        self.0 & 0x0E != 0
    }

    /// 与另一集合求交。
    pub const fn intersect(self, other: LevelSet) -> LevelSet {
        LevelSet(self.0 & other.0)
    }

    /// 是否包含另一集合的全部元素（用于「声明的级别必须真被声明过」这类断言）。
    pub const fn is_superset_of(self, other: LevelSet) -> bool {
        (self.0 & other.0) == other.0
    }

    /// 集合内**最大**的级别（空集返回 `Off`——`Off` 恒被任何真实能力集包含，
    /// 作为空集的"兜底"不会凭空产生支持）。
    pub const fn max_level(self) -> MsaaLevel {
        if self.contains(MsaaLevel::X8) {
            MsaaLevel::X8
        } else if self.contains(MsaaLevel::X4) {
            MsaaLevel::X4
        } else if self.contains(MsaaLevel::X2) {
            MsaaLevel::X2
        } else {
            MsaaLevel::Off
        }
    }

    /// 集合内**≤ 上界**的最大级别（`None` 表示除 `Off` 外无解）。
    ///
    /// **用显式秩比较而非派生的 `PartialOrd`**：派生比较在 `const fn` 里
    /// 不可调用（E0015），而本函数需要在编译期求值的场景不多、但调用点
    /// 遍布协商路径。`rank()` 是显式映射，`K09-协商-位序自洽` 断言它与
    /// 判别序一致——派生序与显式秩一旦漂移，那条判据会红。
    pub const fn max_at_most(self, bound: MsaaLevel) -> Option<MsaaLevel> {
        let b = bound.rank();
        let mut i = MsaaLevel::X8.rank();
        loop {
            let lv = LevelSet::from_bit(i);
            if self.contains(lv) && lv.rank() <= b {
                return Some(lv);
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        None
    }

    /// 集合内级别个数（`Off` 计入）。
    pub const fn count(self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < 4 {
            n += ((self.0 >> i) & 1) as u32;
            i += 1;
        }
        n
    }

    /// 可读列表（如 `1x/4x/8x`）。
    pub fn text(self) -> String {
        let mut s = String::new();
        for lv in MsaaLevel::all_asc().iter() {
            if self.contains(*lv) {
                if !s.is_empty() {
                    s.push('/');
                }
                s.push_str(lv.tag());
            }
        }
        if s.is_empty() {
            s.push_str("无");
        }
        s
    }
}

/// 单格式支持集条目（颜色 + 深度的**配对**能力）。
///
/// **成对而不是两个独立查询**：MSAA 要求颜色与深度**同时**支持同一级别，
/// 分开查询再由调用方合并，会让“颜色 8x / 深度 4x”这种组合以错误的方式
/// 可行。能力以**配对 + 集合**形式给出，从数据结构上就消除了那种误用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatSupport {
    /// 颜色格式。
    pub color: ColorFormat,
    /// 深度格式。
    pub depth: DepthFormat,
    /// 该配对下颜色侧支持的**级别集合**（可非连续）。
    pub color_supported: LevelSet,
    /// 该配对下深度侧支持的**级别集合**（可非连续）。
    pub depth_supported: LevelSet,
}

impl FormatSupport {
    /// 构造「两侧都支持同一组级别」的条目（连续支持集的快捷方式）。
    pub fn uniform(
        color: ColorFormat,
        depth: DepthFormat,
        levels: &[MsaaLevel],
    ) -> FormatSupport {
        let mut s = LevelSet::empty();
        for l in levels.iter() {
            s = s.with(*l);
        }
        FormatSupport {
            color,
            depth,
            color_supported: s,
            depth_supported: s,
        }
    }

    /// 两侧的共同支持集（真实交集）。
    pub const fn common(&self) -> LevelSet {
        self.color_supported.intersect(self.depth_supported)
    }
}

/// 设备能力（D09 单源的消费形态）。
///
/// **固定容量 + 线性查找**：`FORMAT_TABLE_SIZE = 8` 足够覆盖全部现实组合，
/// 而线性查找在 8 项上是O(8) 且**没有哈希表的桶冲突面**——能力表算错了
/// 一项就是错的，查找结构再聪明也救不回来。容量满时**拒绝新增**而不是
/// 覆盖既有项（覆盖会让"某格式支持 8x"悄悄变成"支持 4x"）。
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceCaps {
    origin: CapsOrigin,
    entries: Vec<FormatSupport>,
}

/// 能力表容量。超出即拒绝新增（见`DeviceCaps` 注释）。
pub const FORMAT_TABLE_SIZE: usize = 8;

/// 查询结论（缺失也是一种结论——**缺失≠支持**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapsLookup {
    /// 找到配对条目。
    Found(FormatSupport),
    /// 表中无此配对（**不等于支持**，须按不支持处理）。
    Absent,
}

impl DeviceCaps {
    /// 构造一个空能力表（来源=默认假设）。
    ///
    /// 空表 ⇒ 所有格式落off。这是**保守方向**的正确默认值：宁可不开MSAA，
    /// 也不要假装设备支持。
    pub fn empty_defaulted() -> Self {
        DeviceCaps {
            origin: CapsOrigin::Defaulted,
            entries: Vec::new(),
        }
    }

    /// 构造能力表（来源=探测）。
    pub fn probed(entries: Vec<FormatSupport>) -> Self {
        DeviceCaps {
            origin: CapsOrigin::Probed,
            entries,
        }
    }

    /// 来源。
    pub fn origin(&self) -> CapsOrigin {
        self.origin
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已满。
    pub fn is_full(&self) -> bool {
        self.entries.len() >= FORMAT_TABLE_SIZE
    }

    /// 登记一条能力（**表满则拒绝**，绝不覆盖既有项）。
    ///
    /// 返回 `false` 表示拒绝：重复配对或表满。重复配对**不覆盖**——
    /// 覆盖会让"某格式支持 8x"悄悄变成"支持 4x"，而这正是能力表最不该
    /// 静默发生的事。
    pub fn register(&mut self, entry: FormatSupport) -> bool {
        if self.is_full() {
            return false;
        }
        for e in self.entries.iter() {
            if e.color == entry.color && e.depth == entry.depth {
                return false;
            }
        }
        self.entries.push(entry);
        true
    }

    /// 查询配对能力（**缺失即不支持**）。
    pub fn query(&self, color: ColorFormat, depth: DepthFormat) -> CapsLookup {
        for e in self.entries.iter() {
            if e.color == color && e.depth == depth {
                return CapsLookup::Found(*e);
            }
        }
        CapsLookup::Absent
    }

    /// 查配对下颜色与深度**共同**支持的级别集合（真实交集）。
    ///
    /// `Absent` → 空集。**返回交集**而不是某一侧，因为 MSAA 要求两侧同时支持。
    pub fn query_common_set(&self, color: ColorFormat, depth: DepthFormat) -> LevelSet {
        match self.query(color, depth) {
            CapsLookup::Absent => LevelSet::empty(),
            CapsLookup::Found(e) => e.common(),
        }
    }

    /// 查配对下颜色与深度共同支持的**最大**级别（空集 → `Off`）。
    pub fn query_common_max(&self, color: ColorFormat, depth: DepthFormat) -> MsaaLevel {
        self.query_common_set(color, depth).max_level()
    }

    /// 查配对下深度侧是否支持 MSAA（**独立否决项**的判据）。
    pub fn depth_supports_msaa(&self, color: ColorFormat, depth: DepthFormat) -> bool {
        match self.query(color, depth) {
            CapsLookup::Absent => false,
            CapsLookup::Found(e) => e.depth_supported.any_msaa(),
        }
    }

    /// 全部条目（只读）。
    pub fn entries(&self) -> &[FormatSupport] {
        &self.entries
    }

    /// 人类可读的能力摘要（调试/设置页）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("设备能力（");
        s.push_str(match self.origin {
            CapsOrigin::Probed => "D09 探测",
            CapsOrigin::Defaulted => "默认假设·未探测",
        });
        s.push_str("）：");
        if self.entries.is_empty() {
            s.push_str("空表⇒所有格式 MSAA 关闭");
            return s;
        }
        for e in self.entries.iter() {
            s.push_str("\n  ");
            s.push_str(e.color.tag());
            s.push('+');
            s.push_str(e.depth.tag());
            s.push_str(" 颜色 ");
            s.push_str(&e.color_supported.text());
            s.push_str(" / 深度 ");
            s.push_str(&e.depth_supported.text());
            s.push_str(" / 共同 ");
            s.push_str(&e.common().text());
        }
        s
    }
}

// ===========================================================================
// 五、协商（四步）
// ===========================================================================

/// 降级理由（机器可读；人读文本由 `declaration` 给出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DowngradeReason {
    /// 未降级：请求级即生效。
    None,
    /// 按颜色支持集求交降级。
    ColorSupportIntersection,
    /// 深度格式不支持 → off。
    DepthFormatUnsupported,
    /// 路径不支持 → off。
    PathUnsupported,
    /// 显存超配额 → 降级至配额内最大级。
    QuotaExceeded,
    /// 能力表无此配对 → off。
    FormatPairAbsent,
}

impl DowngradeReason {
    /// 稳定短名（遥测键）。
    pub const fn tag(self) -> &'static str {
        match self {
            DowngradeReason::None => "none",
            DowngradeReason::ColorSupportIntersection => "color_support_intersection",
            DowngradeReason::DepthFormatUnsupported => "depth_format_unsupported",
            DowngradeReason::PathUnsupported => "path_unsupported",
            DowngradeReason::QuotaExceeded => "quota_exceeded",
            DowngradeReason::FormatPairAbsent => "format_pair_absent",
        }
    }

    /// 人读标签。
    pub const fn label(self) -> &'static str {
        match self {
            DowngradeReason::None => "按请求级生效",
            DowngradeReason::ColorSupportIntersection => "按设备颜色支持集降级",
            DowngradeReason::DepthFormatUnsupported => "深度格式不支持 MSAA，降级为关闭",
            DowngradeReason::PathUnsupported => "当前渲染路径不启用 MSAA",
            DowngradeReason::QuotaExceeded => "显存超配额，降级至配额内最大级",
            DowngradeReason::FormatPairAbsent => "能力表无此格式配对，降级为关闭",
        }
    }

    /// 该理由是否**可由换硬件修复**。
    ///
    /// 区分它的用处很实际：`PathUnsupported` 换设备**修不好**，把它归入
    /// "能力不足"会让运维 endlessly 换机器。
    pub const fn fixable_by_hardware(self) -> bool {
        matches!(
            self,
            DowngradeReason::ColorSupportIntersection
                | DowngradeReason::DepthFormatUnsupported
                | DowngradeReason::FormatPairAbsent
                | DowngradeReason::QuotaExceeded
        )
    }

    /// 该理由对应的诊断码（`None` 无诊断）。
    pub const fn diag_code(self) -> Option<MsaaDiagCode> {
        match self {
            DowngradeReason::None => None,
            DowngradeReason::ColorSupportIntersection => Some(MsaaDiagCode::LevelDowngraded),
            DowngradeReason::DepthFormatUnsupported => Some(MsaaDiagCode::DepthFormatNoMsaa),
            DowngradeReason::PathUnsupported => Some(MsaaDiagCode::DeferredPathUnsupported),
            DowngradeReason::QuotaExceeded => Some(MsaaDiagCode::MemoryOverQuota),
            DowngradeReason::FormatPairAbsent => Some(MsaaDiagCode::NoColorLevelSupported),
        }
    }
}

/// 协商输入（把四个参数打包，便于整体传递与测试）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NegotiateRequest {
    /// 请求级别。
    pub requested: MsaaLevel,
    /// 渲染路径。
    pub path: RenderPath,
    /// 颜色格式。
    pub color: ColorFormat,
    /// 深度格式。
    pub depth: DepthFormat,
    /// 渲染目标宽（像素）。
    pub width: u32,
    /// 渲染目标高（像素）。
    pub height: u32,
    /// 可用显存配额（字节）。
    pub quota_bytes: u64,
}

impl NegotiateRequest {
    /// 构造 1080p 前向路径的请求（常用预设）。
    pub fn forward_1080p(requested: MsaaLevel, quota_bytes: u64) -> Self {
        NegotiateRequest {
            requested,
            path: RenderPath::Forward,
            color: ColorFormat::Rgba8Unorm,
            depth: DepthFormat::D32Float,
            width: 1920,
            height: 1080,
            quota_bytes,
        }
    }
}

/// 协商结果（**显性声明是契约的一部分**——见 `MSAA_NEGOTIATION_DOC`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiation {
    /// 请求级别。
    pub requested: MsaaLevel,
    /// 实际生效级别。
    pub granted: MsaaLevel,
    /// 降级理由。
    pub reason: DowngradeReason,
    /// 是否发生了降级（`granted != requested`）。
    pub downgraded: bool,
    /// MSAA 显存实算值（字节）。
    pub memory_bytes: u64,
}

impl Negotiation {
    /// **人读声明文本**（逐字用于设置页与告警）。
    ///
    /// **必须含"实际级别"**且**必须含"降过级"的事实**：用户看到 "MSAA 4x"
    /// 却得到 2x 时，缺了这个声明就只能靠"看起来有点糙"去猜。
    pub fn declaration(&self) -> String {
        let mut s = String::new();
        s.push_str("MSAA：请求 ");
        s.push_str(self.requested.tag());
        s.push_str("，实际 ");
        s.push_str(self.granted.tag());
        if self.downgraded {
            s.push_str("（已降级：");
            s.push_str(self.reason.label());
            s.push('）');
        } else {
            s.push_str("（按请求级生效）");
        }
        s.push_str("｜显存 ");
        s.push_str(&self.memory_bytes.to_string());
        s.push_str(" B");
        if !self.reason.fixable_by_hardware() && self.reason != DowngradeReason::None {
            s.push_str("｜此降级换硬件无效");
        }
        s
    }
}

/// 级别协商（锚点「协商实现」主入口）。
///
/// **四步顺序不可交换**，理由见`MSAA_NEGOTIATION_DOC`：
/// ① 路径 → ② 深度格式 → ③ 颜色级别求交 → ④ 配额。
///
/// 顺序颠倒的具体后果（**真实缺陷，不是假想**）：
/// - 把③ 提到① 之前：延迟路径 + 8x 请求 + 设备支持 4x，会报"设备能力不足"，
///   运维去换设备，永远修不好——真因是路径不支持。
/// - 把② 提到③ 之前本身没错，但**② 的结果必须是 off 而非"再降一级"**：
///   若实现成"深度不支持就取深度侧最大级"，设备深度支持 2x 时会得到 2x，
///   而颜色侧 8x 与深度侧 2x 的**配对从未被能力表声明支持**——那是编造能力。
pub fn negotiate(req: &NegotiateRequest, caps: &DeviceCaps) -> Negotiation {
    // ① 路径否决：设计约束先于能力短板。
    if req.requested.enabled() && !req.path.supports_msaa() {
        return finish(req, MsaaLevel::Off, DowngradeReason::PathUnsupported);
    }
    if !req.requested.enabled() {
        // 请求就是 off：不需要任何能力查询，也不该产生降级诊断。
        return finish(req, MsaaLevel::Off, DowngradeReason::None);
    }

    // 能力表来源为默认假设时显性登记（不冒充探测结果）。
    let pair = match caps.query(req.color, req.depth) {
        CapsLookup::Found(e) => e,
        CapsLookup::Absent => {
            return finish(req, MsaaLevel::Off, DowngradeReason::FormatPairAbsent);
        }
    };

    // ② 深度格式独立否决：无 MSAA 深度 ⇒ 整体 off（不是降一级）。
    if !pair.depth_supported.any_msaa() {
        return finish(req, MsaaLevel::Off, DowngradeReason::DepthFormatUnsupported);
    }

    // ③ 求交：取真实共同支持集内 ≤ 请求级的最大者。
    //
    // **为什么必须用集合而不是「最大级别」**：支持集可非连续。若设备
    // 支持 {1x, 4x, 8x} 而请求 4x，"取 ≤ 请求的最小下探一级"会走到 2x，
    // 而 2x **未被声明支持**——那是编造能力。集合求交给出的是真答案。
    let common = pair.common();
    let after_color = match common.max_at_most(req.requested) {
        Some(l) => l,
        None => {
            // 共同支持集内除 off 外无 ≤ 请求级者。
            return finish(req, MsaaLevel::Off, DowngradeReason::ColorSupportIntersection);
        }
    };

    // ④ 配额否决：降级至配额内最大级（**不是直接 off**——配额是连续约束，
    // 能塞下 2x 就该给 2x）。
    //
    // 理由的归属必须**分清"谁把它压下去的"**：若能力已把after_color 压低，
    // 配额只是恰好也没那么多余量，真因仍是能力；只有在能力本已满足请求级、
    // 纯由配额压下去时，理由才是 QuotaExceeded。否则运维看到"超配额"去扩
    // 显存，扩完还是不够——真因是设备不支持。
    let fitting = quota_fitting_level(req);
    // ④ 的上界必须**同时**受"请求级"与"配额"约束——取二者的较小者。
    //
    // **漏掉请求级上界是真缺陷（本条开发中实测命中）**：只按配额上界求交时，
    // 设备支持 {1,4,8}、请求 4x、配额宽裕 ⇒ 上界=8x ⇒ granted=8x，
    // **比用户请求的还高**。它看起来"更好"，实际是越权：用户选了 4x 却拿到
    // 8x，显存按8x 占用而设置页显示 4x，两者对不上，且多花的显存无人授权。
    let cap_bound = if fitting.rank() <= req.requested.rank() {
        fitting
    } else {
        req.requested
    };
    let after_quota = match common.max_at_most(cap_bound) {
        Some(l) => l,
        None => return finish(req, MsaaLevel::Off, DowngradeReason::QuotaExceeded),
    };
    let reason = if after_quota == req.requested {
        DowngradeReason::None
    } else if after_color == req.requested {
        // 能力已满足请求级 —— 唯一把它压下去的就是配额。
        DowngradeReason::QuotaExceeded
    } else if fitting == after_color {
        // 配额没起作用，压下来的是能力。
        DowngradeReason::ColorSupportIntersection
    } else {
        // 两者都压了：以**先起作用的那一步**（③ 能力）为准，
        // 理由文本里已含"降级"事实，配额维度由 `memory_bytes` 与三要素承载。
        DowngradeReason::ColorSupportIntersection
    };
    finish(req, after_quota, reason)
}

/// 配额内可容纳的最大级别。
///
/// `Off` 恒可容纳（1x 基线不算 MSAA 增量），故恒返回 `Some`——
/// 返回 `Option` 是为了与 `LevelSet::max_at_most` 的语义对齐。
fn quota_fitting_level(req: &NegotiateRequest) -> MsaaLevel {
    for l in MsaaLevel::all_desc().iter() {
        if msaa_memory_bytes(req.width, req.height, req.color, req.depth, *l) <= req.quota_bytes {
            return *l;
        }
    }
    MsaaLevel::Off
}

/// 收尾：算显存、填结果。**所有协商出口都走这里**——保证"结果必带显存实算"
/// 不是一条靠自觉的纪律。
fn finish(req: &NegotiateRequest, granted: MsaaLevel, reason: DowngradeReason) -> Negotiation {
    Negotiation {
        requested: req.requested,
        granted,
        reason,
        downgraded: granted != req.requested,
        memory_bytes: msaa_memory_bytes(
            req.width,
            req.height,
            req.color,
            req.depth,
            granted,
        ),
    }
}

// ===========================================================================
// 六、显存实算（入F1776 配额）
// ===========================================================================

/// MSAA 显存实算（颜色 + 深度，单位字节）。
///
/// **这是纯算术不是估算**，故本条给出精确值并断言：
/// `msaa_memory_bytes(1920, 1080, RGBA8, D32, X4) == 66,355,200`。
///
/// **深度也乘样本数**：MSAA 的深度缓冲是每样本一份（D32 下 4x 即 4 倍），
/// 只算颜色是常见错误——那会让 F1776 配额低估一半以上，而超配额的后果是
/// 分配失败或驱动悄悄降级（本条会在协商层就拦下，故此处必须算准）。
pub fn msaa_memory_bytes(
    width: u32,
    height: u32,
    color: ColorFormat,
    depth: DepthFormat,
    level: MsaaLevel,
) -> u64 {
    let px = width as u64 * height as u64;
    let per_sample = (color.bytes_per_pixel() + depth.bytes_per_pixel()) as u64;
    px * per_sample * level.samples() as u64
}

/// 颜色侧显存实算（拆出便于F1776 分桶记账）。
pub fn color_memory_bytes(width: u32, height: u32, color: ColorFormat, level: MsaaLevel) -> u64 {
    width as u64 * height as u64 * color.bytes_per_pixel() as u64 * level.samples() as u64
}

/// 深度侧显存实算。
pub fn depth_memory_bytes(width: u32, height: u32, depth: DepthFormat, level: MsaaLevel) -> u64 {
    width as u64 * height as u64 * depth.bytes_per_pixel() as u64 * level.samples() as u64
}

/// 配额拒绝三要素（锚点「配额入 F1776 声明」+ 三要素纪律）。
///
/// **三要素缺一不可**：只说"超配额"而不给当前/上限/建议，收到告警的人
/// 无法判断该降哪一档、降到多少。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaRejection {
    /// 当前请求量（字节）。
    pub current_bytes: u64,
    /// 上限（字节）。
    pub limit_bytes: u64,
    /// 建议量（字节）——配额内最大可容纳级别对应的量。
    pub suggested_bytes: u64,
    /// 超出的字节数。
    pub overflow_bytes: u64,
}

impl QuotaRejection {
    /// 三要素文本（"当前/上限/建议"逐项出现，可被逐字断言）。
    pub fn text(&self) -> String {
        format!(
            "当前 {} B / 上限 {} B / 建议 {} B（超出 {} B）",
            self.current_bytes, self.limit_bytes, self.suggested_bytes, self.overflow_bytes
        )
    }
}

/// 构造配额拒绝三要素。
pub fn quota_rejection(
    req: &NegotiateRequest,
    at_level: MsaaLevel,
    fitting_level: MsaaLevel,
) -> QuotaRejection {
    let current_bytes = msaa_memory_bytes(req.width, req.height, req.color, req.depth, at_level);
    let suggested_bytes =
        msaa_memory_bytes(req.width, req.height, req.color, req.depth, fitting_level);
    QuotaRejection {
        current_bytes,
        limit_bytes: req.quota_bytes,
        suggested_bytes,
        overflow_bytes: current_bytes.saturating_sub(req.quota_bytes),
    }
}

// ===========================================================================
// 七、样本模式与 resolve（**面积守恒是本条真判据**）
// ===========================================================================

/// 2x 标准样本位置（像素坐标，像素中心为 (0.5, 0.5)）。
///
/// 取 D3D/Vulkan 通用 2x rotated grid：`(0.75,0.75)` 与 `(0.25,0.25)`。
pub const SAMPLE_2X: [(f32, f32); 2] = [(0.75, 0.75), (0.25, 0.25)];

/// 4x 标准样本位置（经典 4x rotated grid）。
pub const SAMPLE_4X: [(f32, f32); 4] = [
    (0.375, 0.125),
    (0.875, 0.375),
    (0.125, 0.625),
    (0.625, 0.875),
];

/// 8x 标准样本位置（经典 8x rotated grid）。
pub const SAMPLE_8X: [(f32, f32); 8] = [
    (0.5625, 0.3125),
    (0.4375, 0.6875),
    (0.8125, 0.5625),
    (0.3125, 0.1875),
    (0.1875, 0.8125),
    (0.0625, 0.4375),
    (0.6875, 0.9375),
    (0.9375, 0.0625),
];

/// 该级别的标准样本位置（`Off` 返回空切片）。
pub fn sample_positions(level: MsaaLevel) -> &'static [(f32, f32)] {
    match level {
        MsaaLevel::Off => &[],
        MsaaLevel::X2 => &SAMPLE_2X,
        MsaaLevel::X4 => &SAMPLE_4X,
        MsaaLevel::X8 => &SAMPLE_8X,
    }
}

/// 位置表校验的**可注入内核**（`[0,1)²` 逐样本判定）。
///
/// 为什么要有这个入口：`sample_positions_in_unit_square` 只读内置常量表，
/// 于是"该校验能否识别非法"这件事**无法被测**——把它的判断改成恒真
/// `true`，`K09-样本-位置合法且重心居中`照样全绿（实测：反假变体 V14
/// 未被捕获）。**表内元素验检查表函数 = 恒真弱门禁**：判据与被测物读同一
/// 张正确的表，验的是"表对"而非"校验有牙齿"。
///
/// 抽出内核后，判据可用**表外**的非法位置（越界 / NaN / 负值）验证它
/// 真的会说"不合法"。
pub fn positions_in_unit_square(pos: &[(f32, f32)]) -> bool {
    for &(x, y) in pos.iter() {
        // 非有限值一并拒：NaN 会让所有比较为假，落进"合法"分支，
        // 那是把坏数据放进渲染路径的入口。
        //
        // 诚实标注：`is_finite` 在此**并非唯一防线**，实测（反假变体 V15）
        // 去掉它后判据仍全绿——因为 IEEE 754 下 NaN 的任何比较都返回
        // false，`x >= 0.0` 已经隐含拒绝 NaN；`+∞` 也被 `x < 1.0` 拒。
        // 保留理由与 LUT 端点钉死同型：它是把「非有限必拒」写成可读断言
        // 而非依赖读者知道 NaN 比较语义，成本两次调用，删掉省不到可测量的
        // 性能，却让这个不变量退化为对语言细节的信任。
        if !(x.is_finite() && y.is_finite() && x >= 0.0 && x < 1.0 && y >= 0.0 && y < 1.0) {
            return false;
        }
    }
    true
}

/// 样本位置是否落在像素内（`[0,1)²`）——越界即位置表写错。
///
/// **逐样本断言而非只查首末**：位置表写错最常见的是"某个中间项打成 1.5"，
/// 只查首末会漏掉。
pub fn sample_positions_in_unit_square(level: MsaaLevel) -> bool {
    positions_in_unit_square(&sample_positions(level))
}

/// 样本模式的重心是否落在像素中心附近——位置表写歪的第二个判据。
///
/// 只查"在像素内"抓不到"整体偏移 0.05"这种错误（仍在像素内但重心偏了），
/// 而重心偏移会让所有级别的边缘位置系统性偏移。故用两级判据。
pub fn sample_centroid_offset(level: MsaaLevel) -> f32 {
    let pos = sample_positions(level);
    if pos.is_empty() {
        return 0.0;
    }
    let n = pos.len() as f32;
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    for &(x, y) in pos.iter() {
        sx += x;
        sy += y;
    }
    let cx = sx / n - 0.5;
    let cy = sy / n - 0.5;
    (cx * cx + cy * cy).sqrt()
}

/// resolve（盒式滤波：N个样本的平均）。
///
/// **本函数是真判据的落点**，不是装饰。平均 vs 取首样本 vs 少算一个样本，
/// 三者在"有没有渐变"上无法区分，只有**面积守恒**能区分：
/// 一个被垂直边缘恰好切成一半的像素，其解析覆盖恰为 1/2，
/// resolve 结果必须**精确等于**前景与背景的中点。
///
/// 样本数与级别不符时**拒绝并返回 0**，同时落诊断——不静默用部分样本求
/// 平均（那会输出一个看起来正常、实际偏暗的值）。
pub fn resolve(samples: &[f32], level: MsaaLevel, bag: &mut MsaaDiagBag) -> f32 {
    let want = level.samples() as usize;
    if samples.len() != want {
        bag.note(MsaaDiagCode::SampleCountMismatch, samples.len() as f32);
        return 0.0;
    }
    let mut acc = 0.0f64;
    for s in samples.iter() {
        //非有限值不得进入解析：单个 NaN 样本会把整个像素变成 NaN，
        // 而 NaN 会一路穿过后处理链**永不消失**（任何后续映射都吐不出有限值）。
        let v = if s.is_finite() { *s } else { 0.0 };
        acc += v as f64;
    }
    (acc / want as f64) as f32
}

/// 边缘分析（覆盖率与是否为边缘像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeInfo {
    /// 被覆盖的样本数。
    pub covered: u32,
    /// 总样本数。
    pub total: u32,
    /// 覆盖率 ∈ [0,1]。
    pub coverage: f32,
    /// 样本间最大落差（0 ⇒ 全覆盖或全未覆盖，即非边缘）。
    pub spread: f32,
    /// 是否为边缘像素（部分覆盖）。
    pub is_edge: bool,
}

/// 单样本覆盖判定（半平面测试：`x < edge_x`）。
///
/// 用于**解析面积**构造：过中心的垂直边缘 `edge_x = 0.5` 在 4x 模式下恰好
/// 覆盖 2/4（x = 0.375 与 0.125 在内，0.875 与 0.625 在外），解析覆盖 = 0.5。
pub fn sample_covered_x(x: f32, edge_x: f32) -> bool {
    x < edge_x
}

/// 分析一组样本的边缘属性。
pub fn analyze_edge(samples: &[f32], level: MsaaLevel) -> EdgeInfo {
    let total = level.samples();
    let n = samples.len().min(total as usize);
    let mut covered = 0u32;
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for i in 0..n {
        let v = samples[i];
        if v >= 0.5 {
            covered += 1;
        }
        if v < lo {
            lo = v;
        }
        if v > hi {
            hi = v;
        }
    }
    let spread = if n == 0 { 0.0 } else { hi - lo };
    EdgeInfo {
        covered,
        total,
        coverage: if total == 0 {
            0.0
        } else {
            covered as f32 / total as f32
        },
        spread,
        // **边缘 = 部分覆盖**（不是「有落差」）：全 0 与全 1 的样本
        // 内部一致但都不是边缘；用 spread 判会把纯色区也算成边缘，
        // 覆盖率直方图会因此全糊。
        is_edge: covered > 0 && covered < total,
    }
}

/// 构造一个被垂直边缘切成`edge_x` 的样本组（前景 `fg`／背景 `bg`）。
///
/// 这是**面积守恒测试的输入生成器**：给定解析可算的边缘位置，返回该像素
/// 各样本的实际颜色，交给 `resolve` 后与解析值对账。
pub fn make_edge_samples(level: MsaaLevel, edge_x: f32, fg: f32, bg: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(x, _) in sample_positions(level).iter() {
        out.push(if sample_covered_x(x, edge_x) { fg } else { bg });
    }
    out
}

/// 解析覆盖率（用同一套样本位置独立算出，**不依赖被测的 resolve**）。
///
/// **为什么要"独立算一遍"**：如果拿`resolve` 的输出去推覆盖率，两者共享
/// 同一个实现错误 ⇒ 恒真。解析覆盖率由**样本位置表**直接数出来，与
/// resolve 的求平均逻辑完全独立——这才构成对账。
pub fn analytic_coverage_x(level: MsaaLevel, edge_x: f32) -> f32 {
    let pos = sample_positions(level);
    if pos.is_empty() {
        return 0.0;
    }
    let mut covered = 0u32;
    for &(x, _) in pos.iter() {
        if sample_covered_x(x, edge_x) {
            covered += 1;
        }
    }
    covered as f32 / pos.len() as f32
}

/// 面积守恒对账结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConservationResult {
    /// 解析覆盖率（独立算出）。
    pub analytic_coverage: f32,
    /// 实测覆盖率（由 resolve 输出反推）。
    pub measured_coverage: f32,
    /// 偏差（实测 − 解析）。
    pub bias: f32,
    /// 是否守恒（单边判据，见下）。
    pub conserved: bool,
}

/// 面积守恒对账（**本条的核心判据**）。
///
/// **用单边符号而非双边阈值**（方法学要点）：正确实现的偏差恒为**非正**
/// （量化离散化只会让覆盖被低估：样本位置离散，覆盖是阶梯近似，
/// 相对解析真值只会偏少或恰好）。若实现把分辨率算错（例如除以
/// `samples-1`），偏差恒为**正**——故**单边判据即判别式**：
/// `bias <= eps` 捕获正确实现与"低估型"错误，`bias > eps` 捕获高估型错误。
///
/// **双边阈值一旦宽过正确实现的低估幅度，高估型变异就从缝里钻过去**。
/// 故这里用 0 而非 ±5%。
pub fn area_conservation(
    samples: &[f32],
    level: MsaaLevel,
    edge_x: f32,
    fg: f32,
    bg: f32,
) -> ConservationResult {
    let analytic = analytic_coverage_x(level, edge_x);
    let mut bag = MsaaDiagBag::new();
    let resolved = resolve(samples, level, &mut bag);
    // 由 resolve 输出反推实测覆盖率：`resolved = bg + c·(fg − bg)` ⇒ c 反解。
    let denom = fg - bg;
    let measured = if denom.abs() > 1e-6 {
        (resolved - bg) / denom
    } else {
        // 前景=背景时覆盖率不可反解；此时守恒退化为"输出等于该值"。
        if (resolved - bg).abs() < 1e-6 {
            1.0
        } else {
            0.0
        }
    };
    let bias = measured - analytic;
    ConservationResult {
        analytic_coverage: analytic,
        measured_coverage: measured,
        bias,
        // 单边：偏差不得为正。正偏差 = 覆盖被高估 = 面积不守恒。
        conserved: bias <= 0.0,
    }
}

/// 抗锯齿方法（F2012 选型表的枚举，本条提供其中一维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaMethod {
    /// 无抗锯齿。
    None,
    /// 几何多重采样（本条）。
    Msaa,
    /// 快速后处理边缘 AA（F2010）。
    Fxaa,
    /// 时域抗锯齿（F2011）。
    Taa,
    /// SMAA（预留，F2012）。
    SmaaReserved,
}

impl AaMethod {
    /// 全部取值。
    pub const fn all() -> [AaMethod; 5] {
        [
            AaMethod::None,
            AaMethod::Msaa,
            AaMethod::Fxaa,
            AaMethod::Taa,
            AaMethod::SmaaReserved,
        ]
    }

    /// 稳定短名（选型表键）。
    pub const fn tag(self) -> &'static str {
        match self {
            AaMethod::None => "none",
            AaMethod::Msaa => "msaa",
            AaMethod::Fxaa => "fxaa",
            AaMethod::Taa => "taa",
            AaMethod::SmaaReserved => "smaa_reserved",
        }
    }

    /// 是否依赖历史缓冲（TAA 需要，MSAA 不需要——互斥裁决的依据）。
    pub const fn needs_history(self) -> bool {
        matches!(self, AaMethod::Taa)
    }

    /// 是否作用于几何 pass（决定能否用于延迟路径）。
    pub const fn geometry_stage(self) -> bool {
        matches!(self, AaMethod::Msaa)
    }
}

/// MSAA 档案（供 F2012 选型表消费）。
///
/// **只填本条负责的列**：质量排名、路径约束、显存成本由本条给；
/// 每像素成本**留空并注明待 F2017**——填一个编出来的数字会让选型表
/// 变成"看起来有数据、实际是猜的"。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MsaaProfile {
    /// 方法。
    pub method: AaMethod,
    /// 依赖历史缓冲。
    pub needs_history: bool,
    /// 作用于几何 pass。
    pub geometry_stage: bool,
    /// 延迟路径可用。
    pub deferred_ok: bool,
    /// 1x 相对显存倍数（1x 即 1）。
    pub memory_multiple: u32,
    /// 1080p 4x 显存（字节，实算）。
    pub memory_1080p_4x: u64,
    /// 1080p 4x 成本（毫秒）——**预算值，待 F2017 实测**。
    pub cost_ms_1080p_4x_budget: f32,
}

impl MsaaProfile {
    /// MSAA 档案。
    pub fn msaa() -> Self {
        MsaaProfile {
            method: AaMethod::Msaa,
            needs_history: false,
            geometry_stage: true,
            deferred_ok: false,
            memory_multiple: 4,
            memory_1080p_4x: msaa_memory_bytes(
                1920,
                1080,
                ColorFormat::Rgba8Unorm,
                DepthFormat::D32Float,
                MsaaLevel::X4,
            ),
            cost_ms_1080p_4x_budget: RESOLVE_COST_MS_1080P_4X,
        }
    }
}

// ===========================================================================
// 八、成本模型（预算常量 + 可验证的模型性质）
// ===========================================================================

/// resolve 成本预算（1080p 4x，毫秒）。**锚点给定，非本机实测**。
pub const RESOLVE_COST_MS_1080P_4X: f32 = 0.3;

/// 每样本每像素成本常数（相对单位）。
///
/// **不是"实测"**：由 1080p 4x 预算反解的模型常数。模型的可验证性质是
/// **线性与单调**（`K09-成本-` 族断言），绝对值待 F2017 回填。
pub const COST_PER_SAMPLE_PIXEL: f32 = RESOLVE_COST_MS_1080P_4X / (1920.0 * 1080.0 * 4.0);

/// 成本模型（相对毫秒）。
///
/// 形式：`cost = k × 样本数 × 像素数`，**无固定开销**——resolve 是纯
/// 数据搬运，固定开销（管线启动）不进本函数，调用方若需计入须自行加。
pub fn resolve_cost_ms(width: u32, height: u32, level: MsaaLevel) -> f32 {
    COST_PER_SAMPLE_PIXEL * level.samples() as f32 * width as f32 * height as f32
}

// ===========================================================================
// 九、序位（resolve 与后处理）
// ===========================================================================

/// 管线阶段种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    /// I02 主 pass：几何 MSAA 渲染（产出多采样颜色+深度）。
    GeometryPass,
    /// resolve：多采样 → 1x。
    Resolve,
    /// 后处理效果（F2001 管线序内的任一节点）。
    PostEffect,
    /// 交V 域的呈现。
    Present,
}

impl StageKind {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            StageKind::GeometryPass => "geometry",
            StageKind::Resolve => "resolve",
            StageKind::PostEffect => "post",
            StageKind::Present => "present",
        }
    }

    /// 人读标签。
    pub const fn label(self) -> &'static str {
        match self {
            StageKind::GeometryPass => "I02 主 pass（几何 MSAA 渲染）",
            StageKind::Resolve => "resolve（多采样解析）",
            StageKind::PostEffect => "后处理效果",
            StageKind::Present => "呈现（交 V 域）",
        }
    }
}

/// 序中的一个阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    /// 阶段种类。
    pub kind: StageKind,
    /// 该阶段读写的样本数（**必须** 与协商结果一致）。
    pub sample_count: u32,
}

impl Stage {
    /// 构造。
    pub const fn new(kind: StageKind, sample_count: u32) -> Self {
        Stage { kind, sample_count }
    }
}

/// 序位裁决（锚点「序位断言」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderVerdict {
    /// 序位合法。
    Ok,
    /// R2：后处理节点读 MSAA 数据（锚点点名的那一条）。
    PostReadsMsaa,
    /// R1：resolve 排在后处理之后。
    ResolveAfterPost,
    /// R3：MSAA 开启但无 resolve。
    ResolveMissing,
    /// R4：MSAA 关闭但有 resolve（多余解析）。
    ResolveRedundant,
    /// R5：resolve 多次。
    ResolveDuplicated,
}

impl OrderVerdict {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            OrderVerdict::Ok => "ok",
            OrderVerdict::PostReadsMsaa => "post_reads_msaa",
            OrderVerdict::ResolveAfterPost => "resolve_after_post",
            OrderVerdict::ResolveMissing => "resolve_missing",
            OrderVerdict::ResolveRedundant => "resolve_redundant",
            OrderVerdict::ResolveDuplicated => "resolve_duplicated",
        }
    }

    /// 违规对应的诊断码（`Ok` 无）。
    pub const fn diag_code(self) -> Option<MsaaDiagCode> {
        match self {
            OrderVerdict::Ok => None,
            OrderVerdict::PostReadsMsaa => Some(MsaaDiagCode::PostReadsMsaaData),
            OrderVerdict::ResolveAfterPost => Some(MsaaDiagCode::ResolveAfterPost),
            OrderVerdict::ResolveMissing => Some(MsaaDiagCode::ResolveMissing),
            OrderVerdict::ResolveRedundant => Some(MsaaDiagCode::ResolveRedundant),
            OrderVerdict::ResolveDuplicated => Some(MsaaDiagCode::ResolveDuplicated),
        }
    }

    /// 人读文本（说清要改哪里）。
    pub const fn text(self) -> &'static str {
        match self {
            OrderVerdict::Ok => "序位合法",
            OrderVerdict::PostReadsMsaa => {
                "违规R2：后处理节点读取 MSAA 数据——resolve 必须排在后处理之前"
            }
            OrderVerdict::ResolveAfterPost => "违规 R1：resolve 排在后处理之后",
            OrderVerdict::ResolveMissing => "违规 R3：MSAA 开启但序中无 resolve",
            OrderVerdict::ResolveRedundant => "违规 R4：MSAA 关闭但序中有 resolve（多余解析）",
            OrderVerdict::ResolveDuplicated => "违规 R5：resolve 出现多次（二次解析压低对比度）",
        }
    }
}

/// 序位断言（锚点「序位声明」+ 错误路径 2）。
///
/// **判定顺序有讲究**：先查 R2（读MSAA 数据——锚点点名的那条，且危害最大），
/// 再查 R1/R3/R4/R5。为什么 R2 优先：R3/R4/R5 都是**结构**问题（阶段列表
/// 本身不自洽），而 R2 是**语义**问题（结构自洽但意图错误）。结构问题更
/// 显眼、语义问题更隐蔽，把语义问题报在前面能让人先看到真正危险的那个。
///
/// 注意 `granted_samples` 用**样本数**而非级别比较——序里写的是样本数，
/// 两者不同量纲，直接比级别是错的（`X4` 的判别值是 2，不是 4）。
pub fn check_order(stages: &[Stage], granted: MsaaLevel) -> OrderVerdict {
    let granted_samples = granted.samples();

    // R2：任一后处理节点的样本数 > 1 即违规。
    for st in stages.iter() {
        if st.kind == StageKind::PostEffect && st.sample_count > 1 {
            return OrderVerdict::PostReadsMsaa;
        }
    }

    // 收集 resolve 下标与后处理下标。
    let mut resolve_idx: Vec<usize> = Vec::new();
    let mut first_post: Option<usize> = None;
    for (i, st) in stages.iter().enumerate() {
        match st.kind {
            StageKind::Resolve => resolve_idx.push(i),
            StageKind::PostEffect => {
                if first_post.is_none() {
                    first_post = Some(i);
                }
            }
            _ => {}
        }
    }

    // R5：多次 resolve。
    if resolve_idx.len() > 1 {
        return OrderVerdict::ResolveDuplicated;
    }
    // R3：无 resolve 而 MSAA 开启。
    if resolve_idx.is_empty() && granted_samples > 1 {
        return OrderVerdict::ResolveMissing;
    }
    // R4：有 resolve 而 MSAA 关闭（多余解析）。
    if resolve_idx.len() == 1 && granted_samples <= 1 {
        return OrderVerdict::ResolveRedundant;
    }
    // R1：resolve 在首个后处理之后。
    if let (Some(r), Some(p)) = (resolve_idx.first(), first_post) {
        if *r > p {
            return OrderVerdict::ResolveAfterPost;
        }
    }
    OrderVerdict::Ok
}

/// 序位断言（落诊断版）。
pub fn check_order_guarded(
    stages: &[Stage],
    granted: MsaaLevel,
    bag: &mut MsaaDiagBag,
) -> OrderVerdict {
    let v = check_order(stages, granted);
    if let Some(c) = v.diag_code() {
        bag.note(c, granted.samples() as f32);
    }
    v
}

/// 构造一条合法序（供测试与上层复用）。
///
/// **参数化是有意的**：不同协商结果对应不同序（1x 无 resolve、4x 有一个），
/// 写死一种会让"关MSAA 时序里还留着 resolve"这类缺陷无法被测试覆盖。
pub fn canonical_stages(granted: MsaaLevel) -> Vec<Stage> {
    let mut v = Vec::new();
    v.push(Stage::new(StageKind::GeometryPass, granted.samples()));
    if granted.enabled() {
        v.push(Stage::new(StageKind::Resolve, 1));
    }
    v.push(Stage::new(StageKind::PostEffect, 1));
    v.push(Stage::new(StageKind::Present, 1));
    v
}

// ===========================================================================
// 十、互斥守卫（MSAA × TAA）
// ===========================================================================

/// 互斥裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutexVerdict {
    /// 无冲突，两者原样。
    NoConflict,
    /// 保留 MSAA，丢弃 TAA（**被丢弃方具名**）。
    MsaaKeptTaaDropped,
    /// 保留 TAA，丢弃 MSAA。
    TaaKeptMsaaDropped,
    /// 两者皆关闭（请求即为 off/off）。
    BothOff,
}

impl MutexVerdict {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            MutexVerdict::NoConflict => "no_conflict",
            MutexVerdict::MsaaKeptTaaDropped => "msaa_kept_taa_dropped",
            MutexVerdict::TaaKeptMsaaDropped => "taa_kept_msaa_dropped",
            MutexVerdict::BothOff => "both_off",
        }
    }

    /// **被丢弃方**（`None` 表示无丢弃）。
    ///
    /// 这个字段存在的理由：设置页显示"时域抗锯齿：已启用"而实际被丢弃，
    /// 是最难查的一类不一致（画面确实没有时域累积的鬼影改善，但用户看不出来）。
    /// 强制裁决携带被丢弃方，调用方就能把它显性化。
    pub const fn dropped(self) -> Option<AaMethod> {
        match self {
            MutexVerdict::MsaaKeptTaaDropped => Some(AaMethod::Taa),
            MutexVerdict::TaaKeptMsaaDropped => Some(AaMethod::Msaa),
            _ => None,
        }
    }

    /// 裁决理由。
    pub const fn reason(self) -> &'static str {
        match self {
            MutexVerdict::NoConflict => "无冲突",
            MutexVerdict::BothOff => "两者皆关闭：显式接受几何锯齿",
            MutexVerdict::MsaaKeptTaaDropped => {
                "保留 MSAA：它逐帧无状态；TAA 依赖历史缓冲，帧内丢弃会产生整段鬼影"
            }
            MutexVerdict::TaaKeptMsaaDropped => {
                "保留 TAA：MSAA 未获协商级别（协商降级为 off），无可保留者"
            }
        }
    }
}

/// 互斥守卫（锚点「MSAA 与 TAA 互斥（同时启用冲突）→ 互斥守卫二选一」）。
///
/// **裁决顺序**：先看 MSAA 是否**实际获批**（`granted` 而非 `requested`）——
/// 若协商已把 MSAA 降到 off，就不存在"两者同时开启"，TAA 自然保留。
/// 用 `requested` 判会误报：用户请求 8x、设备只支持 2x、2x 与 TAA 并不冲突，
/// 却因"请求了 MSAA"而把 TAA 丢掉。
///
/// **MSAA 赢的理由是状态性不是质量**（见 `AA_MUTEX_DOC`）：丢 TAA 的瞬态
/// 代价（首帧轻微鬼影）小于丢 MSAA 的常态代价（全程几何锯齿）。
pub fn guard_aa_mutex(
    granted: MsaaLevel,
    taa_enabled: bool,
    bag: &mut MsaaDiagBag,
) -> MutexVerdict {
    let msaa_on = granted.enabled();
    match (msaa_on, taa_enabled) {
        (false, false) => MutexVerdict::BothOff,
        (false, true) => MutexVerdict::NoConflict,
        (true, false) => MutexVerdict::NoConflict,
        (true, true) => {
            bag.note(MsaaDiagCode::TaaDroppedByMutex, granted.samples() as f32);
            MutexVerdict::MsaaKeptTaaDropped
        }
    }
}

/// 互斥后的最终生效方法（把裁决落成"实际用哪个"）。
///
/// **入参是裁决加原始请求，而不是只看裁决**：`NoConflict` 蕴含四种组合
/// （都关 / 只 MSAA / 只 TAA / 都不开但都请求过），只靠裁决无法还原
/// 生效的是哪一个。写成"只吃裁决"就会得到本函数初版那个错误——把
/// "MSAA 开启、TAA 关闭"这一无冲突情形判成 `None`（等于白开了 MSAA）。
pub fn effective_method(verdict: MutexVerdict, msaa_granted: MsaaLevel, taa_enabled: bool) -> AaMethod {
    match verdict {
        MutexVerdict::MsaaKeptTaaDropped => AaMethod::Msaa,
        MutexVerdict::TaaKeptMsaaDropped => AaMethod::Taa,
        MutexVerdict::BothOff => AaMethod::None,
        // 无冲突：按实际开启情况落定。两者同时开启在无冲突裁决下**不可能**
        // （那必走MsaaKeptTaaDropped），故此处 MSAA 优先是安全的。
        MutexVerdict::NoConflict => {
            if msaa_granted.enabled() {
                AaMethod::Msaa
            } else if taa_enabled {
                AaMethod::Taa
            } else {
                AaMethod::None
            }
        }
    }
}

// ===========================================================================
// 十一、状态与调试负载
// ===========================================================================

/// MSAA 状态（注入式，可测）。
#[derive(Clone, Debug, PartialEq)]
pub struct MsaaState {
    /// 最近一次协商结果。
    pub negotiation: Negotiation,
    /// 能力来源。
    pub caps_origin: CapsOrigin,
    /// 当前路径。
    pub path: RenderPath,
    /// TAA 是否请求启用。
    pub taa_requested: bool,
    /// 最近一次互斥裁决。
    pub mutex: MutexVerdict,
    /// 最近一次序位裁决。
    pub order: OrderVerdict,
    /// 诊断袋。
    pub diag: MsaaDiagBag,
    /// 累计降级次数（遥测）。
    pub downgrade_count: u32,
    /// 累计序位拦截次数（遥测）。
    pub order_rejections: u32,
    /// 累计互斥丢弃次数（遥测）。
    pub mutex_drops: u32,
}

impl MsaaState {
    /// 构造（默认：off、默认假设能力、前向路径）。
    pub fn new() -> Self {
        MsaaState {
            negotiation: Negotiation {
                requested: MsaaLevel::Off,
                granted: MsaaLevel::Off,
                reason: DowngradeReason::None,
                downgraded: false,
                memory_bytes: 0,
            },
            caps_origin: CapsOrigin::Defaulted,
            path: RenderPath::Forward,
            taa_requested: false,
            mutex: MutexVerdict::BothOff,
            order: OrderVerdict::Ok,
            diag: MsaaDiagBag::new(),
            downgrade_count: 0,
            order_rejections: 0,
            mutex_drops: 0,
        }
    }

    /// 执行协商并落诊断（**降级理由的诊断在此产生**，不在纯函数里）。
    pub fn negotiate(
        &mut self,
        req: &NegotiateRequest,
        caps: &DeviceCaps,
    ) -> Negotiation {
        self.caps_origin = caps.origin();
        if caps.origin() == CapsOrigin::Defaulted {
            self.diag.note(MsaaDiagCode::CapsOriginDefaulted, 0.0);
        }
        let n = negotiate(req, caps);
        if let Some(code) = n.reason.diag_code() {
            self.diag.note(code, n.granted.samples() as f32);
        }
        if n.downgraded {
            self.downgrade_count = self.downgrade_count.saturating_add(1);
        }
        self.negotiation = n;
        n
    }

    /// 序位断言（计入遥测）。
    pub fn check_order(&mut self, stages: &[Stage]) -> OrderVerdict {
        let v = check_order_guarded(stages, self.negotiation.granted, &mut self.diag);
        if v != OrderVerdict::Ok {
            self.order_rejections = self.order_rejections.saturating_add(1);
        }
        self.order = v;
        v
    }

    /// 互斥守卫（计入遥测）。
    pub fn guard_mutex(&mut self, taa_enabled: bool) -> MutexVerdict {
        self.taa_requested = taa_enabled;
        let v = guard_aa_mutex(self.negotiation.granted, taa_enabled, &mut self.diag);
        if v.dropped().is_some() {
            self.mutex_drops = self.mutex_drops.saturating_add(1);
        }
        self.mutex = v;
        v
    }

    /// 阻断项计数。
    pub fn blocking(&self) -> usize {
        self.diag.blocking_count()
    }

    /// 调试负载快照（锚点 F2013 前的过渡形态，见 `DEBUG_PAYLOAD_DOC`）。
    pub fn debug_snapshot(&self) -> MsaaDebugSnapshot {
        MsaaDebugSnapshot {
            requested_level: self.negotiation.requested,
            granted_level: self.negotiation.granted,
            downgrade_reason: self.negotiation.reason,
            declaration: self.negotiation.declaration(),
            caps_origin: self.caps_origin,
            path: self.path,
            memory_bytes: self.negotiation.memory_bytes,
            mutex_tag: self.mutex.tag(),
            order_tag: self.order.tag(),
        }
    }

    /// 调试负载的文本投影（设置页与告警共用同一份数据，避免两处漂移）。
    pub fn debug_text(&self) -> String {
        self.debug_snapshot().text()
    }

    /// 设置页人读文本。
    ///
    /// **降级时必须同时出现"请求"与"实际"两个数**：只显示实际级别，
    /// 用户设了 8x 却看到 4x 会以为设置没生效而反复重设。
    pub fn screen_text(&self) -> String {
        let mut s = self.debug_text();
        s.push_str("｜降级 ");
        s.push_str(&self.downgrade_count.to_string());
        s.push_str(" 次｜序位拦截 ");
        s.push_str(&self.order_rejections.to_string());
        s.push_str(" 次｜互斥丢弃 ");
        s.push_str(&self.mutex_drops.to_string());
        s.push_str(" 次");
        s
    }
}

impl Default for MsaaState {
    fn default() -> Self {
        MsaaState::new()
    }
}

/// 调试负载快照。
#[derive(Clone, Debug, PartialEq)]
pub struct MsaaDebugSnapshot {
    /// 请求级别。
    pub requested_level: MsaaLevel,
    /// 实际级别。
    pub granted_level: MsaaLevel,
    /// 降级理由。
    pub downgrade_reason: DowngradeReason,
    /// 人读声明。
    pub declaration: String,
    /// 能力来源。
    pub caps_origin: CapsOrigin,
    /// 路径。
    pub path: RenderPath,
    /// 显存字节。
    pub memory_bytes: u64,
    /// 互斥裁决短名。
    pub mutex_tag: &'static str,
    /// 序位裁决短名。
    pub order_tag: &'static str,
}

impl MsaaDebugSnapshot {
    /// 文本投影（字段顺序固定，可被逐字断言）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("MSAA[请求 ");
        s.push_str(self.requested_level.tag());
        s.push_str(" | 实际 ");
        s.push_str(self.granted_level.tag());
        s.push_str(" | 理由 ");
        s.push_str(self.downgrade_reason.tag());
        s.push_str(" | 能力 ");
        s.push_str(match self.caps_origin {
            CapsOrigin::Probed => "探测",
            CapsOrigin::Defaulted => "默认假设",
        });
        s.push_str(" | 路径 ");
        s.push_str(self.path.tag());
        s.push_str(" | 显存 ");
        s.push_str(&self.memory_bytes.to_string());
        s.push_str("B | 互斥 ");
        s.push_str(self.mutex_tag);
        s.push_str(" | 序位 ");
        s.push_str(self.order_tag);
        s.push(']');
        s
    }
}

/// 调试负载的键名清单（F2013 接入时的字段契约）。
pub fn debug_keys() -> Vec<&'static str> {
    vec![
        "requested_level",
        "granted_level",
        "downgrade_reason",
        "declaration",
        "caps_origin",
        "path",
        "memory_bytes",
        "mutex_tag",
        "order_tag",
    ]
}