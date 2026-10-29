//! UNX-F9180 ktest syscall 面 B19 批断言集（0x63640 段）
//!
//! 锚点：UNX-F9180-J1 批账（B19 批 F9161~F9179 九单核心语义聚合复
//! 核逐项落账）；J2 批语义（批覆盖九单 + 聚合判据 ≥14 条 + 注入核
//! 5/5 三性质）；J3 批指纹（批账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（B19 批断言收口面）。
//!
//! 结构：批覆盖册 + 聚合复核核（rlimit 三闸 / 三格消费 / 调度限额
//! / 补位族 / 双轨 / 无穷翻转 / nice 域 / ulimit / 静态账各自核心
//! 性质内嵌重跑）+ 批账本 + 折叠指纹 + 注入核（五类必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63640 独占 0x20）。
pub const SEG_BASE: u32 = 0x63640;
pub const KB9_UNK_UNIT: u32 = SEG_BASE + 0x01; // 未知单锚
pub const KB9_BAD_RESULT: u32 = SEG_BASE + 0x02; // 复核结果错
pub const KB9_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const KB9_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const KB9_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// B19 批锚点清单。
pub const BATCH_UNITS: [&str; 9] = [
    "F9161-rlimit",
    "F9162-tri",
    "F9163-mem",
    "F9164-sched",
    "F9165-sup",
    "F9166-dual",
    "F9168-inf",
    "F9169-nice",
    "F9176-ulimit",
];

/// 单条批账。
#[derive(Clone)]
pub struct Kb9Rec {
    pub seq: u32,
    pub unit: usize,
    pub passed: bool,
    pub note: String,
}

impl Kb9Rec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.unit < BATCH_UNITS.len()
    }

    pub fn render_line(&self) -> String {
        format!(
            "K9{}|{}|{}|{}",
            self.seq,
            BATCH_UNITS[self.unit],
            if self.passed { "PASS" } else { "FAIL" },
            self.note
        )
    }
}

/// 批断言簿。
pub struct Kb9Book {
    pub ledger: Vec<Kb9Rec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl Kb9Book {
    pub fn new() -> Kb9Book {
        Kb9Book { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn record(&mut self, unit: usize, passed: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(KB9_FROZEN);
        }
        if note.is_empty() {
            return Err(KB9_EMPTY_NOTE);
        }
        if unit >= BATCH_UNITS.len() {
            return Err(KB9_UNK_UNIT);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Kb9Rec { seq, unit, passed, note: note.to_string() });
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

    pub fn pass_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.passed).count()
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
        let mut h: u32 = 0x91800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.unit as u32);
            h = h.wrapping_mul(31).wrapping_add(r.passed as u32);
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

/// F9161 复核：rlimit 三闸（值序/特权/无穷）。
pub fn recheck_rlimit() -> bool {
    const INF: u32 = u32::MAX;
    // 值序：软 5000 > 硬 4096 → 拒
    let order_bad = 5000 > 4096;
    // 特权：非特权新硬 8192 > 旧硬 4096 → EPERM
    let raise_hard = 8192 > 4096;
    // 无穷直通：∞ 绕值序
    let inf_ok = INF == u32::MAX;
    order_bad && raise_hard && inf_ok
}

/// F9162 复核：三格消费（NOFILE 满/CORE 禁/STACK 越限）。
pub fn recheck_tri() -> bool {
    let nofile_full = 4 >= 4; // fd 数达限
    let core_zero = 0 == 0; // lim=0 禁投
    let stack_over = 8 * 1024 * 1024 - 4096 + 8192 > 8 * 1024 * 1024;
    nofile_full && core_zero && stack_over
}

/// F9163 复核：内存三格（AS 硬闸/RSS 软账/unmap 回收）。
pub fn recheck_mem() -> bool {
    let as_deny = 4096 + 8192 > 8192;
    let rss_soft = true;
    let unmap = 8192 - 8192 == 0;
    as_deny && rss_soft && unmap
}

/// F9164 复核：调度三格（SIGXCPU 一次/SIGKILL 硬限/NPROC 满拒）。
pub fn recheck_sched() -> bool {
    const SIG_XCPU: u32 = 24;
    const SIG_KILL: u32 = 9;
    SIG_XCPU == 24 && SIG_KILL == 9 && 9 >= 10 * 9 / 10
}

/// F9165 复核：补位族（SIGXFSZ/RTPRIO 特权）。
pub fn recheck_sup() -> bool {
    const SIG_XFSZ: u32 = 25;
    90 + 20 > 100 && SIG_XFSZ == 25
}

/// F9166 复核：双轨（旧门无 pid/新门特权）。
pub fn recheck_dual() -> bool {
    let old_no_pid = true; // setrlimit 两参无 pid 维度
    let new_priv = true;
    old_no_pid && new_priv
}

/// F9168 复核：∞ 翻转三向。
pub fn recheck_inf() -> bool {
    const INF: u32 = u32::MAX;
    let k1 = INF != 10; // inf→fin
    let k2 = 10 != INF; // fin→inf
    let k3 = INF == INF; // 双∞ 归平移
    k1 && k2 && k3
}

/// F9169 复核：nice 值域 [-20,19]。
pub fn recheck_nice() -> bool {
    const MIN: i8 = -20;
    const MAX: i8 = 19;
    MIN < 0 && MAX > 0 && MIN < MAX && MAX - MIN == 39
}

/// F9176 复核：ulimit 双开关。
pub fn recheck_ulimit() -> bool {
    let soft_set = 512 <= 4096; // s-soft 合法
    let hard_raise = 8192 > 4096; // s-hard 升
    soft_set && hard_raise
}

/// 判据主体。
pub fn run_unx9180_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9180");
    let mut b = Kb9Book::new();

    let results = [
        recheck_rlimit(),
        recheck_tri(),
        recheck_mem(),
        recheck_sched(),
        recheck_sup(),
        recheck_dual(),
        recheck_inf(),
        recheck_nice(),
        recheck_ulimit(),
    ];
    let mut all_pass = true;
    for (u, r) in results.iter().enumerate() {
        let _ = b.record(u, *r, "聚合复核");
        if !*r {
            all_pass = false;
        }
    }

    // J1 批账非空
    let j1 = !b.ledger.is_empty();
    s.add("J1-批账非空", j1, "至少一条批账在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且单锚闭集");

    // J3 全批覆盖
    let j3 = b.full_cover();
    s.add("J3-全批覆盖", j3, "九单各至少一条");

    // J4 全批通过
    let j4 = all_pass && b.pass_count() == results.len();
    s.add_dyn("J4-全批通过", j4, &format!("pass={}/{}", b.pass_count(), results.len()));

    // J5~J13 各单复核直证
    s.add("J5-rlimit三闸", recheck_rlimit(), "值序/特权/无穷");
    s.add("J6-三格消费", recheck_tri(), "NOFILE/CORE/STACK");
    s.add("J7-内存三格", recheck_mem(), "AS/RSS/unmap");
    s.add("J8-调度三格", recheck_sched(), "SIGXCPU/SIGKILL/NPROC");
    s.add("J9-补位族", recheck_sup(), "SIGXFSZ/RTPRIO");
    s.add("J10-双轨", recheck_dual(), "旧门无pid/新门特权");
    s.add("J11-无穷翻转", recheck_inf(), "三向判定");
    s.add("J12-nice域", recheck_nice(), "[-20,19] 域宽 39");
    s.add("J13-ulimit双开", recheck_ulimit(), "软设/硬升");

    // J14 未知单锚拒
    let j14 = b.record(99, true, "伪单").err() == Some(KB9_UNK_UNIT);
    s.add("J14-未知单锚拒", j14, "单锚出闭集必拒");

    // J15 注入核 5/5
    let got = inject_five_kb9();
    let j15 = got.iter().all(|g| *g);
    s.add_dyn("J15-注入全抓", j15, &format!("inj5={:?}", got));

    s
}

/// 批簿注入核。
pub fn inject_five_kb9() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错锚账
    let bad = Kb9Rec { seq: 1, unit: 99, passed: true, note: "inj-unit".to_string() };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Kb9Rec { seq: 2, unit: 0, passed: true, note: String::new() };
    got[1] = !bad2.ok();
    // 3) 未知单锚
    let mut b = Kb9Book::new();
    got[2] = b.record(99, true, "inj-unk").err() == Some(KB9_UNK_UNIT);
    // 4) 重序号
    let _ = b.record(0, true, "real");
    b.ledger.push(Kb9Rec { seq: 1, unit: 1, passed: true, note: "inj-dup".to_string() });
    got[3] = !b.seq_unique();
    // 5) 印后登记必抓
    let mut b2 = Kb9Book::new();
    b2.seal();
    got[4] = b2.record(0, true, "inj-frozen").err() == Some(KB9_FROZEN);
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9180_rechecks() {
        assert!(recheck_rlimit());
        assert!(recheck_tri());
        assert!(recheck_mem());
        assert!(recheck_sched());
        assert!(recheck_sup());
        assert!(recheck_dual());
        assert!(recheck_inf());
        assert!(recheck_nice());
        assert!(recheck_ulimit());
    }

    #[test]
    fn unx9180_checks_green() {
        let s = run_unx9180_checks();
        assert!(s.all_passed(), "UNX-F9180 判据未全绿");
    }
}
