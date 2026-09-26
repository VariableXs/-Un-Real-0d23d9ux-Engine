//! F488 应用数据位置查看（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三类计量准确性（F392 服务同源）；清缓存只清缓存判据；文档类只读；
//! 路径直达；大小排序。**
//!
//! 功能定义（主册批次三）：vxapp 数据透明页——每应用一行（数据占用大小/
//! 数据目录路径/「打开位置」按钮）；数据类型细分（配置/缓存/用户文档三类
//! 各占多少）；「清缓存」按钮（只动缓存类）；用户文档类只显示不动（数据
//! 安全红线）。
//!
//! 零堆纪律：定长应用数据表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 应用数据行容量。
pub const APP_CAP: usize = 32;

/// 数据三类（主册原文：配置/缓存/用户文档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataKind {
    Config,
    Cache,
    UserDocs,
}

/// 一个应用的数据账。
#[derive(Clone, Copy, Debug)]
pub struct AppData {
    pub app_key: u64,
    pub path: [u8; 48],
    pub path_n: usize,
    /// 三类各自占用（字节）。
    pub config_bytes: u64,
    pub cache_bytes: u64,
    pub docs_bytes: u64,
}

impl AppData {
    pub fn total(&self) -> u64 {
        self.config_bytes + self.cache_bytes + self.docs_bytes
    }

    pub fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_n]).unwrap_or("")
    }
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 数据透明页。
pub struct DataTransparency {
    apps: [Option<AppData>; APP_CAP],
    n: usize,
}

impl DataTransparency {
    pub const fn new() -> Self {
        DataTransparency { apps: [None; APP_CAP], n: 0 }
    }

    pub fn upsert(&mut self, a: AppData) -> bool {
        for i in 0..self.n {
            if let Some(e) = self.apps[i] {
                if e.app_key == a.app_key {
                    self.apps[i] = Some(a);
                    return true;
                }
            }
        }
        if self.n >= APP_CAP {
            return false;
        }
        self.apps[self.n] = Some(a);
        self.n += 1;
        true
    }

    pub fn get(&self, name: &str) -> Option<&AppData> {
        let k = app_key(name);
        (0..self.n).find_map(|i| self.apps[i].as_ref().filter(|a| a.app_key == k))
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 清缓存（主册：只动缓存类——配置与文档纹丝不动）。
    pub fn clear_cache(&mut self, name: &str, confirmed: bool) -> u64 {
        if !confirmed {
            return 0;
        }
        let k = app_key(name);
        for i in 0..self.n {
            if let Some(a) = self.apps[i].as_mut() {
                if a.app_key == k {
                    let freed = a.cache_bytes;
                    a.cache_bytes = 0;
                    return freed;
                }
            }
        }
        0
    }

    /// 文档类只读判据（数据安全红线：文档类永不入清除面）。
    pub fn docs_immutable_after_clear(name: &str, before: u64, after_clear: u64) -> bool {
        let _ = name;
        before == after_clear
    }

    /// 大小排序（主册：大小排序——按总占用降序索引；选择排序零分配）。
    pub fn sorted_by_size(&self) -> [usize; APP_CAP] {
        let mut order = [usize::MAX; APP_CAP];
        let mut m = 0;
        for i in 0..self.n {
            order[m] = i;
            m += 1;
        }
        // 插入排序（n ≤ 32）。
        for i in 1..m {
            let mut j = i;
            while j > 0 {
                let a = self.apps[order[j - 1]].unwrap().total();
                let b = self.apps[order[j]].unwrap().total();
                if a < b {
                    order.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        order
    }

    /// 路径直达（主册：「打开位置」按钮——路径非空可直达）。
    pub fn path_openable(&self, name: &str) -> bool {
        self.get(name).map(|a| !a.path_str().is_empty()).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_appdata_checks() -> CheckSet {
    let mut cs = CheckSet::new("F488-appdata");
    let mut t = DataTransparency::new();
    // 1) 三类计量（F392 同源结构）。
    cs.add("upsert", t.upsert(AppData {
        app_key: app_key("writer"),
        path: {
            let mut p = [0u8; 48];
            let s = b"C:\\data\\writer";
            p[..s.len()].copy_from_slice(s);
            p
        },
        path_n: 14,
        config_bytes: 1_024,
        cache_bytes: 40_960,
        docs_bytes: 1_048_576,
    }), "");
    let w = t.get("writer").unwrap();
    cs.add("three_kinds_metered", w.total() == 1_024 + 40_960 + 1_048_576, "");
    // 2) 清缓存只清缓存（配置/文档纹丝不动）。
    let (cfg_before, docs_before) = (w.config_bytes, w.docs_bytes);
    let freed = t.clear_cache("writer", true);
    let w2 = t.get("writer").unwrap();
    cs.add("clear_cache_only", freed == 40_960 && w2.cache_bytes == 0 && w2.config_bytes == cfg_before && w2.docs_bytes == docs_before, "");
    // 3) 文档类只读（数据安全红线）。
    cs.add("docs_immutable", DataTransparency::docs_immutable_after_clear("writer", docs_before, t.get("writer").unwrap().docs_bytes), "");
    // 4) 未确认不执行。
    cs.add("needs_confirm", t.clear_cache("writer", false) == 0, "");
    // 5) 大小排序（降序）。
    t.upsert(AppData { app_key: app_key("big"), path: [0; 48], path_n: 0, config_bytes: 0, cache_bytes: 0, docs_bytes: 9_000_000 });
    t.upsert(AppData { app_key: app_key("small"), path: [0; 48], path_n: 0, config_bytes: 10, cache_bytes: 0, docs_bytes: 0 });
    let order = t.sorted_by_size();
    cs.add("sorted_desc", order[0] != usize::MAX && t.apps[order[0]].unwrap().total() >= t.apps[order[1]].unwrap().total(), "");
    // 6) 路径直达。
    cs.add("path_openable", t.path_openable("writer"), "");
    cs.add("path_blank_not_openable", !t.path_openable("small"), "");
    // 7) 未知应用诚实（无数据不编造）。
    cs.add("unknown_honest", t.get("ghost").is_none() && t.clear_cache("ghost", true) == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_clear_never_touches_docs() {
        let mut t = DataTransparency::new();
        t.upsert(AppData { app_key: app_key("a"), path: [0; 48], path_n: 0, config_bytes: 5, cache_bytes: 700, docs_bytes: 42 });
        t.clear_cache("a", true);
        let a = t.get("a").unwrap();
        assert_eq!(a.docs_bytes, 42, "用户文档是圣域");
        assert_eq!(a.config_bytes, 5);
        assert_eq!(a.cache_bytes, 0);
    }

    #[test]
    fn size_sort_full_order() {
        let mut t = DataTransparency::new();
        for (name, docs) in [("a", 100u64), ("b", 3_000), ("c", 500)] {
            t.upsert(AppData { app_key: app_key(name), path: [0; 48], path_n: 0, config_bytes: 0, cache_bytes: 0, docs_bytes: docs });
        }
        let order = t.sorted_by_size();
        let sizes: Vec<u64> = order
            .iter()
            .filter(|&&i| i != usize::MAX)
            .filter_map(|&i| t.apps[i].as_ref().map(|a| a.total()))
            .collect();
        assert_eq!(sizes, vec![3_000, 500, 100]);
    }

    #[test]
    fn unconfirmed_clear_is_noop() {
        let mut t = DataTransparency::new();
        t.upsert(AppData { app_key: app_key("x"), path: [0; 48], path_n: 0, config_bytes: 1, cache_bytes: 2, docs_bytes: 3 });
        assert_eq!(t.clear_cache("x", false), 0);
        assert_eq!(t.get("x").unwrap().cache_bytes, 2);
    }
}
