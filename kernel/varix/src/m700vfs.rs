//! m700vfs — VARIX-M700 AI-09 虚拟文件系统域 (F201~F225)
//!
//! VFS 操作大典/装载点族谱/路径行走仪/dentry 缓存谱/inode 生命簿/
//! 文件锁谱/权限裁决台/文件系统魔毯/脏页账本/短读短写律/
//! fsync 语义谱/文件系统压力沙盘/符号链接法庭/稀疏文件账/文件系统血统标签/
//! 挂载原子律/路径缓存考古/文件句柄宗卷/元数据日志带/VFS 回归金样/
//! 大目录分片谱/文件事件流/配额执法官/VFS 自描述清单/VFS 域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 自检入口：`run_m700vfs_checks()`（铁律：断言中凡"操作→读→再操作"，
//! 中间读数必须先落入独立 `let` 变量，禁止末态读取）。

use crate::checks::CheckSet;

// ===========================================================================
// F201 — VFS 操作大典：操作码合法性 + 变更类判定
// ===========================================================================

pub const VFS_OP_OPEN: u8 = 1;
pub const VFS_OP_READ: u8 = 2;
pub const VFS_OP_WRITE: u8 = 3;
pub const VFS_OP_CLOSE: u8 = 4;
pub const VFS_OP_STAT: u8 = 5;
pub const VFS_OP_UNLINK: u8 = 6;
pub const VFS_OP_COUNT: u8 = 6;

pub fn vfs_op_valid(op: u8) -> bool {
    op >= VFS_OP_OPEN && op <= VFS_OP_COUNT
}

/// 变更类操作：写与摘除（其余为只读类）。
pub fn vfs_op_mutates(op: u8) -> bool {
    op == VFS_OP_WRITE || op == VFS_OP_UNLINK
}

// ===========================================================================
// F202 — 装载点族谱：装载表去重（同一路径点只挂一卷）
// ===========================================================================

pub const VFS_MOUNTS_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsMount {
    pub dev: u32,
    pub root_path_id: u32,
}

pub struct VfsMountTable {
    mounts: [VfsMount; VFS_MOUNTS_MAX],
    count: usize,
    pub rejects: u32,
}

impl VfsMountTable {
    pub const fn new() -> VfsMountTable {
        VfsMountTable { mounts: [VfsMount { dev: 0, root_path_id: 0 }; VFS_MOUNTS_MAX], count: 0, rejects: 0 }
    }

    /// 装载：路径点重复拒绝，容量封顶拒绝。
    pub fn mount(&mut self, dev: u32, root_path_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.mounts[i].root_path_id == root_path_id {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= VFS_MOUNTS_MAX {
            self.rejects += 1;
            return false;
        }
        self.mounts[self.count] = VfsMount { dev, root_path_id };
        self.count += 1;
        true
    }

    pub fn unmount(&mut self, root_path_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.mounts[i].root_path_id == root_path_id {
                let mut j = i + 1;
                while j < self.count {
                    self.mounts[j - 1] = self.mounts[j];
                    j += 1;
                }
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn owner_of(&self, root_path_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if self.mounts[i].root_path_id == root_path_id {
                return Some(self.mounts[i].dev);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F203 — 路径行走仪：分量行走，`.` 原地，`..` 上行（根钉死）
// ===========================================================================

pub const VFS_COMP_DOT: u32 = 0;
pub const VFS_COMP_DOTDOT: u32 = 1;

/// 返回行走后的深度；分量 0/1 为特殊分量，其余视作普通名。
pub fn vfs_walk(components: &[u32]) -> u32 {
    let mut depth = 0u32;
    let mut i = 0usize;
    while i < components.len() {
        let c = components[i];
        if c == VFS_COMP_DOT {
            // 原地不动
        } else if c == VFS_COMP_DOTDOT {
            if depth > 0 {
                depth -= 1;
            }
            // 根上 `..` 停在根
        } else {
            depth += 1;
        }
        i += 1;
    }
    depth
}

// ===========================================================================
// F204 — dentry 缓存谱：定容槽位，(parent, name) 键去重
// ===========================================================================

pub const VFS_DENTRIES_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsDentry {
    pub parent: u32,
    pub name: u32,
    pub inode: u32,
}

pub struct VfsDcache {
    entries: [Option<VfsDentry>; VFS_DENTRIES_MAX],
    count: usize,
    pub rejects: u32,
    pub hits: u32,
    pub misses: u32,
}

impl VfsDcache {
    pub const fn new() -> VfsDcache {
        VfsDcache { entries: [None; VFS_DENTRIES_MAX], count: 0, rejects: 0, hits: 0, misses: 0 }
    }

    /// 插入：键重复拒绝，容量封顶拒绝。
    pub fn insert(&mut self, parent: u32, name: u32, inode: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            let e = self.entries[i].unwrap_or(VfsDentry { parent: 0, name: 0, inode: 0 });
            if e.parent == parent && e.name == name {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= VFS_DENTRIES_MAX {
            self.rejects += 1;
            return false;
        }
        self.entries[self.count] = Some(VfsDentry { parent, name, inode });
        self.count += 1;
        true
    }

    pub fn lookup(&mut self, parent: u32, name: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.parent == parent && e.name == name {
                    self.hits += 1;
                    return Some(e.inode);
                }
            }
            i += 1;
        }
        self.misses += 1;
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F205 — inode 生命簿：Free → InUse → Deleted → Free 生命周期
// ===========================================================================

pub const VFS_INODES_MAX: usize = 16;
pub const VFS_INO_FREE: u8 = 0;
pub const VFS_INO_INUSE: u8 = 1;
pub const VFS_INO_DELETED: u8 = 2;

pub struct VfsInodeBook {
    states: [u8; VFS_INODES_MAX],
    refs: [u32; VFS_INODES_MAX],
}

impl VfsInodeBook {
    pub const fn new() -> VfsInodeBook {
        VfsInodeBook { states: [VFS_INO_FREE; VFS_INODES_MAX], refs: [0; VFS_INODES_MAX] }
    }

    pub fn state_of(&self, ino: u32) -> u8 {
        if ino as usize >= VFS_INODES_MAX {
            VFS_INO_FREE
        } else {
            self.states[ino as usize]
        }
    }

    pub fn refs_of(&self, ino: u32) -> u32 {
        if ino as usize >= VFS_INODES_MAX {
            0
        } else {
            self.refs[ino as usize]
        }
    }

    /// 引用：Free → InUse；InUse → 计数 +1；Deleted 拒绝。
    pub fn iget(&mut self, ino: u32) -> bool {
        if ino as usize >= VFS_INODES_MAX {
            return false;
        }
        let s = self.states[ino as usize];
        if s == VFS_INO_DELETED {
            return false;
        }
        if s == VFS_INO_FREE {
            self.states[ino as usize] = VFS_INO_INUSE;
            self.refs[ino as usize] = 1;
        } else {
            self.refs[ino as usize] += 1;
        }
        true
    }

    /// 释放：InUse → 计数 -1，归零转 Deleted。
    pub fn iput(&mut self, ino: u32) -> bool {
        if ino as usize >= VFS_INODES_MAX {
            return false;
        }
        if self.states[ino as usize] != VFS_INO_INUSE {
            return false;
        }
        self.refs[ino as usize] -= 1;
        if self.refs[ino as usize] == 0 {
            self.states[ino as usize] = VFS_INO_DELETED;
        }
        true
    }

    /// 回收：Deleted → Free。
    pub fn reclaim(&mut self, ino: u32) -> bool {
        if ino as usize >= VFS_INODES_MAX {
            return false;
        }
        if self.states[ino as usize] != VFS_INO_DELETED {
            return false;
        }
        self.states[ino as usize] = VFS_INO_FREE;
        true
    }
}

// ===========================================================================
// F206 — 文件锁谱：区间重叠 + 读写冲突裁决
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsFileLock {
    pub owner: u32,
    pub start: u64,
    pub len: u64,
    pub write: bool,
}

pub fn vfs_lock_overlap(a: &VfsFileLock, b: &VfsFileLock) -> bool {
    a.len > 0 && b.len > 0 && a.start < b.start + b.len && b.start < a.start + a.len
}

/// 冲突：重叠 + 双方持有者不同 + 任一为写锁。
pub fn vfs_lock_conflict(a: &VfsFileLock, b: &VfsFileLock) -> bool {
    a.owner != b.owner && vfs_lock_overlap(a, b) && (a.write || b.write)
}

// ===========================================================================
// F207 — 权限裁决台：Unix rwx 三班裁决
// ===========================================================================

pub const VFS_PERM_R: u8 = 4;
pub const VFS_PERM_W: u8 = 2;
pub const VFS_PERM_X: u8 = 1;

/// mode 低 9 位：owner 高 3 位、group 中 3 位、other 低 3 位。
pub fn vfs_perm_check(mode: u16, is_owner: bool, in_group: bool, want: u8) -> bool {
    let shift = if is_owner {
        6
    } else if in_group {
        3
    } else {
        0
    };
    (((mode >> shift) & 7) as u8) & want == want
}

// ===========================================================================
// F208 — 文件系统魔毯：超级块魔数 + 块大小 + 特性位
// ===========================================================================

pub const VFS_SB_MAGIC: u32 = 0x5A56_4653;
pub const VFS_FEAT_COMPRESS: u32 = 1 << 0;
pub const VFS_FEAT_JOURNAL: u32 = 1 << 1;
pub const VFS_FEAT_XATTR: u32 = 1 << 2;

#[derive(Clone, Copy, Debug)]
pub struct VfsSuper {
    pub magic: u32,
    pub features: u32,
    pub block_size: u32,
}

pub fn vfs_super_ok(s: &VfsSuper) -> bool {
    if s.magic != VFS_SB_MAGIC {
        return false;
    }
    if s.block_size < 512 || s.block_size > 65536 {
        return false;
    }
    s.block_size.is_power_of_two()
}

pub fn vfs_feat_enabled(s: &VfsSuper, feat: u32) -> bool {
    s.features & feat == feat
}

// ===========================================================================
// F209 — 脏页账本：dirty = pending + in_flight 恒等式
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct VfsDirtyFile {
    pub dirty: u32,
    pub pending: u32,
    pub in_flight: u32,
}

impl VfsDirtyFile {
    pub fn dirty_page(&mut self, n: u32) {
        self.dirty += n;
        self.pending += n;
    }

    /// 入队回写：pending → in_flight。
    pub fn queue_writeback(&mut self, n: u32) -> bool {
        if n > self.pending {
            return false;
        }
        self.pending -= n;
        self.in_flight += n;
        true
    }

    /// 回写完成：in_flight 消账，dirty 落账。
    pub fn complete_writeback(&mut self, n: u32) -> bool {
        if n > self.in_flight {
            return false;
        }
        self.in_flight -= n;
        self.dirty -= n;
        true
    }

    pub fn consistent(&self) -> bool {
        self.pending + self.in_flight == self.dirty
    }
}

// ===========================================================================
// F210 — 短读短写律：短传只允许发生在文件末尾
// ===========================================================================

pub const VFS_IO_MAX_CHUNK: u32 = 8192;

pub fn vfs_io_chunk_legal(want: u32, done: u32, at_eof: bool) -> bool {
    if done > want || done > VFS_IO_MAX_CHUNK {
        return false;
    }
    done == want || at_eof
}

// ===========================================================================
// F211 — fsync 语义谱：数据先稳、日志再提交、元数据最后
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsFsyncStage {
    DataQueued,
    DataStable,
    JournalCommit,
    MetadataStable,
}

pub fn vfs_fsync_rank(s: VfsFsyncStage) -> u8 {
    match s {
        VfsFsyncStage::DataQueued => 0,
        VfsFsyncStage::DataStable => 1,
        VfsFsyncStage::JournalCommit => 2,
        VfsFsyncStage::MetadataStable => 3,
    }
}

pub fn vfs_fsync_next(s: VfsFsyncStage) -> Option<VfsFsyncStage> {
    match s {
        VfsFsyncStage::DataQueued => Some(VfsFsyncStage::DataStable),
        VfsFsyncStage::DataStable => Some(VfsFsyncStage::JournalCommit),
        VfsFsyncStage::JournalCommit => Some(VfsFsyncStage::MetadataStable),
        VfsFsyncStage::MetadataStable => None,
    }
}

pub fn vfs_fsync_ordered(a: VfsFsyncStage, b: VfsFsyncStage) -> bool {
    vfs_fsync_rank(a) < vfs_fsync_rank(b)
}

// ===========================================================================
// F212 — 文件系统压力沙盘：容量上限 + 删除下限的剧本推演
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct VfsStressRun {
    pub created: u32,
    pub deleted: u32,
    pub failed: u32,
    pub live_cap: u32,
}

impl VfsStressRun {
    pub const fn new(live_cap: u32) -> VfsStressRun {
        VfsStressRun { created: 0, deleted: 0, failed: 0, live_cap }
    }

    pub fn live(&self) -> u32 {
        self.created - self.deleted
    }

    pub fn step(&mut self, create: bool) -> bool {
        if create {
            if self.live() >= self.live_cap {
                self.failed += 1;
                return false;
            }
            self.created += 1;
            true
        } else {
            if self.deleted >= self.created {
                self.failed += 1;
                return false;
            }
            self.deleted += 1;
            true
        }
    }
}

// ===========================================================================
// F213 — 符号链接法庭：跳数上限 + 环路侦破
// ===========================================================================

pub const VFS_SYMLINK_MAX_HOPS: u32 = 8;
pub const VFS_SYM_FINAL: u32 = 0xFFFF;
/// 目标下标上界（visited 位图宽度）。
pub const VFS_SYM_INDEX_MAX: usize = 16;

/// 从下标 0 出发沿 targets 追链；0xFFFF 为真身；环、越界、跳数超限均判负。
pub fn vfs_symlink_follow(targets: &[u32]) -> bool {
    if targets.is_empty() || targets.len() > VFS_SYM_INDEX_MAX {
        return false;
    }
    let mut visited = [false; VFS_SYM_INDEX_MAX];
    let mut cur = 0usize;
    let mut hops = 0u32;
    loop {
        if visited[cur] {
            return false;
        }
        visited[cur] = true;
        let next = targets[cur];
        if next == VFS_SYM_FINAL {
            return true;
        }
        if next as usize >= targets.len() {
            return false;
        }
        cur = next as usize;
        hops += 1;
        if hops > VFS_SYMLINK_MAX_HOPS {
            return false;
        }
    }
}

// ===========================================================================
// F214 — 稀疏文件账：逻辑尺寸 vs 实占块，洞比 permille
// ===========================================================================

pub const VFS_SPARSE_BLOCK: u64 = 4096;

/// 逻辑 nbytes 应占块数（向上取整）。
pub fn vfs_sparse_blocks_should(nbytes: u64) -> u64 {
    (nbytes + VFS_SPARSE_BLOCK - 1) / VFS_SPARSE_BLOCK
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VfsSparseFile {
    pub logical_bytes: u64,
    pub blocks_used: u32,
}

/// 洞比 = 未实占的字节空间占比（‰）；满分配为 0。
pub fn vfs_sparse_hole_permille(f: &VfsSparseFile) -> u32 {
    if f.logical_bytes == 0 {
        return 0;
    }
    let should = vfs_sparse_blocks_should(f.logical_bytes);
    let should_bytes = should * VFS_SPARSE_BLOCK;
    let used_bytes = f.blocks_used as u64 * VFS_SPARSE_BLOCK;
    if used_bytes >= should_bytes {
        return 0;
    }
    ((should_bytes - used_bytes) * 1000 / should_bytes) as u32
}

// ===========================================================================
// F215 — 文件系统血统标签：同卷判定 + 代际新旧 + 陈旧检测
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsLineage {
    pub fs_id: u64,
    pub generation: u32,
}

pub fn vfs_lineage_same_fs(a: VfsLineage, b: VfsLineage) -> bool {
    a.fs_id == b.fs_id
}

pub fn vfs_lineage_older(a: VfsLineage, b: VfsLineage) -> bool {
    a.fs_id == b.fs_id && a.generation < b.generation
}

/// 代际差 ≥ 2 的同卷标签视为陈旧（跨代残影）。
pub fn vfs_lineage_stale(a: VfsLineage, b: VfsLineage) -> bool {
    a.fs_id == b.fs_id && {
        let diff = if a.generation > b.generation {
            a.generation - b.generation
        } else {
            b.generation - a.generation
        };
        diff >= 2
    }
}

// ===========================================================================
// F216 — 挂载原子律：Idle → Mounting → {Mounted | 回滚 Idle}
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsMountPhase {
    Idle,
    Mounting,
    Mounted,
}

#[derive(Clone, Copy, Debug)]
pub struct VfsMountOps {
    pub phase: VfsMountPhase,
    pub attempts: u32,
    pub succeeded: u32,
}

impl VfsMountOps {
    pub const fn new() -> VfsMountOps {
        VfsMountOps { phase: VfsMountPhase::Idle, attempts: 0, succeeded: 0 }
    }

    pub fn begin(&mut self) -> bool {
        if !matches!(self.phase, VfsMountPhase::Idle) {
            return false;
        }
        self.phase = VfsMountPhase::Mounting;
        self.attempts += 1;
        true
    }

    pub fn commit(&mut self) -> bool {
        if !matches!(self.phase, VfsMountPhase::Mounting) {
            return false;
        }
        self.phase = VfsMountPhase::Mounted;
        self.succeeded += 1;
        true
    }

    pub fn rollback(&mut self) -> bool {
        if !matches!(self.phase, VfsMountPhase::Mounting) {
            return false;
        }
        self.phase = VfsMountPhase::Idle;
        true
    }
}

// ===========================================================================
// F217 — 路径缓存考古：代际失效，只有同代条目可信
// ===========================================================================

pub const VFS_PATHCACHE_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsPathEntry {
    pub path_id: u32,
    pub inode: u32,
    pub gen: u32,
}

pub struct VfsPathCache {
    entries: [Option<VfsPathEntry>; VFS_PATHCACHE_MAX],
    count: usize,
    pub gen: u32,
    pub updates: u32,
}

impl VfsPathCache {
    pub const fn new() -> VfsPathCache {
        VfsPathCache { entries: [None; VFS_PATHCACHE_MAX], count: 0, gen: 0, updates: 0 }
    }

    /// 放入：同 path_id 就地更新（去重），容量封顶拒绝。
    pub fn put(&mut self, path_id: u32, inode: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.path_id == path_id {
                    self.entries[i] = Some(VfsPathEntry { path_id, inode, gen: self.gen });
                    self.updates += 1;
                    return true;
                }
            }
            i += 1;
        }
        if self.count >= VFS_PATHCACHE_MAX {
            return false;
        }
        self.entries[self.count] = Some(VfsPathEntry { path_id, inode, gen: self.gen });
        self.count += 1;
        true
    }

    /// 代际推进：全部旧条目作废。
    pub fn bump(&mut self) {
        self.gen += 1;
    }

    /// 只有代际等于当前代的条目才可命中。
    pub fn get(&self, path_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(e) = self.entries[i] {
                if e.path_id == path_id && e.gen == self.gen {
                    return Some(e.inode);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F218 — 文件句柄宗卷：最低空闲 fd 分配 + 开放数高水位
// ===========================================================================

pub const VFS_FD_MAX: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct VfsFdTable {
    used: [bool; VFS_FD_MAX],
    open_count: usize,
    pub high_water: usize,
}

impl VfsFdTable {
    pub const fn new() -> VfsFdTable {
        VfsFdTable { used: [false; VFS_FD_MAX], open_count: 0, high_water: 0 }
    }

    pub fn alloc_fd(&mut self) -> Option<usize> {
        let mut fd = 0usize;
        while fd < VFS_FD_MAX {
            if !self.used[fd] {
                self.used[fd] = true;
                self.open_count += 1;
                if self.open_count > self.high_water {
                    self.high_water = self.open_count;
                }
                return Some(fd);
            }
            fd += 1;
        }
        None
    }

    pub fn close_fd(&mut self, fd: usize) -> bool {
        if fd >= VFS_FD_MAX || !self.used[fd] {
            return false;
        }
        self.used[fd] = false;
        self.open_count -= 1;
        true
    }

    pub fn is_open(&self, fd: usize) -> bool {
        fd < VFS_FD_MAX && self.used[fd]
    }

    pub fn open_count(&self) -> usize {
        self.open_count
    }
}

// ===========================================================================
// F219 — 元数据日志带：事务槽位、提交标记、恢复只重放已提交
// ===========================================================================

pub const VFS_JOURNAL_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsJournalTx {
    pub txid: u32,
    pub committed: bool,
}

pub struct VfsJournal {
    txs: [Option<VfsJournalTx>; VFS_JOURNAL_SLOTS],
    count: usize,
    next_txid: u32,
}

impl VfsJournal {
    pub const fn new() -> VfsJournal {
        VfsJournal { txs: [None; VFS_JOURNAL_SLOTS], count: 0, next_txid: 1 }
    }

    /// 开事务：返回 txid；带满返回 0。
    pub fn begin_tx(&mut self) -> u32 {
        if self.count >= VFS_JOURNAL_SLOTS {
            return 0;
        }
        let txid = self.next_txid;
        self.txs[self.count] = Some(VfsJournalTx { txid, committed: false });
        self.count += 1;
        self.next_txid += 1;
        txid
    }

    /// 提交：只有未提交的同号事务可提交一次。
    pub fn commit_tx(&mut self, txid: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if let Some(tx) = self.txs[i] {
                if tx.txid == txid {
                    if tx.committed {
                        return false;
                    }
                    self.txs[i] = Some(VfsJournalTx { txid, committed: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 恢复重放：只数已提交事务。
    pub fn recovery_replay_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if let Some(tx) = self.txs[i] {
                if tx.committed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F220 — VFS 回归金样：固定剧本 → 固定末态
// ===========================================================================

/// 金样剧本：连开 fd 0/1/2，再关掉 1。
pub fn vfs_golden_scenario() -> VfsFdTable {
    let mut t = VfsFdTable::new();
    t.alloc_fd();
    t.alloc_fd();
    t.alloc_fd();
    t.close_fd(1);
    t
}

/// 期望末态：0/2 开、1 空、高水位 3、当前开放 2。
pub fn vfs_golden_matches(t: &VfsFdTable) -> bool {
    t.is_open(0) && !t.is_open(1) && t.is_open(2) && t.high_water == 3 && t.open_count() == 2
}

// ===========================================================================
// F221 — 大目录分片谱：名字散列分片 + 分片均衡 permille
// ===========================================================================

pub const VFS_DIR_SHARDS: usize = 16;

pub fn vfs_dir_shard(name: u32) -> usize {
    ((name.wrapping_mul(0x9E37_79B9) >> 28) as usize) % VFS_DIR_SHARDS
}

/// 均衡度：((max-min) * 1000 / total)‰；空目录为 0。
pub fn vfs_shard_balance(counts: &[u32; VFS_DIR_SHARDS]) -> u32 {
    let mut total = 0u64;
    let mut min = u32::MAX;
    let mut max = 0u32;
    let mut i = 0usize;
    while i < counts.len() {
        total += counts[i] as u64;
        if counts[i] < min {
            min = counts[i];
        }
        if counts[i] > max {
            max = counts[i];
        }
        i += 1;
    }
    if total == 0 {
        return 0;
    }
    ((max - min) as u64 * 1000 / total) as u32
}

// ===========================================================================
// F222 — 文件事件流：定容事件队列，满则丢弃并记账
// ===========================================================================

pub const VFS_EVENT_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsEventKind {
    Create,
    Write,
    Close,
    Unlink,
}

pub struct VfsEventRing {
    kinds: [Option<VfsEventKind>; VFS_EVENT_RING],
    seqs: [u64; VFS_EVENT_RING],
    head: usize,
    count: usize,
    pub dropped: u64,
}

impl VfsEventRing {
    pub const fn new() -> VfsEventRing {
        VfsEventRing { kinds: [None; VFS_EVENT_RING], seqs: [0; VFS_EVENT_RING], head: 0, count: 0, dropped: 0 }
    }

    pub fn push(&mut self, seq: u64, kind: VfsEventKind) -> bool {
        if self.count >= VFS_EVENT_RING {
            self.dropped += 1;
            return false;
        }
        let idx = (self.head + self.count) % VFS_EVENT_RING;
        self.kinds[idx] = Some(kind);
        self.seqs[idx] = seq;
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<(u64, VfsEventKind)> {
        if self.count == 0 {
            return None;
        }
        let idx = self.head;
        let item = Some((self.seqs[idx], self.kinds[idx].unwrap_or(VfsEventKind::Create)));
        self.kinds[idx] = None;
        self.head = (self.head + 1) % VFS_EVENT_RING;
        self.count -= 1;
        item
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F223 — 配额执法官：软限警告、硬限拒绝
// ===========================================================================

pub const VFS_QUOTA_ALLOW: u8 = 0;
pub const VFS_QUOTA_WARN: u8 = 1;
pub const VFS_QUOTA_DENY: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub struct VfsQuota {
    pub soft_bytes: u64,
    pub hard_bytes: u64,
    pub used: u64,
}

impl VfsQuota {
    pub const fn new(soft_bytes: u64, hard_bytes: u64) -> VfsQuota {
        VfsQuota { soft_bytes, hard_bytes, used: 0 }
    }

    pub fn verdict(&self, want: u64) -> u8 {
        if self.used + want <= self.soft_bytes {
            VFS_QUOTA_ALLOW
        } else if self.used + want <= self.hard_bytes {
            VFS_QUOTA_WARN
        } else {
            VFS_QUOTA_DENY
        }
    }

    /// 入账：硬限内才放行。
    pub fn charge(&mut self, want: u64) -> bool {
        if self.used + want > self.hard_bytes {
            return false;
        }
        self.used += want;
        true
    }
}

// ===========================================================================
// F224 — VFS 自描述清单：inode/块收支平衡 + 特性位封闭
// ===========================================================================

pub const VFS_FEAT_KNOWN: u32 = VFS_FEAT_COMPRESS | VFS_FEAT_JOURNAL | VFS_FEAT_XATTR;

#[derive(Clone, Copy, Debug, Default)]
pub struct VfsSelfDesc {
    pub total_inodes: u32,
    pub used_inodes: u32,
    pub free_inodes: u32,
    pub total_blocks: u32,
    pub used_blocks: u32,
    pub free_blocks: u32,
    pub declared_features: u32,
}

pub fn vfs_selfdesc_ok(d: &VfsSelfDesc) -> bool {
    if d.used_inodes + d.free_inodes != d.total_inodes {
        return false;
    }
    if d.used_blocks + d.free_blocks != d.total_blocks {
        return false;
    }
    d.declared_features & !VFS_FEAT_KNOWN == 0
}

// ===========================================================================
// F225 — VFS 域年报：年报章节完备性
// ===========================================================================

pub const VFS_REPORT_SECTIONS: [&str; 5] = ["mounts", "dentry", "inodes", "journal", "quota"];

pub fn vfs_report_complete(filled: u32) -> bool {
    filled >= VFS_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700vfs_checks() -> CheckSet {
    let mut set = CheckSet::new("m700vfs");

    // F201 VFS 操作大典
    set.add(
        "F201 op validity",
        vfs_op_valid(VFS_OP_OPEN) && vfs_op_valid(VFS_OP_UNLINK) && !vfs_op_valid(0) && !vfs_op_valid(7),
        "codes 1..=6",
    );
    set.add(
        "F201 op classes",
        vfs_op_mutates(VFS_OP_WRITE) && vfs_op_mutates(VFS_OP_UNLINK)
            && !vfs_op_mutates(VFS_OP_READ) && !vfs_op_mutates(VFS_OP_STAT),
        "write/unlink mutate",
    );

    // F202 装载点族谱
    let mut mt = VfsMountTable::new();
    let m1 = mt.mount(1, 10);
    let m_dup = mt.mount(2, 10);
    let owner_first = mt.owner_of(10);
    let un = mt.unmount(10);
    let owner_gone = mt.owner_of(10);
    let m_re = mt.mount(2, 10);
    set.add(
        "F202 mount dedup+cycle",
        m1 && !m_dup && owner_first == Some(1) && un && owner_gone.is_none() && m_re,
        "path pinned once",
    );
    let owner_after = mt.owner_of(10);
    set.add("F202 remount owner", mt.len() == 1 && owner_after == Some(2), "new dev owns path");

    // F203 路径行走仪
    set.add(
        "F203 walk plain",
        vfs_walk(&[2, 3, 4]) == 3 && vfs_walk(&[]) == 0,
        "depth counts names",
    );
    set.add(
        "F203 walk dot dotdot",
        vfs_walk(&[2, VFS_COMP_DOT, 3]) == 2
            && vfs_walk(&[2, VFS_COMP_DOTDOT, 3]) == 1
            && vfs_walk(&[VFS_COMP_DOTDOT, VFS_COMP_DOTDOT]) == 0,
        "root pinned",
    );

    // F204 dentry 缓存谱
    let mut dc = VfsDcache::new();
    let d1 = dc.insert(1, 2, 100);
    let d_dup = dc.insert(1, 2, 200);
    let hit = dc.lookup(1, 2);
    let miss = dc.lookup(1, 9);
    set.add(
        "F204 dentry insert+lookup",
        d1 && !d_dup && hit == Some(100) && miss.is_none() && dc.hits == 1 && dc.misses == 1,
        "key dedup",
    );
    let mut key = 3u32;
    while key < 10 {
        dc.insert(1, key, key * 10);
        key += 1;
    }
    let d_cap = dc.insert(1, 99, 990);
    set.add(
        "F204 dentry capacity",
        dc.len() == 8 && !d_cap && dc.rejects == 2,
        "8 slots full",
    );

    // F205 inode 生命簿
    let mut book = VfsInodeBook::new();
    let g1 = book.iget(5);
    let refs1 = book.refs_of(5);
    let g2 = book.iget(5);
    let refs2 = book.refs_of(5);
    set.add(
        "F205 iget counts",
        g1 && refs1 == 1 && g2 && refs2 == 2 && book.state_of(5) == VFS_INO_INUSE,
        "refcounted",
    );
    let p1 = book.iput(5);
    let p2 = book.iput(5);
    let state_dead = book.state_of(5);
    let g_dead = book.iget(5);
    let rc = book.reclaim(5);
    let state_free = book.state_of(5);
    let p_free = book.iput(5);
    set.add(
        "F205 lifecycle law",
        p1 && p2 && state_dead == VFS_INO_DELETED && !g_dead && rc && state_free == VFS_INO_FREE && !p_free,
        "free->inuse->deleted->free",
    );

    // F206 文件锁谱
    let l_read_a = VfsFileLock { owner: 1, start: 0, len: 4, write: false };
    let l_read_b = VfsFileLock { owner: 2, start: 0, len: 4, write: false };
    let l_write_c = VfsFileLock { owner: 2, start: 2, len: 4, write: true };
    let l_adjacent = VfsFileLock { owner: 3, start: 4, len: 4, write: true };
    set.add(
        "F206 lock overlap",
        vfs_lock_overlap(&l_read_a, &l_write_c) && !vfs_lock_overlap(&l_read_a, &l_adjacent),
        "adjacent is disjoint",
    );
    set.add(
        "F206 lock conflict",
        !vfs_lock_conflict(&l_read_a, &l_read_b) && vfs_lock_conflict(&l_read_a, &l_write_c),
        "read-read fine, write bites",
    );

    // F207 权限裁决台
    set.add(
        "F207 owner class",
        vfs_perm_check(0o640, true, false, VFS_PERM_R | VFS_PERM_W)
            && !vfs_perm_check(0o640, true, false, VFS_PERM_X)
            && !vfs_perm_check(0o060, true, false, VFS_PERM_W),
        "owner bits rule",
    );
    set.add(
        "F207 group/other class",
        vfs_perm_check(0o640, false, true, VFS_PERM_R)
            && !vfs_perm_check(0o640, false, true, VFS_PERM_W)
            && !vfs_perm_check(0o640, false, false, VFS_PERM_R),
        "group then other",
    );

    // F208 文件系统魔毯
    let good_sb = VfsSuper { magic: VFS_SB_MAGIC, features: VFS_FEAT_JOURNAL, block_size: 4096 };
    let bad_magic = VfsSuper { magic: 0x1234, features: 0, block_size: 4096 };
    let bad_bs = VfsSuper { magic: VFS_SB_MAGIC, features: 0, block_size: 300 };
    set.add(
        "F208 super validity",
        vfs_super_ok(&good_sb) && !vfs_super_ok(&bad_magic) && !vfs_super_ok(&bad_bs),
        "magic + pow2 block size",
    );
    set.add(
        "F208 super features",
        vfs_feat_enabled(&good_sb, VFS_FEAT_JOURNAL) && !vfs_feat_enabled(&good_sb, VFS_FEAT_COMPRESS),
        "feature bits",
    );

    // F209 脏页账本
    let mut df = VfsDirtyFile::default();
    df.dirty_page(5);
    let q1 = df.queue_writeback(2);
    let pending_after_q = df.pending;
    let in_flight_after_q = df.in_flight;
    set.add(
        "F209 queue writeback",
        q1 && pending_after_q == 3 && in_flight_after_q == 2 && df.consistent() && df.dirty == 5,
        "pending -> in_flight",
    );
    let c1 = df.complete_writeback(2);
    let overdraft = df.complete_writeback(9);
    set.add(
        "F209 complete writeback",
        c1 && df.dirty == 3 && df.in_flight == 0 && !overdraft && df.consistent(),
        "no overdraft",
    );

    // F210 短读短写律
    set.add(
        "F210 full io",
        vfs_io_chunk_legal(100, 100, false) && vfs_io_chunk_legal(100, 40, true),
        "full or eof-short",
    );
    set.add(
        "F210 illegal shorts",
        !vfs_io_chunk_legal(100, 40, false) && !vfs_io_chunk_legal(100, 101, false)
            && !vfs_io_chunk_legal(9000, 9000, false),
        "mid-io short and oversize rejected",
    );

    // F211 fsync 语义谱
    set.add(
        "F211 fsync chain",
        vfs_fsync_next(VfsFsyncStage::DataQueued) == Some(VfsFsyncStage::DataStable)
            && vfs_fsync_next(VfsFsyncStage::DataStable) == Some(VfsFsyncStage::JournalCommit)
            && vfs_fsync_next(VfsFsyncStage::MetadataStable).is_none(),
        "linear stages",
    );
    set.add(
        "F211 fsync order",
        vfs_fsync_ordered(VfsFsyncStage::DataStable, VfsFsyncStage::JournalCommit)
            && !vfs_fsync_ordered(VfsFsyncStage::MetadataStable, VfsFsyncStage::DataQueued),
        "data before commit",
    );

    // F212 文件系统压力沙盘
    let mut run = VfsStressRun::new(4);
    let mut step = 0;
    while step < 5 {
        run.step(true);
        step += 1;
    }
    let live_after_creates = run.live();
    let failed_creates = run.failed;
    run.step(false);
    run.step(false);
    run.step(false);
    run.step(false);
    let extra_delete_ok = run.step(false);
    set.add(
        "F212 stress cap",
        live_after_creates == 4 && failed_creates == 1 && !extra_delete_ok && run.failed == 2 && run.live() == 0,
        "cap and floor enforced",
    );
    let mut fresh = VfsStressRun::new(2);
    let d_empty = fresh.step(false);
    set.add("F212 stress delete empty", !d_empty && fresh.failed == 1, "nothing to delete");

    // F213 符号链接法庭
    set.add(
        "F213 symlink resolve",
        vfs_symlink_follow(&[1, VFS_SYM_FINAL]) && vfs_symlink_follow(&[1, 2, VFS_SYM_FINAL]),
        "chains reach final",
    );
    set.add(
        "F213 symlink reject",
        !vfs_symlink_follow(&[1, 0]) && !vfs_symlink_follow(&[9]) && !vfs_symlink_follow(&[]),
        "loop / dangling / empty",
    );

    // F214 稀疏文件账
    set.add(
        "F214 sparse blocks",
        vfs_sparse_blocks_should(8192) == 2 && vfs_sparse_blocks_should(4097) == 2 && vfs_sparse_blocks_should(0) == 0,
        "ceil blocks",
    );
    let half = VfsSparseFile { logical_bytes: 8192, blocks_used: 1 };
    let full = VfsSparseFile { logical_bytes: 8192, blocks_used: 2 };
    let empty = VfsSparseFile { logical_bytes: 0, blocks_used: 0 };
    set.add(
        "F214 hole permille",
        vfs_sparse_hole_permille(&half) == 500 && vfs_sparse_hole_permille(&full) == 0
            && vfs_sparse_hole_permille(&empty) == 0,
        "half holed = 500",
    );

    // F215 文件系统血统标签
    let la = VfsLineage { fs_id: 1, generation: 3 };
    let lb = VfsLineage { fs_id: 1, generation: 5 };
    let lc = VfsLineage { fs_id: 1, generation: 4 };
    let ld = VfsLineage { fs_id: 2, generation: 9 };
    set.add(
        "F215 lineage relations",
        vfs_lineage_same_fs(la, lb) && vfs_lineage_older(la, lb) && !vfs_lineage_older(lb, la)
            && !vfs_lineage_same_fs(la, ld),
        "same fs + ordering",
    );
    set.add(
        "F215 lineage staleness",
        vfs_lineage_stale(la, lb) && !vfs_lineage_stale(la, lc),
        "gen diff >= 2 stale",
    );

    // F216 挂载原子律
    let mut mo = VfsMountOps::new();
    let c_early = mo.commit();
    let b1 = mo.begin();
    let rolled = mo.rollback();
    let phase_after_rollback = mo.phase;
    set.add(
        "F216 atomic rollback",
        !c_early && b1 && rolled && phase_after_rollback == VfsMountPhase::Idle && mo.attempts == 1,
        "no partial phase escape",
    );
    let b2 = mo.begin();
    let committed = mo.commit();
    let c_again = mo.commit();
    set.add(
        "F216 atomic commit",
        b2 && committed && !c_again && mo.phase == VfsMountPhase::Mounted && mo.succeeded == 1,
        "commit once",
    );

    // F217 路径缓存考古
    let mut pc = VfsPathCache::new();
    let p1 = pc.put(1, 100);
    let p_dup = pc.put(1, 100);
    let hit0 = pc.get(1);
    pc.bump();
    let hit_stale = pc.get(1);
    let p3 = pc.put(1, 101);
    let hit1 = pc.get(1);
    set.add(
        "F217 pathcache gens",
        p1 && p_dup && hit0 == Some(100) && hit_stale.is_none() && p3 && hit1 == Some(101)
            && pc.len() == 1 && pc.updates == 2,
        "in-place dedup + gen invalidation",
    );

    // F218 文件句柄宗卷
    let mut fd = VfsFdTable::new();
    let a1 = fd.alloc_fd();
    let a2 = fd.alloc_fd();
    let a3 = fd.alloc_fd();
    let open_peak = fd.open_count();
    let closed = fd.close_fd(1);
    let reuse = fd.alloc_fd();
    set.add(
        "F218 fd lowest free",
        a1 == Some(0) && a2 == Some(1) && a3 == Some(2) && open_peak == 3 && closed && reuse == Some(1),
        "reuse freed slot",
    );
    let never_open = fd.close_fd(5);
    let high_water_seen = fd.high_water;
    set.add(
        "F218 fd watermark",
        !never_open && high_water_seen == 3 && fd.open_count() == 3,
        "high water sticks",
    );

    // F219 元数据日志带
    let mut jr = VfsJournal::new();
    let t1 = jr.begin_tx();
    let t2 = jr.begin_tx();
    let t3 = jr.begin_tx();
    let cm1 = jr.commit_tx(t1);
    let cm3 = jr.commit_tx(t3);
    let replay = jr.recovery_replay_count();
    set.add(
        "F219 journal commit",
        t1 == 1 && t2 == 2 && t3 == 3 && cm1 && cm3 && replay == 2,
        "only committed replay",
    );
    let cm3_dup = jr.commit_tx(t3);
    let cm_ghost = jr.commit_tx(99);
    set.add(
        "F219 journal rejects",
        !cm3_dup && !cm_ghost && jr.len() == 3,
        "no double commit, no ghost tx",
    );

    // F220 VFS 回归金样
    let golden = vfs_golden_scenario();
    set.add("F220 golden matches", vfs_golden_matches(&golden), "scripted end state");
    let mut drifted = vfs_golden_scenario();
    drifted.alloc_fd();
    set.add("F220 golden catches drift", !vfs_golden_matches(&drifted), "extra fd detected");

    // F221 大目录分片谱
    let mut single = [0u32; VFS_DIR_SHARDS];
    single[3] = 16;
    let mut even = [0u32; VFS_DIR_SHARDS];
    let mut i = 0usize;
    while i < VFS_DIR_SHARDS {
        even[i] = 2;
        i += 1;
    }
    set.add(
        "F221 shard balance",
        vfs_shard_balance(&single) == 1000 && vfs_shard_balance(&even) == 0,
        "spread permille",
    );
    let s0 = vfs_dir_shard(0);
    let s1 = vfs_dir_shard(7);
    let mut in_range = true;
    let mut name = 0u32;
    while name < 64 {
        if vfs_dir_shard(name) >= VFS_DIR_SHARDS {
            in_range = false;
        }
        name += 1;
    }
    set.add(
        "F221 shard mapping",
        in_range && s0 == 0 && s1 == 5,
        "golden multiply-shift hash",
    );

    // F222 文件事件流
    let mut ev = VfsEventRing::new();
    let mut seq = 1u64;
    let mut pushed_ok = 0u32;
    while seq <= 9 {
        if ev.push(seq, VfsEventKind::Write) {
            pushed_ok += 1;
        }
        seq += 1;
    }
    let dropped_first = ev.dropped;
    let first_pop = ev.pop();
    let second_pop = ev.pop();
    let refill = ev.push(10, VfsEventKind::Create);
    set.add(
        "F222 event queue bound",
        pushed_ok == 8 && dropped_first == 1 && first_pop == Some((1, VfsEventKind::Write))
            && second_pop == Some((2, VfsEventKind::Write)) && refill,
        "8 slots, drops counted, fifo",
    );
    let drained_len = ev.len();
    set.add("F222 event drain", drained_len == 7, "queue accounting");

    // F223 配额执法官
    let mut q = VfsQuota::new(100, 200);
    let v1 = q.verdict(50);
    let c1 = q.charge(50);
    let used_soft = q.used;
    let v2 = q.verdict(60);
    let c2 = q.charge(60);
    let v3 = q.verdict(150);
    let c3 = q.charge(150);
    set.add(
        "F223 quota verdicts",
        v1 == VFS_QUOTA_ALLOW && c1 && used_soft == 50
            && v2 == VFS_QUOTA_WARN && c2 && q.used == 110
            && v3 == VFS_QUOTA_DENY && !c3 && q.used == 110,
        "allow/warn/deny ladder",
    );

    // F224 VFS 自描述清单
    let good_desc = VfsSelfDesc {
        total_inodes: 100,
        used_inodes: 60,
        free_inodes: 40,
        total_blocks: 1000,
        used_blocks: 700,
        free_blocks: 300,
        declared_features: VFS_FEAT_JOURNAL | VFS_FEAT_XATTR,
    };
    let mut bad_sum = good_desc;
    bad_sum.free_inodes = 39;
    let mut bad_feat = good_desc;
    bad_feat.declared_features |= 1 << 7;
    set.add(
        "F224 selfdesc ok",
        vfs_selfdesc_ok(&good_desc) && !vfs_selfdesc_ok(&bad_sum) && !vfs_selfdesc_ok(&bad_feat),
        "balance + known features",
    );

    // F225 VFS 域年报
    set.add(
        "F225 vfs report",
        VFS_REPORT_SECTIONS.len() == 5 && vfs_report_complete(5) && !vfs_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f202_mount_lifecycle() {
        let mut mt = VfsMountTable::new();
        assert!(mt.mount(7, 1));
        assert!(!mt.mount(8, 1));
        assert_eq!(mt.owner_of(1), Some(7));
        assert!(mt.unmount(1));
        assert!(!mt.unmount(1));
        assert!(mt.owner_of(1).is_none());
    }

    #[test]
    fn f205_inode_refcount() {
        let mut b = VfsInodeBook::new();
        assert!(b.iget(0));
        assert!(b.iget(0));
        assert!(b.iget(0));
        assert_eq!(b.refs_of(0), 3);
        assert!(b.iput(0));
        assert!(b.iput(0));
        assert!(b.iput(0));
        assert_eq!(b.state_of(0), VFS_INO_DELETED);
        assert!(b.reclaim(0));
        assert_eq!(b.state_of(0), VFS_INO_FREE);
    }

    #[test]
    fn f207_perm_boundaries() {
        // 0o640: owner rw-, group r--, other ---
        assert!(vfs_perm_check(0o640, true, true, VFS_PERM_R | VFS_PERM_W));
        assert!(vfs_perm_check(0o640, false, true, VFS_PERM_R));
        assert!(!vfs_perm_check(0o640, false, true, VFS_PERM_R | VFS_PERM_X));
        assert!(!vfs_perm_check(0o640, false, false, VFS_PERM_R));
    }

    #[test]
    fn f213_symlink_hops() {
        // 恰好 8 跳（上限内）：0->1->...->8->FINAL
        let mut long_ok = [0u32; 9];
        let mut i = 0usize;
        while i < 8 {
            long_ok[i] = (i + 1) as u32;
            i += 1;
        }
        long_ok[8] = VFS_SYM_FINAL;
        assert!(vfs_symlink_follow(&long_ok));
        // 11 跳超限
        let mut too_long = [0u32; 12];
        let mut j = 0usize;
        while j < 11 {
            too_long[j] = (j + 1) as u32;
            j += 1;
        }
        too_long[11] = VFS_SYM_FINAL;
        assert!(!vfs_symlink_follow(&too_long));
        // 自环：0 -> 1 -> 0
        assert!(!vfs_symlink_follow(&[1, 0]));
    }

    #[test]
    fn f217_pathcache_invalidation() {
        let mut pc = VfsPathCache::new();
        pc.put(4, 400);
        pc.bump();
        pc.bump();
        assert!(pc.get(4).is_none());
        pc.put(4, 404);
        assert_eq!(pc.get(4), Some(404));
        assert_eq!(pc.len(), 1);
    }

    #[test]
    fn f223_quota_ladder() {
        let mut q = VfsQuota::new(10, 20);
        assert_eq!(q.verdict(10), VFS_QUOTA_ALLOW);
        assert!(q.charge(10));
        assert_eq!(q.verdict(10), VFS_QUOTA_WARN);
        assert!(q.charge(10));
        assert_eq!(q.verdict(1), VFS_QUOTA_DENY);
        assert!(!q.charge(1));
    }

    #[test]
    fn vfs_selfcheck_all_pass() {
        let set = run_m700vfs_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
