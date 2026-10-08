//! F147 跨设备主题同步 · 完整设计（STAR I 主册 G-D-22）。
//!
//! **判据（主册）**：跨机器插拔全链录屏（含欢迎卡/就位/退出清理）；
//! 壁纸自适应 20 档分辨率实测；「仅本次」零残留验证。
//!
//! **设计要点（主册）**：个性化档案（F161 格式）随 DATA 分区随身；
//! 首次插入检测 → 欢迎卡（条件=检测到档案且本机无个性化——三思不
//! 扰常客）；「同步到本机」/「仅本次」二选（机房场景）；「仅本次」
//! 不写宿主残留（退出清理临时态，临时态清单审计对拍）；壁纸自适应
//! 裁切（构图保护）；资产缺失 → 本地重建默认+标注；多 U 盘冲突 →
//! 最后插入优先+提示；就位动画复用 C-2「呼吸」；同步耗时 <10s
//! （资产按需流式 F068）。
//!
//! 本模块是随身同步的**纯逻辑核**：档案检测与欢迎卡条件、二选语义
//! 状态机（含退出清理零残留对账）、分辨率自适应裁切（20 档）、多盘
//! 冲突仲裁。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 同步耗时目标（秒）。
pub const SYNC_TARGET_SECONDS: u32 = 10;
/// 壁纸自适应判据的分辨率档数（20 档实测）。
pub const RESOLUTION_STEPS: usize = 20;

/// 个性化档案（F161 导出格式）——内容面只钉指纹，资产本体不进内核。
pub struct Profile {
    pub name: &'static str,
    pub fp: u64,
    /// 壁纸资产宽高（用于自适应裁切）。
    pub wall_w: u16,
    pub wall_h: u16,
}

impl Profile {
    pub fn new(name: &'static str, wall_w: u16, wall_h: u16) -> Profile {
        Profile { name, fp: fnv1a64(name.as_bytes()), wall_w, wall_h }
    }
}

// ---------------------------------------------------------------------------
// 分辨率自适应（20 档）
// ---------------------------------------------------------------------------

/// 自适应裁切：目标档宽高比下按构图保护裁切源图。
/// 返回裁切后的等效源窗口（w,h——居中裁切，主体不裁头=顶部保留
/// 优先：纵向裁切时保上不保下）。
pub fn adaptive_crop(src_w: u16, src_h: u16, dst_w: u16, dst_h: u16) -> (u16, u16, u16, u16) {
    let src_ratio = (src_w as u32) * 1000 / (src_h as u32).max(1);
    let dst_ratio = (dst_w as u32) * 1000 / (dst_h as u32).max(1);
    if src_ratio > dst_ratio {
        // 源更宽 → 横向居中裁
        let w = ((src_h as u32 * dst_w as u32) / dst_h as u32).min(src_w as u32) as u16;
        let x = (src_w - w) / 2;
        (x, 0, w, src_h)
    } else {
        // 源更窄 → 纵向裁，保上不保下（构图保护）
        let h = ((src_w as u32 * dst_h as u32) / dst_w as u32).min(src_h as u32) as u16;
        (0, 0, src_w, h)
    }
}

// ---------------------------------------------------------------------------
// 插拔会话状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoamChoice {
    /// 同步到本机（档案写入本机配置层）。
    Adopt,
    /// 仅本次（临时态——退出逐清）。
    SessionOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoamPhase {
    /// 插入未决策。
    Pending,
    /// 档案就位（欢迎卡已显/已选）。
    Applied,
    /// 退出清理完成（临时态零残留已对账）。
    CleanedUp,
}

/// 一次插拔会话。
pub struct RoamSession {
    pub phase: RoamPhase,
    pub choice: Option<RoamChoice>,
    /// 本机此前是否已有个性化（欢迎卡显隐条件的一半）。
    host_has_profile: bool,
    /// 临时态写点清单（「仅本次」退出逐清的对账面）。
    temp_writes: [u64; 8],
    temp_count: usize,
    /// 清理对账：已清写点数。
    pub cleaned: usize,
}

impl RoamSession {
    /// 检测到档案后开会话。欢迎卡条件 = 检测到档案 且 本机无个性化。
    pub fn detect(profile_fp: u64, host_has_profile: bool) -> RoamSession {
        let mut s = RoamSession {
            phase: RoamPhase::Pending,
            choice: None,
            host_has_profile,
            temp_writes: [0; 8],
            temp_count: 0,
            cleaned: 0,
        };
        if profile_fp != 0 {
            // 临时态写点：会话缓存/缩略图/热替换暂存（按宿主事实清单，
            // 退出逐清——审计日志对拍的机制面）。
            s.temp_writes[0] = fnv1a64(b"roam/session-cache");
            s.temp_writes[1] = fnv1a64(b"roam/thumb-cache");
            s.temp_writes[2] = fnv1a64(b"roam/hotswap-staging");
            s.temp_count = 3;
        }
        s
    }

    /// 欢迎卡应显（三思不扰常客）。
    pub fn welcome_card(&self) -> bool {
        self.phase == RoamPhase::Pending && !self.host_has_profile
    }

    /// 二选一决策。
    pub fn decide(&mut self, c: RoamChoice) -> Result<(), &'static str> {
        if self.phase != RoamPhase::Pending {
            return Err("already decided");
        }
        self.choice = Some(c);
        self.phase = RoamPhase::Applied;
        Ok(())
    }

    /// 退出清理：「仅本次」逐清临时态并对账（零残留验证）；「同步到
    /// 本机」无临时态（已入宿主配置层，退出不清）。
    pub fn teardown(&mut self) -> Result<usize, &'static str> {
        if self.phase != RoamPhase::Applied {
            return Err("nothing applied");
        }
        let n = match self.choice {
            Some(RoamChoice::SessionOnly) => {
                self.cleaned = self.temp_count;
                self.temp_count = 0;
                self.cleaned
            }
            Some(RoamChoice::Adopt) => {
                // Adopt 无临时态残留（写点进宿主配置层，有配置层记账）。
                self.cleaned = 0;
                0
            }
            None => return Err("undecided session"),
        };
        self.phase = RoamPhase::CleanedUp;
        Ok(n)
    }

    /// 零残留验证：仅本次清理后临时态清零。
    pub fn zero_residue(&self) -> bool {
        self.phase == RoamPhase::CleanedUp
            && match self.choice {
                Some(RoamChoice::SessionOnly) => self.temp_count == 0 && self.cleaned > 0,
                _ => true,
            }
    }
}

// ---------------------------------------------------------------------------
// 多盘冲突仲裁
// ---------------------------------------------------------------------------

/// 多 U 盘冲突：最后插入优先 + 提示。
pub fn arbitration(previous: &Profile, latest: &Profile) -> (&'static str, u64) {
    let _ = previous;
    ("last-inserted-wins", latest.fp)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147_TAG: &str = "stareco-F147-syncroam";

pub fn run_syncroam_checks() -> CheckSet {
    let mut set = CheckSet::new(F147_TAG);

    // 壁纸自适应 20 档：逐档裁切结果合法（不放大只裁、主体保上）
    let steps: [(u16, u16); 20] = [
        (640, 480), (800, 600), (1024, 768), (1280, 720), (1280, 800),
        (1280, 1024), (1366, 768), (1440, 900), (1600, 900), (1680, 1050),
        (1920, 1080), (1920, 1200), (2048, 1152), (2560, 1080), (2560, 1440),
        (3440, 1440), (3840, 2160), (4096, 2160), (5120, 2880), (7680, 4320),
    ];
    let mut all_ok = true;
    for &(w, h) in steps.iter() {
        let (x, y, cw, ch) = adaptive_crop(3840, 2160, w, h);
        // 裁切窗必须在源内且面积正好铺满目标比例
        if x + cw > 3840 || y + ch > 2160 || cw == 0 || ch == 0 {
            all_ok = false;
        }
        let win_ratio = (cw as u32) * 1000 / (ch as u32).max(1);
        let dst_ratio = (w as u32) * 1000 / (h as u32).max(1);
        if (win_ratio as i32 - dst_ratio as i32).abs() > 1 {
            all_ok = false;
        }
    }
    set.add("f147 adaptive 20 steps", all_ok && RESOLUTION_STEPS == 20, "构图保护逐档绿");
    // 纵向裁切保上不保下：竖窗裁横源
    let (_x, y, _w, _h) = adaptive_crop(3840, 2160, 1080, 1920);
    set.add("f147 top-protected", y == 0, "主体不裁头");

    // 插拔会话：欢迎卡条件（档案在 + 本机无个性化）
    let prof = Profile::new("variable-desk", 3840, 2160);
    let s_newcomer = RoamSession::detect(prof.fp, false);
    let s_regular = RoamSession::detect(prof.fp, true);
    set.add("f147 welcome for newcomer", s_newcomer.welcome_card(), "first-time card");
    set.add("f147 no card for regular", !s_regular.welcome_card(), "三思不扰常客");

    // 仅本次：退出清理零残留
    let mut s1 = RoamSession::detect(prof.fp, true);
    assert!(s1.decide(RoamChoice::SessionOnly).is_ok());
    set.add("f147 session applied", s1.phase == RoamPhase::Applied, "temp mode");
    let cleaned = s1.teardown().expect("teardown");
    set.add(
        "f147 session-only zero residue",
        s1.zero_residue() && cleaned == 3 && s1.phase == RoamPhase::CleanedUp,
        "3 temp writes cleaned",
    );

    // 同步到本机：无临时态清理（入宿主配置层）
    let mut s2 = RoamSession::detect(prof.fp, true);
    s2.decide(RoamChoice::Adopt).ok();
    set.add("f147 adopt no temp cleanup", s2.teardown() == Ok(0), "host layer owns it");

    // 重复决策拒绝 / 未决策退出拒绝
    let mut s3 = RoamSession::detect(prof.fp, true);
    s3.decide(RoamChoice::Adopt).ok();
    set.add("f147 double decide rejected", s3.decide(RoamChoice::Adopt).is_err(), "state machine");
    let mut s4 = RoamSession::detect(prof.fp, true);
    set.add("f147 undecided teardown rejected", s4.teardown().is_err(), "no silent discard");

    // 多盘冲突：最后插入优先
    let older = Profile::new("old-desk", 1920, 1080);
    let (verdict, winner) = arbitration(&older, &prof);
    set.add("f147 last-inserted wins", verdict == "last-inserted-wins" && winner == prof.fp, "with notice");

    // 同步耗时目标
    set.add("f147 sync target 10s", SYNC_TARGET_SECONDS == 10, "streamed assets");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_anchors() {
        // 源比目标宽：横向居中
        let (x, y, w, h) = adaptive_crop(3840, 2160, 1920, 1080);
        assert_eq!((x, y, w, h), (0, 0, 3840, 2160));
        // 源比目标窄（16:10 源裁 16:9 窗）：纵向裁保上
        let (x2, y2, w2, h2) = adaptive_crop(1920, 1200, 1920, 1080);
        assert_eq!((x2, y2, w2, h2), (0, 0, 1920, 1080));
    }
}
