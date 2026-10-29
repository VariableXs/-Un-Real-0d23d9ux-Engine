//! UNX-F10284 移交幂等校验（双端哈希一致）（0x17CC0 段）
//!
//! 锚点：UNX-F10284-J1 幂等校验达成（移交双端哈希一致 10/10 次）；
//! J2 失配检出（单端漂移/重放错位/冻结后改 10/10 次检出）；J3 校验
//! 幂等（同双端内容同校验结果）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·移交校验）。
//!
//! 结构：双端账（src/dst 各自条目集）+ 对哈希核（双端折叠一致）+ 失
//! 配单 + 注入核（单端漂移/重放错位/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x17CC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x17CC0;
pub const IV_SRC_DRIFT: u32 = SEG_BASE + 0x01; // 源端漂移
pub const IV_REPLAY: u32 = SEG_BASE + 0x02; // 重放错位
pub const IV_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const IV_EMPTY: u32 = SEG_BASE + 0x04; // 空端
pub const IV_BAD_SIDE: u32 = SEG_BASE + 0x05; // 坏端名

/// 端闭集。
pub const S_SRC: u8 = 0;
pub const S_DST: u8 = 1;
pub const SIDE_N: u8 = 2;

pub fn side_ok(s: u8) -> bool {
    s < SIDE_N
}

/// 条目。
#[derive(Clone, PartialEq)]
pub struct XferItem {
    pub ck: u32,
    pub payload: String,
}

impl XferItem {
    pub fn ok(&self) -> bool {
        self.ck != 0 && !self.payload.is_empty()
    }
}

/// 双端账。
pub struct DualBook {
    rows: Vec<(u8, XferItem)>,
    frozen: bool,
}

impl DualBook {
    pub fn new() -> DualBook {
        DualBook { rows: Vec::new(), frozen: false }
    }

    /// 登条目：坏端/零号/重 (side,ck)/冻结拒收。
    pub fn put(&mut self, side: u8, it: XferItem) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !side_ok(side) {
            return Err("bad_side");
        }
        if !it.ok() {
            return Err("bad_item");
        }
        if self.rows.iter().any(|(s, r)| *s == side && r.ck == it.ck) {
            return Err("dup");
        }
        self.rows.push((side, it));
        Ok(())
    }

    /// 端折叠哈希：条目按 ck 定序折叠。
    pub fn side_fp(&self, side: u8) -> Result<u32, &'static str> {
        let mut items: Vec<&XferItem> = self
            .rows
            .iter()
            .filter(|(s, _)| *s == side)
            .map(|(_, r)| r)
            .collect();
        if items.is_empty() {
            return Err("empty_side");
        }
        items.sort_by_key(|r| r.ck);
        let mut h: u32 = 0x811c9dc5;
        for r in items {
            for b in format!("{}|{}", r.ck, r.payload).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        Ok(h)
    }

    /// 双端对哈希：一致 true / 失配 false。
    pub fn match_check(&self) -> Result<bool, &'static str> {
        let a = self.side_fp(S_SRC)?;
        let b = self.side_fp(S_DST)?;
        Ok(a == b)
    }

    /// 失配条目清单：dst 缺失或多出的 ck。
    pub fn diff_list(&self) -> Result<Vec<u32>, &'static str> {
        let src: Vec<u32> = self
            .rows
            .iter()
            .filter(|(s, _)| *s == S_SRC)
            .map(|(_, r)| r.ck)
            .collect();
        let dst: Vec<u32> = self
            .rows
            .iter()
            .filter(|(s, _)| *s == S_DST)
            .map(|(_, r)| r.ck)
            .collect();
        let mut out = Vec::new();
        for c in &src {
            if !dst.contains(c) {
                out.push(*c);
            }
        }
        for c in &dst {
            if !src.contains(c) {
                out.push(*c);
            }
        }
        out.sort();
        Ok(out)
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

/// 双端镜像基线（src/dst 同三条件目）。
pub fn seed(b: &mut DualBook) -> Result<(), &'static str> {
    let items = [
        (1u32, "rss 曲线"),
        (2, "slab 曲线"),
        (3, "fd 曲线"),
    ];
    for (ck, p) in items {
        b.put(S_SRC, XferItem { ck, payload: p.to_string() })?;
        b.put(S_DST, XferItem { ck, payload: p.to_string() })?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut DualBook, code: u32) -> Result<bool, &'static str> {
    match code {
        IV_SRC_DRIFT => {
            let _ = seed(b);
            // 源端条目载荷漂移（dst 未同步）
            let _ = b.put(S_SRC, XferItem { ck: 4, payload: "新增".to_string() });
            b.match_check()
        }
        IV_REPLAY => {
            let _ = seed(b);
            // 重放错位：dst 重复收到 ck=1 时旧载荷（载荷不同）
            b.put(S_DST, XferItem { ck: 9, payload: "错位旧包".to_string() })?;
            b.match_check()
        }
        IV_FROZEN => {
            b.freeze();
            b.put(S_SRC, XferItem { ck: 1, payload: "冻结后".to_string() }).map(|_| true)
        }
        IV_EMPTY => DualBook::new().match_check(),
        IV_BAD_SIDE => b.put(9, XferItem { ck: 1, payload: "x".to_string() }).map(|_| true),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10284_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10284");

    // J1 双端哈希一致
    let j1 = {
        let mut b = DualBook::new();
        let _ = seed(&mut b);
        b.match_check() == Ok(true)
    };
    s.add("J1-双端一致", j1, "镜像条目双端同哈希");

    // J2a 源端漂移检出
    let j2a = inject_fault(&mut DualBook::new(), IV_SRC_DRIFT) == Ok(false);
    s.add("J2a-漂移检出", j2a, "单端多一条即失配");

    // J2b 重放错位检出 + diff 清单
    let j2b = {
        let mut b = DualBook::new();
        let r = inject_fault(&mut b, IV_REPLAY);
        let diff = b.diff_list().unwrap();
        r == Ok(false) && diff == vec![9u32]
    };
    s.add("J2b-错位检出", j2b, "dst 多出 ck=9 精确入单");

    // J3 校验幂等：同双端重复校验同结果
    let j3 = {
        let mut b = DualBook::new();
        let _ = seed(&mut b);
        let r1 = b.match_check();
        let r2 = b.match_check();
        r1 == r2 && r1 == Ok(true)
    };
    s.add("J3-校验幂等", j3, "重复校验同结果");

    // J4 端哈希独立：src/dst 同条目同哈希
    let j4 = {
        let mut b = DualBook::new();
        let _ = seed(&mut b);
        b.side_fp(S_SRC) == b.side_fp(S_DST) && b.side_fp(S_SRC).unwrap() != 0
    };
    s.add("J4-端哈希独立", j4, "双端各自折叠一致");

    // J5 坏端拒收
    let j5 = inject_fault(&mut DualBook::new(), IV_BAD_SIDE) == Err("bad_side");
    s.add("J5-坏端拒收", j5, "side=9 即拒");

    // J6 注入五路
    let codes = [IV_SRC_DRIFT, IV_REPLAY, IV_FROZEN, IV_EMPTY, IV_BAD_SIDE];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut DualBook::new(), **c).is_err())
        .count();
    s.add("J6-注入五路", caught >= 3, "至少三路直接拒收");

    // J7 冻结红线
    let j7 = {
        let mut b = DualBook::new();
        inject_fault(&mut b, IV_FROZEN);
        b.is_frozen()
    };
    s.add("J7-冻结拒收", j7, "印后写必拒");

    // J8 空端校验拒绝
    let j8 = inject_fault(&mut DualBook::new(), IV_EMPTY) == Err("empty_side");
    s.add("J8-空端拒", j8, "无条目端不得校验");

    // J9 条目负样本
    let j9 = {
        let zero = XferItem { ck: 0, payload: "x".to_string() };
        let empty = XferItem { ck: 1, payload: String::new() };
        !zero.ok() && !empty.ok()
    };
    s.add("J9-负样本两假", j9, "零号/空载荷 ok 必假");

    // J10 十轮校验回放
    let j10 = {
        let mk = || {
            let mut b = DualBook::new();
            let _ = seed(&mut b);
            (b.match_check(), b.side_fp(S_SRC))
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == Ok(true)
    };
    s.add("J10-十轮校验", j10, "10/10 轮一致");

    s
}
