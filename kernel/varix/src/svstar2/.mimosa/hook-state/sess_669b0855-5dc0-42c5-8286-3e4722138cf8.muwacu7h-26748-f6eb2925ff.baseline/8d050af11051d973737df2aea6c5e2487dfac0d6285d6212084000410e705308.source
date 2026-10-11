//! VE-F0008 · 域自检（判据逐条对应，见 `vea08_handle.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 统一句柄表 → `A08-表-*`
//! - 悬垂检测 + 崩溃前拦截 → `A08-悬垂-*`
//! - 生成号校验（复用句柄带代数戳）→ `A08-代数-*`
//! - 复用清零 → `A08-复用-*`
//! - 泄漏责任人标注 → `A08-泄漏-*`
//! - 计数漂移审计 → `A08-漂移-*`

use super::vea08_handle::*;
use crate::checks::CheckSet;

use alloc::format;

/// VE-F0008 域自检。
pub fn run_vea08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea08");

    // ---- 判据：统一句柄表 ----

    {
        let mut t = HandleTable::new(64);
        let h1 = t.acquire(ResourceKind::Texture, "合成器").unwrap();
        let h2 = t.acquire(ResourceKind::Buffer, "解码器").unwrap();
        let kinds_ok = t.access(&h1) == Ok(ResourceKind::Texture)
            && t.access(&h2) == Ok(ResourceKind::Buffer);
        set.add("A08-表-统一入口异类同表", kinds_ok, "");
    }

    // ---- 判据：生成号校验（代数戳） ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        t.destroy(&h).unwrap();
        let h2 = t.acquire(ResourceKind::Texture, "B").unwrap();
        // 槽位复用但代数戳 +1：旧代句柄自动失效
        let same_slot = h.slot == h2.slot;
        let gen_bumped = h2.generation == h.generation + 1;
        let old_dead = matches!(t.access(&h), Err(ref e) if e.code == "E_HANDLE_DANGLING");
        let new_alive = t.access(&h2).is_ok();
        set.add(
            "A08-代数-旧代句柄自动失效",
            same_slot && gen_bumped && old_dead && new_alive,
            "",
        );
    }

    // ---- 判据：悬垂检测 + 崩溃前拦截 ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Pipeline, "渲染").unwrap();
        let _ = t.destroy(&h);
        // 悬垂使用（access/retain/release/borrow 全走校验）全部被拦
        let a = matches!(t.access(&h), Err(ref e) if e.severity == Severity::Blocked);
        let r = matches!(t.retain(&h), Err(ref e) if e.code == "E_HANDLE_DANGLING");
        // 伪造句柄（越界槽位）同样阻断
        let fake = Handle {
            slot: 9999,
            generation: 1,
        };
        let f = matches!(t.access(&fake), Err(ref e) if e.code == "E_HANDLE_RANGE" && e.severity == Severity::Blocked);
        // 全部留痕（崩溃前检出的证据链）
        let logged = t.blocked_events.len() >= 3;
        set.add("A08-悬垂-崩溃前拦截留痕", a && r && f && logged, "");
    }
    // 错误三要素完整（拒绝不给路 = 缺陷）
    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.destroy(&h);
        let complete = match t.access(&h) {
            Err(e) => e.is_complete() && e.what.contains("悬垂"),
            Ok(_) => false,
        };
        set.add("A08-悬垂-错误三要素完整", complete, "");
    }

    // ---- 判据：复用清零 ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.borrow(&h, "合成器");
        let _ = t.borrow(&h, "特效");
        let _ = t.destroy(&h); // 销毁清零（refcount=0、借用人表清空）
        let h2 = t.acquire(ResourceKind::Buffer, "B").unwrap();
        let e2 = t.slot_entry(h2.slot).unwrap();
        let clean = e2.refcount == 0 && e2.borrowers.is_empty() && e2.kind == ResourceKind::Buffer;
        set.add(
            "A08-复用-清零断言成立",
            clean && t.zeroing_violations == 0 && h2.generation == 2,
            "",
        );
    }

    // ---- 判据：泄漏责任人标注 ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "壁纸服务").unwrap();
        let _ = t.borrow(&h, "合成器");
        let _ = t.borrow(&h, "合成器");
        let _ = t.borrow(&h, "放大镜");
        let report = t.leak_report();
        let named = report.len() == 1
            && report[0].contains("壁纸服务")
            && report[0].contains("合成器×2")
            && report[0].contains("放大镜×1");
        // 归还后泄漏报告清空
        let _ = t.give_back(&h, "合成器");
        let _ = t.give_back(&h, "合成器");
        let _ = t.give_back(&h, "放大镜");
        set.add(
            "A08-泄漏-责任人逐条点名",
            named && t.leak_report().is_empty(),
            "",
        );
    }
    // 借还同主：代还显性拒绝（归因不断链）
    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.borrow(&h, "合成器");
        let stranger = matches!(
            t.give_back(&h, "别人"),
            Err(ref e) if e.code == "E_GIVEBACK_UNKNOWN"
        );
        let owner_ok = t.give_back(&h, "合成器").is_ok();
        set.add("A08-泄漏-借还同主", stranger && owner_ok, "");
    }
    // 归还超额 = 计数漂移，阻断级
    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.borrow(&h, "合成器");
        let _ = t.give_back(&h, "合成器");
        let over = matches!(
            t.give_back(&h, "合成器"),
            Err(ref e) if e.code == "E_GIVEBACK_OVER" && e.severity == Severity::Blocked
        );
        set.add("A08-漂移-归还超额阻断", over, "");
    }

    // ---- 判据：计数漂移审计 ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        // 未 retain 直接 release = 漂移
        let drift = matches!(t.release(&h), Err(ref e) if e.code == "E_REF_DRIFT");
        let audited = t.blocked_events.iter().any(|e| e.contains("release"));
        set.add("A08-漂移-未借先还审计", drift && audited, "");
    }
    // 正常 retain/release 配对不误报
    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.retain(&h);
        let _ = t.retain(&h);
        let one = t.release(&h) == Ok(1);
        let zero = t.release(&h) == Ok(0);
        set.add("A08-漂移-正常配对零误报", one && zero, "");
    }

    // ---- 边界防护 ----

    // 表容量耗尽显性拒绝
    {
        let mut t = HandleTable::new(2);
        let _ = t.acquire(ResourceKind::Texture, "A");
        let _ = t.acquire(ResourceKind::Buffer, "A");
        let full = matches!(
            t.acquire(ResourceKind::Pipeline, "A"),
            Err(ref e) if e.code == "E_TABLE_FULL"
        );
        // 释放一槽后可复用
        let _ = t.destroy(&Handle { slot: 0, generation: 1 });
        let reused = t.acquire(ResourceKind::Sampler, "B").is_ok();
        set.add("A08-边界-容量耗尽与复用", full && reused, "");
    }
    // 句柄 raw 往返（跨域传递的打包形态无损）
    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Swapchain, "显示").unwrap();
        let packed = h.raw();
        let unpacked = Handle {
            slot: (packed & 0xFFFF_FFFF) as u32,
            generation: (packed >> 32) as u32,
        };
        set.add("A08-边界-句柄raw往返无损", t.access(&unpacked).is_ok(), "");
    }

    // ---- 读屏与确定性 ----

    {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.destroy(&h);
        let s = t.a11y_summary();
        set.add(
            "A08-读屏-表状态可播",
            s.contains("句柄表") && s.contains("阻断级") && s.contains("复用清零"),
            "",
        );
    }
    {
        let run = || {
            let mut t = HandleTable::new(64);
            let h = t.acquire(ResourceKind::Texture, "A").unwrap();
            let _ = t.borrow(&h, "合成器");
            let _ = t.destroy(&h);
            let h2 = t.acquire(ResourceKind::Buffer, "B").unwrap();
            let dead = t.access(&h).is_err();
            (h2.generation, dead, t.blocked_events.len())
        };
        set.add("A08-确定-同操作同结果", run() == run(), "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn vea08_checks_all_green() {
        let set = run_vea08_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-A08 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 代数戳级联：两代句柄同槽并存，只有最新代有效。
    #[test]
    fn generations_are_linearized() {
        let mut t = HandleTable::new(64);
        let g1 = t.acquire(ResourceKind::Texture, "A").unwrap();
        let _ = t.destroy(&g1);
        let g2 = t.acquire(ResourceKind::Texture, "B").unwrap();
        let _ = t.destroy(&g2);
        let g3 = t.acquire(ResourceKind::Texture, "C").unwrap();
        assert!(t.access(&g3).is_ok());
        assert!(t.access(&g2).is_err());
        assert!(t.access(&g1).is_err());
        assert_eq!(g3.generation, 3);
    }

    /// 压测：acquire/destroy 循环一千轮，代数戳严格递增、清零违例恒零。
    #[test]
    fn churn_keeps_generation_and_zeroing_sane() {
        let mut t = HandleTable::new(8);
        let mut last_gen = 0;
        for i in 0..1000u32 {
            let h = t
                .acquire(ResourceKind::Texture, "压测")
                .unwrap_or_else(|_| Handle { slot: 0, generation: 0 });
            if i == 0 {
                last_gen = h.generation;
            } else {
                assert!(h.generation > last_gen, "代数戳必须递增");
                last_gen = h.generation;
            }
            assert!(t.access(&h).is_ok());
            assert!(t.destroy(&h).is_ok());
        }
        assert_eq!(t.zeroing_violations, 0, "清零违例必须恒零");
        assert_eq!(t.created_total, 1000);
        assert_eq!(t.destroyed_total, 1000);
        assert_eq!(t.live_count(), 0);
    }

    /// 泄漏归因：借用不还的名字必须出现在报告里。
    #[test]
    fn leak_report_names_the_culprit() {
        let mut t = HandleTable::new(64);
        let h = t.acquire(ResourceKind::Buffer, "视频服务").unwrap();
        let _ = t.borrow(&h, "播放器");
        let report = t.leak_report();
        assert_eq!(report.len(), 1);
        assert!(report[0].contains("播放器") && report[0].contains("视频服务"));
    }
}
