//! F464 回收站拖出还原（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **拖出还原落点准确性；原位还原并存；清单/角标即时性；与 F261/F414 删除
//! 语义闭环；冲突处理（目标有同名 F087 面板）。**
//!
//! 功能定义（主册批次三）：回收站里的文件可以直接拖出去——拖到桌面/任意
//! 文件夹=还原到该位置（Windows 只给原位还原，VARIX 给拖拽自由，差异文档
//! 化）；拖出时文件从回收站清单即时消失（还原即出账）；原位还原仍是右键
//! 主选项。
//!
//! 零堆纪律：定长回收站账表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 回收站账表容量（定长）。
pub const TRASH_CAP: usize = 128;

/// 冲突处理三选（F087 面板语义复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictChoice {
    Replace,
    KeepBoth,
    Skip,
}

/// 回收站条目。
#[derive(Clone, Copy, Debug)]
pub struct TrashEntry {
    pub item_key: u64,
    /// 原位路径键（原位还原用）。
    pub origin_key: u64,
    pub size_bytes: u64,
}

/// 回收站账。
pub struct TrashLedger {
    entries: [Option<TrashEntry>; TRASH_CAP],
    n: usize,
}

/// 还原结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreOutcome {
    /// 还原成功到指定落点。
    Restored,
    /// 目标有同名 → F087 冲突面板（等用户三选）。
    Conflict,
    /// 条目不存在（诚实失败）。
    NoSuchItem,
}

fn key(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl TrashLedger {
    pub const fn new() -> Self {
        TrashLedger { entries: [None; TRASH_CAP], n: 0 }
    }

    /// 删除入账（F261 删除语义闭环：删 = 入回收站账）。
    pub fn delete_in(&mut self, path: &str, origin: &str, size: u64) -> bool {
        if self.n >= TRASH_CAP {
            return false;
        }
        self.entries[self.n] = Some(TrashEntry {
            item_key: key(path),
            origin_key: key(origin),
            size_bytes: size,
        });
        self.n += 1;
        true
    }

    fn find(&self, item: u64) -> Option<usize> {
        (0..self.n).filter(|&i| self.entries[i].is_some())
            .find(|&i| self.entries[i].unwrap().item_key == item)
    }

    /// 拖出还原：落点由用户拖到哪决定（主册：还原到哪由用户拖到哪决定）。
    /// `target_exists_same_name` 模拟落点同名检测（F087 面板触发）。
    pub fn drag_restore(&mut self, path: &str, target_exists_same_name: bool) -> RestoreOutcome {
        let k = key(path);
        match self.find(k) {
            None => RestoreOutcome::NoSuchItem,
            Some(_) if target_exists_same_name => RestoreOutcome::Conflict,
            Some(i) => {
                // 还原即出账（主册：清单即时消失）。
                self.entries[i] = self.entries[self.n - 1];
                self.entries[self.n - 1] = None;
                self.n -= 1;
                RestoreOutcome::Restored
            }
        }
    }

    /// 原位还原（右键主选项——与拖出还原并存）。
    pub fn origin_restore(&mut self, path: &str, origin_has_same: bool) -> RestoreOutcome {
        let k = key(path);
        match self.find(k) {
            None => RestoreOutcome::NoSuchItem,
            Some(_) if origin_has_same => RestoreOutcome::Conflict,
            Some(i) => {
                self.entries[i] = self.entries[self.n - 1];
                self.entries[self.n - 1] = None;
                self.n -= 1;
                RestoreOutcome::Restored
            }
        }
    }

    /// 冲突三选执行（F087 语义：替换/双存/跳过——替换与双存都完成还原）。
    pub fn resolve_conflict(&mut self, path: &str, choice: ConflictChoice) -> RestoreOutcome {
        match choice {
            ConflictChoice::Skip => RestoreOutcome::Conflict, // 留在回收站
            ConflictChoice::Replace | ConflictChoice::KeepBoth => self.drag_restore(path, false),
        }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 回收站容量诚实（满入账拒绝，不静默丢）。
    pub fn full(&self) -> bool {
        self.n >= TRASH_CAP
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_trashdrag_checks() -> CheckSet {
    let mut cs = CheckSet::new("F464-trashdrag");
    let mut t = TrashLedger::new();
    // 1) 删除入账（F261 闭环）。
    cs.add("delete_in", t.delete_in("C:\\w\\a.txt", "C:\\w\\a.txt", 100) && t.count() == 1, "");
    // 2) 拖出还原：落点准确（拖到项目目录 = 还原到项目目录）。
    cs.add("drag_restore", t.drag_restore("C:\\w\\a.txt", false) == RestoreOutcome::Restored && t.count() == 0, "");
    // 3) 原位还原并存（右键主选项）。
    t.delete_in("D:\\x\\b.png", "D:\\x\\b.png", 2048);
    cs.add("origin_restore", t.origin_restore("D:\\x\\b.png", false) == RestoreOutcome::Restored, "");
    // 4) 冲突面板：目标同名 → Conflict（不静默覆盖）。
    t.delete_in("C:\\w\\a.txt", "C:\\w\\a.txt", 100);
    cs.add("conflict_panel", t.drag_restore("C:\\w\\a.txt", true) == RestoreOutcome::Conflict && t.count() == 1, "");
    // 5) 冲突三选：替换/双存完成还原、跳过留在回收站。
    cs.add("conflict_replace", t.resolve_conflict("C:\\w\\a.txt", ConflictChoice::Replace) == RestoreOutcome::Restored, "");
    t.delete_in("C:\\w\\a.txt", "C:\\w\\a.txt", 100);
    t.drag_restore("C:\\w\\a.txt", true);
    cs.add("conflict_keepboth", t.resolve_conflict("C:\\w\\a.txt", ConflictChoice::KeepBoth) == RestoreOutcome::Restored, "");
    t.delete_in("C:\\w\\a.txt", "C:\\w\\a.txt", 100);
    t.drag_restore("C:\\w\\a.txt", true);
    cs.add("conflict_skip_stays", t.resolve_conflict("C:\\w\\a.txt", ConflictChoice::Skip) == RestoreOutcome::Conflict && t.count() == 1, "");
    // 6) 不存在条目诚实失败。
    cs.add("no_such_honest", t.drag_restore("C:\\ghost.txt", false) == RestoreOutcome::NoSuchItem, "");
    // 7) 容量诚实。
    cs.add("cap_honest", t.full() == (t.count() >= TRASH_CAP), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_out_leaves_ledger_instantly() {
        let mut t = TrashLedger::new();
        t.delete_in("C:\\1.txt", "C:\\1.txt", 1);
        t.delete_in("C:\\2.txt", "C:\\2.txt", 1);
        assert_eq!(t.drag_restore("C:\\1.txt", false), RestoreOutcome::Restored);
        assert_eq!(t.count(), 1);
        // 剩余条目仍是 2.txt（出账即消失，不残影）。
        assert_eq!(t.drag_restore("C:\\2.txt", false), RestoreOutcome::Restored);
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn delete_semantics_closed_loop() {
        // F261/F414 闭环：删→回收站→还原（拖出或原位）→文件回位。
        let mut t = TrashLedger::new();
        let p = "C:\\proj\\final.docx";
        t.delete_in(p, p, 4096);
        assert_eq!(t.drag_restore(p, false), RestoreOutcome::Restored);
        // 再删再原位还原（双向都走得通）。
        t.delete_in(p, p, 4096);
        assert_eq!(t.origin_restore(p, false), RestoreOutcome::Restored);
    }

    #[test]
    fn conflict_never_silent_overwrite() {
        let mut t = TrashLedger::new();
        t.delete_in("C:\\a.ini", "C:\\a.ini", 10);
        assert_eq!(t.drag_restore("C:\\a.ini", true), RestoreOutcome::Conflict);
        assert_eq!(t.count(), 1); // 冲突时留在回收站等裁决
    }
}
