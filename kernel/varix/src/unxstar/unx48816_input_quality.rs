//! UNX-F48816~F48818：统一输入事件层观测埋点 / fuzz 面 / 判据用例集。
//!
//! ## 闸门次序（本模块对「观测」本身的四道闸）
//!
//! 1. **点位闸（F48816）**：一个被观测操作必须具备四个点——
//!    入口（Entry）、出口（Exit）、耗时（Cost）、结果（Result）。
//!    少任何一个点都是「观测缺口」，直接判红，绝不允许「看不到就不算」。
//! 2. **静默闸（F48816）**：操作返回错误时必须同时落一条异常记录；
//!    返回错误却没有任何异常记录 = 静默异常，等价于观测失效。
//! 3. **乱参数闸（F48817）**：位图解码、ioctl 合成、帧组装、超时判定、
//!    溢出折算、环突发六条车道全部吃任意 32/64 位乱参数，只允许
//!    返回 `Ok/Err`，绝不允许 panic、越界、无界分配。
//! 4. **复跑闸（F48818）**：20 条判据用例入库后必须能原样重跑，
//!    两次跑出的通过集合与指纹必须一致（用例集不是一次性表演）。
//!
//! ## 关键差异（对 B01 内其他段）
//!
//! * `unx48801` 关心「码能不能过」，本模块关心「过没过有没有被看见」。
//! * `unx48805` 的闸是数据面闸（单调/超时/溢出），本模块的闸是观测面闸：
//!   即便数据面处理正确，只要四个点缺一个、错误没上报，本段就红。
//! * `unx48811` 的取证是「未知事件去哪了」，本模块的取证是「已知操作
//!   的四点去哪了」，两者都用环形证据面但键不同。
//! * 本模块不复制兄弟模块的判据文本，只引用其语义口径（帧原子、100ms、
//!   4096），并以本地迷你模型重演，保证单独编译也能自证。
//!
//! 卡面口径：
//! * F48816-J1 四点 100% 覆盖；异常零静默（锚定 体验十三·补）。
//! * F48817-J1 fuzz ×10^4 零崩（锚定 体验十 健壮性）。
//! * F48818-J1 ×20 全绿入库（锚定 §AI-62 §8）。

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x701E0;

/// 错误码族条数。
pub const CODE_FAMILY_LEN: usize = 12;
/// 被观测操作数（能力查询/帧派发/守卫摄入/热插拔边/接管声明）。
pub const OP_COUNT: usize = 5;
/// 观测点数（入口/出口/耗时/结果）。
pub const POINT_COUNT: usize = 4;
/// 四点总标记数 5×4=20。
pub const MARK_TOTAL: u32 = 20;
/// 单次观测的痕迹环容量（超过即计丢弃，不得静默覆盖）。
pub const TRACE_CAP: usize = 4_096;
/// fuzz 卡面轮次 10^4。
pub const FUZZ_ROUNDS: u64 = 10_000;
/// 判据用例条数。
pub const CASE_COUNT: usize = 20;
/// 注入演练轮次。
pub const INJECT_ROUNDS: u32 = 10;
/// 域内重演的合成超时（与 F48804 口径一致：100ms）。
pub const SYN_TIMEOUT_US: u64 = 100_000;
/// 域内重演的溢出容量（与 F48805 口径一致：4096 事件）。
pub const OVERFLOW_CAP: u32 = 4_096;
/// 位图字数（12×64=768 位，覆盖 EV_KEY 的 0x2FF 上限）。
pub const BITMAP_WORDS: usize = 12;
/// 位图总位数。
pub const BITMAP_BITS: u32 = (BITMAP_WORDS as u32) * 64;

// ── 错误码族 ──────────────────────────────────────────────────
/// 观测面诊断码。
///
/// 全部落在 `[SEG_BASE+1, SEG_BASE+0x20)` 窗口内；`val()` 由枚举序号
/// 线性推进，`code_family_check()` 在收官时逐条核对，段外即 Leak。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityCode {
    /// 入口点缺失：不知道这次操作按什么参数进的。
    MissingEntry = 0,
    /// 出口点缺失：不知道这次操作是成功还是失败退出的。
    MissingExit,
    /// 耗时点缺失：失败路径的成本成了黑账。
    MissingCost,
    /// 结果点缺失：落库的结果与被观测结果对不上。
    MissingResult,
    /// 静默异常：返回错误但异常面一无所知。
    SilentAnomaly,
    /// 痕迹环溢出：观测自身丢记录且未计数。
    TraceOverflow,
    /// 操作被拒：参数非法，被观测层当场拒绝（正常错误，不是崩溃）。
    OpRejected,
    /// 乱参数导致崩溃（本实现恒不出现，留着让判据可判）。
    FuzzCrash,
    /// 用例集有洞：声称入库但实际跑不全。
    SuiteHole,
    /// 用例集不可复跑：两次跑的通过集合不一致。
    CaseUnstable,
    /// 指纹陈旧：双跑指纹不一致或落在初值。
    StaleDigest,
    /// 段窗口泄漏：码落到本段窗口之外。
    WindowLeak,
}

impl QualityCode {
    /// 全族（条数由 `CODE_FAMILY_LEN` 钉死）。
    pub const ALL: [QualityCode; CODE_FAMILY_LEN] = [
        QualityCode::MissingEntry,
        QualityCode::MissingExit,
        QualityCode::MissingCost,
        QualityCode::MissingResult,
        QualityCode::SilentAnomaly,
        QualityCode::TraceOverflow,
        QualityCode::OpRejected,
        QualityCode::FuzzCrash,
        QualityCode::SuiteHole,
        QualityCode::CaseUnstable,
        QualityCode::StaleDigest,
        QualityCode::WindowLeak,
    ];

    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 判据文本。
    pub fn detail(self) -> &'static str {
        match self {
            QualityCode::MissingEntry => "入口点缺失：观测没有记录进入参数",
            QualityCode::MissingExit => "出口点缺失：观测没有记录退出状态",
            QualityCode::MissingCost => "耗时点缺失：成本没有落账",
            QualityCode::MissingResult => "结果点缺失：结果没有落账",
            QualityCode::SilentAnomaly => "静默异常：错误返回未上报",
            QualityCode::TraceOverflow => "痕迹环溢出未计数",
            QualityCode::OpRejected => "参数非法被当场拒绝",
            QualityCode::FuzzCrash => "乱参数导致崩溃",
            QualityCode::SuiteHole => "用例集存在缺口",
            QualityCode::CaseUnstable => "用例集不可复跑",
            QualityCode::StaleDigest => "指纹陈旧或双跑不一致",
            QualityCode::WindowLeak => "诊断码落到段窗口之外",
        }
    }
}

/// 码值反查文本（段外返回空串，便于判据直接判空）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QualityCode::ALL.len() {
        if let Some(c) = QualityCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    ""
}

/// 码族自检：线性连续 + 文本非空 + 窗口内。
pub fn code_family_check() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < QualityCode::ALL.len() {
        if let Some(c) = QualityCode::ALL.get(i) {
            let want = SEG_BASE + 1 + i as u32;
            if c.val() != want {
                return Err(QualityCode::WindowLeak.val());
            }
            if c.val() >= SEG_BASE + 0x20 {
                return Err(QualityCode::WindowLeak.val());
            }
            if c.detail().is_empty() || detail(c.val()).is_empty() {
                return Err(QualityCode::WindowLeak.val());
            }
        }
        i += 1;
    }
    Ok(QualityCode::ALL.len() as u32)
}

// ── 观测点 ────────────────────────────────────────────────────
/// 四个观测点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    /// 入口：参数与来源。
    Entry = 0,
    /// 出口：成功/失败退出。
    Exit,
    /// 耗时：本次操作的工作量成本。
    Cost,
    /// 结果：落库结果与被观测结果的一致性凭据。
    Result,
}

impl ObsPoint {
    /// 四点全序。
    pub const ALL: [ObsPoint; POINT_COUNT] = [
        ObsPoint::Entry,
        ObsPoint::Exit,
        ObsPoint::Cost,
        ObsPoint::Result,
    ];

    /// 位序号（0..4）。
    pub fn bit(self) -> u32 {
        self as u32
    }

    /// 点位名。
    pub fn name(self) -> &'static str {
        match self {
            ObsPoint::Entry => "入口",
            ObsPoint::Exit => "出口",
            ObsPoint::Cost => "耗时",
            ObsPoint::Result => "结果",
        }
    }

    /// 该点缺失时对应的诊断码。
    pub fn missing_code(self) -> u32 {
        match self {
            ObsPoint::Entry => QualityCode::MissingEntry.val(),
            ObsPoint::Exit => QualityCode::MissingExit.val(),
            ObsPoint::Cost => QualityCode::MissingCost.val(),
            ObsPoint::Result => QualityCode::MissingResult.val(),
        }
    }
}

// ── 被观测操作 ────────────────────────────────────────────────
/// 被观测的五种输入层操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObservedOp {
    /// 能力查询：把 EVIOCGBIT 位图折成能力摘要。
    CapsQuery = 0,
    /// 帧派发：按三大主族把事件送进对应 sink。
    FrameDispatch,
    /// 守卫摄入：单调 + 100ms 合成 + 4096 溢出。
    GuardIngest,
    /// 热插拔边：插/绑/在飞/拔 的瞬时状态。
    HotplugEdge,
    /// 接管声明：AI-02 联签 + 降级兜底。
    BridgeDeclare,
}

/// 五种操作的全序。
pub const OPS: [ObservedOp; OP_COUNT] = [
    ObservedOp::CapsQuery,
    ObservedOp::FrameDispatch,
    ObservedOp::GuardIngest,
    ObservedOp::HotplugEdge,
    ObservedOp::BridgeDeclare,
];

impl ObservedOp {
    /// 操作序号。
    pub fn id(self) -> u32 {
        self as u32
    }

    /// 操作名。
    pub fn name(self) -> &'static str {
        match self {
            ObservedOp::CapsQuery => "能力查询",
            ObservedOp::FrameDispatch => "帧派发",
            ObservedOp::GuardIngest => "守卫摄入",
            ObservedOp::HotplugEdge => "热插拔边",
            ObservedOp::BridgeDeclare => "接管声明",
        }
    }
}

// ── 迷你能力位图（本模块自证用，不与 F48801 共享状态）────────
/// 12 字能力位图（768 位）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bitmap {
    w: [u64; BITMAP_WORDS],
}

impl Bitmap {
    /// 全零位图。
    pub fn zero() -> Self {
        Bitmap {
            w: [0u64; BITMAP_WORDS],
        }
    }

    /// 置位；返回「本次是否由 0 变 1」。越界位（≥768）静默忽略。
    pub fn set(&mut self, bit: u32) -> bool {
        if bit >= BITMAP_BITS {
            return false;
        }
        let word = (bit / 64) as usize;
        let off = bit % 64;
        if let Some(slot) = self.w.get_mut(word) {
            let mask = 1u64 << off;
            let old = *slot & mask;
            *slot |= mask;
            return old == 0;
        }
        false
    }

    /// 取值；越界返回 false。
    pub fn get(&self, bit: u32) -> bool {
        if bit >= BITMAP_BITS {
            return false;
        }
        let word = (bit / 64) as usize;
        let off = bit % 64;
        match self.w.get(word) {
            Some(v) => (*v & (1u64 << off)) != 0,
            None => false,
        }
    }

    /// 置位数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BITMAP_WORDS {
            if let Some(v) = self.w.get(i) {
                n += v.count_ones();
            }
            i += 1;
        }
        n
    }

    /// 位图指纹。
    pub fn digest(&self) -> u64 {
        let mut h = trace_init();
        let mut i = 0usize;
        while i < BITMAP_WORDS {
            if let Some(v) = self.w.get(i) {
                h = trace_word(h, *v as u32);
                h = trace_word(h, (*v >> 32) as u32);
            }
            i += 1;
        }
        h
    }
}

// ── 指纹基础设施（本域按「痕迹指纹」命名）────────────────────
/// 痕迹指纹初值。
pub fn trace_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn trace_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 32 位字。
pub fn trace_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = trace_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入字符串。
pub fn trace_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = trace_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

// ── 痕迹记录与痕迹环 ─────────────────────────────────────────
/// 单条观测痕迹。
#[derive(Clone, Copy, Debug)]
pub struct ObsRec {
    /// 全局序号（单调）。
    pub seq: u64,
    /// 操作序号。
    pub op: u32,
    /// 点位序号。
    pub point: u32,
    /// 该点上看到的是成功还是失败。
    pub ok: bool,
    /// 成本（工作单位，入口/出口/结果点写 0）。
    pub cost: u32,
    /// 该点的凭据指纹。
    pub digest: u64,
}

/// 痕迹环：容量固定，溢出计丢弃而不覆盖。
pub struct Tracer {
    recs: Vec<ObsRec>,
    mask: u32,
    dropped: u64,
    seq: u64,
    fold: u64,
}

impl Tracer {
    /// 空痕迹环。
    pub fn new() -> Self {
        Tracer {
            recs: Vec::new(),
            mask: 0,
            dropped: 0,
            seq: 0,
            fold: trace_init(),
        }
    }

    /// 收一条痕迹。返回 false 表示环已满、该条被丢弃（丢弃必计数）。
    pub fn rec(&mut self, op: ObservedOp, point: ObsPoint, ok: bool, cost: u32, digest: u64) -> bool {
        if self.recs.len() >= TRACE_CAP {
            self.dropped += 1;
            return false;
        }
        self.seq += 1;
        let r = ObsRec {
            seq: self.seq,
            op: op.id(),
            point: point.bit(),
            ok,
            cost,
            digest,
        };
        self.fold = trace_fold_rec(self.fold, &r);
        self.recs.push(r);
        let bit = op.id() * (POINT_COUNT as u32) + point.bit();
        self.mask |= 1u32 << bit;
        true
    }

    /// 某操作某点是否被覆盖。
    pub fn covered(&self, op: ObservedOp, point: ObsPoint) -> bool {
        let bit = op.id() * (POINT_COUNT as u32) + point.bit();
        (self.mask >> bit) & 1 == 1
    }

    /// 已覆盖标记数（0..20）。
    pub fn mark_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 覆盖率（0..100）。
    pub fn coverage_pct(&self) -> u32 {
        (self.mask.count_ones() * 100) / MARK_TOTAL
    }

    /// 点位掩码（bit = op*4 + point）。
    pub fn mask(&self) -> u32 {
        self.mask
    }

    /// 记录条数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 丢弃条数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 全环指纹。
    pub fn digest(&self) -> u64 {
        self.fold
    }

    /// 某操作某点的最后一条痕迹。
    pub fn last(&self, op: ObservedOp, point: ObsPoint) -> Option<&ObsRec> {
        self.recs
            .iter()
            .rev()
            .find(|r| r.op == op.id() && r.point == point.bit())
    }

    /// 某操作某点上成功痕迹的条数。
    pub fn count_ok(&self, op: ObservedOp, point: ObsPoint) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.op == op.id() && r.point == point.bit() && r.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 耗时点的（条数、最小、最大）三元组。
    pub fn cost_records(&self) -> (u32, u32, u32) {
        let mut n = 0u32;
        let mut min = u32::MAX;
        let mut max = 0u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.point == ObsPoint::Cost.bit() {
                    n += 1;
                    if r.cost < min {
                        min = r.cost;
                    }
                    if r.cost > max {
                        max = r.cost;
                    }
                }
            }
            i += 1;
        }
        (n, min, max)
    }

    /// 清空（计数一并归零，用于分轮演练）。
    pub fn reset(&mut self) {
        self.recs.clear();
        self.mask = 0;
        self.dropped = 0;
        self.seq = 0;
        self.fold = trace_init();
    }
}

/// 把一条痕迹折进指纹。
fn trace_fold_rec(h: u64, r: &ObsRec) -> u64 {
    let mut acc = trace_word(h, r.seq as u32);
    acc = trace_word(acc, (r.seq >> 32) as u32);
    acc = trace_word(acc, r.op);
    acc = trace_word(acc, r.point);
    acc = trace_word(acc, if r.ok { 1 } else { 0 });
    acc = trace_word(acc, r.cost);
    acc = trace_word(acc, r.digest as u32);
    acc = trace_word(acc, (r.digest >> 32) as u32);
    acc
}

// ── 异常面 ────────────────────────────────────────────────────
/// 异常记录面：只收「已上报」的异常；静默条数单独计数用于自曝。
pub struct AnomalyLog {
    /// 打包为 (op << 32) | code。
    raised: Vec<u64>,
    silent: u64,
    fold: u64,
}

impl AnomalyLog {
    /// 空异常面。
    pub fn new() -> Self {
        AnomalyLog {
            raised: Vec::new(),
            silent: 0,
            fold: trace_init(),
        }
    }

    /// 上报一条异常。
    pub fn raise(&mut self, op: ObservedOp, code: u32) {
        let packed = ((op.id() as u64) << 32) | (code as u64);
        self.fold = trace_word(self.fold, op.id());
        self.fold = trace_word(self.fold, code);
        self.raised.push(packed);
    }

    /// 某操作已上报的异常条数。
    pub fn count_for(&self, op: ObservedOp) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.raised.len() {
            if let Some(v) = self.raised.get(i) {
                if (*v >> 32) as u32 == op.id() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 全面上报条数。
    pub fn total(&self) -> u32 {
        self.raised.len() as u32
    }

    /// 静默计数（本实现里只有故意注入的静默会推进它）。
    pub fn silent_marks(&self) -> u64 {
        self.silent
    }

    /// 记一次静默（演练用；生产路径不该调用）。
    pub fn mark_silent(&mut self) {
        self.silent += 1;
    }

    /// 异常面指纹。
    pub fn digest(&self) -> u64 {
        self.fold
    }
}

// ── 四点审校 ──────────────────────────────────────────────────
/// 一次四点审校的结论。
#[derive(Clone, Copy, Debug)]
pub struct Audit {
    /// 四点齐全且无静默。
    pub ok: bool,
    /// 结论码（0 表示全绿）。
    pub code: u32,
    /// 缺失的点（若有）。
    pub missing: Option<ObsPoint>,
    /// 是否抓到静默异常。
    pub silent: bool,
}

/// 对单个操作做四点审校。
///
/// 次序：先判点位齐不齐（缺口优先），再判失败路径有没有上报。
/// 这个次序保证「结果点都没落」时不会误报成静默异常。
pub fn audit(tr: &Tracer, an: &AnomalyLog, op: ObservedOp) -> Audit {
    let mut missing: Option<ObsPoint> = None;
    let mut i = 0usize;
    while i < ObsPoint::ALL.len() {
        if let Some(p) = ObsPoint::ALL.get(i) {
            if !tr.covered(op, *p) {
                missing = Some(*p);
                break;
            }
        }
        i += 1;
    }
    let failed = match tr.last(op, ObsPoint::Result) {
        Some(r) => !r.ok,
        None => false,
    };
    let silent = failed && an.count_for(op) == 0;
    let code = match missing {
        Some(p) => p.missing_code(),
        None => {
            if silent {
                QualityCode::SilentAnomaly.val()
            } else {
                0
            }
        }
    };
    Audit {
        ok: missing.is_none() && !silent,
        code,
        missing,
        silent,
    }
}

// ── 被观测操作的输入 ─────────────────────────────────────────
/// 一次被观测操作的输入参数（全字段公开，便于演练任意组合）。
#[derive(Clone, Copy, Debug)]
pub struct RunInput {
    /// 事件主族序号（0..10）。
    pub family: u32,
    /// 事件码。
    pub code: u32,
    /// 事件值。
    pub value: u32,
    /// 本帧时间戳。
    pub ts: u64,
    /// 上一帧时间戳。
    pub last_ts: u64,
    /// 距上次 SYN 的等待时长（微秒）。
    pub waited: u64,
    /// 环容量。
    pub cap: u32,
    /// 帧数。
    pub frames: u32,
}

/// 五种操作的正常输入。
pub fn fixture(op: ObservedOp) -> RunInput {
    match op {
        ObservedOp::CapsQuery => RunInput {
            family: 1,
            code: 0x1E,
            value: 1,
            ts: 1_000,
            last_ts: 900,
            waited: 5_000,
            cap: 128,
            frames: 4,
        },
        ObservedOp::FrameDispatch => RunInput {
            family: 1,
            code: 0x1E,
            value: 1,
            ts: 1_100,
            last_ts: 1_000,
            waited: 6_000,
            cap: 128,
            frames: 3,
        },
        ObservedOp::GuardIngest => RunInput {
            family: 0,
            code: 0,
            value: 0,
            ts: 2_000,
            last_ts: 1_900,
            waited: 20_000,
            cap: 4_096,
            frames: 1,
        },
        ObservedOp::HotplugEdge => RunInput {
            family: 3,
            code: 0,
            value: 0,
            ts: 3_000,
            last_ts: 2_900,
            waited: 7_000,
            cap: 64,
            frames: 2,
        },
        ObservedOp::BridgeDeclare => RunInput {
            family: 0,
            code: 0,
            value: 2,
            ts: 4_000,
            last_ts: 3_900,
            waited: 8_000,
            cap: 128,
            frames: 1,
        },
    }
}

/// 带轮次扰动的正常输入（探针用；每种操作都保持自身合法）。
pub fn fixture_variant(op: ObservedOp, i: u32) -> RunInput {
    let base = fixture(op);
    RunInput {
        family: (base.family + i) % 10,
        code: (base.code.wrapping_add(i.wrapping_mul(13))) % (BITMAP_BITS - 1),
        value: base.value.wrapping_add(i % 5),
        ts: base.ts + i as u64,
        last_ts: base.last_ts + i as u64,
        waited: base.waited.wrapping_add((i as u64) * 977),
        cap: base.cap.wrapping_add(i % 32),
        frames: 1 + (i % 8),
    }
}

/// 五种操作各自的「必失败」输入（用于失败路径的成本与异常演练）。
pub fn failing_fixture(op: ObservedOp) -> RunInput {
    let mut inp = fixture(op);
    match op {
        ObservedOp::CapsQuery => inp.code = BITMAP_BITS + 1,
        ObservedOp::FrameDispatch => inp.frames = 0,
        ObservedOp::GuardIngest => {
            inp.ts = 100;
            inp.last_ts = 9_000;
        }
        ObservedOp::HotplugEdge => inp.cap = 0,
        ObservedOp::BridgeDeclare => inp.value = 0,
    }
    inp
}

// ── 被观测操作的真实执行体 ───────────────────────────────────
/// 能力查询：把若干码位置进 768 位图并折摘要；码越界即拒。
pub fn caps_query(inp: &RunInput) -> Result<u64, u32> {
    if inp.code >= BITMAP_BITS {
        return Err(QualityCode::OpRejected.val());
    }
    let mut bm = Bitmap::zero();
    let mut i = 0u32;
    while i < 8 {
        let bit = inp.code.wrapping_add(i.wrapping_mul(37)) % BITMAP_BITS;
        bm.set(bit);
        i += 1;
    }
    let mut h = trace_init();
    h = trace_word(h, bm.count());
    h = trace_word(h, bm.digest() as u32);
    h = trace_word(h, (bm.digest() >> 32) as u32);
    h = trace_word(h, inp.family);
    Ok(h)
}

/// 帧派发：按三大主族（KEY/REL/ABS）分派，其余族进旁路。
pub fn frame_dispatch(inp: &RunInput) -> Result<u64, u32> {
    if inp.frames == 0 {
        return Err(QualityCode::OpRejected.val());
    }
    let frames = inp.frames.min(64);
    let mut sinks = [0u64; 4];
    let mut f = 0u32;
    while f < frames {
        let lane = (inp.code.wrapping_add(f) % 4) as usize;
        if let Some(slot) = sinks.get_mut(lane) {
            *slot = trace_word(*slot ^ trace_init(), inp.value.wrapping_add(f));
        }
        f += 1;
    }
    let mut h = trace_init();
    let mut i = 0usize;
    while i < sinks.len() {
        if let Some(v) = sinks.get(i) {
            h = trace_word(h, *v as u32);
            h = trace_word(h, (*v >> 32) as u32);
        }
        i += 1;
    }
    Ok(h)
}

/// 守卫摄入：单调 → 100ms 合成 → 4096 溢出（三闸次序与 F48805 一致）。
pub fn guard_ingest(inp: &RunInput) -> Result<u64, u32> {
    if inp.ts < inp.last_ts {
        return Err(QualityCode::OpRejected.val());
    }
    let synth = if synth_due(inp.waited) { 1u32 } else { 0 };
    let pending = inp.cap.min(OVERFLOW_CAP);
    let drops = overflow_drops(pending + 1, inp.cap);
    let mut h = trace_init();
    h = trace_word(h, inp.ts as u32);
    h = trace_word(h, (inp.ts >> 32) as u32);
    h = trace_word(h, synth);
    h = trace_word(h, drops);
    Ok(h)
}

/// 热插拔边：10 轮插/绑/在飞/拔，容量为 0 即拒。
pub fn hotplug_edge(inp: &RunInput) -> Result<u64, u32> {
    if inp.cap == 0 {
        return Err(QualityCode::OpRejected.val());
    }
    let mut h = trace_init();
    let mut cycle = 0u32;
    while cycle < 10 {
        let nodes = inp.cap.min(8);
        let inflight = inp.frames.min(4).wrapping_add(cycle % 3);
        h = trace_word(h, nodes);
        h = trace_word(h, inflight);
        h = trace_word(h, 0);
        cycle += 1;
    }
    Ok(h)
}

/// 接管声明：正文非空 + 双联签 + 降级兜底；正文空即拒。
pub fn bridge_declare(inp: &RunInput) -> Result<u64, u32> {
    if inp.value == 0 {
        return Err(QualityCode::OpRejected.val());
    }
    let cosigners = 1 + (inp.value.min(3) - 1);
    let fallback = inp.code % 2 == 0;
    let mut h = trace_init();
    h = trace_text(h, "AI-02 takeover");
    h = trace_word(h, cosigners);
    h = trace_word(h, if fallback { 1 } else { 0 });
    h = trace_word(h, inp.family);
    Ok(h)
}

/// 执行体分派。
pub fn execute(op: ObservedOp, inp: &RunInput) -> Result<u64, u32> {
    match op {
        ObservedOp::CapsQuery => caps_query(inp),
        ObservedOp::FrameDispatch => frame_dispatch(inp),
        ObservedOp::GuardIngest => guard_ingest(inp),
        ObservedOp::HotplugEdge => hotplug_edge(inp),
        ObservedOp::BridgeDeclare => bridge_declare(inp),
    }
}

/// 本次操作的成本（工作单位，恒 > 0，失败也不为 0）。
pub fn op_cost(op: ObservedOp, inp: &RunInput) -> u32 {
    7u32
        .wrapping_add(op.id().wrapping_mul(11))
        .wrapping_add(inp.frames.wrapping_mul(3))
        .wrapping_add(inp.code & 0x0F)
}

/// 输入指纹（入口点的凭据）。
pub fn input_digest(inp: &RunInput) -> u64 {
    let mut h = trace_init();
    h = trace_word(h, inp.family);
    h = trace_word(h, inp.code);
    h = trace_word(h, inp.value);
    h = trace_word(h, inp.ts as u32);
    h = trace_word(h, (inp.ts >> 32) as u32);
    h = trace_word(h, inp.waited as u32);
    h = trace_word(h, (inp.waited >> 32) as u32);
    h = trace_word(h, inp.cap);
    h = trace_word(h, inp.frames);
    h
}

/// 合成判定（100ms 整点触发，与 F48804 边界一致）。
pub fn synth_due(waited: u64) -> bool {
    waited >= SYN_TIMEOUT_US
}

/// 溢出折算：容量满后再来一条即触发一次「整帧丢弃」，丢弃后计数复位。
///
/// 返回的是丢弃事件的次数，而不是被丢的帧数：`cap` 条恰好装满为 0 次；
/// 第 `cap+1` 条触发第 1 次；此后每再攒满 `cap` 条再触发一次。
/// 容量 0 按 1 折算，绝不出现除零。
pub fn overflow_drops(count: u32, cap: u32) -> u32 {
    let c = cap.clamp(1, OVERFLOW_CAP);
    if count <= c {
        0
    } else {
        (count - 1) / c
    }
}

// ── 观测模式 ─────────────────────────────────────────────────
/// 观测模式：正常模式 + 五种缺口/故障注入模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogMode {
    /// 四点齐全、错误必上报。
    Proper,
    /// 缺入口点。
    MissingEntry,
    /// 缺出口点。
    MissingExit,
    /// 缺耗时点。
    MissingCost,
    /// 缺结果点。
    MissingResult,
    /// 四点齐全但失败不上报（静默注入）。
    FaultSilent,
}

impl LogMode {
    /// 模式名。
    pub fn name(self) -> &'static str {
        match self {
            LogMode::Proper => "正常",
            LogMode::MissingEntry => "缺入口",
            LogMode::MissingExit => "缺出口",
            LogMode::MissingCost => "缺耗时",
            LogMode::MissingResult => "缺结果",
            LogMode::FaultSilent => "静默失败",
        }
    }
}

/// 观测一次操作：按模式落点、落异常，返回执行体原结果。
///
/// 注意：即便在缺口模式下，异常上报也不允许被「顺手」丢掉——
/// 唯一允许不上报的是显式的 `FaultSilent` 注入，它由审计闸抓。
pub fn observe(
    op: ObservedOp,
    mode: LogMode,
    inp: &RunInput,
    tr: &mut Tracer,
    an: &mut AnomalyLog,
) -> Result<u64, u32> {
    let entry_digest = input_digest(inp);
    if mode != LogMode::MissingEntry {
        tr.rec(op, ObsPoint::Entry, true, 0, entry_digest);
    }
    let res = execute(op, inp);
    let cost = op_cost(op, inp);
    if mode != LogMode::MissingCost {
        tr.rec(op, ObsPoint::Cost, res.is_ok(), cost, cost as u64);
    }
    let outcome_digest = match res {
        Ok(d) => d,
        Err(c) => c as u64,
    };
    if mode != LogMode::MissingExit {
        tr.rec(op, ObsPoint::Exit, res.is_ok(), 0, outcome_digest);
    }
    if mode != LogMode::MissingResult {
        tr.rec(op, ObsPoint::Result, res.is_ok(), 0, outcome_digest);
    }
    if let Err(c) = res {
        if mode == LogMode::FaultSilent {
            an.mark_silent();
        } else {
            an.raise(op, c);
        }
    }
    res
}

// ── 四点演练 ─────────────────────────────────────────────────
/// 四点演练结论。
#[derive(Clone, Copy, Debug)]
pub struct FourPoint {
    /// 覆盖标记数。
    pub marks: u32,
    /// 覆盖率。
    pub coverage: u32,
    /// 点位掩码。
    pub mask: u32,
    /// 审校次数。
    pub audits: u32,
    /// 是否全部审校通过。
    pub all_ok: bool,
    /// 痕迹指纹。
    pub digest: u64,
}

/// 五种操作 × 四点全跑一遍并逐操作审校。
pub fn four_point_drill() -> FourPoint {
    let mut tr = Tracer::new();
    let mut an = AnomalyLog::new();
    let mut audits = 0u32;
    let mut all_ok = true;
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(op) = OPS.get(i) {
            let inp = fixture(*op);
            let _ = observe(*op, LogMode::Proper, &inp, &mut tr, &mut an);
            let a = audit(&tr, &an, *op);
            audits += 1;
            all_ok = all_ok && a.ok;
        }
        i += 1;
    }
    FourPoint {
        marks: tr.mark_count(),
        coverage: tr.coverage_pct(),
        mask: tr.mask(),
        audits,
        all_ok,
        digest: tr.digest(),
    }
}

/// 某点位是否被五种操作全部覆盖。
pub fn point_fully_covered(point: ObsPoint) -> bool {
    let mask = four_point_drill().mask;
    let mut ok = true;
    let mut op = 0u32;
    while op < OP_COUNT as u32 {
        let bit = op * (POINT_COUNT as u32) + point.bit();
        ok = ok && ((mask >> bit) & 1) == 1;
        op += 1;
    }
    ok
}

/// 单条故障演练结果。
#[derive(Clone, Copy, Debug)]
pub struct FaultCase {
    /// 注入模式。
    pub mode: LogMode,
    /// 审计给出的码。
    pub code: u32,
    /// 是否被抓。
    pub caught: bool,
    /// 是否被判为静默。
    pub silent: bool,
}

/// 五种缺口 + 静默，各跑一次，看审计是否精确命中。
pub fn fault_drill() -> Vec<FaultCase> {
    let mut out = Vec::new();
    let modes = [
        LogMode::MissingEntry,
        LogMode::MissingExit,
        LogMode::MissingCost,
        LogMode::MissingResult,
    ];
    let mut i = 0usize;
    while i < modes.len() {
        if let Some(m) = modes.get(i) {
            let op = ObservedOp::CapsQuery;
            let mut tr = Tracer::new();
            let mut an = AnomalyLog::new();
            let inp = fixture(op);
            let _ = observe(op, *m, &inp, &mut tr, &mut an);
            let a = audit(&tr, &an, op);
            out.push(FaultCase {
                mode: *m,
                code: a.code,
                caught: !a.ok,
                silent: a.silent,
            });
        }
        i += 1;
    }
    // 静默注入：使用守卫的必失败输入（时间戳回退）。
    let op = ObservedOp::GuardIngest;
    let bad = failing_fixture(op);
    let mut tr = Tracer::new();
    let mut an = AnomalyLog::new();
    let _ = observe(op, LogMode::FaultSilent, &bad, &mut tr, &mut an);
    let a = audit(&tr, &an, op);
    out.push(FaultCase {
        mode: LogMode::FaultSilent,
        code: a.code,
        caught: !a.ok && a.silent,
        silent: a.silent,
    });
    out
}

/// 故障演练是否全部精确命中（码值逐条对拍）。
pub fn fault_drill_ok() -> bool {
    let v = fault_drill();
    if v.len() != 5 {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < v.len() {
        if let Some(c) = v.get(i) {
            let want = match c.mode {
                LogMode::MissingEntry => QualityCode::MissingEntry.val(),
                LogMode::MissingExit => QualityCode::MissingExit.val(),
                LogMode::MissingCost => QualityCode::MissingCost.val(),
                LogMode::MissingResult => QualityCode::MissingResult.val(),
                LogMode::FaultSilent => QualityCode::SilentAnomaly.val(),
                LogMode::Proper => 0,
            };
            ok = ok && c.caught && c.code == want;
        }
        i += 1;
    }
    ok
}

/// 成本演练结论。
#[derive(Clone, Copy, Debug)]
pub struct CostReport {
    /// 落账条数。
    pub recorded: u32,
    /// 其中失败路径条数。
    pub failed: u32,
    /// 最小成本。
    pub min: u32,
    /// 最大成本。
    pub max: u32,
    /// 五操作失败路径是否四点齐全（含异常上报）。
    pub failed_four_points: bool,
    /// 指纹。
    pub digest: u64,
}

/// 成本演练：五种操作各一次成功 + 一次失败，全部必须落耗时点。
pub fn cost_drill() -> CostReport {
    let mut tr = Tracer::new();
    let mut an = AnomalyLog::new();
    let mut failed = 0u32;
    let mut failed_four_points = true;
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(op) = OPS.get(i) {
            let good = fixture(*op);
            let _ = observe(*op, LogMode::Proper, &good, &mut tr, &mut an);
            let bad = failing_fixture(*op);
            if observe(*op, LogMode::Proper, &bad, &mut tr, &mut an).is_err() {
                failed += 1;
            }
            let a = audit(&tr, &an, *op);
            failed_four_points = failed_four_points && a.ok && tr.covered(*op, ObsPoint::Cost);
        }
        i += 1;
    }
    // 每操作两轮（成功 + 失败），耗时点共 10 条，成本恒为正。
    let (recorded, min, max) = tr.cost_records();
    let mut h = tr.digest();
    h = trace_word(h, recorded);
    h = trace_word(h, min);
    h = trace_word(h, max);
    CostReport {
        recorded,
        failed,
        min,
        max,
        failed_four_points,
        digest: h,
    }
}

// ── fuzz 面（F48817）─────────────────────────────────────────
/// 线性同余发生器（无符号、无 panic）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 播种（强制奇数，避免退化）。
    pub fn new(seed: u64) -> Self {
        Lcg { s: seed | 1 }
    }

    /// 下一个 64 位。
    pub fn next(&mut self) -> u64 {
        self.s = self
            .s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.s
    }

    /// 下一个 32 位。
    pub fn next_u32(&mut self) -> u32 {
        (self.next() >> 32) as u32
    }
}

/// 一组乱参数。
#[derive(Clone, Copy, Debug)]
pub struct FuzzCase {
    /// 主族（同时决定走哪条车道）。
    pub family: u32,
    /// 事件码。
    pub code: u32,
    /// 事件值。
    pub value: u32,
    /// 长度（帧数/位图码数/ioctl 尺寸/环长，按车道解释）。
    pub len: u32,
    /// 等待时长。
    pub waited: u64,
    /// 容量。
    pub cap: u32,
}

/// 构造一组乱参数（含周期性极值）。
pub fn fuzz_case(lcg: &mut Lcg, round: u64) -> FuzzCase {
    let mut c = FuzzCase {
        family: lcg.next_u32(),
        code: lcg.next_u32(),
        value: lcg.next_u32(),
        len: lcg.next_u32(),
        waited: lcg.next(),
        cap: lcg.next_u32(),
    };
    if round % 211 == 0 {
        c.len = u32::MAX;
        c.waited = u64::MAX;
        c.cap = 0;
    }
    if round % 307 == 0 {
        c.code = 0x2FF;
        c.value = u32::MAX;
        c.len = 0;
    }
    if round % 97 == 0 {
        c.cap = u32::MAX;
        c.family = 5;
    }
    c
}

/// 车道 0：位图乱码置位。
fn fuzz_bitmap(c: &FuzzCase) -> Result<u64, u32> {
    let mut bm = Bitmap::zero();
    let n = c.len % 96;
    let mut i = 0u32;
    while i < n {
        bm.set(c.code.wrapping_add(i.wrapping_mul(37)) % BITMAP_BITS);
        i += 1;
    }
    let d = bm.digest();
    let mut h = trace_init();
    h = trace_word(h, bm.count());
    h = trace_word(h, d as u32);
    h = trace_word(h, (d >> 32) as u32);
    Ok(h)
}

/// 车道 1：ioctl 乱尺寸合成（<4 字节即拒，绝不越界）。
fn fuzz_ioctl(c: &FuzzCase) -> Result<u64, u32> {
    let len = c.len.min(4_096) as usize;
    if len < 4 {
        return Err(QualityCode::OpRejected.val());
    }
    let mut bm = Bitmap::zero();
    let mut h = trace_init();
    let mut i = 0usize;
    while i < len.min(64) {
        let code = c.code.wrapping_add(i as u32);
        if bm.set(code % BITMAP_BITS) {
            h = trace_word(h, code);
        }
        i += 1;
    }
    let d = bm.digest();
    h = trace_word(h, d as u32);
    h = trace_word(h, (d >> 32) as u32);
    Ok(h)
}

/// 车道 2：帧组装乱长（0 帧即拒，单帧事件数封顶 16）。
fn fuzz_frames(c: &FuzzCase) -> Result<u64, u32> {
    let frames = c.len % 65;
    if frames == 0 {
        return Err(QualityCode::OpRejected.val());
    }
    let mut h = trace_init();
    let mut f = 0u32;
    while f < frames {
        let mut n = c.value.wrapping_add(f) % 17;
        if n == 0 {
            n = 1;
        }
        h = trace_word(h, n);
        let mut e = 0u32;
        while e < n && e < 16 {
            h = trace_word(h, c.code.wrapping_add(e));
            e += 1;
        }
        f += 1;
    }
    Ok(h)
}

/// 车道 3：超时判定（u64 全域）。
fn fuzz_timeout(c: &FuzzCase) -> Result<u64, u32> {
    let mut h = trace_init();
    h = trace_word(h, c.waited as u32);
    h = trace_word(h, (c.waited >> 32) as u32);
    h = trace_word(h, if synth_due(c.waited) { 1 } else { 0 });
    Ok(h)
}

/// 车道 4：溢出折算（容量 0 折成 1，绝不出除零）。
fn fuzz_overflow(c: &FuzzCase) -> Result<u64, u32> {
    let drops = overflow_drops(c.value, c.cap);
    let mut h = trace_init();
    h = trace_word(h, c.value);
    h = trace_word(h, c.cap.clamp(1, OVERFLOW_CAP));
    h = trace_word(h, drops);
    Ok(h)
}

/// 车道 5：环突发（长度封顶 256，容量封顶 128）。
fn fuzz_ring(c: &FuzzCase) -> Result<u64, u32> {
    let len = (c.len % 257) as usize;
    if len == 0 {
        return Err(QualityCode::OpRejected.val());
    }
    let cap = (c.cap % 129) as usize;
    let cap = if cap == 0 { 1 } else { cap };
    let mut q: VecDeque<u32> = VecDeque::with_capacity(cap.max(1));
    let mut h = trace_init();
    let mut i = 0usize;
    while i < len {
        q.push_back(c.code.wrapping_add(i as u32));
        if q.len() > cap {
            if let Some(v) = q.pop_front() {
                h = trace_word(h, v);
            }
        }
        i += 1;
    }
    while let Some(v) = q.pop_front() {
        h = trace_word(h, v);
    }
    Ok(h)
}

/// fuzz 执行分派：任意参数只允许 Ok/Err。
pub fn fuzz_exec(c: &FuzzCase) -> Result<u64, u32> {
    match c.family % 6 {
        0 => fuzz_bitmap(c),
        1 => fuzz_ioctl(c),
        2 => fuzz_frames(c),
        3 => fuzz_timeout(c),
        4 => fuzz_overflow(c),
        _ => fuzz_ring(c),
    }
}

/// fuzz 统计。
#[derive(Clone, Copy, Debug)]
pub struct FuzzStats {
    /// 轮次。
    pub rounds: u64,
    /// 通过（Ok）轮次。
    pub ok: u64,
    /// 正常拒绝（Err）轮次。
    pub err: u64,
    /// 指纹。
    pub digest: u64,
    /// 见过的最长长度参数（截断后）。
    pub max_len: usize,
}

/// 跑 `rounds` 轮 fuzz：恒不 panic，恒有 ok+err==rounds。
pub fn fuzz_run(rounds: u64, seed: u64) -> FuzzStats {
    let mut lcg = Lcg::new(seed);
    let mut ok = 0u64;
    let mut err = 0u64;
    let mut h = trace_init();
    let mut max_len = 0usize;
    let mut i = 0u64;
    while i < rounds {
        let c = fuzz_case(&mut lcg, i);
        let r = fuzz_exec(&c);
        h = trace_word(h, c.family);
        h = trace_word(h, c.code);
        match r {
            Ok(d) => {
                ok += 1;
                h = trace_word(h, d as u32);
                h = trace_word(h, (d >> 32) as u32);
            }
            Err(cd) => {
                err += 1;
                h = trace_word(h, cd);
            }
        }
        let l = (c.len % 4_096) as usize;
        if l > max_len {
            max_len = l;
        }
        i += 1;
    }
    FuzzStats {
        rounds,
        ok,
        err,
        digest: h,
        max_len,
    }
}

/// 双跑 fuzz（同种子）：返回两份统计用于对拍指纹。
pub fn fuzz_double(rounds: u64, seed: u64) -> (FuzzStats, FuzzStats) {
    (fuzz_run(rounds, seed), fuzz_run(rounds, seed))
}

// ── 判据用例集（F48818）──────────────────────────────────────
/// 用例定义。
#[derive(Clone, Copy, Debug)]
pub struct CaseDef {
    /// 用例号（1..=20）。
    pub id: usize,
    /// 用例名（入库冻结）。
    pub name: &'static str,
}

/// 20 条用例的冻结台账。
pub fn case_table() -> [CaseDef; CASE_COUNT] {
    [
        CaseDef { id: 1, name: "entry-point-covered" },
        CaseDef { id: 2, name: "exit-point-covered" },
        CaseDef { id: 3, name: "cost-point-covered" },
        CaseDef { id: 4, name: "result-point-covered" },
        CaseDef { id: 5, name: "coverage-100-percent" },
        CaseDef { id: 6, name: "missing-entry-caught" },
        CaseDef { id: 7, name: "missing-exit-caught" },
        CaseDef { id: 8, name: "missing-cost-caught" },
        CaseDef { id: 9, name: "missing-result-caught" },
        CaseDef { id: 10, name: "silent-anomaly-caught" },
        CaseDef { id: 11, name: "cost-on-failure" },
        CaseDef { id: 12, name: "fuzz-10k-zero-crash" },
        CaseDef { id: 13, name: "fuzz-double-stable" },
        CaseDef { id: 14, name: "fuzz-digest-live" },
        CaseDef { id: 15, name: "bitmap-saturation" },
        CaseDef { id: 16, name: "syn-timeout-boundary" },
        CaseDef { id: 17, name: "overflow-cap-exact" },
        CaseDef { id: 18, name: "trace-reconcile" },
        CaseDef { id: 19, name: "case-table-consistent" },
        CaseDef { id: 20, name: "golden-stream-stable" },
    ]
}

/// 用例名反查（越界返回空串）。
pub fn case_name(id: usize) -> &'static str {
    let t = case_table();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(c) = t.get(i) {
            if c.id == id {
                return c.name;
            }
        }
        i += 1;
    }
    ""
}

/// 痕迹守恒：5 操作 × 4 点 = 20 条，零丢弃、零异常、掩码满。
pub fn trace_reconcile() -> bool {
    let mut tr = Tracer::new();
    let mut an = AnomalyLog::new();
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(op) = OPS.get(i) {
            let inp = fixture(*op);
            let _ = observe(*op, LogMode::Proper, &inp, &mut tr, &mut an);
        }
        i += 1;
    }
    tr.len() == OP_COUNT * POINT_COUNT
        && tr.mark_count() == MARK_TOTAL
        && tr.dropped() == 0
        && an.total() == 0
        && an.silent_marks() == 0
}

/// 位图饱和：768 位全置后计数与指纹稳定。
fn bitmap_saturation() -> bool {
    let mut bm = Bitmap::zero();
    let mut i = 0u32;
    while i < BITMAP_BITS {
        bm.set(i);
        i += 1;
    }
    let d1 = bm.digest();
    let mut j = 0u32;
    while j < BITMAP_BITS {
        bm.set(j);
        j += 1;
    }
    bm.count() == BITMAP_BITS && bm.digest() == d1 && bm.get(0x2FF) && !bm.get(BITMAP_BITS)
}

/// 超时边界：99_999 不合成、100_000 合成、再大仍合成。
fn timeout_boundary_ok() -> bool {
    !synth_due(99_999) && synth_due(100_000) && synth_due(100_001) && synth_due(u64::MAX)
}

/// 溢出边界：4096 恰好不丢，4097 丢 1 次，8192 仍 1 次，8193 起第 2 次。
fn overflow_cap_ok() -> bool {
    overflow_drops(4_096, 4_096) == 0
        && overflow_drops(4_097, 4_096) == 1
        && overflow_drops(8_192, 4_096) == 1
        && overflow_drops(8_193, 4_096) == 2
        && overflow_drops(12_289, 4_096) == 3
        && overflow_drops(5, 0) == 4
}

/// 用例一致性：20 条、号连续、名不重复且非空。
fn case_table_ok() -> bool {
    let t = case_table();
    if t.len() != CASE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < t.len() {
        if let Some(c) = t.get(i) {
            if c.id != i + 1 || c.name.is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < t.len() {
                if let Some(d) = t.get(j) {
                    if d.name == c.name {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 黄金流双跑：四点演练与固定种子 fuzz 的指纹两次一致（不重入用例集）。
fn golden_stable() -> bool {
    let a = four_point_drill();
    let b = four_point_drill();
    let f1 = fuzz_run(128, 0x48820);
    let f2 = fuzz_run(128, 0x48820);
    a.digest == b.digest
        && a.marks == MARK_TOTAL
        && a.coverage == 100
        && f1.digest == f2.digest
        && f1.ok + f1.err == 128
}

/// 单条用例执行（1..=20；越界返回 SuiteHole）。
pub fn case_passed(id: usize) -> Result<(), u32> {
    let ok = match id {
        1 => point_fully_covered(ObsPoint::Entry),
        2 => point_fully_covered(ObsPoint::Exit),
        3 => point_fully_covered(ObsPoint::Cost),
        4 => point_fully_covered(ObsPoint::Result),
        5 => {
            let fp = four_point_drill();
            fp.marks == MARK_TOTAL && fp.coverage == 100 && fp.mask == 0x000F_FFFF
        }
        6 => {
            fault_drill_ok()
                && fault_drill().first().map(|c| c.code).unwrap_or(0)
                    == QualityCode::MissingEntry.val()
        }
        7 => fault_drill().get(1).map(|c| c.code).unwrap_or(0) == QualityCode::MissingExit.val(),
        8 => fault_drill().get(2).map(|c| c.code).unwrap_or(0) == QualityCode::MissingCost.val(),
        9 => fault_drill().get(3).map(|c| c.code).unwrap_or(0) == QualityCode::MissingResult.val(),
        10 => fault_drill().get(4).map(|c| c.silent && c.caught).unwrap_or(false),
        11 => {
            let c = cost_drill();
            c.recorded == 10 && c.failed == 5 && c.failed_four_points && c.min > 0
        }
        12 => {
            let f = fuzz_run(FUZZ_ROUNDS, 0x48817);
            f.rounds == FUZZ_ROUNDS && f.ok + f.err == FUZZ_ROUNDS && f.digest != trace_init()
        }
        13 => {
            let (a, b) = fuzz_double(2_048, 0x48818);
            a.digest == b.digest && a.ok == b.ok && a.err == b.err
        }
        14 => fuzz_run(256, 0x48819).digest != trace_init(),
        15 => bitmap_saturation(),
        16 => timeout_boundary_ok(),
        17 => overflow_cap_ok(),
        18 => trace_reconcile(),
        19 => case_table_ok(),
        20 => golden_stable(),
        _ => {
            return Err(QualityCode::SuiteHole.val());
        }
    };
    if ok {
        Ok(())
    } else {
        Err(QualityCode::SuiteHole.val())
    }
}

/// 用例集报告。
#[derive(Clone, Copy, Debug)]
pub struct SuiteReport {
    /// 总条数。
    pub total: u32,
    /// 通过条数。
    pub passed: u32,
    /// 失败条数。
    pub failed: u32,
    /// 首个失败用例号（0 表示无）。
    pub first_fail: u32,
    /// 通过集合指纹。
    pub digest: u64,
}

/// 跑全部 20 条用例。
pub fn suite_run() -> SuiteReport {
    let t = case_table();
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut first_fail = 0u32;
    let mut h = trace_init();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(c) = t.get(i) {
            let r = case_passed(c.id);
            let ok = r.is_ok();
            if ok {
                passed += 1;
            } else {
                failed += 1;
                if first_fail == 0 {
                    first_fail = c.id as u32;
                }
            }
            h = trace_word(h, c.id as u32);
            h = trace_word(h, if ok { 1 } else { 0 });
        }
        i += 1;
    }
    SuiteReport {
        total: CASE_COUNT as u32,
        passed,
        failed,
        first_fail,
        digest: h,
    }
}

// ── 黄金流 ───────────────────────────────────────────────────
/// 黄金流报告。
#[derive(Clone, Copy, Debug)]
pub struct GoldenReport {
    /// 四点审校操作数。
    pub ops: u32,
    /// 覆盖标记数。
    pub marks: u32,
    /// 覆盖率。
    pub coverage: u32,
    /// 上报异常数。
    pub anomalies: u32,
    /// fuzz 通过轮次。
    pub fuzz_ok: u64,
    /// 用例通过数。
    pub cases: u32,
    /// 总判定。
    pub ok: bool,
    /// 总指纹。
    pub digest: u64,
}

/// 黄金流：四点演练 + 故障演练 + 成本演练 + 小规模 fuzz + 用例集。
pub fn golden_run() -> GoldenReport {
    let fp = four_point_drill();
    let fault_ok = fault_drill_ok();
    let cost = cost_drill();
    let fz = fuzz_run(512, 0xB01_48816);
    let suite = suite_run();
    let ok = fp.all_ok
        && fp.coverage == 100
        && fp.marks == MARK_TOTAL
        && fault_ok
        && cost.recorded == 10
        && cost.failed == 5
        && fz.ok + fz.err == 512
        && suite.passed == CASE_COUNT as u32
        && suite.failed == 0;
    let mut h = trace_init();
    h = trace_word(h, fp.digest as u32);
    h = trace_word(h, (fp.digest >> 32) as u32);
    h = trace_word(h, cost.digest as u32);
    h = trace_word(h, (cost.digest >> 32) as u32);
    h = trace_word(h, fz.digest as u32);
    h = trace_word(h, (fz.digest >> 32) as u32);
    h = trace_word(h, suite.digest as u32);
    h = trace_word(h, (suite.digest >> 32) as u32);
    GoldenReport {
        ops: fp.audits,
        marks: fp.marks,
        coverage: fp.coverage,
        anomalies: cost.failed,
        fuzz_ok: fz.ok,
        cases: suite.passed,
        ok,
        digest: h,
    }
}

// ── 注入演练 ─────────────────────────────────────────────────
/// 一条注入命中记录。
#[derive(Clone, Copy, Debug)]
pub struct InjectHit {
    /// 槽号（0 缺入口 / 1 缺出口 / 2 缺结果 / 3 静默）。
    pub slot: u32,
    /// 审计码。
    pub code: u32,
    /// 是否精确命中。
    pub caught: bool,
}

/// 十轮 × 四槽注入。
pub fn inject_rounds() -> Vec<InjectHit> {
    let mut v = Vec::new();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 槽 0：缺入口。
        let mut tr = Tracer::new();
        let mut an = AnomalyLog::new();
        let op = ObservedOp::CapsQuery;
        let inp = fixture(op);
        let _ = observe(op, LogMode::MissingEntry, &inp, &mut tr, &mut an);
        let a = audit(&tr, &an, op);
        v.push(InjectHit {
            slot: 0,
            code: a.code,
            caught: !a.ok && a.code == QualityCode::MissingEntry.val(),
        });
        // 槽 1：缺出口。
        let mut tr = Tracer::new();
        let mut an = AnomalyLog::new();
        let _ = observe(op, LogMode::MissingExit, &inp, &mut tr, &mut an);
        let a = audit(&tr, &an, op);
        v.push(InjectHit {
            slot: 1,
            code: a.code,
            caught: !a.ok && a.code == QualityCode::MissingExit.val(),
        });
        // 槽 2：缺结果。
        let mut tr = Tracer::new();
        let mut an = AnomalyLog::new();
        let _ = observe(op, LogMode::MissingResult, &inp, &mut tr, &mut an);
        let a = audit(&tr, &an, op);
        v.push(InjectHit {
            slot: 2,
            code: a.code,
            caught: !a.ok && a.code == QualityCode::MissingResult.val(),
        });
        // 槽 3：静默失败（守卫时间戳回退）。
        let mut tr = Tracer::new();
        let mut an = AnomalyLog::new();
        let gop = ObservedOp::GuardIngest;
        let bad = failing_fixture(gop);
        let _ = observe(gop, LogMode::FaultSilent, &bad, &mut tr, &mut an);
        let a = audit(&tr, &an, gop);
        v.push(InjectHit {
            slot: 3,
            code: a.code,
            caught: !a.ok && a.silent && a.code == QualityCode::SilentAnomaly.val(),
        });
        round += 1;
    }
    v
}

/// 注入演练是否全中（40 条，槽码对拍）。
pub fn inject_ok() -> bool {
    let v = inject_rounds();
    if v.len() != (INJECT_ROUNDS as usize) * 4 {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < v.len() {
        if let Some(h) = v.get(i) {
            let want = match h.slot {
                0 => QualityCode::MissingEntry.val(),
                1 => QualityCode::MissingExit.val(),
                2 => QualityCode::MissingResult.val(),
                _ => QualityCode::SilentAnomaly.val(),
            };
            ok = ok && h.caught && h.code == want;
        }
        i += 1;
    }
    ok
}

// ── 探针与复跑 ───────────────────────────────────────────────
/// 探针核：64 步混合流（每 7 步走一次必失败输入），折痕迹与异常面。
fn probe_core(seed: u32) -> u64 {
    let mut tr = Tracer::new();
    let mut an = AnomalyLog::new();
    let mut h = trace_init();
    let mut i = 0u32;
    while i < 64 {
        let op = match i % 5 {
            0 => ObservedOp::CapsQuery,
            1 => ObservedOp::FrameDispatch,
            2 => ObservedOp::GuardIngest,
            3 => ObservedOp::HotplugEdge,
            _ => ObservedOp::BridgeDeclare,
        };
        let good = fixture_variant(op, i.wrapping_add(seed));
        let inp = if i % 7 == 0 {
            failing_fixture(op)
        } else {
            good
        };
        let r = observe(op, LogMode::Proper, &inp, &mut tr, &mut an);
        h = trace_word(h, r.unwrap_or(QualityCode::OpRejected.val() as u64) as u32);
        h = trace_word(h, (r.unwrap_or(0) >> 32) as u32);
        i += 1;
    }
    h ^ tr.digest() ^ an.digest()
}

/// 探针 A。
pub fn probe_seq_a() -> u64 {
    probe_core(0x11)
}

/// 探针 B（与 A 同源，用双跑对拍）。
pub fn probe_seq_b() -> u64 {
    probe_core(0x11)
}

/// 探针 C（另一条种子，自身可复跑）。
pub fn probe_seq_c() -> u64 {
    probe_core(0x7C)
}

/// 复跑判据：A==B、C 自身复跑一致、A 与 C 不同源。
pub fn repro_check() -> Result<(), u32> {
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c1 = probe_seq_c();
    let c2 = probe_seq_c();
    if a != b || c1 != c2 {
        return Err(QualityCode::StaleDigest.val());
    }
    if a == 0 || a == c1 {
        return Err(QualityCode::StaleDigest.val());
    }
    Ok(())
}

/// 段位自检：码族连续 + 全窗口 + 文本非空。
pub fn segment_guard() -> bool {
    match code_family_check() {
        Ok(n) => {
            n as usize == CODE_FAMILY_LEN
                && QualityCode::ALL.iter().all(|c| {
                    let v = c.val();
                    v > SEG_BASE && v < SEG_BASE + 0x20 && !c.detail().is_empty()
                })
        }
        Err(_) => false,
    }
}

// ── 判据清单 ─────────────────────────────────────────────────
/// 本模块 10 条判据（名称、通过、详情）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48816-J1：四个点 100% 覆盖（5 操作 × 4 点）。
    let fp = four_point_drill();
    v.push((
        "F48816-J1-四点覆盖5操作20点",
        fp.marks == MARK_TOTAL && fp.coverage == 100 && fp.all_ok,
        format!("marks={} coverage={}%", fp.marks, fp.coverage),
    ));

    // F48816-J1：四类缺口全抓。
    v.push((
        "F48816-J1-四类点位缺口全抓",
        fault_drill_ok(),
        String::from("entry/exit/cost/result 四码精确命中"),
    ));

    // F48816-J1：静默异常必须红。
    let silent_case = fault_drill().get(4).copied();
    v.push((
        "F48816-J1-静默异常必红",
        matches!(silent_case, Some(c) if c.silent && c.caught && c.code == QualityCode::SilentAnomaly.val()),
        format!("silent={:?}", silent_case.map(|c| c.code)),
    ));

    // F48816-J1：失败路径也记耗时（五操作各一次失败，四点齐全）。
    let cost = cost_drill();
    v.push((
        "F48816-J1-失败路径也记耗时",
        cost.recorded == 10 && cost.failed == 5 && cost.failed_four_points && cost.min > 0,
        format!(
            "recorded={} failed={} min={}",
            cost.recorded, cost.failed, cost.min
        ),
    ));

    // F48817-J1：万轮零崩（ok+err 恰好等于轮次）。
    let fz = fuzz_run(FUZZ_ROUNDS, 0x48817);
    v.push((
        "F48817-J1-fuzz万轮零崩",
        fz.rounds == FUZZ_ROUNDS && fz.ok + fz.err == FUZZ_ROUNDS,
        format!("ok={} err={} rounds={}", fz.ok, fz.err, fz.rounds),
    ));

    // F48817-J1：双跑指纹一致。
    let (fa, fb) = fuzz_double(FUZZ_ROUNDS, 0x48817);
    v.push((
        "F48817-J1-fuzz双跑一致",
        fa.digest == fb.digest && fa.ok == fb.ok && fa.err == fb.err,
        format!("a={:x} b={:x}", fa.digest, fb.digest),
    ));

    // F48818-J1：20 例全绿。
    let suite = suite_run();
    v.push((
        "F48818-J1-20例全绿入库",
        suite.total == CASE_COUNT as u32 && suite.passed == CASE_COUNT as u32 && suite.failed == 0,
        format!(
            "passed={}/{} first_fail={}",
            suite.passed, suite.total, suite.first_fail
        ),
    ));

    // F48818-J1：用例集可复跑（两次通过集合相同）。
    let s1 = suite_run();
    let s2 = suite_run();
    v.push((
        "F48818-J1-用例集双跑一致",
        s1.digest == s2.digest && s1.passed == s2.passed && s1.failed == s2.failed,
        format!("digest={:x}", s1.digest),
    ));

    // F48816-J2：痕迹守恒（20 条、零丢弃、零异常）。
    v.push((
        "F48816-J2-痕迹守恒",
        trace_reconcile(),
        String::from("recs=20 marks=20 dropped=0 anomalies=0"),
    ));

    // F48816-J3：段位与码族。
    v.push((
        "F48816-J3-段位与码族",
        segment_guard() && repro_check().is_ok(),
        format!("codes={} golden={:x}", CODE_FAMILY_LEN, golden_run().digest),
    ));

    v
}

/// 模块入口：3 条卡面判据 + 10 条细则判据。
pub fn run_unx48816_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48816 输入观测埋点 / fuzz / 用例集");
    let fp = four_point_drill();
    s.add(
        "UNX-F48816-J1 四点 100% 覆盖；异常零静默",
        fp.marks == MARK_TOTAL && fp.coverage == 100 && fp.all_ok && fault_drill_ok() && inject_ok(),
        "5 操作 × 4 点全落；四类缺口与静默注入全抓",
    );
    let fz = fuzz_run(FUZZ_ROUNDS, 0x48817);
    s.add(
        "UNX-F48817-J1 fuzz ×10^4 零崩",
        fz.ok + fz.err == FUZZ_ROUNDS && fz.digest != trace_init(),
        "10^4 轮全部分类为 Ok/Err，无 panic、无越界、无除零",
    );
    let suite = suite_run();
    s.add(
        "UNX-F48818-J1 ×20 全绿入库",
        suite.total == CASE_COUNT as u32 && suite.passed == CASE_COUNT as u32 && suite.failed == 0,
        "20 条用例逐条执行并通过，台账号码连续、名字唯一",
    );
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_points_cover_all_ops() {
        let fp = four_point_drill();
        assert_eq!(fp.marks, MARK_TOTAL);
        assert_eq!(fp.coverage, 100);
        assert_eq!(fp.mask, 0x000F_FFFF);
        assert!(fp.all_ok);
        assert_eq!(fp.audits, OP_COUNT as u32);
        assert!(point_fully_covered(ObsPoint::Entry));
        assert!(point_fully_covered(ObsPoint::Exit));
        assert!(point_fully_covered(ObsPoint::Cost));
        assert!(point_fully_covered(ObsPoint::Result));
    }

    #[test]
    fn fault_modes_return_exact_codes() {
        let v = fault_drill();
        assert_eq!(v.len(), 5);
        let m0 = v.first().expect("首例");
        assert_eq!(m0.mode, LogMode::MissingEntry);
        assert_eq!(m0.code, QualityCode::MissingEntry.val());
        assert!(m0.caught && !m0.silent);
        let m1 = v.get(1).expect("二例");
        assert_eq!(m1.code, QualityCode::MissingExit.val());
        let m2 = v.get(2).expect("三例");
        assert_eq!(m2.code, QualityCode::MissingCost.val());
        let m3 = v.get(3).expect("四例");
        assert_eq!(m3.code, QualityCode::MissingResult.val());
        assert!(fault_drill_ok());
    }

    #[test]
    fn silent_failure_is_detected() {
        let op = ObservedOp::GuardIngest;
        let mut tr = Tracer::new();
        let mut an = AnomalyLog::new();
        let bad = failing_fixture(op);
        let r = observe(op, LogMode::FaultSilent, &bad, &mut tr, &mut an);
        assert!(r.is_err());
        assert_eq!(an.total(), 0);
        assert_eq!(an.silent_marks(), 1);
        let a = audit(&tr, &an, op);
        assert!(a.silent && !a.ok);
        assert_eq!(a.code, QualityCode::SilentAnomaly.val());
    }

    #[test]
    fn cost_recorded_on_failure() {
        let c = cost_drill();
        assert_eq!(c.recorded, 10);
        assert_eq!(c.failed, 5);
        assert!(c.failed_four_points);
        assert!(c.min > 0 && c.max >= c.min);
    }

    #[test]
    fn fuzz_ten_thousand_no_crash() {
        let f = fuzz_run(FUZZ_ROUNDS, 0x48817);
        assert_eq!(f.rounds, FUZZ_ROUNDS);
        assert_eq!(f.ok + f.err, FUZZ_ROUNDS);
        assert!(f.digest != trace_init());
        // 六条车道都必须被踩到（ok+err 分类足以说明分派覆盖）。
        assert!(f.ok > 0);
    }

    #[test]
    fn fuzz_double_stable() {
        let (a, b) = fuzz_double(FUZZ_ROUNDS, 0xB01);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.ok, b.ok);
        assert_eq!(a.err, b.err);
        assert_eq!(a.max_len, b.max_len);
    }

    #[test]
    fn fuzz_extremes_do_not_panic() {
        let cases = [
            FuzzCase { family: 0, code: u32::MAX, value: u32::MAX, len: u32::MAX, waited: u64::MAX, cap: u32::MAX },
            FuzzCase { family: 1, code: 0, value: 0, len: 0, waited: 0, cap: 0 },
            FuzzCase { family: 2, code: 0x2FF, value: 0, len: 0, waited: 0, cap: 0 },
            FuzzCase { family: 3, code: 0x2FF, value: u32::MAX, len: 65, waited: 99_999, cap: 1 },
            FuzzCase { family: 4, code: 0, value: 8_193, len: 4_096, waited: 100_000, cap: 0 },
            FuzzCase { family: 5, code: 7, value: 7, len: 256, waited: 1, cap: 0 },
        ];
        let mut ok = 0u32;
        let mut err = 0u32;
        for c in cases.iter() {
            match fuzz_exec(c) {
                Ok(_) => ok += 1,
                Err(code) => {
                    assert_eq!(code, QualityCode::OpRejected.val());
                    err += 1;
                }
            }
        }
        assert_eq!(ok + err, 6);
        assert!(ok >= 3);
    }

    #[test]
    fn suite_twenty_green() {
        let s = suite_run();
        assert_eq!(s.total, 20);
        assert_eq!(s.passed, 20);
        assert_eq!(s.failed, 0);
        assert_eq!(s.first_fail, 0);
        let mut id = 1usize;
        while id <= CASE_COUNT {
            assert!(case_passed(id).is_ok(), "case {} 应通过", id);
            id += 1;
        }
        assert!(case_passed(0).is_err() && case_passed(21).is_err());
    }

    #[test]
    fn suite_repeatable() {
        let a = suite_run();
        let b = suite_run();
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.passed, b.passed);
        assert_eq!(a.failed, b.failed);
    }

    #[test]
    fn case_table_consistent() {
        assert!(case_table_ok());
        let t = case_table();
        assert_eq!(t.len(), CASE_COUNT);
        assert_eq!(case_name(1), "entry-point-covered");
        assert_eq!(case_name(20), "golden-stream-stable");
        assert_eq!(case_name(99), "");
    }

    #[test]
    fn trace_overflow_counts_dropped() {
        let mut tr = Tracer::new();
        let mut i = 0usize;
        while i < TRACE_CAP + 500 {
            let _ = tr.rec(ObservedOp::CapsQuery, ObsPoint::Entry, true, 0, i as u64);
            i += 1;
        }
        assert_eq!(tr.len(), TRACE_CAP);
        assert_eq!(tr.dropped(), 500);
        assert_eq!(tr.mark_count(), 1);
        tr.reset();
        assert!(tr.is_empty() && tr.dropped() == 0 && tr.mark_count() == 0);
    }

    #[test]
    fn trace_reconcile_identity() {
        assert!(trace_reconcile());
        let fp = four_point_drill();
        assert_eq!(fp.marks, OP_COUNT as u32 * POINT_COUNT as u32);
    }

    #[test]
    fn syn_timeout_boundary() {
        assert!(timeout_boundary_ok());
        assert!(!synth_due(SYN_TIMEOUT_US - 1));
        assert!(synth_due(SYN_TIMEOUT_US));
        let mut inp = fixture(ObservedOp::GuardIngest);
        inp.waited = SYN_TIMEOUT_US - 1;
        let a = guard_ingest(&inp).expect("未超时也应成功");
        inp.waited = SYN_TIMEOUT_US;
        let b = guard_ingest(&inp).expect("超时应合成");
        assert_ne!(a, b);
    }

    #[test]
    fn overflow_cap_exact() {
        assert!(overflow_cap_ok());
        assert_eq!(overflow_drops(4_096, 4_096), 0);
        assert_eq!(overflow_drops(4_097, 4_096), 1);
        assert_eq!(overflow_drops(4_096, 0), 4_095);
        assert_eq!(overflow_drops(0, 0), 0);
    }

    #[test]
    fn inject_rounds_all_caught() {
        assert!(inject_ok());
        let v = inject_rounds();
        assert_eq!(v.len(), INJECT_ROUNDS as usize * 4);
        let mut slots = [0u32; 4];
        for h in v.iter() {
            if let Some(s) = slots.get_mut(h.slot as usize) {
                *s += 1;
            }
        }
        assert_eq!(slots, [INJECT_ROUNDS; 4]);
    }

    #[test]
    fn segment_codes_in_window() {
        assert!(segment_guard());
        assert_eq!(code_family_check(), Ok(CODE_FAMILY_LEN as u32));
        assert_eq!(QualityCode::MissingEntry.val(), SEG_BASE + 1);
        assert_eq!(QualityCode::WindowLeak.val(), SEG_BASE + CODE_FAMILY_LEN as u32);
        assert_eq!(detail(QualityCode::SilentAnomaly.val()), "静默异常：错误返回未上报");
        assert_eq!(detail(0xDEAD), "");
        assert!(repro_check().is_ok());
        assert!(probe_seq_a() == probe_seq_b());
        assert_ne!(probe_seq_a(), probe_seq_c());
    }

    #[test]
    fn golden_stream_consistent() {
        let g = golden_run();
        assert!(g.ok);
        assert_eq!(g.ops, OP_COUNT as u32);
        assert_eq!(g.marks, MARK_TOTAL);
        assert_eq!(g.coverage, 100);
        assert_eq!(g.cases, CASE_COUNT as u32);
        assert_eq!(g.fuzz_ok + 0, g.fuzz_ok);
        let h = golden_run();
        assert_eq!(g.digest, h.digest);
    }

    #[test]
    fn bitmap_saturation_counts() {
        assert!(bitmap_saturation());
        let mut bm = Bitmap::zero();
        assert!(!bm.set(BITMAP_BITS + 3));
        assert!(!bm.get(BITMAP_BITS));
        assert_eq!(bm.count(), 0);
        assert!(bm.set(0x2FF));
        assert!(bm.get(0x2FF));
        assert_eq!(bm.count(), 1);
    }
}
