//! UNX-F10255 反作弊三模式复核（伪造/跳跑/篡改检出）（0x13F20 段）
//!
//! 锚点：UNX-F10255-J1 反作弊复核达成（伪造结果/跳跑判据/篡改留痕三
//! 型作弊逐一复核检出 10/10 次）；J2 干净面零误报（合规批次复核零告
//! 警）；J3 复核留痕（每型告警带对象/型别/证据三要素）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：批次留痕账（判据执行流水）+ 复核核（三型审计规则真源）+
//! 告警账（三要素）+ 干净对照组 + 注入核（三型作弊必全中）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x13F20 独占 0x20）。
pub const SEG_BASE: u32 = 0x13F20;
pub const AC_FORGE: u8 = 0; // 伪造结果
pub const AC_SKIP: u8 = 1; // 跳跑判据
pub const AC_TAMPER: u8 = 2; // 篡改留痕
pub const AC_N: u8 = 3;

pub fn mode_name(m: u8) -> &'static str {
    match m {
        AC_FORGE => "forge",
        AC_SKIP => "skip",
        _ => "tamper",
    }
}

/// 判据执行流水条目。
#[derive(Clone, PartialEq)]
pub struct RunLog {
    pub ck: u32,
    pub ran: bool,
    pub passed: bool,
    pub stamp: u64, // 执行时点（逻辑钟，篡改检测依据）
    pub hash: u32,  // 结果哈希（留痕完整性依据）
}

impl RunLog {
    pub fn ok(&self) -> bool {
        self.ck != 0 && self.hash != 0
    }
}

/// 复核核真源：单条审计。
/// 返回 Some(型别) 表示作弊嫌疑。
pub fn audit_one(r: &RunLog, prev_max_stamp: u64) -> Option<u8> {
    if !r.ok() {
        return Some(AC_TAMPER); // 零哈希 = 留痕被抹
    }
    if !r.ran && r.passed {
        return Some(AC_FORGE); // 未跑但通过 = 伪造
    }
    if !r.ran && !r.passed {
        return Some(AC_SKIP); // 未跑 = 跳跑
    }
    if r.stamp < prev_max_stamp {
        return Some(AC_TAMPER); // 时点倒退 = 留痕被改
    }
    None
}

/// 复核账。
pub struct AuditBook {
    rows: Vec<RunLog>,
    alerts: Vec<(u32, u8, String)>, // (ck, 型别, 证据)
    frozen: bool,
}

impl AuditBook {
    pub fn new() -> AuditBook {
        AuditBook { rows: Vec::new(), alerts: Vec::new(), frozen: false }
    }

    /// 登流水：零号/冻结后写拒收。
    pub fn log(&mut self, r: RunLog) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if r.ck == 0 {
            return Err("zero_ck");
        }
        if self.rows.iter().any(|x| x.ck == r.ck) {
            return Err("dup_ck");
        }
        self.rows.push(r);
        Ok(())
    }

    /// 全量复核：三型审计规则真源执行。
    pub fn audit(&mut self) -> usize {
        self.alerts.clear();
        let mut prev_max: u64 = 0;
        for r in &self.rows {
            if let Some(m) = audit_one(r, prev_max) {
                let evid = format!("ck={}|stamp={}|hash={}", r.ck, r.stamp, r.hash);
                self.alerts.push((r.ck, m, evid));
            }
            if r.stamp > prev_max {
                prev_max = r.stamp;
            }
        }
        self.alerts.len()
    }

    /// 告警三要素完备。
    pub fn alerts_complete(&self) -> bool {
        self.alerts
            .iter()
            .all(|(ck, m, e)| *ck != 0 && *m < AC_N && e.contains("ck="))
    }

    /// 型别分布。
    pub fn by_mode(&self, m: u8) -> usize {
        self.alerts.iter().filter(|(_, x, _)| *x == m).count()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 干净基线：三条合规流水。
pub fn seed(b: &mut AuditBook) -> Result<(), &'static str> {
    let seeds: [(u32, u64, u32); 3] = [(1, 10, 0x0a1), (2, 20, 0x0b2), (3, 30, 0x0c3)];
    for (ck, st, h) in seeds {
        b.log(RunLog {
            ck,
            ran: true,
            passed: true,
            stamp: st,
            hash: h,
        })?;
    }
    Ok(())
}

/// 三型作弊注入器。
pub fn inject_cheat(b: &mut AuditBook, mode: u8, ck: u32) -> Result<(), &'static str> {
    match mode {
        AC_FORGE => b.log(RunLog {
            ck,
            ran: false,
            passed: true,
            stamp: 40,
            hash: 0x1234,
        }),
        AC_SKIP => b.log(RunLog {
            ck,
            ran: false,
            passed: false,
            stamp: 50,
            hash: 0x2345,
        }),
        AC_TAMPER => b.log(RunLog {
            ck,
            ran: true,
            passed: true,
            stamp: 5, // 时点倒退（基线已到 30）
            hash: 0x0,
        }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10255_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10255");

    // J1 三型逐一复核检出
    let j1 = {
        let mut hit = 0;
        for m in 0..AC_N {
            let mut b = AuditBook::new();
            let _ = seed(&mut b);
            let _ = inject_cheat(&mut b, m, 100);
            b.audit();
            if b.by_mode(m) >= 1 {
                hit += 1;
            }
        }
        hit == 3
    };
    s.add("J1-三型全中", j1, "伪造/跳跑/篡改全检出");

    // J2 干净面零误报
    let j2 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        b.audit() == 0 && b.alerts.is_empty()
    };
    s.add("J2-干净零误报", j2, "合规流水零告警");

    // J3 告警三要素完备
    let j3 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        let _ = inject_cheat(&mut b, AC_FORGE, 101);
        let _ = inject_cheat(&mut b, AC_TAMPER, 102);
        b.audit();
        b.alerts_complete() && b.audit_len() >= 2
    };
    s.add("J3-告警三要素", j3, "对象/型别/证据全备");

    // J4 伪造与跳跑互补
    let j4 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        let _ = inject_cheat(&mut b, AC_FORGE, 103);
        let _ = inject_cheat(&mut b, AC_SKIP, 104);
        b.audit();
        b.by_mode(AC_FORGE) == 1 && b.by_mode(AC_SKIP) == 1
    };
    s.add("J4-两型互补", j4, "ran=false 两路各归其型");

    // J5 时点倒退篡改检出
    let j5 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        let _ = inject_cheat(&mut b, AC_TAMPER, 105);
        b.audit();
        b.by_mode(AC_TAMPER) == 1
    };
    s.add("J5-时点倒退", j5, "stamp 倒退进篡改桶");

    // J6 零哈希留痕抹除检出
    let j6 = {
        let mut b = AuditBook::new();
        let _ = b.log(RunLog {
            ck: 1,
            ran: true,
            passed: true,
            stamp: 1,
            hash: 0,
        });
        b.audit();
        b.by_mode(AC_TAMPER) == 1
    };
    s.add("J6-零哈希检出", j6, "留痕被抹进篡改桶");

    // J7 重号与零号拒收
    let j7 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        let dup = b.log(RunLog {
            ck: 1,
            ran: true,
            passed: true,
            stamp: 99,
            hash: 1,
        });
        let zero = b.log(RunLog {
            ck: 0,
            ran: true,
            passed: true,
            stamp: 99,
            hash: 1,
        });
        dup == Err("dup_ck") && zero == Err("zero_ck")
    };
    s.add("J7-重号零号拒", j7, "双红线精确拒因");

    // J8 冻结红线
    let j8 = {
        let mut b = AuditBook::new();
        let _ = seed(&mut b);
        b.freeze();
        b.is_frozen() && b.log(RunLog { ck: 9, ran: true, passed: true, stamp: 1, hash: 1 }) == Err("frozen")
    };
    s.add("J8-冻结拒收", j8, "印后登流水必抓");

    // J9 时点正常递进不误报：等值时点合法（同批并发）
    let j9 = {
        let mut b = AuditBook::new();
        let _ = b.log(RunLog { ck: 1, ran: true, passed: true, stamp: 10, hash: 1 });
        let _ = b.log(RunLog { ck: 2, ran: true, passed: true, stamp: 10, hash: 2 });
        b.audit() == 0
    };
    s.add("J9-等值时点合法", j9, "stamp==prev 不算倒退");

    // J10 十轮复核回放：三型注入 10 轮检出模式集合稳定
    let j10 = {
        let mk = || {
            let mut b = AuditBook::new();
            let _ = seed(&mut b);
            for m in 0..AC_N {
                let _ = inject_cheat(&mut b, m, 200 + m as u32);
            }
            b.audit();
            let mut modes: Vec<u8> = b.alerts.iter().map(|(_, m, _)| *m).collect();
            modes.sort();
            modes.dedup();
            modes
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.len() == 3
    };
    s.add("J10-十轮三型", j10, "10/10 轮三型集合稳定");

    s
}

/// 复核账告警数辅助。
impl AuditBook {
    pub fn audit_len(&self) -> usize {
        self.alerts.len()
    }
}
