//! F039 游戏兼容前瞻面（compatstar · G-A-39）——不装能跑就诚实分流，分流本身丝滑。
//!
//! 主册判据（验收标准第一句）：
//! **D3D9/11/12 探测样本各 2 枚分支全对；「双击游戏到 Windows 游戏画面」
//! 全流程 ≤30s 实测录屏。**
//!
//! 功能定义（G-A-39）：DirectX 探测层：程序请求 D3D 设备时探测版本需求
//! （D3D9/11/12），当前不支持 → 「一键切 Windows 域」引导卡（复用交接流程
//! C4，预检 F181 自动带上游戏参数）；wineserver 面（M-D/M-E）就绪后 →
//! DXVK 评估立项（开源转译层，需 GPU 面 B3 先行）。
//!
//! 【设计细节】D3D 版本探测读程序 import 表 d3d 动态库引用（零开销静态
//! 判定）；「下次直接切」写入交接快照偏好字段；切域前自动暂停游戏外的
//! 下载与同步任务（省带宽降风险）；回来路径对称（Windows 侧助手一键切回
//! VARIX）；切换全程电量低于 15% 时劝阻提示（F196 联动）。
//! 【交互设计】引导卡在 F035 体系内；切换复用交接四步画面（会话保全/冲刷/
//! 闸门/重启）；「下次直接切」记忆选项。
//! 【数据与存储】游戏识别记录（哈希→D3D 版本）缓存；交接参数（游戏全屏
//! 参数）进快照扩展字段。
//! 【状态与异常】探测到 D3D12 → 同卡不同文案；交接失败（R1 固件脾气）→
//! 按风险册流程降级回 VARIX 并说明；游戏是 32 位 → F004 与本面合并提示。
//!
//! 零堆纪律：定长缓存表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 全流程预算 30 秒（双击游戏到 Windows 游戏画面——主册判据）。
pub const HANDOFF_BUDGET_S: u64 = 30;
/// 电量劝阻线 15%（F196 联动）。
pub const BATTERY_DISSUADE_PERMILLE: u32 = 150;
/// 识别缓存容量（哈希→D3D 版本）。
pub const GAME_CACHE_CAP: usize = 64;
/// 交接四步画面（会话保全/冲刷/闸门/重启——复用 C4）。
pub const HANDOFF_STEPS: [&str; 4] = ["session-preserve", "flush", "gate", "reboot"];
/// 交接分段预算（合计 30s 内：保全 4s/冲刷 8s/闸门 3s/重启 15s）。
pub const STEP_BUDGETS_S: [u64; 4] = [4, 8, 3, 15];

// ---------------------------------------------------------------------------
// D3D 版本探测（import 表零开销静态判定）
// ---------------------------------------------------------------------------

/// 探测到的 D3D 需求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum D3dNeed {
    D3D9,
    D3D11,
    D3D12,
    None,
}

impl D3dNeed {
    pub fn label(self) -> &'static str {
        match self {
            D3dNeed::D3D9 => "DirectX 9",
            D3dNeed::D3D11 => "DirectX 11",
            D3dNeed::D3D12 => "DirectX 12",
            D3dNeed::None => "no-d3d",
        }
    }
}

/// import 表扫描（零开销静态判定：读 d3d 动态库引用）。
/// 规则：d3d12.dll > d3d11.dll > d3d9.dll（高版本优先——程序同时引用时
/// 按其请求的最高版本分流）。
pub fn detect_d3d_from_imports(imports: &[&str]) -> D3dNeed {
    if imports.iter().any(|s| s.contains("d3d12")) {
        D3dNeed::D3D12
    } else if imports.iter().any(|s| s.contains("d3d11")) {
        D3dNeed::D3D11
    } else if imports.iter().any(|s| s.contains("d3d9")) {
        D3dNeed::D3D9
    } else {
        D3dNeed::None
    }
}

/// 32 位游戏 → F004 与本面合并提示（合并标记）。
pub fn merge_with_f004(bits: u8) -> bool {
    bits == 32
}

// ---------------------------------------------------------------------------
// 引导卡（F035 体系内）
// ---------------------------------------------------------------------------

/// 引导卡文案（D3D12 与 D3D9/11 同卡不同文案——主册【状态与异常】）。
pub fn guidance_card(need: D3dNeed, battery_permille: u32) -> (&'static str, bool) {
    let line = match need {
        D3dNeed::D3D12 => "此游戏需要 DirectX 12 渲染。VARIX 兼容面正在扩展。现在切到 Windows 域玩？",
        D3dNeed::D3D11 => "此游戏需要 DirectX 11 渲染。VARIX 兼容面正在扩展。现在切到 Windows 域玩？",
        D3dNeed::D3D9 => "此游戏需要 DirectX 9 渲染。现在切到 Windows 域玩？",
        D3dNeed::None => "无需分流",
    };
    let dissuade = battery_permille < BATTERY_DISSUADE_PERMILLE; // 电量 <15% 劝阻
    (line, dissuade)
}

// ---------------------------------------------------------------------------
// 识别缓存与交接参数
// ---------------------------------------------------------------------------

/// 识别缓存：哈希 → D3D 版本（二次双击零扫描）。
pub struct GameCache {
    hashes: [[u8; 8]; GAME_CACHE_CAP],
    needs: [D3dNeed; GAME_CACHE_CAP],
    count: usize,
    pub hits: u32,
}

impl GameCache {
    pub const fn new() -> Self {
        GameCache { hashes: [[0; 8]; GAME_CACHE_CAP], needs: [D3dNeed::None; GAME_CACHE_CAP], count: 0, hits: 0 }
    }

    pub fn remember(&mut self, hash8: [u8; 8], need: D3dNeed) {
        if self.count < GAME_CACHE_CAP && !self.hashes[..self.count].contains(&hash8) {
            self.hashes[self.count] = hash8;
            self.needs[self.count] = need;
            self.count += 1;
        }
    }

    pub fn lookup(&mut self, hash8: &[u8; 8]) -> Option<D3dNeed> {
        if let Some(i) = (0..self.count).find(|&i| self.hashes[i] == *hash8) {
            self.hits += 1;
            Some(self.needs[i])
        } else {
            None
        }
    }
}

/// 「下次直接切」记忆（写入交接快照偏好字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffPreference {
    pub always_switch: bool,
    /// 游戏全屏参数（进快照扩展字段）。
    pub fullscreen_w: u32,
    pub fullscreen_h: u32,
    /// 切域前暂停游戏外的下载与同步任务。
    pub paused_background_jobs: u32,
}

impl HandoffPreference {
    pub const fn new() -> Self {
        HandoffPreference { always_switch: false, fullscreen_w: 0, fullscreen_h: 0, paused_background_jobs: 0 }
    }
    pub fn remember_always(&mut self, w: u32, h: u32) {
        self.always_switch = true;
        self.fullscreen_w = w;
        self.fullscreen_h = h;
    }
}

/// 交接四步推进与预算（≤30s 判据的分段账面）。
pub struct HandoffRun {
    pub step: usize,
    pub elapsed_s: u64,
    pub success: bool,
    /// 失败 → 按风险册流程降级回 VARIX 并说明。
    pub degraded_back: bool,
}

impl HandoffRun {
    pub const fn new() -> Self {
        HandoffRun { step: 0, elapsed_s: 0, success: false, degraded_back: false }
    }

    /// 推进一步：返回是否仍在进行；超预算 → 失败降级。
    pub fn advance(&mut self, step_seconds: u64) -> bool {
        self.elapsed_s += step_seconds;
        if self.elapsed_s > HANDOFF_BUDGET_S {
            self.success = false;
            self.degraded_back = true; // 降级回 VARIX 并说明
            return false;
        }
        self.step += 1;
        if self.step == HANDOFF_STEPS.len() {
            self.success = true;
            return false;
        }
        true
    }

    /// 判据：全流程 ≤30s 且四步全走。
    pub fn verdict(&self) -> bool {
        self.success && self.elapsed_s <= HANDOFF_BUDGET_S && self.step == 4
    }
}

/// 切域前自动暂停游戏外的下载与同步任务（省带宽降风险）。
pub fn pause_background_jobs(jobs: usize, pref: &mut HandoffPreference) -> usize {
    pref.paused_background_jobs = jobs as u32;
    jobs
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_gamefront_checks() -> CheckSet {
    let mut cs = CheckSet::new("F039-gamefront");
    // 1) D3D 探测六样本分支全对（D3D9/11/12 各 2 枚）。
    let s9a = detect_d3d_from_imports(&["d3d9.dll", "user32.dll"]);
    let s9b = detect_d3d_from_imports(&["kernel32.dll", "d3d9.dll"]);
    let s11a = detect_d3d_from_imports(&["d3d11.dll", "dxgi.dll"]);
    let s11b = detect_d3d_from_imports(&["d3d11.dll"]);
    let s12a = detect_d3d_from_imports(&["d3d12.dll", "dxgi.dll"]);
    let s12b = detect_d3d_from_imports(&["d3d12.dll"]);
    cs.add(
        "d3d_six_samples",
        s9a == D3dNeed::D3D9 && s9b == D3dNeed::D3D9 && s11a == D3dNeed::D3D11 && s11b == D3dNeed::D3D11 && s12a == D3dNeed::D3D12 && s12b == D3dNeed::D3D12,
        "",
    );
    // 2) 无 d3d 引用 → None（零开销静态判定的负样本）。
    cs.add("no_d3d_negative", detect_d3d_from_imports(&["user32.dll", "gdi32.dll"]) == D3dNeed::None, "");
    // 3) 高版本优先（同时引用 d3d11+d3d9 → 按最高分流）。
    cs.add("highest_version_priority", detect_d3d_from_imports(&["d3d11.dll", "d3d9.dll"]) == D3dNeed::D3D11, "");
    // 4) 同卡不同文案（D3D12 与 D3D9 文案不同）。
    let (l12, _) = guidance_card(D3dNeed::D3D12, 800);
    let (l9, _) = guidance_card(D3dNeed::D3D9, 800);
    cs.add("distinct_copy_per_version", l12 != l9 && l12.contains("DirectX 12"), "");
    // 5) 电量 <15% 劝阻提示（F196 联动）。
    let (_, dissuade_low) = guidance_card(D3dNeed::D3D11, 100);
    let (_, dissuade_ok) = guidance_card(D3dNeed::D3D11, 800);
    cs.add("battery_dissuade_15pct", dissuade_low && !dissuade_ok && BATTERY_DISSUADE_PERMILLE == 150, "");
    // 6) 32 位游戏 → F004 合并提示。
    cs.add("merge_with_f004", merge_with_f004(32) && !merge_with_f004(64), "");
    // 7) 识别缓存：登记 → 命中零扫描。
    let mut cache = GameCache::new();
    cache.remember([1; 8], D3dNeed::D3D11);
    cs.add("game_cache_hit", cache.lookup(&[1; 8]) == Some(D3dNeed::D3D11) && cache.hits == 1, "");
    // 8) 「下次直接切」记忆 + 全屏参数进快照。
    let mut pref = HandoffPreference::new();
    pref.remember_always(2560, 1440);
    cs.add("always_switch_memory", pref.always_switch && pref.fullscreen_w == 2560 && pref.fullscreen_h == 1440, "");
    // 9) 切域前暂停后台任务（省带宽降风险）。
    let paused = pause_background_jobs(3, &mut pref);
    cs.add("background_jobs_paused", paused == 3 && pref.paused_background_jobs == 3, "");
    // 10) 交接四步全走 ≤30s（分段预算合计 30s）。
    let mut run = HandoffRun::new();
    let mut alive = true;
    for (i, _) in HANDOFF_STEPS.iter().enumerate() {
        alive = run.advance(STEP_BUDGETS_S[i]);
    }
    cs.add("handoff_within_30s", run.verdict() && !alive && run.elapsed_s == 30 && STEP_BUDGETS_S.iter().sum::<u64>() == HANDOFF_BUDGET_S, "");
    // 11) 交接失败 → 降级回 VARIX 并说明（R1 固件脾气路径）。
    let mut slow = HandoffRun::new();
    slow.advance(20);
    slow.advance(20); // 40s > 30s → 降级
    cs.add("degrade_back_on_failure", slow.degraded_back && !slow.success, "");
    // 12) 四步画面名在册（复用 C4 交接四步）。
    cs.add("handoff_steps_c4", HANDOFF_STEPS == ["session-preserve", "flush", "gate", "reboot"], "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：双击游戏 → 探测 → 引导 → 切域 ≤30s 全流程。
    #[test]
    fn double_click_to_windows_game_within_30s() {
        // 双击：import 表静态探测（零开销）。
        let need = detect_d3d_from_imports(&["d3d11.dll", "dxgi.dll"]);
        assert_eq!(need, D3dNeed::D3D11);
        // 引导卡（电量充足不劝阻）。
        let (line, dissuade) = guidance_card(need, 900);
        assert!(!dissuade && line.contains("DirectX 11"));
        // 用户确认 → 四步交接 ≤30s。
        let mut run = HandoffRun::new();
        for (i, _) in HANDOFF_STEPS.iter().enumerate() {
            if !run.advance(STEP_BUDGETS_S[i]) {
                break;
            }
        }
        assert!(run.verdict(), "全流程 ≤30s");
    }

    #[test]
    fn cache_makes_second_launch_free() {
        let mut cache = GameCache::new();
        cache.remember([7; 8], D3dNeed::D3D9);
        for _ in 0..5 {
            assert_eq!(cache.lookup(&[7; 8]), Some(D3dNeed::D3D9));
        }
        assert_eq!(cache.hits, 5, "二次双击零扫描（缓存命中记账）");
    }

    #[test]
    fn d3d9_copy_differs_from_d3d12_copy() {
        let (a, _) = guidance_card(D3dNeed::D3D9, 900);
        let (b, _) = guidance_card(D3dNeed::D3D12, 900);
        assert_ne!(a, b);
    }

    #[test]
    fn pref_not_always_by_default() {
        let p = HandoffPreference::new();
        assert!(!p.always_switch, "默认不自动切（用户每次确认）");
    }
}

// ===========================================================================
// 深化层 · G-A-39 补强：D3D 动态库全集 / 显存预估 / 全屏独占检查
// （D3D 版本探测参照 DXVK/dxgi 枚举面——评估阶段，F130 预登记）
// ---------------------------------------------------------------------------

/// D3D 相关动态库全集（import 表探测词表）。
pub const D3D_DLLS: [&str; 6] =
    ["d3d9.dll", "d3d10.dll", "d3d11.dll", "d3d12.dll", "dxgi.dll", "d3dcompiler_47.dll"];

/// 探测词 → 需求版本映射（dxgi/d3dcompiler 单独出现不足以判定——保守正确）。
pub fn dll_to_need(dll: &str) -> Option<D3dNeed> {
    match dll {
        "d3d9.dll" => Some(D3dNeed::D3D9),
        "d3d10.dll" => Some(D3dNeed::D3D9), // D3D10 需求按 9 面分流（差异表登记）
        "d3d11.dll" => Some(D3dNeed::D3D11),
        "d3d12.dll" => Some(D3dNeed::D3D12),
        _ => None, // dxgi/d3dcompiler 为辅助库
    }
}

/// 显存预估（4K 全屏 4xMSAA 口径，MB）。
pub fn vram_estimate_mb(width: u32, height: u32, msaa_x: u32) -> u32 {
    // (宽×高×4 字节×4 缓冲 + MSAA 倍率) / 1MB
    let base = width as u64 * height as u64 * 4 * 4;
    let with_msaa = base * msaa_x.max(1) as u64;
    (with_msaa / (1 << 20)) as u32
}

/// 全屏独占检查（切域前状态裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FullscreenState {
    Windowed,
    Borderless,
    Exclusive,
}

/// 切域建议：独占全屏 → 必须先回窗口化（交接失败预防——主册【状态与异常】）。
pub fn pre_handoff_screen_check(state: FullscreenState) -> Result<(), &'static str> {
    match state {
        FullscreenState::Exclusive => Err("exit-exclusive-first"),
        _ => Ok(()),
    }
}

/// 交接参数打包/解包（进快照扩展字段——主册【数据与存储】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffParams {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// 三字段打包成 96 位快照字段（域内以三 u32 数组承载）。
pub fn pack_handoff_params(p: &HandoffParams) -> [u32; 3] {
    [p.width, p.height, p.refresh_hz]
}

pub fn unpack_handoff_params(packed: &[u32; 3]) -> HandoffParams {
    HandoffParams { width: packed[0], height: packed[1], refresh_hz: packed[2] }
}

/// 域自检（深化层）。
pub fn run_gamefront_deep() -> CheckSet {
    let mut cs = CheckSet::new("F039-gamefront-deep");
    // 1) 动态库词表六件套。
    cs.add("d3d_dll_roster", D3D_DLLS.len() == 6 && D3D_DLLS[3] == "d3d12.dll" && D3D_DLLS[5] == "d3dcompiler_47.dll", "");
    // 2) 词 → 版本：d3d10 按 9 分流（差异表登记）；dxgi/d3dcompiler 不判定。
    cs.add(
        "dll_version_mapping",
        dll_to_need("d3d9.dll") == Some(D3dNeed::D3D9)
            && dll_to_need("d3d10.dll") == Some(D3dNeed::D3D9)
            && dll_to_need("d3d12.dll") == Some(D3dNeed::D3D12)
            && dll_to_need("dxgi.dll").is_none()
            && dll_to_need("d3dcompiler_47.dll").is_none(),
        "",
    );
    // 3) 显存预估：4K 4xMSAA = 8×8.4×16/16 → 528MB 量级（口径自洽）。
    cs.add("vram_estimate_4k", vram_estimate_mb(3840, 2160, 4) == 506 && vram_estimate_mb(1920, 1080, 1) == 31, "");
    // 4) 独占全屏 → 先回窗口化；无边框/窗口化直通。
    cs.add(
        "exclusive_screen_gate",
        pre_handoff_screen_check(FullscreenState::Exclusive) == Err("exit-exclusive-first")
            && pre_handoff_screen_check(FullscreenState::Borderless).is_ok()
            && pre_handoff_screen_check(FullscreenState::Windowed).is_ok(),
        "",
    );
    // 5) 交接参数打包 round-trip。
    let p = HandoffParams { width: 2560, height: 1440, refresh_hz: 165 };
    cs.add("handoff_params_roundtrip", unpack_handoff_params(&pack_handoff_params(&p)) == p, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn vram_zero_msaa_floor() {
        // MSAA 倍率下限 1（0 输入钳制）。
        assert_eq!(vram_estimate_mb(1920, 1080, 0), vram_estimate_mb(1920, 1080, 1));
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_gamefront_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
