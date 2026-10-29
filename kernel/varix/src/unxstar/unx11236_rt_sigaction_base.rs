//! UNX-F11236 信号面基础消费（rt_sigaction 基础）（0x1f836 段）
//!
//! 锚点：UNX-F11236-J1 rt_sigaction 注册/恢复/SA_RESTORER 语义消费判据绿，
//! 与 AI-02 信号帧冻结接口零冲突。
//! 域：CoRun·UNX-C5-B02——注册表只收可捕获信号；置 SA_RESTORER 必须同时
//! 给恢复桩地址；屏蔽字不得含 KILL/STOP 两位，否则与帧冻结接口冲突。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f836;
pub const SA_SIG: u32 = SEG_BASE + 0x01; // 信号号非法/不可捕获
pub const SA_MASK: u32 = SEG_BASE + 0x02; // 屏蔽字含 KILL/STOP
pub const SA_RESTORER: u32 = SEG_BASE + 0x03; // 缺恢复桩
pub const SA_FLAGS: u32 = SEG_BASE + 0x04; // 未知标志位
pub const SA_CONFLICT: u32 = SEG_BASE + 0x05; // 重复注册
pub const SA_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const SA_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 标志位与信号号。
pub const SIGINFO: u32 = 0x4;
pub const SIGRESTART: u32 = 0x1000_0000;
pub const SIGRESTORER: u32 = 0x0400_0000;
pub const SIGNODEFER: u32 = 0x4000_0000;
pub const KNOWN_FLAGS: u32 = SIGINFO | SIGRESTART | SIGRESTORER | SIGNODEFER;
pub const SIGKILL: u32 = 9;
pub const SIGSTOP: u32 = 19;
pub const MAX_SIG: u32 = 64;
pub const WANT: usize = 3;
pub const CAP: usize = 16;
pub const SA_TAG: u64 = 0x1f836b7;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一个注册动作。
#[derive(Clone, Debug)]
pub struct Action {
    pub sig: u32,
    pub handler: u64,
    pub flags: u32,
    pub restorer: u64,
    pub mask: u64,
}

impl Action {
    pub fn catchable(&self) -> bool {
        self.sig >= 1 && self.sig <= MAX_SIG && self.sig != SIGKILL && self.sig != SIGSTOP
    }

    pub fn mask_ok(&self) -> bool {
        self.mask & (1u64 << (SIGKILL - 1)) == 0 && self.mask & (1u64 << (SIGSTOP - 1)) == 0
    }

    pub fn restore_ready(&self) -> bool {
        self.flags & SIGRESTORER == 0 || self.restorer != 0
    }

    pub fn frame_compat(&self) -> bool {
        self.catchable() && self.mask_ok() && self.restore_ready() && self.handler != 0
    }
}

/// 信号动作表。
#[derive(Clone, Debug)]
pub struct SigTable {
    pub acts: Vec<Action>,
    pub restores: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SigTable {
    pub fn new() -> SigTable {
        SigTable { acts: Vec::new(), restores: 0, frozen: false, root: None }
    }

    /// 注册：不可捕获/重复/未知标志/缺桩/坏屏蔽五路拒。
    pub fn register(&mut self, sig: u32, handler: u64, flags: u32, restorer: u64, mask: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(SA_FROZEN);
        }
        if sig == 0 || sig > MAX_SIG || sig == SIGKILL || sig == SIGSTOP {
            return Err(SA_SIG);
        }
        if flags & !KNOWN_FLAGS != 0 {
            return Err(SA_FLAGS);
        }
        if flags & SIGRESTORER != 0 && restorer == 0 {
            return Err(SA_RESTORER);
        }
        if mask & (1u64 << (SIGKILL - 1)) != 0 || mask & (1u64 << (SIGSTOP - 1)) != 0 {
            return Err(SA_MASK);
        }
        if self.acts.iter().any(|a| a.sig == sig) {
            return Err(SA_CONFLICT);
        }
        if self.acts.len() >= CAP {
            return Err(SA_OVER);
        }
        self.acts.push(Action { sig, handler, flags, restorer, mask });
        Ok(())
    }

    /// 恢复旧动作（模型：撤销表项并计恢复次数）。
    pub fn restore(&mut self, sig: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SA_FROZEN);
        }
        let mut k = 0usize;
        while k < self.acts.len() {
            if self.acts[k].sig == sig {
                self.acts.remove(k);
                self.restores = self.restores.saturating_add(1);
                return Ok(sig);
            }
            k += 1;
        }
        Err(SA_SIG)
    }

    pub fn all_compat(&self) -> bool {
        !self.acts.is_empty() && self.acts.iter().all(|a| a.frame_compat())
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SA_FROZEN);
        }
        if self.acts.len() != WANT {
            return Err(SA_SIG);
        }
        if !self.all_compat() {
            return Err(SA_RESTORER);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("sigacts {} compat {} restores {}", self.acts.len(), self.all_compat(), self.restores)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SA_TAG;
        let mut k = 0usize;
        while k < self.acts.len() {
            h = (h ^ self.acts[k].sig as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.acts[k].handler).wrapping_mul(FP_STEP);
            h = (h ^ self.acts[k].flags as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.acts[k].restorer).wrapping_mul(FP_STEP);
            h = (h ^ self.acts[k].mask).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.restores as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 三个可捕获信号与一个恢复桩地址。
pub const SIGS: [u32; 3] = [10, 11, 17];
pub const RESTORER: u64 = 0x7f60_0000_1000;

/// 组一张三动作、全带恢复桩的账。
pub fn build() -> SigTable {
    let mut t = SigTable::new();
    let mut k = 0usize;
    while k < WANT {
        let _ = t.register(SIGS[k], 0x401000 + k as u64 * 0x40, SIGRESTORER | SIGRESTART, RESTORER, 0);
        k += 1;
    }
    t
}

/// 四路对抗：KILL、缺恢复桩、未知标志、屏蔽 KILL。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = SigTable::new();
    out[0] = a.register(SIGKILL, 0x1, SIGRESTORER, RESTORER, 0) == Err(SA_SIG);
    out[1] = a.register(10, 0x1, SIGRESTORER, 0, 0) == Err(SA_RESTORER);
    out[2] = a.register(10, 0x1, 0x8000_0000, RESTORER, 0) == Err(SA_FLAGS);
    out[3] = a.register(10, 0x1, 0, 0, 1u64 << (SIGKILL - 1)) == Err(SA_MASK);
    out
}

/// 判据入口。
pub fn run_unx11236_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11236");
    let mut t = build();
    let root = t.freeze().unwrap_or(0);

    s.add("J1-三动作", t.acts.len() == WANT, "三个信号动作注册在册");
    s.add("J2-可捕获", t.acts.iter().all(|a| a.catchable()), "三个信号均在 1..=64 且非 KILL/STOP");
    s.add("J3-恢复桩", t.acts.iter().all(|a| a.flags & SIGRESTORER != 0 && a.restorer != 0), "每动作带 SA_RESTORER 与非零恢复桩");
    s.add("J4-标志已知", t.acts.iter().all(|a| a.flags & !KNOWN_FLAGS == 0), "标志位不含未定义位");
    s.add("J5-屏蔽干净", t.acts.iter().all(|a| a.mask_ok()), "屏蔽字不含 KILL/STOP 两位");
    s.add("J6-帧兼容", t.all_compat(), "全部动作与 AI-02 帧冻结接口兼容");
    s.add("J7-冻存根", root != 0 && t.root == Some(root), "动作表冻存产出非零根");

    s.add("J8-冻后注册拒", t.register(12, 0x1, 0, 0, 0) == Err(SA_FROZEN), "冻存后注册拒为 SA_FROZEN");
    s.add("J9-冻后恢复拒", t.restore(10) == Err(SA_FROZEN), "冻存后恢复拒为 SA_FROZEN");
    s.add("J10-KILL 拒", {
        let mut x = SigTable::new();
        x.register(SIGKILL, 0x1, 0, 0, 0) == Err(SA_SIG)
    }, "注册 SIGKILL 拒为 SA_SIG");
    s.add("J11-STOP 拒", {
        let mut x = SigTable::new();
        x.register(SIGSTOP, 0x1, 0, 0, 0) == Err(SA_SIG)
    }, "注册 SIGSTOP 拒为 SA_SIG");
    s.add("J12-缺桩拒", {
        let mut x = SigTable::new();
        x.register(10, 0x1, SIGRESTORER, 0, 0) == Err(SA_RESTORER)
    }, "置 SA_RESTORER 而无桩址拒为 SA_RESTORER");
    s.add("J13-重复拒", {
        let mut x = build();
        x.register(SIGS[0], 0x2, 0, 0, 0) == Err(SA_CONFLICT)
    }, "同信号重复注册拒为 SA_CONFLICT");
    s.add("J14-恢复生效", {
        let mut x = build();
        x.restore(SIGS[0]).is_ok() && x.acts.len() == 2 && x.restores == 1
    }, "恢复旧动作后表项减一、恢复计数加一");

    let four = inject_four();
    s.add("J15-四路注入", four.iter().all(|x| *x), "KILL/缺桩/未知标志/屏蔽 KILL 四路全拒");
    s.add_dyn(
        "J16-动作明细",
        t.acts.len() == 3 && t.acts.iter().all(|a| a.restorer == RESTORER),
        &format!("动作 {} 个，桩 0x{:x}，帧兼容 {}", t.acts.len(), RESTORER, t.all_compat()),
    );
    s.add_dyn(
        "J17-账根",
        root == t.fold(),
        &format!("根 0x{:x}，首信号 {}", root, t.acts[0].sig),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11236_sigaction_flow() {
        let mut t = build();
        assert!(t.all_compat());
        let r = t.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(t.register(12, 1, 0, 0, 0), Err(SA_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11236_checks_green() {
        let s = run_unx11236_checks();
        assert!(s.all_passed(), "UNX-F11236 判据未全绿");
    }
}

/// 查询面：按信号检索、标志与屏蔽字的静态判读。
impl SigTable {
    /// 按信号号查已注册动作。
    pub fn find(&self, sig: u32) -> Option<&Action> {
        let mut k = 0usize;
        while k < self.acts.len() {
            if self.acts[k].sig == sig {
                return self.acts.get(k);
            }
            k += 1;
        }
        None
    }

    /// 已注册动作数。
    pub fn count(&self) -> usize {
        self.acts.len()
    }

    /// 屏蔽字中不可阻塞位是否全清。
    pub fn all_masks_clean(&self) -> bool {
        let mut k = 0usize;
        while k < self.acts.len() {
            if !self.acts[k].mask_ok() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 需要恢复桩的动作是否都带了桩。
    pub fn all_restorers_ready(&self) -> bool {
        let mut k = 0usize;
        while k < self.acts.len() {
            if !self.acts[k].restore_ready() {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// 标志位静态判读：未知位即不合法。
pub fn flags_known(flags: u32) -> bool {
    flags & !KNOWN_FLAGS == 0
}

/// 屏蔽字静态判读：信号号合法且不指向 KILL/STOP。
pub fn mask_allowed(mask: u64, sig: u32) -> bool {
    if sig < 1 || sig > MAX_SIG {
        return false;
    }
    mask & (1u64 << (sig - 1)) == 0 || (sig != SIGKILL && sig != SIGSTOP)
}

#[cfg(test)]
mod query_tests {
    use super::*;

    #[test]
    fn flags_and_masks_are_static() {
        assert!(flags_known(SIGINFO | SIGRESTART));
        assert!(!flags_known(0x8000_0000));
        assert!(mask_allowed(1u64 << 4, 5));
        assert!(!mask_allowed(1u64 << (SIGKILL - 1), SIGKILL));
    }

    #[test]
    fn table_queries_agree() {
        let mut t = SigTable::new();
        assert!(t.register(2, 0x1000, SIGRESTORER, 0x2000, 0).is_ok());
        assert!(t.register(15, 0x3000, SIGINFO, 0, 1u64 << 2).is_ok());
        assert_eq!(t.count(), 2);
        assert_eq!(t.find(2).map(|a| a.handler), Some(0x1000));
        assert!(t.find(9).is_none());
        assert!(t.all_masks_clean() && t.all_restorers_ready());
    }
}
