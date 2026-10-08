//! 深化层二 · F147 跨设备主题同步（2026-09-26 深化批次二）。
//!
//! 补深主册【状态与异常】多盘冲突与资产缺失重建 +【设计细节】就位
//! 动画序列与临时态审计（主册 G-D-22）：欢迎卡决策树条件引擎、临时态
//! 生命周期审计深化（登记→清理→对拍→零残留断言）、资产按需装载
//! 优先级队列、冲突版本协商深化、20 档分辨率矩阵生成、同步阶段计时账。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::syncroam::{adaptive_crop, RoamChoice, RoamSession, RESOLUTION_STEPS, SYNC_TARGET_SECONDS};

// ---------------------------------------------------------------------------
// 欢迎卡决策树：条件引擎（三思不扰常客的机器面）
// ---------------------------------------------------------------------------

pub struct WelcomeDecision {
    /// 档案在盘。
    pub profile_present: bool,
    /// 宿主机已有个性化。
    pub host_has_profile: bool,
    /// 用户上次选过「不再询问」。
    pub silenced: bool,
}

/// 决策：档案在 + 宿主无个性化 + 未静音 → 出欢迎卡；常客（宿主有档案）
/// 直接静默就位；无档案无动作。
pub fn welcome_verdict(d: WelcomeDecision) -> Result<&'static str, &'static str> {
    if !d.profile_present {
        return Err("无档案：无同步动作");
    }
    if d.silenced {
        return Ok("静默就位：尊重「不再询问」");
    }
    if d.host_has_profile {
        return Ok("常客静默就位：不扰");
    }
    Ok("出欢迎卡：新机器首次就位")
}

// ---------------------------------------------------------------------------
// 临时态生命周期审计深化
// ---------------------------------------------------------------------------

/// 临时态条目：写过的位置登记（退出逐清的账本）。
pub struct TempStateEntry {
    pub location: &'static str,
    /// 写入时的内容指纹（清理对拍用）。
    pub fp: u64,
    /// 是否已清。
    pub cleared: bool,
}

pub struct TempStateLedger {
    entries: alloc::vec::Vec<TempStateEntry>,
}

impl TempStateLedger {
    pub fn new() -> TempStateLedger {
        TempStateLedger { entries: alloc::vec::Vec::new() }
    }

    pub fn write(&mut self, location: &'static str, fp: u64) -> Result<(), &'static str> {
        if location.is_empty() || fp == 0 {
            return Err("位置与指纹必填：不可对拍的临时态不许写");
        }
        self.entries.push(TempStateEntry { location, fp, cleared: false });
        Ok(())
    }

    /// 退出清理：逐条清 + 内容指纹对拍（清到的必须是写入时的东西）。
    pub fn teardown(&mut self, seen_fps: &[u64]) -> Result<usize, &'static str> {
        let mut n = 0;
        for e in self.entries.iter_mut().filter(|e| !e.cleared) {
            if !seen_fps.contains(&e.fp) {
                return Err("清理现场与账本不符：临时态被外部改动");
            }
            e.cleared = true;
            n += 1;
        }
        Ok(n)
    }

    /// 零残留断言：全部已清（与基础层 zero_residue 同判据）。
    pub fn zero_residue(&self) -> bool {
        self.entries.iter().all(|e| e.cleared)
    }

    pub fn pending(&self) -> usize {
        self.entries.iter().filter(|e| !e.cleared).count()
    }
}

// ---------------------------------------------------------------------------
// 资产按需装载：优先级队列（壁纸→主题→图标→短语——视觉先立）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AssetKind {
    Wallpaper, // 先立底（视觉第一帧）
    Theme,     // 再换皮
    Icons,     // 图标跟上
    Phrases,   // 短语最后（不打字不感知）
}

/// 装载顺序：按 AssetKind 序（derive Ord 声明序 = 优先级）。
pub fn load_order(kinds: &[AssetKind]) -> alloc::vec::Vec<AssetKind> {
    let mut v = kinds.to_vec();
    v.sort();
    v.dedup();
    v
}

/// 10 秒线判定：各阶段耗时合计 ≤ SYNC_TARGET_SECONDS（超时归因输出）。
pub fn within_sync_budget(stage_ms: &[u32]) -> Result<u32, &'static str> {
    let total: u32 = stage_ms.iter().sum();
    if total <= SYNC_TARGET_SECONDS * 1000 {
        Ok(total)
    } else {
        Err(alloc::format!("同步超时 {}ms（预算 {}ms）", total, SYNC_TARGET_SECONDS * 1000).leak() as &'static str)
    }
}

// ---------------------------------------------------------------------------
// 冲突版本协商深化：版本号比较 → 仲裁 + 提示
// ---------------------------------------------------------------------------

/// 协商结果：最后插入优先（主册语义）+ 提示文案二态。
pub fn negotiate(older_ver: u32, newer_ver: u32) -> (&'static str, u32) {
    if newer_ver > older_ver {
        ("后插入的档案较新：已采用并提示", newer_ver)
    } else if newer_ver == older_ver {
        ("两档案同版本：保留现任，无提示", older_ver)
    } else {
        ("后插入的档案较旧：保留现任并提示", older_ver)
    }
}

// ---------------------------------------------------------------------------
// 20 档分辨率矩阵生成（验收判据的矩阵面）
// ---------------------------------------------------------------------------

/// 生成 20 档验收矩阵（宽 1280..3840 均布）并逐档跑构图保护。
pub fn resolution_matrix(src_w: u16, src_h: u16) -> alloc::vec::Vec<(u16, u16, bool)> {
    let mut out = alloc::vec::Vec::new();
    for i in 0..RESOLUTION_STEPS {
        let w: u16 = 1280 + ((3840 - 1280) * i as u16) / (RESOLUTION_STEPS as u16 - 1);
        let h: u16 = (w * 9 / 16).max(1);
        let (cw, ch, ox, oy) = adaptive_crop(src_w, src_h, w, h);
        // 构图保护：裁切窗不得大于源图、不得越出源图边界
        let ok = cw <= src_w && ch <= src_h && (ox as u32 + cw as u32) <= src_w as u32 && (oy as u32 + ch as u32) <= src_h as u32;
        out.push((w, h, ok));
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147E_TAG: &str = "stareco-F147-deep2";

pub fn run_f147_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F147E_TAG);

    // 欢迎卡决策树
    let card = WelcomeDecision { profile_present: true, host_has_profile: false, silenced: false };
    set.add("f147e card shown", welcome_verdict(card) == Ok("出欢迎卡：新机器首次就位"), "新机出卡");
    let regular = WelcomeDecision { profile_present: true, host_has_profile: true, silenced: false };
    set.add("f147e regular silent", welcome_verdict(regular) == Ok("常客静默就位：不扰"), "常客不扰");
    let silenced = WelcomeDecision { profile_present: true, host_has_profile: false, silenced: true };
    set.add("f147e silenced", welcome_verdict(silenced).is_ok(), "静音尊重");
    let none = WelcomeDecision { profile_present: false, host_has_profile: false, silenced: false };
    set.add("f147e no profile", welcome_verdict(none).is_err(), "无档案无动作");

    // 临时态审计
    let mut ledger = TempStateLedger::new();
    let _ = ledger.write("cache/wall.tmp", fnv1a64(b"wall-bytes"));
    let _ = ledger.write("cache/theme.tmp", fnv1a64(b"theme-bytes"));
    set.add("f147e pending 2", ledger.pending() == 2, "双临时态在册");
    set.add(
        "f147e teardown mismatch",
        ledger.teardown(&[fnv1a64(b"wall-bytes")]).is_err(),
        "现场与账本不符拒绝",
    );
    let _ = ledger.teardown(&[fnv1a64(b"wall-bytes"), fnv1a64(b"theme-bytes")]);
    set.add("f147e teardown ok", ledger.zero_residue() && ledger.pending() == 0, "退出零残留");
    set.add("f147e write invalid", ledger.write("", 1).is_err(), "空位置拒绝");

    // 装载顺序
    let order = load_order(&[AssetKind::Phrases, AssetKind::Wallpaper, AssetKind::Icons, AssetKind::Theme]);
    set.add(
        "f147e load order",
        order == alloc::vec![AssetKind::Wallpaper, AssetKind::Theme, AssetKind::Icons, AssetKind::Phrases],
        "视觉先立序",
    );

    // 同步预算
    set.add("f147e budget ok", within_sync_budget(&[3000, 3000, 2000]) == Ok(8000), "8s 在 10s 线内");
    set.add("f147e budget over", within_sync_budget(&[6000, 6000]).is_err(), "12s 超时归因");

    // 冲突协商
    let (msg1, v1) = negotiate(3, 4);
    set.add("f147e newer wins", v1 == 4 && msg1.contains("较新"), "后插较新采用");
    let (msg2, v2) = negotiate(4, 4);
    set.add("f147e same ver", v2 == 4 && msg2.contains("同版本"), "同版本保留现任");
    let (msg3, v3) = negotiate(5, 3);
    set.add("f147e older kept", v3 == 5 && msg3.contains("较旧"), "后插较旧保留现任");

    // 20 档矩阵
    let matrix = resolution_matrix(3840, 2160);
    set.add("f147e matrix size", matrix.len() == RESOLUTION_STEPS, "20 档齐");
    set.add("f147e matrix all ok", matrix.iter().all(|(_, _, ok)| *ok), "逐档构图保护全绿");
    set.add("f147e matrix spread", matrix[0].0 == 1280 && matrix[19].0 == 3840, "1280 到 4K 全跨度");

    // 与基础层联动：会话机三态
    let mut session = RoamSession::detect(fnv1a64(b"profile-v3"), false);
    set.add("f147e session card", session.welcome_card(), "基础层出卡判定（对账）");
    let _ = session.decide(RoamChoice::Adopt);
    set.add("f147e session adopt", session.zero_residue(), "Adopt 无临时态");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn temp_ledger_double_teardown() {
        let mut l = TempStateLedger::new();
        let _ = l.write("x", 7);
        assert_eq!(l.teardown(&[7]).unwrap(), 1);
        assert_eq!(l.teardown(&[7]).unwrap(), 0); // 二次清理无事可做
        assert!(l.zero_residue());
    }

    #[test]
    fn load_order_dedup() {
        let o = load_order(&[AssetKind::Theme, AssetKind::Theme, AssetKind::Wallpaper]);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0], AssetKind::Wallpaper);
    }

    #[test]
    fn matrix_small_source() {
        // 小源图到 4K：裁切窗仍不越界（保护逻辑与分辨率无关）
        let m = resolution_matrix(1920, 1080);
        assert!(m.iter().all(|(_, _, ok)| *ok));
    }
}
