//! 深化层五 · F136 示例应用仓库（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：示例仓库 → F118 欢迎中心五卡数据、示例启动器
//! 行（先修未完置灰锁）、源码本体只读视图（g 层内容的展示面）。

use super::f136g;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 欢迎中心五卡：示例登记 → 卡片数据（标题/一句话/入口键）
// ---------------------------------------------------------------------------

pub struct WelcomeCard {
    pub key: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
}

const BLURBS: [(&str, &str); 5] = [
    ("hello", "最小窗口骨架，从这里开始"),
    ("clipboard", "剪贴板读写与所有权红线"),
    ("theme-tokens", "令牌取色与深浅联动"),
    ("mini-editor", "编辑三件套与撤销链"),
    ("notepad-lite", "标签页/查找/键盘可达"),
];

pub fn welcome_cards() -> alloc::vec::Vec<WelcomeCard> {
    f136g::examples()
        .iter()
        .map(|(name, _, _)| {
            let blurb = BLURBS
                .iter()
                .find(|b| b.0 == *name)
                .map(|(_, b)| *b)
                .unwrap_or("");
            WelcomeCard { key: name, title: name, blurb }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 示例启动器行：先修完成集 → (示例, 可启动, 锁定原因)
// ---------------------------------------------------------------------------

pub struct LaunchRow {
    pub key: &'static str,
    pub launchable: bool,
    pub lock_reason: Option<&'static str>,
}

/// completed: 用户已学完的示例名。先修未完成 → 置灰 + 原因指名。
pub fn launch_rows(completed: &[&'static str]) -> alloc::vec::Vec<LaunchRow> {
    let ex = f136g::examples();
    let mut rows: alloc::vec::Vec<LaunchRow> = alloc::vec::Vec::new();
    // 先修是"前 n 个"语义：示例 i 的先修 = ex[0..prereqs]（钳到 i 防越界）。
    for (i, (name, _, prereqs)) in ex.iter().enumerate() {
        let pre = (*prereqs).min(i);
        let missing: alloc::vec::Vec<&&'static str> = ex[..pre]
            .iter()
            .map(|(n, _, _)| n)
            .filter(|n| !completed.contains(n))
            .collect();
        if missing.is_empty() {
            rows.push(LaunchRow { key: name, launchable: true, lock_reason: None });
        } else {
            rows.push(LaunchRow {
                key: name,
                launchable: false,
                lock_reason: Some("先修未完成"),
            });
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// 源码只读视图：示例名 → 源码文本（本体在 g 层，展示面不复制内容）
// ---------------------------------------------------------------------------

pub fn source_view(key: &str) -> Option<&'static str> {
    f136g::examples()
        .iter()
        .find(|(n, _, _)| *n == key)
        .map(|(_, src, _)| *src)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136H_TAG: &str = "stareco-F136-deep5";

pub fn run_f136_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F136H_TAG);

    // 欢迎中心五卡
    let cards = welcome_cards();
    set.add("f136h five cards", cards.len() == 5, "五卡齐");
    set.add(
        "f136h blurbs non-empty",
        cards.iter().all(|c| !c.blurb.is_empty()),
        "一句话说明零缺",
    );
    set.add(
        "f136h order",
        cards[0].key == "hello" && cards[4].key == "notepad-lite",
        "链序即卡序",
    );

    // 启动器
    let none = launch_rows(&[]);
    set.add(
        "f136h locked chain",
        none[0].launchable && !none[4].launchable && none[4].lock_reason == Some("先修未完成"),
        "零完成时链尾锁定",
    );
    let all = launch_rows(&["hello", "clipboard", "theme-tokens", "mini-editor", "notepad-lite"]);
    set.add("f136h all open", all.iter().all(|r| r.launchable), "全完成全解锁");
    let partial = launch_rows(&["hello", "theme-tokens"]);
    set.add(
        "f136h prereq chain",
        partial[1].launchable && partial[2].launchable && !partial[3].launchable,
        "clipboard 可开；mini-editor 需 clipboard 锁；theme 侧链独立",
    );

    // 源码只读视图（本体不在 h 层复制——指向 g 层唯一事实）
    set.add(
        "f136h source view",
        source_view("hello").is_some() && source_view("ghost").is_none(),
        "视图指向本体",
    );
    set.add(
        "f136h view no copy",
        source_view("hello") == Some(f136g::HELLO_SRC),
        "同源零拷贝",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn launcher_parallel_branch() {
        // theme-tokens 先修只有 hello——clipboard 未学不拦侧链。
        let rows = launch_rows(&["hello"]);
        assert!(rows[1].launchable); // clipboard：先修 hello 已完成
        assert!(rows[2].launchable); // theme-tokens：侧链独立
        assert!(!rows[3].launchable); // mini-editor 先修 2 个（clipboard 缺）
        assert_eq!(rows[3].lock_reason, Some("先修未完成"));
    }
}
