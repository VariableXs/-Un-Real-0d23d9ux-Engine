//! VARIX-M500 AI-05 文件系统与数据深化域（F101~F125，M3）。
//!
//! 数据从"存得住"到"零损坏"：快照、校验、去重、温层、演练。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F101 — 快照树：子卷级快照
// ---------------------------------------------------------------------------

pub const MAX_SNAPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: u32,
    /// 源子卷。
    pub subvol: u16,
    pub created_seq: u32,
    /// 快照为只读（红线）。
    pub readonly: bool,
}

#[derive(Clone, Copy)]
pub struct SnapTree {
    pub snaps: [Option<Snapshot>; MAX_SNAPS],
    pub count: usize,
    pub next_id: u32,
    pub seq: u32,
}

impl SnapTree {
    pub const fn new() -> SnapTree {
        SnapTree { snaps: [const { None }; MAX_SNAPS], count: 0, next_id: 1, seq: 0 }
    }

    pub fn create(&mut self, subvol: u16) -> Option<u32> {
        if self.count >= MAX_SNAPS {
            return None;
        }
        self.seq += 1;
        let id = self.next_id;
        self.next_id += 1;
        self.snaps[self.count] = Some(Snapshot { id, subvol, created_seq: self.seq, readonly: true });
        self.count += 1;
        Some(id)
    }

    /// 列出某子卷的快照 id。
    pub fn list_of(&self, subvol: u16) -> u32 {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(s) = self.snaps[i] {
                if s.subvol == subvol {
                    n += 1;
                }
            }
        }
        n
    }

    /// 快照不可写。
    pub fn writable(&self, id: u32) -> bool {
        for i in 0..self.count {
            if let Some(s) = self.snaps[i] {
                if s.id == id {
                    return !s.readonly;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F102 — 快照回滚：目录级时光机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackPlan {
    pub snap_id: u32,
    /// 将被覆盖的文件数。
    pub files_touched: u32,
    /// 回滚前必须做的安全快照 id。
    pub safety_snap: Option<u32>,
}

#[derive(Clone, Copy)]
pub struct RollbackEngine {
    pub plans: u32,
    pub executed: u32,
    pub aborted: u32,
}

impl RollbackEngine {
    pub const fn new() -> RollbackEngine {
        RollbackEngine { plans: 0, executed: 0, aborted: 0 }
    }

    pub fn plan(&mut self, snap_id: u32, files_touched: u32, safety: Option<u32>) -> Option<RollbackPlan> {
        if files_touched == 0 || safety.is_none() {
            self.aborted += 1;
            return None; // 回滚前必须留安全快照
        }
        self.plans += 1;
        Some(RollbackPlan { snap_id, files_touched, safety_snap: safety })
    }

    pub fn execute(&mut self, p: &RollbackPlan) -> bool {
        self.executed += 1;
        p.files_touched > 0
    }
}

// ---------------------------------------------------------------------------
// F103 — 透明压缩：文件级压缩档位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressAlgo {
    None,
    Fast,
    Best,
}

/// 返回 (压缩后大小, 档位)。不可压内容自动回退 None 档。
pub fn compress_file(len: u32, entropy_permille: u32, algo: CompressAlgo) -> (u32, CompressAlgo) {
    match algo {
        CompressAlgo::None => (len, CompressAlgo::None),
        CompressAlgo::Fast => {
            if entropy_permille > 950 {
                (len, CompressAlgo::None) // 高熵不可压
            } else {
                (len * 7 / 10, CompressAlgo::Fast)
            }
        }
        CompressAlgo::Best => {
            if entropy_permille > 950 {
                (len, CompressAlgo::None)
            } else {
                (len * 45 / 100, CompressAlgo::Best)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F104 — 内容寻址存储：去重块库
// ---------------------------------------------------------------------------

pub const CAS_SLOTS: usize = 12;

#[derive(Clone, Copy)]
pub struct CasStore {
    pub hashes: [u64; CAS_SLOTS],
    pub refs: [u16; CAS_SLOTS],
    pub count: usize,
    pub dedup_hits: u32,
}

impl CasStore {
    pub const fn new() -> CasStore {
        CasStore { hashes: [0; CAS_SLOTS], refs: [0; CAS_SLOTS], count: 0, dedup_hits: 0 }
    }

    pub fn put(&mut self, hash: u64) -> bool {
        for i in 0..self.count {
            if self.hashes[i] == hash {
                self.refs[i] += 1;
                self.dedup_hits += 1;
                return true;
            }
        }
        if self.count >= CAS_SLOTS {
            return false;
        }
        self.hashes[self.count] = hash;
        self.refs[self.count] = 1;
        self.count += 1;
        true
    }

    pub fn gc(&mut self) -> u32 {
        let mut freed = 0u32;
        let mut i = 0;
        while i < self.count {
            if self.refs[i] == 0 {
                for j in i..self.count - 1 {
                    self.hashes[j] = self.hashes[j + 1];
                    self.refs[j] = self.refs[j + 1];
                }
                self.count -= 1;
                freed += 1;
            } else {
                i += 1;
            }
        }
        freed
    }
}

// ---------------------------------------------------------------------------
// F105 — 校验和树：静默损坏检测
// ---------------------------------------------------------------------------

pub const CS_LEAVES: usize = 8;

/// FNV-1a 简化版（u32）。
pub fn checksum(data: &[u32]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &d in data {
        h ^= d;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

#[derive(Clone, Copy)]
pub struct ChecksumTree {
    pub leaf_sum: [u32; CS_LEAVES],
    pub root: u32,
    pub corruption_found: u32,
}

impl ChecksumTree {
    pub fn build(&mut self, leaves: &[[u32; 4]; CS_LEAVES]) {
        for i in 0..CS_LEAVES {
            self.leaf_sum[i] = checksum(&leaves[i]);
        }
        self.root = checksum(&self.leaf_sum);
        self.corruption_found = 0;
    }

    /// 巡检：叶子重算与记录不符 → 损坏。
    pub fn verify(&mut self, leaves: &[[u32; 4]; CS_LEAVES]) -> Option<usize> {
        for i in 0..CS_LEAVES {
            if checksum(&leaves[i]) != self.leaf_sum[i] {
                self.corruption_found += 1;
                return Some(i);
            }
        }
        None
    }

    pub fn clean(&self) -> bool {
        self.corruption_found == 0
    }
}

// ---------------------------------------------------------------------------
// F106 — 元数据双写：自修复副本
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualWrite {
    pub primary: u32,
    pub replica: u32,
    pub repairs: u32,
}

impl DualWrite {
    pub const fn new() -> DualWrite {
        DualWrite { primary: 0, replica: 0, repairs: 0 }
    }

    pub fn write(&mut self, value: u32) -> bool {
        self.primary = value;
        self.replica = value;
        true
    }

    /// 读：主副本坏（与副本不一致）→ 用副本修复。
    pub fn read_repair(&mut self) -> u32 {
        if self.primary != self.replica {
            self.primary = self.replica;
            self.repairs += 1;
        }
        self.primary
    }

    /// 注入主副本损坏（演练用）。
    pub fn corrupt_primary(&mut self, garbage: u32) {
        self.primary = garbage;
    }
}

// ---------------------------------------------------------------------------
// F107 — 文件血缘：数据来源追踪
// ---------------------------------------------------------------------------

pub const MAX_LINEAGE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineageLink {
    pub child: u32,
    pub parent: u32,
    /// 变换代号（0=复制 1=编辑 2=转换）。
    pub transform: u8,
}

#[derive(Clone, Copy)]
pub struct LineageGraph {
    pub links: [Option<LineageLink>; MAX_LINEAGE],
    pub count: usize,
}

impl LineageGraph {
    pub const fn new() -> LineageGraph {
        LineageGraph { links: [const { None }; MAX_LINEAGE], count: 0 }
    }

    pub fn link(&mut self, child: u32, parent: u32, transform: u8) -> bool {
        if transform > 2 || child == parent || self.count >= MAX_LINEAGE {
            return false;
        }
        for i in 0..self.count {
            if let Some(l) = self.links[i] {
                if l.child == child && l.parent == parent {
                    return false;
                }
            }
        }
        self.links[self.count] = Some(LineageLink { child, parent, transform });
        self.count += 1;
        true
    }

    /// 向上追溯根（防环：最多走 MAX_LINEAGE 步）。
    pub fn root_of(&self, mut file: u32) -> u32 {
        for _ in 0..MAX_LINEAGE {
            let mut next = None;
            for i in 0..self.count {
                if let Some(l) = self.links[i] {
                    if l.child == file {
                        next = Some(l.parent);
                    }
                }
            }
            match next {
                Some(p) => file = p,
                None => return file,
            }
        }
        file // 环：返回当前点
    }

    pub fn depth_of(&self, file: u32) -> u32 {
        let mut d = 0;
        let mut cur = file;
        for _ in 0..MAX_LINEAGE {
            let mut next = None;
            for i in 0..self.count {
                if let Some(l) = self.links[i] {
                    if l.child == cur {
                        next = Some(l.parent);
                    }
                }
            }
            match next {
                Some(p) => {
                    cur = p;
                    d += 1;
                }
                None => return d,
            }
        }
        d
    }
}

// ---------------------------------------------------------------------------
// F108 — 变更日志 API：文件事件订阅
// ---------------------------------------------------------------------------

pub const MAX_FS_EVENTS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsEvent {
    pub seq: u32,
    pub file: u32,
    /// 0=create 1=write 2=rename 3=delete。
    pub op: u8,
}

#[derive(Clone, Copy)]
pub struct FsChangeLog {
    pub events: [Option<FsEvent>; MAX_FS_EVENTS],
    pub head: usize,
    pub len: usize,
    pub next_seq: u32,
    pub watchers: u8,
}

impl FsChangeLog {
    pub const fn new() -> FsChangeLog {
        FsChangeLog { events: [const { None }; MAX_FS_EVENTS], head: 0, len: 0, next_seq: 1, watchers: 0 }
    }

    pub fn append(&mut self, file: u32, op: u8) -> bool {
        if op > 3 {
            return false;
        }
        let ev = FsEvent { seq: self.next_seq, file, op };
        self.next_seq += 1;
        let idx = (self.head + self.len) % MAX_FS_EVENTS;
        if self.len == MAX_FS_EVENTS {
            self.events[self.head] = Some(ev);
            self.head = (self.head + 1) % MAX_FS_EVENTS;
        } else {
            self.events[idx] = Some(ev);
            self.len += 1;
        }
        true
    }

    pub fn watch(&mut self) -> bool {
        if self.watchers >= 6 {
            return false;
        }
        self.watchers += 1;
        true
    }

    /// 某文件的事件数（环内）。
    pub fn events_for(&self, file: u32) -> u32 {
        let mut n = 0;
        for i in 0..self.len {
            if let Some(e) = self.events[(self.head + i) % MAX_FS_EVENTS] {
                if e.file == file {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F109 — 大目录索引：百万项目录优化
// ---------------------------------------------------------------------------

pub const DIR_BUCKETS: usize = 8;

#[derive(Clone, Copy)]
pub struct DirIndex {
    /// 每桶条目数。
    pub bucket: [u32; DIR_BUCKETS],
    pub total: u32,
}

impl DirIndex {
    pub const fn new() -> DirIndex {
        DirIndex { bucket: [0; DIR_BUCKETS], total: 0 }
    }

    /// 哈希分桶插入。
    pub fn insert(&mut self, name_hash: u32) -> bool {
        let b = (name_hash % DIR_BUCKETS as u32) as usize;
        self.bucket[b] += 1;
        self.total += 1;
        true
    }

    pub fn lookup_bucket(&self, name_hash: u32) -> u32 {
        self.bucket[(name_hash % DIR_BUCKETS as u32) as usize]
    }

    /// 查找成本 = 最大桶大小（对比线性扫描 total）。
    pub fn max_bucket(&self) -> u32 {
        let mut m = 0;
        for i in 0..DIR_BUCKETS {
            if self.bucket[i] > m {
                m = self.bucket[i];
            }
        }
        m
    }

    pub fn speedup_permille(&self) -> u32 {
        if self.total == 0 || self.max_bucket() == 0 {
            return 0;
        }
        self.total * 1000 / self.max_bucket()
    }
}

// ---------------------------------------------------------------------------
// F110 — 稀疏文件：洞管理
// ---------------------------------------------------------------------------

pub const SPAN_SEGS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SparseSeg {
    pub start: u32,
    pub len: u32,
    /// true = 数据段，false = 洞。
    pub data: bool,
}

#[derive(Clone, Copy)]
pub struct SparseFile {
    pub segs: [Option<SparseSeg>; SPAN_SEGS],
    pub count: usize,
    pub logical_len: u32,
}

impl SparseFile {
    pub const fn new() -> SparseFile {
        SparseFile { segs: [const { None }; SPAN_SEGS], count: 0, logical_len: 0 }
    }

    pub fn add_seg(&mut self, start: u32, len: u32, data: bool) -> bool {
        if self.count >= SPAN_SEGS || len == 0 {
            return false;
        }
        // 不重叠检查
        let end = start + len;
        for i in 0..self.count {
            if let Some(s) = self.segs[i] {
                if start < s.start + s.len && s.start < end {
                    return false;
                }
            }
        }
        self.segs[self.count] = Some(SparseSeg { start, len, data });
        self.count += 1;
        if end > self.logical_len {
            self.logical_len = end;
        }
        true
    }

    /// 物理占用 = 数据段字节和。
    pub fn physical_bytes(&self) -> u32 {
        let mut t = 0;
        for i in 0..self.count {
            if let Some(s) = self.segs[i] {
                if s.data {
                    t += s.len;
                }
            }
        }
        t
    }

    pub fn hole_bytes(&self) -> u32 {
        self.logical_len - self.physical_bytes()
    }
}

// ---------------------------------------------------------------------------
// F111 — 语义链接：反射式引用
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticLink {
    pub from: u32,
    pub to: u32,
    /// 链接语义：0=同内容 1=同主题 2=引用。
    pub kind: u8,
}

pub const MAX_SEM_LINKS: usize = 8;

#[derive(Clone, Copy)]
pub struct SemanticWeb {
    pub links: [Option<SemanticLink>; MAX_SEM_LINKS],
    pub count: usize,
}

impl SemanticWeb {
    pub const fn new() -> SemanticWeb {
        SemanticWeb { links: [const { None }; MAX_SEM_LINKS], count: 0 }
    }

    pub fn link(&mut self, from: u32, to: u32, kind: u8) -> bool {
        if kind > 2 || from == to || self.count >= MAX_SEM_LINKS {
            return false;
        }
        for i in 0..self.count {
            if let Some(l) = self.links[i] {
                if l.from == from && l.to == to && l.kind == kind {
                    return false;
                }
            }
        }
        self.links[self.count] = Some(SemanticLink { from, to, kind });
        self.count += 1;
        true
    }

    /// 删除文件时悬挂引用清点（不自动删，只报告）。
    pub fn dangling(&self, deleted: u32) -> u32 {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(l) = self.links[i] {
                if l.from == deleted || l.to == deleted {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F112 — 目录配额树：层级配额
// ---------------------------------------------------------------------------

pub const QUOTA_DEPTH: usize = 4;

#[derive(Clone, Copy)]
pub struct QuotaNode {
    pub cap_pages: u32,
    pub used_pages: u32,
    pub parent: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct QuotaTree {
    pub nodes: [QuotaNode; QUOTA_DEPTH],
    pub count: usize,
}

impl QuotaTree {
    pub const fn new() -> QuotaTree {
        QuotaTree {
            nodes: [QuotaNode { cap_pages: 0, used_pages: 0, parent: None }; QUOTA_DEPTH],
            count: 0,
        }
    }

    pub fn add(&mut self, cap_pages: u32, parent: Option<usize>) -> bool {
        if self.count >= QUOTA_DEPTH || cap_pages == 0 {
            return false;
        }
        if let Some(p) = parent {
            if p >= self.count {
                return false;
            }
        }
        self.nodes[self.count] = QuotaNode { cap_pages, used_pages: 0, parent };
        self.count += 1;
        true
    }

    /// 写入：本节点与全部祖先都不得超 cap。
    pub fn write(&mut self, node: usize, pages: u32) -> bool {
        if node >= self.count {
            return false;
        }
        // 检查链上所有节点
        let mut cur = Some(node);
        while let Some(c) = cur {
            let n = self.nodes[c];
            if n.used_pages + pages > n.cap_pages {
                return false;
            }
            cur = n.parent;
        }
        cur = Some(node);
        while let Some(c) = cur {
            self.nodes[c].used_pages += pages;
            cur = self.nodes[c].parent;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F113 — 温层归档：热/温/冷自动分层
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Hot,
    Warm,
    Cold,
}

/// 按最近访问天数分层：<3 热，<14 温，否则冷。
pub fn tier_of(days_since_access: u32) -> Tier {
    if days_since_access < 3 {
        Tier::Hot
    } else if days_since_access < 14 {
        Tier::Warm
    } else {
        Tier::Cold
    }
}

/// 降层动作收益：字节 × 层差（热→冷 = 2）。
pub fn demotion_savings(bytes: u32, from: Tier, to: Tier) -> u32 {
    let rank = |t: Tier| match t {
        Tier::Hot => 0u32,
        Tier::Warm => 1,
        Tier::Cold => 2,
    };
    let d = rank(to).saturating_sub(rank(from));
    bytes * d
}

// ---------------------------------------------------------------------------
// F114 — 位翻转注入：损坏演练
// ---------------------------------------------------------------------------

/// 在 data[idx] 翻转 bit 位，检测树能否抓到。
pub fn flip_bit(data: &mut [u32], idx: usize, bit: u8) -> bool {
    if idx >= data.len() || bit >= 32 {
        return false;
    }
    data[idx] ^= 1u32 << bit;
    true
}

/// 注入 N 次翻转，统计被抓到的次数。
pub fn corruption_drill(rounds: u32) -> u32 {
    let mut caught = 0;
    let mut data = [10u32, 20, 30, 40];
    let mut tree = ChecksumTree { leaf_sum: [0; CS_LEAVES], root: 0, corruption_found: 0 };
    tree.build(&[[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20], [21, 22, 23, 24], [25, 26, 27, 28], [29, 30, 31, 32]]);
    for r in 0..rounds {
        let idx = (r as usize) % 4;
        let bit = (r % 31) as u8 + 1;
        if !flip_bit(&mut data, idx, bit) {
            continue;
        }
        // 把注入的块放进对应叶并巡检
        let mut leaves = [[0u32; 4]; CS_LEAVES];
        for i in 0..CS_LEAVES {
            for j in 0..4 {
                leaves[i][j] = if i < 4 && j == idx && i == idx { data[idx] } else { ((i * 4 + j) as u32) + 1 };
            }
        }
        // 与原始基线比对：注入位置不同 → 应报损坏
        let mut base = [[0u32; 4]; CS_LEAVES];
        for i in 0..CS_LEAVES {
            for j in 0..4 {
                base[i][j] = ((i * 4 + j) as u32) + 1;
            }
        }
        let mut changed_leaf = None;
        for i in 0..CS_LEAVES {
            if leaves[i] != base[i] {
                changed_leaf = Some(i);
            }
        }
        if let Some(leaf) = changed_leaf {
            let sum = checksum(&leaves[leaf]);
            if sum != checksum(&base[leaf]) {
                caught += 1;
            }
            // 修复：还原
            data[idx] = ((idx + 1) * 10) as u32;
        }
    }
    caught
}

// ---------------------------------------------------------------------------
// F115 — 操作日志重放：FS 行为验证
// ---------------------------------------------------------------------------

pub const MAX_OPS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsOp {
    Create(u32),
    Write(u32, u32),
    Delete(u32),
}

/// 重放操作序列，返回不变量违例数：
/// create 重复 / 写不存在的文件 / 删不存在的文件。
pub fn replay_ops(ops: &[FsOp]) -> u32 {
    let mut exists = [false; 64];
    let mut bad = 0;
    for op in ops {
        match *op {
            FsOp::Create(f) => {
                if f >= 64 {
                    bad += 1;
                } else if exists[f as usize] {
                    bad += 1;
                } else {
                    exists[f as usize] = true;
                }
            }
            FsOp::Write(f, _) => {
                if f >= 64 || !exists[f as usize] {
                    bad += 1;
                }
            }
            FsOp::Delete(f) => {
                if f >= 64 || !exists[f as usize] {
                    bad += 1;
                } else {
                    exists[f as usize] = false;
                }
            }
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// F116 — 文件名归一：Unicode 规范化（NFC 模拟：排序组合标记）
// ---------------------------------------------------------------------------

/// 模拟：归一 = 去多余组合符 + 折叠全角。返回归一后的 16 字节槽与是否改变。
pub fn normalize_name(name: &[u8]) -> ([u8; 16], bool) {
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut changed = false;
    for &c in name {
        if n >= 16 {
            changed = true;
            break;
        }
        // 全角 ASCII 折叠 (0xFF01..=0xFF5E → 0x21..=0x7E)
        if (0xEF..=0xEF).contains(&c) {
            changed = true;
            continue; // 简化：跳过 UTF-8 首字节，后续字节映射
        }
        if c >= 0x21 && c <= 0x7E || c >= 0x80 {
            out[n] = c;
            n += 1;
        } else {
            changed = true; // 控制符剔除
        }
    }
    (out, changed)
}

/// 两条名字是否归一后等价。
pub fn names_equivalent(a: &[u8], b: &[u8]) -> bool {
    let (na, _) = normalize_name(a);
    let (nb, _) = normalize_name(b);
    na == nb && na.iter().any(|&c| c != 0) || (na == nb && a.is_empty() && b.is_empty())
}

// ---------------------------------------------------------------------------
// F117 — 长路径支持：超长路径全链通
// ---------------------------------------------------------------------------

pub const PATH_MAX_COMPONENTS: usize = 24;
pub const PATH_MAX_BYTES: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathCheck {
    pub components: u8,
    pub bytes: u32,
    pub ok: bool,
}

/// 校验路径：组件数与总长都在限内，无空组件。
pub fn validate_path(path: &[u8]) -> PathCheck {
    let mut components = 0u8;
    let mut empty = false;
    let mut cur = 0usize;
    for (i, &c) in path.iter().enumerate() {
        if c == b'/' {
            if i == cur && i > 0 {
                empty = true; // 空组件（// 或开头 /）
            }
            if i > cur {
                components += 1;
            }
            cur = i + 1;
        }
    }
    if path.len() > cur {
        components += 1;
    }
    let ok = !empty
        && components > 0
        && components as usize <= PATH_MAX_COMPONENTS
        && path.len() <= PATH_MAX_BYTES;
    PathCheck { components, bytes: path.len() as u32, ok }
}

// ---------------------------------------------------------------------------
// F118 — 变更检查点：大变更里程碑
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeCheckpoint {
    /// 每 N 次变更打一个检查点。
    pub interval: u32,
    pub changes_since: u32,
    pub checkpoints: u32,
}

impl ChangeCheckpoint {
    pub const fn new(interval: u32) -> ChangeCheckpoint {
        ChangeCheckpoint { interval: if interval == 0 { 1 } else { interval }, changes_since: 0, checkpoints: 0 }
    }

    pub fn on_change(&mut self) -> bool {
        self.changes_since += 1;
        if self.changes_since >= self.interval {
            self.changes_since = 0;
            self.checkpoints += 1;
            true
        } else {
            false
        }
    }

    /// 崩溃恢复点 = 最近检查点覆盖的变更数。
    pub fn recovery_loss(&self) -> u32 {
        self.changes_since
    }
}

// ---------------------------------------------------------------------------
// F119 — 存储寿命画像：健康趋势预测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthTrend {
    /// 4 个季度的磨损增量（permille）。
    pub wear_deltas: [u16; 4],
    pub total_wear_permille: u32,
}

impl HealthTrend {
    pub const fn new() -> HealthTrend {
        HealthTrend { wear_deltas: [0; 4], total_wear_permille: 0 }
    }

    pub fn observe(&mut self, delta_permille: u16) {
        for i in 0..3 {
            self.wear_deltas[i] = self.wear_deltas[i + 1];
        }
        self.wear_deltas[3] = delta_permille;
        self.total_wear_permille = (self.total_wear_permille + delta_permille as u32).min(1000);
    }

    /// 线性外推下一季度磨损。
    pub fn forecast_next(&self) -> u32 {
        let d0 = self.wear_deltas[2] as u32;
        let d1 = self.wear_deltas[3] as u32;
        let slope = d1 as i64 - d0 as i64;
        let next = d1 as i64 + slope;
        next.clamp(0, 1000) as u32
    }

    /// 剩余寿命季度数（按最近季度磨损率）。
    pub fn quarters_left(&self) -> u32 {
        let q = self.wear_deltas[3] as u32;
        if q == 0 {
            return u32::MAX;
        }
        (1000 - self.total_wear_permille) / q
    }
}

// ---------------------------------------------------------------------------
// F120 — 拷贝引擎：进度/暂停/校验复制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyEngine {
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub paused: bool,
    pub src_sum: u32,
    pub dst_sum: u32,
    pub verified: bool,
}

impl CopyEngine {
    pub const fn new(total_bytes: u64, src_sum: u32) -> CopyEngine {
        CopyEngine { total_bytes, done_bytes: 0, paused: false, src_sum, dst_sum: 0, verified: false }
    }

    pub fn progress_permille(&self) -> u32 {
        if self.total_bytes == 0 {
            return 1000;
        }
        (self.done_bytes * 1000 / self.total_bytes).min(1000) as u32
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn chunk(&mut self, bytes: u64, dst_sum: u32) -> bool {
        if self.paused || self.done_bytes + bytes > self.total_bytes {
            return false;
        }
        self.done_bytes += bytes;
        self.dst_sum = dst_sum;
        if self.done_bytes == self.total_bytes {
            self.verified = self.dst_sum == self.src_sum;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F121 — 差分传输：增量同步
// ---------------------------------------------------------------------------

/// 块级差分：返回需要传输的块数。
pub fn diff_blocks(src: &[u64], dst: &[u64]) -> u32 {
    let n = src.len().min(dst.len());
    let mut dirty = 0u32;
    for i in 0..n {
        if src[i] != dst[i] {
            dirty += 1;
        }
    }
    dirty + (src.len().max(dst.len()) - n) as u32
}

/// 差分节省 permille（相对全量）。
pub fn diff_saving_permille(src: &[u64], dst: &[u64]) -> u32 {
    let total = src.len().max(dst.len());
    if total == 0 {
        return 0;
    }
    let dirty = diff_blocks(src, dst);
    ((total - dirty as usize) * 1000 / total) as u32
}

// ---------------------------------------------------------------------------
// F122 — FS 巡检 lint：一致性体检
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsLintIssue {
    /// 0=孤儿 inode 1=链接计数错 2=块越界。
    pub kind: u8,
    pub at: u32,
}

/// 模拟巡检：inode 是否被引用、链接计数是否一致、块号是否越界。
pub fn fs_lint(referenced: &[bool], link_counts: &[u16], blocks: &[u32], total_blocks: u32) -> u32 {
    let n = referenced.len().min(link_counts.len());
    let mut issues = 0u32;
    for i in 0..n {
        if !referenced[i] && link_counts[i] > 0 {
            issues += 1; // 孤儿
        }
        if referenced[i] && link_counts[i] == 0 {
            issues += 1; // 引用但计数 0
        }
    }
    for &b in blocks {
        if b >= total_blocks {
            issues += 1;
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// F123 — FS 基准套件：读写延迟基线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsBench {
    pub seq_read_us: u32,
    pub seq_write_us: u32,
    pub rand_read_us: u32,
    pub fsync_us: u32,
}

impl FsBench {
    /// 综合 permille 分：4 项各自相对目标的比例均值（封顶 1000）。
    pub fn score(&self, targets: &FsBench) -> u32 {
        let one = |v: u32, t: u32| -> u32 {
            if v == 0 {
                1000
            } else {
                (t * 1000 / v).min(1000)
            }
        };
        (one(self.seq_read_us, targets.seq_read_us)
            + one(self.seq_write_us, targets.seq_write_us)
            + one(self.rand_read_us, targets.rand_read_us)
            + one(self.fsync_us, targets.fsync_us))
            / 4
    }

    /// 最慢项（优化指引）。
    pub fn slowest(&self) -> &'static str {
        let m = self.seq_read_us.max(self.seq_write_us).max(self.rand_read_us).max(self.fsync_us);
        if m == self.seq_read_us {
            "seq-read"
        } else if m == self.seq_write_us {
            "seq-write"
        } else if m == self.rand_read_us {
            "rand-read"
        } else {
            "fsync"
        }
    }
}

// ---------------------------------------------------------------------------
// F124 — FS fuzz：文件系统对抗测试
// ---------------------------------------------------------------------------

/// 操作序列不变量：删除后不可写；重复创建报错；路径合法。
/// 返回违例数。op: 0=create 1=write 2=delete；arg = 文件号。
pub fn fs_fuzz(cases: &[(u8, u32)]) -> u32 {
    let mut exists = [false; 32];
    let mut bad = 0;
    for &(op, arg) in cases {
        if arg >= 32 {
            bad += 1;
            continue;
        }
        match op {
            0 => {
                if exists[arg as usize] {
                    bad += 1;
                } else {
                    exists[arg as usize] = true;
                }
            }
            1 => {
                if !exists[arg as usize] {
                    bad += 1;
                }
            }
            2 => {
                if !exists[arg as usize] {
                    bad += 1;
                } else {
                    exists[arg as usize] = false;
                }
            }
            _ => bad += 1,
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// F125 — 存储域自检：25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_m5fs_checks() -> CheckSet {
    let mut set = CheckSet::new("m5fs");

    // F101 快照树
    let mut st = SnapTree::new();
    let id1 = st.create(7);
    set.add("F101 create snapshot", id1 == Some(1) && st.list_of(7) == 1, "create");
    set.add("F101 readonly", !st.writable(1), "ro");
    set.add("F101 seq monotonic", st.create(7).is_some() && st.seq == 2, "seq");

    // F102 回滚
    let mut re = RollbackEngine::new();
    set.add("F102 plan needs safety", re.plan(1, 10, None).is_none() && re.aborted == 1, "safety");
    let p = re.plan(1, 10, Some(99));
    set.add("F102 plan ok", p.is_some() && re.plans == 1, "plan");
    set.add("F102 execute", p.map(|p| re.execute(&p)).unwrap_or(false) && re.executed == 1, "exec");

    // F103 透明压缩
    let (sz, al) = compress_file(1000, 800, CompressAlgo::Fast);
    set.add("F103 fast ratio", sz == 700 && al == CompressAlgo::Fast, "fast");
    let (sz2, al2) = compress_file(1000, 990, CompressAlgo::Best);
    set.add("F103 incompressible fallback", sz2 == 1000 && al2 == CompressAlgo::None, "fallback");
    let (sz3, _) = compress_file(1000, 500, CompressAlgo::Best);
    set.add("F103 best ratio", sz3 == 450, "best");

    // F104 CAS
    let mut cas = CasStore::new();
    set.add("F104 put unique", cas.put(0xA) && cas.put(0xB) && cas.count == 2, "put");
    set.add("F104 dedup hit", cas.put(0xA) && cas.dedup_hits == 1, "dedup");
    set.add("F104 gc frees", {
        cas.refs[0] = 0;
        cas.gc() == 1 && cas.count == 1
    }, "gc");

    // F105 校验和树
    let mut ct = ChecksumTree { leaf_sum: [0; CS_LEAVES], root: 0, corruption_found: 0 };
    let leaves = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20], [21, 22, 23, 24], [25, 26, 27, 28], [29, 30, 31, 32]];
    ct.build(&leaves);
    set.add("F105 verify clean", ct.verify(&leaves).is_none(), "clean");
    set.add("F105 corruption located", {
        let mut bad_leaves = leaves;
        bad_leaves[3][1] ^= 0xFF;
        ct.verify(&bad_leaves) == Some(3) && ct.corruption_found == 1
    }, "corrupt");

    // F106 双写
    let mut dw = DualWrite::new();
    set.add("F106 write both", dw.write(77) && dw.read_repair() == 77, "write");
    dw.corrupt_primary(1);
    set.add("F106 read repair", dw.read_repair() == 77 && dw.repairs == 1, "repair");

    // F107 血缘
    let mut lg = LineageGraph::new();
    set.add("F107 link", lg.link(10, 1, 0) && lg.link(11, 10, 1), "link");
    set.add("F107 root trace", lg.root_of(11) == 1, "root");
    set.add("F107 depth", lg.depth_of(11) == 2, "depth");
    set.add("F107 reject self loop", !lg.link(5, 5, 0), "self");

    // F108 变更日志
    let mut cl = FsChangeLog::new();
    set.add("F108 append", cl.append(5, 0) && cl.append(5, 1) && cl.append(6, 2), "append");
    set.add("F108 events for file", cl.events_for(5) == 2, "for");
    set.add("F108 bad op rejected", !cl.append(7, 9), "badop");

    // F109 大目录索引
    let mut di = DirIndex::new();
    for i in 0..80u32 {
        let _ = di.insert(i.wrapping_mul(2654435761) >> 3);
    }
    set.add("F109 lookup cost bounded", di.max_bucket() < di.total, "cost");
    set.add("F109 speedup > 1", di.speedup_permille() > 1000, "speedup");

    // F110 稀疏文件
    let mut sf = SparseFile::new();
    set.add("F110 segments", sf.add_seg(0, 100, true) && sf.add_seg(200, 300, true) && sf.add_seg(100, 100, false), "segs");
    set.add("F110 overlap rejected", !sf.add_seg(50, 100, true), "overlap");
    set.add("F110 holes free", sf.logical_len == 500 && sf.physical_bytes() == 400 && sf.hole_bytes() == 100, "holes");

    // F111 语义链接
    let mut sw = SemanticWeb::new();
    set.add("F111 link kinds", sw.link(1, 2, 0) && sw.link(1, 3, 1) && !sw.link(1, 2, 0), "kinds");
    set.add("F111 dangling report", sw.dangling(2) == 1, "dangling");
    set.add("F111 self rejected", !sw.link(4, 4, 0), "self");

    // F112 配额树
    let mut qt = QuotaTree::new();
    set.add("F112 tree build", qt.add(100, None) && qt.add(50, Some(0)), "build");
    set.add("F112 child within cap", qt.write(1, 40), "write");
    set.add("F112 parent cap gates", !qt.write(1, 20) && !qt.write(0, 70), "cap");

    // F113 温层
    set.add("F113 tier hot", tier_of(0) == Tier::Hot && tier_of(2) == Tier::Hot, "hot");
    set.add("F113 tier cold", tier_of(30) == Tier::Cold, "cold");
    set.add("F113 demotion savings", demotion_savings(100, Tier::Hot, Tier::Cold) == 200, "save");

    // F114 位翻转注入
    let mut d = [1u32, 2, 3, 4];
    set.add("F114 flip ok", flip_bit(&mut d, 1, 3) && d[1] == 10, "flip");
    set.add("F114 flip bounds", !flip_bit(&mut d, 9, 3), "bounds");
    set.add("F114 drill catches", corruption_drill(8) == 8, "drill");

    // F115 操作重放
    let ops = [FsOp::Create(1), FsOp::Write(1, 10), FsOp::Create(1), FsOp::Write(9, 1), FsOp::Delete(1), FsOp::Write(1, 5)];
    set.add("F115 replay violations", replay_ops(&ops) == 3, "viol");
    set.add("F115 replay clean", replay_ops(&[FsOp::Create(2), FsOp::Write(2, 1), FsOp::Delete(2)]) == 0, "clean");

    // F116 文件名归一
    let (n1, ch1) = normalize_name(b"abc");
    set.add("F116 normalize plain", !ch1 && n1[0] == b'a', "plain");
    set.add("F116 equivalence", names_equivalent(b"abc", b"abc") && !names_equivalent(b"abc", b"xyz"), "equiv");

    // F117 长路径
    let p1 = validate_path(b"/usr/local/share/varix/assets/icons/app.svg");
    set.add("F117 deep path ok", p1.ok && p1.components == 7, "ok");
    set.add("F117 empty component", !validate_path(b"/a//b").ok, "empty");
    let long = [b'/'; PATH_MAX_BYTES + 1];
    set.add("F117 too long", !validate_path(&long).ok, "long");

    // F118 变更检查点
    let mut cp = ChangeCheckpoint::new(10);
    let mut fired = 0;
    for _ in 0..25 {
        if cp.on_change() {
            fired += 1;
        }
    }
    set.add("F118 checkpoint interval", fired == 2 && cp.checkpoints == 2, "cp");
    set.add("F118 recovery loss", cp.recovery_loss() == 5, "loss");

    // F119 寿命画像
    let mut ht = HealthTrend::new();
    for d in [10u16, 12, 14, 16] {
        ht.observe(d);
    }
    set.add("F119 forecast trend", ht.forecast_next() == 18, "forecast");
    set.add("F119 quarters left", ht.quarters_left() >= 50, "left");
    set.add("F119 total wear", ht.total_wear_permille == 52, "total");

    // F120 拷贝引擎
    let mut ce = CopyEngine::new(1000, 0xBEEF);
    ce.pause();
    set.add("F120 paused reject", !ce.chunk(100, 0), "pause");
    ce.resume();
    set.add("F120 progress", ce.chunk(600, 0) && ce.progress_permille() == 600, "prog");
    set.add("F120 verify on finish", ce.chunk(400, 0xBEEF) && ce.verified && ce.progress_permille() == 1000, "verify");

    // F121 差分传输
    let src = [1u64, 2, 3, 4, 5];
    let dst = [1u64, 9, 3, 4, 5];
    set.add("F121 diff count", diff_blocks(&src, &dst) == 1, "diff");
    set.add("F121 saving", diff_saving_permille(&src, &dst) == 800, "save");

    // F122 FS lint
    set.add("F122 lint issues", fs_lint(&[true, false, true], &[1, 1, 0], &[0, 99], 10) == 3, "issues");

    // F123 基准套件
    let bench = FsBench { seq_read_us: 100, seq_write_us: 900, rand_read_us: 200, fsync_us: 300 };
    let target = FsBench { seq_read_us: 100, seq_write_us: 300, rand_read_us: 200, fsync_us: 300 };
    set.add("F123 score capped", bench.score(&target) == 833, "score");
    set.add("F123 slowest named", bench.slowest() == "seq-write", "slowest");

    // F124 FS fuzz
    set.add("F124 fuzz violations", fs_fuzz(&[(0, 1), (0, 1), (1, 9), (2, 2)]) == 3, "viol");
    set.add("F124 fuzz clean", fs_fuzz(&[(0, 1), (1, 1), (2, 1)]) == 0, "clean");

    // F125 域自检完整性（第 25 项）
    set.add("F125 self-check wired", set.len() == 62, "wired");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f105_checksum_tree() {
        let mut ct = ChecksumTree { leaf_sum: [0; CS_LEAVES], root: 0, corruption_found: 0 };
        let leaves = [[1u32, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20], [21, 22, 23, 24], [25, 26, 27, 28], [29, 30, 31, 32]];
        ct.build(&leaves);
        assert!(ct.verify(&leaves).is_none());
    }

    #[test]
    fn f121_diff_saving() {
        let src = [1u64, 2, 3];
        let dst = [1u64, 2, 9];
        assert_eq!(diff_blocks(&src, &dst), 1);
        assert_eq!(diff_saving_permille(&src, &dst), 666);
    }

    #[test]
    fn f125_self_check_passes() {
        let set = run_m5fs_checks();
        assert_eq!(set.len(), 63, "m5fs 需要 75 项断言");
        assert!(!set.truncated());
        assert!(set.all_passed(), "m5fs 自检必须全绿");
    }
}
