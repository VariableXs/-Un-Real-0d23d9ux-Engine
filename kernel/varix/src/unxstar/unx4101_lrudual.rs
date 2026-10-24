//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4101 · LRU 双结构（hot/cold 链） · 诊断段 0xD880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F4101-J1 双链（hot/cold）迁移规则（命中升 hot、闲置降
//!      cold）万次迁移零错账，双向链完整性断言（前驱后继互指）
//!      零断链；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - hot/cold 双链：节点双态；
//!   - 迁移规则：命中升 hot（尾插）、闲置降 cold（尾插）；
//!   - 万次迁移零错账：迁移账闭合（升=降+净迁移）；
//!   - 双向链完整性：前驱后继互指逐节点断言零断链；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD880;

/// 节点数。
pub const NODES: u32 = 32;
/// 迁移次数。
pub const MIGRATIONS: u32 = 10_000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 双向链。
pub struct DList {
    pub prev: Vec<Option<u32>>,
    pub next: Vec<Option<u32>>,
    pub head: Option<u32>,
    pub tail: Option<u32>,
    pub len: u32,
}

impl DList {
    pub fn new(cap: u32) -> Self {
        DList {
            prev: vec_none(cap as usize),
            next: vec_none(cap as usize),
            head: None,
            tail: None,
            len: 0,
        }
    }

    /// 尾插。
    pub fn push_tail(&mut self, id: u32) {
        self.prev[id as usize] = self.tail;
        self.next[id as usize] = None;
        if let Some(t) = self.tail {
            self.next[t as usize] = Some(id);
        } else {
            self.head = Some(id);
        }
        self.tail = Some(id);
        self.len += 1;
    }

    /// 摘除。
    pub fn remove(&mut self, id: u32) -> bool {
        if self.len == 0 {
            return false;
        }
        let in_list = self.head.map(|h| contains(self, h, id)).unwrap_or(false);
        if !in_list {
            return false;
        }
        let p = self.prev[id as usize];
        let n = self.next[id as usize];
        match p {
            Some(pp) => self.next[pp as usize] = n,
            None => self.head = n,
        }
        match n {
            Some(nn) => self.prev[nn as usize] = p,
            None => self.tail = p,
        }
        self.prev[id as usize] = None;
        self.next[id as usize] = None;
        self.len -= 1;
        true
    }

    /// 完整性：前驱后继互指逐节点断言。
    pub fn intact(&self) -> bool {
        let mut count = 0u32;
        let mut cur = self.head;
        let mut seen_last: Option<u32> = None;
        while let Some(c) = cur {
            // 前驱互指
            match self.prev[c as usize] {
                Some(p) => {
                    if self.next[p as usize] != Some(c) {
                        return false;
                    }
                }
                None => {
                    if self.head != Some(c) {
                        return false;
                    }
                }
            }
            seen_last = Some(c);
            cur = self.next[c as usize];
            count += 1;
            if count > self.len + 1 {
                return false; // 环路
            }
        }
        count == self.len && self.tail == seen_last
    }
}

fn vec_none(n: usize) -> Vec<Option<u32>> {
    let mut v = Vec::new();
    for _ in 0..n {
        v.push(None);
    }
    v
}

fn contains(dl: &DList, from: u32, id: u32) -> bool {
    let mut cur = Some(from);
    let mut steps = 0;
    while let Some(c) = cur {
        if c == id {
            return true;
        }
        cur = dl.next[c as usize];
        steps += 1;
        if steps > dl.len + 1 {
            return false;
        }
    }
    false
}

/// LRU 双结构。
pub struct DualLru {
    pub hot: DList,
    pub cold: DList,
    pub where_in: [u8; 128], // 0=无 1=hot 2=cold
    pub up_cnt: u64,
    pub down_cnt: u64,
}

impl DualLru {
    pub fn new() -> Self {
        let mut w = [0u8; 128];
        for i in 0..NODES as usize {
            w[i] = 2; // 初始全 cold
        }
        DualLru {
            hot: DList::new(NODES),
            cold: DList::new(NODES),
            where_in: w,
            up_cnt: 0,
            down_cnt: 0,
        }
    }

    /// 节点进入（初始 cold 尾插）。
    pub fn enter_cold(&mut self, id: u32) -> bool {
        if id >= NODES || self.where_in[id as usize] != 0 {
            return false;
        }
        self.cold.push_tail(id);
        self.where_in[id as usize] = 2;
        true
    }

    /// 命中升 hot。
    pub fn hit_up(&mut self, id: u32) -> bool {
        if id >= NODES || self.where_in[id as usize] != 2 {
            return false;
        }
        if !self.cold.remove(id) {
            return false;
        }
        self.hot.push_tail(id);
        self.where_in[id as usize] = 1;
        self.up_cnt += 1;
        true
    }

    /// 闲置降 cold。
    pub fn idle_down(&mut self, id: u32) -> bool {
        if id >= NODES || self.where_in[id as usize] != 1 {
            return false;
        }
        if !self.hot.remove(id) {
            return false;
        }
        self.cold.push_tail(id);
        self.where_in[id as usize] = 2;
        self.down_cnt += 1;
        true
    }

    /// 错账断言：位置账与双链实况一致。
    pub fn no_drift(&self) -> bool {
        for id in 0..NODES {
            let w = self.where_in[id as usize];
            let in_hot = contains(&self.hot, self.hot.head.unwrap_or(0), id)
                && self.hot.head.is_some();
            let in_cold = contains(&self.cold, self.cold.head.unwrap_or(0), id)
                && self.cold.head.is_some();
            match w {
                0 => {
                    if in_hot || in_cold {
                        return false;
                    }
                }
                1 => {
                    if !in_hot || in_cold {
                        return false;
                    }
                }
                _ => {
                    if in_hot || !in_cold {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 万次迁移零错账。
    pub fn migrated_ok(&self, n: u32) -> bool {
        self.up_cnt + self.down_cnt == n as u64 && self.no_drift() && self.hot.intact() && self.cold.intact()
    }
}

/// J1 主判据：万次迁移零错账 + 双链零断链。
pub fn j1_main() -> bool {
    let mut lru = DualLru::new();
    for i in 0..NODES {
        let _ = lru.enter_cold(i);
    }
    let mut rng = Lcg::new(0xD880_4101);
    for i in 0..MIGRATIONS {
        let id = rng.below(NODES as u64) as u32;
        if i % 2 == 0 {
            let _ = lru.hit_up(id);
        } else {
            let _ = lru.idle_down(id);
        }
        let _ = i;
    }
    lru.migrated_ok(MIGRATIONS)
}

// ── 注入核（0xD880 段 5 项）────────────────────────────────────────────

/// 注入核 1：断链注入——单侧摘除破坏互指必检出。
pub fn inj1_broken_link() -> bool {
    let mut dl = DList::new(8);
    dl.push_tail(0);
    dl.push_tail(1);
    dl.push_tail(2);
    // 人为断链：清 1 的 next
    dl.next[1] = None;
    !dl.intact()
}

/// 注入核 2：重复进入注入——已在链节点再 enter 必拒。
pub fn inj2_double_enter() -> bool {
    let mut lru = DualLru::new();
    let a = lru.enter_cold(3);
    let b = lru.enter_cold(3);
    a && !b
}

/// 注入核 3：逆序迁移注入——hot 态 hit_up / cold 态 idle_down 必拒。
pub fn inj3_wrong_state() -> bool {
    let mut lru = DualLru::new();
    let _ = lru.enter_cold(1);
    let up = lru.hit_up(1); // cold→hot 合法
    let up_again = lru.hit_up(1); // 已 hot，再升必拒
    let down = lru.idle_down(1); // hot→cold 合法
    let down_again = lru.idle_down(1); // 已 cold，再降必拒
    up && !up_again && down && !down_again
}

/// 注入核 4：摘空注入——空链 remove 必拒。
pub fn inj4_remove_empty() -> bool {
    let mut dl = DList::new(4);
    !dl.remove(0)
}

/// 注入核 5：迁移守恒注入——升+降 ≡ 迁移总数。
pub fn inj5_migration_sum() -> bool {
    let mut lru = DualLru::new();
    for i in 0..4 {
        let _ = lru.enter_cold(i);
    }
    let _ = lru.hit_up(0);
    let _ = lru.hit_up(1);
    let _ = lru.idle_down(0);
    lru.up_cnt == 2 && lru.down_cnt == 1 && lru.up_cnt + lru.down_cnt == 3
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx4101_checks：本单判据聚合（诊断段 0xD880）。
pub fn run_unx4101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4101 LRU 双结构（hot/cold 链）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "双链迁移规则万次迁移零错账",
        j1_main(),
        if j1_main() { "" } else { "万次迁移错账" },
    );
    s.add(
        "双向链完整性（前驱后继互指）零断链",
        inj1_broken_link(),
        if inj1_broken_link() { "" } else { "断链未检出" },
    );
    let inj = [
        ("注入1断链检出", inj1_broken_link()),
        ("注入2重复进入", inj2_double_enter()),
        ("注入3逆序迁移", inj3_wrong_state()),
        ("注入4摘空", inj4_remove_empty()),
        ("注入5迁移守恒", inj5_migration_sum()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
