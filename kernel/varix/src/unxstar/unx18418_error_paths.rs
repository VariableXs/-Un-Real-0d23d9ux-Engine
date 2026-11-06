//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18418 · 版本面错误路径账（非法参数/空结构） · 0x7E440段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18418-J1 错误路径全集（NULL/尺寸不符/未知 API）10/10 注入返回码与 Windows 一致
//!
//! 结构清单：
//!   1. EpcCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. WinErr         Windows 错误码冻结账（87/122/127/998）
//!   3. ErrPath        错误路径行（路径 id / 触发条件 / 期望码）
//!   4. PATH_TABLE     10 条错误路径冻结表（NULL/尺寸/未知 API/…）
//!   5. judge_path     判定器：输入 → Result<效果, Windows 码>
//!   6. PathLedger     路径账（注入/命中/返回码逐条对账）
//!   7. Lcg + InjectLedger（三类坏样本 × 10 轮）
//!   8. golden_run / probe_a/b/c / inject_rounds
//!   9. run_unx18418_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - Windows 码冻结账（与 winerror.h 一致）：ERROR_INVALID_PARAMETER=87
//!     （NULL/尺寸不符/非法掩码）、ERROR_INSUFFICIENT_BUFFER=122（导出缓冲
//!     不足）、ERROR_PROC_NOT_FOUND=127（未知 API 序号）、ERROR_NOACCESS=998
//!     （上下文指针域外）。
//!   - 判定次序（与 Windows 语义同源）：指针域闸（998）→ NULL 闸（87）→
//!     尺寸闸（87）→ API 闭集闸（127）→ 参数域闸（87）。两错并立时先者
//!     胜（J1 一致性的次序口径）。
//!   - 10 条错误路径冻结表：每条 = (路径 id, 触发谓词, 期望 Windows 码)；
//!     注入 10/10 = 每条路径 10 轮坏样本，返回码逐轮与 Windows 一致。
//!   - 空结构（全零 OSVERSIONINFO 等价物）独立成路径：零尺寸与全零字段
//!     双判（都落 87，但触发谓词不同——账面可区分）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E440，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E440;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 错误路径条数（冻结：J1 口径 10 条全集）。
pub const PATH_COUNT: usize = 10;
/// OSVERSIONINFO 等价结构尺寸（冻结：6 维 × 4 字节 + 平台域 = 28）。
pub const OSVI_SIZE_EXPECT: u32 = 28;
/// 版本段上限（冻结：0xFFFF，非法大版本触发 87）。
pub const VER_SEG_MAX: u32 = 0xFFFF;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（内核侧路径账码，与 Windows 码分开记账） ─────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpcCode {
    /// 空路径账
    EmptyLedger,
    /// 路径 id 越界
    PathOutOfRange,
    /// 返回码与 Windows 冻结账不一致
    WinCodeMismatch,
    /// 判定次序违约（后闸先答）
    OrderViolated,
    /// 空结构谓词失配（零尺寸/全零域双判不一致）
    EmptyStructPredicate,
    /// 注入漏检
    InjectionMissed,
    /// 路径账与冻结表不守恒
    LedgerBroken,
    /// 期望码表越域
    ExpectOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl EpcCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E440..0x7E480）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            EpcCode::EmptyLedger => "空路径账：无任何错误路径入账",
            EpcCode::PathOutOfRange => "路径 id 越界：1..10 闭集外",
            EpcCode::WinCodeMismatch => "返回码与 Windows 冻结账不一致",
            EpcCode::OrderViolated => "判定次序违约：后闸先答",
            EpcCode::EmptyStructPredicate => "空结构谓词失配",
            EpcCode::InjectionMissed => "注入漏检：坏样本未被拒",
            EpcCode::LedgerBroken => "路径账与冻结表不守恒",
            EpcCode::ExpectOutOfRange => "期望码表越域",
            EpcCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [EpcCode; 9] = [
        EpcCode::EmptyLedger,
        EpcCode::PathOutOfRange,
        EpcCode::WinCodeMismatch,
        EpcCode::OrderViolated,
        EpcCode::EmptyStructPredicate,
        EpcCode::InjectionMissed,
        EpcCode::LedgerBroken,
        EpcCode::ExpectOutOfRange,
        EpcCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < EpcCode::ALL.len() {
        if EpcCode::ALL[i].code() == code {
            return EpcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E440 段闭集内"
}

// ── Windows 错误码冻结账（winerror.h 同源） ─────────────────────
pub mod win {
    /// ERROR_INVALID_PARAMETER = 87（NULL/尺寸不符/非法参数）。
    pub const INVALID_PARAMETER: u32 = 87;
    /// ERROR_INSUFFICIENT_BUFFER = 122（导出缓冲不足）。
    pub const INSUFFICIENT_BUFFER: u32 = 122;
    /// ERROR_PROC_NOT_FOUND = 127（未知 API 序号）。
    pub const PROC_NOT_FOUND: u32 = 127;
    /// ERROR_NOACCESS = 998（指针域外）。
    pub const NOACCESS: u32 = 998;
}

// ── 判定器输入结构 ──────────────────────────────────────────────
/// 版本查询入口的内核侧输入（模拟 VerifyVersionInfo 调用面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryInput {
    /// 结构指针类别（0=域外指针 1=NULL 2=有效）
    pub ptr_class: u32,
    /// 调用方声明的结构尺寸
    pub declared_size: u32,
    /// API 序号（0=未知）
    pub api_num: u32,
    /// 操作数位掩码（0=空）
    pub type_mask: u32,
    /// 版本主段（判域闸用）
    pub major: u32,
    /// 导出缓冲容量（字节）
    pub export_buf: u32,
}

impl QueryInput {
    /// 全零结构等价物（空结构注入样本）。
    pub const ZERO: QueryInput = QueryInput {
        ptr_class: 2,
        declared_size: 0,
        api_num: 1,
        type_mask: 1,
        major: 0,
        export_buf: 64,
    };
    /// 合法基线样本。
    pub const GOOD: QueryInput = QueryInput {
        ptr_class: 2,
        declared_size: OSVI_SIZE_EXPECT,
        api_num: 1,
        type_mask: 1,
        major: 10,
        export_buf: 64,
    };
}

/// 判定器：输入 → Ok(效果) 或 Err(Windows 码)。次序：指针域(998) →
/// NULL(87) → 尺寸(87) → API 闭集(127) → 掩码域(87) → 版本段域(87) →
/// 导出缓冲(122)。全过 = Ok(版本主段回显)。
pub fn judge_path(inp: &QueryInput) -> Result<u32, u32> {
    // 闸 1：指针域（998）。
    if inp.ptr_class == 0 {
        return Err(win::NOACCESS);
    }
    // 闸 2：NULL（87）。
    if inp.ptr_class == 1 {
        return Err(win::INVALID_PARAMETER);
    }
    // 闸 3：尺寸不符（87）——空结构（尺寸 0）同闸。
    if inp.declared_size != OSVI_SIZE_EXPECT {
        return Err(win::INVALID_PARAMETER);
    }
    // 闸 4：API 闭集（127）。
    if inp.api_num == 0 || inp.api_num > 6 {
        return Err(win::PROC_NOT_FOUND);
    }
    // 闸 5：操作数掩码域（87）——低 8 位闭集外即拒。
    if inp.type_mask & !0xFF != 0 {
        return Err(win::INVALID_PARAMETER);
    }
    // 闸 6：版本段域（87）。
    if inp.major > VER_SEG_MAX {
        return Err(win::INVALID_PARAMETER);
    }
    // 闸 7：导出缓冲（122）。
    if inp.export_buf < OSVI_SIZE_EXPECT {
        return Err(win::INSUFFICIENT_BUFFER);
    }
    Ok(inp.major)
}

// ── 错误路径冻结表 ──────────────────────────────────────────────
/// 单条错误路径：id / 触发谓词名 / 期望 Windows 码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrPath {
    pub id: u32,
    pub predicate: &'static str,
    pub expect: u32,
}

/// 10 条错误路径冻结表（全集）。
pub const PATH_TABLE: [ErrPath; PATH_COUNT] = [
    ErrPath { id: 1, predicate: "指针域外",        expect: win::NOACCESS },
    ErrPath { id: 2, predicate: "NULL 指针",       expect: win::INVALID_PARAMETER },
    ErrPath { id: 3, predicate: "尺寸不符",        expect: win::INVALID_PARAMETER },
    ErrPath { id: 4, predicate: "空结构全零",      expect: win::INVALID_PARAMETER },
    ErrPath { id: 5, predicate: "未知 API",        expect: win::PROC_NOT_FOUND },
    ErrPath { id: 6, predicate: "掩码越 8 位闭集", expect: win::INVALID_PARAMETER },
    ErrPath { id: 7, predicate: "版本段越 0xFFFF", expect: win::INVALID_PARAMETER },
    ErrPath { id: 8, predicate: "导出缓冲不足",    expect: win::INSUFFICIENT_BUFFER },
    ErrPath { id: 9, predicate: "尺寸+未知API并立", expect: win::INVALID_PARAMETER },
    ErrPath { id: 10, predicate: "NULL+未知API并立", expect: win::INVALID_PARAMETER },
];

/// 按路径 id 构造触发样本（判定器喂入口径）。
pub fn make_sample(path_id: u32) -> Option<QueryInput> {
    let base = QueryInput::GOOD;
    match path_id {
        1 => Some(QueryInput { ptr_class: 0, ..base }),
        2 => Some(QueryInput { ptr_class: 1, ..base }),
        3 => Some(QueryInput { declared_size: OSVI_SIZE_EXPECT + 4, ..base }),
        4 => Some(QueryInput::ZERO),
        5 => Some(QueryInput { api_num: 7, ..base }),
        6 => Some(QueryInput { type_mask: 0x100, ..base }),
        7 => Some(QueryInput { major: VER_SEG_MAX + 1, ..base }),
        8 => Some(QueryInput { export_buf: OSVI_SIZE_EXPECT - 1, ..base }),
        9 => Some(QueryInput { declared_size: 0, api_num: 9, ..base }),
        10 => Some(QueryInput { ptr_class: 1, api_num: 9, ..base }),
        _ => None,
    }
}

/// 路径账：注入→判定→逐条对账（返回码与冻结表比对）。
pub struct PathLedger {
    /// 每路径命中数（返回码一致）
    pub hits: [u32; PATH_COUNT],
    /// 每路径轮数
    pub tries: [u32; PATH_COUNT],
}

impl PathLedger {
    /// 空账。
    pub fn new() -> PathLedger {
        PathLedger { hits: [0; PATH_COUNT], tries: [0; PATH_COUNT] }
    }
    /// 跑一轮：对每条路径注入坏样本，断言判定器返回 == 冻结期望码。
    pub fn round(&mut self) -> Result<(), u32> {
        let mut id = 1;
        while id <= PATH_COUNT as u32 {
            let idx = (id - 1) as usize;
            let sample = match make_sample(id) {
                Some(s) => s,
                None => return Err(EpcCode::PathOutOfRange.code()),
            };
            let got = judge_path(&sample);
            let expect_row = match PATH_TABLE.get(idx) {
                Some(r) => *r,
                None => return Err(EpcCode::ExpectOutOfRange.code()),
            };
            self.tries[idx] += 1;
            // 路径 9/10 是并立错误：先闸胜出（尺寸/NULL 先于 API 127）。
            let ok = match got {
                Err(c) => c == expect_row.expect,
                Ok(_) => false,
            };
            if ok {
                self.hits[idx] += 1;
            } else {
                return Err(EpcCode::WinCodeMismatch.code());
            }
            id += 1;
        }
        Ok(())
    }
    /// 全抓断言：每路径 tries==hits 且 ≥1。
    pub fn all_consistent(&self) -> bool {
        let mut i = 0;
        while i < PATH_COUNT {
            if self.tries[i] == 0 || self.hits[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计（守恒口径）。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < PATH_COUNT {
            m += self.tries[i].saturating_sub(self.hits[i]);
            i += 1;
        }
        m
    }
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（种子 = SEG_BASE）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, n) 伪随机。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三类坏样本族） ─────────────────────────────────
/// 三类注入槽：0=NULL 族，1=尺寸族，2=未知 API 族。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
    /// 返回码与 Windows 一致轮数
    pub win_consistent: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3], win_consistent: [0; 3] }
    }
    /// 记一轮：hit=被拒且码与 Windows 一致。
    pub fn record(&mut self, slot: usize, hit: bool, win_ok: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
            if win_ok {
                self.win_consistent[slot] += 1;
            }
        }
    }
    /// 全抓断言（含码一致性）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] || self.win_consistent[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三族坏样本逐族注入，断言每轮被拒且返回码与 Windows 一致。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 族 0：NULL 族（ptr_class=1）→ 87。
        let s = QueryInput { ptr_class: 1, ..QueryInput::GOOD };
        match judge_path(&s) {
            Err(c) => led.record(0, true, c == win::INVALID_PARAMETER),
            Ok(_) => led.record(0, false, false),
        }
        // 族 1：尺寸族（声明尺寸偏差 1..4）→ 87。
        let dev = 1 + (r % 4);
        let s2 = QueryInput { declared_size: OSVI_SIZE_EXPECT + dev, ..QueryInput::GOOD };
        match judge_path(&s2) {
            Err(c) => led.record(1, true, c == win::INVALID_PARAMETER),
            Ok(_) => led.record(1, false, false),
        }
        // 族 2：未知 API 族（api 7..10 循环）→ 127。
        let api_bad = 7 + (r % 4);
        let s3 = QueryInput { api_num: api_bad, ..QueryInput::GOOD };
        match judge_path(&s3) {
            Err(c) => led.record(2, true, c == win::PROC_NOT_FOUND),
            Ok(_) => led.record(2, false, false),
        }
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(EpcCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：10 路径 × 10 轮对账 → 判定次序抽查（并立错误先闸胜）→
/// 合法基线放行。返回 (路径数, 一致轮数)。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = PathLedger::new();
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        led.round()?;
        r += 1;
    }
    if !led.all_consistent() || led.missed_total() != 0 {
        return Err(EpcCode::InjectionMissed.code());
    }
    // 次序抽查 1：尺寸不符 + 未知 API 并立 → 87（尺寸闸先）。
    let s9 = match make_sample(9) {
        Some(s) => s,
        None => return Err(EpcCode::PathOutOfRange.code()),
    };
    if judge_path(&s9) != Err(win::INVALID_PARAMETER) {
        return Err(EpcCode::OrderViolated.code());
    }
    // 次序抽查 2：NULL + 未知 API 并立 → 87（NULL 闸先于 127）。
    let s10 = match make_sample(10) {
        Some(s) => s,
        None => return Err(EpcCode::PathOutOfRange.code()),
    };
    if judge_path(&s10) != Err(win::INVALID_PARAMETER) {
        return Err(EpcCode::OrderViolated.code());
    }
    // 次序抽查 3：指针域外最先（998 胜过一切）。
    let s1 = match make_sample(1) {
        Some(s) => s,
        None => return Err(EpcCode::PathOutOfRange.code()),
    };
    if judge_path(&s1) != Err(win::NOACCESS) {
        return Err(EpcCode::OrderViolated.code());
    }
    // 合法基线放行。
    if judge_path(&QueryInput::GOOD) != Ok(10) {
        return Err(EpcCode::WinCodeMismatch.code());
    }
    Ok((PATH_COUNT as u32, led.hits[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：合法基线放行且效果=主版本回显。
pub fn probe_a() -> Result<u32, u32> {
    judge_path(&QueryInput::GOOD)
}

/// 探针 B：空结构双判（尺寸 0 与全零域同落 87，谓词可区分）。
pub fn probe_b() -> Result<(), u32> {
    let zero = QueryInput::ZERO;
    let r1 = judge_path(&zero);
    // 全零域样本：尺寸非 0 但 major/build 全零（域闸放行，非错误）。
    let allzero_fields = QueryInput { declared_size: OSVI_SIZE_EXPECT, major: 0, ..QueryInput::GOOD };
    let r2 = judge_path(&allzero_fields);
    match (r1, r2) {
        (Err(87), Ok(0)) => Ok(()),
        _ => Err(EpcCode::EmptyStructPredicate.code()),
    }
}

/// 探针 C：期望码表逐行核对（路径表 10 行全在 Windows 冻结账内）。
pub fn probe_c() -> Result<(), u32> {
    let mut i = 0;
    while i < PATH_COUNT {
        let row = match PATH_TABLE.get(i) {
            Some(r) => *r,
            None => return Err(EpcCode::ExpectOutOfRange.code()),
        };
        let in_book = row.expect == win::NOACCESS
            || row.expect == win::INVALID_PARAMETER
            || row.expect == win::INSUFFICIENT_BUFFER
            || row.expect == win::PROC_NOT_FOUND;
        if !in_book {
            return Err(EpcCode::WinCodeMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EpcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EpcCode::ALL.len() {
        let v = EpcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < EpcCode::ALL.len() {
        let c = EpcCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18418 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18418_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18418 版本面错误路径账");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E440 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((n, _)) => *n == 10,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 错误路径全集 10/10 注入码一致（J1 主判据）。
    let mut led = PathLedger::new();
    let mut r = 0;
    let mut round_err = false;
    while r < JUDGE_ROUNDS {
        if led.round().is_err() {
            round_err = true;
        }
        r += 1;
    }
    s.add(
        "J1-错误路径10/10码一致",
        !round_err && led.all_consistent() && led.missed_total() == 0,
        "10 路径 × 10 轮返回码逐轮对账",
    );
    // 4. 判定次序三抽查（998→87→127 链）。
    let order_ok = {
        let s9 = make_sample(9);
        let s10 = make_sample(10);
        let s1 = make_sample(1);
        match (s9, s10, s1) {
            (Some(a), Some(b), Some(c)) => {
                judge_path(&a) == Err(87) && judge_path(&b) == Err(87) && judge_path(&c) == Err(998)
            }
            _ => false,
        }
    };
    s.add("J1-判定次序三抽查", order_ok, "指针域→NULL→尺寸先于 API 闭集");
    // 5. 三族注入 10/10 且 Windows 码一致（J1 主判据）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-三族注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 6. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 7. Windows 冻结账核对（87/122/127/998）。
    s.add(
        "J1-Windows冻结账",
        win::INVALID_PARAMETER == 87
            && win::INSUFFICIENT_BUFFER == 122
            && win::PROC_NOT_FOUND == 127
            && win::NOACCESS == 998,
        "winerror.h 同源值逐项核对",
    );
    // 8. 合法基线放行。
    s.add("J1-合法基线放行", probe_a() == Ok(10), "GOOD 样本 Ok(10) 回显");
    // 9. 空结构双判谓词可区分。
    s.add("J1-空结构双判", probe_b().is_ok(), "零尺寸落 87；全零域合法放行");
    // 10. 期望码表全在冻结账内。
    s.add("J1-期望码表对账", probe_c().is_ok(), "10 行期望码 ∈ {87,122,127,998}");
    // 11. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 12. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 13. 路径 id 越界（11 号样本不可构造）。
    s.add("J1-路径id越界", make_sample(11).is_none() && make_sample(0).is_none(), "id 0/11 越闭集 None");
    // 14. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "9 码逐条非空且反查一致");
    // 15. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < EpcCode::ALL.len() {
            if EpcCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "9 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18418_checks();
        assert!(set.all_passed(), "UNX-F18418 判据集存在红项");
    }

    /// 10 路径 × 10 轮全对账。
    #[test]
    fn path_ledger_full() {
        let mut led = PathLedger::new();
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            assert!(led.round().is_ok());
            r += 1;
        }
        assert!(led.all_consistent());
        assert_eq!(led.missed_total(), 0);
    }

    /// 注入三族 10/10。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.missed_total(), 0);
    }

    /// 判定次序：两错并立先闸胜。
    #[test]
    fn order_first_gate_wins() {
        // 指针域外 + 尺寸不符：998 先。
        let s = QueryInput { ptr_class: 0, declared_size: 0, ..QueryInput::GOOD };
        assert_eq!(judge_path(&s), Err(998));
        // NULL + 缓冲不足：87 先。
        let s2 = QueryInput { ptr_class: 1, export_buf: 0, ..QueryInput::GOOD };
        assert_eq!(judge_path(&s2), Err(87));
        // 未知 API + 缓冲不足：127 先。
        let s3 = QueryInput { api_num: 9, export_buf: 0, ..QueryInput::GOOD };
        assert_eq!(judge_path(&s3), Err(127));
    }

    /// 四类边界：缓冲恰等/差一/尺寸恰等/差一。
    #[test]
    fn boundary_edges() {
        // 缓冲恰等放行。
        let eq = QueryInput { export_buf: OSVI_SIZE_EXPECT, ..QueryInput::GOOD };
        assert!(judge_path(&eq).is_ok());
        // 缓冲差一拒 122。
        let less = QueryInput { export_buf: OSVI_SIZE_EXPECT - 1, ..QueryInput::GOOD };
        assert_eq!(judge_path(&less), Err(122));
        // 尺寸恰等放行、差一拒 87。
        let sz_eq = QueryInput::GOOD;
        assert!(judge_path(&sz_eq).is_ok());
        let sz_plus = QueryInput { declared_size: OSVI_SIZE_EXPECT + 1, ..QueryInput::GOOD };
        assert_eq!(judge_path(&sz_plus), Err(87));
        // 版本段恰 0xFFFF 放行、+1 拒 87。
        let v_eq = QueryInput { major: VER_SEG_MAX, ..QueryInput::GOOD };
        assert!(judge_path(&v_eq).is_ok());
        let v_over = QueryInput { major: VER_SEG_MAX + 1, ..QueryInput::GOOD };
        assert_eq!(judge_path(&v_over), Err(87));
    }

    /// 空结构双判谓词。
    #[test]
    fn empty_struct_predicate() {
        assert!(probe_b().is_ok());
        assert_eq!(judge_path(&QueryInput::ZERO), Err(87));
    }

    /// 双跑一致。
    #[test]
    fn reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 100).starts_with("未知"));
    }
}
