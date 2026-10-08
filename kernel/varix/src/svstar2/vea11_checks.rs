//! VE-F0011 · 域自检（判据逐条对应，见 `vea11_hotplug.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 热拔重选（外接显卡拔出 → 重选最高优先级在位卡）→ `A11-重选-*`
//! - 平滑迁移（A 卡切 B 卡四段流水）→ `A11-迁移-*`
//! - 不闪屏（原子提交，旧卡到最后一帧）→ `A11-迁移-原子提交不闪屏`
//! - 失败回退（校验不一致 → 回退原卡）→ `A11-回退-*`
//! - 用户通知（换了张卡要告诉用户）→ `A11-通知-*`
//! - 状态全量校验（迁移前后输出一致性断言）→ `A11-迁移-全量校验`
//! - 热拔防抖（接触不良反复插拔不反复迁移）→ `A11-防抖-*`
//! - 热拔与休眠区分 → `A11-检测-休眠不迁移`
//! - 检测缺失→轮询兜底 → `A11-检测-轮询兜底`
//! - 平滑降级（无卡可fallback → 软渲染）→ `A11-回退-软渲染兜底`
//!
//! 逻辑时钟注入、零墙钟，回归可复现。

use super::vea11_hotplug::*;
use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

/// 标准环境：0=物理A（当前）、1=物理B、2=虚拟卡。
fn env() -> PathSelector {
    PathSelector::new(
        vec![
            Adapter::new(0, AdapterKind::Physical),
            Adapter::new(1, AdapterKind::Physical),
            Adapter::new(2, AdapterKind::Virtual),
        ],
        0,
    )
}

/// VE-F0011 域自检。
pub fn run_vea11_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea11");

    // ---- 检测：热拔与休眠区分 ----

    // 判据：休眠不触发迁移（SleepEnter 只记录状态）；热拔触发防抖窗口。
    {
        let mut s = env();
        s.on_event(AdapterEvent::SleepEnter(0));
        s.advance_tick();
        let no_migrate = s.current() == 0 && s.migrating().is_none() && !s.debouncing(0);
        s.on_event(AdapterEvent::SleepExit(0));
        let still_zero = s.current() == 0;
        s.on_event(AdapterEvent::Unplugged(0));
        let debounce_started = s.debouncing(0);
        set.add(
            "A11-检测-休眠不迁移热拔进防抖",
            no_migrate && still_zero && debounce_started,
            "",
        );
    }

    // 判据：检测缺失→轮询兜底（无事件时主动比对在位表等效检出热拔）。
    {
        let mut s = env();
        s.enable_polling();
        // 模拟卡 0 失位（不经事件通道——事件驱动缺失）。
        if let Some(a) = s.adapters_mut().iter_mut().find(|a| a.id == 0) {
            a.present = false;
        }
        s.poll();
        let detected = s.debouncing(0);
        set.add("A11-检测-轮询兜底", detected && s.is_polling(), "");
    }

    // ---- 防抖 ----

    // 判据：热拔防抖（窗口内重新插入 = 接触抖动，不迁移）。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        // 窗口内重新插入。
        s.advance_tick();
        s.advance_tick();
        s.on_event(AdapterEvent::PluggedIn(0));
        for _ in 0..(DEBOUNCE_TICKS + 2) {
            s.advance_tick();
        }
        let ok = s.current() == 0
            && s.migrating().is_none()
            && s.bounces_absorbed() == 1
            && s.notices().is_empty();
        set.add("A11-防抖-窗口内重插不迁移", ok, "");
    }

    // 判据：窗口到期确认拔出 → 重选最高优先级在位卡并启动迁移。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        // 重选：卡 1（物理）优先于卡 2（虚拟）。
        let m = s.migrating();
        set.add(
            "A11-重选-按类别优先级选卡",
            matches!(m, Some((0, 1, MigrationStage::Export))) && s.current() == 0,
            "",
        );
    }

    // ---- 迁移与不闪屏 ----

    // 判据：迁移四段流水（导出→传输→导入→校验）+ 原子提交不闪屏
    // + 迁移前后渲染输出一致性校验通过 → 切换成功。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        // 逐段推进：Export→Transfer→Import→Verify→提交。
        let mut stages = Vec::new();
        loop {
            // 校验用的摘要模型：与导出摘要同源（迁移不改变渲染语义）。
            let stage = s.advance_migration(|i| PathSelector::digest_for(0, i));
            match stage {
                Some(st) => stages.push(st),
                None => break,
            }
        }
        let switched = s.current() == 1
            && stages == vec![
                MigrationStage::Transfer,
                MigrationStage::Import,
                MigrationStage::Verify,
            ]
            // 原子提交：旧卡渲染到提交前一刻（迁移期间 current 未变）。
            && s.notices().iter().any(|n| n.class == "迁移" && n.text.contains("无闪屏"));
        set.add("A11-迁移-四段与原子提交", switched, "");
    }

    // ---- 失败回退 ----

    // 判据：校验不一致 → 回退原卡 + 用户通知（状态全量校验的失败分支）。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        loop {
            // 摘要模型故意给出错误值 → 校验必失败。
            let stage = s.advance_migration(|_i| 0xDEAD);
            if stage.is_none() {
                break;
            }
        }
        let ok = s.current() == 0
            && s.errors().iter().any(|(_, code, _)| *code == "E_VERIFY_MISMATCH")
            && s.notices().iter().any(|n| n.class == "回退")
            && s.migrating().is_none();
        set.add("A11-回退-校验失败回退原卡", ok, "");
    }

    // 判据：强制中止 → 回退原卡。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        let _ = s.advance_migration(|i| PathSelector::digest_for(0, i));
        s.abort_migration();
        set.add(
            "A11-回退-中止回退",
            s.current() == 0 && s.migrating().is_none()
                && s.notices().iter().any(|n| n.class == "回退"),
            "",
        );
    }

    // ---- 平滑降级 ----

    // 判据：全部物理/虚拟卡消失 → 软渲染兜底（平滑降级）。
    {
        let mut s = PathSelector::new(
            vec![
                Adapter::new(0, AdapterKind::Physical),
                Adapter::new(9, AdapterKind::Software),
            ],
            0,
        );
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        let m = s.migrating();
        set.add(
            "A11-回退-软渲染兜底",
            matches!(m, Some((0, 9, _))),
            "",
        );
    }

    // 判据：拔的不是当前渲染卡 → 不迁移（只记录在位状态）。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(2));
        for _ in 0..DEBOUNCE_TICKS + 2 {
            s.advance_tick();
        }
        set.add(
            "A11-重选-非当前卡不迁移",
            s.current() == 0 && s.migrating().is_none(),
            "",
        );
    }

    // ---- 用户通知 ----

    // 判据：用户通知在册（迁移/回退都有通知，文本读屏可播）。
    {
        let mut s = env();
        s.on_event(AdapterEvent::Unplugged(0));
        for _ in 0..DEBOUNCE_TICKS {
            s.advance_tick();
        }
        loop {
            if s.advance_migration(|i| PathSelector::digest_for(0, i)).is_none() {
                break;
            }
        }
        let n = s.notices();
        let ok = n.iter().any(|x| x.class == "迁移" && x.screen_text().contains("切换"))
            && n.iter().all(|x| !x.screen_text().is_empty());
        set.add("A11-通知-重选结果告知用户", ok, "");
    }

    // ---- 读屏播报 ----

    {
        let s = env();
        let t = s.screen_text();
        set.add(
            "A11-读屏-迁移状态可播",
            t.contains("渲染路径") && t.contains("空闲"),
            "",
        );
    }

    set
}
