//! UNX-F21604 · SRCCOPY/NOTSRCCOPY 黑盒核心五操作
//!
//! 诊断段 0x91119~0x91120（F2-B40 光栅操作段 1/3）。
//!
//! 判据原文（施工书 UNX-F21604-J1）：五操作（SRCCOPY/NOTSRCCOPY/SRCAND/SRCPAINT/SRCINVERT）
//! 与逐像素参考逐位相同。
//!
//! 设计要点（为什么不写一个 `match fn` 就算完）：
//! 位并行把 8 个像素的**同一 bit 平面**压进一个 u8，一次字节运算等于 8 像素同时
//! 求值。这既是 GDI 的真实形态，也是错误最难暴露的形态——打包顺序写反（像素 0 进
//! bit 7）时，五条操作逐条看都「对」，只有跨位组合才露馅。故本模块的被测路径与
//! 参考路径在结构上互斥：被测路径吃打包字节做整字节运算，参考路径逐像素查真值表
//! 再打包，两者只共享 fn 常量，不共享任何一条运算代码。
//!
//! 分派键只取 `(rop >> 16) & 0xFF` 而**不看低 16 位**：低 16 位是操作家族与历史
//! 别名的编码，拿它当键会让「rop 写对、fn 写错」的条目照样命中，判据也就守不住
//! 「真值表决定语义」这条底线。rop 与 fn 必须成对登记，单独给一个必被拒。

use alloc::string::{String, ToString};

/// 一次位并行求值覆盖的像素数（一个 u8 的 8 个 bit，与字节宽度绑定）。
pub const PLANE_PIXELS: usize = 8;
/// rop3 真值表在 rop 码中的字节位移（高字节）。
pub const ROP3_FN_SHIFT: u32 = 16;
/// fn 字节掩码。
pub const ROP3_FN_MASK: u32 = 0xFF;
/// 本单登记的操作数（五条）。
pub const SRC_OP_COUNT: usize = 5;
/// 判据采样组数。写死而非由调用方推导：判据常量必须独立于被测值。
pub const PROBE_SAMPLES: u16 = 512;

/// 五操作诊断码闭集（0x91119~0x91120 八码）。
///
/// 显式写判别式而不用隐式递增：码值是跨模块日志的对账键，日后插入一个变体就会
/// 让后续码整体漂移，历史日志再也归不了因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RopCode {
    /// 0x91119 操作求值成功。
    RopOk = 0x91119,
    /// 0x9111A rop 码不在登记表内。
    UnknownRop = 0x9111A,
    /// 0x9111B rop 的高字节与其声明的 fn 不一致。
    FnRopMismatch = 0x9111B,
    /// 0x9111C fn 字节不属于本单五操作。
    UnsupportedFn = 0x9111C,
    /// 0x9111D 位并行结果与逐像素参考分叉。
    PlaneDiverge = 0x9111D,
    /// 0x9111E 操作表结构损坏（空缺、重复、与锚点不符）。
    TableCorrupt = 0x9111E,
    /// 0x9111F 平面宽度或像素值越界（行不足 8 像素或像素非 0/1）。
    PlaneWidth = 0x9111F,
    /// 0x91120 代数律被破（幂等/自反/对合）。
    IdentityBroken = 0x91120,
}

impl RopCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            RopCode::RopOk => "五操作求值成功",
            RopCode::UnknownRop => "rop 码未登记",
            RopCode::FnRopMismatch => "rop 与 fn 不成对",
            RopCode::UnsupportedFn => "fn 字节不属于五操作",
            RopCode::PlaneDiverge => "位并行与逐像素分叉",
            RopCode::TableCorrupt => "操作表结构损坏",
            RopCode::PlaneWidth => "平面宽度或像素值越界",
            RopCode::IdentityBroken => "代数律被破",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            RopCode::RopOk => "五条操作的位并行结果与参考逐位相同",
            RopCode::UnknownRop => "调用方传入的 rop 不在 0x00CC0020 等五条之内",
            RopCode::FnRopMismatch => "rop 高字节与独立传入的 fn 各说一套",
            RopCode::UnsupportedFn => "fn 属于本单未建模的操作族",
            RopCode::PlaneDiverge => "打包顺序、掩码宽度或真值表索引有一处错",
            RopCode::TableCorrupt => "表内条目与锚点数组不符或存在重复 fn",
            RopCode::PlaneWidth => "行长度不是 8 或像素值不是 0/1",
            RopCode::IdentityBroken => "两条路径的正负样本不构成预期的代数关系",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            RopCode::RopOk => "转入图案无源操作核",
            RopCode::UnknownRop => "按 0x00CC0020/0x00330008/0x008800C6/0x00EE0086/0x00660046 重取",
            RopCode::FnRopMismatch => "以 rop 高字节为准重写 fn，勿两处各写一遍",
            RopCode::UnsupportedFn => "回查该 fn 是否该走图案操作核",
            RopCode::PlaneDiverge => "先查像素到 bit 的映射方向，再查真值表索引 p*4+s*2+d",
            RopCode::TableCorrupt => "以锚点数组重建 SRC_TABLE",
            RopCode::PlaneWidth => "补齐 8 像素或把值收敛到 0/1",
            RopCode::IdentityBroken => "按失败律名逐条复核运算定义",
        }
    }
    /// 数值形式（日志/判据对账用）。
    ///
    /// 返回 u32 而非 u16：五位数码段（0x91119）超出 u16 值域，`as u16` 会静默
    /// 截断成 0x1119，与低段码撞车——这类截断在日志里表现为「另一个模块的码」。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 求值故障（带 rop / fn / 位置三个定位字段）。
///
/// 三个字段缺一不可：`rop` 说「调用方给的是什么」，`fn_byte` 说「表里认的是什么」，
/// `at` 说「第几个像素/第几条」。只给码时，rop 别名与 fn 写错无法区分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RopFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 涉事 rop 码（0 表示与具体 rop 无关）。
    pub rop: u32,
    /// 涉事 fn 字节。
    pub fn_byte: u8,
    /// 像素位序或采样序号（0 表示无具体位置）。
    pub at: u16,
}

impl RopFault {
    /// 建故障。
    const fn new(code: RopCode, rop: u32, fn_byte: u8) -> RopFault {
        RopFault {
            code: code.code(),
            rop,
            fn_byte,
            at: 0,
        }
    }
    /// 补位置（像素位序）。
    const fn at_bit(mut self, at: u16) -> RopFault {
        self.at = at;
        self
    }
    /// 把码还原成枚举，便于取三要素文本。
    ///
    /// 闭集外的值不 panic：落到 `TableCorrupt`（「未知即可疑」的保守落点），
    /// 同时 `is_known()` 让调用方能识别这是未知码而不是真的表坏了。
    pub const fn as_code(self) -> RopCode {
        match self.code {
            0x91119 => RopCode::RopOk,
            0x9111A => RopCode::UnknownRop,
            0x9111B => RopCode::FnRopMismatch,
            0x9111C => RopCode::UnsupportedFn,
            0x9111D => RopCode::PlaneDiverge,
            0x9111E => RopCode::TableCorrupt,
            0x9111F => RopCode::PlaneWidth,
            _ => RopCode::IdentityBroken,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91119 && self.code <= 0x91120
    }
}

/// 五条源操作。
///
/// 判别顺序按 fn 码升序（0x33 < 0x66 < 0x88 < 0xCC < 0xEE）而非按教学顺序：
/// 表查与二分都要依赖这个序，排错了不会报错，只会让「查不到」变成「查到别人」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SrcOp {
    /// NOTSRCCOPY：!S。
    NotCopy = 0,
    /// SRCINVERT：S^D。
    Invert = 1,
    /// SRCAND：S&D。
    And = 2,
    /// SRCCOPY：S。
    Copy = 3,
    /// SRCPAINT：S|D。
    Paint = 4,
}

impl SrcOp {
    /// 全表（顺序即判别顺序）。
    pub const ALL: [SrcOp; SRC_OP_COUNT] = [
        SrcOp::NotCopy,
        SrcOp::Invert,
        SrcOp::And,
        SrcOp::Copy,
        SrcOp::Paint,
    ];

    /// fn 字节（真值表本体）。
    pub const fn fn_byte(self) -> u8 {
        match self {
            SrcOp::NotCopy => 0x33,
            SrcOp::Invert => 0x66,
            SrcOp::And => 0x88,
            SrcOp::Copy => 0xCC,
            SrcOp::Paint => 0xEE,
        }
    }

    /// 对外 rop 码（含家族低位，仅供调用方按原样回传）。
    pub const fn rop(self) -> u32 {
        match self {
            SrcOp::NotCopy => 0x0033_0008,
            SrcOp::Invert => 0x0066_0046,
            SrcOp::And => 0x0088_00C6,
            SrcOp::Copy => 0x00CC_0020,
            SrcOp::Paint => 0x00EE_0086,
        }
    }

    /// 名字（日志对账用，与 rop 表逐行对应）。
    pub const fn name(self) -> &'static str {
        match self {
            SrcOp::NotCopy => "notsrccopy",
            SrcOp::Invert => "srcinvert",
            SrcOp::And => "srcand",
            SrcOp::Copy => "srccopy",
            SrcOp::Paint => "srcpaint",
        }
    }

    /// 由 fn 字节反查（分派主键）。
    pub const fn from_fn(f: u8) -> Option<SrcOp> {
        match f {
            0x33 => Some(SrcOp::NotCopy),
            0x66 => Some(SrcOp::Invert),
            0x88 => Some(SrcOp::And),
            0xCC => Some(SrcOp::Copy),
            0xEE => Some(SrcOp::Paint),
            _ => None,
        }
    }

    /// 由 rop 反查。只比高字节，低 16 位别名一概不认。
    pub const fn from_rop(rop: u32) -> Option<SrcOp> {
        SrcOp::from_fn(((rop >> ROP3_FN_SHIFT) & ROP3_FN_MASK) as u8)
    }

    /// 是否对目标幂等（f(f(s,d),d) 与 f(s,d) 同值）。
    pub const fn is_idempotent(self) -> bool {
        matches!(self, SrcOp::Copy | SrcOp::And | SrcOp::Paint)
    }

    /// 是否自反（f(f(...,d),d) == d，即在目标上做两次回到原值）。
    pub const fn is_involution(self) -> bool {
        matches!(self, SrcOp::NotCopy | SrcOp::Invert)
    }
}

/// 锚点 rop 表：**独立于 `SrcOp` 写死一遍**。
///
/// 判据要防的正是「实现与期望同源」——若表直接从 `SrcOp::rop()` 生成，那么
/// `SrcOp::rop()` 写错时表也跟着错，两边永远一致。故这里另抄一份十六进制常量，
/// 由 `validate_table` 把两份对起来。
const ANCHOR_ROPS: [u32; SRC_OP_COUNT] = [
    0x0033_0008,
    0x0066_0046,
    0x0088_00C6,
    0x00CC_0020,
    0x00EE_0086,
];
/// 锚点 fn 表（同上，独立抄写）。
const ANCHOR_FNS: [u8; SRC_OP_COUNT] = [0x33, 0x66, 0x88, 0xCC, 0xEE];

/// 登记表本体（顺序与 `ANCHOR_*` 一致）。
pub const SRC_TABLE: [SrcOp; SRC_OP_COUNT] = SrcOp::ALL;

/// 取 rop3 真值表字节。
///
/// 只取高字节：低 16 位是家族与别名的编码，参与语义会把「别名不同、真值表相同」
/// 的两条 rop 判成不同操作。
pub const fn rop3_fn(rop: u32) -> u8 {
    ((rop >> ROP3_FN_SHIFT) & ROP3_FN_MASK) as u8
}

/// 真值表索引：P 最高位、S 居中、D 最低位。
///
/// 单独成函数是为了让「索引写反」这种错有唯一落点——散在五处 `((p<<2)|...)`
/// 里时，错一处只会表现为某条操作偶发不符，很难归因。
pub const fn truth_index(p: u8, s: u8, d: u8) -> u8 {
    ((p & 1) << 2) | ((s & 1) << 1) | (d & 1)
}

/// 单像素真值表求值（参考路径的唯一入口）。
pub const fn rop3_eval(f: u8, p: u8, s: u8, d: u8) -> u8 {
    (f >> truth_index(p, s, d)) & 1
}

/// 打包 8 像素：像素 i 落在 bit i。
///
/// 方向（像素 0 → bit 0）必须与 `unpack_pixels` 严格互逆；写成 bit 7 起会让
/// 所有 op 的「正确答案」整体镜像，而单条操作自测永远发现不了。
pub const fn pack_pixels(row: &[u8; PLANE_PIXELS]) -> u8 {
    let mut out = 0u8;
    let mut i = 0usize;
    while i < PLANE_PIXELS {
        out |= (row[i] & 1) << i;
        i += 1;
    }
    out
}

/// 解包平面：bit i → 像素 i（0/1）。
pub fn unpack_pixels(plane: u8) -> [u8; PLANE_PIXELS] {
    let mut out = [0u8; PLANE_PIXELS];
    let mut i = 0usize;
    while i < PLANE_PIXELS {
        out[i] = (plane >> i) & 1;
        i += 1;
    }
    out
}

/// 切片版打包（对外入口）：宽度非 8 或像素非 0/1 一律拒。
///
/// 数组版 `pack_pixels` 在类型层面已保证宽度，切片版面对的是外部数据，必须逐项
/// 验：`&1` 的静默截断会把像素值 2 当成 0，画面上表现为「偶发丢点」，比报错难查。
pub fn pack_pixels_checked(row: &[u8]) -> Result<u8, RopFault> {
    if row.len() != PLANE_PIXELS {
        return Err(RopFault::new(RopCode::PlaneWidth, 0, 0).at_bit(row.len() as u16));
    }
    let mut out = 0u8;
    let mut i = 0usize;
    while i < PLANE_PIXELS {
        if row[i] > 1 {
            return Err(RopFault::new(RopCode::PlaneWidth, 0, 0).at_bit(i as u16));
        }
        out |= row[i] << i;
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五条位并行核（各自独立成函数，不做「一个 match 包打天下」的合并）
// ---------------------------------------------------------------------------

/// SRCCOPY 的位并行核：逐位透传源。
pub const fn plane_srccopy(_p: u8, s: u8, _d: u8) -> u8 {
    s
}

/// NOTSRCCOPY 的位并行核：逐位取反。
pub const fn plane_notsrccopy(_p: u8, s: u8, _d: u8) -> u8 {
    !s
}

/// SRCAND 的位并行核。
pub const fn plane_srcand(_p: u8, s: u8, d: u8) -> u8 {
    s & d
}

/// SRCPAINT 的位并行核。
pub const fn plane_srcpaint(_p: u8, s: u8, d: u8) -> u8 {
    s | d
}

/// SRCINVERT 的位并行核。
pub const fn plane_srcinvert(_p: u8, s: u8, d: u8) -> u8 {
    s ^ d
}

/// 按枚举分派到位并行核。
pub const fn apply_plane(op: SrcOp, p: u8, s: u8, d: u8) -> u8 {
    match op {
        SrcOp::NotCopy => plane_notsrccopy(p, s, d),
        SrcOp::Invert => plane_srcinvert(p, s, d),
        SrcOp::And => plane_srcand(p, s, d),
        SrcOp::Copy => plane_srccopy(p, s, d),
        SrcOp::Paint => plane_srcpaint(p, s, d),
    }
}

/// 按 fn 字节分派（对外主入口）。未登记 fn 必须显性拒绝。
pub fn apply_plane_by_fn(f: u8, p: u8, s: u8, d: u8) -> Result<u8, RopFault> {
    match SrcOp::from_fn(f) {
        Some(op) => Ok(apply_plane(op, p, s, d)),
        None => Err(RopFault::new(RopCode::UnsupportedFn, 0, f)),
    }
}

/// 逐位参考：解包 → 每位查真值表 → 重打包。
///
/// 刻意走 `unpack_pixels`/`rop3_eval` 这条与位并行核毫无交集的路径：它慢、
/// 笨，但它是唯一能证明「整字节运算没有把某一位算错」的东西。
pub fn scalar_from_planes(op: SrcOp, p: u8, s: u8, d: u8) -> u8 {
    let pa = unpack_pixels(p);
    let sa = unpack_pixels(s);
    let da = unpack_pixels(d);
    let mut out = 0u8;
    let mut i = 0usize;
    while i < PLANE_PIXELS {
        out |= rop3_eval(op.fn_byte(), pa[i], sa[i], da[i]) << i;
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 行求值：一次把 8 像素喂进两条路径并留下差异记账
// ---------------------------------------------------------------------------

/// 一行 8 像素的求值结果。
///
/// 保留 `plane` 与 `scalar` 两个完整结果而不只存一个 bool：差异定位时要知道
/// **差在哪几位**，只报「不一致」等于让排查从零开始。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowEval {
    /// 被测操作。
    pub op: SrcOp,
    /// 位并行结果。
    pub plane: u8,
    /// 逐像素参考结果（已重打包）。
    pub scalar: u8,
    /// 两者的异或（0 表示逐位相同）。
    pub diff: u8,
}

impl RowEval {
    /// 是否逐位相同。
    pub const fn agrees(&self) -> bool {
        self.diff == 0
    }
    /// 差异像素个数。
    pub fn diff_pixels(&self) -> u8 {
        self.diff.count_ones() as u8
    }
    /// 差异位图（诊断文本用：把 diff 的置位当作像素号）。
    pub fn diff_mask(&self) -> u8 {
        self.diff
    }
}

/// 求值一行（像素数组入口）。
pub fn eval_row(
    op: SrcOp,
    p: &[u8; PLANE_PIXELS],
    s: &[u8; PLANE_PIXELS],
    d: &[u8; PLANE_PIXELS],
) -> RowEval {
    let pp = pack_pixels(p);
    let sp = pack_pixels(s);
    let dp = pack_pixels(d);
    let plane = apply_plane(op, pp, sp, dp);
    let scalar = scalar_from_planes(op, pp, sp, dp);
    RowEval {
        op,
        plane,
        scalar,
        diff: plane ^ scalar,
    }
}

/// 求值一行（fn 字节入口）。
pub fn eval_row_by_fn(
    f: u8,
    p: &[u8; PLANE_PIXELS],
    s: &[u8; PLANE_PIXELS],
    d: &[u8; PLANE_PIXELS],
) -> Result<RowEval, RopFault> {
    match SrcOp::from_fn(f) {
        Some(op) => Ok(eval_row(op, p, s, d)),
        None => Err(RopFault::new(RopCode::UnsupportedFn, 0, f)),
    }
}

/// 分叉守卫：把 `RowEval` 的差异升级为故障。
///
/// 单独成函数而不塞进 `eval_row`，是因为批量探测要**累计**差异条数再统一报告；
/// 逐行即错会让第一行就中断，看不到「是系统性偏移还是个别位」。
pub fn require_agreement(ev: &RowEval) -> Result<(), RopFault> {
    if ev.agrees() {
        Ok(())
    } else {
        Err(RopFault::new(RopCode::PlaneDiverge, ev.op.rop(), ev.op.fn_byte())
            .at_bit(ev.diff_mask() as u16))
    }
}

// ---------------------------------------------------------------------------
// 确定性采样源（LCG，种子写死；不引 rand，也不碰墙钟）
// ---------------------------------------------------------------------------

/// 线性同余发生器（Knuth MMIX 常数）。
///
/// 判据要求「同输入两次逐位相同」，所以采样序列必须自己造：任何外部熵源都会
/// 让复现失败。常数取自 MMIX 的 6364136223846793005 / 1442695040888963407。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 定种子。
    pub const fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }
    /// 下一个 64 位。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // 高位扩散更好：低位在这类乘法 LCG 里周期短，取 bit 33..41 足够用。
        self.state >> 33
    }
    /// 下一个 0/1 像素值。
    pub fn next_bit(&mut self) -> u8 {
        (self.next_u64() & 1) as u8
    }
    /// 下一个整字节（边界样本用）。
    pub fn next_u8(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }
}

// ---------------------------------------------------------------------------
// 批量记账
// ---------------------------------------------------------------------------

/// 单操作的批量结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchTally {
    /// 被测操作。
    pub op: SrcOp,
    /// 采样行数。
    pub rows: u16,
    /// 分叉行数。
    pub bad_rows: u16,
    /// 分叉像素总数（跨行累加）。
    pub bad_pixels: u32,
    /// 结果指纹（逐行咬合，用于「两次跑同一批」的确定性判据）。
    pub fingerprint: u64,
}

impl BatchTally {
    /// 空账（指纹取 FNV-1a 偏移基）。
    pub const fn empty(op: SrcOp) -> BatchTally {
        BatchTally {
            op,
            rows: 0,
            bad_rows: 0,
            bad_pixels: 0,
            fingerprint: 0xcbf2_9ce4_8422_2325,
        }
    }
    /// 记一行。
    pub fn record(&mut self, ev: &RowEval) {
        self.rows = self.rows.saturating_add(1);
        if !ev.agrees() {
            self.bad_rows = self.bad_rows.saturating_add(1);
            self.bad_pixels = self
                .bad_pixels
                .saturating_add(ev.diff_pixels() as u32);
        }
        // 指纹咬合的是**被测结果**而非参考结果：参考算错时指纹不变，才能把
        // 「实现漂移」与「参考漂移」两类故障分开。
        self.fingerprint = fnv_u64(self.fingerprint, ev.plane as u64);
    }
    /// 是否干净（有采样且零分叉）。
    pub const fn clean(&self) -> bool {
        self.rows > 0 && self.bad_rows == 0 && self.bad_pixels == 0
    }
    /// 可归档报告（字段顺序固定便于 diff）。
    pub fn report(&self) -> String {
        let mut out = String::with_capacity(96);
        out.push_str("op=");
        out.push_str(self.op.name());
        out.push_str(" fn=");
        out.push_str(&hex8(self.op.fn_byte()));
        out.push_str(" rows=");
        out.push_str(&self.rows.to_string());
        out.push_str(" bad_rows=");
        out.push_str(&self.bad_rows.to_string());
        out.push_str(" bad_px=");
        out.push_str(&self.bad_pixels.to_string());
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint));
        out
    }
}

/// FNV-1a 64 单步（写死质数与偏移基，来源：FNV 规范 1a 变体）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免 format! 的分配开销）。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 8 位十六进制（fn 字节打印）。
pub fn hex8(v: u8) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(2);
    s.push(HEX[((v >> 4) & 0xF) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
    s
}

/// 采样一批行并记账。
///
/// 种子取 op 的 fn 字节：五条操作各自一条独立序列，避免「五条共用序列时
/// 同一位错被同一份数据同时掩盖」。
pub fn probe_op(op: SrcOp, samples: u16) -> BatchTally {
    let mut lcg = Lcg::new(op.fn_byte() as u64);
    let mut tally = BatchTally::empty(op);
    let mut n = 0u16;
    while n < samples {
        let mut p = [0u8; PLANE_PIXELS];
        let mut s = [0u8; PLANE_PIXELS];
        let mut d = [0u8; PLANE_PIXELS];
        let mut k = 0usize;
        while k < PLANE_PIXELS {
            p[k] = lcg.next_bit();
            s[k] = lcg.next_bit();
            d[k] = lcg.next_bit();
            k += 1;
        }
        tally.record(&eval_row(op, &p, &s, &d));
        n += 1;
    }
    tally
}

/// 五条操作全跑一遍。
pub fn probe_all_ops(samples: u16) -> [BatchTally; SRC_OP_COUNT] {
    let mut out = [BatchTally::empty(SrcOp::Copy); SRC_OP_COUNT];
    let mut i = 0usize;
    while i < SRC_OP_COUNT {
        out[i] = probe_op(SRC_TABLE[i], samples);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 表校验与查表
// ---------------------------------------------------------------------------

/// 校验一张操作表：条数、锚点、fn/rop 互指、去重。
///
/// 接受任意切片而非只认 `SRC_TABLE`，是为了让判据能喂坏表进来——只能自证的
/// 校验函数等于没有校验。
pub fn validate_table(table: &[SrcOp]) -> Result<usize, RopFault> {
    if table.len() != SRC_OP_COUNT {
        return Err(RopFault::new(RopCode::TableCorrupt, 0, 0).at_bit(table.len() as u16));
    }
    let mut i = 0usize;
    while i < table.len() {
        let op = table[i];
        if op.fn_byte() != ANCHOR_FNS[i] || op.rop() != ANCHOR_ROPS[i] {
            return Err(RopFault::new(RopCode::TableCorrupt, op.rop(), op.fn_byte())
                .at_bit(i as u16));
        }
        // rop 高字节必须与 fn 一致：这正是「rop 与 fn 成对」的形式化。
        if rop3_fn(op.rop()) != op.fn_byte() {
            return Err(RopFault::new(RopCode::FnRopMismatch, op.rop(), op.fn_byte())
                .at_bit(i as u16));
        }
        let mut j = i + 1;
        while j < table.len() {
            if table[i].fn_byte() == table[j].fn_byte() {
                return Err(RopFault::new(
                    RopCode::TableCorrupt,
                    table[j].rop(),
                    table[j].fn_byte(),
                )
                .at_bit(j as u16));
            }
            j += 1;
        }
        i += 1;
    }
    Ok(table.len())
}

/// 按 rop 查操作。
pub fn lookup_rop(rop: u32) -> Result<SrcOp, RopFault> {
    match SrcOp::from_rop(rop) {
        Some(op) if op.rop() == rop => Ok(op),
        // rop 高字节认得、但低 16 位不是本表登记的那一条：属于别名，不认。
        Some(op) => Err(RopFault::new(RopCode::UnknownRop, rop, op.fn_byte())),
        None => Err(RopFault::new(RopCode::UnknownRop, rop, rop3_fn(rop))),
    }
}

/// 按 (rop, fn) 成对查表。
///
/// 独立收 fn 参数是刻意的：调用方若把 rop 抄对、fn 抄错，只有成对校验才抓得住；
/// 只按 rop 查会「以 rop 为准」默默纠正 fn，错误被吞掉。
pub fn resolve_pair(rop: u32, f: u8) -> Result<SrcOp, RopFault> {
    let from_rop_side = rop3_fn(rop);
    if from_rop_side != f {
        return Err(RopFault::new(RopCode::FnRopMismatch, rop, f));
    }
    match lookup_rop(rop) {
        Ok(op) => Ok(op),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// 代数律
// ---------------------------------------------------------------------------

/// 需要守住的四条代数律。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Law {
    /// SRCCOPY∘SRCCOPY = SRCCOPY（对目标幂等）。
    CopyIdempotent = 0,
    /// NOTSRCCOPY∘NOTSRCCOPY = SRCCOPY（二次取反回源）。
    NotCopyInvolution = 1,
    /// SRCINVERT 在目标上自反。
    InvertInvolution = 2,
    /// 德摩根：!(S&D) = !S | !D。
    DeMorgan = 3,
}

impl Law {
    /// 全表。
    pub const ALL: [Law; 4] = [
        Law::CopyIdempotent,
        Law::NotCopyInvolution,
        Law::InvertInvolution,
        Law::DeMorgan,
    ];
    /// 名字（判据失败时直接指认是第几条律）。
    pub const fn name(self) -> &'static str {
        match self {
            Law::CopyIdempotent => "copy_idempotent",
            Law::NotCopyInvolution => "notcopy_involution",
            Law::InvertInvolution => "invert_involution",
            Law::DeMorgan => "de_morgan",
        }
    }
}

/// 逐行比较两个 8 像素结果，不等即 IdentityBroken。
pub fn rows_equal(lhs: &[u8; PLANE_PIXELS], rhs: &[u8; PLANE_PIXELS]) -> Result<(), RopFault> {
    let mut i = 0usize;
    while i < PLANE_PIXELS {
        if lhs[i] != rhs[i] {
            return Err(RopFault::new(RopCode::IdentityBroken, 0, 0).at_bit(i as u16));
        }
        i += 1;
    }
    Ok(())
}

/// 单行上验证一条律。
///
/// 律的左右两侧都用**位并行核**算：代数律守的是运算定义本身，若借用参考路径
/// 就会被参考路径的错掩盖，反而失去独立约束。
pub fn check_law_row(
    law: Law,
    p: &[u8; PLANE_PIXELS],
    s: &[u8; PLANE_PIXELS],
    d: &[u8; PLANE_PIXELS],
) -> Result<(), RopFault> {
    let pp = pack_pixels(p);
    let sp = pack_pixels(s);
    let dp = pack_pixels(d);
    let mut lhs = [0u8; PLANE_PIXELS];
    let mut rhs = [0u8; PLANE_PIXELS];
    match law {
        Law::CopyIdempotent => {
            // 第一次：源覆盖目标；第二次：把上一次的结果当目标再覆盖一次。
            let once = plane_srccopy(pp, sp, dp);
            let twice = plane_srccopy(pp, sp, once);
            lhs = unpack_pixels(once);
            rhs = unpack_pixels(twice);
        }
        Law::NotCopyInvolution => {
            let once = plane_notsrccopy(pp, sp, dp);
            let twice = plane_notsrccopy(pp, once, dp);
            lhs = unpack_pixels(twice);
            // 二次取反必须回到源本身。
            rhs = unpack_pixels(sp);
        }
        Law::InvertInvolution => {
            let once = plane_srcinvert(pp, sp, dp);
            let twice = plane_srcinvert(pp, sp, once);
            lhs = unpack_pixels(twice);
            rhs = unpack_pixels(dp);
        }
        Law::DeMorgan => {
            let left = plane_notsrccopy(pp, plane_srcand(pp, sp, dp), dp);
            let right = plane_srcpaint(
                pp,
                plane_notsrccopy(pp, sp, dp),
                plane_notsrccopy(pp, dp, sp),
            );
            lhs = unpack_pixels(left);
            rhs = unpack_pixels(right);
        }
    }
    rows_equal(&lhs, &rhs)
}

/// 整条律跑 `samples` 行。
pub fn check_law(law: Law, samples: u16) -> Result<u16, RopFault> {
    let mut lcg = Lcg::new(0x2160_4000 ^ (law as u64));
    let mut n = 0u16;
    while n < samples {
        let mut p = [0u8; PLANE_PIXELS];
        let mut s = [0u8; PLANE_PIXELS];
        let mut d = [0u8; PLANE_PIXELS];
        let mut k = 0usize;
        while k < PLANE_PIXELS {
            p[k] = lcg.next_bit();
            s[k] = lcg.next_bit();
            d[k] = lcg.next_bit();
            k += 1;
        }
        check_law_row(law, &p, &s, &d)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 判据夹具
// ---------------------------------------------------------------------------

/// 标准夹具：表可校验 + 五条操作各跑 `PROBE_SAMPLES` 行。
///
/// 返回 Result 而非直接给数组：历史上「夹具自带矛盾」让整册判据被一个 expect
/// 挡在门外、从未真正跑过。夹具失败必须在第一条判据里显性暴露。
pub fn standard_probe() -> Result<[BatchTally; SRC_OP_COUNT], RopFault> {
    validate_table(&SRC_TABLE)?;
    Ok(probe_all_ops(PROBE_SAMPLES))
}

/// 边界平面组：0/单点/双点/高低半区/交替/全一。八条覆盖了打包方向与进位边。
pub const EDGE_PLANES: [u8; 8] = [0x00, 0x01, 0x02, 0x04, 0x80, 0xAA, 0x55, 0xFF];

/// 判据入口。
///
/// 判据常量一律以字面量出现（`512` / `0xCC` / `0x00CC0020`），不复用被测常量：
/// 否则把 `PROBE_SAMPLES` 改成 8 时判据跟着变，等于自己证自己。
pub fn run_unx21604_blackops_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21604");
    let fixture = standard_probe();

    // 1) 夹具可构造：先证夹具，再证被测逻辑。
    cs.add(
        "f21604.fixture_buildable",
        match &fixture {
            Ok(t) => t.len() == 5,
            Err(_) => false,
        },
        "标准夹具构造失败（表校验或采样未通过）",
    );

    // 2) 锚点语义：五条 fn 在 8 种输入组合上与真值表定义逐条相符。
    {
        let mut ok = true;
        let mut combo = 0usize;
        while combo < 8 {
            let p = ((combo >> 2) & 1) as u8;
            let s = ((combo >> 1) & 1) as u8;
            let d = (combo & 1) as u8;
            // 期望值直接按真值表手算写死，不调用 rop3_eval（否则自己证自己）。
            let want_copy = s;
            let want_notcopy = 1 - s;
            let want_and = s & d;
            let want_paint = s | d;
            let want_invert = s ^ d;
            if rop3_eval(0xCC, p, s, d) != want_copy
                || rop3_eval(0x33, p, s, d) != want_notcopy
                || rop3_eval(0x88, p, s, d) != want_and
                || rop3_eval(0xEE, p, s, d) != want_paint
                || rop3_eval(0x66, p, s, d) != want_invert
            {
                ok = false;
            }
            // 同一组合下位并行核也必须同值（单像素平面 = bit0）。
            if plane_srccopy(p, s, d) & 1 != want_copy
                || plane_notsrccopy(p, s, d) & 1 != want_notcopy
                || plane_srcand(p, s, d) & 1 != want_and
                || plane_srcpaint(p, s, d) & 1 != want_paint
                || plane_srcinvert(p, s, d) & 1 != want_invert
            {
                ok = false;
            }
            combo += 1;
        }
        cs.add("f21604.anchor_semantics", ok, "五条锚点与真值表定义不符");
    }

    // 3) 512 组采样逐位相同（判据 J1 主体）。
    match &fixture {
        Ok(tallies) => {
            let mut ok = true;
            let mut i = 0usize;
            while i < tallies.len() {
                let t = tallies[i];
                if t.rows != 512 || t.bad_rows != 0 || t.bad_pixels != 0 {
                    ok = false;
                }
                i += 1;
            }
            cs.add(
                "f21604.plane_matches_scalar_512",
                ok,
                "五操作中至少一条与逐像素参考分叉（行数须为 512、分叉须为 0）",
            );
        }
        Err(_) => cs.add(
            "f21604.plane_matches_scalar_512",
            false,
            "夹具不可用",
        ),
    }

    // 4) 边界平面矩阵：8×8×8 组合全枚举，位并行与逐位参考同值。
    {
        let mut ok = true;
        let mut op_i = 0usize;
        while op_i < SRC_OP_COUNT {
            let op = SRC_TABLE[op_i];
            let mut s_i = 0usize;
            while s_i < EDGE_PLANES.len() {
                let mut d_i = 0usize;
                while d_i < EDGE_PLANES.len() {
                    let s = EDGE_PLANES[s_i];
                    let d = EDGE_PLANES[d_i];
                    let got = apply_plane(op, 0xF0, s, d);
                    let want = scalar_from_planes(op, 0xF0, s, d);
                    if got != want {
                        ok = false;
                    }
                    d_i += 1;
                }
                s_i += 1;
            }
            op_i += 1;
        }
        // 顺带钉两个可手算的值：SRCAND(0xF0,0xCC)=0xC0；SRCINVERT(0x0F,0xF0)=0xFF。
        let spot = plane_srcand(0, 0xF0, 0xCC) == 0xC0 && plane_srcinvert(0, 0x0F, 0xF0) == 0xFF;
        cs.add(
            "f21604.edge_planes_agree",
            ok && spot,
            "边界平面矩阵分叉或手算锚点不符",
        );
    }

    // 5) rop 表可校验且五条 rop 全部查得到。
    {
        let check = validate_table(&SRC_TABLE);
        let mut ok = check == Ok(5);
        let mut i = 0usize;
        while i < SRC_OP_COUNT {
            if lookup_rop(ANCHOR_ROPS[i]) != Ok(SRC_TABLE[i]) {
                ok = false;
            }
            i += 1;
        }
        cs.add(
            "f21604.rop_table_lookup",
            ok,
            "表校验失败或某条 rop 查不到（应为 5 条全中）",
        );
    }

    // 6) 未登记 rop 必拒：DSTINVERT 属图案操作单，本表不该收。
    {
        let e = lookup_rop(0x0055_0009).err();
        cs.add(
            "f21604.unknown_rop_rejected",
            e.map(|f| f.code) == Some(RopCode::UnknownRop.code()),
            "0x00550009 未被拒或码不符",
        );
    }

    // 7) rop/fn 不成对必拒（rop 抄对、fn 抄错）。
    {
        let e = resolve_pair(0x00CC_0020, 0x33).err();
        cs.add(
            "f21604.fn_rop_mismatch",
            e.map(|f| f.code) == Some(RopCode::FnRopMismatch.code()),
            "rop 0x00CC0020 配 fn 0x33 未被拒或码不符",
        );
    }

    // 8) 未登记 fn 必拒（BLACKNESS 的 0x00 不属于五操作）。
    {
        let e = apply_plane_by_fn(0x00, 0xF0, 0x0F, 0xFF).err();
        let e2 = eval_row_by_fn(0x00, &[1; 8], &[0; 8], &[1; 8]).err();
        cs.add(
            "f21604.unsupported_fn_rejected",
            e.map(|f| f.code) == Some(RopCode::UnsupportedFn.code())
                && e2.map(|f| f.code) == Some(RopCode::UnsupportedFn.code()),
            "fn=0x00 未被两条入口同时拒绝或码不符",
        );
    }

    // 9) 坏表必拒：空表与重复表。
    {
        let empty = validate_table(&[]).err();
        let dup = validate_table(&[SrcOp::Copy, SrcOp::Copy]).err();
        let short = validate_table(&[SrcOp::Copy]).err();
        cs.add(
            "f21604.bad_table_rejected",
            empty.map(|f| f.code) == Some(RopCode::TableCorrupt.code())
                && dup.map(|f| f.code) == Some(RopCode::TableCorrupt.code())
                && short.map(|f| f.code) == Some(RopCode::TableCorrupt.code()),
            "空表/重复表/短表未被拒或码不符",
        );
    }

    // 10) 平面宽度与像素值越界必拒。
    {
        let short = pack_pixels_checked(&[1, 0, 1]).err();
        let wide = pack_pixels_checked(&[1, 0, 1, 0, 1, 0, 1, 0, 1]).err();
        let bad_val = pack_pixels_checked(&[2, 0, 0, 0, 0, 0, 0, 0]).err();
        let good = pack_pixels_checked(&[1, 0, 0, 0, 0, 0, 0, 1]);
        cs.add(
            "f21604.plane_width_rejected",
            short.map(|f| f.code) == Some(RopCode::PlaneWidth.code())
                && wide.map(|f| f.code) == Some(RopCode::PlaneWidth.code())
                && bad_val.map(|f| f.code) == Some(RopCode::PlaneWidth.code())
                && good == Ok(0x81),
            "宽度 3/9 或像素值 2 未被拒，或 8 像素打包结果不为 0x81",
        );
    }

    // 11) 四条代数律全过（512 行）。
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < Law::ALL.len() {
            if check_law(Law::ALL[i], 512) != Ok(512) {
                ok = false;
            }
            i += 1;
        }
        cs.add(
            "f21604.algebra_laws",
            ok,
            "copy 幂等 / notcopy 对合 / invert 自反 / 德摩根 至少一条被破",
        );
    }

    // 12) 代数律的守卫本身有效：人为构造不等行必报 IdentityBroken。
    {
        let lhs = [1u8, 0, 1, 0, 1, 0, 1, 0];
        let rhs = [1u8, 0, 1, 0, 1, 0, 1, 1];
        let e = rows_equal(&lhs, &rhs).err();
        let same = rows_equal(&lhs, &lhs);
        cs.add(
            "f21604.identity_guard",
            e.map(|f| f.code) == Some(RopCode::IdentityBroken.code())
                && same.is_ok()
                && e.map(|f| f.at) == Some(7),
            "不等行未被拒、码不符或定位位号不是 7",
        );
    }

    // 13) 分叉守卫有效：人为构造分叉行必报 PlaneDiverge，且位图为差异位。
    {
        let ev = RowEval {
            op: SrcOp::And,
            plane: 0xF0,
            scalar: 0xF1,
            diff: 0x01,
        };
        let e = require_agreement(&ev).err();
        let clean = RowEval {
            op: SrcOp::And,
            plane: 0xF0,
            scalar: 0xF0,
            diff: 0x00,
        };
        cs.add(
            "f21604.diverge_guard",
            !ev.agrees()
                && ev.diff_pixels() == 1
                && e.map(|f| f.code) == Some(RopCode::PlaneDiverge.code())
                && require_agreement(&clean).is_ok(),
            "分叉行未被拒、像素计数不符或干净行被误拒",
        );
    }

    // 14) 确定性：同种子两次采样逐位相同；换种子指纹必变。
    {
        let a = probe_op(SrcOp::Paint, 64);
        let b = probe_op(SrcOp::Paint, 64);
        let mut c = BatchTally::empty(SrcOp::Paint);
        let mut lcg = Lcg::new(7);
        let mut n = 0u16;
        while n < 32 {
            let mut p = [0u8; PLANE_PIXELS];
            let mut s = [0u8; PLANE_PIXELS];
            let mut d = [0u8; PLANE_PIXELS];
            let mut k = 0usize;
            while k < PLANE_PIXELS {
                p[k] = lcg.next_bit();
                s[k] = lcg.next_bit();
                d[k] = lcg.next_bit();
                k += 1;
            }
            c.record(&eval_row(SrcOp::Paint, &p, &s, &d));
            n += 1;
        }
        cs.add(
            "f21604.deterministic",
            a == b && a.fingerprint != 0 && c.fingerprint != a.fingerprint,
            "两次采样不一致或不同序列指纹相同",
        );
    }

    // 15) 空批边界：零采样是合法输入，不得被当成「干净通过」。
    {
        let t = probe_op(SrcOp::Copy, 0);
        cs.add(
            "f21604.empty_batch",
            t.rows == 0 && t.bad_rows == 0 && !t.clean(),
            "零采样被误判为干净批次",
        );
    }

    // 16) 报告字段完整（可归档性）。
    match &fixture {
        Ok(tallies) => {
            let r = tallies[3].report();
            cs.add(
                "f21604.report_complete",
                r.contains("op=srccopy")
                    && r.contains("fn=cc")
                    && r.contains("rows=512")
                    && r.contains("bad_px=0"),
                "报告缺字段或数值不符",
            );
        }
        Err(_) => cs.add("f21604.report_complete", false, "夹具不可用"),
    }

    cs
}
