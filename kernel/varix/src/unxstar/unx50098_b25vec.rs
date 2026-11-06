//! UNX-F50098 B25 批向量账骨架（字体用例集生成器 + 校验和）（0x43860 段）
//!
//! 锚点：UNX-F50098-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43860`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 12`，
//! 逐项见 [`VecError`]。
//!
//! 批向量账要解决的不是「造几条用例」，而是「凭什么叫人相信这批用例没被
//! 改动过」。做法是把用例分成两层：
//!
//! ```text
//! 种子 --CaseGen(LCG)--> CaseRec --wire--> RawCase --parse+checksum--> VecLedger
//! ```
//!
//! * 生成器是**纯函数式**的：种子相同、序号相同，逐位相同（判据 J1-1）；
//! * 每条用例带 FNV-1a 校验和，覆盖 id / kind / upem / glyphs / seed 五个
//!   字段的规范字节序编码——任何一个字段被改，校验和都对不上（J1-3）；
//! * 装载是**事务性**的：任一条坏样本都让整批拒收，账本保持 0 条，绝不
//!   留下「半批已入账」的中间态（J1-6）；
//! * id 必须从 1 起连续升序，重复与跳号分别报 `Duplicate` / `Order`，
//!   这样「少一条」和「多一条」都无法伪装成另一个长度。
//!
//! 结构清单：
//! * [`CaseKind`] / [`CaseRec`]：六类字体用例与用例记录；
//! * [`CaseGen`] / [`GenState`]：确定性生成器状态机（Idle→Generating→Done）；
//! * [`RawCase`] / [`VecLedger`]：线上格式、解析 + 校验和、账本（装载 / 校验 /
//!   冻结 / 摘要）；
//! * [`injection_core`]：12 类坏样本注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43860 独占 0x20）。
pub const SEG_BASE: u32 = 0x43860;

const E_EMPTY: u16 = 1;
const E_KIND: u16 = 2;
const E_UPEM: u16 = 3;
const E_GLYPHS: u16 = 4;
const E_CHECKSUM: u16 = 5;
const E_DUPLICATE: u16 = 6;
const E_ORDER: u16 = 7;
const E_CAPACITY: u16 = 8;
const E_COUNT: u16 = 9;
const E_SEED: u16 = 10;
const E_FROZEN: u16 = 11;
const E_STATE: u16 = 12;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    /// 批次为空。
    Empty,
    /// 用例类别 tag 不在闭集内。
    Kind,
    /// upem 不是 1000 / 2048。
    Upem,
    /// 字形数不在 1..=1024。
    Glyphs,
    /// 校验和与规范编码不符。
    Checksum,
    /// 用例 id 重复。
    Duplicate,
    /// 用例 id 不是「上一个 + 1」。
    Order,
    /// 账本或批次已满。
    Capacity,
    /// 条数与声明的 expected 不符。
    Count,
    /// 种子为 0（0 是「未播种」哨兵，不许当种子用）。
    Seed,
    /// 账本已冻结，只读。
    Frozen,
    /// 生成器状态机不接受当前操作。
    State,
}

impl VecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Kind => E_KIND,
            VecError::Upem => E_UPEM,
            VecError::Glyphs => E_GLYPHS,
            VecError::Checksum => E_CHECKSUM,
            VecError::Duplicate => E_DUPLICATE,
            VecError::Order => E_ORDER,
            VecError::Capacity => E_CAPACITY,
            VecError::Count => E_COUNT,
            VecError::Seed => E_SEED,
            VecError::Frozen => E_FROZEN,
            VecError::State => E_STATE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志与串口共用）。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "vector batch empty",
            VecError::Kind => "case kind tag outside closed set",
            VecError::Upem => "upem is not 1000 or 2048",
            VecError::Glyphs => "glyph count outside 1..=1024",
            VecError::Checksum => "case checksum mismatch",
            VecError::Duplicate => "case id registered twice",
            VecError::Order => "case id not previous + 1",
            VecError::Capacity => "vector ledger full",
            VecError::Count => "case count differs from expected",
            VecError::Seed => "generator seed zero",
            VecError::Frozen => "vector ledger frozen",
            VecError::State => "generator state rejects this call",
        }
    }
}

/// 单批最大用例数。64 条足以覆盖六类 × 两种 upem × 四档字形量，
/// 再大就不是「骨架」而是灌水。
pub const MAX_CASES: usize = 64;
/// 用例类别数。
pub const KIND_COUNT: u8 = 6;
/// 校验和初值（FNV-1a 32 位 offset basis）。
const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 32 位质数。
const FNV_PRIME: u32 = 0x0100_0193;

/// 六类字体用例。类别决定这条用例在 B25 里压的是哪条路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 度量一致性（字距 / 行高 / upem 归一）。
    Metric,
    /// 子集化（字符集裁剪 + cmap 重建）。
    Subset,
    /// 嵌入（TrueType / Type1 双格式走 profile 决策）。
    Embed,
    /// 下载（PCL / PS 通道的分块与超时）。
    Download,
    /// 许可证（嵌入位 os2.fsType 判定）。
    License,
    /// 替换（缺失字体的显式映射）。
    Subst,
}

impl CaseKind {
    /// 线上 tag（1..=6，0 保留为「未定」）。
    pub fn tag(self) -> u8 {
        match self {
            CaseKind::Metric => 1,
            CaseKind::Subset => 2,
            CaseKind::Embed => 3,
            CaseKind::Download => 4,
            CaseKind::License => 5,
            CaseKind::Subst => 6,
        }
    }

    /// tag 反解。闭集之外一律显式拒绝，不做「就近归类」。
    pub fn from_tag(t: u8) -> Result<CaseKind, VecError> {
        match t {
            1 => Ok(CaseKind::Metric),
            2 => Ok(CaseKind::Subset),
            3 => Ok(CaseKind::Embed),
            4 => Ok(CaseKind::Download),
            5 => Ok(CaseKind::License),
            6 => Ok(CaseKind::Subst),
            _ => Err(VecError::Kind),
        }
    }

    /// 报告用 slug。
    pub fn slug(self) -> &'static str {
        match self {
            CaseKind::Metric => "metric",
            CaseKind::Subset => "subset",
            CaseKind::Embed => "embed",
            CaseKind::Download => "download",
            CaseKind::License => "license",
            CaseKind::Subst => "subst",
        }
    }
}

/// 一条用例记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseRec {
    /// 用例序号（1 起连续）。
    pub id: u16,
    /// 类别。
    pub kind: CaseKind,
    /// 目标 upem（1000 / 2048）。
    pub upem: u16,
    /// 用例用到的字形数（1..=1024）。
    pub glyphs: u16,
    /// 派生该用例的种子（同批一致）。
    pub seed: u32,
    /// 规范编码的 FNV-1a 校验和。
    pub checksum: u32,
}

/// 校验和的规范字节序：id(2) kind(1) upem(2) glyphs(2) seed(4) = 11 字节。
///
/// 为什么不用 `#[repr(C)]` 直接哈希结构体：结构体布局含填充与 enum 判别
/// 宽度，跨编译器/跨版本不稳定；写死字段序才是可复算的口径。
pub fn checksum_of(id: u16, kind_tag: u8, upem: u16, glyphs: u16, seed: u32) -> u32 {
    let mut b = [0u8; 11];
    b[0] = (id >> 8) as u8;
    b[1] = id as u8;
    b[2] = kind_tag;
    b[3] = (upem >> 8) as u8;
    b[4] = upem as u8;
    b[5] = (glyphs >> 8) as u8;
    b[6] = glyphs as u8;
    b[7] = (seed >> 24) as u8;
    b[8] = (seed >> 16) as u8;
    b[9] = (seed >> 8) as u8;
    b[10] = seed as u8;
    fnv_bytes(FNV_OFFSET, &b)
}

fn fnv_bytes(h: u32, bytes: &[u8]) -> u32 {
    let mut out = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            out = (out ^ *b as u32).wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    out
}

/// 一条用例的线上形态。字段与 [`CaseRec`] 一一对应，只是类别用 tag 承载，
/// 便于「先解析后信任」：解析成功即代表校验和已复算通过。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawCase {
    /// 用例序号。
    pub id: u16,
    /// 类别 tag。
    pub kind_tag: u8,
    /// 目标 upem。
    pub upem: u16,
    /// 字形数。
    pub glyphs: u16,
    /// 种子。
    pub seed: u32,
    /// 声称的校验和。
    pub checksum: u32,
}

impl RawCase {
    /// 由用例记录导出线上形态（校验和原样带走，解析时会复算）。
    pub fn from_case(c: CaseRec) -> RawCase {
        RawCase {
            id: c.id,
            kind_tag: c.kind.tag(),
            upem: c.upem,
            glyphs: c.glyphs,
            seed: c.seed,
            checksum: c.checksum,
        }
    }

    /// 解析 + 校验：类别闭集 → upem 口径 → 字形量 → 校验和。
    ///
    /// 校验和放最后，是因为它覆盖前三项的规范编码——前三项都合法时再比
    /// 校验和，报错才不会把「字段非法」误报成「校验和不符」。
    pub fn parse(&self) -> Result<CaseRec, VecError> {
        let kind = CaseKind::from_tag(self.kind_tag)?;
        if self.upem != 1000 && self.upem != 2048 {
            return Err(VecError::Upem);
        }
        if self.glyphs == 0 || self.glyphs > 1024 {
            return Err(VecError::Glyphs);
        }
        let want = checksum_of(self.id, self.kind_tag, self.upem, self.glyphs, self.seed);
        if want != self.checksum {
            return Err(VecError::Checksum);
        }
        Ok(CaseRec {
            id: self.id,
            kind,
            upem: self.upem,
            glyphs: self.glyphs,
            seed: self.seed,
            checksum: self.checksum,
        })
    }
}

/// 生成器状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenState {
    /// 未播种：任何 `next` 都是状态错误。
    Idle,
    /// 生成中。
    Generating,
    /// 已收尾：再 `next` 仍是状态错误（防止悄悄多造用例）。
    Done,
}

/// 确定性用例生成器（线性同余，参数取 Numerical Recipes 常数）。
///
/// 选 LCG 而不是哈希：LCG 的状态转移是**可手算**的，任何一条用例都能由
/// 「种子 + 序号」独立复现，不需要重放前 n 条。
#[derive(Clone, Debug)]
pub struct CaseGen {
    state: GenState,
    seed: u32,
    cursor: u16,
    produced: u32,
}

impl CaseGen {
    /// 空生成器（Idle）。
    pub fn new() -> CaseGen {
        CaseGen { state: GenState::Idle, seed: 0, cursor: 0, produced: 0 }
    }

    /// 播种开跑。种子 0 是「未播种」哨兵，显式拒绝。
    pub fn start(seed: u32) -> Result<CaseGen, VecError> {
        if seed == 0 {
            return Err(VecError::Seed);
        }
        Ok(CaseGen { state: GenState::Generating, seed, cursor: 0, produced: 0 })
    }

    /// 当前状态。
    pub fn state(&self) -> GenState {
        self.state
    }

    /// 已产生条数。
    pub fn produced(&self) -> u32 {
        self.produced
    }

    /// 下一条用例。状态不是 `Generating` 时报 `State`，不返回占位用例。
    pub fn next_case(&mut self) -> Result<CaseRec, VecError> {
        if self.state != GenState::Generating {
            return Err(VecError::State);
        }
        if self.produced as usize >= MAX_CASES {
            return Err(VecError::Capacity);
        }
        let id = self.cursor + 1;
        // 第 n 条的状态 = LCG^n(seed)：随机量只由 (seed, n) 决定。
        let mut x = self.seed;
        let mut n = 0u16;
        while n < id {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            n += 1;
        }
        let kind_tag = (x % KIND_COUNT as u32) as u8 + 1;
        let kind = CaseKind::from_tag(kind_tag)?;
        let upem = if (x >> 8) & 1 == 0 { 1000 } else { 2048 };
        let glyphs = 16 + (x % 497) as u16;
        let checksum = checksum_of(id, kind_tag, upem, glyphs, self.seed);
        self.cursor = id;
        self.produced += 1;
        Ok(CaseRec { id, kind, upem, glyphs, seed: self.seed, checksum })
    }

    /// 收尾。Idle 下收尾也算状态错误（没开跑就没有「收尾」）。
    pub fn finish(&mut self) -> Result<u32, VecError> {
        if self.state != GenState::Generating {
            return Err(VecError::State);
        }
        self.state = GenState::Done;
        Ok(self.produced)
    }
}

impl Default for CaseGen {
    fn default() -> CaseGen {
        CaseGen::new()
    }
}

/// 按种子生成 `count` 条用例（等价于 `start` + count 次 `next` + `finish`）。
pub fn gen_cases(seed: u32, count: usize) -> Result<Vec<CaseRec>, VecError> {
    if count == 0 {
        return Err(VecError::Empty);
    }
    if count > MAX_CASES {
        return Err(VecError::Capacity);
    }
    let mut g = CaseGen::start(seed)?;
    let mut out = Vec::with_capacity(count);
    let mut i = 0usize;
    while i < count {
        out.push(g.next_case()?);
        i += 1;
    }
    g.finish()?;
    Ok(out)
}

/// 向量账：装载 → 校验 → 冻结。
#[derive(Clone, Debug)]
pub struct VecLedger {
    cases: Vec<CaseRec>,
    expected: usize,
    frozen: bool,
    verify_runs: u32,
}

impl VecLedger {
    /// 新建账本。`expected == 0` 表示「不限定条数」。
    pub fn new(expected: usize) -> Result<VecLedger, VecError> {
        if expected > MAX_CASES {
            return Err(VecError::Capacity);
        }
        Ok(VecLedger {
            cases: Vec::with_capacity(if expected == 0 { MAX_CASES } else { expected }),
            expected,
            frozen: false,
            verify_runs: 0,
        })
    }

    /// 追加一条用例：冻结 / 容量 / 期望条数 / 重复 / 升序逐项校验。
    pub fn push(&mut self, c: CaseRec) -> Result<(), VecError> {
        if self.frozen {
            return Err(VecError::Frozen);
        }
        if self.cases.len() >= MAX_CASES {
            return Err(VecError::Capacity);
        }
        if self.expected != 0 && self.cases.len() >= self.expected {
            return Err(VecError::Count);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(e) = self.cases.get(i) {
                if e.id == c.id {
                    return Err(VecError::Duplicate);
                }
            }
            i += 1;
        }
        if c.id as usize != self.cases.len() + 1 {
            return Err(VecError::Order);
        }
        // 入账前复算校验和：账本里的每一条都必须自证完整。
        let want = checksum_of(c.id, c.kind.tag(), c.upem, c.glyphs, c.seed);
        if want != c.checksum {
            return Err(VecError::Checksum);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 事务性装载：先解析全部坏样本，全部通过才写账——失败绝不留下半批。
    pub fn load(raws: &[RawCase], expected: usize) -> Result<VecLedger, VecError> {
        if raws.is_empty() {
            return Err(VecError::Empty);
        }
        if raws.len() != expected {
            return Err(VecError::Count);
        }
        let mut led = VecLedger::new(expected)?;
        let mut i = 0usize;
        while i < raws.len() {
            let raw = match raws.get(i) {
                Some(r) => r,
                None => return Err(VecError::Empty),
            };
            let rec = raw.parse()?;
            led.push(rec)?;
            i += 1;
        }
        Ok(led)
    }

    /// 逐条复核：校验和 + id 序列。幂等，返回条数。
    pub fn verify(&mut self) -> Result<usize, VecError> {
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(VecError::Empty),
            };
            if c.id as usize != i + 1 {
                return Err(VecError::Order);
            }
            let want = checksum_of(c.id, c.kind.tag(), c.upem, c.glyphs, c.seed);
            if want != c.checksum {
                return Err(VecError::Checksum);
            }
            i += 1;
        }
        if self.expected != 0 && self.cases.len() != self.expected {
            return Err(VecError::Count);
        }
        self.verify_runs += 1;
        Ok(self.cases.len())
    }

    /// 冻结：先复核再置只读。冻结后任何写入都是 `Frozen`。
    pub fn freeze(&mut self) -> Result<usize, VecError> {
        if self.frozen {
            return Err(VecError::Frozen);
        }
        let n = self.verify()?;
        self.frozen = true;
        Ok(n)
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 读一条。
    pub fn case(&self, idx: usize) -> Option<CaseRec> {
        self.cases.get(idx).copied()
    }

    /// 全账摘要：逐条折叠规范编码（与校验和同口径，便于交叉复核）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                let mut b = [0u8; 11];
                b[0] = (c.id >> 8) as u8;
                b[1] = c.id as u8;
                b[2] = c.kind.tag();
                b[3] = (c.upem >> 8) as u8;
                b[4] = c.upem as u8;
                b[5] = (c.glyphs >> 8) as u8;
                b[6] = c.glyphs as u8;
                b[7] = (c.seed >> 24) as u8;
                b[8] = (c.seed >> 16) as u8;
                b[9] = (c.seed >> 8) as u8;
                b[10] = c.seed as u8;
                h = fnv_bytes(h, &b);
            }
            i += 1;
        }
        h
    }

    /// 复核次数（幂等判据用）。
    pub fn verify_runs(&self) -> u32 {
        self.verify_runs
    }

    /// 一行报告（结果摘要非空，供软件写回）。
    pub fn report(&self) -> String {
        format!(
            "B25 vec cases={}/{} frozen={} digest={:08x}",
            self.cases.len(),
            self.expected,
            self.frozen as u8,
            self.digest()
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn raws_of(cases: &[CaseRec]) -> Vec<RawCase> {
    let mut v = Vec::with_capacity(cases.len());
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            v.push(RawCase::from_case(*c));
        }
        i += 1;
    }
    v
}

fn sample(n: usize) -> Vec<CaseRec> {
    match gen_cases(0x5EED_1234, n) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    }
}

/// 坏样本注入核：12 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;
    let sample6 = sample(6);

    // 1 空批
    if VecLedger::load(&[], 0).err() == Some(VecError::Empty) {
        caught += 1;
    }
    // 2 种子为 0
    if CaseGen::start(0).err() == Some(VecError::Seed) {
        caught += 1;
    }
    // 3 未播种就取用例
    let mut idle = CaseGen::new();
    if idle.next_case().err() == Some(VecError::State) {
        caught += 1;
    }
    // 4 类别 tag 越界
    let mut bad_kind = raws_of(&sample6);
    if let Some(r) = bad_kind.get_mut(0) {
        r.kind_tag = 9;
    }
    if VecLedger::load(&bad_kind, 6).err() == Some(VecError::Kind) {
        caught += 1;
    }
    // 5 upem 非法（先把校验和改对，确保命中的是 Upem 而不是 Checksum）
    let mut bad_upem = raws_of(&sample6);
    if let Some(r) = bad_upem.get_mut(1) {
        r.upem = 900;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    if VecLedger::load(&bad_upem, 6).err() == Some(VecError::Upem) {
        caught += 1;
    }
    // 6 字形数为 0（同样先把校验和改对）
    let mut bad_glyphs = raws_of(&sample6);
    if let Some(r) = bad_glyphs.get_mut(2) {
        r.glyphs = 0;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    if VecLedger::load(&bad_glyphs, 6).err() == Some(VecError::Glyphs) {
        caught += 1;
    }
    // 7 校验和被篡改（只改一位）
    let mut bad_sum = raws_of(&sample6);
    if let Some(r) = bad_sum.get_mut(3) {
        r.checksum ^= 1;
    }
    if VecLedger::load(&bad_sum, 6).err() == Some(VecError::Checksum) {
        caught += 1;
    }
    // 8 id 重复
    let mut dup = raws_of(&sample6);
    let first = dup.get(0).copied();
    if let (Some(a), Some(b)) = (dup.get_mut(1), first) {
        *a = b;
    }
    if VecLedger::load(&dup, 6).err() == Some(VecError::Duplicate) {
        caught += 1;
    }
    // 9 id 跳号（第 2 条 id 改成 5，校验和同步改对）
    let mut skip = raws_of(&sample6);
    if let Some(r) = skip.get_mut(1) {
        r.id = 5;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    if VecLedger::load(&skip, 6).err() == Some(VecError::Order) {
        caught += 1;
    }
    // 10 条数与声明不符
    if VecLedger::load(&raws_of(&sample6), 7).err() == Some(VecError::Count) {
        caught += 1;
    }
    // 11 容量满（不限定 expected 时第 65 条越界）
    let mut over = match VecLedger::new(0) {
        Ok(v) => v,
        Err(_) => VecLedger { cases: Vec::new(), expected: 0, frozen: true, verify_runs: 0 },
    };
    let big = sample(MAX_CASES);
    let mut k = 0usize;
    while k < big.len() {
        if let Some(c) = big.get(k) {
            let _ = over.push(*c);
        }
        k += 1;
    }
    let extra = CaseRec { id: 65, kind: CaseKind::Metric, upem: 1000, glyphs: 32, seed: 1, checksum: 0 };
    if over.push(extra).err() == Some(VecError::Capacity) {
        caught += 1;
    }
    // 12 冻结后写入
    let mut frozen = match VecLedger::load(&raws_of(&sample6), 6) {
        Ok(v) => v,
        Err(_) => VecLedger { cases: Vec::new(), expected: 0, frozen: true, verify_runs: 0 },
    };
    let _ = frozen.freeze();
    let one = CaseRec { id: 7, kind: CaseKind::Subst, upem: 1000, glyphs: 32, seed: 1, checksum: 0 };
    if frozen.push(one).err() == Some(VecError::Frozen) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50098 的 14 条判据（含 J1 用例带校验和落账与注入核）。
pub fn run_unx50098_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50098");

    // J1-1 生成器确定性：同种子两次生成，逐字段相同。
    let a = gen_cases(0x1234_5678, 12);
    let b = gen_cases(0x1234_5678, 12);
    let mut ok = true;
    let mut same = 0usize;
    match (&a, &b) {
        (Ok(x), Ok(y)) => {
            if x.len() != y.len() {
                ok = false;
            }
            let mut i = 0usize;
            while i < x.len() && i < y.len() {
                match (x.get(i), y.get(i)) {
                    (Some(p), Some(q)) => {
                        if p != q {
                            ok = false;
                        } else {
                            same += 1;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
        }
        _ => ok = false,
    }
    let det = format!("same={}", same);
    s.add_dyn("50098-J1-生成确定", ok && same == 12, &det);

    let cases = match a {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };

    // J1-2 序号连续 + 类别覆盖：id 必须是 1..=n，且六类至少出现 3 类。
    let mut ids_ok = cases.len() == 12;
    let mut seen = [false; 6];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            if c.id as usize != i + 1 {
                ids_ok = false;
            }
            let tag = c.kind.tag();
            if let Some(slot) = seen.get_mut((tag - 1) as usize) {
                *slot = true;
            }
            if c.upem != 1000 && c.upem != 2048 {
                ids_ok = false;
            }
        }
        i += 1;
    }
    let mut kinds = 0usize;
    let mut j = 0usize;
    while j < seen.len() {
        if let Some(v) = seen.get(j) {
            if *v {
                kinds += 1;
            }
        }
        j += 1;
    }
    let det = format!("ids_ok={} kinds={}", ids_ok as u8, kinds);
    s.add_dyn("50098-J1-序号与覆盖", ids_ok && kinds >= 3, &det);

    // J1-3 校验和可复算：账本里每条的 checksum 与规范编码重算逐位一致。
    let mut bad = 0usize;
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(c) = cases.get(k) {
            let want = checksum_of(c.id, c.kind.tag(), c.upem, c.glyphs, c.seed);
            if want != c.checksum {
                bad += 1;
            }
        }
        k += 1;
    }
    let det = format!("bad={}", bad);
    s.add_dyn("50098-J1-校验和可复算", bad == 0 && !cases.is_empty(), &det);

    // J1-4 落账守恒：生成 12 条全部入账，expected 与 len 相符。
    let loaded = VecLedger::load(&raws_of(&cases), 12);
    let (mut led, ok) = match loaded {
        Ok(v) => {
            let good = v.len() == 12 && !v.is_empty() && v.report().contains("B25 vec");
            (v, good)
        }
        Err(_) => (
            VecLedger { cases: Vec::new(), expected: 0, frozen: false, verify_runs: 0 },
            false,
        ),
    };
    let det = format!("len={}", led.len());
    s.add_dyn("50098-J1-落账守恒", ok, &det);

    // J1-5 摘要确定：同种子同摘要，换种子摘要必变。
    let other = match gen_cases(0x1234_5679, 12) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let other_led = match VecLedger::load(&raws_of(&other), 12) {
        Ok(v) => v,
        Err(_) => VecLedger { cases: Vec::new(), expected: 0, frozen: false, verify_runs: 0 },
    };
    let d1 = led.digest();
    let d2 = other_led.digest();
    let ok = d1 != 0 && d1 != d2;
    let det = format!("d1={:08x} d2={:08x}", d1, d2);
    s.add_dyn("50098-J1-摘要确定", ok, &det);

    // J1-6 一条坏校验和整体拒收，且失败不留半账。
    let mut corrupt = raws_of(&cases);
    if let Some(r) = corrupt.get_mut(4) {
        r.checksum ^= 0x8000_0000;
    }
    let rejected = VecLedger::load(&corrupt, 12);
    let ok = rejected.err() == Some(VecError::Checksum);
    s.add("50098-J1-坏批整体拒收", ok, if ok { "" } else { "坏校验和被吞" });

    // J1-7 乱序拒收：把第 2 条 id 推到 5（校验和同步改对，只留顺序错）。
    let mut skip = raws_of(&cases);
    if let Some(r) = skip.get_mut(1) {
        r.id = 5;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    let ok = VecLedger::load(&skip, 12).err() == Some(VecError::Order);
    s.add("50098-J1-乱序拒收", ok, if ok { "" } else { "跳号被接受" });

    // J1-8 重复 id 拒收。
    let mut dup = raws_of(&cases);
    let first = dup.get(0).copied();
    if let (Some(a), Some(b)) = (dup.get_mut(1), first) {
        *a = b;
    }
    let ok = VecLedger::load(&dup, 12).err() == Some(VecError::Duplicate);
    s.add("50098-J1-重复拒收", ok, if ok { "" } else { "重复 id 被接受" });

    // J1-9 upem 口径：900 一律拒（校验和改对后仍须被 Upem 挡下）。
    let mut upem = raws_of(&cases);
    if let Some(r) = upem.get_mut(0) {
        r.upem = 900;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    let ok = VecLedger::load(&upem, 12).err() == Some(VecError::Upem);
    s.add("50098-J1-upem口径", ok, if ok { "" } else { "非法 upem 被接受" });

    // J1-10 字形量口径：0 与 1025 都在闭集外。
    let mut g0 = raws_of(&cases);
    if let Some(r) = g0.get_mut(0) {
        r.glyphs = 0;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    let mut g1 = raws_of(&cases);
    if let Some(r) = g1.get_mut(0) {
        r.glyphs = 1025;
        r.checksum = checksum_of(r.id, r.kind_tag, r.upem, r.glyphs, r.seed);
    }
    let ok = VecLedger::load(&g0, 12).err() == Some(VecError::Glyphs)
        && VecLedger::load(&g1, 12).err() == Some(VecError::Glyphs);
    s.add("50098-J1-字形量口径", ok, if ok { "" } else { "越界字形量被接受" });

    // J1-11 条数不符拒收（少声明 / 多声明都必须报 Count）。
    let raws = raws_of(&cases);
    let short = VecLedger::load(&raws, 11);
    let long = VecLedger::load(&raws, 13);
    let ok = short.err() == Some(VecError::Count) && long.err() == Some(VecError::Count);
    s.add("50098-J1-条数不符", ok, if ok { "" } else { "条数不符未被抓" });

    // J1-12 冻结不可逆 + 校验幂等：两次 verify 同结果，冻结后写入一律 Frozen。
    let v1 = led.verify();
    let v2 = led.verify();
    let frozen_ok = led.freeze();
    let one = CaseRec { id: 13, kind: CaseKind::Metric, upem: 1000, glyphs: 32, seed: 1, checksum: 0 };
    let write = led.push(one);
    let again = led.freeze();
    let ok = v1 == Ok(12)
        && v2 == Ok(12)
        && frozen_ok == Ok(12)
        && write.err() == Some(VecError::Frozen)
        && again.err() == Some(VecError::Frozen)
        && led.verify_runs() == 3;
    let det = format!("v1={:?} runs={}", v1, led.verify_runs());
    s.add_dyn("50098-J1-冻结与幂等", ok, &det);

    // J1-13 容量边界：单批上限 64，65 条必须在生成入口就被拒。
    let cap = gen_cases(0x5EED_1234, MAX_CASES + 1);
    let ok = cap.err() == Some(VecError::Capacity)
        && gen_cases(0x5EED_1234, 0).err() == Some(VecError::Empty)
        && gen_cases(0, 4).err() == Some(VecError::Seed);
    s.add("50098-J1-容量边界", ok, if ok { "" } else { "容量边界失效" });

    // 注入核：12 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50098-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50098_checks_green() {
        let set = run_unx50098_checks();
        assert!(set.all_passed(), "UNX-F50098 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50098_checksum_binds_every_field() {
        let base = checksum_of(3, 2, 1000, 64, 0xAB);
        assert_ne!(base, checksum_of(4, 2, 1000, 64, 0xAB));
        assert_ne!(base, checksum_of(3, 3, 1000, 64, 0xAB));
        assert_ne!(base, checksum_of(3, 2, 2048, 64, 0xAB));
        assert_ne!(base, checksum_of(3, 2, 1000, 65, 0xAB));
        assert_ne!(base, checksum_of(3, 2, 1000, 64, 0xAC));
    }
}
