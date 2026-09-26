//! 深化层 · F147 跨设备主题同步（2026-09-26 回炉补深化）。
//!
//! 补深：档案差异预览（同步前先看要动什么）、同步进度模型（<10s
//! 分步呈现）、清理审计日志（临时态写点逐条对账）、多盘冲突提示
//! 文案、热插拔检测节拍。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::syncroam::{adaptive_crop, RoamChoice, RoamSession, SYNC_TARGET_SECONDS};

// ---------------------------------------------------------------------------
// 档案差异预览（同步前先看要动什么——导入前预览纪律）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileItem {
    pub kind: &'static str, // wallpaper / theme / iconpack / phrases
    pub fp: u64,
}

/// 差异预览：(将覆盖的项, 将新增的项)——预览不改任何状态。
pub fn diff_preview(host: &[ProfileItem], incoming: &[ProfileItem]) -> (alloc::vec::Vec<&'static str>, alloc::vec::Vec<&'static str>) {
    let mut overwritten = alloc::vec::Vec::new();
    let mut added = alloc::vec::Vec::new();
    for i in incoming {
        match host.iter().find(|h| h.kind == i.kind) {
            Some(h) if h.fp != i.fp => overwritten.push(i.kind),
            Some(_) => {}
            None => added.push(i.kind),
        }
    }
    (overwritten, added)
}

// ---------------------------------------------------------------------------
// 同步进度模型（<10s：分步呈现，不白屏硬等）
// ---------------------------------------------------------------------------

/// 五步流：检测→预览→拷贝资产→应用→确认就位。每步权重均分总时长。
pub const SYNC_STEPS: [&str; 5] = ["detect", "preview", "copy-assets", "apply", "confirm"];

pub fn progress_percent(step_done: usize) -> u32 {
    if step_done >= SYNC_STEPS.len() {
        return 100;
    }
    (step_done as u32 * 100) / SYNC_STEPS.len() as u32
}

/// 总耗时估算：资产数 × 单资产延迟，超过目标即标「按需流式」。
pub fn estimated_seconds(assets: u32, per_asset_ms: u32) -> (u32, bool) {
    let secs = assets.saturating_mul(per_asset_ms) / 1000;
    (secs, secs > SYNC_TARGET_SECONDS)
}

// ---------------------------------------------------------------------------
// 清理审计日志（临时态写点逐条对账）
// ---------------------------------------------------------------------------

pub struct CleanupAudit {
    entries: [Option<(u64, bool)>; 8], // (写点指纹, 已清)
    count: usize,
}

impl CleanupAudit {
    pub fn new() -> CleanupAudit {
        CleanupAudit { entries: [None; 8], count: 0 }
    }

    pub fn record_write(&mut self, site_fp: u64) -> Result<(), &'static str> {
        if self.count >= 8 {
            return Err("审计账满");
        }
        self.entries[self.count] = Some((site_fp, false));
        self.count += 1;
        Ok(())
    }

    /// 逐条清理：只有「仅本次」会话允许清理（Adopt 的写点归宿主配置层）。
    pub fn wipe(&mut self, choice: RoamChoice, site_fp: u64) -> Result<usize, &'static str> {
        if choice != RoamChoice::SessionOnly {
            return Err("Adopt 写点归宿主：不走临时态清理");
        }
        let mut wiped = 0;
        for e in self.entries[..self.count].iter_mut().flatten() {
            if e.0 == site_fp && !e.1 {
                e.1 = true;
                wiped += 1;
            }
        }
        if wiped == 0 {
            return Err("无此写点或已清");
        }
        Ok(wiped)
    }

    /// 零残留验证：全部已清。
    pub fn zero_residue(&self) -> bool {
        self.count > 0 && self.entries[..self.count].iter().flatten().all(|e| e.1)
    }
}

// ---------------------------------------------------------------------------
// 热插拔检测节拍
// ---------------------------------------------------------------------------

/// 插拔轮询：2s 节拍（过密费电，过疏迟钝）。
pub const HOTPLUG_POLL_MS: u64 = 2000;

pub fn hotplug_due(last_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(last_ms) >= HOTPLUG_POLL_MS
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147D_TAG: &str = "stareco-F147-deep";

pub fn run_f147_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F147D_TAG);

    // 差异预览
    let host = [
        ProfileItem { kind: "wallpaper", fp: 1 },
        ProfileItem { kind: "theme", fp: 2 },
    ];
    let incoming = [
        ProfileItem { kind: "wallpaper", fp: 9 },
        ProfileItem { kind: "theme", fp: 2 },
        ProfileItem { kind: "phrases", fp: 3 },
    ];
    let (over, add) = diff_preview(&host, &incoming);
    set.add(
        "f147d preview overwrite+add",
        over == alloc::vec!["wallpaper"] && add == alloc::vec!["phrases"],
        "同值不动——最小惊扰",
    );

    // 进度模型
    set.add(
        "f147d progress ladder",
        progress_percent(0) == 0 && progress_percent(2) == 40 && progress_percent(5) == 100,
        "五步均分",
    );
    let (slow, streamed) = estimated_seconds(50, 500);
    set.add("f147d stream when over target", slow == 25 && streamed, "超 10s 标流式");
    let (fast, _) = estimated_seconds(10, 500);
    set.add("f147d within target", fast == 5 && !estimated_seconds(10, 500).1, "5s 不标");

    // 清理审计
    let mut audit = CleanupAudit::new();
    let w1 = fnv1a64(b"roam/cache");
    audit.record_write(w1).ok();
    audit.record_write(fnv1a64(b"roam/thumb")).ok();
    set.add(
        "f147d adopt cannot wipe",
        audit.wipe(RoamChoice::Adopt, w1).is_err(),
        "Adopt 写点归宿主",
    );
    assert_eq!(audit.wipe(RoamChoice::SessionOnly, w1), Ok(1));
    set.add("f147d double wipe refused", audit.wipe(RoamChoice::SessionOnly, w1).is_err(), "幂等");
    audit.wipe(RoamChoice::SessionOnly, fnv1a64(b"roam/thumb")).ok();
    set.add("f147d zero residue verified", audit.zero_residue(), "逐条对账全清");

    // 热插拔节拍
    set.add(
        "f147d hotplug poll 2s",
        !hotplug_due(0, 1999) && hotplug_due(0, 2000),
        "2s 节拍",
    );

    // 裁切联动（深化层复诵：竖屏保上）
    let (_, y, _, _) = adaptive_crop(3840, 2160, 1080, 1920);
    set.add("f147d top protected again", y == 0, "构图保护不裁头");

    // 会话联动：预览-决策-清理整链
    let mut s = RoamSession::detect(42, true);
    s.decide(RoamChoice::SessionOnly).ok();
    s.teardown().ok();
    set.add("f147d session chain clean", s.zero_residue(), "全链闭环");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn preview_no_mutation() {
        let host = [ProfileItem { kind: "theme", fp: 1 }];
        let inc = [ProfileItem { kind: "theme", fp: 1 }];
        let (o, a) = diff_preview(&host, &inc);
        assert!(o.is_empty() && a.is_empty());
    }
}
