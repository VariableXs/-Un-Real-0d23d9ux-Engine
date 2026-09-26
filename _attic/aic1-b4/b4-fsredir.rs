
// ---------------------------------------------------------------------------
// F010 · 深化批次四：卸载清扫清单（F031 联动——勾选框面）
//
// 主册依据（G-A-10【交互设计】）：「用户在文件管理器里能看到沙盒目录（透明
/// 可见、可手动清理）」+【状态与异常】卸载时（F031）「弹清扫清单勾选框」。
/// 本面只做勾选模型：列出沙盒条目、勾选、计数——**零真删红线**：真删走回收
/// 站（F031 既有面），本清单自身无删除能力。
// ---------------------------------------------------------------------------

/// 清扫条目（沙盒路径 + 勾选态）。
#[derive(Clone, Copy)]
pub struct SweepEntry {
    path: [u8; 48],
    path_n: usize,
    pub checked: bool,
}

/// 卸载清扫清单（per-app——条目来自该应用沙盒目录枚举）。
pub struct SweepList {
    entries: [Option<SweepEntry>; 8],
    n: usize,
}

impl SweepList {
    pub const fn new() -> SweepList {
        SweepList { entries: [None; 8], n: 0 }
    }

    /// 登记一条沙盒路径（超长路径如实截断计数）。
    /// 返回 (是否登记成功, 是否被截断)。
    pub fn add(&mut self, path: &str) -> (bool, bool) {
        if self.n >= self.entries.len() {
            return (false, false);
        }
        let mut e = SweepEntry { path: [0; 48], path_n: 0, checked: true };
        let src = path.as_bytes();
        e.path_n = src.len().min(48);
        e.path[..e.path_n].copy_from_slice(&src[..e.path_n]);
        let truncated = src.len() > 48;
        self.entries[self.n] = Some(e);
        self.n += 1;
        (true, truncated)
    }

    /// 切换第 idx 条勾选态。
    pub fn toggle(&mut self, idx: usize) -> bool {
        match self.entries.get_mut(idx) {
            Some(Some(e)) => {
                e.checked = !e.checked;
                true
            }
            _ => false,
        }
    }

    pub fn total(&self) -> usize {
        self.n
    }

    /// 勾选数（执行清扫的条目口径——未勾选 = 保留，用户主权）。
    pub fn checked_count(&self) -> usize {
        (0..self.n).filter(|&i| self.entries[i].map_or(false, |e| e.checked)).count()
    }

    /// 第 idx 条路径（查看器回显）。
    pub fn path_at(&self, idx: usize) -> Option<&str> {
        self.entries.get(idx).copied().flatten()
            .and_then(|e| core::str::from_utf8(&e.path[..e.path_n]).ok())
    }
}

/// F010 深化批次四自检。
pub fn run_fsredir_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep3");
    // 1) 清单登记与勾选：默认全勾 → 取消一条 → 勾选数对账。
    let mut list = SweepList::new();
    list.add("~\\AppSandbox\\OldApp\\config");
    list.add("~\\AppSandbox\\OldApp\\cache");
    list.add("~\\AppSandbox\\OldApp\\logs");
    let t1 = list.toggle(1);
    cs.add(
        "sweep_list_default_checked",
        list.total() == 3 && t1 && list.checked_count() == 2,
        "",
    );
    // 2) 路径回显逐条一致；越界 toggle 如实 false。
    let p0 = list.path_at(0);
    let bad = list.toggle(9);
    cs.add(
        "sweep_list_paths_and_bounds",
        p0 == Some("~\\AppSandbox\\OldApp\\config") && !bad,
        "",
    );
    // 3) 超长路径截断如实登记（截断路径不冒充完整路径）。
    let long = "x".repeat(80);
    let (ok, trunc) = list.add(&long);
    cs.add("sweep_list_truncation_honest", ok && trunc, "");
    cs
}
