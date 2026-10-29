#![allow(dead_code)]
//! cgb762_perapi —— CGPU-F0762 感知 API 冻结 v1。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0762 感知 API 冻结 v1  <SEG 0x6A80>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0762；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知 API 冻结面 v1，整数面）：
//!   1. 冻结入口闭集八项（写死 id/arity）：METRICS_GET(0,0) /
//!      SAMPLE_PUSH(1,2) / SESSION_SETTLE(2,0) / REPORT(3,0) /
//!      RING_READ(4,1) / FRAME_PUSH(5,2) / SCORE(6,0) / DIGEST(7,0)；
//!   2. 冻结协议：调用面必须先 freeze()（未冻结红码）；
//!      入口 id 越界红码；arity 不符红码——三查先于分派；
//!   3. 冻结指纹：FNV-1a 逐字节滚过入口表（id+arity），冻结时
//!      定格；表篡改 → 指纹漂移红码；
//!   4. 调用账：定容 32 槽环（满则逐出最老），守恒恒等式：
//!      调用总数 = 成功数 + 失败数 = 在环数 + 逐出数；
//!   5. 后端微模型：16 槽样本环 + 帧账 + 收口位，全部整数；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6A80 段为本模块独占（SEG_BASE=0x6A80），判别式
//!   SEG_BASE+1..+N；与 0x6A60/0x6AA0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PA_VERSION: &str = "cgb762_perapi-v1";

/// 诊断段基址（0x6A80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6A80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6A81 入口 id 越界（0..=7 之外）。
pub const XC_BAD_API: u16 = SEG_BASE + 1;
/// 0x6A82 arity 不符。
pub const XC_ARITY_BAD: u16 = SEG_BASE + 2;
/// 0x6A83 未冻结即调用。
pub const XC_NOT_FROZEN: u16 = SEG_BASE + 3;
/// 0x6A84 重复冻结。
pub const XC_DUP_FREEZE: u16 = SEG_BASE + 4;
/// 0x6A85 冻结指纹漂移。
pub const XC_DIGEST_DRIFT: u16 = SEG_BASE + 5;
/// 0x6A86 调用账不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x6A87 后端样本非法。
pub const XC_BAD_SAMPLE: u16 = SEG_BASE + 7;
/// 0x6A88 后端状态非法（未收口先评分等）。
pub const XC_STATE_BAD: u16 = SEG_BASE + 8;
/// 0x6A89 后端空账评分。
pub const XC_EMPTY_SCORE: u16 = SEG_BASE + 9;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 入口闭集 id。
pub const A_METRICS_GET: usize = 0;
pub const A_SAMPLE_PUSH: usize = 1;
pub const A_SESSION_SETTLE: usize = 2;
pub const A_REPORT: usize = 3;
pub const A_RING_READ: usize = 4;
pub const A_FRAME_PUSH: usize = 5;
pub const A_SCORE: usize = 6;
pub const A_DIGEST: usize = 7;
/// 入口数（写死：8）。
pub const API_CAP: usize = 8;
/// 调用账容量（写死：32）。
pub const LEDGER_CAP: usize = 32;
/// 后端样本环容量（写死：16）。
pub const RING_CAP: usize = 16;
/// 样本上限（写死：10000）。
pub const SAMPLE_MAX: u32 = 10_000;
/// 阈值（good 100 / poor 300）。
pub const THRESH: [u32; 2] = [100, 300];

/// 冻结入口表（id, arity）——v1 写死。
pub const API_TABLE: [[usize; 2]; API_CAP] = [
    [A_METRICS_GET, 0],
    [A_SAMPLE_PUSH, 2],
    [A_SESSION_SETTLE, 0],
    [A_REPORT, 0],
    [A_RING_READ, 1],
    [A_FRAME_PUSH, 2],
    [A_SCORE, 0],
    [A_DIGEST, 0],
];

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 冻结指纹：逐字节滚过入口表。
pub fn freeze_digest() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for row in API_TABLE.iter() {
        h = fnv_step(h, row[0] as u8);
        h = fnv_step(h, row[1] as u8);
    }
    h
}

/// 入口 arity 查表。
pub fn arity_of(id: usize) -> Result<usize, u16> {
    if id >= API_CAP {
        return Err(XC_BAD_API);
    }
    Ok(API_TABLE[id][1])
}

/// 调用账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallRec {
    /// 入口 id。
    pub id: usize,
    /// 成功与否。
    pub ok: bool,
}

/// 后端微模型：16 槽样本环 + 帧账 + 收口位。
pub struct MiniModel {
    buf: [Option<u32>; RING_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    pushed: usize,
    frame_sum: u64,
    frame_cnt: u32,
    jank: u32,
    settled: bool,
}

impl MiniModel {
    /// 空模型。
    pub fn new() -> Self {
        MiniModel {
            buf: [None; RING_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            pushed: 0,
            frame_sum: 0,
            frame_cnt: 0,
            jank: 0,
            settled: false,
        }
    }

    /// 样本入账。
    pub fn sample(&mut self, lat: u32) -> Result<u8, u16> {
        if lat == 0 || lat > SAMPLE_MAX {
            return Err(XC_BAD_SAMPLE);
        }
        if self.in_ring < RING_CAP {
            self.in_ring += 1;
        } else {
            self.evicted += 1;
        }
        self.buf[self.head] = Some(lat);
        self.head = (self.head + 1) % RING_CAP;
        self.pushed += 1;
        Ok(if lat <= THRESH[0] {
            0
        } else if lat > THRESH[1] {
            2
        } else {
            1
        })
    }

    /// 帧入账（≥34ms 记卡顿）。
    pub fn frame(&mut self, dur: u32) -> Result<(), u16> {
        if dur == 0 || dur > 1000 {
            return Err(XC_BAD_SAMPLE);
        }
        self.frame_cnt += 1;
        self.frame_sum += dur as u64;
        if dur >= 34 {
            self.jank += 1;
        }
        Ok(())
    }

    /// 收口。
    pub fn settle(&mut self) -> Result<(), u16> {
        if self.settled {
            return Err(XC_STATE_BAD);
        }
        self.settled = true;
        Ok(())
    }

    /// 中位（插入排序）。
    pub fn median(&self) -> u32 {
        let mut v: Vec<u32> = self.buf.iter().flatten().copied().collect();
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        let n = v.len();
        if n == 0 {
            return 0;
        }
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2
        }
    }

    /// 在环数。
    pub fn ring_count(&self) -> usize {
        self.in_ring
    }

    /// 评分：未收口红码；空账红码；中位档位×2 为罚分。
    pub fn score(&self) -> Result<u32, u16> {
        if !self.settled {
            return Err(XC_STATE_BAD);
        }
        if self.pushed == 0 {
            return Err(XC_EMPTY_SCORE);
        }
        let m = self.median();
        Ok(if m <= THRESH[0] {
            0
        } else if m > THRESH[1] {
            4
        } else {
            2
        })
    }

    /// 模型守恒：入账数 = 在环 + 逐出。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.pushed != self.in_ring + self.evicted {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }
}

/// 感知 API 冻结面 v1：入口表 + 指纹 + 调用账 + 后端微模型。
pub struct ApiSurface {
    model: MiniModel,
    ledger: [Option<CallRec>; LEDGER_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    frozen: bool,
    digest: u32,
    calls_ok: u32,
    calls_err: u32,
}

impl ApiSurface {
    /// 未冻结空面。
    pub fn new() -> Self {
        ApiSurface {
            model: MiniModel::new(),
            ledger: [None; LEDGER_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            frozen: false,
            digest: 0,
            calls_ok: 0,
            calls_err: 0,
        }
    }

    /// 冻结：定格入口表指纹；重复冻结红码。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(XC_DUP_FREEZE);
        }
        self.frozen = true;
        self.digest = freeze_digest();
        Ok(self.digest)
    }

    /// 指纹复核。
    pub fn verify_frozen(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        if self.digest != freeze_digest() {
            return Err(XC_DIGEST_DRIFT);
        }
        Ok(())
    }

    /// 调用分派：三查（冻结/id/arity）先于后端执行。
    pub fn call(&mut self, id: usize, args: &[u32]) -> Result<u32, u16> {
        let r = self.dispatch(id, args);
        self.record(CallRec {
            id,
            ok: r.is_ok(),
        });
        r
    }

    fn dispatch(&mut self, id: usize, args: &[u32]) -> Result<u32, u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        let ar = arity_of(id)?;
        if args.len() != ar {
            return Err(XC_ARITY_BAD);
        }
        match id {
            A_METRICS_GET => Ok(self.model.ring_count() as u32),
            A_SAMPLE_PUSH => self.model.sample(args[0]),
            A_SESSION_SETTLE => self.model.settle().map(|_| 0),
            A_REPORT => {
                let m = self.model.median();
                Ok((m << 8) | (self.model.jank & 0xFF))
            }
            A_RING_READ => {
                if args[0] != 0 {
                    return Err(XC_BAD_SAMPLE);
                }
                Ok(self.model.ring_count() as u32)
            }
            A_FRAME_PUSH => self.model.frame(args[0]).map(|_| 0),
            A_SCORE => self.model.score(),
            A_DIGEST => {
                self.model.conserved()?;
                Ok(self.digest ^ (self.model.median() & 0xFFFF))
            }
            _ => Err(XC_BAD_API),
        }
    }

    /// 调用账入账（定容 32，满则逐出最老）。
    fn record(&mut self, rec: CallRec) {
        if rec.ok {
            self.calls_ok += 1;
        } else {
            self.calls_err += 1;
        }
        if self.in_ring < LEDGER_CAP {
            self.in_ring += 1;
        } else {
            self.evicted += 1;
        }
        self.ledger[self.head] = Some(rec);
        self.head = (self.head + 1) % LEDGER_CAP;
    }

    /// 调用账守恒：总数 = 成功 + 失败 = 在环 + 逐出。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.calls_ok + self.calls_err != (self.in_ring + self.evicted) as u32 {
            return Err(XC_UNBALANCED);
        }
        self.model.conserved()
    }

    /// 成功/失败计数只读。
    pub fn call_stats(&self) -> (u32, u32) {
        (self.calls_ok, self.calls_err)
    }
}

/// 自例程：冻结 → 全入口调用 → 三查错误路径 → 账守恒 →
/// 指纹漂移 → 后端评分线。
pub fn drill() -> Result<u32, u16> {
    let mut api = ApiSurface::new();
    // 错误路径 1：未冻结即调用。
    if api.call(A_METRICS_GET, &[]) != Err(XC_NOT_FROZEN) {
        return Err(XC_NOT_FROZEN);
    }
    // 冻结 + 指纹复核。
    let d = api.freeze()?;
    api.verify_frozen()?;
    if d != freeze_digest() {
        return Err(XC_DIGEST_DRIFT);
    }
    // 错误路径 2：重复冻结。
    if api.freeze() != Err(XC_DUP_FREEZE) {
        return Err(XC_DUP_FREEZE);
    }
    // 错误路径 3：id 越界 / arity 不符。
    if api.call(8, &[]) != Err(XC_BAD_API) {
        return Err(XC_BAD_API);
    }
    if api.call(A_SAMPLE_PUSH, &[100]) != Err(XC_ARITY_BAD) {
        return Err(XC_ARITY_BAD);
    }
    // 主路径：推 5 样本（100/150/200/250/90）+ 2 帧。
    api.call(A_SAMPLE_PUSH, &[100])?;
    api.call(A_SAMPLE_PUSH, &[150])?;
    api.call(A_SAMPLE_PUSH, &[200])?;
    api.call(A_SAMPLE_PUSH, &[250])?;
    api.call(A_SAMPLE_PUSH, &[90])?;
    api.call(A_FRAME_PUSH, &[16])?;
    api.call(A_FRAME_PUSH, &[40])?;
    // METRICS_GET：在环 5。
    if api.call(A_METRICS_GET, &[])? != 5 {
        return Err(XC_UNBALANCED);
    }
    // RING_READ(0)：同在环数。
    if api.call(A_RING_READ, &[0])? != 5 {
        return Err(XC_UNBALANCED);
    }
    // RING_READ(1)：参数面红码。
    if api.call(A_RING_READ, &[1]) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 未收口评分红码。
    if api.call(A_SCORE, &[]) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 收口 → 评分：中位 150 → needs → 罚 2。
    api.call(A_SESSION_SETTLE, &[])?;
    if api.call(A_SCORE, &[])? != 2 {
        return Err(XC_UNBALANCED);
    }
    // 重复收口红码。
    if api.call(A_SESSION_SETTLE, &[]) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // REPORT：中位 150<<8 | jank 1。
    if api.call(A_REPORT, &[])? != (150 << 8) | 1 {
        return Err(XC_UNBALANCED);
    }
    // DIGEST：守恒通过才出指纹异或值。
    let dv = api.call(A_DIGEST, &[])?;
    if dv != d ^ (150 & 0xFFFF) {
        return Err(XC_DIGEST_DRIFT);
    }
    // 坏样本：0 值。
    if api.call(A_SAMPLE_PUSH, &[0]) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 帧账：0 耗时红码。
    if api.call(A_FRAME_PUSH, &[0]) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 调用账守恒 + 计数核对。
    api.conserved()?;
    let (ok, err) = api.call_stats();
    if ok + err != 21 {
        return Err(XC_UNBALANCED);
    }
    // 后端环逐出：再推 12 样本（总 17 > 16）→ 逐出 1。
    for i in 1..=12u32 {
        api.call(A_SAMPLE_PUSH, &[100 + i])?;
    }
    api.conserved()?;
    if api.call(A_METRICS_GET, &[])? != RING_CAP as u32 {
        return Err(XC_UNBALANCED);
    }
    Ok(0)
}
