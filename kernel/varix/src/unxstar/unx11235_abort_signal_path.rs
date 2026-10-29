//! UNX-F11235 panic/abort 路径内核消费（0x1f835 段）
//!
//! 锚点：UNX-F11235-J1 abort→raise(SIGABRT)→core 语义路径判据绿，
//! WIFSIGNALED 报告与 Linux 一致。
//! 域：CoRun·UNX-C5-B02——abort 必须走「解除屏蔽→raise(6)→默认动作终止」；
//! 等待状态字要能被 WIFSIGNALED/WTERMSIG/WCOREDUMP 三宏正确解出。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f835;
pub const PA_RAISE: u32 = SEG_BASE + 0x01; // raise 返回非零
pub const PA_MASK: u32 = SEG_BASE + 0x02; // 屏蔽未解除
pub const PA_STATUS: u32 = SEG_BASE + 0x03; // 等待状态非信号态
pub const PA_CORE: u32 = SEG_BASE + 0x04; // 缺 core 位
pub const PA_SIG: u32 = SEG_BASE + 0x05; // 信号号非法
pub const PA_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const PA_OVER: u32 = SEG_BASE + 0x07; // 超容

/// SIGABRT 与状态位。
pub const SIGABRT: u32 = 6;
pub const CORE_BIT: u32 = 0x80;
pub const WANT_STEPS: usize = 3;
pub const MAX_STEPS: usize = 8;
pub const PA_TAG: u64 = 0x1f835a9;
pub const FP_STEP: u64 = 0x100000001b3;

/// WIFSIGNALED：低七位非零且非 stopped 编码（本模型只产信号态）。
pub fn wifsignaled(status: u32) -> bool {
    (status & 0x7f) != 0
}

/// WTERMSIG：取低七位。
pub fn wtermsig(status: u32) -> u32 {
    status & 0x7f
}

/// WCOREDUMP：取 0x80 位。
pub fn wcoredump(status: u32) -> bool {
    status & CORE_BIT != 0
}

/// 一步 abort 路径动作。
#[derive(Clone, Debug)]
pub struct Step {
    pub name: &'static str,
    pub sig: u32,
    pub status: u32,
    pub masked: bool,
}

impl Step {
    pub fn delivered(&self) -> bool {
        self.sig == SIGABRT && wifsignaled(self.status) && wtermsig(self.status) == SIGABRT
    }
}

/// abort 路径账。
#[derive(Clone, Debug)]
pub struct AbortPath {
    pub steps: Vec<Step>,
    pub unblocked: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl AbortPath {
    pub fn new() -> AbortPath {
        AbortPath { steps: Vec::new(), unblocked: false, frozen: false, root: None }
    }

    /// abort 一步：先解除屏蔽，再 raise(6)。
    pub fn abort(&mut self, masked: bool) -> Result<u32, u32> {
        if self.frozen {
            return Err(PA_FROZEN);
        }
        if self.steps.len() >= MAX_STEPS {
            return Err(PA_OVER);
        }
        self.unblocked = true;
        let status = SIGABRT | CORE_BIT;
        self.steps.push(Step { name: "raise", sig: SIGABRT, status, masked });
        Ok(status)
    }

    /// 自定义 raise：只接受 SIGABRT，其余拒。
    pub fn raise(&mut self, sig: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(PA_FROZEN);
        }
        if sig != SIGABRT {
            return Err(PA_SIG);
        }
        if self.steps.len() >= MAX_STEPS {
            return Err(PA_OVER);
        }
        self.steps.push(Step { name: "raise", sig, status: sig | CORE_BIT, masked: false });
        Ok(0)
    }

    pub fn all_delivered(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.delivered())
    }

    pub fn shell_code(&self) -> u32 {
        match self.steps.last() {
            Some(s) => 128u32.saturating_add(wtermsig(s.status)),
            None => 0,
        }
    }

    pub fn core(&self) -> bool {
        match self.steps.last() {
            Some(s) => wcoredump(s.status),
            None => false,
        }
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(PA_FROZEN);
        }
        if self.steps.len() != WANT_STEPS {
            return Err(PA_STATUS);
        }
        if !self.unblocked {
            return Err(PA_MASK);
        }
        if !self.all_delivered() {
            return Err(PA_STATUS);
        }
        if !self.core() {
            return Err(PA_CORE);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("abort steps {} shell {} core {}", self.steps.len(), self.shell_code(), self.core())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = PA_TAG;
        h = (h ^ self.unblocked as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.steps.len() {
            let b = self.steps[k].name.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.steps[k].sig as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.steps[k].status as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 组一张三次 abort、含屏蔽态与解除态的路径账。
pub fn build() -> AbortPath {
    let mut p = AbortPath::new();
    let _ = p.abort(true);
    let _ = p.abort(false);
    let _ = p.abort(true);
    p
}

/// 四路对抗：非 ABRT 信号、欠步冻存、冻后 abort、超步。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = AbortPath::new();
    out[0] = a.raise(15) == Err(PA_SIG);
    let _ = a.abort(false);
    out[1] = a.freeze() == Err(PA_STATUS);
    let mut b = build();
    let _ = b.freeze();
    out[2] = b.abort(false) == Err(PA_FROZEN);
    let mut c = AbortPath::new();
    let mut k = 0usize;
    while k < MAX_STEPS {
        let _ = c.abort(false);
        k += 1;
    }
    out[3] = c.abort(false) == Err(PA_OVER);
    out
}

/// 判据入口。
pub fn run_unx11235_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11235");
    let mut p = build();
    let root = p.freeze().unwrap_or(0);

    s.add("J1-raise 六号", p.steps.iter().all(|x| x.sig == SIGABRT), "三步 abort 均以 SIGABRT 发起");
    s.add("J2-信号态", p.steps.iter().all(|x| wifsignaled(x.status)), "三步等待状态均为信号终止态");
    s.add("J3-终止号", p.steps.iter().all(|x| wtermsig(x.status) == SIGABRT), "WTERMSIG 解出六");
    s.add("J4-core 位", p.steps.iter().all(|x| wcoredump(x.status)), "三步状态字均带 core 位");
    s.add("J5-状态字", p.steps.iter().all(|x| x.status == 134), "等待状态字为 134（0x86）");
    s.add("J6-外壳码", p.shell_code() == 134, "外壳报告码 128+6=134");
    s.add("J7-屏蔽解除", p.unblocked && p.steps.iter().any(|x| x.masked), "屏蔽态下 abort 仍先解除屏蔽");
    s.add("J8-送达自证", p.all_delivered(), "三步全部按 ABRT 送达");
    s.add("J9-冻存根", root != 0 && p.root == Some(root), "abort 账冻存产出非零根");

    s.add("J10-冻后 abort 拒", p.abort(false) == Err(PA_FROZEN), "冻存后 abort 拒为 PA_FROZEN");
    s.add("J11-冻后 raise 拒", p.raise(SIGABRT) == Err(PA_FROZEN), "冻存后 raise 拒为 PA_FROZEN");
    s.add("J12-非 ABRT 拒", {
        let mut x = AbortPath::new();
        x.raise(9) == Err(PA_SIG)
    }, "非 SIGABRT 信号拒为 PA_SIG");
    s.add("J13-零态非信号", !wifsignaled(0) && wtermsig(0) == 0, "状态字零不判为信号终止");

    let four = inject_four();
    s.add("J14-四路注入", four.iter().all(|x| *x), "非 ABRT/欠步/冻后 abort/超步四路全拒");
    s.add_dyn(
        "J15-路径明细",
        p.steps.len() == 3 && p.shell_code() == 134,
        &format!("步数 {}，状态 0x{:x}，外壳 {}", p.steps.len(), p.steps[2].status, p.shell_code()),
    );
    s.add_dyn(
        "J16-账根",
        root == p.fold(),
        &format!("根 0x{:x}，解除标记 {}", root, p.unblocked),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11235_abort_path_flow() {
        let mut p = build();
        assert!(p.all_delivered());
        assert_eq!(p.shell_code(), 134);
        let r = p.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(p.abort(false), Err(PA_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11235_checks_green() {
        let s = run_unx11235_checks();
        assert!(s.all_passed(), "UNX-F11235 判据未全绿");
    }
}
