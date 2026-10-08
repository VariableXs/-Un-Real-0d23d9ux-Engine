//! H2 快速访问服务 · 深化批次五（F253 的完整服务化——固定表、
//! 推荐共存、拖排序、持久化、去重）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F253 快速访问固定**：固定/取消/拖排序用例、自动推荐与固定
//!   项**共存显示**（固定区在上、推荐区在下、推荐里已固定的不再
//!   出现——去重是结构规则）、持久化（重启验证——顺序封包）、
//!   拖拽固定动画与落点指示（重排即模型重排——h2tabmgr 同纪律）。
//!
//! 时间纪律：无时钟；排序稳定可回放。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 固定表
// ---------------------------------------------------------------------------

/// 固定上限（快速访问钉位 10——超出拒绝留因）。
pub const PIN_CAP: usize = 10;

/// 快速访问服务。
pub struct PinService {
    /// 固定表（顺序 = 显示序）。
    pub pins: Vec<String>,
    /// 被拒操作账（超上限/重复固定/非法移动——拒绝可见）。
    pub rejected: Vec<&'static str>,
}

impl PinService {
    pub fn new() -> PinService {
        PinService { pins: Vec::new(), rejected: Vec::new() }
    }

    /// 固定：去重（已在表 = 拒绝——「再固定一次」不是新操作）、
    /// 上限拒绝、成功置顶吗？不——固定 = 追加到表尾（用户心智：
    /// 新钉的排在钉过的一起，不打乱旧序）。
    pub fn pin(&mut self, path: &str) -> bool {
        if self.pins.iter().any(|p| p == path) {
            self.rejected.push("dup");
            return false;
        }
        if self.pins.len() >= PIN_CAP {
            self.rejected.push("cap");
            return false;
        }
        self.pins.push(path.into());
        true
    }

    /// 取消固定（按路径——幂等：不在表也返回 true，终态一致）。
    pub fn unpin(&mut self, path: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|p| p != path);
        let _ = before;
        true
    }

    /// 拖排序：from → to，越界/原位拒绝留账（h2tabmgr 同规则）。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.pins.len() || to >= self.pins.len() || from == to {
            self.rejected.push("move");
            return false;
        }
        let t = self.pins.remove(from);
        self.pins.insert(to, t);
        true
    }

    /// 共存显示：固定区（原序）+ 推荐区（过滤掉已固定的——去重是
    /// 结构规则：同一路径不允许出现两次）。
    pub fn display(&self, recommendations: &[String]) -> (Vec<String>, Vec<String>) {
        let recs: Vec<String> = recommendations
            .iter()
            .filter(|r| !self.pins.iter().any(|p| p == *r))
            .cloned()
            .collect();
        (self.pins.clone(), recs)
    }

    /// 持久化封包（行式——每行一路径，\n 分隔；顺序即恢复序）。
    pub fn pack(&self) -> String {
        let mut s = String::new();
        for p in &self.pins {
            s.push_str(p);
            s.push('\n');
        }
        s
    }

    /// 恢复（重启验证口径）：整表回填；路径含换行的坏包拒绝
    /// （路径不可能合法含 \n——防注入）。
    pub fn restore(&mut self, pack: &str) -> bool {
        if pack.contains("\r") {
            self.rejected.push("pack");
            return false;
        }
        let paths: Vec<String> = pack.lines().map(|s| s.to_string()).collect();
        let mut seen = Vec::new();
        for p in &paths {
            if seen.iter().any(|s| s == p) {
                self.rejected.push("pack");
                return false;
            }
            seen.push(p.clone());
        }
        self.pins = paths;
        true
    }

    pub fn len(&self) -> usize {
        self.pins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2pinsvc_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2pinsvc");
    let mut svc = PinService::new();
    // 固定：追加保序 + 重复拒绝。
    svc.pin("/c/项目");
    svc.pin("/c/下载");
    set.add(
        "h2pinsvc pin order",
        svc.len() == 2 && svc.pins[0] == "/c/项目" && !svc.pin("/c/项目"),
        "append + dedupe",
    );
    // 共存显示：推荐里的已固定项被滤掉（同路径只出现一次）。
    let recs: Vec<String> = ["/c/项目", "/c/报告", "/c/下载"].iter().map(|s| s.to_string()).collect();
    let (pinned, shown) = svc.display(&recs);
    set.add(
        "h2pinsvc co-display dedupe",
        pinned == vec!["/c/项目".to_string(), "/c/下载".to_string()]
            && shown == vec!["/c/报告".to_string()],
        "fixed above, recs filtered",
    );
    // 拖排序：合法移动 + 越界/原位拒绝。
    set.add(
        "h2pinsvc reorder",
        svc.reorder(1, 0) && svc.pins[0] == "/c/下载" && !svc.reorder(0, 0) && !svc.reorder(0, 9),
        "move audited",
    );
    // 取消固定幂等。
    svc.unpin("/c/下载");
    let _ = svc.unpin("/c/不存在");
    set.add("h2pinsvc unpin idempotent", svc.len() == 1 && !svc.is_empty(), "idempotent unpin");
    // 上限：钉满 10 拒第 11。
    let mut full = PinService::new();
    for i in 0..PIN_CAP {
        assert!(full.pin(&alloc::format!("/c/{i}")));
    }
    set.add(
        "h2pinsvc cap",
        !full.pin("/c/10") && full.rejected.last() == Some(&"cap"),
        "cap 10 refused",
    );
    // 持久化 round-trip：顺序封包→恢复逐条等值。
    let pack = full.pack();
    let mut restored = PinService::new();
    set.add(
        "h2pinsvc roundtrip",
        restored.restore(&pack) && restored.pins == full.pins,
        "order survives reboot",
    );
    // 坏包拒绝：重复行/回车注入。
    set.add(
        "h2pinsvc bad pack refused",
        !restored.restore("/c/a\n/c/a\n") && !restored.restore("/c/a\r/c/b"),
        "injection rejected",
    );
    set.add(
        "h2pinsvc cap const",
        PIN_CAP == 10,
        "10 pins",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2pinsvc_all_green() {
        let set = run_h2pinsvc_checks();
        assert!(set.all_passed(), "h2pinsvc 自检有红项");
        assert!(!set.truncated(), "h2pinsvc 自检溢出");
    }

    #[test]
    fn display_never_duplicates_paths() {
        // 大推荐流灌入：显示结果里任何路径至多出现一次（去重不变式）。
        let mut svc = PinService::new();
        svc.pin("/c/热点");
        let recs: Vec<String> = (0..100).map(|i| alloc::format!("/c/文档{i}.docx")).chain(["/c/热点".to_string()]).collect();
        let (_, shown) = svc.display(&recs);
        let mut uniq = shown.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), shown.len());
    }
}
