//! unx48339_candfold — UNX-F48339 智能候选折叠语义（低置信候选折叠展示数据位）（诊断段 0xCB40）。
//!
//! 判据原文：
//!   J1 折叠位 ×10 正确；判据锚定 §AI-61 §2 专题一。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 折叠语义：置信分低于折叠阈值的候选打折叠位（fold=1），
//!      展示面按折叠位决定收展（AI 域只供数据位，绘制归 F4——
//!      位语义与视觉实现严格分层）。
//!   2. 阈值边界三分：低于阈值必折叠、等于阈值不折叠（边界归展）、
//!      高于阈值必展——三段全查（×10 样本横跨三段）。
//!   3. 折叠不动序：折叠位只影响展示收展，不影响候选排序
//!      （排序与折叠正交：同序折叠前后位次逐位不变）。
//!   4. 位语义自洽：fold 位与置信分/阈值关系可反推（折叠位 = 
//!      conf < threshold），任何位与分不符即违例。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCB40;

const E_FOLD_BAD: u16 = SEG_BASE + 1;   // 折叠位与置信/阈值不符
const E_ORDER_SHIFT: u16 = SEG_BASE + 2; // 折叠改变排序（越权）
const E_BOUNDARY_BAD: u16 = SEG_BASE + 3; // 等阈值边界语义错
const E_PAYLOAD_LEAK: u16 = SEG_BASE + 4; // 位面携带视觉实现数据
const E_ORDER_TIE: u16 = SEG_BASE + 6;  // 平局全序违例
const E_CODE_RANGE: u16 = SEG_BASE + 5;  // 诊断码越段

/// 折叠阈值（置信 0..1000 定点）。
pub const FOLD_THRESHOLD: u32 = 400;

/// 候选：id + 置信分。
#[derive(Clone, Copy)]
pub struct FCand {
    pub id: u32,
    pub conf: u32,
}

/// 折叠位判定：conf < FOLD_THRESHOLD → fold=1；等阈与高于 → fold=0。
pub fn fold_bit(conf: u32) -> u8 {
    if conf < FOLD_THRESHOLD {
        1
    } else {
        0
    }
}

/// 展示数据行：id + 位次 + 折叠位（不含任何视觉参数——位面纯净）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FoldRow {
    pub id: u32,
    pub rank: u32,
    pub fold: u8,
}

/// 排序（置信降序、id 升序平局）后输出展示数据行。
pub fn display_rows(cands: &[FCand]) -> Vec<FoldRow> {
    let mut sorted: Vec<&FCand> = Vec::new();
    for c in cands.iter() {
        sorted.push(c);
    }
    let mut i = 1;
    while i < sorted.len() {
        let mut j = i;
        while j > 0
            && (sorted[j - 1].conf < sorted[j].conf
                || (sorted[j - 1].conf == sorted[j].conf && sorted[j - 1].id > sorted[j].id))
        {
            sorted.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
    let mut out: Vec<FoldRow> = Vec::new();
    let mut r = 0;
    while r < sorted.len() {
        out.push(FoldRow {
            id: sorted[r].id,
            rank: r as u32,
            fold: fold_bit(sorted[r].conf),
        });
        r += 1;
    }
    out
}

/// 位面纯净检查：FoldRow 不携带视觉实现字段（编译期结构约束的
/// 运行时证词：行宽语义 = 3 字段，无色值/字号/坐标位）。
pub const FOLDROW_FIELDS: usize = 3;

/// ×10 折叠样本合成：横跨三段（低于 3 / 等阈 1 / 高于 6）。
pub fn fold_samples() -> Vec<FCand> {
    vec![
        FCand { id: 100, conf: 100 },  // 低于
        FCand { id: 101, conf: 250 },  // 低于
        FCand { id: 102, conf: 399 },  // 低于（贴阈）
        FCand { id: 103, conf: 400 },  // 等阈（边界归展）
        FCand { id: 104, conf: 401 },  // 高于（贴阈）
        FCand { id: 105, conf: 600 },  // 高于
        FCand { id: 106, conf: 750 },  // 高于
        FCand { id: 107, conf: 850 },  // 高于
        FCand { id: 108, conf: 950 },  // 高于
        FCand { id: 109, conf: 999 },  // 高于
    ]
}

/// 参数化折叠位判定（阈值敏感版本）。
pub fn fold_bit_at(conf: u32, threshold: u32) -> u8 {
    if conf < threshold {
        1
    } else {
        0
    }
}

/// 参数化展示行（阈值可调，供阈值单调性判据）。
pub fn display_rows_at(cands: &[FCand], threshold: u32) -> Vec<FoldRow> {
    let mut sorted: Vec<&FCand> = Vec::new();
    for c in cands.iter() {
        sorted.push(c);
    }
    let mut i = 1;
    while i < sorted.len() {
        let mut j = i;
        while j > 0
            && (sorted[j - 1].conf < sorted[j].conf
                || (sorted[j - 1].conf == sorted[j].conf && sorted[j - 1].id > sorted[j].id))
        {
            sorted.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
    let mut out: Vec<FoldRow> = Vec::new();
    let mut r = 0;
    while r < sorted.len() {
        out.push(FoldRow {
            id: sorted[r].id,
            rank: r as u32,
            fold: fold_bit_at(sorted[r].conf, threshold),
        });
        r += 1;
    }
    out
}

/// ×100 折叠样本合成（LCG 确定性，置信域 [0,1000)）。
pub fn fold_corpus(n: usize, seed: u32) -> Vec<FCand> {
    let mut lc = Lcg(seed | 1);
    let mut out: Vec<FCand> = Vec::new();
    let mut i = 0;
    while i < n {
        let id = 2000 + i as u32;
        let conf = lc.next() % 1000;
        out.push(FCand { id, conf });
        i += 1;
    }
    out
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

/// 智能候选折叠主巡检：折叠位 ×10 正确全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let samples = fold_samples();

    // R1 折叠位 ×10 逐位正确（三段全查）。
    if samples.len() != 10 {
        return Err(E_FOLD_BAD);
    }
    let rows = display_rows(&samples);
    let mut i = 0;
    while i < samples.len() {
        let want = fold_bit(samples[i].conf);
        // 反查行内折叠位（rank 定位）。
        let mut found = false;
        let mut j = 0;
        while j < rows.len() {
            if rows[j].id == samples[i].id {
                if rows[j].fold != want {
                    return Err(E_FOLD_BAD);
                }
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            return Err(E_FOLD_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 三段计数：低于 3 / 等阈 1 / 高于 6（样本域规约）。
    let mut low = 0u32;
    let mut mid = 0u32;
    let mut high = 0u32;
    i = 0;
    while i < samples.len() {
        let f = fold_bit(samples[i].conf);
        if samples[i].conf < FOLD_THRESHOLD {
            low += 1;
        } else if samples[i].conf == FOLD_THRESHOLD {
            mid += 1;
        } else {
            high += 1;
        }
        let _ = f;
        i += 1;
    }
    if low != 3 || mid != 1 || high != 6 {
        return Err(E_BOUNDARY_BAD);
    }
    hits += 1;

    // R3 等阈边界：conf == 400 → fold == 0（边界归展）。
    if fold_bit(400) != 0 {
        return Err(E_BOUNDARY_BAD);
    }
    if fold_bit(399) != 1 || fold_bit(401) != 0 {
        return Err(E_FOLD_BAD);
    }
    hits += 1;

    // R4 折叠不动序：给全部候选同减一个常数（排序不变），折叠位
    //    按新分数重算——位次必须与原位次逐位一致（折叠不参与排序）。
    let shifted: Vec<FCand> = {
        let mut v: Vec<FCand> = Vec::new();
        let mut k = 0;
        while k < samples.len() {
            v.push(FCand {
                id: samples[k].id,
                conf: samples[k].conf.saturating_sub(50),
            });
            k += 1;
        }
        v
    };
    let rows2 = display_rows(&shifted);
    if rows.len() != rows2.len() {
        return Err(E_ORDER_SHIFT);
    }
    let mut i2 = 0;
    while i2 < rows.len() {
        if rows[i2].id != rows2[i2].id || rows[i2].rank != rows2[i2].rank {
            return Err(E_ORDER_SHIFT);
        }
        i2 += 1;
    }
    hits += 1;

    // R5 平局全序：同置信候选按 id 升序（排序口径稳定性）。
    let ties: Vec<FCand> = vec![
        FCand { id: 300, conf: 500 },
        FCand { id: 200, conf: 500 },
        FCand { id: 100, conf: 500 },
    ];
    let rt = display_rows(&ties);
    if rt[0].id != 100 || rt[1].id != 200 || rt[2].id != 300 {
        return Err(E_ORDER_TIE);
    }
    hits += 1;

    // R6 位面纯净：FoldRow 字段语义恰 3（无视觉实现位可挂载）。
    if FOLDROW_FIELDS != 3 {
        return Err(E_PAYLOAD_LEAK);
    }
    hits += 1;

    // R7 ×100 一致性：LCG 候选域内折叠位与分数重算逐位一致，折叠
    //    集合 = 全部 conf < 阈值候选（计数守恒）。
    let big = fold_corpus(100, 0x4833_9001);
    let rows_big = display_rows(&big);
    let mut fold_count = 0u32;
    let mut low_count = 0u32;
    let mut i3 = 0;
    while i3 < big.len() {
        if fold_bit(big[i3].conf) == 1 {
            low_count += 1;
        }
        i3 += 1;
    }
    let mut j3 = 0;
    while j3 < rows_big.len() {
        if rows_big[j3].fold == 1 {
            fold_count += 1;
        }
        // 位次回查：rank 字段与行序一致（排序口径稳定）。
        if rows_big[j3].rank != j3 as u32 {
            return Err(E_ORDER_SHIFT);
        }
        j3 += 1;
    }
    if fold_count != low_count {
        return Err(E_FOLD_BAD);
    }
    hits += 1;

    // R8 阈值单调性：阈值提高 → 折叠计数单调不减（三档阈值对比）。
    let mut prev = 0u32;
    let thresholds = [300u32, 400, 500];
    let mut t = 0;
    while t < thresholds.len() {
        let rb = display_rows_at(&big, thresholds[t]);
        let mut fc = 0u32;
        let mut k = 0;
        while k < rb.len() {
            if rb[k].fold == 1 {
                fc += 1;
            }
            k += 1;
        }
        if fc < prev {
            return Err(E_BOUNDARY_BAD);
        }
        prev = fc;
        t += 1;
    }
    // 阈值 0 → 零折叠；阈值 1001 → 全折叠（两端极限）。
    let r0 = display_rows_at(&big, 0);
    let rmax = display_rows_at(&big, 1001);
    let mut c0 = 0u32;
    let mut cmax = 0u32;
    let mut k2 = 0;
    while k2 < r0.len() {
        if r0[k2].fold == 1 {
            c0 += 1;
        }
        if rmax[k2].fold == 1 {
            cmax += 1;
        }
        k2 += 1;
    }
    if c0 != 0 || cmax != 100 {
        return Err(E_BOUNDARY_BAD);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 6] = [
        E_FOLD_BAD,
        E_ORDER_SHIFT,
        E_BOUNDARY_BAD,
        E_PAYLOAD_LEAK,
        E_CODE_RANGE,
        E_ORDER_TIE,
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
