//! UNX-F56154 对照行重放复测器（抽 3 行重采框架）（0x61340 段）
//!
//! 锚点：UNX-F56154-J1 重放框架账（抽 3 行重采——行选择器（等距抽
//! 样）× 复测执行位 × 偏差核 × 复测账四元组落账）；J2 复测偏差语义
//! （重放值 vs 原值万分偏差 ≤ 容差带判 PASS，超带判 DRIFT 并留痕）；
//! J3 抽样确定性（同账本同行集同抽样——选择器纯函数）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·重放复测面）。
//!
//! 结构：等距抽样器（n 行抽 3：首/中/尾等距槽位）+ 复测执行核（重放
//! 值合成器：同键同参确定性重算）+ 偏差核（万分偏差 + 容差带判定）+
//! 复测账（行键 × 原值 × 重放值 × 判定四元组）+ 注入核（重放值篡改/
//! 容差带穿透/抽样漂移/复测账后改判/键缺失必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61340 独占 0x20）。
pub const SEG_BASE: u32 = 0x61340;
pub const RP_TAMPER: u32 = SEG_BASE + 0x01; // 重放值篡改（超带）
pub const RP_KEY_MISS: u32 = SEG_BASE + 0x02; // 复测键缺失
pub const RP_SAMPLE_DRIFT: u32 = SEG_BASE + 0x03; // 抽样漂移
pub const RP_VERDICT_FLIP: u32 = SEG_BASE + 0x04; // 复测账后改判
pub const RP_EMPTY_LEDGER: u32 = SEG_BASE + 0x05; // 空账复测

/// 复测容差带（万分比）。
pub const TOLERANCE: u32 = 500;

/// 等距抽样器：n 行抽 3（首/中/尾），n < 3 时全抽。
pub fn pick3(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    if n < 3 {
        return (0..n).collect();
    }
    let last = n - 1;
    let mid = n / 2;
    alloc::vec![0, mid, last]
}

/// 万分偏差核：|重放 - 原| × 10000 / 原（原非零），零原值特殊口径。
pub fn deviation(orig: u32, replay: u32) -> u32 {
    if orig == 0 {
        return if replay == 0 { 0 } else { u32::MAX };
    }
    let d = if replay > orig { replay - orig } else { orig - replay };
    ((d as u64 * 10_000) / orig as u64) as u32
}

/// 偏差判定：≤ 容差 PASS，否则 DRIFT。
pub fn verdict_of(orig: u32, replay: u32) -> &'static str {
    if deviation(orig, replay) <= TOLERANCE {
        "PASS"
    } else {
        "DRIFT"
    }
}

/// 单条复测账。
#[derive(Clone)]
pub struct ReplayEntry {
    pub row_key: String,
    pub orig: u32,
    pub replay: u32,
    pub verdict: &'static str,
}

impl ReplayEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|o{}|r{}|{}|dev{}",
            self.row_key, self.orig, self.replay, self.verdict, deviation(self.orig, self.replay)
        )
    }
}

/// 重放复测器。
pub struct ReplayTester {
    entries: Vec<ReplayEntry>,
    sealed: bool,
}

impl ReplayTester {
    pub fn new() -> Self {
        ReplayTester {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 复测一行：键必须在账本行集内，判定入账（账后改判拒）。
    pub fn test_row(&mut self, ledger_keys: &[&str], row_key: &str, orig: u32, replay: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_VERDICT_FLIP);
        }
        if !ledger_keys.contains(&row_key) {
            return Err(RP_KEY_MISS);
        }
        if self.entries.iter().any(|e| e.row_key == row_key) {
            return Err(RP_VERDICT_FLIP);
        }
        self.entries.push(ReplayEntry {
            row_key: row_key.to_string(),
            orig,
            replay,
            verdict: verdict_of(orig, replay),
        });
        Ok(())
    }

    /// 按抽样执行三行复测：重放值由确定性合成器生成（演示口径 = 原值 ± 扰动）。
    pub fn run_pick3(&mut self, ledger_keys: &[&str], origs: &[u32], replay_deltas: &[i64]) -> Result<usize, u32> {
        if ledger_keys.is_empty() || origs.is_empty() {
            return Err(RP_EMPTY_LEDGER);
        }
        let idxs = pick3(ledger_keys.len());
        for i in idxs {
            if i >= origs.len() {
                return Err(RP_SAMPLE_DRIFT);
            }
            let orig = origs[i];
            let delta = replay_deltas.get(i).copied().unwrap_or(0);
            let replay = ((orig as i64 + delta).max(0)) as u32;
            self.test_row(ledger_keys, ledger_keys[i], orig, replay)?;
        }
        Ok(self.entries.len())
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// DRIFT 计数。
    pub fn drift_count(&self) -> usize {
        self.entries.iter().filter(|e| e.verdict == "DRIFT").count()
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("REPLAY-SEAL\n");
        }
        out
    }
}

impl Default for ReplayTester {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：6 行账抽 3 行复测（2 PASS + 1 DRIFT）。
fn standard_tester() -> ReplayTester {
    let keys = ["r1", "r2", "r3", "r4", "r5", "r6"];
    let origs = [9500u32, 9480, 9510, 9490, 9520, 9470];
    let mut t = ReplayTester::new();
    // 抽样：r1 / r3 / r6；重放扰动：0 / 0 / +1000（超 500 容差）
    let _ = t.run_pick3(&keys, &origs, &[0, 0, 0, 0, 0, 1000]);
    t
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：键缺失复测
    let mut t = ReplayTester::new();
    if t.test_row(&["a"], "ghost", 100, 100) == Err(RP_KEY_MISS) {
        caught += 1;
    }
    // 注 2：空账复测
    let mut t = ReplayTester::new();
    if t.run_pick3(&[], &[], &[0]) == Err(RP_EMPTY_LEDGER) {
        caught += 1;
    }
    // 注 3：同键二次复测拒
    let mut t = standard_tester();
    if t.test_row(&["r1", "r2"], "r1", 9500, 9500) == Err(RP_VERDICT_FLIP) {
        caught += 1;
    }
    // 注 4：篡改必抓（超带判 DRIFT）
    let t = standard_tester();
    if t.drift_count() == 1 {
        caught += 1;
    }
    // 注 5：零原值特殊口径（原 0 重放 5 → 偏差 MAX 判 DRIFT）
    let ok = verdict_of(0, 5) == "DRIFT" && verdict_of(0, 0) == "PASS";
    if ok {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 重放框架账；J2 偏差语义；J3 抽样确定性。
pub fn run_unx56154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56154");
    // J1-1 三行复测落账
    let t = standard_tester();
    let ok = t.len() == 3;
    s.add("56154-J1-三行复测", ok, if ok { "" } else { "复测账不全" });
    // J1-2 渲染导出齐
    let text = standard_tester().render_ledger();
    let ok = text.contains("r1|o9500|r9500|PASS") && text.contains("r6|o9470|r10470|DRIFT");
    s.add("56154-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 容差带内 PASS（偏差 0）
    let ok = verdict_of(9500, 9500) == "PASS";
    s.add("56154-J2-带内PASS", ok, if ok { "" } else { "判定漂移" });
    // J2-2 容差带缘 PASS（偏差恰 500：9500×0.05=475）
    let ok = verdict_of(9500, 9500 - 475) == "PASS";
    s.add("56154-J2-带缘PASS", ok, if ok { "" } else { "带缘漂移" });
    // J2-3 超带 DRIFT（偏差 1000/9500 ≈ 1052 > 500）
    let ok = verdict_of(9500, 10470) == "DRIFT";
    s.add("56154-J2-超带DRIFT", ok, if ok { "" } else { "容差穿透" });
    // J2-4 偏差算术锚（orig=200, replay=210 → 偏差 500）
    let ok = deviation(200, 210) == 500;
    s.add("56154-J2-偏差锚", ok, if ok { "" } else { "偏差算术漂移" });
    // J2-5 负向偏差对称（orig=200, replay=190 → 偏差 500）
    let ok = deviation(200, 190) == 500;
    s.add("56154-J2-负向对称", ok, if ok { "" } else { "对称漂移" });
    // J2-6 DRIFT 计数
    let ok = standard_tester().drift_count() == 1;
    s.add("56154-J2-DRIFT计数", ok, if ok { "" } else { "计数漂移" });
    // J3-1 抽样确定性：同 n 同抽样
    let ok = pick3(6) == pick3(6) && pick3(6) == alloc::vec![0, 3, 5];
    s.add("56154-J3-抽样确定", ok, if ok { "" } else { "抽样漂移" });
    // J3-2 抽样边界：n=1/2 全抽
    let ok = pick3(1) == alloc::vec![0] && pick3(2) == alloc::vec![0, 1];
    s.add("56154-J3-边界全抽", ok, if ok { "" } else { "边界漂移" });
    // J3-3 抽样 n=3 恰满
    let ok = pick3(3) == alloc::vec![0, 1, 2];
    s.add("56154-J3-满抽", ok, if ok { "" } else { "满抽漂移" });
    // J3-4 复测账封账只读
    let mut t = standard_tester();
    let _ = t.seal();
    let ok = t.sealed() && t.test_row(&["r1"], "r1", 1, 1) == Err(RP_VERDICT_FLIP);
    s.add("56154-J3-封账只读", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56154-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56154_all_checks_green() {
        let set = run_unx56154_checks();
        assert!(set.all_passed(), "UNX-F56154 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56154_pick_stable() {
        assert_eq!(pick3(6), alloc::vec![0, 3, 5]);
    }
}
