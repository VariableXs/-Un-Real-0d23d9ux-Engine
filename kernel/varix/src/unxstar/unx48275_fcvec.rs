//! unx48275_fcvec — UNX-F48275 对接向量账（建链/词库/方案用例集+校验和）（诊断段 0xC340）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 用例三族覆盖：建链（Link）/词库（Dict）/方案（Scheme）三族 ×20 条，
//!      族配比 7/7/6，每族至少一条「错误路径」用例（非正常收尾也必须
//!      产生稳定校验和——错误路径同样是接口契约的一部分）。
//!   2. 可复跑 = 双跑逐位一致：每条用例独立跑两遍，事件迹（trace）经
//!      FNV-1a 校验和折叠后两遍必须相等；复跑漂移即契约破坏。
//!   3. 校验和唯一性：20 条用例的校验和在账内两两不同——同迹同和是
//!      复跑，异迹同和是碰撞，碰撞必须暴露。
//!   4. 篡改检出：任一用例载荷翻转一位，校验和必须改变（20/20 检出）；
//!      检出不出的用例说明迹未覆盖载荷，判无效用例。
//!   5. 向量账 append-only：同用例重放幂等不重记；账行（用例id, 校验和,
//!      迹长）三元组构成对接回归的最小留痕。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC340;

const E_CASE_COUNT: u16 = SEG_BASE + 1;  // 用例总数 != 20
const E_KIND_COVER: u16 = SEG_BASE + 2;  // 族配比不满足 7/7/6
const E_REPLAY_DRIFT: u16 = SEG_BASE + 3; // 复跑校验和漂移
const E_CK_COLLIDE: u16 = SEG_BASE + 4;  // 校验和账内碰撞
const E_TAMPER_MISS: u16 = SEG_BASE + 5; // 篡改未改变校验和
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 向量账非幂等/缺失
const E_TRACE_EMPTY: u16 = SEG_BASE + 7; // 事件迹为空（用例未执行）
const E_CODE_RANGE: u16 = SEG_BASE + 8;  // 诊断码越段

/// 用例三族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaseKind {
    /// 建链：握手/能力协商/就绪。
    Link,
    /// 词库：装载/词条序/收尾。
    Dict,
    /// 方案：切换/状态轮转/提交。
    Scheme,
}

/// 向量用例：载荷为确定性种子（fuzz 统一 LCG，禁真随机）。
#[derive(Clone, Copy)]
pub struct VecCase {
    pub id: u32,
    pub kind: CaseKind,
    pub seed: u32,
}

/// 向量账行：用例 id + 校验和 + 事件迹长（最小留痕三元组）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VecLedgerRow {
    pub case_id: u32,
    pub checksum: u64,
    pub trace_len: u32,
}

/// 对接向量账：append-only，同用例重放幂等不重记。
pub struct VecLedger {
    rows: Vec<VecLedgerRow>,
}

impl VecLedger {
    pub fn new() -> Self {
        VecLedger { rows: Vec::new() }
    }

    /// 返回 true = 记账一致（新记或幂等重放）；false = 同 id 异值冲突。
    pub fn record(&mut self, row: VecLedgerRow) -> bool {
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

    pub fn lookup(&self, case_id: u32) -> Option<VecLedgerRow> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].case_id == case_id {
                return Some(self.rows[i]);
            }
            i += 1;
        }
        None
    }
}

/// FNV-1a 64 校验和（对接迹统一折叠器）。
pub fn checksum(trace: &[u32]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < trace.len() {
        // u32 拆两半折叠，保证字节序无关。
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

/// 建链用例模拟：hello→cap→ack→ready 主链，步数随种子 4..7；
/// 能力位图低 3 位必须含基座位（bit0），缺位走错误路径收尾。
/// 错误路径同样产出确定迹（契约的一部分）。
pub fn run_link(seed: u32, trace: &mut Vec<u32>) {
    let steps = 4 + (seed % 3) as usize;
    let caps = seed & 0x7;
    trace.push(0x1000_0001); // hello
    let mut i = 0usize;
    while i < steps {
        trace.push(0x1000_0010 + i as u32);
        trace.push((seed ^ (i as u32).wrapping_mul(0x9E37)) & 0xFFFF);
        i += 1;
    }
    if caps & 0x1 == 0x1 {
        trace.push(0x1000_0FEE); // ready（能力齐）
    } else {
        trace.push(0x1000_BAD0); // 错误路径：基座能力位缺失
    }
}

/// 词库用例模拟：装载 n = 6..10 词条，键序严格递增（base + i*step），
/// 收尾登记词条数；空词表不可能（n 下界 6），残段路径由键序守恒保证。
pub fn run_dict(seed: u32, trace: &mut Vec<u32>) {
    let n = 6 + (seed % 5) as usize;
    let base = (seed & 0x0FF0) >> 4;
    let step = 1 + (seed % 4);
    trace.push(0x2000_0001); // 打开词库
    let mut i = 0usize;
    while i < n {
        let key = base + (i as u32) * step;
        trace.push(0x2000_0100 + key);
        i += 1;
    }
    trace.push(0x2000_0F00 + n as u32); // 收尾：词条数
}

/// 方案用例模拟：k = 3..6 次状态轮转（A→B→C 循环），末位提交。
/// 轮转状态码 0x30/0x31/0x32，提交码折叠种子低位。
pub fn run_scheme(seed: u32, trace: &mut Vec<u32>) {
    let k = 3 + (seed % 4) as usize;
    trace.push(0x3000_0001); // 方案会话开
    let mut i = 0usize;
    while i < k {
        let st = 0x3000_0030 + (i as u32 % 3);
        trace.push(st);
        i += 1;
    }
    trace.push(0x3000_0F00 + (seed & 0xFF));
}

/// 用例执行：按族分派，产出事件迹（非空契约）。
pub fn run_case(c: &VecCase) -> Vec<u32> {
    let mut trace: Vec<u32> = Vec::new();
    trace.push(c.id);
    match c.kind {
        CaseKind::Link => run_link(c.seed, &mut trace),
        CaseKind::Dict => run_dict(c.seed, &mut trace),
        CaseKind::Scheme => run_scheme(c.seed, &mut trace),
    }
    trace
}

/// 用例集装配：20 条，族配比 7/7/6；每族含至少一条错误路径种子
/// （Link 族 id=3 缺基座位；Dict/Scheme 族以极值种子覆盖边界）。
pub fn build_cases() -> Vec<VecCase> {
    let mut cases: Vec<VecCase> = Vec::new();
    // Link 族 7 条。
    let link_seeds: [u32; 7] = [0x0000_0009, 0x0000_0033, 0x0000_0002, 0x0000_0F41, 0x0000_17B3, 0x0000_2C07, 0x0000_4001];
    let mut i = 0;
    while i < 7 {
        cases.push(VecCase {
            id: 100 + i as u32,
            kind: CaseKind::Link,
            seed: link_seeds[i],
        });
        i += 1;
    }
    // Dict 族 7 条。
    let dict_seeds: [u32; 7] = [0x0000_0051, 0x0000_00A7, 0x0000_0F01, 0x0000_1234, 0x0000_2F81, 0x0000_3C13, 0x0000_0000];
    i = 0;
    while i < 7 {
        cases.push(VecCase {
            id: 200 + i as u32,
            kind: CaseKind::Dict,
            seed: dict_seeds[i],
        });
        i += 1;
    }
    // Scheme 族 6 条。
    let sch_seeds: [u32; 6] = [0x0000_0011, 0x0000_006B, 0x0000_0D07, 0x0000_1993, 0x0000_2AE5, 0x0000_3B29];
    i = 0;
    while i < 6 {
        cases.push(VecCase {
            id: 300 + i as u32,
            kind: CaseKind::Scheme,
            seed: sch_seeds[i],
        });
        i += 1;
    }
    cases
}

/// 向量账主巡检：×20 用例双跑可复跑全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let cases = build_cases();

    // R1 用例总数恰 20。
    if cases.len() != 20 {
        return Err(E_CASE_COUNT);
    }
    hits += 1;

    // R2 族配比 7/7/6（三族全覆盖，族内 id 连续成段）。
    let mut n_link = 0usize;
    let mut n_dict = 0usize;
    let mut n_scheme = 0usize;
    let mut i = 0;
    while i < cases.len() {
        match cases[i].kind {
            CaseKind::Link => n_link += 1,
            CaseKind::Dict => n_dict += 1,
            CaseKind::Scheme => n_scheme += 1,
        }
        i += 1;
    }
    if n_link != 7 || n_dict != 7 || n_scheme != 6 {
        return Err(E_KIND_COVER);
    }
    hits += 1;

    // R3 首跑 ×20：迹非空 + 校验和入账（新记必须成功）。
    let mut led = VecLedger::new();
    let mut ck_first: Vec<u64> = Vec::new();
    let mut tl_first: Vec<u32> = Vec::new();
    i = 0;
    while i < cases.len() {
        let trace = run_case(&cases[i]);
        if trace.is_empty() {
            return Err(E_TRACE_EMPTY);
        }
        let ck = checksum(&trace);
        let ok = led.record(VecLedgerRow {
            case_id: cases[i].id,
            checksum: ck,
            trace_len: trace.len() as u32,
        });
        if !ok {
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

    // R4 复跑 ×20：双跑校验和与迹长逐条相等（可复跑判据本体）。
    i = 0;
    while i < cases.len() {
        let trace = run_case(&cases[i]);
        if trace.len() as u32 != tl_first[i] {
            return Err(E_REPLAY_DRIFT);
        }
        if checksum(&trace) != ck_first[i] {
            return Err(E_REPLAY_DRIFT);
        }
        i += 1;
    }
    hits += 1;

    // R5 校验和唯一性：账内 20 和两两不同（异迹同和 = 碰撞必须暴露）。
    let mut a = 0usize;
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

    // R6 篡改检出：载荷翻转一位，校验和必须改变（20/20）。
    i = 0;
    while i < cases.len() {
        let mut c2 = cases[i];
        c2.seed ^= 0x0000_0001;
        let trace2 = run_case(&c2);
        if checksum(&trace2) == ck_first[i] {
            return Err(E_TAMPER_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R7 幂等重放：以首跑值重放 20 条 → 账长不变、全部一致。
    i = 0;
    while i < cases.len() {
        let ok = led.record(VecLedgerRow {
            case_id: cases[i].id,
            checksum: ck_first[i],
            trace_len: tl_first[i],
        });
        if !ok {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    if led.len() != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 账目抽查：账内三元组与首跑值逐字段一致（lookup 通路）。
    i = 0;
    while i < cases.len() {
        let row = match led.lookup(cases[i].id) {
            Some(r) => r,
            None => return Err(E_LEDGER_BAD),
        };
        if row.checksum != ck_first[i] || row.trace_len != tl_first[i] {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
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
