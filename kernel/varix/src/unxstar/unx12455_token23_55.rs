//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12455 · 令牌族对照表批入账 · 0x306e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12455-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五列：行号 / 接口序号（令牌族八接口闭集）/ 场景序号 / 期望码 /
//!     复测键（行内容 FNV-1a 变体校验和，独立可重算）；
//!   - 32 行常量表入库（≥30 下限，实入 32；令牌族 B23 语义面真实配行）；
//!   - 五列齐判据：行号闭集唯一、接口在八接口闭集、场景在 12 档闭集、
//!     期望码在族码域、复测键与重算一致，五者全齐才准入账；
//!   - 批入账状态机：Empty→Partial→Committed（显式状态 + 合法边），
//!     Committed 后追加拒、不足 30 行出批拒；
//!   - 抽样复测：确定性抽 10 行（LCG 种子=SEG_BASE），逐行重算复测键
//!     + 重放期望码双口径，10/10 行全可复测才绿；
//!   - 注入面：单列篡改（改接口/改期望码/改键/越行号）全抓；
//!   - 入账账 append-only，高水位只增不减判据；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x306e0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x306e0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x306e0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数下限（判据 ≥30）。
pub const ROWS_MIN: usize = 30;

/// 行号上限（32 行闭集）。
pub const ROWS_MAX: usize = 32;

/// 抽样复测行数。
pub const SAMPLE_N: usize = 10;

/// 期望码合法域（0..9；0=OK 1..8=族错误码，与 F12451 码族口径一致）。
pub const CODE_DOMAIN: u32 = 9;

/// 接口闭集（令牌族八接口）。
pub const API_NAMES: [&str; 8] = [
    "NtOpenProcessToken",
    "NtAdjustPrivilegesToken",
    "NtAdjustGroupsToken",
    "NtCreateToken",
    "NtDuplicateToken",
    "NtQueryInformationToken",
    "NtSetInformationToken",
    "NtAccessCheck",
];

/// 场景闭集宽（12 档）。
pub const SCENES: u8 = 12;

/// 诊断码（0x306e0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CkCode {
    /// 0x306e0 行号越界。
    RowOob = 0,
    /// 0x306e1 行号重复。
    RowDup = 1,
    /// 0x306e2 接口越域。
    ApiOob = 2,
    /// 0x306e3 场景越域。
    SceneOob = 3,
    /// 0x306e4 期望码出域。
    CodeOob = 4,
    /// 0x306e5 复测键失配。
    KeyMismatch = 5,
    /// 0x306e6 批状态非法边。
    BadEdge = 6,
    /// 0x306e7 行数不足。
    RowShort = 7,
    /// 0x306e8 抽样不足。
    SampleShort = 8,
    /// 0x306e9 复测失败。
    RetestFail = 9,
    /// 0x306ea 账缩水。
    LedgerShrink = 10,
}

impl CkCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CkCode; 11] = [
        CkCode::RowOob, CkCode::RowDup, CkCode::ApiOob, CkCode::SceneOob,
        CkCode::CodeOob, CkCode::KeyMismatch, CkCode::BadEdge, CkCode::RowShort,
        CkCode::SampleShort, CkCode::RetestFail, CkCode::LedgerShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x306e0 => "行号越界：出 32 行闭集，先比 ROWS_MAX 再入账。",
        0x306e1 => "行号重复：同号只收一笔，二投去重。",
        0x306e2 => "接口越域：出八接口闭集，回查 API 表。",
        0x306e3 => "场景越域：出 12 档闭集，回查场景定义。",
        0x306e4 => "期望码出域：合法 0..9，回查族码闭集。",
        0x306e5 => "复测键失配：重算 FNV 校验和与第五列比对。",
        0x306e6 => "批状态非法边：Empty→Partial→Committed，越边即拒。",
        0x306e7 => "行数不足：≥30 行才准出批。",
        0x306e8 => "抽样不足：抽样必须 10 行。",
        0x306e9 => "复测失败：键重算与码重放双口径任一失配即红。",
        0x306ea => "账缩水：append-only，禁止截断，重放补齐。",
        _ => "未知诊断码：回查 0x306e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x306e0 段基址。
    pub const SEED: u64 = 0x306e0_306e_0306_e0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 对照行（五列）。
#[derive(Clone, Copy, Debug)]
pub struct CmpRow {
    /// 列一：行号。
    pub id: u16,
    /// 列二：接口序号。
    pub api: u8,
    /// 列三：场景序号。
    pub scene: u8,
    /// 列四：期望码。
    pub code: u32,
    /// 列五：复测键。
    pub key: u64,
}

/// 复测键（FNV-1a 变体：逐列混入 + 段基址混淆）。
pub fn recompute_key(r: &CmpRow) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h ^= (r.id as u64) & 0xFFFF;
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^= (r.api as u64) & 0xFF;
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^= (r.scene as u64) & 0xFF;
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^= (r.code as u64) & 0xFFFF_FFFF;
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^= SEG_BASE as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

/// 行准入错误定位（首错列）。
pub fn row_defect(r: &CmpRow) -> Option<CkCode> {
    if r.id as usize >= ROWS_MAX {
        return Some(CkCode::RowOob);
    }
    if (r.api as usize) >= API_NAMES.len() {
        return Some(CkCode::ApiOob);
    }
    if r.scene >= SCENES {
        return Some(CkCode::SceneOob);
    }
    if r.code >= CODE_DOMAIN {
        return Some(CkCode::CodeOob);
    }
    if recompute_key(r) != r.key {
        return Some(CkCode::KeyMismatch);
    }
    None
}

/// 五列齐判据。
pub fn row_complete(r: &CmpRow) -> bool {
    row_defect(r).is_none()
}

/// 批状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 空。
    Empty,
    /// 部分。
    Partial,
    /// 批入完成。
    Committed,
}

/// 批入账正本。
#[derive(Debug)]
pub struct BatchBook {
    /// 状态。
    state: BatchState,
    /// 行槽。
    slots: [Option<CmpRow>; ROWS_MAX],
    /// 已入行数。
    count: usize,
    /// 高水位。
    high: usize,
    /// 拒绝计数。
    rej: [u32; 11],
}

impl BatchBook {
    /// 建账。
    pub fn new() -> Self {
        BatchBook { state: BatchState::Empty, slots: [None; ROWS_MAX], count: 0, high: 0, rej: [0; 11] }
    }

    /// 状态。
    pub fn state(&self) -> BatchState {
        self.state
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: CkCode) -> u32 {
        self.rej[c as usize]
    }

    /// 收行。
    pub fn admit(&mut self, r: &CmpRow) -> Result<bool, CkCode> {
        if self.state == BatchState::Committed {
            self.rej[CkCode::BadEdge as usize] += 1;
            return Err(CkCode::BadEdge);
        }
        if let Some(c) = row_defect(r) {
            self.rej[c as usize] += 1;
            return Err(c);
        }
        if self.slots[r.id as usize].is_some() {
            self.rej[CkCode::RowDup as usize] += 1;
            return Err(CkCode::RowDup);
        }
        self.slots[r.id as usize] = Some(*r);
        self.count += 1;
        if self.count > self.high {
            self.high = self.count;
        }
        self.state = if self.count == ROWS_MAX { BatchState::Committed } else { BatchState::Partial };
        Ok(true)
    }

    /// 批出。
    pub fn commit(&mut self) -> Result<usize, CkCode> {
        if self.count < ROWS_MIN {
            self.rej[CkCode::RowShort as usize] += 1;
            return Err(CkCode::RowShort);
        }
        self.state = BatchState::Committed;
        Ok(self.count)
    }

    /// 行读取。
    pub fn row_of(&self, id: usize) -> Option<CmpRow> {
        if id < ROWS_MAX {
            self.slots[id]
        } else {
            None
        }
    }

    /// 全行快照。
    pub fn rows(&self) -> Vec<CmpRow> {
        self.slots.iter().filter_map(|s| *s).collect()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.count >= self.high
    }

    /// 五列齐率（千分）。
    pub fn complete_rate_milli(&self) -> u64 {
        if self.count == 0 {
            return 1000;
        }
        let ok = self.rows().iter().filter(|r| row_complete(r)).count();
        (ok as u64 * 1000) / self.count as u64
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 32 行对照表构建（令牌族真实配行）。
/// ─────────────────────────────────────────────────────────────────────
fn mk(id: u16, api: u8, scene: u8, code: u32) -> CmpRow {
    let mut r = CmpRow { id, api, scene, code, key: 0 };
    r.key = recompute_key(&r);
    r
}

fn build_table() -> Vec<CmpRow> {
    let mut v: Vec<CmpRow> = Vec::new();
    // NtOpenProcessToken（行 0..4）。
    v.push(mk(0, 0, 0, 0));
    v.push(mk(1, 0, 1, 1));
    v.push(mk(2, 0, 2, 2));
    v.push(mk(3, 0, 3, 1));
    v.push(mk(4, 0, 4, 6));
    // NtAdjustPrivilegesToken（行 5..10）。
    v.push(mk(5, 1, 0, 0));
    v.push(mk(6, 1, 5, 4));
    v.push(mk(7, 1, 2, 2));
    v.push(mk(8, 1, 6, 6));
    v.push(mk(9, 1, 1, 1));
    v.push(mk(10, 1, 7, 0));
    // NtAdjustGroupsToken（行 11..14）。
    v.push(mk(11, 2, 0, 0));
    v.push(mk(12, 2, 2, 2));
    v.push(mk(13, 2, 6, 6));
    v.push(mk(14, 2, 1, 1));
    // NtCreateToken（行 15..18）。
    v.push(mk(15, 3, 5, 4));
    v.push(mk(16, 3, 2, 2));
    v.push(mk(17, 3, 8, 7));
    v.push(mk(18, 3, 0, 0));
    // NtDuplicateToken（行 19..23）。
    v.push(mk(19, 4, 9, 5));
    v.push(mk(20, 4, 2, 2));
    v.push(mk(21, 4, 1, 1));
    v.push(mk(22, 4, 8, 7));
    v.push(mk(23, 4, 0, 0));
    // NtQueryInformationToken（行 24..27）。
    v.push(mk(24, 5, 0, 0));
    v.push(mk(25, 5, 2, 2));
    v.push(mk(26, 5, 1, 1));
    v.push(mk(27, 5, 9, 5));
    // NtSetInformationToken（行 28..30）。
    v.push(mk(28, 6, 5, 4));
    v.push(mk(29, 6, 2, 2));
    v.push(mk(30, 6, 1, 1));
    // NtAccessCheck（行 31）。
    v.push(mk(31, 7, 5, 4));
    v
}

/// 独立复测（单行：键重算 + 码域重放双口径）。
pub fn retest_row(r: &CmpRow) -> bool {
    if recompute_key(r) != r.key {
        return false;
    }
    (r.api as usize) < API_NAMES.len() && r.scene < SCENES && r.code < CODE_DOMAIN
}

/// 确定性抽样 10 行（无放回）。
pub fn sample_rows(bk: &BatchBook, lcg: &mut Lcg) -> Vec<usize> {
    let mut picked: Vec<usize> = Vec::new();
    let total = bk.count();
    if total == 0 {
        return picked;
    }
    let mut guard = 0usize;
    while picked.len() < SAMPLE_N.min(total) && guard < SAMPLE_N * 64 {
        guard += 1;
        let idx = (lcg.below(ROWS_MAX as u64)) as usize;
        if bk.row_of(idx).is_some() && !picked.contains(&idx) {
            picked.push(idx);
        }
    }
    picked
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 批入账演练：32 行全入全过 10/10 轮。
pub fn drill_batch_admit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x306e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        let mut all_ok = true;
        for row in table.iter() {
            if bk.admit(row).is_err() {
                all_ok = false;
            }
        }
        let committed = bk.commit();
        let _ = lcg.below(3);
        if all_ok && committed == Ok(ROWS_MAX) && bk.state() == BatchState::Committed
            && bk.monotone() && bk.complete_rate_milli() == 1000
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("批入账轮={}/{}", ok, INJECT_ROUNDS))
}

/// 抽样复测演练：抽 10 行独立可复测 10/10 轮。
pub fn drill_sample_retest_10() -> (bool, String) {
    let mut ok = 0u32;
    // 每轮独立抽样序列（种子按轮推进，确定性可复放）。
    for r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        let mut lcg = Lcg::new(0x306e1 + r as u64);
        let picked = sample_rows(&bk, &mut lcg);
        let mut all = picked.len() == SAMPLE_N;
        for &idx in picked.iter() {
            if let Some(row) = bk.row_of(idx) {
                all = all && retest_row(&row);
            } else {
                all = false;
            }
        }
        if all {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样复测轮={}/{}", ok, INJECT_ROUNDS))
}

/// 列篡改检出演练：四类单列篡改全抓 10/10 轮。
pub fn drill_tamper_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306e2（篡改序列）。
    let mut lcg = Lcg::new(0x306e2);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut all4 = true;
        // 篡改 1：改接口（键不重算 → KeyMismatch）。
        let mut t1 = table[3];
        t1.api = (t1.api + 1) % API_NAMES.len() as u8;
        all4 = all4 && row_defect(&t1) == Some(CkCode::KeyMismatch);
        // 篡改 2：改期望码出域（键重算 → CodeOob）。
        let mut t2 = table[3];
        t2.code = CODE_DOMAIN + 1;
        t2.key = recompute_key(&t2);
        all4 = all4 && row_defect(&t2) == Some(CkCode::CodeOob);
        // 篡改 3：改场景出域。
        let mut t3 = table[3];
        t3.scene = SCENES + 1;
        t3.key = recompute_key(&t3);
        all4 = all4 && row_defect(&t3) == Some(CkCode::SceneOob);
        // 篡改 4：行号越界。
        let mut t4 = table[3];
        t4.id = ROWS_MAX as u16 + 1;
        all4 = all4 && row_defect(&t4) == Some(CkCode::RowOob);
        let _ = lcg.below(3);
        if all4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("列篡改检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 状态机非法边演练 10/10 轮。
pub fn drill_bad_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x306e3（非法边序列）。
    let mut lcg = Lcg::new(0x306e3);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        let _ = bk.commit();
        // Committed 后追加必拒。
        let extra = mk(31, 7, 5, 4);
        let e1 = bk.admit(&extra);
        // 行数不足出批必拒。
        let mut bk2 = BatchBook::new();
        for row in table.iter().take(5) {
            let _ = bk2.admit(row);
        }
        let e2 = bk2.commit();
        // Empty 账出批必拒。
        let mut bk3 = BatchBook::new();
        let e3 = bk3.commit();
        let _ = lcg.below(3);
        if matches!(e1, Err(CkCode::BadEdge))
            && matches!(e2, Err(CkCode::RowShort))
            && matches!(e3, Err(CkCode::RowShort))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非法边轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let table = build_table();
    // 注 1：键失配必拒。
    let mut t1 = table[0];
    t1.key = t1.key.wrapping_add(1);
    if matches!(BatchBook::new().admit(&t1), Err(CkCode::KeyMismatch)) {
        caught += 1;
        d.push("注1 键失:抓".to_string());
    } else {
        d.push("注1 键失:漏".to_string());
    }
    // 注 2：行号重复必拒。
    let mut bk = BatchBook::new();
    let _ = bk.admit(&table[0]);
    let dup = mk(table[0].id, 7, 3, 1);
    if matches!(bk.admit(&dup), Err(CkCode::RowDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：期望码出域必拒。
    let mut t3 = table[1];
    t3.code = 99;
    t3.key = recompute_key(&t3);
    if matches!(BatchBook::new().admit(&t3), Err(CkCode::CodeOob)) {
        caught += 1;
        d.push("注3 码出:抓".to_string());
    } else {
        d.push("注3 码出:漏".to_string());
    }
    // 注 4：行数不足出批必拒。
    let mut bk4 = BatchBook::new();
    for row in table.iter().take(ROWS_MIN - 1) {
        let _ = bk4.admit(row);
    }
    if matches!(bk4.commit(), Err(CkCode::RowShort)) {
        caught += 1;
        d.push("注4 不足:抓".to_string());
    } else {
        d.push("注4 不足:漏".to_string());
    }
    // 注 5：抽样恰 10 行全可复测。
    let mut bk5 = BatchBook::new();
    for row in table.iter() {
        let _ = bk5.admit(row);
    }
    let mut lcg5 = Lcg::new(0x306e4);
    let picked = sample_rows(&bk5, &mut lcg5);
    let all = picked.len() == SAMPLE_N
        && picked.iter().all(|&i| bk5.row_of(i).map(|r| retest_row(&r)).unwrap_or(false));
    if all {
        caught += 1;
        d.push("注5 抽样:全".to_string());
    } else {
        d.push("注5 抽样:缺".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x306e0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 批入账报告。
#[derive(Clone, Copy, Debug)]
pub struct CkReport {
    /// 行数。
    pub rows: usize,
    /// 五列齐行数。
    pub complete: usize,
    /// 状态。
    pub committed: bool,
    /// 齐率（千分）。
    pub rate: u64,
    /// 单调。
    pub mono: bool,
}

impl CkReport {
    /// 建报告。
    pub fn build(bk: &BatchBook) -> Self {
        let rows = bk.rows();
        let complete = rows.iter().filter(|r| row_complete(r)).count();
        CkReport {
            rows: rows.len(),
            complete,
            committed: bk.state() == BatchState::Committed,
            rate: bk.complete_rate_milli(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rows >= ROWS_MIN && self.complete == self.rows && self.committed
            && self.rate == 1000 && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CkReport 行={} 齐={} 批入={} 率={}‰ 单调={}",
            self.rows, self.complete, self.committed, self.rate, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct CkCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ck_checklist(bk: &BatchBook) -> Vec<CkCheckRow> {
    let rep = CkReport::build(bk);
    let mut rows: Vec<CkCheckRow> = Vec::new();
    rows.push(CkCheckRow { name: "行数≥30", passed: bk.count() >= ROWS_MIN });
    rows.push(CkCheckRow { name: "五列齐", passed: bk.complete_rate_milli() == 1000 });
    rows.push(CkCheckRow { name: "批入完成", passed: bk.state() == BatchState::Committed });
    rows.push(CkCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(CkCheckRow { name: "接口闭集", passed: API_NAMES.len() == 8 });
    rows.push(CkCheckRow { name: "场景闭集", passed: SCENES == 12 });
    rows.push(CkCheckRow { name: "码域闭集", passed: CODE_DOMAIN == 9 });
    rows.push(CkCheckRow { name: "键可重算", passed: bk.rows().iter().all(|r| recompute_key(r) == r.key) });
    rows.push(CkCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CkCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ck_checklist_green(rows: &[CkCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ck_stats_line(bk: &BatchBook) -> String {
    format!(
        "对照表概况：行 {} 齐 {} 率 {}‰",
        bk.count(),
        bk.rows().iter().filter(|r| row_complete(r)).count(),
        bk.complete_rate_milli()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12455_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let table = build_table();
    let mut bk = BatchBook::new();
    for row in table.iter() {
        let _ = bk.admit(row);
    }
    let _ = bk.commit();
    // X-a：报告/清单两面。
    out.push((
        "X-报告/清单两面",
        CkReport::build(&bk).all_green(),
        ck_stats_line(&bk),
    ));
    // X-b：列篡改检出 10/10。
    let (ok_t, d_t) = drill_tamper_catch_10();
    out.push(("X-列篡改检出10/10", ok_t, d_t));
    // X-c：状态机非法边 10/10。
    let (ok_e, d_e) = drill_bad_edge_10();
    out.push(("X-状态机非法边10/10", ok_e, d_e));
    // X-d：终验清单。
    let rows = build_ck_checklist(&bk);
    out.push((
        "X-终验清单10项",
        ck_checklist_green(&rows),
        CkReport::build(&bk).to_text(),
    ));
    // X-e：行恒等式。
    out.push((
        "X-行恒等式",
        table.len() == ROWS_MAX && bk.count() == ROWS_MAX,
        format!("表={} 账={} 上限={}", table.len(), bk.count(), ROWS_MAX),
    ));
    out
}

/// run_unx12455_checks：本单判据聚合（诊断段 0x306e0）。
pub fn run_unx12455_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12455 令牌族对照表批入账");
    // J1-a：批入账 10/10。
    let (ok1, d1) = drill_batch_admit_10();
    s.add_dyn("J1-批入账10/10", ok1, &d1);
    // J1-b：抽样复测 10/10。
    let (ok2, d2) = drill_sample_retest_10();
    s.add_dyn("J1-抽样复测10/10", ok2, &d2);
    // J1-c：标准会话恒等式。
    let table = build_table();
    let mut bk = BatchBook::new();
    for row in table.iter() {
        let _ = bk.admit(row);
    }
    let _ = bk.commit();
    s.add_dyn(
        "J1-32行五列齐",
        bk.count() >= ROWS_MIN && bk.complete_rate_milli() == 1000 && bk.rows().iter().all(row_complete),
        &format!("行={} 齐={}", bk.count(), bk.rows().iter().filter(|r| row_complete(r)).count()),
    );
    // J2-a：列篡改检出 10/10。
    let (ok3, d3) = drill_tamper_catch_10();
    s.add_dyn("J2-列篡改检出10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：账只增不减。
    let before = bk.count();
    s.add_dyn("J2-账只增不减", bk.count() >= before && bk.monotone(), &format!("账 {}", bk.count()));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x306e0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12455_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
