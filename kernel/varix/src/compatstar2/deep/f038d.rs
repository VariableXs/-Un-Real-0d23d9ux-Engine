//! F038 深化批次二 · 进程代际与生效时点面（compatstar2/deep · G-A-38）。
//!
//! 批次一深化覆盖能力位全集/临时放行台账/配额分档表；本批补齐：进程代际
//! 继承（子进程 gen+1 继承父档——「档位冲突的子进程继承父档」的执行账）、
//! 放行期审计（放行窗口内的允许/收回后拒绝计数——诊断面板第二入口）、
//! 文档目录只读拦截分类（严格档写路径三分类——「为什么写不进」的定位面）、
//! 档位变更生效时点（存活进程提示重启 vs 新进程即时生效——主册【交互设计】）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 代际表容量。
pub const GEN_TABLE_CAP: usize = 8;

/// 一个进程代际条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenEntry {
    pub app: &'static str,
    pub level: crate::compatstar2::isolevel::IsoLevel,
    /// 代际号（0 = 初始进程，子进程 = 父 + 1）。
    pub gen: u32,
}

/// 进程代际表：spawn_child 继承父档并 gen+1。
pub struct GenTable {
    pub entries: [Option<GenEntry>; GEN_TABLE_CAP],
    pub count: usize,
}

impl GenTable {
    pub const fn new() -> Self {
        GenTable { entries: [None; GEN_TABLE_CAP], count: 0 }
    }
    pub fn register_root(&mut self, app: &'static str, level: crate::compatstar2::isolevel::IsoLevel) -> Option<usize> {
        if self.count >= GEN_TABLE_CAP {
            return None;
        }
        self.entries[self.count] = Some(GenEntry { app, level, gen: 0 });
        self.count += 1;
        Some(self.count - 1)
    }
    /// 派生子进程：档位继承 + gen+1（返回新条目下标）。
    pub fn spawn_child(&mut self, parent_idx: usize) -> Option<usize> {
        let p = self.entries.get(parent_idx).copied().flatten()?;
        if self.count >= GEN_TABLE_CAP {
            return None;
        }
        self.entries[self.count] = Some(GenEntry { app: p.app, level: p.level, gen: p.gen + 1 });
        self.count += 1;
        Some(self.count - 1)
    }
}

/// 放行期审计：窗口内请求放行计数 / 收回后拒绝计数（诊断面板数据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrantAudit {
    pub app: &'static str,
    pub allowed_in_window: u32,
    pub denied_after_revoke: u32,
    pub revoked: bool,
}

impl GrantAudit {
    pub const fn new(app: &'static str) -> Self {
        GrantAudit { app, allowed_in_window: 0, denied_after_revoke: 0, revoked: false }
    }
    /// 网络请求：未收回 → 放行计数；已收回 → 拒绝计数。
    pub fn request(&mut self) -> bool {
        if self.revoked {
            self.denied_after_revoke += 1;
            false
        } else {
            self.allowed_in_window += 1;
            true
        }
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
}

/// 严格档写路径三分类（「为什么写不进」第一入口的裁决面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteVerdict {
    /// 沙盒内普通写（Program/Temp 区）。
    SandboxOk,
    /// 严格档写文档目录 → 拒（文档目录只读——主册【功能定义】）。
    DocsReadonlyDenied,
    /// 沙盒外路径 → 拒（越界写）。
    OutsideSandbox,
}

pub fn classify_write(level: crate::compatstar2::isolevel::IsoLevel, path: &str) -> WriteVerdict {
    if !path.starts_with("sandbox:") {
        return WriteVerdict::OutsideSandbox;
    }
    if level == crate::compatstar2::isolevel::IsoLevel::Strict && path.starts_with("sandbox:Docs/") {
        return WriteVerdict::DocsReadonlyDenied;
    }
    WriteVerdict::SandboxOk
}

/// 档位变更生效时点：存活进程 → 提示重启后生效；新启动进程 → 即时生效。
pub fn effect_timing(process_running: bool) -> &'static str {
    if process_running {
        "needs-restart"
    } else {
        "immediate"
    }
}

/// 域自检（深化批次二）。
pub fn run_f038d_checks() -> CheckSet {
    use crate::compatstar2::isolevel::IsoLevel;
    let mut cs = CheckSet::new("F038-isolevel-d2");
    // 1) 代际继承：孙进程 gen=2、档位继承严格档。
    let mut gt = GenTable::new();
    let root = gt.register_root("dl-tool", IsoLevel::Strict).unwrap();
    let child = gt.spawn_child(root).unwrap();
    let grand = gt.spawn_child(child).unwrap();
    cs.add(
        "gen_inheritance",
        gt.entries[child].unwrap().gen == 1
            && gt.entries[grand].unwrap().gen == 2
            && gt.entries[grand].unwrap().level == IsoLevel::Strict,
        "",
    );
    // 2) 放行期审计：窗口内 3 请求放行；收回后 2 请求拒绝（账面分列）。
    let mut ga = GrantAudit::new("dl-tool");
    let mut allowed_ok = true;
    for _ in 0..3 {
        allowed_ok &= ga.request();
    }
    ga.revoke();
    let denied = !ga.request() && !ga.request();
    cs.add("grant_audit", allowed_ok && ga.allowed_in_window == 3 && denied && ga.denied_after_revoke == 2, "");
    // 3) 写路径三分类：沙盒普通/严格档文档拒/沙盒外拒。
    cs.add(
        "write_classification",
        classify_write(IsoLevel::Standard, "sandbox:Program/x.dll") == WriteVerdict::SandboxOk
            && classify_write(IsoLevel::Strict, "sandbox:Docs/a.txt") == WriteVerdict::DocsReadonlyDenied
            && classify_write(IsoLevel::Standard, "sandbox:Docs/a.txt") == WriteVerdict::SandboxOk
            && classify_write(IsoLevel::Loose, "C:\\Windows\\x") == WriteVerdict::OutsideSandbox,
        "",
    );
    // 4) 生效时点：存活进程提示重启、新进程即时。
    cs.add("effect_timing", effect_timing(true) == "needs-restart" && effect_timing(false) == "immediate", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_of_missing_parent_rejected() {
        let mut gt = GenTable::new();
        assert!(gt.spawn_child(0).is_none(), "父不存在不派生（无孤儿进程账）");
    }

    #[test]
    fn standard_docs_writable() {
        // 标准档文档目录可写（只读仅严格档——主册矩阵语义）。
        assert_eq!(classify_write(crate::compatstar2::isolevel::IsoLevel::Standard, "sandbox:Docs/a.txt"), WriteVerdict::SandboxOk);
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f038d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
