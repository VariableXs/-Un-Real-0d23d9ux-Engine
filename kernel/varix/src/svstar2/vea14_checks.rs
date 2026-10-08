//! VE-F0014 · 域自检（判据逐条对应，见 `vea14_snapshot.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 状态快照（设备丢失前后对比诊断） → `A14-快照-采集与丢失原因`
//! - 快照含差量模式（两次快照只存差异省体积） → `A14-快照-差量只存差异`
//! - 快照体积预算（超预算→裁剪声明） → `A14-预算-裁剪声明`
//! - 关键状态的定义公开（存什么不存什么写明） → `A14-预算-关键状态公开`
//! - 快照含加密选项 → `A14-快照-加密选项`
//! - 重放确定性（同快照同渲染序列） → `A14-重放-确定性`
//! - 重放含与实机对拍校验（偏差量化） → `A14-重放-对拍偏差量化`
//! - 重放失真→归因 → `A14-重放-失真归因`
//! - 快照损坏→标注 → `A14-防护-损坏标注拒重放`
//! - 隐私红线（凭据/墙钟不入快照） → `A14-防护-隐私键源头拒绝`
//! - A09 诊断联动 → `A14-衔接-A09契约`
//! - 快照状态读屏可达 → `A14-读屏-快照摘要`
//!
//! 逻辑时钟注入、零墙钟，回归可复现。

use super::vea14_snapshot::*;
use crate::checks::CheckSet;

/// VE-F0014 域自检。
pub fn run_vea14_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea14");

    fn state() -> Vec<(&'static str, &'static str)> {
        vec![
            ("viewport", "3840x2160"),
            ("clear_color", "#101418"),
            ("blend_mode", "over"),
            ("shader_profile", "sm2_soft"),
            ("vsync", "on"),
            ("texture_cache_bytes", "18446744073709551"),
            ("cmd_queue_depth", "3"),
        ]
    }

    fn seq() -> Vec<ReplayStep> {
        vec![
            ReplayStep::Clear(0),
            ReplayStep::Rect(10, 20, 300, 200),
            ReplayStep::Blend(1),
        ]
    }

    // ---- 状态快照：设备丢失前后对比诊断 ----

    {
        let mut s = Snapshotter::new();
        let before = s.capture(10, &state(), false, false, "");
        let mut after_state = state();
        after_state[5] = ("texture_cache_bytes", "0");
        after_state[6] = ("cmd_queue_depth", "0");
        let after = s.capture(11, &after_state, false, false, "TDR 重置");
        let comparable = before.entries.len() == after.entries.len()
            && after.lost_reason == "TDR 重置"
            && before.lost_reason.is_empty()
            && before.intact()
            && after.intact();
        set.add("A14-快照-采集与丢失原因", comparable, "");
    }

    // ---- 差量模式 ----

    // 判据：两次快照只存差异省体积（同键同值剔除，delta_of 指向基线）。
    {
        let mut s = Snapshotter::new();
        let _base = s.capture(1, &state(), false, false, "");
        let mut st2 = state();
        st2[2] = ("blend_mode", "multiply");
        let d = s.capture(2, &st2, true, false, "");
        let ok = d.delta_of == Some(1)
            && d.entries.len() == 1
            && d.entries[0].key == "blend_mode"
            && d.entries[0].val == "multiply";
        set.add("A14-快照-差量只存差异", ok, "");
    }

    // ---- 体积预算 ----

    // 判据：超预算→裁剪声明（真裁剪路径：注入小预算，10 项 > 3 上限触发）。
    {
        let mut s = Snapshotter::new();
        s.set_budget_override(3);
        let mut st = state();
        st.push(("probe_aux_a", "AAAAAAAAAAAAAAAAAAAA"));
        st.push(("probe_aux_b", "BBBB"));
        st.push(("probe_aux_c", "CCCCCCCCCCCCCCCC"));
        let snap = s.capture(12, &st, false, false, "");
        let trimmed = snap.entries.len() == 3 && snap.trimmed.len() == 7;
        let declared = s.trim_decls.iter().any(|(f, m)| *f == 12 && m.contains("超预算裁剪 7 项"));
        // 生产默认预算应大于声明键数（正常工作区不裁剪）。
        let mut s2 = Snapshotter::new();
        let normal = s2.capture(13, &st, false, false, "");
        let headroom = normal.trimmed.is_empty()
            && KEY_STATE_KEYS.len() < SNAPSHOT_BUDGET_ENTRIES;
        set.add("A14-预算-裁剪声明", trimmed && declared && headroom, "");
    }

    // ---- 关键状态定义公开 ----

    // 判据：存什么不存什么写明（每键带公开理由，隐私键显式 false）。
    {
        let declared = KEY_STATE_KEYS.len() >= 13;
        let store = KEY_STATE_KEYS.iter().filter(|(_, s, _)| *s).count();
        let nos = KEY_STATE_KEYS.iter().filter(|(_, s, _)| !*s).count();
        let reasons = KEY_STATE_KEYS.iter().all(|(_, _, why)| !why.is_empty());
        let privacy = KEY_STATE_KEYS.iter().any(|(k, s, _)| *k == "raw_password_or_token" && !s);
        let clock = KEY_STATE_KEYS.iter().any(|(k, s, _)| *k == "wall_clock" && !s);
        set.add(
            "A14-预算-关键状态公开",
            declared && store >= 10 && nos >= 3 && reasons && privacy && clock,
            "",
        );
    }

    // ---- 加密选项 ----

    // 判据：诊断数据可含敏感场景——加密旗标 + 载荷混淆 + 校验一致。
    {
        let mut s = Snapshotter::new();
        let e = s.capture(9, &state(), false, true, "");
        let plain = s.capture(10, &state(), false, false, "");
        let ok = e.encrypted
            && !e.encrypted == plain.encrypted
            && e.intact()
            && e.payload != plain.payload
            && !String::from_utf8_lossy(&e.payload).contains("clear_color=");
        set.add("A14-快照-加密选项", ok, "");
    }

    // ---- 重放确定性 ----

    // 判据：同快照同渲染序列 = 同结果；状态或序列变则摘要变（可证伪）。
    {
        let mut s = Snapshotter::new();
        let snap = s.capture(3, &state(), false, false, "");
        let d1 = Replayer::replay(&snap, &seq()).unwrap();
        let d2 = Replayer::replay(&snap, &seq()).unwrap();
        let same = d1 == d2;
        let mut seq2 = seq();
        seq2.push(ReplayStep::Rect(1, 1, 2, 3));
        let different_seq = Replayer::replay(&snap, &seq2).unwrap() != d1;
        let mut snap2 = snap.clone();
        snap2.entries[0].val = "1920x1080".to_string();
        let different_state = Replayer::replay(&snap2, &seq()).unwrap() != d1;
        set.add("A14-重放-确定性", same && different_seq && different_state, "");
    }

    // ---- 实机对拍校验 ----

    // 判据：重放结果与实际渲染的偏差量化（divergence 数字 + 容限判定）。
    {
        let mut s = Snapshotter::new();
        let snap = s.capture(4, &state(), false, false, "");
        let d = Replayer::replay(&snap, &seq()).unwrap();
        let zero = Replayer::verify(&snap, &seq(), d);
        let quant = Replayer::verify(&snap, &seq(), d.wrapping_add(7));
        set.add(
            "A14-重放-对拍偏差量化",
            !zero.divergent
                && zero.divergence == 0
                && quant.divergent
                && quant.divergence == 7,
            "",
        );
    }

    // ---- 重放失真→归因 ----

    // 判据：失真输出归因（最可疑差异键进文本）。
    {
        let mut s = Snapshotter::new();
        let snap = s.capture(5, &state(), false, false, "");
        let d = Replayer::replay(&snap, &seq()).unwrap();
        let r = Replayer::verify(&snap, &seq(), d.wrapping_add(0x1234));
        set.add(
            "A14-重放-失真归因",
            r.divergent && r.attribution.contains("最可疑差异键"),
            "",
        );
    }

    // ---- 损坏→标注 ----

    // 判据：校验和失配标注 E_SNAPSHOT_CORRUPT，拒绝重放不静默。
    {
        let mut s = Snapshotter::new();
        let mut snap = s.capture(6, &state(), false, false, "");
        snap.payload[1] ^= 0x55;
        let marked = !snap.intact();
        let refused = Replayer::replay(&snap, &seq()) == Err("E_SNAPSHOT_CORRUPT");
        let v = Replayer::verify(&snap, &seq(), 0);
        set.add(
            "A14-防护-损坏标注拒重放",
            marked && refused && v.divergent && v.attribution == "E_SNAPSHOT_CORRUPT",
            "",
        );
    }

    // ---- 隐私红线 ----

    // 判据：凭据/墙钟/驱动私有块在源头拒绝（载荷零泄漏 + 拒绝入账）。
    {
        let mut s = Snapshotter::new();
        let mut st = state();
        st.push(("raw_password_or_token", "topsecret"));
        st.push(("wall_clock", "2062-01-01"));
        st.push(("driver_private_blob", "0xDEADBEEF"));
        st.push(("not_in_keys", "v"));
        let snap = s.capture(7, &st, false, false, "");
        let leak = String::from_utf8_lossy(&snap.payload);
        let clean = !leak.contains("topsecret")
            && !leak.contains("2062-01-01")
            && !leak.contains("0xDEADBEEF")
            && !leak.contains("not_in_keys");
        set.add(
            "A14-防护-隐私键源头拒绝",
            clean && s.key_rejects.len() == 4,
            "",
        );
    }

    // ---- A09 衔接 ----

    {
        set.add("A14-衔接-A09契约", A09_LINK >= 1 && REPLAY_TOLERANCE == 0, "");
    }

    // ---- 读屏可达 ----

    {
        let mut s = Snapshotter::new();
        let snap = s.capture(8, &state(), false, true, "热拔");
        let t = Snapshotter::screen_text(&snap);
        set.add(
            "A14-读屏-快照摘要",
            t.contains("快照状态") && t.contains("已加密") && t.contains("帧 8"),
            "",
        );
    }

    set
}
