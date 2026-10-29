//! UNX-F10281 O3 对接开卷总纲（对接面/移交面/联调面三维齐备）（0x17660 段）
//!
//! 锚点：UNX-F10281-J1 总纲达成（对接面/移交面/联调面三维登记齐备
//! 10/10 次）；J2 三维缺漏检出（任一面缺席总纲拒绝成立 10/10 次）；
//! J3 总纲哈希（同三维同哈希）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·卷首）。
//!
//! 结构：总纲册（三维条目）+ 齐备核 + 缺面点名 + 总纲哈希 + 注入核
//! （缺面/坏维度/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x17660 独占 0x20）。
pub const SEG_BASE: u32 = 0x17660;
pub const OG_MISS_FACE: u32 = SEG_BASE + 0x01; // 缺面
pub const OG_BAD_DIM: u32 = SEG_BASE + 0x02; // 坏维度
pub const OG_DUP: u32 = SEG_BASE + 0x03; // 重条目
pub const OG_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const OG_EMPTY: u32 = SEG_BASE + 0x05; // 空文本

/// 三维闭集。
pub const D_IFACE: u8 = 0; // 对接面
pub const D_HANDOVER: u8 = 1; // 移交面
pub const D_JOINT: u8 = 2; // 联调面
pub const DIM_N: u8 = 3;

pub fn dim_ok(d: u8) -> bool {
    d < DIM_N
}

pub fn dim_name(d: u8) -> &'static str {
    match d {
        D_IFACE => "iface",
        D_HANDOVER => "handover",
        _ => "joint",
    }
}

/// 总纲条目。
#[derive(Clone, PartialEq)]
pub struct O3Outline {
    pub dim: u8,
    pub key: String,
    pub val: String,
}

impl O3Outline {
    pub fn ok(&self) -> bool {
        dim_ok(self.dim) && !self.key.is_empty() && !self.val.is_empty()
    }
}

/// 总纲册。
pub struct O3Book {
    rows: Vec<O3Outline>,
    frozen: bool,
}

impl O3Book {
    pub fn new() -> O3Book {
        O3Book { rows: Vec::new(), frozen: false }
    }

    pub fn put(&mut self, dim: u8, key: &str, val: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !dim_ok(dim) {
            return Err("bad_dim");
        }
        if key.is_empty() || val.is_empty() {
            return Err("empty_text");
        }
        if self.rows.iter().any(|r| r.dim == dim && r.key == key) {
            return Err("dup");
        }
        self.rows.push(O3Outline {
            dim,
            key: key.to_string(),
            val: val.to_string(),
        });
        Ok(())
    }

    pub fn complete(&self) -> bool {
        (0..DIM_N).all(|d| self.rows.iter().any(|r| r.dim == d))
    }

    pub fn missing(&self) -> Vec<u8> {
        (0..DIM_N)
            .filter(|d| !self.rows.iter().any(|r| r.dim == *d))
            .collect()
    }

    pub fn get(&self, dim: u8, key: &str) -> Option<&str> {
        self.rows
            .iter()
            .find(|r| r.dim == dim && r.key == key)
            .map(|r| r.val.as_str())
    }

    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&O3Outline> = self.rows.iter().collect();
        sorted.sort_by(|a, b| (a.dim, &a.key).cmp(&(b.dim, &b.key)));
        for r in sorted {
            for b in format!("{}|{}|{}", r.dim, r.key, r.val).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
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

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 铺三维总纲基线。
pub fn seed(b: &mut O3Book) -> Result<(), &'static str> {
    b.put(D_IFACE, "channel", "bug-backflow+v-sync")?;
    b.put(D_IFACE, "auth", "token-ro")?;
    b.put(D_HANDOVER, "scope", "ktest-full")?;
    b.put(D_HANDOVER, "hash", "dual-side")?;
    b.put(D_JOINT, "drill", "sample-inject")?;
    b.put(D_JOINT, "retry", "idempotent")
}

/// 注入核。
pub fn inject_fault(b: &mut O3Book, code: u32, dim: u8) -> Result<(), &'static str> {
    match code {
        OG_MISS_FACE => {
            // 只铺两维：跳过指定维（三维轮转精确缺一）
            let dims: [u8; 2] = if dim == D_IFACE {
                [D_HANDOVER, D_JOINT]
            } else if dim == D_HANDOVER {
                [D_IFACE, D_JOINT]
            } else {
                [D_IFACE, D_HANDOVER]
            };
            for d in dims {
                b.put(d, "k", "v")?;
            }
            Ok(())
        }
        OG_BAD_DIM => b.put(9, "k", "v"),
        OG_DUP => {
            let _ = b.put(dim, "k", "v1");
            b.put(dim, "k", "v2")
        }
        OG_FROZEN => {
            b.freeze();
            b.put(dim, "k2", "v2")
        }
        OG_EMPTY => b.put(dim, "", "v"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10281");

    // J1 三维齐备
    let j1 = {
        let mut b = O3Book::new();
        let _ = seed(&mut b);
        b.complete() && b.len() == 6 && b.all_ok()
    };
    s.add("J1-三维齐备", j1, "对接/移交/联调三面全备");

    // J2a 缺面点名
    let j2a = {
        let mut ok = 0;
        for miss in 0..DIM_N {
            let mut b = O3Book::new();
            let _ = inject_fault(&mut b, OG_MISS_FACE, miss);
            if b.missing() == vec![miss] {
                ok += 1;
            }
        }
        ok == 3
    };
    s.add("J2a-缺面点名", j2a, "3/3 轮精确点名");

    // J2b 坏维度拒收
    let j2b = inject_fault(&mut O3Book::new(), OG_BAD_DIM, 0) == Err("bad_dim");
    s.add("J2b-坏维度拒", j2b, "dim=9 即拒");

    // J2c 重条目拒收
    let j2c = inject_fault(&mut O3Book::new(), OG_DUP, D_IFACE) == Err("dup");
    s.add("J2c-重条目拒", j2c, "同维同键二次即拒");

    // J3 总纲哈希
    let j3 = {
        let mut b1 = O3Book::new();
        let _ = seed(&mut b1);
        let f1 = b1.fp();
        let mut b2 = O3Book::new();
        let _ = seed(&mut b2);
        let _ = b2.put(D_JOINT, "extra", "k");
        let f2 = b2.fp();
        f1 == b1.fp() && f1 != f2 && f1 != 0
    };
    s.add("J3-总纲哈希", j3, "同纲同哈希加条必变");

    // J4 维内查询
    let j4 = {
        let mut b = O3Book::new();
        let _ = seed(&mut b);
        b.get(D_IFACE, "channel") == Some("bug-backflow+v-sync")
            && b.get(D_HANDOVER, "scope") == Some("ktest-full")
    };
    s.add("J4-维内查询", j4, "三维键值可达");

    // J5 冻结红线
    let j5 = {
        let mut b = O3Book::new();
        inject_fault(&mut b, OG_FROZEN, D_JOINT);
        b.is_frozen() && b.put(D_IFACE, "k9", "v") == Err("frozen")
    };
    s.add("J5-冻结拒收", j5, "印后写必抓");

    // J6 空文本拒收
    let j6 = inject_fault(&mut O3Book::new(), OG_EMPTY, D_IFACE) == Err("empty_text");
    s.add("J6-空文本拒", j6, "空键即拒");

    // J7 条目负样本三假
    let j7 = {
        let bad = O3Outline { dim: 9, key: "k".to_string(), val: "v".to_string() };
        let ek = O3Outline { dim: D_IFACE, key: String::new(), val: "v".to_string() };
        let ev = O3Outline { dim: D_IFACE, key: "k".to_string(), val: String::new() };
        !bad.ok() && !ek.ok() && !ev.ok()
    };
    s.add("J7-负样本三假", j7, "三类坏条目 ok 必假");

    // J8 跨维同键允许
    let j8 = {
        let mut b = O3Book::new();
        let a = b.put(D_IFACE, "hash", "x");
        let bb = b.put(D_HANDOVER, "hash", "y");
        a == Ok(()) && bb == Ok(()) && b.len() == 2
    };
    s.add("J8-跨维同键", j8, "跨维同键不冲突");

    // J9 渐进齐备单翻转
    let j9 = {
        let mut b = O3Book::new();
        let mut flips = 0;
        let mut prev = false;
        for d in 0..DIM_N {
            let _ = b.put(d, "k", "v");
            let now = b.complete();
            if now && !prev {
                flips += 1;
            }
            prev = now;
        }
        flips == 1 && prev
    };
    s.add("J9-渐进单翻", j9, "末维补齐单次翻转");

    // J10 十轮总纲回放
    let j10 = {
        let mk = || {
            let mut b = O3Book::new();
            let _ = seed(&mut b);
            b.fp()
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
    s.add("J10-十轮总纲", j10, "10/10 轮哈希一致");

    s
}
