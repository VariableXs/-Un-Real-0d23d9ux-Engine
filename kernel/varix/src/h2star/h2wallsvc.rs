//! H2 壁纸服务编排 · 深化批次四（F286 多屏设置 × F297 暗色压暗的
//! 服务化落位——配置解析、压暗参数、轮换调度、引用账）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F286 壁纸多屏设置**：三模式（All 同图 / Span 拼接 / Per 每屏
//!   各配）逐屏解析；轮换默认关（判据原文——调度器出厂禁用）；**引
//!   用不驻留**（配置只存路径引用，不复制文件——源删除的回退流程
//!   在引用账上可见）；
//! - **F297 壁纸暗色压暗**：压暗 30%±5% 与降饱和 15% 是「参数查表
//!   →逐屏应用」的服务——浅色壁纸不处理判据（亮度阈值上不启动）、
//!   滑杆 0-50 与 walldim 同源（本层只编排，参数机在 walldim）；
//! - **十二章**：源删除回退——引用账发现路径失效 → 回退默认壁纸
//!   并自首（用户看见哪屏换了、为什么）。
//!
//! 时间纪律：轮换调度以分钟戳注入；无时钟。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 多屏配置模型（F286 三模式）
// ---------------------------------------------------------------------------

/// 多屏模式三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiMode {
    /// 同一张图铺所有屏。
    All,
    /// 一张图跨屏拼接。
    Span,
    /// 每屏各配各的。
    Per,
}

/// 一屏的壁纸引用（引用不驻留——只存路径与展示参数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallRef {
    pub path: String,
    /// 填充方式（0 居中 / 1 拉伸 / 2 适配裁切——构图保护的载体）。
    pub fit: u8,
}

/// 多屏壁纸配置：模式 + 逐屏引用（All/Span 模式下引用表只有一项，
/// 逐屏生效——解析函数把两种形态归一成逐屏表）。
#[derive(Clone, Debug)]
pub struct WallConfig {
    pub mode: MultiMode,
    pub refs: Vec<WallRef>,
}

/// 配置解析：模式 → 逐屏引用表（屏数 `screens`）。
/// Per 模式屏数不足用最后一项兜底（配置短于屏数不炸——回退可解释）。
pub fn resolve(cfg: &WallConfig, screens: u8) -> Vec<WallRef> {
    let screens = screens.max(1);
    match cfg.mode {
        MultiMode::All | MultiMode::Span => {
            let primary = cfg.refs.first().cloned();
            (0..screens)
                .map(|_| primary.clone().unwrap_or(WallRef { path: String::new(), fit: 2 }))
                .collect()
        }
        MultiMode::Per => (0..screens)
            .map(|i| {
                cfg.refs
                    .get(i as usize)
                    .cloned()
                    .unwrap_or_else(|| cfg.refs.last().cloned().unwrap_or(WallRef { path: String::new(), fit: 2 }))
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// F297 压暗编排（参数机在 walldim——本层做逐屏应用决策）
// ---------------------------------------------------------------------------

/// 浅色阈值：壁纸平均亮度高于此（0-255）不启动压暗（判据「浅色不
/// 处理」——压暗是给亮壁纸在暗主题下减刺眼的，深壁纸再压就看不清）。
pub const LIGHT_LUMA_THRESHOLD: u8 = 170;

/// 逐屏压暗决策：亮度 + 暗主题开关 + 滑杆值 → 实际压暗量（0 = 不压）。
/// 决策表：暗主题关 → 0；亮度 ≤ 阈值 → 0；其余 → 滑杆值钳 0-50。
pub fn dim_for(dark_theme: bool, luma: u8, slider: u32) -> u32 {
    if !dark_theme || luma <= LIGHT_LUMA_THRESHOLD {
        return 0;
    }
    slider.min(50)
}

// ---------------------------------------------------------------------------
// 轮换调度（F286 轮换默认关）
// ---------------------------------------------------------------------------

/// 轮换配置：默认关（判据原文）——开启必须显式置 true。
#[derive(Clone, Copy, Debug)]
pub struct Rotation {
    pub enabled: bool,
    /// 间隔（分钟）。
    pub interval_min: u64,
    /// 上次轮换时刻。
    pub last_min: u64,
}

impl Rotation {
    pub const fn disabled() -> Rotation {
        Rotation { enabled: false, interval_min: 1440, last_min: 0 }
    }

    /// 到点判定：关着永不到点；开着按间隔（分钟戳差 ≥ interval）。
    pub fn due(&self, now_min: u64) -> bool {
        self.enabled && now_min.saturating_sub(self.last_min) >= self.interval_min
    }
}

// ---------------------------------------------------------------------------
// 引用账（源删除回退——十二章）
// ---------------------------------------------------------------------------

/// 引用失效回退结果。
#[derive(Debug, PartialEq, Eq)]
pub struct Fallback {
    /// 哪些屏失效了。
    pub screens: Vec<u8>,
    /// 回退动作说明（人话——三要素齐）。
    pub reason: &'static str,
}

/// 引用体检：给定文件存在性判定闭包，产出逐屏回退计划。
/// 闭包注入（不碰真实文件系统——编排层无 IO，宿主可测）。
pub fn audit_refs(
    refs: &[WallRef],
    exists: impl Fn(&str) -> bool,
) -> Option<Fallback> {
    let dead: Vec<u8> = refs
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.path.is_empty() && !exists(&r.path))
        .map(|(i, _)| i as u8)
        .collect();
    if dead.is_empty() {
        return None;
    }
    Some(Fallback { screens: dead, reason: "壁纸文件已不存在——该屏已回退默认壁纸，可在设置中重新选择" })
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2wallsvc_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2wallsvc");
    // 三模式解析：All 一图三屏同参；Per 逐屏各配；Per 缺项兜底末项。
    let all = WallConfig {
        mode: MultiMode::All,
        refs: vec![WallRef { path: "/w/晨.png".into(), fit: 2 }],
    };
    let r_all = resolve(&all, 3);
    set.add(
        "h2wallsvc all mode",
        r_all.len() == 3 && r_all.iter().all(|r| r.path == "/w/晨.png" && r.fit == 2),
        "one ref per screen",
    );
    let per = WallConfig {
        mode: MultiMode::Per,
        refs: vec![
            WallRef { path: "/w/左.png".into(), fit: 2 },
            WallRef { path: "/w/右.png".into(), fit: 0 },
        ],
    };
    let r_per = resolve(&per, 3);
    set.add(
        "h2wallsvc per mode",
        r_per[0].path == "/w/左.png" && r_per[1].path == "/w/右.png" && r_per[2].path == "/w/右.png",
        "short config falls back to last",
    );
    // 轮换默认关；开着按间隔到点。
    let rot = Rotation::disabled();
    set.add(
        "h2wallsvc rotation off default",
        !rot.due(0) && !rot.due(9_999_999),
        "出厂禁用",
    );
    let rot_on = Rotation { enabled: true, interval_min: 60, last_min: 100 };
    set.add(
        "h2wallsvc rotation due",
        !rot_on.due(159) && rot_on.due(160),
        "interval boundary",
    );
    // F297 决策表：暗主题关不压；浅色不压；其余按滑杆钳 0-50。
    set.add(
        "h2wallsvc dim decision",
        dim_for(false, 255, 30) == 0
            && dim_for(true, 255, 0) == 0
            && dim_for(true, LIGHT_LUMA_THRESHOLD, 30) == 0
            && dim_for(true, LIGHT_LUMA_THRESHOLD + 1, 30) == 30
            && dim_for(true, 200, 80) == 50,
        "light luma gate + clamp",
    );
    set.add(
        "h2wallsvc dim threshold const",
        LIGHT_LUMA_THRESHOLD == 170,
        "luma line",
    );
    // 引用体检：全在 → None；失效屏点名 + 人话原因。
    let refs = resolve(&per, 2);
    set.add(
        "h2wallsvc audit clean",
        audit_refs(&refs, |_| true).is_none(),
        "no false alarm",
    );
    let fb = audit_refs(&refs, |p| p != "/w/右.png").unwrap();
    set.add(
        "h2wallsvc fallback plan",
        fb.screens == vec![1] && fb.reason.contains("回退默认壁纸"),
        "dead screen named",
    );
    // Span 模式：单引用表也逐屏归一（拼接是渲染语义，配置层同 All）。
    let span = WallConfig { mode: MultiMode::Span, refs: all.refs.clone() };
    set.add(
        "h2wallsvc span normalize",
        resolve(&span, 2).len() == 2,
        "span resolves per screen",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2wallsvc_all_green() {
        let set = run_h2wallsvc_checks();
        assert!(set.all_passed(), "h2wallsvc 自检有红项");
        assert!(!set.truncated(), "h2wallsvc 自检溢出");
    }

    #[test]
    fn zero_screen_never_panics() {
        // 零屏（热拔瞬间）：解析退化为 1 屏——不炸不空表。
        let r = resolve(&WallConfig { mode: MultiMode::Per, refs: vec![] }, 0);
        assert_eq!(r.len(), 1);
    }
}
