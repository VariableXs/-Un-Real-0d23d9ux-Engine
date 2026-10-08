//! VE-F0814 · 文字渲染 fuzz（VE-E 域 · 文字渲染）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0814`
//!
//! **判据（锚点原文）**：两面 fuzz、oracle 三件套、回归语料闭环、30 分钟
//! nightly、崩溃零容忍。
//!
//! 文字渲染域前 13 册保证了「写出来的用例是对的」，本条回答另一个问题：
//! **怎么知道没写出来的用例里没有能打穿文字渲染管线的输入**。复用全域 fuzz
//! 总闸范式（VE-F0419 `vec19_fuzz` 的自持 LCG 与发现账结构），按文字域两个
//! 真实攻击面重造：
//!
//! 1. **两面 fuzz**（判据一）。两个面各打一条真实管线，缺一面就有盲区：
//!    - **字体文件面**（`FuzzFace::FontFile`）：结构变异三算子——表裁剪
//!      （目录/负载被截）、长度篡改（声明长度虚高——真实解析器按声明值
//!      预留内存，虚高即 OOM）、循环链构造（表偏移指回表目录，链跟踪
//!      永不逃逸）。变异真喂进沙箱解析模型 `parse_font_sandboxed`，再与
//!      F0810 的真 `sandbox_check` 对账——不是自说自话的模型互证。
//!    - **编码序列面**（`FuzzFace::EncodingSeq`）：随机字节流 + 边界注入
//!      三算子——截断多字节（缓冲在序列中间结束）、代理区（CESU-8 形式
//!      的 U+D800）、过长编码（0xC0 引导的'/）——真喂进 F0812 的真解码器
//!      `Decoder::decode`，oracle 盯四档处置与下游码点合法性。
//!
//! 2. **oracle 三件套**（判据二）。字体面三条逐条可判：解析器零崩溃
//!    （报告必达终态：完成或被护栏截停，二者皆无=脱轨即崩溃等价物）、
//!    沙箱内存 ≤64MB（声明总和超限必被 MemCap 拦截，且过冲不超过最后一笔
//!    声明——多走一整表就是护栏迟钝）、超时 ≤500ms 必终止（工作量模型
//!    过线必被 TimeCap 截停且不得跑到完成）。编码面三条：四档处置全部生效
//!    且计数一致（注入类别必须激起对应处置档）、下游不接收无效码点（输出
//!    码点逐个过判据侧**独立实现**的合法性谓词——不从被测模块借函数）。
//!
//! 3. **语料三源与回归闭环**（判据三）。真实字体集（含畸形字体集，确定性
//!    种子构造器）、变异种子、历史崩溃回归语料。闭环是结构不是文案：
//!    fuzz 发现 → 最小化样本（增量式 delta debugging）→ 定级 → `promote`
//!    进回归账 → 之后每一轮（nightly 与增量）每例必跑——发现过的崩溃
//!    永远留在回归面上，回归破坏即门禁红。
//!
//! 4. **运行节奏**（判据四）。nightly 全量 30 分钟（1800s）、提交前增量
//!    3 分钟（180s）。无墙钟环境（no_std 内核）用**确定性成本模型**：每条
//!    语料成本 = 1 + 字节数/256，吞吐 64 单位/秒——规划器据此给出预算内
//!    的批次；增量必须全量跑回归账（缩水只能缩新语料，不能缩回归）。
//!
//! 5. **崩溃零容忍与 oracle 修订纪律**（判据五）。`CrashClass::MemorySafety`
//!    定级最高（与全域宪法 `VS-P1` 同名同序，AE10 F6385 总闸落位后按
//!    常量对接，本域先持口径常量）；未修的 MemorySafety/Hang 即门禁阻断。
//!    oracle 误报的唯一出口是**修 oracle**（`ORACLE_VERSION` 递增后按版本
//!    免责），判据五条锚点 stamp 常量化——修 oracle 不得顺手动判据。
//!
//! 零静默纪律：发现逐条入账带复现键（face+source+seed+index 四元组，
//! `replay` 可逐位重建输入）；护栏截停必须写明 GuardStop 变体；回归账
//! 每例必跑且结果如实记账；`open_critical` 不为空如实报出。
//! 零 panic 面、零 IO、无全局可变状态、自持确定性随机（LCG 复用 vec19）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vec19_fuzz::Rng;
use super::vee02_utf8::{
    DecodeOptions, DecodeResult, Decoder, Disposition, MAX_CODEPOINT, REPLACEMENT_CHAR,
    SURROGATE_FIRST, SURROGATE_LAST,
};
use super::vee10_textsecurity::{sandbox_check, SandboxInput, SandboxLimits, SandboxVerdict};

// ---------------------------------------------------------------------------
// 一、Fuzz 面与语料三源（判据一、判据三的骨架）
// ---------------------------------------------------------------------------

/// fuzz 面（锚点两面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzFace {
    /// 字体文件面：结构变异打沙箱解析与文件校验。
    FontFile,
    /// 编码序列面：随机流+边界注入打 UTF-8 解码。
    EncodingSeq,
}

impl FuzzFace {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            FuzzFace::FontFile => "字体文件面",
            FuzzFace::EncodingSeq => "编码序列面",
        }
    }

    /// 两面全集（缺一面即盲区，门禁按此查覆盖）。
    pub const ALL: [FuzzFace; 2] = [FuzzFace::FontFile, FuzzFace::EncodingSeq];

    /// 数组下标。
    pub const fn ordinal(self) -> usize {
        match self {
            FuzzFace::FontFile => 0,
            FuzzFace::EncodingSeq => 1,
        }
    }
}

/// 语料三源（锚点：真实字体集 / 变异种子 / 历史崩溃回归）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusSource {
    /// 真实字体集（含畸形字体集）——确定性种子构造器产出。
    RealFonts,
    /// 变异种子——以合法种子为基线做结构破坏。
    MutatedSeeds,
    /// 历史崩溃回归语料——回归账里的既有用例。
    CrashRegression,
}

impl CorpusSource {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            CorpusSource::RealFonts => "真实字体集",
            CorpusSource::MutatedSeeds => "变异种子",
            CorpusSource::CrashRegression => "历史崩溃回归",
        }
    }

    /// 三源全集（缺一源即盲区，门禁按此查覆盖）。
    pub const ALL: [CorpusSource; 3] = [
        CorpusSource::RealFonts,
        CorpusSource::MutatedSeeds,
        CorpusSource::CrashRegression,
    ];

    /// 数组下标。
    pub const fn ordinal(self) -> usize {
        match self {
            CorpusSource::RealFonts => 0,
            CorpusSource::MutatedSeeds => 1,
            CorpusSource::CrashRegression => 2,
        }
    }
}

/// 复现键：face+source+seed+index 四元组唯一决定一条输入。
///
/// 三源在批次里轮流出现，只靠 index 无法定位生成路径；报缺陷要的是
/// 「怎么生成的」，四元组 + `replay` 就是完整答案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReproKey {
    /// 面。
    pub face: FuzzFace,
    /// 源。
    pub source: CorpusSource,
    /// 主种子。
    pub seed: u64,
    /// 批内序号。
    pub index: u64,
}

// ---------------------------------------------------------------------------
// 二、最小 sfnt 字体种子构造器（真实字体集的确定性替身，含畸形字体集）
// ---------------------------------------------------------------------------

/// sfnt 头长（版本 4 + 表数 2 + 查参 6）。
pub const SFNT_HEADER_LEN: usize = 12;
/// sfnt 表目录记录长（tag4 + checksum4 + offset4 + length4）。
pub const SFNT_RECORD_LEN: usize = 16;

/// 字体种子规格（确定性构造的全部参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSeedSpec {
    /// 表目录项数。
    pub num_tables: u16,
    /// 每张表声明长度（字节）。
    pub table_len: u32,
    /// 第几张表构成循环链（offset 指回目录起点）。
    pub circular_at: Option<usize>,
    /// 第几张表长度被篡改。
    pub length_tamper_at: Option<usize>,
    /// 篡改后的长度值。
    pub tampered_len: u32,
}

impl FontSeedSpec {
    /// 干净小字体：4 表 × 4KB。
    pub const fn clean_small() -> FontSeedSpec {
        FontSeedSpec {
            num_tables: 4,
            table_len: 4096,
            circular_at: None,
            length_tamper_at: None,
            tampered_len: 0,
        }
    }

    /// 宽表字体：512 表 × 8KB——表数触顶、逐表固定开销足以越过 500ms
    /// 工作量线而内存总和（4MB）远低于 64MB，专测「超时必终止」。
    pub const fn wide_many_tables() -> FontSeedSpec {
        FontSeedSpec {
            num_tables: 512,
            table_len: 8192,
            circular_at: None,
            length_tamper_at: None,
            tampered_len: 0,
        }
    }
}

/// 按规格构造最小 sfnt 容器（确定性纯函数）。
///
/// 这是「真实字体集」语料源的替身：容器结构（头+目录+负载）与真实 sfnt
/// 同构，目录声明值可被三个变异算子独立破坏；真实字体字节级解析在 F0822，
/// 落位后本构造器的输出可直接换为其输入。
pub fn build_font_seed(spec: &FontSeedSpec) -> Vec<u8> {
    let dir_len = spec.num_tables as usize * SFNT_RECORD_LEN;
    let payload_len = spec.num_tables as usize * spec.table_len as usize;
    let mut v: Vec<u8> = Vec::with_capacity(SFNT_HEADER_LEN + dir_len + payload_len);
    // 头：TrueType 1.0 签名。
    push_be_u32(&mut v, 0x0001_0000);
    push_be_u16(&mut v, spec.num_tables);
    push_be_u16(&mut v, 64); // searchRange
    push_be_u16(&mut v, 3); // entrySelector
    push_be_u16(&mut v, spec.num_tables * 16); // rangeShift
    // 目录 + 负载布局：非循环表连续排布负载；循环表无负载（offset 入目录）。
    let mut next_payload = SFNT_HEADER_LEN + dir_len;
    let mut offsets: Vec<u32> = Vec::new();
    let mut lens: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < spec.num_tables as usize {
        if spec.circular_at == Some(i) {
            offsets.push(SFNT_HEADER_LEN as u32); // 指回目录起点 → 链永不逃逸
            lens.push(SFNT_RECORD_LEN as u32);
        } else {
            offsets.push(next_payload as u32);
            lens.push(spec.table_len);
            next_payload += spec.table_len as usize;
        }
        i += 1;
    }
    i = 0usize;
    while i < spec.num_tables as usize {
        push_be_u32(&mut v, 0x676C_7966u32.wrapping_add(i as u32)); // 'glyf'+i
        push_be_u32(&mut v, 0); // checksum
        push_be_u32(&mut v, offsets[i]);
        let len = if spec.length_tamper_at == Some(i) {
            spec.tampered_len
        } else {
            lens[i]
        };
        push_be_u32(&mut v, len);
        i += 1;
    }
    // 负载（全零占位——解析模型按目录声明值计账，不逐字节读负载）。
    let payload_actual = next_payload - (SFNT_HEADER_LEN + dir_len);
    v.resize(SFNT_HEADER_LEN + dir_len + payload_actual, 0);
    v
}

/// 大端写 u32。
fn push_be_u32(v: &mut Vec<u8>, x: u32) {
    v.push((x >> 24) as u8);
    v.push((x >> 16) as u8);
    v.push((x >> 8) as u8);
    v.push(x as u8);
}

/// 大端写 u16。
fn push_be_u16(v: &mut Vec<u8>, x: u16) {
    v.push((x >> 8) as u8);
    v.push(x as u8);
}

/// 大端读 u32（越界返回 0——读侧恒不越界是解析模型自己的纪律）。
fn be_u32(b: &[u8], at: usize) -> u32 {
    if at + 4 > b.len() {
        return 0;
    }
    ((b[at] as u32) << 24)
        | ((b[at + 1] as u32) << 16)
        | ((b[at + 2] as u32) << 8)
        | (b[at + 3] as u32)
}

/// 大端读 u16（越界返回 0）。
fn be_u16(b: &[u8], at: usize) -> u16 {
    if at + 2 > b.len() {
        return 0;
    }
    ((b[at] as u16) << 8) | (b[at + 1] as u16)
}

// ---------------------------------------------------------------------------
// 三、字体文件面：结构变异 + 沙箱解析模型
// ---------------------------------------------------------------------------

/// 字体文件结构变异算子（锚点三算子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontMutationOp {
    /// 表裁剪：从目录或负载区随机截断文件。
    TableTruncate,
    /// 长度篡改：把某表声明长度改到虚高（真实解析器按声明值预留内存）。
    LengthTamper,
    /// 循环链构造：把某表 offset 改指目录区（链跟踪永不逃逸）。
    CircularChain,
}

impl FontMutationOp {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            FontMutationOp::TableTruncate => "表裁剪",
            FontMutationOp::LengthTamper => "长度篡改",
            FontMutationOp::CircularChain => "循环链构造",
        }
    }

    /// 三算子全集（自检逐一验证真被用到）。
    pub const ALL: [FontMutationOp; 3] = [
        FontMutationOp::TableTruncate,
        FontMutationOp::LengthTamper,
        FontMutationOp::CircularChain,
    ];
}

/// 篡改用的虚高长度（256MB 附近，足以击穿 64MB 内存线）。
pub const TAMPERED_HUGE_LEN: u32 = 0x0F00_0000;

/// 对字体字节流做一次结构变异，返回实际用的算子。
///
/// 变异直接改**真实字节**（目录字段按 sfnt 布局定位），不是在模型层打标记；
/// 目录不完整时无法定位字段 → 退化为尾部追加垃圾字节（仍是有效变异：
/// 短头必然触发 ShortHeader 护栏）。返回算子供自检核对三算子真被用到。
pub fn mutate_font(base: &[u8], op: FontMutationOp, r: &mut Rng) -> (Vec<u8>, FontMutationOp) {
    let num_tables = be_u16(base, 4) as usize;
    let dir_end = SFNT_HEADER_LEN + num_tables * SFNT_RECORD_LEN;
    match op {
        FontMutationOp::TableTruncate => {
            let mut v = base.to_vec();
            if v.len() > SFNT_HEADER_LEN + 1 {
                // 截断点落在头之后、文件尾之前的随机位置。
                let cut = SFNT_HEADER_LEN + r.below((v.len() - SFNT_HEADER_LEN) as u64) as usize;
                v.truncate(cut);
            }
            (v, op)
        }
        FontMutationOp::LengthTamper => {
            let mut v = base.to_vec();
            if num_tables > 0 && dir_end <= v.len() {
                // 目录完整 → 定位随机一张表的 length 字段写虚高值。
                let slot = r.below(num_tables as u64) as usize;
                let at = SFNT_HEADER_LEN + slot * SFNT_RECORD_LEN + 12;
                v[at] = (TAMPERED_HUGE_LEN >> 24) as u8;
                v[at + 1] = (TAMPERED_HUGE_LEN >> 16) as u8;
                v[at + 2] = (TAMPERED_HUGE_LEN >> 8) as u8;
                v[at + 3] = TAMPERED_HUGE_LEN as u8;
            } else {
                // 目录不完整 → 尾部追加垃圾（短头/短目录亦是被测输入）。
                v.push(r.next_u64() as u8);
            }
            (v, op)
        }
        FontMutationOp::CircularChain => {
            let mut v = base.to_vec();
            if num_tables > 0 && dir_end <= v.len() {
                let slot = r.below(num_tables as u64) as usize;
                let at = SFNT_HEADER_LEN + slot * SFNT_RECORD_LEN + 8;
                v[at] = 0;
                v[at + 1] = 0;
                v[at + 2] = 0;
                v[at + 3] = SFNT_HEADER_LEN as u8; // offset → 目录起点
            } else {
                v.push(r.next_u64() as u8);
            }
            (v, op)
        }
    }
}

/// 沙箱护栏截停原因（互斥；与 F0810 SandboxVerdict 一一映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardStop {
    /// 头或目录不完整（表裁剪的可观测后果之一）。
    ShortHeader,
    /// 表数超限。
    TableCap,
    /// 声明内存总和超 64MB。
    MemCap,
    /// 工作量模型超 500ms。
    TimeCap,
    /// 循环链跳数超深度上限。
    DepthCap,
}

impl GuardStop {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            GuardStop::ShortHeader => "头/目录不完整",
            GuardStop::TableCap => "表数超限",
            GuardStop::MemCap => "内存超限",
            GuardStop::TimeCap => "超时截停",
            GuardStop::DepthCap => "深度超限",
        }
    }
}

/// 工作量模型：每表固定开销（ms）。
pub const WORKLOAD_MS_PER_TABLE: u64 = 2;
/// 工作量模型：每声明 8KB 计 1ms（确定性，无墙钟）。
pub const WORKLOAD_BYTES_PER_MS: u64 = 8192;

/// 沙箱解析模型的一次回报。
///
/// `panic_free` 由解析入口如实填写（本入口为全函数：一切读取走 `be_u32/
/// be_u16` 的边界检查包装，护栏全用显式计数器）；调用方**不得代填**——
/// 代填等于把「零崩溃」恒真化（vec19 同款纪律）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontParseReport {
    /// 是否全程无 panic / 无越界（由被测解析入口填写）。
    pub panic_free: bool,
    /// 是否跑完所有目录项（未被护栏截停）。
    pub completed: bool,
    /// 护栏截停原因。
    pub guarded: Option<GuardStop>,
    /// 声明内存峰值（各表声明长度之和，含最后一笔——真实解析器按声明值
    /// 预留，故「预留即计入」而非「读到才计入」）。
    pub peak_bytes: u64,
    /// 读到的表数（护栏截停时为截停前计数）。
    pub tables_seen: u32,
    /// 头里声明的表数（TableCap 对账用）。
    pub tables: u32,
    /// 链跟踪跳数（循环链的观测计）。
    pub hops: u32,
    /// 工作量模型耗时（ms）。
    pub workload_ms: u64,
    /// 声明区越过文件尾的表数（表裁剪的可观测后果）。
    pub truncated_payloads: u32,
    /// 截停前最后一笔声明长度（过冲界用）。
    pub last_claim: u64,
}

/// 跑沙箱解析模型：迭代、显式计数器、一切读取边界检查。
///
/// 这不是 F0822 的真实字体解析器——它是**声明值计账模型**：真实解析器
/// 的第一危害面正是「按目录声明值预留内存/展开循环」，模型按同一语义
/// 计账（声明即预留、链即展开），护栏数值与 F0810 沙箱同源
/// （`SandboxLimits::standard()`），并用真 `sandbox_check` 对账。
pub fn parse_font_sandboxed(bytes: &[u8], lim: &SandboxLimits) -> FontParseReport {
    let mut rep = FontParseReport {
        panic_free: true,
        completed: false,
        guarded: None,
        peak_bytes: 0,
        tables_seen: 0,
        tables: 0,
        hops: 0,
        workload_ms: 0,
        truncated_payloads: 0,
        last_claim: 0,
    };
    if bytes.len() < SFNT_HEADER_LEN {
        rep.guarded = Some(GuardStop::ShortHeader);
        return rep;
    }
    let num_tables = be_u16(bytes, 4) as u32;
    rep.tables = num_tables;
    if num_tables > lim.max_tables {
        rep.guarded = Some(GuardStop::TableCap);
        return rep;
    }
    let dir_end = SFNT_HEADER_LEN + num_tables as usize * SFNT_RECORD_LEN;
    if dir_end > bytes.len() {
        rep.guarded = Some(GuardStop::ShortHeader);
        return rep;
    }
    let mut i = 0u32;
    while i < num_tables {
        let rec = SFNT_HEADER_LEN + i as usize * SFNT_RECORD_LEN;
        let mut off = be_u32(bytes, rec + 8) as usize;
        let mut claimed = be_u32(bytes, rec + 12) as u64;
        // 链跟踪：offset 落在目录区内 → 逐跳跟随（循环链在此耗尽跳数）。
        while off >= SFNT_HEADER_LEN && off < dir_end {
            rep.hops += 1;
            if rep.hops > lim.max_depth {
                rep.guarded = Some(GuardStop::DepthCap);
                return rep;
            }
            let slot = (off - SFNT_HEADER_LEN) / SFNT_RECORD_LEN;
            let r2 = SFNT_HEADER_LEN + slot * SFNT_RECORD_LEN;
            off = be_u32(bytes, r2 + 8) as usize;
            claimed = be_u32(bytes, r2 + 12) as u64;
        }
        // 声明即预留：峰值与工作量按声明值计，不看文件里实际有几字节。
        rep.last_claim = claimed;
        rep.peak_bytes += claimed;
        rep.tables_seen += 1;
        rep.workload_ms =
            rep.tables_seen as u64 * WORKLOAD_MS_PER_TABLE + rep.peak_bytes / WORKLOAD_BYTES_PER_MS;
        if off as u64 + claimed > bytes.len() as u64 {
            rep.truncated_payloads += 1;
        }
        if rep.peak_bytes > lim.mem_bytes {
            rep.guarded = Some(GuardStop::MemCap);
            return rep;
        }
        if rep.workload_ms > lim.time_ms as u64 {
            rep.guarded = Some(GuardStop::TimeCap);
            return rep;
        }
        i += 1;
    }
    rep.completed = true;
    rep
}

// ---------------------------------------------------------------------------
// 四、字体面 oracle 三件套（判据二）
// ---------------------------------------------------------------------------

/// 字体面 oracle 结论（三件逐条可判，缺一即破）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontOracleResult {
    /// 解析器零崩溃：报告必达终态（完成或护栏截停）且 panic_free。
    pub zero_crash: bool,
    /// 沙箱内存 ≤64MB：声明总和超限必被 MemCap 拦截且过冲有界。
    pub mem_within: bool,
    /// 超时 ≤500ms 必终止：工作量过线必被 TimeCap 截停且不得跑完。
    pub timeout_terminates: bool,
    /// 与 F0810 真 `sandbox_check` 的判定一致（护栏理由 ↔ 判定结论映射）。
    pub sandbox_agrees: bool,
}

impl FontOracleResult {
    /// 三件全成立且沙箱对账一致。
    pub const fn all_hold(&self) -> bool {
        self.zero_crash && self.mem_within && self.timeout_terminates && self.sandbox_agrees
    }

    /// 被破坏的 oracle 项（逐条点名）。
    pub fn broken(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        if !self.zero_crash {
            v.push("解析器零崩溃");
        }
        if !self.mem_within {
            v.push("沙箱内存上限");
        }
        if !self.timeout_terminates {
            v.push("超时必终止");
        }
        if !self.sandbox_agrees {
            v.push("沙箱判定对账");
        }
        v
    }
}

/// 字体面 oracle 判定。
pub fn font_oracle(rep: &FontParseReport, lim: &SandboxLimits) -> FontOracleResult {
    // 真 sandbox_check 拿报告字段作输入——护栏与判定必须同结论。
    let verdict = sandbox_check(
        &SandboxInput {
            peak_bytes: rep.peak_bytes,
            elapsed_ms: rep.workload_ms.min(u32::MAX as u64) as u32,
            depth: rep.hops,
            tables: rep.tables,
        },
        lim,
    );
    let zero_crash = rep.panic_free && (rep.completed || rep.guarded.is_some());
    // 内存件：超限 ⇒ MemCap 拦截 + 过冲不超过最后一笔声明（多走一整表
    // 即护栏迟钝）+ 真 sandbox_check 同判 MemExceeded。
    let mem_exceeded = rep.peak_bytes > lim.mem_bytes || rep.tables > lim.max_tables;
    let mem_within = if mem_exceeded {
        matches!(rep.guarded, Some(GuardStop::MemCap) | Some(GuardStop::TableCap))
            && rep.peak_bytes.saturating_sub(lim.mem_bytes) <= rep.last_claim
            && verdict == SandboxVerdict::MemExceeded
    } else {
        verdict != SandboxVerdict::MemExceeded
    };
    // 超时件：内存线内且工作量过线 ⇒ TimeCap 截停且未跑完。
    let timeout_terminates = if rep.peak_bytes <= lim.mem_bytes
        && rep.tables <= lim.max_tables
        && rep.workload_ms > lim.time_ms as u64
    {
        rep.guarded == Some(GuardStop::TimeCap)
            && !rep.completed
            && verdict == SandboxVerdict::TimedOut
    } else {
        verdict != SandboxVerdict::TimedOut
    };
    // 对账件：护栏理由 ↔ sandbox_check 结论映射（含 TableCap→MemExceeded，
    // 表目录超限归内存类——与 F0810 sandbox_check 同一优先级语义）。
    let expected_verdict = match rep.guarded {
        Some(GuardStop::MemCap) | Some(GuardStop::TableCap) => SandboxVerdict::MemExceeded,
        Some(GuardStop::TimeCap) => SandboxVerdict::TimedOut,
        Some(GuardStop::DepthCap) => SandboxVerdict::DepthExceeded,
        // ShortHeader 是结构拒绝，资源四项全在限内 → Passed（结构拒绝是
        // verify_file 的职责，sandbox_check 只判资源）。
        Some(GuardStop::ShortHeader) | None => SandboxVerdict::Passed,
    };
    FontOracleResult {
        zero_crash,
        mem_within,
        timeout_terminates,
        sandbox_agrees: verdict == expected_verdict,
    }
}

// ---------------------------------------------------------------------------
// 五、编码序列面：边界注入 + 真解码器 oracle（判据一、二）
// ---------------------------------------------------------------------------

/// 编码边界注入算子（锚点三注入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncInjection {
    /// 截断多字节：缓冲在序列中间结束。
    TruncateMultibyte,
    /// 代理区：CESU-8 形式的 U+D800。
    SurrogateZone,
    /// 过长编码：0xC0 引导的两字节「/」。
    OverlongEncoding,
}

impl EncInjection {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            EncInjection::TruncateMultibyte => "截断多字节",
            EncInjection::SurrogateZone => "代理区",
            EncInjection::OverlongEncoding => "过长编码",
        }
    }

    /// 三算子全集。
    pub const ALL: [EncInjection; 3] = [
        EncInjection::TruncateMultibyte,
        EncInjection::SurrogateZone,
        EncInjection::OverlongEncoding,
    ];
}

/// 代理区注入载荷：0xD800 的 CESU-8 形式（0xED 0xA0 0x80）。
pub const SURROGATE_PAYLOAD: [u8; 3] = [0xED, 0xA0, 0x80];
/// 过长编码注入载荷：0xC0 0xAF = overlong '/'。
pub const OVERLONG_PAYLOAD: [u8; 2] = [0xC0, 0xAF];
/// 孤立引导字节（无续字节可用时的截断注入载荷）。
pub const TRUNCATED_LEAD: u8 = 0xE5;

/// 合法多字节种子（变异层基线）。
pub const VALID_SEEDS: [&[u8]; 4] = [
    b"Hello, \xE4\xB8\x96\xE7\x95\x8C!", // "Hello, 世界!"
    b"UTF-8 \xE5\xB1\xB1\xE6\xB2\xB3 123", // "UTF-8 山河 123"
    b"caf\xC3\xA9 na\xC3\xAFve",         // "café naïve"
    b"plain ascii only",
];

/// 在指定位置做边界注入（确定性；回归语料用固定位置）。
///
/// TruncateMultibyte 的 pos 语义：截断到 pos（pos 应落在某多字节序列内部，
/// 缓冲于是结束在序列中间）；找不到合适位置时调用方传 lead+1。其余两算子
/// 忽略 pos、直接尾部追加载荷。
pub fn inject_encoding_at(base: &[u8], kind: EncInjection, pos: usize) -> Vec<u8> {
    let mut v = base.to_vec();
    match kind {
        EncInjection::TruncateMultibyte => {
            let cut = pos.min(v.len());
            v.truncate(cut);
        }
        EncInjection::SurrogateZone => {
            v.extend_from_slice(&SURROGATE_PAYLOAD);
        }
        EncInjection::OverlongEncoding => {
            v.extend_from_slice(&OVERLONG_PAYLOAD);
        }
    }
    v
}

/// 随机位置版注入（fuzz 批次用）：返回（注入后字节、实际算子、注入位置）。
pub fn inject_encoding(
    base: &[u8],
    kind: EncInjection,
    r: &mut Rng,
) -> (Vec<u8>, EncInjection, usize) {
    match kind {
        EncInjection::TruncateMultibyte => {
            // 找最后一个多字节引导字节，截在 lead+1（序列中间）。
            let mut lead = None;
            let mut i = base.len();
            while i > 0 {
                i -= 1;
                if base[i] >= 0x80 && base[i] & 0xC0 != 0x80 {
                    lead = Some(i + 1);
                    break;
                }
            }
            match lead {
                Some(pos) => (inject_encoding_at(base, kind, pos), kind, pos),
                // 无多字节 → 追加孤立引导字节再截断（等价于尾部截断序列）。
                None => {
                    let mut v = base.to_vec();
                    v.push(TRUNCATED_LEAD);
                    let end = v.len();
                    (v, kind, end)
                }
            }
        }
        _ => {
            let pos = r.below(base.len() as u64) as usize;
            (inject_encoding_at(base, kind, pos), kind, pos)
        }
    }
}

/// 下游码点合法性谓词（**判据侧独立实现**：不从 vee02 借 is_noncharacter）。
///
/// 下游（轮廓抽取 F0803 / 整形 F0842）只允许收到合法 Unicode 标量值
/// （替换码点 U+FFFD 本身合法）。非法三类：超出 Unicode 上界、代理区、
/// 非字符（U+FDD0..=U+FDEF 与各平面末两位 FFFE/FFFF）。
pub fn illegal_for_downstream(cp: u32) -> bool {
    if cp > MAX_CODEPOINT {
        return true;
    }
    if (SURROGATE_FIRST..=SURROGATE_LAST).contains(&cp) {
        return true;
    }
    if (0xFDD0..=0xFDEF).contains(&cp) {
        return true;
    }
    let low = cp & 0xFFFF;
    low == 0xFFFE || low == 0xFFFF
}

/// 编码面 oracle 结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncOracleResult {
    /// 四档处置计数一致：非法处置总数 == 无效偏移表长度，行尾待续 ⇒ 截断
    /// 档已计数（两条计数通道必须同步——各自独立累加，改坏任一路径即分叉）。
    pub counts_consistent: bool,
    /// 注入类别对应的处置档真被激起（expect=None 时恒真——干净种子走这里）。
    pub expected_fired: bool,
    /// 下游不接收无效码点：输出逐个过独立合法性谓词。
    pub downstream_clean: bool,
}

impl EncOracleResult {
    /// 三件全成立。
    pub const fn all_hold(&self) -> bool {
        self.counts_consistent && self.expected_fired && self.downstream_clean
    }

    /// 被破坏的 oracle 项。
    pub fn broken(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        if !self.counts_consistent {
            v.push("处置计数一致");
        }
        if !self.expected_fired {
            v.push("注入档位生效");
        }
        if !self.downstream_clean {
            v.push("下游码点合法");
        }
        v
    }
}

/// 编码面 oracle 判定：拿真解码结果 + 期望档位（若该条带注入标签）。
pub fn encoding_oracle(res: &DecodeResult, expect: Option<EncInjection>) -> EncOracleResult {
    let counts_consistent = res.stats.invalid_total() as usize == res.invalid_offsets.len()
        && res.stats.codepoints == res.codepoints.len()
        && (res.stats.truncated_tail || res.stats.disp_count(Disposition::Truncated) == 0);
    let expected_fired = match expect {
        None => true,
        Some(EncInjection::TruncateMultibyte) => {
            res.stats.disp_count(Disposition::Truncated) >= 1
        }
        Some(EncInjection::SurrogateZone) => res.stats.disp_count(Disposition::OutOfRange) >= 1,
        Some(EncInjection::OverlongEncoding) => res.stats.disp_count(Disposition::Overlong) >= 1,
    };
    let downstream_clean = !res.codepoints.iter().any(|cp| illegal_for_downstream(*cp));
    EncOracleResult {
        counts_consistent,
        expected_fired,
        downstream_clean,
    }
}

// ---------------------------------------------------------------------------
// 六、发现定级与最小化（错误路径：崩溃→最小化→定级→入回归账）
// ---------------------------------------------------------------------------

/// 崩溃定级（内存安全最高——与全域宪法 VS-P1 同名同序；AE10 F6385 总闸
/// 落位后按此序对接，本域先持口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashClass {
    /// 内存安全（越界/超限/崩溃等价物）——最高级。
    MemorySafety,
    /// 挂起（该终止不终止）。
    Hang,
    /// 误解析（处置档/下游码点错误）。
    Misparse,
    /// oracle 自身问题（修 oracle 不修判据）。
    OracleFault,
}

impl CrashClass {
    /// 定级序（0 最高）。
    pub const fn rank(self) -> usize {
        match self {
            CrashClass::MemorySafety => 0,
            CrashClass::Hang => 1,
            CrashClass::Misparse => 2,
            CrashClass::OracleFault => 3,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            CrashClass::MemorySafety => "内存安全",
            CrashClass::Hang => "挂起",
            CrashClass::Misparse => "误解析",
            CrashClass::OracleFault => "oracle 误报",
        }
    }
}

/// 一条发现项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding14 {
    /// 定级。
    pub class: CrashClass,
    /// 被破坏的 oracle 项名。
    pub reason: &'static str,
    /// 复现键。
    pub key: ReproKey,
    /// 触发输入（保留供复现与最小化）。
    pub input: Vec<u8>,
    /// 产生本发现时的 oracle 版本（免责对账用）。
    pub oracle_ver: u32,
}

/// 发现账：如实记账、逐条定级、按版本免责。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindingLog14 {
    findings: Vec<Finding14>,
}

impl FindingLog14 {
    /// 新建空账。
    pub fn new() -> FindingLog14 {
        FindingLog14 {
            findings: Vec::new(),
        }
    }

    /// 记一条。
    pub fn record(&mut self, f: Finding14) {
        self.findings.push(f);
    }

    /// 未修发现总数。
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// 全部发现。
    pub fn findings(&self) -> &[Finding14] {
        self.findings.as_slice()
    }

    /// 未修的崩溃级（MemorySafety/Hang）数量——>0 即门禁阻断。
    pub fn open_critical(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.findings.len() {
            if self.findings[i].class.rank() <= CrashClass::Hang.rank() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 按版本免责：只有当免责版本严格大于发现产生时的 oracle 版本
    /// （即 oracle 真被修过）才允许摘除。**免责的唯一出口是修 oracle**，
    /// 想靠改判据放行在这里没有入口。
    pub fn dismiss_by_oracle_fix(&mut self, reason: &'static str, fixed_ver: u32) -> usize {
        let mut removed = 0usize;
        let mut i = self.findings.len();
        while i > 0 {
            i -= 1;
            if self.findings[i].reason == reason && fixed_ver > self.findings[i].oracle_ver {
                self.findings.remove(i);
                removed += 1;
            }
        }
        removed
    }
}

impl Default for FindingLog14 {
    fn default() -> FindingLog14 {
        FindingLog14::new()
    }
}

/// 从 oracle 破坏项定级（字体面）。
fn classify_font(broken: &[&'static str]) -> CrashClass {
    let mut i = 0usize;
    while i < broken.len() {
        if broken[i] == "解析器零崩溃" || broken[i] == "沙箱内存上限" {
            return CrashClass::MemorySafety;
        }
        if broken[i] == "超时必终止" {
            return CrashClass::Hang;
        }
        i += 1;
    }
    // 只剩「沙箱判定对账」：护栏与 sandbox_check 各执一词 → oracle 层问题。
    CrashClass::OracleFault
}

/// 从 oracle 破坏项定级（编码面）。
fn classify_enc(broken: &[&'static str]) -> CrashClass {
    let mut i = 0usize;
    while i < broken.len() {
        if broken[i] == "下游码点合法" {
            return CrashClass::MemorySafety;
        }
        i += 1;
    }
    CrashClass::Misparse
}

/// 最小化样本：增量式 delta debugging（确定性）。
///
/// 折半块长从大到小逐块尝试删除，删除后失败谓词仍真才收下；块长到 1
/// 再扫一轮为止。判据（`still_fails`）由调用方提供——最小化只对「仍能
/// 复现」负责，不解释为什么仍复现。
pub fn minimize_sample(bytes: &[u8], still_fails: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut cur = bytes.to_vec();
    if cur.is_empty() || !still_fails(&cur) {
        return cur;
    }
    let mut chunk = cur.len() / 2;
    while chunk >= 1 {
        let mut i = 0usize;
        while i < cur.len() {
            let end = (i + chunk).min(cur.len());
            let mut cand: Vec<u8> = Vec::with_capacity(cur.len() - (end - i));
            cand.extend_from_slice(&cur[..i]);
            cand.extend_from_slice(&cur[end..]);
            if still_fails(&cand) {
                cur = cand;
            } else {
                i += chunk;
            }
            if cur.len() < chunk {
                break;
            }
        }
        chunk /= 2;
    }
    cur
}

// ---------------------------------------------------------------------------
// 七、回归语料闭环（判据三）
// ---------------------------------------------------------------------------

/// oracle 版本（误报免责的版本基准；修 oracle 必须递增）。
pub const ORACLE_VERSION: u32 = 3;

/// 回归期望（回归用例断言的当前实现行为）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegressionExpect {
    /// 指定护栏必须截停。
    GuardFires(GuardStop),
    /// 不得有任何护栏截停（干净输入的回归面——护栏误伤即红）。
    GuardQuiet,
    /// 指定处置档必须激起。
    DispositionFires(Disposition),
}

/// 一条回归用例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegressionCase {
    /// 用例名（账内去重键）。
    pub name: String,
    /// 所属面。
    pub face: FuzzFace,
    /// 回归输入。
    pub bytes: Vec<u8>,
    /// 期望行为。
    pub expect: RegressionExpect,
    /// 是否来自真实 fuzz 发现（promote 上来的都为真）。
    pub from_crash: bool,
}

/// 回归执行结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegressionOutcome {
    /// 用例名。
    pub name: String,
    /// 期望是否仍成立。
    pub held: bool,
}

/// 回归账：闭环的载体——每例必跑、结果如实记账、发现可晋升入账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegressionLedger {
    cases: Vec<RegressionCase>,
}

impl RegressionLedger {
    /// 标准回归账（历史崩溃回归语料的种子集：三算子各一例 + 干净基线 +
    /// 两个编码边界——缺一条，「护栏/处置回归」就少一层既有防）。
    pub fn standard() -> RegressionLedger {
        let mut led = RegressionLedger { cases: Vec::new() };
        // 字体面：循环链 → DepthCap。
        led.cases.push(RegressionCase {
            name: "字体-循环链-深度截停".to_string(),
            face: FuzzFace::FontFile,
            bytes: build_font_seed(&FontSeedSpec {
                circular_at: Some(0),
                ..FontSeedSpec::clean_small()
            }),
            expect: RegressionExpect::GuardFires(GuardStop::DepthCap),
            from_crash: true,
        });
        // 字体面：长度篡改 → MemCap。
        led.cases.push(RegressionCase {
            name: "字体-长度篡改-内存截停".to_string(),
            face: FuzzFace::FontFile,
            bytes: build_font_seed(&FontSeedSpec {
                length_tamper_at: Some(1),
                tampered_len: TAMPERED_HUGE_LEN,
                ..FontSeedSpec::clean_small()
            }),
            expect: RegressionExpect::GuardFires(GuardStop::MemCap),
            from_crash: true,
        });
        // 字体面：干净基线 → 护栏全静（护栏误伤干净字体即红）。
        led.cases.push(RegressionCase {
            name: "字体-干净基线-护栏静默".to_string(),
            face: FuzzFace::FontFile,
            bytes: build_font_seed(&FontSeedSpec::clean_small()),
            expect: RegressionExpect::GuardQuiet,
            from_crash: false,
        });
        // 编码面：代理区 → 越界档。
        led.cases.push(RegressionCase {
            name: "编码-代理区-越界档".to_string(),
            face: FuzzFace::EncodingSeq,
            bytes: inject_encoding_at(VALID_SEEDS[0], EncInjection::SurrogateZone, 0),
            expect: RegressionExpect::DispositionFires(Disposition::OutOfRange),
            from_crash: true,
        });
        // 编码面：过长编码 → 过长档。
        led.cases.push(RegressionCase {
            name: "编码-过长-过长档".to_string(),
            face: FuzzFace::EncodingSeq,
            bytes: inject_encoding_at(VALID_SEEDS[0], EncInjection::OverlongEncoding, 0),
            expect: RegressionExpect::DispositionFires(Disposition::Overlong),
            from_crash: true,
        });
        // 编码面：截断多字节 → 截断档（"世界" 的 引导字节后截）。
        led.cases.push(RegressionCase {
            name: "编码-截断尾-截断档".to_string(),
            face: FuzzFace::EncodingSeq,
            bytes: inject_encoding_at(VALID_SEEDS[0], EncInjection::TruncateMultibyte, 9),
            expect: RegressionExpect::DispositionFires(Disposition::Truncated),
            from_crash: true,
        });
        led
    }

    /// 用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 全部用例。
    pub fn cases(&self) -> &[RegressionCase] {
        self.cases.as_slice()
    }

    /// 晋升：fuzz 发现（最小化后）入账。重名拒绝（回归账不收重复账）。
    pub fn promote(&mut self, case: RegressionCase) -> bool {
        if self.cases.iter().any(|c| c.name == case.name) {
            return false;
        }
        self.cases.push(case);
        true
    }

    /// 跑全部用例（每例必跑——nightly 与增量一视同仁）。
    pub fn run_all(&self, lim: &SandboxLimits) -> Vec<RegressionOutcome> {
        let mut out: Vec<RegressionOutcome> = Vec::new();
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = &self.cases[i];
            let held = match c.face {
                FuzzFace::FontFile => {
                    let rep = parse_font_sandboxed(&c.bytes, lim);
                    match c.expect {
                        RegressionExpect::GuardFires(g) => rep.guarded == Some(g),
                        RegressionExpect::GuardQuiet => rep.guarded.is_none(),
                        RegressionExpect::DispositionFires(_) => false,
                    }
                }
                FuzzFace::EncodingSeq => {
                    let res = Decoder::new().decode(&c.bytes, &DecodeOptions::new());
                    match c.expect {
                        RegressionExpect::DispositionFires(d) => {
                            res.stats.disp_count(d) >= 1
                        }
                        _ => false,
                    }
                }
            };
            out.push(RegressionOutcome {
                name: c.name.clone(),
                held,
            });
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 八、运行节奏与门禁（判据四、判据五）
// ---------------------------------------------------------------------------

/// 运行节奏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    /// nightly 全量：预算 30 分钟。
    Nightly,
    /// 提交前增量：预算 3 分钟（回归账必须全量跑）。
    PreCommit,
}

impl Cadence {
    /// 预算（秒）。
    pub const fn budget_secs(self) -> u64 {
        match self {
            Cadence::Nightly => NIGHTLY_BUDGET_SECS as u64,
            Cadence::PreCommit => PRECOMMIT_BUDGET_SECS as u64,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            Cadence::Nightly => "nightly 全量",
            Cadence::PreCommit => "提交前增量",
        }
    }
}

/// nightly 预算：30 分钟（锚点原文数值）。
pub const NIGHTLY_BUDGET_SECS: u32 = 1800;
/// 提交前增量预算：3 分钟。
pub const PRECOMMIT_BUDGET_SECS: u32 = 180;
/// 确定性成本模型吞吐（成本单位/秒）——无墙钟环境的预算折算。
pub const COST_UNITS_PER_SEC: u64 = 64;

/// 单条语料成本（单位）= 1 + 字节数/256。
pub fn entry_cost_units(bytes_len: usize) -> u64 {
    1 + (bytes_len / 256) as u64
}

/// 批次规划结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchPlan {
    /// 计划语料条数。
    pub total_entries: usize,
    /// 估算耗时（秒，确定性模型）。
    pub est_secs: u64,
    /// 是否覆盖全量语料（nightly 必须全量）。
    pub covers_full_corpus: bool,
    /// 回归账是否全量纳入（两种节奏都必须）。
    pub regression_included: bool,
}

/// 批次规划。
///
/// nightly 全量；增量取全量的 1/10（下限 8 条，保证新语料面仍有抽样）
/// 但回归账两种节奏都全量——增量可以缩新语料，不能缩回归。
pub fn plan_batch(
    cadence: Cadence,
    corpus_entries: usize,
    corpus_cost: u64,
    regression_cases: usize,
) -> BatchPlan {
    match cadence {
        Cadence::Nightly => BatchPlan {
            total_entries: corpus_entries,
            est_secs: corpus_cost / COST_UNITS_PER_SEC,
            covers_full_corpus: true,
            regression_included: true,
        },
        Cadence::PreCommit => {
            let subset = (corpus_entries / 10).max(8).min(corpus_entries.max(1));
            let unit = if corpus_entries > 0 {
                corpus_cost / corpus_entries as u64
            } else {
                0
            };
            BatchPlan {
                total_entries: subset,
                est_secs: (subset as u64 * unit) / COST_UNITS_PER_SEC,
                covers_full_corpus: false,
                regression_included: true,
            }
        }
    }
    .with_regression(regression_cases)
}

impl BatchPlan {
    /// 补记回归账纳入情况（回归成本并入估算）。
    fn with_regression(mut self, regression_cases: usize) -> BatchPlan {
        if regression_cases > 0 {
            // 回归每例按最小成本单位计。
            self.est_secs += regression_cases as u64 / COST_UNITS_PER_SEC;
        }
        self
    }
}

/// 一轮文字渲染 fuzz 的汇总。
#[derive(Clone, Debug)]
pub struct TextFuzzReport {
    /// 节奏。
    pub cadence: Cadence,
    /// 各面条数。
    pub face_entries: [usize; 2],
    /// 各源条数。
    pub source_entries: [usize; 3],
    /// 实跑条数。
    pub processed: usize,
    /// oracle 全绿条数。
    pub passed: usize,
    /// 发现账。
    pub log: FindingLog14,
    /// 回归执行数。
    pub regressions_run: usize,
    /// 回归破坏数。
    pub regressions_failed: usize,
    /// 批次规划。
    pub plan: BatchPlan,
}

/// 门禁裁定（崩溃零容忍 + 覆盖完整性 + 回归闭环 + 预算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFuzzGate {
    /// 放行。
    Allow,
    /// 有未修崩溃级发现（MemorySafety/Hang）。
    CriticalOpen {
        /// 条数。
        open: usize,
    },
    /// 回归账有破坏（历史发现复发）。
    RegressionBroken {
        /// 破坏数。
        failed: usize,
    },
    /// 面×源覆盖退化（缺任一面或任一源）。
    CorpusDegenerate,
    /// 一条都没跑。
    NoInput,
    /// 估算超节奏预算。
    BudgetBroken {
        /// 估算秒。
        est: u64,
        /// 预算秒。
        cap: u64,
    },
}

impl TextFuzzGate {
    /// 是否阻断。
    pub const fn blocked(self) -> bool {
        !matches!(self, TextFuzzGate::Allow)
    }
}

/// 判据五条 stamp（锚点原文判据，常量化——修 oracle 不得顺手动判据；
/// 判据侧按此表逐条对账）。
pub const CRITERIA_STAMP: [&str; 5] = [
    "两面 fuzz",
    "oracle 三件套",
    "回归语料闭环",
    "30 分钟 nightly",
    "崩溃零容忍",
];

/// 门禁裁定。
pub fn gate_of(report: &TextFuzzReport) -> TextFuzzGate {
    if report.processed == 0 {
        return TextFuzzGate::NoInput;
    }
    // 面×源覆盖：任一为 0 即退化（两面 fuzz、语料三源都写进门禁）。
    let mut i = 0usize;
    while i < 2 {
        if report.face_entries[i] == 0 {
            return TextFuzzGate::CorpusDegenerate;
        }
        i += 1;
    }
    i = 0;
    while i < 3 {
        if report.source_entries[i] == 0 {
            return TextFuzzGate::CorpusDegenerate;
        }
        i += 1;
    }
    if report.regressions_failed > 0 {
        return TextFuzzGate::RegressionBroken {
            failed: report.regressions_failed,
        };
    }
    let open = report.log.open_critical();
    if open > 0 {
        return TextFuzzGate::CriticalOpen { open };
    }
    if report.plan.est_secs > report.cadence.budget_secs() {
        return TextFuzzGate::BudgetBroken {
            est: report.plan.est_secs,
            cap: report.cadence.budget_secs(),
        };
    }
    TextFuzzGate::Allow
}

// ---------------------------------------------------------------------------
// 九、批次执行（把三源×两面喂进 oracle，发现逐条入账）
// ---------------------------------------------------------------------------

/// fuzz 运行配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFuzzConfig {
    /// 主种子（自持 LCG，跨机可复现）。
    pub seed: u64,
    /// 每源条数。
    pub per_source: usize,
    /// 节奏。
    pub cadence: Cadence,
}

/// 混合子种子（face/source/index 折进主种子——单独抽第 N 条可复现）。
fn mix_seed(seed: u64, face: FuzzFace, source: CorpusSource, index: u64) -> u64 {
    let f = match face {
        FuzzFace::FontFile => 0x5Au64,
        FuzzFace::EncodingSeq => 0xA5,
    };
    let s = match source {
        CorpusSource::RealFonts => 0x11u64,
        CorpusSource::MutatedSeeds => 0x22,
        CorpusSource::CrashRegression => 0x33,
    };
    seed ^ f.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ s.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ index
}

/// 按（面,源,序号）确定性重建输入——`replay` 的实现核心。
pub fn build_entry(
    face: FuzzFace,
    source: CorpusSource,
    seed: u64,
    index: usize,
    led: &RegressionLedger,
) -> Vec<u8> {
    let mut r = Rng::new(mix_seed(seed, face, source, index as u64));
    match (face, source) {
        (FuzzFace::FontFile, CorpusSource::RealFonts) => {
            if index % 2 == 0 {
                build_font_seed(&FontSeedSpec::clean_small())
            } else {
                build_font_seed(&FontSeedSpec::wide_many_tables())
            }
        }
        (FuzzFace::FontFile, CorpusSource::MutatedSeeds) => {
            let base = build_font_seed(&FontSeedSpec::clean_small());
            let op = FontMutationOp::ALL[index % FontMutationOp::ALL.len()];
            mutate_font(&base, op, &mut r).0
        }
        (FuzzFace::FontFile, CorpusSource::CrashRegression) => {
            // 回归源：抽字体面回归用例（标准账恒有，抽空时退回干净种子）。
            let font_cases: Vec<&RegressionCase> = led
                .cases()
                .iter()
                .filter(|c| c.face == FuzzFace::FontFile)
                .collect();
            if font_cases.is_empty() {
                build_font_seed(&FontSeedSpec::clean_small())
            } else {
                font_cases[index % font_cases.len()].bytes.clone()
            }
        }
        (FuzzFace::EncodingSeq, CorpusSource::RealFonts) => {
            VALID_SEEDS[index % VALID_SEEDS.len()].to_vec()
        }
        (FuzzFace::EncodingSeq, CorpusSource::MutatedSeeds) => {
            let base = VALID_SEEDS[index % VALID_SEEDS.len()];
            let op = EncInjection::ALL[index % EncInjection::ALL.len()];
            inject_encoding(base, op, &mut r).0
        }
        (FuzzFace::EncodingSeq, CorpusSource::CrashRegression) => {
            let enc_cases: Vec<&RegressionCase> = led
                .cases()
                .iter()
                .filter(|c| c.face == FuzzFace::EncodingSeq)
                .collect();
            if enc_cases.is_empty() {
                VALID_SEEDS[0].to_vec()
            } else {
                enc_cases[index % enc_cases.len()].bytes.clone()
            }
        }
    }
}

/// 跑一轮文字渲染 fuzz（两面×三源全覆盖 → oracle → 发现入账 → 回归账）。
pub fn run_text_fuzz(cfg: &TextFuzzConfig, lim: &SandboxLimits, led: &RegressionLedger) -> TextFuzzReport {
    let mut face_entries = [0usize; 2];
    let mut source_entries = [0usize; 3];
    let mut log = FindingLog14::new();
    let mut processed = 0usize;
    let mut passed = 0usize;
    let mut corpus_cost = 0u64;

    let mut face_i = 0usize;
    while face_i < FuzzFace::ALL.len() {
        let face = FuzzFace::ALL[face_i];
        let mut src_i = 0usize;
        while src_i < CorpusSource::ALL.len() {
            let source = CorpusSource::ALL[src_i];
            let mut k = 0usize;
            while k < cfg.per_source {
                let bytes = build_entry(face, source, cfg.seed, k, led);
                let key = ReproKey {
                    face,
                    source,
                    seed: cfg.seed,
                    index: k as u64,
                };
                corpus_cost += entry_cost_units(bytes.len());
                face_entries[face.ordinal()] += 1;
                source_entries[source.ordinal()] += 1;
                processed += 1;
                let mut ok = true;
                match face {
                    FuzzFace::FontFile => {
                        let rep = parse_font_sandboxed(&bytes, lim);
                        let o = font_oracle(&rep, lim);
                        if !o.all_hold() {
                            ok = false;
                            let broken = o.broken();
                            log.record(Finding14 {
                                class: classify_font(&broken),
                                reason: broken[0],
                                key,
                                input: bytes.clone(),
                                oracle_ver: ORACLE_VERSION,
                            });
                        }
                    }
                    FuzzFace::EncodingSeq => {
                        let res = Decoder::new().decode(&bytes, &DecodeOptions::new());
                        // 变异源带注入标签（期望档位），其余源无标签。
                        let expect = match source {
                            CorpusSource::MutatedSeeds => {
                                Some(EncInjection::ALL[k % EncInjection::ALL.len()])
                            }
                            _ => None,
                        };
                        let o = encoding_oracle(&res, expect);
                        if !o.all_hold() {
                            ok = false;
                            let broken = o.broken();
                            log.record(Finding14 {
                                class: classify_enc(&broken),
                                reason: broken[0],
                                key,
                                input: bytes.clone(),
                                oracle_ver: ORACLE_VERSION,
                            });
                        }
                    }
                }
                if ok {
                    passed += 1;
                }
                k += 1;
            }
            src_i += 1;
        }
        face_i += 1;
    }

    // 回归账每例必跑（两种节奏同权）。
    let outcomes = led.run_all(lim);
    let mut regressions_failed = 0usize;
    let mut i = 0usize;
    while i < outcomes.len() {
        if !outcomes[i].held {
            regressions_failed += 1;
        }
        i += 1;
    }

    let plan = plan_batch(
        cfg.cadence,
        processed,
        corpus_cost,
        led.len(),
    );
    TextFuzzReport {
        cadence: cfg.cadence,
        face_entries,
        source_entries,
        processed,
        passed,
        log,
        regressions_run: outcomes.len(),
        regressions_failed,
        plan,
    }
}

/// 从复现键重建输入（报缺陷/复现入口）。
pub fn replay(key: &ReproKey, led: &RegressionLedger) -> Vec<u8> {
    build_entry(key.face, key.source, key.seed, key.index as usize, led)
}

/// 人话呈现（缺陷流程入口）。
pub fn render_report(rep: &TextFuzzReport) -> String {
    format!(
        "文字渲染 fuzz[{}]：跑 {} 条，过 {} 条；面 字体文件 {}/编码 {}；源 真实 {}/变异 {}/回归 {}；回归 {}/{} 破坏\n发现账未修 {} 条（崩溃级 {}）",
        rep.cadence.label(),
        rep.processed,
        rep.passed,
        rep.face_entries[0],
        rep.face_entries[1],
        rep.source_entries[0],
        rep.source_entries[1],
        rep.source_entries[2],
        rep.regressions_run - rep.regressions_failed,
        rep.regressions_run,
        rep.log.len(),
        rep.log.open_critical(),
    )
}

// ---------------------------------------------------------------------------
// 十、域自检（判据区零 panic 面：固定下标一律走 get/Option，None 记红）
// ---------------------------------------------------------------------------

/// 判据侧独立重排的锚点判据五条（与 `CRITERIA_STAMP` 逐条对账——判据常量
/// 被误改时这里先红；修 oracle 不得顺手动判据，两边都不许悄悄改）。
const CRITERIA_RECHECK: [&str; 5] = [
    "两面 fuzz",
    "oracle 三件套",
    "回归语料闭环",
    "30 分钟 nightly",
    "崩溃零容忍",
];

/// 独立实现的「含过长编码载荷」谓词（最小化判据用，不从被测路径借）。
fn recheck_has_overlong(b: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == 0xC0 && b[i + 1] == 0xAF {
            return true;
        }
        i += 1;
    }
    false
}

/// VE-F0814 域自检入口（聚合器 `run_svstar2_checks` 调用）。
///
/// 判据五条逐条对账；关键判据配**反向语料**（脏输入/污染报告），证明
/// 门禁真的会转红——只断内置路径恒绿的门禁等于没门禁。
pub fn run_vee14_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vee14_textfuzz");
    let lim = SandboxLimits::standard();
    let led = RegressionLedger::standard();

    // —— 判据一 · 两面 fuzz × 语料三源：真跑一轮，计数非零且总量守恒 ——
    let cfg = TextFuzzConfig {
        seed: 0x5EED_0814,
        per_source: 4,
        cadence: Cadence::PreCommit,
    };
    let rep = run_text_fuzz(&cfg, &lim, &led);
    let total_expected = FuzzFace::ALL.len() * CorpusSource::ALL.len() * cfg.per_source;
    let mut face_sum = 0usize;
    let mut faces_nonzero = true;
    let mut fi = 0usize;
    while fi < FuzzFace::ALL.len() {
        match rep.face_entries.get(fi) {
            Some(v) => {
                if *v == 0 {
                    faces_nonzero = false;
                }
                face_sum += *v;
            }
            None => faces_nonzero = false,
        }
        fi += 1;
    }
    let mut src_sum = 0usize;
    let mut srcs_nonzero = true;
    let mut si = 0usize;
    while si < CorpusSource::ALL.len() {
        match rep.source_entries.get(si) {
            Some(v) => {
                if *v == 0 {
                    srcs_nonzero = false;
                }
                src_sum += *v;
            }
            None => srcs_nonzero = false,
        }
        si += 1;
    }
    s.add(
        "A46-两面三源-真跑一轮计数守恒",
        rep.processed == total_expected
            && faces_nonzero
            && srcs_nonzero
            && face_sum == rep.processed
            && src_sum == rep.processed
            && rep.passed == rep.processed
            && rep.log.is_empty(),
        "两面×三源×4 条=24 全跑全过、面/源计数非零且各自求和=processed",
    );

    // —— 判据二 · oracle 三件套（字体面）：干净种子全绿且跑完 ——
    let clean = build_font_seed(&FontSeedSpec::clean_small());
    let rep_clean = parse_font_sandboxed(&clean, &lim);
    let o_clean = font_oracle(&rep_clean, &lim);
    s.add(
        "A46-oracle字体-干净种子全绿且完成",
        rep_clean.completed && rep_clean.guarded.is_none() && o_clean.all_hold(),
        "干净 4 表字体：跑完、零护栏、三件（零崩溃/内存/超时）+沙箱对账全成立",
    );

    // —— 判据二 · oracle 三件套（字体面）：三算子变异逐个喂，全达终态 ——
    let mut ops_ok = true;
    let mut oi = 0usize;
    while oi < FontMutationOp::ALL.len() {
        let op = match FontMutationOp::ALL.get(oi) {
            Some(v) => *v,
            None => {
                ops_ok = false;
                break;
            }
        };
        let (mutated, used) = mutate_font(&clean, op, &mut Rng::new(0x0814 + oi as u64));
        let m = parse_font_sandboxed(&mutated, &lim);
        let o = font_oracle(&m, &lim);
        if used != op || !o.all_hold() || (m.guarded.is_none() && !m.completed) {
            ops_ok = false;
        }
        oi += 1;
    }
    s.add(
        "A46-oracle字体-三算子变异皆达终态",
        ops_ok,
        "表裁剪/长度篡改/循环链真改字节，解析必达终态且 oracle 三件+对账全绿",
    );

    // —— 判据二 · oracle 三件套（字体面）：护栏分档各就各位 ——
    let rep_wide = parse_font_sandboxed(
        &build_font_seed(&FontSeedSpec::wide_many_tables()),
        &lim,
    );
    let rep_circ = parse_font_sandboxed(
        &build_font_seed(&FontSeedSpec {
            circular_at: Some(0),
            ..FontSeedSpec::clean_small()
        }),
        &lim,
    );
    let rep_tamper = parse_font_sandboxed(
        &build_font_seed(&FontSeedSpec {
            length_tamper_at: Some(1),
            tampered_len: TAMPERED_HUGE_LEN,
            ..FontSeedSpec::clean_small()
        }),
        &lim,
    );
    s.add(
        "A46-oracle字体-超时分档与终止",
        rep_wide.guarded == Some(GuardStop::TimeCap)
            && !rep_wide.completed
            && font_oracle(&rep_wide, &lim).all_hold(),
        "512 表×8KB：内存线内但工作量过 500ms → TimeCap 截停且未跑完",
    );
    s.add(
        "A46-oracle字体-循环链深度截停",
        rep_circ.guarded == Some(GuardStop::DepthCap)
            && !rep_circ.completed
            && font_oracle(&rep_circ, &lim).all_hold(),
        "offset 指回目录起点 → DepthCap 截停且未跑完",
    );
    s.add(
        "A46-oracle字体-虚高长度内存截停",
        rep_tamper.guarded == Some(GuardStop::MemCap)
            && !rep_tamper.completed
            && font_oracle(&rep_tamper, &lim).all_hold(),
        "单表声明虚高 ~251MB → MemCap 拦截且过冲不超最后一笔声明",
    );

    // —— 判据二 · oracle 三件套（编码面）：三注入档位真被激起 ——
    let base_seed = match VALID_SEEDS.get(0) {
        Some(v) => *v,
        None => &[] as &[u8],
    };
    let mut enc_ok = base_seed.len() > 0;
    let mut ei = 0usize;
    while ei < EncInjection::ALL.len() {
        let op = match EncInjection::ALL.get(ei) {
            Some(v) => *v,
            None => {
                enc_ok = false;
                break;
            }
        };
        let (injected, _, _) = inject_encoding(base_seed, op, &mut Rng::new(0x0EED + ei as u64));
        let res = Decoder::new().decode(&injected, &DecodeOptions::new());
        let o = encoding_oracle(&res, Some(op));
        if !o.all_hold() || !o.expected_fired {
            enc_ok = false;
        }
        ei += 1;
    }
    s.add(
        "A46-oracle编码-三注入档位真激起",
        enc_ok,
        "截断多字节→截断档、代理区→越界档、过长编码→过长档，三件 oracle 全绿",
    );

    // —— 判据二 · 反向语料：oracle 真会亮红（期望档未激起 ⇒ expected_fired=false）——
    let ascii_seed = match VALID_SEEDS.get(3) {
        Some(v) => *v,
        None => &[] as &[u8],
    };
    let res_ascii = Decoder::new().decode(ascii_seed, &DecodeOptions::new());
    let o_rev = encoding_oracle(&res_ascii, Some(EncInjection::TruncateMultibyte));
    let illegal_probe_ok = illegal_for_downstream(0xD800)
        && illegal_for_downstream(0x11_0000)
        && illegal_for_downstream(0xFDD0)
        && illegal_for_downstream(0xFFFE)
        && illegal_for_downstream(0xFFFF)
        && !illegal_for_downstream(REPLACEMENT_CHAR)
        && !illegal_for_downstream(0x41);
    s.add(
        "A46-oracle反向-期望档未激起必亮红",
        !o_rev.expected_fired && o_rev.counts_consistent && o_rev.downstream_clean && illegal_probe_ok,
        "干净 ASCII 配截断期望 → expected_fired=false（oracle 非恒绿）；独立合法性谓词边界逐点核对",
    );

    // —— 判据三 · 回归闭环：标准账全跑全过 + promote 入账 + 重名拒绝 ——
    let outcomes = led.run_all(&lim);
    let mut reg_failed = 0usize;
    let mut ri = 0usize;
    while ri < outcomes.len() {
        match outcomes.get(ri) {
            Some(o) => {
                if !o.held {
                    reg_failed += 1;
                }
            }
            None => reg_failed += 1,
        }
        ri += 1;
    }
    let mut led2 = RegressionLedger::standard();
    let promo_ok = led2.promote(RegressionCase {
        name: "自检-最小化样本-护栏静默".to_string(),
        face: FuzzFace::FontFile,
        bytes: clean.clone(),
        expect: RegressionExpect::GuardQuiet,
        from_crash: true,
    }) && !led2.promote(RegressionCase {
        name: "自检-最小化样本-护栏静默".to_string(),
        face: FuzzFace::FontFile,
        bytes: clean.clone(),
        expect: RegressionExpect::GuardQuiet,
        from_crash: true,
    })
        && led2.len() == led.len() + 1;
    s.add(
        "A46-回归闭环-标准账全过且晋升去重",
        led.len() == 6
            && outcomes.len() == led.len()
            && reg_failed == 0
            && promo_ok,
        "标准账 6 例（三算子+干净基线+两编码边界）每例必跑全过；promote 入账、重名拒绝",
    );

    // —— 判据三 · 最小化：样本缩短且判据仍复现（增量式 delta debugging）——
    let mut mini_in: Vec<u8> = Vec::new();
    mini_in.extend_from_slice(b"prefix noise unrelated to the finding, long enough to shrink ");
    mini_in.extend_from_slice(&OVERLONG_PAYLOAD);
    mini_in.extend_from_slice(b" and suffix noise after the marker, also quite long here");
    let mini_base_holds = recheck_has_overlong(&mini_in);
    let mini = minimize_sample(&mini_in, |b| recheck_has_overlong(b));
    s.add(
        "A46-最小化-样本缩短且复现保持",
        mini_base_holds
            && mini.len() < mini_in.len()
            && recheck_has_overlong(&mini),
        "判据基线先证非平凡；最小化后严格更短且失败谓词仍真",
    );

    // —— 判据四 · 运行节奏：预算常量与批次规划（增量缩新语料不缩回归）——
    let pn = plan_batch(Cadence::Nightly, 1000, 5000, 6);
    let pc = plan_batch(Cadence::PreCommit, 1000, 5000, 6);
    s.add(
        "A46-节奏-预算常量与规划分档",
        NIGHTLY_BUDGET_SECS == 1800
            && PRECOMMIT_BUDGET_SECS == 180
            && Cadence::Nightly.budget_secs() == 1800
            && Cadence::PreCommit.budget_secs() == 180
            && pn.covers_full_corpus
            && pn.regression_included
            && pn.total_entries == 1000
            && !pc.covers_full_corpus
            && pc.regression_included
            && pc.total_entries == 100
            && pc.total_entries < pn.total_entries
            && pn.est_secs <= Cadence::Nightly.budget_secs(),
        "30 分钟 nightly 全量 / 3 分钟增量取 1/10；两种节奏回归账都全量纳入",
    );

    // —— 判据五 · 崩溃零容忍：门禁六路逐路核对（含反向污染报告）——
    let mut rep_deg = rep.clone();
    if let Some(f0) = rep_deg.face_entries.get_mut(0) {
        *f0 = 0;
    }
    let mut rep_crit = rep.clone();
    rep_crit.log.record(Finding14 {
        class: CrashClass::MemorySafety,
        reason: "解析器零崩溃",
        key: ReproKey {
            face: FuzzFace::FontFile,
            source: CorpusSource::MutatedSeeds,
            seed: 1,
            index: 0,
        },
        input: Vec::new(),
        oracle_ver: ORACLE_VERSION,
    });
    let mut rep_brk = rep.clone();
    rep_brk.regressions_failed = 1;
    let mut rep_bud = rep.clone();
    rep_bud.plan.est_secs = rep_bud.cadence.budget_secs() + 1;
    let no_input = gate_of(&TextFuzzReport {
        cadence: Cadence::PreCommit,
        face_entries: [0, 0],
        source_entries: [0, 0, 0],
        processed: 0,
        passed: 0,
        log: FindingLog14::new(),
        regressions_run: 0,
        regressions_failed: 0,
        plan: BatchPlan {
            total_entries: 0,
            est_secs: 0,
            covers_full_corpus: false,
            regression_included: true,
        },
    });
    s.add(
        "A46-门禁-六路裁定逐路核对",
        matches!(gate_of(&rep), TextFuzzGate::Allow)
            && matches!(gate_of(&rep_deg), TextFuzzGate::CorpusDegenerate)
            && matches!(gate_of(&rep_crit), TextFuzzGate::CriticalOpen { open: 1 })
            && matches!(gate_of(&rep_brk), TextFuzzGate::RegressionBroken { failed: 1 })
            && matches!(gate_of(&rep_bud), TextFuzzGate::BudgetBroken { .. })
            && matches!(no_input, TextFuzzGate::NoInput)
            && gate_of(&rep_crit).blocked()
            && !gate_of(&rep).blocked(),
        "Allow/退化/未修崩溃级/回归破坏/超预算/零输入 六路各就各位；未修 MemorySafety 必阻断",
    );

    // —— 判据五 · oracle 修订纪律：免责必须版本严格递增 ——
    let mut log_d = FindingLog14::new();
    log_d.record(Finding14 {
        class: CrashClass::OracleFault,
        reason: "沙箱判定对账",
        key: ReproKey {
            face: FuzzFace::FontFile,
            source: CorpusSource::RealFonts,
            seed: 7,
            index: 3,
        },
        input: Vec::new(),
        oracle_ver: ORACLE_VERSION,
    });
    let dismiss_same = log_d.dismiss_by_oracle_fix("沙箱判定对账", ORACLE_VERSION);
    let dismiss_newer = log_d.dismiss_by_oracle_fix("沙箱判定对账", ORACLE_VERSION + 1);
    s.add(
        "A46-oracle免责-版本不递增不摘除",
        dismiss_same == 0 && dismiss_newer == 1 && log_d.is_empty(),
        "同版本免责被拒（想靠改判据放行没有入口）；递增版本（真修过 oracle）才摘除",
    );

    // —— 复现闭环：同配置重跑逐位确定 + 复现键可重建输入 ——
    let rep2 = run_text_fuzz(&cfg, &lim, &led);
    let key = ReproKey {
        face: FuzzFace::EncodingSeq,
        source: CorpusSource::MutatedSeeds,
        seed: cfg.seed,
        index: 1,
    };
    let via_replay = replay(&key, &led);
    s.add(
        "A46-复现-重跑确定且键可重建",
        rep2.processed == rep.processed
            && rep2.passed == rep.passed
            && rep2.face_entries == rep.face_entries
            && rep2.source_entries == rep.source_entries
            && rep2.plan == rep.plan
            && !via_replay.is_empty()
            && replay(&key, &led) == via_replay,
        "同种子两轮结果全等（自持 LCG 跨机可复现）；四元组键 replay 重建非空且确定",
    );

    // —— 判据 stamp 独立对账 + 人话呈现非空 ——
    let mut stamp_ok = CRITERIA_STAMP.len() == CRITERIA_RECHECK.len();
    let mut ci = 0usize;
    while ci < CRITERIA_RECHECK.len() {
        if CRITERIA_STAMP.get(ci) != Some(&CRITERIA_RECHECK[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    let txt = render_report(&rep);
    s.add(
        "A46-判据stamp-独立重算与呈现",
        stamp_ok
            && txt.contains("文字渲染 fuzz")
            && txt.contains(rep.cadence.label())
            && txt.contains("发现账未修"),
        "判据五条与判据侧独立重排逐条全等（常量被误改先红）；呈现含节奏与发现账摘要",
    );

    s
}
