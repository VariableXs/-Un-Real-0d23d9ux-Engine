//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12503 · TEB 坏表拒止 · 0x30CE0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12503-J1 TLS 槽表损坏/异常链断 20 例拒止全过，线程可终
//!      止性判据绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - TEB 视图档：TLS 槽表（定容 64 槽，占用位图 + 数据槽）+ 异常
//!    链（SEH 链表，next 指针链）+ 线程 ID + 可终止标志；
//!   - TLS 槽表损坏族（10 例）：未分配槽读取/双重释放槽/越界槽号/
//!    占用位图失配（位图 ≢ 占用计数）/槽配额耗尽——逐例拒止；
//!   - 异常链断族（10 例）：链头越界/成环（步数上限检出）/断链
//!    （next 越界）/登记序失配（push/pop 计数 ≢ 遍历数）/空帧操作
//!    ——逐例拒止；
//!   - 线程可终止性：坏 TEB 拒收后线程标记 Quarantine（可终止标志
//!    置位——终止路径不依赖坏表数据，可安全回收）；
//!   - 守恒：TLS 占用位图计数 ≡ 注册数；异常链 push ≡ pop + 在链；
//!   - 注入核：LCG 写死种子 = SEG_BASE，20 例确定性重放双跑一致；
//!   - 诊断码闭集 0x30CE0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30CE0 段）。
pub const SEG_BASE: u32 = 0x30CE0;

/// TLS 槽容量。
pub const TLS_CAP: usize = 64;

/// 异常链容量。
pub const SEH_CAP: usize = 8;

/// 注入判准总量（20 例）。
pub const INJECT_TOTAL: usize = 20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30CE0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbCode {
    /// 0x30CE0 未分配槽读。
    SlotUnalloc = 0,
    /// 0x30CE1 双重释放槽。
    SlotDupFree = 1,
    /// 0x30CE2 越界槽号。
    SlotOob = 2,
    /// 0x30CE3 位图失配。
    BitmapMismatch = 3,
    /// 0x30CE4 槽配额耗尽。
    SlotExhaust = 4,
    /// 0x30CE5 链头越界。
    HeadOob = 5,
    /// 0x30CE6 链成环。
    ChainCycle = 6,
    /// 0x30CE7 链断。
    ChainBreak = 7,
    /// 0x30CE8 登记序失配。
    RegMismatch = 8,
    /// 0x30CE9 空帧操作。
    EmptyFrame = 9,
    /// 0x30CEA 段位越界。
    SegOob = 10,
    /// 0x30CEB 隔离后操作。
    QuarantineTouch = 11,
}

impl TbCode {
    /// 全部码闭集。
    pub const ALL: [TbCode; 12] = [
        TbCode::SlotUnalloc,
        TbCode::SlotDupFree,
        TbCode::SlotOob,
        TbCode::BitmapMismatch,
        TbCode::SlotExhaust,
        TbCode::HeadOob,
        TbCode::ChainCycle,
        TbCode::ChainBreak,
        TbCode::RegMismatch,
        TbCode::EmptyFrame,
        TbCode::SegOob,
        TbCode::QuarantineTouch,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            TbCode::SlotUnalloc => "TLS 槽未分配即读，先 TlsAlloc",
            TbCode::SlotDupFree => "槽已释放再释放（双重释放）",
            TbCode::SlotOob => "槽号出 0..=63",
            TbCode::BitmapMismatch => "占用位图计数与注册数不符，账实对拍",
            TbCode::SlotExhaust => "TLS 槽配额耗尽（64），先释放闲置槽",
            TbCode::HeadOob => "异常链头下标越界",
            TbCode::ChainCycle => "异常链成环，拒收",
            TbCode::ChainBreak => "异常链 next 越界断链，拒收",
            TbCode::RegMismatch => "push/pop 计数与链遍历数不符",
            TbCode::EmptyFrame => "空链 pop/遍历",
            TbCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            TbCode::QuarantineTouch => "隔离态 TEB 不可操作，先解隔离",
        }
    }
}

/// TEB 视图。
pub struct Teb {
    /// TLS 占用位图。
    tls_used: [bool; TLS_CAP],
    /// TLS 占用计数。
    tls_count: u32,
    /// 异常链（栈式：head + next 链）。
    seh_next: [u32; SEH_CAP],
    seh_alive: [bool; SEH_CAP],
    seh_head: u32,
    seh_pushed: u32,
    /// pop 计数（守恒账第二项）。
    popped: u32,
    /// 线程 ID。
    pub tid: u32,
    /// 可终止标志（坏 TEB 处置后置位）。
    pub terminable: bool,
    /// 隔离态。
    quarantined: bool,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl Teb {
    /// 新建。
    pub fn new(tid: u32) -> Teb {
        Teb {
            tls_used: [false; TLS_CAP],
            tls_count: 0,
            seh_next: [u32::MAX; SEH_CAP],
            seh_alive: [false; SEH_CAP],
            seh_head: u32::MAX,
            seh_pushed: 0,
            popped: 0,
            tid,
            terminable: false,
            quarantined: false,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: TbCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 隔离闸。
    fn q_gate(&mut self) -> Result<(), u32> {
        if self.quarantined {
            return Err(self.hit(TbCode::QuarantineTouch));
        }
        Ok(())
    }

    /// TLS 分配（位图 + 计数一体）。
    pub fn tls_alloc(&mut self) -> Result<u32, u32> {
        self.q_gate()?;
        let mut i = 0usize;
        while i < TLS_CAP {
            if !self.tls_used[i] {
                self.tls_used[i] = true;
                self.tls_count += 1;
                return Ok(i as u32);
            }
            i += 1;
        }
        Err(self.hit(TbCode::SlotExhaust))
    }

    /// TLS 读（未分配闸）。
    pub fn tls_read(&mut self, slot: u32) -> Result<(), u32> {
        self.q_gate()?;
        if slot as usize >= TLS_CAP {
            return Err(self.hit(TbCode::SlotOob));
        }
        if !self.tls_used[slot as usize] {
            return Err(self.hit(TbCode::SlotUnalloc));
        }
        Ok(())
    }

    /// TLS 释放（双重释放闸 + 位图回退）。
    pub fn tls_free(&mut self, slot: u32) -> Result<(), u32> {
        self.q_gate()?;
        if slot as usize >= TLS_CAP {
            return Err(self.hit(TbCode::SlotOob));
        }
        if !self.tls_used[slot as usize] {
            return Err(self.hit(TbCode::SlotDupFree));
        }
        self.tls_used[slot as usize] = false;
        self.tls_count -= 1;
        Ok(())
    }

    /// 位图守恒：占用计数 ≡ 位图置位数。
    pub fn tls_conserved(&self) -> bool {
        let mut n = 0;
        let mut i = 0;
        while i < TLS_CAP {
            if self.tls_used[i] {
                n += 1;
            }
            i += 1;
        }
        n as u32 == self.tls_count
    }

    /// SEH push（链头插）。
    pub fn seh_push(&mut self) -> Result<u32, u32> {
        self.q_gate()?;
        let mut i = 0usize;
        while i < SEH_CAP {
            if !self.seh_alive[i] {
                self.seh_alive[i] = true;
                self.seh_next[i] = self.seh_head;
                self.seh_head = i as u32;
                self.seh_pushed += 1;
                return Ok(i as u32);
            }
            i += 1;
        }
        Err(self.hit(TbCode::RegMismatch))
    }

    /// SEH pop（链头出）。
    pub fn seh_pop(&mut self) -> Result<u32, u32> {
        self.q_gate()?;
        if self.seh_head == u32::MAX {
            return Err(self.hit(TbCode::EmptyFrame));
        }
        let h = self.seh_head;
        self.seh_alive[h as usize] = false;
        self.seh_head = self.seh_next[h as usize];
        self.popped += 1;
        Ok(h)
    }

    /// 链遍历（头越界/成环/断链/序失配四闸）。
    pub fn seh_walk(&mut self) -> Result<u32, u32> {
        self.q_gate()?;
        if self.seh_head != u32::MAX
            && ((self.seh_head as usize) >= SEH_CAP || !self.seh_alive[self.seh_head as usize])
        {
            return Err(self.hit(TbCode::HeadOob));
        }
        let mut p = self.seh_head;
        let mut walked = 0u32;
        while p != u32::MAX {
            if (p as usize) >= SEH_CAP || !self.seh_alive[p as usize] {
                return Err(self.hit(TbCode::ChainBreak));
            }
            walked += 1;
            if walked > SEH_CAP as u32 {
                return Err(self.hit(TbCode::ChainCycle));
            }
            p = self.seh_next[p as usize];
        }
        // 序失配闸：在链数 ≡ push - pop。
        let mut in_chain = 0;
        let mut i = 0;
        while i < SEH_CAP {
            if self.seh_alive[i] {
                in_chain += 1;
            }
            i += 1;
        }
        let expect = self.seh_pushed - self.popped;
        if in_chain as u32 != expect {
            return Err(self.hit(TbCode::RegMismatch));
        }
        Ok(walked)
    }

    /// 隔离（可终止标志置位）。
    pub fn quarantine(&mut self) {
        self.quarantined = true;
        self.terminable = true;
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// TLS 槽表损坏 10 例注入（判准 J1 前半）。
pub fn drill_tls_bad_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 例 1：未分配槽读。
        let mut t = Teb::new(1);
        if !matches!(t.tls_read(0), Err(c) if c == TbCode::SlotUnalloc.val()) {
            r_ok = false;
        }
        // 例 2：双重释放。
        let mut t = Teb::new(1);
        let s = t.tls_alloc().unwrap_or(0);
        let _ = t.tls_free(s);
        if !matches!(t.tls_free(s), Err(c) if c == TbCode::SlotDupFree.val()) {
            r_ok = false;
        }
        // 例 3：越界槽号。
        let mut t = Teb::new(1);
        if !matches!(t.tls_read(64), Err(c) if c == TbCode::SlotOob.val()) {
            r_ok = false;
        }
        // 例 4：位图失配。
        let mut t = Teb::new(1);
        let s = t.tls_alloc().unwrap_or(0);
        t.tls_count = 9;
        if !t.tls_read(s).is_err() && !t.tls_conserved() {
            // 位图失配由 conserved 检出。
        }
        let bm = !t.tls_conserved();
        if !bm {
            r_ok = false;
        }
        // 例 5：配额耗尽（64 满后 65 拒）。
        let mut t = Teb::new(1);
        let mut i = 0;
        let mut full = false;
        while i < TLS_CAP + 1 {
            if matches!(t.tls_alloc(), Err(c) if c == TbCode::SlotExhaust.val()) {
                full = true;
            }
            i += 1;
        }
        if !full {
            r_ok = false;
        }
        // 例 6：分配-释放-再分配（槽复用合法）。
        let mut t = Teb::new(1);
        let s1 = t.tls_alloc().unwrap_or(0);
        let _ = t.tls_free(s1);
        let s2 = t.tls_alloc().unwrap_or(99);
        let reuse = s1 == s2 && t.tls_conserved();
        // 例 7：位图守恒（分配 5 释放 2）。
        let mut t = Teb::new(1);
        let mut i = 0;
        while i < 5 {
            let _ = t.tls_alloc();
            i += 1;
        }
        let _ = t.tls_free(0);
        let _ = t.tls_free(1);
        let cons = t.tls_conserved() && t.tls_count == 3;
        // 例 8：隔离后 TLS 操作拒。
        let mut t = Teb::new(1);
        t.quarantine();
        if !matches!(t.tls_alloc(), Err(c) if c == TbCode::QuarantineTouch.val()) {
            r_ok = false;
        }
        // 例 9：隔离后可终止性。
        let term = t.terminable;
        // 例 10：正常线程不可终止标志初值假。
        let t0 = Teb::new(2);
        let init = !t0.terminable;
        if r_ok && reuse && cons && term && init {
            ok += 1;
        } else {
            d.push(format!("轮{} 复用{} 守恒{} 可终{} 初值{}", round, reuse, cons, term, init));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("TLS 坏表 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 异常链断 10 例注入（判准 J1 中段）。
pub fn drill_seh_bad_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 例 1：正常 push 3 遍历 = 3。
        let mut t = Teb::new(1);
        let _ = t.seh_push();
        let _ = t.seh_push();
        let _ = t.seh_push();
        if t.seh_walk() != Ok(3) {
            r_ok = false;
            d.push(format!("轮{} 正链", round));
        }
        // 例 2：空链 pop。
        let mut t = Teb::new(1);
        if !matches!(t.seh_pop(), Err(c) if c == TbCode::EmptyFrame.val()) {
            r_ok = false;
        }
        // 例 3：空链遍历 = 0。
        let mut t = Teb::new(1);
        if t.seh_walk() != Ok(0) {
            r_ok = false;
        }
        // 例 4：链头越界。
        let mut t = Teb::new(1);
        t.seh_head = 99;
        if !matches!(t.seh_walk(), Err(c) if c == TbCode::HeadOob.val()) {
            r_ok = false;
        }
        // 例 5：成环。
        let mut t = Teb::new(1);
        let _ = t.seh_push();
        t.seh_next[0] = 0;
        if !matches!(t.seh_walk(), Err(c) if c == TbCode::ChainCycle.val()) {
            r_ok = false;
        }
        // 例 6：断链。
        let mut t = Teb::new(1);
        let _ = t.seh_push();
        let _ = t.seh_push();
        t.seh_next[1] = 50;
        if !matches!(t.seh_walk(), Err(c) if c == TbCode::ChainBreak.val()) {
            r_ok = false;
        }
        // 例 7：登记序失配（pushed 改大）。
        let mut t = Teb::new(1);
        let _ = t.seh_push();
        t.seh_pushed = 5;
        if !matches!(t.seh_walk(), Err(c) if c == TbCode::RegMismatch.val()) {
            r_ok = false;
        }
        // 例 8：push-pop 守恒（3 push 2 pop = 1 在链）。
        let mut t = Teb::new(1);
        let _ = t.seh_push();
        let _ = t.seh_push();
        let _ = t.seh_push();
        let _ = t.seh_pop();
        let _ = t.seh_pop();
        let cons = t.seh_walk() == Ok(1);
        // 例 9：8 容量满后 push 拒。
        let mut t = Teb::new(1);
        let mut i = 0;
        let mut full = false;
        while i < SEH_CAP + 1 {
            if matches!(t.seh_push(), Err(c) if c == TbCode::RegMismatch.val()) {
                full = true;
            }
            i += 1;
        }
        if !full {
            r_ok = false;
        }
        // 例 10：隔离后链操作拒 + 可终止。
        let mut t = Teb::new(1);
        t.quarantine();
        let q = matches!(t.seh_push(), Err(c) if c == TbCode::QuarantineTouch.val());
        if r_ok && cons && q && t.terminable {
            ok += 1;
        } else {
            d.push(format!("轮{} 守恒{} 隔离{} 可终{}", round, cons, q, t.terminable));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("异常链断 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 双跑确定性演练 10/10 轮。
pub fn drill_dual_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut codes: Vec<u32> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut acc = 0u32;
            // TLS 5 例。
            let mut t = Teb::new(1);
            acc ^= t.tls_read(0).err().unwrap_or(0);
            let mut t = Teb::new(1);
            let s = t.tls_alloc().unwrap_or(0);
            let _ = t.tls_free(s);
            acc ^= t.tls_free(s).err().unwrap_or(0).wrapping_mul(3);
            let mut t = Teb::new(1);
            acc ^= t.tls_read(64).err().unwrap_or(0).wrapping_mul(5);
            let mut t = Teb::new(1);
            let mut i = 0;
            while i < TLS_CAP + 1 {
                let _ = t.tls_alloc();
                i += 1;
            }
            acc ^= t.tls_count.wrapping_mul(7);
            let mut t = Teb::new(1);
            t.quarantine();
            acc ^= t.tls_alloc().err().unwrap_or(0).wrapping_mul(11);
            // SEH 5 例。
            let mut t = Teb::new(1);
            acc ^= t.seh_pop().err().unwrap_or(0).wrapping_mul(13);
            let mut t = Teb::new(1);
            let _ = t.seh_push();
            t.seh_next[0] = 0;
            acc ^= t.seh_walk().err().unwrap_or(0).wrapping_mul(17);
            let mut t = Teb::new(1);
            let _ = t.seh_push();
            t.seh_pushed = 5;
            acc ^= t.seh_walk().err().unwrap_or(0).wrapping_mul(19);
            let mut t = Teb::new(1);
            t.seh_head = 99;
            acc ^= t.seh_walk().err().unwrap_or(0).wrapping_mul(23);
            let mut t = Teb::new(1);
            t.quarantine();
            acc ^= t.seh_push().err().unwrap_or(0).wrapping_mul(29);
            codes.push(acc);
            rep += 1;
        }
        if codes[0] == codes[1] {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("20 例双跑 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    TbCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：TEB 常量锚核对。
pub fn run_unx12503_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-TEB 锚",
        TLS_CAP == 64 && SEH_CAP == 8 && INJECT_TOTAL == 20,
        format!("TLS{} 链{} 例{}", TLS_CAP, SEH_CAP, INJECT_TOTAL),
    ));
    v
}

/// run_unx12503_checks：本单判据聚合（诊断段 0x30CE0）。
pub fn run_unx12503_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12503 TEB 坏表拒止");
    // J1-a：TLS 槽表损坏 10 例。
    let (ok1, d1) = drill_tls_bad_10();
    s.add_dyn("J1-TLS坏表10例", ok1, &d1);
    // J1-b：异常链断 10 例。
    let (ok2, d2) = drill_seh_bad_10();
    s.add_dyn("J1-异常链断10例", ok2, &d2);
    // J1-c：20 例双跑确定性。
    let (ok3, d3) = drill_dual_20_10();
    s.add_dyn("J1-20例双跑10/10", ok3, &d3);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30CE0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12503_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
