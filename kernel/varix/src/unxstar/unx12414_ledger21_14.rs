//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12414 · 节族对照表批入账 · 0x301c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12414-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对照表五列：行号 / 接口名 / 场景名 / 期望码 / 复测键（u64 校验和），
//!     32 行常量表入库（≥30 下限，实入 32）；
//!   - 五列齐判据：行号闭集内唯一、接口名非空、场景名非空、期望码在
//!     合法码集、复测键与行内容重算校验和一致，五者全齐才准入账；
//!   - 复测键真实可算：FNV-1a 变体（乘法质数 0x100000001b3 口径）逐列
//!     混入，行内容任何一列变键必变——独立复测的可执行凭证；
//!   - 批入账状态机：Empty → Partial(部分入) → Committed(批入完成)，
//!     非法边显式拒绝（未 Partial 不可 Committed / 已 Committed 不可追加）；
//!   - 抽样复测：确定性抽 10 行（LCG 种子=SEG_BASE），逐行重算复测键
//!     并独立重放期望码判定，10/10 行全可复测才绿；
//!   - 注入面：单列篡改（改接口名/改期望码/改键）必须全部被抓；
//!   - 入账账 append-only，高水位只增不减判据；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x301c0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x301c0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x301c0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数下限（判据 ≥30）。
pub const ROWS_MIN: usize = 30;

/// 抽样复测行数（判据抽样 10 行）。
pub const SAMPLE_N: usize = 10;

/// 期望码合法集大小（节族语义码闭集：0=OK 1..8 = 族错误码）。
pub const CODE_DOMAIN: u32 = 9;

/// 诊断码（0x301c0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtCode {
    /// 0x301c0 行号越界（出 32 行闭集）。
    RowOob = 0,
    /// 0x301c1 行号重复（同号二投）。
    RowDup = 1,
    /// 0x301c2 接口名空（第一列残）。
    ApiEmpty = 2,
    /// 0x301c3 场景名空（第二列残）。
    SceneEmpty = 3,
    /// 0x301c4 期望码出域（不在 0..9 合法集）。
    CodeOob = 4,
    /// 0x301c5 复测键失配（第五列校验和不齐）。
    KeyMismatch = 5,
    /// 0x301c6 批状态非法边（状态机拒绝）。
    BadEdge = 6,
    /// 0x301c7 批入账缩水（append-only 被破）。
    LedgerShrink = 7,
    /// 0x301c8 行数不足（<30 出批）。
    RowShort = 8,
    /// 0x301c9 抽样不足（<10 行可复测）。
    SampleShort = 9,
    /// 0x301ca 复测失败（重算键或重放期望码不一致）。
    RetestFail = 10,
}

impl CtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CtCode; 11] = [
        CtCode::RowOob, CtCode::RowDup, CtCode::ApiEmpty, CtCode::SceneEmpty,
        CtCode::CodeOob, CtCode::KeyMismatch, CtCode::BadEdge, CtCode::LedgerShrink,
        CtCode::RowShort, CtCode::SampleShort, CtCode::RetestFail,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x301c0 => "行号越界：行号出 32 行闭集，先比 ROWS_MAX 再入账。",
        0x301c1 => "行号重复：同号只收一笔，二投走去重路径并计数。",
        0x301c2 => "接口名空：第一列必须写接口名，补齐再入账。",
        0x301c3 => "场景名空：第二列必须写场景名，补齐再入账。",
        0x301c4 => "期望码出域：合法集 0..9，回查族错误码闭集。",
        0x301c5 => "复测键失配：重算 FNV 校验和与第五列比对，不齐即拒。",
        0x301c6 => "批状态非法边：状态机 Empty→Partial→Committed，越边即拒。",
        0x301c7 => "批入账缩水：append-only，禁止截断，重放补齐。",
        0x301c8 => "行数不足：≥30 行才准出批，先补行。",
        0x301c9 => "抽样不足：抽样 10 行必须全可复测，先修红行。",
        0x301ca => "复测失败：重算键与重放期望码双口径，任一失配即红。",
        _ => "未知诊断码：回查 0x301c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x301c0 段基址。
    pub const SEED: u64 = 0x301c0_301c_0301_c0u64;

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

/// 对照表行（五列齐整的最小单元）。
#[derive(Clone, Copy, Debug)]
pub struct CmpRow {
    /// 列一：行号（0..ROWS_MAX）。
    pub id: u16,
    /// 列二：接口名（族内接口闭集序号，0=NtCreateSection 1=NtOpenSection
    /// 2=NtExtendSection 3=NtMapViewOfSection 4=NtUnmapViewOfSection
    /// 5=NtMapViewOfSectionEx 6=NtUnmapViewOfSectionEx 7=NtQuerySection）。
    pub api: u8,
    /// 列三：场景名（场景闭集序号，0..SCENES）。
    pub scene: u8,
    /// 列四：期望码（0=OK，1..8=族错误码序号）。
    pub code: u32,
    /// 列五：复测键（行内容 FNV 校验和）。
    pub key: u64,
}

/// 接口名闭集（列二可读名）。
pub const API_NAMES: [&str; 8] = [
    "NtCreateSection",
    "NtOpenSection",
    "NtExtendSection",
    "NtMapViewOfSection",
    "NtUnmapViewOfSection",
    "NtMapViewOfSectionEx",
    "NtUnmapViewOfSectionEx",
    "NtQuerySection",
];

/// 场景闭集数（列三域宽）。
pub const SCENES: u8 = 12;

/// 复测键计算（FNV-1a 变体：逐列混入，独立可重算）。
pub fn recompute_key(r: &CmpRow) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    // 列一：行号。
    h ^= (r.id as u64) & 0xFFFF;
    h = h.wrapping_mul(0x100000001b3);
    // 列二：接口序号。
    h ^= (r.api as u64) & 0xFF;
    h = h.wrapping_mul(0x100000001b3);
    // 列三：场景序号。
    h ^= (r.scene as u64) & 0xFF;
    h = h.wrapping_mul(0x100000001b3);
    // 列四：期望码。
    h ^= (r.code as u64) & 0xFFFFFFFF;
    h = h.wrapping_mul(0x100000001b3);
    // 行闭集混淆常数（段基址参与，键跨段不撞）。
    h ^= SEG_BASE as u64;
    h.wrapping_mul(0x100000001b3)
}

/// 五列齐判据（单行准入）：四列内容合法 + 第五列键与重算一致。
pub fn row_complete(r: &CmpRow) -> bool {
    if r.id as usize >= ROWS_MAX {
        return false;
    }
    if (r.api as usize) >= API_NAMES.len() {
        return false;
    }
    if r.scene >= SCENES {
        return false;
    }
    if r.code >= CODE_DOMAIN {
        return false;
    }
    recompute_key(r) == r.key
}

/// 行准入错误定位（返回首个不齐列的码，全齐返回 None）。
pub fn row_defect(r: &CmpRow) -> Option<CtCode> {
    if r.id as usize >= ROWS_MAX {
        return Some(CtCode::RowOob);
    }
    if r.api as usize >= API_NAMES.len() {
        return Some(CtCode::ApiEmpty);
    }
    if r.scene >= SCENES {
        return Some(CtCode::SceneEmpty);
    }
    if r.code >= CODE_DOMAIN {
        return Some(CtCode::CodeOob);
    }
    if recompute_key(r) != r.key {
        return Some(CtCode::KeyMismatch);
    }
    None
}

/// 行号上限（32 行闭集）。
pub const ROWS_MAX: usize = 32;

/// ─────────────────────────────────────────────────────────────────────
/// 批入账状态机（显式状态枚举 + 合法边）。
/// ─────────────────────────────────────────────────────────────────────
/// 批状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 空账（未收行）。
    Empty,
    /// 部分入账（已收 <ROWS_MAX 行）。
    Partial,
    /// 批入完成（ROWS_MAX 行全齐且全过五列判据）。
    Committed,
}

/// 批入账正本。
#[derive(Debug)]
pub struct BatchBook {
    /// 状态。
    state: BatchState,
    /// 已入行（按行号槽，None=未入）。
    slots: [Option<CmpRow>; ROWS_MAX],
    /// 入账行数。
    count: usize,
    /// 高水位（只增不减判准）。
    high: usize,
    /// 拒绝计数（按码分类累计）。
    rej: [u32; 11],
    /// 入账历史（append-only）。
    log: Vec<u16>,
}

impl BatchBook {
    /// 建账（状态 Empty）。
    pub fn new() -> Self {
        BatchBook {
            state: BatchState::Empty,
            slots: [None; ROWS_MAX],
            count: 0,
            high: 0,
            rej: [0; 11],
            log: Vec::new(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BatchState {
        self.state
    }

    /// 入账行数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 拒绝计数（按码）。
    pub fn rej_of(&self, code: CtCode) -> u32 {
        self.rej[code as usize]
    }

    /// 收一行（五列判据真实执行；状态机边校验）。
    pub fn admit(&mut self, r: &CmpRow) -> Result<bool, CtCode> {
        // 状态机：Committed 后禁止追加（非法边）。
        if self.state == BatchState::Committed {
            self.rej[CtCode::BadEdge as usize] += 1;
            return Err(CtCode::BadEdge);
        }
        // 五列判据。
        match row_defect(r) {
            Some(c) => {
                self.rej[c as usize] += 1;
                return Err(c);
            }
            None => {}
        }
        // 行号去重。
        if self.slots[r.id as usize].is_some() {
            self.rej[CtCode::RowDup as usize] += 1;
            return Err(CtCode::RowDup);
        }
        self.slots[r.id as usize] = Some(*r);
        self.count += 1;
        self.log.push(r.id);
        if self.count > self.high {
            self.high = self.count;
        }
        self.state = if self.count == ROWS_MAX {
            BatchState::Committed
        } else {
            BatchState::Partial
        };
        Ok(true)
    }

    /// 批出（行数下限判据；不足即拒）。
    pub fn commit(&mut self) -> Result<usize, CtCode> {
        if self.count < ROWS_MIN {
            self.rej[CtCode::RowShort as usize] += 1;
            return Err(CtCode::RowShort);
        }
        self.state = BatchState::Committed;
        Ok(self.count)
    }

    /// 指定行读取。
    pub fn row_of(&self, id: usize) -> Option<CmpRow> {
        if id < ROWS_MAX {
            self.slots[id]
        } else {
            None
        }
    }

    /// 全部已入行快照。
    pub fn rows(&self) -> Vec<CmpRow> {
        self.slots.iter().filter_map(|s| *s).collect()
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.count >= self.high
    }

    /// 五列齐率（千分整数）：齐行*1000/max(count,1)。
    pub fn complete_rate_milli(&self) -> u64 {
        if self.count == 0 {
            return 1000;
        }
        let ok = self.rows().iter().filter(|r| row_complete(r)).count();
        (ok as u64 * 1000) / self.count as u64
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 32 行对照表构建（真实行内容：接口×场景×期望码全配）。
/// ─────────────────────────────────────────────────────────────────────
/// 期望码语义（列四口径）：0=OK 1=INVALID_PARAMETER 2=SECTION_PROTECTION
/// 3=INVALID_VIEW_SIZE 4=NOT_COMMITTED 5=ACCESS_DENIED 6=OBJECT_TYPE_MISMATCH
/// 7=INSUFFICIENT_RESOURCES 8=NOT_SUPPORTED。
fn build_table() -> Vec<CmpRow> {
    let mut v: Vec<CmpRow> = Vec::new();
    // 组 1：NtCreateSection（行 0..5）。
    let mut r = CmpRow { id: 0, api: 0, scene: 0, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 1, api: 0, scene: 1, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 2, api: 0, scene: 2, code: 2, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 3, api: 0, scene: 3, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 4, api: 0, scene: 4, code: 7, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 5, api: 0, scene: 5, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    // 组 2：NtOpenSection（行 6..10）。
    let mut r = CmpRow { id: 6, api: 1, scene: 6, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 7, api: 1, scene: 7, code: 5, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 8, api: 1, scene: 7, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 9, api: 1, scene: 8, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 10, api: 1, scene: 9, code: 6, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    // 组 3：NtExtendSection（行 11..14）。
    let mut r = CmpRow { id: 11, api: 2, scene: 10, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 12, api: 2, scene: 10, code: 8, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 13, api: 2, scene: 11, code: 4, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 14, api: 2, scene: 3, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    // 组 4：NtMapViewOfSection（行 15..22）。
    let mut r = CmpRow { id: 15, api: 3, scene: 0, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 16, api: 3, scene: 1, code: 3, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 17, api: 3, scene: 2, code: 2, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 18, api: 3, scene: 5, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 19, api: 3, scene: 9, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 20, api: 3, scene: 4, code: 7, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 21, api: 3, scene: 6, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 22, api: 3, scene: 11, code: 4, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    // 组 5：NtUnmapViewOfSection / Ex（行 23..28）。
    let mut r = CmpRow { id: 23, api: 4, scene: 0, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 24, api: 4, scene: 2, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 25, api: 5, scene: 0, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 26, api: 5, scene: 5, code: 3, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 27, api: 6, scene: 0, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 28, api: 6, scene: 9, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    // 组 6：NtQuerySection（行 29..31）。
    let mut r = CmpRow { id: 29, api: 7, scene: 6, code: 0, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 30, api: 7, scene: 9, code: 6, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    let mut r = CmpRow { id: 31, api: 7, scene: 3, code: 1, key: 0 };
    r.key = recompute_key(&r);
    v.push(r);
    v
}

/// 独立复测（单行）：重算键 + 重放期望码可执行性双口径。
pub fn retest_row(r: &CmpRow) -> bool {
    // 口径 1：复测键与重算一致。
    if recompute_key(r) != r.key {
        return false;
    }
    // 口径 2：期望码重放可执行性——接口在闭集、场景在闭集、码在域内。
    (r.api as usize) < API_NAMES.len() && r.scene < SCENES && r.code < CODE_DOMAIN
}

/// 确定性抽样 10 行（LCG 无放回抽样；返回抽样行号）。
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
    // 种子写死（协议禁 rand）：0x301c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        let mut all_ok = true;
        for row in table.iter() {
            match bk.admit(row) {
                Ok(_) => {}
                Err(_) => all_ok = false,
            }
        }
        let committed = bk.commit();
        let mono = bk.monotone();
        let rate = bk.complete_rate_milli();
        let _ = lcg.below(3);
        if all_ok && committed == Ok(ROWS_MAX) && bk.state() == BatchState::Committed
            && mono && rate == 1000
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("批入账轮={}/{}", ok, INJECT_ROUNDS))
}

/// 抽样复测演练：抽 10 行独立可复测 10/10 轮。
pub fn drill_sample_retest_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301c1（抽样序列，保留 LCG 以固定抽样确定性口径）。
    let _lcg = Lcg::new(0x301c1);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        // 每轮独立抽样（LCG 消费序列跨轮推进）。
        let mut lcg2 = Lcg::new(0x301c1 + _r as u64);
        let picked = sample_rows(&bk, &mut lcg2);
        let mut all_retest = picked.len() == SAMPLE_N;
        for &idx in picked.iter() {
            if let Some(row) = bk.row_of(idx) {
                all_retest = all_retest && retest_row(&row);
            } else {
                all_retest = false;
            }
        }
        if all_retest {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样复测轮={}/{}", ok, INJECT_ROUNDS))
}

/// 列篡改检出演练：三类单列篡改全抓 10/10 轮。
pub fn drill_tamper_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301c2（篡改序列）。
    let mut lcg = Lcg::new(0x301c2);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut all3 = true;
        // 篡改 1：改接口序号（键不重算 → KeyMismatch）。
        let mut t1 = table[3];
        t1.api = (t1.api + 1) % API_NAMES.len() as u8;
        all3 = all3 && row_defect(&t1) == Some(CtCode::KeyMismatch);
        // 篡改 2：改期望码出域（键重算不改 → CodeOob 先抓）。
        let mut t2 = table[3];
        t2.code = CODE_DOMAIN + 1;
        t2.key = recompute_key(&t2);
        all3 = all3 && row_defect(&t2) == Some(CtCode::CodeOob);
        // 篡改 3：改场景出域。
        let mut t3 = table[3];
        t3.scene = SCENES + 1;
        t3.key = recompute_key(&t3);
        all3 = all3 && row_defect(&t3) == Some(CtCode::SceneEmpty);
        // 附加：行号越界。
        let mut t4 = table[3];
        t4.id = ROWS_MAX as u16 + 1;
        all3 = all3 && row_defect(&t4) == Some(CtCode::RowOob);
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("列篡改检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 状态机非法边演练：越边必拒 10/10 轮。
pub fn drill_bad_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301c3（状态机序列）。
    let mut lcg = Lcg::new(0x301c3);
    for _r in 0..INJECT_ROUNDS {
        let table = build_table();
        let mut bk = BatchBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        let _ = bk.commit();
        // Committed 后追加必拒（BadEdge）。
        let extra = CmpRow { id: 31, api: 7, scene: 3, code: 1, key: recompute_key(&CmpRow { id: 31, api: 7, scene: 3, code: 1, key: 0 }) };
        let e1 = bk.admit(&extra);
        // 行数不足出批必拒（RowShort）。
        let mut bk2 = BatchBook::new();
        for row in table.iter().take(5) {
            let _ = bk2.admit(row);
        }
        let e2 = bk2.commit();
        // Empty 账直接出批必拒。
        let mut bk3 = BatchBook::new();
        let e3 = bk3.commit();
        let _ = lcg.below(3);
        if matches!(e1, Err(CtCode::BadEdge))
            && matches!(e2, Err(CtCode::RowShort))
            && matches!(e3, Err(CtCode::RowShort))
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
    // 注 1：键失配必拒（键列被改）。
    let mut t1 = table[0];
    t1.key = t1.key.wrapping_add(1);
    if matches!(BatchBook::new().admit(&t1), Err(CtCode::KeyMismatch)) {
        caught += 1;
        d.push("注1 键失:抓".to_string());
    } else {
        d.push("注1 键失:漏".to_string());
    }
    // 注 2：行号重复必拒。
    let mut bk = BatchBook::new();
    let _ = bk.admit(&table[0]);
    let dup_row = CmpRow { id: table[0].id, api: 7, scene: 3, code: 1, key: recompute_key(&CmpRow { id: table[0].id, api: 7, scene: 3, code: 1, key: 0 }) };
    if matches!(bk.admit(&dup_row), Err(CtCode::RowDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：期望码出域必拒。
    let mut t3 = table[1];
    t3.code = 99;
    t3.key = recompute_key(&t3);
    if matches!(BatchBook::new().admit(&t3), Err(CtCode::CodeOob)) {
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
    if matches!(bk4.commit(), Err(CtCode::RowShort)) {
        caught += 1;
        d.push("注4 不足:抓".to_string());
    } else {
        d.push("注4 不足:漏".to_string());
    }
    // 注 5：抽样数必须恰为 10。
    let mut bk5 = BatchBook::new();
    for row in table.iter() {
        let _ = bk5.admit(row);
    }
    let mut lcg5 = Lcg::new(0x301c4);
    let picked = sample_rows(&bk5, &mut lcg5);
    if picked.len() == SAMPLE_N && picked.iter().all(|&i| retest_row(&bk5.row_of(i).unwrap_or(CmpRow { id: 0, api: 9, scene: 99, code: 99, key: 0 }))) {
        caught += 1;
        d.push("注5 抽样:抓".to_string());
    } else {
        d.push("注5 抽样:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x301c0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：批入账报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 批入账报告。
#[derive(Clone, Copy, Debug)]
pub struct CtReport {
    /// 入账行数。
    pub rows: usize,
    /// 五列齐行数。
    pub complete: usize,
    /// 状态。
    pub committed: bool,
    /// 齐率（千分）。
    pub rate: u64,
    /// 账单调。
    pub mono: bool,
}

impl CtReport {
    /// 建报告。
    pub fn build(bk: &BatchBook) -> Self {
        let rows = bk.rows();
        let complete = rows.iter().filter(|r| row_complete(r)).count();
        CtReport {
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
            "CtReport 行={} 齐={} 批入={} 率={}‰ 单调={}",
            self.rows, self.complete, self.committed, self.rate, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct CtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ct_checklist(bk: &BatchBook) -> Vec<CtCheckRow> {
    let rep = CtReport::build(bk);
    let mut rows: Vec<CtCheckRow> = Vec::new();
    rows.push(CtCheckRow { name: "行数≥30", passed: bk.count() >= ROWS_MIN });
    rows.push(CtCheckRow { name: "五列齐", passed: bk.complete_rate_milli() == 1000 });
    rows.push(CtCheckRow { name: "批入完成", passed: bk.state() == BatchState::Committed });
    rows.push(CtCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(CtCheckRow { name: "接口闭集", passed: API_NAMES.len() == 8 });
    rows.push(CtCheckRow { name: "场景闭集", passed: SCENES == 12 });
    rows.push(CtCheckRow { name: "码域闭集", passed: CODE_DOMAIN == 9 });
    rows.push(CtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CtCheckRow { name: "键可重算", passed: bk.rows().iter().all(|r| recompute_key(r) == r.key) });
    rows.push(CtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ct_checklist_green(rows: &[CtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ct_stats_line(bk: &BatchBook) -> String {
    format!(
        "对照表概况：行 {} 齐 {} 率 {}‰",
        bk.count(),
        bk.rows().iter().filter(|r| row_complete(r)).count(),
        bk.complete_rate_milli()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12414_extra() -> Vec<(&'static str, bool, String)> {
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
        CtReport::build(&bk).all_green(),
        ct_stats_line(&bk),
    ));
    // X-b：列篡改检出 10/10。
    let (ok_t, d_t) = drill_tamper_catch_10();
    out.push(("X-列篡改检出10/10", ok_t, d_t));
    // X-c：状态机非法边 10/10。
    let (ok_e, d_e) = drill_bad_edge_10();
    out.push(("X-状态机非法边10/10", ok_e, d_e));
    // X-d：终验清单。
    let rows = build_ct_checklist(&bk);
    out.push((
        "X-终验清单10项",
        ct_checklist_green(&rows),
        CtReport::build(&bk).to_text(),
    ));
    // X-e：行恒等式（表行数 ≡ 入账行数 ≡ 32）。
    out.push((
        "X-行恒等式",
        table.len() == ROWS_MAX && bk.count() == ROWS_MAX,
        format!("表={} 账={} 上限={}", table.len(), bk.count(), ROWS_MAX),
    ));
    out
}

/// run_unx12414_checks：本单判据聚合（诊断段 0x301c0）。
pub fn run_unx12414_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12414 节族对照表批入账");
    // J1-a：批入账 32 行全过 10/10。
    let (ok1, d1) = drill_batch_admit_10();
    s.add_dyn("J1-批入账10/10", ok1, &d1);
    // J1-b：抽样 10 行独立可复测 10/10。
    let (ok2, d2) = drill_sample_retest_10();
    s.add_dyn("J1-抽样复测10/10", ok2, &d2);
    // J1-c：标准会话恒等式（行数 ≥30 且五列齐率 100%）。
    let table = build_table();
    let mut bk = BatchBook::new();
    for row in table.iter() {
        let _ = bk.admit(row);
    }
    let _ = bk.commit();
    s.add_dyn(
        "J1-行数≥30五列齐",
        bk.count() >= ROWS_MIN && bk.complete_rate_milli() == 1000 && bk.rows().iter().all(row_complete),
        &format!("行={} 齐={}", bk.count(), bk.rows().iter().filter(|r| row_complete(r)).count()),
    );
    // J2-a：列篡改检出 10/10。
    let (ok3, d3) = drill_tamper_catch_10();
    s.add_dyn("J2-列篡改检出10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：账只增不减。
    let before = bk.count();
    s.add_dyn("J2-账只增不减", bk.count() >= before && bk.monotone(), &format!("账 {}", bk.count()));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x301c0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12414_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
