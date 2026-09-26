//! 深化层 · F578 打开文件位置（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F578 节）：
//! ①「.lnk 打开其目标所在目录」的 **.lnk 解链引擎**——路径级逐层解链
//!   （与基础件名字级解析互补）：%环境变量% 展开、相对路径按 lnk 所在
//!   目录落位、多层 lnk 套娃防御（限深 3 层，第 4 层拒）；
//! ②「与 F292 断链自愈衔接」的 **断链诊断状态机**——基础件 BrokenLnk
//!   结论 → 三态推进：可修复建议/已自愈/不可恢复（终态后事件拒）；
//! ③「高亮让文件跳出来」的 **高亮选中账（纯计算）**——目录枚举 → 目标
//!   定位（不在册不高亮）→ 滚动锚定（目标落视口中部，越界钳顶）；
//! ④「与 F419 菜单不重复项」的 **菜单项协调**——既有菜单已含同义动作
//!   则不再追加第二项（规范动作名去重）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::openloc::{LocResolver, OpenLoc};

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// .lnk 解链引擎（路径级，与基础件名字级解析互补）
// ---------------------------------------------------------------------------

/// lnk 套娃深度红线（主册：限深 3 层）。
pub const LNK_MAX_DEPTH: usize = 3;

/// .lnk 解链引擎：lnk → 目标路径逐层解析。目标可含 %环境变量%、可再
/// 指向另一条 lnk（套娃——超红线拒解）。
pub struct LnkChain {
    /// lnk 名 → 目标（目标本身也可能是另一条 lnk 的名字）。
    lnk_map: Vec<(String, String)>,
    /// 环境变量表：%NAME% → 值。
    env: Vec<(String, String)>,
}

impl LnkChain {
    pub fn new() -> LnkChain {
        LnkChain { lnk_map: Vec::new(), env: Vec::new() }
    }

    pub fn add_lnk(&mut self, name: &str, target: &str) {
        self.lnk_map.push((String::from(name), String::from(target)));
    }

    pub fn add_env(&mut self, var: &str, val: &str) {
        self.env.push((String::from(var), String::from(val)));
    }

    fn lookup(&self, name: &str) -> Option<&str> {
        self.lnk_map.iter().find(|(n, _)| n == name).map(|(_, t)| t.as_str())
    }

    fn env_lookup(&self, var: &str) -> &str {
        self.env.iter().find(|(n, _)| n == var).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    /// %VAR% 展开（无配对 % 的散字符原样保留——不吞用户的百分号）。
    pub fn expand_env(&self, path: &str) -> String {
        let mut out = String::new();
        let mut rest = path;
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) => {
                    out.push_str(self.env_lookup(&after[..end]));
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// 逐层解链：返回 (最终目标路径, 层数)。lnk 不在册 → None；超出
    /// LNK_MAX_DEPTH 层 → None（套娃防御）。
    pub fn resolve(&self, lnk_name: &str) -> Option<(String, usize)> {
        let mut cur = String::from(lnk_name);
        let mut depth = 0usize;
        loop {
            let target = self.lookup(&cur)?;
            depth += 1;
            if depth > LNK_MAX_DEPTH {
                return None;
            }
            let expanded = self.expand_env(target);
            if self.lookup(&expanded).is_some() {
                cur = expanded; // 目标仍是另一条 lnk——下一层。
            } else {
                return Some((expanded, depth));
            }
        }
    }
}

/// 相对路径落位：无盘符（非 "X:" 开头、非根路径）的目标按 lnk 所在
/// 目录补全；空目标 = 断链原料，不落位（None）。
pub fn absolutize(base_dir: &str, target: &str) -> Option<String> {
    let b = target.as_bytes();
    if b.is_empty() {
        return None;
    }
    let absolute = (b.len() >= 2 && b[1] == b':') || b[0] == b'\\';
    if absolute {
        Some(String::from(target))
    } else {
        if base_dir.is_empty() {
            return None;
        }
        let mut out = String::from(base_dir);
        out.push('\\');
        out.push_str(target);
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// 断链诊断状态机（F292 衔接）
// ---------------------------------------------------------------------------

/// 断链诊断三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagState {
    Fixable,       // 目标缺失已登记——给可修复建议（重装/重指）。
    Healed,        // F292 自愈成功——已修复态。
    Unrecoverable, // 自愈失败——不可恢复终态。
}

/// 断链诊断状态机：基础件 BrokenLnk 结论 → 三态推进，单行不回退。
pub struct DiagMachine {
    state: Option<DiagState>,
}

impl DiagMachine {
    pub fn new() -> DiagMachine {
        DiagMachine { state: None }
    }

    pub fn state(&self) -> Option<DiagState> {
        self.state
    }

    /// 目标缺失登记（入口态 Fixable；重复登记拒）。
    pub fn target_missing(&mut self) -> bool {
        if self.state.is_none() {
            self.state = Some(DiagState::Fixable);
            true
        } else {
            false
        }
    }

    /// 自愈成功（Fixable → Healed 终态）。
    pub fn heal_ok(&mut self) -> bool {
        if self.state == Some(DiagState::Fixable) {
            self.state = Some(DiagState::Healed);
            true
        } else {
            false
        }
    }

    /// 自愈失败（Fixable → Unrecoverable 终态）。
    pub fn heal_fail(&mut self) -> bool {
        if self.state == Some(DiagState::Fixable) {
            self.state = Some(DiagState::Unrecoverable);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 高亮选中账（纯计算）+ 菜单项协调
// ---------------------------------------------------------------------------

/// 高亮选中账：目录枚举 → 目标项定位 → 滚动锚定（全部纯计算——宿主
/// 拿账面结论去驱动真滚动）。
pub struct HighlightLedger;

impl HighlightLedger {
    /// 目标项在目录列表中的位次（不在册 → None——不高亮不瞎滚）。
    pub fn locate(listing: &[&str], target: &str) -> Option<usize> {
        listing.iter().position(|n| *n == target)
    }

    /// 滚动锚定：目标项尽量落视口中部（越界钳到 0——顶部不为负）。
    pub fn scroll_anchor(index: usize, viewport: usize) -> usize {
        index.saturating_sub(viewport / 2)
    }
}

/// 「打开文件位置」规范动作名（菜单协调唯一源）。
pub const MENU_ACTION: &str = "打开文件位置";

/// 菜单协调（F419 去重规则）：既有菜单已含同义动作则不再追加第二项。
/// 返回 true = 可追加；false = 已有，不追加。
pub fn menu_dedup(existing_actions: &[&str]) -> bool {
    !existing_actions.contains(&MENU_ACTION)
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f578_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 两层账互证：名字级判 ViaLnk；路径级解链同一本体（层数 1）。
    let mut r = LocResolver::new();
    r.register("播放器", "C:\\Apps\\Star\\player.exe");
    r.register_lnk("播放器.lnk", "播放器");
    let via_ok = matches!(r.resolve("播放器.lnk"), OpenLoc::ViaLnk { .. });
    let mut chain = LnkChain::new();
    chain.add_lnk("播放器.lnk", "C:\\Apps\\Star\\player.exe");
    let want1 = Some((String::from("C:\\Apps\\Star\\player.exe"), 1));
    cs.add("name and path ledgers agree", via_ok && chain.resolve("播放器.lnk") == want1, "");

    // 2) 环境变量展开：%ProgramFiles% 落成真实盘路径。
    let mut c2 = LnkChain::new();
    c2.add_env("ProgramFiles", "C:\\Program Files");
    c2.add_lnk("编辑器.lnk", "%ProgramFiles%\\Star\\edit.exe");
    let want2 = Some((String::from("C:\\Program Files\\Star\\edit.exe"), 1));
    cs.add("env var expanded", c2.resolve("编辑器.lnk") == want2, "");

    // 3) 多层解链：a→b→c→exe 三层恰好卡线通过（层数 3）。
    let mut c3 = LnkChain::new();
    c3.add_lnk("a.lnk", "b.lnk");
    c3.add_lnk("b.lnk", "c.lnk");
    c3.add_lnk("c.lnk", "C:\\deep\\app.exe");
    let want3 = Some((String::from("C:\\deep\\app.exe"), 3));
    cs.add("three layer chain ok", c3.resolve("a.lnk") == want3, "");

    // 4) 套娃防御：四层拒解（限深 3 层红线）。
    let mut c4 = LnkChain::new();
    c4.add_lnk("a.lnk", "b.lnk");
    c4.add_lnk("b.lnk", "c.lnk");
    c4.add_lnk("c.lnk", "d.lnk");
    c4.add_lnk("d.lnk", "C:\\deep\\app.exe");
    cs.add("four layer refused", c4.resolve("a.lnk").is_none() && LNK_MAX_DEPTH == 3, "");

    // 5) 相对路径落位：按 lnk 所在目录补全；已带盘符的目标原样通过。
    let rel = absolutize("C:\\Menu\\Stars", "Star\\app.exe");
    let abs = absolutize("C:\\Menu", "D:\\Elsewhere\\x.exe");
    cs.add(
        "relative target resolved",
        rel == Some(String::from("C:\\Menu\\Stars\\Star\\app.exe"))
            && abs == Some(String::from("D:\\Elsewhere\\x.exe")),
        "",
    );

    // 6) 断链衔接：基础件判 BrokenLnk → 诊断机 Fixable → 自愈 Healed。
    let mut r6 = LocResolver::new();
    r6.register_lnk("断链.lnk", "");
    let broken = matches!(r6.resolve("断链.lnk"), OpenLoc::BrokenLnk { .. });
    let mut diag = DiagMachine::new();
    let entered = broken && diag.target_missing();
    let healed = diag.heal_ok();
    cs.add("broken lnk heals via f292",
        entered && healed && diag.state() == Some(DiagState::Healed), "");

    // 7) 诊断终态诚实：自愈失败走 Unrecoverable；终态后事件全拒。
    let mut diag2 = DiagMachine::new();
    let _ = diag2.target_missing();
    let failed = diag2.heal_fail();
    let closed = diag2.state() == Some(DiagState::Unrecoverable);
    let no_more = !diag2.heal_ok() && !diag2.target_missing();
    cs.add("unrecoverable terminal", failed && closed && no_more, "");

    // 8) 高亮选中账：定位目标 → 锚定视口中部；不在册不高亮。
    let listing = ["a.exe", "b.dll", "c.txt", "star.exe", "e.ini", "f.dll", "g.dll", "h.dll"];
    let idx = HighlightLedger::locate(&listing, "star.exe");
    let anchor = idx.map(|i| HighlightLedger::scroll_anchor(i, 5));
    let missing = HighlightLedger::locate(&listing, "ghost.exe");
    cs.add("highlight locate and anchor",
        idx == Some(3) && anchor == Some(1) && missing.is_none(), "");

    // 9) 高亮锚定钳制：目标在头部锚 0（不为负）；视口大于列表锚 0。
    let top = HighlightLedger::scroll_anchor(0, 5);
    let big_viewport = HighlightLedger::scroll_anchor(4, 100);
    cs.add("anchor clamped to zero", top == 0 && big_viewport == 0, "");

    // 10) 菜单协调（F419）：已有同义动作不追加；没有则可追加。
    cs.add("menu dedup with f419",
        !menu_dedup(&["固定到任务栏", "打开文件位置"]) && menu_dedup(&["固定到任务栏", "卸载"]), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_unmatched_percent_kept() {
        let c = LnkChain::new();
        assert_eq!(c.expand_env("50%off\\x.exe"), String::from("50%off\\x.exe"));
    }

    #[test]
    fn resolve_unknown_lnk_none() {
        let c = LnkChain::new();
        assert!(c.resolve("无.lnk").is_none());
    }

    #[test]
    fn absolutize_empty_target_none() {
        assert!(absolutize("C:\\Menu", "").is_none());
        assert!(absolutize("", "rel.exe").is_none());
    }
}
