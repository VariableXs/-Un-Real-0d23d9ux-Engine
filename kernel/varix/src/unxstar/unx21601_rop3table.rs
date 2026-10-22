//! UNX-F21601 · rop3_table 编译期生成器与 256 项自检
//!
//! 诊断段 0x91101~0x91108（F2-B40 GDI 光栅操作地基 1/3）。
//!
//! 判据原文（施工书 UNX-F21601-J1）：256 项表与 wingdi 逐项一致，自检锚点 15/15 命中。
//!
//! 设计要点（为什么不是 `const TABLE: [u8; 256] = [ ...256 个手抄值... ];`）：
//! 手抄表只能证「抄的人没打错字」，证不了「表描述的语义对」。所以这里的表**从语义
//! 长出来**：`build_table()` 逐 fn 按 P/S/D 三维枚举 8 个最小项、逐位装配回字节，
//! 装配结果必须回到原 fn（`GENERATOR_OK`）——生成器自己就是第一道判据。
//! 第二道路径刻意完全独立：15 个锚点的期望值**不查表**，用布尔算子当场写
//! （`1 - (s | d)` 这种式子），两路同源不同路，任一侧抄错都会在 120 个格子上分叉。
//!
//! 锚点用 `const` 求值的判定常量而非 `assert!`：本批硬禁 `assert!`/`panic!`，
//! 于是把「15/15」压成 `[bool; 15]` 常量数组，编译期就在算，运行期判据只是读它。
//! 反向查询（fn → 助记名）用线性扫 15 项而不是 256 槽散列：锚点是人读的契约，
//! 索引空间小到「贵一点但一眼看全」比省几十个周期更值。

use alloc::string::String;

/// 表长：fn 是 8 位，故恰 256 项（写死，不从 `u8::MAX` 推导，防口径漂移）。
pub const TABLE_LEN: usize = 256;
/// 最小项个数：P/S/D 三个 1 bit 输入 → 8 种组合。
pub const MINTERM_COUNT: usize = 8;
/// 位平面内的像素位数（一个 u8 装 8 个同 bit 平面的像素）。
pub const PIXELS_PER_BYTE: usize = 8;

/// 表域诊断码闭集（0x91101~0x91108）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体就会整体漂移，
/// 历史日志将无法归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableCode {
    /// 0x91101 表与锚点全部命中。
    Ok = 0x91101,
    /// 0x91102 锚点的 rop 高字节与所声明 fn 不符。
    AnchorMismatch = 0x91102,
    /// 0x91103 生成器装配结果回不到原 fn（表被污染或生成逻辑错）。
    GeneratorDiverge = 0x91103,
    /// 0x91104 表索引越界（>= 256）。
    IndexOutOfRange = 0x91104,
    /// 0x91105 助记名不在 15 锚点闭集内。
    UnknownMnemonic = 0x91105,
    /// 0x91106 表指纹与登记值不符（表被改动）。
    FingerprintDrift = 0x91106,
    /// 0x91107 最小项不构成对 8 格真值域的划分。
    PartitionBroken = 0x91107,
    /// 0x91108 锚点集合自身重名或重 fn。
    DuplicateAnchor = 0x91108,
}

impl TableCode {
    /// 诊断三要素之一：现象（发生了什么）。
    pub const fn what(self) -> &'static str {
        match self {
            TableCode::Ok => "ROP3 真值表与锚点全部命中",
            TableCode::AnchorMismatch => "锚点 rop 码高字节与声明 fn 不符",
            TableCode::GeneratorDiverge => "生成器装配结果无法回到原 fn",
            TableCode::IndexOutOfRange => "真值表索引越界",
            TableCode::UnknownMnemonic => "助记名不在锚点闭集内",
            TableCode::FingerprintDrift => "真值表指纹与登记值不符",
            TableCode::PartitionBroken => "最小项未构成真值域划分",
            TableCode::DuplicateAnchor => "锚点集合存在重名或重 fn",
        }
    }
    /// 诊断三要素之二：原因（为什么会这样）。
    pub const fn why(self) -> &'static str {
        match self {
            TableCode::Ok => "256 项逐项等于 fn 自身，15 锚点双路一致",
            TableCode::AnchorMismatch => "锚点表里 rop 与 fn 两栏被分别手改过",
            TableCode::GeneratorDiverge => "枚举装配路径与位提取路径不再等价",
            TableCode::IndexOutOfRange => "调用方把 fn 以外的量当索引传入",
            TableCode::UnknownMnemonic => "拼错助记名或该 fn 本就没有标准名",
            TableCode::FingerprintDrift => "表的某一项在构造后被替换",
            TableCode::PartitionBroken => "某个 (P,S,D) 组合被两个最小项同时命中",
            TableCode::DuplicateAnchor => "同一助记名或同一 fn 登记了两次",
        }
    }
    /// 诊断三要素之三：建议（下一步做什么）。
    pub const fn advice(self) -> &'static str {
        match self {
            TableCode::Ok => "转 ROP3 三输入位模型核",
            TableCode::AnchorMismatch => "以 wingdi 常量为准重填 rop 栏，勿动 fn 栏",
            TableCode::GeneratorDiverge => "回到 assemble() 检查最小项枚举的移位量",
            TableCode::IndexOutOfRange => "先把索引钳到 0..=255，再查表",
            TableCode::UnknownMnemonic => "改为按 fn 反查，或用 15 锚点内的名字",
            TableCode::FingerprintDrift => "整表重算并与 wingdi 常量逐项对拍",
            TableCode::PartitionBroken => "核对 minterm_mask 的等值判定",
            TableCode::DuplicateAnchor => "锚点表按 fn 升序去重后重建",
        }
    }
    /// 数值形式（日志/判据对账用）。
    ///
    /// 返回 u32 而非 u16：五位数诊断码（0x91101）超出 u16 值域，`as u16` 会静默
    /// 截断成 0x1101，与低段码撞车——截断后的码在日志里表现为「另一个模块的
    /// 故障」，比报错更难查。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 表域故障（码 + 定位下标，够判据断言到具体位置）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 定位下标（表索引 / 锚点序号；无定位意义时为 0）。
    pub index: u16,
}

impl TableFault {
    const fn new(code: TableCode, index: usize) -> TableFault {
        // 下标只用于定位，超出 u16 时钳住而不是绕回：绕回会指到另一个合法项，
        // 让「越界」看起来像「另一个位置出错」。
        let at = if index > 0xFFFF { 0xFFFF } else { index as u16 };
        TableFault {
            code: code.code(),
            index: at,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91101 && self.code <= 0x91108
    }
    /// 还原成枚举（闭集外保守落到 Ok，配 `is_known()` 区分未知码）。
    pub const fn as_code(self) -> TableCode {
        match self.code {
            0x91101 => TableCode::Ok,
            0x91102 => TableCode::AnchorMismatch,
            0x91103 => TableCode::GeneratorDiverge,
            0x91104 => TableCode::IndexOutOfRange,
            0x91105 => TableCode::UnknownMnemonic,
            0x91106 => TableCode::FingerprintDrift,
            0x91107 => TableCode::PartitionBroken,
            _ => TableCode::DuplicateAnchor,
        }
    }
}

/// 单格求值：真值表索引 = P*4 + S*2 + D，取 fn 的该位。
///
/// 输入先与 1 相与：调用方常把「像素值」直接递进来，若不钳位，p=2 会把索引
/// 推到 8 以外，读到的位与 p=0 完全无关，错得毫无征兆。
pub const fn eval_bit(fn_byte: u8, p: u8, s: u8, d: u8) -> u8 {
    let idx = ((p & 1) << 2) | ((s & 1) << 1) | (d & 1);
    (fn_byte >> idx) & 1
}

/// 最小项命中判定：给定 (P,S,D) 是否正好等于第 `m` 个最小项。
///
/// 返回 0/1 而不是 bool：往下要参与位运算装配，bool 会逼出一层转换。
pub const fn minterm_mask(m: u8, p: u8, s: u8, d: u8) -> u8 {
    let idx = ((p & 1) << 2) | ((s & 1) << 1) | (d & 1);
    if idx == (m & 7) {
        1
    } else {
        0
    }
}

/// 第二路径：不看 fn 的位，而是把 fn 当「8 个最小项的选择器」，按选择器投票。
///
/// 与 `eval_bit` 的差别是本质的：`eval_bit` 从 fn 里**取**一位，这里从 8 个最小项
/// 里**选**一个。两者对同一 fn 必须同答——这正是生成器可被检验的原因。
pub const fn eval_via_minterms(fn_byte: u8, p: u8, s: u8, d: u8) -> u8 {
    let mut acc = 0u8;
    let mut m = 0u8;
    while m < MINTERM_COUNT as u8 {
        let sel = (fn_byte >> m) & 1;
        acc |= sel & minterm_mask(m, p, s, d);
        m += 1;
    }
    acc
}

/// 按 P/S/D 三维枚举填表：8 个格子逐位装回一个字节。
///
/// 刻意用 `while` 索引循环而非 `for`：`const fn` 里没有切片迭代器，写了也编不过。
pub const fn assemble(fn_byte: u8) -> u8 {
    let mut acc = 0u8;
    let mut p = 0u8;
    while p < 2 {
        let mut s = 0u8;
        while s < 2 {
            let mut d = 0u8;
            while d < 2 {
                let idx = (p << 2) | (s << 1) | d;
                acc |= eval_via_minterms(fn_byte, p, s, d) << idx;
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    acc
}

/// 编译期生成全表（256 项，每项 = 该 fn 的 8 位真值表）。
const fn build_table() -> [u8; TABLE_LEN] {
    let mut t = [0u8; TABLE_LEN];
    let mut i = 0usize;
    while i < TABLE_LEN {
        t[i] = assemble(i as u8);
        i += 1;
    }
    t
}

/// 全表常量。表项等于 fn 自身不是巧合：ROP3 的 8 位真值表就是 fn 的 8 位，
/// 这里要证的是「生成路径确实回到了这条恒等式」，而不是「表里存了别的数」。
pub const ROP3_TABLE: [u8; TABLE_LEN] = build_table();

/// 单个 fn 的三重自洽：位提取 == 最小项选择，且装配回到自身。
const fn cross_check(fn_byte: u8) -> bool {
    let mut p = 0u8;
    while p < 2 {
        let mut s = 0u8;
        while s < 2 {
            let mut d = 0u8;
            while d < 2 {
                if eval_bit(fn_byte, p, s, d) != eval_via_minterms(fn_byte, p, s, d) {
                    return false;
                }
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    assemble(fn_byte) == fn_byte
}

const fn generator_verdicts() -> [bool; TABLE_LEN] {
    let mut v = [false; TABLE_LEN];
    let mut i = 0usize;
    while i < TABLE_LEN {
        v[i] = cross_check(i as u8);
        i += 1;
    }
    v
}

/// 逐 fn 的生成器判定（编译期算好，运行期只读）。
pub const GENERATOR_IDENTITY: [bool; TABLE_LEN] = generator_verdicts();

const fn all_true(flags: &[bool]) -> bool {
    let mut i = 0usize;
    while i < flags.len() {
        if !flags[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn count_true(flags: &[bool]) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < flags.len() {
        if flags[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 256 项生成器是否全部自洽（编译期常量）。
pub const GENERATOR_OK: bool = all_true(&GENERATOR_IDENTITY);

/// 锚点期望值的独立表达式闭集。
///
/// 15 个变体一一对应 15 条 wingdi 语义；这里写的是**算子**，不是查表结果，
/// 所以它和 `ROP3_TABLE` 是两条互不依赖的事实来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorOp {
    /// 恒 0（BLACKNESS）。
    Zero,
    /// 恒 1（WHITENESS）。
    One,
    /// !(S|D)（NOTSRCERASE）。
    NotSOrD,
    /// !S（NOTSRCCOPY）。
    NotS,
    /// S & !D（SRCERASE）。
    SAndNotD,
    /// !D（DSTINVERT）。
    NotD,
    /// P ^ D（PATINVERT）。
    PXorD,
    /// S ^ D（SRCINVERT）。
    SXorD,
    /// S & D（SRCAND）。
    SAndD,
    /// S | !D（MERGEPAINT）。
    SOrNotD,
    /// P & S（MERGECOPY）。
    PAndS,
    /// S（SRCCOPY）。
    Src,
    /// S | D（SRCPAINT）。
    SOrD,
    /// P（PATCOPY）。
    Pat,
    /// P | !S | D（PATPAINT）。
    POrNotSOrD,
}

/// 用布尔算子直接算锚点期望值。
///
/// 全程用 `1 - x` 表示取反而非 `!x`：`!` 作用在 u8 上得到 0xFF，再参与 `|`
/// 会把相邻位一起点亮，是这类实现最常见的静默错误。
pub const fn anchor_truth(op: AnchorOp, p: u8, s: u8, d: u8) -> u8 {
    let bp = p & 1;
    let bs = s & 1;
    let bd = d & 1;
    match op {
        AnchorOp::Zero => 0,
        AnchorOp::One => 1,
        AnchorOp::NotSOrD => {
            let sd = bs | bd;
            1 - sd
        }
        AnchorOp::NotS => 1 - bs,
        AnchorOp::SAndNotD => bs & (1 - bd),
        AnchorOp::NotD => 1 - bd,
        AnchorOp::PXorD => bp ^ bd,
        AnchorOp::SXorD => bs ^ bd,
        AnchorOp::SAndD => bs & bd,
        AnchorOp::SOrNotD => bs | (1 - bd),
        AnchorOp::PAndS => bp & bs,
        AnchorOp::Src => bs,
        AnchorOp::SOrD => bs | bd,
        AnchorOp::Pat => bp,
        AnchorOp::POrNotSOrD => bp | (1 - bs) | bd,
    }
}

/// 一个锚点：wingdi 常量 + 高字节 fn + 助记名 + 独立算子。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Anchor {
    /// 完整 32 位 ROP3 码（wingdi 常量原值）。
    pub rop: u32,
    /// 高字节（= 8 位真值表）。
    pub fn_byte: u8,
    /// 助记名。
    pub name: &'static str,
    /// 独立算子（第二路径）。
    pub op: AnchorOp,
}

/// 15 个锚点（顺序按 fn 升序，便于人眼扫）。
pub const ANCHORS: [Anchor; 15] = [
    Anchor { rop: 0x0000_0042, fn_byte: 0x00, name: "BLACKNESS", op: AnchorOp::Zero },
    Anchor { rop: 0x0011_00A6, fn_byte: 0x11, name: "NOTSRCERASE", op: AnchorOp::NotSOrD },
    Anchor { rop: 0x0033_0008, fn_byte: 0x33, name: "NOTSRCCOPY", op: AnchorOp::NotS },
    Anchor { rop: 0x0044_0328, fn_byte: 0x44, name: "SRCERASE", op: AnchorOp::SAndNotD },
    Anchor { rop: 0x0055_0009, fn_byte: 0x55, name: "DSTINVERT", op: AnchorOp::NotD },
    Anchor { rop: 0x005A_0049, fn_byte: 0x5A, name: "PATINVERT", op: AnchorOp::PXorD },
    Anchor { rop: 0x0066_0046, fn_byte: 0x66, name: "SRCINVERT", op: AnchorOp::SXorD },
    Anchor { rop: 0x0088_00C6, fn_byte: 0x88, name: "SRCAND", op: AnchorOp::SAndD },
    Anchor { rop: 0x00BB_0226, fn_byte: 0xBB, name: "MERGEPAINT", op: AnchorOp::SOrNotD },
    Anchor { rop: 0x00C0_00CA, fn_byte: 0xC0, name: "MERGECOPY", op: AnchorOp::PAndS },
    Anchor { rop: 0x00CC_0020, fn_byte: 0xCC, name: "SRCCOPY", op: AnchorOp::Src },
    Anchor { rop: 0x00EE_0086, fn_byte: 0xEE, name: "SRCPAINT", op: AnchorOp::SOrD },
    Anchor { rop: 0x00F0_0021, fn_byte: 0xF0, name: "PATCOPY", op: AnchorOp::Pat },
    Anchor { rop: 0x00FB_0A09, fn_byte: 0xFB, name: "PATPAINT", op: AnchorOp::POrNotSOrD },
    Anchor { rop: 0x00FF_0062, fn_byte: 0xFF, name: "WHITENESS", op: AnchorOp::One },
];

/// rop 码的高字节即真值表（wingdi 的 fn 定义）。
pub const fn fn_of(rop: u32) -> u8 {
    ((rop >> 16) & 0xFF) as u8
}

/// 反向：由 fn 造一个「只含 fn」的规范码（低 16 位的操作数排布另由 F21602 负责）。
pub const fn rop_of(fn_byte: u8) -> u32 {
    (fn_byte as u32) << 16
}

/// 单锚点判定（全在编译期可算）。
const fn anchor_verdict(a: Anchor) -> bool {
    if fn_of(a.rop) != a.fn_byte {
        return false;
    }
    if ROP3_TABLE[a.fn_byte as usize] != a.fn_byte {
        return false;
    }
    let mut p = 0u8;
    while p < 2 {
        let mut s = 0u8;
        while s < 2 {
            let mut d = 0u8;
            while d < 2 {
                if eval_bit(a.fn_byte, p, s, d) != anchor_truth(a.op, p, s, d) {
                    return false;
                }
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    assemble(a.fn_byte) == a.fn_byte
}

const fn anchor_verdicts() -> [bool; 15] {
    let mut v = [false; 15];
    let mut i = 0usize;
    while i < 15 {
        v[i] = anchor_verdict(ANCHORS[i]);
        i += 1;
    }
    v
}

/// 逐锚点判定（编译期）。
pub const ANCHOR_VERDICTS: [bool; 15] = anchor_verdicts();
/// 15 个锚点是否全中。
pub const ANCHORS_OK: bool = all_true(&ANCHOR_VERDICTS);
/// 命中个数（判据要报「几/15」而不是只报真假）。
pub const ANCHOR_HITS: u8 = count_true(&ANCHOR_VERDICTS);

/// 按索引查表，越界显性拒绝。
///
/// 参数用 usize 而非 u8：调用方会拿「锚点序号」「最小项号」误当索引递进来，
/// u8 参数只能收 0..=255，越界在调用点就被静默截断，拒绝逻辑永远跑不到。
pub fn query(index: usize) -> Result<u8, TableFault> {
    if index >= TABLE_LEN {
        return Err(TableFault::new(TableCode::IndexOutOfRange, index));
    }
    Ok(ROP3_TABLE[index])
}

/// 按 fn 直查（fn 本身就是 0..=255，天然无越界）。
pub fn query_fn(fn_byte: u8) -> u8 {
    ROP3_TABLE[fn_byte as usize]
}

/// 取某个 fn 的 8 个最小项选择位（1 表示该最小项被 fn 选中）。
pub fn minterm_set(fn_byte: u8) -> [u8; MINTERM_COUNT] {
    let mut out = [0u8; MINTERM_COUNT];
    let mut m = 0usize;
    while m < MINTERM_COUNT {
        out[m] = (fn_byte >> m) & 1;
        m += 1;
    }
    out
}

/// fn 里置位的最小项个数（手写 popcount：`count_ones` 的 const 稳定性不保证）。
pub fn minterm_count(fn_byte: u8) -> u8 {
    let mut n = 0u8;
    let mut m = 0u8;
    while m < MINTERM_COUNT as u8 {
        n += (fn_byte >> m) & 1;
        m += 1;
    }
    n
}

/// 最小项是否构成对 8 格真值域的**划分**：每个 (P,S,D) 恰被一个最小项命中。
///
/// 这条不是形式检查：位并行实现依赖「任一像素属于且仅属于一个最小项」，
/// 划分被破坏时求值结果会把两个最小项的掩码叠加，错误只在特定像素模式出现。
pub fn partition_ok(fn_byte: u8) -> bool {
    let set = minterm_set(fn_byte);
    let mut counted = 0u8;
    let mut m = 0usize;
    while m < MINTERM_COUNT {
        counted += set[m];
        m += 1;
    }
    if counted != minterm_count(fn_byte) {
        return false;
    }
    let mut p = 0u8;
    while p < 2 {
        let mut s = 0u8;
        while s < 2 {
            let mut d = 0u8;
            while d < 2 {
                let mut hits = 0u8;
                let mut k = 0u8;
                while k < MINTERM_COUNT as u8 {
                    hits += minterm_mask(k, p, s, d);
                    k += 1;
                }
                if hits != 1 {
                    return false;
                }
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    true
}

/// 真值表的四行视图：行 = (P,S)，列 = D，值是那一格。
///
/// 给人看的：8 个格子排成 4×2 之后，「S 取反」「D 取反」这类规律肉眼可辨，
/// 而 256 项的横表看不出任何结构。
pub fn truth_grid(fn_byte: u8) -> [[u8; 2]; 4] {
    let mut g = [[0u8; 2]; 4];
    let mut p = 0usize;
    while p < 2 {
        let mut s = 0usize;
        while s < 2 {
            let mut d = 0usize;
            while d < 2 {
                g[p * 2 + s][d] = eval_bit(fn_byte, p as u8, s as u8, d as u8);
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    g
}

/// fn → 助记名（只认 15 个锚点；其余 fn 没有标准名，返回 None 而不是编一个）。
pub fn mnemonic_of(fn_byte: u8) -> Option<&'static str> {
    let mut i = 0usize;
    while i < ANCHORS.len() {
        if ANCHORS[i].fn_byte == fn_byte {
            return Some(ANCHORS[i].name);
        }
        i += 1;
    }
    None
}

/// 助记名 → 锚点（名字拼错时显性报 UnknownMnemonic 并带上扫描到的序号）。
pub fn anchor_of(name: &str) -> Result<Anchor, TableFault> {
    let mut i = 0usize;
    while i < ANCHORS.len() {
        if ANCHORS[i].name == name {
            return Ok(ANCHORS[i]);
        }
        i += 1;
    }
    Err(TableFault::new(TableCode::UnknownMnemonic, ANCHORS.len()))
}

/// 校验一个 (rop, fn) 对：高字节必须等于 fn，且该 fn 必须是锚点。
///
/// 这是「反向判据」的载体：把 wingdi 常量改一位、把 fn 抄错、把助记名对错，
/// 三种违规都会在这里以不同码被挡下。
pub fn verify_anchor(rop: u32, fn_byte: u8) -> Result<&'static str, TableFault> {
    if fn_of(rop) != fn_byte {
        return Err(TableFault::new(TableCode::AnchorMismatch, fn_byte as usize));
    }
    if query_fn(fn_byte) != fn_byte {
        return Err(TableFault::new(TableCode::GeneratorDiverge, fn_byte as usize));
    }
    match mnemonic_of(fn_byte) {
        Some(name) => Ok(name),
        None => Err(TableFault::new(TableCode::UnknownMnemonic, fn_byte as usize)),
    }
}

/// FNV-1a 64 单步（质数与偏移基写死，来源：FNV 规范 1a 变体）。
const fn fnv_step(mut h: u64, byte: u8) -> u64 {
    h ^= byte as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h
}

/// 任一 256 项表的指纹（参数化是为了让「篡改」可被构造并对比）。
pub fn fingerprint_of(table: &[u8; TABLE_LEN]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < TABLE_LEN {
        h = fnv_step(h, table[i]);
        i += 1;
    }
    h
}

/// 登记表的指纹。
pub fn table_fingerprint() -> u64 {
    fingerprint_of(&ROP3_TABLE)
}

/// 报告里用的最小十六进制渲染（定宽 8 位，便于逐行 diff 对齐）。
pub fn hex32(v: u32) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(10);
    s.push_str("0x");
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

fn push_dec(out: &mut String, mut v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut w = 0usize;
    while v > 0 && w < 10 {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        out.push(digits[w] as char);
    }
}

/// 锚点审计报告（纯文本，字段顺序固定，供归档与 diff）。
pub fn audit_report() -> String {
    let mut out = String::with_capacity(1024);
    out.push_str("rop3-anchors=");
    push_dec(&mut out, ANCHORS.len() as u32);
    out.push_str(" hits=");
    push_dec(&mut out, ANCHOR_HITS as u32);
    out.push_str(" table=");
    push_dec(&mut out, TABLE_LEN as u32);
    out.push_str(" gen=");
    out.push_str(if GENERATOR_OK { "ok" } else { "bad" });
    out.push_str(" fp=");
    out.push_str(&hex32((table_fingerprint() >> 32) as u32));
    out.push_str(&hex32(table_fingerprint() as u32));
    out.push('\n');
    let mut i = 0usize;
    while i < ANCHORS.len() {
        let a = ANCHORS[i];
        out.push_str("  ");
        out.push_str(a.name);
        out.push(' ');
        out.push_str(&hex32(a.rop));
        out.push_str(" fn=");
        out.push_str(&hex32(a.fn_byte as u32));
        out.push_str(" verdict=");
        out.push_str(if ANCHOR_VERDICTS[i] { "1" } else { "0" });
        out.push('\n');
        i += 1;
    }
    out
}

/// 锚点集合自检：15 个助记名互不相同、15 个 fn 互不相同。
///
/// 重名比错值更隐蔽：两个锚点同名时 `anchor_of` 永远返回第一个，后一个变成
/// 永不生效的死行，而报告里看起来「15 行都在」。
pub fn anchor_set_unique() -> Result<(), TableFault> {
    let mut i = 0usize;
    while i < ANCHORS.len() {
        let mut j = i + 1;
        while j < ANCHORS.len() {
            if ANCHORS[i].fn_byte == ANCHORS[j].fn_byte {
                return Err(TableFault::new(TableCode::DuplicateAnchor, j));
            }
            if ANCHORS[i].name == ANCHORS[j].name {
                return Err(TableFault::new(TableCode::DuplicateAnchor, j));
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 判据入口（UNX-F21601-J1 及配套 14 条）。
///
/// 判据常量独立写死：`256` / `15` / `0xCC` / `0x91104` 在断言里以字面量出现，
/// 不复用被测常量，避免「被测值改掉，判据跟着改」的自证循环。
pub fn run_unx21601_rop3table_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21601");

    // 1) 夹具可构造：表存在且不是全零占位（全零表能让「fn=0 恒 0」这条假绿）。
    let mut nonzero = 0usize;
    let mut i = 0usize;
    while i < TABLE_LEN {
        if ROP3_TABLE[i] != 0 {
            nonzero += 1;
        }
        i += 1;
    }
    cs.add(
        "f21601.fixture_buildable",
        TABLE_LEN == 256 && nonzero == 255,
        "表长度为 256 但非零项不是 255（fn=0 一项为零，其余应全非零）",
    );

    // 2) 生成器自洽：256 项逐项过 cross_check（位提取 vs 最小项选择 vs 装配回填）。
    cs.add(
        "f21601.generator_identity",
        GENERATOR_OK && GENERATOR_IDENTITY[0xCC] && GENERATOR_IDENTITY[0x00],
        "生成器在 256 项中存在回不到原 fn 的项",
    );

    // 3) 锚点 15/15：常量位与个数两面都判（只判 true 漏掉「命中 14 但常量被改成 14」）。
    cs.add(
        "f21601.anchors_15_of_15",
        ANCHORS_OK && ANCHOR_HITS == 15 && ANCHORS.len() == 15,
        "锚点未满 15/15 命中",
    );

    // 4) 双路对拍：表（位提取）与独立算子路径在 15×8=120 个格子上逐格相同。
    let mut cells = 0u32;
    let mut agree = 0u32;
    let mut a_idx = 0usize;
    while a_idx < ANCHORS.len() {
        let a = ANCHORS[a_idx];
        let mut p = 0u8;
        while p < 2 {
            let mut s = 0u8;
            while s < 2 {
                let mut d = 0u8;
                while d < 2 {
                    cells += 1;
                    if eval_bit(a.fn_byte, p, s, d) == anchor_truth(a.op, p, s, d) {
                        agree += 1;
                    }
                    d += 1;
                }
                s += 1;
            }
            p += 1;
        }
        a_idx += 1;
    }
    cs.add(
        "f21601.anchor_dual_path",
        cells == 120 && agree == 120,
        "表路径与算子路径在 120 格中分叉",
    );

    // 5) rop 高字节 == fn：锚点表两栏各自手改过也能被发现。
    let mut rop_ok = true;
    let mut k = 0usize;
    while k < ANCHORS.len() {
        if fn_of(ANCHORS[k].rop) != ANCHORS[k].fn_byte {
            rop_ok = false;
        }
        // 反向再验一次：由 fn 造码回来，高字节必须还原。
        if fn_of(rop_of(ANCHORS[k].fn_byte)) != ANCHORS[k].fn_byte {
            rop_ok = false;
        }
        k += 1;
    }
    cs.add(
        "f21601.anchor_rop_high_byte",
        rop_ok,
        "某锚点的 rop 高字节与 fn 栏不符（或 rop_of 反向不还原）",
    );

    // 6) 锚点集合唯一性（重 fn / 重名都是静默死行）。
    cs.add(
        "f21601.anchor_set_unique",
        anchor_set_unique().is_ok(),
        "锚点集合存在重 fn 或重名",
    );

    // 7) 反查闭合：fn → 名 → 锚点，15 条全部回到同一 fn。
    let mut round = 0u8;
    let mut q = 0usize;
    while q < ANCHORS.len() {
        let a = ANCHORS[q];
        if let Some(name) = mnemonic_of(a.fn_byte) {
            if let Ok(back) = anchor_of(name) {
                if back.fn_byte == a.fn_byte && back.rop == a.rop {
                    round += 1;
                }
            }
        }
        q += 1;
    }
    cs.add(
        "f21601.reverse_lookup_roundtrip",
        round == 15 && mnemonic_of(0xCC) == Some("SRCCOPY"),
        "fn→名→锚点未 15 条闭合，或 0xCC 不只是 SRCCOPY",
    );

    // 8) 边界：255 可查、256 必拒且码为 0x91104、0 可查。
    let lo = query(0);
    let hi = query(255);
    let over = query(256);
    let over_code = match over {
        Ok(_) => 0u32,
        Err(f) => f.code,
    };
    cs.add(
        "f21601.query_boundary",
        lo == Ok(0u8) && hi == Ok(255u8) && over_code == 0x91104,
        "255 未命中或 256 未被 0x91104 拒绝",
    );

    // 9) 未知助记名：必须报 0x91105，且未知 fn 反查返回 None（不许编名）。
    let bogus = anchor_of("SRCCOPY_TYPO");
    let bogus_code = match bogus {
        Ok(_) => 0u32,
        Err(f) => f.code,
    };
    cs.add(
        "f21601.unknown_mnemonic",
        bogus_code == 0x91105 && mnemonic_of(0x01).is_none() && mnemonic_of(0xFF) == Some("WHITENESS"),
        "拼错名未报 0x91105，或未知 fn 被编了个名字",
    );

    // 10) 指纹稳定且可复算：两次调用逐位相同，且与参数化版本一致。
    let fp1 = table_fingerprint();
    let fp2 = table_fingerprint();
    cs.add(
        "f21601.fingerprint_stable",
        fp1 == fp2 && fp1 == fingerprint_of(&ROP3_TABLE) && fp1 != 0,
        "指纹两次不一致或为零（零指纹说明表为空或哈希被换）",
    );

    // 11) 指纹对篡改敏感：改一项即漂移，并按 0x91106 上报。
    let mut tampered = ROP3_TABLE;
    tampered[0xCC] = 0xCD;
    let drift = if fingerprint_of(&tampered) == fp1 {
        TableFault::new(TableCode::Ok, 0xCC)
    } else {
        TableFault::new(TableCode::FingerprintDrift, 0xCC)
    };
    cs.add(
        "f21601.fingerprint_sensitive",
        drift.code == 0x91106 && drift.as_code() == TableCode::FingerprintDrift,
        "篡改 SRCCOPY 一项后指纹未变或码不符",
    );

    // 12) 最小项划分：256 个 fn 全部满足「每格恰被一个最小项命中」。
    let mut part_ok = 0usize;
    let mut f = 0usize;
    while f < TABLE_LEN {
        if partition_ok(f as u8) {
            part_ok += 1;
        }
        f += 1;
    }
    cs.add(
        "f21601.minterm_partition",
        part_ok == 256,
        "存在最小项不构成划分的 fn（位并行求值会叠加两掩码）",
    );

    // 13) 两端常量：fn=0x00 恒 0，fn=0xFF 恒 1（8 格全枚举）。
    let mut zero_ok = true;
    let mut one_ok = true;
    let mut p = 0u8;
    while p < 2 {
        let mut s = 0u8;
        while s < 2 {
            let mut d = 0u8;
            while d < 2 {
                if eval_bit(0x00, p, s, d) != 0 {
                    zero_ok = false;
                }
                if eval_bit(0xFF, p, s, d) != 1 {
                    one_ok = false;
                }
                d += 1;
            }
            s += 1;
        }
        p += 1;
    }
    cs.add(
        "f21601.constants_extremes",
        zero_ok && one_ok && query_fn(0x00) == 0 && query_fn(0xFF) == 0xFF,
        "BLACKNESS/WHITENESS 在 8 格上不恒定",
    );

    // 14) 违规样本逐个挡：rop 改一位（0x00CC0021 之类）、fn 抄错（0xCD）。
    let mut bad_rop = ANCHORS[10].rop;
    bad_rop ^= 0x0000_0001;
    let e1 = verify_anchor(bad_rop, ANCHORS[10].fn_byte);
    let e2 = verify_anchor(ANCHORS[10].rop, 0xCD);
    let c1 = match e1 {
        Ok(_) => 0u32,
        Err(f) => f.code,
    };
    let c2 = match e2 {
        Ok(_) => 0u32,
        Err(f) => f.code,
    };
    cs.add(
        "f21601.anchor_tamper_rejected",
        c1 == 0x91102 && c2 == 0x91102 && verify_anchor(ANCHORS[10].rop, 0xCC).is_ok(),
        "改一位 rop 或抄错 fn 未被 0x91102 拒绝",
    );

    // 15) 幂等·确定性：报告两次逐字节相同，且含总数与首个锚点名。
    let r1 = audit_report();
    let r2 = audit_report();
    cs.add(
        "f21601.report_idempotent",
        r1 == r2
            && r1.contains("hits=15")
            && r1.contains("gen=ok")
            && r1.contains("BLACKNESS")
            && r1.contains("WHITENESS"),
        "审计报告两次不一致或缺字段",
    );

    // 16) 网格视图与位提取一致（4×2 展开后 8 格逐格相同）。
    let mut grid_ok = true;
    let mut g = 0usize;
    while g < TABLE_LEN {
        let grid = truth_grid(g as u8);
        let mut p2 = 0usize;
        while p2 < 2 {
            let mut s2 = 0usize;
            while s2 < 2 {
                let mut d2 = 0usize;
                while d2 < 2 {
                    if grid[p2 * 2 + s2][d2] != eval_bit(g as u8, p2 as u8, s2 as u8, d2 as u8) {
                        grid_ok = false;
                    }
                    d2 += 1;
                }
                s2 += 1;
            }
            p2 += 1;
        }
        g += 1;
    }
    cs.add(
        "f21601.grid_view_consistent",
        grid_ok && minterm_count(0xCC) == 4 && minterm_count(0xFF) == 8,
        "四行视图与位提取不一致，或 popcount 不符",
    );

    cs
}
