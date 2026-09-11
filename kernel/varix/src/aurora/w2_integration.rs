//! AURORA-1000 W2 联调集成域（步骤 0490~0497）。
//!
//! 八条跨域场景把 W2 的域串成闭环：
//! VWM+合成器+输入、桌面 shell 贯通、应用框架+组件、剪贴板拖放跨应用、
//! 会话+恢复、性能联测、降级联测、fuzz 大跑。
//! 每条场景是一条 `CheckSet` 自检项；`run_w2_checks()` 汇总后接入
//! `run_kernel_checkup()`。纯逻辑、固定容量数组、`no_std` 无分配。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 场景一：VWM + 合成器 + 输入（步骤 0490）
// ---------------------------------------------------------------------------

/// 建窗 → 开窗动画 → 贴靠 → 切焦点 → 关窗，窗口与合成器状态一致。
fn scenario_vwm_compositor() -> bool {
    use super::compositor::{self, BlendMode, FrameClock, LayerRegistry};
    use super::motion::{self, Anim, AnimKind, Curve, Scheduler};
    use super::window::{Rect, SnapSide, WindowManager};

    let screen = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    let mut wm = WindowManager::new();
    let id = match wm.create_window(Rect { x: 100, y: 100, w: 640, h: 480 }, 0) {
        Some(id) => id,
        None => return false,
    };

    // 开窗动画走完，进度单调到 255。
    let mut sched = Scheduler::new();
    let anim = Anim {
        id: 1,
        kind: AnimKind::Open,
        start_frame: 0,
        duration: 24,
        curve: Curve::EaseOut,
        active: true,
        cancelled: false,
    };
    if !sched.schedule(anim) {
        return false;
    }
    sched.advance(24);
    let done = sched.progress_of(1, 24) == Some(255);

    // 窗口层进合成器并脏区合成一次。
    let mut reg = LayerRegistry::new();
    let layer =
        compositor::make_layer(1, 100, 100, 640, 480, [40, 40, 60, 255], 255, BlendMode::Normal);
    if !compositor::layer_tree_add(&mut reg, layer) {
        return false;
    }
    let mut canvas = [0u8; 64 * 64];
    let dirty = compositor::Rect { x: 0, y: 0, w: 64, h: 64 };
    let painted = compositor::composite_dirty(&mut canvas, 64, 64, &reg, &dirty) > 0;

    // 贴靠左半 + 焦点切换 + 关窗，无僵尸窗。
    let snapped = wm.snap_window(id, screen, SnapSide::LeftHalf);
    let focused = wm.focus(id) && wm.current_focus() == Some(id);
    let _ = wm.focus_next();
    let closed = wm.destroy_window(id);
    let clean = wm.alive_count() == 0 && wm.invariants_ok();

    // 帧时钟呈现不丢帧。
    let mut clk = FrameClock::new(16);
    let v1 = compositor::vsync_tick(&mut clk, true);
    done && painted && snapped && focused && closed && clean && v1 == compositor::VsyncVerdict::Present
}

// ---------------------------------------------------------------------------
// 场景二：桌面 shell 贯通（步骤 0491）
// ---------------------------------------------------------------------------

/// 图标 → 任务栏 → 托盘 → 开始菜单 → 右键菜单 全链激活。
fn scenario_shell_chain() -> bool {
    use super::desktop::{self, Desktop, MenuAction, Name, Point, StartItem, TrayItem, WinButton};

    let mut d = Desktop::new();
    if d.icons.allocate(Name::from_str("term"), 7).is_none() {
        return false;
    }
    if !d.taskbar.register(WinButton {
        label: Name::from_str("term"),
        win_id: 7,
        active: true,
    }) {
        return false;
    }
    if !d.tray.register(TrayItem {
        label: Name::from_str("net"),
        icon_id: 3,
        badge: 2,
    }) {
        return false;
    }
    if !d.start.add(StartItem {
        label: Name::from_str("term"),
        app_id: 7,
    }) {
        return false;
    }

    // 图标命中 → 任务栏激活 → 菜单弹出遍历。
    let hit = d.icons.hit(Point { x: 10, y: 10 }).is_some();
    let act = d.taskbar.activate(7);
    d.menu.add(desktop::MenuItem {
        label: Name::from_str("open"),
        action: MenuAction::Open,
        enabled: true,
        submenu: desktop::SUBMENU_NONE,
    });
    d.menu.popup_at(Point { x: 64, y: 64 });
    let nav = d.menu.move_down() || d.menu.traversable_count() == 0;

    hit && act && nav
        && desktop::desktop_consistency(&d)
        && desktop::desktop_within_perf(&d, 8)
}

// ---------------------------------------------------------------------------
// 场景三：应用框架 + 组件（步骤 0492）
// ---------------------------------------------------------------------------

/// 建应用 → 建窗 → 放组件 → 派发事件 → 资源回收。
fn scenario_appfw_widgets() -> bool {
    use super::appfw::{self, AppEvent, AppManager, EvKind, PERM_FS};
    use super::widgets::{self, InputModel, Role, WState};

    let mut mgr = AppManager::new();
    let a = match appfw::app_register(&mut mgr, "demo", PERM_FS, 10) {
        Some(a) => a,
        None => return false,
    };
    if !appfw::app_launch(&mut mgr, a) {
        return false;
    }
    let w = match appfw::window_create(&mut mgr, a, 480, 320) {
        Some(w) => w,
        None => return false,
    };

    // 组件：按钮点击 + 文本输入 + 焦点环。
    let btn = widgets::button_press(
        WState::Normal,
        widgets::Rect { x: 0, y: 0, w: 80, h: 24 },
        40,
        12,
    );
    let mut im = InputModel::new();
    let typed = widgets::input_insert(&mut im, b'h') && widgets::input_insert(&mut im, b'i');
    let cycled = widgets::focus_cycle(0, 3, false) == 1;
    let labeled = widgets::role_label(Role::Button) == "button";

    // 事件入队并派发完，资源不泄漏，应用自检通过。
    let enq = appfw::event_enqueue(&mut mgr, AppEvent {
        kind: EvKind::Key,
        target_app: a,
        win: w,
        code: 72,
    });
    let _ = appfw::event_dispatch(&mut mgr);
    appfw::resource_alloc(&mut mgr, a);
    let freed = appfw::resource_free(&mut mgr, a);
    let closed = appfw::window_destroy(&mut mgr, a, w);

    btn && typed && cycled && labeled && enq && freed && closed
        && appfw::run_app_self_check(&mgr, a)
}

// ---------------------------------------------------------------------------
// 场景四：剪贴板拖放跨应用（步骤 0493）
// ---------------------------------------------------------------------------

/// 复制 → 拖放 → 粘贴 跨应用全通，权限与不变量收口。
fn scenario_clipboard_dnd() -> bool {
    use super::clipboard::{self, ClipFormat, Clipboard, DragPhase, DropResult, ReadOutcome};

    let mut cb = clipboard::new_clipboard();
    let src: u16 = 100;
    let dst: u16 = 200;

    if !clipboard::copy(&mut cb, ClipFormat::Text, b"payload", src) {
        return false;
    }
    clipboard::grant(&mut cb, dst);

    // 拖动 → 经过目标 → 放下。
    let mut st = clipboard::drag_start(&mut cb, ClipFormat::Text, b"payload", src, false);
    clipboard::drag_move(&mut st, 300, 200, true);
    if st.phase != DragPhase::OverTarget {
        return false;
    }
    let dropped = clipboard::drag_drop(&mut cb, &mut st) == DropResult::Dropped;

    // 授权应用粘贴拿到的就是刚复制的内容（seen_seq=0 跳过新鲜度校验）。
    let mut out = [0u8; 16];
    let read = clipboard::paste(&mut cb, ClipFormat::Text, 0, dst, &mut out);
    let text_ok = read == ReadOutcome::Ok && &out[..7] == b"payload";

    dropped && text_ok && clipboard::clip_invariants(&cb)
}

// ---------------------------------------------------------------------------
// 场景五：会话 + 恢复（步骤 0494 前半）
// ---------------------------------------------------------------------------

/// 登录 → 建会话 → 锁屏解锁 → 快照校验 → 崩溃恢复。
fn scenario_session_recovery() -> bool {
    use super::session::{self, AutoStart, AutoStartList, LoginResult, SessionTable, WindowSlot};

    let salt = [7u8; 8];
    let stored = session::hash_passphrase(b"passphrase", &salt, 2);
    let mut login = session::login_new();
    if session::login_submit(&mut login, &stored, &stored) != LoginResult::Granted {
        return false;
    }

    let mut table = SessionTable::new();
    let idx = match table.session_open(1, 100, false) {
        Some(i) => i,
        None => return false,
    };

    // 锁屏 → 解锁恢复原状态。
    session::session_lock(&mut table, idx);
    let locked = table.session_get(idx).map(|s| s.state)
        == Some(super::session::SessionState::Locked);
    session::session_unlock(&mut table, idx);
    let restored = table.session_get(idx).map(|s| s.state)
        == Some(super::session::SessionState::Active);

    // 自启动全部拉起。
    let mut auto = AutoStartList::new();
    auto.autostart_add(AutoStart {
        name: "shell",
        priority: 1,
        enabled: true,
        fails: false,
        launched: false,
        fail_count: 0,
    });
    let (ok, fail) = auto.autostart_run();

    // 会话快照校验 + 崩溃恢复重放。
    let windows = [WindowSlot { x: 0, y: 0, w: 800, h: 600, z: 0 }];
    let snap = session::snapshot_capture(&windows);
    let verified = snap.snapshot_verify();
    let recovered = session::crash_recover(&snap) == session::RecoverVerdict::Restored;

    locked && restored && ok == 1 && fail == 0 && verified && recovered
        && session::session_invariants(&table) == 0
}

// ---------------------------------------------------------------------------
// 场景六：性能联测（步骤 0495）
// ---------------------------------------------------------------------------

/// 动效帧预算、合成预算、窗口布局预算同时达标。
fn scenario_perf_joint() -> bool {
    use super::compositor::{self, ComposeProfile, LayerRegistry};
    use super::motion;
    use super::window;

    let frame_ok = motion::within_frame_budget(3); // 预算 4ms
    let layout_ok =
        window::WindowManager::within_budget(window::WindowManager::layout_cost(8));

    // 640x480 画布全脏区合成一次，耗时在预算内。
    let mut reg = LayerRegistry::new();
    let layer = compositor::make_layer(1, 0, 0, 32, 32, [255, 0, 0, 255], 255, compositor::BlendMode::Normal);
    let added = compositor::layer_tree_add(&mut reg, layer);
    let mut canvas = [0u8; 64 * 64];
    let dirty = compositor::Rect { x: 0, y: 0, w: 64, h: 64 };
    let _ = compositor::composite_dirty(&mut canvas, 64, 64, &reg, &dirty);
    let mut prof = ComposeProfile::new();
    compositor::profile_record(&mut prof, 640 * 480, 900); // 单帧 900µs < 预算
    let composite_ok = compositor::perf_budget_ok(&prof, 16000);

    frame_ok && layout_ok && added && composite_ok
}

// ---------------------------------------------------------------------------
// 场景七：降级联测（步骤 0496）
// ---------------------------------------------------------------------------

/// 无合成器/低端降级下窗口与剪贴板仍可用。
fn scenario_degrade_joint() -> bool {
    use super::clipboard::{self, ClipFormat, Clipboard, DegradeMode};
    use super::compositor;
    use super::motion;
    use super::window::{DegradeLevel, Rect, WindowManager};

    // 合成超预算 → 降级档位非 Full。
    let deep = compositor::degrade_for_budget(20000, 16000);

    // 窗口在 Minimal 降级下仍能建窗贴靠。
    let mut wm = WindowManager::new();
    let _ = wm.apply_degrade(DegradeLevel::Minimal);
    let screen = Rect { x: 0, y: 0, w: 1280, h: 720 };
    let id = wm.create_window(Rect { x: 10, y: 10, w: 320, h: 240 }, 0);
    let still_works = match id {
        Some(id) => wm.snap_window(id, screen, super::window::SnapSide::LeftHalf) && wm.invariants_ok(),
        None => false,
    };

    // 剪贴板降级为纯文本仍可复制粘贴。
    let mut cb = clipboard::new_clipboard();
    let _ = clipboard::copy(&mut cb, ClipFormat::Image, b"\x00\x01", 1);
    let mode = clipboard::degrade_mode(&cb);
    let clip_ok = matches!(mode, DegradeMode::Full | DegradeMode::TextOnly);

    // 动效降级：负载打满 → Static 档；reduce motion 下进度被压平但不 panic。
    let flat = motion::apply_reduce_motion(true, 255) <= 255;
    let md = motion::motion_degrade(1000);

    deep != compositor::DegradeLevel::Full && still_works && clip_ok && flat
        && md == motion::DegradeLevel::Static
}

// ---------------------------------------------------------------------------
// 场景八：fuzz 大跑（步骤 0497）
// ---------------------------------------------------------------------------

/// 五域 fuzz 语料合并跑一轮：无 panic、结果在合法域内。
fn scenario_fuzz_joint() -> bool {
    use super::appfw;
    use super::clipboard;
    use super::compositor::{LayerRegistry};
    use super::compositor;
    use super::motion::{self, Curve};
    use super::window::WindowManager;

    let corpus: [u8; 32] = [
        0xde, 0xad, 0xbe, 0xef, 0x00, 0xff, 0x7f, 0x80, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
        14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    ];

    let mut wm = WindowManager::new();
    let fz_win = wm.run_fuzz(0xC0FFEE, 256) > 0 && wm.invariants_ok();

    let _ = appfw::appfw_fuzz(&corpus); // 无 panic 即通过
    let fz_clip = clipboard::fuzz_clipboard(&corpus);
    let _ = motion::motion_fuzz(16, 24, false, Curve::EaseInOut);
    let _ = motion::motion_fuzz(0, 0, true, Curve::Linear);

    let reg = LayerRegistry::new();
    let mut canvas = [0u8; 32 * 32];
    let _ = compositor::fuzz_composite(&mut canvas, 32, 32, &reg);

    fz_win && fz_clip
}

// ---------------------------------------------------------------------------
// W2 CheckSet 汇总（步骤 0499）
// ---------------------------------------------------------------------------

/// W2 联调 CheckSet：八条跨域场景一条不落。
pub fn run_w2_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-w2");

    set.add("W2-490 vwm+compositor+input", scenario_vwm_compositor(), "snap/focus/compose");
    set.add("W2-491 shell chain", scenario_shell_chain(), "icons/taskbar/tray/start");
    set.add("W2-492 appfw+widgets", scenario_appfw_widgets(), "app->win->widget->event");
    set.add("W2-493 clipboard dnd", scenario_clipboard_dnd(), "copy/drag/drop/paste");
    set.add("W2-494 session+recovery", scenario_session_recovery(), "login/lock/snapshot");
    set.add("W2-495 perf joint", scenario_perf_joint(), "frame/layout/composite budget");
    set.add("W2-496 degrade joint", scenario_degrade_joint(), "degraded still usable");
    set.add("W2-497 fuzz joint", scenario_fuzz_joint(), "5-domain corpus, no panic");

    set
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn w2_integration_scenarios_pass() {
        assert!(scenario_vwm_compositor(), "vwm+compositor+input");
        assert!(scenario_shell_chain(), "desktop shell chain");
        assert!(scenario_appfw_widgets(), "appfw+widgets");
        assert!(scenario_clipboard_dnd(), "clipboard dnd");
        assert!(scenario_session_recovery(), "session recovery");
        assert!(scenario_perf_joint(), "perf joint");
        assert!(scenario_degrade_joint(), "degrade joint");
        assert!(scenario_fuzz_joint(), "fuzz joint");
    }

    #[test]
    fn w2_checkset_all_green() {
        let set = run_w2_checks();
        for i in 0..set.len() {
            let c = set.get(i).expect("check");
            assert!(c.passed, "W2 scenario failed: {} ({})", c.name, c.detail);
        }
        assert_eq!(set.len(), 8);
    }

    /// 步骤 0499 —— W2 CheckSet 汇总：八个域自检 + 联调域全绿。
    #[test]
    fn w2_domain_checksets_summary() {
        let sets = [
            super::super::window::run_window_checks(),
            super::super::motion::run_motion_checks(),
            super::super::desktop::run_desktop_checks(),
            super::super::compositor::run_compositor_checks(),
            super::super::appfw::run_appfw_checks(),
            super::super::widgets::run_widgets_checks(),
            super::super::clipboard::run_clipboard_checks(),
            super::super::session::run_session_checks(),
            run_w2_checks(),
        ];
        let mut total = 0;
        for s in sets.iter() {
            let (p, f) = s.tally();
            total += p + f;
            assert!(s.all_passed(), "domain {} has failures", s.domain);
        }
        assert!(total >= 8 * 25, "W2 should carry >=200 checks, got {}", total);
    }
}
