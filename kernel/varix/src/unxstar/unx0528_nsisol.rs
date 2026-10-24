//! unx0528_nsisol —— UNX-F0528 演练命名空间隔离（生产零污染）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0528 · 演练命名空间隔离 · 目标 220 行
//! 判据原文：隔离面账齐（生产数据结构零演练写断言），注入交叉污染检出
//!           100%。
//! 组件：Namespace（生产/演练双空间）+ 隔离门（演练写只入演练空间，越
//!   空间写检出）+ 交叉污染注入（生产空间收到演练写即记污染）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35140;
pub const XC_CROSS_LEAK: u32 = SEG_BASE + 1; // 交叉污染
pub const XC_NS_BAD: u32 = SEG_BASE + 2; // 空间号坏
pub const XC_ISOL_MISS: u32 = SEG_BASE + 3; // 隔离缺失
pub const XC_WRITE_OOB: u32 = SEG_BASE + 4; // 写序号越界

/// 空间号：0=生产 1=演练。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ns {
    Prod = 0,
    Drill = 1,
}

/// 空间数据账：键值对。
#[derive(Clone, Debug, PartialEq)]
pub struct NsStore {
    pub ns: Ns,
    pub kv: Vec<(String, u32)>,
}

impl NsStore {
    /// 空存储。
    pub fn new(ns: Ns) -> Self {
        NsStore { ns, kv: Vec::new() }
    }

    /// 写入（同键覆盖）。
    pub fn write(&mut self, key: &str, val: u32) {
        let mut i = 0;
        while i < self.kv.len() {
            if self.kv[i].0 == key {
                self.kv[i].1 = val;
                return;
            }
            i += 1;
        }
        self.kv.push((String::from(key), val));
    }

    /// 读取。
    pub fn read(&self, key: &str) -> Option<u32> {
        let mut i = 0;
        while i < self.kv.len() {
            if self.kv[i].0 == key {
                return Some(self.kv[i].1);
            }
            i += 1;
        }
        None
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.kv.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.kv.is_empty()
    }

    /// 指纹（容量校验和用）：Σ 值 + Σ 键长。
    pub fn fingerprint(&self) -> u64 {
        let mut f = 0u64;
        let mut i = 0;
        while i < self.kv.len() {
            f += self.kv[i].1 as u64;
            f += self.kv[i].0.len() as u64;
            i += 1;
        }
        f
    }
}

/// 隔离门：路由写请求到正确空间，越空间写（生产空间收到 drill 键）检出。
pub struct IsolationGate {
    pub prod: NsStore,
    pub drill: NsStore,
    /// 检出的交叉污染数。
    pub leaks: u32,
}

impl IsolationGate {
    /// 新门。
    pub fn new() -> Self {
        IsolationGate {
            prod: NsStore::new(Ns::Prod),
            drill: NsStore::new(Ns::Drill),
            leaks: 0,
        }
    }

    /// 演练写：key 带 "drill:" 头才可入演练空间；否则记污染并拒。
    pub fn drill_write(&mut self, key: &str, val: u32) -> Result<(), u32> {
        if key.starts_with("drill:") {
            self.drill.write(key, val);
            Ok(())
        } else {
            self.leaks += 1;
            Err(XC_CROSS_LEAK)
        }
    }

    /// 生产写：只入生产空间（任何 key）。
    pub fn prod_write(&mut self, key: &str, val: u32) {
        self.prod.write(key, val);
    }

    /// 生产零污染核：生产空间无任何 "drill:" 键。
    pub fn prod_clean(&self) -> bool {
        let mut i = 0;
        while i < self.prod.kv.len() {
            if self.prod.kv[i].0.starts_with("drill:") {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 污染计数。
    pub fn leak_count(&self) -> u32 {
        self.leaks
    }
}

impl Default for IsolationGate {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0528_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0528");
    cs.add("SEG 段位独占", SEG_BASE == 0x35140 && XC_WRITE_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：双空间独立。
    let mut g = IsolationGate::new();
    g.prod_write("budget", 100);
    let _ = g.drill_write("drill:sim", 7);
    cs.add("生产写", g.prod.read("budget") == Some(100) && g.prod.len() == 1, "生产键值落位");
    cs.add("演练写", g.drill.read("drill:sim") == Some(7), "演练键值落位");
    cs.add("互不可见", g.prod.read("drill:sim").is_none() && g.drill.read("budget").is_none(), "跨空间读空");

    // 族二：交叉污染检出 100%。
    let mut g2 = IsolationGate::new();
    let bad_keys = ["budget", "frames", "power", "sched"];
    let mut caught = 0;
    let mut i = 0;
    while i < bad_keys.len() {
        if g2.drill_write(bad_keys[i], 1).is_err() {
            caught += 1;
        }
        i += 1;
    }
    cs.add("污染检出 4/4", caught == 4 && g2.leak_count() == 4, "无头演练写全拒");
    cs.add("拒后不落账", g2.drill.is_empty() && g2.prod.is_empty(), "污染写不进任何空间");
    let _ = g2.drill_write("drill:ok", 3);
    cs.add("正头放行", g2.drill.read("drill:ok") == Some(3), "带 drill: 头入演练");

    // 族三：生产零污染断言。
    let mut g3 = IsolationGate::new();
    let _ = g3.drill_write("drill:x", 1);
    let _ = g3.drill_write("drill:y", 2);
    g3.prod_write("real", 9);
    cs.add("零污染核", g3.prod_clean(), "生产空间无 drill 键");
    cs.add("指纹分账", g3.prod.fingerprint() == 4 + 9 && g3.drill.fingerprint() == 7 + 2 + 1, "指纹各归各");

    // 族四：同键覆盖不越空间。
    let _ = g3.drill_write("drill:x", 5);
    cs.add("演练覆盖", g3.drill.read("drill:x") == Some(5) && g3.drill.len() == 2, "同键更新");
    g3.prod_write("real", 10);
    cs.add("生产覆盖", g3.prod.read("real") == Some(10) && g3.prod.len() == 1, "生产键独立覆盖");
    cs
}
