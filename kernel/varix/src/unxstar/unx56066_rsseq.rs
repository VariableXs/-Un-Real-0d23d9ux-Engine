//! UNX-F56066 RSS↔工作集语义等价声明件（unxstar 段）
//!
//! [J1] 配对观测：RSS 与工作集（WSS）逐对观测落账（五口径配对：anon/file/
//!      shared/stack/heap）；任一侧为零拒绝；配对漂移万分比可算。
//! [J2] 等级判定：漂移 ≤ 500pbp 为 L1（强等价）、≤ 1500 为 L2、≤ 3000 为 L3、
//!      其余 L4（声明不可等价）；标准夹具五口径等级须与期望逐一相符。
//! [J3] 声明签发：五口径全落账且等级齐备才可签发声明指纹（逐对折叠）；
//!      同内容两次签发一致；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（配对为零/口径未知/重复口径/声明未齐/指纹漂移/等级越界/配对失衡）
//!   纯函数：fnv32、万分比漂移、等级判定
//!   EquivPair：单口径配对条目，new() 守闸
//!   EquivBook：等价声明账本（put/level_of/declare/level_split）
//!   injection_core：5 例必抓
//!   run_unx56066_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60820;

/// 配对含零值
pub const D_PAIR_ZERO: u32 = SEG_BASE + 0x01;
/// 口径未知（不在五口径闭集）
pub const D_SCOPE_UNKNOWN: u32 = SEG_BASE + 0x02;
/// 重复口径
pub const D_DUP_SCOPE: u32 = SEG_BASE + 0x03;
/// 声明未齐（五口径未全落账即签发）
pub const D_DECLARE_INCOMPLETE: u32 = SEG_BASE + 0x04;
/// 指纹漂移
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x05;
/// 等级越界（等级号不在 1..=4）
pub const D_LEVEL_RANGE: u32 = SEG_BASE + 0x06;
/// 配对失衡（RSS < WSS，物理上不可能）
pub const D_PAIR_IMBALANCE: u32 = SEG_BASE + 0x07;

/// 五口径闭集
pub const SCOPES: [&str; 5] = ["anon", "file", "shared", "stack", "heap"];
/// 等级阈值（万分比）：L1/L2/L3 上限
pub const L1_GATE: u64 = 500;
pub const L2_GATE: u64 = 1500;
pub const L3_GATE: u64 = 3000;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 万分比漂移：|a-b|*10000/max(a,b)
pub fn drift_pbp(a: u32, b: u32) -> u64 {
    let a = a as u64;
    let b = b as u64;
    let d = if a > b { a - b } else { b - a };
    let base = if a > b { a } else { b };
    if base == 0 {
        0
    } else {
        (d * 10_000) / base
    }
}

/// 等级判定：L1/L2/L3/L4
pub fn level_of(d: u64) -> u32 {
    if d <= L1_GATE {
        1
    } else if d <= L2_GATE {
        2
    } else if d <= L3_GATE {
        3
    } else {
        4
    }
}

/// 单口径配对条目
#[derive(Clone)]
pub struct EquivPair {
    pub scope: String,
    pub rss_kb: u32,
    pub wss_kb: u32,
}

impl EquivPair {
    /// 守闸：口径闭集 + 双侧非零 + RSS ≥ WSS
    pub fn new(scope: &str, rss_kb: u32, wss_kb: u32) -> Result<Self, u32> {
        if !SCOPES.contains(&scope) {
            return Err(D_SCOPE_UNKNOWN);
        }
        if rss_kb == 0 || wss_kb == 0 {
            return Err(D_PAIR_ZERO);
        }
        if rss_kb < wss_kb {
            return Err(D_PAIR_IMBALANCE);
        }
        Ok(EquivPair { scope: scope.to_string(), rss_kb, wss_kb })
    }

    pub fn drift(&self) -> u64 {
        drift_pbp(self.rss_kb, self.wss_kb)
    }

    pub fn level(&self) -> u32 {
        level_of(self.drift())
    }

    pub fn fp(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.scope.as_bytes());
        buf.extend_from_slice(&self.rss_kb.to_le_bytes());
        buf.extend_from_slice(&self.wss_kb.to_le_bytes());
        fnv32(&buf)
    }
}

/// 等价声明账本
pub struct EquivBook {
    pub book_name: String,
    pairs: Vec<EquivPair>,
    declared: Option<u32>,
}

impl EquivBook {
    pub fn new(book_name: &str) -> Self {
        EquivBook { book_name: book_name.to_string(), pairs: Vec::new(), declared: None }
    }

    /// 落账：重复口径拒绝
    pub fn put(&mut self, p: EquivPair) -> Result<u32, u32> {
        if self.pairs.iter().any(|x| x.scope == p.scope) {
            return Err(D_DUP_SCOPE);
        }
        let fp = p.fp();
        self.pairs.push(p);
        Ok(fp)
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn complete(&self) -> bool {
        self.pairs.len() == 5
            && SCOPES.iter().all(|s| self.pairs.iter().any(|p| &p.scope == s))
    }

    pub fn get(&self, scope: &str) -> Option<&EquivPair> {
        self.pairs.iter().find(|p| p.scope == scope)
    }

    /// 等级分账 (L1, L2, L3, L4)
    pub fn level_split(&self) -> (usize, usize, usize, usize) {
        (
            self.pairs.iter().filter(|p| p.level() == 1).count(),
            self.pairs.iter().filter(|p| p.level() == 2).count(),
            self.pairs.iter().filter(|p| p.level() == 3).count(),
            self.pairs.iter().filter(|p| p.level() == 4).count(),
        )
    }

    /// 签发声明：未齐拒绝；重复签发拒绝
    pub fn declare(&mut self) -> Result<u32, u32> {
        if self.declared.is_some() {
            return Err(D_DUP_SCOPE);
        }
        if !self.complete() {
            return Err(D_DECLARE_INCOMPLETE);
        }
        let mut acc: u32 = 0x56066_00;
        for p in &self.pairs {
            acc = acc.wrapping_mul(16777619) ^ p.fp();
        }
        self.declared = Some(acc);
        Ok(acc)
    }

    pub fn declared_fp(&self) -> Option<u32> {
        self.declared
    }

    /// 等级行（机器可读）
    pub fn level_rows(&self) -> Vec<String> {
        self.pairs
            .iter()
            .map(|p| {
                format!("EQL|{}|{}|{}|{}|L{}", p.scope, p.rss_kb, p.wss_kb, p.drift(), p.level())
            })
            .collect()
    }
}

/// 标准夹具：五口径配对，漂移逐口径设计。
/// anon:   40960/40960 → 0     → L1
/// file:   20480/20096 → 384/20480=187 → L1
/// shared: 8192/8100    → 92/8192=1    → L1（心算 92*10000/8192=112）
/// stack:  512/400      → 112/512=2187 → L2
/// heap:   10240/7000   → 3240/10240=3164 → L3
pub fn fixture_book() -> EquivBook {
    let mut b = EquivBook::new("rss-wss-equiv");
    let plans: [(&str, u32, u32); 5] = [
        ("anon", 40960, 40960),
        ("file", 20480, 20096),
        ("shared", 8192, 8100),
        ("stack", 512, 400),
        ("heap", 10240, 7000),
    ];
    for (scope, rss, wss) in plans {
        let p = EquivPair::new(scope, rss, wss).expect("pair");
        b.put(p).expect("put");
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 配对含零
    if EquivPair::new("anon", 0, 100).err() == Some(D_PAIR_ZERO) {
        caught += 1;
    }

    // 2) 口径未知
    if EquivPair::new("gpu", 100, 100).err() == Some(D_SCOPE_UNKNOWN) {
        caught += 1;
    }

    // 3) 配对失衡（RSS < WSS）
    if EquivPair::new("anon", 100, 200).err() == Some(D_PAIR_IMBALANCE) {
        caught += 1;
    }

    // 4) 重复口径
    let mut b = fixture_book();
    let dup = EquivPair::new("anon", 40960, 40960).expect("dup");
    if b.put(dup) == Err(D_DUP_SCOPE) {
        caught += 1;
    }

    // 5) 未齐签发
    let mut h = EquivBook::new("half");
    let p = EquivPair::new("anon", 40960, 40960).expect("p");
    let _ = h.put(p);
    if h.declare() == Err(D_DECLARE_INCOMPLETE) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性
pub fn fp_stability() -> Result<(), u32> {
    let a = {
        let mut b = fixture_book();
        b.declare().map_err(|e| e)?
    };
    let b = {
        let mut c = fixture_book();
        c.declare().map_err(|e| e)?
    };
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56066_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56066");

    // J1 配对观测
    let mut b = fixture_book();
    s.add("56066-J1-五口径齐", b.len() == 5 && b.complete(), "五口径配对未齐");
    s.add("56066-J1-anon零漂移", b.get("anon").map(|p| p.drift()) == Some(0), "anon 漂移不为 0");
    // shared: 92*10000/8192 = 112
    s.add("56066-J1-shared漂移112", b.get("shared").map(|p| p.drift()) == Some(112), "shared 漂移不为 112");
    // file: 384*10000/20480 = 187
    s.add("56066-J1-file漂移187", b.get("file").map(|p| p.drift()) == Some(187), "file 漂移不为 187");

    // J2 等级判定
    // stack: 112*10000/512 = 2187 → L2；heap: 3240*10000/10240 = 3164 → L3
    s.add("56066-J2-stack-L2", b.get("stack").map(|p| p.level()) == Some(2), "stack 应为 L2");
    s.add("56066-J2-heap-L3", b.get("heap").map(|p| p.level()) == Some(3), "heap 应为 L3");
    let (l1, l2, l3, l4) = b.level_split();
    s.add("56066-J2-等级分账", (l1, l2, l3, l4) == (3, 1, 1, 0), "等级分账不为 3/1/1/0");
    s.add("56066-J2-等级边界", level_of(501) == 2 && level_of(1501) == 3 && level_of(3001) == 4, "等级边界判定异常");

    // J3 声明签发
    s.add("56066-J3-等级行5", b.level_rows().len() == 5, "等级行数不为 5");
    s.add("56066-J3-行格式锚", b.level_rows()[0] == "EQL|anon|40960|40960|0|L1", "首行格式不符");
    let fp1 = b.declare().expect("declare");
    s.add("56066-J3-签发回读", b.declared_fp() == Some(fp1), "声明指纹回读不一致");
    s.add("56066-J3-重复签发拒", b.declare() == Err(D_DUP_SCOPE), "重复签发未被拒绝");
    s.add("56066-J3-指纹稳定", fp_stability().is_ok(), "同内容两次签发指纹漂移");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56066-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56066_all_checks_green() {
        let s = run_unx56066_checks();
        assert!(
            s.all_passed(),
            "UNX-F56066 有红项: {}",
            s.red_items_text()
        );
    }
}
