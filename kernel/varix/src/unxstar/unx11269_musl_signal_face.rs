//! UNX-F11269 musl 信号面消费（静态路径）（0x1f450 段）
//!
//! 纯功能模型：musl 静态件把信号面折成「动作表 + 阻塞掩码 + 待决位图 + 交付
//! 帧栈」四件套。动作表 64 槽（1..=64），每槽记处置（默认/忽略/捕获）、
//! handler、sa_mask、flags 与 restorer；阻塞掩码与待决位图都是 64 位位图；
//! 交付帧按交付序入栈，帧内冻结「信号号 / 交付时掩码 / handler / restorer /
//! flags / 深度」六元组，供 AI-02 帧冻结接口只读取。
//!
//! 规则面遵循 Linux/musl 的硬语义：SIGKILL 与 SIGSTOP 既不可捕获也不可忽略，
//! 其屏蔽位写进去只被默默丢弃（不报错，但记账）；捕获类动作必须有非零
//! handler 与非零 restorer（静态 musl 的 SA_RESTORER 恒置），flag 只认闭集
//! 四位；sa_mask 与非 SA_NODEFER 下的隐式自身屏蔽在交付时并入掩码；解除
//! 屏蔽时待决信号按信号号升序交付；默认动作为终止的信号（如 SIGTERM）会
//! 记下 128+sig 的退出码并让实例进入终止态，此后一切 raise 皆拒。
//!
//! 拒止面：SG_ESIG（信号号 0 或 >64）、SG_EKILL（试图捕获/忽略不可屏蔽
//! 信号）、SG_EACTION（handler/restorer 缺失或 flag 越界）、SG_EMASK（how
//! 非法）、SG_EFRAME（交付帧栈满）、SG_EDEAD（终止态再 raise）、SG_EFROZEN
//! （冻结后改动作表）、SG_EMISMATCH（重放指纹不一致）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f450 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f450;

/// 信号号为零或超过 NSIG
pub const SG_ESIG: u32 = SEG_BASE + 0x01;
/// 试图捕获/忽略 SIGKILL 或 SIGSTOP
pub const SG_EKILL: u32 = SEG_BASE + 0x02;
/// 动作非法（handler/restorer 缺失或 flag 越界）
pub const SG_EACTION: u32 = SEG_BASE + 0x03;
/// sigprocmask 的 how 不在闭集
pub const SG_EMASK: u32 = SEG_BASE + 0x04;
/// 交付帧栈满
pub const SG_EFRAME: u32 = SEG_BASE + 0x05;
/// 终止态再 raise
pub const SG_EDEAD: u32 = SEG_BASE + 0x06;
/// 冻结后改动作表
pub const SG_EFROZEN: u32 = SEG_BASE + 0x07;
/// 重放指纹不一致 / 帧冻结被破坏
pub const SG_EMISMATCH: u32 = SEG_BASE + 0x08;
/// 动作表空查询（未装即查不存在的槽）
pub const SG_ESLOT: u32 = SEG_BASE + 0x09;

/// 信号总数（Linux x86-64 标准信号 1..=64）。
pub const NSIG: usize = 64;
/// 交付帧栈深（嵌套 handler 上限）。
pub const MAX_FRAMES: usize = 8;
/// SIGKILL 号。
pub const SIGKILL: u8 = 9;
/// SIGSTOP 号。
pub const SIGSTOP: u8 = 19;
/// SIGTERM 号（默认终止）。
pub const SIGTERM: u8 = 15;
/// SIGCHLD 号（默认忽略）。
pub const SIGCHLD: u8 = 17;
/// SIGCONT 号（默认忽略）。
pub const SIGCONT: u8 = 18;

/// 不可屏蔽位（SIGKILL/SIGSTOP）。
pub const UNMASKABLE: u64 = (1u64 << 8) | (1u64 << 18);
/// 默认忽略集（SIGCHLD/SIGCONT/SIGURG/SIGWINCH）。
pub const DEFAULT_IGNORE: u64 =
    (1u64 << 16) | (1u64 << 17) | (1u64 << 22) | (1u64 << 27);

/// SA_SIGINFO：handler 收 siginfo。
pub const SA_SIGINFO: u32 = 0x0000_0004;
/// SA_RESTORER：静态 musl 恒置，交付时回跳 restorer。
pub const SA_RESTORER: u32 = 0x0400_0000;
/// SA_RESTART：被中断的慢系统调用自动重启。
pub const SA_RESTART: u32 = 0x1000_0000;
/// SA_NODEFER：交付时不隐式屏蔽自身。
pub const SA_NODEFER: u32 = 0x4000_0000;
/// flag 闭集。
pub const SA_ALL: u32 = SA_SIGINFO | SA_RESTORER | SA_RESTART | SA_NODEFER;

/// SIG_BLOCK。
pub const SIG_BLOCK: u32 = 0;
/// SIG_UNBLOCK。
pub const SIG_UNBLOCK: u32 = 1;
/// SIG_SETMASK。
pub const SIG_SETMASK: u32 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单信号位。
#[inline]
pub fn sig_bit(sig: u8) -> u64 {
    if sig == 0 || sig as usize > NSIG {
        return 0;
    }
    1u64 << (sig.wrapping_sub(1))
}

/// 位图基数（手算 popcount，不依赖 intrinsics）。
#[inline]
pub fn popcount(mut v: u64) -> u32 {
    let mut n = 0u32;
    while v != 0 {
        n = n.wrapping_add((v & 1) as u32);
        v >>= 1;
    }
    n
}

/// 处置种类。
pub const DISP_DEFAULT: u8 = 0;
/// 忽略。
pub const DISP_IGNORE: u8 = 1;
/// 捕获。
pub const DISP_CATCH: u8 = 2;

/// 一个动作槽（对应 struct sigaction 的内核副本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Act {
    pub disp: u8,
    pub handler: u64,
    pub mask: u64,
    pub flags: u32,
    pub restorer: u64,
}

/// 空动作（默认处置）。
pub const ACT_DEFAULT: Act = Act {
    disp: DISP_DEFAULT,
    handler: 0,
    mask: 0,
    flags: 0,
    restorer: 0,
};

/// 一个交付帧：AI-02 帧冻结接口的只读输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub sig: u8,
    pub depth: u8,
    pub blocked_at: u64,
    pub handler: u64,
    pub restorer: u64,
    pub flags: u32,
}

/// 信号面账：动作表 + 掩码 + 待决 + 交付帧。
#[derive(Clone, Debug)]
pub struct SignalFace {
    acts: [Act; NSIG],
    frames: [Option<Frame>; MAX_FRAMES],
    frame_n: usize,
    blocked: u64,
    pending: u64,
    terminated: Option<u16>,
    root: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub delivered: u64,
    pub dropped: u64,
    pub dropped_bits: u64,
    pub pending_merges: u64,
    pub mask_sets: u64,
}

impl SignalFace {
    /// 全新进程：动作全默认、掩码空、无待决。
    pub fn new() -> SignalFace {
        SignalFace {
            acts: [ACT_DEFAULT; NSIG],
            frames: [None; MAX_FRAMES],
            frame_n: 0,
            blocked: 0,
            pending: 0,
            terminated: None,
            root: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            delivered: 0,
            dropped: 0,
            dropped_bits: 0,
            pending_merges: 0,
            mask_sets: 0,
        }
    }

    /// 记账一次拒止。
    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前阻塞掩码。
    pub fn blocked(&self) -> u64 {
        self.blocked
    }

    /// 当前待决位图。
    pub fn pending(&self) -> u64 {
        self.pending
    }

    /// 终止码（128+sig），未终止为 None。
    pub fn exit_code(&self) -> Option<u16> {
        self.terminated
    }

    /// 查一槽动作。
    pub fn action(&self, sig: u8) -> Option<Act> {
        if sig == 0 || sig as usize > NSIG {
            return None;
        }
        match self.acts.get((sig as usize).wrapping_sub(1)) {
            Some(a) => Some(*a),
            None => None,
        }
    }

    /// 安装动作。校验序：冻结 → 信号号 → 不可屏蔽 → flag 闭集 → 动作自洽。
    pub fn install(
        &mut self,
        sig: u8,
        disp: u8,
        handler: u64,
        mask: u64,
        flags: u32,
        restorer: u64,
    ) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(SG_EFROZEN));
        }
        if sig == 0 || sig as usize > NSIG {
            return Err(self.note(SG_ESIG));
        }
        if (sig == SIGKILL || sig == SIGSTOP) && disp != DISP_DEFAULT {
            return Err(self.note(SG_EKILL));
        }
        if flags & !SA_ALL != 0 || disp > DISP_CATCH {
            return Err(self.note(SG_EACTION));
        }
        if disp == DISP_CATCH && (handler == 0 || restorer == 0) {
            return Err(self.note(SG_EACTION));
        }
        let a = Act {
            disp,
            handler: if disp == DISP_CATCH { handler } else { 0 },
            mask,
            flags,
            restorer: if disp == DISP_CATCH { restorer } else { 0 },
        };
        match self.acts.get_mut((sig as usize).wrapping_sub(1)) {
            Some(cell) => *cell = a,
            None => return Err(self.note(SG_ESLOT)),
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.table_fold())
    }

    /// 动作表指纹：只含 64 槽，与运行态无关（重放比对用）。
    pub fn table_fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < NSIG {
            match self.acts.get(i) {
                Some(a) => {
                    h = (h ^ a.disp as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ a.handler).wrapping_mul(FNV_PRIME);
                    h = (h ^ a.mask).wrapping_mul(FNV_PRIME);
                    h = (h ^ a.flags as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ a.restorer).wrapping_mul(FNV_PRIME);
                }
                None => {}
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h
    }

    /// sigprocmask：不可屏蔽位静默丢弃并记账；解除屏蔽后待决按号升序交付。
    pub fn set_mask(&mut self, how: u32, set: u64) -> Result<u64, u32> {
        if how > SIG_SETMASK {
            return Err(self.note(SG_EMASK));
        }
        let lost = set & UNMASKABLE;
        if lost != 0 {
            self.dropped_bits = self.dropped_bits.wrapping_add(popcount(lost) as u64);
        }
        let eff = set & !UNMASKABLE;
        match how {
            SIG_BLOCK => self.blocked |= eff,
            SIG_UNBLOCK => self.blocked &= !eff,
            _ => self.blocked = eff,
        }
        self.mask_sets = self.mask_sets.wrapping_add(1);
        self.drain_pending()?;
        Ok(self.state_fold())
    }

    /// 把已解除屏蔽的待决信号按升序交付（标准信号不排队，位图天然合并）。
    fn drain_pending(&mut self) -> Result<u64, u32> {
        let ready = self.pending & !self.blocked;
        let mut b = 0u8;
        while b < NSIG as u8 {
            let sig = b.wrapping_add(1);
            if ready & sig_bit(sig) != 0 {
                self.pending &= !sig_bit(sig);
                self.deliver(sig)?;
            }
            b = b.wrapping_add(1);
        }
        Ok(self.state_fold())
    }

    /// raise：屏蔽中入待决（重复位合并），否则立即交付。
    pub fn raise(&mut self, sig: u8) -> Result<u64, u32> {
        if self.terminated.is_some() {
            return Err(self.note(SG_EDEAD));
        }
        if sig == 0 || sig as usize > NSIG {
            return Err(self.note(SG_ESIG));
        }
        let bit = sig_bit(sig);
        if self.blocked & bit != 0 {
            if self.pending & bit != 0 {
                self.pending_merges = self.pending_merges.wrapping_add(1);
            }
            self.pending |= bit;
            return Ok(self.state_fold());
        }
        self.deliver(sig)
    }

    /// 交付一个信号：捕获入帧、忽略丢弃、默认终止（默认忽略集除外）。
    fn deliver(&mut self, sig: u8) -> Result<u64, u32> {
        let a = match self.action(sig) {
            Some(v) => v,
            None => return Err(self.note(SG_ESIG)),
        };
        if a.disp == DISP_CATCH {
            if self.frame_n >= MAX_FRAMES {
                return Err(self.note(SG_EFRAME));
            }
            let fr = Frame {
                sig,
                depth: self.frame_n as u8,
                blocked_at: self.blocked,
                handler: a.handler,
                restorer: a.restorer,
                flags: a.flags,
            };
            match self.frames.get_mut(self.frame_n) {
                Some(cell) => *cell = Some(fr),
                None => return Err(self.note(SG_EFRAME)),
            }
            self.frame_n = self.frame_n.wrapping_add(1);
            let mut next = self.blocked | a.mask;
            if a.flags & SA_NODEFER == 0 {
                next |= sig_bit(sig);
            }
            self.blocked = next & !UNMASKABLE;
            self.delivered = self.delivered.wrapping_add(1);
        } else if a.disp == DISP_IGNORE || DEFAULT_IGNORE & sig_bit(sig) != 0 {
            self.dropped = self.dropped.wrapping_add(1);
        } else {
            self.terminated = Some(128u16.wrapping_add(sig as u16));
            self.delivered = self.delivered.wrapping_add(1);
        }
        Ok(self.state_fold())
    }

    /// 交付帧数。
    pub fn frame_count(&self) -> usize {
        self.frame_n
    }

    /// 交付帧只读视图。
    pub fn frame(&self, i: usize) -> Option<Frame> {
        match self.frames.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// AI-02 帧冻结接口：帧栈指纹（只读，与掩码/待决的后续变动无关）。
    pub fn frame_digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.frame_n {
            match self.frames.get(i) {
                Some(Some(f)) => {
                    h = (h ^ f.sig as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ f.depth as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ f.blocked_at).wrapping_mul(FNV_PRIME);
                    h = (h ^ f.handler).wrapping_mul(FNV_PRIME);
                    h = (h ^ f.restorer).wrapping_mul(FNV_PRIME);
                    h = (h ^ f.flags as u64).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 运行态指纹（掩码/待决/交付/帧/终止）。
    pub fn state_fold(&self) -> u64 {
        let mut h = self.table_fold();
        h = (h ^ self.blocked).wrapping_mul(FNV_PRIME);
        h = (h ^ self.pending).wrapping_mul(FNV_PRIME);
        h = (h ^ self.delivered).wrapping_mul(FNV_PRIME);
        h = (h ^ self.dropped).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frame_n as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.terminated.unwrap_or(0) as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结动作表（静态路径配置定稿），幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.table_fold()),
            };
        }
        let v = self.table_fold();
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 定锚值。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 重放：动作表指纹必须与原定锚一致。
    pub fn replay(&mut self) -> Result<u64, u32> {
        let root = match self.root {
            Some(r) => r,
            None => return Err(self.note(SG_EMISMATCH)),
        };
        if self.table_fold() != root {
            return Err(self.note(SG_EMISMATCH));
        }
        Ok(root)
    }
}

/// 基准动作表：10 捕获（SA_RESTART）、12 捕获（NODEFER + 自带掩码 15）、15 捕获。
pub fn build_face() -> (SignalFace, bool) {
    let mut f = SignalFace::new();
    let ok1 = f
        .install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER | SA_RESTART, 0x9000)
        .is_ok();
    let ok2 = f
        .install(12, DISP_CATCH, 0x2000, sig_bit(15), SA_RESTORER | SA_NODEFER, 0x9000)
        .is_ok();
    let ok3 = f
        .install(15, DISP_CATCH, 0x3000, 0, SA_RESTORER, 0x9000)
        .is_ok();
    let frozen = f.freeze().is_ok();
    (f, ok1 && ok2 && ok3 && frozen)
}

/// 四类注入：不可屏蔽信号 / 动作非法 / how 非法 / 帧栈满。
pub fn inject_four() -> [bool; 4] {
    // ① SIGKILL/SIGSTOP 捕获与忽略皆拒
    let mut a0 = SignalFace::new();
    let r0 = a0
        .install(SIGKILL, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000)
        .err()
        == Some(SG_EKILL)
        && a0.install(SIGSTOP, DISP_IGNORE, 0, 0, 0, 0).err() == Some(SG_EKILL)
        && a0.install(SIGKILL, DISP_DEFAULT, 0, 0, 0, 0).is_ok();

    // ② 动作非法：无 restorer / 无 handler / flag 越界
    let mut a1 = SignalFace::new();
    let r1 = a1
        .install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0)
        .err()
        == Some(SG_EACTION)
        && a1.install(10, DISP_CATCH, 0, 0, SA_RESTORER, 0x9000).err()
            == Some(SG_EACTION)
        && a1.install(10, DISP_CATCH, 0x1000, 0, 0x8000_0000, 0x9000)
            .err()
            == Some(SG_EACTION);

    // ③ how 非法
    let mut a2 = SignalFace::new();
    let r2 = a2.set_mask(3, 0).err() == Some(SG_EMASK)
        && a2.set_mask(SIG_SETMASK, 0).is_ok()
        && a2.rejects == 1;

    // ④ 帧栈满：九个捕获信号依次交付，第九帧被拒
    let mut a3 = SignalFace::new();
    let picks: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 10];
    let mut ins = true;
    let mut t = 0usize;
    while t < picks.len() {
        match picks.get(t) {
            Some(sig) => {
                if a3.install(*sig, DISP_CATCH, 0x4000u64 + *sig as u64, 0, SA_RESTORER, 0x9000)
                    .is_err()
                {
                    ins = false;
                }
            }
            None => break,
        }
        t = t.wrapping_add(1);
    }
    let mut j = 0usize;
    let mut overflow = None;
    while j < picks.len() {
        match picks.get(j) {
            Some(sig) => {
                if let Err(c) = a3.raise(*sig) {
                    overflow = Some(c);
                }
            }
            None => break,
        }
        j = j.wrapping_add(1);
    }
    let r3 = ins && overflow == Some(SG_EFRAME) && a3.frame_count() == MAX_FRAMES;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11269 musl 信号面（静态路径）。
pub fn run_unx11269_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11269.musl_signal_face");
    let (f, ok) = build_face();

    // SG1 默认表快照
    let fresh = SignalFace::new();
    let mut all_default = true;
    let mut i = 0usize;
    while i < NSIG {
        match fresh.action((i as u8).wrapping_add(1)) {
            Some(a) => {
                if a.disp != DISP_DEFAULT || a.handler != 0 || a.restorer != 0 {
                    all_default = false;
                }
            }
            None => all_default = false,
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "SG1-默认表快照",
        all_default && fresh.blocked() == 0 && fresh.pending() == 0,
        "64 槽全默认、掩码与待决皆空",
    );

    // SG2 安装与查询
    let a10 = f.action(10);
    let a15 = f.action(15);
    s.add_dyn(
        "SG2-安装与查询",
        ok && f.accepts == 3
            && a10.map(|a| a.handler) == Some(0x1000)
            && a15.map(|a| a.restorer) == Some(0x9000)
            && a10.map(|a| a.flags & SA_RESTART != 0) == Some(true),
        &format!("acc={} h10={:#x}", f.accepts, a10.map(|a| a.handler).unwrap_or(0)),
    );

    // SG3 不可屏蔽信号拒止
    let mut k = SignalFace::new();
    let k1 = k
        .install(SIGKILL, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000)
        .err()
        == Some(SG_EKILL);
    let k2 = k.install(SIGSTOP, DISP_IGNORE, 0, 0, 0, 0).err() == Some(SG_EKILL);
    s.add(
        "SG3-不可屏蔽信号拒止",
        k1 && k2 && k.accepts == 0 && k.rejects == 2,
        "SIGKILL/SIGSTOP 捕获与忽略两路皆拒",
    );

    // SG4 动作非法拒止
    let mut j = SignalFace::new();
    let j1 = j.install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0).err()
        == Some(SG_EACTION);
    let j2 = j.install(10, DISP_CATCH, 0, 0, SA_RESTORER, 0x9000).err()
        == Some(SG_EACTION);
    let j3 = j.install(10, DISP_CATCH, 0x1000, 0, 0x8000_0000, 0x9000).err()
        == Some(SG_EACTION);
    s.add(
        "SG4-动作非法拒止",
        j1 && j2 && j3 && j.accepts == 0,
        "无 restorer / 无 handler / flag 越界三路俱拒",
    );

    // SG5 how 非法 + 不可屏蔽位静默丢弃
    let mut m = SignalFace::new();
    let bad_how = m.set_mask(9, 0).err() == Some(SG_EMASK);
    let masked = m
        .set_mask(SIG_BLOCK, sig_bit(SIGKILL) | sig_bit(SIGSTOP) | sig_bit(10))
        .is_ok();
    s.add_dyn(
        "SG5-掩码语义",
        bad_how && masked && m.dropped_bits == 2 && m.blocked() == sig_bit(10)
            && m.blocked() & UNMASKABLE == 0,
        &format!("drop={} blk={:#x}", m.dropped_bits, m.blocked()),
    );

    // SG6 屏蔽下入待决（重复位合并）
    let mut p = SignalFace::new();
    let _ = p.install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000);
    let _ = p.install(12, DISP_CATCH, 0x2000, sig_bit(15), SA_RESTORER, 0x9000);
    let _ = p.set_mask(SIG_BLOCK, sig_bit(10) | sig_bit(12));
    let r1 = p.raise(10).is_ok();
    let r2 = p.raise(12).is_ok();
    let r3 = p.raise(10).is_ok();
    s.add_dyn(
        "SG6-屏蔽入待决",
        r1 && r2 && r3 && p.frame_count() == 0 && p.delivered == 0
            && p.pending() == sig_bit(10) | sig_bit(12) && p.pending_merges == 1,
        &format!("pend={:#x} merge={} frm={}", p.pending(), p.pending_merges, p.frame_count()),
    );

    // SG7 解除屏蔽后按号升序交付
    let order_ok = p.set_mask(SIG_UNBLOCK, sig_bit(10) | sig_bit(12)).is_ok()
        && p.frame_count() == 2
        && p.frame(0).map(|x| x.sig) == Some(10)
        && p.frame(1).map(|x| x.sig) == Some(12)
        && p.delivered == 2;
    s.add_dyn(
        "SG7-解屏蔽升序交付",
        order_ok,
        &format!(
            "f0={} f1={} dlv={}",
            p.frame(0).map(|x| x.sig).unwrap_or(0),
            p.frame(1).map(|x| x.sig).unwrap_or(0),
            p.delivered
        ),
    );

    // SG8 sa_mask 并入 + 隐式自身屏蔽
    s.add(
        "SG8-sa_mask并入",
        p.blocked() & sig_bit(10) != 0 && p.blocked() & sig_bit(12) != 0,
        "交付 10 后隐式屏蔽自身；12 自带 sa_mask 15",
    );

    // SG9 NODEFER 不隐式屏蔽自身
    let mut n = SignalFace::new();
    let _ = n.install(12, DISP_CATCH, 0x2000, 0, SA_RESTORER | SA_NODEFER, 0x9000);
    let before = n.blocked();
    let raised = n.raise(12).is_ok();
    s.add(
        "SG9-NODEFER语义",
        raised && n.frame_count() == 1 && n.blocked() == before,
        "NODEFER 下交付不改变掩码",
    );

    // SG10 默认动作终止（SIGTERM → 143）且终止后拒 raise
    let mut t = SignalFace::new();
    let term = t.raise(SIGTERM);
    let dead = t.raise(SIGTERM).err() == Some(SG_EDEAD);
    s.add_dyn(
        "SG10-默认终止",
        term.is_ok() && t.exit_code() == Some(143) && dead,
        &format!("exit={:?} dead={}", t.exit_code(), dead as u8),
    );

    // SG11 默认忽略集
    let mut g = SignalFace::new();
    let gr = g.raise(SIGCHLD).is_ok() && g.raise(SIGCONT).is_ok();
    s.add_dyn(
        "SG11-默认忽略集",
        gr && g.exit_code().is_none() && g.dropped == 2 && g.frame_count() == 0,
        &format!("drop={} exit={:?}", g.dropped, g.exit_code()),
    );

    // SG12 帧冻结接口（指纹与后续掩码变动无关）
    let mut h = SignalFace::new();
    let _ = h.install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000);
    let seed = h.frame_digest();
    let _ = h.raise(10);
    let d1 = h.frame_digest();
    let _ = h.set_mask(SIG_UNBLOCK, 0xffff_ffff_ffff_ffff);
    let d2 = h.frame_digest();
    s.add_dyn(
        "SG12-帧冻结接口",
        seed != d1 && d1 == d2
            && h.frame(0).map(|x| x.handler) == Some(0x1000)
            && h.frame(0).map(|x| x.blocked_at) == Some(0),
        &format!("seed={:#x} d1={:#x} d2={:#x}", seed, d1, d2),
    );

    // SG13 注入四拒 + 重放
    let hits = inject_four();
    let (mut rf, ok2) = build_face();
    let rep = rf.replay().unwrap_or(0);
    s.add_dyn(
        "SG13-注入四拒与重放",
        hits[0] && hits[1] && hits[2] && hits[3] && ok2
            && rep == rf.root_value().unwrap_or(1),
        &format!("inj={}{}{}{} root={:#x}", hits[0] as u8, hits[1] as u8,
            hits[2] as u8, hits[3] as u8, rep),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11269_signal_flow() {
        let (mut f, ok) = build_face();
        assert!(ok);
        assert_eq!(f.accepts, 3);
        assert_eq!(f.raise(10), Ok(f.state_fold()));
        assert_eq!(f.frame_count(), 1);
        assert_eq!(f.frame(0).map(|x| x.sig), Some(10));
        // 隐式屏蔽后同号再 raise 只入待决
        assert!(f.raise(10).is_ok());
        assert_eq!(f.frame_count(), 1);
        assert_eq!(f.pending() & sig_bit(10), sig_bit(10));
        assert!(f.set_mask(SIG_UNBLOCK, sig_bit(10)).is_ok());
        assert_eq!(f.frame_count(), 2);
        assert!(f.replay().is_ok());
    }

    #[test]
    fn f11269_signal_gates() {
        let mut t = SignalFace::new();
        assert_eq!(
            t.install(SIGKILL, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000),
            Err(SG_EKILL)
        );
        assert_eq!(t.install(SIGSTOP, DISP_IGNORE, 0, 0, 0, 0), Err(SG_EKILL));
        assert_eq!(
            t.install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0),
            Err(SG_EACTION)
        );
        assert_eq!(t.install(0, DISP_DEFAULT, 0, 0, 0, 0), Err(SG_ESIG));
        assert_eq!(t.install(65, DISP_DEFAULT, 0, 0, 0, 0), Err(SG_ESIG));
        assert_eq!(t.set_mask(9, 0), Err(SG_EMASK));
        assert_eq!(t.raise(0), Err(SG_ESIG));
        assert_eq!(t.accepts, 0);
        assert_eq!(t.rejects, 7);
        assert!(t.install(10, DISP_CATCH, 0x1000, 0, SA_RESTORER, 0x9000).is_ok());
        assert!(t.raise(10).is_ok());
        assert_eq!(t.frame_count(), 1);
        assert_eq!(t.delivered, 1);
        assert_eq!(t.rejects, 7);
    }

    #[test]
    fn f11269_signal_rules() {
        // 不可屏蔽位写掩码只被丢弃
        let mut m = SignalFace::new();
        assert!(m.set_mask(SIG_BLOCK, UNMASKABLE).is_ok());
        assert_eq!(m.blocked(), 0);
        assert_eq!(m.dropped_bits, 2);
        // 默认忽略集不终止
        assert!(m.raise(SIGCHLD).is_ok());
        assert_eq!(m.exit_code(), None);
        assert_eq!(m.dropped, 1);
        // 默认终止写退出码且封死后续 raise
        let mut d = SignalFace::new();
        assert!(d.raise(SIGTERM).is_ok());
        assert_eq!(d.exit_code(), Some(143));
        assert_eq!(d.raise(10), Err(SG_EDEAD));
        // NODEFER 不动掩码
        let mut n = SignalFace::new();
        assert!(n.install(12, DISP_CATCH, 0x2000, 0, SA_RESTORER | SA_NODEFER, 0x9000).is_ok());
        assert!(n.raise(12).is_ok());
        assert_eq!(n.blocked(), 0);
        assert_eq!(n.frame_count(), 1);
        // 冻结后改表拒
        assert!(n.freeze().is_ok());
        assert_eq!(n.install(13, DISP_IGNORE, 0, 0, 0, 0), Err(SG_EFROZEN));
    }

    #[test]
    fn f11269_signal_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11269_checks_green() {
        let cs = run_unx11269_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
