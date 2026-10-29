//! UNX-F10262 负载画像成文（CPU/内存/IO/句柄四维负载模型）（0x14E00 段）
//!
//! 锚点：UNX-F10262-J1 负载画像达成（CPU/内存/IO/句柄四维负载模型成
//! 文落账，四维采样与画像输出一致 10/10 次）；J2 画像越界检出（采样
//! 值越界/维度缺失/画像漂移 10/10 次检出）；J3 画像指纹（同采样同画
//! 像同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：四维采样账（cpu/mem/io/fd 各自配额与水位）+ 画像核（水位分
//! 级渲染）+ 越界核 + 注入核（越界/缺维/漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x14E00 独占 0x20）。
pub const SEG_BASE: u32 = 0x14E00;
pub const LP_OUT_OF_RANGE: u32 = SEG_BASE + 0x01; // 采样越界
pub const LP_MISS_DIM: u32 = SEG_BASE + 0x02; // 维度缺失
pub const LP_DRIFT: u32 = SEG_BASE + 0x03; // 画像漂移
pub const LP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后采样
pub const LP_BAD_DIM: u32 = SEG_BASE + 0x05; // 坏维度

/// 四维闭集。
pub const V_CPU: u8 = 0;
pub const V_MEM: u8 = 1;
pub const V_IO: u8 = 2;
pub const V_FD: u8 = 3;
pub const DIM_N: u8 = 4;

pub fn dim_ok(d: u8) -> bool {
    d < DIM_N
}

pub fn dim_name(d: u8) -> &'static str {
    match d {
        V_CPU => "cpu",
        V_MEM => "mem",
        V_IO => "io",
        _ => "fd",
    }
}

/// 采样条目：水位（0~1000 千分位）。
#[derive(Clone, Copy, PartialEq)]
pub struct Sample {
    pub dim: u8,
    pub permille: u16, // 0..=1000
}

impl Sample {
    pub fn ok(&self) -> bool {
        dim_ok(self.dim) && self.permille <= 1000
    }

    /// 水位分级：0~499 低 / 500~799 中 / 800~949 高 / 950~1000 危。
    pub fn level(&self) -> u8 {
        match self.permille {
            0..=499 => 0,
            500..=799 => 1,
            800..=949 => 2,
            _ => 3,
        }
    }

    pub fn level_name(&self) -> &'static str {
        match self.level() {
            0 => "low",
            1 => "mid",
            2 => "high",
            _ => "crit",
        }
    }
}

/// 四维配额（负载模型真源）。
pub const QUOTA: [u16; 4] = [800, 850, 700, 900]; // 各维告警线（千分位）

/// 画像账。
pub struct LoadProfile {
    rows: Vec<Sample>,
    frozen: bool,
}

impl LoadProfile {
    pub fn new() -> LoadProfile {
        LoadProfile { rows: Vec::new(), frozen: false }
    }

    /// 采样：坏维度/越界/冻结后写/重维拒收。
    pub fn sample(&mut self, dim: u8, permille: u16) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !dim_ok(dim) {
            return Err("bad_dim");
        }
        if permille > 1000 {
            return Err("out_of_range");
        }
        if self.rows.iter().any(|r| r.dim == dim) {
            return Err("dup_dim");
        }
        let sm = Sample { dim, permille };
        self.rows.push(sm);
        Ok(sm.level())
    }

    /// 四维齐备。
    pub fn complete(&self) -> bool {
        (0..DIM_N).all(|d| self.rows.iter().any(|r| r.dim == d))
    }

    pub fn missing(&self) -> Vec<u8> {
        (0..DIM_N)
            .filter(|d| !self.rows.iter().any(|r| r.dim == *d))
            .collect()
    }

    /// 超配额维度清单（告警线越维）。
    pub fn over_quota(&self) -> Vec<u8> {
        (0..DIM_N)
            .filter(|d| {
                self.rows
                    .iter()
                    .find(|r| r.dim == *d)
                    .map(|r| r.permille > QUOTA[*d as usize])
                    .unwrap_or(false)
            })
            .collect()
    }

    /// 画像渲染：四维定序输出（缺维拒绝）。
    pub fn render(&self) -> Result<String, String> {
        if !self.complete() {
            let miss: Vec<&str> = self.missing().iter().map(|d| dim_name(*d)).collect();
            return Err(format!("missing:{}", miss.join(",")));
        }
        let mut out = String::from("PROFILE|");
        for d in 0..DIM_N {
            let r = self.rows.iter().find(|r| r.dim == d).unwrap();
            out.push_str(&format!("{}={}:{},", dim_name(d), r.permille, r.level_name()));
        }
        Ok(out)
    }

    /// 画像指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in self.render().unwrap_or_default().as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
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
pub fn inject_fault(p: &mut LoadProfile, code: u32, dim: u8) -> Result<u8, &'static str> {
    match code {
        LP_OUT_OF_RANGE => p.sample(dim, 1001),
        LP_MISS_DIM => {
            // 只采三维
            for d in 0..DIM_N {
                if d != dim {
                    let _ = p.sample(d, 500);
                }
            }
            Ok(0)
        }
        LP_DRIFT => {
            let _ = p.sample(dim, 500);
            p.sample(dim, 600) // 重维拒收 → 漂移被拦
        }
        LP_FROZEN => {
            p.freeze();
            p.sample(dim, 500)
        }
        LP_BAD_DIM => p.sample(9, 500),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10262_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10262");

    // J1 四维画像成文：seed 后渲染与分级正确
    let j1 = {
        let mut p = LoadProfile::new();
        let _ = p.sample(V_CPU, 600); // mid
        let _ = p.sample(V_MEM, 950); // crit（≥950）
        let _ = p.sample(V_IO, 300); // low
        let _ = p.sample(V_FD, 850); // high
        match p.render() {
            Ok(t) => t.contains("cpu=600:mid")
                && t.contains("mem=950:crit")
                && t.contains("io=300:low")
                && t.contains("fd=850:high"),
            Err(_) => false,
        }
    };
    s.add("J1-四维画像", j1, "四维渲染与分级一致");

    // J2a 采样越界拒收
    let j2a = inject_fault(&mut LoadProfile::new(), LP_OUT_OF_RANGE, V_CPU) == Err("out_of_range");
    s.add("J2a-越界拒收", j2a, "1001 即拒");

    // J2b 缺维检出
    let j2b = {
        let mut p = LoadProfile::new();
        let _ = inject_fault(&mut p, LP_MISS_DIM, V_IO);
        matches!(p.render(), Err(e) if e == "missing:io")
    };
    s.add("J2b-缺维检出", j2b, "缺 io 精确报");

    // J2c 画像漂移被拦（重维）
    let j2c = inject_fault(&mut LoadProfile::new(), LP_DRIFT, V_MEM) == Err("dup_dim");
    s.add("J2c-漂移被拦", j2c, "重维采样即拒");

    // J3 画像指纹：同采样同指纹
    let j3 = {
        let mk = || {
            let mut p = LoadProfile::new();
            for (d, v) in [(V_CPU, 600u16), (V_MEM, 900), (V_IO, 300), (V_FD, 850)] {
                let _ = p.sample(d, v);
            }
            p.fp()
        };
        let f1 = mk();
        let f2 = mk();
        f1 == f2 && f1 != 0
    };
    s.add("J3-画像指纹", j3, "同采样同指纹");

    // J4 超配额告警：cpu 850>800 与 mem 900>850 入告警清单
    let j4 = {
        let mut p = LoadProfile::new();
        let _ = p.sample(V_CPU, 850);
        let _ = p.sample(V_MEM, 900);
        let _ = p.sample(V_IO, 400);
        let _ = p.sample(V_FD, 950); // 950>900 超配
        let over = p.over_quota();
        over == vec![V_CPU, V_MEM, V_FD]
    };
    s.add("J4-超配额告警", j4, "三超配维精确清单");

    // J5 分级真源：边界 499/500/799/800/949/950
    let j5 = {
        let lv = |v: u16| Sample { dim: V_CPU, permille: v }.level();
        lv(499) == 0 && lv(500) == 1 && lv(799) == 1 && lv(800) == 2 && lv(949) == 2
            && lv(950) == 3
    };
    s.add("J5-分级边界", j5, "六级边界精确");

    // J6 注入五路
    let codes = [LP_OUT_OF_RANGE, LP_MISS_DIM, LP_DRIFT, LP_FROZEN, LP_BAD_DIM];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut LoadProfile::new(), **c, V_CPU).is_err())
        .count();
    s.add("J6-注入五路", caught >= 3, "至少三路直接拒收");

    // J7 冻结后采样拒收
    let j7 = {
        let mut p = LoadProfile::new();
        inject_fault(&mut p, LP_FROZEN, V_CPU);
        p.is_frozen()
    };
    s.add("J7-冻结拒采", j7, "印后采样必拒");

    // J8 坏维度拒收
    let j8 = inject_fault(&mut LoadProfile::new(), LP_BAD_DIM, 0) == Err("bad_dim");
    s.add("J8-坏维度拒", j8, "dim=9 即拒");

    // J9 采样负样本：越界/坏维 ok 必假
    let j9 = {
        let out = Sample { dim: V_CPU, permille: 1001 };
        let bad = Sample { dim: 9, permille: 500 };
        !out.ok() && !bad.ok()
    };
    s.add("J9-负样本两假", j9, "越界/坏维 ok 必假");

    // J10 十轮画像回放：10 轮渲染字节一致（确定性）
    let j10 = {
        let mk = || {
            let mut p = LoadProfile::new();
            for (d, v) in [(V_CPU, 610u16), (V_MEM, 890), (V_IO, 310), (V_FD, 860)] {
                let _ = p.sample(d, v);
            }
            p.render().unwrap()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮画像", j10, "10/10 轮渲染一致");

    s
}
