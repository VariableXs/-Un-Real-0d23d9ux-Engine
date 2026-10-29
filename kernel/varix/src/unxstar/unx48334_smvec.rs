//! unx48334_smvec — UNX-F48334 智能面向量账（联想/纠错用例集+校验和）（诊断段 0xCAA0）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 两族用例：联想（Suggest ×11）/纠错（Autocorrect ×9），载荷为
//!      确定性种子（键入序列 + bigram 表版本号）。
//!   2. 可复跑 = 双跑逐位一致：每条用例跑两遍，事件迹（联想三元组
//!      序列 / 纠错降权位序列）经 FNV-1a 折叠后两遍相等。
//!   3. 校验和唯一性：20 条用例校验和两两不同（异迹同和 = 碰撞
//!      必须暴露）；迹首带用例 id，天然可分但 uniqueness 判据照查。
//!   4. 篡改检出：任一载荷翻转一位 → 校验和必变（20/20）。
//!   5. 向量账 append-only：同用例重放幂等不重记；账行（用例id,
//!      校验和, 迹长）三元组构成智能面回归最小留痕。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCAA0;

const E_CASE_COUNT: u16 = SEG_BASE + 1;   // 用例总数 != 20
const E_KIND_COVER: u16 = SEG_BASE + 2;   // 族配比不满足 11/9
const E_REPLAY_DRIFT: u16 = SEG_BASE + 3; // 复跑校验和漂移
const E_CK_COLLIDE: u16 = SEG_BASE + 4;   // 校验和账内碰撞
const E_TAMPER_MISS: u16 = SEG_BASE + 5;  // 篡改未改变校验和
const E_LEDGER_BAD: u16 = SEG_BASE + 6;   // 向量账非幂等/缺失
const E_TRACE_EMPTY: u16 = SEG_BASE + 7;  // 事件迹为空
const E_CODE_RANGE: u16 = SEG_BASE + 8;   // 诊断码越段

/// 用例两族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SmKind {
    /// 联想用例。
    Suggest,
    /// 纠错用例。
    Autocorrect,
}

/// 向量用例：种子 + bigram 表版本（版本位参与指纹）。
#[derive(Clone, Copy)]
pub struct SmCase {
    pub id: u32,
    pub kind: SmKind,
    pub seed: u32,
    pub table_ver: u32,
}

/// 向量账行。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SmLedgerRow {
    pub case_id: u32,
    pub checksum: u64,
    pub trace_len: u32,
}

/// 向量账：append-only，同用例重放幂等不重记。
pub struct SmVecLedger {
    rows: Vec<SmLedgerRow>,
}

impl SmVecLedger {
    pub fn new() -> Self {
        SmVecLedger { rows: Vec::new() }
    }

    pub fn record(&mut self, row: SmLedgerRow) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].case_id == row.case_id {
                return self.rows[i].checksum == row.checksum
                    && self.rows[i].trace_len == row.trace_len;
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

/// FNV-1a 64（u32 双半折叠）。
pub fn checksum(trace: &[u32]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < trace.len() {
        h ^= (trace[i] as u64) & 0xFFFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= ((trace[i] >> 16) as u64) & 0xFFFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
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

/// bigram 续接表（与 48333 同源版本 v2：行序=键位，三元组续接）。
pub const BIGRAM_V2: [[u8; 3]; 26] = [
    [2, 3, 5],
    [7, 11, 13],
    [17, 19, 23],
    [25, 1, 4],
    [6, 8, 10],
    [12, 14, 16],
    [18, 20, 22],
    [24, 0, 3],
    [5, 9, 15],
    [21, 25, 2],
    [4, 14, 24],
    [1, 11, 21],
    [2, 12, 22],
    [3, 13, 23],
    [4, 14, 24],
    [5, 15, 25],
    [6, 16, 0],
    [7, 17, 1],
    [8, 18, 2],
    [9, 19, 3],
    [10, 20, 4],
    [11, 21, 5],
    [12, 22, 6],
    [13, 23, 7],
    [14, 24, 8],
    [15, 25, 9],
];

/// 键位混淆表（n↔l=0x1、f↔h=0x2、z↔c=0x3 语义位）。
pub const CONFUSABLE_V2: [u8; 26] = [
    0, 0, 0x3, 0, 0, 0, 0x2, 0, 0, 0, 0, 0x1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// 联想用例模拟：从起始键走 bigram 链 k 步（k=4..8），迹 = 三元组序列。
pub fn run_suggest(seed: u32, table_ver: u32, trace: &mut Vec<u32>) {
    let mut lc = Lcg::new(seed);
    let steps = 4 + (seed % 5) as usize;
    trace.push(0x5000_0001); // 联想会话开
    trace.push(table_ver & 0xFFFF);
    let mut key = (lc.next() % 26) as u8;
    let mut i = 0;
    while i < steps {
        let trip = BIGRAM_V2[(key % 26) as usize];
        trace.push(0x5000_0100 + (trip[0] as u32) * 0x10000 + (trip[1] as u32) * 0x100 + trip[2] as u32);
        // 链推进：取三元组中位为下一键（确定性游走）。
        key = trip[1];
        i += 1;
    }
    trace.push(0x5000_0F00 + steps as u32);
}

/// 纠错用例模拟：扫描键序，降权位命中即登记（迹 = (键,惩罚位) 对）。
pub fn run_autocorrect(seed: u32, table_ver: u32, trace: &mut Vec<u32>) {
    let mut lc = Lcg::new(seed);
    let n = 6 + (seed % 4) as usize;
    trace.push(0x5100_0001); // 纠错会话开
    trace.push(table_ver & 0xFFFF);
    let mut hits = 0u32;
    let mut i = 0;
    while i < n {
        let key = (lc.next() % 26) as u8;
        let pen = CONFUSABLE_V2[(key % 26) as usize];
        trace.push(0x5100_0100 + (key as u32) * 0x10 + pen as u32);
        if pen != 0 {
            hits += 1;
        }
        i += 1;
    }
    trace.push(0x5100_0F00 + hits);
}

/// 用例执行：按族分派。
pub fn run_case(c: &SmCase) -> Vec<u32> {
    let mut trace: Vec<u32> = Vec::new();
    trace.push(c.id);
    match c.kind {
        SmKind::Suggest => run_suggest(c.seed, c.table_ver, &mut trace),
        SmKind::Autocorrect => run_autocorrect(c.seed, c.table_ver, &mut trace),
    }
    trace
}

/// 用例集装配：20 条（Suggest 11 / Autocorrect 9），每族含纠错命中
/// 与未命中两类样本（覆盖两路）。
pub fn build_cases() -> Vec<SmCase> {
    let mut cases: Vec<SmCase> = Vec::new();
    let sug_seeds: [u32; 11] = [
        0x0000_0011, 0x0000_0023, 0x0000_0037, 0x0000_004B, 0x0000_005D,
        0x0000_0067, 0x0000_0079, 0x0000_008B, 0x0000_009D, 0x0000_00A1,
        0x0000_00B3,
    ];
    let mut i = 0;
    while i < 11 {
        cases.push(SmCase {
            id: 700 + i as u32,
            kind: SmKind::Suggest,
            seed: sug_seeds[i],
            table_ver: 2,
        });
        i += 1;
    }
    // 纠错种子：含高混淆命中种子（低位聚集混淆键位）与低命中种子。
    let ac_seeds: [u32; 9] = [
        0x0000_0B02, 0x0000_0C03, 0x0000_0A01, 0x0000_1F07, 0x0000_2F06,
        0x0000_3F05, 0x0000_4411, 0x0000_5502, 0x0000_6604,
    ];
    i = 0;
    while i < 9 {
        cases.push(SmCase {
            id: 800 + i as u32,
            kind: SmKind::Autocorrect,
            seed: ac_seeds[i],
            table_ver: 2,
        });
        i += 1;
    }
    cases
}

/// 智能面向量账主巡检：×20 用例双跑可复跑全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let cases = build_cases();

    // R1 用例总数恰 20。
    if cases.len() != 20 {
        return Err(E_CASE_COUNT);
    }
    hits += 1;

    // R2 族配比 11/9。
    let mut n_sug = 0usize;
    let mut n_ac = 0usize;
    let mut i = 0;
    while i < cases.len() {
        match cases[i].kind {
            SmKind::Suggest => n_sug += 1,
            SmKind::Autocorrect => n_ac += 1,
        }
        i += 1;
    }
    if n_sug != 11 || n_ac != 9 {
        return Err(E_KIND_COVER);
    }
    hits += 1;

    // R3 首跑 ×20：迹非空 + 校验和入账。
    let mut led = SmVecLedger::new();
    let mut ck_first: Vec<u64> = Vec::new();
    let mut tl_first: Vec<u32> = Vec::new();
    i = 0;
    while i < cases.len() {
        let trace = run_case(&cases[i]);
        if trace.is_empty() {
            return Err(E_TRACE_EMPTY);
        }
        let ck = checksum(&trace);
        if !led.record(SmLedgerRow {
            case_id: cases[i].id,
            checksum: ck,
            trace_len: trace.len() as u32,
        }) {
            return Err(E_LEDGER_BAD);
        }
        ck_first.push(ck);
        tl_first.push(trace.len() as u32);
        i += 1;
    }
    if led.len() != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 复跑 ×20：双跑校验和与迹长逐条相等。
    i = 0;
    while i < cases.len() {
        let trace = run_case(&cases[i]);
        if trace.len() as u32 != tl_first[i] || checksum(&trace) != ck_first[i] {
            return Err(E_REPLAY_DRIFT);
        }
        i += 1;
    }
    hits += 1;

    // R5 校验和唯一性：20 和两两不同。
    let mut a = 0;
    while a < ck_first.len() {
        let mut b = a + 1;
        while b < ck_first.len() {
            if ck_first[a] == ck_first[b] {
                return Err(E_CK_COLLIDE);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R6 篡改检出：载荷翻转一位 → 校验和必变（20/20）。
    i = 0;
    while i < cases.len() {
        let mut c2 = cases[i];
        c2.seed ^= 0x0000_0001;
        if checksum(&run_case(&c2)) == ck_first[i] {
            return Err(E_TAMPER_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R7 幂等重放：首跑值重放 20 条 → 账长不变、全部一致。
    i = 0;
    while i < cases.len() {
        if !led.record(SmLedgerRow {
            case_id: cases[i].id,
            checksum: ck_first[i],
            trace_len: tl_first[i],
        }) {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    if led.len() != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 纠错双路覆盖：9 条纠错用例中命中与未命中两类都有样本
    //    （迹尾命中计数 >0 与 ==0 各至少 1 条）。
    let mut hit_pos = 0u32;
    let mut hit_zero = 0u32;
    i = 0;
    while i < cases.len() {
        if cases[i].kind == SmKind::Autocorrect {
            let trace = run_case(&cases[i]);
            let tail = trace[trace.len() - 1];
            if (tail & 0xFFFF) > 0 {
                hit_pos += 1;
            } else {
                hit_zero += 1;
            }
        }
        i += 1;
    }
    if hit_pos == 0 || hit_zero == 0 {
        return Err(E_KIND_COVER);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_CASE_COUNT,
        E_KIND_COVER,
        E_REPLAY_DRIFT,
        E_CK_COLLIDE,
        E_TAMPER_MISS,
        E_LEDGER_BAD,
        E_TRACE_EMPTY,
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
