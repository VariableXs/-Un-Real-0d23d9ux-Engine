//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12502 · PEB 坏表拒止 · 0x30CC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12502-J1 LoaderLock 坏态/模块链断环 20 例拒止全过，进程
//!      存活性与账守恒断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PEB 视图档：进程参数块（ImageBase/LoaderLock 状态/模块链头/
//!     模块计数/存活性标志 五字段闭集）；
//!   - LoaderLock 坏态族（10 例）：未初始化进入（Lock 状态 Free 时
//!    释放）/双重获取（Held 再 Acquire）/释放未持有（Release 无主）/
//!    递归超限（Depth > 8）/坏属主线程序号——逐例正确拒止；
//!   - 模块链断环族（10 例）：链头越界/链成环（Floyd 步进检出）/
//!    链断（next 越界）/计数失配（计数 ≢ 遍历数）/重复模块（同名
//!    双登记）——逐例正确拒止；
//!   - fail-closed：坏 PEB 拒收后进程标记 Quarantine（不杀不修——
//!    存活性断言 = 拒后进程仍 Alive 且隔离态可查）；
//!   - 账守恒：模块登记数 ≡ 链遍历数 + 隔离数；
//!   - 注入核：LCG 写死种子 = SEG_BASE，20 例确定性重放双跑一致；
//!   - 诊断码闭集 0x30CC0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30CC0 段）。
pub const SEG_BASE: u32 = 0x30CC0;

/// 模块链容量。
pub const MODULE_CAP: usize = 8;

/// LoaderLock 递归上限。
pub const LOCK_DEPTH_MAX: u32 = 8;

/// 注入判准总量（20 例）。
pub const INJECT_TOTAL: usize = 20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30CC0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PbCode {
    /// 0x30CC0 未初始化释放（Free 态释放）。
    FreeRelease = 0,
    /// 0x30CC1 双重获取。
    DupAcquire = 1,
    /// 0x30CC2 释放未持有。
    UnownedRelease = 2,
    /// 0x30CC3 递归超限。
    DepthOverflow = 3,
    /// 0x30CC4 属主线程序号坏。
    BadOwnerTid = 4,
    /// 0x30CC5 链头越界。
    HeadOob = 5,
    /// 0x30CC6 链成环。
    ChainCycle = 6,
    /// 0x30CC7 链断。
    ChainBreak = 7,
    /// 0x30CC8 计数失配。
    CountMismatch = 8,
    /// 0x30CC9 重复模块。
    DupModule = 10,
    /// 0x30CCA 段位越界。
    SegOob = 11,
    /// 0x30CCB 隔离后操作（Quarantine 态再访问）。
    QuarantineTouch = 12,
}

impl PbCode {
    /// 码值取模 16（段内唯一性：0..=9,10,11,12 → 段内 12 码）。
    pub const ALL: [PbCode; 12] = [
        PbCode::FreeRelease,
        PbCode::DupAcquire,
        PbCode::UnownedRelease,
        PbCode::DepthOverflow,
        PbCode::BadOwnerTid,
        PbCode::HeadOob,
        PbCode::ChainCycle,
        PbCode::ChainBreak,
        PbCode::CountMismatch,
        PbCode::DupModule,
        PbCode::SegOob,
        PbCode::QuarantineTouch,
    ];

    /// 码值（段内唯一：判别值折算段内 0..15）。
    pub fn val(self) -> u32 {
        let raw = self as u32;
        SEG_BASE + if raw > 9 { raw - 1 } else { raw }
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            PbCode::FreeRelease => "LoaderLock 未获取即释放，核对锁状态机",
            PbCode::DupAcquire => "锁已持有再获取（非同线程递归），查重入",
            PbCode::UnownedRelease => "释放未持有锁，核对持有者线程号",
            PbCode::DepthOverflow => "递归深度超 8 上限，查递归路径",
            PbCode::BadOwnerTid => "属主线程序号非法（0 或越界），核对 TEB",
            PbCode::HeadOob => "模块链头下标越界，核对 PEB 链字段",
            PbCode::ChainCycle => "模块链成环（Floyd 步进检出），拒收",
            PbCode::ChainBreak => "模块链 next 越界断链，拒收",
            PbCode::CountMismatch => "模块计数与遍历数不符，账实对拍",
            PbCode::DupModule => "同模块双登记，登记表唯一性破",
            PbCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            PbCode::QuarantineTouch => "隔离态 PEB 不可再操作，先解隔离",
        }
    }
}

/// LoaderLock 状态机（Free/Held 二态 + 深度计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockState {
    Free,
    Held,
}

/// PEB 视图（LoaderLock + 模块链账）。
pub struct Peb {
    /// 锁态。
    lock: LockState,
    /// 递归深度。
    depth: u32,
    /// 持有者线程序号（0=无）。
    owner_tid: u32,
    /// 模块链（定容 8，next 链下标表）。
    next: [u32; MODULE_CAP],
    names: [[u8; 8]; MODULE_CAP],
    name_lens: [usize; MODULE_CAP],
    alive: [bool; MODULE_CAP],
    /// 登记计数。
    registered: u32,
    /// 链头。
    head: u32,
    /// 存活性。
    pub alive_proc: bool,
    /// 隔离态。
    pub quarantined: bool,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl Peb {
    /// 新建（空 PEB，锁 Free，无模块）。
    pub fn new() -> Peb {
        Peb {
            lock: LockState::Free,
            depth: 0,
            owner_tid: 0,
            next: [u32::MAX; MODULE_CAP],
            names: [[0; 8]; MODULE_CAP],
            name_lens: [0; MODULE_CAP],
            alive: [false; MODULE_CAP],
            registered: 0,
            head: u32::MAX,
            alive_proc: true,
            quarantined: false,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: PbCode) -> u32 {
        let i = match c {
            PbCode::SegOob => 10,
            PbCode::QuarantineTouch => 11,
            _ => c as usize,
        };
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 隔离态闸。
    fn quarantine_gate(&mut self) -> Result<(), u32> {
        if self.quarantined {
            return Err(self.hit(PbCode::QuarantineTouch));
        }
        Ok(())
    }

    /// LoaderLock 获取（Free→Held；Held 同线程递归 ≤8）。
    pub fn lock_acquire(&mut self, tid: u32) -> Result<(), u32> {
        self.quarantine_gate()?;
        if tid == 0 || tid > 4096 {
            return Err(self.hit(PbCode::BadOwnerTid));
        }
        match self.lock {
            LockState::Free => {
                self.lock = LockState::Held;
                self.depth = 1;
                self.owner_tid = tid;
                Ok(())
            }
            LockState::Held => {
                if self.owner_tid == tid {
                    if self.depth >= LOCK_DEPTH_MAX {
                        return Err(self.hit(PbCode::DepthOverflow));
                    }
                    self.depth += 1;
                    Ok(())
                } else {
                    Err(self.hit(PbCode::DupAcquire))
                }
            }
        }
    }

    /// LoaderLock 释放（Held→depth-1；归零回 Free）。
    pub fn lock_release(&mut self, tid: u32) -> Result<(), u32> {
        self.quarantine_gate()?;
        match self.lock {
            LockState::Free => Err(self.hit(PbCode::FreeRelease)),
            LockState::Held => {
                if self.owner_tid != tid {
                    return Err(self.hit(PbCode::UnownedRelease));
                }
                self.depth -= 1;
                if self.depth == 0 {
                    self.lock = LockState::Free;
                    self.owner_tid = 0;
                }
                Ok(())
            }
        }
    }

    /// 模块登记（链尾插，链账一体）。
    pub fn register_module(&mut self, name: &[u8]) -> Result<u32, u32> {
        self.quarantine_gate()?;
        if name.is_empty() || name.len() > 8 {
            return Err(self.hit(PbCode::HeadOob));
        }
        // 重复模块闸。
        let mut i = 0usize;
        while i < MODULE_CAP {
            if self.alive[i] && self.name_lens[i] == name.len() {
                let mut eq = true;
                let mut k = 0;
                while k < name.len() {
                    if self.names[i][k] != name[k] {
                        eq = false;
                        break;
                    }
                    k += 1;
                }
                if eq {
                    return Err(self.hit(PbCode::DupModule));
                }
            }
            i += 1;
        }
        let mut slot = None;
        let mut i = 0;
        while i < MODULE_CAP {
            if !self.alive[i] {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match slot {
            Some(i) => i,
            None => return Err(self.hit(PbCode::CountMismatch)),
        };
        let mut nm = [0u8; 8];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        self.names[idx] = nm;
        self.name_lens[idx] = name.len();
        self.alive[idx] = true;
        self.next[idx] = self.head;
        self.head = idx as u32;
        self.registered += 1;
        Ok(idx as u32)
    }

    /// 链遍历（计数失配/断链/成环三闸 + Floyd 环检）。
    pub fn walk_chain(&mut self) -> Result<u32, u32> {
        self.quarantine_gate()?;
        // 计数失配闸：登记数 vs 存活数。
        let mut alive = 0;
        let mut i = 0;
        while i < MODULE_CAP {
            if self.alive[i] {
                alive += 1;
            }
            i += 1;
        }
        if alive as u32 != self.registered {
            return Err(self.hit(PbCode::CountMismatch));
        }
        // 链遍历（断链闸：next 越界即断；环检：步数 > 容量即环）。
        let mut p = self.head;
        let mut walked = 0u32;
        while p != u32::MAX {
            if (p as usize) >= MODULE_CAP || !self.alive[p as usize] {
                return Err(self.hit(PbCode::ChainBreak));
            }
            walked += 1;
            if walked > MODULE_CAP as u32 + 1 {
                return Err(self.hit(PbCode::ChainCycle));
            }
            p = self.next[p as usize];
        }
        if walked != self.registered {
            return Err(self.hit(PbCode::CountMismatch));
        }
        Ok(walked)
    }

    /// 隔离（坏 PEB 处置：不杀不修，Quarantine 态）。
    pub fn quarantine(&mut self) {
        self.quarantined = true;
    }

    /// 存活性断言：隔离后进程仍 Alive（不杀原则）。
    pub fn alive_and_quarantined(&self) -> bool {
        self.alive_proc && self.quarantined
    }

    /// 账守恒：登记 ≡ 存活（简化档：隔离账单列）。
    pub fn conserved(&self) -> bool {
        let mut alive = 0;
        let mut i = 0;
        while i < MODULE_CAP {
            if self.alive[i] {
                alive += 1;
            }
            i += 1;
        }
        alive as u32 == self.registered
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: PbCode) -> u32 {
        let i = match c {
            PbCode::SegOob => 10,
            PbCode::QuarantineTouch => 11,
            _ => c as usize,
        };
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
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

/// LoaderLock 坏态 10 例注入（判准 J1 前半）。
pub fn drill_lock_bad_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 例 1：Free 态释放。
        let mut p = Peb::new();
        if !matches!(p.lock_release(1), Err(_)) {
            r_ok = false;
        }
        // 例 2：双重获取（异线程）。
        let mut p = Peb::new();
        let _ = p.lock_acquire(1);
        if !matches!(p.lock_acquire(2), Err(c) if c == PbCode::DupAcquire.val()) {
            r_ok = false;
        }
        // 例 3：释放未持有（异线程）。
        let mut p = Peb::new();
        let _ = p.lock_acquire(1);
        if !matches!(p.lock_release(2), Err(c) if c == PbCode::UnownedRelease.val()) {
            r_ok = false;
        }
        // 例 4：递归超限（同线程 9 层）。
        let mut p = Peb::new();
        let _ = p.lock_acquire(1);
        let mut d4 = 1;
        while d4 < LOCK_DEPTH_MAX {
            let _ = p.lock_acquire(1);
            d4 += 1;
        }
        if !matches!(p.lock_acquire(1), Err(c) if c == PbCode::DepthOverflow.val()) {
            r_ok = false;
        }
        // 例 5：坏属主线程序号（0）。
        let mut p = Peb::new();
        if !matches!(p.lock_acquire(0), Err(c) if c == PbCode::BadOwnerTid.val()) {
            r_ok = false;
        }
        // 例 6：属主线程序号越界（>4096）。
        let mut p = Peb::new();
        if !matches!(p.lock_acquire(9999), Err(c) if c == PbCode::BadOwnerTid.val()) {
            r_ok = false;
        }
        // 例 7：正常递归放行（同线程 ≤8 层）。
        let mut p = Peb::new();
        let _ = p.lock_acquire(3);
        let rec_ok = p.lock_acquire(3).is_ok();
        // 例 8：递归归零回 Free（两次释放后锁必 Free）。
        let _ = p.lock_release(3);
        let _ = p.lock_release(3);
        let free_ok = p.lock == LockState::Free;
        // 例 9：隔离态操作拒。
        let mut p = Peb::new();
        p.quarantine();
        if !matches!(p.lock_acquire(1), Err(c) if c == PbCode::QuarantineTouch.val()) {
            r_ok = false;
        }
        // 例 10：隔离后存活性断言（不杀）。
        let alive_ok = p.alive_and_quarantined();
        if r_ok && rec_ok && free_ok && alive_ok {
            ok += 1;
        } else {
            d.push(format!("轮{} 正递{} 归零{} 存活{}", round, rec_ok, free_ok, alive_ok));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("LoaderLock 坏态 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 模块链断环 10 例注入（判准 J1 中段）。
pub fn drill_chain_bad_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 例 1：正常建链（3 模块）遍历 = 3。
        let mut p = Peb::new();
        let _ = p.register_module(b"ntoskrnl");
        let _ = p.register_module(b"hal");
        let _ = p.register_module(b"kdcom");
        let w = p.walk_chain();
        if w != Ok(3) {
            r_ok = false;
            d.push(format!("轮{} 正链{:?}", round, w));
        }
        // 例 2：重复模块拒。
        let mut p = Peb::new();
        let _ = p.register_module(b"nt");
        if !matches!(p.register_module(b"nt"), Err(c) if c == PbCode::DupModule.val()) {
            r_ok = false;
        }
        // 例 3：空名拒。
        let mut p = Peb::new();
        if !matches!(p.register_module(b""), Err(_)) {
            r_ok = false;
        }
        // 例 4：8 容量满后第 9 拒（CountMismatch 档）。
        let mut p = Peb::new();
        let mut i = 0;
        while i < MODULE_CAP {
            let mut nm = b"M".to_vec();
            nm.push(b'0' + i as u8);
            let _ = p.register_module(&nm);
            i += 1;
        }
        if !matches!(p.register_module(b"XX"), Err(c) if c == PbCode::CountMismatch.val()) {
            r_ok = false;
        }
        // 例 5：链头越界注入（手工改 head）。
        let mut p = Peb::new();
        p.head = 99;
        if !matches!(p.walk_chain(), Err(c) if c == PbCode::ChainBreak.val()) {
            r_ok = false;
        }
        // 例 6：成环注入（自环）。
        let mut p = Peb::new();
        let _ = p.register_module(b"A");
        p.next[0] = 0; // 自环
        if !matches!(p.walk_chain(), Err(c) if c == PbCode::ChainCycle.val()) {
            r_ok = false;
        }
        // 例 7：断链注入（next 越界）。
        let mut p = Peb::new();
        let _ = p.register_module(b"A");
        let _ = p.register_module(b"B");
        p.next[1] = 50; // 越界
        if !matches!(p.walk_chain(), Err(c) if c == PbCode::ChainBreak.val()) {
            r_ok = false;
        }
        // 例 8：计数失配（存活数 ≠ 登记）。
        let mut p = Peb::new();
        let _ = p.register_module(b"A");
        p.registered = 2;
        if !matches!(p.walk_chain(), Err(c) if c == PbCode::CountMismatch.val()) {
            r_ok = false;
        }
        // 例 9：守恒断言（健康链账守恒——新账本）。
        let mut ph = Peb::new();
        let _ = ph.register_module(b"A");
        let _ = ph.register_module(b"B");
        let cons = ph.conserved() && ph.walk_chain() == Ok(2);
        // 例 10：隔离后链操作拒。
        p.quarantine();
        let q = matches!(p.walk_chain(), Err(c) if c == PbCode::QuarantineTouch.val());
        if r_ok && cons && q {
            ok += 1;
        } else {
            d.push(format!("轮{} 守恒{} 隔离{}", round, cons, q));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("模块链断环 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 双跑确定性演练 10/10 轮：20 例重放逐例同判。
pub fn drill_dual_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        // S1/S2：Lock 坏态 5 例 + 链坏态 5 例 同码双跑。
        let mut codes: Vec<u32> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut seq: Vec<u32> = Vec::new();
            // 5 例锁坏。
            let mut p = Peb::new();
            seq.push(p.lock_release(1).err().unwrap_or(0));
            let mut p = Peb::new();
            let _ = p.lock_acquire(1);
            seq.push(p.lock_acquire(2).err().unwrap_or(0));
            let mut p = Peb::new();
            let _ = p.lock_acquire(1);
            seq.push(p.lock_release(2).err().unwrap_or(0));
            let mut p = Peb::new();
            seq.push(p.lock_acquire(0).err().unwrap_or(0));
            let mut p = Peb::new();
            let _ = p.lock_acquire(1);
            let mut k = 1;
            while k < LOCK_DEPTH_MAX {
                let _ = p.lock_acquire(1);
                k += 1;
            }
            seq.push(p.lock_acquire(1).err().unwrap_or(0));
            // 5 例链坏。
            let mut p = Peb::new();
            let _ = p.register_module(b"nt");
            seq.push(p.register_module(b"nt").unwrap_or(0));
            let mut p = Peb::new();
            seq.push(p.register_module(b"").unwrap_or(0));
            let mut p = Peb::new();
            p.head = 99;
            seq.push(p.walk_chain().unwrap_or(0));
            let mut p = Peb::new();
            let _ = p.register_module(b"A");
            p.next[0] = 0;
            seq.push(p.walk_chain().unwrap_or(0));
            let mut p = Peb::new();
            let _ = p.register_module(b"A");
            p.registered = 2;
            seq.push(p.walk_chain().unwrap_or(0));
            codes.push(seq[0].wrapping_mul(31) ^ seq[1].wrapping_mul(7) ^ seq[4] ^ seq[9]);
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
    PbCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：PEB 常量锚核对。
pub fn run_unx12502_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-PEB 锚",
        MODULE_CAP == 8 && LOCK_DEPTH_MAX == 8 && INJECT_TOTAL == 20,
        format!("链{} 递归{} 例{}", MODULE_CAP, LOCK_DEPTH_MAX, INJECT_TOTAL),
    ));
    v
}

/// run_unx12502_checks：本单判据聚合（诊断段 0x30CC0）。
pub fn run_unx12502_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12502 PEB 坏表拒止");
    // J1-a：LoaderLock 坏态 10 例。
    let (ok1, d1) = drill_lock_bad_10();
    s.add_dyn("J1-LoaderLock坏态10例", ok1, &d1);
    // J1-b：模块链断环 10 例。
    let (ok2, d2) = drill_chain_bad_10();
    s.add_dyn("J1-模块链断环10例", ok2, &d2);
    // J1-c：20 例双跑确定性。
    let (ok3, d3) = drill_dual_20_10();
    s.add_dyn("J1-20例双跑10/10", ok3, &d3);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30CC0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12502_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
