//! 任务43（AI-B）· Wine prefix 模板与每进程隔离目录。
//!
//! 语义（对齐宿主 Wine 运行模型 + isolation.rs「一软件一隔离域」）：
//! - **prefix 模板**：标准目录骨架（drive_c / system32 / 用户目录 / temp），
//!   每个进程创建 prefix 时按模板物化（真实文件系统骨架由 VFS 落盘时
//!   按此清单展开——本模块是清单与归属的单一事实源）。
//! - **每进程隔离**：pid → prefix 槽位一对一；路径解析只在本进程 prefix
//!   内做，跨进程访问在设计上不可达（对方 prefix 路径不可解析）。
//! - **逃逸防护**：`..` 规范化越出 prefix 根 → 拒绝（与 vfsguard Escape
//!   语义同构）；绝对路径一律按 prefix 根内相对解释。
//! - **生命周期**：进程退出（含 KILL_ON_JOB_CLOSE 连带）→ prefix 销毁，
//!   槽位归还（零泄漏，压力面见任务45）。
//!
//! 栈纪律：全表 static；路径片段定长字节缓冲（PREFIX_PATH_MAX），零堆。

/// prefix 槽数（≥10 软件并发压力）。
pub const PREFIX_MAX: usize = 16;
/// 路径字节上限（含 NUL）。
pub const PREFIX_PATH_MAX: usize = 128;
/// 模板目录数。
pub const TEMPLATE_DIRS: usize = 8;

/// Wine prefix 标准骨架（任务43 模板单一事实源；落盘清单按此展开）。
pub const PREFIX_TEMPLATE: [&str; TEMPLATE_DIRS] = [
    "drive_c/windows/system32",
    "drive_c/windows/syswow64",
    "drive_c/Program Files",
    "drive_c/users/user/AppData/Local/Temp",
    "drive_c/users/user/AppData/Roaming",
    "drive_c/users/user/Documents",
    "drive_c/windows/temp",
    "dosdevices",
];

/// 路径规范化结果。
#[derive(Debug, PartialEq, Eq)]
pub struct PrefixPath {
    /// prefix 槽内规范化路径（不含 prefix 根前缀）。
    pub rel: [u8; PREFIX_PATH_MAX],
    pub len: usize,
}

impl PrefixPath {
    pub fn as_str(&self) -> &[u8] {
        &self.rel[..self.len]
    }
}

struct PrefixEntry {
    used: bool,
    owner_pid: u32,
    /// 已物化目录清单（模板子集或全部，行内定长）。
    dirs: [[u8; PREFIX_PATH_MAX]; TEMPLATE_DIRS],
    dir_lens: [usize; TEMPLATE_DIRS],
    dir_count: usize,
}

impl PrefixEntry {
    const fn new() -> PrefixEntry {
        PrefixEntry {
            used: false,
            owner_pid: 0,
            dirs: [[0; PREFIX_PATH_MAX]; TEMPLATE_DIRS],
            dir_lens: [0; TEMPLATE_DIRS],
            dir_count: 0,
        }
    }
}

static PREFIXES: crate::cpu::sync::SpinProtected<[PrefixEntry; PREFIX_MAX]> =
    crate::cpu::sync::SpinProtected::new([const { PrefixEntry::new() }; PREFIX_MAX]);

/// 模板物化 + pid 归属（pid 已有 prefix → 幂等返回 true）。
pub fn prefix_create(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut px = PREFIXES.lock();
    if px.iter().any(|e| e.used && e.owner_pid == pid) {
        return true; // 幂等
    }
    let slot = match px.iter_mut().enumerate().find(|(_, e)| !e.used) {
        Some((i, _)) => i,
        None => return false, // 表满
    };
    let e = &mut px[slot];
    e.used = true;
    e.owner_pid = pid;
    e.dir_count = TEMPLATE_DIRS;
    for (i, d) in PREFIX_TEMPLATE.iter().enumerate() {
        let b = d.as_bytes();
        e.dirs[i][..b.len()].copy_from_slice(b);
        e.dir_lens[i] = b.len();
    }
    drop(px);
    // 任务61 · Wine 进程能力收敛：prefix 创建即登记默认能力（无 NET/无宿主盘）。
    let _ = super::winecaps::wine_caps_grant(pid);
    true
}

/// pid 是否持有 prefix。
pub fn prefix_of(pid: u32) -> bool {
    PREFIXES.lock().iter().any(|e| e.used && e.owner_pid == pid)
}

/// 路径规范化：折叠 `.`/`..`，越出根拒绝。仅在本进程 prefix 语义内调用。
pub fn normalize(rel: &[u8]) -> Option<PrefixPath> {
    let mut out = [0u8; PREFIX_PATH_MAX];
    let mut out_len = 0usize;
    let mut seg_start = 0usize;
    let mut depth = 0usize;
    let mut i = 0usize;
    while i <= rel.len() {
        let is_sep = i == rel.len() || rel[i] == b'/' || rel[i] == b'\\';
        if is_sep {
            let seg = &rel[seg_start..i];
            match seg {
                b"" | b"." => {}
                b".." => {
                    if depth == 0 {
                        return None; // 越出 prefix 根（Escape 拒绝）
                    }
                    // 回退上一段
                    while out_len > 0 && out[out_len - 1] != b'/' {
                        out_len -= 1;
                    }
                    if out_len > 0 {
                        out_len -= 1; // 去掉末尾 '/'
                    }
                    depth -= 1;
                }
                _ => {
                    if seg.len() >= PREFIX_PATH_MAX {
                        return None;
                    }
                    if depth > 0 {
                        if out_len + 1 >= PREFIX_PATH_MAX {
                            return None;
                        }
                        out[out_len] = b'/';
                        out_len += 1;
                    }
                    if out_len + seg.len() >= PREFIX_PATH_MAX {
                        return None;
                    }
                    out[out_len..out_len + seg.len()].copy_from_slice(seg);
                    out_len += seg.len();
                    depth += 1;
                }
            }
            seg_start = i + 1;
        }
        i += 1;
    }
    Some(PrefixPath { rel: out, len: out_len })
}

/// 解析进程 pid 的 prefix 内路径 → 规范化路径。
/// **跨进程隔离**：路径语义只在本进程 prefix 内成立；调用方拿不到他人
/// prefix 的解析通道（本函数按 owner 校验）。模板目录内 → 命中；否则
/// 仍可解析（运行期可创建新路径），由 VFS 白名单层做二次裁决。
pub fn prefix_resolve(pid: u32, rel: &[u8]) -> Option<PrefixPath> {
    if !prefix_of(pid) {
        return None; // 无 prefix（未创建/已销毁/他人域）→ 解析不可达
    }
    normalize(rel)
}

/// 相对路径是否命中模板目录（骨架存在性）。
pub fn prefix_has_template_dir(pid: u32, rel: &[u8]) -> bool {
    if !prefix_of(pid) {
        return false;
    }
    let Some(norm) = normalize(rel) else { return false };
    let px = PREFIXES.lock();
    let Some(e) = px.iter().find(|e| e.used && e.owner_pid == pid) else {
        return false;
    };
    (0..e.dir_count).any(|i| e.dirs[i][..e.dir_lens[i]] == *norm.as_str())
}

/// 销毁 pid 的 prefix（退出/Job 连带关闭路径）。返回是否存在过。
pub fn prefix_destroy(pid: u32) -> bool {
    let mut px = PREFIXES.lock();
    let mut existed = false;
    for e in px.iter_mut() {
        if e.used && e.owner_pid == pid {
            *e = PrefixEntry::new();
            existed = true;
        }
    }
    drop(px);
    // 任务61 · 能力表随 prefix 销毁同步退槽（零泄漏，与任务45 同口径）。
    if existed {
        let _ = super::winecaps::wine_caps_revoke_all(pid);
    }
    existed
}

/// 占用槽数（零泄漏断言）。
pub fn prefix_slots_used() -> usize {
    PREFIXES.lock().iter().filter(|e| e.used).count()
}

// ---------------------------------------------------------------------------
// 宿主测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_materialized_per_pid() {
        let _g = crate::proc::testgate::lock();
        assert!(prefix_create(100));
        assert!(prefix_create(100)); // 幂等
        assert!(prefix_of(100));
        for d in PREFIX_TEMPLATE {
            assert!(prefix_has_template_dir(100, d.as_bytes()), "模板缺失: {}", d);
        }
        assert!(prefix_destroy(100));
        assert!(!prefix_of(100));
        assert_eq!(prefix_slots_used(), 0);
    }

    #[test]
    fn per_process_isolation_unreachable() {
        let _g = crate::proc::testgate::lock();
        assert!(prefix_create(101));
        assert!(prefix_create(102));
        // 各自解析自己的路径 ✓
        assert!(prefix_resolve(101, b"drive_c/users/user/Documents/a.txt").is_some());
        assert!(prefix_resolve(102, b"drive_c/users/user/Documents/b.txt").is_some());
        // 101 退出后，其 prefix 不可再解析（他人域不可达）
        assert!(prefix_destroy(101));
        assert!(prefix_resolve(101, b"drive_c/windows/system32/x.dll").is_none());
        // 102 的域不受影响
        assert!(prefix_resolve(102, b"drive_c/windows/system32/x.dll").is_some());
        assert!(prefix_destroy(102));
        assert_eq!(prefix_slots_used(), 0);
    }

    #[test]
    fn normalize_folds_and_rejects_escape() {
        let _g = crate::proc::testgate::lock();
        let n = normalize(b"drive_c/users/../users/user/./x.txt").unwrap();
        assert_eq!(n.as_str(), b"drive_c/users/user/x.txt");
        assert!(normalize(b"../escape").is_none()); // 越根拒绝
        assert!(normalize(b"drive_c/../../etc").is_none());
        assert!(normalize(b"").is_some()); // 空路径 = prefix 根
        let root = normalize(b"").unwrap();
        assert_eq!(root.len, 0);
    }

    #[test]
    fn template_dirs_count_and_no_leak_under_churn() {
        let _g = crate::proc::testgate::lock();
        for k in 0..PREFIX_MAX {
            assert!(prefix_create(200 + k as u32));
        }
        assert!(!prefix_create(999)); // 表满
        assert_eq!(prefix_slots_used(), PREFIX_MAX);
        for k in 0..PREFIX_MAX {
            assert!(prefix_destroy(200 + k as u32));
        }
        assert_eq!(prefix_slots_used(), 0);
        assert!(prefix_create(999)); // 槽位回收后可复用
        assert!(prefix_destroy(999));
    }

    #[test]
    fn zero_pid_rejected() {
        let _g = crate::proc::testgate::lock();
        assert!(!prefix_create(0));
        assert!(!prefix_of(0));
        assert!(!prefix_destroy(0));
    }
}
