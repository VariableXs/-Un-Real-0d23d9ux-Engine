//! UNX-F10313 联签审计账（双向操作留痕）（0x1b1a0 段）
//!
//! 锚点：UNX-F10313-J1 双向留痕——A→B 与 B→A 逐事件落账，序号严格
//! 递增，双向计数对平且涉题主体两向齐备。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：事件账（方向+操作码+主体，序号自动配发）+ 平衡核（双向计数
//! 相等且非零）+ 序核（seq 严格单调）+ 主体核（涉题主体双向留痕齐备）+
//! 注入核（方向非法/单向失衡/封账后写/空主体四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b1a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b1a0;
pub const AU_BADDIR: u32 = SEG_BASE + 0x01; // 方向非法
pub const AU_BADOP: u32 = SEG_BASE + 0x02; // 操作码非法
pub const AU_EMPTY: u32 = SEG_BASE + 0x03; // 主体空
pub const AU_CAP: u32 = SEG_BASE + 0x04; // 账超容
pub const AU_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账
pub const AU_UNBAL: u32 = SEG_BASE + 0x06; // 双向失衡

/// 方向：1=A→B 2=B→A。
pub const DIR_A2B: u8 = 1;
pub const DIR_B2A: u8 = 2;
/// 操作码：1=登记 2=确认 3=变更 4=查账。
pub const OP_ENROLL: u8 = 1;
pub const OP_CONFIRM: u8 = 2;
pub const OP_CHANGE: u8 = 3;
pub const OP_QUERY: u8 = 4;

/// 审计账定容。
pub const CAP: usize = 24;
/// 双向事件对数锚（每向至少 6 条）。
pub const PER_DIR_NEEDED: usize = 6;

/// 单条审计事件。
#[derive(Clone)]
pub struct AuditEvent {
    pub seq: u64,
    pub dir: u8,
    pub op: u8,
    pub subject: String,
}

impl AuditEvent {
    pub fn ok(&self) -> bool {
        (self.dir == DIR_A2B || self.dir == DIR_B2A)
            && self.op >= OP_ENROLL
            && self.op <= OP_QUERY
            && !self.subject.is_empty()
    }
}

/// 联签审计账。
pub struct JointAudit {
    pub log: Vec<AuditEvent>,
    pub next_seq: u64,
    pub sealed: bool,
}

impl JointAudit {
    pub fn new() -> JointAudit {
        JointAudit { log: Vec::new(), next_seq: 1, sealed: false }
    }

    /// 记一条事件：封账、方向、操作码、空主体、超容均拒；序号自动配发。
    pub fn record(&mut self, dir: u8, op: u8, subject: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(AU_SEALED);
        }
        if dir != DIR_A2B && dir != DIR_B2A {
            return Err(AU_BADDIR);
        }
        if op < OP_ENROLL || op > OP_QUERY {
            return Err(AU_BADOP);
        }
        if subject.is_empty() {
            return Err(AU_EMPTY);
        }
        if self.log.len() >= CAP {
            return Err(AU_CAP);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.log.push(AuditEvent { seq, dir, op, subject: subject.to_string() });
        Ok(seq)
    }

    /// 方向计数。
    pub fn dir_count(&self, dir: u8) -> usize {
        self.log.iter().filter(|e| e.dir == dir).count()
    }

    /// 双向平衡：两向各达锚且计数相等。
    pub fn balanced(&self) -> bool {
        let a = self.dir_count(DIR_A2B);
        let b = self.dir_count(DIR_B2A);
        a >= PER_DIR_NEEDED && b >= PER_DIR_NEEDED && a == b
    }

    /// 序号严格单调核。
    pub fn seq_monotonic(&self) -> bool {
        let mut k = 1usize;
        while k < self.log.len() {
            if self.log[k].seq <= self.log[k - 1].seq {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 涉题主体双向留痕齐备。
    pub fn subject_covered(&self, subject: &str) -> bool {
        self.log.iter().any(|e| e.subject == subject && e.dir == DIR_A2B)
            && self.log.iter().any(|e| e.subject == subject && e.dir == DIR_B2A)
    }

    /// 主体留痕计数。
    pub fn subject_events(&self, subject: &str) -> usize {
        self.log.iter().filter(|e| e.subject == subject).count()
    }

    /// 涉题主体清单（按首现序去重）。
    pub fn subjects(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.log.len() {
            let s = &self.log[k].subject;
            if !v.iter().any(|x| x == s) {
                v.push(s.clone());
            }
            k += 1;
        }
        v
    }

    /// 某方向留痕序号列（序敏感）。
    pub fn dir_seqs(&self, dir: u8) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::new();
        let mut k = 0usize;
        while k < self.log.len() {
            if self.log[k].dir == dir {
                v.push(self.log[k].seq);
            }
            k += 1;
        }
        v
    }

    /// 事件行渲染。
    pub fn render(&self, k: usize) -> String {
        let e = &self.log[k];
        format!("#{}|{}|{}|{}", e.seq, e.dir, e.op, e.subject)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账折叠指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1b1a00;
        let mut k = 0usize;
        while k < self.log.len() {
            let e = &self.log[k];
            h = (h ^ e.seq).wrapping_mul(0x100000001b3);
            h = (h ^ e.dir as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.op as u64).wrapping_mul(0x100000001b3);
            let bytes = e.subject.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 方向非法必拒
    let mut a = JointAudit::new();
    got[0] = a.record(7, OP_ENROLL, "坏向").err() == Some(AU_BADDIR);
    // 2) 单向失衡必判不平衡
    let mut n = 0usize;
    while n < PER_DIR_NEEDED {
        let _ = a.record(DIR_A2B, OP_ENROLL, "单向主体");
        n += 1;
    }
    got[1] = !a.balanced();
    // 3) 空主体必拒
    got[2] = a.record(DIR_B2A, OP_CONFIRM, "").err() == Some(AU_EMPTY);
    // 4) 封账后写账必抓
    a.seal();
    got[3] = a.record(DIR_B2A, OP_CONFIRM, "迟到").err() == Some(AU_SEALED);
    got
}

/// 批内判据。
pub fn run_unx10313_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10313");

    // J1 正路径：六对双向事件逐条落账且平衡
    let mut a = JointAudit::new();
    let mut ok = true;
    let mut n = 0usize;
    while n < PER_DIR_NEEDED {
        let sub = format!("主体{}", n);
        if a.record(DIR_A2B, OP_ENROLL, &sub).is_err()
            || a.record(DIR_B2A, OP_CONFIRM, &sub).is_err()
        {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-双向六对落账", ok && a.balanced(), "A→B 与 B→A 各六条且计数对平");

    // J2 序号严格单调且从 1 起
    let j2 = a.seq_monotonic() && a.log.first().map(|e| e.seq) == Some(1);
    s.add("J2-序号单调", j2, "seq 从 1 起严格递增");

    // J3 主体双向齐备
    let j3 = {
        let mut ok = true;
        let mut n = 0usize;
        while n < PER_DIR_NEEDED {
            let sub = format!("主体{}", n);
            if !a.subject_covered(&sub) || a.subject_events(&sub) != 2 {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J3-主体双向齐", j3, "每个主体 A→B/B→A 双向留痕各一条");

    // J4 注入检出：坏向 + 单向失衡
    let inj = inject_four();
    s.add("J4-坏向失衡检出", inj[0] && inj[1], "方向非法必拒且单向必判失衡");

    // J5 注入检出：空主体 + 封账后写
    s.add("J5-空体封账检出", inj[2] && inj[3], "空主体必拒且封账后写必抓");

    // J6 操作码全谱：四码均可落账
    let j6 = {
        let mut c = JointAudit::new();
        c.record(DIR_A2B, OP_ENROLL, "o").is_ok()
            && c.record(DIR_B2A, OP_CONFIRM, "o").is_ok()
            && c.record(DIR_A2B, OP_CHANGE, "o").is_ok()
            && c.record(DIR_B2A, OP_QUERY, "o").is_ok()
    };
    s.add("J6-操作码全谱", j6, "登记/确认/变更/查账四码全可落账");

    // J7 操作码越界必拒
    let j7 = JointAudit::new().record(DIR_A2B, 0, "o").err() == Some(AU_BADOP)
        && JointAudit::new().record(DIR_A2B, 9, "o").err() == Some(AU_BADOP);
    s.add("J7-操作码越界", j7, "操作码出四码域必拒");

    // J8 容量边界
    let j8 = {
        let mut c = JointAudit::new();
        let mut filled = true;
        let mut n = 0usize;
        while n < CAP {
            let sub = format!("x{}", n);
            if c.record(DIR_A2B, OP_ENROLL, &sub).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.record(DIR_B2A, OP_QUERY, "over").err() == Some(AU_CAP)
    };
    s.add("J8-容量边界", j8, "账超容必拒且原账不损");

    // J9 封账不翻指纹
    let j9 = {
        let f0 = a.fold();
        a.seal();
        a.fold() == f0 && a.sealed
    };
    s.add("J9-封账指纹", j9, "封账不翻指纹且封单位翻转");

    // J10 事件行渲染含序号
    let j10 = a.render(0).contains("#1|") && !a.render(0).is_empty();
    s.add("J10-渲染含序号", j10, "事件行渲染带序号前缀");

    // J11 未知主体无留痕
    let j11 = !a.subject_covered("幽灵主体") && a.subject_events("幽灵主体") == 0;
    s.add("J11-幽灵无痕", j11, "未落账主体两向均无留痕");

    // J12 动态明细：双向计数落账
    s.add_dyn(
        "J12-双向计数",
        a.dir_count(DIR_A2B) == PER_DIR_NEEDED && a.dir_count(DIR_B2A) == PER_DIR_NEEDED,
        &format!("a2b={} b2a={}", a.dir_count(DIR_A2B), a.dir_count(DIR_B2A)),
    );

    // J13 动态明细：账指纹落账
    let fp = a.fold();
    s.add_dyn("J13-账指纹", fp != 0, &format!("fold={:#x}", fp));

    // J14 动态明细：主体清单与方向序号列
    let subs = a.subjects();
    let seqs = a.dir_seqs(DIR_A2B);
    let j14 = subs.len() == PER_DIR_NEEDED
        && seqs == [1u64, 3, 5, 7, 9, 11]
        && a.dir_seqs(DIR_B2A) == [2u64, 4, 6, 8, 10, 12];
    s.add_dyn(
        "J14-主体序列",
        j14,
        &format!("主体 {:?} A2B 序 {:?}", subs, seqs),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10313_audit_flow() {
        let mut a = JointAudit::new();
        assert_eq!(a.record(DIR_A2B, OP_ENROLL, "s"), Ok(1));
        assert_eq!(a.record(DIR_B2A, OP_CONFIRM, "s"), Ok(2));
        assert!(a.record(9, OP_ENROLL, "s").err() == Some(AU_BADDIR));
        assert!(a.record(DIR_A2B, 8, "s").err() == Some(AU_BADOP));
        assert!(a.record(DIR_A2B, OP_ENROLL, "").err() == Some(AU_EMPTY));
        assert!(a.subject_covered("s"));
        assert_eq!(a.subject_events("s"), 2);
        assert!(!a.subject_covered("ghost"));
        assert!(a.seq_monotonic());
        a.seal();
        assert!(a.record(DIR_B2A, OP_QUERY, "late").err() == Some(AU_SEALED));
    }

    #[test]
    fn unx10313_checks_green() {
        let s = run_unx10313_checks();
        assert!(s.all_passed(), "UNX-F10313 判据未全绿");
    }
}
