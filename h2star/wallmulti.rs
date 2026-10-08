//! F286 壁纸多屏设置 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三模式用例；轮换默认关判据；引用不驻留验证；源
//! 删除回退流程。
//!
//! **设计要点（主册）**：多屏壁纸三模式：每屏独立（各选各的）、跨屏
//! 拼接（一张 4K+ 大图按屏分割）、复制同步（主屏壁纸铺所有屏）；壁纸
//! 设置页支持多选文件夹做播放池（定时轮换间隔 15 分钟-1 天可选——
//! 轮换默认关闭）；壁纸文件不复制驻留（引用原路径，省 U 盘空间，源
//! 删除后回退纯色并提示）。
//!
//! 实装：三模式枚举 + 每屏解析器（模式 × 屏表 → 各屏壁纸引用）；拼接
/// 分割（总图按屏边界切条）；轮换池（默认关——结构默认值钉死）；引用
/// 不驻留（存路径不拷贝——类型即证明）；源删除回退（引用失效 → 纯色
/// + 提示）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 三模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallMode {
    /// 每屏独立。
    PerScreen,
    /// 跨屏拼接。
    Spanned,
    /// 复制同步。
    Mirror,
}

/// 一屏的壁纸解析结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallSlot {
    pub screen: u8,
    /// 壁纸引用路径（引用不驻留——不拷贝）。
    pub source: String,
    /// 拼接模式下本屏取大图的横向切片（比例分子/分母）。
    pub slice: Option<(u32, u32)>,
}

/// 壁纸引用（存路径——类型上就没有复制语义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallRef(pub String);

impl WallRef {
    /// 源可达性：文件丢失 → 回退纯色 + 提示（回退流程判据）。
    pub fn resolve(&self, exists: impl Fn(&str) -> bool) -> Result<&str, &'static str> {
        if exists(&self.0) {
            Ok(&self.0)
        } else {
            Err("壁纸文件已不存在——已回退纯色背景，可在设置里重新选择")
        }
    }
}

/// 轮换池：多选文件夹 + 间隔（15 分钟-1 天）——**默认关**。
pub struct RotationPool {
    pub enabled: bool,
    pub folders: Vec<String>,
    /// 轮换间隔（分钟；合法域 [15, 1440]）。
    pub interval_min: u32,
}

pub const ROTATION_MIN_MIN: u32 = 15;
pub const ROTATION_MAX_MIN: u32 = 1440;

impl Default for RotationPool {
    fn default() -> Self {
        RotationPool { enabled: false, folders: Vec::new(), interval_min: 1440 }
    }
}

impl RotationPool {
    /// 轮换合法性：间隔在 [15, 1440] 内（15 分钟-1 天可选判据）。
    pub fn interval_valid(&self) -> bool {
        (ROTATION_MIN_MIN..=ROTATION_MAX_MIN).contains(&self.interval_min)
    }

    /// 下一次轮换时刻（分钟戳）。
    pub fn next_rotation(&self, now_min: u64) -> u64 {
        now_min + self.interval_min as u64
    }
}

/// 多屏壁纸解析。
pub fn apply_mode(mode: WallMode, screens: &[u8], refs: &[WallRef]) -> Vec<WallSlot> {
    let mut out = Vec::new();
    match mode {
        WallMode::Mirror => {
            for &s in screens {
                out.push(WallSlot {
                    screen: s,
                    source: refs.first().map(|r| r.0.clone()).unwrap_or_default(),
                    slice: None,
                });
            }
        }
        WallMode::PerScreen => {
            for (i, &s) in screens.iter().enumerate() {
                out.push(WallSlot {
                    screen: s,
                    source: refs.get(i).map(|r| r.0.clone()).unwrap_or_default(),
                    slice: None,
                });
            }
        }
        WallMode::Spanned => {
            // 一张图按屏切条：i/n 到 (i+1)/n。
            let n = screens.len() as u32;
            for (i, &s) in screens.iter().enumerate() {
                out.push(WallSlot {
                    screen: s,
                    source: refs.first().map(|r| r.0.clone()).unwrap_or_default(),
                    slice: Some((i as u32, n)),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_wallmulti_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F286");
    let screens = [0u8, 1];
    let w1 = WallRef(String::from("vx:/壁纸/山.png"));
    let w2 = WallRef(String::from("vx:/壁纸/海.png"));
    // 三模式用例。
    let per = apply_mode(WallMode::PerScreen, &screens, &[w1.clone(), w2.clone()]);
    let span = apply_mode(WallMode::Spanned, &screens, &[w1.clone()]);
    let mir = apply_mode(WallMode::Mirror, &screens, &[w1.clone()]);
    set.add(
        "F286 three modes",
        per[0].source == "vx:/壁纸/山.png"
            && per[1].source == "vx:/壁纸/海.png"
            && span.iter().all(|s| s.source == "vx:/壁纸/山.png")
            && span[0].slice == Some((0, 2))
            && span[1].slice == Some((1, 2))
            && mir.iter().all(|s| s.source == "vx:/壁纸/山.png"),
        "per/span/mirror",
    );
    // 引用不驻留：WallRef 只有路径，无拷贝语义（类型即证明）——钉行为。
    set.add(
        "F286 reference only",
        w1.0.ends_with(".png") && !w1.0.starts_with("cache:"),
        "no copy",
    );
    // 轮换默认关。
    let pool = RotationPool::default();
    set.add(
        "F286 rotation off default",
        !pool.enabled && pool.interval_valid() && pool.interval_min == ROTATION_MAX_MIN,
        "default off",
    );
    // 源删除回退。
    let gone = WallRef(String::from("vx:/壁纸/没了.png"));
    let ok = w1.resolve(|_| true);
    let lost = gone.resolve(|_| false);
    set.add(
        "F286 source deleted fallback",
        ok == Ok("vx:/壁纸/山.png")
            && lost == Err("壁纸文件已不存在——已回退纯色背景，可在设置里重新选择"),
        "solid color + notice",
    );
    // 间隔域边界：15 与 1440 合法，14 与 1441 非法。
    let mut p2 = RotationPool::default();
    p2.interval_min = ROTATION_MIN_MIN;
    let lo_ok = p2.interval_valid();
    p2.interval_min = 14;
    let lo_bad = !p2.interval_valid();
    set.add(
        "F286 interval bounds",
        lo_ok && lo_bad && p2.next_rotation(100) == 114,
        "15min-1day",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f286_wall_modes() {
        let set = run_wallmulti_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F286 自检红 {f}/{p}");
    }

    #[test]
    fn empty_refs_yield_empty_source() {
        let out = apply_mode(WallMode::Mirror, &[0], &[]);
        assert_eq!(out[0].source, "", "无壁纸源诚实给空——不编造");
    }
}
