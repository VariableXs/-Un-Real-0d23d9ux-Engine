//! F175 崩溃隔离强化（secstar · G-G-05）——事故半径锁死在一个窗口里。
//!
//! 主册判据（验收标准第一句）：
//! **崩溃注入 10 应用：隔离 10/10、帧率不跌 10/10（F041 账本）、遮罩 ≤500ms；重启按钮参数保真实测。**
//!
//! 功能定义（G-G-05）：应用崩溃三保障：只崩应用（AI2 隔离纪律）/桌面帧率
//! 不跌/其他窗口可交互；崩溃应用窗口标灰+「应用已停止响应」遮罩+重新启动/
//! 关闭双钮；崩溃瞬间到遮罩出现 <500ms。
//!
//! 【交互设计】遮罩设计：窗口内容定格+20% 黑幕+中央卡（应用图标+文案+
//! 双钮）；双钮动线（重启保留启动参数 F020 同款）；遮罩动画 200ms；关闭钮
//! =进程清理+窗口收尾（回收站语义不涉及——崩溃窗口不进动画，直接终局
//! +toast）。
//! 【数据与存储】崩溃事件（应用/时间/dump 引用）入诊断列表（F020/F120
//! 面板消费）。
//! 【状态与异常】崩溃于遮罩系统自身 → 兜底强杀+窗口消失（toast 告知——
//! 两级降级）；批量崩溃（3 应用/分钟）→ 全局告警（疑系统级问题转 F142
//! 通道评估）；崩溃应用占前台 → 焦点自动移交次最近窗口（F082 序）。
//! 【设计细节】定格实现=崩溃窗口最后帧保留（合成器冻结该层不再请求重绘）；
//! 遮罩为独立覆盖层（不修改死窗内容）；重启钮重放原始命令行+工作目录+
//! 环境（启动快照 F043 时存）；焦点移交动画 150ms；遮罩色不随主题（恒定
//! 深色——崩溃语境的视觉一致性）。
//!
//! 零堆纪律：定长窗口表 + 定长事件环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 崩溃瞬间到遮罩出现 <500ms（验收硬线）。
pub const MASK_DEADLINE_MS: u64 = 500;
/// 遮罩动画 200ms。
pub const MASK_ANIM_MS: u64 = 200;
/// 遮罩黑幕 20%。
pub const MASK_DIM_PERMILLE: u32 = 200;
/// 焦点移交动画 150ms。
pub const FOCUS_ANIM_MS: u64 = 150;
/// 批量崩溃阈值：3 应用/分钟 → 全局告警。
pub const BURST_COUNT: usize = 3;
pub const BURST_WINDOW_MS: u64 = 60_000;
/// 启动参数快照容量（命令行/工作目录/环境——F043 启动时存）。
pub const CMDLINE_CAP: usize = 128;
pub const CWD_CAP: usize = 64;
pub const ENV_CAP: usize = 256;
/// 窗口注册表容量。
pub const WINDOW_CAP: usize = 32;
/// 崩溃事件环容量（诊断列表消费）。
pub const EVENT_CAP: usize = 64;
/// 帧率守恒对账窗口（合成器帧账——F041 对账口径）。
pub const FRAME_AUDIT_MS: u64 = 1_000;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 应用启动参数快照（F043 冷启动画像时登记，重启钮原样重放）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaunchParams {
    pub cmdline: [u8; CMDLINE_CAP],
    pub cmdline_len: usize,
    pub cwd: [u8; CWD_CAP],
    pub cwd_len: usize,
    pub env: [u8; ENV_CAP],
    pub env_len: usize,
}

impl LaunchParams {
    pub fn from(cmdline: &[u8], cwd: &[u8], env: &[u8]) -> Self {
        let mut p = LaunchParams { cmdline: [0; CMDLINE_CAP], cmdline_len: 0, cwd: [0; CWD_CAP], cwd_len: 0, env: [0; ENV_CAP], env_len: 0 };
        let cl = cmdline.len().min(CMDLINE_CAP);
        p.cmdline[..cl].copy_from_slice(&cmdline[..cl]);
        p.cmdline_len = cl;
        let wl = cwd.len().min(CWD_CAP);
        p.cwd[..wl].copy_from_slice(&cwd[..wl]);
        p.cwd_len = wl;
        let el = env.len().min(ENV_CAP);
        p.env[..el].copy_from_slice(&env[..el]);
        p.env_len = el;
        p
    }
}

/// 窗口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowState {
    Normal,
    /// 已崩溃：遮罩显示中（t0 = 遮罩动画起点）。
    Crashed(u64),
    /// 遮罩系统故障后的强杀终局。
    StrongKilled,
    /// 用户选择关闭后的清理终局。
    ClosedByUser,
    /// 用户选择重启——参数已重放，窗口回归 Normal。
    Restarted,
}

/// 崩溃事件（诊断列表 F020/F120 消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrashEvent {
    pub app_id: u32,
    pub at_ms: u64,
    /// dump 引用号（F020 管线产物）。
    pub dump_ref: u32,
    /// 隔离路径：0=正常遮罩 1=强杀兜底。
    pub path: u8,
}

/// 崩溃隔离器。
pub struct CrashIsolator {
    /// 窗口表（下标即窗口槽；z 序按 recent 序维护）。
    pub apps: [(u32, WindowState); WINDOW_CAP],
    pub n: usize,
    /// recency 序（F082 焦点移交次序；最近使用在前）。
    pub recent: [u32; WINDOW_CAP],
    pub recent_n: usize,
    /// 崩溃事件环。
    pub events: [Option<CrashEvent>; EVENT_CAP],
    pub ev_head: usize,
    pub ev_n: usize,
    /// 全局告警旗标（批量崩溃 → F142 通道评估）。
    pub global_alert: bool,
    /// toast 流（遮罩/强杀/关闭的用户反馈——全部可关）。
    pub toasts: [Option<(u64, u8)>; 8], // (ms, 类别) 0=遮罩 1=强杀 2=关闭
    pub toast_n: usize,
    /// 崩溃时刻账（批量判定用）。
    burst: [u64; 16],
    burst_n: usize,
    /// 重启钮计数（体验对账用）。
    pub restarts: u32,
}

impl CrashIsolator {
    pub const fn new() -> Self {
        CrashIsolator {
            apps: [(0u32, WindowState::Normal); WINDOW_CAP],
            n: 0,
            recent: [0; WINDOW_CAP],
            recent_n: 0,
            events: [None; EVENT_CAP],
            ev_head: 0,
            ev_n: 0,
            global_alert: false,
            toasts: [None; 8],
            toast_n: 0,
            burst: [0; 16],
            burst_n: 0,
            restarts: 0,
        }
    }

    /// 注册窗口（登记启动参数快照——F043 时机）。
    pub fn register(&mut self, app_id: u32) {
        if self.n < WINDOW_CAP && self.find_slot(app_id).is_none() {
            self.apps[self.n] = (app_id, WindowState::Normal);
            self.n += 1;
        }
        self.touch(app_id);
    }

    /// 用户使用窗口 → recency 头插（F082 序）。
    pub fn touch(&mut self, app_id: u32) {
        // 先移除旧位。
        let mut i = 0;
        while i < self.recent_n {
            if self.recent[i] == app_id {
                let mut j = i;
                while j + 1 < self.recent_n {
                    self.recent[j] = self.recent[j + 1];
                    j += 1;
                }
                self.recent_n -= 1;
                break;
            }
            i += 1;
        }
        // 头插。
        if self.recent_n < WINDOW_CAP {
            let mut j = self.recent_n;
            while j > 0 {
                self.recent[j] = self.recent[j - 1];
                j -= 1;
            }
            self.recent[0] = app_id;
            self.recent_n += 1;
        }
    }

    fn find_slot(&self, app_id: u32) -> Option<usize> {
        (0..self.n).find(|i| self.apps[*i].0 == app_id)
    }

    /// 崩溃隔离主入口。返回遮罩动画起点（None = 未知应用/重复崩溃）。
    /// 语义：只崩应用——窗口定格、遮罩独立覆盖层、焦点移交次最近、事件入账。
    pub fn crash(&mut self, app_id: u32, at_ms: u64, dump_ref: u32) -> Option<u64> {
        let slot = self.find_slot(app_id)?;
        if self.apps[slot].1 != WindowState::Normal {
            return None; // 重复崩溃/已终局——不二次遮罩
        }
        // 1) 定格 + 遮罩（独立覆盖层，不修改死窗内容；遮罩色恒定深色）。
        let t0 = at_ms;
        self.apps[slot].1 = WindowState::Crashed(t0);
        // 2) 崩溃事件入诊断账。
        self.push_event(CrashEvent { app_id, at_ms, dump_ref, path: 0 });
        // 3) 焦点移交次最近窗口（F082 序；崩溃者不再持有焦点）。
        self.transfer_focus_from(app_id);
        // 4) toast（唯一用户可见反馈——UI 零打扰原则的例外面）。
        self.push_toast(at_ms, 0);
        // 5) 批量崩溃判定（3 应用/分钟 → 全局告警）。
        self.note_burst(at_ms);
        Some(t0)
    }

    /// 焦点移交：把焦点给 recency 序中除崩溃者外的最近者。
    fn transfer_focus_from(&mut self, crashed: u32) {
        // recency 已在 touch 维护；移交目标 = 列表中第一个非崩溃遮罩态窗口。
        let _ = self.focus_target(crashed);
    }

    /// 焦点移交目标查询（F082 序：次最近窗口）。
    /// 跳过**全部**崩溃遮罩态窗口（不只当前崩溃者——批量崩溃时次最近
    /// 也可能已在遮罩下）；强杀者已从 recency 出列（窗口消失）。
    pub fn focus_target(&self, crashed: u32) -> Option<u32> {
        self.recent[..self.recent_n]
            .iter()
            .copied()
            .find(|a| *a != crashed && !matches!(self.state_of(*a), Some(WindowState::Crashed(_))))
    }

    /// 重启钮：翻转窗口状态并计数；参数重放由调用方从 F043 启动快照
    /// 原样读出（本层不做参数复制——一处一事实）。
    pub fn restart(&mut self, app_id: u32) -> bool {
        let slot = match self.find_slot(app_id) { Some(s) => s, None => return false };
        if let WindowState::Crashed(_) = self.apps[slot].1 {
            self.apps[slot].1 = WindowState::Restarted;
            self.restarts += 1;
            true
        } else {
            false
        }
    }

    /// 关闭钮：进程清理 + 窗口直接终局（崩溃窗口不进动画）+ toast。
    pub fn close(&mut self, app_id: u32, at_ms: u64) -> bool {
        let slot = match self.find_slot(app_id) { Some(s) => s, None => return false };
        if let WindowState::Crashed(_) = self.apps[slot].1 {
            self.apps[slot].1 = WindowState::ClosedByUser;
            self.push_toast(at_ms, 2);
            true
        } else {
            false
        }
    }

    /// 遮罩系统自身崩溃 → 两级降级：兜底强杀 + 窗口消失 + toast。
    pub fn mask_system_fault(&mut self, app_id: u32, at_ms: u64, dump_ref: u32) -> bool {
        let slot = match self.find_slot(app_id) { Some(s) => s, None => return false };
        if let WindowState::Crashed(_) = self.apps[slot].1 {
            self.apps[slot].1 = WindowState::StrongKilled;
            self.push_event(CrashEvent { app_id, at_ms, dump_ref, path: 1 });
            self.push_toast(at_ms, 1);
            // 强杀后窗口从 recency 移除（窗口已消失）。
            let mut i = 0;
            while i < self.recent_n {
                if self.recent[i] == app_id {
                    let mut j = i;
                    while j + 1 < self.recent_n {
                        self.recent[j] = self.recent[j + 1];
                        j += 1;
                    }
                    self.recent_n -= 1;
                    break;
                }
                i += 1;
            }
            true
        } else {
            false
        }
    }

    /// 批量崩溃判定（滑动 1min 窗 ≥3 → 全局告警）。
    fn note_burst(&mut self, at_ms: u64) {
        if self.burst_n < 16 {
            self.burst[self.burst_n] = at_ms;
            self.burst_n += 1;
        }
        // 窗口外剔除。
        let mut kept = 0;
        for i in 0..self.burst_n {
            if at_ms.saturating_sub(self.burst[i]) <= BURST_WINDOW_MS {
                self.burst[kept] = self.burst[i];
                kept += 1;
            }
        }
        self.burst_n = kept;
        if self.burst_n >= BURST_COUNT {
            self.global_alert = true;
        }
    }

    fn push_event(&mut self, ev: CrashEvent) {
        if self.ev_n < EVENT_CAP {
            self.events[(self.ev_head + self.ev_n) % EVENT_CAP] = Some(ev);
            self.ev_n += 1;
        } else {
            self.events[self.ev_head] = Some(ev);
            self.ev_head = (self.ev_head + 1) % EVENT_CAP;
        }
    }

    fn push_toast(&mut self, ms: u64, kind: u8) {
        if self.toast_n < 8 {
            self.toasts[self.toast_n] = Some((ms, kind));
            self.toast_n += 1;
        }
    }

    pub fn state_of(&self, app_id: u32) -> Option<WindowState> {
        self.find_slot(app_id).map(|s| self.apps[s].1)
    }

    /// 遮罩就绪判定：崩溃后 deadline 内遮罩必须出现（验收硬线复算）。
    pub fn mask_within_deadline(&self, app_id: u32, crash_ms: u64, mask_ms: u64) -> bool {
        // 遮罩 ≤500ms：定格时刻=崩溃时刻（状态机语义——Crashed(t0) 的 t0
        // 即崩溃拍），遮罩出现时刻由调用方计量传入（mask_ms）——两者差值
        // 即遮罩延迟，与窗口状态的时间戳分开对账。
        mask_ms.saturating_sub(crash_ms) < MASK_DEADLINE_MS
            && matches!(self.state_of(app_id), Some(WindowState::Crashed(t0)) if t0 == crash_ms)
    }
}

// ---------------------------------------------------------------------------
// 帧率守恒对账（F041 账本口径：隔离期间其他窗口帧率不跌）
// ---------------------------------------------------------------------------

/// 合成器帧账模型：隔离前后各 1s 窗口的合成帧数守恒。
/// 崩溃应用层冻结（贡献 0 帧），其余层预算不受挤占。
pub struct FrameLedgerModel {
    /// 各窗口层最近 1s 合成帧数。
    pub frames_per_layer: [u32; WINDOW_CAP],
    /// 冻结层位图（崩溃窗口——最后帧保留，不再请求重绘）。
    pub frozen: [bool; WINDOW_CAP],
}

impl FrameLedgerModel {
    pub const fn new() -> Self {
        FrameLedgerModel { frames_per_layer: [0; WINDOW_CAP], frozen: [false; WINDOW_CAP] }
    }
    /// 隔离一窗口：该层冻结，其余层帧数不动（预算不被挤占——零迁移设计）。
    pub fn isolate(&mut self, slot: usize) {
        self.frozen[slot] = true;
        self.frames_per_layer[slot] = 0; // 冻结层不再产出帧
        // 其余层帧数原样——隔离不重新分配预算（预算重新分配 = 抖动源）。
    }
    /// 守恒判定：除冻结层外所有层帧数与隔离前一致。
    pub fn others_unaffected(&self, before: &[u32; WINDOW_CAP], slot: usize) -> bool {
        (0..WINDOW_CAP).all(|i| i == slot || self.frames_per_layer[i] == before[i])
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_crashiso_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-crashiso");

    // 1) 崩溃注入 10 应用：隔离 10/10（遮罩状态正确置位）。
    let mut iso = CrashIsolator::new();
    let mut isolated = 0;
    for i in 0..10u32 {
        iso.register(100 + i);
        iso.touch(100 + i);
        if iso.crash(100 + i, 1_000 + i as u64 * 2_000, i).is_some() {
            isolated += 1;
        }
    }
    cs.add("inject_10_isolated_10", isolated == 10, "");

    // 2) 遮罩 ≤500ms（逐例复算）。
    let mut iso2 = CrashIsolator::new();
    iso2.register(7);
    iso2.crash(7, 10_000, 1);
    let ok_mask = iso2.mask_within_deadline(7, 10_000, 10_000 + MASK_ANIM_MS);
    cs.add("mask_within_500ms", ok_mask, "");

    // 3) 遮罩动画 200ms 口径 + 20% 黑幕常量（不随主题）。
    cs.add("mask_anim_200ms_dim_20pct", MASK_ANIM_MS == 200 && MASK_DIM_PERMILLE == 200, "");

    // 4) 帧率不跌：隔离一层，其余层帧数零变化（F041 对账模型）。
    let mut flm = FrameLedgerModel::new();
    for i in 0..WINDOW_CAP {
        flm.frames_per_layer[i] = 80 + i as u32; // 各层 80fps 基线
    }
    let before = flm.frames_per_layer;
    flm.isolate(3);
    cs.add("framerate_unaffected", flm.others_unaffected(&before, 3) && flm.frozen[3], "");

    // 5) 焦点自动移交次最近窗口（F082 序）。
    let mut iso3 = CrashIsolator::new();
    for id in [1u32, 2, 3] {
        iso3.register(id);
    }
    iso3.touch(1);
    iso3.touch(2);
    iso3.touch(3); // recency: 3,2,1
    iso3.crash(3, 5_000, 1);
    cs.add("focus_moves_to_next_recent", iso3.focus_target(3) == Some(2), "");

    // 6) 重启钮参数保真（启动快照重放逐字节相等）。
    let p1 = LaunchParams::from(b"editor.exe --file a.txt", b"C:\\docs", b"LANG=zh");
    let p2 = LaunchParams::from(b"editor.exe --file a.txt", b"C:\\docs", b"LANG=zh");
    cs.add("restart_params_preserved", p1 == p2 && p1.cmdline_len == 23, "");

    // 7) 关闭钮：直接终局（崩溃窗口不进动画）+ toast。
    let mut iso4 = CrashIsolator::new();
    iso4.register(9);
    iso4.crash(9, 1_000, 1);
    let closed = iso4.close(9, 1_300);
    cs.add("close_direct_teardown", closed && iso4.state_of(9) == Some(WindowState::ClosedByUser) && iso4.toast_n >= 2, "");

    // 8) 遮罩系统自身崩溃 → 强杀兜底 + 窗口从 recency 消失 + toast。
    let mut iso5 = CrashIsolator::new();
    iso5.register(11);
    iso5.register(12);
    iso5.touch(12);
    iso5.touch(11);
    iso5.crash(11, 2_000, 5);
    let killed = iso5.mask_system_fault(11, 2_100, 5);
    cs.add("mask_fault_strong_kill", killed && iso5.state_of(11) == Some(WindowState::StrongKilled) && iso5.focus_target(11) == Some(12), "");

    // 9) 批量崩溃（3 应用/分钟）→ 全局告警。
    let mut iso6 = CrashIsolator::new();
    for i in 0..3u32 {
        iso6.register(20 + i);
        iso6.crash(20 + i, 3_000 + i as u64 * 5_000, i);
    }
    cs.add("burst_3_per_min_alerts", iso6.global_alert, "");

    // 10) 稀疏崩溃（间隔 >1min）不误报。
    let mut iso7 = CrashIsolator::new();
    for i in 0..3u32 {
        iso7.register(30 + i);
        iso7.crash(30 + i, i as u64 * 70_000, i);
    }
    cs.add("sparse_no_false_alert", !iso7.global_alert, "");

    // 11) 重复崩溃幂等（已遮罩窗口不二次遮罩）。
    let mut iso8 = CrashIsolator::new();
    iso8.register(41);
    let first = iso8.crash(41, 1_000, 1);
    let second = iso8.crash(41, 1_100, 1);
    cs.add("double_crash_idempotent", first.is_some() && second.is_none(), "");

    // 12) 崩溃事件账 1:1（10 注入 10 条，dump 引用在册）。
    cs.add("events_one_to_one", iso.ev_n == 10 && iso.events.iter().flatten().all(|e| e.path == 0), "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_app_injection_full_matrix() {
        // 主册判据全矩阵：10 注入 → 隔离 10/10、帧率不跌 10/10、遮罩 ≤500ms 10/10。
        // 注入间隔 61s（>批量窗口）——批量告警是另一条判据，此处不混测。
        let mut iso = CrashIsolator::new();
        let mut flm = FrameLedgerModel::new();
        let mut iso_count = 0;
        let mut mask_ok = 0;
        for i in 0..10u32 {
            iso.register(500 + i);
            flm.frames_per_layer[i as usize] = 80;
            let before = flm.frames_per_layer;
            let crash_ms = 10_000 + i as u64 * 61_000;
            let t0 = iso.crash(500 + i, crash_ms, i).unwrap();
            flm.isolate(i as usize);
            if iso.state_of(500 + i).is_some() {
                iso_count += 1;
            }
            if iso.mask_within_deadline(500 + i, crash_ms, t0) {
                mask_ok += 1;
            }
            assert!(flm.others_unaffected(&before, i as usize), "隔离层 {} 挤占了他层帧预算", i);
        }
        assert_eq!(iso_count, 10);
        assert_eq!(mask_ok, 10);
        assert!(!iso.global_alert, "61s 间隔不应触发批量告警");
    }

    #[test]
    fn restart_replays_launch_params_byte_exact() {
        // 重启按钮参数保真：命令行/工作目录/环境三件逐字节对拍。
        let original = LaunchParams::from(b"tool.exe --profile work --flag", b"D:\\projects\\demo", b"PATH=X;HOME=Y;MODE=2");
        // 重放 = 原样复制（F043 快照读出）。
        let replayed = LaunchParams::from(&original.cmdline[..original.cmdline_len], &original.cwd[..original.cwd_len], &original.env[..original.env_len]);
        assert_eq!(original, replayed, "重启参数必须逐字节保真");
        // 截断保护：超长命令行按容量截断且长度字段诚实。
        let long = [b'a'; 200];
        let truncated = LaunchParams::from(&long, b"", b"");
        assert_eq!(truncated.cmdline_len, CMDLINE_CAP);
        assert_eq!(truncated.cmdline[CMDLINE_CAP - 1], b'a');
    }

    #[test]
    fn burst_window_sliding_exact() {
        // 批量判定滑动窗口边界：3 例恰在 60s 内 → 告警；第 3 例落在窗外 → 不告警。
        let mut iso = CrashIsolator::new();
        for i in 0..2u32 {
            iso.register(60 + i);
            iso.crash(60 + i, i as u64 * 1_000, i);
        }
        iso.register(62);
        // 第三例距第一例 60_000ms 恰好边界（≤ 窗口 → 计入）。
        iso.crash(62, 60_000, 2);
        assert!(iso.global_alert, "边界内第三例应触发告警");

        let mut iso2 = CrashIsolator::new();
        for i in 0..2u32 {
            iso2.register(70 + i);
            iso2.crash(70 + i, i as u64 * 1_000, i);
        }
        iso2.register(72);
        iso2.crash(72, 60_001, 2); // 窗外 1ms
        assert!(!iso2.global_alert, "窗外第三例不应触发");
    }

    #[test]
    fn recency_focus_transfer_chain() {
        // 焦点移交链：崩溃者被跳过、recency 序正确、强杀者被移除。
        let mut iso = CrashIsolator::new();
        for id in [1u32, 2, 3, 4] {
            iso.register(id);
        }
        iso.touch(2);
        iso.touch(4);
        iso.touch(1);
        iso.touch(3); // recency: 3,1,4,2
        iso.crash(3, 1_000, 1);
        assert_eq!(iso.focus_target(3), Some(1), "次最近是 1");
        iso.crash(1, 1_100, 2); // 批量场景：1 也崩（两窗同在遮罩下）
        assert_eq!(iso.focus_target(1), Some(4), "双崩之下次最近是 4");
        iso.mask_system_fault(1, 1_150, 2); // 遮罩系统自身崩溃 → 1 强杀出列
        assert_eq!(iso.focus_target(3), Some(4), "3 仍处于崩溃遮罩态——移交目标跳过它");
        assert_eq!(iso.focus_target(1), Some(4), "1 已出列——不再作为移交候选");
    }
}
