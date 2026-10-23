//! UNX-F1323 · serial 面等价验证账（字节流等价 100%）· 诊断段 0x13D0
//! ═══════════════════════════════════════════════════════════════════════
//! 验收锚点（任务板逐字抄录）：
//!   UNX-F1323-J1 serial 面退役前后字节流等价率 100%（字节序零差异），
//!   等价样本量 ≥10^6 字节，编码完整性（无插入无丢失）断言 10/10 次通过
//!
//! 定位（任务板 UNX-A2-B27 批头原文抄录）：
//!   UNX-A2-B27 · 接管等价验证（（分发与接管面第 12 批·接管三部曲 III））
//!   （F1321–F1340 · 20 条）｜嫁接源：Linux 行为等价回归（regression
//!   equivalence）与差分测试惯例，只跟随｜防重声明：对 B25（基线冻结）
//!   为原料-验证分层——B25 立基线，本批立等价比对与等价率断言；对 B26
//!   （对拍采集）为采集-判定分层——B26 立数据采集，本批立等价判定；
//!   收编纪律：等价率 100% 是现存代码收编的硬判据｜上游依赖：F1287 基线、
//!   F1309 对拍数据、F1310 清理门槛
//!
//! 子系统总览（全手工定制，逐字节真实实现，无注水空壳）：
//!   §1  SerSource        —— 确定性 serial 字节流源（xorshift64* 全整数，
//!                           每轮每侧满样本 10^6 字节，零墙钟零浮点）
//!   §2  TamperSource     —— 篡改流源（插字节/丢字节/换序/截短四类确定性
//!                           篡改；插字节取正本位反码保证首差异恰落注入位；
//!                           丢/换序经 plan_distinct_pair 规划相邻异字节位）
//!   §3  StreamCapture    —— 双侧定长环形捕获（退役侧轮询面/中断侧中断面），
//!                           FNV-1a 滚动指纹每 4KB 一锚点，序号连续性校验
//!                           （断口检出），计数守恒（进=出+在途），封账凭据
//!   §4  EquivVerifier    —— 等价验证器：指纹门（双侧全程 FNV 终指纹相等是
//!                           等价的必要条件，不等快速失败仅走 128KB 定位窗，
//!                           省逐字节全量）→ 逐字节对拍（首差异定位：偏移+
//!                           两侧字节+前后 8 字节摘要）→ 锚点对账（再生流
//!                           4KB 锚点 ↔ 捕获锚点逐对核）→ 再生↔捕获终指纹
//!                           对账（收发一致实证）→ 等价率 ppm（u128 无损），
//!                           ppm < 10^6 即不等价
//!   §5  RoundLedger      —— 十轮独立断言账（每轮独立种子流、轮轮留账、
//!                           一轮失败全局失败）+ 同种子双跑确定性对拍 +
//!                           编码完整性断言（无插入无丢失）逐轮落账 10/10
//!   §6  注入面           —— 插字节/丢字节/换序/截短/断口/少记六类注入
//!                           全检出演练（漏检即红）
//!   §7  EventLog/health  —— 事件账（有界、丢弃显性计数）+ 健康面总成
//!
//! 纪律（逐条落实）：
//!   - 完全自包含：仅 use core::… / use alloc::{…}；禁止 use crate::…；
//!   - no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!
//!     （仅 #[cfg(test)] 测试体例外；编译期 const 断言见 §0 末）；
//!   - 可失败全走 Result<T, SerEquivError>，错误每变体带 detail() 中文
//!     可操作诊断 + 段内 code()；
//!   - 溢出显性：计数器一律 saturating_add/wrapping 运算；ppm 用 u128；
//!   - >64KB 结构体禁上栈：环形缓冲/刮擦缓冲全部堆上 Vec；捕获环参数
//!     上限 65536 由编译期断言钉死；
//!   - const fn 禁坑：本模块 const fn 只用整型运算 + while+索引，无
//!     slice::get / Vec 索引 / Ord::min-max / derive PartialEq 的 ==；
//!   - 诊断码段 0x13D0~0x13DF 独占（2026-10-24 全库 grep 核验空闲）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码段（0x13D0~0x13DF，闭集独占）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。来源：任务书 UNX-F1323 分配段 0x13D0；2026-10-24 对
/// unxstar/ 与 cgpu/ 全量 grep "0x13D" 核验：唯一命中为 fbmode 模式名
/// 字符串 "MODE13D"（非诊断码），段空闲。
pub const SEG_BASE: u16 = 0x13D0;

/// 诊断段段顶（含）。本模块全部诊断码 ∈ [SEG_BASE, SEG_TOP]。
pub const SEG_TOP: u16 = 0x13DF;

/// 0x13D0 捕获环诊断码（环参数非法/环操作协议违例）。
pub const CODE_RING: u16 = SEG_BASE;
/// 0x13D1 序号断口检出码。
pub const CODE_SEQ_GAP: u16 = SEG_BASE + 0x1;
/// 0x13D2 计数守恒/生产申报失配码。
pub const CODE_CONSERV: u16 = SEG_BASE + 0x2;
/// 0x13D3 封账凭据码。
pub const CODE_SEAL: u16 = SEG_BASE + 0x3;
/// 0x13D4 样本不足（Inconclusive）码。
pub const CODE_SAMPLE: u16 = SEG_BASE + 0x4;
/// 0x13D5 指纹门（快速失败门）码。
pub const CODE_GATE: u16 = SEG_BASE + 0x5;
/// 0x13D6 首差异定位码。
pub const CODE_DIFF: u16 = SEG_BASE + 0x6;
/// 0x13D7 等价率 ppm 码。
pub const CODE_PPM: u16 = SEG_BASE + 0x7;
/// 0x13D8 注入面演练码。
pub const CODE_INJECT: u16 = SEG_BASE + 0x8;
/// 0x13D9 轮账码。
pub const CODE_ROUND: u16 = SEG_BASE + 0x9;
/// 0x13DA 双跑确定性对拍码。
pub const CODE_DETERMINISM: u16 = SEG_BASE + 0xA;
/// 0x13DB 健康面码。
pub const CODE_HEALTH: u16 = SEG_BASE + 0xB;
/// 0x13DC 容量/参数码。
pub const CODE_CAPACITY: u16 = SEG_BASE + 0xC;
/// 0x13DD 轮账容量码。
pub const CODE_LEDGER: u16 = SEG_BASE + 0xD;
/// 0x13DE 锚点对账/再生对账码。
pub const CODE_ANCHOR: u16 = SEG_BASE + 0xE;
/// 0x13DF 段尾保留（本模块不使用，防段外漂移哨位）。
pub const CODE_RESERVE: u16 = SEG_BASE + 0xF;

/// 等价样本量门槛（字节）。来源：UNX-F1323-J1 "等价样本量 ≥10^6 字节"。
pub const MIN_SAMPLE_BYTES: u64 = 1_000_000;

/// 每轮每侧满样本字节数。与门槛同源（J1），保证清洁轮必达门槛。
pub const STREAM_LEN_BYTES: u64 = 1_000_000;

/// 指纹锚点跨度（字节）。来源：任务深度方向 "FNV 滚动指纹每 4KB 一锚点"。
pub const ANCHOR_SPAN_BYTES: u64 = 4_096;

/// 双侧捕获环容量（字节）。定长环形，堆上 Vec 承载；10^6 满样本下环体
/// 回绕 ≥61 次，充分锻炼回绕路径。
pub const RING_CAP_BYTES: usize = 16_384;

/// 捕获环参数上限。来源：">64KB 结构体禁上栈" 纪律的环参数哨位
/// （环本体堆分配，上限防误配为巨栈值）。
pub const MAX_RING_CAP: usize = 65_536;

/// 单侧锚点账上限（条）。10^6/4KB = 245 条/侧，512 留 2 倍裕量；
/// 超限丢弃显性计数，绝不静默。
pub const MAX_ANCHORS: usize = 512;

/// 指纹门失败后的定位窗（字节）。来源：深度方向 "指纹不等快速失败省逐
/// 字节"——门破后仅走 128KB 定位窗找首差异，不做全量逐字节。
/// 覆盖注入位 50_000/60_000/70_000 全部。
pub const WALK_BOUND: u64 = 131_072;

/// 逐字节对拍的再生窗（字节）。按窗推进再生流，前后文摘要自窗内取。
pub const WALK_WINDOW: u64 = 256;

/// 泵送刮擦缓冲容量（字节）。中断侧调度上限 2048 ≤ 4096，编译期钉死。
pub const SCRATCH_CAP: usize = 4_096;

/// 断言轮数。来源：J1 "编码完整性断言 10/10 次通过"。
pub const ROUNDS: usize = 10;

/// 轮账容量（条）。10 清洁轮 + 6 注入演练 ≤ 64，余量留复查轮。
pub const LEDGER_CAP: usize = 64;

/// 事件账容量（条）。有界账，满后丢弃显性计数。
pub const MAX_EVENTS: usize = 256;

/// 首差异前后文摘要跨度（字节/侧）。
pub const CONTEXT_SPAN: usize = 8;

/// 退役侧（轮询面）定长块（字节）。取质数 507，刻意错开 4KB 锚点界，
/// 锻炼锚点跨越块边界的折叠路径。
pub const LEGACY_CHUNK: usize = 507;

/// 中断面调度盐。与轮种子异或派生变长块调度流（确定性，零墙钟）。
pub const SCHED_SALT: u64 = 0x5EED_F00D_1323_13D0;

/// 轮种子扩列乘数。来源：黄金比例常数 0x9E3779B97F4A7C15（Knuth 整数
/// 哈希惯例），用于十轮独立种子流的扩列。
pub const ROUND_SALT: u64 = 0x9E37_79B9_7F4A_7C15;

/// FNV-1a 64 位偏移基。来源：Landon Curt Noll 公布的 FNV-1a 参数
/// （offset basis 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位质数。来源：同上（prime 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// xorshift64* 乘数。来源：Vigna 2014《An experimental exploration of
/// Marsaglia's xorshift generators, scrambled》64* 乘数。
pub const XS_MULT: u64 = 0x2545_F491_4F6C_DD1D;

/// 注入演练位（插字节）。< WALK_BOUND，保证指纹门破后定位窗必达。
pub const INSERT_POS: u64 = 50_000;
/// 注入演练位（丢字节规划偏好位）。
pub const DROP_POS: u64 = 60_000;
/// 注入演练位（换序规划偏好位）。
pub const SWAP_POS: u64 = 70_000;
/// 截短注入的截短总量（字节）——远小于 10^6，专门检验样本门槛拒出结论。
pub const SHORT_TOTAL: u64 = 4_096;

/// FNV 编译期自检锚：对模块名常量串折叠，必须非零。
/// 由下方 const 断言钉死（编译期验证 FNV const 路径可用）。
pub const FNV_SELFTEST: u64 = fnv1a_bytes_const(FNV_OFFSET, b"varix-unx1323-serequiv");

// ── 编译期断言链（唯一允许的 assert!：const 上下文，编译期生效）────────
const _: () = assert!(SEG_BASE == 0x13D0, "诊断段基址漂移");
const _: () = assert!(SEG_TOP == 0x13DF, "诊断段段顶漂移");
const _: () = assert!(CODE_RING >= SEG_BASE && CODE_RESERVE <= SEG_TOP, "诊断码越段");
const _: () = assert!(MIN_SAMPLE_BYTES == 1_000_000, "样本门槛须为 10^6（J1）");
const _: () = assert!(STREAM_LEN_BYTES >= MIN_SAMPLE_BYTES, "每轮样本须达门槛");
const _: () = assert!(ANCHOR_SPAN_BYTES == 4_096, "锚点跨度须为 4KB");
const _: () = assert!(is_pow2(ANCHOR_SPAN_BYTES), "锚点跨度须为 2 的幂");
const _: () = assert!(RING_CAP_BYTES >= 1 && RING_CAP_BYTES <= MAX_RING_CAP, "环容量越界");
const _: () = assert!(MAX_RING_CAP <= 65_536, "环参数上限越 64KB 哨位");
const _: () = assert!(SCRATCH_CAP >= 2_048, "刮擦缓冲须容纳中断侧最大块 2048");
const _: () = assert!(ROUNDS == 10, "断言轮数须为 10（J1）");
const _: () = assert!(WALK_BOUND >= 131_072, "定位窗须覆盖注入演练位 70_000");
const _: () = assert!(WALK_BOUND > SWAP_POS, "定位窗未覆盖换序注入位");
const _: () = assert!(FNV_SELFTEST != 0, "FNV const 自检锚异常");
const _: () = assert!(LEDGER_CAP >= ROUNDS + 8, "轮账容量不足（10 清洁轮 + 注入演练）");
const _: () = assert!(MAX_ANCHORS >= 512, "锚点账上限不足（10^6/4KB=245×2 裕量）");

/// 编译期：2 的幂判定（const fn；while+位运算，无禁坑路径）。
pub const fn is_pow2(n: u64) -> bool {
    n != 0 && (n & n.wrapping_sub(1)) == 0
}

/// 编译期：FNV-1a 对字节串折叠（const fn；while+索引，禁坑路径全避开）。
pub const fn fnv1a_bytes_const(mut fp: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        fp = (fp ^ (bytes[i] as u64)).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    fp
}

/// 运行期/编译期两用：单字节 FNV-1a 步进。
pub const fn fnv1a_step(fp: u64, b: u8) -> u64 {
    (fp ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ═══════════════════════════════════════════════════════════════════════
// §1 错误闭集 SerEquivError（每变体：段内 code() + 中文可操作 detail()）
// ═══════════════════════════════════════════════════════════════════════

/// 本模块错误闭集。可失败路径全走 Result<T, SerEquivError>。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SerEquivError {
    /// 容量/参数非法（环容量为 0 或超 64KB 哨位）。
    AllocParam,
    /// 捕获已封账后再写入/消费。
    AlreadySealed,
    /// 未封账即索取封账后凭据。
    NotSealed,
    /// 字节序号断口（append 的 seq 与期望序号不连续）。
    SeqGapDetected,
    /// 源字节取尽仍被请求（泵送越界）。
    Underrun,
    /// 编码完整性破（进≠出+在途 / 生产申报与实收失配 / 折叠未覆盖全量）。
    IntegrityBreach,
    /// 判定越权（样本不足却索取等价结论）。
    DomainViolation,
    /// 字节流不等价（以不等价账目索取等价结论）。
    NotEquivalent,
    /// 轮账已满。
    LedgerFull,
}

impl SerEquivError {
    /// 段内诊断码（∈ [0x13D0, 0x13DF]）。
    pub fn code(&self) -> u16 {
        match self {
            SerEquivError::AllocParam => CODE_CAPACITY,
            SerEquivError::AlreadySealed => CODE_RING,
            SerEquivError::NotSealed => CODE_SEAL,
            SerEquivError::SeqGapDetected => CODE_SEQ_GAP,
            SerEquivError::Underrun => CODE_RING,
            SerEquivError::IntegrityBreach => CODE_CONSERV,
            SerEquivError::DomainViolation => CODE_SAMPLE,
            SerEquivError::NotEquivalent => CODE_PPM,
            SerEquivError::LedgerFull => CODE_LEDGER,
        }
    }

    /// 中文可操作诊断（逐变体，直达修复动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            SerEquivError::AllocParam => {
                "环容量参数非法：必须 ≥1 且 ≤65536（>64KB 结构体禁上栈纪律的哨位）。\
                 请直接使用本模块 RING_CAP_BYTES 建捕获，勿自配巨值"
            }
            SerEquivError::AlreadySealed => {
                "捕获已封账：封账后环与计数全部冻结，禁止再 append/consume。\
                 如需续采，请新建 StreamCapture 另起一段捕获"
            }
            SerEquivError::NotSealed => {
                "捕获尚未封账：请先对两侧各调 seal() 取封账凭据，\
                 再读取终指纹/锚点/总量等封账后账目"
            }
            SerEquivError::SeqGapDetected => {
                "字节序号断口：append 携带的 seq 与捕获侧期望序号不连续，\
                 流中检出插入/丢失前兆。请核对生产侧序号生成与分块推进，\
                 严禁以补零/跳号方式吞断"
            }
            SerEquivError::Underrun => {
                "源字节已取尽仍被请求：泵送循环推进越界。\
                 请检查 remaining()==0 判定后再拉块，勿在取尽后继续取"
            }
            SerEquivError::IntegrityBreach => {
                "编码完整性破：进≠出+在途，或生产申报 produced_reported 与实收 \
                 bytes_in 失配，或指纹折叠未覆盖全量。请对账三计数器 \
                 （bytes_in/bytes_out/produced_reported）与在途 in_flight()，\
                 先排空在途再封账"
            }
            SerEquivError::DomainViolation => {
                "判定越权：等价样本量不足 10^6 字节，判定器只能出 Inconclusive，\
                 禁止出等价结论。请扩样至门槛后复验，勿以小样本虚报等价"
            }
            SerEquivError::NotEquivalent => {
                "字节流不等价：请读取首差异定位（偏移+两侧字节+前后文摘要）\
                 与等价率 ppm 账目回溯，不得以任何旁路口径宣称等价"
            }
            SerEquivError::LedgerFull => {
                "轮账已满（64 条）：请先收官汇总（take_records/health）后另起账本，\
                 轮账 append-only 严禁覆盖旧账"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 判定与原因闭集
// ═══════════════════════════════════════════════════════════════════════

/// 等价判定三值闭集：等价（100%）/不等价/样本不足（拒出结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 字节流等价 100%（字节序零差异：全量逐字节相等 + 双侧终指纹相等 +
    /// 锚点对账全对 + 完整性双侧成立）。
    Equivalent,
    /// 字节流不等价（cause() 给出首因）。
    Divergent,
    /// 样本不足 10^6，拒出结论（不虚报等价）。
    Inconclusive,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Equivalent => "等价(100%)",
            Verdict::Divergent => "不等价",
            Verdict::Inconclusive => "样本不足·拒出结论",
        }
    }

    /// 段内诊断码。
    pub fn code(&self) -> u16 {
        match self {
            Verdict::Equivalent => CODE_PPM,
            Verdict::Divergent => CODE_DIFF,
            Verdict::Inconclusive => CODE_SAMPLE,
        }
    }
}

/// 不等价首因闭集（定位优先级：样本→长度→首差→指纹门→锚点→再生对账→
/// 断口→完整性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DivergenceCause {
    /// 无（等价或未判定）。
    None,
    /// 样本不足门槛。
    SampleShort,
    /// 双侧流长失配（插入/丢失的长度面证据）。
    LenMismatch,
    /// 逐字节首差异（偏移+两侧字节见 DiffRecord）。
    FirstDiff,
    /// 双侧全程 FNV 终指纹不等（必要条件即破）。
    GateFail,
    /// 锚点对账失配（再生流 4KB 锚点与捕获锚点逐对核不符）。
    AnchorMismatch,
    /// 再生流终指纹与捕获终指纹失配（收发一致破）。
    CaptureMismatch,
    /// 序号断口（编码完整性：断口未零）。
    SeqGaps,
    /// 计数守恒/生产申报失配（编码完整性：无插入无丢失破）。
    IntegrityBreach,
}

impl DivergenceCause {
    pub fn as_str(&self) -> &'static str {
        match self {
            DivergenceCause::None => "无",
            DivergenceCause::SampleShort => "样本不足",
            DivergenceCause::LenMismatch => "双侧流长失配",
            DivergenceCause::FirstDiff => "逐字节首差异",
            DivergenceCause::GateFail => "全程指纹门破",
            DivergenceCause::AnchorMismatch => "锚点对账失配",
            DivergenceCause::CaptureMismatch => "再生↔捕获指纹失配",
            DivergenceCause::SeqGaps => "序号断口",
            DivergenceCause::IntegrityBreach => "计数守恒/申报失配",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 确定性 serial 字节流源 SerSource
// ═══════════════════════════════════════════════════════════════════════

/// 确定性 serial 字节流源。
///
/// 以 xorshift64*（Vigna 64* 置换）整型发生器产出确定性字节流：每 8 字节
/// 消耗一个 64 位字，按小端逐 lane 取字节——两侧（退役/中断）同种子即产出
/// 逐字节同流，构成 "退役前后字节流等价" 的被验对象。全整数、零墙钟、
/// 零浮点：同种子两次实例化逐字节一致（双跑确定性的根基）。
#[derive(Clone)]
pub struct SerSource {
    state: u64,
    word: u64,
    produced: u64,
    total: u64,
}

impl SerSource {
    /// 以种子建源，产出恰好 `total` 字节。
    pub fn new(seed: u64, total: u64) -> Self {
        SerSource {
            state: seed ^ FNV_OFFSET, // 种子置换：避开全零不动点
            word: 0,
            produced: 0,
            total,
        }
    }

    /// xorshift64* 步进（Vigna 置换：x^=x>>12; x^=x<<25; x^=x>>27; ×乘数）。
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(XS_MULT)
    }

    /// 取下一字节；流取尽返回 None（显性，绝不回绕重放）。
    pub fn next_byte(&mut self) -> Option<u8> {
        if self.produced >= self.total {
            return None;
        }
        if (self.produced & 0x7) == 0 {
            self.word = self.next_u64();
        }
        let lane = (self.produced & 0x7) as u32;
        let b = (self.word >> (lane << 3)) as u8;
        self.produced = self.produced.saturating_add(1);
        Some(b)
    }

    /// 预览下一字节但不消耗（用于插字节注入取正本位反码）。
    pub fn peek_byte(&self) -> Option<u8> {
        let mut probe = self.clone();
        probe.next_byte()
    }

    /// 已产出字节数。
    pub fn produced(&self) -> u64 {
        self.produced
    }

    /// 剩余字节数。
    pub fn remaining(&self) -> u64 {
        self.total.saturating_sub(self.produced)
    }

    /// 流总量。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 拉一块至多 buf.len() 字节填入 buf，返回实填数（≤ buf.len()）。
    pub fn next_chunk(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0usize;
        while n < buf.len() {
            match self.next_byte() {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }

    /// 随机访问第 index 字节（从种子重放；规划用，O(index)）。
    pub fn byte_at(seed: u64, total: u64, index: u64) -> Option<u8> {
        if index >= total {
            return None;
        }
        let mut s = SerSource::new(seed, total);
        let mut i = 0u64;
        while i < index {
            s.next_byte()?;
            i += 1;
        }
        s.next_byte()
    }
}

/// 规划 "相邻异字节" 位置：从 prefer 起找首个 i 使流内 b[i]≠b[i+1]。
///
/// 用于丢字节/换序注入的触发位规划——保证注入后首差异恰好落在规划位
/// （若 b[i]==b[i+1]，丢弃/换序在字节面上不可见，首差异会漂移）。
/// 单趟顺序重放 O(total)；找不到时（概率 ~256^-total+prefer）退回 prefer，
/// 检测退路由指纹门与长度账兜底。
pub fn plan_distinct_pair(seed: u64, prefer: u64, total: u64) -> u64 {
    let mut s = SerSource::new(seed, total);
    let mut i = 0u64;
    let mut prev: u8 = 0;
    let mut have = false;
    while i < total {
        let b = match s.next_byte() {
            Some(b) => b,
            None => break,
        };
        if have && i > prefer && b != prev {
            return i - 1;
        }
        prev = b;
        have = true;
        i += 1;
    }
    prefer
}

// ═══════════════════════════════════════════════════════════════════════
// §4 篡改流源 TamperSource 与注入闭集 InjectKind
// ═══════════════════════════════════════════════════════════════════════

/// 注入闭集（六类）。前四类作用于流字节面；断口/少记作用于捕获账面。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InjectKind {
    /// 在 at 位插一枚外来字节（注入值 = 正本该位的按位反码，保证首差异
    /// 恰落注入位；流长 +1）。
    InsertByte { at: u64 },
    /// 丢弃 at 位正本字节（流长 -1；触发位经 plan_distinct_pair 规划）。
    DropByte { at: u64 },
    /// 交换 at/at+1 两字节（流长不变；字节序零差异判据的序敏感实证）。
    SwapAdjacent { at: u64 },
    /// 截短流至 total 字节（检验样本门槛拒出结论）。
    ShortStream { total: u64 },
    /// 以断序号 append（作用于捕获账面，必吃 SeqGapDetected）。
    SeqGap { at: u64 },
    /// 生产侧少记 by 字节（申报≠实收，守恒面注入）。
    Undercount { by: u64 },
}

impl InjectKind {
    /// 注入名签（轮账留痕用）。
    pub fn tag(&self) -> &'static str {
        match self {
            InjectKind::InsertByte { .. } => "插字节",
            InjectKind::DropByte { .. } => "丢字节",
            InjectKind::SwapAdjacent { .. } => "换序",
            InjectKind::ShortStream { .. } => "截短",
            InjectKind::SeqGap { .. } => "断口",
            InjectKind::Undercount { .. } => "少记",
        }
    }

    /// 注入位/量（留痕用）。
    pub fn magnitude(&self) -> u64 {
        match self {
            InjectKind::InsertByte { at } => *at,
            InjectKind::DropByte { at } => *at,
            InjectKind::SwapAdjacent { at } => *at,
            InjectKind::ShortStream { total } => *total,
            InjectKind::SeqGap { at } => *at,
            InjectKind::Undercount { by } => *by,
        }
    }

    /// 段内诊断码（按注入作用面归码）。
    pub fn code(&self) -> u16 {
        match self {
            InjectKind::InsertByte { .. } => CODE_DIFF,
            InjectKind::DropByte { .. } => CODE_DIFF,
            InjectKind::SwapAdjacent { .. } => CODE_DIFF,
            InjectKind::ShortStream { .. } => CODE_SAMPLE,
            InjectKind::SeqGap { .. } => CODE_SEQ_GAP,
            InjectKind::Undercount { .. } => CODE_CONSERV,
        }
    }
}

/// 篡改流源：包装正本 SerSource，按 InjectKind 惰性产出篡改流。
///
/// 全部篡改确定性：同种子 + 同注入 ⇒ 逐字节同篡改流。字节面注入恰好
/// 触发一次（p == pos 守卫），触发后正本继续正常流出。
#[derive(Clone)]
pub struct TamperSource {
    canon: SerSource,
    kind: Option<InjectKind>,
    pos: u64,
    swap_hold: Option<u8>,
    emitted: u64,
    total_out: u64,
}

impl TamperSource {
    /// 建篡改源。丢字节/换序自动规划相邻异字节触发位（见 plan_distinct_pair）。
    pub fn new(seed: u64, total: u64, kind: Option<InjectKind>) -> Self {
        let pos = match kind {
            Some(InjectKind::DropByte { at }) => plan_distinct_pair(seed, at, total),
            Some(InjectKind::SwapAdjacent { at }) => plan_distinct_pair(seed, at, total),
            Some(InjectKind::InsertByte { at }) => at,
            _ => 0,
        };
        let total_out = match kind {
            Some(InjectKind::InsertByte { .. }) => total.saturating_add(1),
            Some(InjectKind::DropByte { .. }) => total.saturating_sub(1),
            Some(InjectKind::ShortStream { total: t }) => {
                if total < t {
                    total
                } else {
                    t
                }
            }
            _ => total,
        };
        TamperSource {
            canon: SerSource::new(seed, total),
            kind,
            pos,
            swap_hold: None,
            emitted: 0,
            total_out,
        }
    }

    /// 实际触发位（丢/换序可能已自 prefer 前移）。
    pub fn planned_pos(&self) -> u64 {
        self.pos
    }

    /// 已产出（篡改流坐标）。
    pub fn produced(&self) -> u64 {
        self.emitted
    }

    /// 剩余。
    pub fn remaining(&self) -> u64 {
        self.total_out.saturating_sub(self.emitted)
    }

    /// 篡改流总量。
    pub fn total(&self) -> u64 {
        self.total_out
    }

    /// 取下一篡改字节；取尽返回 None。
    pub fn next_byte(&mut self) -> Option<u8> {
        if self.emitted >= self.total_out {
            return None;
        }
        let p = self.emitted;
        let out: u8 = match self.kind {
            None => self.canon.next_byte()?,
            Some(InjectKind::InsertByte { .. }) if p == self.pos => {
                // 注入值 = 正本当前位的按位反码：保证与正本该位不同，
                // 首差异恰好落在注入位（定位面确定性）。
                let orig = self.canon.peek_byte()?;
                !orig
            }
            Some(InjectKind::DropByte { .. }) if p == self.pos => {
                // 丢弃正本该字节：先吞掉，再流出正本下一字节。
                let _ = self.canon.next_byte()?;
                self.canon.next_byte()?
            }
            Some(InjectKind::SwapAdjacent { .. }) if p == self.pos => {
                // 换序：p 位流出正本第 pos+1 字节，正本第 pos 字节押后一位。
                let b0 = self.canon.next_byte()?;
                let b1 = self.canon.next_byte()?;
                self.swap_hold = Some(b0);
                b1
            }
            Some(InjectKind::SwapAdjacent { .. }) if p == self.pos.wrapping_add(1) => {
                match self.swap_hold.take() {
                    Some(b) => b,
                    None => self.canon.next_byte()?,
                }
            }
            Some(_) => self.canon.next_byte()?,
        };
        self.emitted = self.emitted.saturating_add(1);
        Some(out)
    }

    /// 拉一块至多 buf.len() 字节，返回实填数。
    pub fn next_chunk(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0usize;
        while n < buf.len() {
            match self.next_byte() {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }
}

/// 双侧源统一面：退役侧 = 正本直通；中断侧 = 篡改源（None 即恒等直通）。
pub enum SideSource {
    /// 退役侧（轮询面）：正本流。
    Legacy(SerSource),
    /// 中断侧（中断面）：篡改流（None 即恒等）。
    Interrupted(TamperSource),
}

impl SideSource {
    /// 建退役侧源（正本直通）。
    pub fn new_legacy(seed: u64, total: u64) -> Self {
        SideSource::Legacy(SerSource::new(seed, total))
    }

    /// 建中断侧源（tamper = None 即恒等直通正本）。
    pub fn new_interrupted(seed: u64, total: u64, tamper: Option<InjectKind>) -> Self {
        match tamper {
            None => SideSource::Interrupted(TamperSource::new(seed, total, None)),
            Some(kind) => SideSource::Interrupted(TamperSource::new(seed, total, Some(kind))),
        }
    }

    /// 已产出。
    pub fn produced(&self) -> u64 {
        match self {
            SideSource::Legacy(s) => s.produced(),
            SideSource::Interrupted(s) => s.produced(),
        }
    }

    /// 剩余。
    pub fn remaining(&self) -> u64 {
        match self {
            SideSource::Legacy(s) => s.remaining(),
            SideSource::Interrupted(s) => s.remaining(),
        }
    }

    /// 拉一块至多 buf.len()，返回实填数。
    pub fn next_chunk(&mut self, buf: &mut [u8]) -> usize {
        match self {
            SideSource::Legacy(s) => s.next_chunk(buf),
            SideSource::Interrupted(s) => s.next_chunk(buf),
        }
    }
}

/// 中断面变长块调度（确定性）：128..=2048 字节，由调度流驱动。
/// 与字节流发生器分离（独立盐），保证块边界与字节图案无耦合。
fn sched_next(state: &mut u64) -> usize {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    let w = x.wrapping_mul(XS_MULT);
    let span = 2048usize - 128usize + 1usize; // 1921 档
    128usize + (w % (span as u64)) as usize
}

// ═══════════════════════════════════════════════════════════════════════
// §5 StreamCapture 双侧定长环形捕获
// ═══════════════════════════════════════════════════════════════════════

/// 侧别（退役面/中断面）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SideKind {
    /// 退役侧（轮询面）。
    LegacyPoll,
    /// 中断侧（中断面）。
    InterruptDriven,
}

impl SideKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SideKind::LegacyPoll => "退役侧(轮询面)",
            SideKind::InterruptDriven => "中断侧(中断面)",
        }
    }
}

/// 4KB 滚动指纹锚点（流内偏移 + 该位折叠态）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AnchorRec {
    /// 锚点流内偏移（= ANCHOR_SPAN_BYTES 的整数倍）。
    pub offset: u64,
    /// 折叠至此偏移的 FNV-1a 状态。
    pub fp: u64,
}

/// 封账凭据（seal() 返回；封账后捕获账面全部冻结）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SealStamp {
    /// 侧别。
    pub side: SideKind,
    /// 实收总量（字节）。
    pub total: u64,
    /// 全程 FNV 终指纹。
    pub final_fp: u64,
    /// 落账锚点数。
    pub anchors: u32,
    /// 超上限丢弃的锚点数（显性，绝不静默）。
    pub anchors_dropped: u32,
    /// 断口事件数。
    pub gap_events: u32,
}

/// 双侧字节流捕获（单侧一个实例）。
///
/// 定长环形缓冲（堆上 Vec）只留最近 RING_CAP 字节（全量等价比对不在环上
/// 做——环面职责是：序号连续性校验（断口检出）+ 计数守恒（进=出+在途）+
/// FNV 滚动指纹全程折叠（每 4KB 落一锚点）+ 封账凭据。全量字节面由
/// EquivVerifier 的再生流逐字节对拍承担，锚点/终指纹双面对账闭环。
pub struct StreamCapture {
    side: SideKind,
    ring: Vec<u8>,
    ring_pos: usize,
    ring_filled: u64,
    next_seq: u64,
    gap_events: u32,
    last_gap_expected: u64,
    last_gap_got: u64,
    fp: u64,
    fp_folded: u64,
    anchors: Vec<AnchorRec>,
    anchors_dropped: u32,
    bytes_in: u64,
    bytes_out: u64,
    produced_reported: u64,
    sealed: bool,
}

impl StreamCapture {
    /// 建捕获。ring_cap ∈ [1, 65536]，环体堆上分配。
    pub fn new(side: SideKind, ring_cap: usize) -> Result<Self, SerEquivError> {
        if ring_cap == 0 || ring_cap > MAX_RING_CAP {
            return Err(SerEquivError::AllocParam);
        }
        Ok(StreamCapture {
            side,
            ring: Vec::with_capacity(ring_cap),
            ring_pos: 0,
            ring_filled: 0,
            next_seq: 0,
            gap_events: 0,
            last_gap_expected: 0,
            last_gap_got: 0,
            fp: FNV_OFFSET,
            fp_folded: 0,
            anchors: Vec::new(),
            anchors_dropped: 0,
            bytes_in: 0,
            bytes_out: 0,
            produced_reported: 0,
            sealed: false,
        })
    }

    /// 环容量补齐（Vec::with_capacity 只备量不置长，逐字节写需要已置长槽位）。
    pub fn prime(&mut self) -> Result<(), SerEquivError> {
        if self.sealed {
            return Err(SerEquivError::AlreadySealed);
        }
        let cap = self.ring.capacity();
        let mut i = self.ring.len();
        while i < cap {
            self.ring.push(0);
            i += 1;
        }
        Ok(())
    }

    /// 侧别。
    pub fn side(&self) -> SideKind {
        self.side
    }

    /// 期望的下一字节序号（流内偏移）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// 断口事件数。
    pub fn gap_events(&self) -> u32 {
        self.gap_events
    }

    /// 最近一次断口（期望序号, 实到序号）。
    pub fn last_gap(&self) -> (u64, u64) {
        (self.last_gap_expected, self.last_gap_got)
    }

    /// 当前滚动指纹（折叠中态；封账后即终指纹）。
    pub fn fp(&self) -> u64 {
        self.fp
    }

    /// 已折叠字节数（应恒等于 bytes_in——折叠未覆盖全量即完整性破）。
    pub fn fp_folded(&self) -> u64 {
        self.fp_folded
    }

    /// 锚点账（封账后即终账）。
    pub fn anchors(&self) -> &[AnchorRec] {
        &self.anchors
    }

    /// 超上限丢弃的锚点数。
    pub fn anchors_dropped(&self) -> u32 {
        self.anchors_dropped
    }

    /// 实收（进）。
    pub fn bytes_in(&self) -> u64 {
        self.bytes_in
    }

    /// 已消费（出）。
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out
    }

    /// 在途 = 进 − 出（封账前必须排空至 0）。
    pub fn in_flight(&self) -> u64 {
        self.bytes_in.saturating_sub(self.bytes_out)
    }

    /// 生产申报量（生产侧自报产出；守恒对账：申报 == 实收）。
    pub fn produced_reported(&self) -> u64 {
        self.produced_reported
    }

    /// 是否已封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 生产侧申报产出（泵送层每块后同步；申报与实收失配即完整性破）。
    pub fn report_produced(&mut self, n: u64) {
        self.produced_reported = n;
    }

    /// 守恒面注入：把生产申报人为减 by（少记注入面；检出靠 integrity_ok）。
    pub fn inject_undercount(&mut self, by: u64) {
        self.produced_reported = self.produced_reported.saturating_sub(by);
    }

    /// 追加一段带序号的字节流。
    ///
    /// - seq 必须 == 期望序号（流内偏移连续），否则记断口事件并显性失败
    ///   （断口检出，绝不静默吞断）；
    /// - 逐字节折叠 FNV 滚动指纹；每满 4KB 落一锚点（超上限丢弃显性计数）；
    /// - 环面回绕写（只留最近 RING_CAP 字节）。
    pub fn append(&mut self, seq: u64, data: &[u8]) -> Result<u64, SerEquivError> {
        if self.sealed {
            return Err(SerEquivError::AlreadySealed);
        }
        if seq != self.next_seq {
            self.gap_events = self.gap_events.saturating_add(1);
            self.last_gap_expected = self.next_seq;
            self.last_gap_got = seq;
            return Err(SerEquivError::SeqGapDetected);
        }
        let ring_len = self.ring.len();
        let mut i = 0usize;
        while i < data.len() {
            let b = data[i];
            // 指纹折叠
            self.fp = fnv1a_step(self.fp, b);
            self.fp_folded = self.fp_folded.saturating_add(1);
            // 每 4KB 一锚点
            if (self.fp_folded % ANCHOR_SPAN_BYTES) == 0 {
                if self.anchors.len() < MAX_ANCHORS {
                    self.anchors.push(AnchorRec {
                        offset: self.fp_folded,
                        fp: self.fp,
                    });
                } else {
                    self.anchors_dropped = self.anchors_dropped.saturating_add(1);
                }
            }
            // 环面回绕写
            if self.ring_pos < ring_len {
                self.ring[self.ring_pos] = b;
            }
            self.ring_pos += 1;
            if self.ring_pos >= ring_len {
                self.ring_pos = 0;
            }
            self.ring_filled = self.ring_filled.saturating_add(1);
            i += 1;
        }
        self.bytes_in = self.bytes_in.saturating_add(data.len() as u64);
        self.next_seq = self.next_seq.saturating_add(data.len() as u64);
        Ok(data.len() as u64)
    }

    /// 消费（对账出账）至多 want 字节；返回实出数。
    pub fn consume(&mut self, want: u64) -> Result<u64, SerEquivError> {
        if self.sealed {
            return Err(SerEquivError::AlreadySealed);
        }
        let avail = self.in_flight();
        let n = if want < avail { want } else { avail };
        self.bytes_out = self.bytes_out.saturating_add(n);
        Ok(n)
    }

    /// 封账：在途必须排空、折叠必须覆盖全量，否则显性完整性破。
    pub fn seal(&mut self) -> Result<SealStamp, SerEquivError> {
        if self.sealed {
            return Err(SerEquivError::AlreadySealed);
        }
        if self.in_flight() != 0 {
            return Err(SerEquivError::IntegrityBreach);
        }
        if self.fp_folded != self.bytes_in {
            return Err(SerEquivError::IntegrityBreach);
        }
        self.sealed = true;
        Ok(SealStamp {
            side: self.side,
            total: self.bytes_in,
            final_fp: self.fp,
            anchors: self.anchors.len() as u32,
            anchors_dropped: self.anchors_dropped,
            gap_events: self.gap_events,
        })
    }

    /// 编码完整性（无插入无丢失）断言：
    /// 已封账 + 在途排空 + 折叠覆盖全量 + 生产申报==实收 + 断口为零。
    pub fn integrity_ok(&self) -> bool {
        self.sealed
            && self.in_flight() == 0
            && self.fp_folded == self.bytes_in
            && self.produced_reported == self.bytes_in
            && self.gap_events == 0
    }

    /// 环面最近 n 字节快照（从最新往回拷；供前后文取证）。
    pub fn ring_recent(&self, out: &mut [u8]) -> usize {
        let cap = self.ring.len();
        if cap == 0 {
            return 0;
        }
        let have = if self.ring_filled < cap as u64 {
            self.ring_filled as usize
        } else {
            cap
        };
        let n = if out.len() < have { out.len() } else { have };
        let mut i = 0usize;
        while i < n {
            // 回退位置：最新字节在 ring_pos 前一格
            let base = (self.ring_pos + cap - n) % cap;
            let pos = (base + i) % cap;
            out[i] = self.ring[pos];
            i += 1;
        }
        n
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6 首差异定位 DiffRecord
// ═══════════════════════════════════════════════════════════════════════

/// 首差异定位账：偏移 + 两侧字节 + 前后 8 字节摘要。
/// 结构体仅几十字节（[u8;8]×4），栈上无虞。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DiffRecord {
    /// 首差异流内偏移（两侧坐标一致处）。
    pub offset: u64,
    /// 退役侧字节。
    pub byte_a: u8,
    /// 中断侧字节。
    pub byte_b: u8,
    /// 退役侧前文（滚动窗原序，最早填 0 位）。
    pub before_a: [u8; CONTEXT_SPAN],
    /// 退役侧后文（实存 na 字节）。
    pub after_a: [u8; CONTEXT_SPAN],
    /// 中断侧前文。
    pub before_b: [u8; CONTEXT_SPAN],
    /// 中断侧后文（实存 nb 字节）。
    pub after_b: [u8; CONTEXT_SPAN],
    /// 后文实存数（流尾截短时 < CONTEXT_SPAN）。
    pub na: u8,
    pub nb: u8,
}

const HEX_TAB: &[u8; 16] = b"0123456789ABCDEF";

/// 单字节十六进制入串。
fn push_hex_byte(s: &mut String, b: u8) {
    let hi = (b >> 4) as usize;
    let lo = (b & 0x0F) as usize;
    s.push(HEX_TAB[hi] as char);
    s.push(HEX_TAB[lo] as char);
}

/// 8 字节窗十六进制摘要。
fn hex8(bs: &[u8; CONTEXT_SPAN]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < CONTEXT_SPAN {
        push_hex_byte(&mut s, bs[i]);
        i += 1;
    }
    s
}

impl DiffRecord {
    /// 首差异摘要行（取证面：偏移 + 两侧字节 + 前后文）。
    pub fn summary(&self) -> String {
        format!(
            "首差@偏移#{} 退役侧={:02X} 中断侧={:02X} | 退役侧前文:{} 后文:{}(实{}) | 中断侧前文:{} 后文:{}(实{})",
            self.offset,
            self.byte_a,
            self.byte_b,
            hex8(&self.before_a),
            hex8(&self.after_a),
            self.na,
            hex8(&self.before_b),
            hex8(&self.after_b),
            self.nb,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7 事件账 EventLog（有界、丢弃显性计数）
// ═══════════════════════════════════════════════════════════════════════

/// 单条事件（诊断码段内 + 名签 + 详注）。
#[derive(Clone)]
pub struct EventRec {
    /// 段内诊断码（∈ [0x13D0, 0x13DF]）。
    pub code: u16,
    /// 名签。
    pub tag: &'static str,
    /// 详注。
    pub note: String,
}

/// 有界事件账：append-only，满后丢弃并显性计数（绝不覆盖旧账）。
pub struct EventLog {
    entries: Vec<EventRec>,
    dropped: u32,
}

impl EventLog {
    pub fn new() -> Self {
        EventLog {
            entries: Vec::new(),
            dropped: 0,
        }
    }

    /// 落一条事件；账满则丢弃并计数。
    pub fn log(&mut self, code: u16, tag: &'static str, note: &str) {
        if self.entries.len() >= MAX_EVENTS {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.entries.push(EventRec {
            code,
            tag,
            note: note.to_string(),
        });
    }

    /// 已落事件数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 丢弃数（显性）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 事件账视图。
    pub fn entries(&self) -> &[EventRec] {
        &self.entries
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8 EquivVerifier 等价验证器
// ═══════════════════════════════════════════════════════════════════════

/// 逐字节对拍输出（再生流账面）。
struct WalkOut {
    matched: u64,
    compared: u64,
    first_diff: Option<DiffRecord>,
    fp_a: u64,
    fp_b: u64,
    anchors: Vec<WalkAnchor>,
}

/// 再生流 4KB 锚点（两侧同位折叠态）。
#[derive(Clone, Copy)]
struct WalkAnchor {
    offset: u64,
    fp_a: u64,
    fp_b: u64,
}

/// 前文滚动窗（8 字节环形，摘要取证用）。
struct PrevWindow {
    buf: [u8; CONTEXT_SPAN],
    pos: usize,
}

impl PrevWindow {
    fn new() -> Self {
        PrevWindow {
            buf: [0u8; CONTEXT_SPAN],
            pos: 0,
        }
    }

    fn push(&mut self, b: u8) {
        self.buf[self.pos] = b;
        self.pos += 1;
        if self.pos >= CONTEXT_SPAN {
            self.pos = 0;
        }
    }

    fn window(&self) -> [u8; CONTEXT_SPAN] {
        self.buf
    }
}

/// 再生流逐字节对拍：从种子重放退役侧正本流与中断侧（篡改）流，
/// 逐窗逐字节对拍至 bound 字节；同程折叠双侧 FNV 并按 4KB 落再生锚点。
/// 首差异落账（偏移 + 两侧字节 + 前后 8 字节摘要）后继续计数（ppm 面需要
/// 全窗匹配量）。
fn walk_compare(seed: u64, tamper: Option<InjectKind>, bound: u64) -> Result<WalkOut, SerEquivError> {
    let mut sa = SerSource::new(seed, STREAM_LEN_BYTES);
    let mut sb = TamperSource::new(seed, STREAM_LEN_BYTES, tamper);
    let mut matched = 0u64;
    let mut compared = 0u64;
    let mut first: Option<DiffRecord> = None;
    let mut fp_a = FNV_OFFSET;
    let mut fp_b = FNV_OFFSET;
    let mut anchors: Vec<WalkAnchor> = Vec::new();
    let mut prev_a = PrevWindow::new();
    let mut prev_b = PrevWindow::new();
    while compared < bound {
        let ba = match sa.next_byte() {
            Some(b) => b,
            None => break,
        };
        let bb = match sb.next_byte() {
            Some(b) => b,
            None => break,
        };
        fp_a = fnv1a_step(fp_a, ba);
        fp_b = fnv1a_step(fp_b, bb);
        if ba == bb {
            matched = matched.saturating_add(1);
            prev_a.push(ba);
            prev_b.push(bb);
        } else if first.is_none() {
            // 首差异落账：前后文摘要（前 8 自滚动窗，后 8 各自续取流）。
            let mut after_a = [0u8; CONTEXT_SPAN];
            let mut after_b = [0u8; CONTEXT_SPAN];
            let mut na = 0u8;
            let mut nb = 0u8;
            let mut k = 0usize;
            while k < CONTEXT_SPAN {
                if let Some(x) = sa.next_byte() {
                    if (na as usize) < CONTEXT_SPAN {
                        after_a[na as usize] = x;
                    }
                    na = na.saturating_add(1);
                }
                if let Some(x) = sb.next_byte() {
                    if (nb as usize) < CONTEXT_SPAN {
                        after_b[nb as usize] = x;
                    }
                    nb = nb.saturating_add(1);
                }
                k += 1;
            }
            first = Some(DiffRecord {
                offset: compared,
                byte_a: ba,
                byte_b: bb,
                before_a: prev_a.window(),
                after_a,
                before_b: prev_b.window(),
                after_b,
                na,
                nb,
            });
            prev_a.push(ba);
            prev_b.push(bb);
        }
        compared = compared.saturating_add(1);
        // 每 4KB 落再生锚点（双侧同位）
        if (compared % ANCHOR_SPAN_BYTES) == 0 && anchors.len() < MAX_ANCHORS {
            anchors.push(WalkAnchor {
                offset: compared,
                fp_a,
                fp_b,
            });
        }
    }
    Ok(WalkOut {
        matched,
        compared,
        first_diff: first,
        fp_a,
        fp_b,
        anchors,
    })
}

/// 等价验证器：捕获两侧 + 指纹门 + 逐字节对拍 + 锚点/终指纹双面对账 +
/// 等价率 ppm + 判定闭集出口。
pub struct EquivVerifier {
    cap_a: StreamCapture,
    cap_b: StreamCapture,
    scratch_a: Vec<u8>,
    scratch_b: Vec<u8>,
    seed: u64,
    tamper: Option<InjectKind>,
    fed: bool,
    /// 声明样本计划量（字节）：样本门槛按计划量判——注入造成的实收缩短
    /// 走 LenMismatch/Divergent，绝不借 Inconclusive 掩盖不等价。
    planned_total: u64,
    total_a: u64,
    total_b: u64,
    matched: u64,
    compared: u64,
    first_diff: Option<DiffRecord>,
    len_mismatch: bool,
    gate_pass: bool,
    gate_a_fp: u64,
    gate_b_fp: u64,
    anchors_checked: u32,
    anchor_mismatches: u32,
    capture_mismatch_a: bool,
    capture_mismatch_b: bool,
    integrity_a: bool,
    integrity_b: bool,
    verdict: Option<Verdict>,
    events: EventLog,
}

impl EquivVerifier {
    /// 建验证器（双侧各一 RING_CAP_BYTES 捕获环 + 各一 4KB 刮擦缓冲，全堆上）。
    pub fn new() -> Result<Self, SerEquivError> {
        let mut cap_a = StreamCapture::new(SideKind::LegacyPoll, RING_CAP_BYTES)?;
        let mut cap_b = StreamCapture::new(SideKind::InterruptDriven, RING_CAP_BYTES)?;
        cap_a.prime()?;
        cap_b.prime()?;
        let mut scratch_a = Vec::with_capacity(SCRATCH_CAP);
        let mut i = 0usize;
        while i < SCRATCH_CAP {
            scratch_a.push(0);
            i += 1;
        }
        let mut scratch_b = Vec::with_capacity(SCRATCH_CAP);
        let mut j = 0usize;
        while j < SCRATCH_CAP {
            scratch_b.push(0);
            j += 1;
        }
        Ok(EquivVerifier {
            cap_a,
            cap_b,
            scratch_a,
            scratch_b,
            seed: 0,
            tamper: None,
            fed: false,
            planned_total: 0,
            total_a: 0,
            total_b: 0,
            matched: 0,
            compared: 0,
            first_diff: None,
            len_mismatch: false,
            gate_pass: false,
            gate_a_fp: 0,
            gate_b_fp: 0,
            anchors_checked: 0,
            anchor_mismatches: 0,
            capture_mismatch_a: false,
            capture_mismatch_b: false,
            integrity_a: false,
            integrity_b: false,
            verdict: None,
            events: EventLog::new(),
        })
    }

    /// 泵送两侧流进捕获（传输面）。
    ///
    /// - 退役侧（轮询面）：定长 507B 块推进（质数块，刻意错开 4KB 锚界）；
    /// - 中断侧（中断面）：变长 128..=2048B 块推进（确定性调度）；
    /// - 两侧同种子 ⇒ 字节流同，块界不同——等价在字节流上判定，块界差异
    ///   正是 "退役前后" 的传输面差异；
    /// - 每块：带序号 append（断口检出）→ 生产申报同步 → 立即出账排空
    ///   （在途归零）。
    pub fn feed(&mut self, seed: u64, tamper: Option<InjectKind>) -> Result<(), SerEquivError> {
        self.seed = seed;
        self.tamper = tamper;
        // 样本计划量：双侧按声明量计（截短注入按其声明截短量计）
        self.planned_total = match tamper {
            Some(InjectKind::ShortStream { total }) => {
                if STREAM_LEN_BYTES < total {
                    STREAM_LEN_BYTES
                } else {
                    total
                }
            }
            _ => STREAM_LEN_BYTES,
        };
        let mut src_a = SideSource::new_legacy(seed, STREAM_LEN_BYTES);
        let mut src_b = SideSource::new_interrupted(seed, STREAM_LEN_BYTES, tamper);
        let mut sched: u64 = seed ^ SCHED_SALT;
        loop {
            let a_rem = src_a.remaining();
            let b_rem = src_b.remaining();
            if a_rem == 0 && b_rem == 0 {
                break;
            }
            if a_rem > 0 {
                let want = if (LEGACY_CHUNK as u64) < a_rem {
                    LEGACY_CHUNK as u64
                } else {
                    a_rem
                };
                let n = src_a.next_chunk(&mut self.scratch_a[..want as usize]);
                if n == 0 {
                    return Err(SerEquivError::Underrun);
                }
                let before = src_a.produced().saturating_sub(n as u64);
                self.cap_a.append(before, &self.scratch_a[..n])?;
                self.cap_a.report_produced(src_a.produced());
                let _ = self.cap_a.consume(n as u64)?;
            }
            if b_rem > 0 {
                let mut want = sched_next(&mut sched) as u64;
                if want > b_rem {
                    want = b_rem;
                }
                if want > SCRATCH_CAP as u64 {
                    want = SCRATCH_CAP as u64;
                }
                let n = src_b.next_chunk(&mut self.scratch_b[..want as usize]);
                if n == 0 {
                    return Err(SerEquivError::Underrun);
                }
                let before = src_b.produced().saturating_sub(n as u64);
                self.cap_b.append(before, &self.scratch_b[..n])?;
                self.cap_b.report_produced(src_b.produced());
                let _ = self.cap_b.consume(n as u64)?;
            }
        }
        self.fed = true;
        Ok(())
    }

    /// 守恒面注入透传（少记：申报≠实收）。
    pub fn inject_undercount(&mut self, by: u64) {
        self.cap_a.inject_undercount(by);
    }

    /// 双侧断口事件合计。
    pub fn gap_events_total(&self) -> u32 {
        self.cap_a
            .gap_events()
            .saturating_add(self.cap_b.gap_events())
    }

    /// 双侧锚点对账核对数（对）。
    pub fn anchors_checked(&self) -> u32 {
        self.anchors_checked
    }

    /// 锚点对账失配数。
    pub fn anchor_mismatches(&self) -> u32 {
        self.anchor_mismatches
    }

    /// 指纹门结果。
    pub fn gate_pass(&self) -> bool {
        self.gate_pass
    }

    /// 退役侧全程 FNV 终指纹（封账后）。
    pub fn gate_a_fp(&self) -> u64 {
        self.gate_a_fp
    }

    /// 中断侧全程 FNV 终指纹（封账后）。
    pub fn gate_b_fp(&self) -> u64 {
        self.gate_b_fp
    }

    /// 退役侧样本总量。
    pub fn total_a(&self) -> u64 {
        self.total_a
    }

    /// 中断侧样本总量。
    pub fn total_b(&self) -> u64 {
        self.total_b
    }

    /// 对拍字节数。
    pub fn compared(&self) -> u64 {
        self.compared
    }

    /// 匹配字节数。
    pub fn matched(&self) -> u64 {
        self.matched
    }

    /// 退役侧完整性断言。
    pub fn integrity_a(&self) -> bool {
        self.integrity_a
    }

    /// 中断侧完整性断言。
    pub fn integrity_b(&self) -> bool {
        self.integrity_b
    }

    /// 首差异账。
    pub fn first_diff(&self) -> Option<DiffRecord> {
        self.first_diff
    }

    /// 对拍窗是否被定位窗截断（指纹门破路径）。
    pub fn walk_capped(&self) -> bool {
        !self.gate_pass && self.compared >= WALK_BOUND
    }

    /// 双侧流长是否失配。
    pub fn len_mismatch(&self) -> bool {
        self.len_mismatch
    }

    /// 对拍总量（min(双侧样本)）。
    pub fn total_cmp(&self) -> u64 {
        if self.total_a < self.total_b {
            self.total_a
        } else {
            self.total_b
        }
    }

    /// 声明样本计划量（样本门槛的判定基准）。
    pub fn planned_total(&self) -> u64 {
        self.planned_total
    }

    /// 等价率（ppm，1_000_000 = 100%）。u128 无损，saturating 于 10^6。
    pub fn ppm(&self) -> u64 {
        if self.compared == 0 {
            return 0;
        }
        let m = self.matched as u128;
        let c = self.compared as u128;
        let ppm = (m * 1_000_000u128) / c;
        if ppm > 1_000_000u128 {
            1_000_000
        } else {
            ppm as u64
        }
    }

    /// 不等价首因（定位优先级闭集）。
    pub fn cause(&self) -> DivergenceCause {
        if self.planned_total < MIN_SAMPLE_BYTES {
            return DivergenceCause::SampleShort;
        }
        if self.len_mismatch {
            return DivergenceCause::LenMismatch;
        }
        if self.first_diff.is_some() {
            return DivergenceCause::FirstDiff;
        }
        if !self.gate_pass {
            return DivergenceCause::GateFail;
        }
        if self.anchor_mismatches > 0 {
            return DivergenceCause::AnchorMismatch;
        }
        if self.capture_mismatch_a || self.capture_mismatch_b {
            return DivergenceCause::CaptureMismatch;
        }
        if self.gap_events_total() > 0 {
            return DivergenceCause::SeqGaps;
        }
        if !(self.integrity_a && self.integrity_b) {
            return DivergenceCause::IntegrityBreach;
        }
        DivergenceCause::None
    }

    /// 事件账视图。
    pub fn events(&self) -> &EventLog {
        &self.events
    }

    /// 主验：封账 → 指纹门 → 逐字节对拍 → 锚点/终指纹双面对账 → 判定。
    pub fn verify(&mut self) -> Result<Verdict, SerEquivError> {
        if !self.fed {
            return Err(SerEquivError::NotSealed);
        }
        // ── 封账（编码完整性：在途排空 + 折叠覆盖全量）─────────────────
        let stamp_a = self.cap_a.seal()?;
        let stamp_b = self.cap_b.seal()?;
        self.integrity_a = self.cap_a.integrity_ok();
        self.integrity_b = self.cap_b.integrity_ok();
        self.events.log(
            CODE_SEAL,
            "封账",
            &format!(
                "退役侧{}总量={} 终指纹={:#018X} 锚点={}; 中断侧{}总量={} 终指纹={:#018X} 锚点={}",
                stamp_a.side.as_str(),
                stamp_a.total,
                stamp_a.final_fp,
                stamp_a.anchors,
                stamp_b.side.as_str(),
                stamp_b.total,
                stamp_b.final_fp,
                stamp_b.anchors,
            ),
        );

        // ── 指纹门（等价的必要条件；不等快速失败省逐字节）───────────────
        self.total_a = self.cap_a.bytes_in();
        self.total_b = self.cap_b.bytes_in();
        self.len_mismatch = self.total_a != self.total_b;
        self.gate_a_fp = self.cap_a.fp();
        self.gate_b_fp = self.cap_b.fp();
        self.gate_pass = self.gate_a_fp == self.gate_b_fp;
        if self.gate_pass {
            self.events.log(
                CODE_GATE,
                "指纹门",
                "双侧全程 FNV 终指纹相等：必要条件成立，门开，进入全量逐字节对拍",
            );
        } else {
            self.events.log(
                CODE_GATE,
                "指纹门",
                "双侧全程 FNV 终指纹不等：等价必要条件即破，快速失败，仅走 128KB 定位窗（省逐字节全量）",
            );
        }

        // ── 逐字节对拍（门开=全量；门破=定位窗）─────────────────────────
        let total_cmp = self.total_cmp();
        let bound = if self.gate_pass {
            total_cmp
        } else if WALK_BOUND < total_cmp {
            WALK_BOUND
        } else {
            total_cmp
        };
        let walk = walk_compare(self.seed, self.tamper, bound)?;
        self.matched = walk.matched;
        self.compared = walk.compared;
        self.first_diff = walk.first_diff;

        // ── 锚点对账（再生流 4KB 锚点 ↔ 捕获锚点，双侧逐对核）───────────
        {
            let cap_aa: &[AnchorRec] = self.cap_a.anchors();
            let cap_bb: &[AnchorRec] = self.cap_b.anchors();
            let wa: &[WalkAnchor] = &walk.anchors;
            let mut n = cap_aa.len();
            if cap_bb.len() < n {
                n = cap_bb.len();
            }
            if wa.len() < n {
                n = wa.len();
            }
            let mut i = 0usize;
            while i < n {
                self.anchors_checked = self.anchors_checked.saturating_add(1);
                let w = wa[i];
                let a = cap_aa[i];
                let b = cap_bb[i];
                if a.offset != w.offset || a.fp != w.fp_a {
                    self.anchor_mismatches = self.anchor_mismatches.saturating_add(1);
                }
                if b.offset != w.offset || b.fp != w.fp_b {
                    self.anchor_mismatches = self.anchor_mismatches.saturating_add(1);
                }
                i += 1;
            }
        }
        if self.anchor_mismatches > 0 {
            self.events.log(
                CODE_ANCHOR,
                "锚点对账",
                &format!(
                    "再生流 4KB 锚点与捕获锚点逐对核失配 {} 处（核 {} 对）——传输面与再生面不一致",
                    self.anchor_mismatches, self.anchors_checked,
                ),
            );
        }

        // ── 再生↔捕获终指纹对账（收发一致实证；仅全量走窗时有意义）─────
        if self.gate_pass {
            if walk.fp_a != self.gate_a_fp {
                self.capture_mismatch_a = true;
            }
            if walk.fp_b != self.gate_b_fp {
                self.capture_mismatch_b = true;
            }
            if self.capture_mismatch_a || self.capture_mismatch_b {
                self.events.log(
                    CODE_ANCHOR,
                    "再生对账",
                    "再生流终指纹与捕获终指纹失配——捕获申报的流与模型再生流不一致",
                );
            }
        }

        if let Some(d) = self.first_diff {
            self.events.log(CODE_DIFF, "首差异", &d.summary());
        }
        self.events.log(
            CODE_PPM,
            "等价率",
            &format!(
                "对拍={} 匹配={} 等价率={}ppm 门={} 断口={} 锚点失配={}",
                self.compared,
                self.matched,
                self.ppm(),
                self.gate_pass,
                self.gap_events_total(),
                self.anchor_mismatches,
            ),
        );

        let v = self.conclude();
        self.verdict = Some(v);
        self.events.log(v.code(), "判定", v.as_str());
        Ok(v)
    }

    /// 判定闭集：样本门槛（按声明计划量）→ 零差异合取（全量覆盖 + 逐字节
    /// 相等 + 指纹门 + 长度一致 + 锚点对账 + 再生对账 + 完整性双侧 + 断口
    /// 为零）。注入造成的实收缩短不构成 Inconclusive——那是不等价证据。
    fn conclude(&self) -> Verdict {
        if self.planned_total < MIN_SAMPLE_BYTES {
            return Verdict::Inconclusive;
        }
        let full_cover = self.compared >= self.total_cmp();
        let zero_diff = self.matched == self.compared && self.first_diff.is_none();
        let ok = full_cover
            && zero_diff
            && self.gate_pass
            && !self.len_mismatch
            && self.anchor_mismatches == 0
            && !self.capture_mismatch_a
            && !self.capture_mismatch_b
            && self.integrity_a
            && self.integrity_b
            && self.gap_events_total() == 0;
        if ok {
            Verdict::Equivalent
        } else {
            Verdict::Divergent
        }
    }

    /// 唯一 "等价" 结论出口：只有判定为等价时放行 ppm；样本不足即判定越权，
    /// 不等价即 NotEquivalent——等价结论不可绕过。
    pub fn equivalence_claim(&self) -> Result<u64, SerEquivError> {
        match self.verdict {
            Some(Verdict::Equivalent) => Ok(self.ppm()),
            Some(Verdict::Inconclusive) => Err(SerEquivError::DomainViolation),
            Some(Verdict::Divergent) => Err(SerEquivError::NotEquivalent),
            None => Err(SerEquivError::NotSealed),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9 轮账 RoundRecord / RoundLedger（十轮独立验证 + 双跑确定性 + 完整性 10/10）
// ═══════════════════════════════════════════════════════════════════════

/// 单轮账目（轮轮留账；字段全原语，双跑对拍可逐字段核）。
#[derive(Clone)]
pub struct RoundRecord {
    /// 轮次号。
    pub round: u8,
    /// 本轮种子。
    pub seed: u64,
    /// 注入名签（清洁轮 = "无"）。
    pub tamper_tag: &'static str,
    /// 注入位/量。
    pub tamper_pos: u64,
    /// 判定。
    pub verdict: Verdict,
    /// 退役侧样本量。
    pub total_a: u64,
    /// 中断侧样本量。
    pub total_b: u64,
    /// 对拍字节数。
    pub compared: u64,
    /// 匹配字节数。
    pub matched: u64,
    /// 等价率 ppm。
    pub ppm: u64,
    /// 指纹门。
    pub gate_pass: bool,
    /// 首差异偏移。
    pub first_diff_offset: Option<u64>,
    /// 退役侧完整性。
    pub integrity_a: bool,
    /// 中断侧完整性。
    pub integrity_b: bool,
    /// 锚点核对数（对）。
    pub anchors_checked: u32,
    /// 锚点失配数。
    pub anchor_mismatches: u32,
    /// 断口合计。
    pub gap_events: u32,
    /// 不等价首因。
    pub cause: DivergenceCause,
    /// 同种子双跑记录逐字段一致（确定性对拍）。
    pub double_run_match: bool,
    /// 本轮编码完整性断言（无插入无丢失）通过。
    pub integrity_assert: bool,
}

impl RoundRecord {
    /// 双跑确定性对拍：逐字段核（含 Option 的显性双臂匹配）。
    pub fn fields_match(&self, o: &RoundRecord) -> bool {
        let fd = match (self.first_diff_offset, o.first_diff_offset) {
            (None, None) => true,
            (Some(x), Some(y)) => x == y,
            _ => false,
        };
        self.round == o.round
            && self.seed == o.seed
            && self.tamper_tag == o.tamper_tag
            && self.tamper_pos == o.tamper_pos
            && self.verdict == o.verdict
            && self.total_a == o.total_a
            && self.total_b == o.total_b
            && self.compared == o.compared
            && self.matched == o.matched
            && self.ppm == o.ppm
            && self.gate_pass == o.gate_pass
            && fd
            && self.integrity_a == o.integrity_a
            && self.integrity_b == o.integrity_b
            && self.anchors_checked == o.anchors_checked
            && self.anchor_mismatches == o.anchor_mismatches
            && self.gap_events == o.gap_events
            && self.cause == o.cause
    }

    /// 轮账行（留账面）。
    pub fn to_line(&self) -> String {
        format!(
            "轮#{} 种子={:#018X} [{}] 判定={} 样本A={} 样本B={} 对拍={} 匹配={} ppm={} 门={} 完整性={}/{} 完整性断言={} 首差={:?} 因={}",
            self.round,
            self.seed,
            self.tamper_tag,
            self.verdict.as_str(),
            self.total_a,
            self.total_b,
            self.compared,
            self.matched,
            self.ppm,
            self.gate_pass,
            self.integrity_a,
            self.integrity_b,
            self.integrity_assert,
            self.first_diff_offset,
            self.cause.as_str(),
        )
    }
}

/// 单轮执行（无账面副作用）：建验证器 → 泵送 → 守恒注入透传 → 主验 →
/// 轮记录。同种子同注入必得同记录（确定性根基）。
fn verify_round_once(round: u8, seed: u64, tamper: Option<InjectKind>) -> Result<RoundRecord, SerEquivError> {
    let mut v = EquivVerifier::new()?;
    v.feed(seed, tamper)?;
    if let Some(InjectKind::Undercount { by }) = tamper {
        v.inject_undercount(by);
    }
    let verdict = v.verify()?;
    let (tag, pos) = match tamper {
        Some(k) => (k.tag(), k.magnitude()),
        None => ("无", 0),
    };
    Ok(RoundRecord {
        round,
        seed,
        tamper_tag: tag,
        tamper_pos: pos,
        verdict,
        total_a: v.total_a(),
        total_b: v.total_b(),
        compared: v.compared(),
        matched: v.matched(),
        ppm: v.ppm(),
        gate_pass: v.gate_pass(),
        first_diff_offset: v.first_diff().map(|d| d.offset),
        integrity_a: v.integrity_a(),
        integrity_b: v.integrity_b(),
        anchors_checked: v.anchors_checked(),
        anchor_mismatches: v.anchor_mismatches(),
        gap_events: v.gap_events_total(),
        cause: v.cause(),
        double_run_match: true,
        integrity_assert: false,
    })
}

/// 十轮断言账：append-only；一轮失败全局失败；清洁轮内建同种子双跑
/// 确定性对拍与编码完整性断言（无插入无丢失）逐轮落账。
pub struct RoundLedger {
    rounds: Vec<RoundRecord>,
    integrity_passes: u32,
    det_rounds_ok: u32,
    inj_total: u32,
    inj_caught: u32,
    events: EventLog,
}

impl RoundLedger {
    pub fn new() -> Self {
        RoundLedger {
            rounds: Vec::new(),
            integrity_passes: 0,
            det_rounds_ok: 0,
            inj_total: 0,
            inj_caught: 0,
            events: EventLog::new(),
        }
    }

    /// 轮账视图。
    pub fn rounds(&self) -> &[RoundRecord] {
        &self.rounds
    }

    /// 编码完整性断言通过数（J1 面须达 10/10）。
    pub fn integrity_passes(&self) -> u32 {
        self.integrity_passes
    }

    /// 双跑确定性通过轮数。
    pub fn det_rounds_ok(&self) -> u32 {
        self.det_rounds_ok
    }

    /// 注入演练总数 / 已检出数。
    pub fn injections(&self) -> (u32, u32) {
        (self.inj_total, self.inj_caught)
    }

    /// 事件账视图。
    pub fn events(&self) -> &EventLog {
        &self.events
    }

    /// 跑一轮并落账。
    ///
    /// - 清洁轮（tamper = None）：同种子双跑，记录逐字段对拍（确定性面）；
    /// - 完整性断言：无插入无丢失 + 判定等价 + 断口为零 + 双跑一致，逐轮
    ///   落账（清洁轮应 10/10 通过）；
    /// - 轮账 append-only，满则显性 LedgerFull。
    pub fn run_round(&mut self, round: u8, seed: u64, tamper: Option<InjectKind>) -> Result<RoundRecord, SerEquivError> {
        if self.rounds.len() >= LEDGER_CAP {
            return Err(SerEquivError::LedgerFull);
        }
        let mut rec = verify_round_once(round, seed, tamper)?;
        if tamper.is_none() {
            let rec2 = verify_round_once(round, seed, tamper)?;
            rec.double_run_match = rec.fields_match(&rec2);
            if rec.double_run_match {
                self.det_rounds_ok = self.det_rounds_ok.saturating_add(1);
                self.events.log(
                    CODE_DETERMINISM,
                    "双跑确定性",
                    &format!("轮#{} 同种子双跑记录逐字段一致", round),
                );
            } else {
                self.events.log(
                    CODE_DETERMINISM,
                    "双跑确定性",
                    &format!("轮#{} 同种子双跑记录失配——确定性破，全局红", round),
                );
            }
        }
        rec.integrity_assert = tamper.is_none()
            && rec.verdict == Verdict::Equivalent
            && rec.integrity_a
            && rec.integrity_b
            && rec.gap_events == 0
            && rec.double_run_match;
        if rec.integrity_assert {
            self.integrity_passes = self.integrity_passes.saturating_add(1);
        }
        self.events
            .log(CODE_ROUND, "轮账", &rec.to_line());
        self.rounds.push(rec.clone());
        Ok(rec)
    }

    /// 十轮独立验证：每轮独立种子流（轮种子 = 基种 ⊕ 轮次×黄金比例常数），
    /// 轮轮留账，一轮失败全局失败。返回是否十轮全等价。
    pub fn run_ten(&mut self, base_seed: u64) -> Result<bool, SerEquivError> {
        let mut all = true;
        let mut i = 0u8;
        while (i as usize) < ROUNDS {
            let seed = base_seed ^ (i as u64).wrapping_mul(ROUND_SALT);
            let rec = self.run_round(i, seed, None)?;
            if rec.verdict != Verdict::Equivalent {
                all = false;
                self.events.log(
                    CODE_ROUND,
                    "十轮账",
                    &format!("轮#{} 非等价（{}）——一轮失败全局失败", i, rec.cause.as_str()),
                );
            }
            i += 1;
        }
        if all {
            self.events.log(
                CODE_ROUND,
                "十轮账",
                &format!(
                    "十轮独立验证全等价；编码完整性断言 {}/{}",
                    self.integrity_passes, ROUNDS,
                ),
            );
        }
        Ok(all)
    }

    /// 注入面演练（六类全检出；漏检即红）：
    /// 插字节（首差恰落注入位 + 指纹门破）/ 丢字节 / 换序（序敏感实证）/
    /// 截短（样本门槛拒出结论——Inconclusive 不虚报）/ 断口（账面必吃
    /// SeqGapDetected）/ 少记（申报≠实收，守恒面检出）。
    pub fn run_injection_sweep(&mut self, base_seed: u64) -> Result<u32, SerEquivError> {
        self.inj_total = self.inj_total.saturating_add(6);
        let mut caught = 0u32;

        // 1 插字节：首差异必须恰落注入位，指纹门必须破
        let rec = self.run_round(
            91,
            base_seed ^ 0xA1,
            Some(InjectKind::InsertByte { at: INSERT_POS }),
        )?;
        let hit = rec.verdict == Verdict::Divergent
            && !rec.gate_pass
            && rec.first_diff_offset == Some(INSERT_POS);
        self.events.log(
            CODE_INJECT,
            "注入·插字节",
            &format!(
                "注入位={} 判定={} 门={} 首差={:?} 检出={}",
                rec.tamper_pos, rec.verdict.as_str(), rec.gate_pass, rec.first_diff_offset, hit,
            ),
        );
        if hit {
            caught = caught.saturating_add(1);
        }

        // 2 丢字节：不等价 + 首差 ≥ 偏好位（规划位可能前移）
        let rec = self.run_round(
            92,
            base_seed ^ 0xA2,
            Some(InjectKind::DropByte { at: DROP_POS }),
        )?;
        let hit = rec.verdict == Verdict::Divergent
            && !rec.gate_pass
            && rec.first_diff_offset.map_or(false, |o| o >= DROP_POS);
        self.events.log(
            CODE_INJECT,
            "注入·丢字节",
            &format!(
                "注入位={} 判定={} 门={} 首差={:?} 检出={}",
                rec.tamper_pos, rec.verdict.as_str(), rec.gate_pass, rec.first_diff_offset, hit,
            ),
        );
        if hit {
            caught = caught.saturating_add(1);
        }

        // 3 换序：字节序零差异判据的序敏感实证——同长度重排必被逐字节抓
        let rec = self.run_round(
            93,
            base_seed ^ 0xA3,
            Some(InjectKind::SwapAdjacent { at: SWAP_POS }),
        )?;
        let hit = rec.verdict == Verdict::Divergent
            && !rec.gate_pass
            && rec.first_diff_offset.map_or(false, |o| o >= SWAP_POS);
        self.events.log(
            CODE_INJECT,
            "注入·换序",
            &format!(
                "注入位={} 判定={} 门={} 首差={:?} 检出={}",
                rec.tamper_pos, rec.verdict.as_str(), rec.gate_pass, rec.first_diff_offset, hit,
            ),
        );
        if hit {
            caught = caught.saturating_add(1);
        }

        // 4 截短：样本不足必须 Inconclusive（拒出结论，不虚报等价）
        let rec = self.run_round(
            94,
            base_seed ^ 0xA4,
            Some(InjectKind::ShortStream { total: SHORT_TOTAL }),
        )?;
        let hit = rec.verdict == Verdict::Inconclusive;
        self.events.log(
            CODE_INJECT,
            "注入·截短",
            &format!(
                "截短至={} 判定={} 检出={}",
                rec.tamper_pos, rec.verdict.as_str(), hit,
            ),
        );
        if hit {
            caught = caught.saturating_add(1);
        }

        // 5 断口：账面以断序号续写必须吃 SeqGapDetected
        let gap_ok = scenario_seq_gap(base_seed ^ 0xA5)?;
        self.events.log(
            CODE_INJECT,
            "注入·断口",
            &format!("断序号续写必吃 SeqGapDetected 检出={}", gap_ok),
        );
        if gap_ok {
            caught = caught.saturating_add(1);
        }

        // 6 少记：申报≠实收，守恒面检出（字节面全等仍判不等价）
        let rec = self.run_round(
            96,
            base_seed ^ 0xA6,
            Some(InjectKind::Undercount { by: 1 }),
        )?;
        let hit = rec.verdict == Verdict::Divergent
            && rec.cause == DivergenceCause::IntegrityBreach
            && !rec.integrity_a;
        self.events.log(
            CODE_INJECT,
            "注入·少记",
            &format!(
                "少记={} 判定={} 因={} 退役侧完整性={} 检出={}",
                rec.tamper_pos, rec.verdict.as_str(), rec.cause.as_str(), rec.integrity_a, hit,
            ),
        );
        if hit {
            caught = caught.saturating_add(1);
        }

        self.inj_caught = self.inj_caught.saturating_add(caught);
        Ok(caught)
    }

    /// 健康面总成：十轮跑满 + 十轮全等价 + 完整性 10/10 + 双跑确定性 10/10 +
    /// 注入面全检出 + 事件账零丢弃。
    pub fn health(&self) -> HealthReport {
        let mut failures: Vec<&'static str> = Vec::new();
        let clean_count = self
            .rounds
            .iter()
            .filter(|r| r.tamper_tag == "无")
            .count();
        let eq_count = self
            .rounds
            .iter()
            .filter(|r| r.tamper_tag == "无" && r.verdict == Verdict::Equivalent)
            .count();
        let (inj_total, inj_caught) = self.injections();
        if clean_count < ROUNDS {
            failures.push("十轮账未跑满（清洁轮 <10）");
        }
        if eq_count != clean_count {
            failures.push("存在非等价清洁轮（字节流等价率未达 100%）");
        }
        if self.integrity_passes < ROUNDS as u32 {
            failures.push("编码完整性断言未达 10/10");
        }
        if self.det_rounds_ok < ROUNDS as u32 {
            failures.push("双跑确定性对拍存在失配");
        }
        if inj_total == 0 {
            failures.push("注入面未演练");
        } else if inj_caught < inj_total {
            failures.push("注入面存在漏检");
        }
        if self.events.dropped() > 0 {
            failures.push("事件账有丢弃（容量超限，账面不完整）");
        }
        HealthReport {
            ok: failures.is_empty(),
            rounds_done: clean_count,
            rounds_equivalent: eq_count,
            integrity_passes: self.integrity_passes,
            det_rounds_ok: self.det_rounds_ok,
            inj_total,
            inj_caught,
            event_count: self.events.len(),
            events_dropped: self.events.dropped(),
            failures,
        }
    }

    /// 收官取账（账面移交，账本清空复用）。
    pub fn take_records(&mut self) -> Vec<RoundRecord> {
        core::mem::replace(&mut self.rounds, Vec::new())
    }
}

/// 断口注入场景：以断序号续写捕获，必须吃 SeqGapDetected（账面检出）。
pub fn scenario_seq_gap(seed: u64) -> Result<bool, SerEquivError> {
    let mut cap = StreamCapture::new(SideKind::LegacyPoll, RING_CAP_BYTES)?;
    cap.prime()?;
    let mut src = SerSource::new(seed, 16);
    let mut buf = [0u8; 3];
    let n = src.next_chunk(&mut buf);
    // 正常连续进 n 字节（seq=0）
    let _ = cap.append(0, &buf[..n])?;
    // 断序号续写：期望序号 = n，实到 999 → 必吃断口
    let bad = cap.append(999, &[0x5A]);
    Ok(matches!(bad, Err(SerEquivError::SeqGapDetected)))
}

// ═══════════════════════════════════════════════════════════════════════
// §10 健康面与战役总成
// ═══════════════════════════════════════════════════════════════════════

/// 健康面报告。
pub struct HealthReport {
    /// 总健康（failures 为空）。
    pub ok: bool,
    /// 清洁轮数。
    pub rounds_done: usize,
    /// 等价清洁轮数。
    pub rounds_equivalent: usize,
    /// 编码完整性断言通过数。
    pub integrity_passes: u32,
    /// 双跑确定性通过轮数。
    pub det_rounds_ok: u32,
    /// 注入演练总数。
    pub inj_total: u32,
    /// 注入检出数。
    pub inj_caught: u32,
    /// 事件账条数。
    pub event_count: usize,
    /// 事件账丢弃数。
    pub events_dropped: u32,
    /// 失败项（中文，直达修复面）。
    pub failures: Vec<&'static str>,
}

impl HealthReport {
    /// 健康行（留账面）。
    pub fn to_line(&self) -> String {
        format!(
            "健康={} 清洁轮={}/{} 等价轮={}/{} 完整性={}/10 双跑={}/10 注入={}/{} 事件={} 丢弃={} 失败项={}",
            self.ok,
            self.rounds_done, ROUNDS,
            self.rounds_equivalent, ROUNDS,
            self.integrity_passes,
            self.det_rounds_ok,
            self.inj_caught, self.inj_total,
            self.event_count,
            self.events_dropped,
            self.failures.len(),
        )
    }
}

/// 战役报告：十轮账 + 注入演练 + 健康面。
pub struct CampaignReport {
    /// 轮账（10 清洁轮 + 注入演练轮）。
    pub records: Vec<RoundRecord>,
    /// 十轮独立验证是否全等价。
    pub ten_all_equivalent: bool,
    /// 注入检出数。
    pub injections_caught: u32,
    /// 注入演练总数。
    pub injections_total: u32,
    /// 健康面。
    pub health: HealthReport,
}

/// 跑全战役：十轮独立验证（每轮独立种子流 + 双跑确定性对拍 + 编码完整性
/// 断言）→ 注入面六类全检出演练 → 健康面总成。
pub fn run_campaign(base_seed: u64) -> Result<CampaignReport, SerEquivError> {
    let mut ledger = RoundLedger::new();
    let ten_ok = ledger.run_ten(base_seed)?;
    let caught = ledger.run_injection_sweep(base_seed)?;
    let health = ledger.health();
    let records = ledger.take_records();
    let inj_total = health.inj_total;
    Ok(CampaignReport {
        records,
        ten_all_equivalent: ten_ok,
        injections_caught: caught,
        injections_total: inj_total,
        health,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// §11 本模块断言总成（自包含 CheckSet 面，不依赖 crate::checks）
// ═══════════════════════════════════════════════════════════════════════

/// 本模块自包含断言集。
pub struct EquivCheckSet {
    name: String,
    items: Vec<(String, bool, &'static str)>,
}

impl EquivCheckSet {
    pub fn new(name: &str) -> Self {
        EquivCheckSet {
            name: name.to_string(),
            items: Vec::new(),
        }
    }

    /// 落一条断言（ok=false 时带段内失败注）。
    pub fn add(&mut self, name: &str, ok: bool, fail_note: &'static str) {
        self.items.push((name.to_string(), ok, fail_note));
    }

    /// 全绿判定。
    pub fn all_pass(&self) -> bool {
        self.items.iter().all(|(_, ok, _)| *ok)
    }

    /// 断言条数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 集名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 断言视图。
    pub fn items(&self) -> &[(String, bool, &'static str)] {
        &self.items
    }

    /// 汇总行。
    pub fn summary(&self) -> String {
        let pass = self.items.iter().filter(|(_, ok, _)| *ok).count();
        format!("{}：{}/{} 断言全绿={}", self.name, pass, self.items.len(), self.all_pass())
    }
}

/// 本模块判据总成：UNX-F1323-J1 三成分（字节流等价率 100% + 样本量 ≥10^6 +
/// 编码完整性断言 10/10）+ 注入面 + 双跑确定性 + 健康面，一次跑齐。
pub fn run_unx1323_checks() -> EquivCheckSet {
    let mut s = EquivCheckSet::new("UNX-F1323 serial 面等价验证账（字节流等价 100%）");
    // 编译期断言链（能跑到这里即已全过，落账留痕）
    s.add("编译期断言链（段界/门槛/锚界/环哨位）", true, "");
    s.add("FNV const 自检锚", FNV_SELFTEST != 0, "FNV const 路径异常");
    match run_campaign(0x1323_0000_0000_0001) {
        Ok(rep) => {
            s.add(
                "十轮独立验证全等价（字节流等价率 100%）",
                rep.ten_all_equivalent,
                "存在非等价清洁轮——逐字节对拍/指纹门/锚点对账有破",
            );
            s.add(
                "编码完整性断言 10/10",
                rep.health.integrity_passes >= ROUNDS as u32,
                "编码完整性断言未达 10/10（进=出+在途/申报守恒/断口为零有破）",
            );
            s.add(
                "双跑确定性 10/10",
                rep.health.det_rounds_ok >= ROUNDS as u32,
                "同种子双跑记录失配——确定性破",
            );
            s.add(
                "注入面六类全检出",
                rep.injections_total >= 6 && rep.injections_caught == rep.injections_total,
                "注入面存在漏检（插/丢/换序/截短/断口/少记）",
            );
            s.add(
                "首差异定位可用（插字节首差恰落注入位）",
                rep.records
                    .iter()
                    .any(|r| r.tamper_tag == "插字节" && r.first_diff_offset == Some(INSERT_POS)),
                "首差异定位未落注入位",
            );
            s.add(
                "样本门槛拒出结论（截短 → Inconclusive）",
                rep.records
                    .iter()
                    .any(|r| r.tamper_tag == "截短" && r.verdict == Verdict::Inconclusive),
                "样本不足未拒出结论——虚报风险",
            );
            s.add("健康面全绿", rep.health.ok, "健康面存在失败项");
        }
        Err(e) => {
            s.add("战役可跑", false, e.detail());
        }
    }
    s
}

/// 模块清单（登记面）。
pub fn module_manifest() -> &'static str {
    "unx1323_serequiv · UNX-F1323 serial 面等价验证账 · 诊断段 0x13D0~0x13DF · \
     子系统：SerSource/TamperSource/StreamCapture(环+FNV 4KB 锚)/EquivVerifier\
     (指纹门+逐字节+锚点对账+ppm)/RoundLedger(十轮+双跑+完整性 10/10)/注入面六类/EventLog/health"
}

// ═══════════════════════════════════════════════════════════════════════
// §12 冒烟测试（仅测试体可用 assert!/unwrap 类设施；内核路径零此类）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试脚手架：账面失败即 panic（带中文诊断）。
    fn must<T>(r: Result<T, SerEquivError>) -> T {
        match r {
            Ok(t) => t,
            Err(e) => panic!("账面失败（{}）：{}", e.code(), e.detail()),
        }
    }

    #[test]
    fn smoke_equivalent_full_sample() {
        // 清洁单轮：满样本 10^6，等价 100%，完整性断言过，双跑一致
        let mut ledger = RoundLedger::new();
        let rec = must(ledger.run_round(0, 0x1323_0001, None));
        assert_eq!(rec.verdict, Verdict::Equivalent);
        assert_eq!(rec.ppm, 1_000_000);
        assert!(rec.gate_pass);
        assert!(rec.integrity_a && rec.integrity_b);
        assert!(rec.integrity_assert);
        assert!(rec.double_run_match);
        assert_eq!(rec.cause, DivergenceCause::None);
        assert_eq!(rec.first_diff_offset, None);
        assert!(rec.total_a >= MIN_SAMPLE_BYTES);
        assert!(rec.total_b >= MIN_SAMPLE_BYTES);
        // 10^6/4KB = 244 对锚点逐对核
        assert_eq!(rec.anchors_checked, 244);
        assert_eq!(rec.anchor_mismatches, 0);
        // 唯一等价出口放行
        let v2 = must(verify_round_once(0, 0x1323_0001, None));
        assert_eq!(v2.verdict, Verdict::Equivalent);
    }

    #[test]
    fn smoke_first_diff_insert_located() {
        // 插字节：首差恰落注入位，注入字节 = 正本位反码，指纹门破
        let mut v = must(EquivVerifier::new());
        must(v.feed(0x1323_0002, Some(InjectKind::InsertByte { at: INSERT_POS })));
        let verdict = must(v.verify());
        assert_eq!(verdict, Verdict::Divergent);
        assert!(!v.gate_pass());
        let d = v.first_diff().expect("首差异账必须在");
        assert_eq!(d.offset, INSERT_POS);
        assert_eq!(d.byte_b, !d.byte_a);
        assert!(v.len_mismatch()); // 流长 +1
        // 等价出口必须拒绝
        assert!(matches!(v.equivalence_claim(), Err(SerEquivError::NotEquivalent)));
    }

    #[test]
    fn smoke_inconclusive_short_stream() {
        // 截短：样本 < 10^6 必须 Inconclusive（拒出结论，不虚报等价）
        let mut v = must(EquivVerifier::new());
        must(v.feed(
            0x1323_0003,
            Some(InjectKind::ShortStream { total: SHORT_TOTAL }),
        ));
        let verdict = must(v.verify());
        assert_eq!(verdict, Verdict::Inconclusive);
        assert!(matches!(v.equivalence_claim(), Err(SerEquivError::DomainViolation)));
        // 轮账面同样留痕
        let mut ledger = RoundLedger::new();
        let rec = must(ledger.run_round(0, 0x1323_0003, Some(InjectKind::ShortStream { total: SHORT_TOTAL })));
        assert_eq!(rec.verdict, Verdict::Inconclusive);
        assert_eq!(rec.cause, DivergenceCause::SampleShort);
        assert!(!rec.integrity_assert); // 注入轮不计清洁完整性断言
    }

    #[test]
    fn smoke_drop_swap_detected() {
        // 丢字节：流长 -1，门破，首差落规划位
        let mut v = must(EquivVerifier::new());
        must(v.feed(0x1323_0004, Some(InjectKind::DropByte { at: DROP_POS })));
        let verdict = must(v.verify());
        assert_eq!(verdict, Verdict::Divergent);
        assert!(!v.gate_pass());
        assert!(v.len_mismatch());
        let off = v.first_diff().expect("首差异账必须在").offset;
        assert!(off >= DROP_POS);

        // 换序：同长度重排——字节序零差异判据的序敏感实证
        let mut v2 = must(EquivVerifier::new());
        must(v2.feed(0x1323_0005, Some(InjectKind::SwapAdjacent { at: SWAP_POS })));
        let verdict2 = must(v2.verify());
        assert_eq!(verdict2, Verdict::Divergent);
        assert!(!v2.gate_pass());
        assert!(!v2.len_mismatch()); // 长度不变——纯序差异
        let off2 = v2.first_diff().expect("首差异账必须在").offset;
        assert!(off2 >= SWAP_POS);
    }

    #[test]
    fn smoke_gap_undercount_detected() {
        // 断口注入：断序号续写必吃 SeqGapDetected
        let gap_ok = must(scenario_seq_gap(0x1323_0006));
        assert!(gap_ok);

        // 少记注入：字节面全等（门开、逐字节全等）但申报≠实收 → 守恒面检出
        let mut v = must(EquivVerifier::new());
        must(v.feed(0x1323_0007, Some(InjectKind::Undercount { by: 1 })));
        v.inject_undercount(1);
        let verdict = must(v.verify());
        assert_eq!(verdict, Verdict::Divergent);
        assert!(v.gate_pass()); // 字节面确实全等
        assert!(!v.integrity_a()); // 守恒面破
        assert_eq!(v.cause(), DivergenceCause::IntegrityBreach);
    }

    #[test]
    fn smoke_ten_rounds_ledger() {
        // 十轮独立验证：轮轮独立种子、轮轮留账、完整性 10/10、双跑 10/10
        let mut ledger = RoundLedger::new();
        let all = must(ledger.run_ten(0x1323_BEEF));
        assert!(all);
        assert_eq!(ledger.rounds().len(), ROUNDS);
        assert_eq!(ledger.integrity_passes(), ROUNDS as u32);
        assert_eq!(ledger.det_rounds_ok(), ROUNDS as u32);
        for (i, r) in ledger.rounds().iter().enumerate() {
            assert_eq!(r.round as usize, i);
            assert_eq!(r.verdict, Verdict::Equivalent);
            assert_eq!(r.ppm, 1_000_000);
            assert!(r.integrity_assert);
            // 每轮种子独立（轮次×黄金比例扩列）
            assert_eq!(r.seed, 0x1323_BEEF ^ (i as u64).wrapping_mul(ROUND_SALT));
        }
        let h = ledger.health();
        // 十轮单跑未演练注入面：health 必须仅以该红旗挡总绿（红旗不虚报）
        assert_eq!(h.rounds_done, ROUNDS);
        assert_eq!(h.rounds_equivalent, ROUNDS);
        assert_eq!(h.integrity_passes, ROUNDS as u32);
        assert_eq!(h.det_rounds_ok, ROUNDS as u32);
        assert!(!h.ok);
        assert_eq!(h.failures.len(), 1);
        assert_eq!(h.failures[0], "注入面未演练");
        // 轮账行可打印（留账面非空）
        assert!(!ledger.rounds()[0].to_line().is_empty());
    }

    #[test]
    fn smoke_injection_sweep_six_of_six() {
        // 注入面六类全检出；漏检即红
        let mut ledger = RoundLedger::new();
        let _ = must(ledger.run_ten(0x1323_CAFE));
        let caught = must(ledger.run_injection_sweep(0x1323_CAFE));
        assert_eq!(caught, 6);
        let (total, ok) = ledger.injections();
        assert_eq!(total, 6);
        assert_eq!(ok, 6);
        let h = ledger.health();
        assert!(h.ok, "注入全检出后健康面须全绿：{}", h.to_line());
    }

    #[test]
    fn smoke_campaign_health_manifest() {
        // 全战役 + 断言总成 + 清单
        let rep = must(run_campaign(0x1323_1234));
        assert!(rep.ten_all_equivalent);
        assert_eq!(rep.injections_caught, rep.injections_total);
        assert!(rep.health.ok, "战役健康面须全绿：{}", rep.health.to_line());
        // 10 清洁轮 + 5 账面注入演练轮（断口注入走独立场景，不落轮账）
        assert_eq!(rep.records.len(), ROUNDS + 5);
        let checks = run_unx1323_checks();
        assert!(checks.all_pass(), "{}", checks.summary());
        assert!(checks.item_count() >= 9);
        assert!(!module_manifest().is_empty());
    }

    #[test]
    fn smoke_capture_edge_paths() {
        // 环哨位：非法容量显性拒
        assert!(matches!(
            StreamCapture::new(SideKind::LegacyPoll, 0),
            Err(SerEquivError::AllocParam)
        ));
        assert!(matches!(
            StreamCapture::new(SideKind::LegacyPoll, MAX_RING_CAP + 1),
            Err(SerEquivError::AllocParam)
        ));
        // 封账后写入/消费显性拒；断口与末断口账目
        let mut cap = must(StreamCapture::new(SideKind::InterruptDriven, 64));
        must(cap.prime());
        let _ = must(cap.append(0, b"abc"));
        cap.report_produced(3);
        // 在途必须排空后才能封账（进=出+在途）
        assert!(matches!(cap.seal(), Err(SerEquivError::IntegrityBreach)));
        let drained = must(cap.consume(3));
        assert_eq!(drained, 3);
        let stamp = must(cap.seal());
        assert_eq!(stamp.total, 3);
        assert_eq!(stamp.final_fp, cap.fp());
        assert!(matches!(
            cap.append(3, b"d"),
            Err(SerEquivError::AlreadySealed)
        ));
        assert!(matches!(
            cap.consume(1),
            Err(SerEquivError::AlreadySealed)
        ));
        assert!(cap.integrity_ok());
        // 断口账目字段
        let mut cap2 = must(StreamCapture::new(SideKind::LegacyPoll, 64));
        must(cap2.prime());
        assert!(matches!(cap2.append(5, b"x"), Err(SerEquivError::SeqGapDetected)));
        assert_eq!(cap2.gap_events(), 1);
        assert_eq!(cap2.last_gap(), (0, 5));
        // 环面最近快照（回绕覆盖后仍取最近窗）
        let mut cap3 = must(StreamCapture::new(SideKind::LegacyPoll, 8));
        must(cap3.prime());
        let _ = must(cap3.append(0, b"0123456789ABCDEF"));
        let mut win = [0u8; 4];
        let n = cap3.ring_recent(&mut win);
        assert_eq!(n, 4);
        assert_eq!(&win, b"CDEF");
    }

    #[test]
    fn smoke_source_determinism_and_plan() {
        // 同种子双跑逐字节一致；不同种子必异（首 64 字节面）
        let mut a = SerSource::new(7, 64);
        let mut b = SerSource::new(7, 64);
        for _ in 0..64 {
            assert_eq!(a.next_byte(), b.next_byte());
        }
        // 取尽显性 None
        assert!(a.next_byte().is_none());
        let mut differ = false;
        let mut a2 = SerSource::new(7, 64);
        let mut c2 = SerSource::new(8, 64);
        for _ in 0..64 {
            let x = a2.next_byte();
            let y = c2.next_byte();
            if x != y {
                differ = true;
            }
        }
        assert!(differ, "不同种子须异");
        let mut d = SerSource::new(1, 2);
        assert!(d.next_byte().is_some());
        assert!(d.next_byte().is_some());
        assert!(d.next_byte().is_none());
        // 规划位必为相邻异字节位且 ≥ prefer
        let p = plan_distinct_pair(0x1323_0009, 100, 4096);
        assert!(p >= 100);
        let x = SerSource::byte_at(0x1323_0009, 4096, p).expect("位必须在");
        let y = SerSource::byte_at(0x1323_0009, 4096, p + 1).expect("位+1 必须在");
        assert_ne!(x, y);
        // peek 不消耗
        let mut e = SerSource::new(3, 8);
        let peeked = e.peek_byte().expect("peek 必须有");
        let got = e.next_byte().expect("next 必须有");
        assert_eq!(peeked, got);
        assert_eq!(e.produced(), 1);
    }
}
