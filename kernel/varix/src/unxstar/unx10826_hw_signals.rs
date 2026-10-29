//! UNX-F10826 SIGSEGV/SIGFPE/SIGILL 硬件信号源矩阵（si_code 面）（0x1bfc0 段）
//!
//! 锚点：UNX-F10826-J1 硬件源对平——SIGSEGV 携 SEGV_MAPERR/ACCERR、
//! SIGFPE 携整除/浮除码、SIGILL 携非法操作码、异步 kill 源必 SI_USER
//! 不可冒充硬件码四线全对平；注入坏实现（异步源冒硬件码/同步位漏查）
//! 必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：故障事件账（信号号 × 故障类别 × 地址）+ si_code 判定器（同步
//! 故障按类别定码、异步源恒 SI_USER）+ 源分类状态机 + 注记核（码语义
//! 渲染）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGSEGV=11、SIGFPE=8、SIGILL=4。
pub const SIGSEGV: u32 = 11;
pub const SIGFPE: u32 = 8;
pub const SIGILL: u32 = 4;

/// si_code 锚：SI_USER=0、SEGV_MAPERR=1、SEGV_ACCERR=2、FPE_INTDIV=1、FPE_FLTDIV=2、ILL_ILLOPC=1。
pub const SI_USER: u32 = 0;
pub const SEGV_MAPERR: u32 = 1;
pub const SEGV_ACCERR: u32 = 2;
pub const FPE_INTDIV: u32 = 1;
pub const FPE_FLTDIV: u32 = 2;
pub const ILL_ILLOPC: u32 = 1;

/// 诊断段（0x1bfc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bfc0;
pub const HW_ASYNC_FAKE: u32 = SEG_BASE + 0x01; // 异步源冒硬件码
pub const HW_SYNC_MISS: u32 = SEG_BASE + 0x02; // 同步故障漏定码
pub const HW_CODE_SKEW: u32 = SEG_BASE + 0x03; // 码类错账
pub const HW_ADDR_LOSS: u32 = SEG_BASE + 0x04; // 故障地址丢失
pub const HW_KIND_MISS: u32 = SEG_BASE + 0x05; // 故障类别漏账
pub const HW_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        HW_ASYNC_FAKE => "异步源冒硬件码",
        HW_SYNC_MISS => "同步故障漏定码",
        HW_CODE_SKEW => "码类错账",
        HW_ADDR_LOSS => "故障地址丢失",
        HW_KIND_MISS => "故障类别漏账",
        HW_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 故障类别。
#[derive(Clone, PartialEq, Eq)]
pub enum Fault {
    /// 映射缺失（地址未映射）。
    MapErr,
    /// 权限错（映射存在但权限不符）。
    AccErr,
    /// 整数除零。
    IntDiv,
    /// 浮点除零。
    FltDiv,
    /// 非法操作码。
    IllOpc,
}

/// 源型。
#[derive(Clone, PartialEq, Eq)]
pub enum Source {
    /// 同步硬件故障。
    Sync,
    /// 异步投递（kill/raise 面）。
    Async,
}

/// 场景输入：信号号 × 源型 × 故障类别 × 故障地址。
#[derive(Clone)]
pub struct Scenario {
    pub sig: u32,
    pub src: Source,
    pub fault: Option<Fault>,
    pub addr: u64,
}

/// 故障事件（si_code 判定输出）。
#[derive(Clone, PartialEq, Eq)]
pub struct SiEvent {
    pub sig: u32,
    pub code: u32,
    pub addr: u64,
}

/// si_code 判定核：同步故障按类别定码；异步源恒 SI_USER 不可冒硬件码。
pub fn judge(sc: &Scenario) -> SiEvent {
    match sc.src {
        Source::Async => SiEvent { sig: sc.sig, code: SI_USER, addr: 0 },
        Source::Sync => {
            let code = match (&sc.fault, sc.sig) {
                (Some(Fault::MapErr), SIGSEGV) => SEGV_MAPERR,
                (Some(Fault::AccErr), SIGSEGV) => SEGV_ACCERR,
                (Some(Fault::IntDiv), SIGFPE) => FPE_INTDIV,
                (Some(Fault::FltDiv), SIGFPE) => FPE_FLTDIV,
                (Some(Fault::IllOpc), SIGILL) => ILL_ILLOPC,
                _ => SI_USER,
            };
            SiEvent { sig: sc.sig, code, addr: sc.addr }
        }
    }
}

/// 硬件源核：同步故障必须携带非 SI_USER 码。
pub fn sync_code_ok(sc: &Scenario) -> bool {
    let ev = judge(sc);
    ev.code != SI_USER
}

/// 故障账状态机：逐事件落账重放。
pub struct FaultLedger {
    pub rows: Vec<(SiEvent, String)>,
    pub note: String,
}

impl FaultLedger {
    pub fn new() -> FaultLedger {
        FaultLedger { rows: Vec::new(), note: "ledger".to_string() }
    }

    /// 一次事件：判定入账。
    pub fn feed(&mut self, sc: &Scenario) -> SiEvent {
        let ev = judge(sc);
        let tag = if ev.code == SI_USER { "user" } else { "hw" };
        self.rows.push((ev.clone(), tag.to_string()));
        ev
    }

    /// 重放核：逐行与权威同判。
    pub fn replay_ok(&self, scs: &[Scenario]) -> bool {
        let mut ok = true;
        if self.rows.len() != scs.len() {
            return false;
        }
        let mut i = 0usize;
        while i < scs.len() {
            if self.rows[i].0 != judge(&scs[i]) {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 源分类核：hw 行必同步源、user 行必异步源。
    pub fn classify_ok(&self, scs: &[Scenario]) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (_ev, tag) = &self.rows[i];
            let want = if scs[i].src == Source::Sync { "hw" } else { "user" };
            if *tag != want {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 SiEvent。
pub fn sample_matrix() -> Vec<(Scenario, SiEvent)> {
    let mut m: Vec<(Scenario, SiEvent)> = Vec::new();
    // 正：SIGSEGV 映射缺失
    m.push((
        Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 0xDEAD },
        SiEvent { sig: SIGSEGV, code: SEGV_MAPERR, addr: 0xDEAD },
    ));
    // 正：SIGSEGV 权限错
    m.push((
        Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::AccErr), addr: 0xBEEF },
        SiEvent { sig: SIGSEGV, code: SEGV_ACCERR, addr: 0xBEEF },
    ));
    // 正：SIGFPE 整除零
    m.push((
        Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::IntDiv), addr: 0 },
        SiEvent { sig: SIGFPE, code: FPE_INTDIV, addr: 0 },
    ));
    // 正：SIGFPE 浮除零
    m.push((
        Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::FltDiv), addr: 0 },
        SiEvent { sig: SIGFPE, code: FPE_FLTDIV, addr: 0 },
    ));
    // 正：SIGILL 非法操作码
    m.push((
        Scenario { sig: SIGILL, src: Source::Sync, fault: Some(Fault::IllOpc), addr: 0x4000 },
        SiEvent { sig: SIGILL, code: ILL_ILLOPC, addr: 0x4000 },
    ));
    // 反：异步 kill(SIGSEGV) 恒 SI_USER 不冒硬件码
    m.push((
        Scenario { sig: SIGSEGV, src: Source::Async, fault: Some(Fault::MapErr), addr: 0xDEAD },
        SiEvent { sig: SIGSEGV, code: SI_USER, addr: 0 },
    ));
    // 边界：异步 SIGFPE 恒 SI_USER
    m.push((
        Scenario { sig: SIGFPE, src: Source::Async, fault: Some(Fault::IntDiv), addr: 0 },
        SiEvent { sig: SIGFPE, code: SI_USER, addr: 0 },
    ));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        if &judge(sc) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：异步源冒硬件码。
pub fn faulty_async_fake(sc: &Scenario) -> SiEvent {
    let mut ev = judge(sc);
    if sc.src == Source::Async {
        ev.code = SEGV_MAPERR;
    }
    ev
}

/// 坏实现 B：同步故障漏定码（一律 SI_USER）。
pub fn faulty_sync_miss(sc: &Scenario) -> SiEvent {
    SiEvent { sig: sc.sig, code: SI_USER, addr: sc.addr }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 异步冒硬件码：权威 SI_USER、坏实现 SEGV_MAPERR → 检出
    let sc1 = Scenario { sig: SIGSEGV, src: Source::Async, fault: Some(Fault::MapErr), addr: 8 };
    got[0] = judge(&sc1) != faulty_async_fake(&sc1) && judge(&sc1).code == SI_USER;
    // 2) 同步漏定码：权威 SEGV_MAPERR、坏实现 SI_USER → 检出
    let sc2 = Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 8 };
    got[1] = judge(&sc2) != faulty_sync_miss(&sc2)
        && judge(&sc2).code == SEGV_MAPERR
        && faulty_sync_miss(&sc2).code == SI_USER;
    got
}

/// 硬件源全序核：五类同步事件各携正码、地址入账。
pub fn sync_matrix_ok() -> bool {
    let seq = [
        Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 0x10 },
        Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::AccErr), addr: 0x20 },
        Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::IntDiv), addr: 0 },
        Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::FltDiv), addr: 0 },
        Scenario { sig: SIGILL, src: Source::Sync, fault: Some(Fault::IllOpc), addr: 0x40 },
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < seq.len() {
        if !sync_code_ok(&seq[i]) {
            ok = false;
        }
        i += 1;
    }
    ok && judge(&seq[0]).addr == 0x10 && judge(&seq[4]).addr == 0x40
}

/// si_code 渲染（对账/落账用）。
pub fn code_text(code: u32) -> &'static str {
    match code {
        SI_USER => "SI_USER",
        SEGV_MAPERR => "SEGV_MAPERR",
        SEGV_ACCERR => "SEGV_ACCERR",
        FPE_INTDIV => "FPE_INTDIV",
        FPE_FLTDIV => "FPE_FLTDIV",
        ILL_ILLOPC => "ILL_ILLOPC",
        _ => "UNKNOWN",
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10826_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10826");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线故障样本全命中");

    // J2 正路径：SEGV 映射缺失携 SEGV_MAPERR
    let j2 = judge(&Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 0x8 }).code
        == SEGV_MAPERR;
    s.add("J2-MAPERR面", j2, "未映射地址必 1");

    // J3 正路径：SEGV 权限错携 SEGV_ACCERR
    let j3 = judge(&Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::AccErr), addr: 0x8 }).code
        == SEGV_ACCERR;
    s.add("J3-ACCERR面", j3, "权限错必 2");

    // J4 正路径：FPE 整除/浮除双码
    let j4 = judge(&Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::IntDiv), addr: 0 }).code
        == FPE_INTDIV
        && judge(&Scenario { sig: SIGFPE, src: Source::Sync, fault: Some(Fault::FltDiv), addr: 0 }).code
            == FPE_FLTDIV;
    s.add("J4-FPE双码", j4, "整除 1 / 浮除 2");

    // J5 反路径：异步源恒 SI_USER
    let j5 = judge(&Scenario { sig: SIGSEGV, src: Source::Async, fault: Some(Fault::MapErr), addr: 8 }).code
        == SI_USER;
    s.add("J5-异步SI_USER", j5, "kill 源不可冒硬件码");

    // J6 硬件源全序核
    s.add("J6-同步全序", sync_matrix_ok(), "五类同步事件全携正码");

    // J7 注入1 异步冒码检出
    s.add("J7-冒码检出", inj[0], "异步硬件码坏实现必分歧");

    // J8 注入2 同步漏码检出
    s.add("J8-漏码检出", inj[1], "同步 SI_USER 坏实现必分歧");

    // J9 故障账重放核
    let seq = vec![
        Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 0x10 },
        Scenario { sig: SIGSEGV, src: Source::Async, fault: Some(Fault::MapErr), addr: 0x10 },
        Scenario { sig: SIGILL, src: Source::Sync, fault: Some(Fault::IllOpc), addr: 0x40 },
    ];
    let mut lg = FaultLedger::new();
    let mut i = 0usize;
    while i < seq.len() {
        let _ = lg.feed(&seq[i]);
        i += 1;
    }
    let j9 = lg.replay_ok(&seq) && lg.classify_ok(&seq) && lg.rows.len() == 3;
    s.add("J9-故障账重放", j9, "三行账逐条同判且源分类对");

    // J10 码渲染闭集
    let j10 = code_text(SI_USER) == "SI_USER" && code_text(SEGV_MAPERR) == "SEGV_MAPERR"
        && code_text(0xFFFF) == "UNKNOWN";
    s.add("J10-码渲染闭集", j10, "渲染表完备");

    // J11 字面锚：11/8/4 三信号锚
    let j11 = SIGSEGV == 11 && SIGFPE == 8 && SIGILL == 4 && diag_text(HW_ASYNC_FAKE).len() > 0;
    s.add("J11-字面锚对平", j11, "11/8/4 三锚");

    // J12 异步地址零化：异步事件不携故障地址
    let j12 = judge(&Scenario { sig: SIGSEGV, src: Source::Async, fault: Some(Fault::MapErr), addr: 0x1234 }).addr
        == 0;
    s.add("J12-异步零址", j12, "异步事件地址清零");

    // J13 (dyn) 故障账指纹落账
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg.rows.len() as u32);
    let mut i2 = 0usize;
    while i2 < lg.rows.len() {
        h = h.wrapping_mul(31).wrapping_add(lg.rows[i2].0.code);
        i2 += 1;
    }
    for c in lg.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-故障账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10826_hw_flow() {
        let ev = judge(&Scenario { sig: SIGSEGV, src: Source::Sync, fault: Some(Fault::MapErr), addr: 0x99 });
        assert_eq!(ev.code, SEGV_MAPERR);
        assert_eq!(ev.addr, 0x99);
        let ev2 = judge(&Scenario { sig: SIGFPE, src: Source::Async, fault: Some(Fault::IntDiv), addr: 5 });
        assert_eq!(ev2.code, SI_USER);
        assert_eq!(ev2.addr, 0);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
        assert!(code_text(FPE_FLTDIV) == "FPE_FLTDIV");
    }

    #[test]
    fn unx10826_checks_green() {
        let s = run_unx10826_checks();
        assert!(s.all_passed(), "UNX-F10826 判据未全绿");
    }
}
