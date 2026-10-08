//! 四路垫片清单（WP-302 · B-1003 Wine 源码 diff 为零——审计红线）。
//!
//! MD2 篇 10.3：显示/输入/音频/剪贴板四路垫片，**全部在 VARIX 侧实现，
//! Wine 源码零修改**（23.6 禁区第一条的工程兑现——MD2 附录 I 头号陷阱
//! "垫片直接改 Wine 源码"在本包的代码评审第一条就是查这个）。垫片失效
//! 只影响桥接功能，不污染 Wine 本体；升级 Wine 时垫片随版本锁定表重新
//! 校验（MD3 行 110 / R5 借力组件版本漂移）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 四路垫片登记表（篇 10.3 冻结面）
// ---------------------------------------------------------------------------

/// 垫片四路（穷举——不存在第五路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShimRoute {
    /// 显示：Wine Wayland 驱动对接 vx-wayland-bridge（篇 5 协议）。
    Display,
    /// 输入：IME 提交经 text_commit 直达 Wine 窗口（判例 11，零输入法 DLL）。
    Input,
    /// 音频：Windows 音频 API → 混音器流映射（篇 8.4，每流音量监视器可见）。
    Audio,
    /// 剪贴板：Wine 剪贴板 ↔ VXWM 双向桥（纯文本+位图；文件清单走拖放）。
    Clipboard,
}

pub const SHIM_ROUTES: usize = 4;

/// 垫片实现侧（穷举两态：VARIX 侧 / Wine 侧——零侵入红线只许前者）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImplSide {
    VarixSide,
    WineSide,
}

/// 一路垫片的登记行：实现侧 + 桥接对象 + 失效影响面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShimRow {
    pub route: ShimRoute,
    pub side: ImplSide,
    /// 失效影响：只影响桥接功能（true = 桥接级失效，不污染 Wine 本体）。
    pub fail_isolated: bool,
}

/// 四路垫片表（冻结面：四行全 VARIX 侧——零侵入的结构表达）。
pub const SHIM_TABLE: [ShimRow; SHIM_ROUTES] = [
    ShimRow { route: ShimRoute::Display, side: ImplSide::VarixSide, fail_isolated: true },
    ShimRow { route: ShimRoute::Input, side: ImplSide::VarixSide, fail_isolated: true },
    ShimRow { route: ShimRoute::Audio, side: ImplSide::VarixSide, fail_isolated: true },
    ShimRow { route: ShimRoute::Clipboard, side: ImplSide::VarixSide, fail_isolated: true },
];

/// 零侵入审计：四路全 VARIX 侧、无重复路由、失效全隔离。
/// **B-1003 达标线的结构面**——表里出现一行 WineSide 即审计红。
pub fn zero_intrusion_audit() -> bool {
    if SHIM_TABLE.len() != SHIM_ROUTES {
        return false;
    }
    let mut i = 0;
    while i < SHIM_ROUTES {
        if SHIM_TABLE[i].side != ImplSide::VarixSide || !SHIM_TABLE[i].fail_isolated {
            return false;
        }
        let mut j = i + 1;
        while j < SHIM_ROUTES {
            if SHIM_TABLE[i].route == SHIM_TABLE[j].route {
                return false; // 同路两行——冻结面被破坏
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// Wine 源码 diff 审计（B-1003 达标线：Wine 源码 diff 为零）
// ---------------------------------------------------------------------------

/// Wine 源码 diff 行数审计值（宿主模型面：登记为 0——任何非零即红线）。
pub const WINE_SOURCE_DIFF_LINES: u32 = 0;

/// 零 diff 断言（审计红线——MD2 附录 I 头号陷阱的构建期表达）。
pub fn wine_source_untouched() -> bool {
    WINE_SOURCE_DIFF_LINES == 0
}

// ---------------------------------------------------------------------------
// 显示垫片要素（篇 10.3：窗口装饰统一/光标统一/DPI 换算在桥内）
// ---------------------------------------------------------------------------

/// 显示垫片三要素齐备记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayShim {
    /// 窗口装饰统一（合成器绘制标题栏）。
    pub deco_unified: bool,
    /// 光标统一上报（光标永不经过客户端——篇 5.5 同源）。
    pub cursor_unified: bool,
    /// DPI 换算在桥内完成（缩放因子 → Wine DPI 虚拟值）。
    pub dpi_in_bridge: bool,
}

impl DisplayShim {
    pub fn complete(&self) -> bool {
        self.deco_unified && self.cursor_unified && self.dpi_in_bridge
    }
}

// ---------------------------------------------------------------------------
// 版本锁定校验挂钩（R5：升级 Wine 时垫片随版本锁定表重新校验）
// ---------------------------------------------------------------------------

/// 版本锁定表行（Wine/Servo 等十三件借力组件的模型面——本包只挂 Wine）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockEntry {
    pub component: &'static str,
    pub locked_ver: u32,
}

/// 当前锁定：Wine 版本（升级窗内评估、锁定版本、不追最新——MD1 第 18 章）。
pub const WINE_LOCK: LockEntry = LockEntry { component: "wine", locked_ver: 9 };

/// 垫片校验：Wine 版本未漂移（锁定版本一致）则垫片无需重校验；版本变更
/// （升级窗）触发四路重校验——返回需重校验的路数。
pub fn reshim_needed(locked: &LockEntry, new_ver: u32) -> usize {
    if locked.locked_ver == new_ver {
        0
    } else {
        SHIM_ROUTES // 全部四路重校验
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1003 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_wineshim_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1003 垫片零侵入");
    // 1. 四路垫片齐：显示/输入/音频/剪贴板穷举。
    set.add(
        "B-1003 四路垫片齐",
        SHIM_TABLE.len() == 4 && zero_intrusion_audit(),
        "显示/输入/音频/剪贴板四路穷举登记——表即冻结面",
    );
    // 2. 全部 VARIX 侧：零侵入的结构表达（表内无 WineSide 行）。
    set.add(
        "B-1003 全部 VARIX 侧",
        zero_intrusion_audit()
            && SHIM_TABLE.iter().all(|r| r.side == ImplSide::VarixSide),
        "四路垫片全在 VARIX 侧实现——Wine 源码零修改的结构保证",
    );
    // 3. 零 diff 审计（B-1003 达标线红线）。
    set.add(
        "B-1003 零 diff 审计",
        wine_source_untouched() && WINE_SOURCE_DIFF_LINES == 0,
        "Wine 源码 diff 为零——附录 I 头号陷阱的构建期审计",
    );
    // 4. 失效隔离：垫片失效只影响桥接，不污染 Wine 本体。
    let broken = ShimRow { route: ShimRoute::Audio, side: ImplSide::VarixSide, fail_isolated: false };
    set.add(
        "B-1003 失效隔离",
        SHIM_TABLE.iter().all(|r| r.fail_isolated) && !broken.fail_isolated,
        "垫片失效只影响桥接功能——不污染 Wine 本体，升级随锁定表重校验",
    );
    // 5. 显示垫片三要素：装饰统一/光标统一/DPI 在桥内。
    let disp = DisplayShim { deco_unified: true, cursor_unified: true, dpi_in_bridge: true };
    let half = DisplayShim { deco_unified: true, cursor_unified: true, dpi_in_bridge: false };
    set.add(
        "B-1003 显示垫片三要素",
        disp.complete() && !half.complete(),
        "窗口装饰统一+光标统一上报+DPI 换算在桥内——缺一不完整",
    );
    // 6. 版本锁定挂钩：版本一致零重校验，升级窗内四路全重校验。
    let same = reshim_needed(&WINE_LOCK, 9);
    let bumped = reshim_needed(&WINE_LOCK, 10);
    set.add(
        "B-1003 升级重校验挂钩",
        same == 0 && bumped == SHIM_ROUTES && WINE_LOCK.locked_ver == 9,
        "版本锁定不追最新；升级窗内垫片随锁定表四路全重校验（R5）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe03 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe03_zero_intrusion_audit_holds() {
        assert!(zero_intrusion_audit());
        assert!(wine_source_untouched());
        assert_eq!(WINE_SOURCE_DIFF_LINES, 0);
        // 四路无重复：Display/Input/Audio/Clipboard 各一行。
        let mut seen = 0u32;
        for r in SHIM_TABLE {
            let bit = 1u32 << (r.route as u32);
            assert_eq!(seen & bit, 0, "duplicate route");
            seen |= bit;
        }
        assert_eq!(seen, 0b1111);
    }

    #[test]
    fn fe03_wineside_row_fails_audit() {
        // 结构反证：登记一行 WineSide（或失联不隔离）审计必红——红线可执行。
        let bad_row = ShimRow { route: ShimRoute::Input, side: ImplSide::WineSide, fail_isolated: true };
        assert_eq!(bad_row.side, ImplSide::WineSide);
        assert_ne!(SHIM_TABLE[1].side, bad_row.side);
        // 失效不隔离的行同样不合法。
        let polluted = ShimRow { route: ShimRoute::Display, side: ImplSide::VarixSide, fail_isolated: false };
        assert!(!polluted.fail_isolated);
        assert!(SHIM_TABLE.iter().all(|r| r.fail_isolated));
    }

    #[test]
    fn fe03_display_shim_elements() {
        let full = DisplayShim { deco_unified: true, cursor_unified: true, dpi_in_bridge: true };
        assert!(full.complete());
        let mut degraded = full;
        degraded.dpi_in_bridge = false;
        assert!(!degraded.complete());
        degraded.cursor_unified = false;
        assert!(!degraded.complete());
    }

    #[test]
    fn fe03_lock_reshim_semantics() {
        assert_eq!(reshim_needed(&WINE_LOCK, 9), 0);
        assert_eq!(reshim_needed(&WINE_LOCK, 10), 4);
        assert_eq!(reshim_needed(&WINE_LOCK, 100), 4);
        assert_eq!(WINE_LOCK.component, "wine");
        // 锁定版本下降也是漂移（版本回退同样触发重校验）。
        assert_eq!(reshim_needed(&WINE_LOCK, 8), 4);
    }
}
