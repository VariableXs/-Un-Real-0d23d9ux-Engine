#![allow(dead_code)]
//! cgb760_pertelem —— CGPU-F0760 感知遥测。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0760 感知遥测  <SEG 0x6A40>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0760；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知遥测批量账，整数面）：
//!   1. 遥测标签闭集八项（写死语义）：FPS 帧率 / INP 中位 /
//!      JANK 卡顿率 / MEM 内存千字节 / CPU 千分比 / LAT_P90 /
//!      DROP 丢弃数 / HB 心跳；全部整数，禁浮点；
//!   2. 记录打包：word = tag<<28 | val（val ≤ 0x0FFF_FFFF），
//!      打包/解包往返一致（解包 tag 面越界红码）；
//!   3. 定容批量账（写死 32 槽）：满则溢出弃（计数不阻塞），
//!      守恒恒等式：总入账 = 在库 + 溢出弃，标签账合计 = 在库；
//!   4. 滚动摘要：每标签 min/max/sum/cnt 四账随入账滚动，
//!      平均 = sum/cnt（cnt=0 返 0，除零防御）；
//!   5. 封印状态机：Open → Sealed 单向；封印时冻结 FNV-1a 摘要，
//!      封印后禁追加；摘要可外部比对（不符红码）；
//!   6. 时间戳单调不回退（倒挂红码）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6A40 段为本模块独占（SEG_BASE=0x6A40），判别式
//!   SEG_BASE+1..+N；与 0x6A20/0x6A60 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PT_VERSION: &str = "cgb760_pertelem-v1";

/// 诊断段基址（0x6A40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6A40;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6A41 遥测标签越界（0..=7 之外）。
pub const XC_BAD_TAG: u16 = SEG_BASE + 1;
/// 0x6A42 遥测值越界（> 0x0FFF_FFFF）。
pub const XC_BAD_VAL: u16 = SEG_BASE + 2;
/// 0x6A43 批量账满（溢出弃）。
pub const XC_BATCH_FULL: u16 = SEG_BASE + 3;
/// 0x6A44 已封印仍追加。
pub const XC_SEALED: u16 = SEG_BASE + 4;
/// 0x6A45 空账封印。
pub const XC_SEAL_EMPTY: u16 = SEG_BASE + 5;
/// 0x6A46 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x6A47 摘要比对不符。
pub const XC_DIGEST_MISMATCH: u16 = SEG_BASE + 7;
/// 0x6A48 解包非法（tag 面越界）。
pub const XC_BAD_UNPACK: u16 = SEG_BASE + 8;
/// 0x6A49 时间戳倒挂。
pub const XC_TS_BACKWARD: u16 = SEG_BASE + 9;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 标签闭集：0=FPS 1=INP 2=JANK 3=MEM 4=CPU 5=LAT_P90 6=DROP 7=HB。
pub const T_FPS: usize = 0;
pub const T_INP: usize = 1;
pub const T_JANK: usize = 2;
pub const T_MEM: usize = 3;
pub const T_CPU: usize = 4;
pub const T_LAT_P90: usize = 5;
pub const T_DROP: usize = 6;
pub const T_HB: usize = 7;
/// 标签数（写死：8）。
pub const TAG_CAP: usize = 8;
/// 批量账容量（写死：32）。
pub const BATCH_CAP: usize = 32;
/// 遥测值上限（写死：0x0FFF_FFFF，占 word 低 28 位）。
pub const VAL_MAX: u32 = 0x0FFF_FFFF;
/// FNV-1a 32 位初始偏移。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 遥测记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemRec {
    /// 标签（0..=7）。
    pub tag: usize,
    /// 整数值。
    pub val: u32,
    /// 时间戳（ms）。
    pub ts: u32,
    /// 打包字。
    pub word: u32,
}

/// 打包：tag 占高 4 位（实际用低 3 位），val 占低 28 位。
pub fn pack(tag: usize, val: u32) -> Result<u32, u16> {
    if tag >= TAG_CAP {
        return Err(XC_BAD_TAG);
    }
    if val > VAL_MAX {
        return Err(XC_BAD_VAL);
    }
    Ok(((tag as u32) << 28) | val)
}

/// 解包：tag 面越界（高 4 位 > 7）即红码。
pub fn unpack(word: u32) -> Result<(usize, u32), u16> {
    let tag = (word >> 28) as usize;
    if tag >= TAG_CAP {
        return Err(XC_BAD_UNPACK);
    }
    Ok((tag, word & VAL_MAX))
}

/// 单步 FNV-1a（滚动摘要基元）。
pub fn fnv_step(h: u32, word: u32) -> u32 {
    let mut x = h;
    for b in word.to_le_bytes() {
        x ^= b as u32;
        x = x.wrapping_mul(FNV_PRIME);
    }
    x
}

/// 批量账状态机：Open → Sealed（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchState {
    /// 采集中。
    Open,
    /// 已封印。
    Sealed,
}

/// 感知遥测批量账：定容 32 槽 + 滚动摘要 + 封印。
/// 守恒恒等式：
///   总入账 = 在库 + 溢出弃；
///   标签账合计 = 在库（封印后仍成立）。
pub struct TelemBatch {
    recs: Vec<TelemRec>,
    state: BatchState,
    digest: u32,
    last_ts: u32,
    total_in: u32,
    overflow_drop: u32,
    tag_min: [u32; TAG_CAP],
    tag_max: [u32; TAG_CAP],
    tag_sum: [u64; TAG_CAP],
    tag_cnt: [u32; TAG_CAP],
}

impl TelemBatch {
    /// 空批量账（Open，摘要 = FNV 偏移基）。
    pub fn new() -> Self {
        TelemBatch {
            recs: Vec::new(),
            state: BatchState::Open,
            digest: FNV_OFFSET,
            last_ts: 0,
            total_in: 0,
            overflow_drop: 0,
            tag_min: [0; TAG_CAP],
            tag_max: [0; TAG_CAP],
            tag_sum: [0; TAG_CAP],
            tag_cnt: [0; TAG_CAP],
        }
    }

    /// 遥测入账：标签/值/时间戳三查 + 定容 + 滚动摘要。
    pub fn push(&mut self, tag: usize, val: u32, ts: u32) -> Result<u32, u16> {
        if self.state != BatchState::Open {
            return Err(XC_SEALED);
        }
        if tag >= TAG_CAP {
            return Err(XC_BAD_TAG);
        }
        if val > VAL_MAX {
            return Err(XC_BAD_VAL);
        }
        if self.total_in > 0 && ts < self.last_ts {
            return Err(XC_TS_BACKWARD);
        }
        let word = pack(tag, val)?;
        if self.recs.len() >= BATCH_CAP {
            self.overflow_drop += 1;
            self.total_in += 1;
            return Err(XC_BATCH_FULL);
        }
        self.total_in += 1;
        // 滚动摘要（仅对实际入库的记录）。
        self.digest = fnv_step(self.digest, word);
        // 滚动 min/max/sum/cnt。
        if self.tag_cnt[tag] == 0 {
            self.tag_min[tag] = val;
            self.tag_max[tag] = val;
        } else {
            if val < self.tag_min[tag] {
                self.tag_min[tag] = val;
            }
            if val > self.tag_max[tag] {
                self.tag_max[tag] = val;
            }
        }
        self.tag_sum[tag] += val as u64;
        self.tag_cnt[tag] += 1;
        self.last_ts = ts;
        self.recs.push(TelemRec {
            tag,
            val,
            ts,
            word,
        });
        Ok(word)
    }

    /// 在库数。
    pub fn stored(&self) -> usize {
        self.recs.len()
    }

    /// 封印：Open → Sealed；空账封印红码。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.state != BatchState::Open {
            return Err(XC_SEALED);
        }
        if self.recs.is_empty() {
            return Err(XC_SEAL_EMPTY);
        }
        self.state = BatchState::Sealed;
        Ok(self.digest)
    }

    /// 当前摘要只读。
    pub fn digest_now(&self) -> u32 {
        self.digest
    }

    /// 摘要外部比对。
    pub fn verify_digest(&self, want: u32) -> Result<(), u16> {
        if self.digest != want {
            return Err(XC_DIGEST_MISMATCH);
        }
        Ok(())
    }

    /// 标签四账查询：(min, max, avg, cnt)；无样本返回全零。
    pub fn stat(&self, tag: usize) -> Result<(u32, u32, u32, u32), u16> {
        if tag >= TAG_CAP {
            return Err(XC_BAD_TAG);
        }
        let c = self.tag_cnt[tag];
        if c == 0 {
            return Ok((0, 0, 0, 0));
        }
        let avg = (self.tag_sum[tag] / c as u64) as u32;
        Ok((self.tag_min[tag], self.tag_max[tag], avg, c))
    }

    /// 守恒核：总入账 = 在库 + 溢出弃；标签账合计 = 在库。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.total_in != self.recs.len() as u32 + self.overflow_drop {
            return Err(XC_UNBALANCED);
        }
        let mut kc = 0u32;
        for i in 0..TAG_CAP {
            kc += self.tag_cnt[i];
        }
        if kc != self.recs.len() as u32 {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 打包往返复核：所有在库记录 word 解包与字段一致。
    pub fn roundtrip_check(&self) -> Result<(), u16> {
        for r in self.recs.iter() {
            let (t, v) = unpack(r.word)?;
            if t != r.tag || v != r.val {
                return Err(XC_BAD_UNPACK);
            }
        }
        Ok(())
    }

    /// 摘要串：总入账/在库/溢出弃/封印态。
    pub fn summary(&self) -> u32 {
        (self.total_in << 20)
            | ((self.recs.len() as u32) << 8)
            | (self.overflow_drop & 0xFF) << 0
            | if self.state == BatchState::Sealed {
                1 << 31
            } else {
                0
            }
    }
}

/// 自例程：入账 → 滚动摘要 → 统计四账 → 打包往返 → 封印比对 →
/// 守恒 → 错误路径（封印后追加/坏标签/坏值/倒挂/空封印/满溢出）。
pub fn drill() -> Result<u32, u16> {
    let mut b = TelemBatch::new();
    // 空账封印红码。
    if b.seal() != Err(XC_SEAL_EMPTY) {
        return Err(XC_SEAL_EMPTY);
    }
    // 主路径：8 标签 × 3 值 = 24 条。
    for i in 0..3u32 {
        b.push(T_FPS, 60 + i, 10 + i)?;
        b.push(T_INP, 100 + i, 20 + i)?;
        b.push(T_JANK, 30 + i, 30 + i)?;
        b.push(T_MEM, 1024 + i, 40 + i)?;
        b.push(T_CPU, 500 + i, 50 + i)?;
        b.push(T_LAT_P90, 200 + i, 60 + i)?;
        b.push(T_DROP, i, 70 + i)?;
        b.push(T_HB, 1, 80 + i)?;
    }
    if b.stored() != 24 {
        return Err(XC_UNBALANCED);
    }
    b.conserved()?;
    b.roundtrip_check()?;
    // 统计四账：FPS 三样本 60/61/62 → min 60 max 62 avg 61。
    let (mn, mx, av, c) = b.stat(T_FPS)?;
    if mn != 60 || mx != 62 || av != 61 || c != 3 {
        return Err(XC_UNBALANCED);
    }
    // 无样本标签返回全零。
    let mut b2 = TelemBatch::new();
    if b2.stat(T_DROP)? != (0, 0, 0, 0) {
        return Err(XC_UNBALANCED);
    }
    // 打包/解包直查。
    let w = pack(T_HB, 1)?;
    if unpack(w)? != (T_HB, 1) {
        return Err(XC_BAD_UNPACK);
    }
    if unpack(0xF000_0001) != Err(XC_BAD_UNPACK) {
        return Err(XC_BAD_UNPACK);
    }
    if pack(8, 1) != Err(XC_BAD_TAG) {
        return Err(XC_BAD_TAG);
    }
    if pack(T_HB, VAL_MAX + 1) != Err(XC_BAD_VAL) {
        return Err(XC_BAD_VAL);
    }
    // 时间戳倒挂。
    if b.push(T_FPS, 60, 5) != Err(XC_TS_BACKWARD) {
        return Err(XC_TS_BACKWARD);
    }
    // 封印 + 摘要比对。
    let d = b.seal()?;
    b.verify_digest(d)?;
    if b.verify_digest(d ^ 1) != Err(XC_DIGEST_MISMATCH) {
        return Err(XC_DIGEST_MISMATCH);
    }
    if b.digest_now() != d {
        return Err(XC_DIGEST_MISMATCH);
    }
    // 封印后追加红码。
    if b.push(T_HB, 1, 200) != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    // 满溢出：32 条灌满后第 33 条弃。
    let mut b3 = TelemBatch::new();
    for i in 0..32u32 {
        b3.push(T_HB, i + 1, i + 1)?;
    }
    if b3.push(T_HB, 99, 99) != Err(XC_BATCH_FULL) {
        return Err(XC_BATCH_FULL);
    }
    b3.conserved()?;
    if b3.stored() != BATCH_CAP {
        return Err(XC_UNBALANCED);
    }
    // 滚动摘要确定性：同序列双账摘要一致。
    let mut b4 = TelemBatch::new();
    for i in 0..32u32 {
        b4.push(T_HB, i + 1, i + 1)?;
    }
    if b3.digest_now() != b4.digest_now() {
        return Err(XC_DIGEST_MISMATCH);
    }
    // 摘要串位型核对：32 在库 / 1 溢出 / 未封印。
    let s = b3.summary();
    if s & (1 << 31) != 0 {
        return Err(XC_UNBALANCED);
    }
    if (s >> 8) & 0xFF != BATCH_CAP as u32 {
        return Err(XC_UNBALANCED);
    }
    Ok(0)
}
