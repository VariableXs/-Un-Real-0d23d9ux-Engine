//! UNX-F56171 syscall 重放复测执行件（深化版）（0x61560 段）
//!
//! 锚点：UNX-F56171-J1 重放执行账（syscall 对照行重放复测执行——行
//! 键 × 原延迟 × 重放延迟 × 万分偏差 × 判定五元组逐行落账）；J2 容差
//! 带判定（偏差 ≤ 800 万分比 PASS / 800~2000 WARN / > 2000 DRIFT 三带
//! 语义）；J3 执行印（全部复测行判定齐 + 无 DRIFT 可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·重放执行面）。
//!
//! 结构：三带判定核（PASS/WARN/DRIFT 容差带）+ 执行账（行键唯一）+
//! 偏差核（万分比整数域）+ 执行印（DRIFT 拒印）+ 注入核（键缺失/带缘
//！ 穿透/判定篡改/印后执行/DRIFT 强印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61560 独占 0x20）。
pub const SEG_BASE: u32 = 0x61560;
pub const RX_KEY_MISS: u32 = SEG_BASE + 0x01; // 键缺失
pub const RX_BAND_DRIFT: u32 = SEG_BASE + 0x02; // 带外 DRIFT
pub const RX_VERDICT_FLIP: u32 = SEG_BASE + 0x03; // 判定篡改（同键二次）
pub const RX_SEALED_EXEC: u32 = SEG_BASE + 0x04; // 印后执行
pub const RX_DRIFT_SEAL: u32 = SEG_BASE + 0x05; // DRIFT 强印

/// 三带容差（万分比）。
pub const BAND_PASS_MAX: u32 = 800;
pub const BAND_WARN_MAX: u32 = 2_000;

/// 万分偏差核。
pub fn deviation(orig: u64, replay: u64) -> u32 {
    if orig == 0 {
        return if replay == 0 { 0 } else { u32::MAX };
    }
    let d = if replay > orig { replay - orig } else { orig - replay };
    ((d as u128 * 10_000) / orig as u128) as u32
}

/// 三带判定。
pub fn verdict_of(orig: u64, replay: u64) -> &'static str {
    let dev = deviation(orig, replay);
    if dev <= BAND_PASS_MAX {
        "PASS"
    } else if dev <= BAND_WARN_MAX {
        "WARN"
    } else {
        "DRIFT"
    }
}

/// 单条执行账。
#[derive(Clone)]
pub struct ExecEntry {
    pub key: String,
    pub orig: u64,
    pub replay: u64,
    pub dev: u32,
    pub verdict: &'static str,
}

impl ExecEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|o{}|r{}|dev{}|{}",
            self.key, self.orig, self.replay, self.dev, self.verdict
        )
    }
}

/// 重放执行器。
pub struct ReplayExecutor {
    entries: Vec<ExecEntry>,
    sealed: bool,
}

impl ReplayExecutor {
    pub fn new() -> Self {
        ReplayExecutor {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 执行一行复测：键在账 + 键唯一 + 三带判定。
    pub fn exec(&mut self, ledger_keys: &[&str], key: &str, orig: u64, replay: u64) -> Result<&'static str, u32> {
        if self.sealed {
            return Err(RX_SEALED_EXEC);
        }
        if !ledger_keys.contains(&key) {
            return Err(RX_KEY_MISS);
        }
        if self.entries.iter().any(|e| e.key == key) {
            return Err(RX_VERDICT_FLIP);
        }
        let dev = deviation(orig, replay);
        let v = verdict_of(orig, replay);
        self.entries.push(ExecEntry {
            key: key.to_string(),
            orig,
            replay,
            dev,
            verdict: v,
        });
        Ok(v)
    }

    pub fn entry(&self, key: &str) -> Option<&ExecEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 带分布统计。
    pub fn band_dist(&self) -> (usize, usize, usize) {
        (
            self.entries.iter().filter(|e| e.verdict == "PASS").count(),
            self.entries.iter().filter(|e| e.verdict == "WARN").count(),
            self.entries.iter().filter(|e| e.verdict == "DRIFT").count(),
        )
    }

    pub fn has_drift(&self) -> bool {
        self.entries.iter().any(|e| e.verdict == "DRIFT")
    }

    /// 执行印：全判定齐 + 无 DRIFT 才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() || self.has_drift() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_exec(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("REPLAYX-SEAL\n");
        }
        out
    }
}

impl Default for ReplayExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四行执行（2 PASS / 1 WARN / 1 DRIFT）。
fn standard_exec() -> ReplayExecutor {
    let keys = ["r1", "r2", "r3", "r4"];
    let mut e = ReplayExecutor::new();
    let _ = e.exec(&keys, "r1", 530, 532); // dev ≈ 37 → PASS
    let _ = e.exec(&keys, "r2", 450, 470); // dev = 2000/450 ≈ 444 → PASS
    let _ = e.exec(&keys, "r3", 360, 380); // dev = 2000/360 ≈ 555 → PASS
    // 改造：制造 WARN（dev 1500）与 DRIFT（dev 3000）
    let _ = e.exec(&keys, "r4", 100, 115); // dev = 1500 → WARN
    // 追加一行 DRIFT
    let keys2 = ["r1", "r2", "r3", "r4", "r5"];
    let _ = e.exec(&keys2, "r5", 100, 400); // dev = 3000 → DRIFT
    e
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：键缺失
    let mut e = ReplayExecutor::new();
    if e.exec(&["a"], "ghost", 100, 100) == Err(RX_KEY_MISS) {
        caught += 1;
    }
    // 注 2：同键二次
    let mut e = standard_exec();
    if e.exec(&["r1", "r2"], "r1", 530, 531) == Err(RX_VERDICT_FLIP) {
        caught += 1;
    }
    // 注 3：带缘判定（dev 恰 800 = PASS；801 = WARN）
    let ok = deviation(1000, 1080) == 800 && verdict_of(1000, 1080) == "PASS"
        && verdict_of(1000, 1081) == "WARN";
    if ok {
        caught += 1;
    }
    // 注 4：DRIFT 强印拒
    let mut e = standard_exec();
    if !e.seal() && !e.sealed() {
        caught += 1;
    }
    // 注 5：印后执行拒
    let mut e2 = ReplayExecutor::new();
    let _ = e2.exec(&["a"], "a", 100, 101);
    let _ = e2.seal();
    if e2.exec(&["a"], "a", 100, 101) == Err(RX_SEALED_EXEC) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 执行账；J2 三带判定；J3 执行印。
pub fn run_unx56171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56171");
    // J1-1 五行全执行
    let e = standard_exec();
    let ok = e.len() == 5;
    s.add("56171-J1-五行全执行", ok, if ok { "" } else { "执行账不全" });
    // J1-2 渲染导出齐
    let text = standard_exec().render_exec();
    let ok = text.contains("r1|o530|r532|dev37|PASS") && text.contains("r5|o100|r400|dev3000|DRIFT");
    s.add("56171-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 带分布（3 PASS / 1 WARN / 1 DRIFT）
    let ok = standard_exec().band_dist() == (3, 1, 1);
    s.add("56171-J2-带分布", ok, if ok { "" } else { "带分布漂移" });
    // J2-2 PASS 带缘（dev 800 PASS）
    let ok = verdict_of(1000, 1080) == "PASS";
    s.add("56171-J2-PASS带缘", ok, if ok { "" } else { "带缘漂移" });
    // J2-3 WARN 带（801~2000）
    let ok = verdict_of(1000, 1081) == "WARN" && verdict_of(1000, 1200) == "WARN"
        && verdict_of(1000, 2000) == "WARN";
    s.add("56171-J2-WARN带", ok, if ok { "" } else { "WARN 漂移" });
    // J2-4 DRIFT 带（>2000）
    let ok = verdict_of(1000, 2001) == "DRIFT" && verdict_of(1000, 4000) == "DRIFT";
    s.add("56171-J2-DRIFT带", ok, if ok { "" } else { "DRIFT 漂移" });
    // J2-5 零原值守卫
    let ok = deviation(0, 0) == 0 && deviation(0, 1) == u32::MAX;
    s.add("56171-J2-零原守卫", ok, if ok { "" } else { "除零穿透" });
    // J2-6 负向偏差对称
    let ok = deviation(1000, 920) == 800;
    s.add("56171-J2-负向对称", ok, if ok { "" } else { "对称漂移" });
    // J3-1 DRIFT 拒印
    let mut e = standard_exec();
    let ok = !e.seal() && e.has_drift();
    s.add("56171-J3-DRIFT拒印", ok, if ok { "" } else { "印闸失效" });
    // J3-2 无 DRIFT 印（三 PASS 批）
    let mut e2 = ReplayExecutor::new();
    let keys = ["a", "b"];
    let _ = e2.exec(&keys, "a", 100, 102);
    let _ = e2.exec(&keys, "b", 200, 198);
    let ok = e2.seal() && e2.sealed();
    s.add("56171-J3-无DRIFT印", ok, if ok { "" } else { "印失败" });
    // J3-3 印文渲染
    let ok = e2.render_exec().contains("REPLAYX-SEAL");
    s.add("56171-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 印后执行拒
    let ok = e2.exec(&keys, "a", 100, 101) == Err(RX_SEALED_EXEC);
    s.add("56171-J3-印后执行拒", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56171-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56171_all_checks_green() {
        let set = run_unx56171_checks();
        assert!(set.all_passed(), "UNX-F56171 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56171_band_stable() {
        assert_eq!(verdict_of(1000, 1080), "PASS");
        assert_eq!(verdict_of(1000, 1081), "WARN");
        assert_eq!(verdict_of(1000, 2001), "DRIFT");
    }
}
