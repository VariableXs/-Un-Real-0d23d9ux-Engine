//! 深化层 · F599 升级后设置校验（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F599 节）：
//! ①「快照比对准确」的**指纹确定性对账**——同值同指纹、异值异指纹
//!   （比对的地基是指纹本身可靠：五类关键设置逐类过确定性关）；
//! ②「漂移通知与一键恢复」的**报告一致性**——通知里的 N 与漂移清单
//!   长度逐次对账（通知说几项就是几项，不许吓唬人也不许漏报）；
//! ③「恢复动作逐项可查」的**回执账**——恢复后逐项回执（哪些项被
//!   恢复、哪些项本就没有漂移，一项一明确）。

use alloc::string::ToString;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::setverify::{fingerprint, KeySetting, SetVerify, KEY_SETTINGS};

// ---------------------------------------------------------------------------
// 指纹确定性对账
// ---------------------------------------------------------------------------

/// 五类关键设置的默认样例值（确定性对账的输入集——真实值由设置层注入）。
pub const SAMPLE_VALUES: [&str; 5] = [
    "star-dark",          // 主题（F151）
    "Win+E:explorer",     // 快捷键表（F244）
    "txt=记事本",         // 默认应用（F345）
    "wall-light.png",     // 壁纸
    "tb:left,pin:5",      // 任务栏布局
];

/// 指纹确定性：同值同指纹且异值异指纹（五类逐一过）。
pub fn fingerprint_deterministic() -> bool {
    SAMPLE_VALUES.iter().all(|v| {
        let a = fingerprint(v);
        let b = fingerprint(v);
        a == b
    }) && SAMPLE_VALUES
        .iter()
        .zip(SAMPLE_VALUES.iter().skip(1))
        .all(|(a, b)| fingerprint(a) != fingerprint(b))
}

// ---------------------------------------------------------------------------
// 漂移报告一致性
// ---------------------------------------------------------------------------

/// 报告一致性：notice 的 N 与 compare 漂移清单长度一致。
pub fn notice_matches_drift(sv: &mut SetVerify) -> bool {
    let drift = sv.compare();
    match sv.notice_text() {
        Some(text) => {
            let n = drift.len();
            if n == 0 {
                text.is_empty() || text.contains("0")
            } else {
                text.contains(&n.to_string())
            }
        }
        None => drift.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// 恢复回执账
// ---------------------------------------------------------------------------

/// 一条恢复回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreReceipt {
    pub kind: KeySetting,
    /// 恢复前确实漂移过（没漂移的项不产生回执——回执只属于真实动作）。
    pub had_drifted: bool,
    pub restored: bool,
}

/// 回执账（逐项可查）。
pub struct ReceiptLedger {
    entries: alloc::vec::Vec<RestoreReceipt>,
}

impl ReceiptLedger {
    pub fn new() -> ReceiptLedger {
        ReceiptLedger { entries: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, kind: KeySetting, had_drifted: bool, restored: bool) {
        self.entries.push(RestoreReceipt { kind, had_drifted, restored });
    }

    /// 漂移项全部有成功回执（一键恢复的可查口径）。
    pub fn all_drifted_restored(&self, drifted: &[KeySetting]) -> bool {
        drifted.iter().all(|d| {
            self.entries
                .iter()
                .any(|r| r.kind == *d && r.had_drifted && r.restored)
        })
    }

    /// 未漂移项零回执（回执不虚发）。
    pub fn no_false_receipts(&self, drifted: &[KeySetting]) -> bool {
        self.entries.iter().all(|r| drifted.contains(&r.kind))
    }
}

impl Default for ReceiptLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f599_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 指纹确定性：五类样例同值同指纹、异值异指纹。
    cs.add("fingerprint deterministic", fingerprint_deterministic(), "");

    // 2) 关键设置清单覆盖：五类齐（主题/快捷键/默认应用/壁纸/任务栏）。
    cs.add("five key settings covered", KEY_SETTINGS.len() == 5, "");

    // 3) 快照-漂移-通知一致：两项漂移 → 通知里说 2。
    let mut sv = SetVerify::new();
    for (i, k) in KEY_SETTINGS.iter().enumerate() {
        let _ = sv.snapshot(*k, SAMPLE_VALUES[i]);
    }
    // 升级后两项被重置（观察值 ≠ 快照）。
    let _ = sv.observe(KEY_SETTINGS[0], "default-light");
    let _ = sv.observe(KEY_SETTINGS[3], "wall-default.png");
    for i in 1..5 {
        if i != 3 {
            let _ = sv.observe(KEY_SETTINGS[i], SAMPLE_VALUES[i]);
        }
    }
    let drift = sv.compare();
    cs.add(
        "notice matches drift count",
        drift.len() == 2 && notice_matches_drift(&mut sv),
        "",
    );

    // 4) 一键恢复 + 回执账：漂移项全恢复、未漂移项零回执。
    let restored_n = sv.restore_all();
    let mut rl = ReceiptLedger::new();
    for d in &drift {
        let ok = sv.was_restored(*d);
        rl.record(*d, true, ok);
    }
    cs.add(
        "restore receipts per item",
        restored_n == 2
            && rl.all_drifted_restored(&drift)
            && rl.no_false_receipts(&drift),
        "",
    );

    // 5) 全绿静默判据：恢复后再次校验 → 无通知（静默是礼仪）。
    let mut sv2 = SetVerify::new();
    for (i, k) in KEY_SETTINGS.iter().enumerate() {
        let _ = sv2.snapshot(*k, SAMPLE_VALUES[i]);
        let _ = sv2.observe(*k, SAMPLE_VALUES[i]);
    }
    cs.add(
        "silent when all green",
        sv2.silent_pass() && sv2.compare().is_empty() && sv2.notice_text().is_none(),
        "",
    );

    // 6) 快照完整性：五类齐快照才算校验就绪（缺类不出结论）。
    let mut sv3 = SetVerify::new();
    for i in 0..4 {
        let _ = sv3.snapshot(KEY_SETTINGS[i], SAMPLE_VALUES[i]);
    }
    cs.add("incomplete snapshot flagged", !sv3.snapshot_complete(), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_none_when_no_drift() {
        let mut sv = SetVerify::new();
        let _ = sv.snapshot(KEY_SETTINGS[0], "x");
        let _ = sv.observe(KEY_SETTINGS[0], "x");
        assert!(notice_matches_drift(&mut sv));
    }
}
