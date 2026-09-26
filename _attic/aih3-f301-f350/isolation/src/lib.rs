//! AI-H3 隔离舱（#[path] 直挂真实文件——K2/D1 批同款工艺）。
//!
//! 用途：共享 crate（kernel/varix）被其他分队的在途状态挡住编译时
//! （本次：deskstar/svstar/uni1 等他队模块 201 项在途编译错——均非
//! H3 任务面），本舱把 AI-H3 的真实模块文件原样挂进来独立编译 +
//! 独立测试。**零拷贝、零分叉**：所有 #[path] 指回仓库内的唯一事实
//! 文件，舱内任何红绿都不是另一份事实，只是同一份事实的独立验证口。
//!
//! 垫片面（仅一处，直挂真实文件而非仿写）：
//! - `crate::checks` → 直挂真实 checks.rs（判定口径同源）；
//! - `crate::h3star` → 直挂真实 h3star/mod.rs（兄弟模块按 mod.rs 所在
//!   目录自动解析，五十项 + hbase 全量入舱）。
//!
//! 收口后本舱整体留 `_attic/aih3-f301-f350/` 归档（非功能产物）。

extern crate alloc;

#[path = "../../../../kernel/varix/src/checks.rs"]
pub mod checks;

#[path = "../../../../kernel/varix/src/h3star/mod.rs"]
pub mod h3star;

#[cfg(test)]
mod probe {
    use crate::h3star;

    /// 红灯探针：域聚合 51 块逐块展开——红块列出红项名（K2/D1 舱同款工艺）。
    #[test]
    fn dump_red_checks() {
        let mut reds: Vec<String> = Vec::new();
        for (tag, sub) in h3star::h3star_blocks() {
            if sub.all_passed() && !sub.truncated() {
                continue;
            }
            for i in 0..sub.len() {
                if let Some(c) = sub.get(i) {
                    if !c.passed {
                        reds.push(alloc::format!("{tag} :: {} :: {}", c.name, c.detail));
                    }
                }
            }
            if sub.truncated() {
                reds.push(alloc::format!("{tag} :: TRUNCATED"));
            }
        }
        assert!(reds.is_empty(), "红灯 {} 项:\n{}", reds.len(), reds.join("\n"));
    }

    /// pyfault 用例表逐例探针（定位 15 例中哪例未按期望层命中）。
    #[test]
    fn probe_pyfault_cases() {
        let items = h3star::pyfault::demo_items();
        for (q, n, l) in h3star::pyfault::CASE_TABLE.iter() {
            let hits = h3star::pyfault::search(&items, q, true);
            assert!(
                hits.iter().any(|h| h.name == *n && h.layer == *l),
                "case {q} 期望 {n}@{:?} 实得 {:?}",
                l,
                hits.iter().map(|h| (h.name.as_str(), h.layer)).collect::<Vec<_>>()
            );
        }
    }
}

#[cfg(test)]
mod probe2 {
    use crate::h3star;

    #[test]
    fn probe_filevers_incremental() {
        let mut fv = h3star::filevers::FileVersions::new("空稿");
        let (seq, saved) = h3star::filevers::save_incremental(&mut fv, "首存", 1_000);
        println!("probe: seq={seq} saved={saved} len={}", fv.len());
        let (seq2, saved2) = h3star::filevers::save_incremental(&mut fv, "首存", 2_000);
        println!("probe2: seq={seq2} saved={saved2}");
    }
}
