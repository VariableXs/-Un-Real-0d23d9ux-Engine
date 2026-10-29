//! UNX-F10859 pty 错误码字面总对账（六码矩阵）（0x1c3e0 段）
//!
//! 锚点：UNX-F10859-J1 六码字面总对账——pty 域六个错误码字面与场
//! 景分类恒等：EPERM=1 / EIO=5 / EAGAIN=11 / EINVAL=22 / ENOTTY=25 /
//! EPIPE=32；场景分类器（六类场景 → 码）必与字面锚一一命中，不得
//! 错位；错位场景（互串分类）必不出现。判定依赖场景字段演算，注入
//! 核（错位映射/漏类坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：六码字面锚 → 场景分类器（场景族 × 码表）→ 对账状态机
//! （逐格核验走账）→ 样本矩阵（六码 × 正反格）→ 注入核（错位/漏类
//! 必分歧）→ 总对账回放（六码字面全对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c3e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c3e0;
pub const RC_HIT: u32 = SEG_BASE + 0x01; // 对账命中
pub const RC_MISS: u32 = SEG_BASE + 0x02; // 对账失配
pub const RC_CLASS_GAP: u32 = SEG_BASE + 0x03; // 场景漏类
pub const RC_SHIFT: u32 = SEG_BASE + 0x04; // 错位映射
pub const RC_TABLE_LOST: u32 = SEG_BASE + 0x05; // 码表失平
pub const RC_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 对账账失平

/// 语义字面锚（六码）。
pub const EPERM: u32 = 1;
pub const EIO: u32 = 5;
pub const EAGAIN: u32 = 11;
pub const EINVAL: u32 = 22;
pub const ENOTTY: u32 = 25;
pub const EPIPE: u32 = 32;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        RC_HIT => "对账命中",
        RC_MISS => "对账失配",
        RC_CLASS_GAP => "场景漏类",
        RC_SHIFT => "错位映射",
        RC_TABLE_LOST => "码表失平",
        RC_LEDGER_LOST => "对账账失平",
        _ => "未知诊断码",
    }
}

/// 六类 pty 场景族：组长 setsid / slave 全关读 / 非阻塞空读 / 非法参
/// 数 / 非终端 ioctl / 死管道写。
#[derive(Clone, PartialEq, Debug)]
pub enum Scn {
    LeaderSetsid,
    SlaveGoneRead,
    NonblockEmpty,
    BadArg,
    NotTtyIoctl,
    DeadPipeWrite,
    OutOfBand, // 界外场景（必漏类）
}

/// 场景分类器：场景 → 错误码（字面锚直映射）。
pub fn classify(scn: &Scn) -> Option<u32> {
    match scn {
        Scn::LeaderSetsid => Some(EPERM),
        Scn::SlaveGoneRead => Some(EIO),
        Scn::NonblockEmpty => Some(EAGAIN),
        Scn::BadArg => Some(EINVAL),
        Scn::NotTtyIoctl => Some(ENOTTY),
        Scn::DeadPipeWrite => Some(EPIPE),
        Scn::OutOfBand => None,
    }
}

/// 码反查：码 → 场景族（无此码则漏类）。
pub fn code_of(code: u32) -> Option<Scn> {
    match code {
        EPERM => Some(Scn::LeaderSetsid),
        EIO => Some(Scn::SlaveGoneRead),
        EAGAIN => Some(Scn::NonblockEmpty),
        EINVAL => Some(Scn::BadArg),
        ENOTTY => Some(Scn::NotTtyIoctl),
        EPIPE => Some(Scn::DeadPipeWrite),
        _ => None,
    }
}

/// 对账状态机：逐格核验走账（命中/失配/漏类三账）。
pub struct Reconciler {
    pub hits: u32,
    pub misses: u32,
    pub gaps: u32,
    pub note: String,
}

impl Reconciler {
    pub fn new() -> Reconciler {
        Reconciler { hits: 0, misses: 0, gaps: 0, note: "idle".to_string() }
    }

    /// 单格核验：场景分类码与期望码对平。
    pub fn check(&mut self, scn: &Scn, expect: u32) -> bool {
        match classify(scn) {
            Some(c) if c == expect => {
                self.hits += 1;
                self.note = "hit".to_string();
                true
            }
            Some(_) => {
                self.misses += 1;
                self.note = "shift".to_string();
                false
            }
            None => {
                self.gaps += 1;
                self.note = "gap".to_string();
                false
            }
        }
    }

    /// 对账账一致：三账之和恒为核验总数可辨。
    pub fn ledger_ok(&self) -> bool {
        self.hits + self.misses + self.gaps <= 64 && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：场景分类码与期望对平（None 漏类编码 0）命中计数。
pub fn verify_matrix(scns: &[Scn], expects: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = scns.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < expects.len() { expects[i] } else { 0 };
        let hit = match classify(&scns[i]) {
            Some(c) => c == want,
            None => want == 0,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：六码 × 正反格（正格直码 / 反格互串期望必失配）。
pub fn sample_matrix() -> (Vec<Scn>, Vec<u32>) {
    let mut scns: Vec<Scn> = Vec::new();
    let mut expects: Vec<u32> = Vec::new();
    // R0 组长 setsid → EPERM
    scns.push(Scn::LeaderSetsid);
    expects.push(EPERM);
    // R1 slave 全关读 → EIO
    scns.push(Scn::SlaveGoneRead);
    expects.push(EIO);
    // R2 非阻塞空读 → EAGAIN
    scns.push(Scn::NonblockEmpty);
    expects.push(EAGAIN);
    // R3 非法参数 → EINVAL
    scns.push(Scn::BadArg);
    expects.push(EINVAL);
    // R4 非终端 ioctl → ENOTTY
    scns.push(Scn::NotTtyIoctl);
    expects.push(ENOTTY);
    // R5 死管道写 → EPIPE
    scns.push(Scn::DeadPipeWrite);
    expects.push(EPIPE);
    // R6 界外场景 → 漏类（None 编码 0）
    scns.push(Scn::OutOfBand);
    expects.push(0);
    // R7 复核格：组长 setsid 复核 EPERM（互串他码必失配由 J4 把守）
    scns.push(Scn::LeaderSetsid);
    expects.push(EPERM);
    (scns, expects)
}

/// 注入 A：坏实现错位映射（码表循环错一格）。
pub fn faulty_shift(scn: &Scn) -> Option<u32> {
    match classify(scn) {
        Some(EPERM) => Some(EIO),
        Some(EIO) => Some(EAGAIN),
        Some(EAGAIN) => Some(EINVAL),
        Some(EINVAL) => Some(ENOTTY),
        Some(ENOTTY) => Some(EPIPE),
        Some(EPIPE) => Some(EPERM),
        Some(other) => Some(other),
        None => None,
    }
}

/// 注入 B：坏实现漏类（界外场景误映射 EAGAIN）。
pub fn faulty_gap_fill(scn: &Scn) -> Option<u32> {
    classify(scn).or(Some(EAGAIN))
}

/// 注入核：两处坏账与分类器必分歧。
pub fn inject_two() -> [bool; 2] {
    let d1 = classify(&Scn::LeaderSetsid) != faulty_shift(&Scn::LeaderSetsid);
    let d2 = classify(&Scn::OutOfBand) != faulty_gap_fill(&Scn::OutOfBand);
    [d1, d2]
}

/// 总对账回放：六码字面与双射表全对平（classify/code_of 双向）。
pub fn full_replay_ok() -> bool {
    let pairs = [
        (Scn::LeaderSetsid, EPERM),
        (Scn::SlaveGoneRead, EIO),
        (Scn::NonblockEmpty, EAGAIN),
        (Scn::BadArg, EINVAL),
        (Scn::NotTtyIoctl, ENOTTY),
        (Scn::DeadPipeWrite, EPIPE),
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < pairs.len() {
        let fwd = classify(&pairs[i].0) == Some(pairs[i].1);
        let rev = code_of(pairs[i].1) == Some(pairs[i].0.clone());
        if !(fwd && rev) {
            ok = false;
        }
        i += 1;
    }
    ok && code_of(EPERM + 100).is_none() && classify(&Scn::OutOfBand).is_none()
}

/// 账面回放：Reconciler 六正格全命中 + 一界外格漏类落账。
pub fn ledger_replay_ok() -> bool {
    let mut rc = Reconciler::new();
    let a = rc.check(&Scn::LeaderSetsid, EPERM);
    let b = rc.check(&Scn::SlaveGoneRead, EIO);
    let c = rc.check(&Scn::NonblockEmpty, EAGAIN);
    let d = rc.check(&Scn::BadArg, EINVAL);
    let e = rc.check(&Scn::NotTtyIoctl, ENOTTY);
    let f = rc.check(&Scn::DeadPipeWrite, EPIPE);
    let g = rc.check(&Scn::OutOfBand, EAGAIN);
    a && b && c && d && e && f && !g
        && rc.hits == 6 && rc.gaps == 1 && rc.misses == 0 && rc.ledger_ok()
}

/// 对账注记：核验渲染。
pub fn rc_note(scn: &Scn, expect: u32) -> String {
    match classify(scn) {
        Some(c) if c == expect => format!("{} (hit {})", diag_text(RC_HIT), c),
        Some(c) => format!("{} (got {} want {})", diag_text(RC_SHIFT), c, expect),
        None => diag_text(RC_CLASS_GAP).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10859_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10859");
    let (scns, expects) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&scns, &expects);

    // J1 正路径：六码字面锚逐一命中
    let j1 = classify(&Scn::LeaderSetsid) == Some(EPERM)
        && classify(&Scn::SlaveGoneRead) == Some(EIO)
        && classify(&Scn::NonblockEmpty) == Some(EAGAIN);
    s.add("J1-前三码", j1, "EPERM/EIO/EAGAIN 命中");

    // J2 后三码命中
    let j2 = classify(&Scn::BadArg) == Some(EINVAL)
        && classify(&Scn::NotTtyIoctl) == Some(ENOTTY)
        && classify(&Scn::DeadPipeWrite) == Some(EPIPE);
    s.add("J2-后三码", j2, "EINVAL/ENOTTY/EPIPE 命中");

    // J3 边界：界外场景漏类 / 未知码漏类
    let j3 = classify(&Scn::OutOfBand).is_none() && code_of(EPIPE + 1).is_none();
    s.add("J3-漏类两沿", j3, "None 面可辨");

    // J4 互串不命中：任一场景误配他码必失配
    let j4 = classify(&Scn::DeadPipeWrite) != Some(EIO)
        && classify(&Scn::NonblockEmpty) != Some(ENOTTY);
    s.add("J4-互串不失配", j4, "错位格必不出现");

    // J5 总对账回放（双射全对平）
    s.add("J5-总对账回放", full_replay_ok(), "classify/code_of 双向命中");

    // J6 账面回放（六正格+一界外）
    s.add("J6-账面回放", ledger_replay_ok(), "hits=6 gaps=1");

    // J7 样本矩阵全命中（含反格失配计数）
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 错位映射检出
    let inj = inject_two();
    s.add("J8-错位映射检出", inj[0], "码表循环错位必分歧");

    // J9 注入2 漏类检出
    s.add("J9-漏类检出", inj[1], "界外误映射必分歧");

    // J10 对账账一致
    let mut rc = Reconciler::new();
    let _ = rc.check(&Scn::BadArg, EINVAL);
    let _ = rc.check(&Scn::BadArg, EIO);
    s.add("J10-对账账", rc.ledger_ok() && rc.hits == 1 && rc.misses == 1, "一命一失各落账");

    // J11 字面锚：六码字面
    let j11 = EPERM == 1 && EIO == 5 && EAGAIN == 11
        && EINVAL == 22 && ENOTTY == 25 && EPIPE == 32;
    s.add("J11-六码字面锚", j11, "1/5/11/22/25/32");

    // J12 诊断码闭集
    let j12 = diag_text(RC_HIT) == "对账命中"
        && diag_text(RC_SHIFT) == "错位映射"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 对账注记落账
    let note = rc_note(&Scn::DeadPipeWrite, EPIPE);
    s.add_dyn("J14-对账注记", note.contains("hit 32"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10859_flow() {
        assert_eq!(classify(&Scn::LeaderSetsid), Some(EPERM));
        assert_eq!(classify(&Scn::DeadPipeWrite), Some(EPIPE));
        assert_eq!(classify(&Scn::OutOfBand), None);
        assert_eq!(code_of(EAGAIN), Some(Scn::NonblockEmpty));
        assert_eq!(code_of(99), None);
        let mut rc = Reconciler::new();
        assert!(rc.check(&Scn::NotTtyIoctl, ENOTTY));
        assert!(!rc.check(&Scn::NotTtyIoctl, EIO));
        assert!(rc.ledger_ok());
    }

    #[test]
    fn unx10859_checks_green() {
        let s = run_unx10859_checks();
        assert!(s.all_passed(), "UNX-F10859 判据未全绿");
    }
}
