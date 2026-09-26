//! 深化层 · F556 用户目录重定向（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F556 节）：
//! ①「迁移动画进度」的**字节级账**——基础层 progress() 只有文件计数，
//!   深化层补逐文件字节账（进度条的真实数据源：大文件小文件不同权）；
//! ②「所有应用透明跟随（F233/F404 验证）」的**跟随探针**——重定向后
//!   以保存对话框的口吻 probe resolve()，读到新址逐笔记账（跟随不是
//!   声明，是探针实测）；探针落在旧址 = 跟随失败，立红；
//! ③「改回原位同样三步」的**对称校验器**——改回后 resolve/可见性账
//!   必须与初始态镜像对称（改去与改回是同一台机器的两个方向）。

use alloc::string::{String, ToString};
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::userredir::{MigrateMode, RedirMgr};

// ---------------------------------------------------------------------------
// 字节级进度账
// ---------------------------------------------------------------------------

/// 字节级进度账（进度条数据源：文件计数 + 字节计数双口径）。
pub struct ProgressLedger {
    /// 每文件 (名, KiB, 已迁)。
    entries: alloc::vec::Vec<(&'static str, u64, bool)>,
    total_kib: u64,
    done_kib: u64,
}

impl ProgressLedger {
    /// 从目录文件账构建（与 RedirMgr::add_dir 同一份清单——一处一事实）。
    pub fn new(files: &[(&'static str, u64)]) -> ProgressLedger {
        let entries = files.iter().map(|&(n, k)| (n, k, false)).collect();
        ProgressLedger { entries, total_kib: files.iter().map(|&(_, k)| k).sum(), done_kib: 0 }
    }

    /// 标记下一文件已迁（与 mgr.step() 的成功同步喂入）。
    pub fn mark_next_done(&mut self) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|(_, _, done)| !*done) {
            e.2 = true;
            self.done_kib += e.1;
            return true;
        }
        false
    }

    /// (已迁文件数, 总文件数)。
    pub fn files_progress(&self) -> (usize, usize) {
        let done = self.entries.iter().filter(|(_, _, d)| *d).count();
        (done, self.entries.len())
    }

    /// 字节完成度（千分比——进度条按字节权重，不按条数骗平）。
    pub fn bytes_permille(&self) -> u32 {
        if self.total_kib == 0 {
            return 1000;
        }
        (self.done_kib * 1000 / self.total_kib) as u32
    }

    pub fn total_kib(&self) -> u64 {
        self.total_kib
    }
}

// ---------------------------------------------------------------------------
// 应用透明跟随探针
// ---------------------------------------------------------------------------

/// 一条探针记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRecord {
    pub dir_name: String,
    pub observed: String,
    pub expected: String,
}

/// 跟随探针（F233 保存对话框 / F404 应用读址的替身——读到哪记到哪）。
pub struct FollowerProbe {
    log: alloc::vec::Vec<ProbeRecord>,
}

impl FollowerProbe {
    pub fn new() -> FollowerProbe {
        FollowerProbe { log: alloc::vec::Vec::new() }
    }

    /// 一次探针：应用经 resolve() 读到的址 vs 期望（重定向目标）。
    pub fn probe(&mut self, dir_name: &str, observed: &str, expected: &str) -> bool {
        let ok = observed == expected;
        self.log.push(ProbeRecord {
            dir_name: String::from(dir_name),
            observed: String::from(observed),
            expected: String::from(expected),
        });
        ok
    }

    /// 全部探针都跟上了吗（一条落旧址即跟随失败）。
    pub fn all_followed(&self) -> bool {
        self.log.iter().all(|r| r.observed == r.expected)
    }

    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// 失败明细（跟随失败要能指出哪条读的旧址）。
    pub fn failures(&self) -> alloc::vec::Vec<&ProbeRecord> {
        self.log.iter().filter(|r| r.observed != r.expected).collect()
    }
}

impl Default for FollowerProbe {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 改回对称校验器
// ---------------------------------------------------------------------------

/// 改回对称：改回后 resolve 归原址、可见性账与初始态镜像一致。
///
/// `initial_visibility` 取重定向**之前**的可见性账快照。
pub fn restore_symmetry(
    mgr: &RedirMgr,
    dir_name: &str,
    original: &str,
    initial_visibility: &[(String, String)],
) -> bool {
    let resolved_back = mgr.resolve(dir_name) == Some(original);
    let now = mgr.visibility();
    let mirrored = now
        .iter()
        .zip(initial_visibility.iter())
        .all(|(a, b)| a.0 == b.0 && a.1 == b.1);
    resolved_back && mirrored && now.len() == initial_visibility.len()
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f556_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    const FILES: [(&str, u64); 3] = [("报告.docx", 512), ("电影.mkv", 4_000_000), ("笔记.txt", 8)];

    // 1) 字节级进度：三个文件轻重悬殊——字节口径 ≠ 条数口径。
    let mut pl = ProgressLedger::new(&FILES);
    let _ = pl.mark_next_done(); // 报告.docx (512)
    let count_pct = {
        let (done, total) = pl.files_progress();
        done * 100 / total
    };
    cs.add(
        "bytes weighting not file count",
        count_pct == 33 && pl.bytes_permille() < 2 && pl.total_kib() == 4_000_520,
        "",
    );

    // 2) 进度账与迁移步同步喂入：账走完 = 迁移完。
    let mut mgr = RedirMgr::new();
    let _ = mgr.add_dir("下载", "C:\\Users\\v\\Downloads", &FILES).unwrap();
    // 初始态快照（改回对称的镜像基准）——必须在迁移开始前取。
    let initial: alloc::vec::Vec<(String, String)> =
        mgr.visibility().iter().map(|(a, b)| (a.clone(), b.clone())).collect();
    let mut pl2 = ProgressLedger::new(&FILES);
    let _ = mgr.begin("下载", "S:\\Downloads", MigrateMode::MoveContent);
    while mgr.step() {
        pl2.mark_next_done();
    }
    cs.add(
        "ledger finishes with migration",
        mgr.finished() && pl2.files_progress() == (3, 3) && pl2.bytes_permille() == 1000,
        "",
    );

    // 3) 跟随探针：重定向后 resolve 读到新址（F233 保存对话框口径）。
    let mut probe = FollowerProbe::new();
    let observed = mgr.resolve("下载").unwrap_or("").to_string();
    let followed = probe.probe("下载", &observed, "S:\\Downloads");
    cs.add("follower probe reads new home", followed && probe.all_followed() && probe.len() == 1, "");

    // 4) 跟随失败可指认：读到旧址的探针在 failures 里点名。
    let mut probe2 = FollowerProbe::new();
    probe2.probe("下载", "C:\\Users\\v\\Downloads", "S:\\Downloads");
    cs.add(
        "stale reader named in failures",
        !probe2.all_followed() && probe2.failures().len() == 1,
        "",
    );

    // 5) 改回对称：改回同样是一次迁移（begin 反向 + step 走完）——
    //    走完后 resolve/可见性与初始态镜像。
    cs.add("restore original accepted", mgr.restore_original("下载", MigrateMode::MoveContent), "");
    while mgr.step() {
        pl2.mark_next_done(); // 反向迁移的回搬账与正向同一台账口径
    }
    cs.add(
        "restore symmetry mirror",
        restore_symmetry(&mgr, "下载", "C:\\Users\\v\\Downloads", &initial),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ledger_full() {
        let pl = ProgressLedger::new(&[]);
        assert_eq!(pl.bytes_permille(), 1000);
        assert_eq!(pl.files_progress(), (0, 0));
    }

    #[test]
    fn probe_never_probed_is_vacuous_follow() {
        let p = FollowerProbe::new();
        assert!(p.all_followed());
        assert_eq!(p.failures().len(), 0);
    }
}
