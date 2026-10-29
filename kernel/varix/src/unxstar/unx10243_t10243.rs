//! UNX-F10243 假全绿五模式检出（逐一检出可复现）（0x125A0 段）
//!
//! 锚点：UNX-F10243-J1 五模式检出（伪造通过/跳跑判据/篡改账本/证据
//! 复用/静默截断五型假全绿逐一检出 10/10 次可复现）；J2 真全绿放行
//! （干净批次零误报）；J3 检出可复现（同注入同检出指纹稳定）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·全绿收口组）。
//!
//! 结构：五模式注入器（每型一个真实构造）+ 检出核（审计规则真源）+
//! 复现账（检出指纹）+ 干净基线对照组 + 注入核自反（五型必全中）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x125A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x125A0;
pub const FG_FORGE: u8 = 0; // 伪造通过
pub const FG_SKIP: u8 = 1; // 跳跑判据
pub const FG_TAMPER: u8 = 2; // 篡改账本
pub const FG_REUSE: u8 = 3; // 证据复用
pub const FG_TRUNC: u8 = 4; // 静默截断
pub const FG_N: u8 = 5;

/// 判据条目：运行位 + 哈希位（审计依据）。
#[derive(Clone, PartialEq)]
pub struct CkRec {
    pub name: String,
    pub ran: bool,     // 真的跑过
    pub passed: bool,  // 通过位
    pub cksum: u32,    // 结果校验和（篡改检测依据）
    pub sample_n: u32, // 样本量（截断检测依据）
    pub expect_n: u32, // 应有样本量
}

impl CkRec {
    pub fn honest(&self) -> bool {
        self.ran
            && (!self.passed || self.cksum != 0)
            && self.sample_n >= self.expect_n
            && !self.name.is_empty()
    }
}

/// 全绿申报册。
pub struct ClaimBook {
    rows: Vec<CkRec>,
}

impl ClaimBook {
    pub fn new() -> ClaimBook {
        ClaimBook { rows: Vec::new() }
    }

    pub fn put(&mut self, r: CkRec) {
        self.rows.push(r);
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 检出核真源：五型逐一审计。
    /// 返回检出清单 (mode, name)。
    pub fn audit(&self) -> Vec<(u8, String)> {
        let mut out = Vec::new();
        for r in &self.rows {
            // FG_FORGE：passed 但从未运行（伪造通过）
            if !r.ran && r.passed {
                out.push((FG_FORGE, r.name.clone()));
            }
            // FG_SKIP：整册有判据从未运行（跳跑）——ran=false 且未伪造
            // （单独列入，与 forge 互补：ran=false 且 passed=false 也是跳跑）
            if !r.ran && !r.passed {
                out.push((FG_SKIP, r.name.clone()));
            }
            // FG_TAMPER：ran 且 passed 但校验和为零（账本被改）
            if r.ran && r.passed && r.cksum == 0 {
                out.push((FG_TAMPER, r.name.clone()));
            }
            // FG_REUSE：同校验和出现两次以上（证据复用）
            let dup = self
                .rows
                .iter()
                .filter(|x| x.cksum == r.cksum && r.cksum != 0)
                .count();
            if dup >= 2 {
                out.push((FG_REUSE, r.name.clone()));
            }
            // FG_TRUNC：样本量不足应量（静默截断）
            if r.sample_n < r.expect_n {
                out.push((FG_TRUNC, r.name.clone()));
            }
        }
        out
    }

    /// 干净性：零检出。
    pub fn clean(&self) -> bool {
        self.audit().is_empty()
    }
}

/// 五型注入器：向册注入指定型假全绿。
pub fn inject_fake(b: &mut ClaimBook, mode: u8, tag: &str) {
    match mode {
        FG_FORGE => b.put(CkRec {
            name: format!("forge-{}", tag),
            ran: false,
            passed: true,
            cksum: 0x1234,
            sample_n: 10,
            expect_n: 10,
        }),
        FG_SKIP => b.put(CkRec {
            name: format!("skip-{}", tag),
            ran: false,
            passed: false,
            cksum: 0x2345,
            sample_n: 10,
            expect_n: 10,
        }),
        FG_TAMPER => b.put(CkRec {
            name: format!("tamper-{}", tag),
            ran: true,
            passed: true,
            cksum: 0,
            sample_n: 10,
            expect_n: 10,
        }),
        FG_REUSE => {
            b.put(CkRec {
                name: format!("reuse-a-{}", tag),
                ran: true,
                passed: true,
                cksum: 0x9999,
                sample_n: 10,
                expect_n: 10,
            });
            b.put(CkRec {
                name: format!("reuse-b-{}", tag),
                ran: true,
                passed: true,
                cksum: 0x9999,
                sample_n: 10,
                expect_n: 10,
            });
        }
        FG_TRUNC => b.put(CkRec {
            name: format!("trunc-{}", tag),
            ran: true,
            passed: true,
            cksum: 0x5555,
            sample_n: 3,
            expect_n: 10,
        }),
        _ => {}
    }
}

/// 干净基线：五条真判据（各自独立校验和）。
pub fn seed_clean(b: &mut ClaimBook) {
    let seeds: [(&str, u32, u32); 5] = [
        ("probe-all", 0x0aa1, 10),
        ("zerored-all", 0x0bb2, 12),
        ("compile-all", 0x0cc3, 8),
        ("writeback-all", 0x0dd4, 200),
        ("snapshot-all", 0x0ee5, 4),
    ];
    for (n, c, s) in seeds {
        b.put(CkRec {
            name: n.to_string(),
            ran: true,
            passed: true,
            cksum: c,
            sample_n: s,
            expect_n: s,
        });
    }
}

/// 复现指纹：检出清单折叠（同注入同指纹）。
pub fn detect_fp(b: &ClaimBook) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for (m, n) in b.audit() {
        for byte in format!("{}:{}", m, n).as_bytes() {
            h ^= *byte as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    h
}

/// 批内判据。
pub fn run_unx10243_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10243");

    // J1 五型逐一检出
    let j1 = {
        let mut hits = 0;
        for m in 0..FG_N {
            let mut b = ClaimBook::new();
            inject_fake(&mut b, m, "x");
            let aud = b.audit();
            if aud.iter().any(|(mode, _)| *mode == m) {
                hits += 1;
            }
        }
        hits == 5
    };
    s.add("J1-五型逐一检出", j1, "五型注入五型检出");

    // J2 真全绿放行（干净基线零误报）
    let j2 = {
        let mut b = ClaimBook::new();
        seed_clean(&mut b);
        b.clean()
    };
    s.add("J2-干净零误报", j2, "真判据全放行");

    // J3 检出可复现：同注入同指纹
    let j3 = {
        let mk = || {
            let mut b = ClaimBook::new();
            inject_fake(&mut b, FG_FORGE, "r");
            inject_fake(&mut b, FG_TRUNC, "r");
            detect_fp(&b)
        };
        mk() == mk() && mk() != 0
    };
    s.add("J3-指纹可复现", j3, "同注入同指纹非零");

    // J4 伪造与跳跑互补：ran=false 全部落入两型之一
    let j4 = {
        let mut b = ClaimBook::new();
        inject_fake(&mut b, FG_FORGE, "a");
        inject_fake(&mut b, FG_SKIP, "b");
        let aud = b.audit();
        let forged: Vec<&(u8, String)> = aud.iter().filter(|(m, _)| *m == FG_FORGE).collect();
        let skipped: Vec<&(u8, String)> = aud.iter().filter(|(m, _)| *m == FG_SKIP).collect();
        forged.len() == 1 && skipped.len() == 1
    };
    s.add("J4-互补两型", j4, "伪造/跳跑互不吞并");

    // J5 证据复用：同校验和两判据双双检出
    let j5 = {
        let mut b = ClaimBook::new();
        inject_fake(&mut b, FG_REUSE, "d");
        let aud = b.audit();
        aud.iter().filter(|(m, _)| *m == FG_REUSE).count() == 2
    };
    s.add("J5-复用双检出", j5, "复用双方均上榜");

    // J6 静默截断：样本不足精确点名
    let j6 = {
        let mut b = ClaimBook::new();
        inject_fake(&mut b, FG_TRUNC, "t");
        b.audit()
            .iter()
            .any(|(m, n)| *m == FG_TRUNC && n.contains("trunc-t"))
    };
    s.add("J6-截断点名", j6, "样本不足带名字上榜");

    // J7 篡改检出：零校验和上榜
    let j7 = {
        let mut b = ClaimBook::new();
        inject_fake(&mut b, FG_TAMPER, "p");
        b.audit().iter().any(|(m, _)| *m == FG_TAMPER)
    };
    s.add("J7-篡改检出", j7, "零校验和上榜");

    // J8 混合册：干净四条 + 五型各一 → 恰五型检出零误伤干净条
    let j8 = {
        let mut b = ClaimBook::new();
        seed_clean(&mut b);
        for m in 0..FG_N {
            inject_fake(&mut b, m, "mix");
        }
        let aud = b.audit();
        let modes: Vec<u8> = [FG_FORGE, FG_SKIP, FG_TAMPER, FG_REUSE, FG_TRUNC].to_vec();
        modes.iter().all(|m| aud.iter().any(|(x, _)| x == m))
            && !aud.iter().any(|(_, n)| n.contains("all"))
    };
    s.add("J8-混合零误伤", j8, "五型全中干净条不误伤");

    // J9 判据条目负样本：未跑且通过且零校验和的条目 honest 必假
    let j9 = {
        let bad = CkRec {
            name: "x".to_string(),
            ran: false,
            passed: true,
            cksum: 0,
            sample_n: 0,
            expect_n: 0,
        };
        let good = CkRec {
            name: "g".to_string(),
            ran: true,
            passed: true,
            cksum: 1,
            sample_n: 5,
            expect_n: 5,
        };
        !bad.honest() && good.honest()
    };
    s.add("J9-honest负样本", j9, "坏条目必假好条目必真");

    // J10 十轮五型回放：10 轮检出模式集合稳定（确定性）
    let j10 = {
        let mk = || {
            let mut b = ClaimBook::new();
            for m in 0..FG_N {
                inject_fake(&mut b, m, "loop");
            }
            let aud = b.audit();
            let mut modes: Vec<u8> = aud.iter().map(|(m, _)| *m).collect();
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
        ok == 10 && base.len() == 5
    };
    s.add("J10-十轮稳定", j10, "10/10 轮五型集合稳定");

    s
}
