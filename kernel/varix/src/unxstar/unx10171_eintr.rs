//! UNX-F10171 睡眠中断 EINTR 正反双判据（中断剩余达成与无误报误用受控）（0x10880 段）
//!
//! 锚点：UNX-F10171-J1 中断剩余达成（信号中断→EINTR+rem 剩余 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：睡眠册（请求时长 × 中断点 × 剩余量）+ 睡眠核（nanosleep 语义：
//! 无信号睡满、有信号 EINTR 且 rem>0 严格小于请求）+ 中断账本 +
//! 折叠指纹 + 注入核（零剩余 EINTR 伪造/超额 rem/无信号误报/幽灵/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10880 独占 0x20）。
pub const SEG_BASE: u32 = 0x10880;
pub const SL_GHOST: u32 = SEG_BASE + 0x01; // 幽灵任务
pub const SL_ZERO: u32 = SEG_BASE + 0x02; // 零时长请求
pub const SL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const SL_EINTR: u32 = SEG_BASE + 0x04; // 信号中断
pub const SL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍睡眠

/// 睡眠失败面：错误码 + 剩余量（EINTR 时 rem ∈ (0, req)）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SleepFail {
    pub code: u32,
    pub rem: u32,
}

/// 睡眠成功面：睡满量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slept {
    pub ms: u32,
}

/// 一次睡眠的账本留痕。
#[derive(Clone)]
pub struct SlpRec {
    pub seq: u32,
    pub pid: u32,
    pub req: u32,
    pub interrupted: bool,
    pub rem: u32,
    pub note: String,
}

impl SlpRec {
    /// 账目合法性：留痕非空；中断时 rem 严格落在 (0, req)；
    /// 未中断时 rem 必为 0（无误报）。
    pub fn ok(&self) -> bool {
        if self.note.is_empty() || self.req == 0 {
            return false;
        }
        if self.interrupted {
            self.rem > 0 && self.rem < self.req
        } else {
            self.rem == 0
        }
    }

    pub fn render_line(&self) -> String {
        format!(
            "Z{}|p{}|r{}|int{}|rem{}|{}",
            self.seq, self.pid, self.req, self.interrupted as u8, self.rem, self.note
        )
    }
}

/// 睡眠册：任务注册表 + 挂起信号位 + 账本。
pub struct SleepBook {
    pub pids: Vec<u32>,
    /// 挂起中断：(pid, 中断时刻 ms)——先于睡眠请求登记。
    pub pending: Vec<(u32, u32)>,
    pub ledger: Vec<SlpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SleepBook {
    pub fn new() -> SleepBook {
        SleepBook {
            pids: Vec::new(),
            pending: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn enroll(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SL_FROZEN);
        }
        if pid == 0 || self.pids.contains(&pid) {
            return Err(SL_GHOST);
        }
        self.pids.push(pid);
        Ok(())
    }

    /// 预埋一次中断：pid 在睡眠进行到 at ms 时被信号打断。
    pub fn arm_signal(&mut self, pid: u32, at: u32) {
        self.pending.push((pid, at));
    }

    /// nanosleep 核心：有先期挂起信号即中断并回报剩余；无信号睡满。
    pub fn sleep(&mut self, pid: u32, req: u32, note: &str) -> Result<Slept, SleepFail> {
        if self.sealed {
            return Err(SleepFail { code: SL_FROZEN, rem: 0 });
        }
        if note.is_empty() {
            return Err(SleepFail { code: SL_EMPTY_NOTE, rem: 0 });
        }
        if !self.pids.contains(&pid) {
            return Err(SleepFail { code: SL_GHOST, rem: 0 });
        }
        if req == 0 {
            return Err(SleepFail { code: SL_ZERO, rem: 0 });
        }
        // 中断点检索：取该 pid 的首个挂起信号。
        let at = self.pending.iter().find(|p| p.0 == pid).map(|p| p.1);
        self.pending.retain(|p| p.0 != pid);
        let (interrupted, rem) = match at {
            Some(a) if a < req => (true, req - a),
            _ => (false, 0),
        };
        self.ledger.push(SlpRec {
            seq: self.seq_next,
            pid,
            req,
            interrupted,
            rem,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if interrupted {
            Err(SleepFail { code: SL_EINTR, rem })
        } else {
            Ok(Slept { ms: req })
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（37 进制卷积，本批独立常量）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10880;
        for r in &self.ledger {
            h = h.wrapping_mul(37).wrapping_add(r.seq);
            h = h.wrapping_mul(37).wrapping_add(r.pid);
            h = h.wrapping_mul(37).wrapping_add(r.req);
            h = h.wrapping_mul(37).wrapping_add(r.rem);
            h = h.wrapping_mul(37).wrapping_add(r.interrupted as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(37).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SleepBook::new();
    let _ = b.enroll(1);
    // 1) 伪造零剩余 EINTR：rem==0 的中断账必判非法。
    let forged = SlpRec {
        seq: 1,
        pid: 1,
        req: 100,
        interrupted: true,
        rem: 0,
        note: "inj-zero-rem".to_string(),
    };
    got[0] = !forged.ok();
    // 2) 超额 rem：rem >= req 必判非法。
    let forged2 = SlpRec {
        seq: 2,
        pid: 1,
        req: 100,
        interrupted: true,
        rem: 100,
        note: "inj-over-rem".to_string(),
    };
    got[1] = !forged2.ok();
    // 3) 无信号误报：未中断账却带剩余必判非法。
    let forged3 = SlpRec {
        seq: 3,
        pid: 1,
        req: 100,
        interrupted: false,
        rem: 7,
        note: "inj-false-eintr".to_string(),
    };
    got[2] = !forged3.ok();
    // 4) 幽灵任务睡眠。
    got[3] = b.sleep(99, 50, "inj-ghost").err().map(|f| f.code) == Some(SL_GHOST);
    // 5) 印账后睡眠必抓。
    b.seal();
    got[4] = b.sleep(1, 50, "inj-frozen").err().map(|f| f.code) == Some(SL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10171");

    // J1 中断剩余达成：100ms 睡眠在 30ms 被打断 → EINTR + rem 70。
    let j1 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        b.arm_signal(1, 30);
        let e = b.sleep(1, 100, "sig");
        e == Err(SleepFail { code: SL_EINTR, rem: 70 })
    };
    s.add("J1-中断剩余", j1, "EINTR 且 rem=70 精确回报");

    // J2 完整睡眠：无信号睡满 Ok(req)。
    let j2 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(2);
        b.sleep(2, 100, "full") == Ok(Slept { ms: 100 })
    };
    s.add("J2-完整睡眠", j2, "无中断睡满全程");

    // J3 无误报：无挂起信号绝不返回 EINTR。
    let j3 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(3);
        let r = b.sleep(3, 80, "clean");
        !matches!(r, Err(SleepFail { code: SL_EINTR, .. }))
    };
    s.add("J3-无误报", j3, "干净路径零 EINTR");

    // J4 rem 域性质：中断点采样扫描，rem 恒在 (0, req)。
    let j4 = {
        let mut ok = true;
        for at in [1u32, 13, 37, 61, 99] {
            let mut b = SleepBook::new();
            let _ = b.enroll(9);
            b.arm_signal(9, at);
            if let Err(f) = b.sleep(9, 100, "scan") {
                if f.code == SL_EINTR && (f.rem == 0 || f.rem >= 100) {
                    ok = false;
                }
            } else {
                ok = false;
            }
        }
        ok
    };
    s.add("J4-rem域性质", j4, "五采样点 rem 均在开区间");

    // J5 幽灵任务拒。
    let j5 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        b.sleep(77, 50, "ghost").err().map(|f| f.code) == Some(SL_GHOST)
    };
    s.add("J5-幽灵拒", j5, "未注册 pid 睡眠必拒");

    // J6 零时长拒。
    let j6 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        b.sleep(1, 0, "zero").err().map(|f| f.code) == Some(SL_ZERO)
    };
    s.add("J6-零时长拒", j6, "req=0 必拒");

    // J7 账本完整：混合中断/完整账全部合法。
    let j7 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        let _ = b.enroll(2);
        b.arm_signal(1, 10);
        let _ = b.sleep(1, 100, "a");
        let _ = b.sleep(2, 100, "b");
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "两类账均过合法性判据");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        let _ = b.sleep(1, 50, "x");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.sleep(1, 50, "post").err().map(|f| f.code) == Some(SL_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|g| *g);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10171_eintr_flow() {
        let mut b = SleepBook::new();
        let _ = b.enroll(1);
        let _ = b.enroll(2);
        b.arm_signal(2, 25);
        assert_eq!(b.sleep(1, 100, "ok"), Ok(Slept { ms: 100 }));
        assert_eq!(b.sleep(2, 100, "sig"), Err(SleepFail { code: SL_EINTR, rem: 75 }));
        assert!(b.ledger[1].interrupted);
        assert_eq!(b.sleep(9, 5, "g").err().map(|f| f.code), Some(SL_GHOST));
    }

    #[test]
    fn unx10171_checks_green() {
        let s = run_unx10171_checks();
        assert!(s.all_passed(), "UNX-F10171 判据未全绿");
    }
}
