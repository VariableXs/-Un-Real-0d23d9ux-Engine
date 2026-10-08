//! TRINITY-500 · AI-04 共享卷数据协议域（F076~F093/F090，W1）
//!
//! 定义「三系统共同读写的那座桥」上的目录与数据契约。
//! 铁律：**可执行程序不共享（ABI 墙）**，只共享文档/媒体/配置/数据与便携软件的数据目录。

// ---------------------------------------------------------------------------
// F076 共享卷目录树规范 — docs/media/config/apps/vault
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Docs,
    Media,
    Config,
    Apps,
    Vault,
    Trash,
}

impl TreeKind {
    pub fn path(self) -> &'static str {
        match self {
            TreeKind::Docs => "/docs",
            TreeKind::Media => "/media",
            TreeKind::Config => "/config",
            TreeKind::Apps => "/apps",
            TreeKind::Vault => "/vault",
            TreeKind::Trash => "/$RECYCLE.BIN",
        }
    }

    pub fn writable_by_default(self) -> bool {
        !matches!(self, TreeKind::Vault)
    }
}

pub const TREE: [TreeKind; 6] = [
    TreeKind::Docs,
    TreeKind::Media,
    TreeKind::Config,
    TreeKind::Apps,
    TreeKind::Vault,
    TreeKind::Trash,
];

/// 目录树是否完整（挂载后自检项之一）。
pub fn tree_complete(present: &[TreeKind]) -> bool {
    TREE.iter().all(|t| present.contains(t))
}

// ---------------------------------------------------------------------------
// F077 文档共享契约 — 记录/导图/推演/代码档案
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    /// 写作空间记录。
    Record,
    /// 思维导图。
    Mind,
    /// 命运推演。
    Fate,
    /// 代码分析档案。
    Code,
}

impl DocKind {
    pub fn subdir(self) -> &'static str {
        match self {
            DocKind::Record => "records",
            DocKind::Mind => "minds",
            DocKind::Fate => "fates",
            DocKind::Code => "code",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            DocKind::Record => ".vrec",
            DocKind::Mind => ".vmind",
            DocKind::Fate => ".vfate",
            DocKind::Code => ".vcode",
        }
    }

    pub fn from_extension(ext: &str) -> Option<DocKind> {
        match ext {
            ".vrec" => Some(DocKind::Record),
            ".vmind" => Some(DocKind::Mind),
            ".vfate" => Some(DocKind::Fate),
            ".vcode" => Some(DocKind::Code),
            _ => None,
        }
    }
}

pub const DOC_KINDS: [DocKind; 4] = [DocKind::Record, DocKind::Mind, DocKind::Fate, DocKind::Code];

/// 拼出文档在共享卷上的规范路径；`id` 必须是纯十六进制名（无路径分隔符）。
pub fn doc_path(kind: DocKind, id: &str, out: &mut [u8]) -> Option<usize> {
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return None;
    }
    let prefix = TreeKind::Docs.path();
    let sub = kind.subdir();
    let ext = kind.extension();
    if out.len() < prefix.len() + 1 + sub.len() + 1 + id.len() + ext.len() {
        return None;
    }
    let mut n = 0usize;
    for part in [prefix, "/", sub, "/", id, ext] {
        out[n..n + part.len()].copy_from_slice(part.as_bytes());
        n += part.len();
    }
    Some(n)
}

// ---------------------------------------------------------------------------
// F078 媒体共享契约 — 图片/音视频/附件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Attachment,
}

impl MediaKind {
    pub fn subdir(self) -> &'static str {
        match self {
            MediaKind::Image => "images",
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
            MediaKind::Attachment => "attachments",
        }
    }

    /// 该类型是否需要 exFAT（>4GB）。视频与附件可能很大。
    pub fn needs_large_fs(self) -> bool {
        matches!(self, MediaKind::Video | MediaKind::Attachment)
    }
}

/// 媒体文件按内容哈希前两位分桶，避免单目录过大（U 盘枚举性能）。
pub fn media_shard(hash: u64) -> u8 {
    (hash & 0xFF) as u8
}

pub fn media_path(kind: MediaKind, hash: u64, ext: &str, out: &mut [u8]) -> Option<usize> {
    let prefix = TreeKind::Media.path();
    let sub = kind.subdir();
    let shard = media_shard(hash);
    let hex = b"0123456789abcdef";
    let shard_str = [hex[(shard >> 4) as usize], hex[(shard & 0xF) as usize]];
    let name_len = 16;
    if out.len() < prefix.len() + 1 + sub.len() + 3 + name_len + ext.len() {
        return None;
    }
    let mut n = 0usize;
    for part in [prefix, "/", sub, "/"] {
        out[n..n + part.len()].copy_from_slice(part.as_bytes());
        n += part.len();
    }
    out[n..n + 2].copy_from_slice(&shard_str);
    n += 2;
    out[n] = b'/';
    n += 1;
    let mut h = hash;
    for i in 0..name_len {
        out[n + name_len - 1 - i] = hex[(h & 0xF) as usize];
        h >>= 4;
    }
    n += name_len;
    out[n..n + ext.len()].copy_from_slice(ext.as_bytes());
    n += ext.len();
    Some(n)
}

// ---------------------------------------------------------------------------
// F079 配置共享契约 — 74 项设置跨系统
// ---------------------------------------------------------------------------

pub const SETTING_COUNT: usize = 74;

#[derive(Clone, Copy, Debug)]
pub struct SettingGroup {
    pub name: &'static str,
    pub offset: usize,
    pub len: usize,
}

pub const SETTING_GROUPS: [SettingGroup; 8] = [
    SettingGroup { name: "appearance", offset: 0, len: 12 },
    SettingGroup { name: "input", offset: 12, len: 10 },
    SettingGroup { name: "privacy", offset: 22, len: 8 },
    SettingGroup { name: "power", offset: 30, len: 6 },
    SettingGroup { name: "storage", offset: 36, len: 8 },
    SettingGroup { name: "apps", offset: 44, len: 10 },
    SettingGroup { name: "desktop", offset: 54, len: 12 },
    SettingGroup { name: "advanced", offset: 66, len: 8 },
];

pub struct SettingsTable {
    values: [u64; SETTING_COUNT],
    pub revision: u32,
}

impl SettingsTable {
    pub const fn new() -> SettingsTable {
        SettingsTable { values: [0u64; SETTING_COUNT], revision: 0 }
    }

    pub fn get(&self, id: usize) -> Option<u64> {
        self.values.get(id).copied()
    }

    pub fn set(&mut self, id: usize, value: u64) -> bool {
        if id >= SETTING_COUNT {
            return false;
        }
        if self.values[id] == value {
            return true; // 幂等：不推 revision
        }
        self.values[id] = value;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// 全表校验和（跨系统比对用：不一致说明需要仲裁）。
    pub fn checksum(&self) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64 ^ self.revision as u64;
        for v in self.values.iter() {
            h ^= *v;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        h
    }

    pub fn group(&self, name: &str) -> Option<&[u64]> {
        SETTING_GROUPS
            .iter()
            .find(|g| g.name == name)
            .map(|g| &self.values[g.offset..g.offset + g.len])
    }

    pub fn capacity_ok() -> bool {
        SETTING_GROUPS.iter().map(|g| g.len).sum::<usize>() == SETTING_COUNT
    }
}

impl Default for SettingsTable {
    fn default() -> Self {
        SettingsTable::new()
    }
}

// ---------------------------------------------------------------------------
// F080 便携软件数据目录 — apps/<slug>/ 规范
// F081 可执行程序清单 — 各系统一份、共享数据
// ---------------------------------------------------------------------------

pub const MAX_SLUG: usize = 32;

/// slug 只允许小写字母、数字与连字符，且不能以连字符开头/结尾。
pub fn slug_valid(slug: &str) -> bool {
    let b = slug.as_bytes();
    if b.is_empty() || b.len() > MAX_SLUG {
        return false;
    }
    if b[0] == b'-' || b[b.len() - 1] == b'-' {
        return false;
    }
    b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// 数据目录：三系统共享的**只有这一处**。
pub fn app_data_dir(slug: &str, out: &mut [u8]) -> Option<usize> {
    if !slug_valid(slug) {
        return None;
    }
    let prefix = TreeKind::Apps.path();
    if out.len() < prefix.len() + 1 + slug.len() + "/data".len() {
        return None;
    }
    let mut n = 0usize;
    for part in [prefix, "/", slug, "/data"] {
        out[n..n + part.len()].copy_from_slice(part.as_bytes());
        n += part.len();
    }
    Some(n)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryHost {
    Windows,
    Variable,
    Varix,
}

impl BinaryHost {
    /// 各系统各自的可执行落点——**永不共享**。
    pub fn dir(self) -> &'static str {
        match self {
            BinaryHost::Windows => "/apps-bin/windows",
            BinaryHost::Variable => "/apps-bin/variable",
            BinaryHost::Varix => "/apps-bin/varix",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            BinaryHost::Windows => ".exe",
            BinaryHost::Variable => ".bin",
            BinaryHost::Varix => ".elf",
        }
    }
}

/// F081：ABI 墙——任一系统的可执行都不允许被另一系统加载。
pub fn binary_allowed(host: BinaryHost, ext: &str) -> bool {
    host.extension() == ext
}

// ---------------------------------------------------------------------------
// F085 回收站三系统对齐
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrashEntry {
    /// 原路径哈希（不存明文路径，减少信息泄露）。
    pub path_hash: u64,
    pub size: u64,
    pub deleted_ms: u64,
    pub owner: u8,
}

pub const TRASH_RETENTION_DAYS: u32 = 30;

pub fn trash_expired(e: &TrashEntry, now_ms: u64) -> bool {
    let day_ms = 86_400_000u64;
    now_ms.saturating_sub(e.deleted_ms) > day_ms * TRASH_RETENTION_DAYS as u64
}

/// 三个系统共用同一个回收站根目录，但各自一个子桶，避免互相清空。
pub fn trash_bucket(owner: u8) -> &'static str {
    match owner {
        1 => "/$RECYCLE.BIN/windows",
        2 => "/$RECYCLE.BIN/variable",
        3 => "/$RECYCLE.BIN/varix",
        _ => "/$RECYCLE.BIN/shared",
    }
}

// ---------------------------------------------------------------------------
// F090 数据迁移向导
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateStep {
    Scan = 0,
    Plan = 1,
    Copy = 2,
    Verify = 3,
    Commit = 4,
}

impl MigrateStep {
    pub fn name(self) -> &'static str {
        match self {
            MigrateStep::Scan => "scan",
            MigrateStep::Plan => "plan",
            MigrateStep::Copy => "copy",
            MigrateStep::Verify => "verify",
            MigrateStep::Commit => "commit",
        }
    }
    pub fn next(self) -> Option<MigrateStep> {
        match self {
            MigrateStep::Scan => Some(MigrateStep::Plan),
            MigrateStep::Plan => Some(MigrateStep::Copy),
            MigrateStep::Copy => Some(MigrateStep::Verify),
            MigrateStep::Verify => Some(MigrateStep::Commit),
            MigrateStep::Commit => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MigrationPlan {
    pub items: u32,
    pub bytes: u64,
    pub conflict: bool,
}

/// 迁移必须可回滚：冲突未解决时不允许进入 Commit。
pub fn may_commit(plan: &MigrationPlan, verified: bool) -> bool {
    verified && !plan.conflict
}

// ---------------------------------------------------------------------------
// F093 只读/可写边界定义
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    None,
    Read,
    ReadWrite,
}

/// 边界表：vault 只接受密文写（F086），apps-bin 只读（ABI 墙），其余可读写。
pub fn access_for(path: &str, is_ciphertext: bool) -> Access {
    if path.starts_with("/vault") {
        return if is_ciphertext { Access::ReadWrite } else { Access::Read };
    }
    if path.starts_with("/apps-bin") {
        return Access::Read;
    }
    if path.starts_with("/docs") || path.starts_with("/media") || path.starts_with("/config") {
        return Access::ReadWrite;
    }
    if path.starts_with("/apps") {
        return Access::ReadWrite;
    }
    Access::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f076_tree_is_complete() {
        assert!(tree_complete(&TREE));
        assert!(!tree_complete(&[TreeKind::Docs]));
        assert!(!TreeKind::Vault.writable_by_default());
    }

    #[test]
    fn f077_doc_paths_are_canonical() {
        let mut out = [0u8; 64];
        let n = doc_path(DocKind::Record, "abc-1", &mut out).unwrap();
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "/docs/records/abc-1.vrec");
        assert!(doc_path(DocKind::Mind, "../evil", &mut out).is_none());
        assert_eq!(DocKind::from_extension(".vcode"), Some(DocKind::Code));
    }

    #[test]
    fn f078_media_shards() {
        let mut out = [0u8; 96];
        let n = media_path(MediaKind::Video, 0xABCD, ".mp4", &mut out).unwrap();
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("/media/video/"));
        assert!(s.ends_with(".mp4"));
        assert!(MediaKind::Video.needs_large_fs());
        assert!(!MediaKind::Image.needs_large_fs());
    }

    #[test]
    fn f079_settings_capacity() {
        assert!(SettingsTable::capacity_ok());
        let mut t = SettingsTable::new();
        assert!(t.set(0, 5));
        let rev = t.revision;
        assert!(t.set(0, 5));
        assert_eq!(t.revision, rev);
        assert!(t.set(0, 6));
        assert_eq!(t.revision, rev + 1);
        assert!(!t.set(SETTING_COUNT, 1));
        assert_eq!(t.group("power").map(|g| g.len()), Some(6));
        assert!(t.checksum() != 0);
    }

    #[test]
    fn f080_slug_rules() {
        assert!(slug_valid("vs-code"));
        assert!(!slug_valid("VS-Code"));
        assert!(!slug_valid("-x"));
        let mut out = [0u8; 64];
        let n = app_data_dir("vs-code", &mut out).unwrap();
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "/apps/vs-code/data");
        assert!(app_data_dir("bad/Slug", &mut out).is_none());
    }

    #[test]
    fn f081_abi_wall() {
        assert!(binary_allowed(BinaryHost::Windows, ".exe"));
        assert!(!binary_allowed(BinaryHost::Varix, ".exe"));
        assert!(binary_allowed(BinaryHost::Varix, ".elf"));
    }

    #[test]
    fn f085_trash_retention() {
        let e = TrashEntry { path_hash: 1, size: 1, deleted_ms: 0, owner: 3 };
        assert!(!trash_expired(&e, 86_400_000 - 1));
        assert!(trash_expired(&e, 86_400_000u64 * 31));
        assert_eq!(trash_bucket(3), "/$RECYCLE.BIN/varix");
    }

    #[test]
    fn f090_migration_walks() {
        let mut s = MigrateStep::Scan;
        let mut count = 1;
        while let Some(n) = s.next() {
            s = n;
            count += 1;
        }
        assert_eq!(count, 5);
        let p = MigrationPlan { items: 1, bytes: 1, conflict: true };
        assert!(!may_commit(&p, true));
        assert!(may_commit(&MigrationPlan { conflict: false, ..p }, true));
    }

    #[test]
    fn f093_access_boundary() {
        assert_eq!(access_for("/vault/a", false), Access::Read);
        assert_eq!(access_for("/vault/a", true), Access::ReadWrite);
        assert_eq!(access_for("/apps-bin/windows/x.exe", false), Access::Read);
        assert_eq!(access_for("/docs/a", false), Access::ReadWrite);
        assert_eq!(access_for("/etc/passwd", false), Access::None);
    }
}
