//! UNX-F9182 setresuid/setfsuid 语义档（0x63680 段）
//!
//! 锚点：UNX-F9182-J1 三值账（每次 setresuid/setfsuid 落账：r/e/s
//! 旧三元 → 新三元、fsuid 变更、结果、留痕）；J2 三值语义（KEEP 保
//! 持 + 特权任意 + 非特权三值闸 + fsuid 特权专属四性质）；J3 三值指
//! 纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（setresuid/setfsuid 面）。
//!
//! 结构：身份四元组册（r/e/s/f）+ 翻转核（res 门 + fs 门）+ 翻转账
//! 本 + 折叠指纹 + 注入核（错码/空留痕/越权 fs/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63680 独占 0x20）。
pub const SEG_BASE: u32 = 0x63680;
pub const RS_UNK_MOVE: u32 = SEG_BASE + 0x01; // 越权翻转
pub const RS_BAD_ARG: u32 = SEG_BASE + 0x02; // 非法参数
pub const RS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RS_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RS_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1;
pub const R_COUNT: usize = 2;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM"];

/// 身份四元组。
#[derive(Clone, Copy, PartialEq)]
pub struct Creds4 {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
    pub fsuid: u32,
}

impl Creds4 {
    pub fn is_priv(&self) -> bool {
        self.euid == 0
    }

    pub fn target_ok(&self, v: u32) -> bool {
        v == self.ruid || v == self.euid || v == self.suid
    }
}

/// KEEP 哨兵。
pub const KEEP: u32 = u32::MAX;

/// 单条账。
#[derive(Clone)]
pub struct RsRec {
    pub seq: u32,
    pub op: usize, // 0=setresuid 1=setfsuid
    pub old: Creds4,
    pub new: Creds4,
    pub code: u32,
    pub note: String,
}

impl RsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < 2 && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "RS{}|{}|{:?}->{:?}|{}|{}",
            self.seq,
            if self.op == 0 { "resuid" } else { "fsuid" },
            (self.old.ruid, self.old.euid, self.old.suid, self.old.fsuid),
            (self.new.ruid, self.new.euid, self.new.suid, self.new.fsuid),
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 四值簿。
pub struct RsBook {
    pub creds: Creds4,
    pub ledger: Vec<RsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RsBook {
    pub fn new(r: u32, e: u32, su: u32, f: u32) -> RsBook {
        RsBook {
            creds: Creds4 { ruid: r, euid: e, suid: su, fsuid: f },
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// setresuid：KEEP 保持；特权任意；非特权三值闸。
    pub fn setresuid(&mut self, r: u32, e: u32, su: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        if note.is_empty() {
            return Err(RS_EMPTY_NOTE);
        }
        let old = self.creds;
        if !old.is_priv() {
            let ok_r = r == KEEP || old.target_ok(r);
            let ok_e = e == KEEP || old.target_ok(e);
            let ok_s = su == KEEP || old.target_ok(su);
            if !ok_r || !ok_e || !ok_s {
                self.record(0, old, old, R_EPERM, note);
                return Err(R_EPERM);
            }
        }
        let nr = if r == KEEP { old.ruid } else { r };
        let ne = if e == KEEP { old.euid } else { e };
        let ns = if su == KEEP { old.suid } else { su };
        self.creds = Creds4 { ruid: nr, euid: ne, suid: ns, fsuid: ne }; // fs 跟随 e
        self.record(0, old, self.creds, R_OK, note);
        Ok(())
    }

    /// setfsuid：仅特权（euid=0）可改。
    pub fn setfsuid(&mut self, f: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        if note.is_empty() {
            return Err(RS_EMPTY_NOTE);
        }
        let old = self.creds;
        let prev = old.fsuid;
        if !old.is_priv() {
            // Linux 语义：非特权静默不改（返回旧值），此处按 EPERM 档
            self.record(1, old, old, R_EPERM, note);
            return Err(R_EPERM);
        }
        self.creds.fsuid = f;
        self.record(1, old, self.creds, R_OK, note);
        Ok(prev)
    }

    fn record(&mut self, op: usize, old: Creds4, new: Creds4, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(RsRec { seq, op, old, new, code, note: note.to_string() });
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
        let mut h: u32 = 0x91820;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.new.ruid);
            h = h.wrapping_mul(31).wrapping_add(r.new.euid);
            h = h.wrapping_mul(31).wrapping_add(r.new.suid);
            h = h.wrapping_mul(31).wrapping_add(r.new.fsuid);
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

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码账
    let bad = RsRec {
        seq: 1,
        op: 0,
        old: Creds4 { ruid: 0, euid: 0, suid: 0, fsuid: 0 },
        new: Creds4 { ruid: 0, euid: 0, suid: 0, fsuid: 0 },
        code: 9,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = RsRec {
        seq: 2,
        op: 0,
        old: Creds4 { ruid: 0, euid: 0, suid: 0, fsuid: 0 },
        new: Creds4 { ruid: 0, euid: 0, suid: 0, fsuid: 0 },
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 非特权 setresuid 外来值
    let mut b = RsBook::new(1000, 1000, 1000, 1000);
    got[2] = b.setresuid(7, KEEP, KEEP, "inj-foreign").err() == Some(R_EPERM);
    // 4) 非特权 setfsuid
    got[3] = b.setfsuid(7, "inj-fs").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.setresuid(KEEP, KEEP, KEEP, "inj-frozen").err() == Some(RS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9182");
    let mut b = RsBook::new(1000, 1000, 1000, 1000);

    // J1 三值账非空
    let _ = b.setresuid(KEEP, KEEP, KEEP, "自检");
    let j1 = !b.ledger.is_empty();
    s.add("J1-三值账非空", j1, "setresuid 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/码闭集");

    // J3 KEEP 三保持：全 KEEP 无变化
    let j3 = {
        let mut u = RsBook::new(1, 2, 3, 2);
        let _ = u.setresuid(KEEP, KEEP, KEEP, "全保持");
        u.creds == Creds4 { ruid: 1, euid: 2, suid: 3, fsuid: 2 }
    };
    s.add("J3-KEEP三保持", j3, "全 KEEP 身份不动");

    // J4 特权任意
    let j4 = {
        let mut p = RsBook::new(0, 0, 0, 0);
        p.setresuid(42, 43, 44, "任意").is_ok()
            && p.creds.ruid == 42
            && p.creds.euid == 43
            && p.creds.suid == 44
    };
    s.add("J4-特权任意", j4, "root 三值任意设");

    // J5 非特权三值闸
    let j5 = b.setresuid(7, KEEP, KEEP, "外来").err() == Some(R_EPERM);
    s.add("J5-三值闸", j5, "非特权外来值必 EPERM");

    // J6 fs 跟随 euid
    let j6 = {
        let mut p = RsBook::new(0, 0, 0, 0);
        let _ = p.setresuid(42, 43, 44, "a");
        p.creds.fsuid == 43
    };
    s.add("J6-fs跟随e", j6, "setresuid 后 fsuid=euid");

    // J7 fsuid 特权专属
    let j7 = b.setfsuid(7, "非特权fs").err() == Some(R_EPERM);
    s.add("J7-fs特权专属", j7, "非特权 setfsuid 必 EPERM");

    // J8 fsuid 特权放行 + 返回旧值
    let j8 = {
        let mut p = RsBook::new(0, 0, 0, 7);
        p.setfsuid(9, "特权fs") == Ok(7) && p.creds.fsuid == 9
    };
    s.add("J8-fs特权放行", j8, "特权改 fs 返回旧值");

    // J9 三值合法放行
    let j9 = {
        let mut u = RsBook::new(1000, 1001, 1001, 1001);
        u.setresuid(1001, KEEP, KEEP, "合法").is_ok()
    };
    s.add("J9-三值放行", j9, "uid∈{r,e,s} 放行");

    // J10 双门混合流
    let j10 = {
        let mut p = RsBook::new(0, 0, 0, 0);
        let _ = p.setresuid(1000, 1000, 0, "降权留 s=0");
        // euid=1000 非特权，但 suid=0 可复权
        let re = p.setresuid(KEEP, 0, KEEP, "复权");
        re.is_ok() && p.creds.euid == 0
    };
    s.add("J10-复权路径", j10, "suid=0 复权路径通");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.setresuid(KEEP, KEEP, KEEP, "").err() == Some(RS_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

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
    fn unx9182_res_flow() {
        // 特权任意
        let mut p = RsBook::new(0, 0, 0, 0);
        assert!(p.setresuid(42, 43, 44, "a").is_ok());
        assert_eq!(p.creds.fsuid, 43);
        assert_eq!(p.setfsuid(9, "b"), Ok(43));
        // 非特权
        let mut u = RsBook::new(1000, 1000, 1000, 1000);
        assert!(u.setresuid(KEEP, KEEP, KEEP, "c").is_ok());
        assert_eq!(u.setresuid(7, KEEP, KEEP, "d"), Err(R_EPERM));
        assert_eq!(u.setfsuid(7, "e"), Err(R_EPERM));
        // 复权
        let mut w = RsBook::new(1000, 1000, 0, 1000);
        assert!(w.setresuid(KEEP, 0, KEEP, "f").is_ok());
    }

    #[test]
    fn unx9182_checks_green() {
        let s = run_unx9182_checks();
        assert!(s.all_passed(), "UNX-F9182 判据未全绿");
    }
}
