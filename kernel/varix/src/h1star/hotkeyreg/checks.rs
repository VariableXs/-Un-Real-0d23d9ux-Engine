//! F244 自检判定面：fuzz 引擎与判定 1~14（完整性审计、冲突 3 例、
//! 改键持久化、第三方接入、序列键状态机、派发留痕）。
//!
//! 判据红线：判定次序与原实现逐条一致，组间共享状态经返回值显性
//! 传递（reg → reg2 → reset 状态链原样）；判据块内 `set.add` 的
//! 断言表达式与顺序保持判据语义独立，不做归并。

use crate::checks::CheckSet;

use super::decl::{ParsedDecl, parse_decl};
use super::keys::{
    Combo, VK_A, VK_F1, VK_LWIN, VK_TAB, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN, combo_str,
};
use super::registry::{
    AppId, ChordStep, CHORD_DOC, CONFLICT_HINT_DOC, ChangeKind, DescBuf, HK_CAP, HotkeyReg,
    PRESETS, RegOutcome, RebindOutcome, fnv1a, uid_of_label,
};

// ---------------------------------------------------------------------------
// fuzz 引擎
// ---------------------------------------------------------------------------

/// xors32 一步（fuzz 面共用引擎步进——三行异或移位一处定义，消除
/// 判定 13/判定 v2-5 内十余处复制粘贴）。v2 判定面跨文件复用
/// （`super::v2` 的 fuzz 主体同用此步进）。
pub(crate) fn xors32(x: &mut u32) {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
}

/// fuzz 注册臂（判定 13 混合流的注册分支）：注册成功必可派发、占用必
/// 有前主、表满必容量有界。返回不变量是否守住（与原 if-survived=false
/// 逐分支等价）。
fn fuzz_register_step(fz: &mut HotkeyReg, x: &mut u32, i: u32, ts: u64) -> bool {
    xors32(x);
    let combo = Combo::new((*x % 16) as u8, VK_A + (*x % 26) as u16);
    xors32(x);
    let mut appb = [0u8; 8];
    appb[0] = b'a';
    appb[1..5].copy_from_slice(&i.to_le_bytes());
    let decl = ParsedDecl { mods: combo.mods, key: combo.key, desc: DescBuf::from_bytes(b"fuzz") };
    let before = fz.dispatch(combo);
    match fz.register_app_decl(AppId::from_bytes(&appb), &decl, 0, ts) {
        RegOutcome::Registered => {
            let want_uid = fnv1a(&appb).rotate_left(8);
            !(before.is_some() || fz.dispatch(combo).map_or(true, |e| e.uid() != want_uid))
        }
        RegOutcome::Occupied(_) => before.is_some(),
        RegOutcome::Full => fz.count >= HK_CAP,
        _ => true,
    }
}

/// fuzz 注销臂：在册条目注销后不得再派发命中（探测链删除正确性的
/// 运行时抽查）。返回不变量是否守住。
fn fuzz_unregister_step(fz: &mut HotkeyReg, x: &mut u32, ts: u64) -> bool {
    xors32(x);
    let combo = Combo::new((*x % 16) as u8, VK_A + (*x % 26) as u16);
    match fz.dispatch(combo) {
        Some(e) => {
            let uid = e.uid();
            let removed = fz.unregister(uid, ts);
            !(removed && fz.dispatch(combo).is_some())
        }
        None => true,
    }
}

/// fuzz 改键臂：改键成功后旧键位不得命中原条目、冲突拒绝后新键位必须
/// 有主。返回不变量是否守住。
fn fuzz_rebind_step(fz: &mut HotkeyReg, x: &mut u32, ts: u64) -> bool {
    xors32(x);
    let pick = (*x as usize) % HK_CAP;
    let Some(e) = fz.slot_at(pick) else {
        return true;
    };
    xors32(x);
    let new_combo = Combo::new((*x % 16) as u8, VK_A + (*x % 26) as u16);
    let old = e.combo;
    match fz.rebind(e.uid(), new_combo, ts) {
        RebindOutcome::Done => {
            !(new_combo != old && fz.dispatch(old).map_or(false, |e2| e2.uid() == e.uid()))
        }
        RebindOutcome::Occupied(_) => fz.dispatch(new_combo).is_some(),
        _ => true,
    }
}

/// fuzz 主体（判定 13：2000 轮注册/注销/改键混合流——注册成功必可
/// 派发、冲突必显性拒绝且原条目键位不变、表容量有界、无 panic）。
/// 返回 survived；分支臂提取为独立函数——主循环只留调度。
fn fuzz_registry_2000() -> bool {
    let mut x: u32 = 0x853C_49E7;
    let mut fz = HotkeyReg::new();
    let mut survived = true;
    let mut ts: u64 = 0;
    for i in 0..2000u32 {
        xors32(&mut x);
        ts += (x % 7 + 1) as u64;
        let ok = match x % 10 {
            0..=4 => fuzz_register_step(&mut fz, &mut x, i, ts),
            5..=6 => fuzz_unregister_step(&mut fz, &mut x, ts),
            _ => fuzz_rebind_step(&mut fz, &mut x, ts),
        };
        survived &= ok;
    }
    survived && fz.count <= HK_CAP
}

// ---------------------------------------------------------------------------
// 判定 1~12（组间共享状态链 reg → reg2 → reset 原样）
// ---------------------------------------------------------------------------

/// 判定 1~2：预置表全量安装（判据本体：46 条全量入表）+ 完整性审计与
/// 数值抽查（三条代表键位与主册一致）。返回装好的注册表供后续判定
/// 继续使用（判定 3~9 共享同一张表——状态链保持）。
fn chk_presets_installed(set: &mut CheckSet) -> HotkeyReg {
    let mut reg = HotkeyReg::new();
    let installed = reg.preset_all(1000);
    set.add(
        "preset table fully installed",
        installed == PRESETS.len() && reg.count == PRESETS.len() && reg.preset_installed == PRESETS.len() as u32,
        "",
    );

    let cc = reg.entry_by_uid(uid_of_label("sys.copy"));
    let at = reg.dispatch(Combo::new(MOD_ALT, VK_TAB));
    let wl = reg.dispatch(Combo::new(MOD_WIN, 0x4C));
    set.add(
        "preset audit + spot checks",
        reg.audit_presets()
            && cc.map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43) && e.enabled)
            && at.map_or(false, |e| e.label == "sys.alt-tab")
            && wl.map_or(false, |e| e.label == "sys.lock"),
        "",
    );
    reg
}

/// 判定 3~5：冲突即检三例（系统 vs 系统 / 第三方 vs 系统 / 改键撞第
/// 三方）。声明行解析失败 = 判定面自身构造错误：显性记红早退，不
/// panic（裸 expect 消灭岗位：可恢复错误路径，判定面对自障不炸）。
fn chk_conflict_cases(set: &mut CheckSet, reg: &mut HotkeyReg) {
    // 例 1（系统 vs 系统）：注册 Ctrl+C → Occupied 且列占用方。
    let o1 = reg.register_system("sys.my-copy", Combo::new(MOD_CTRL, 0x43), "", 1100);
    set.add(
        "conflict case 1: lists system occupant",
        matches!(o1, RegOutcome::Occupied(ref occ) if occ.is_system() && occ.label == "sys.copy"),
        "",
    );

    // 例 2（第三方 vs 系统）：声明 Win+E → 占用可见、原条目键位未被
    //     覆盖（先到先得，无静默覆盖）。
    let Some(decl) = parse_decl("hotkey=win+e|打开资源管理器（第三方）".as_bytes()) else {
        set.add("conflict case 2: third-party sees system occupant", false, "");
        return;
    };
    let o2 = reg.register_app_decl(AppId::from_bytes(b"vxapp-demo"), &decl, 0, 1200);
    set.add(
        "conflict case 2: third-party sees system occupant",
        matches!(o2, RegOutcome::Occupied(ref occ) if occ.is_system() && occ.label == "sys.explorer")
            && reg.dispatch(Combo::new(MOD_WIN, 0x45)).map_or(false, |e| e.label == "sys.explorer"),
        "",
    );

    // 例 3（改键撞第三方）：先让第三方成功注册 ctrl+alt+f9，再把
    //     sys.copy 改到该键位 → Occupied（第三方占用方可见）。
    let Some(decl_ok) = parse_decl("hotkey=ctrl+alt+f9|全局截屏增强".as_bytes()) else {
        set.add("conflict case 3: rebind hits third-party occupant", false, "");
        return;
    };
    let app2 = AppId::from_bytes(b"vxapp-shot");
    let o3a = reg.register_app_decl(app2, &decl_ok, 0, 1300);
    let copy_uid = uid_of_label("sys.copy");
    let o3b = reg.rebind(copy_uid, Combo::new(MOD_CTRL | MOD_ALT, VK_F1 + 8), 1400);
    set.add(
        "conflict case 3: rebind hits third-party occupant",
        o3a == RegOutcome::Registered
            && matches!(o3b, RebindOutcome::Occupied(ref occ) if !occ.is_system() && occ.app.as_bytes() == b"vxapp-shot"),
        "",
    );
}

/// 判定 6~8：用户改键派发翻转 + 改键持久化 round-trip + 禁用位。返回
/// 装载目标注册表 reg2（判定 8 对拍两表面用）。
fn chk_rebind_and_persistence(set: &mut CheckSet, reg: &mut HotkeyReg) -> HotkeyReg {
    // 改键成功路径：sys.copy → Ctrl+Shift+C；新旧派发面翻转。
    let copy_uid = uid_of_label("sys.copy");
    let r6 = reg.rebind(copy_uid, Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 1500);
    set.add(
        "user rebind flips dispatch",
        r6 == RebindOutcome::Done
            && reg.dispatch(Combo::new(MOD_CTRL | MOD_SHIFT, 0x43)).map_or(false, |e| e.uid() == copy_uid && e.overridden)
            && reg.entry_by_uid(copy_uid).map_or(true, |e| e.combo != Combo::new(MOD_CTRL, 0x43))
            && reg.version >= 5,
        "",
    );

    // 改键持久化 round-trip：导出差异 → 新表装载 → 状态一致。
    let _ = reg.set_enabled(uid_of_label("sys.undo"), false, 1600);
    let mut blob = [0u8; 256];
    let n = reg.export_overrides(&mut blob);
    // 修障登记（check 7 红）：applied 是生效条数、n 是导出字节长度
    // （6 + 条数×9），二者结构性不等——对拍对象改为 blob 头部声明的
    // 差异条数（判据本体：每条导出差异都须生效）。
    let want = u16::from_le_bytes([blob[4], blob[5]]) as usize;
    let mut reg2 = HotkeyReg::new();
    let _ = reg2.preset_all(2000);
    let applied = reg2.import_overrides(&blob[..n], 2100);
    // 条目缺失 = 判定面前置失败：显性记红（可恢复路径，不 panic）。
    let rt_ok = match (reg.entry_by_uid(copy_uid), reg2.entry_by_uid(copy_uid)) {
        (Some(e_src), Some(e_dst)) => {
            e_dst.combo == e_src.combo && e_dst.overridden == e_src.overridden
        }
        _ => false,
    };
    set.add(
        "rebind persistence round-trip",
        n > 0
            && applied == want
            && rt_ok
            && reg2.entry_by_uid(uid_of_label("sys.undo")).map_or(false, |e| !e.enabled),
        "",
    );

    // 禁用位：禁用后活跃派发不命中、普通派发仍可查（设置中心可查可禁用）。
    let undo_uid = uid_of_label("sys.undo");
    set.add(
        "disabled entry visible but not active",
        reg.dispatch(Combo::new(MOD_CTRL, 0x5A)).map_or(false, |e| e.uid() == undo_uid)
            && reg.dispatch_active(Combo::new(MOD_CTRL, 0x5A)).is_none()
            && reg2.dispatch_active(Combo::new(MOD_CTRL, 0x5A)).is_none(),
        "",
    );
    reg2
}

/// 判定 9~10：恢复默认一键（全表回出厂、审计转绿、版本推进）+ 清单
/// 声明解析（合法行逐项正确、三类非法行显性拒绝）。
fn chk_reset_and_parse(set: &mut CheckSet, reg: &mut HotkeyReg) {
    let v_before = reg.version;
    let restored = reg.reset_all(3000);
    set.add(
        "reset all restores factory",
        restored >= 2
            && reg.audit_presets()
            && reg.version > v_before
            && reg.entry_by_uid(uid_of_label("sys.copy")).map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43)),
        "",
    );

    let d1 = parse_decl("hotkey=ctrl+shift+f5|快速收集".as_bytes());
    let bad1 = parse_decl(b"hotkey=ctrl|no key");
    let bad2 = parse_decl(b"hotkey=ctrl+f10+f11|double key");
    let bad3 = parse_decl(b"hotkey=hyper+f1|unknown mod");
    set.add(
        "manifest parse valid & explicit rejects",
        d1.map_or(false, |d| d.mods == (MOD_CTRL | MOD_SHIFT) && d.key == VK_F1 + 4 && d.desc.as_bytes() == "快速收集".as_bytes())
            && bad1.is_none()
            && bad2.is_none()
            && bad3.is_none(),
        "",
    );
}

/// 判定 11：第三方接入验证（两条声明注册、apps() 去重列出、stats()
/// 计数）。声明行解析失败 = 判定面构造错误：显性记红，不 panic。
fn chk_third_party_intake(set: &mut CheckSet) {
    let mut reg3 = HotkeyReg::new();
    let _ = reg3.preset_all(4000);
    let (Some(d_a), Some(d_b)) = (
        parse_decl("hotkey=ctrl+alt+7|应用 A 快捷动作一".as_bytes()),
        parse_decl("hotkey=ctrl+alt+8|应用 A 快捷动作二".as_bytes()),
    ) else {
        set.add("third-party intake & stats", false, "");
        return;
    };
    let appa = AppId::from_bytes(b"vxapp-a");
    let ra1 = reg3.register_app_decl(appa, &d_a, 0, 4100);
    let ra2 = reg3.register_app_decl(appa, &d_b, 1, 4200);
    let mut app_list = [AppId::none(); 8];
    let napps = reg3.apps(&mut app_list);
    let st = reg3.stats();
    set.add(
        "third-party intake & stats",
        ra1 == RegOutcome::Registered
            && ra2 == RegOutcome::Registered
            && napps == 1
            && app_list[0].as_bytes() == b"vxapp-a"
            && st.app == 2
            && st.total == PRESETS.len() + 2,
        "",
    );
}

/// 判定 12：键位显示串（设置中心显示面）——四条参考例逐字核对。
fn chk_combo_display(set: &mut CheckSet) {
    // 各用独立缓冲——s1..s4 借用各自数组，避免借用冲突（零堆纪律不变）。
    let mut b1 = [0u8; 24];
    let mut b2 = [0u8; 24];
    let mut b3 = [0u8; 24];
    let mut b4 = [0u8; 24];
    let n1 = combo_str(Combo::new(MOD_CTRL | MOD_SHIFT, 0x56), &mut b1);
    let s1 = core::str::from_utf8(&b1[..n1]).unwrap_or("?");
    let n2 = combo_str(Combo::new(MOD_ALT, VK_TAB), &mut b2);
    let s2 = core::str::from_utf8(&b2[..n2]).unwrap_or("?");
    let n3 = combo_str(Combo::new(0, VK_LWIN), &mut b3);
    let s3 = core::str::from_utf8(&b3[..n3]).unwrap_or("?");
    let n4 = combo_str(Combo::new(0, VK_F1 + 4), &mut b4);
    let s4 = core::str::from_utf8(&b4[..n4]).unwrap_or("?");
    set.add(
        "combo display strings",
        s1 == "Ctrl+Shift+V" && s2 == "Alt+Tab" && s3 == "Win" && s4 == "F5",
        "",
    );
}

/// F244 自检（判据：完整性审计、冲突 3 例、改键持久化、第三方接入；
/// 含 xors32 fuzz）。判定按组拆分为独立子函数（单一职责、可独立追溯）：
/// 判定次序与原实现逐条一致，组间共享状态经返回值显性传递。
pub fn run_hotkeyreg_checks() -> CheckSet {
    let mut set = CheckSet::new("F244-hotkeyreg");
    // 判定 1~2：预置安装 + 完整性审计。
    let mut reg = chk_presets_installed(&mut set);
    // 判定 3~5：冲突即检三例。
    chk_conflict_cases(&mut set, &mut reg);
    // 判定 6~8：改键翻转 + 持久化 round-trip + 禁用位。
    chk_rebind_and_persistence(&mut set, &mut reg);
    // 判定 9~10：恢复默认 + 清单解析。
    chk_reset_and_parse(&mut set, &mut reg);
    // 判定 11：第三方接入。
    chk_third_party_intake(&mut set);
    // 判定 12：显示串。
    chk_combo_display(&mut set);
    // 判定 13：xors32 fuzz 2000 轮——注册成功必可派发、冲突必显性拒绝
    //     且原条目键位不变、表容量有界、无 panic。
    set.add(
        "fuzz 2000 registry ops invariants hold",
        fuzz_registry_2000(),
        "",
    );
    // 判定 14：序列键状态机 + 派发留痕 + 批量注销 + 两级审计。
    chk_chords_and_dispatch(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 判定 14：序列键状态机 + 派发留痕 + 批量注销 + 两级审计
// ---------------------------------------------------------------------------

/// 判定 14 之派发留痕与热度对账（判定主链的只读复查段）。
fn dispatch_log_asserts(g: &HotkeyReg) -> bool {
    let copy_uid = uid_of_label("sys.copy");
    let dlog = g.dispatch_log();
    g.dispatch_log_len() == 2
        && dlog[0].map_or(false, |r| r.uid == copy_uid && r.active && r.ts == 510)
        && g.hits_for(copy_uid) == 2
        && g.total_hits() == 2
}

/// 判定 14 之弦键登记与预置分组计数（只读复查段）。
fn chord_state_asserts(g: &HotkeyReg, f2: Combo, s2: Combo) -> bool {
    g.chord_count() == 1
        && g.chord_by_label("app.emacs.kill").map_or(false, |c| c.first == f2 && c.second == s2)
        && g.preset_category_counts() == [12, 13, 15, 6]
}

/// 判定 14 之应用批量注销与两级审计断言（只读复查段；判定次序与原
/// 实现的断言集合逐条一致，仅归组收纳）。
fn purge_audit_asserts(g: &HotkeyReg, app3: AppId, r3a: RegOutcome, r3b: RegOutcome, removed: usize) -> bool {
    let copy_uid = uid_of_label("sys.copy");
    r3a == RegOutcome::Registered
        && r3b == RegOutcome::Registered
        && removed == 2
        && g.entry_by_uid(fnv1a(b"vxapp-zap").rotate_left(8)).is_none()
        && !g.changes_for_uid(copy_uid).is_empty()
        && g.changes_for_uid(copy_uid)[0].kind == ChangeKind::Register
        && g.change_log_len() >= 2
        && g.audit_conflict_free()
        && g.audit_chord_no_collision()
        && CHORD_DOC.contains("800ms")
        && CONFLICT_HINT_DOC.contains("占用方")
}

/// 判定 14：序列键状态机 + 派发留痕与热度计数 + 应用批量注销 + 预置
/// 分组计数 + 两级审计（g 表自含，不与其它判定共享状态）。
fn chk_chords_and_dispatch(set: &mut CheckSet) {
    // 登记门（段位撞单键拒绝/第一段互异）→ 逐键状态机（Armed 吞键/
    // 窗内第二段 Fired/超时回落）→ 派发留痕与热度计数对账 → 应用批量
    // 注销 → 预置分组计数 → 两级审计全绿。
    let mut g = HotkeyReg::new();
    let _ = g.preset_all(100);
    let f1 = Combo::new(MOD_CTRL, 0x58); // Ctrl+X
    let f2 = Combo::new(MOD_CTRL | MOD_ALT, 0x4B); // Ctrl+Alt+K
    let s2 = Combo::new(MOD_CTRL | MOD_ALT, 0x4C); // Ctrl+Alt+L
    let ok1 = g.register_chord("app.emacs.kill", f2, s2, 200);
    // 第一段撞单键（Ctrl+X 与 sys.cut 冲突）→ 显性拒绝。
    let bad1 = g.register_chord("app.bad.chord", f1, s2, 210) == false;
    let dup_first = g.register_chord("app.dup.chord", f2, Combo::new(MOD_ALT, 0x4A), 220) == false;
    let chord_step = g.chord_key(f2, 300);
    let armed_hint = g.chord_armed_label();
    let fired = g.chord_key(s2, 400);
    let idle_after = g.chord_armed_label().is_none();
    // 普通单键派发带留痕 + 热度计数。
    let hit = g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 500); // sys.copy
    let copy_uid = uid_of_label("sys.copy");
    let _ = g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 510);
    let app3 = AppId::from_bytes(b"vxapp-zap");
    // 修障登记（check 14 红）：哈希表以键位码为主键，同一组合键（Alt+J）
    // 注册两条必撞冲突门（第二条 Occupied、不入表），`removed == 2`
    // 结构性不可达——两条声明改为互异键位（Alt+J / Alt+K），批量注销
    // 判据恢复可满足，并以注册成功前置断言补强。
    let d3 = ParsedDecl { mods: MOD_ALT, key: 0x4A, desc: DescBuf::from_bytes(b"zap") };
    let d4 = ParsedDecl { mods: MOD_ALT, key: 0x4B, desc: DescBuf::from_bytes(b"zap2") };
    let r3a = g.register_app_decl(app3, &d3, 0, 600);
    let r3b = g.register_app_decl(app3, &d4, 1, 610);
    let removed = g.unregister_app(app3, 620);
    set.add(
        "chord state machine & dispatch log & app purge",
        ok1 && bad1 && dup_first
            && chord_step == ChordStep::Armed
            && armed_hint == Some("app.emacs.kill")
            && fired == ChordStep::Fired { label: "app.emacs.kill" }
            && idle_after
            && hit.map_or(false, |e| e.uid() == copy_uid)
            && dispatch_log_asserts(&g)
            && chord_state_asserts(&g, f2, s2)
            && purge_audit_asserts(&g, app3, r3a, r3b, removed),
        "",
    );
}
