//! H2 回收站语义引擎 · 深化批次四（F261 Trash 形制的站内深化——
//! 入站账、容量水位、同名还原、还原账）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F261 删除两路**：普通删 = 进回收站（可撤销）——入站即记账
//!   （原路径 / 删除时刻 / 字节量 / 原名），还原 = 原路送回 + 账目
//!   销账；Shift+Del 直删不经本引擎（Purge 无站内账——结构上不存
//!   在「从直删恢复」的假路径）；
//! - **F085 车道（经 F261 锚）**：删-还原-再删 100 轮零数据损失
//!   ——往返一致性的数据源是本账（哈希对拍的键）；
//! - **十二章「诚实透明」**：容量水位可见（占用多少 / 上限多少 /
//!   驱逐了什么——驱逐留账，不是静默消失）。
//!
//! 时间纪律：分钟戳调用方注入；字节量以 u64 记账。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

use crate::h2star::h2base::bump_copy_name;

// ---------------------------------------------------------------------------
// 站内账
// ---------------------------------------------------------------------------

/// 回收站容量默认（字节——与 F085 容量环同源口径；演示值 1GB）。
pub const DEFAULT_CAPACITY: u64 = 1024 * 1024 * 1024;

/// 一条站内条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashedItem {
    /// 站内键（入站序号——还原/驱逐的句柄）。
    pub slot: u64,
    /// 原完整路径（还原目的地）。
    pub origin: String,
    /// 原文件名（同名还原碰撞时 bump 的对象）。
    pub name: String,
    pub bytes: u64,
    /// 删除时刻（分钟戳）。
    pub deleted_min: u64,
    /// 钉选（用户保护——驱逐豁免）。
    pub pinned: bool,
}

/// 回收站。
pub struct TrashBin {
    items: Vec<TrashedItem>,
    next_slot: u64,
    capacity: u64,
    /// 驱逐账（谁、何时、多大——诚实透明，不静默消失）。
    pub evicted: Vec<(u64, String, u64)>,
}

impl TrashBin {
    pub fn new(capacity: u64) -> TrashBin {
        TrashBin { items: Vec::new(), next_slot: 0, capacity: capacity.max(1), evicted: Vec::new() }
    }

    pub fn with_default() -> TrashBin {
        TrashBin::new(DEFAULT_CAPACITY)
    }

    /// 站内占用（守恒口径：Σ 条目）。
    pub fn used(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 入站：容量不足先驱逐（LRU 按删除时刻最旧、钉选豁免）；
    /// 单件超容量直接拒绝（拒绝留因——不静默丢用户的删除请求）。
    pub fn throw(&mut self, origin: &str, bytes: u64, now_min: u64, pinned: bool) -> Result<u64, &'static str> {
        if bytes > self.capacity {
            return Err("单件超过回收站总容量——请直接删除或调大容量");
        }
        while self.used() + bytes > self.capacity {
            let cand = self
                .items
                .iter()
                .filter(|i| !i.pinned)
                .min_by_key(|i| i.deleted_min)
                .map(|i| (i.slot, i.name.clone(), i.bytes));
            match cand {
                Some((slot, name, b)) => {
                    self.evicted.push((slot, name, b));
                    self.items.retain(|i| i.slot != slot);
                }
                None => {
                    return Err("回收站已满且全部钉选——请清理后再删除");
                }
            }
        }
        let name = origin.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(origin).to_string();
        let slot = self.next_slot;
        self.next_slot += 1;
        self.items.push(TrashedItem {
            slot,
            origin: origin.into(),
            name,
            bytes,
            deleted_min: now_min,
            pinned,
        });
        Ok(slot)
    }

    /// 还原：原路送回。目的地已有同名 → 副本递增（bump_copy_name
    /// ——还原也不许静默覆盖）。返回 (最终落点, 销账 slot)。
    pub fn restore(&mut self, slot: u64, existing: &[String]) -> Option<(String, u64)> {
        let idx = self.items.iter().position(|i| i.slot == slot)?;
        let item = self.items.remove(idx);
        let mut target = item.origin.clone();
        if existing.iter().any(|e| *e == target) {
            let mut cand = bump_copy_name(&target);
            while existing.iter().any(|e| *e == cand) {
                cand = bump_copy_name(&cand);
            }
            target = cand;
        }
        Some((target, item.slot))
    }

    /// 清空（显式动作）：钉选保留——「清空回收站」不清用户钉选，
    /// 钉选是保护标记不是装饰。返回清除条数。
    pub fn empty(&mut self, keep_pinned: bool) -> usize {
        let before = self.items.len();
        if keep_pinned {
            self.items.retain(|i| i.pinned);
        } else {
            self.items.clear();
        }
        before - self.items.len()
    }

    /// 按原目录聚合（「最近删了这个目录下的什么」——恢复向导口径）。
    pub fn count_by_dir(&self, dir: &str) -> usize {
        self.items.iter().filter(|i| {
            let p = &i.origin;
            p.len() > dir.len()
                && p.starts_with(dir)
                && !p[dir.len()..].starts_with(|c: char| c.is_alphanumeric())
        }).count()
    }

    /// 驱逐数（诊断口径）。
    pub fn evicted_count(&self) -> usize {
        self.evicted.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2trash_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2trash");
    let mut bin = TrashBin::new(1000);
    // 入站：记账全字段；占用守恒。
    let s1 = bin.throw("/c/报告.docx", 400, 10, false);
    let s2 = bin.throw("/c/图.png", 300, 20, false);
    set.add(
        "h2trash ledger full",
        s1.is_ok() && s2.is_ok() && bin.used() == 700 && bin.len() == 2,
        "sum = used",
    );
    // 水位驱逐：超限逐出最旧（分钟戳小者）、留驱逐账。
    let s3 = bin.throw("/c/大.zip", 500, 30, false);
    set.add(
        "h2trash lru evict",
        s3.is_ok()
            && bin.used() == 800
            && bin.evicted_count() == 1
            && bin.evicted[0].2 == 400,
        "oldest out, account kept",
    );
    // 钉选豁免：全钉时拒绝并留因。
    let mut bin2 = TrashBin::new(1000);
    let _ = bin2.throw("/c/a.txt", 600, 1, true);
    let r = bin2.throw("/c/b.txt", 600, 2, false);
    set.add(
        "h2trash pinned refuse",
        r.is_err(),
        "full+pinned → refuse",
    );
    // 单件超容量：拒绝留因（不静默丢删除请求）。
    let r2 = bin2.throw("/c/巨型.iso", 2000, 3, false);
    set.add(
        "h2trash single cap",
        r2.is_err(),
        "oversize refused",
    );
    // 还原：原路送回 + 销账；同名走副本阶梯。
    let mut bin3 = TrashBin::new(10_000);
    let a = bin3.throw("/c/方案.txt", 100, 1, false).unwrap();
    bin3.throw("/c/照片.png", 100, 2, false).unwrap();
    let (back1, gone1) = bin3.restore(a, &[]).unwrap();
    set.add(
        "h2trash restore path",
        back1 == "/c/方案.txt" && gone1 == a && bin3.len() == 1,
        "origin + slot freed",
    );
    let b = bin3.throw("/c/方案.txt", 100, 3, false).unwrap();
    let (back2, _) = bin3.restore(b, &["/c/方案.txt".to_string()]).unwrap();
    set.add(
        "h2trash restore collision",
        back2 == "/c/方案 - 副本.txt",
        "bump on collision",
    );
    set.add(
        "h2trash restore missing honest",
        bin3.restore(999, &[]).is_none(),
        "no fake restore",
    );
    // 清空：钉选保留口径。
    let mut bin4 = TrashBin::new(10_000);
    let _ = bin4.throw("/c/1", 1, 1, true);;
    let _ = bin4.throw("/c/2", 1, 2, false);;
    let _ = bin4.throw("/c/3", 1, 3, false);;
    let cleared = bin4.empty(true);
    set.add(
        "h2trash empty keeps pinned",
        cleared == 2 && bin4.len() == 1,
        "pinned survives",
    );
    // 目录聚合：同目录计数（不误伤兄弟目录前缀）。
    let mut bin5 = TrashBin::new(10_000);
    let _ = bin5.throw("/c/Docs/a", 1, 1, false);;
    let _ = bin5.throw("/c/Docs/b", 1, 2, false);;
    let _ = bin5.throw("/c/DocsOld/c", 1, 3, false);;
    set.add(
        "h2trash dir aggregate",
        bin5.count_by_dir("/c/Docs") == 2 && bin5.count_by_dir("/c") == 3,
        "prefix not confused",
    );
    set.add("h2trash capacity const", DEFAULT_CAPACITY == 1024 * 1024 * 1024, "1GiB default");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2trash_all_green() {
        let set = run_h2trash_checks();
        assert!(set.all_passed(), "h2trash 自检有红项");
        assert!(!set.truncated(), "h2trash 自检溢出");
    }

    #[test]
    fn delete_restore_redelete_loop() {
        // F085 车道口径：删-还原-再删 100 轮——键与字节恒守恒。
        let mut bin = TrashBin::with_default();
        for i in 0..100u64 {
            let slot = bin.throw("/c/循环.txt", 128, i, false).unwrap();
            assert_eq!(bin.used(), 128);
            let (back, _) = bin.restore(slot, &[]).unwrap();
            assert_eq!(back, "/c/循环.txt");
            assert_eq!(bin.used(), 0);
        }
    }

    #[test]
    fn capacity_never_exceeded_under_churn() {
        // 容量线在驱逐翻搅下一次都不破。
        let mut bin = TrashBin::new(10_000);
        for i in 0..5_000u64 {
            let _ = bin.throw(&alloc::format!("/c/f{i}"), 300 + (i % 7) * 50, i, i % 50 == 0);
            assert!(bin.used() <= 10_000);
        }
    }
}
