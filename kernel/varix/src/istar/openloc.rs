//! F578 打开文件位置 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：直开/lnk 两场景；高亮选中；断链诊断联动；
//! 目录打开性能；菜单项位置（与 F419 协调）。
//!
//! **设计要点（主册）**：
//! - 开始菜单/搜索结果右键「打开文件位置」：直达应用所在目录并高亮选中
//!   该应用文件；
//! - 快捷方式（.lnk）场景打开其目标所在目录（不是 .lnk 所在处——用户想看
//!   的是本体）；
//! - 与 F292 断链自愈衔接（目标丢失时此处给诊断）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 「打开文件位置」动作结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenLoc {
    /// 直开：应用本体所在目录 + 高亮本体。
    Direct { dir: String, highlight: String },
    /// .lnk 场景：目标所在目录 + 高亮目标本体（不是 .lnk 所在处）。
    ViaLnk { dir: String, highlight: String },
    /// 断链：.lnk 目标丢失 → 诊断联动（F292 自愈入口）。
    BrokenLnk { lnk_path: String },
    /// 应用未登记（搜索索引外）——诚实告知。
    Unknown,
}

/// 位置解析器。
pub struct LocResolver {
    /// 应用登记账（应用名 → 本体路径）。
    registry: [(String, String); 32],
    reg_len: usize,
    /// .lnk 账（lnk 路径 → 目标路径）。
    lnks: [(String, String); 32],
    lnk_len: usize,
    /// 最近一次目录打开耗时（ms，宿主注入——性能对账）。
    last_open_ms: u64,
}

impl LocResolver {
    pub fn new() -> LocResolver {
        LocResolver {
            registry: [(); 32].map(|_| (String::new(), String::new())),
            reg_len: 0,
            lnks: [(); 32].map(|_| (String::new(), String::new())),
            lnk_len: 0,
            last_open_ms: 0,
        }
    }

    /// 登记应用本体。
    pub fn register(&mut self, app: &str, exe_path: &str) -> bool {
        if self.reg_len >= 32 {
            return false;
        }
        self.registry[self.reg_len] = (String::from(app), String::from(exe_path));
        self.reg_len += 1;
        true
    }

    /// 登记 .lnk 映射。
    pub fn register_lnk(&mut self, lnk: &str, target: &str) -> bool {
        if self.lnk_len >= 32 {
            return false;
        }
        self.lnks[self.lnk_len] = (String::from(lnk), String::from(target));
        self.lnk_len += 1;
        true
    }

    pub fn note_open_ms(&mut self, ms: u64) {
        self.last_open_ms = ms;
    }

    /// 目录打开性能（Windows 目录打开口径：<1s 常规线——本域对账线）。
    pub fn open_within_budget(&self) -> bool {
        self.last_open_ms <= 1_000
    }

    /// 右键「打开文件位置」解析（三场景 + 未知）。
    pub fn resolve(&self, app: &str) -> OpenLoc {
        // 1) .lnk 场景优先（开始菜单项多为 .lnk——先查映射）。
        for (lnk, target) in self.lnks[..self.lnk_len].iter() {
            if lnk == app {
                if target.is_empty() {
                    // 断链登记（目标丢失）。
                    return OpenLoc::BrokenLnk { lnk_path: lnk.clone() };
                }
                let exe = self.registry[..self.reg_len]
                    .iter()
                    .find(|(a, _)| a == target)
                    .map(|(_, p)| p.clone());
                return match exe {
                    Some(p) => OpenLoc::ViaLnk { dir: dir_of(&p), highlight: file_of(&p) },
                    None => OpenLoc::BrokenLnk { lnk_path: lnk.clone() },
                };
            }
        }
        // 2) 直开场景。
        for (a, p) in self.registry[..self.reg_len].iter() {
            if a == app {
                return OpenLoc::Direct { dir: dir_of(p), highlight: file_of(p) };
            }
        }
        OpenLoc::Unknown
    }
}

impl Default for LocResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// 路径 → 目录段（反斜杠分隔；无分隔符视为根项）。
fn dir_of(path: &str) -> String {
    match path.rfind('\\') {
        Some(i) => String::from(&path[..i]),
        None => String::from(""),
    }
}

/// 路径 → 文件名段。
fn file_of(path: &str) -> String {
    match path.rfind('\\') {
        Some(i) => String::from(&path[i + 1..]),
        None => String::from(path),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_openloc_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 直开场景：直达本体目录 + 高亮本体。
    let mut r = LocResolver::new();
    r.register("记事本", "C:\\Windows\\System32\\notepad.exe");
    let direct = r.resolve("记事本");
    set.add(
        "direct opens exe dir and highlights",
        direct
            == OpenLoc::Direct {
                dir: String::from("C:\\Windows\\System32"),
                highlight: String::from("notepad.exe"),
            },
        "",
    );

    // 2. .lnk 场景：打开目标所在目录（不是 .lnk 所在处——本体不套娃）。
    r.register("游戏", "D:\\Games\\Star\\star.exe");
    r.register_lnk("游戏.lnk", "游戏");
    let via = r.resolve("游戏.lnk");
    set.add(
        "lnk opens target dir not lnk dir",
        via
            == OpenLoc::ViaLnk {
                dir: String::from("D:\\Games\\Star"),
                highlight: String::from("star.exe"),
            },
        "",
    );

    // 3. 断链诊断联动：目标丢失 → BrokenLnk（F292 自愈入口参数齐）。
    r.register_lnk("幽灵.lnk", "");
    let broken = r.resolve("幽灵.lnk");
    r.register_lnk("悬空.lnk", "已被卸载的应用");
    let broken2 = r.resolve("悬空.lnk");
    set.add(
        "broken lnk routes to diagnosis",
        broken
            == OpenLoc::BrokenLnk {
                lnk_path: String::from("幽灵.lnk"),
            }
            && matches!(broken2, OpenLoc::BrokenLnk { .. }),
        "",
    );

    // 4. 未知应用诚实告知（不猜路径）。
    set.add("unknown app honest", r.resolve("不存在") == OpenLoc::Unknown, "");

    // 5. 高亮选中：highlight 字段与目录段互证（高亮文件确实属于该目录）。
    if let OpenLoc::Direct { dir, highlight } = r.resolve("记事本") {
        let full = alloc::format!("{}\\{}", dir, highlight);
        set.add(
            "highlight belongs to dir",
            full == "C:\\Windows\\System32\\notepad.exe",
            "",
        );
    } else {
        set.add("highlight belongs to dir", false, "");
    }

    // 6. 目录打开性能：999ms 过线、1001ms 拒。
    r.note_open_ms(999);
    let under = r.open_within_budget();
    r.note_open_ms(1_001);
    set.add("dir open under one second", under && !r.open_within_budget(), "");

    // 7. .lnk 优先于同名直登记（开始菜单项多为 .lnk——先走映射面）。
    r.register("同名", "C:\\a\\b.exe");
    r.register_lnk("同名.lnk", "同名");
    set.add(
        "lnk takes precedence",
        matches!(r.resolve("同名.lnk"), OpenLoc::ViaLnk { .. }),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_split_no_backslash() {
        assert_eq!(dir_of("a.exe"), "");
        assert_eq!(file_of("a.exe"), "a.exe");
    }

    #[test]
    fn registry_cap_honest() {
        let mut r = LocResolver::new();
        for i in 0..32 {
            assert!(r.register(&alloc::format!("a{}", i), "C:\\x.exe"));
        }
        assert!(!r.register("溢出", "C:\\x.exe"));
    }

    #[test]
    fn lnk_cap_honest() {
        let mut r = LocResolver::new();
        for i in 0..32 {
            assert!(r.register_lnk(&alloc::format!("{}.lnk", i), "x"));
        }
        assert!(!r.register_lnk("溢出.lnk", "x"));
    }
}
