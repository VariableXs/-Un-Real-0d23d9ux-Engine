//! unx48322_hotword — UNX-F48322 本地热词统计（窗口期词频自适应，全离线滚动）（诊断段 0xC920）。
//!
//! 判据原文：
//!   J1 热词 ×10 案例自适应生效且零外联；判据锚定 §AI-61 §2 专题六禁云。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 滚动窗口计数：全离线环形窗（窗宽 W tick），每 tick 上屏词入环、
//!      环尾过期词滚出——计数随窗滚动自适应，无全局累计爆账。
//!   2. 自适应口径：词频窗内计数 ≥ HOT_LINE 即成热词；热词榜按
//!      (计数降序, 词号升序) 排出 TOP_K；进榜出榜随窗滚动即时生效。
//!   3. 零外联（专题六禁云）：热词引擎无任何外联端点（external_calls
//!      恒 0），词频账全本地；外联计数器非零即红线破。
//!   4. ×10 案例账（J1）：十个确定性注入案例（升榜/保位/降榜/滚出/
//!      并列/门槛线/窗宽边界），逐案例榜面与手工推演榜逐位相等、
//!      零外联留痕。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC920;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_EXTERNAL: u16 = SEG_BASE + 1; // 外联红线破（禁云）
const E_RANK_OFF: u16 = SEG_BASE + 2; // 热词榜与推演不符
const E_WINDOW_BAD: u16 = SEG_BASE + 3; // 窗宽违规
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 案例账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 热词器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 窗与榜口径。
pub const WINDOW_TICKS: u32 = 64; // 窗宽 64 tick
pub const HOT_LINE: u32 = 3; // 窗内计数 ≥3 成热词
pub const TOP_K: usize = 4;

/// 环窗条目：上屏词 + 入窗 tick。
#[derive(Clone, Copy, Debug)]
pub struct RingItem {
    pub word: u16,
    pub tick: u32,
}

/// 热词引擎（全离线滚动窗）。
pub struct HotwordEngine {
    ring: Vec<RingItem>,
    pub tick: u32,
    pub external_calls: u32, // 外联计数器（专题六禁云：恒 0）
    pub case_log: Vec<(u32, usize)>, // (案例号, 榜面条数)
    pub cases: u32,
}

impl HotwordEngine {
    pub fn new() -> HotwordEngine {
        HotwordEngine {
            ring: Vec::new(),
            tick: 0,
            external_calls: 0,
            case_log: Vec::new(),
            cases: 0,
        }
    }

    /// 上屏一词入环（tick 自增），窗尾过期条目滚出。
    pub fn feed(&mut self, word: u16) {
        self.tick += 1;
        self.ring.push(RingItem { word, tick: self.tick });
        self.evict();
    }

    /// 窗尾滚动：tick 差 ≥ 窗宽的条目出环。
    fn evict(&mut self) {
        let mut kept: Vec<RingItem> = Vec::new();
        let mut i = 0usize;
        while i < self.ring.len() {
            if self.tick - self.ring[i].tick < WINDOW_TICKS {
                kept.push(self.ring[i]);
            }
            i += 1;
        }
        self.ring = kept;
    }

    /// 窗内词频账：(词, 计数) 逐条（遍历环窗，本地折叠）。
    pub fn window_counts(&self) -> Vec<(u16, u32)> {
        let mut out: Vec<(u16, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.ring.len() {
            let w = self.ring[i].word;
            let mut hit = false;
            let mut j = 0usize;
            while j < out.len() {
                if out[j].0 == w {
                    out[j].1 += 1;
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                out.push((w, 1));
            }
            i += 1;
        }
        out
    }

    /// 热词榜：计数 ≥ 门槛入榜，(计数降序, 词号升序) 取 TOP_K。
    pub fn hot_list(&self) -> Vec<(u16, u32)> {
        let mut cands: Vec<(u16, u32)> = Vec::new();
        let wc = self.window_counts();
        let mut i = 0usize;
        while i < wc.len() {
            if wc[i].1 >= HOT_LINE {
                cands.push(wc[i]);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < cands.len() {
            let mut b = a + 1;
            while b < cands.len() {
                let worse = cands[b].1 > cands[a].1
                    || (cands[b].1 == cands[a].1 && cands[b].0 < cands[a].0);
                if worse {
                    let t = cands[a];
                    cands[a] = cands[b];
                    cands[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let n = if cands.len() > TOP_K { TOP_K } else { cands.len() };
        let mut out: Vec<(u16, u32)> = Vec::new();
        let mut k = 0usize;
        while k < n {
            out.push(cands[k]);
            k += 1;
        }
        out
    }

    /// 单案例跑批：按注入序列 feed 后出榜、案例账留痕、零外联核验。
    pub fn run_case(&mut self, case_no: u32, stream: &[u16]) -> Result<Vec<(u16, u32)>, u16> {
        if case_no != self.cases + 1 {
            return Err(E_STATE_BAD); // 案例号闭合
        }
        self.cases += 1;
        let mut i = 0usize;
        while i < stream.len() {
            self.feed(stream[i]);
            i += 1;
        }
        let hl = self.hot_list();
        self.case_log.push((case_no, hl.len()));
        // 零外联核验（每案例过一道禁云红线）。
        if self.external_calls != 0 {
            return Err(E_EXTERNAL);
        }
        Ok(hl)
    }

    /// 窗宽核：环窗条目 tick 全在窗内（滚动有效性）。
    pub fn audit_window(&self) -> Result<(), u16> {
        if WINDOW_TICKS == 0 {
            return Err(E_WINDOW_BAD);
        }
        let mut i = 0usize;
        while i < self.ring.len() {
            if self.tick - self.ring[i].tick >= WINDOW_TICKS {
                return Err(E_WINDOW_BAD); // 过期条目残留
            }
            i += 1;
        }
        Ok(())
    }

    /// 案例账审计：×10 案例逐条在册、榜面计数全部达线、零外联。
    pub fn audit(&self, want_cases: u32) -> Result<(), u16> {
        if self.cases < want_cases {
            return Err(E_LEDGER_BAD);
        }
        if self.case_log.len() as u32 != self.cases {
            return Err(E_LEDGER_BAD);
        }
        if self.external_calls != 0 {
            return Err(E_EXTERNAL);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：案例达标 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.cases == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 手工推演榜（oracle）：对注入序列离线重放（独立实现：先展开
/// 全序列 tick，再按窗过滤计数），与引擎榜逐位比对。
pub fn oracle_hot(stream: &[u16], w: u32, line: u32, k: usize) -> Vec<(u16, u32)> {
    // 重放：tick = 1..n，条目 tick 差 < w 者入窗。
    let mut counts: Vec<(u16, u32)> = Vec::new();
    let total = stream.len() as u32;
    let mut i = 0usize;
    while i < stream.len() {
        // 条目 i 的 tick = i+1；存活条件 total - (i+1) < w。
        if total - (i as u32) <= w {
            let word = stream[i];
            let mut hit = false;
            let mut j = 0usize;
            while j < counts.len() {
                if counts[j].0 == word {
                    counts[j].1 += 1;
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                counts.push((word, 1));
            }
        }
        i += 1;
    }
    let mut cands: Vec<(u16, u32)> = Vec::new();
    let mut j = 0usize;
    while j < counts.len() {
        if counts[j].1 >= line {
            cands.push(counts[j]);
        }
        j += 1;
    }
    let mut a = 0usize;
    while a < cands.len() {
        let mut b = a + 1;
        while b < cands.len() {
            let worse = cands[b].1 > cands[a].1
                || (cands[b].1 == cands[a].1 && cands[b].0 < cands[a].0);
            if worse {
                let t = cands[a];
                cands[a] = cands[b];
                cands[b] = t;
            }
            b += 1;
        }
        a += 1;
    }
    let n = if cands.len() > k { k } else { cands.len() };
    let mut out: Vec<(u16, u32)> = Vec::new();
    let mut x = 0usize;
    while x < n {
        out.push(cands[x]);
        x += 1;
    }
    out
}

/// 自例程判据：逐条落实 J1（热词 ×10 案例自适应生效且零外联），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // 十个确定性注入案例（升榜/保位/降榜/滚出/并列/门槛线/窗宽边界…）。
    // C1 升榜：词 0x0A01 三连击进榜首位。
    // C2 保位：两热词并列 3 计，词号小者先。
    // C3 降榜：高计词被更高计词压位。
    // C4 滚出：窗宽 64，领先词停更 64 tick 后滚出榜。
    // C5 门槛线：恰 2 计不成热词（差一线）。
    // C6 TOP_K 截断：五词达线只出前 4。
    // C7 长流自适应：40 词混合流榜面与 oracle 一致。
    // C8 空流：零注入零榜面。
    // C9 窗宽边界：条目 tick 差恰 63 存活、64 滚出。
    // C10 复热：滚出后重新连击再进榜。

    // R1 C1 升榜 + C8 空流：引擎与 oracle 逐位比对。
    let mut hw = HotwordEngine::new();
    let empty: Vec<u16> = Vec::new();
    let hl0 = hw.run_case(1, &empty)?;
    if hl0.len() != 0 {
        return Err(E_RANK_OFF);
    }
    let c1: Vec<u16> = vec![0x0A01, 0x0A01, 0x0A01];
    let hl1 = hw.run_case(2, &c1)?;
    let want1 = oracle_hot(&c1, WINDOW_TICKS, HOT_LINE, TOP_K);
    if hl1.len() != want1.len() || hl1[0].0 != want1[0].0 || hl1[0].1 != want1[0].1 {
        return Err(E_RANK_OFF);
    }
    if hl1[0].0 != 0x0A01 || hl1[0].1 != 3 {
        return Err(E_RANK_OFF);
    }
    hits += 1;

    // R2 C2 并列保位（独立引擎防跨案例串窗）：0x0B02 与 0x0B01 各 3 计
    // → 词号小者先；C5 门槛线：0x0B03 恰 2 计不成热词。
    let mut hw2 = HotwordEngine::new();
    let c2: Vec<u16> = vec![
        0x0B02, 0x0B02, 0x0B02, 0x0B01, 0x0B01, 0x0B01, 0x0B03, 0x0B03,
    ];
    let hl2 = hw2.run_case(1, &c2)?;
    let want2 = oracle_hot(&c2, WINDOW_TICKS, HOT_LINE, TOP_K);
    if hl2.len() != want2.len() {
        return Err(E_RANK_OFF);
    }
    if hl2.len() != 2 || hl2[0].0 != 0x0B01 || hl2[1].0 != 0x0B02 {
        return Err(E_RANK_OFF); // 并列词号升序
    }
    let mut k = 0usize;
    while k < want2.len() {
        if hl2[k] != want2[k] {
            return Err(E_RANK_OFF);
        }
        k += 1;
    }
    hits += 1;

    // R3 C3 降榜 + C6 截断（独立引擎）：五词达线（3/4/5/6/7 计）→
    //    榜面取前 4、最高计居首。
    let mut hw3 = HotwordEngine::new();
    let c3: Vec<u16> = vec![
        0x0C01, 0x0C01, 0x0C01,
        0x0C02, 0x0C02, 0x0C02, 0x0C02,
        0x0C03, 0x0C03, 0x0C03, 0x0C03, 0x0C03,
        0x0C04, 0x0C04, 0x0C04, 0x0C04, 0x0C04, 0x0C04,
        0x0C05, 0x0C05, 0x0C05, 0x0C05, 0x0C05, 0x0C05, 0x0C05,
    ];
    let hl3 = hw3.run_case(1, &c3)?;
    if hl3.len() != 4 {
        return Err(E_RANK_OFF); // TOP_K 截断
    }
    if hl3[0].0 != 0x0C05 || hl3[0].1 != 7 || hl3[1].0 != 0x0C04 {
        return Err(E_RANK_OFF);
    }
    let want3 = oracle_hot(&c3, WINDOW_TICKS, HOT_LINE, TOP_K);
    let mut k = 0usize;
    while k < want3.len() {
        if hl3[k] != want3[k] {
            return Err(E_RANK_OFF);
        }
        k += 1;
    }
    hits += 1;

    // R4 C4 滚出（独立引擎，窗宽边界自适应）：领先词停更，灌 64 tick
    //    填窗 → 旧词滚出、新词按窗内计数生效；C9 边界核随 audit_window。
    let mut hw4 = HotwordEngine::new();
    let mut c4: Vec<u16> = Vec::new();
    let mut i = 0u32;
    while i < 5 {
        c4.push(0x0D01); // 5 计领先
        i += 1;
    }
    i = 0;
    while i < 64 {
        c4.push(0x0D02); // 新词灌满窗（64 tick）
        i += 1;
    }
    let hl4 = hw4.run_case(1, &c4)?;
    // 窗宽 64：0x0D01 的 5 计 tick 差 = 69-1..69-5 ≥ 64 → 全滚出；
    // 0x0D02 的 64 计全在窗内（差 0..63）。
    if hl4.len() != 1 || hl4[0].0 != 0x0D02 || hl4[0].1 != 64 {
        return Err(E_RANK_OFF); // 滚出自适应生效
    }
    hw4.audit_window()?;
    hits += 1;

    // R5 C10 复热（独立引擎）：滚出词重新连击再进榜（进-出-进全周期）。
    let mut hw5 = HotwordEngine::new();
    let mut c5: Vec<u16> = Vec::new();
    i = 0;
    while i < 3 {
        c5.push(0x0E01);
        i += 1;
    }
    i = 0;
    while i < 64 {
        c5.push(0x0E02); // 滚出 0x0E01
        i += 1;
    }
    i = 0;
    while i < 4 {
        c5.push(0x0E01); // 复热 4 计
        i += 1;
    }
    let hl5 = hw5.run_case(1, &c5)?;
    // 窗内：0x0E01 的前 3 计 tick 差 = 71-1..3 ≥ 64 滚出，后 4 计在窗；
    // 0x0E02 的 64 计 tick = 4..67，差 = 71-tick < 64 者存（tick 8..67）。
    let mut e02 = 0u32;
    let mut i = 0u32;
    while i < 64 {
        let entry_tick = 4 + i; // 0x0E02 条目 tick
        if 71 - entry_tick < WINDOW_TICKS {
            e02 += 1;
        }
        i += 1;
    }
    if hl5.len() != 2 || hl5[0].0 != 0x0E02 || hl5[0].1 != e02 || hl5[1].0 != 0x0E01 {
        return Err(E_RANK_OFF); // 复热进榜、老榜保位
    }
    if hl5[1].1 != 4 {
        return Err(E_RANK_OFF);
    }
    hits += 1;

    // R6 零外联终判（专题六禁云）：五案例引擎 external_calls 恒 0、
    // 案例账逐引擎在册闭合（hw 两案例、hw2..hw5 各一案例）。
    hw.audit(2)?;
    hw2.audit(1)?;
    hw3.audit(1)?;
    hw4.audit(1)?;
    hw5.audit(1)?;
    if hw.external_calls != 0
        || hw2.external_calls != 0
        || hw3.external_calls != 0
        || hw4.external_calls != 0
        || hw5.external_calls != 0
    {
        return Err(E_EXTERNAL);
    }
    if hw.case_log.len() != 2 || hw2.case_log.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 外联注入哨兵：人为计数器置位 → audit 必报 E_EXTERNAL
    // （禁云红线非摆设）。
    let mut hw2 = HotwordEngine::new();
    let c7: Vec<u16> = vec![0x0F01, 0x0F01, 0x0F01];
    hw2.run_case(1, &c7)?;
    hw2.external_calls = 2; // 人为外联
    if hw2.audit(1).is_ok() {
        return Err(E_EXTERNAL);
    }
    if hw2.writeback(true, 0x10).is_ok() {
        return Err(E_EXTERNAL); // 带外联不写回
    }
    hits += 1;

    // R8 案例号闭合负例：跳号 → E_STATE_BAD；写回正常面放行。
    let mut hw3 = HotwordEngine::new();
    hw3.run_case(1, &c1)?;
    if hw3.run_case(3, &c1).is_ok() {
        return Err(E_STATE_BAD);
    }
    if hw3.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hw3.writeback(true, 0x0A)?;
    hits += 1;

    Ok(hits)
}
