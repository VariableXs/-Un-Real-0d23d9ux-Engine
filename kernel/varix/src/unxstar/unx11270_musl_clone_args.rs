//! UNX-F11270 musl 线程面消费（clone 参数族）（0x1fa40 段）
//! 纯功能模型：clone 六参数（栈/TLS/ctid/ptid/flags/arg）逐项语义核验——
//! 建线程必须 CLONE_VM + 16 对齐非零栈；SETTLS 必须 16 对齐 TP；
//! SETTID 地址必须在用户区且 ctid 不得重复绑定；fn 指针非零。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1fa40 独占 0x10）。
pub const SEG_BASE: u32 = 0x1fa40;
/// 栈参数违规（建线程栈零/未 16 对齐；或未建线程却给栈）。
pub const CL_STACK: u32 = SEG_BASE + 0x01;
/// TLS 参数违规（SETTLS 下 TP 零/未 16 对齐；未置位却给 TP）。
pub const CL_TLS: u32 = SEG_BASE + 0x02;
/// ctid/ptid 违规（地址落在核心区或缺失）。
pub const CL_TID: u32 = SEG_BASE + 0x03;
/// flags 违规（未知位或低字节非退出信号）。
pub const CL_FLAGS: u32 = SEG_BASE + 0x04;
/// 入口函数指针为零。
pub const CL_ARG: u32 = SEG_BASE + 0x05;
/// ctid 地址已绑定到在线任务。
pub const CL_DUP: u32 = SEG_BASE + 0x06;
/// 封册后写账。
pub const CL_FROZEN: u32 = SEG_BASE + 0x07;
/// 线程表已满。
pub const CL_CAP: u32 = SEG_BASE + 0x08;

/// 线程表定容。
pub const THR_CAP: usize = 32;
/// 用户地址上界（之下为用户区）。
pub const USER_HI: u64 = 0x0000_8000_0000_0000;
/// 用户地址下界（0 页保留）。
pub const USER_LO: u64 = 0x0000_0000_0001_0000;
/// 栈/TLS 对齐。
pub const ALIGN16: u64 = 16;
/// clone flags 闭集。
pub const CV_VM: u64 = 0x0000_0100;
pub const CV_FS: u64 = 0x0000_0200;
pub const CV_FILES: u64 = 0x0000_0400;
pub const CV_SIGHAND: u64 = 0x0000_0800;
pub const CV_THREAD: u64 = 0x0001_0000;
pub const CV_SYSVSEM: u64 = 0x0004_0000;
pub const CV_SETTLS: u64 = 0x0008_0000;
pub const CV_CHILD_CLEARTID: u64 = 0x0020_0000;
pub const CV_CHILD_SETTID: u64 = 0x0100_0000;
pub const CV_PARENT_SETTID: u64 = 0x0800_0000;
/// 允许位掩码。
pub const ALLOWED: u64 = CV_VM
    | CV_FS
    | CV_FILES
    | CV_SIGHAND
    | CV_THREAD
    | CV_SYSVSEM
    | CV_SETTLS
    | CV_CHILD_CLEARTID
    | CV_CHILD_SETTID
    | CV_PARENT_SETTID;
/// 退出信号闭集（SIGCHLD=17 或 0）。
pub const SIGCHLD: u64 = 17;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_70;

/// clone 六参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloneSpec {
    pub stack: u64,
    pub tls: u64,
    pub ctid: u64,
    pub ptid: u64,
    pub flags: u64,
    pub arg: u64,
}

impl CloneSpec {
    /// 是否建线程（CLONE_THREAD）。
    pub fn threaded(&self) -> bool {
        self.flags & CV_THREAD != 0
    }

    /// 六参数逐项呈现（联签缺项自查）。
    pub fn fields(&self) -> [u64; 6] {
        [self.stack, self.tls, self.ctid, self.ptid, self.flags, self.arg]
    }

    /// 渲染：`S.. T.. C.. P.. F.. A..`。
    pub fn render(&self) -> String {
        format!(
            "S{:x} T{:x} C{:x} P{:x} F{:x} A{:x}",
            self.stack, self.tls, self.ctid, self.ptid, self.flags, self.arg
        )
    }
}

/// 线程账：clone 参数入账 + 号分配 + ctid 绑定表。
#[derive(Clone, Debug)]
pub struct ThreadBook {
    pub specs: Vec<CloneSpec>,
    pub tids: Vec<u32>,
    pub next_tid: u32,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ThreadBook {
    pub fn new() -> ThreadBook {
        ThreadBook { specs: Vec::new(), tids: Vec::new(), next_tid: 100, accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 六参数核验并建线程。判定序：封册 → flags → arg → 栈 → TLS
    /// → tid 区 → ctid 重绑 → 定容。
    pub fn clone_task(&mut self, sp: CloneSpec) -> Result<u32, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_FROZEN);
        }
        if sp.flags & !ALLOWED != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_FLAGS);
        }
        let sig = sp.flags & 0xff;
        if sig != 0 && sig != SIGCHLD {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_FLAGS);
        }
        if sp.arg == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_ARG);
        }
        if sp.threaded() {
            if sp.stack == 0 || sp.stack & (ALIGN16 - 1) != 0 || sp.stack >= USER_HI {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CL_STACK);
            }
        } else if sp.stack != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_STACK);
        }
        if sp.flags & CV_SETTLS != 0 {
            if sp.tls == 0 || sp.tls & (ALIGN16 - 1) != 0 || sp.tls >= USER_HI {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CL_TLS);
            }
        } else if sp.tls != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_TLS);
        }
        if sp.flags & CV_CHILD_SETTID != 0 {
            if sp.ctid < USER_LO || sp.ctid >= USER_HI || sp.ctid & 3 != 0 {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CL_TID);
            }
        } else if sp.ctid != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_TID);
        }
        if sp.flags & CV_PARENT_SETTID != 0 {
            if sp.ptid < USER_LO || sp.ptid >= USER_HI || sp.ptid & 3 != 0 {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CL_TID);
            }
        } else if sp.ptid != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_TID);
        }
        if sp.ctid != 0 {
            let mut k = 0usize;
            while k < self.specs.len() {
                if let Some(o) = self.specs.get(k) {
                    if o.ctid == sp.ctid {
                        self.rejects = self.rejects.wrapping_add(1);
                        return Err(CL_DUP);
                    }
                }
                k = k.wrapping_add(1);
            }
        }
        if self.specs.len() >= THR_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CL_CAP);
        }
        let tid = self.next_tid;
        self.next_tid = tid.wrapping_add(1);
        self.specs.push(sp);
        self.tids.push(tid);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(tid)
    }

    /// 六参数联签自查（与 AI-14 逐项对齐，缺项即假）。
    pub fn joint_six(&self, idx: usize) -> [bool; 6] {
        let mut out = [false; 6];
        if let Some(sp) = self.specs.get(idx) {
            let f = sp.fields();
            let mut k = 0usize;
            while k < 6 {
                out[k] = match f.get(k) {
                    Some(v) => *v != 0 || k == 4 || k == 1,
                    None => false,
                };
                k = k.wrapping_add(1);
            }
        }
        out
    }

    /// FNV-1a 链指纹：六参数序敏感 + 号 + 计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.specs.len() {
            if let Some(sp) = self.specs.get(k) {
                let f = sp.fields();
                let mut j = 0usize;
                while j < 6 {
                    if let Some(v) = f.get(j) {
                        h = (h ^ *v).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
            }
            if let Some(t) = self.tids.get(k) {
                h = (h ^ *t as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h ^ (self.next_tid as u64)
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 线程样本：六参数齐备（建线程 + TLS + 双 SETTID）。
pub fn thread_spec(i: u32) -> CloneSpec {
    let n = i as u64;
    CloneSpec {
        stack: 0x7f00_0000u64.wrapping_add(n.wrapping_mul(0x1_0000)),
        tls: 0x7f10_0000u64.wrapping_add(n.wrapping_mul(0x100)),
        ctid: 0x7f20_0000u64.wrapping_add(n.wrapping_mul(0x10)),
        ptid: 0x7f20_0008u64.wrapping_add(n.wrapping_mul(0x10)),
        flags: CV_VM | CV_THREAD | CV_SIGHAND | CV_SETTLS | CV_CHILD_SETTID | CV_PARENT_SETTID,
        arg: 0x401000,
    }
}

/// 四线程基准账。
pub fn full_book() -> ThreadBook {
    let mut b = ThreadBook::new();
    let mut i = 1u32;
    while i <= 4 {
        let _ = b.clone_task(thread_spec(i));
        i = i.wrapping_add(1);
    }
    b
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) ctid 越出用户区（号域越界）
    let mut a = ThreadBook::new();
    let mut sa = thread_spec(1);
    sa.ctid = 0x8000_0000_0000;
    got[0] = a.clone_task(sa).err() == Some(CL_TID);
    // (b) ctid 重绑
    let mut b = ThreadBook::new();
    let _ = b.clone_task(thread_spec(1));
    got[1] = b.clone_task(thread_spec(1)).err() == Some(CL_DUP);
    // (c) 栈未 16 对齐
    let mut c = ThreadBook::new();
    let mut sc = thread_spec(1);
    sc.stack = sc.stack.wrapping_add(8);
    got[2] = c.clone_task(sc).err() == Some(CL_STACK);
    // (d) 封册后写账
    let mut d = full_book();
    let _ = d.freeze();
    got[3] = d.clone_task(thread_spec(9)).err() == Some(CL_FROZEN);
    got
}

/// clone 六参数族判据。
pub fn run_unx11270_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11270");

    // X1 四线程入账与号分配
    let x1 = {
        let b = full_book();
        b.specs.len() == 4 && b.tids == [100, 101, 102, 103] && b.next_tid == 104
    };
    s.add("X1-号分配", x1, "四线程号 100..103，游标 104");

    // X2 六参数联签零缺项
    let x2 = {
        let b = full_book();
        let mut all = true;
        let mut i = 0usize;
        while i < b.specs.len() {
            let f = b.joint_six(i);
            let mut k = 0usize;
            while k < 6 {
                if f.get(k) != Some(&true) {
                    all = false;
                }
                k = k.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        all
    };
    s.add("X2-六参数联签", x2, "栈/TLS/ctid/ptid/flags/arg 六项无缺");

    // X3 flags 闭集
    let x3 = {
        let mut a = ThreadBook::new();
        let mut s1 = thread_spec(1);
        s1.flags |= 1u64 << 40;
        let unknown = a.clone_task(s1).err() == Some(CL_FLAGS);
        let mut s2 = thread_spec(1);
        s2.flags |= 9;
        let sig = a.clone_task(s2).err() == Some(CL_FLAGS);
        unknown && sig
    };
    s.add("X3-flags闭集", x3, "未知位与退出信号 9 均拒");

    // X4 栈语义
    let x4 = {
        let mut a = ThreadBook::new();
        let mut z = thread_spec(1);
        z.stack = 0;
        let zero = a.clone_task(z).err() == Some(CL_STACK);
        let mut off = thread_spec(1);
        off.stack = off.stack.wrapping_add(4);
        let mis = a.clone_task(off).err() == Some(CL_STACK);
        let mut no = thread_spec(1);
        no.flags = 0;
        let gave = a.clone_task(no).err() == Some(CL_STACK);
        zero && mis && gave
    };
    s.add("X4-栈语义", x4, "零栈/未对齐/非线程给栈三路均拒");

    // X5 TLS 语义
    let x5 = {
        let mut a = ThreadBook::new();
        let mut z = thread_spec(1);
        z.tls = 0;
        let zero = a.clone_task(z).err() == Some(CL_TLS);
        let mut off = thread_spec(1);
        off.tls = off.tls.wrapping_add(8);
        let mis = a.clone_task(off).err() == Some(CL_TLS);
        let mut no = thread_spec(1);
        no.flags &= !CV_SETTLS;
        let gave = a.clone_task(no).err() == Some(CL_TLS);
        zero && mis && gave
    };
    s.add("X5-TLS语义", x5, "零 TP/未对齐/未置位给 TP 三路均拒");

    // X6 tid 语义
    let x6 = {
        let mut a = ThreadBook::new();
        let mut k = thread_spec(1);
        k.ctid = 0x8000_0000_0000;
        let kernel = a.clone_task(k).err() == Some(CL_TID);
        let mut u = thread_spec(1);
        u.ctid = u.ctid.wrapping_add(2);
        let mis = a.clone_task(u).err() == Some(CL_TID);
        let mut m = thread_spec(1);
        m.ctid = 0;
        let miss = a.clone_task(m).err() == Some(CL_TID);
        let _ = a.clone_task(thread_spec(1));
        let dup = a.clone_task(thread_spec(1)).err() == Some(CL_DUP);
        kernel && mis && miss && dup
    };
    s.add("X6-tid语义", x6, "核区/未对齐/缺 ctid/重绑四路均拒");

    // X7 入口指针
    let x7 = {
        let mut a = ThreadBook::new();
        let mut z = thread_spec(1);
        z.arg = 0;
        a.clone_task(z).err() == Some(CL_ARG)
    };
    s.add("X7-入口指针", x7, "零入口函数指针拒建");

    // X8 线程表定容
    let x8 = {
        let mut b = ThreadBook::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= THR_CAP as u32 {
            if b.clone_task(thread_spec(i)).is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && b.clone_task(thread_spec(33)).err() == Some(CL_CAP)
    };
    s.add("X8-线程表定容", x8, "32 线程后第 33 个报满");

    // X9 指纹确定
    let x9 = {
        let b = full_book();
        let a = b.fold();
        a == b.fold() && a != 0
    };
    s.add("X9-指纹确定", x9, "同账两次指纹一致且非零");

    // X10 封册幂等
    let x10 = {
        let mut b = full_book();
        let r1 = b.freeze();
        let r2 = b.freeze();
        let live = b.fold();
        b.specs.clear();
        let r3 = b.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != b.fold()
    };
    s.add("X10-封册幂等", x10, "二次封册回原根，账变不改根");

    // X11 封册拒写
    let x11 = {
        let mut b = full_book();
        let _ = b.freeze();
        b.clone_task(thread_spec(9)).err() == Some(CL_FROZEN)
    };
    s.add("X11-封册拒写", x11, "封册后建线程即拒");

    // X12 非线程 clone：六参数全零栈/TLS/ctid
    let x12 = {
        let mut b = ThreadBook::new();
        let plain = CloneSpec { stack: 0, tls: 0, ctid: 0, ptid: 0, flags: 0, arg: 0x401000 };
        b.clone_task(plain) == Ok(100)
    };
    s.add("X12-非线程clone", x12, "进程 clone 允许零栈零 TLS 零 tid");

    // X13 参数行明细
    let (x13, line13) = {
        let b = full_book();
        let sp = match b.specs.first() {
            Some(v) => *v,
            None => CloneSpec { stack: 0, tls: 0, ctid: 0, ptid: 0, flags: 0, arg: 0 },
        };
        let f = b.joint_six(0);
        let n = f.iter().filter(|x| **x).count();
        let line = format!("six={} {}", n, sp.render());
        (n == 6 && sp.threaded() && line.contains("S7f01"), line)
    };
    s.add_dyn("X13-参数行明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11270_flow() {
        let mut b = full_book();
        assert_eq!(b.specs.len(), 4);
        assert_eq!(b.tids, [100, 101, 102, 103]);
        assert_eq!(b.accepts, 4);
        assert_eq!(b.rejects, 0);
        assert!(b.joint_six(0) == [true; 6]);
        let r1 = b.freeze();
        let r2 = b.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(b.root == r1);
        assert!(b.clone_task(thread_spec(9)).err() == Some(CL_FROZEN));
    }

    #[test]
    fn unx11270_gates() {
        let mut b = ThreadBook::new();
        assert!(b.clone_task(thread_spec(1)).is_ok());
        assert!(b.clone_task(thread_spec(1)).err() == Some(CL_DUP));
        let mut u = thread_spec(2);
        u.flags |= 1u64 << 40;
        assert!(b.clone_task(u).err() == Some(CL_FLAGS));
        let mut z = thread_spec(2);
        z.arg = 0;
        assert!(b.clone_task(z).err() == Some(CL_ARG));
        let mut st = thread_spec(2);
        st.stack = 0;
        assert!(b.clone_task(st).err() == Some(CL_STACK));
        let mut sa = thread_spec(2);
        sa.stack = sa.stack.wrapping_add(4);
        assert!(b.clone_task(sa).err() == Some(CL_STACK));
        let mut t = thread_spec(2);
        t.tls = 0;
        assert!(b.clone_task(t).err() == Some(CL_TLS));
        let mut c = thread_spec(2);
        c.ctid = 0x8000_0000_0000;
        assert!(b.clone_task(c).err() == Some(CL_TID));
        let plain = CloneSpec { stack: 0, tls: 0, ctid: 0, ptid: 0, flags: 0, arg: 0x401000 };
        assert!(b.clone_task(plain).is_ok());
        assert!(b.freeze().is_some());
        assert!(b.clone_task(thread_spec(3)).err() == Some(CL_FROZEN));
        assert_eq!(b.accepts, 2);
        assert_eq!(b.rejects, 8);
    }

    #[test]
    fn unx11270_checks_green() {
        assert!(run_unx11270_checks().all_passed(), "UNX-F11270 判据未全绿");
    }
}
