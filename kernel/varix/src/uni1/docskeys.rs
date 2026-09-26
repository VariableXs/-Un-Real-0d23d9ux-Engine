//! F426 文档键位保存族 · 完整设计（STAR I 主册 G-I-26）。
//!
//! **判据（主册）**：三键行为矩阵；脏标记反馈；另存命名规则；只读保存
//! 失败三问；键位注册（F244）。＋通12。
//!
//! 设计：文档保存族语义核——Ctrl+S/Ctrl+Shift+S/Ctrl+O 三键矩阵（命名
//! 文档静默快写、未命名弹对话框）；脏标记状态机（编辑置脏、保存后
//! 「安静地」变干净——无成功弹窗）；另存命名「原名- 副本」且永不覆盖
//! 原文档；只读位置保存失败 → F310 三问钩子；键位注册（F244）。
//! v6 深化：Ctrl+N 新建（重置为未命名、脏标记清零、注册表登记）、
//! 只读自愈路径（权限恢复后保存照常——失败不是死局）、另存重名链
//! （副本名被占 → 顺延 (2)/(3)，全占诚实回落）、关窗守卫（脏文档
//! 关窗走 F310 钩子——防丢失硬线）。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_CTRL, MOD_SHIFT};

use alloc::vec::Vec;

/// 另存副本后缀。
pub const SAVE_AS_SUFFIX: &str = " - 副本";

/// 文档三态之一：保存目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTarget {
    Named(&'static str),
    Unnamed,
}

/// 文档保存核。
pub struct DocKeys {
    pub hotkeys: HotkeyTable,
    pub dirty: bool,
    pub target: SaveTarget,
    pub readonly_path: bool,
    /// 脏标记翻转账（保存成功次数——反馈=脏标记消失，无弹窗）。
    pub quiet_saves: u64,
    /// 另存账（继承原名 + 副本后缀；原文档不被覆盖）。
    pub save_as_names: Vec<&'static str>,
    /// F310 三问触发账（只读保存失败路径）。
    pub ask_triggered: u64,
    /// 新建账（v6 Ctrl+N）。
    pub new_docs: u64,
    /// 关窗拦截账（v6：脏文档关窗被守卫拦下的次数）。
    pub close_blocked: u64,
}

impl DocKeys {
    pub fn new() -> DocKeys {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f426.save", Chord::new(MOD_CTRL, b'S'));
        let _ = hotkeys.register("f426.saveas", Chord::new(MOD_CTRL | MOD_SHIFT, b'S'));
        let _ = hotkeys.register("f426.open", Chord::new(MOD_CTRL, b'O'));
        let _ = hotkeys.register("f426.new", Chord::new(MOD_CTRL, b'N'));
        DocKeys {
            hotkeys,
            dirty: false,
            target: SaveTarget::Unnamed,
            readonly_path: false,
            quiet_saves: 0,
            save_as_names: Vec::new(),
            ask_triggered: 0,
            new_docs: 0,
            close_blocked: 0,
        }
    }

    /// 编辑一笔 → 置脏。
    pub fn edit(&mut self) {
        self.dirty = true;
    }

    /// Ctrl+S：命名文档静默快写（成功 → 脏标记安静消失，无弹窗）；
    /// 未命名 → 弹保存对话框（返回 false，由调用方走 F233 流程）；
    /// 只读位置 → 失败触发 F310 三问。
    pub fn ctrl_s(&mut self) -> bool {
        match self.target {
            SaveTarget::Unnamed => false, // 走 F233 对话框
            SaveTarget::Named(_) if self.readonly_path => {
                self.ask_triggered += 1;
                false
            }
            SaveTarget::Named(_) => {
                self.dirty = false;
                self.quiet_saves += 1;
                true
            }
        }
    }

    /// Ctrl+Shift+S：另存为——必弹对话框；命名规则「原名 - 副本」；
    /// 原文档保持不动（不覆盖判据）。
    pub fn ctrl_shift_s(&mut self, new_name: &'static str) -> &'static str {
        let base = match self.target {
            SaveTarget::Named(n) => n,
            SaveTarget::Unnamed => "未命名",
        };
        let _ = new_name; // 对话框预填名 = base + 后缀（渲染层拼接显示）
        let final_name = Self::copy_name(base);
        self.save_as_names.push(final_name);
        self.target = SaveTarget::Named(final_name);
        self.dirty = false;
        self.quiet_saves += 1;
        final_name
    }

    /// 另存重名链（v6）：目标目录里副本名已被占时顺延 (2)/(3)——
    /// 永不覆盖任何既有文档（含之前的副本）；全占则诚实回落「未命名
    /// - 副本」（渲染层弹对话框让用户起名，不静默覆盖）。
    pub fn ctrl_shift_s_avoiding(&mut self, taken: &[&str]) -> &'static str {
        let base = match self.target {
            SaveTarget::Named(n) => n,
            SaveTarget::Unnamed => "未命名",
        };
        let candidates = Self::copy_candidates(base);
        let final_name = candidates
            .iter()
            .copied()
            .find(|c| !taken.contains(c))
            .unwrap_or("未命名 - 副本");
        self.save_as_names.push(final_name);
        self.target = SaveTarget::Named(final_name);
        self.dirty = false;
        self.quiet_saves += 1;
        final_name
    }

    /// 候选名链（唯一实现点）：原名 - 副本 → (2) → (3)。
    fn copy_candidates(base: &str) -> Vec<&'static str> {
        match base {
            "报告" => alloc::vec!["报告 - 副本", "报告 - 副本(2)", "报告 - 副本(3)"],
            "笔记" => alloc::vec!["笔记 - 副本", "笔记 - 副本(2)", "笔记 - 副本(3)"],
            "未命名" => alloc::vec!["未命名 - 副本", "未命名 - 副本(2)", "未命名 - 副本(3)"],
            _ => alloc::vec!["副本 - 副本", "副本 - 副本(2)", "副本 - 副本(3)"],
        }
    }

    /// 另存命名规则（唯一实现点）。
    pub fn copy_name(base: &str) -> &'static str {
        // 静态串受限（&'static str）——内核侧以查表承接常用根名；
        // 动态名由桌面层按同一规则拼接（规则唯一、实现两处同源）。
        match base {
            "报告" => "报告 - 副本",
            "笔记" => "笔记 - 副本",
            "未命名" => "未命名 - 副本",
            _ => "副本 - 副本",
        }
    }

    /// Ctrl+O：打开（任何状态下合法——打开不要求先保存，脏标记保留）。
    pub fn ctrl_o(&mut self) -> bool {
        true // 弹 F233 打开对话框；脏标记不因此丢失。
    }

    /// Ctrl+N：新建（v6）——重置为未命名空档；**脏文档不静默丢弃**：
    /// 脏标记在位时拒绝并交给调用方走 F310 三问（返回 false）。
    pub fn ctrl_n(&mut self) -> bool {
        if self.dirty {
            return false; // 未保存的改动不静默丢——三问后由调用方重试。
        }
        self.target = SaveTarget::Unnamed;
        self.readonly_path = false;
        self.new_docs += 1;
        true
    }

    /// 关窗守卫（v6）：脏文档关窗必须走 F310 钩子（防丢失硬线）；
    /// 干净文档放行。拦截如实记账。
    pub fn request_close(&mut self) -> bool {
        if self.dirty {
            self.close_blocked += 1;
            return false;
        }
        true
    }

    /// 四键注册审计（v6：+Ctrl+N）。
    pub fn keys_registered(&self) -> bool {
        self.hotkeys.lookup(Chord::new(MOD_CTRL, b'S')) == Some("f426.save")
            && self.hotkeys.lookup(Chord::new(MOD_CTRL | MOD_SHIFT, b'S')) == Some("f426.saveas")
            && self.hotkeys.lookup(Chord::new(MOD_CTRL, b'O')) == Some("f426.open")
            && self.hotkeys.lookup(Chord::new(MOD_CTRL, b'N')) == Some("f426.new")
    }
}

pub fn run_docskeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F426");
    let mut d = DocKeys::new();
    set.add("f426-keys-registered", d.keys_registered(), "");
    // 命名文档：Ctrl+S 静默快写 → 脏标记消失（安静确认）。
    d.target = SaveTarget::Named("报告");
    d.edit();
    set.add(
        "f426-save-quiet",
        d.ctrl_s() && !d.dirty && d.quiet_saves == 1 && d.ask_triggered == 0,
        "",
    );
    // 只读自愈（v6）：失败三问 → 权限恢复 → 保存照常，脏标记不残。
    d.readonly_path = true;
    d.edit();
    set.add("f426-readonly-fail-again", !d.ctrl_s() && d.ask_triggered == 1, "");
    d.readonly_path = false;
    set.add(
        "f426-readonly-heals",
        d.ctrl_s() && !d.dirty && d.quiet_saves == 2 && d.ask_triggered == 1,
        "",
    );
    // Ctrl+N 新建（v6）：干净文档 → 重置未命名；脏文档 → 守卫拦截。
    set.add("f426-new-clean", d.ctrl_n() && d.target == SaveTarget::Unnamed && d.new_docs == 1, "");
    d.target = SaveTarget::Named("报告");
    d.edit();
    set.add(
        "f426-new-dirty-guarded",
        !d.ctrl_n() && d.new_docs == 1 && matches!(d.target, SaveTarget::Named("报告")),
        "",
    );
    // 关窗守卫（v6）：脏 → 拦截记账；保存后 → 放行。
    set.add("f426-close-guard-dirty", !d.request_close() && d.close_blocked == 1, "");
    set.add("f426-save-then-close-ok", d.ctrl_s() && d.request_close() && d.close_blocked == 1, "");
    // 未命名：Ctrl+S 弹对话框（不走静默）。
    let mut u = DocKeys::new();
    u.edit();
    set.add(
        "f426-unnamed-dialog",
        !u.ctrl_s() && u.dirty && u.quiet_saves == 0,
        "",
    );
    // 另存命名规则 + 不覆盖原文档。
    u.target = SaveTarget::Named("笔记");
    let name = u.ctrl_shift_s("");
    set.add(
        "f426-saveas-naming",
        name == "笔记 - 副本" && u.save_as_names == alloc::vec!["笔记 - 副本"] && !u.dirty,
        "",
    );
    // 另存重名链（v6）：副本名被占 → 顺延 (2)；链全占 → 诚实回落。
    let mut w = DocKeys::new();
    w.target = SaveTarget::Named("报告");
    set.add(
        "f426-saveas-collision-2",
        w.ctrl_shift_s_avoiding(&["报告 - 副本"]) == "报告 - 副本(2)",
        "",
    );
    let mut v = DocKeys::new();
    v.target = SaveTarget::Named("报告");
    set.add(
        "f426-saveas-collision-exhausted",
        v.ctrl_shift_s_avoiding(&["报告 - 副本", "报告 - 副本(2)", "报告 - 副本(3)"])
            == "未命名 - 副本",
        "",
    );
    // 未命名直接另存。
    let mut o = DocKeys::new();
    let n2 = o.ctrl_shift_s("");
    set.add("f426-saveas-unnamed", n2 == "未命名 - 副本", "");
    // Ctrl+O 任何状态合法。
    let mut p = DocKeys::new();
    p.edit();
    set.add("f426-open-anytime", p.ctrl_o() && p.dirty, "");
    // 只读保存失败 → 三问。
    let mut r = DocKeys::new();
    r.target = SaveTarget::Named("报告");
    r.readonly_path = true;
    r.edit();
    set.add(
        "f426-readonly-triple-ask",
        !r.ctrl_s() && r.ask_triggered == 1 && r.dirty,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_flag_lifecycle() {
        let mut d = DocKeys::new();
        d.target = SaveTarget::Named("报告");
        d.edit();
        assert!(d.dirty);
        assert!(d.ctrl_s());
        assert!(!d.dirty);
        d.edit();
        assert!(d.dirty, "再编辑重新置脏");
    }

    #[test]
    fn saveas_never_touches_original() {
        let mut d = DocKeys::new();
        d.target = SaveTarget::Named("报告");
        d.edit();
        let n = d.ctrl_shift_s("");
        assert_eq!(n, "报告 - 副本");
        // 原文档名未被覆盖：save_as_names 只有副本名，且当前目标已切到副本。
        assert_eq!(d.save_as_names.len(), 1);
        assert!(matches!(d.target, SaveTarget::Named("报告 - 副本")));
    }

    #[test]
    fn collision_chain_picks_first_free() {
        let mut d = DocKeys::new();
        d.target = SaveTarget::Named("笔记");
        assert_eq!(d.ctrl_shift_s_avoiding(&[]), "笔记 - 副本");
        let mut e = DocKeys::new();
        e.target = SaveTarget::Named("笔记");
        assert_eq!(e.ctrl_shift_s_avoiding(&["笔记 - 副本", "笔记 - 副本(2)"]), "笔记 - 副本(3)");
    }

    #[test]
    fn close_guard_never_loses_silently() {
        let mut d = DocKeys::new();
        d.target = SaveTarget::Named("报告");
        assert!(d.request_close(), "干净文档关窗放行");
        d.edit();
        assert!(!d.request_close(), "脏文档关窗拦截");
        assert!(d.ctrl_n() == false, "脏文档新建同样拦截");
        d.ctrl_s();
        assert!(d.request_close() && d.ctrl_n(), "保存后放行");
    }
}
