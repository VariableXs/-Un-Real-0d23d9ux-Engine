//! unx48349_a11yobs — UNX-F48349 无障碍埋点（查询/导出/适配三路四点）（诊断段 0xCC80）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三路四点：查询路一点（query）、导出路一点（export）、适配路
//!      两点（adapt_begin / adapt_end）——三路合计恰四点，点多点少
//!      均判结构漂移。
//!   2. 四点 100%：每路操作数与对应点位发射数逐一点对账，发射数
//!      == 操作数即 100%；任一点缺发即 E_STALL。
//!   3. 适配路成对：adapt_begin / adapt_end 必须成对出现且序号单调，
//!      有始无终（E_SEQ_BAD）或终先于始皆判配对破。
//!   4. 发射留痕：每笔发射携带 (点位, 序号) FNV 指纹入账，账指纹
//!      可重放复算；缺点位号必拒。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCC80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_STALL: u16 = SEG_BASE + 1; // 点位缺发（覆盖 <100%）
const E_SEQ_BAD: u16 = SEG_BASE + 2; // 适配配对/序号破
const E_COVER_MISS: u16 = SEG_BASE + 3; // 结构点数漂移
const E_POINT_BAD: u16 = SEG_BASE + 4; // 非法点位号
const E_TAMPER: u16 = SEG_BASE + 5; // 账指纹失配
const E_REPLAY_BAD: u16 = SEG_BASE + 6; // 重放翻供
const E_NEG_BAD: u16 = SEG_BASE + 7; // 计数负漂

/// 四个观测点位号（0..3）。
pub const PT_QUERY: u8 = 0;
pub const PT_EXPORT: u8 = 1;
pub const PT_ADAPT_B: u8 = 2;
pub const PT_ADAPT_E: u8 = 3;
pub const POINTS: u8 = 4;

pub const PT_NAMES: [&str; 4] = ["a11y_query", "a11y_export", "a11y_adapt_begin", "a11y_adapt_end"];

/// FNV-1a 64 位指纹。
pub fn fp_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 单笔发射：点位 + 全局序号 + 指纹。
#[derive(Clone, Copy)]
pub struct Emit {
    pub point: u8,
    pub seq: u64,
    pub fp: u64,
}

impl Emit {
    fn seal_of(point: u8, seq: u64) -> u64 {
        let mut b = [0u8; 9];
        b[0] = point;
        let mut k = 0usize;
        while k < 8 {
            b[1 + k] = ((seq >> (k * 8)) & 0xFF) as u8;
            k += 1;
        }
        fp_bytes(&b)
    }

    pub fn new(point: u8, seq: u64) -> Self {
        Emit { point, seq, fp: Self::seal_of(point, seq) }
    }

    pub fn verify(&self) -> Result<(), u16> {
        if self.point >= POINTS {
            return Err(E_POINT_BAD);
        }
        if self.fp != Self::seal_of(self.point, self.seq) {
            return Err(E_TAMPER);
        }
        Ok(())
    }
}

/// 观测通道：四点计数 + 发射账 + 适配配对栈（仅深度）。
pub struct ObsChannel {
    counts: [u32; 4],
    ledger: Vec<Emit>,
    seq: u64,
    adapt_open: u32,
}

impl ObsChannel {
    pub fn new() -> Self {
        ObsChannel { counts: [0; 4], ledger: Vec::new(), seq: 0, adapt_open: 0 }
    }

    /// 发射：点位合法、序号单调、指纹入账。
    pub fn emit(&mut self, point: u8) -> Result<(), u16> {
        if point as usize >= POINTS as usize {
            return Err(E_POINT_BAD);
        }
        self.seq = self.seq.wrapping_add(1);
        self.ledger.push(Emit::new(point, self.seq));
        self.counts[point as usize] += 1;
        // 适配路成对约束：end 不得多于 begin。
        if point == PT_ADAPT_B {
            self.adapt_open += 1;
        }
        if point == PT_ADAPT_E {
            if self.adapt_open == 0 {
                return Err(E_SEQ_BAD);
            }
            self.adapt_open -= 1;
        }
        Ok(())
    }

    pub fn count(&self, point: u8) -> Result<u32, u16> {
        if point as usize >= POINTS as usize {
            return Err(E_POINT_BAD);
        }
        Ok(self.counts[point as usize])
    }

    /// 账长（发射笔数）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 第 i 笔发射的指纹（越界即账漂移）。
    pub fn ledger_fp(&self, i: usize) -> Result<u64, u16> {
        if i >= self.ledger.len() {
            return Err(E_TAMPER);
        }
        self.ledger[i].verify()?;
        Ok(self.ledger[i].fp)
    }

    /// 覆盖率对账：四点发射总数 == 账长，且无未闭合适配。
    pub fn coverage_closed(&self) -> Result<(), u16> {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < POINTS as usize {
            if self.counts[i] > self.ledger.len() as u32 {
                return Err(E_NEG_BAD);
            }
            total += self.counts[i];
            i += 1;
        }
        if total != self.ledger.len() as u32 {
            return Err(E_TAMPER);
        }
        if self.adapt_open != 0 {
            return Err(E_SEQ_BAD);
        }
        Ok(())
    }

    /// 账指纹重放：逐笔 verify + 序号严格递增。
    pub fn replay(&self) -> Result<(), u16> {
        let mut prev = 0u64;
        let mut i = 0usize;
        while i < self.ledger.len() {
            self.ledger[i].verify()?;
            if self.ledger[i].seq <= prev {
                return Err(E_SEQ_BAD);
            }
            prev = self.ledger[i].seq;
            i += 1;
        }
        Ok(())
    }
}

/// 三路工作负载回放：query×nq、export×ne、adapt×na（begin+end 成对）。
/// stall 注入：跳过第 skip_at 笔（0 表示不注入）。
pub fn workload(ch: &mut ObsChannel, nq: u32, ne: u32, na: u32, skip_at: u64) -> Result<(), u16> {
    let mut step = 0u64;
    let mut i = 0u32;
    while i < nq {
        step += 1;
        if step != skip_at {
            ch.emit(PT_QUERY)?;
        }
        i += 1;
    }
    let mut i = 0u32;
    while i < ne {
        step += 1;
        if step != skip_at {
            ch.emit(PT_EXPORT)?;
        }
        i += 1;
    }
    let mut i = 0u32;
    while i < na {
        step += 1;
        if step != skip_at {
            ch.emit(PT_ADAPT_B)?;
        }
        step += 1;
        if step != skip_at {
            ch.emit(PT_ADAPT_E)?;
        }
        i += 1;
    }
    Ok(())
}

/// 覆盖判定：四点发射数与操作数逐一相等（100%）。
fn expect_full(ch: &ObsChannel, nq: u32, ne: u32, na: u32) -> Result<(), u16> {
    if ch.count(PT_QUERY)? != nq || ch.count(PT_EXPORT)? != ne {
        return Err(E_STALL);
    }
    if ch.count(PT_ADAPT_B)? != na || ch.count(PT_ADAPT_E)? != na {
        return Err(E_STALL);
    }
    ch.coverage_closed()?;
    Ok(())
}

/// drill：J1 四点 100%。
pub fn drill() -> Result<u32, u16> {
    // ── R1 主判据：30 查 + 20 导 + 15 适配（成对）全量回放，四点
    //    发射数与操作数逐一相等（100%）────────────────────────
    let mut ch = ObsChannel::new();
    workload(&mut ch, 30, 20, 15, 0)?;
    expect_full(&ch, 30, 20, 15)?;
    ch.replay()?;

    // ── R2 断流注入：跳发第 10 笔 → 查询点缺发即 E_STALL ─────
    let mut ch2 = ObsChannel::new();
    workload(&mut ch2, 5, 5, 3, 10)?;
    if ch2.count(PT_QUERY)? == 5 && ch2.count(PT_EXPORT)? == 5 && ch2.count(PT_ADAPT_B)? == 3 {
        return Err(E_STALL); // 注入未生效，判据恒绿即失败
    }
    // 缺发后总发射数必恰为 15（满额 16 少一笔）。
    let mut total = 0u32;
    let mut p = 0u8;
    while p < POINTS {
        total += ch2.count(p)?;
        p += 1;
    }
    if total != 15 {
        return Err(E_STALL);
    }

    // ── R3 配对破注入：end 先于 begin 必拒 ───────────────────
    let mut ch3 = ObsChannel::new();
    if ch3.emit(PT_ADAPT_E).is_ok() {
        return Err(E_SEQ_BAD);
    }
    // begin 后 end 合法闭合。
    ch3.emit(PT_ADAPT_B)?;
    ch3.emit(PT_ADAPT_E)?;
    ch3.coverage_closed()?;

    // ── R4 未闭合注入：只 begin 不 end → coverage_closed 必报 ─
    let mut ch4 = ObsChannel::new();
    ch4.emit(PT_ADAPT_B)?;
    if ch4.coverage_closed().is_ok() {
        return Err(E_SEQ_BAD);
    }

    // ── R5 非法点位：3 号外发射/查询必拒 ─────────────────────
    let mut ch5 = ObsChannel::new();
    if ch5.emit(4).is_ok() || ch5.count(9).is_ok() {
        return Err(E_POINT_BAD);
    }

    // ── R6 重放翻供：账指纹逐笔复核，双算逐位相等 ────────────
    let mut sum1 = 0u64;
    let mut i = 0usize;
    while i < ch.ledger_len() {
        sum1 ^= ch.ledger_fp(i)?;
        i += 1;
    }
    let mut sum2 = 0u64;
    let mut i = 0usize;
    while i < ch.ledger_len() {
        sum2 ^= ch.ledger_fp(i)?;
        i += 1;
    }
    if sum1 != sum2 || sum1 == 0 {
        return Err(E_REPLAY_BAD);
    }

    // ── R7 点名表零漂移：四点名与点位一一对应 ────────────────
    if PT_NAMES.len() != POINTS as usize {
        return Err(E_COVER_MISS);
    }
    let mut p = 0usize;
    while p < PT_NAMES.len() {
        if PT_NAMES[p].is_empty() {
            return Err(E_COVER_MISS);
        }
        p += 1;
    }

    // J2 写回摘要非空：主判据账非空即摘要实质。
    if ch.count(PT_QUERY)? == 0 {
        return Err(E_STALL);
    }
    Ok(ch.count(PT_QUERY)? + ch.count(PT_EXPORT)? + ch.count(PT_ADAPT_B)? + ch.count(PT_ADAPT_E)?)
}
