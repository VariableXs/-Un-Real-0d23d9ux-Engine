//! UNX-F9181 setuid/setreuid 语义档（三身份翻转协议）（0x63660 段）
//!
//! 锚点：UNX-F9181-J1 翻转账（每次身份操作落账：ruid/euid/suid 旧
//! 三元 → 新三元、结果、留痕）；J2 翻转语义（特权任意翻 + 非特权
//! 三值闸 + setuid 特权全翻 + setreuid 负一保持四性质）；J3 翻转指
//! 纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（三身份翻转面）。
//!
//! 结构：身份三元组册 + 翻转核（setuid/setreuid 双门）+ 翻转账本 +
//! 折叠指纹 + 注入核（错码/空留痕/越权翻转/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63660 独占 0x20）。
pub const SEG_BASE: u32 = 0x63660;
pub const UI_UNK_MOVE: u32 = SEG_BASE + 0x01; // 越权翻转
pub const UI_BAD_ARG: u32 = SEG_BASE + 0x02; // 非法参数
pub const UI_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UI_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const UI_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1;
pub const R_EINVAL: u32 = 2;
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 身份三元组。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Creds {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
}

impl Creds {
    pub fn is_priv(&self) -> bool {
        self.euid == 0
    }

    /// 非特权目标值合法：∈ {ruid, euid, suid}。
    pub fn target_ok(&self, v: u32) -> bool {
        v == self.ruid || v == self.euid || v == self.suid
    }
}

/// 保持哨兵（setreuid 的 -1）。
pub const KEEP: u32 = u32::MAX;

/// 单条翻转账。
#[derive(Clone)]
pub struct UiRec {
    pub seq: u32,
    pub op: usize, // 0=setuid 1=setreuid
    pub old: Creds,
    pub new: Creds,
    pub code: u32,
    pub note: String,
}

impl UiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < 2 && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "UI{}|{}|{:?}->|{:?}|{}|{}",
            self.seq,
            if self.op == 0 { "setuid" } else { "setreuid" },
            (self.old.ruid, self.old.euid, self.old.suid),
            (self.new.ruid, self.new.euid, self.new.suid),
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 身份簿。
pub struct UiBook {
    pub creds: Creds,
    pub ledger: Vec<UiRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UiBook {
    pub fn new(ruid: u32, euid: u32, suid: u32) -> UiBook {
        UiBook {
            creds: Creds { ruid, euid, suid },
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// setuid 门：特权 → r=e=s=uid；非特权 → 仅 euid=uid（uid∈三值）。
    pub fn setuid(&mut self, uid: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UI_FROZEN);
        }
        if note.is_empty() {
            return Err(UI_EMPTY_NOTE);
        }
        let old = self.creds;
        if old.is_priv() {
            self.creds = Creds { ruid: uid, euid: uid, suid: uid };
            self.record(0, old, self.creds, R_OK, note);
            return Ok(());
        }
        if !old.target_ok(uid) {
            self.record(0, old, old, R_EPERM, note);
            return Err(R_EPERM);
        }
        // 非特权 setuid 只改 euid
        self.creds.euid = uid;
        self.record(0, old, self.creds, R_OK, note);
        Ok(())
    }

    /// setreuid 门：KEEP 保持；特权任意；非特权目标∈三值（且允许 r/e 互换）。
    pub fn setreuid(&mut self, r: u32, e: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UI_FROZEN);
        }
        if note.is_empty() {
            return Err(UI_EMPTY_NOTE);
        }
        let old = self.creds;
        let nr = if r == KEEP { old.ruid } else { r };
        let ne = if e == KEEP { old.euid } else { e };
        if !old.is_priv() {
            let r_ok = r == KEEP || old.target_ok(r);
            let e_ok = e == KEEP || old.target_ok(e);
            if !r_ok || !e_ok {
                self.record(1, old, old, R_EPERM, note);
                return Err(R_EPERM);
            }
        }
        self.creds = Creds { ruid: nr, euid: ne, suid: old.suid };
        self.record(1, old, self.creds, R_OK, note);
        Ok(())
    }

    fn record(&mut self, op: usize, old: Creds, new: Creds, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(UiRec { seq, op, old, new, code, note: note.to_string() });
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91810;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.new.ruid);
            h = h.wrapping_mul(31).wrapping_add(r.new.euid);
            h = h.wrapping_mul(31).wrapping_add(r.new.suid);
            h = h.wrapping_mul(31).wrapping_add(r.code);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：特权进程 (0,0,0)；非特权进程 (1000,1000,1000)。
pub fn mk_priv() -> UiBook {
    UiBook::new(0, 0, 0)
}

pub fn mk_user() -> UiBook {
    UiBook::new(1000, 1000, 1000)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码账
    let bad = UiRec {
        seq: 1,
        op: 0,
        old: Creds { ruid: 0, euid: 0, suid: 0 },
        new: Creds { ruid: 0, euid: 0, suid: 0 },
        code: 9,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = UiRec {
        seq: 2,
        op: 0,
        old: Creds { ruid: 0, euid: 0, suid: 0 },
        new: Creds { ruid: 0, euid: 0, suid: 0 },
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 非特权翻外来 uid
    let mut b = mk_user();
    got[2] = b.setuid(7, "inj-foreign").err() == Some(R_EPERM);
    // 4) 非特权 setreuid 外来值
    got[3] = b.setreuid(7, KEEP, "inj-re").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.setuid(1000, "inj-frozen").err() == Some(UI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9181");
    let mut b = mk_user();

    // J1 翻转账非空
    let _ = b.setuid(1000, "自翻");
    let j1 = !b.ledger.is_empty();
    s.add("J1-翻转账非空", j1, "set 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/码闭集");

    // J3 特权全翻：euid=0 → setuid(1001) 三值全变
    let j3 = {
        let mut p = mk_priv();
        let _ = p.setuid(1001, "降权");
        p.creds == Creds { ruid: 1001, euid: 1001, suid: 1001 }
    };
    s.add("J3-特权全翻", j3, "root setuid 三值齐变");

    // J4 非特权三值闸：外来 uid 拒
    let j4 = b.setuid(7, "外来").err() == Some(R_EPERM);
    s.add("J4-三值闸", j4, "非特权外来 uid 必 EPERM");

    // J5 非特权同值放行（euid=ruid）
    let j5 = {
        let mut u = mk_user();
        u.setuid(1000, "同值").is_ok()
    };
    s.add("J5-同值放行", j5, "uid∈{r,e,s} 放行");

    // J6 setreuid KEEP 保持
    let j6 = {
        let mut u = mk_user();
        let _ = u.setreuid(KEEP, 1000, "只改e");
        u.creds.ruid == 1000 && u.creds.euid == 1000
    };
    s.add("J6-KEEP保持", j6, "setreuid(-1,x) ruid 不动");

    // J7 setreuid 互换放行（r↔e）
    let j7 = {
        let mut u = UiBook::new(1000, 1001, 1001);
        u.setreuid(1001, 1000, "互换").is_ok()
            && u.creds.ruid == 1001
            && u.creds.euid == 1000
    };
    s.add("J7-互换放行", j7, "非特权 r/e 互换合法");

    // J8 特权 setreuid 任意
    let j8 = {
        let mut p = mk_priv();
        p.setreuid(42, 43, "任意").is_ok() && p.creds.ruid == 42 && p.creds.euid == 43
    };
    s.add("J8-特权任意", j8, "root setreuid 任意值放行");

    // J9 非特权 setreuid 外来拒
    let j9 = b.setreuid(7, KEEP, "外来r").err() == Some(R_EPERM);
    s.add("J9-外来拒", j9, "setreuid 外来值必 EPERM");

    // J10 降权后失去特权：setuid(0,0,0) 后再 setreuid 任意被拒
    let j10 = {
        let mut p = mk_priv();
        let _ = p.setuid(1001, "降权");
        p.setreuid(42, 43, "已失权").err() == Some(R_EPERM)
    };
    s.add("J10-降权失权", j10, "降权后不再特权");

    // J11 suid 保留：非特权 setuid 后 suid 不动
    let j11 = {
        let mut u = UiBook::new(1000, 0, 0);
        // 非特权（euid=0? 不——euid=0 即特权）。改样：suid 保留在 setreuid
        let mut u2 = UiBook::new(1000, 1000, 0);
        // 非特权（euid=1000），setreuid(r=KEEP, e=0)：0 ∈ {r,e,s} 合法
        let _ = u2.setreuid(KEEP, 0, "复权");
        u2.creds.euid == 0 && u2.creds.suid == 0
    };
    s.add("J11-suid保留", j11, "suid 复权路径保持");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9181_uid_flow() {
        // 特权全翻
        let mut p = mk_priv();
        assert!(p.setuid(1001, "a").is_ok());
        assert_eq!(p.creds, Creds { ruid: 1001, euid: 1001, suid: 1001 });
        // 非特权
        let mut u = mk_user();
        assert!(u.setuid(1000, "b").is_ok());
        assert_eq!(u.setuid(7, "c"), Err(R_EPERM));
        // setreuid
        assert!(u.setreuid(KEEP, 1000, "d").is_ok());
        assert_eq!(u.setreuid(7, KEEP, "e"), Err(R_EPERM));
        // 互换
        let mut x = UiBook::new(1000, 1001, 1001);
        assert!(x.setreuid(1001, 1000, "f").is_ok());
    }

    #[test]
    fn unx9181_checks_green() {
        let s = run_unx9181_checks();
        assert!(s.all_passed(), "UNX-F9181 判据未全绿");
    }
}
