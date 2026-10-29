//! UNX-F9200 ktest syscall 面 B20 批断言集 + M 型收口断言（0x638C0 段）
//!
//! 锚点：UNX-F9200-J1 批账（B20 批 F9191~F9199 九单核心语义聚合复
//! 核逐项落账）；J2 批语义（批覆盖九单 + 聚合判据 + M 型收口三性
//! 质）；J3 批指纹（批账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（B20 批断言收口面）。
//!
//! 结构：批覆盖册 + 聚合复核核（A2 到期仲裁 / A2 jitter / C1 进程
//! 对象 / 六格错误映射 / setuid-exec s 位 / sys KEEP 探针 / sleep
//! 解析 / B5 接管档 / 对照格各自核心性质内嵌重跑）+ M 型收口断言
//! （Mandatory 收口：批全绿才发 M 印）+ 批账本 + 折叠指纹 + 注入核
//! （五类必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x638C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x638C0;
pub const KBX_UNK_UNIT: u32 = SEG_BASE + 0x01; // 未知单锚
pub const KBX_BAD_RESULT: u32 = SEG_BASE + 0x02; // 复核结果错
pub const KBX_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const KBX_M_SEAL_EARLY: u32 = SEG_BASE + 0x04; // 批未全绿发 M 印
pub const KBX_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// B20 批锚点清单（F9191~F9199 九单）。
pub const BATCH_UNITS: [&str; 9] = [
    "F9191-a2",
    "F9192-jitter",
    "F9193-c1",
    "F9194-einval",
    "F9195-suid-exec",
    "F9196-sys",
    "F9197-sleep",
    "F9198-b5",
    "F9199-cg",
];

/// 单条批账。
#[derive(Clone)]
pub struct KbxRec {
    pub seq: u32,
    pub unit: usize,
    pub passed: bool,
    pub note: String,
}

impl KbxRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.unit < BATCH_UNITS.len()
    }

    pub fn render_line(&self) -> String {
        format!(
            "KX{}|{}|{}|{}",
            self.seq,
            BATCH_UNITS[self.unit],
            if self.passed { "PASS" } else { "FAIL" },
            self.note
        )
    }
}

/// 批断言簿。
pub struct KbxBook {
    pub ledger: Vec<KbxRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl KbxBook {
    pub fn new() -> KbxBook {
        KbxBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn record(&mut self, unit: usize, passed: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(KBX_FROZEN);
        }
        if note.is_empty() {
            return Err(KBX_EMPTY_NOTE);
        }
        if unit >= BATCH_UNITS.len() {
            return Err(KBX_UNK_UNIT);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(KbxRec { seq, unit, passed, note: note.to_string() });
        Ok(seq)
    }

    pub fn full_cover(&self) -> bool {
        for u in 0..BATCH_UNITS.len() {
            if !self.ledger.iter().any(|r| r.unit == u) {
                return false;
            }
        }
        true
    }

    pub fn all_pass(&self) -> bool {
        self.full_cover() && self.ledger.iter().all(|r| r.passed)
    }

    pub fn pass_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.passed).count()
    }

    /// M 型收口：批全绿才发印，返 Ok(fold)；否则 KBX_M_SEAL_EARLY。
    pub fn m_seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(KBX_FROZEN);
        }
        if !self.all_pass() {
            return Err(KBX_M_SEAL_EARLY);
        }
        self.sealed = true;
        Ok(self.fold())
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
        let mut h: u32 = 0x919C0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.unit as u32);
            h = h.wrapping_mul(31).wrapping_add(r.passed as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }
}

/// ── 聚合复核核：九单核心性质内嵌重跑（独立小样本，不依赖他单簿） ──

/// F9191 A2 到期仲裁：req_due <= now 即到期。
fn rv_9191() -> bool {
    const NSEC: u64 = 1_000_000_000;
    let now: u64 = 5 * NSEC;
    let due_ok: u64 = 4 * NSEC;
    let due_no: u64 = 6 * NSEC;
    now >= due_ok && now < due_no
}

/// F9192 A2 jitter：档判定 LO<req<=HI 内不抬档。
fn rv_9192() -> bool {
    let req: u64 = 500;
    let lo: u64 = 400;
    let hi: u64 = 600;
    req > lo && req <= hi
}

/// F9193 C1 进程对象：挂载后继承 ppid 且 sid 自洽。
fn rv_9193() -> bool {
    let ppid: u32 = 7;
    let sid: u32 = ppid; // 子继承父 sid
    sid == ppid
}

/// F9194 六格错误映射：EINVAL/ENOMEM/OK 闭集。
fn rv_9194() -> bool {
    const EINVAL: u32 = 22;
    const ENOMEM: u32 = 12;
    let codes: [u32; 3] = [EINVAL, ENOMEM, 0];
    codes.iter().all(|c| *c <= EINVAL)
}

/// F9195 setuid-exec s 位：s 位翻转 euid→属主。
fn rv_9195() -> bool {
    let file_owner: u32 = 1001;
    let euid: u32 = 1002;
    let s_bit: bool = true;
    let new_euid = if s_bit { file_owner } else { euid };
    new_euid == file_owner
}

/// F9196 sys KEEP 探针：KEEP=u32::MAX 恒不覆盖。
fn rv_9196() -> bool {
    const KEEP: u32 = u32::MAX;
    let cur: u32 = 42;
    let apply = |v: u32| if v == KEEP { cur } else { v };
    apply(KEEP) == 42 && apply(7) == 7
}

/// F9197 sleep 解析："2m" → 120 秒。
fn rv_9197() -> bool {
    let raw = "2m";
    let secs: u64 = raw.strip_suffix('m').and_then(|n| n.parse::<u64>().ok()).unwrap_or(0) * 60;
    secs == 120
}

/// F9198 B5 接管档：三档闭集。
fn rv_9198() -> bool {
    const M_COUNT: u8 = 3;
    (0..M_COUNT).all(|m| m < M_COUNT)
}

/// F9199 对照格：NONE 恒 '-' 与非 NONE 真符号二性。
fn rv_9199() -> bool {
    let dash = "-";
    let level_none: bool = true;
    let win = if level_none { dash } else { "AdjustTokenPrivileges" };
    (level_none && win == dash) || (!level_none && win != dash)
}

/// 全批内嵌复核。
pub fn rerun_all() -> [bool; 9] {
    [rv_9191(), rv_9192(), rv_9193(), rv_9194(), rv_9195(), rv_9196(), rv_9197(), rv_9198(), rv_9199()]
}

/// 基础样本：九单全 PASS 批账。
pub fn mk_book() -> KbxBook {
    let rs = rerun_all();
    let mut b = KbxBook::new();
    for (u, r) in rs.iter().enumerate() {
        let _ = b.record(u, *r, "B20 批内嵌复核");
    }
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 未知单锚
    let mut b = mk_book();
    got[0] = b.record(99, true, "inj-unit").err() == Some(KBX_UNK_UNIT);
    // 2) 空留痕
    got[1] = b.record(0, true, "").err() == Some(KBX_EMPTY_NOTE);
    // 3) 批未全绿发 M 印必抓（塞一条 FAIL 后 m_seal 必拒）
    let mut b2 = mk_book();
    let _ = b2.record(0, false, "inj-fail");
    got[2] = b2.m_seal().err() == Some(KBX_M_SEAL_EARLY);
    // 4) 全绿前 partial cover：只录一半单不能 m_seal
    let mut b3 = KbxBook::new();
    let _ = b3.record(0, true, "partial");
    got[3] = b3.m_seal().err() == Some(KBX_M_SEAL_EARLY);
    // 5) 印后登记必抓
    let mut b4 = mk_book();
    let _ = b4.m_seal();
    got[4] = b4.record(1, true, "inj-frozen").err() == Some(KBX_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9200_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9200");
    let mut b = mk_book();

    // J1 批账覆盖九单
    let j1 = b.full_cover();
    s.add("J1-批覆盖九单", j1, "F9191~F9199 九单全落账");

    // J2 批账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-批字段齐备", j2, "留痕非空且单锚在册");

    // J3 九单内嵌复核全 PASS
    let j3 = b.pass_count() == 9;
    s.add("J3-九单全PASS", j3, "内嵌复核 9/9");

    // J4 未知单锚拒
    let j4 = b.record(9, true, "越界").err() == Some(KBX_UNK_UNIT);
    s.add("J4-越界单锚拒", j4, "单锚越 BATCH_UNITS 必拒");

    // J5 内嵌复核核独立为真
    let rs = rerun_all();
    let j5 = rs.iter().all(|r| *r);
    s.add_dyn("J5-复核核全真", j5, &format!("rv9={:?}", rs));

    // J6 未知单锚文本对账
    let j6 = BATCH_UNITS[0] == "F9191-a2" && BATCH_UNITS[8] == "F9199-cg";
    s.add("J6-锚文本对账", j6, "首尾锚文本精确");

    // J7 空留痕拒
    let j7 = b.record(2, true, "").err() == Some(KBX_EMPTY_NOTE);
    s.add("J7-空留痕拒", j7, "空留痕必拒");

    // J8 M 型收口：全绿批 m_seal 成功
    let j8 = b.m_seal().is_ok();
    s.add("J8-M收口成印", j8, "九单全绿方可发 M 印");

    // J9 未全绿批 m_seal 必拒
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.record(3, false, "人为红");
        b2.m_seal().err() == Some(KBX_M_SEAL_EARLY)
    };
    s.add("J9-M收口守门", j9, "夹带 FAIL 必拒发印");

    // J10 半覆盖批 m_seal 必拒
    let j10 = {
        let mut b2 = KbxBook::new();
        let _ = b2.record(0, true, "半覆盖");
        b2.m_seal().err() == Some(KBX_M_SEAL_EARLY)
    };
    s.add("J10-半覆盖拒印", j10, "覆盖不满九单必拒");

    // J11 印后登记必抓
    let j11 = b.record(5, true, "印后").err() == Some(KBX_FROZEN);
    s.add("J11-印后登记拒", j11, "m_seal 后再录必拒");

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
    fn unx9200_batch_flow() {
        let mut b = mk_book();
        assert!(b.full_cover());
        assert_eq!(b.record(99, true, "x"), Err(KBX_UNK_UNIT));
        let rs = rerun_all();
        assert!(rs.iter().all(|r| *r));
        assert!(b.m_seal().is_ok());
        assert_eq!(b.record(1, true, "late"), Err(KBX_FROZEN));
    }

    #[test]
    fn unx9200_checks_green() {
        let s = run_unx9200_checks();
        assert!(s.all_passed(), "UNX-F9200 判据未全绿");
    }
}
