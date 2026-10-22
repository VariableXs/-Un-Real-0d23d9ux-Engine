//! F244 全局快捷键注册表与冲突审计 · 判据实装。
//!
//! **判据锚**：主册 F244「全局快捷键注册表与冲突审计」。
//!
//! **验收标准第一句（任务包原文）**：注册表条目完整性审计（系统快捷键
//! 全量入表）。
//!
//! **判据（主册原文摘录）**：所有系统级快捷键集中一处注册表（设置中心
//! 可查可改可禁用），注册时冲突即检——新快捷键与已有冲突时明确列出占用
//! 方让用户选（先到先得+手动改），禁止静默覆盖；第三方应用申请全局热键
//! 走同一注册表（vxapp 清单声明），冲突同样可见。注册表带「恢复默认」
//! 一键。验收：冲突检测用例（故意注册冲突 3 例）；用户改键持久化；
//! 第三方声明接入验证。
//!
//! **设计要点**：键域=修饰键 4 位+Windows VK 码域 u16（锚定 VK 原值，键位编码一处一事实）；
//! 冲突即检哈希表：32 位码 → 乘法散列 → 96 槽线性探测，注册/改键 O(1) 冲突判定，
//! 删除用后退移位法（Knuth 6.4 Algorithm R）保持无墓碑探测链；冲突语义=先到先得+显性拒绝
//! （`Occupied` 携带占用方身份），**无静默覆盖**；系统预置表 46 条全量入表，完整性审计逐条核对；
//! 改键持久化只导出差异条目（9 字节/条），导入逐条冲突复核——round-trip 语义完整；
//! 第三方接入走 vxapp 清单声明行（逐 token 校验），注册走同一冲突门；恢复默认一键全表回出厂；
//! 零堆热路径：派发/冲突检测/审计全定长数组，时间一律注入（毫秒戳）。
//!
//! **依赖锚点**：F220（Ctrl+Shift+V 纯文本粘贴——预置表一条）、
//! F249（热角同源动作 Win+A/Win+N——预置表两条）、F081（任务视图
//! Win+Tab）、F235（虚拟桌面 Win+Ctrl+方向）。
//!
//! **模块布局**（拆分外壳——pub 路径与本模块不变，crate 其余部分无感）：
//! - `keys`：键域（修饰键位/VK 锚点/Combo/键名互转/显示串）；
//! - `registry`：条目与身份、注册表本体核心、系统预置表；
//! - `registry::ops`：chord 运行面、批量查询、持久化导出导入；
//! - `decl`：vxapp 清单声明解析；
//! - `checks`：自检判定面（fuzz + 判定 1~14）；
//! - `v2`：v2 深化批（持久化快照册 / UI 壳接线 / v2 判定面）。

mod checks;
mod decl;
mod keys;
mod registry;
mod v2;

// 键域（keys）
pub use self::keys::{
    MOD_ALT, MOD_CTRL, MOD_MASK, MOD_SHIFT, MOD_WIN, VK_0, VK_A, VK_BACK, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RETURN,
    VK_RIGHT, VK_SNAPSHOT, VK_SPACE, VK_TAB, VK_UP, Combo, combo_str, key_name, name_to_key,
};

// 注册表本体与预置表（registry）
pub use self::registry::{
    AppId, CATEGORY_NAMES, CHANGE_CAP, CHORD_CAP, CHORD_DOC, CHORD_TIMEOUT_MS, Change,
    ChangeKind, ChordRec, ChordStep, DISPATCH_CAP, DescBuf, DispatchRec, Entry, HK_CAP,
    HotkeyReg, Occupant, PRESET_CAT, PRESETS, RegOutcome, RegStats, RebindOutcome, Source,
    chord_uid, fnv1a, uid_of_label,
};

// 清单声明解析（decl）
pub use self::decl::{ParsedDecl, parse_decl};

// 自检判定面（checks）
pub use self::checks::run_hotkeyreg_checks;

// v2 深化批（v2）
pub use self::v2::{
    HkRow, HkSnap, HkSnapBook, ListNav, VX2_HK_BLOB, VX2_HK_PAYLOAD, VX2_HK_SNAP_CAP,
    VX2_KEY_DOWN, VX2_KEY_ENTER, VX2_KEY_UP, VX2_ROW_H_PX, VX2_TEXT_CAP, Vx2Error, hk_list_nav,
    hk_row_hit, hk_rows, run_hotkeyreg_v2_checks,
};

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_table_has_no_internal_collision() {
        let mut reg = HotkeyReg::new();
        for (label, combo, desc) in PRESETS.iter() {
            assert_eq!(
                reg.register_system(*label, *combo, *desc, 1),
                RegOutcome::Registered,
                "预置表内部冲突：{label}"
            );
        }
        assert_eq!(reg.count, PRESETS.len());
    }

    #[test]
    fn backward_shift_keeps_probe_chain_intact() {
        // 密集注册 → 从中段抽删 → 全部幸存条目必须仍可派发（后退移位
        // 正确性的直接验证，线性探测删除的经典坑）。
        let mut reg = HotkeyReg::new();
        let mut labels: [&'static str; 40] = [""; 40];
        for i in 0..40usize {
            // 每条不同的键位码（key = A+i, mods 交错制造同槽聚集）。
            let label: &'static str = LABELS[i];
            labels[i] = label;
            let combo = Combo::new((i % 4) as u8 * MOD_CTRL, VK_A + i as u16);
            assert_eq!(reg.register_system(label, combo, "", 1), RegOutcome::Registered);
        }
        // 删除全部偶数下标条目。
        for i in (0..40usize).step_by(2) {
            assert!(reg.unregister(uid_of_label(labels[i]), 2));
        }
        for i in (1..40usize).step_by(2) {
            let combo = Combo::new((i % 4) as u8 * MOD_CTRL, VK_A + i as u16);
            assert_eq!(
                reg.dispatch(combo).map(|e| e.uid()),
                Some(uid_of_label(labels[i])),
                "后退移位后探测链断裂 i={i}"
            );
        }
        assert_eq!(reg.count, 20);
    }

    const LABELS: [&str; 40] = [
        "t00", "t01", "t02", "t03", "t04", "t05", "t06", "t07", "t08", "t09", "t10", "t11",
        "t12", "t13", "t14", "t15", "t16", "t17", "t18", "t19", "t20", "t21", "t22", "t23",
        "t24", "t25", "t26", "t27", "t28", "t29", "t30", "t31", "t32", "t33", "t34", "t35",
        "t36", "t37", "t38", "t39",
    ];

    #[test]
    fn combo_str_bare_win_and_unknown() {
        let mut buf = [0u8; 24];
        let n = combo_str(Combo::new(0, VK_LWIN), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("Win"));
        let n2 = combo_str(Combo::new(MOD_WIN | MOD_CTRL, VK_F1 + 9), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n2]), Ok("Ctrl+Win+F10"));
        let n3 = combo_str(Combo::new(0, 0xFF), &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n3]), Ok("?"));
    }

    #[test]
    fn import_rejects_malformed_buffers() {
        let mut reg = HotkeyReg::new();
        assert_eq!(reg.import_overrides(&[], 1), 0);
        assert_eq!(reg.import_overrides(b"VXHK", 1), 0);
        assert_eq!(reg.import_overrides(b"NOPE\x00\x00", 1), 0);
        // 声明条数与实际字节数不符。
        assert_eq!(reg.import_overrides(b"VXHK\x03\x00", 1), 0);
    }

    #[test]
    fn rebind_idempotent_and_none_such() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        let uid = uid_of_label("sys.refresh");
        // 不存在的 uid。
        assert_eq!(reg.rebind(0xDEAD_BEEF, Combo::new(MOD_CTRL, 0x42), 2), RebindOutcome::NoSuchId);
        // 同键位重改 = 幂等 Done，不推进版本、不新增留痕（环内既有的
        // 出厂 Register 不算新增——对账口径以 change_log_len 零增量为准）。
        let v0 = reg.version;
        let len0 = reg.change_log_len();
        assert_eq!(reg.rebind(uid, Combo::new(0, VK_F1 + 4), 3), RebindOutcome::Done);
        assert_eq!(reg.version, v0);
        assert_eq!(reg.change_log_len(), len0, "同值改键不得新增留痕");
        // 纯修饰键（无键）→ Malformed。
        assert_eq!(reg.rebind(uid, Combo::new(MOD_CTRL, 0), 4), RebindOutcome::Malformed);
    }

    #[test]
    fn chord_timeout_and_fallthrough() {
        let mut g = HotkeyReg::new();
        let _ = g.preset_all(0);
        let f = Combo::new(MOD_CTRL | MOD_ALT, 0x4B);
        let s = Combo::new(MOD_CTRL | MOD_ALT, 0x4C);
        assert!(g.register_chord("app.demo", f, s, 0));
        // 窗内第二段 → Fired。
        assert_eq!(g.chord_key(f, 100), ChordStep::Armed);
        assert_eq!(g.chord_key(s, 500), ChordStep::Fired { label: "app.demo" });
        // 超时回落：窗外第二段不触发，回落为 Idle。
        assert_eq!(g.chord_key(f, 1000), ChordStep::Armed);
        assert_eq!(g.chord_key(s, 1000 + CHORD_TIMEOUT_MS + 1), ChordStep::Idle);
        assert!(g.chord_armed_label().is_none());
        // 回落后的第二段若是单键 → 正常派发（序列键不吞单键）。
        let copy_uid = uid_of_label("sys.copy");
        assert!(g.dispatch_logged(Combo::new(MOD_CTRL, 0x43), 2000).map_or(false, |e| e.uid() == copy_uid));
        // 主动复位面。
        let _ = g.chord_key(f, 3000);
        assert!(g.expire_chord());
        assert!(!g.expire_chord());
        assert!(g.audit_chord_no_collision());
    }

    #[test]
    fn unregister_app_purges_all_and_rehash_holds() {
        let mut g = HotkeyReg::new();
        let app = AppId::from_bytes(b"vxapp-many");
        // 同应用注册 6 条不同键位声明。
        for seq in 0..6u8 {
            let d = ParsedDecl {
                mods: MOD_ALT,
                key: 0x61 + (seq as u16) * 3, // Alt+J,Alt+M,...（避开预置键域）
                desc: DescBuf::from_bytes(b"bulk"),
            };
            assert_eq!(g.register_app_decl(app, &d, seq, seq as u64), RegOutcome::Registered);
        }
        assert_eq!(g.preset_all(10), PRESETS.len());
        let mut uids = [0u32; 8];
        let un = g.app_uids(&app, &mut uids);
        assert_eq!(un, 6);
        // 批量注销后：全部不可派发、表计数正确、审计双绿。
        assert_eq!(g.unregister_app(app, 20), 6);
        for &u in uids[..un].iter() {
            assert!(g.entry_by_uid(u).is_none());
        }
        assert_eq!(g.count, PRESETS.len());
        assert!(g.audit_conflict_free());
        assert!(g.audit_presets());
        // 注销面留痕可查（每条 uid 都有 Unregister 事件）。
        for &u in uids[..un].iter() {
            let chain = g.changes_for_uid(u);
            assert!(chain.iter().any(|c| c.kind == ChangeKind::Unregister));
        }
        // 再注册同应用同键位 → 重新占位成功（注销彻底、无残留墓碑）。
        let d = ParsedDecl { mods: MOD_ALT, key: 0x61, desc: DescBuf::from_bytes(b"again") };
        assert_eq!(g.register_app_decl(app, &d, 0, 30), RegOutcome::Registered);
    }

    #[test]
    fn hotkeyreg_selfcheck_all_green() {
        let set = run_hotkeyreg_checks();
        assert!(set.all_passed(), "F244 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_snapshot_roundtrip_and_reject() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        let _ = reg.rebind(uid_of_label("sys.find"), Combo::new(MOD_ALT, 0x46), 2);
        let b = HkSnapBook::snapshot(&reg).expect("diff under cap");
        let mut buf = [0u8; VX2_HK_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_HK_BLOB);
        assert_eq!(HkSnapBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[9] ^= 0x01;
        assert_eq!(HkSnapBook::from_bytes(&bad), Err(Vx2Error::BadChecksum));
        assert_eq!(HkSnapBook::from_bytes(&buf[..9]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_rows_text_uses_combo_str() {
        let mut reg = HotkeyReg::new();
        let _ = reg.preset_all(1);
        // 修障登记：hk_rows 按哈希槽序输出，rows[0] 未必是 sys.copy、
        // sys.copy 也未必落在 4 行视窗内——缓冲扩到全量后按 label 定位
        // 再核对显示串（与 v2 check 4 同病同修）。
        let mut rows = [HkRow { uid: 0, label: "", y: 0, h: 0, enabled: false, overridden: false, text_len: 0, text: [0u8; VX2_TEXT_CAP] }; 48];
        let n = hk_rows(&reg, &mut rows);
        assert_eq!(n, PRESETS.len());
        let pos = (0..n).find(|&k| rows[k].label == "sys.copy").expect("sys.copy in rows");
        let s = core::str::from_utf8(&rows[pos].text[..rows[pos].text_len]).expect("combo str");
        assert_eq!(s, "Ctrl+C");
        for k in 1..n {
            assert!(rows[k].y >= rows[k - 1].y + rows[k - 1].h, "行矩形不得重叠");
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_hotkeyreg_v2_checks();
        assert!(set.all_passed(), "F244 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
