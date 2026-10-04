//! F039 深化批次二 · 游戏识别与回程账面（compatstar2/deep · G-A-39）。
//!
//! 批次一深化覆盖 D3D 库词表/显存预估/独占全屏闸/交接参数打包；本批补齐：
//! exe 名关键词识别（大小写不敏感——「双击游戏」第一识别面）、全屏降级
//! 协商链（独占 → 无边框 → 窗口化——交接失败的预防链）、回程四步对称账
//! （Windows 侧一键切回——「回来路径对称」主册【设计细节】）、分辨率安全
//! 交集（游戏请求分辨率必须在显示器 EDID 档位表内——黑屏预防）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 回程四步（与切域四步对称——主册【设计细节】「回来路径对称」）。
pub const RETURN_STEPS: [&str; 4] = ["windows-flush", "gate", "boot-select", "varix-resume"];
/// 回程分段预算（与去程合计口径一致：8+3+4+15 = 30s）。
pub const RETURN_STEP_BUDGETS_S: [u64; 4] = [8, 3, 4, 15];
/// 游戏关键词表（exe 名识别；大小写不敏感匹配）。
pub const GAME_KEYWORDS: [&str; 4] = ["game", "play", "quest", "craft"];

/// ASCII 大小写不敏感包含（零分配：逐字节比对，不建小写副本）。
pub fn ascii_contains_ci(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    for start in 0..=h.len() - n.len() {
        let mut matched = true;
        for (k, &nc) in n.iter().enumerate() {
            let hc = h[start + k];
            let eq = hc == nc
                || (hc.is_ascii_uppercase() && hc + 32 == nc)
                || (hc.is_ascii_lowercase() && hc - 32 == nc);
            if !eq {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}

/// exe 名识别：命中任一关键词 → 判定为游戏（识别缓存未命中时的静态面）。
pub fn guess_is_game(exe_name: &str) -> bool {
    GAME_KEYWORDS.iter().any(|k| ascii_contains_ci(exe_name, k))
}

/// 全屏降级协商链：独占 → 无边框 → 窗口化（交接前逐级协商，
/// 每级返回下一建议态；到达窗口化或无边框成功即止）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScreenState {
    Exclusive,
    Borderless,
    Windowed,
}

pub fn downgrade_next(state: ScreenState) -> Option<ScreenState> {
    match state {
        ScreenState::Exclusive => Some(ScreenState::Borderless),
        ScreenState::Borderless => Some(ScreenState::Windowed),
        ScreenState::Windowed => None, // 已到最底，无更降级
    }
}

/// 回程推进账（对称四步 ≤30s 判据；结构镜像批次一 HandoffRun）。
pub struct ReturnRun {
    pub step: usize,
    pub elapsed_s: u64,
    pub success: bool,
}

impl ReturnRun {
    pub const fn new() -> Self {
        ReturnRun { step: 0, elapsed_s: 0, success: false }
    }
    pub fn advance(&mut self, step_seconds: u64) -> bool {
        self.elapsed_s += step_seconds;
        if self.elapsed_s > RETURN_STEP_BUDGETS_S.iter().sum::<u64>() {
            return false;
        }
        self.step += 1;
        if self.step == RETURN_STEPS.len() {
            self.success = true;
            return false;
        }
        true
    }
    pub fn verdict(&self) -> bool {
        self.success && self.step == 4 && self.elapsed_s <= 30
    }
}

/// 分辨率安全交集：游戏请求 (w,h) 必须在显示器档位表内（EDID 交集——
/// 切域前拦截黑屏分辨率的预防面）。
pub fn resolution_allowed(monitor_modes: &[(u32, u32)], req: (u32, u32)) -> bool {
    monitor_modes.iter().any(|&m| m == req)
}

/// 域自检（深化批次二）。
pub fn run_f039d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F039-gamefront-d2");
    // 1) exe 识别：大小写不敏感命中（MyGame.EXE / puzzle-quest）/非游戏名不误判。
    cs.add(
        "exe_keyword_ci",
        guess_is_game("MyGame.EXE") && guess_is_game("puzzle-quest.exe") && guess_is_game("PLAYER.exe") && !guess_is_game("notepad2.exe") && !guess_is_game("7zFM.exe"),
        "",
    );
    // 2) 全屏降级链：独占→无边框→窗口化→到底。
    cs.add(
        "fullscreen_downgrade_chain",
        downgrade_next(ScreenState::Exclusive) == Some(ScreenState::Borderless)
            && downgrade_next(ScreenState::Borderless) == Some(ScreenState::Windowed)
            && downgrade_next(ScreenState::Windowed).is_none(),
        "",
    );
    // 3) 回程四步：预算合计 30s、四步全走判据；超预算失败。
    let mut rr = ReturnRun::new();
    let mut alive = true;
    for (i, _) in RETURN_STEPS.iter().enumerate() {
        alive = rr.advance(RETURN_STEP_BUDGETS_S[i]);
    }
    let mut slow = ReturnRun::new();
    slow.advance(31);
    cs.add(
        "return_trip_symmetric",
        rr.verdict() && !alive && RETURN_STEP_BUDGETS_S.iter().sum::<u64>() == 30 && slow.elapsed_s > 30 && !slow.success,
        "",
    );
    // 4) 分辨率安全交集：EDID 表内过、表外拒（黑屏预防）。
    let modes = [(3840, 2160u32), (2560, 1440), (1920, 1080)];
    cs.add(
        "resolution_gate",
        resolution_allowed(&modes, (2560, 1440)) && !resolution_allowed(&modes, (1234, 567)),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_contains_boundaries() {
        assert!(ascii_contains_ci("Setup-GAME.exe", "game"), "中段命中");
        assert!(!ascii_contains_ci("gam", "game"), "hay 短于 needle → false");
        assert!(ascii_contains_ci("game.exe", "GAME"), "小写 hay 命中大写 needle");
    }

    #[test]
    fn return_step_names_symmetric() {
        // 回程四步与批次一去程四步（session-preserve/flush/gate/reboot）职责对称：
        // 冲刷与闸门两步成对（镜像对账面）。
        assert_eq!(RETURN_STEPS[1], "gate");
        assert_eq!(RETURN_STEPS[0], "windows-flush");
        assert_eq!(RETURN_STEPS.len(), 4);
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f039d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
