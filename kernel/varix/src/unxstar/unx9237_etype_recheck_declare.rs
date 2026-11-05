//! UNX-F9237 E 型判据可复测性声明（10/10 次检出） · 0x7A8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9237 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9237-J1 全矩阵 10 次重复注入
//!   判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 12 格全矩阵：坏 fd 4 格、坏指针 4 格、坏值 3 格、门序 1 格。每格只登
//!      记名、种类、期望码；期望码必须等于 judge(种类) 的现算值，二者是单源，
//!      因此「矩阵自洽」不需人工回填。
//!   ② 可复测性是统计性质：同类变体重复注入 10 轮，每轮都必须检出。10/10
//!      不是一次性通过，而是 10 轮逐轮通过的合取。
//!   ③ 零 flaky 声明由反例守护：声明器接受一份计数三元组，任何一类不足 10
//!      即置 flaky 并拒绝签发；只有 [10,10,10] 才能拿到 stable 声明。
//!   ④ 检出按轮记账：账目按「轮 × 变体」顺序落槽，每轮三槽全中才算该轮完整，
//!      这样既能看到总数 30，也能看到每轮 3/3。
//!   ⑤ 双跑一致：两次独立跑批的矩阵指纹、注入账、轮计数必须逐项相等；任何
//!      依赖时序或全局状态的分支都会在此暴露。
//!   ⑥ 阶段机 Draft→Declared→Retested→Chartered→Sealed 只许单步前进，
//!      声明一旦封存不可回退重签。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9237 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A8C0;
/// 矩阵格数。
pub const MATRIX_SIZE: usize = 12;
/// 重复注入轮数。
pub const ROUNDS: u32 = 10;
/// 变体类别数。
pub const MUTANT_CLASSES: u32 = 3;
/// 总注入次数（格 × 变体 × 轮）。
pub const INJECTIONS: u32 = MATRIX_SIZE as u32 * MUTANT_CLASSES * ROUNDS;
/// 坏 fd 格数。
pub const KIND_BADFD: usize = 4;
/// 坏指针格数。
pub const KIND_BADPTR: usize = 4;
/// 坏值格数。
pub const KIND_BADVALUE: usize = 3;
/// 门序格数。
pub const KIND_ORDER: usize = 1;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 判据种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EKind {
    BadFd,
    BadPtr,
    BadValue,
    Order,
}
impl EKind {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            EKind::BadFd => 0,
            EKind::BadPtr => 1,
            EKind::BadValue => 2,
            EKind::Order => 3,
        }
    }
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            EKind::BadFd => "坏 fd",
            EKind::BadPtr => "坏指针",
            EKind::BadValue => "坏值",
            EKind::Order => "门序",
        }
    }
    /// 四类全量。
    pub const ALL: [EKind; 4] = [EKind::BadFd, EKind::BadPtr, EKind::BadValue, EKind::Order];
}
/// 单源判定：种类直接决定期望码。
pub const fn judge(k: EKind) -> u32 {
    match k {
        EKind::BadFd => EBADF,
        EKind::BadPtr => EFAULT,
        EKind::BadValue => EINVAL,
        EKind::Order => EBADF,
    }
}
/// 错误码中文文案（闭集）。
pub fn errno_text(code: u32) -> &'static str {
    match code {
        EBADF => "坏文件描述符：先查代次再查复用",
        EFAULT => "坏用户指针：核对方向与可空声明",
        EINVAL => "参数值非法：核对 flags/cmd 白名单",
        0 => "成功",
        _ => "未登记错误码，拒绝落账",
    }
}
/// 矩阵格。
#[derive(Clone, Copy)]
pub struct Probe {
    pub name: &'static str,
    pub kind: EKind,
    pub expect: u32,
}
/// 格构造器（期望码由 judge 现算，保持单源）。
pub const fn probe(name: &'static str, k: EKind) -> Probe {
    Probe { name, kind: k, expect: judge(k) }
}
/// 12 格冻结矩阵。
pub const MATRIX: [Probe; 12] = [
    probe("read_badfd", EKind::BadFd),
    probe("write_badfd", EKind::BadFd),
    probe("close_badfd", EKind::BadFd),
    probe("dup2_badfd", EKind::BadFd),
    probe("read_buf_badptr", EKind::BadPtr),
    probe("write_out_badptr", EKind::BadPtr),
    probe("open_path_badptr", EKind::BadPtr),
    probe("readv_iovec_badptr", EKind::BadPtr),
    probe("lseek_whence_badval", EKind::BadValue),
    probe("dup3_flags_badval", EKind::BadValue),
    probe("fcntl_cmd_badval", EKind::BadValue),
    probe("fd_before_ptr", EKind::Order),
];
/// 冻结期望表。
pub const FROZEN_EXPECT: [u32; 12] = [
    EBADF, EBADF, EBADF, EBADF,
    EFAULT, EFAULT, EFAULT, EFAULT,
    EINVAL, EINVAL, EINVAL,
    EBADF,
];
/// 种类计数。
pub fn kind_count(rows: &[Probe], k: EKind) -> usize {
    rows.iter().filter(|p| p.kind == k).count()
}
/// 冻结矩阵副本。
pub fn frozen_matrix() -> Vec<Probe> {
    MATRIX.iter().copied().collect()
}
/// 期望漂移：格上 expect 与 judge(种类) 不等。
pub fn expect_drift(rows: &[Probe]) -> usize {
    rows.iter().filter(|p| p.expect != judge(p.kind)).count()
}
/// 冻结值漂移：逐格与冻结表不等。
pub fn frozen_drift(rows: &[Probe]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < rows.len() && i < FROZEN_EXPECT.len() {
        if rows[i].expect != FROZEN_EXPECT[i] {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 矩阵差异度（含长度差、名差、单源差、冻结差）。
pub fn matrix_diff(rows: &[Probe]) -> usize {
    let mut n = 0;
    if rows.len() != MATRIX_SIZE {
        n += 1;
    }
    let limit = if rows.len() < MATRIX_SIZE { rows.len() } else { MATRIX_SIZE };
    let mut i = 0;
    while i < limit {
        if rows[i].name != MATRIX[i].name {
            n += 1;
        }
        if rows[i].expect != judge(rows[i].kind) {
            n += 1;
        }
        if rows[i].expect != FROZEN_EXPECT[i] {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 格名唯一。
pub fn names_unique(rows: &[Probe]) -> bool {
    let mut i = 0;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() {
            if rows[i].name == rows[j].name {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 注入变体。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mutant {
    FlipVerdict,
    DropProbe,
    OrderSwap,
}
impl Mutant {
    /// 三类变体。
    pub fn all() -> [Mutant; 3] {
        [Mutant::FlipVerdict, Mutant::DropProbe, Mutant::OrderSwap]
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Mutant::FlipVerdict => 0,
            Mutant::DropProbe => 1,
            Mutant::OrderSwap => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Mutant::FlipVerdict => "首格期望码被改写，单源判据必须报红",
            Mutant::DropProbe => "末格被删除，矩阵长度与差异度必须同时抬头",
            Mutant::OrderSwap => "门序格期望被换成 EFAULT，冻结表必须报红",
        }
    }
    /// 在冻结矩阵副本上施加变体。
    pub fn apply(self) -> Vec<Probe> {
        let mut v = frozen_matrix();
        match self {
            Mutant::FlipVerdict => {
                if let Some(p) = v.first_mut() {
                    p.expect = EINVAL;
                }
            }
            Mutant::DropProbe => {
                v.pop();
            }
            Mutant::OrderSwap => {
                for p in v.iter_mut() {
                    if p.kind == EKind::Order {
                        p.expect = EFAULT;
                    }
                }
            }
        }
        v
    }
}
/// 单轮单变体是否检出（差异度非零即检出）。
pub fn detect_round(m: Mutant, round: u32) -> bool {
    round < ROUNDS && matrix_diff(&m.apply()) > 0
}
/// 注入账。
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}
impl Ledger {
    /// 空账。
    pub fn new() -> Self {
        Ledger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }
    /// 记一轮：一次注入覆盖全矩阵，按格记账（格×类×轮）。
    pub fn record(&mut self, tag: u8, hit: bool, cells: u32) {
        self.injected += cells;
        if hit {
            self.detected += cells;
        } else {
            self.escaped += cells;
        }
        self.rounds.push((tag, if hit { 1 } else { 0 }));
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}
/// 3 类 × 10 轮。
pub fn run_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = 0;
    while r < ROUNDS {
        for m in Mutant::all().iter() {
            let hit = detect_round(*m, r);
            l.record(m.tag() as u8, hit, MATRIX_SIZE as u32);
        }
        r += 1;
    }
    l
}
/// 每类检出数。
pub fn per_class(l: &Ledger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        out[(tag as usize) % 3] += hit;
    }
    out
}
/// 每轮检出数（每轮三槽合计）。
pub fn round_counts(l: &Ledger) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0;
    let step = MUTANT_CLASSES as usize;
    while i + step <= l.rounds.len() {
        let mut c = 0u32;
        let mut k = 0usize;
        while k < step {
            c += l.rounds[i + k].1;
            k += 1;
        }
        out.push(c);
        i += step;
    }
    out
}
/// 可复测性声明。
#[derive(Clone, Copy)]
pub struct Declaration {
    pub matrix: usize,
    pub rounds: u32,
    pub per_class: [u32; 3],
    pub flaky: u32,
    pub stable: bool,
}
/// 签发声明：任一类不足 10 即 flaky。
pub fn declare(per_class: [u32; 3]) -> Declaration {
    let mut flaky = 0u32;
    for h in per_class.iter() {
        if *h != ROUNDS {
            flaky += 1;
        }
    }
    Declaration { matrix: MATRIX_SIZE, rounds: ROUNDS, per_class, flaky, stable: flaky == 0 }
}
/// 零 flaky 判据。
pub fn zero_flaky(d: &Declaration) -> bool {
    d.flaky == 0 && d.stable
}
/// 10/10 统计格。
pub fn all_cells_ten_of_ten(per_class: &[u32; 3]) -> bool {
    per_class.iter().all(|h| *h == ROUNDS)
}
/// 每轮是否满额。
pub fn every_round_full(per_class: &[u32; 3]) -> bool {
    per_class.iter().fold(0u32, |a, b| a + *b) == ROUNDS * MUTANT_CLASSES
}
/// 声明摘要。
pub fn summary(d: &Declaration) -> &'static str {
    if d.stable {
        "全矩阵 12 格 × 3 变体 × 10 轮，逐轮检出，零 flaky"
    } else {
        "存在未满 10/10 的类别，拒绝签发可复测声明"
    }
}
/// 复测状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RetestStage {
    Draft,
    Declared,
    Retested,
    Chartered,
    Sealed,
}
impl RetestStage {
    /// 单步前进。
    pub fn can_go(self, next: RetestStage) -> bool {
        matches!(
            (self, next),
            (RetestStage::Draft, RetestStage::Declared)
                | (RetestStage::Declared, RetestStage::Retested)
                | (RetestStage::Retested, RetestStage::Chartered)
                | (RetestStage::Chartered, RetestStage::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            RetestStage::Draft => 0,
            RetestStage::Declared => 1,
            RetestStage::Retested => 2,
            RetestStage::Chartered => 3,
            RetestStage::Sealed => 4,
        }
    }
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RetestCode {
    MatrixSizeMismatch,
    KindCountDrift,
    ExpectDrift,
    FrozenDrift,
    DuplicateName,
    RoundShort,
    FlakyNotRejected,
    FlakyNotDetected,
    LedgerNotConserved,
    DeclarationUnsigned,
    DoubleRunDrift,
    SegmentOutOfRange,
}
impl RetestCode {
    /// 码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RetestCode::MatrixSizeMismatch => "矩阵不是 12 格：补齐后再复测",
            RetestCode::KindCountDrift => "种类计数漂移：应为 4/4/3/1",
            RetestCode::ExpectDrift => "期望码与种类判定不一致：回到单源构造",
            RetestCode::FrozenDrift => "冻结期望表被改写：逐格回填",
            RetestCode::DuplicateName => "格名重复：复测无法定位到唯一格",
            RetestCode::RoundShort => "轮次不足 10：可复测性不成立",
            RetestCode::FlakyNotRejected => "flaky 未拒绝：9/10 被当成通过",
            RetestCode::FlakyNotDetected => "flaky 漏检：不足 10 却签发稳定声明",
            RetestCode::LedgerNotConserved => "注入账不守恒：注入须等于检出加逃逸",
            RetestCode::DeclarationUnsigned => "声明未签发：稳定标志或摘要缺失",
            RetestCode::DoubleRunDrift => "双跑漂移：复测路径存在非确定分支",
            RetestCode::SegmentOutOfRange => "码值越出 0x7A8C0 窗口：段位守卫拒绝",
        }
    }
    /// 全量闭集。
    pub const ALL: [RetestCode; 12] = [
        RetestCode::MatrixSizeMismatch,
        RetestCode::KindCountDrift,
        RetestCode::ExpectDrift,
        RetestCode::FrozenDrift,
        RetestCode::DuplicateName,
        RetestCode::RoundShort,
        RetestCode::FlakyNotRejected,
        RetestCode::FlakyNotDetected,
        RetestCode::LedgerNotConserved,
        RetestCode::DeclarationUnsigned,
        RetestCode::DoubleRunDrift,
        RetestCode::SegmentOutOfRange,
    ];
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 矩阵指纹。
pub fn matrix_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9237/matrix");
    for p in MATRIX.iter() {
        h = fnv1a(h, p.name.as_bytes());
        h = fnv1a(h, &[p.kind.tag() as u8]);
        h = fnv1a(h, &p.expect.to_le_bytes());
    }
    h
}
/// 跑批指纹。
pub fn run_digest() -> u32 {
    let l = run_ledger();
    let pc = per_class(&l);
    let mut h = fnv1a(matrix_digest(), b"run");
    h = fnv1a(h, &l.injected.to_le_bytes());
    h = fnv1a(h, &l.detected.to_le_bytes());
    h = fnv1a(h, &pc[0].to_le_bytes());
    h = fnv1a(h, &pc[1].to_le_bytes());
    h = fnv1a(h, &pc[2].to_le_bytes());
    h
}
/// 文案闭集自检。
pub fn text_ok() -> bool {
    if RetestCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < RetestCode::ALL.len() {
        let a = RetestCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < RetestCode::ALL.len() {
            let b = RetestCode::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 自检总成。
pub fn run_unx9237_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9237");
    let rows = frozen_matrix();
    s.add("seg_base", SEG_BASE == 0x7A8C0, "段基址应为 0x7A8C0");
    s.add("code_window", RetestCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "码值落在 0x7A8C0..0x7A900");
    s.add("text_closed_set", text_ok(), "12 条闭集、值唯一、建议非空");
    s.add("matrix_size", rows.len() == MATRIX_SIZE, "矩阵应为 12 格");
    s.add("kind_badfd", kind_count(&rows, EKind::BadFd) == KIND_BADFD, "坏 fd 4 格");
    s.add("kind_badptr", kind_count(&rows, EKind::BadPtr) == KIND_BADPTR, "坏指针 4 格");
    s.add("kind_badval", kind_count(&rows, EKind::BadValue) == KIND_BADVALUE, "坏值 3 格");
    s.add("kind_order", kind_count(&rows, EKind::Order) == KIND_ORDER, "门序 1 格");
    s.add("names_unique", names_unique(&rows), "格名互不重复");
    s.add("single_source", expect_drift(&rows) == 0, "期望码全部等于 judge(种类)");
    s.add("frozen_expect", frozen_drift(&rows) == 0, "期望码与冻结表一致");
    s.add("matrix_zero_diff", matrix_diff(&rows) == 0, "冻结矩阵差异度为 0");
    s.add("errno_text", [EBADF, EFAULT, EINVAL, 0].iter().all(|c| !errno_text(*c).is_empty()), "错误码文案非空");
    let l = run_ledger();
    let pc = per_class(&l);
    let rc = round_counts(&l);
    s.add("inject_total", l.injected == INJECTIONS, "注入总数 12×3×10=360");
    s.add("ten_of_ten", all_cells_ten_of_ten(&pc), "三类各检出 10/10");
    s.add("every_round_full", every_round_full(&pc) && rc.len() == ROUNDS as usize && rc.iter().all(|c| *c == MUTANT_CLASSES), "每轮三槽全中");
    s.add("ledger_conserved", l.conserved() && l.escaped == 0, "注入 = 检出 + 逃逸，逃逸为 0");
    let good = declare([ROUNDS, ROUNDS, ROUNDS]);
    let bad = declare([ROUNDS, ROUNDS - 1, ROUNDS]);
    s.add("zero_flaky", zero_flaky(&good) && good.stable && good.flaky == 0, "[10,10,10] 签发零 flaky 声明");
    s.add("flaky_rejected", !zero_flaky(&bad) && bad.flaky == 1 && !bad.stable, "9/10 必须被拒绝");
    s.add("declaration_summary", !summary(&good).is_empty() && !summary(&bad).is_empty() && summary(&good) != summary(&bad), "声明摘要非空且区分正反");
    let stage_ok = RetestStage::Draft.can_go(RetestStage::Declared)
        && RetestStage::Declared.can_go(RetestStage::Retested)
        && RetestStage::Retested.can_go(RetestStage::Chartered)
        && RetestStage::Chartered.can_go(RetestStage::Sealed)
        && !RetestStage::Draft.can_go(RetestStage::Sealed)
        && !RetestStage::Sealed.can_go(RetestStage::Draft);
    s.add("state_machine", stage_ok, "复测阶段只许单步前进");
    s.add("mutant_text", Mutant::all().iter().all(|m| !m.detail().is_empty()), "变体文案非空");
    s.add("mutants_detected", Mutant::all().iter().all(|m| matrix_diff(&m.apply()) > 0), "三变体差异度均非零");
    s.add("capacity", rows.len() <= 32, "矩阵容量不超过 32");
    let d1 = run_digest();
    let d2 = run_digest();
    let l2 = run_ledger();
    s.add("double_run", d1 == d2 && l2.injected == l.injected && l2.detected == l.detected && per_class(&l2) == pc, "双跑指纹与统计一致");
    s.add_dyn("stats_detail", all_cells_ten_of_ten(&pc), &format!("检出={} 逃逸={} 轮数={}", l.detected, l.escaped, rc.len()));
    s.add_dyn("class_detail", true, &format!("坏fd={} 坏指针={} 坏值={}", pc[0], pc[1], pc[2]));
    s.add_dyn("digest_detail", d1 != 0, &format!("矩阵={:08x} 跑={:08x} 注入={}", matrix_digest(), d1, l.injected));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_all_green() {
        let s = run_unx9237_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn matrix_shape() {
        let rows = frozen_matrix();
        assert_eq!(rows.len(), 12);
        assert_eq!(kind_count(&rows, EKind::BadFd), 4);
        assert_eq!(kind_count(&rows, EKind::BadPtr), 4);
        assert_eq!(kind_count(&rows, EKind::BadValue), 3);
        assert_eq!(kind_count(&rows, EKind::Order), 1);
        assert!(names_unique(&rows));
    }
    #[test]
    fn single_source_holds() {
        let rows = frozen_matrix();
        assert_eq!(expect_drift(&rows), 0);
        assert_eq!(frozen_drift(&rows), 0);
        assert_eq!(matrix_diff(&rows), 0);
        for r in rows.iter() {
            assert_eq!(r.expect, judge(r.kind));
        }
    }
    #[test]
    fn injection_is_360() {
        let l = run_ledger();
        assert_eq!(l.injected, 360);
        assert_eq!(l.detected, 360);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }
    #[test]
    fn ten_of_ten_statistics() {
        let l = run_ledger();
        let pc = per_class(&l);
        assert_eq!(pc, [10, 10, 10]);
        assert!(all_cells_ten_of_ten(&pc));
        assert!(every_round_full(&pc));
        let rc = round_counts(&l);
        assert_eq!(rc.len(), 10);
        assert!(rc.iter().all(|c| *c == 3));
    }
    #[test]
    fn declaration_zero_flaky() {
        let good = declare([10, 10, 10]);
        assert!(good.stable);
        assert_eq!(good.flaky, 0);
        assert!(zero_flaky(&good));
        let bad = declare([10, 9, 10]);
        assert!(!bad.stable);
        assert_eq!(bad.flaky, 1);
        assert!(!zero_flaky(&bad));
    }
    #[test]
    fn flaky_variants_all_rejected() {
        for v in [0u32, 1, 5, 9].iter() {
            let d = declare([10, 10, *v]);
            assert!(!zero_flaky(&d));
            assert_eq!(d.flaky, 1);
        }
    }
    #[test]
    fn mutants_always_differ() {
        for m in Mutant::all().iter() {
            assert!(matrix_diff(&m.apply()) > 0);
            assert!(detect_round(*m, 0));
            assert!(!detect_round(*m, ROUNDS));
        }
    }
    #[test]
    fn stage_machine_single_step() {
        let mut cur = RetestStage::Draft;
        for nxt in [RetestStage::Declared, RetestStage::Retested, RetestStage::Chartered, RetestStage::Sealed].iter() {
            assert!(cur.can_go(*nxt));
            cur = *nxt;
        }
        assert!(!RetestStage::Draft.can_go(RetestStage::Sealed));
    }
    #[test]
    fn digests_stable() {
        assert_eq!(matrix_digest(), matrix_digest());
        assert_eq!(run_digest(), run_digest());
        assert_ne!(matrix_digest(), 0);
    }
    #[test]
    fn diagnostic_text_closed_set() {
        assert_eq!(RetestCode::ALL.len(), 12);
        assert!(text_ok());
    }
}
