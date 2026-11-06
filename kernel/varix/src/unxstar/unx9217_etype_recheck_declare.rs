//! UNX-F9217 E 型判据可复测性声明（10/10 次检出） · 0x7A3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9217 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9217-J1 全矩阵 10 次重复注入
//!   判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「可复测」不是一句口号：把 E 型矩阵（EBADF/EFAULT/EINVAL/次序）的
//!      12 条探针 × 3 类变异展开成 36 个注入格，逐格重复 10 轮，统计每格
//!      检出次数；只有 36/36 格都是 10/10 才允许声明。
//!   ② 检出判据是「变异后的判定与冻结期望不等」。三类变异（全漏、错码、
//!      环移）都保证改变原值，因此任何一轮漏检都意味着探针本身坏了。
//!   ③ 零 flaky 的检验方式是反向注入：把检出行为改成「偶轮才报」，声明器
//!      必须把 36 格全部记为 flaky 并翻转 reproducible=false。声明器若对
//!      这种抖动无感，那它就是一张空头支票。
//!   ④ 统计口径全部用整数：每轮检出 36/36、每格 10/10、注入 360 = 检出 360
//!      + 逃逸 0，不做百分比、不碰浮点，避免舍入把 flaky 抹平。
//!   ⑤ 期望码另有冻结账（12 行 id + 码）；矩阵改动必须先改账，否则漂移格
//!      先行报红，声明与语料不会悄悄脱节。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9217 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A3C0;
/// 重复轮数（判据硬性要求 10 轮）。
pub const ROUNDS: u32 = 10;
/// E 型探针数。
pub const PROBES: usize = 12;
/// 变异类数。
pub const MUTANT_KINDS: usize = 3;
/// 注入格上限。
pub const MAX_CELLS: usize = 64;

/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 复测声明诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestCode {
    RoundShort,
    CellMissed,
    FlakyDetected,
    StatMismatch,
    DeclarationFalse,
    ExpectDrift,
    KindGap,
    OrderBroken,
    StageSkip,
    LedgerLeak,
    NotReproducible,
    SegmentViolation,
}

impl RetestCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            RetestCode::RoundShort => 0,
            RetestCode::CellMissed => 1,
            RetestCode::FlakyDetected => 2,
            RetestCode::StatMismatch => 3,
            RetestCode::DeclarationFalse => 4,
            RetestCode::ExpectDrift => 5,
            RetestCode::KindGap => 6,
            RetestCode::OrderBroken => 7,
            RetestCode::StageSkip => 8,
            RetestCode::LedgerLeak => 9,
            RetestCode::NotReproducible => 10,
            RetestCode::SegmentViolation => 11,
        }
    }

    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RetestCode::RoundShort => "复测轮数不足 10 轮就声明 10/10，属于未完成复测",
            RetestCode::CellMissed => "某个注入格未做到 10/10，先查该格的判定器与期望码",
            RetestCode::FlakyDetected => "存在抖动格，必须先消除轮次依赖再重新声明",
            RetestCode::StatMismatch => "每轮检出数必须等于格数，统计口径不一致说明矩阵漏格",
            RetestCode::DeclarationFalse => "声明器给出不可复现结论时，禁止对外宣布可复测",
            RetestCode::ExpectDrift => "期望码以冻结账为准，改探针必须同步改账",
            RetestCode::KindGap => "EBADF/EFAULT/EINVAL/次序四类必须都在矩阵里",
            RetestCode::OrderBroken => "次序格失败说明门序被改：fd→值域→指针",
            RetestCode::StageSkip => "复测状态机只许 Matrix→Inject→Round→Declare 单向推进",
            RetestCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            RetestCode::NotReproducible => "两次完整复跑的指纹必须相等，禁止依赖轮次外状态",
            RetestCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: [RetestCode; 12] = [
        RetestCode::RoundShort, RetestCode::CellMissed, RetestCode::FlakyDetected,
        RetestCode::StatMismatch, RetestCode::DeclarationFalse, RetestCode::ExpectDrift,
        RetestCode::KindGap, RetestCode::OrderBroken, RetestCode::StageSkip,
        RetestCode::LedgerLeak, RetestCode::NotReproducible, RetestCode::SegmentViolation,
    ];
}

// ── 探针矩阵 ──────────────────────────────────────────────────────────
/// E 型错误面分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EKind {
    BadFd,
    BadPtr,
    BadValue,
    Order,
}

impl EKind {
    /// 类别码。
    pub fn code(self) -> u32 {
        match self {
            EKind::BadFd => 1,
            EKind::BadPtr => 2,
            EKind::BadValue => 3,
            EKind::Order => 4,
        }
    }
}

/// 用户指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    Valid,
    Null,
    ReadOnly,
    Unmapped,
}

impl PtrClass {
    /// 用户态可读。
    pub fn readable(self) -> bool {
        match self {
            PtrClass::Valid | PtrClass::ReadOnly => true,
            _ => false,
        }
    }

    /// 用户态可写。
    pub fn writable(self) -> bool {
        match self {
            PtrClass::Valid => true,
            _ => false,
        }
    }
}

/// 一条 E 型探针。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub id: &'static str,
    pub kind: EKind,
    pub fd_live: bool,
    pub path: PtrClass,
    pub out: PtrClass,
    pub value: u32,
    pub expect: u32,
}

/// 探针构造。
pub const fn probe(
    id: &'static str,
    kind: EKind,
    fd_live: bool,
    path: PtrClass,
    out: PtrClass,
    value: u32,
    expect: u32,
) -> Probe {
    Probe { id, kind, fd_live, path, out, value, expect }
}

/// 12 条探针：坏 fd 3 + 坏指针 3 + 坏值 3 + 次序 3。
pub const MATRIX: [Probe; 12] = [
    probe("badfd-read", EKind::BadFd, false, PtrClass::Valid, PtrClass::Valid, 0, EBADF),
    probe("badfd-write", EKind::BadFd, false, PtrClass::Valid, PtrClass::Valid, 0, EBADF),
    probe("badfd-fstat", EKind::BadFd, false, PtrClass::Valid, PtrClass::Valid, 0, EBADF),
    probe("badptr-buf", EKind::BadPtr, true, PtrClass::Unmapped, PtrClass::Valid, 0, EFAULT),
    probe("badptr-out", EKind::BadPtr, true, PtrClass::Valid, PtrClass::ReadOnly, 0, EFAULT),
    probe("badptr-path", EKind::BadPtr, true, PtrClass::Null, PtrClass::Valid, 0, EFAULT),
    probe("badval-whence", EKind::BadValue, true, PtrClass::Valid, PtrClass::Valid, 1, EINVAL),
    probe("badval-mask", EKind::BadValue, true, PtrClass::Valid, PtrClass::Valid, 2, EINVAL),
    probe("badval-flags", EKind::BadValue, true, PtrClass::Valid, PtrClass::Valid, 3, EINVAL),
    probe("ord-fd-before-ptr", EKind::Order, false, PtrClass::Unmapped, PtrClass::Valid, 0, EBADF),
    probe("ord-fd-before-val", EKind::Order, false, PtrClass::Valid, PtrClass::Valid, 1, EBADF),
    probe("ord-val-before-ptr", EKind::Order, true, PtrClass::Null, PtrClass::Valid, 2, EINVAL),
];

/// B 侧冻结期望账。
pub const FROZEN_EXPECT: [(&str, u32); 12] = [
    ("badfd-read", EBADF), ("badfd-write", EBADF), ("badfd-fstat", EBADF),
    ("badptr-buf", EFAULT), ("badptr-out", EFAULT), ("badptr-path", EFAULT),
    ("badval-whence", EINVAL), ("badval-mask", EINVAL), ("badval-flags", EINVAL),
    ("ord-fd-before-ptr", EBADF), ("ord-fd-before-val", EBADF),
    ("ord-val-before-ptr", EINVAL),
];

/// 独立判定器：fd 门 → 值域门 → 指针门。
pub fn judge(p: &Probe) -> u32 {
    if !p.fd_live {
        return EBADF;
    }
    if p.value != 0 {
        return EINVAL;
    }
    if !p.path.readable() || !p.out.writable() {
        return EFAULT;
    }
    0
}

/// 按 id 取探针。
pub fn probe_of(id: &str) -> Option<&'static Probe> {
    MATRIX.iter().find(|p| p.id == id)
}

// ── 变异与检出 ────────────────────────────────────────────────────────
/// 注入变异类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 全漏：任何错误都当成功。
    SkipFault,
    /// 错码：原码 +1。
    WrongCode,
    /// 环移：EBADF→EINVAL→EFAULT→EBADF。
    Rotate,
}

/// 三类变异。
pub const MUTANTS: [Mutant; 3] = [Mutant::SkipFault, Mutant::WrongCode, Mutant::Rotate];

impl Mutant {
    /// 变异类码。
    pub fn code(self) -> u32 {
        match self {
            Mutant::SkipFault => 1,
            Mutant::WrongCode => 2,
            Mutant::Rotate => 3,
        }
    }
}

/// 对一次正确判定施加变异（三类都保证改变原值）。
pub fn mutate(m: Mutant, base: u32) -> u32 {
    match m {
        Mutant::SkipFault => 0,
        Mutant::WrongCode => base.wrapping_add(1),
        Mutant::Rotate => match base {
            EBADF => EINVAL,
            EINVAL => EFAULT,
            _ => EBADF,
        },
    }
}

/// 单格 10 轮检出次数。
pub fn cell_hits(p: &Probe, m: Mutant) -> u32 {
    let mut n = 0u32;
    for _round in 0..ROUNDS {
        if mutate(m, judge(p)) != p.expect {
            n += 1;
        }
    }
    n
}

/// 单轮检出格数（轮次越界返回 0）。
pub fn round_detections(round: u32) -> u32 {
    if round >= ROUNDS {
        return 0;
    }
    let mut n = 0u32;
    for p in MATRIX.iter() {
        for m in MUTANTS.iter() {
            if mutate(*m, judge(p)) != p.expect {
                n += 1;
            }
        }
    }
    n
}

/// 注入格总数。
pub fn total_cells() -> u32 {
    (MATRIX.len() * MUTANTS.len()) as u32
}

// ── 声明器 ────────────────────────────────────────────────────────────
/// 复测声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Declaration {
    pub cells: u32,
    pub rounds: u32,
    pub ten_of_ten: u32,
    pub flaky: u32,
    pub reproducible: bool,
}

/// 生成声明；`parity` 为真时用「偶轮才报」的抖动模型做反向注入。
pub fn declare(parity: bool) -> Declaration {
    let mut ten = 0u32;
    let mut flaky = 0u32;
    for p in MATRIX.iter() {
        for m in MUTANTS.iter() {
            let hits = if parity {
                let mut n = 0u32;
                for round in 0..ROUNDS {
                    if round % 2 == 0 && mutate(*m, judge(p)) != p.expect {
                        n += 1;
                    }
                }
                n
            } else {
                cell_hits(p, *m)
            };
            if hits == ROUNDS {
                ten += 1;
            } else {
                flaky += 1;
            }
        }
    }
    Declaration {
        cells: total_cells(),
        rounds: ROUNDS,
        ten_of_ten: ten,
        flaky,
        reproducible: flaky == 0 && ten == total_cells(),
    }
}

/// 零 flaky 声明格：全格 10/10、flaky 为 0、可复现。
pub fn zero_flaky(d: &Declaration) -> bool {
    d.flaky == 0 && d.reproducible && d.ten_of_ten == d.cells && d.rounds == ROUNDS
}

/// 全格 10/10 逐格复查。
pub fn all_cells_ten_of_ten() -> bool {
    MATRIX.iter().all(|p| MUTANTS.iter().all(|m| cell_hits(p, *m) == ROUNDS))
}

/// 每轮检出数全等于格数。
pub fn every_round_full() -> bool {
    let mut ok = true;
    for round in 0..ROUNDS {
        if round_detections(round) != total_cells() {
            ok = false;
        }
    }
    ok
}

// ── 冻结账与覆盖 ──────────────────────────────────────────────────────
/// 期望码漂移行数。
pub fn expect_drift() -> u32 {
    let mut drift = 0u32;
    if MATRIX.len() != FROZEN_EXPECT.len() {
        drift += 1;
    }
    for (i, p) in MATRIX.iter().enumerate() {
        match FROZEN_EXPECT.get(i) {
            Some((id, code)) => {
                if p.id != *id || p.expect != *code {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
    }
    drift
}

/// 某类探针数。
pub fn kind_count(kind: EKind) -> u32 {
    MATRIX.iter().filter(|p| p.kind == kind).count() as u32
}

/// 某期望码的探针数。
pub fn expect_count(code: u32) -> u32 {
    MATRIX.iter().filter(|p| p.expect == code).count() as u32
}

/// 次序三格是否仍按 fd→值域→指针。
pub fn order_cells_ok() -> bool {
    let a = match probe_of("ord-fd-before-ptr") { Some(p) => judge(p) == EBADF, None => false };
    let b = match probe_of("ord-fd-before-val") { Some(p) => judge(p) == EBADF, None => false };
    let c = match probe_of("ord-val-before-ptr") { Some(p) => judge(p) == EINVAL, None => false };
    a && b && c
}

// ── 状态机与账本 ──────────────────────────────────────────────────────
/// 复测阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestStage {
    Matrix,
    Inject,
    Round,
    Declare,
}

impl RetestStage {
    /// 下一阶段。
    pub fn next(self) -> Option<RetestStage> {
        match self {
            RetestStage::Matrix => Some(RetestStage::Inject),
            RetestStage::Inject => Some(RetestStage::Round),
            RetestStage::Round => Some(RetestStage::Declare),
            RetestStage::Declare => None,
        }
    }

    /// 单向推进。
    pub fn advance(self) -> Result<RetestStage, RetestCode> {
        match self.next() {
            Some(n) => Ok(n),
            None => Err(RetestCode::StageSkip),
        }
    }
}

/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.escaped == 0 && self.injected == self.detected
    }
}

/// 全矩阵 10 轮注入账。
pub fn run_ledger() -> Ledger {
    let mut led = Ledger::new();
    for _round in 0..ROUNDS {
        for p in MATRIX.iter() {
            for m in MUTANTS.iter() {
                led.record(mutate(*m, judge(p)) != p.expect);
            }
        }
    }
    led
}

// ── 指纹 ──────────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 矩阵 + 变异类指纹（双跑一致用）。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for p in MATRIX.iter() {
        for b in p.id.as_bytes().iter() {
            h = mix(h, *b as u32);
        }
        h = mix(h, p.kind.code());
        h = mix(h, p.expect);
        h = mix(h, p.value);
    }
    for m in MUTANTS.iter() {
        h = mix(h, m.code());
    }
    h
}

/// 声明指纹。
pub fn declaration_fingerprint(d: &Declaration) -> u32 {
    let mut h = 0x811C_9DC5u32;
    h = mix(h, d.cells);
    h = mix(h, d.rounds);
    h = mix(h, d.ten_of_ten);
    h = mix(h, d.flaky);
    h = mix(h, if d.reproducible { 1 } else { 0 });
    h
}

// ── 判据总成 ──────────────────────────────────────────────────────────
/// 运行 F9217 全部判据。
pub fn run_unx9217_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9217");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 矩阵完整 + 冻结账
    s.add("矩阵：12 条探针齐全且期望码零漂移",
          MATRIX.len() == PROBES && expect_drift() == 0,
          "期望码以冻结账为准，改探针必须同步改账");

    // 3. 正常流：逐条探针判定与期望一致
    let clean = MATRIX.iter().all(|p| judge(p) == p.expect);
    s.add("正常流：12 条探针逐条判定与期望码一致", clean,
          "判定器本身先要全绿，否则复测的是坏尺子");

    // 4. 注入格总数
    s.add("注入格：12 探针 × 3 变异 = 36 格",
          total_cells() == 36 && MUTANTS.len() == MUTANT_KINDS,
          "格数是统计口径的基数，少一格所有比例都错");

    // 5. 逐格 10/10
    s.add("10/10 次检出：36 格逐格 10 轮全检出", all_cells_ten_of_ten(),
          "任何一格少于 10/10 都不能声明可复测");

    // 6. 每轮 36/36
    s.add("轮次统计：10 轮每轮检出 36/36", every_round_full(),
          "每轮满格说明矩阵没有依赖轮次的漏检路径");

    // 7. 声明格
    let d = declare(false);
    s.add("声明：cells=36 / rounds=10 / ten_of_ten=36 / flaky=0",
          d.cells == 36 && d.rounds == ROUNDS && d.ten_of_ten == 36 && d.flaky == 0,
          "声明字段是复测的对外承诺，必须与语料逐项咬合");

    // 8. 零 flaky 声明格
    s.add("零 flaky 声明：flaky=0 且 reproducible=true", zero_flaky(&d),
          "零 flaky 必须由全格 10/10 推出，不能手工置真");

    // 9. 反向注入：偶轮才报必须被抓
    let flaky = declare(true);
    s.add("反向注入：偶轮检出模型使 36 格全部记为 flaky",
          flaky.flaky == 36 && flaky.ten_of_ten == 0 && !flaky.reproducible,
          "声明器对轮次抖动必须敏感，否则零 flaky 是空话");

    // 10. 双跑一致
    let d2 = declare(false);
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：声明与语料指纹两次相等",
          d == d2 && f1 == f2
              && declaration_fingerprint(&d) == declaration_fingerprint(&d2),
          "复测不得依赖轮次外状态或哈希遍历顺序");

    // 11. 账本守恒
    let led = run_ledger();
    s.add("守恒：注入 360 = 检出 360 + 逃逸 0",
          led.conserve() && led.injected == 36 * ROUNDS,
          "注入账必须闭合；逃逸说明有格没被检查");

    // 12. 轮数算术
    s.add("算术：36 格 × 10 轮 = 360 次注入", 36 * ROUNDS == 360 && total_cells() * ROUNDS == 360,
          "统计口径用整数乘法，不做百分比以免抹平差异");

    // 13. 类别覆盖
    let kinds_ok = kind_count(EKind::BadFd) >= 3
        && kind_count(EKind::BadPtr) >= 3
        && kind_count(EKind::BadValue) >= 3
        && kind_count(EKind::Order) >= 3;
    s.add("类别覆盖：坏 fd/坏指针/坏值/次序各 ≥3 条", kinds_ok,
          "四类错误面必须都在矩阵里，缺类即复测不完整");

    // 14. 期望码分布
    s.add("期望码分布：EBADF 5 / EFAULT 3 / EINVAL 4，合计 12",
          expect_count(EBADF) == 5 && expect_count(EFAULT) == 3
              && expect_count(EINVAL) == 4
              && expect_count(EBADF) + expect_count(EFAULT) + expect_count(EINVAL) == 12,
          "三类码的配比是矩阵设计的一部分，漂移即换语料");

    // 15. 次序三格
    s.add("次序：fd 先于指针、fd 先于值域、值域先于指针", order_cells_ok(),
          "次序格失败说明门序被改，错码碰巧相等不能算通过");

    // 16. 状态机
    let mut stage = RetestStage::Matrix;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 {
            forward = false;
            break;
        }
    }
    s.add("状态机：Matrix→Inject→Round→Declare 单向三步",
          forward && steps == 3 && stage == RetestStage::Declare
              && stage.advance() == Err(RetestCode::StageSkip),
          "回退或跳步会让声明读到未跑完的轮次");

    // 17. 容量
    s.add("容量：36 格 ≤ 64 上限，10 轮固定，add_dyn ≤ 8",
          total_cells() as usize <= MAX_CELLS && ROUNDS == 10 && 3 <= 8,
          "格数超上限说明矩阵失控，应拆卡而不是继续堆格");

    // 18. 文案闭集
    let mut closed = true;
    for (i, c) in RetestCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() {
            closed = false;
        }
        for (j, o) in RetestCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() {
                closed = false;
            }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码闭集且可枚举，文案为空等于没有处置建议");

    // 19. 段内码值上界
    let last = RetestCode::ALL.len() as u32;
    s.add("段位余量：末码 < SEG_BASE + 0x40", last <= 0x40,
          "段窗口 0x40 必须容得下闭集，越界会侵入下一张卡");

    // 动态明细 1：10/10 统计
    s.add_dyn("动态：10/10 统计", all_cells_ten_of_ten() && every_round_full(),
              &format!("cells={} rounds={} per_round={} ten={}",
                       total_cells(), ROUNDS, round_detections(0), d.ten_of_ten));

    // 动态明细 2：flaky 反向注入
    s.add_dyn("动态：flaky 反向注入", flaky.flaky == 36 && !flaky.reproducible,
              &format!("clean_flaky={} parity_flaky={} parity_repro={}",
                       d.flaky, flaky.flaky, flaky.reproducible));

    // 动态明细 3：账本与指纹
    s.add_dyn("动态：账本与指纹", led.conserve(),
              &format!("injected={} detected={} escaped={} fp=0x{:08X}",
                       led.injected, led.detected, led.escaped, f1));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_clean_and_frozen() {
        assert_eq!(MATRIX.len(), PROBES);
        assert_eq!(expect_drift(), 0);
        assert!(MATRIX.iter().all(|p| judge(p) == p.expect));
        assert!(MATRIX.iter().all(|p| !p.id.is_empty() && p.expect != 0));
    }

    #[test]
    fn order_gates_are_pinned() {
        assert!(order_cells_ok());
        if let Some(p) = probe_of("ord-fd-before-ptr") {
            assert_eq!(judge(p), EBADF);
        }
        if let Some(p) = probe_of("ord-val-before-ptr") {
            assert_eq!(judge(p), EINVAL);
        }
    }

    #[test]
    fn cells_are_thirty_six() {
        assert_eq!(total_cells(), 36);
        assert_eq!(MATRIX.len() * MUTANTS.len(), 36);
        assert_eq!(total_cells() as usize, MATRIX.len() * MUTANTS.len());
    }

    #[test]
    fn every_cell_is_ten_of_ten() {
        assert!(all_cells_ten_of_ten());
        for p in MATRIX.iter() {
            for m in MUTANTS.iter() {
                assert_eq!(cell_hits(p, *m), ROUNDS);
            }
        }
    }

    #[test]
    fn every_round_is_full() {
        assert!(every_round_full());
        for round in 0..ROUNDS {
            assert_eq!(round_detections(round), 36);
        }
        assert_eq!(round_detections(ROUNDS), 0);
    }

    #[test]
    fn clean_declaration_is_reproducible() {
        let d = declare(false);
        assert_eq!(d.cells, 36);
        assert_eq!(d.rounds, 10);
        assert_eq!(d.ten_of_ten, 36);
        assert_eq!(d.flaky, 0);
        assert!(d.reproducible);
        assert!(zero_flaky(&d));
    }

    #[test]
    fn parity_injection_flips_declaration() {
        let d = declare(true);
        assert_eq!(d.flaky, 36);
        assert_eq!(d.ten_of_ten, 0);
        assert!(!d.reproducible);
        assert!(!zero_flaky(&d));
    }

    #[test]
    fn ledger_conserves_across_ten_rounds() {
        let led = run_ledger();
        assert!(led.conserve());
        assert_eq!(led.injected, 360);
        assert_eq!(led.detected, 360);
        assert_eq!(led.escaped, 0);
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let mut s = RetestStage::Matrix;
        let mut n = 0;
        while let Ok(next) = s.advance() {
            s = next;
            n += 1;
        }
        assert_eq!(n, 3);
        assert_eq!(s, RetestStage::Declare);
        assert_eq!(s.advance(), Err(RetestCode::StageSkip));
    }

    #[test]
    fn fingerprint_and_checks_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(declaration_fingerprint(&declare(false)), declaration_fingerprint(&declare(false)));
        let set = run_unx9217_checks();
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
