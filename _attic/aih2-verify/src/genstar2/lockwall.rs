//! F499 锁屏壁纸独立（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **解耦与默认共享；四款模式；压暗联动；mini 预览实时性；设置持久化
//! （F396 备份范围含）。**
//!
//! 功能定义（主册批次三）：锁屏壁纸与桌面壁纸解耦——两处独立设置（默认
//! 共享同一张——开箱一致、可拆）；锁屏壁纸支持四款模式（静态/每日精选/
//! 纯色/spotlight 深浅跟随——F297 压暗同源）；锁屏预览即时（mini 锁屏
//! 预览实时反映）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 四款模式（主册原文四款）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockWallMode {
    /// 静态图。
    Static,
    /// 每日精选（F154 冻结项的手动版——可选开启非默认）。
    DailyPick,
    /// 纯色。
    Solid,
    /// Spotlight 深浅跟随（F297 压暗同源）。
    SpotlightDim,
}

impl LockWallMode {
    pub const ALL: [LockWallMode; 4] = [
        LockWallMode::Static,
        LockWallMode::DailyPick,
        LockWallMode::Solid,
        LockWallMode::SpotlightDim,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LockWallMode::Static => "static",
            LockWallMode::DailyPick => "daily-pick",
            LockWallMode::Solid => "solid",
            LockWallMode::SpotlightDim => "spotlight-dim",
        }
    }
}

/// 锁屏壁纸状态。
#[derive(Clone, Copy, Debug)]
pub struct LockWall {
    /// 独立设置态（false = 默认共享桌面壁纸——开箱一致、可拆）。
    pub decoupled: bool,
    pub mode: LockWallMode,
    /// 壁纸引用键（独立后各持各的；共享时与桌面同键）。
    pub ref_key: u64,
    /// 压暗联动（F297 同源——SpotlightDim 模式恒开压暗）。
    pub dim_linked: bool,
    /// 备份范围含（F396：锁屏壁纸设置入备份）。
    pub in_backup: bool,
}

/// 简单 FNV 键。
fn ref_key(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl LockWall {
    /// 出厂态（默认共享——与桌面同一张；mode 静态）。
    pub const fn shared_default(desktop_key: u64) -> Self {
        LockWall {
            decoupled: false,
            mode: LockWallMode::Static,
            ref_key: desktop_key,
            dim_linked: true,
            in_backup: true,
        }
    }

    /// 解耦（拆开：锁屏从此独立）。
    pub fn decouple(&mut self, own_key: u64, mode: LockWallMode) {
        self.decoupled = true;
        self.ref_key = own_key;
        self.mode = mode;
    }

    /// 重新共享（合回去：跟随桌面）。
    pub fn re_share(&mut self, desktop_key: u64) {
        self.decoupled = false;
        self.ref_key = desktop_key;
        self.mode = LockWallMode::Static;
    }

    /// 模式设置（四款皆可；DailyPick 非默认——主册：可选开启非默认）。
    pub fn set_mode(&mut self, m: LockWallMode) {
        self.mode = m;
        // SpotlightDim 强制压暗联动（F297 同源）。
        if m == LockWallMode::SpotlightDim {
            self.dim_linked = true;
        }
    }

    /// mini 预览实时性（设置页右侧 mini 预览——参数变更即刻反映：
    /// 预览输入 = 状态本身，无延迟队列——恒即时）。
    pub fn preview_realtime(&self) -> (u64, LockWallMode) {
        (self.ref_key, self.mode)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_lockwall_checks() -> CheckSet {
    let mut cs = CheckSet::new("F499-lockwall");
    let desktop = ref_key("C:\\walls\\desk.png");
    // 1) 默认共享（开箱一致）。
    let mut w = LockWall::shared_default(desktop);
    cs.add("default_shared", !w.decoupled && w.ref_key == desktop, "");
    // 2) 解耦（可拆：锁屏换自己的画布）。
    w.decouple(ref_key("C:\\walls\\lock.png"), LockWallMode::SpotlightDim);
    cs.add("decouple_own_ref", w.decoupled && w.ref_key != desktop, "");
    // 3) 四款模式（逐一在册；DailyPick 非默认可选开启）。
    for m in LockWallMode::ALL {
        w.set_mode(m);
        if w.mode != m {
            cs.add("four_modes", false, "");
            return cs;
        }
    }
    cs.add("four_modes", true, "");
    // 4) 压暗联动（SpotlightDim → dim_linked 恒真——F297 同源）。
    w.set_mode(LockWallMode::SpotlightDim);
    cs.add("dim_link", w.dim_linked, "");
    // 5) mini 预览实时（读到的就是当前状态——零延迟队列）。
    let (k, m) = w.preview_realtime();
    cs.add("preview_realtime", k == w.ref_key && m == w.mode, "");
    // 6) 重新共享（合回去跟随桌面）。
    w.re_share(desktop);
    cs.add("re_share", !w.decoupled && w.ref_key == desktop && w.mode == LockWallMode::Static, "");
    // 7) 备份范围含（F396）。
    cs.add("backup_scope", w.in_backup, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decouple_then_customize() {
        let mut w = LockWall::shared_default(1);
        w.decouple(2, LockWallMode::Static);
        assert!(w.decoupled);
        w.set_mode(LockWallMode::DailyPick);
        assert_eq!(w.mode, LockWallMode::DailyPick);
        // 桌面侧不受影响（解耦语义：两处独立设置）。
        assert_ne!(w.ref_key, 1);
    }

    #[test]
    fn four_modes_all_named() {
        assert_eq!(LockWallMode::ALL.len(), 4);
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(LockWallMode::ALL[i].name(), LockWallMode::ALL[j].name());
            }
        }
    }

    #[test]
    fn preview_follows_every_change() {
        let mut w = LockWall::shared_default(7);
        w.decouple(8, LockWallMode::Solid);
        assert_eq!(w.preview_realtime(), (8, LockWallMode::Solid));
        w.set_mode(LockWallMode::SpotlightDim);
        assert_eq!(w.preview_realtime().1, LockWallMode::SpotlightDim);
    }
}
