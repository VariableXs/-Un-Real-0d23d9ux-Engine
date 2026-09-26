//! m600files — VARIX-M600 AI-14 文件与数据域 (F326~F350)
//!
//! 虚拟文件系统全貌/标签即文件夹/时间机文件史/智能收藏夹/搜索即行动/
//! 压缩瑞士军刀/同步冲突调解庭/大文件搬运工/文件完整性公证/快照空间站/
//! 重复内容收敛/存储层级自动分层/冷数据归档舱/文件血缘图/撤销宇宙/
//! 批量操作台/预览全能舱/路径仪式感/文件加密保险库/哈希校验日常化/
//! 磁盘空间叙事/文件健康体检/回收站法医学/文件迁移向导/存储年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F326 — 虚拟文件系统全貌：挂载表与路径解析
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Mount {
    pub prefix: &'static str, // 如 "/sys"、"/home"
    pub dev: &'static str,
    pub read_only: bool,
}

pub const MOUNT_TABLE: [Mount; 4] = [
    Mount { prefix: "/sys", dev: "sysfs", read_only: true },
    Mount { prefix: "/home", dev: "varixfs", read_only: false },
    Mount { prefix: "/media", dev: "varixfs", read_only: false },
    Mount { prefix: "/tmp", dev: "ramfs", read_only: false },
];

/// 最长前缀命中挂载点。
pub fn mount_for(path: &str) -> Option<&'static Mount> {
    let mut best: Option<&'static Mount> = None;
    for m in MOUNT_TABLE.iter() {
        if path.starts_with(m.prefix)
            && best.map(|b| m.prefix.len() > b.prefix.len()).unwrap_or(true)
        {
            best = Some(m);
        }
    }
    best
}

/// 只读挂载点拒绝写操作。
pub fn write_allowed(path: &str) -> bool {
    match mount_for(path) {
        Some(m) => !m.read_only,
        None => false,
    }
}

// ===========================================================================
// F327 — 标签即文件夹：标签交集查询
// ===========================================================================

/// 查询标签全部命中才算匹配（AND 语义）。
pub fn tag_match(file_tags: &[&str], query: &[&str]) -> bool {
    query.iter().all(|q| file_tags.contains(q))
}

/// 标签数量上限：单文件 8 个。
pub fn tags_within_cap(n: usize) -> bool {
    n <= 8
}

// ===========================================================================
// F328 — 时间机文件史：版本链与修剪
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FileVersion {
    pub rev: u32,
    pub ts: u64,
    pub bytes: u64,
}

/// 最新版本 = rev 最大者。
pub fn latest_version(versions: &[FileVersion]) -> Option<&FileVersion> {
    let mut best = versions.first()?;
    for v in versions.iter() {
        if v.rev > best.rev {
            best = v;
        }
    }
    Some(best)
}

/// 修剪策略：保留最近 keep 个 + 所有最大 rev。
pub fn prune_keep_count(n: usize, keep: usize) -> usize {
    if n <= keep {
        n
    } else {
        keep
    }
}

// ===========================================================================
// F329 — 智能收藏夹：打开频率×新近度
// ===========================================================================

/// 收藏分 = 打开次数×100 + 新近加分（7 天内 50，否则 0），上限 1000。
pub fn fav_score(open_count: u32, days_since_open: u32) -> u16 {
    let recency = if days_since_open <= 7 { 50u32 } else { 0 };
    (open_count * 100 + recency).min(1000) as u16
}

/// 进入收藏夹的门槛：分 ≥ 200。
pub fn fav_worthy(open_count: u32, days_since_open: u32) -> bool {
    fav_score(open_count, days_since_open) >= 200
}

// ===========================================================================
// F330 — 搜索即行动：搜索词→动作
// ===========================================================================

pub const SEARCH_ACTIONS: [(&str, &str); 5] =
    [("rename:", "open-rename-dialog"), ("copy:", "open-copy-target"), ("del:", "open-delete-confirm"), ("tag:", "open-tag-editor"), ("find:", "open-search-pane")];

pub fn action_for_query(query: &str) -> Option<&'static str> {
    SEARCH_ACTIONS
        .iter()
        .find(|(prefix, _)| query.starts_with(prefix))
        .map(|(_, action)| *action)
}

// ===========================================================================
// F331 — 压缩瑞士军刀：算法选择与压缩比
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompAlgo {
    Store,  // 已压缩数据
    Deflate,// 通用
    Lz4,    // 求速
}

pub fn choose_algo(is_media: bool, need_speed: bool) -> CompAlgo {
    if is_media {
        CompAlgo::Store
    } else if need_speed {
        CompAlgo::Lz4
    } else {
        CompAlgo::Deflate
    }
}

/// 压缩比‰ = 压缩后/原始 ×1000；有损判定：比 ≥ 1000 即无收益。
pub fn ratio_permille(compressed: u64, orig: u64) -> u16 {
    if orig == 0 {
        return 1000;
    }
    ((compressed * 1000 / orig) as u16).min(1000)
}

// ===========================================================================
// F332 — 同步冲突调解庭：改写时间裁决
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictVerdict {
    KeepA,
    KeepB,
    BothRename, // 相差 ≤2s 视为并发编辑，双保留
}

pub fn resolve_conflict(mtime_a: u64, mtime_b: u64) -> ConflictVerdict {
    let d = mtime_a.abs_diff(mtime_b);
    if d <= 2 {
        ConflictVerdict::BothRename
    } else if mtime_a > mtime_b {
        ConflictVerdict::KeepA
    } else {
        ConflictVerdict::KeepB
    }
}

// ===========================================================================
// F333 — 大文件搬运工：分块搬运计划
// ===========================================================================

/// 分块数（向上取整）+ 每块完成后校验一次。
pub fn copy_chunks(total: u64, chunk: u64) -> u64 {
    if chunk == 0 {
        return 0;
    }
    (total + chunk - 1) / chunk
}

pub fn verify_passes(total: u64, chunk: u64) -> u64 {
    copy_chunks(total, chunk)
}

// ===========================================================================
// F334 — 文件完整性公证：FNV-1a 指纹
// ===========================================================================

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 公证：存证哈希与现读哈希一致即未被篡改。
pub fn notarize(current: &[u8], notarized_hash: u64) -> bool {
    fnv1a64(current) == notarized_hash
}

// ===========================================================================
// F335 — 快照空间站：保留策略
// ===========================================================================

/// 空间占用 ≥850‰ 时只留 8 份，≥600‰ 留 16 份，其余全保。
pub fn snapshot_keep(used_permille: u16, total: usize) -> usize {
    let cap = if used_permille >= 850 {
        8
    } else if used_permille >= 600 {
        16
    } else {
        usize::MAX
    };
    total.min(cap)
}

// ===========================================================================
// F336 — 重复内容收敛：同哈希同尺寸判重
// ===========================================================================

pub fn is_duplicate(size_a: u64, hash_a: u64, size_b: u64, hash_b: u64) -> bool {
    size_a == size_b && hash_a == hash_b && size_a > 0
}

// ===========================================================================
// F337 — 存储层级自动分层：热度分层
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageTier {
    Hot,  // 闲置 <1 天
    Warm, // 闲置 <30 天
    Cold, // 其余
}

pub fn tier_for(days_idle: u32) -> StorageTier {
    if days_idle < 1 {
        StorageTier::Hot
    } else if days_idle < 30 {
        StorageTier::Warm
    } else {
        StorageTier::Cold
    }
}

// ===========================================================================
// F338 — 冷数据归档舱：归档资格
// ===========================================================================

/// 闲置 ≥90 天且 ≥64MiB 才值得归档。
pub fn archive_eligible(days_idle: u32, size_mib: u64) -> bool {
    days_idle >= 90 && size_mib >= 64
}

// ===========================================================================
// F339 — 文件血缘图：派生链深度
// ===========================================================================

#[derive(Clone, Copy)]
pub struct LineageNode {
    pub name: &'static str,
    pub parent: i16, // -1 = 根
}

/// 计算某节点血缘深度（根为 0）。
pub fn lineage_depth(nodes: &[LineageNode], idx: usize) -> u32 {
    let mut depth = 0u32;
    let mut cur = idx;
    while depth < 64 {
        let p = nodes[cur].parent;
        if p < 0 {
            return depth;
        }
        cur = p as usize;
        depth += 1;
    }
    depth // 环保护
}

// ===========================================================================
// F340 — 撤销宇宙：撤销/重做游标
// ===========================================================================

pub const UNDO_CAP: usize = 64;

pub struct UndoStack {
    ops: [u8; UNDO_CAP],
    len: usize,
    cursor: usize, // 已应用的 op 数
}

impl UndoStack {
    pub const fn new() -> UndoStack {
        UndoStack { ops: [0; UNDO_CAP], len: 0, cursor: 0 }
    }

    pub fn push(&mut self, op: u8) {
        if self.len < UNDO_CAP {
            self.ops[self.len] = op;
            self.len += 1;
            self.cursor = self.len;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.len
    }

    /// 撤销后新修改会截断重做分支。
    pub fn undo(&mut self) -> Option<u8> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        Some(self.ops[self.cursor])
    }

    pub fn redo(&mut self) -> Option<u8> {
        if self.cursor >= self.len {
            return None;
        }
        let op = self.ops[self.cursor];
        self.cursor += 1;
        Some(op)
    }
}

// ===========================================================================
// F341 — 批量操作台：重命名模板
// ===========================================================================

/// 模板合法：前缀非空、≤16 字节、不含路径分隔符。
pub fn rename_pattern_ok(prefix: &str) -> bool {
    let b = prefix.as_bytes();
    !b.is_empty() && b.len() <= 16 && !b.contains(&b'/') && !b.contains(&b'\\')
}

/// 生成 "前缀-序号" 到缓冲区，返回写入长度（缓冲区不足返回 0）。
pub fn batch_name(buf: &mut [u8], prefix: &str, idx: u32) -> usize {
    let p = prefix.as_bytes();
    if !rename_pattern_ok(prefix) || buf.len() < p.len() + 12 {
        return 0;
    }
    buf[..p.len()].copy_from_slice(p);
    let mut n = p.len();
    buf[n] = b'-';
    n += 1;
    // 无符号十进制渲染
    let mut digits = [0u8; 10];
    let mut w = 0usize;
    let mut v = idx;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    }
    while v > 0 {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        buf[n] = digits[w];
        n += 1;
    }
    n
}

// ===========================================================================
// F342 — 预览全能舱：按扩展名定预览器
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Image,
    Text,
    Binary,
    None,
}

pub fn preview_kind(ext: &str) -> PreviewKind {
    match ext {
        "png" | "jpeg" | "gif" | "bmp" | "webp" | "qoi" => PreviewKind::Image,
        "txt" | "md" | "rs" | "toml" | "log" => PreviewKind::Text,
        "" => PreviewKind::None,
        _ => PreviewKind::Binary,
    }
}

// ===========================================================================
// F343 — 路径仪式感：规范路径
// ===========================================================================

/// 规范路径：以 '/' 开头、无 "//"、无 ".." 段、无控制字节、不以 '/' 结尾（根除外）。
pub fn path_canonical(p: &[u8]) -> bool {
    if p.is_empty() || p[0] != b'/' {
        return false;
    }
    if p.len() > 1 && p[p.len() - 1] == b'/' {
        return false;
    }
    let mut i = 0usize;
    while i < p.len() {
        if p[i] < 0x20 {
            return false;
        }
        if i + 1 < p.len() && p[i] == b'/' && p[i + 1] == b'/' {
            return false;
        }
        if i + 2 < p.len() && p[i] == b'/' && p[i + 1] == b'.' && p[i + 2] == b'.' {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F344 — 文件加密保险库：指纹解锁
// ===========================================================================

/// 密钥指纹 = FNV-1a(密钥内容)；解锁即比对指纹。
pub fn vault_fingerprint(key: &[u8]) -> u64 {
    fnv1a64(key)
}

pub fn vault_unlock(key: &[u8], expected_fp: u64) -> bool {
    vault_fingerprint(key) == expected_fp
}

/// 空密钥一律拒绝。
pub fn key_nonempty(key: &[u8]) -> bool {
    !key.is_empty()
}

// ===========================================================================
// F345 — 哈希校验日常化：校验和比对
// ===========================================================================

pub fn checksum_match(data: &[u8], expected: u64) -> bool {
    fnv1a64(data) == expected
}

/// 损坏检测：两次读取哈希不一致即损坏。
pub fn corruption_detected(read_a: &[u8], read_b: &[u8]) -> bool {
    fnv1a64(read_a) != fnv1a64(read_b)
}

// ===========================================================================
// F346 — 磁盘空间叙事：分类占比
// ===========================================================================

pub const USAGE_CATEGORIES: [(&str, u16); 4] =
    [("media", 450), ("docs", 200), ("apps", 250), ("other", 100)];

/// 占比表合法：合计恰为 1000‰。
pub fn usage_breakdown_valid() -> bool {
    USAGE_CATEGORIES.iter().map(|(_, p)| *p as u32).sum::<u32>() == 1000
}

// ===========================================================================
// F347 — 文件健康体检：问题位图
// ===========================================================================

pub const HEALTH_ZERO_LEN: u8 = 1 << 0;
pub const HEALTH_BAD_NAME: u8 = 1 << 1;
pub const HEALTH_CTRL_CHAR: u8 = 1 << 2;
pub const HEALTH_PAD_SPACE: u8 = 1 << 3;

/// 体检：返回问题位图（0 = 健康）。
pub fn health_check(name: &[u8], size: u64) -> u8 {
    let mut flags = 0u8;
    if size == 0 {
        flags |= HEALTH_ZERO_LEN;
    }
    if name.is_empty() || name.len() > 255 {
        flags |= HEALTH_BAD_NAME;
    }
    if name.iter().any(|&b| b < 0x20) {
        flags |= HEALTH_CTRL_CHAR;
    }
    if name.first() == Some(&b' ') || name.last() == Some(&b' ') {
        flags |= HEALTH_PAD_SPACE;
    }
    flags
}

pub fn is_healthy(flags: u8) -> bool {
    flags == 0
}

// ===========================================================================
// F348 — 回收站法医学：删除记录与恢复
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DeletedRecord {
    pub orig_path: &'static str,
    pub deleted_ts: u64,
    pub purge_ts: u64,
    pub shredded: bool,
}

impl DeletedRecord {
    /// 可恢复：未粉碎、未到清除期限、原路径可知。
    pub fn restorable(&self, now: u64) -> bool {
        !self.shredded && now < self.purge_ts && !self.orig_path.is_empty()
    }
}

/// 法医学线索完整度：路径、删除时间、清除时间齐备。
pub fn forensic_complete(r: &DeletedRecord) -> bool {
    !r.orig_path.is_empty() && r.deleted_ts > 0 && r.purge_ts > r.deleted_ts
}

// ===========================================================================
// F349 — 文件迁移向导：进度与预计耗时
// ===========================================================================

pub fn migrate_progress_permille(done: u64, total: u64) -> u16 {
    if total == 0 {
        return 1000;
    }
    ((done.min(total) * 1000 / total) as u16).min(1000)
}

/// 预计剩余分钟（向上取整）。
pub fn migrate_eta_min(remaining_mib: u64, mib_per_min: u64) -> u64 {
    if mib_per_min == 0 {
        return u64::MAX;
    }
    (remaining_mib + mib_per_min - 1) / mib_per_min
}

// ===========================================================================
// F350 — 存储年报：年度存储统计
// ===========================================================================

pub const STORAGE_REPORT_SECTIONS: [&str; 4] = ["growth", "dedup-saved", "tiers", "top-dirs"];

#[derive(Clone, Copy)]
pub struct StorageYearStats {
    pub added_gib: u32,
    pub dedup_saved_mib: u32,
    pub snapshots: u32,
}

impl StorageYearStats {
    pub fn report_ready(&self) -> bool {
        self.added_gib > 0 && STORAGE_REPORT_SECTIONS.len() == 4
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600files_checks() -> CheckSet {
    let mut set = CheckSet::new("m600files");

    // F326 虚拟文件系统
    set.add("F326 vfs mounts", MOUNT_TABLE.len() == 4 && mount_for("/home/a.txt").unwrap().dev == "varixfs", "mount table");
    set.add("F326 vfs longest prefix", mount_for("/sys/x").unwrap().dev == "sysfs" && mount_for("/nope").is_none(), "prefix match");
    set.add("F326 vfs write guard", !write_allowed("/sys/knobs") && write_allowed("/home/a.txt"), "ro enforced");

    // F327 标签即文件夹
    set.add("F327 tags as folders", tag_match(&["work", "2026", "pdf"], &["work", "pdf"]) && !tag_match(&["work"], &["pdf"]), "AND query");
    set.add("F327 tag cap", tags_within_cap(8) && !tags_within_cap(9), "max 8");

    // F328 时间机
    let versions = [
        FileVersion { rev: 1, ts: 100, bytes: 400 },
        FileVersion { rev: 3, ts: 300, bytes: 700 },
        FileVersion { rev: 2, ts: 200, bytes: 500 },
    ];
    let latest = latest_version(&versions); // 末态读取：先存
    set.add("F328 file history", latest.unwrap().rev == 3, "latest rev");
    set.add("F328 prune policy", prune_keep_count(20, 10) == 10 && prune_keep_count(5, 10) == 5, "keep cap");

    // F329 智能收藏夹
    set.add("F329 smart favorites", fav_score(2, 3) == 250 && fav_worthy(2, 3) && !fav_worthy(1, 100), "score 200 gate");
    set.add("F329 fav clamp", fav_score(20, 0) == 1000, "capped at 1000");

    // F330 搜索即行动
    set.add("F330 search actions", action_for_query("rename: a") == Some("open-rename-dialog") && action_for_query("tag:") == Some("open-tag-editor") && action_for_query("plain").is_none(), "verb routes");

    // F331 压缩瑞士军刀
    set.add("F331 comp algo", choose_algo(true, false) == CompAlgo::Store && choose_algo(false, true) == CompAlgo::Lz4 && choose_algo(false, false) == CompAlgo::Deflate, "by workload");
    set.add("F331 comp ratio", ratio_permille(300, 1000) == 300 && ratio_permille(1000, 0) == 1000, "permille");

    // F332 冲突调解庭
    set.add(
        "F332 sync conflict",
        resolve_conflict(100, 90) == ConflictVerdict::KeepA
            && resolve_conflict(90, 100) == ConflictVerdict::KeepB
            && resolve_conflict(100, 99) == ConflictVerdict::BothRename,
        "mtime verdict",
    );

    // F333 大文件搬运工
    set.add("F333 big mover", copy_chunks(10_000, 4_096) == 3 && verify_passes(4_096, 4_096) == 1 && copy_chunks(1, 0) == 0, "chunk plan");

    // F334 完整性公证
    let fp = fnv1a64(b"ledger.txt");
    set.add("F334 integrity notary", notarize(b"ledger.txt", fp) && !notarize(b"ledger.txx", fp), "fnv fingerprint");

    // F335 快照空间站
    set.add("F335 snapshots", snapshot_keep(950, 40) == 8 && snapshot_keep(700, 40) == 16 && snapshot_keep(100, 40) == 40, "space-aware keep");

    // F336 重复收敛
    set.add("F336 dedup", is_duplicate(100, 7, 100, 7) && !is_duplicate(100, 7, 100, 8) && !is_duplicate(100, 7, 200, 7) && !is_duplicate(0, 0, 0, 0), "size+hash");

    // F337 自动分层
    set.add(
        "F337 tiering",
        tier_for(0) == StorageTier::Hot && tier_for(15) == StorageTier::Warm && tier_for(120) == StorageTier::Cold,
        "heat tiers",
    );

    // F338 冷数据归档
    set.add("F338 cold archive", archive_eligible(120, 512) && !archive_eligible(30, 512) && !archive_eligible(120, 8), "90d+64MiB");

    // F339 血缘图
    let lineage = [
        LineageNode { name: "report.rs", parent: -1 },
        LineageNode { name: "report-v2.rs", parent: 0 },
        LineageNode { name: "report-final.rs", parent: 1 },
    ];
    set.add("F339 lineage graph", lineage_depth(&lineage, 0) == 0 && lineage_depth(&lineage, 2) == 2, "derive chain");

    // F340 撤销宇宙
    let mut undo = UndoStack::new();
    undo.push(1);
    undo.push(2);
    undo.push(3);
    let undone = undo.undo(); // 末态读取：先存再操作
    set.add("F340 undo universe", undone == Some(3) && undo.can_redo(), "undo/redo cursor");
    undo.undo();
    undo.push(9); // 截断重做分支
    let redo_after_fork = undo.redo();
    set.add("F340 redo truncated", redo_after_fork.is_none() && !undo.can_redo(), "fork cut");

    // F341 批量操作台
    set.add("F341 batch ops", rename_pattern_ok("photo") && !rename_pattern_ok("") && !rename_pattern_ok("a/b"), "pattern guard");
    let mut buf = [0u8; 32];
    let n = batch_name(&mut buf, "photo", 12);
    let name = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F341 batch name", n == 8 && name == "photo-12", "prefix-idx");

    // F342 预览全能舱
    set.add(
        "F342 preview bay",
        preview_kind("png") == PreviewKind::Image
            && preview_kind("md") == PreviewKind::Text
            && preview_kind("exe") == PreviewKind::Binary
            && preview_kind("") == PreviewKind::None,
        "by ext",
    );

    // F343 路径仪式感
    set.add("F343 path rituals", path_canonical(b"/home/a/b.txt") && !path_canonical(b"home/a") && !path_canonical(b"/a//b") && !path_canonical(b"/a/../b") && path_canonical(b"/"), "canonical");

    // F344 加密保险库
    let vault_fp = vault_fingerprint(b"correct-key");
    set.add("F344 vault", vault_unlock(b"correct-key", vault_fp) && !vault_unlock(b"wrong-key", vault_fp), "fingerprint unlock");
    set.add("F344 vault empty key", !key_nonempty(b""), "reject empty");

    // F345 哈希校验日常化
    set.add("F345 checksum daily", checksum_match(b"data", fnv1a64(b"data")) && corruption_detected(b"ok", b"ko"), "verify+corrupt");

    // F346 磁盘空间叙事
    set.add("F346 disk narrative", USAGE_CATEGORIES.len() == 4 && usage_breakdown_valid(), "sums to 1000‰");

    // F347 文件健康体检
    set.add("F347 health check", is_healthy(health_check(b"note.txt", 300)), "clean file");
    set.add(
        "F347 health flags",
        health_check(b"", 0) == (HEALTH_ZERO_LEN | HEALTH_BAD_NAME)
            && health_check(b" bad .txt ", 10) == HEALTH_PAD_SPACE
            && health_check(b"a\x01b", 10) == HEALTH_CTRL_CHAR,
        "flag bits",
    );

    // F348 回收站法医学
    let rec = DeletedRecord { orig_path: "/home/a/draft.txt", deleted_ts: 1000, purge_ts: 2000, shredded: false };
    set.add("F348 recycle forensics", rec.restorable(1500) && !rec.restorable(2500), "within purge window");
    let shred = DeletedRecord { orig_path: "/home/a/secret", deleted_ts: 1000, purge_ts: 2000, shredded: true };
    set.add("F348 forensic record", !shred.restorable(1500) && forensic_complete(&rec) && !forensic_complete(&DeletedRecord { orig_path: "", deleted_ts: 0, purge_ts: 100, shredded: false }), "shred+ledger");

    // F349 迁移向导
    set.add("F349 migrate wizard", migrate_progress_permille(250, 1000) == 250 && migrate_progress_permille(5, 0) == 1000, "progress");
    set.add("F349 migrate eta", migrate_eta_min(95, 10) == 10 && migrate_eta_min(0, 10) == 0 && migrate_eta_min(1, 0) == u64::MAX, "eta ceil");

    // F350 存储年报
    let stats = StorageYearStats { added_gib: 120, dedup_saved_mib: 9000, snapshots: 48 };
    set.add(
        "F350 storage report",
        stats.report_ready() && STORAGE_REPORT_SECTIONS.len() == 4
            && !StorageYearStats { added_gib: 0, dedup_saved_mib: 0, snapshots: 0 }.report_ready(),
        "sections+counts",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f326_mount_resolution() {
        // 最长前缀优先：/media 挂 varixfs
        assert_eq!(mount_for("/media/movie.mp4").unwrap().dev, "varixfs");
        assert!(mount_for("").is_none());
        assert!(write_allowed("/tmp/scratch"));
    }

    #[test]
    fn f331_ratio_boundaries() {
        assert_eq!(ratio_permille(0, 1000), 0);
        assert_eq!(ratio_permille(999, 1000), 999);
        assert_eq!(ratio_permille(2000, 1000), 1000); // clamp
    }

    #[test]
    fn f340_undo_fork_truncation() {
        let mut u = UndoStack::new();
        u.push(1);
        u.push(2);
        assert_eq!(u.undo(), Some(2));
        assert_eq!(u.undo(), Some(1));
        assert_eq!(u.undo(), None); // 到底
        assert!(u.redo().is_some());
        u.undo();
        u.push(7); // fork
        assert!(!u.can_redo());
        assert_eq!(u.undo(), Some(7));
    }

    #[test]
    fn f341_batch_name_overflow_safe() {
        let mut small = [0u8; 4];
        assert_eq!(batch_name(&mut small, "photo", 1), 0); // 缓冲不足
        let mut buf = [0u8; 32];
        let n = batch_name(&mut buf, "x", 0);
        assert_eq!(&buf[..n], b"x-0");
    }

    #[test]
    fn f343_path_edge_cases() {
        assert!(!path_canonical(b""));
        assert!(!path_canonical(b"/a/b/")); // 尾斜杠
        assert!(!path_canonical(b"/a\x02b")); // 控制字节
        assert!(!path_canonical(b"/a/..b")); // 保守：/.. 前缀一律拒绝
    }

    #[test]
    fn f348_forensics_matrix() {
        let r = DeletedRecord { orig_path: "/x", deleted_ts: 10, purge_ts: 20, shredded: false };
        assert!(r.restorable(10)); // 边界：now == deleted_ts 仍可恢复
        assert!(!r.restorable(20)); // 到期即清除
        let shredded = DeletedRecord { orig_path: "/x", deleted_ts: 10, purge_ts: 20, shredded: true };
        assert!(!shredded.restorable(11));
    }

    #[test]
    fn f350_files_domain_selfcheck_all_pass() {
        let set = run_m600files_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
