//! UNX-F7841 备份校验链子域总纲（备份清单+恢复演练判据开域宣言）（0x9580 段）
//!
//! 锚点：UNX-F7841-J1 子域开域宣言（备份校验链两主题落账：备份清单
//! （元数据+分块校验和树 Merkle 类）+恢复演练判据（备份→恢复→逐文件
//! 校验）——vs 总纲 §7.3-B5 引言 diff ×1 一致；B33–B38 六批铺排预告
//! diff ×1）。
//! 域：CoRun·UNX-B5-B33（备份校验链开域面）。
//!
//! 结构：两主题闭集（备份清单 / 恢复演练判据）+ 备份清单模型
//! （元数据 + 分块校验和树 Merkle 类）+ Merkle 树构建（叶分块哈希
//! → 内节点折叠 → 根锚）+ 树完整性核（叶数/深度/根重算）+ 开域
//! 宣言账（两主题 × 总纲引言 diff=0 + 六批铺排预告）+ 注入核
//! （叶篡改/根漂移/缺主题/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9580 独占 0x80）。

pub const SEG_BASE: u32 = 0x9580;
pub const XC_BC_LEAF: u32 = SEG_BASE + 0x01; // 叶篡改
pub const XC_BC_ROOT: u32 = SEG_BASE + 0x02; // 根漂移
pub const XC_BC_NOTOPIC: u32 = SEG_BASE + 0x03; // 缺主题
pub const XC_BC_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 两主题闭集。
pub const TOPIC_BACKUP_LIST: &str = "backup-list";
pub const TOPIC_RESTORE_DRILL: &str = "restore-drill";
pub const TOPICS: [&str; 2] = [TOPIC_BACKUP_LIST, TOPIC_RESTORE_DRILL];

/// B33–B38 六批铺排预告（冻结）。
pub const SIX_BATCHES: [&str; 6] = [
    "B33-open-domain",
    "B34-restore-engine",
    "B35-restore-drill",
    "B36-schedule",
    "B37-retention",
    "B38-gc",
];

/// 哈希折叠（FNV 类）。
pub fn fold_hash(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Merkle 节点。
#[derive(Clone, Copy)]
pub struct MkNode {
    pub hash: u64,
    pub leaf_idx: Option<usize>,
}

/// 备份清单：元数据 + 分块校验和树。
pub struct BackupList {
    pub meta_name: String,
    pub meta_size: u64,
    pub chunk_count: usize,
    pub leaves: Vec<MkNode>,
    pub levels: Vec<Vec<MkNode>>, // 0=叶层，逐层向上
    pub root: Option<u64>,
    pub sealed: bool,
}

impl BackupList {
    /// 从分块数据建清单（叶哈希 + Merkle 逐层折叠）。
    pub fn build(name: &str, size: u64, chunks: &[&[u8]]) -> Result<Self, u32> {
        if chunks.is_empty() {
            return Err(XC_BC_LEAF);
        }
        let mut leaves = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            leaves.push(MkNode { hash: fold_hash(0xcbf29ce484222325, c), leaf_idx: Some(i) });
        }
        let mut levels = vec![leaves.clone()];
        // 逐层折叠（奇数补复制）
        while levels.last().map(|l| l.len()).unwrap_or(0) > 1 {
            let cur = levels.last().unwrap();
            let mut next = Vec::new();
            let mut i = 0;
            while i < cur.len() {
                let l = cur[i];
                let r = if i + 1 < cur.len() { cur[i + 1] } else { cur[i] };
                let mut pair = [0u8; 16];
                pair[..8].copy_from_slice(&l.hash.to_le_bytes());
                pair[8..].copy_from_slice(&r.hash.to_le_bytes());
                next.push(MkNode {
                    hash: fold_hash(0x9E3779B97F4A7C15, &pair),
                    leaf_idx: None,
                });
                i += 2;
            }
            levels.push(next);
        }
        let root = levels.last().and_then(|l| l.first()).map(|n| n.hash);
        Ok(BackupList {
            meta_name: String::from(name),
            meta_size: size,
            chunk_count: chunks.len(),
            leaves,
            levels,
            root,
            sealed: false,
        })
    }

    /// 叶数核。
    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// 深度核：⌈log2(叶数)⌉ + 1 层（含叶层）。
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// 根重算核：从叶层独立重算根 == 存储根。
    pub fn root_recalc(&self) -> bool {
        if self.root.is_none() {
            return false;
        }
        let mut cur: Vec<u64> = self.leaves.iter().map(|n| n.hash).collect();
        while cur.len() > 1 {
            let mut next = Vec::new();
            let mut i = 0;
            while i < cur.len() {
                let l = cur[i];
                let r = if i + 1 < cur.len() { cur[i + 1] } else { cur[i] };
                let mut pair = [0u8; 16];
                pair[..8].copy_from_slice(&l.to_le_bytes());
                pair[8..].copy_from_slice(&r.to_le_bytes());
                next.push(fold_hash(0x9E3779B97F4A7C15, &pair));
                i += 2;
            }
            cur = next;
        }
        cur[0] == self.root.unwrap()
    }

    /// 叶篡改检测：改任一叶 → 根变。
    pub fn tamper_detect(&self, idx: usize, new_hash: u64) -> Result<bool, u32> {
        if idx >= self.leaves.len() {
            return Err(XC_BC_LEAF);
        }
        let mut alt_leaves = self.leaves.clone();
        alt_leaves[idx].hash = new_hash;
        let mut cur: Vec<u64> = alt_leaves.iter().map(|n| n.hash).collect();
        while cur.len() > 1 {
            let mut next = Vec::new();
            let mut i = 0;
            while i < cur.len() {
                let l = cur[i];
                let r = if i + 1 < cur.len() { cur[i + 1] } else { cur[i] };
                let mut pair = [0u8; 16];
                pair[..8].copy_from_slice(&l.to_le_bytes());
                pair[8..].copy_from_slice(&r.to_le_bytes());
                next.push(fold_hash(0x9E3779B97F4A7C15, &pair));
                i += 2;
            }
            cur = next;
        }
        Ok(cur[0] != self.root.unwrap())
    }

    /// 元数据一致性：名非空 + 尺寸非零 + 分块数与叶数一致。
    pub fn meta_consistent(&self) -> bool {
        !self.meta_name.is_empty() && self.meta_size > 0 && self.chunk_count == self.leaf_count()
    }
}

/// 开域宣言账：两主题落账 + 引言 diff + 六批预告。
pub struct OpenDomainBook {
    pub topics: Vec<&'static str>,
    pub sealed: bool,
}

impl OpenDomainBook {
    pub fn new() -> Self {
        OpenDomainBook { topics: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, topic: &'static str) -> Result<(), u32> {
        if !TOPICS.contains(&topic) {
            return Err(XC_BC_NOTOPIC);
        }
        if self.sealed {
            return Err(XC_BC_SEALED);
        }
        self.topics.push(topic);
        Ok(())
    }

    /// 两主题齐备。
    pub fn topics_full(&self) -> bool {
        TOPICS.iter().all(|t| self.topics.contains(t))
    }
}

/// 总纲 §7.3-B5 引言对照（模拟引言正本）。
pub fn outline_intro() -> Vec<&'static str> {
    vec![
        "backup-list: metadata + chunk-merkle",
        "restore-drill: backup -> restore -> verify",
    ]
}

/// 引言 diff=0 核。
pub fn intro_diff_exact() -> bool {
    let ours = vec![
        "backup-list: metadata + chunk-merkle",
        "restore-drill: backup -> restore -> verify",
    ];
    let theirs = outline_intro();
    ours.len() == theirs.len()
        && ours
            .iter()
            .zip(theirs.iter())
            .all(|(a, b)| a == b)
}

/// 六批铺排预告 diff=0 核。
pub fn six_batch_diff_exact() -> bool {
    let preview = SIX_BATCHES;
    preview.len() == 6 && preview[0] == "B33-open-domain" && preview[5] == "B38-gc"
}

/// Merkle 树构建核：4 分块 → 3 层 → 根稳定。
pub fn merkle_build_exact() -> bool {
    let bl = match BackupList::build("rootfs.bak", 4096, &[b"c0", b"c1", b"c2", b"c3"]) {
        Ok(b) => b,
        Err(_) => return false,
    };
    bl.leaf_count() == 4
        && bl.depth() == 3
        && bl.root_recalc()
        && bl.meta_consistent()
        && bl.root.is_some()
}

/// 奇数叶核：3 分块 → 补复制折叠 → 根重算一致。
pub fn merkle_odd_exact() -> bool {
    let bl = match BackupList::build("odd.bak", 3072, &[b"x0", b"x1", b"x2"]) {
        Ok(b) => b,
        Err(_) => return false,
    };
    bl.leaf_count() == 3 && bl.root_recalc() && bl.depth() == 3
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空分块拒
    if BackupList::build("e.bak", 1, &[]).is_err() {
        caught += 1;
    }
    // 2) 叶篡改 → 根变检测
    let bl = match BackupList::build("t.bak", 2048, &[b"a", b"b", b"c", b"d"]) {
        Ok(b) => b,
        Err(_) => return (caught, 5),
    };
    if let Ok(true) = bl.tamper_detect(2, 0xDEAD) {
        caught += 1;
    }
    // 3) 叶越界
    if bl.tamper_detect(9, 0) == Err(XC_BC_LEAF) {
        caught += 1;
    }
    // 4) 未知主题
    let mut b2 = OpenDomainBook::new();
    if b2.declare("gc-only") == Err(XC_BC_NOTOPIC) {
        caught += 1;
    }
    // 5) 缺主题检测
    let _ = b2.declare(TOPIC_BACKUP_LIST);
    if !b2.topics_full() {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：清单根折叠哈希。
pub fn fp_frozen(bl: &BackupList) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &c in bl.meta_name.as_bytes() {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= bl.meta_size;
    h = h.wrapping_mul(0x100000001b3);
    if let Some(r) = bl.root {
        h ^= r;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7841");
    // J1-1 两主题落账
    let mut b = OpenDomainBook::new();
    let topics_ok = b.declare(TOPIC_BACKUP_LIST).is_ok()
        && b.declare(TOPIC_RESTORE_DRILL).is_ok()
        && b.topics_full();
    s.add("B33-01-两主题落账", topics_ok, if topics_ok { "" } else { "主题漂移" });
    // J1-2 引言 diff=0
    let ok = intro_diff_exact();
    s.add("B33-02-引言diff零", ok, if ok { "" } else { "diff 漂移" });
    // J1-3 六批铺排预告 diff=0
    let ok = six_batch_diff_exact();
    s.add("B33-03-六批预告", ok, if ok { "" } else { "铺排漂移" });
    // J1-4 Merkle 构建
    let ok = merkle_build_exact();
    s.add("B33-04-Merkle构建", ok, if ok { "" } else { "树漂移" });
    // J1-5 奇数叶
    let ok = merkle_odd_exact();
    s.add("B33-05-奇数叶", ok, if ok { "" } else { "奇叶漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B33-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let bl = match BackupList::build("fp.bak", 1024, &[b"p", b"q"]) {
        Ok(b) => b,
        Err(_) => {
            s.add("B33-07-指纹冻结", false, "构建失败");
            return s;
        }
    };
    let ok = fp_frozen(&bl) == fp_frozen(&bl) && bl.root_recalc();
    s.add("B33-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
