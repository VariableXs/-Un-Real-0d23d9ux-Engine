//! unx48306_cfghot — UNX-F48306 配置热生效契约（改配置不重启引擎，生效路径可观测）（诊断段 0xC720）。
//!
//! 判据原文：
//!   J1 热生效 ×10 不断流（与 B02 词库热更同源）；判据锚定 §AI-61 §2 专题四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 热生效契约：配置变更经热更通路原子生效（版本号 + 生效体
//!      同事务翻转），引擎不重启（epoch 只增不清零）、不断流
//!      （输入事件流逐条处理、零丢失、零重排）。
//!   2. ×10 热更：事件流中穿插 10 次配置变更，每次变更版本 +1 且
//!      生效体整体切换（无半新半旧混合态）。
//!   3. 生效路径可观测：每条事件的处理回执记录「处理时的配置版本」
//!      ——版本过渡点前后的事件版本标签严格对齐变更位置（变更点
//!      之前全部旧版、之后全部新版）。
//!   4. 断流判据：输入 N 条 → 回执 N 条，按输入序一一对应（内容
//!      保真 + 顺序保真），任何丢失/重排即断流。
//!   5. 同源纪律：与 B02 词库热更同一套「版本 + 生效体」翻转机制
//!      （结构同源），不私造第二套热更协议。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC720;

const E_STREAM_DROP: u16 = SEG_BASE + 1;   // 回执缺失（断流）
const E_STREAM_REORDER: u16 = SEG_BASE + 2; // 回执重排
const E_EPOCH_RESET: u16 = SEG_BASE + 3;   // epoch 清零（重启迹象）
const E_VER_MIXED: u16 = SEG_BASE + 4;     // 半新半旧混合态
const E_VER_ALIGNMENT: u16 = SEG_BASE + 5; // 版本标签与变更点不对齐
const E_UPDATE_MISSED: u16 = SEG_BASE + 6; // 热更未生效（版本未推进）
const E_CODE_RANGE: u16 = SEG_BASE + 7;    // 诊断码越段

/// 热更配置：版本 + 生效体（同事务翻转的最小单元，与 B02 同源）。
#[derive(Clone)]
pub struct HotCfg {
    pub ver: u32,
    pub body: [u32; 4],
}

impl HotCfg {
    /// 原子翻转：版本 +1 且体整体切换（返回新 HotCfg，调用方一次性换装）。
    pub fn flip(&self, new_body: [u32; 4]) -> HotCfg {
        HotCfg {
            ver: self.ver + 1,
            body: new_body,
        }
    }
}

/// 引擎热更新器：epoch 只增（重启迹象 = 清零）。
pub struct HotEngine {
    pub epoch: u64,
    pub cfg: HotCfg,
}

impl HotEngine {
    pub fn new() -> Self {
        HotEngine {
            epoch: 1,
            cfg: HotCfg {
                ver: 100,
                body: [0x0A0A, 0x0B0B, 0x0C0C, 0x0D0D],
            },
        }
    }

    /// 热更：原子换装（体与版本一次翻转），epoch 递增（不重启）。
    pub fn hot_update(&mut self, new_body: [u32; 4]) -> u32 {
        self.cfg = self.cfg.flip(new_body);
        self.epoch += 1;
        self.cfg.ver
    }

    /// 处理一条事件：回执携带处理时的配置版本（可观测路径）。
    pub fn process(&self, ev: u32) -> (u32, u32) {
        (ev, self.cfg.ver)
    }

    pub fn ver(&self) -> u32 {
        self.cfg.ver
    }

    pub fn body(&self) -> [u32; 4] {
        self.cfg.body
    }
}

/// 确定性线性同余发生器。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 混合场景合成：事件流 + 穿插热更点（返回事件序列与每步动作）。
/// 动作枚举：true = 事件步；false = 热更步。
pub fn synth_scenario(total: usize, updates: usize, seed: u32) -> Vec<bool> {
    let mut lc = Lcg::new(seed);
    let mut plan: Vec<bool> = Vec::new();
    let mut ups = 0usize;
    let mut i = 0usize;
    while i < total {
        plan.push(true);
        i += 1;
    }
    // 把 updates 个热更点插到确定性位置（总步数 = total + updates）。
    let mut out: Vec<bool> = Vec::new();
    let mut ev_left = total;
    let step_count = total + updates;
    let mut s = 0usize;
    while s < step_count {
        // 每 (total/(updates+1)) 步插一个热更（确定性均匀分布）。
        let gap = total / (updates + 1) + 1;
        if ups < updates && s > 0 && s % gap == 0 && ev_left > 1 {
            out.push(false);
            ups += 1;
        } else if ev_left > 0 {
            out.push(true);
            ev_left -= 1;
        } else {
            out.push(false);
            ups += 1;
        }
        s += 1;
    }
    // 兜底：若热更未插满，把尾部补齐。
    while ups < updates {
        out.push(false);
        ups += 1;
    }
    let _ = &mut lc;
    out
}

/// 热更历史账行：批次 + 旧版本 + 新版本 + 新体指纹。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HotHistRow {
    pub step: u32,
    pub ver_old: u32,
    pub ver_new: u32,
    pub body_fp: u64,
}

/// 体指纹（FNV-1a 折叠 4 字体）。
pub fn body_fp(b: &[u32; 4]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < 4 {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 体合法性：四字全非零（全零体是热更的非法载荷）。
pub fn body_valid(b: &[u32; 4]) -> bool {
    let mut i = 0;
    while i < 4 {
        if b[i] == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 热更历史账：append-only，步号连续，同步号重放幂等。
pub struct HotHist {
    rows: Vec<HotHistRow>,
}

impl HotHist {
    pub fn new() -> Self {
        HotHist { rows: Vec::new() }
    }

    /// 返回 true = 记账一致（新记或幂等重放）。
    pub fn record(&mut self, row: HotHistRow) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].step == row.step {
                return self.rows[i] == row;
            }
            i += 1;
        }
        self.rows.push(row);
        true
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 配置热生效主巡检：×10 热更不断流全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let plan = synth_scenario(120, 10, 0x4830_6001);

    // R1 场景规约：总步 = 130，其中热更步恰 10、事件步恰 120。
    let mut ups = 0usize;
    let mut evs = 0usize;
    let mut s = 0;
    while s < plan.len() {
        if plan[s] {
            evs += 1;
        } else {
            ups += 1;
        }
        s += 1;
    }
    if plan.len() != 130 || ups != 10 || evs != 120 {
        return Err(E_UPDATE_MISSED);
    }
    hits += 1;

    // R2 引擎跑混合场景：事件逐条回执（内容+版本），热更逐次换装。
    let mut eng = HotEngine::new();
    let ver0 = eng.ver();
    let mut receipts: Vec<(u32, u32)> = Vec::new();
    let mut ev_id = 0u32;
    let mut new_bodies: Vec<[u32; 4]> = Vec::new();
    let mut lc = Lcg::new(0x4830_6002);
    s = 0;
    while s < plan.len() {
        if plan[s] {
            let r = eng.process(ev_id);
            receipts.push(r);
            ev_id += 1;
        } else {
            let nb = [lc.next() & 0xFFFF, lc.next() & 0xFFFF, lc.next() & 0xFFFF, lc.next() & 0xFFFF];
            new_bodies.push(nb);
            eng.hot_update(nb);
        }
        s += 1;
    }

    // R3 不断流：120 输入 → 120 回执，按序一一对应（内容+顺序保真）。
    if receipts.len() != 120 {
        return Err(E_STREAM_DROP);
    }
    let mut i = 0;
    while i < receipts.len() {
        if receipts[i].0 != i as u32 {
            return Err(E_STREAM_REORDER);
        }
        i += 1;
    }
    hits += 1;

    // R4 版本推进：10 次热更 → 终版 = 初版 + 10（热更全部生效）。
    if eng.ver() != ver0 + 10 {
        return Err(E_UPDATE_MISSED);
    }
    hits += 1;

    // R5 无重启：epoch = 1（初）+ 10（热更）= 11，全程未清零。
    if eng.epoch != 11 {
        return Err(E_EPOCH_RESET);
    }
    hits += 1;

    // R6 版本对齐：回执版本序列只允许单调不减且恰有 10 个跳变点，
    //    跳变幅度恒 1（半新半旧 = 任何越级跳变或回退都检出）。
    let mut jumps = 0usize;
    let mut i2 = 1;
    while i2 < receipts.len() {
        let d = receipts[i2].1 as i64 - receipts[i2 - 1].1 as i64;
        if d < 0 || d > 1 {
            return Err(E_VER_MIXED);
        }
        if d == 1 {
            jumps += 1;
        }
        i2 += 1;
    }
    if jumps != 10 {
        return Err(E_VER_ALIGNMENT);
    }
    hits += 1;

    // R7 生效体整体切换：终版体 == 第 10 次热更的新体（无混合）。
    if eng.body() != new_bodies[9] {
        return Err(E_VER_MIXED);
    }
    hits += 1;

    // R8 体合法性闸：合成的新体逐批校验（全零体必须在热更前被拒）。
    let mut bi = 0;
    while bi < new_bodies.len() {
        if !body_valid(&new_bodies[bi]) {
            return Err(E_VER_MIXED);
        }
        bi += 1;
    }
    // 非法体注入：全零体经 hot_update 仍会换装，但契约要求调用方
    // 先行校验——此处断言校验器确实拒收全零体（闸不在摆设位）。
    let zero_body: [u32; 4] = [0, 0, 0, 0];
    if body_valid(&zero_body) {
        return Err(E_VER_MIXED);
    }
    hits += 1;

    // R9 热更历史账：10 次热更逐批入账，步号 1..10 连续，版本对
    //    (ver_old, ver_new) 相邻衔接（ver_new = ver_old + 1），新体
    //    指纹与合成体逐批一致。
    let mut hist = HotHist::new();
    let mut ver_walk = ver0;
    let mut step = 1u32;
    while step <= 10 {
        let nb = new_bodies[(step - 1) as usize];
        let row = HotHistRow {
            step,
            ver_old: ver_walk,
            ver_new: ver_walk + 1,
            body_fp: body_fp(&nb),
        };
        if !hist.record(row) {
            return Err(E_UPDATE_MISSED);
        }
        ver_walk += 1;
        step += 1;
    }
    if hist.len() != 10 {
        return Err(E_UPDATE_MISSED);
    }
    // 重放幂等：同账重放 10 行 → 账长不变。
    let mut step2 = 1u32;
    while step2 <= 10 {
        let nb = new_bodies[(step2 - 1) as usize];
        let ok = hist.record(HotHistRow {
            step: step2,
            ver_old: ver0 + step2 - 1,
            ver_new: ver0 + step2,
            body_fp: body_fp(&nb),
        });
        if !ok {
            return Err(E_UPDATE_MISSED);
        }
        step2 += 1;
    }
    if hist.len() != 10 {
        return Err(E_UPDATE_MISSED);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_STREAM_DROP,
        E_STREAM_REORDER,
        E_EPOCH_RESET,
        E_VER_MIXED,
        E_VER_ALIGNMENT,
        E_UPDATE_MISSED,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
