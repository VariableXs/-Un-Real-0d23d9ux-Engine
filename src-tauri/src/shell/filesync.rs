//! 跨域文件级双向同步（双域总案 ③-a）。
//!
//! 目标：Windows 侧与 Variable 侧的同一份数据**同源可见**——一边改了文件，
//! 另一边在秒级看到；改的是同一份字节，不是两份副本互相猜测。
//!
//! ## 做法：单一事实源 + 事件通知，而不是双写复制
//!
//! 直觉方案是「两边各存一份，后台互相拷贝」。那是错的：两份副本必然出现
//! 「哪份是最新的」问题，冲突时无论选哪边都会丢数据。正确做法是**让两边
//! 指向同一份物理文件**——即 U 盘上的 SHARED 卷（exFAT，Windows 与内核都能
//! 读写）。同步器的职责因此缩小为两件事：
//!
//! 1. **挂载点对齐**：把 SHARED 卷映射到 Variable 工作区里的 `Shared` 目录，
//!    让用户在 Variable 里改 Shared 下的文件 = 直接改 U 盘那份；
//! 2. **变更通知**：目录快照（路径 + 大小 + mtime）轮询出差异，通过事件推给
//!    前端，界面实时刷新，用户不必手动点「刷新」。
//!
//! 这样做的直接好处：内存里永远只有一份真相，断电/拔出不会出现「两边不一致
//! 需要挑一个」的窘境——物理文件本身就是唯一答案。
//!
//! ## 内存级同步（③-b）为什么不在这里做
//!
//! 把 Windows 进程的内存页实时镜像进内核地址空间，需要跨越两个没有任何
//! 共享内存协议的操作系统实例（Windows 有自己的 MMU 页表与物理页管理器，
//! 内核运行在完全独立的机器状态上）。这只能通过**约定的串行化通道**做
//! （共享文件 + 显式 RPC），本质仍是文件级同步之上的一层协议，不是另一种
//! 底座。因此先把文件级打牢，内存级作为它的上层消费者接进来。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 同步条目：文件/目录的稳定指纹（跨平台可比的字段）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncEntry {
    /// 相对 SHARED 根的路径（分隔符统一为 `/`，便于跨平台比较与展示）。
    pub rel: String,
    /// 是否目录。
    pub dir: bool,
    /// 字节大小（目录恒 0）。
    pub size: u64,
    /// 修改时间（Unix 毫秒；取不到恒 0——不编造时间）。
    pub mtime_ms: u64,
}

/// 两次快照的差异。
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncDiff {
    /// 新增或内容变化（大小/mtime 变了）的条目。
    pub changed: Vec<SyncEntry>,
    /// 消失的条目（相对路径）。
    pub removed: Vec<String>,
}

impl SyncDiff {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
    /// 差异条目总数（前端徽标直接显示这个数）。
    pub fn count(&self) -> usize {
        self.changed.len() + self.removed.len()
    }
}

/// 目录快照的容量上限：单个 SHARED 卷理论可放几十万文件，全量扫描会拖慢
/// 界面。取一个既能覆盖真实使用、又不至于卡顿的上限，超出即如实截断并
/// 由 `truncated` 标注（**不假装扫全了**）。
pub const SNAPSHOT_LIMIT: usize = 20_000;

/// 递归深度上限（防止符号链接环或病态深层目录把扫描拖死）。
pub const MAX_DEPTH: usize = 24;

/// 把平台路径转成稳定的相对路径（统一 `/`，去掉前导分隔符）。
pub fn rel_key(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    let mut s = String::new();
    for (i, c) in rel.components().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(&c.as_os_str().to_string_lossy());
    }
    Some(s)
}

/// 单条路径 → SyncEntry（失败返回 None，跳过而不是伪造零值）。
pub fn entry_for(root: &Path, p: &Path, md: &std::fs::Metadata) -> Option<SyncEntry> {
    let rel = rel_key(root, p)?;
    if rel.is_empty() {
        return None;
    }
    let mtime_ms = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Some(SyncEntry {
        rel,
        dir: md.is_dir(),
        size: if md.is_dir() { 0 } else { md.len() },
        mtime_ms,
    })
}

/// 扫描结果：快照 + 是否被上限截断。
pub struct Snapshot {
    pub map: BTreeMap<String, SyncEntry>,
    pub truncated: bool,
}

/// 对目录做一次快照（迭代式 BFS，不用递归——深层目录不会爆栈）。
///
/// 跳过：`.` 开头的隐藏项（与工作区扫描口径一致）、符号链接（防环）。
/// 读取失败的子目录**跳过并继续**：一个权限不足的目录不该让整次同步失败。
pub fn snapshot(root: &Path) -> Snapshot {
    let mut map = BTreeMap::new();
    let mut truncated = false;
    if !root.is_dir() {
        return Snapshot { map, truncated };
    }
    // (路径, 深度)
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> =
        std::collections::VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        if depth > MAX_DEPTH {
            truncated = true;
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue; // 权限/瞬时错误：跳过这个目录，不中断整体同步
        };
        for ent in rd.flatten() {
            let path = ent.path();
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue; // 隐藏项与临时文件不进同步视野
            }
            let Ok(md) = ent.metadata() else { continue };
            // symlink_metadata 视角：符号链接一律跳过（防环 + 防跨卷逃逸）
            if std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(e) = entry_for(root, &path, &md) {
                if map.len() >= SNAPSHOT_LIMIT {
                    truncated = true;
                    return Snapshot { map, truncated };
                }
                let is_dir = e.dir;
                map.insert(e.rel.clone(), e);
                if is_dir {
                    queue.push_back((path, depth + 1));
                }
            }
        }
    }
    Snapshot { map, truncated }
}

/// 比较两次快照。mtime 容差 2 秒：exFAT 的时间戳粒度较粗（2 秒），
/// 严格 `!=` 会在每次扫描都报「变化」——假警报比漏报更消耗信任。
pub fn diff_snapshots(
    before: &BTreeMap<String, SyncEntry>,
    after: &BTreeMap<String, SyncEntry>,
) -> SyncDiff {
    const MTIME_TOLERANCE_MS: u64 = 2000;
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for (rel, a) in after {
        match before.get(rel) {
            None => changed.push(a.clone()),
            Some(b) => {
                let size_differs = a.size != b.size;
                let mtime_differs = a.mtime_ms.abs_diff(b.mtime_ms) > MTIME_TOLERANCE_MS;
                // 只看目录/文件的身份切换或内容变化；纯 mtime 抖动不算。
                if a.dir != b.dir || size_differs || mtime_differs {
                    changed.push(a.clone());
                }
            }
        }
    }
    for rel in before.keys() {
        if !after.contains_key(rel) {
            removed.push(rel.clone());
        }
    }
    SyncDiff { changed, removed }
}

/// 同步根解析：复用 SHARED 卷发现逻辑，取第一个有效根。
///
/// 返回 `None` 表示**这台机器上没有 SHARED 卷**（没插 U 盘 / 未装配）——
/// 这是正常状态而非错误：界面上如实显示「未接共享盘」，绝不假装同步成功。
pub fn resolve_sync_root(data_roots: &[PathBuf]) -> Option<PathBuf> {
    super::shared_apps::discover_shared_roots(data_roots)
        .into_iter()
        .find(|p| p.is_dir())
}

// ---------------------------------------------------------------------------
// 运行期状态
// ---------------------------------------------------------------------------

use std::sync::{Mutex, OnceLock};

/// 上一次快照（跨轮次比对用）。进程内单例——同步是全局视角的事。
static LAST_SNAPSHOT: OnceLock<Mutex<BTreeMap<String, SyncEntry>>> = OnceLock::new();

fn last_snapshot() -> &'static Mutex<BTreeMap<String, SyncEntry>> {
    LAST_SNAPSHOT.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// 计算「自上次调用以来」的差异，并把当前快照存为新的基线。
/// 首次调用（无基线）返回**空差异**——初始建账不该被当成一大波「新增」。
pub fn poll_diff(root: &Path) -> (SyncDiff, bool) {
    let snap = snapshot(root);
    let mut last = match last_snapshot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let diff = if last.is_empty() {
        SyncDiff::default()
    } else {
        diff_snapshots(&last, &snap.map)
    };
    *last = snap.map;
    (diff, snap.truncated)
}

/// 丢弃基线（切根/停用同步时调用）——否则新根的第一帧会把旧根的所有条目
/// 报成「消失」。
pub fn reset_baseline() {
    let mut last = match last_snapshot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    last.clear();
}

// ---------------------------------------------------------------------------
// 命令面
// ---------------------------------------------------------------------------

use crate::error::{AppError, CmdResult};
use tauri::{Emitter, Manager};

/// 同步状态快照（前端首屏与「共享盘」面板的数据源）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// 是否找到可用的 SHARED 卷。
    pub available: bool,
    /// 卷根路径（不可用时为空串）。
    pub root: String,
    /// 当前受管条目数。
    pub entries: usize,
    /// 本轮扫描是否被上限截断（如实标注，不假装扫全了）。
    pub truncated: bool,
}

fn sync_status(root: Option<&Path>) -> SyncStatus {
    let Some(root) = root else {
        return SyncStatus {
            available: false,
            root: String::new(),
            entries: 0,
            truncated: false,
        };
    };
    let snap = snapshot(root);
    SyncStatus {
        available: true,
        root: root.display().to_string(),
        entries: snap.map.len(),
        truncated: snap.truncated,
    }
}

/// 查同步状态（只读）。
#[tauri::command(async)]
pub fn filesync_status(st: tauri::State<'_, crate::state::AppState>) -> SyncStatus {
    let root = resolve_sync_root(&[st.data_dir.clone()]);
    sync_status(root.as_deref())
}

/// 查一次差异并推进基线。前端轮询这个命令做「实时看到对方改动」。
///
/// 返回 `(status, diff)`；无共享卷时 diff 恒空、status.available=false，
/// 前端据此显示「未接共享盘」而不是「一切正常」。
#[tauri::command(async)]
pub fn filesync_poll(
    st: tauri::State<'_, crate::state::AppState>,
) -> (SyncStatus, SyncDiff) {
    let Some(root) = resolve_sync_root(&[st.data_dir.clone()]) else {
        reset_baseline();
        return (sync_status(None), SyncDiff::default());
    };
    let (diff, truncated) = poll_diff(&root);
    let mut status = sync_status(Some(&root));
    // 用本轮的真实截断状态覆盖二次扫描的结果，避免两次扫描口径不一致。
    status.truncated = truncated;
    (status, diff)
}

/// 把 SHARED 卷在 Variable 工作区里的落点路径报给前端（用于「打开共享盘」）。
/// 不可用时返回 None。
#[tauri::command(async)]
pub fn filesync_root(st: tauri::State<'_, crate::state::AppState>) -> Option<String> {
    resolve_sync_root(&[st.data_dir.clone()]).map(|p| p.display().to_string())
}

/// 在系统文件管理器里打开 SHARED 卷（用户点「打开共享盘」）。
#[tauri::command(async)]
pub fn filesync_reveal(app: tauri::AppHandle) -> CmdResult<()> {
    let st = app.state::<crate::state::AppState>();
    let root = resolve_sync_root(&[st.data_dir.clone()])
        .ok_or_else(|| AppError::validation("未找到共享盘（SHARED）— 请插入系统 U 盘"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .arg(root.as_os_str())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| AppError::io(format!("打开共享盘失败：{e}")))?;
    }
    #[cfg(not(windows))]
    {
        let _ = root;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 后台看护线程
// ---------------------------------------------------------------------------

/// 轮询间隔：1 秒。文件级同步的体感目标是「秒级可见」，1 秒轮询在
/// 2 万条目上限下开销可控（一次 stat 遍历），且比文件系统事件监听可靠——
/// exFAT 的网络/可移动卷上事件通知经常不投递，轮询是这里唯一诚实的方案。
pub const POLL_INTERVAL_MS: u64 = 1000;

/// 事件名：前端 listen 这个事件做实时刷新。
pub const SYNC_EVENT: &str = "filesync://changed";

/// 启动同步看护线程（setup 调用）。无共享卷时不报错——安静等待即可，
/// 用户插上 U 盘后下一轮扫描自然发现。
pub fn spawn_watcher(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut announced_absent = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
            let st = app.state::<crate::state::AppState>();
            let Some(root) = resolve_sync_root(&[st.data_dir.clone()]) else {
                if !announced_absent {
                    crate::shell::applog::log("filesync", "未发现 SHARED 卷，同步待命");
                    announced_absent = true;
                }
                reset_baseline();
                continue;
            };
            if announced_absent {
                crate::shell::applog::log("filesync", &format!("SHARED 卷已就绪：{}", root.display()));
                announced_absent = false;
            }
            let (diff, truncated) = poll_diff(&root);
            if diff.is_empty() && !truncated {
                continue;
            }
            let status = sync_status(Some(&root));
            // 事件投递失败（无监听者）不算错误：前端可能还没挂载。
            let _ = app.emit(SYNC_EVENT, (&status, &diff));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "varix-filesync-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(p: &Path, data: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, data).unwrap();
    }

    #[test]
    fn snapshot_sees_files_and_dirs() {
        let root = tmpdir("basic");
        write(&root.join("a.txt"), b"hello");
        write(&root.join("sub/b.txt"), b"world!");
        let s = snapshot(&root);
        assert!(s.map.contains_key("a.txt"));
        assert!(s.map.contains_key("sub"));
        assert!(s.map.contains_key("sub/b.txt"));
        assert_eq!(s.map["a.txt"].size, 5);
        assert!(s.map["a.txt"].dir == false);
        assert!(s.map["sub"].dir);
        assert!(!s.truncated);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_skips_hidden_and_counts_zero_for_dirs() {
        let root = tmpdir("hidden");
        write(&root.join(".secret"), b"x");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        write(&root.join("visible.txt"), b"y");
        let s = snapshot(&root);
        assert!(!s.map.keys().any(|k| k.starts_with('.')));
        assert!(s.map.contains_key("visible.txt"));
        assert_eq!(s.map["visible.txt"].size, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_missing_root_is_empty_not_error() {
        // 没插 U 盘时这里是正常状态：空快照，不是错误。
        let s = snapshot(Path::new("Z:\\definitely-not-here-varix"));
        assert!(s.map.is_empty());
        assert!(!s.truncated);
    }

    #[test]
    fn diff_detects_add_change_remove() {
        let root = tmpdir("diff");
        write(&root.join("keep.txt"), b"12345");
        let before = snapshot(&root).map;

        write(&root.join("keep.txt"), b"1234567890"); // 变长 → changed
        write(&root.join("new.txt"), b"n"); // 新增
        let after = snapshot(&root).map;

        let d = diff_snapshots(&before, &after);
        assert!(d.changed.iter().any(|e| e.rel == "keep.txt"));
        assert!(d.changed.iter().any(|e| e.rel == "new.txt"));
        assert!(d.removed.is_empty());
        assert_eq!(d.count(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn diff_detects_removal() {
        let root = tmpdir("remove");
        write(&root.join("gone.txt"), b"x");
        let before = snapshot(&root).map;
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        let after = snapshot(&root).map;
        let d = diff_snapshots(&before, &after);
        assert_eq!(d.removed, std::vec!["gone.txt"]);
        assert!(d.changed.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn identical_snapshots_produce_empty_diff() {
        // 关键：没有改动时绝不能报「有变化」——假警报会让用户不信这个功能。
        let root = tmpdir("stable");
        write(&root.join("a.txt"), b"content");
        write(&root.join("d/b.txt"), b"more");
        let a = snapshot(&root).map;
        let b = snapshot(&root).map;
        let d = diff_snapshots(&a, &b);
        assert!(d.is_empty(), "同一目录两次快照必须无差异: {d:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mtime_tolerance_absorbs_exfat_coarseness() {
        // exFAT 时间戳粒度 2 秒：刚写过的文件在两秒内重扫，mtime 可能只差
        // 1.x 秒，此时不该报变化（否则每次轮询都刷屏）。
        let mut a: BTreeMap<String, SyncEntry> = BTreeMap::new();
        let mut b: BTreeMap<String, SyncEntry> = BTreeMap::new();
        let base = SyncEntry {
            rel: "f.txt".into(),
            dir: false,
            size: 10,
            mtime_ms: 1_000_000,
        };
        a.insert("f.txt".into(), base.clone());
        b.insert(
            "f.txt".into(),
            SyncEntry {
                mtime_ms: 1_001_900, // +1.9s，在容差内
                ..base.clone()
            },
        );
        assert!(diff_snapshots(&a, &b).is_empty());

        // 超过容差且大小也变 → 必须报出来。
        b.insert(
            "f.txt".into(),
            SyncEntry {
                size: 11,
                mtime_ms: 1_003_000,
                ..base
            },
        );
        assert_eq!(diff_snapshots(&a, &b).count(), 1);
    }

    #[test]
    fn poll_diff_first_call_is_silent() {
        // 共用进程级基线 → 必须串行（否则并行测试互相 reset，见 testgate 说明）。
        let _g = crate::testgate::lock();
        // 首次建立基线不该把整个目录报成「新增」。
        reset_baseline();
        let root = tmpdir("firstpoll");
        write(&root.join("a.txt"), b"x");
        write(&root.join("b.txt"), b"y");
        let (d, _) = poll_diff(&root);
        assert!(d.is_empty(), "首次轮询必须静默建账: {d:?}");
        let _ = std::fs::remove_dir_all(&root);
        reset_baseline();
    }

    #[test]
    fn poll_diff_reports_after_baseline() {
        let _g = crate::testgate::lock();
        reset_baseline();
        let root = tmpdir("secondpoll");
        write(&root.join("a.txt"), b"x");
        let _ = poll_diff(&root); // 建基线
        write(&root.join("b.txt"), b"yy");
        let (d, _) = poll_diff(&root);
        assert_eq!(d.changed.len(), 1);
        assert_eq!(d.changed[0].rel, "b.txt");
        // 再轮一次：无新变化 → 空（不会把上一轮的变化重复报）。
        let (d2, _) = poll_diff(&root);
        assert!(d2.is_empty(), "变化不得重复上报: {d2:?}");
        let _ = std::fs::remove_dir_all(&root);
        reset_baseline();
    }

    #[test]
    fn reset_baseline_prevents_cross_root_false_removals() {
        let _g = crate::testgate::lock();
        // 换根时若不重置基线，新根的第一帧会把旧根所有条目报成「消失」。
        reset_baseline();
        let r1 = tmpdir("rootchange1");
        write(&r1.join("old.txt"), b"x");
        let _ = poll_diff(&r1);
        reset_baseline();
        let r2 = tmpdir("rootchange2");
        write(&r2.join("new.txt"), b"y");
        let (d, _) = poll_diff(&r2);
        assert!(d.is_empty(), "重置基线后不该出现跨根假删除: {d:?}");
        let _ = std::fs::remove_dir_all(&r1);
        let _ = std::fs::remove_dir_all(&r2);
        reset_baseline();
    }

    #[test]
    fn rel_key_normalises_separators() {
        let root = Path::new("C:\\shared");
        let p = root.join("sub").join("file.txt");
        assert_eq!(rel_key(root, &p).as_deref(), Some("sub/file.txt"));
        // 根本身 → 空串（调用方负责跳过）
        assert_eq!(rel_key(root, root).as_deref(), Some(""));
    }

    #[test]
    fn snapshot_limit_is_sane() {
        // 上限本身要是个合理值：太小会漏，太大一帧扫不动。
        assert!(SNAPSHOT_LIMIT >= 1000);
        assert!(MAX_DEPTH >= 8 && MAX_DEPTH <= 64);
        assert!(POLL_INTERVAL_MS >= 200 && POLL_INTERVAL_MS <= 10_000);
    }

    #[test]
    fn sync_event_name_is_stable() {
        // 前端 listen 的字符串是契约，改了就是「实时刷新静默失效」。
        assert_eq!(SYNC_EVENT, "filesync://changed");
    }
}
