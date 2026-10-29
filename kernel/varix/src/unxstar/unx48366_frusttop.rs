//! unx48366_frusttop — UNX-F48366 最挫败十次操作报表（按信号加权出改进清单）（诊断段 0xCEA0）。
//!
//! 判据原文：
//!   J1 报表 ×3 生成且条目可溯源；判据锚定 体验十三 改进清单。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 挫败感只从信号加权来：卡候选/错上屏/反复重进/模式连翻/超时五类
//!      挫败信号各带固定权重，操作分 = Σ(信号次数 × 权重)，整数算术，
//!      不引入任何拍脑袋系数。
//!   2. 报表 ×3：三份独立数据快照（不同种子）各出一份改进清单，每份恰
//!      取 top-10，短缺 10 条即报表残缺。
//!   3. 条目可溯源：报表每一行都回指操作 id、原始信号计数与溯源校验和，
//!      得分必须能用原始计数独立重算复核——重算不符即账目造假。
//!   4. 真最大性质：落选操作里不存在严格大于第 10 名得分的条目，且得分
//!      相同时按操作 id 升序取——排序规则确定可复现。
//!   5. 幂等可复算：同一快照重复出表，指纹必须一致；对快照注入得分扰动
//!      后，报表必须随之改变（报表是活的，不是罐头）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCEA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_REPORT_SHORT: u16 = SEG_BASE + 1;   // 报表不足 10 条
const E_SORT_FAULT: u16 = SEG_BASE + 2;     // 排序规则被破坏
const E_PROVENANCE_FAULT: u16 = SEG_BASE + 3; // 条目溯源失败/得分重算不符
const E_MAXIMALITY_FAULT: u16 = SEG_BASE + 4; // 落选者存在更高分
const E_REPRO_FAULT: u16 = SEG_BASE + 5;    // 同快照重复出表指纹不一致
const E_TAMPER_INERT: u16 = SEG_BASE + 6;   // 注入扰动后报表未变（罐头）
const E_SIG_OVERFLOW: u16 = SEG_BASE + 7;   // 信号计数越合法域
const E_CODE_RANGE: u16 = SEG_BASE + 8;     // 诊断码越段

/// 挫败信号类别与固定权重表（指数）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SigKind {
    /// 卡候选：候选窗卡顿/不出词。
    StuckCandidate,
    /// 错上屏：提交串与预期不符。
    WrongCommit,
    /// 反复重进：同词反复删除重输。
    ReEnter,
    /// 模式连翻：短时中英反复切换。
    ModeFlip,
    /// 超时：等待响应超时。
    Timeout,
}

pub const SIG_KINDS: usize = 5;

/// 固定权重表（挫败感权重，整数）。
pub const SIG_WEIGHTS: [u32; SIG_KINDS] = [5, 8, 6, 3, 4];

/// 单次操作的信号账：五类信号各自的触发次数。
#[derive(Clone, Copy)]
pub struct OpRecord {
    pub op_id: u32,
    pub sig_counts: [u32; SIG_KINDS],
    /// 溯源校验和：原始事件的 FNV 折叠（报表核对用）。
    pub provenance: u64,
}

impl OpRecord {
    /// 得分 = Σ(信号次数 × 权重)。
    pub fn score(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < SIG_KINDS {
            s = s.wrapping_add(self.sig_counts[k].wrapping_mul(SIG_WEIGHTS[k]));
            k += 1;
        }
        s
    }
}

/// 报表条目：得分 + 回指原始操作账。
#[derive(Clone, Copy)]
pub struct ReportEntry {
    pub op_id: u32,
    pub score: u32,
    pub provenance: u64,
    pub sig_counts: [u32; SIG_KINDS],
}

/// 改进清单：top-10 操作报表。
pub struct FrustrTop {
    pub entries: Vec<ReportEntry>,
}

/// 从操作账出表：按得分降序、同分按 op_id 升序取前 10。
pub fn make_report(ops: &[OpRecord]) -> FrustrTop {
    // 选择排序：逐名次取「得分最高、同分 id 最小」的未选条目。
    let mut picked: Vec<bool> = Vec::new();
    let mut i = 0usize;
    while i < ops.len() {
        picked.push(false);
        i += 1;
    }
    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut rank = 0usize;
    while rank < 10 && rank < ops.len() {
        let mut best: Option<usize> = None;
        let mut i = 0usize;
        while i < ops.len() {
            if picked[i] {
                i += 1;
                continue;
            }
            let better = match best {
                None => true,
                Some(b) => {
                    let si = ops[i].score();
                    let sb = ops[b].score();
                    si > sb || (si == sb && ops[i].op_id < ops[b].op_id)
                }
            };
            if better {
                best = Some(i);
            }
            i += 1;
        }
        match best {
            Some(b) => {
                picked[b] = true;
                entries.push(ReportEntry {
                    op_id: ops[b].op_id,
                    score: ops[b].score(),
                    provenance: ops[b].provenance,
                    sig_counts: ops[b].sig_counts,
                });
            }
            None => break,
        }
        rank += 1;
    }
    FrustrTop { entries }
}

/// 报表指纹：FNV-1a 折叠条目序列。
pub fn fingerprint(r: &FrustrTop) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < r.entries.len() {
        h ^= r.entries[i].op_id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= r.entries[i].score as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= r.entries[i].provenance;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 确定性线性同余发生器（快照生成统一入口，禁真随机）。
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

/// 数据快照：确定性生成 40 个操作账（信号计数 + 溯源校验和）。
pub fn snapshot(seed: u32) -> Vec<OpRecord> {
    let mut lc = Lcg::new(seed);
    let mut ops: Vec<OpRecord> = Vec::new();
    let mut id: u32 = 0;
    while id < 40 {
        let mut counts = [0u32; SIG_KINDS];
        let mut k = 0usize;
        while k < SIG_KINDS {
            counts[k] = lc.next() % 7; // 0..6 次
            k += 1;
        }
        // 溯源校验和：折叠 (op_id, 计数) 序列。
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        let mut k = 0usize;
        while k < SIG_KINDS {
            h ^= counts[k] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            k += 1;
        }
        ops.push(OpRecord {
            op_id: id,
            sig_counts: counts,
            provenance: h,
        });
        id += 1;
    }
    ops
}

/// 自例程判据：逐条落实 J1 的全部断言（报表 ×3 生成且条目可溯源）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 报表 ×3：三份独立快照各出一表，每表恰 10 条。
    let mut reports: Vec<FrustrTop> = Vec::new();
    let mut r = 0u32;
    while r < 3 {
        let ops = snapshot((0x4836_6001) ^ (r * 104_729));
        let rep = make_report(&ops);
        if rep.entries.len() != 10 {
            return Err(E_REPORT_SHORT);
        }
        reports.push(rep);
        r += 1;
    }
    hits += 1;

    // R2 排序规则：表内得分非升序；同分者 op_id 升序。
    let mut r = 0usize;
    while r < reports.len() {
        let e = &reports[r].entries;
        let mut i = 1usize;
        while i < e.len() {
            if e[i].score > e[i - 1].score {
                return Err(E_SORT_FAULT);
            }
            if e[i].score == e[i - 1].score && e[i].op_id < e[i - 1].op_id {
                return Err(E_SORT_FAULT);
            }
            i += 1;
        }
        r += 1;
    }
    hits += 1;

    // R3 条目可溯源：每条得分用原始计数独立重算一致；校验和复算一致。
    r = 0;
    while r < reports.len() {
        let e = &reports[r].entries;
        let mut i = 0usize;
        while i < e.len() {
            let rec = OpRecord {
                op_id: e[i].op_id,
                sig_counts: e[i].sig_counts,
                provenance: e[i].provenance,
            };
            if rec.score() != e[i].score {
                return Err(E_PROVENANCE_FAULT);
            }
            let mut h: u64 = 0xCBF2_9CE4_8422_2325;
            h ^= rec.op_id as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            let mut k = 0usize;
            while k < SIG_KINDS {
                h ^= rec.sig_counts[k] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
                k += 1;
            }
            if h != e[i].provenance {
                return Err(E_PROVENANCE_FAULT);
            }
            i += 1;
        }
        r += 1;
    }
    hits += 1;

    // R4 真最大性质：快照落选操作中不存在严格大于第 10 名的得分。
    r = 0;
    while r < reports.len() {
        let seed = (0x4836_6001u32) ^ (r as u32 * 104_729);
        let ops = snapshot(seed);
        let cutoff = reports[r].entries[9].score;
        let mut i = 0usize;
        while i < ops.len() {
            let mut selected = false;
            let mut j = 0usize;
            while j < 10 {
                if reports[r].entries[j].op_id == ops[i].op_id {
                    selected = true;
                    break;
                }
                j += 1;
            }
            if !selected && ops[i].score() > cutoff {
                return Err(E_MAXIMALITY_FAULT);
            }
            i += 1;
        }
        r += 1;
    }
    hits += 1;

    // R5 幂等可复算：同一快照重复出表，指纹必须一致。
    r = 0;
    while r < reports.len() {
        let seed = (0x4836_6001u32) ^ (r as u32 * 104_729);
        let ops = snapshot(seed);
        let rep2 = make_report(&ops);
        if fingerprint(&rep2) != fingerprint(&reports[r]) {
            return Err(E_REPRO_FAULT);
        }
        r += 1;
    }
    hits += 1;

    // R6 报表活性自查：把一个落选低分操作的第一类信号拉满，出表必须变化。
    let mut ops = snapshot(0x4836_6001);
    let cutoff = reports[0].entries[9].score;
    // 找一个当前得分低于 cutoff 的落选操作。
    let mut victim: Option<u32> = None;
    let mut i = 0usize;
    while i < ops.len() {
        let mut selected = false;
        let mut j = 0usize;
        while j < 10 {
            if reports[0].entries[j].op_id == ops[i].op_id {
                selected = true;
                break;
            }
            j += 1;
        }
        if !selected && ops[i].score() < cutoff {
            victim = Some(ops[i].op_id);
            break;
        }
        i += 1;
    }
    match victim {
        Some(v) => {
            let vi = v as usize;
            ops[vi].sig_counts[1] = 20; // 错上屏 ×20：权重 8 → +160 分
            let rep2 = make_report(&ops);
            if fingerprint(&rep2) == fingerprint(&reports[0]) {
                return Err(E_TAMPER_INERT);
            }
            // 新表必须包含被拉满的操作。
            let mut found = false;
            let mut j = 0usize;
            while j < rep2.entries.len() {
                if rep2.entries[j].op_id == v {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(E_TAMPER_INERT);
            }
        }
        None => return Err(E_SIG_OVERFLOW),
    }
    hits += 1;

    // R7 信号合法域：快照全部计数 ≤ 6（生成域），权重表 5 项非零。
    let ops = snapshot(0x4836_6001);
    let mut i = 0usize;
    while i < ops.len() {
        let mut k = 0usize;
        while k < SIG_KINDS {
            if ops[i].sig_counts[k] > 6 {
                return Err(E_SIG_OVERFLOW);
            }
            k += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < SIG_KINDS {
        if SIG_WEIGHTS[k] == 0 {
            return Err(E_SIG_OVERFLOW);
        }
        k += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_REPORT_SHORT,
        E_SORT_FAULT,
        E_PROVENANCE_FAULT,
        E_MAXIMALITY_FAULT,
        E_REPRO_FAULT,
        E_TAMPER_INERT,
        E_SIG_OVERFLOW,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
