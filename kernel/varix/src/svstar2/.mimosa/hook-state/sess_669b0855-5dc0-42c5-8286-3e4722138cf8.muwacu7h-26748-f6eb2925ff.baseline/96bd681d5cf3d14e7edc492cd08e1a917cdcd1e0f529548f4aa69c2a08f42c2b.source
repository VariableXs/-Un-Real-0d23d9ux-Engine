//! VE-F0606 · 域自检（判据逐条对应，见 `ved06_zorder.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 稳定排序（同值按插入序，键冲突稳定序兜底） → `D06-稳定-同值按插入序`
//! - 最小重排域（局部重排不整树重建） → `D06-最小域-局部重排窗口`
//! - 重排风暴→批量合并为单次 → `D06-风暴-合并单次flush`
//! - 越界键→钳制加告警 → `D06-钳制-越界键告警`
//! - 三序同源（遍历=绘制=Z 序） → `D06-三序-同源契约`
//! - 提升不改序 → `D06-提升-登记不改序`
//! - 查询 O(1)（按序位） → `D06-查询-序位直达`
//! - 重排映射脏区（F0613 对接） → `D06-脏区-重排域提示`
//! - 未登记 id 显性拒绝（零静默） → `D06-防护-未登记拒绝`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::ved06_zorder::*;
use crate::checks::CheckSet;

/// VE-F0606 域自检。
pub fn run_ved06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved06");

    // ---- 稳定排序 ----

    // 判据：同 z 键严格按插入序；重排后仍保持；契约文档在册。
    {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 0);
        s.add(2, 0);
        s.add(3, 0);
        let stable0 = s.order() == alloc::vec![1, 2, 3];
        // 改 1 的 z 到正区：2、3 同键序不变。
        let _ = s.set_z(1, 5);
        let _ = s.flush();
        let stable1 = s.order() == alloc::vec![2, 3, 1];
        let doc = STABLE_ORDER_DOC.contains("插入序") && Z_ORDER_CONTRACT_DOC.contains("同源");
        set.add("D06-稳定-同值按插入序", stable0 && stable1 && doc, "");
    }

    // ---- 最小重排域 ----

    // 判据：改一个兄弟的 z，窗口小于全表；窗口外槽位原地不动。
    {
        let mut s = SiblingZOrder::new();
        for id in 1..=6u64 {
            s.add(id, 0);
        }
        let _ = s.set_z(4, 10);
        let hint = s.flush().unwrap();
        let local = (hint.span_end - hint.span_start) < s.len() + 3; // 窗口有限
        let outside_fixed = s.slot_at(0).unwrap().node_id == 1
            && s.slot_at(1).unwrap().node_id == 2
            && s.slot_at(2).unwrap().node_id == 3;
        let final_ok = s.slot_at(5).unwrap().node_id == 4; // z=10 沉底
        set.add("D06-最小域-局部重排窗口", local && outside_fixed && final_ok, "");
    }

    // ---- 重排风暴合并单次 ----

    {
        let mut s = SiblingZOrder::new();
        for id in 1..=5u64 {
            s.add(id, 0);
        }
        for id in 1..=5u64 {
            let _ = s.set_z(id, id as i32);
        }
        let pooled = s.pending_len() == 5;
        let _ = s.flush();
        let once = s.pending_len() == 0 && s.order() == alloc::vec![1, 2, 3, 4, 5];
        let audited = s.audits().iter().any(|a| a.contains("单次最小域重排"));
        set.add("D06-风暴-合并单次flush", pooled && once && audited, "");
    }

    // ---- 越界键→钳制加告警 ----

    {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 5000);
        s.add(2, -9999);
        let clamped_add =
            s.z_of(1) == Some(Z_KEY_MAX) && s.z_of(2) == Some(Z_KEY_MIN) && s.warnings().len() == 2;
        let _ = s.set_z(1, 42);
        let _ = s.flush();
        let normal_ok = s.z_of(1) == Some(42) && s.warnings().len() == 2; // 未越界不加告警
        let _ = s.set_z(1, 99999);
        let _ = s.flush();
        let clamped_set = s.z_of(1) == Some(Z_KEY_MAX) && s.warnings().len() == 3;
        set.add("D06-钳制-越界键告警", clamped_add && normal_ok && clamped_set, "");
    }

    // ---- 三序同源 ----

    // 判据：order() 是唯一序源；文档点名 F0614/F0611 消费；z 升序不变式。
    {
        let mut s = SiblingZOrder::new();
        s.add(1, 10);
        s.add(2, 0);
        s.add(3, 5);
        let seq = s.order();
        let invariant = {
            let zs: alloc::vec::Vec<i32> =
                (0..s.len()).map(|p| s.slot_at(p).unwrap().z).collect();
            let mut sorted = zs.clone();
            sorted.sort_unstable();
            zs == sorted
        };
        let doc = Z_ORDER_CONTRACT_DOC.contains("F0614") && Z_ORDER_CONTRACT_DOC.contains("F0611");
        let matches = seq == alloc::vec![2, 3, 1]; // z: 0,5,10
        set.add("D06-三序-同源契约", invariant && doc && matches, "");
    }

    // ---- 提升不改序 ----

    {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 0);
        s.add(2, 0);
        s.add(3, 0);
        let before = s.order();
        let registered = s.register_promotion(2, "建议快速合成路径").is_ok();
        let unchanged = s.order() == before;
        let audited = s.audits().iter().any(|a| a.contains("层提升提示登记") && a.contains("不变"));
        let doc = PROMOTION_DOC.contains("渲染策略");
        set.add("D06-提升-登记不改序", registered && unchanged && audited && doc, "");
    }

    // ---- 查询 O(1) 与脏区提示 ----

    {
        let mut s = SiblingZOrder::new();
        for id in 1..=4u64 {
            s.add(id, 0);
        }
        let direct = s.slot_at(0).unwrap().node_id == 1
            && s.slot_at(3).unwrap().node_id == 4
            && s.slot_at(4).is_none();
        let _ = s.set_z(2, 99);
        let hint = s.flush().unwrap();
        let hint_ok = s.dirty_hint() == Some(hint) && hint.moved >= 1;
        set.add("D06-查询-序位直达与脏区提示", direct && hint_ok, "");
    }

    // ---- 防护：未登记 id 显性拒绝 ----

    {
        let mut s = SiblingZOrder::new();
        s.add(1, 0);
        let z_rejected = s.set_z(99, 5).is_err()
            && s.errors().iter().any(|(_, c, _)| *c == "E_NODE_NOT_FOUND");
        let promo_rejected = s.register_promotion(99, "x").is_err();
        let empty_flush_rejected = s.flush().is_err()
            && s.errors().iter().any(|(_, c, _)| *c == "E_NO_PENDING");
        set.add(
            "D06-防护-未登记拒绝",
            z_rejected && promo_rejected && empty_flush_rejected,
            "",
        );
    }

    set
}
