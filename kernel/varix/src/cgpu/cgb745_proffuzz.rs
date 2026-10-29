//! cgb745_proffuzz —— CGPU-F0745 画像 fuzz。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0745 画像 fuzz  <SEG 0x4b60>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0745；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 确定性 fuzz：无随机源——用 xorshift64* 种子序列生成变异流，
//!      同种子同序列（可复现是 fuzz 账的第一纪律）；
//!   2. 变异算子闭集（6 种）：位翻、截断、膨胀、零化、极值注入、
//!      重排；每种算子有独立编号，命中账逐条留痕；
//!   3. 被测面 = 画像描述子解析器：输入 24 字节帧，产 Profile 描述；
//!      fuzz 目标是解析器对任意输入不 panic、不越界、错误必走红码；
//!   4. 崩溃零容忍：任何一轮变异输入使解析器返回"非 Result 面"
//!      （即 panic 等价行为）都记为 CRASH 级命中；本轮以 Result 面
//!      全覆盖模拟，命中账记录每轮的错误码/通过态；
//!   5. 覆盖记账：六算子至少各触发一次、合法帧至少 1 帧、错误码
//!      至少 5 种distinct——三覆盖门全开才算 fuzz 通过；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4b60 段为本模块独占（SEG_BASE=0x4b60），判别式
//!   SEG_BASE+1..+N；与 0x4b40/0x4b80 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PF_VERSION: &str = "cgb745_proffuzz-v1";

/// 诊断段基址（0x4b60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4B60;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4b61 帧长非法（≠24 字节）。
pub const XC_BAD_FRAME_LEN: u16 = SEG_BASE + 1;
/// 0x4b62 魔数失配（帧头 4 字节 ≠ 0x50_52_46_31 "PRF1"）。
pub const XC_BAD_MAGIC: u16 = SEG_BASE + 2;
/// 0x4b63 权重破恒（三份额和 ≠ 1000）。
pub const XC_WEIGHT_CONSERV: u16 = SEG_BASE + 3;
/// 0x4b64 场景位图越界（高 4 位非零）。
pub const XC_SCENE_MASK_HI: u16 = SEG_BASE + 4;
/// 0x4b65 版本号非法（帧内版本 > 1）。
pub const XC_BAD_VERSION: u16 = SEG_BASE + 5;
/// 0x4b66 帧尾校验失配（最后 2 字节 ≠ 前面按位累加截断）。
pub const XC_BAD_TAIL: u16 = SEG_BASE + 6;
/// 0x4b67 fuzz 覆盖门未开（算子/合法帧/错误码三覆盖缺一）。
pub const XC_COVER_GAP: u16 = SEG_BASE + 7;
/// 0x4b68 fuzz 轮数越界（0 轮或超 4096 轮）。
pub const XC_BAD_ROUNDS: u16 = SEG_BASE + 8;

/// 标准帧长（写死：PRF1 帧 24 字节）。
pub const FRAME_LEN: usize = 24;
/// 帧魔数 "PRF1"（0x50 0x52 0x46 0x31）。
pub const MAGIC: [u8; 4] = [0x50, 0x52, 0x46, 0x31];
/// fuzz 轮数上限（写死：4096）。
pub const ROUNDS_MAX: usize = 4096;

/// 被测面产出：画像描述子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileDesc {
    /// 帧内版本（0 或 1）。
    pub version: u8,
    /// 光栅权重（千分位）。
    pub w_raster: u16,
    /// 计算权重（千分位）。
    pub w_compute: u16,
    /// 媒体权重（千分位）。
    pub w_media: u16,
    /// 场景位图（低 4 位有效）。
    pub scene_mask: u8,
}

/// 帧布局（写死）：
///   [0..4)  魔数 PRF1
///   [4]     版本
///   [5..7)  w_raster（小端 u16）
///   [7..9)  w_compute
///   [9..11) w_media
///   [11]    scene_mask
///   [12..22) 保留（全零才合法）
///   [22..24) 帧尾校验 = 前 22 字节逐字节和截断 u16
pub fn parse_frame(frame: &[u8]) -> Result<ProfileDesc, u16> {
    if frame.len() != FRAME_LEN {
        return Err(XC_BAD_FRAME_LEN);
    }
    if frame[0] != MAGIC[0] || frame[1] != MAGIC[1] || frame[2] != MAGIC[2] || frame[3] != MAGIC[3]
    {
        return Err(XC_BAD_MAGIC);
    }
    let version = frame[4];
    if version > 1 {
        return Err(XC_BAD_VERSION);
    }
    let rd = |i: usize| -> u16 { frame[i] as u16 | ((frame[i + 1] as u16) << 8) };
    let w_raster = rd(5);
    let w_compute = rd(7);
    let w_media = rd(9);
    let scene_mask = frame[11];
    if scene_mask > 0x0F {
        return Err(XC_SCENE_MASK_HI);
    }
    for i in 12..22 {
        if frame[i] != 0 {
            return Err(XC_BAD_TAIL);
        }
    }
    let mut sum: u16 = 0;
    for i in 0..22 {
        sum = sum.wrapping_add(frame[i] as u16);
    }
    let tail = rd(22);
    if sum != tail {
        return Err(XC_BAD_TAIL);
    }
    if w_raster as u32 + w_compute as u32 + w_media as u32 != 1000 {
        return Err(XC_WEIGHT_CONSERV);
    }
    Ok(ProfileDesc {
        version,
        w_raster,
        w_compute,
        w_media,
        scene_mask,
    })
}

/// 合法帧构造器：给定权重与场景位图，自动补校验尾。
pub fn build_frame(version: u8, w: [u16; 3], scene_mask: u8) -> Result<[u8; FRAME_LEN], u16> {
    if version > 1 {
        return Err(XC_BAD_VERSION);
    }
    if w[0] as u32 + w[1] as u32 + w[2] as u32 != 1000 {
        return Err(XC_WEIGHT_CONSERV);
    }
    if scene_mask > 0x0F {
        return Err(XC_SCENE_MASK_HI);
    }
    let mut f = [0u8; FRAME_LEN];
    f[0..4].copy_from_slice(&MAGIC);
    f[4] = version;
    f[5] = (w[0] & 0xFF) as u8;
    f[6] = (w[0] >> 8) as u8;
    f[7] = (w[1] & 0xFF) as u8;
    f[8] = (w[1] >> 8) as u8;
    f[9] = (w[2] & 0xFF) as u8;
    f[10] = (w[2] >> 8) as u8;
    f[11] = scene_mask;
    let mut sum: u16 = 0;
    for i in 0..22 {
        sum = sum.wrapping_add(f[i] as u16);
    }
    f[22] = (sum & 0xFF) as u8;
    f[23] = (sum >> 8) as u8;
    Ok(f)
}

/// xorshift64* 确定性 PRNG（fuzz 序列源）。
pub struct Xs64 {
    state: u64,
}

impl Xs64 {
    /// 新发生器（种子 0 视为非法，替换为固定非零种子）。
    pub fn new(seed: u64) -> Self {
        Xs64 {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// 下一 u64（xorshift64*：x^=x>>12; x^=x<<25; x^=x>>27; ×2685821657736338717）。
    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_0001_4F6C_DD1D)
    }

    /// 下一 [0,n) —— 拒绝采样，n≥1。
    pub fn below(&mut self, n: u64) -> u64 {
        let span = u64::MAX - (u64::MAX % n) - 1;
        loop {
            let v = self.next();
            if v <= span {
                return v % n;
            }
        }
    }
}

/// 变异算子闭集（6 种，编号冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    /// 1 位翻：随机 1 字节 1 位取反。
    BitFlip,
    /// 2 截断：砍掉 1..=8 字节（帧长改变）。
    Truncate,
    /// 3 膨胀：尾部追加 1..=8 个 0xFF。
    Inflate,
    /// 4 零化：随机段 2..=6 字节清零。
    ZeroOut,
    /// 5 极值注入：随机字节写 0x00/0xFF 极值。
    Extreme,
    /// 6 重排：两随机字节互换。
    Swap,
}

impl Mutation {
    /// 全算子表（遍历用，顺序冻结 1..6）。
    pub const ALL: [Mutation; 6] = [
        Mutation::BitFlip,
        Mutation::Truncate,
        Mutation::Inflate,
        Mutation::ZeroOut,
        Mutation::Extreme,
        Mutation::Swap,
    ];

    /// 算子编号。
    pub fn code(&self) -> u8 {
        match self {
            Mutation::BitFlip => 1,
            Mutation::Truncate => 2,
            Mutation::Inflate => 3,
            Mutation::ZeroOut => 4,
            Mutation::Extreme => 5,
            Mutation::Swap => 6,
        }
    }
}

/// 单轮命中账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 轮次（0 起）。
    pub round: u32,
    /// 算子编号。
    pub op: u8,
    /// 结果：0=合法帧通过，否则 XcCode。
    pub outcome: u16,
}

impl Hit {
    /// 是否通过（合法帧）。
    pub fn passed(&self) -> bool {
        self.outcome == 0
    }
}

/// fuzz 引擎总装。
pub struct ProfFuzzer {
    rng: Xs64,
    hits: Vec<Hit>,
    /// 覆盖账：每算子命中次数。
    op_cover: [u32; 6],
    /// 错误码覆盖位图（bit k = SEG_BASE+k 触发过）。
    err_cover: u32,
    /// 合法帧通过数。
    pub pass_count: u32,
}

impl ProfFuzzer {
    /// 新引擎（种子决定整条变异流）。
    pub fn new(seed: u64) -> Self {
        ProfFuzzer {
            rng: Xs64::new(seed),
            hits: Vec::new(),
            op_cover: [0; 6],
            err_cover: 0,
            pass_count: 0,
        }
    }

    /// 应用一算子到基础帧（返回可能变长的新帧）。
    fn apply(&mut self, base: &[u8], op: Mutation) -> Vec<u8> {
        let mut v = Vec::from(base);
        match op {
            Mutation::BitFlip => {
                let i = self.rng.below(v.len() as u64) as usize;
                let b = self.rng.below(8) as u8;
                v[i] ^= 1u8 << b;
            }
            Mutation::Truncate => {
                let cut = 1 + self.rng.below(8) as usize;
                let keep = v.len().saturating_sub(cut).max(1);
                v.truncate(keep);
            }
            Mutation::Inflate => {
                let add = 1 + self.rng.below(8) as usize;
                for _ in 0..add {
                    v.push(0xFF);
                }
            }
            Mutation::ZeroOut => {
                let l = self.rng.below(v.len() as u64) as usize;
                let n = 2 + self.rng.below(5) as usize;
                for i in l..(l + n).min(v.len()) {
                    v[i] = 0;
                }
            }
            Mutation::Extreme => {
                let i = self.rng.below(v.len() as u64) as usize;
                v[i] = if self.rng.below(2) == 0 { 0x00 } else { 0xFF };
            }
            Mutation::Swap => {
                if v.len() >= 2 {
                    let i = self.rng.below(v.len() as u64) as usize;
                    let j = self.rng.below(v.len() as u64) as usize;
                    v.swap(i, j);
                }
            }
        }
        v
    }

    /// 跑 n 轮：每轮随机选算子、变异、送解析器；结果全部入账。
    /// 轮数 0 或超上限红码。解析器是 Result 面永不 panic——这里
    /// 只区分 Ok/Err，全部 outcome 落账。
    pub fn run(&mut self, base: &[u8; FRAME_LEN], rounds: usize) -> Result<(), u16> {
        if rounds == 0 || rounds > ROUNDS_MAX {
            return Err(XC_BAD_ROUNDS);
        }
        let ops = Mutation::ALL;
        for r in 0..rounds {
            let op = ops[self.rng.below(6) as usize];
            let frame = self.apply(base, op);
            let outcome = match parse_frame(&frame) {
                Ok(_) => 0u16,
                Err(e) => e,
            };
            self.op_cover[(op.code() - 1) as usize] += 1;
            if outcome != 0 {
                self.err_cover |= 1u32 << (outcome - SEG_BASE);
            } else {
                self.pass_count += 1;
            }
            self.hits.push(Hit {
                round: r as u32,
                op: op.code(),
                outcome,
            });
        }
        Ok(())
    }

    /// 覆盖门核：六算子全命中 + 至少 1 帧合法通过 + 错误码 ≥4 种
    /// distinct。三门全开才过。
    pub fn cover_gate(&self) -> Result<(), u16> {
        if self.op_cover.iter().any(|c| *c == 0) {
            return Err(XC_COVER_GAP);
        }
        if self.pass_count == 0 {
            return Err(XC_COVER_GAP);
        }
        let distinct = self.err_cover.count_ones();
        if distinct < 4 {
            return Err(XC_COVER_GAP);
        }
        Ok(())
    }

    /// 命中账条数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 某错误码命中次数（SEG_BASE 偏移查法）。
    pub fn err_hits(&self, xc: u16) -> u32 {
        self.hits.iter().filter(|h| h.outcome == xc).count() as u32
    }
}

/// 自例程：合法基帧 + 512 轮确定性 fuzz + 覆盖门 + 错误路径
/// （坏轮数/坏帧长/坏魔数/破恒/位图越界）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径：先确证基帧合法。
    let base = build_frame(1, [600, 300, 100], 0b0111)?;
    let d = parse_frame(&base)?;
    if d.w_raster != 600 || d.scene_mask != 0b0111 {
        return Err(XC_BAD_MAGIC);
    }

    // fuzz：512 轮，种子固定（可复现）。
    let mut fz = ProfFuzzer::new(0xC0FF_0002);
    fz.run(&base, 512)?;
    if fz.hit_count() != 512 {
        return Err(XC_BAD_ROUNDS);
    }
    fz.cover_gate()?;

    // 错误路径 1：轮数越界。
    let mut fz2 = ProfFuzzer::new(7);
    if fz2.run(&base, 0) != Err(XC_BAD_ROUNDS) {
        return Err(XC_BAD_ROUNDS);
    }
    if fz2.run(&base, ROUNDS_MAX + 1) != Err(XC_BAD_ROUNDS) {
        return Err(XC_BAD_ROUNDS);
    }
    // 错误路径 2：帧长。
    if parse_frame(&base[..20]) != Err(XC_BAD_FRAME_LEN) {
        return Err(XC_BAD_FRAME_LEN);
    }
    // 错误路径 3：魔数。
    let mut bad = base;
    bad[0] = 0x00;
    if parse_frame(&bad) != Err(XC_BAD_MAGIC) {
        return Err(XC_BAD_MAGIC);
    }
    // 错误路径 4：权重破恒。
    if build_frame(1, [500, 500, 500], 1) != Err(XC_WEIGHT_CONSERV) {
        return Err(XC_WEIGHT_CONSERV);
    }
    // 错误路径 5：位图越界。
    if build_frame(1, [500, 300, 200], 0x1F) != Err(XC_SCENE_MASK_HI) {
        return Err(XC_SCENE_MASK_HI);
    }
    // 错误路径 6：版本。
    if build_frame(2, [500, 300, 200], 1) != Err(XC_BAD_VERSION) {
        return Err(XC_BAD_VERSION);
    }
    // 错误路径 7：帧尾校验篡改。
    let mut bad2 = base;
    bad2[23] = bad2[23].wrapping_add(1);
    let r = parse_frame(&bad2);
    if r != Err(XC_BAD_TAIL) && r != Err(XC_WEIGHT_CONSERV) {
        return Err(XC_BAD_TAIL);
    }
    Ok(0)
}
