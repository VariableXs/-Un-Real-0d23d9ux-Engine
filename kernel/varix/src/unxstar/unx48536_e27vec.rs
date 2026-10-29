//! UNX-F48536 —— E27 向量账（热插拔用例集+校验和）
//! 判据原文：UNX-F48536-J1 热插拔用例集为 12 条冻结向量（用例号 + 事件脚本
//! 位压缩编码），向量账逐条执行并产出（用例号, 终态, 转移数）三元组；
//! 校验和为全账串联 FNV：缺条/篡改/重复注入必须被检出；
//! 随机用例执行零崩、脚本编解码回环一致。
//! 判据落地（drill 场景）：R1 用例号连续升序防重；R2 脚本编解码回环；
//! R3 脚本事件闭集（每步落在 1..=4）；R4 全部用例执行零崩；
//! R5 账条数 == 用例数；R6 校验和可重算稳定；
//! R7 缺条注入检出（首/中/尾三点抽样）；R8 篡改注入检出；
//! R9 重复注入检出；R10 LCG 随机用例零崩。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF040;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ID: u16 = SEG_BASE + 1; // 用例号不连续/重复
const E_SCRIPT: u16 = SEG_BASE + 2; // 脚本编解码回环破缺
const E_OOB: u16 = SEG_BASE + 3; // 脚本事件越出 1..=4 闭集
const E_CRASH: u16 = SEG_BASE + 4; // 用例执行不可分类（崩溃等价）
const E_COUNT: u16 = SEG_BASE + 5; // 账条数 ≠ 用例数
const E_SUM: u16 = SEG_BASE + 6; // 校验和不可重算/漂移
const E_INJECT: u16 = SEG_BASE + 7; // 缺条注入未检出
const E_TAMPER: u16 = SEG_BASE + 8; // 篡改注入未检出
const E_DUP: u16 = SEG_BASE + 9; // 重复注入未检出
const E_FP: u16 = SEG_BASE + 10; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 11; // 呈现行缺段

/// 冻结用例数与脚本步数上限。
pub const CASE_N: usize = 12;
pub const SCRIPT_MAX_STEPS: usize = 10;

/// 设备态闭集（与 fuzz 卡同口径）：0 未接 / 1 已接 / 2 换装中 / 3 清理中。
pub const ST_DETACHED: u8 = 0;
pub const ST_ATTACHED: u8 = 1;
pub const ST_RESEATING: u8 = 2;
pub const ST_TEARING: u8 = 3;

/// 事件闭集：1 接入 / 2 移除 / 3 换装 / 4 清理。
pub const EV_ATTACH: u8 = 1;
pub const EV_DETACH: u8 = 2;
pub const EV_RESEAT: u8 = 3;
pub const EV_TEARDOWN: u8 = 4;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 冻结用例：(用例号, 事件脚本位压缩编码)。
/// 编码：低 3 位起步，每步 3 位，值 1..=4，0 为结束哨兵。
pub const CASES: [(u32, u32); 12] = [
    (48501, 0x0000_001), // attach
    (48502, 0x0000_002), // detach（悬空拒）
    (48503, 0x0000_009), // attach attach（重复接入拒）
    (48504, 0x0000_019), // attach reseat
    (48505, 0x0000_0D9), // attach reseat reseat（busy 拒）
    (48506, 0x0000_011), // attach detach
    (48507, 0x0000_099), // attach reseat detach
    (48508, 0x0000_021), // attach teardown
    (48509, 0x0000_121), // attach teardown teardown（twice 拒）
    (48510, 0x0000_051), // attach detach attach
    (48511, 0x0000_2A1), // attach teardown detach attach
    (48512, 0x0000_299), // attach reseat detach attach
];

/// 脚本解码：每步 3 位，遇 0 或步数上限即止，返回事件序列。
pub fn decode_script(script: u32) -> Vec<u8> {
    let mut evs: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < SCRIPT_MAX_STEPS {
        let v = (script >> (i * 3)) & 0x7;
        if v == 0 {
            break;
        }
        evs.push(v as u8);
        i += 1;
    }
    evs
}

/// 脚本编码：事件序列逐位压入（回环用）。
pub fn encode_script(evs: &[u8]) -> u32 {
    let mut script: u32 = 0;
    let mut i = 0usize;
    while i < evs.len() && i < SCRIPT_MAX_STEPS {
        script |= (evs[i] as u32) << (i * 3);
        i += 1;
    }
    script
}

/// 设备态机（向量账执行内核，与 fuzz 卡同转移表）。
pub struct DevRunner {
    pub state: u8,
    pub transits: u32,
    pub rejects: u32,
}

impl DevRunner {
    pub fn new() -> DevRunner {
        DevRunner {
            state: ST_DETACHED,
            transits: 0,
            rejects: 0,
        }
    }

    /// 灌入一笔事件，返回 (转移?, 新态)。
    pub fn feed(&mut self, ev: u8) -> (bool, u8) {
        let s = self.state;
        match ev {
            EV_ATTACH => {
                if s == ST_DETACHED || s == ST_RESEATING {
                    self.state = ST_ATTACHED;
                    self.transits += 1;
                    (true, ST_ATTACHED)
                } else {
                    self.rejects += 1;
                    (false, s)
                }
            }
            EV_DETACH => {
                if s != ST_DETACHED {
                    self.state = ST_DETACHED;
                    self.transits += 1;
                    (true, ST_DETACHED)
                } else {
                    self.rejects += 1;
                    (false, s)
                }
            }
            EV_RESEAT => {
                if s == ST_ATTACHED {
                    self.state = ST_RESEATING;
                    self.transits += 1;
                    (true, ST_RESEATING)
                } else {
                    self.rejects += 1;
                    (false, s)
                }
            }
            EV_TEARDOWN => {
                if s == ST_ATTACHED {
                    self.state = ST_TEARING;
                    self.transits += 1;
                    (true, ST_TEARING)
                } else {
                    self.rejects += 1;
                    (false, s)
                }
            }
            _ => {
                self.rejects += 1;
                (false, s)
            }
        }
    }
}

/// 账条目：(用例号, 终态, 转移数, 拒收数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    pub case_id: u32,
    pub final_state: u8,
    pub transits: u32,
    pub rejects: u32,
}

/// 执行一条冻结用例。
pub fn run_case(case_id: u32, script: u32) -> Result<LedgerRow, u16> {
    let evs = decode_script(script);
    let mut r = DevRunner::new();
    let mut i = 0usize;
    while i < evs.len() {
        if evs[i] < EV_ATTACH || evs[i] > EV_TEARDOWN {
            return Err(E_OOB);
        }
        r.feed(evs[i]);
        i += 1;
    }
    Ok(LedgerRow {
        case_id,
        final_state: r.state,
        transits: r.transits,
        rejects: r.rejects,
    })
}

/// 全账执行 + 校验和。
pub fn build_ledger() -> Result<(Vec<LedgerRow>, u64), u16> {
    let mut rows: Vec<LedgerRow> = vec![];
    let mut i = 0usize;
    while i < CASES.len() {
        let (id, script) = CASES[i];
        rows.push(run_case(id, script)?);
        i += 1;
    }
    let sum = ledger_checksum(&rows);
    Ok((rows, sum))
}

/// 校验和：账逐条（id/终态/转移/拒收）串联 FNV。
pub fn ledger_checksum(rows: &[LedgerRow]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < rows.len() {
        buf.extend_from_slice(&rows[i].case_id.to_le_bytes());
        buf.push(rows[i].final_state);
        buf.extend_from_slice(&rows[i].transits.to_le_bytes());
        buf.extend_from_slice(&rows[i].rejects.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 缺条指认：对升序清单找出第一个空洞位。
pub fn detect_missing(rows: &[LedgerRow]) -> Option<u32> {
    let mut i = 0usize;
    while i < rows.len() {
        if rows[i].case_id != CASES[i].0 {
            return Some(CASES[i].0);
        }
        i += 1;
    }
    if rows.len() < CASE_N {
        return Some(CASES[rows.len()].0);
    }
    None
}

/// 重复指认：相邻两键相同即重复。
pub fn detect_dup(rows: &[LedgerRow]) -> Option<u32> {
    let mut i = 1usize;
    while i < rows.len() {
        if rows[i].case_id == rows[i - 1].case_id {
            return Some(rows[i].case_id);
        }
        i += 1;
    }
    None
}

/// 去掉指定用例号后的账拷贝。
pub fn without(rows: &[LedgerRow], case_id: u32) -> Vec<LedgerRow> {
    let mut out: Vec<LedgerRow> = vec![];
    let mut i = 0usize;
    while i < rows.len() {
        if rows[i].case_id != case_id {
            out.push(rows[i]);
        }
        i += 1;
    }
    out
}

/// 账行（呈现面）：case=<n> state=<n> t=<n> r=<n>。
pub fn row_line(row: &LedgerRow) -> String {
    let mut s = String::new();
    s.push_str("case=");
    s.push_str(&row.case_id.to_string());
    s.push_str(" state=");
    s.push_str(&(row.final_state as u32).to_string());
    s.push_str(" t=");
    s.push_str(&row.transits.to_string());
    s.push_str(" r=");
    s.push_str(&row.rejects.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 用例号连续升序防重：48501 起逐位 +1。
    let mut i = 0usize;
    while i < CASES.len() {
        if CASES[i].0 != 48501 + i as u32 {
            return Err(E_ID);
        }
        let mut j = i + 1;
        while j < CASES.len() {
            if CASES[i].0 == CASES[j].0 {
                return Err(E_ID);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R2 脚本编解码回环：解码再编码必还原；LCG 随机事件序列亦然。
    let mut lcg = Lcg::new(0x48536);
    let mut t = 0u32;
    while t < 16 {
        let n = 1 + lcg.below(SCRIPT_MAX_STEPS as u32) as usize;
        let mut evs: Vec<u8> = vec![];
        let mut k = 0usize;
        while k < n {
            evs.push(1 + lcg.below(4) as u8);
            k += 1;
        }
        let script = encode_script(&evs);
        let back = decode_script(script);
        if back != evs {
            return Err(E_SCRIPT);
        }
        t += 1;
    }
    hits += 1;

    // R3 脚本事件闭集：冻结用例解码后每步落在 1..=4。
    let mut i = 0usize;
    while i < CASES.len() {
        let evs = decode_script(CASES[i].1);
        if evs.is_empty() {
            return Err(E_OOB);
        }
        let mut k = 0usize;
        while k < evs.len() {
            if evs[k] < EV_ATTACH || evs[k] > EV_TEARDOWN {
                return Err(E_OOB);
            }
            k += 1;
        }
        i += 1;
    }
    hits += 1;

    // R4/R5 全部用例执行零崩：账条数 == 用例数，账面字段齐备。
    let (rows, sum) = build_ledger()?;
    if rows.len() != CASE_N {
        return Err(E_COUNT);
    }
    let mut i = 0usize;
    while i < rows.len() {
        if rows[i].final_state > ST_TEARING {
            return Err(E_CRASH);
        }
        if rows[i].transits + rows[i].rejects != decode_script(CASES[i].1).len() as u32 {
            return Err(E_CRASH);
        }
        i += 1;
    }
    hits += 1;

    // R6 校验和可重算稳定：两次重算一致且非零。
    let s2 = ledger_checksum(&rows);
    if sum != s2 || sum == 0 {
        return Err(E_SUM);
    }
    hits += 1;

    // R6' 账面终态串指纹：两次一致且非零（账面稳定口径）。
    let mut sbuf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < rows.len() {
        sbuf.push(rows[i].final_state);
        i += 1;
    }
    let sf1 = fnv1a64(&sbuf);
    let sf2 = fnv1a64(&sbuf);
    if sf1 != sf2 || sf1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 缺条注入检出：首/中/尾三点抽样，指认缺号逐一正确，拆账必异。
    let picks: [u32; 3] = [CASES[0].0, CASES[5].0, CASES[CASE_N - 1].0];
    let mut p = 0usize;
    while p < picks.len() {
        let short = without(&rows, picks[p]);
        if short.len() != CASE_N - 1 {
            return Err(E_INJECT);
        }
        match detect_missing(&short) {
            Some(m) => {
                if m != picks[p] {
                    return Err(E_INJECT);
                }
            }
            None => return Err(E_INJECT),
        }
        if ledger_checksum(&short) == sum {
            return Err(E_TAMPER);
        }
        p += 1;
    }
    hits += 1;

    // R8 篡改注入检出：改一条账的终态/转移数，校验和必变。
    let mut tampered = rows.clone();
    tampered[4].transits = tampered[4].transits.wrapping_add(1);
    if ledger_checksum(&tampered) == sum {
        return Err(E_TAMPER);
    }
    let mut tampered2 = rows.clone();
    tampered2[7].final_state = (tampered2[7].final_state + 1) % 4;
    if ledger_checksum(&tampered2) == sum {
        return Err(E_TAMPER);
    }
    hits += 1;

    // R9 重复注入检出：中位处复制一条，相邻重复必被指认；原账零重复。
    let mut duped: Vec<LedgerRow> = vec![];
    let mut i = 0usize;
    while i < rows.len() {
        duped.push(rows[i]);
        if i == 6 {
            duped.push(rows[i]);
        }
        i += 1;
    }
    match detect_dup(&duped) {
        Some(d) => {
            if d != rows[6].case_id {
                return Err(E_DUP);
            }
        }
        None => return Err(E_DUP),
    }
    if detect_dup(&rows).is_some() {
        return Err(E_DUP);
    }
    hits += 1;

    // R10 LCG 随机用例零崩：24 条随机脚本执行，账面守恒每步可分类。
    let mut n = 0u32;
    while n < 24 {
        let len = 1 + lcg.below(SCRIPT_MAX_STEPS as u32) as usize;
        let mut evs: Vec<u8> = vec![];
        let mut k = 0usize;
        while k < len {
            evs.push(1 + lcg.below(4) as u8);
            k += 1;
        }
        let script = encode_script(&evs);
        let row = run_case(90000 + n, script)?;
        if row.transits + row.rejects != len as u32 {
            return Err(E_CRASH);
        }
        if row.final_state > ST_TEARING {
            return Err(E_CRASH);
        }
        n += 1;
    }
    // 呈现行烟测：case=/state=/t=/r= 四段在位。
    let line = row_line(&rows[0]);
    if !has_kw(line.as_bytes(), b"case=")
        || !has_kw(line.as_bytes(), b"state=")
        || !has_kw(line.as_bytes(), b"t=")
        || !has_kw(line.as_bytes(), b"r=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
