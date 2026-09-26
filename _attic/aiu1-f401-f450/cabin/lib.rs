//! AI-U1 隔离舱：#[path] 直挂 kernel/varix/src/uni1 真实文件 + 真实
//! checks.rs，宿主侧独立验证。用途：多 AI 并行施工期，主 crate 可能被
//! 其他分队的在建模块挡住编译——隔离舱让本队判据验证不排队、不被卡。
//!
//! 挂载的都是**真实生产文件**（不是副本）：这里绿 = 仓库里的代码绿。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

// 真实自检底座（与主 crate 同一份 checks.rs）。
#[path = "../../../kernel/varix/src/checks.rs"]
pub mod checks;

// AI-U1 uni1 全域（mod.rs 相对路径加载同目录九个真实模块）。
#[path = "../../../kernel/varix/src/uni1/mod.rs"]
pub mod uni1;

#[cfg(test)]
mod tests {
    #[test]
    fn cabin_full_domain_green() {
        let blocks: [(&str, crate::checks::CheckSet); 9] = [
            ("ubase", crate::uni1::ubase::run_ubase_checks()),
            ("F401", crate::uni1::autoarrange::run_autoarrange_checks()),
            ("F403", crate::uni1::lockhot::run_lockhot_checks()),
            ("F404", crate::uni1::explorehot::run_explorehot_checks()),
            ("F405", crate::uni1::altf4::run_altf4_checks()),
            ("F407", crate::uni1::sethot::run_sethot_checks()),
            ("F408", crate::uni1::winxmenu::run_winxmenu_checks()),
            ("F416", crate::uni1::winkey::run_winkey_checks()),
            ("F424", crate::uni1::escstack::run_escstack_checks()),
        ];
        for (tag, set) in blocks {
            let mut bad = alloc::vec::Vec::new();
            for i in 0..set.len() {
                if let Some(c) = set.get(i) {
                    if !c.passed {
                        bad.push(c.name);
                    }
                }
            }
            assert!(set.all_passed() && !set.truncated(), "[{tag}] 红项：{:?}", bad);
        }
    }
}
