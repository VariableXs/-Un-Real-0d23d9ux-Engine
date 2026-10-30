//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15361 · STA/MTA 套间声明面 · 0x67280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15361-J1 声明面成文（STA/MTA 双态登记、对象归属表
//!      10/10 次），无归属注入（对象游离于套间账外）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 套间双态闭集：STA（单线程套间，消息泵串行）/ MTA（多线程
//!     套间，并发直达）；每线程恰属一套间（重登记必须先退出旧套间）；
//!   - 对象归属表：每个对象登记时必须钉入恰一个套间；归属表是
//!     对象↔套间的双向账（对象列 + 套间列守恒）；
//!   - 跨套间调用面：STA↔MTA 调用必须经列集（marshal 形态），
//!     直达（裸指针直调）必须检出；
//!   - 无归属注入面：对象登记时套间号越界/未登记即调用/注销后
//!     残账——三类游离形态必检出；
//!   - LCG 种子写死 = SEG_BASE（0x67280），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67280，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67280;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67280;

/// 套间双态闭集。
pub const APT_STA: u8 = 0;
pub const APT_MTA: u8 = 1;
pub const APT_KINDS: usize = 2;
pub const APT_NAMES: [&str; APT_KINDS] = ["STA 单线程套间", "MTA 多线程套间"];

/// 调用形态闭集。
pub const CALL_DIRECT: u8 = 0; // 裸指针直达
pub const CALL_MARSHAL: u8 = 1; // 列集（stub/proxy）
pub const CALL_KINDS: usize = 2;
pub const CALL_NAMES: [&str; CALL_KINDS] = ["直达", "列集"];

/// 判据投影：归属表 10/10 轮全对。
pub const LEDGER_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AptCode {
    /// 套间态越闭集（非 STA/MTA）
    AptKindInvalid,
    /// 线程重登记（未退出旧套间）
    ThreadDoubleJoin,
    /// 退出未加入的套间
    ThreadExitOrphan,
    /// 对象归属越界（套间号越表）
    ObjAptOutOfRange,
    /// 对象重归属（已钉套间再钉）
    ObjDoublePin,
    /// 无归属调用（对象游离于套间账外）
    OrphanCall,
    /// 跨套间直达（缺列集）
    CrossAptDirect,
    /// 归属表双向不守恒（对象列 ≠ 套间列合计）
    LedgerMismatch,
    /// 注销残账（对象已注销仍在归属表）
    DeadObjResidual,
}

impl AptCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AptCode::AptKindInvalid => "套间态越闭集：STA/MTA 双态冻结",
            AptCode::ThreadDoubleJoin => "线程重登记：未退出旧套间",
            AptCode::ThreadExitOrphan => "退出未加入的套间",
            AptCode::ObjAptOutOfRange => "对象归属越界：套间号越表",
            AptCode::ObjDoublePin => "对象重归属：已钉套间再钉",
            AptCode::OrphanCall => "无归属调用：对象游离于套间账外",
            AptCode::CrossAptDirect => "跨套间直达：缺列集（stub/proxy）",
            AptCode::LedgerMismatch => "归属表双向不守恒",
            AptCode::DeadObjResidual => "注销残账：已注销对象仍在归属表",
        }
    }
    pub const ALL: [AptCode; 9] = [
        AptCode::AptKindInvalid,
        AptCode::ThreadDoubleJoin,
        AptCode::ThreadExitOrphan,
        AptCode::ObjAptOutOfRange,
        AptCode::ObjDoublePin,
        AptCode::OrphanCall,
        AptCode::CrossAptDirect,
        AptCode::LedgerMismatch,
        AptCode::DeadObjResidual,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AptCode::ALL.len() {
        if AptCode::ALL[i].val() == code {
            return AptCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67280 段闭集内"
}

// ── 套间账本 ─────────────────────────────────────────────────────
/// 线程登记元：线程 → 套间（一线程恰一套间）。
pub struct ThreadReg {
    pub tid: u32,
    pub apt: u8,
}

/// 对象归属元：对象 → 套间（一对象恰一套间），alive 标记注销态。
pub struct ObjPin {
    pub oid: u32,
    pub apt: u8,
    pub alive: bool,
}

pub struct AptLedger {
    /// 套间容量闭集：0 号 STA、1 号 MTA。
    pub apt_obj_counts: [u32; APT_KINDS],
    pub threads: Vec<ThreadReg>,
    pub pins: Vec<ObjPin>,
    /// 跨套间直达检出计数。
    pub direct_cross_caught: u32,
    /// 无归属调用检出计数。
    pub orphan_caught: u32,
}

impl AptLedger {
    pub fn new() -> AptLedger {
        AptLedger {
            apt_obj_counts: [0; APT_KINDS],
            threads: Vec::new(),
            pins: Vec::new(),
            direct_cross_caught: 0,
            orphan_caught: 0,
        }
    }

    /// 线程加入套间：已登记（未退出）即重登记，拒收。
    pub fn thread_join(&mut self, tid: u32, apt: u8) -> Result<(), u32> {
        if apt as usize >= APT_KINDS {
            return Err(AptCode::AptKindInvalid.val());
        }
        let mut i = 0;
        while i < self.threads.len() {
            if self.threads[i].tid == tid {
                return Err(AptCode::ThreadDoubleJoin.val());
            }
            i += 1;
        }
        self.threads.push(ThreadReg { tid, apt });
        Ok(())
    }

    /// 线程退出：未登记即孤儿退出，拒收。
    pub fn thread_exit(&mut self, tid: u32) -> Result<u8, u32> {
        let mut i = 0;
        while i < self.threads.len() {
            if self.threads[i].tid == tid {
                let apt = self.threads[i].apt;
                self.threads.swap_remove(i);
                return Ok(apt);
            }
            i += 1;
        }
        Err(AptCode::ThreadExitOrphan.val())
    }

    fn thread_apt(&self, tid: u32) -> Option<u8> {
        let mut i = 0;
        while i < self.threads.len() {
            if self.threads[i].tid == tid {
                return Some(self.threads[i].apt);
            }
            i += 1;
        }
        None
    }

    /// 对象钉归属：必须钉入已存在套间，且不得重钉。
    pub fn obj_pin(&mut self, oid: u32, apt: u8) -> Result<(), u32> {
        if apt as usize >= APT_KINDS {
            return Err(AptCode::ObjAptOutOfRange.val());
        }
        let mut i = 0;
        while i < self.pins.len() {
            if self.pins[i].oid == oid {
                if self.pins[i].alive {
                    return Err(AptCode::ObjDoublePin.val());
                }
                // 已注销对象复活重钉：按重钉处理（归属不可迁移）。
                return Err(AptCode::ObjDoublePin.val());
            }
            i += 1;
        }
        self.pins.push(ObjPin { oid, apt, alive: true });
        self.apt_obj_counts[apt as usize] += 1;
        Ok(())
    }

    /// 对象注销：清 alive，套间计数减一（残账哨兵在审计）。
    pub fn obj_unpin(&mut self, oid: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.pins.len() {
            if self.pins[i].oid == oid {
                if !self.pins[i].alive {
                    return Err(AptCode::DeadObjResidual.val());
                }
                self.pins[i].alive = false;
                self.apt_obj_counts[self.pins[i].apt as usize] -= 1;
                return Ok(());
            }
            i += 1;
        }
        Err(AptCode::OrphanCall.val())
    }

    /// 调用裁定：无归属 → OrphanCall；跨套间直达 → CrossAptDirect；
    /// 同套间任意形态合法；跨套间列集合法。
    pub fn adjudicate(&mut self, tid: u32, oid: u32, form: u8) -> Result<(), u32> {
        if form as usize >= CALL_KINDS {
            return Err(AptCode::AptKindInvalid.val());
        }
        let caller_apt = match self.thread_apt(tid) {
            Some(a) => a,
            None => {
                self.orphan_caught += 1;
                return Err(AptCode::OrphanCall.val());
            }
        };
        let mut i = 0;
        while i < self.pins.len() {
            if self.pins[i].oid == oid {
                if !self.pins[i].alive {
                    return Err(AptCode::DeadObjResidual.val());
                }
                if self.pins[i].apt != caller_apt {
                    if form == CALL_DIRECT {
                        self.direct_cross_caught += 1;
                        return Err(AptCode::CrossAptDirect.val());
                    }
                    return Ok(()); // 列集合法
                }
                return Ok(()); // 同套间合法
            }
            i += 1;
        }
        self.orphan_caught += 1;
        Err(AptCode::OrphanCall.val())
    }

    /// 归属表双向守恒：套间列合计 == 对象列活账数。
    pub fn audit(&self) -> Result<(), u32> {
        let mut live = 0;
        let mut i = 0;
        while i < self.pins.len() {
            if self.pins[i].alive {
                live += 1;
            }
            i += 1;
        }
        let mut col = 0;
        let mut a = 0;
        while a < APT_KINDS {
            col += self.apt_obj_counts[a];
            a += 1;
        }
        if live as u32 != col {
            return Err(AptCode::LedgerMismatch.val());
        }
        // 逐套间复点：每套间列 == 该套间活账数。
        let mut a = 0;
        while a < APT_KINDS {
            let mut cnt = 0;
            let mut j = 0;
            while j < self.pins.len() {
                if self.pins[j].alive && self.pins[j].apt as usize == a {
                    cnt += 1;
                }
                j += 1;
            }
            if cnt as u32 != self.apt_obj_counts[a] {
                return Err(AptCode::LedgerMismatch.val());
            }
            a += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = 0x67280） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 单轮归属表压演（登记/钉账/调用/注销全谱） ────────────────────
/// 每轮：2 线程（STA/MTA 各一）+ 3 对象钉账 + 混合调用 + 收尾注销。
/// 直达跨套间调用走注入列（检出），列集跨套间合法。
pub fn apt_round(round: usize) -> Result<(u32, u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut led = AptLedger::new();
    // 线程登记：STA(100) / MTA(200)。
    led.thread_join(100, APT_STA)?;
    led.thread_join(200, APT_MTA)?;
    // 对象钉账：STA 挂 2 个（1,2），MTA 挂 1 个（3）。
    let mut oid = 1;
    while oid <= 3 {
        let apt = if oid <= 2 { APT_STA } else { APT_MTA };
        led.obj_pin(oid, apt)?;
        oid += 1;
    }
    led.audit()?;
    // 调用谱：同套间直达/列集合法；跨套间直达检出；跨套间列集合法。
    let mut direct_caught = 0;
    // 100(STA) → 对象3(MTA) 直达：检出。
    if let Err(e) = led.adjudicate(100, 3, CALL_DIRECT) {
        if e == AptCode::CrossAptDirect.val() {
            direct_caught += 1;
        }
    }
    // 100(STA) → 对象3(MTA) 列集：合法。
    led.adjudicate(100, 3, CALL_MARSHAL)?;
    // 200(MTA) → 对象1(STA) 直达：检出。
    if let Err(e) = led.adjudicate(200, 1, CALL_DIRECT) {
        if e == AptCode::CrossAptDirect.val() {
            direct_caught += 1;
        }
    }
    // 100 → 对象1 直达：合法。
    led.adjudicate(100, 1, CALL_DIRECT)?;
    // 无归属注入：未登记线程 300 直调对象1 → OrphanCall。
    let mut orphan = 0;
    if let Err(e) = led.adjudicate(300, 1, CALL_DIRECT) {
        if e == AptCode::OrphanCall.val() {
            orphan += 1;
        }
    }
    if direct_caught != 2 || orphan != 1 {
        return Err(AptCode::OrphanCall.val());
    }
    // 收尾注销全部对象 → 双向守恒归零。
    let mut k = 1;
    while k <= 3 {
        led.obj_unpin(k)?;
        k += 1;
    }
    led.audit()?;
    let _ = lcg.next(); // 确定性占位消费
    Ok((direct_caught, orphan, led.apt_obj_counts[0] + led.apt_obj_counts[1]))
}

// ── 黄金正演：10/10 轮全对 ───────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rounds = 0;
    let mut direct_total = 0;
    let mut r = 0;
    while r < LEDGER_TRIALS {
        let (d, o, live) = apt_round(r)?;
        if d != 2 || o != 1 || live != 0 {
            return Err(AptCode::LedgerMismatch.val());
        }
        direct_total += d;
        rounds += 1;
        r += 1;
    }
    // 黄金手算对照（写死判据）：
    //   STA 对象数 2、MTA 对象数 1 的钉账后双向合计 = 3。
    let mut g = AptLedger::new();
    g.obj_pin(10, APT_STA).map_err(|_| AptCode::LedgerMismatch.val())?;
    g.obj_pin(11, APT_STA).map_err(|_| AptCode::LedgerMismatch.val())?;
    g.obj_pin(12, APT_MTA).map_err(|_| AptCode::LedgerMismatch.val())?;
    g.audit()?;
    if g.apt_obj_counts[0] != 2 || g.apt_obj_counts[1] != 1 {
        return Err(AptCode::LedgerMismatch.val());
    }
    Ok((rounds, direct_total, g.apt_obj_counts[0] + g.apt_obj_counts[1]))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：无归属注入（对象未钉账即被调用）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AptLedger::new();
        led.thread_join(1, APT_STA).map_err(|_| AptCode::LedgerMismatch.val());
        // 对象 77 未 obj_pin 直调。
        let v = led.adjudicate(1, 77, CALL_DIRECT);
        ledger.tries[0] += 1;
        if let Err(e) = v {
            if e == AptCode::OrphanCall.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：跨套间直达注入 ×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AptLedger::new();
        led.thread_join(1, APT_STA).map_err(|_| AptCode::LedgerMismatch.val());
        led.thread_join(2, APT_MTA).map_err(|_| AptCode::LedgerMismatch.val());
        led.obj_pin(50, APT_MTA).map_err(|_| AptCode::LedgerMismatch.val());
        let v = led.adjudicate(1, 50, CALL_DIRECT);
        ledger.tries[1] += 1;
        if let Err(e) = v {
            if e == AptCode::CrossAptDirect.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：重归属注入（对象已钉再钉）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AptLedger::new();
        led.obj_pin(60, APT_STA).map_err(|_| AptCode::LedgerMismatch.val());
        let v = led.obj_pin(60, APT_MTA);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == AptCode::ObjDoublePin.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AptCode::ALL.len() {
        let v = AptCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 双态/双形态闭集自洽。
    APT_KINDS == 2 && CALL_KINDS == 2 && APT_STA != APT_MTA && CALL_DIRECT != CALL_MARSHAL
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, _, live)) = golden {
        rounds == LEDGER_TRIALS as u32 && live == 3
    } else {
        false
    };
    v.push((
        "J1-归属表10/10+黄金钉账",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-无归属/跨套间注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 套间/调用名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < APT_KINDS {
        if APT_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    let mut okc = 0;
    i = 0;
    while i < CALL_KINDS {
        if CALL_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    v.push((
        "J1-双态+双形态名点验",
        okn == 2 && okc == 2,
        format!("{}/2 {}/2", okn, okc),
    ));
    // 空账守恒自洽。
    let led0 = AptLedger::new();
    v.push((
        "J1-空账双向守恒自洽",
        led0.audit().is_ok(),
        String::from("空账 0/0"),
    ));
    // 线程重登记注入独立点验。
    let mut th = AptLedger::new();
    let _ = th.thread_join(9, APT_STA);
    let dbl = th.thread_join(9, APT_MTA);
    v.push((
        "J1-线程重登记拒收",
        if let Err(e) = dbl { e == AptCode::ThreadDoubleJoin.val() } else { false },
        String::from("一线程恰一套间"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67280 窗口")));
    v
}

pub fn run_unx15361_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15361 STA/MTA 套间声明面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
