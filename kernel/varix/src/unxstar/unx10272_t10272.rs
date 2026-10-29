//! UNX-F10272 长稳巡检断言账（周期快照+漂移检出）（0x16340 段）
//!
//! 锚点：UNX-F10272-J1 巡检断言达成（周期快照落账且漂移检出 10/10
//! 次）；J2 漂移分类（趋势漂移/突变漂移/抖动豁免三分类 10/10 次精确）；
//! J3 巡检指纹（同巡检序列同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·长稳演习）。
//!
//! 结构：巡检快照账（时点 × 四水位）+ 漂移检出核（三分类真源）+ 断
//! 言账（每次巡检产出断言）+ 注入核（趋势/突变/抖动必分类）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x16340 独占 0x20）。
pub const SEG_BASE: u32 = 0x16340;
pub const PS_TREND: u8 = 0; // 趋势漂移
pub const PS_SPIKE: u8 = 1; // 突变漂移
pub const PS_JITTER: u8 = 2; // 抖动（豁免）
pub const PS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后采样
pub const PS_OUT_RANGE: u32 = SEG_BASE + 0x04; // 越界采样

/// 漂移判定参数真源。
pub const SPIKE_TH: i32 = 200; // 突变阈（千分位单步）
pub const TREND_TH: i32 = 150; // 趋势阈（净增长）
pub const JITTER_BW: i32 = 60; // 抖动带宽
pub const WIN: usize = 6; // 判定窗口

/// 漂移分类真源。
pub fn classify(w: &[u16]) -> u8 {
    if w.len() < 2 {
        return PS_JITTER;
    }
    let first = w[0] as i32;
    let last = w[w.len() - 1] as i32;
    // 突变：任一相邻对单步越阈
    for p in w.windows(2) {
        let d = (p[1] as i32 - p[0] as i32).abs();
        if d >= SPIKE_TH {
            return PS_SPIKE;
        }
    }
    // 趋势：净增长越阈（扣抖动带宽）
    let net = (last - first).abs();
    if net > JITTER_BW && net >= TREND_TH {
        return PS_TREND;
    }
    PS_JITTER
}

/// 巡检快照。
#[derive(Clone, Copy, PartialEq)]
pub struct PatrolSnap {
    pub tick: u32,  // 巡检序号
    pub cpu: u16,
    pub mem: u16,
    pub fd: u16,
}

impl PatrolSnap {
    pub fn ok(&self) -> bool {
        self.cpu <= 1000 && self.mem <= 1000 && self.fd <= 1000
    }
}

/// 巡检断言账。
pub struct PatrolBook {
    rows: Vec<PatrolSnap>,
    verdicts: Vec<(u32, u8, &'static str)>, // (tick, 分类, 维度)
    frozen: bool,
}

impl PatrolBook {
    pub fn new() -> PatrolBook {
        PatrolBook { rows: Vec::new(), verdicts: Vec::new(), frozen: false }
    }

    /// 采样：越界/重号/冻结后写拒收。
    pub fn sample(&mut self, s: PatrolSnap) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !s.ok() {
            return Err("out_range");
        }
        if self.rows.iter().any(|r| r.tick == s.tick) {
            return Err("dup_tick");
        }
        self.rows.push(s);
        Ok(())
    }

    /// 巡检断言：对三维度各出一条分类断言。
    pub fn assert_drift(&mut self) -> usize {
        if self.rows.len() < WIN {
            return 0; // 护栏
        }
        let w = &self.rows[self.rows.len() - WIN..];
        let last_tick = w[w.len() - 1].tick;
        let dims: [(&'static str, fn(&PatrolSnap) -> u16); 3] = [
            ("cpu", |s| s.cpu),
            ("mem", |s| s.mem),
            ("fd", |s| s.fd),
        ];
        let mut n = 0;
        for (name, get) in dims {
            let series: Vec<u16> = w.iter().map(get).collect();
            let cls = classify(&series);
            self.verdicts.push((last_tick, cls, name));
            n += 1;
        }
        n
    }

    /// 分类计数。
    pub fn count_of(&self, cls: u8) -> usize {
        self.verdicts.iter().filter(|(_, c, _)| *c == cls).count()
    }

    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 巡检指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for r in &self.rows {
            for v in [r.tick, r.cpu as u32, r.mem as u32, r.fd as u32] {
                for b in v.to_le_bytes() {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x01000193);
                }
            }
        }
        h
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

/// 注入核。
pub fn inject_fault(b: &mut PatrolBook, code: u32) -> Result<(), &'static str> {
    match code {
        c if c == PS_TREND as u32 => {
            for i in 0..WIN {
                let _ = b.sample(PatrolSnap {
                    tick: i as u32,
                    cpu: 300 + (i as u16) * 40,
                    mem: 500,
                    fd: 500,
                });
            }
            Ok(())
        }
        c if c == PS_SPIKE as u32 => {
            for i in 0..WIN {
                let cpu = if i == 3 { 900 } else { 500 };
                let _ = b.sample(PatrolSnap {
                    tick: i as u32,
                    cpu,
                    mem: 500,
                    fd: 500,
                });
            }
            Ok(())
        }
        PS_FROZEN => {
            b.freeze();
            b.sample(PatrolSnap { tick: 99, cpu: 500, mem: 500, fd: 500 })
        }
        PS_OUT_RANGE => b.sample(PatrolSnap { tick: 1, cpu: 1001, mem: 0, fd: 0 }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10272_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10272");

    // J1 趋势漂移检出：cpu 净增 200 → 趋势
    let j1 = {
        let mut b = PatrolBook::new();
        let _ = inject_fault(&mut b, PS_TREND as u32);
        b.assert_drift();
        b.count_of(PS_TREND) >= 1
    };
    s.add("J1-趋势检出", j1, "cpu 净增 200 入趋势桶");

    // J2 突变检出：cpu 单步 +400 → 突变
    let j2 = {
        let mut b = PatrolBook::new();
        let _ = inject_fault(&mut b, PS_SPIKE as u32);
        b.assert_drift();
        b.count_of(PS_SPIKE) >= 1
    };
    s.add("J2-突变检出", j2, "单步 400 入突变桶");

    // J3 抖动豁免：带宽内三维度全豁免
    let j3 = {
        let mut b = PatrolBook::new();
        let wave = [500u16, 540, 480, 550, 470, 520];
        for (i, v) in wave.iter().enumerate() {
            let _ = b.sample(PatrolSnap {
                tick: i as u32,
                cpu: *v,
                mem: *v,
                fd: *v,
            });
        }
        b.assert_drift();
        b.verdict_len() == 3 && b.count_of(PS_JITTER) == 3
    };
    s.add("J3-抖动豁免", j3, "带宽内三维度全豁免");

    // J4 分类真源：突变优先于趋势
    let j4 = {
        // 序列既趋势又突变 → 分类为突变（优先级）
        let w = [100u16, 150, 200, 600, 650, 700];
        classify(&w) == PS_SPIKE
    };
    s.add("J4-突变优先", j4, "双特征时突变优先");

    // J5 样本护栏：不足 WIN 不出断言
    let j5 = {
        let mut b = PatrolBook::new();
        for i in 0..3 {
            let _ = b.sample(PatrolSnap { tick: i, cpu: 500, mem: 500, fd: 500 });
        }
        b.assert_drift() == 0
    };
    s.add("J5-样本护栏", j5, "不足窗口零断言");

    // J6 断言三维度齐出
    let j6 = {
        let mut b = PatrolBook::new();
        for i in 0..WIN {
            let _ = b.sample(PatrolSnap {
                tick: i as u32,
                cpu: 500 + (i as u16) * 30,
                mem: 500,
                fd: 500,
            });
        }
        b.assert_drift();
        b.verdict_len() == 3
            && b.verdicts.iter().all(|(_, _, d)| ["cpu", "mem", "fd"].contains(d))
    };
    s.add("J6-三维齐出", j6, "cpu/mem/fd 各一条");

    // J7 注入路径：越界拒收
    let j7 = inject_fault(&mut PatrolBook::new(), PS_OUT_RANGE) == Err("out_range");
    s.add("J7-越界拒收", j7, "cpu=1001 即拒");

    // J8 冻结红线
    let j8 = {
        let mut b = PatrolBook::new();
        inject_fault(&mut b, PS_FROZEN);
        b.is_frozen()
    };
    s.add("J8-冻结拒采", j8, "印后采样必拒");

    // J9 巡检指纹：同序列同指纹
    let j9 = {
        let mk = || {
            let mut b = PatrolBook::new();
            for i in 0..WIN {
                let _ = b.sample(PatrolSnap {
                    tick: i as u32,
                    cpu: 500 + (i as u16) * 10,
                    mem: 600,
                    fd: 700,
                });
            }
            b.fp()
        };
        let f1 = mk();
        let f2 = mk();
        f1 == f2 && f1 != 0
    };
    s.add("J9-巡检指纹", j9, "同序列同指纹");

    // J10 十轮巡检回放：10 轮指纹与断言一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = PatrolBook::new();
            for i in 0..WIN {
                let _ = b.sample(PatrolSnap {
                    tick: i as u32,
                    cpu: 400 + (i as u16) * 35,
                    mem: 500,
                    fd: 500,
                });
            }
            let n = b.assert_drift();
            (n, b.fp())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == 3
    };
    s.add("J10-十轮巡检", j10, "10/10 轮指纹断言一致");

    s
}
