//! UNX-F56036 verdict 阈值判定件（0x60460 段）
//!
//! 锚点：UNX-F56036-J1 verdict 按 P1 阈值判（对照 ≤ 倍率，P95 口径，
//! 样本 ≥30）；J2 阈值收紧/放宽走 ADR 的钩子在位（先例集 #6 默认收紧
//! ——放宽必须显式 ADR 行且留理由，收紧可默认执行留痕）；J3 判定日志
//! 可复查（逐次判定落账：ratio × 阈值 × verdict 三元可回放）三判据全
//! 绿。判据锚定 §AI-71.2 专题三+专题五。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·verdict 阈值面）。
//!
//! 结构：阈值账（当前倍率 × P95 口径 × 样本下限）+ ADR 钩子账（收紧
//! 默认留痕 / 放宽须理由行）+ 判定日志（逐次三元落账只追加）+ 回放核
//! （同输入同 verdict）+ 注入核（样本不足判/零阈值/放宽无理由/日志回
//! 退/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60460 独占 0x20）。
pub const SEG_BASE: u32 = 0x60460;
pub const TH_SAMPLE_SHORT: u32 = SEG_BASE + 0x01; // 样本不足判
pub const TH_ZERO: u32 = SEG_BASE + 0x02; // 零阈值
pub const TH_LOOSE_NO_NOTE: u32 = SEG_BASE + 0x03; // 放宽无理由
pub const TH_LOG_BACK: u32 = SEG_BASE + 0x04; // 判定日志回退
pub const TH_SEALED: u32 = SEG_BASE + 0x05; // 阈值账已封账拒改
pub const TH_DUP_ADR: u32 = SEG_BASE + 0x06; // 同 ADR 号重复
pub const TH_RATIO_ZERO: u32 = SEG_BASE + 0x07; // 零 ratio 判定

/// 样本下限（P1 口径 ≥30）。
pub const SAMPLE_MIN: u32 = 30;

/// verdict 闭集。
pub const V_PASS: usize = 0;
pub const V_FAIL: usize = 1;
pub const V_NAMES: [&str; 2] = ["pass", "fail"];

/// 判定日志行：ratio × 阈值 × verdict。
#[derive(Clone)]
pub struct VerdictLogRow {
    pub seq: u32,
    pub ratio_pbp: u64,
    pub threshold_pbp: u64,
    pub verdict: usize,
}

/// ADR 钩子行：号 × 方向 × 理由（放宽必附）。
#[derive(Clone)]
pub struct AdrHookRow {
    pub seq: u32,
    pub tighten: bool,
    pub new_threshold_pbp: u64,
    pub note: String,
}

/// 阈值判定器。
pub struct ThresholdBook {
    pub threshold_pbp: u64,
    pub adr: Vec<AdrHookRow>,
    pub log: Vec<VerdictLogRow>,
    pub sealed: bool,
}

impl ThresholdBook {
    pub fn new(threshold_pbp: u64) -> Result<Self, u32> {
        if threshold_pbp == 0 {
            return Err(TH_ZERO);
        }
        Ok(ThresholdBook { threshold_pbp, adr: Vec::new(), log: Vec::new(), sealed: false })
    }

    /// J1：判定（样本 ≥30 才判；ratio > 0）。
    pub fn judge(&mut self, seq: u32, ratio_pbp: u64, sample_n: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(TH_SEALED);
        }
        if sample_n < SAMPLE_MIN {
            return Err(TH_SAMPLE_SHORT);
        }
        if ratio_pbp == 0 {
            return Err(TH_RATIO_ZERO);
        }
        if let Some(prev) = self.log.last() {
            if seq <= prev.seq {
                return Err(TH_LOG_BACK);
            }
        }
        let v = if ratio_pbp <= self.threshold_pbp { V_PASS } else { V_FAIL };
        self.log.push(VerdictLogRow { seq, ratio_pbp, threshold_pbp: self.threshold_pbp, verdict: v });
        Ok(v)
    }

    /// J2：收紧（默认可执行，留痕；阈值必须真降）。
    pub fn tighten(&mut self, seq: u32, new_pbp: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_SEALED);
        }
        if new_pbp == 0 {
            return Err(TH_ZERO);
        }
        if new_pbp >= self.threshold_pbp {
            return Err(TH_LOOSE_NO_NOTE);
        }
        self.push_adr(AdrHookRow { seq, tighten: true, new_threshold_pbp: new_pbp, note: note.to_string() })?;
        self.threshold_pbp = new_pbp;
        Ok(())
    }

    /// J2：放宽（必须显式理由，先例集 #6 默认拒无理由放宽）。
    pub fn loosen(&mut self, seq: u32, new_pbp: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_SEALED);
        }
        if new_pbp <= self.threshold_pbp {
            return Err(TH_LOOSE_NO_NOTE);
        }
        if note.is_empty() {
            return Err(TH_LOOSE_NO_NOTE);
        }
        self.push_adr(AdrHookRow { seq, tighten: false, new_threshold_pbp: new_pbp, note: note.to_string() })?;
        self.threshold_pbp = new_pbp;
        Ok(())
    }

    /// ADR 行追加（同号重复拒）。
    fn push_adr(&mut self, row: AdrHookRow) -> Result<(), u32> {
        for a in self.adr.iter() {
            if a.seq == row.seq {
                return Err(TH_DUP_ADR);
            }
        }
        self.adr.push(row);
        Ok(())
    }

    /// J3：判定日志回放核（同 ratio × 同阈值 → 同 verdict）。
    pub fn replay_consistent(&self) -> bool {
        for r in self.log.iter() {
            let expect = if r.ratio_pbp <= r.threshold_pbp { V_PASS } else { V_FAIL };
            if r.verdict != expect {
                return false;
            }
        }
        !self.log.is_empty()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：阈值 11000，三次判定（10000 pass / 11910 fail / 11000 pass）。
pub fn standard_threshold() -> ThresholdBook {
    let mut t = ThresholdBook::new(11_000).expect("t");
    t.judge(1, 10_000, 100).expect("j1");
    t.judge(2, 11_910, 100).expect("j2");
    t.judge(3, 11_000, 100).expect("j3");
    t
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 样本不足判（29）
    let mut t = ThresholdBook::new(11_000).expect("t");
    if t.judge(1, 10_000, 29) == Err(TH_SAMPLE_SHORT) {
        caught += 1;
    }
    // 2) 零阈值构造
    if ThresholdBook::new(0).err() == Some(TH_ZERO) {
        caught += 1;
    }
    // 3) 放宽无理由
    let mut t2 = standard_threshold();
    if t2.loosen(9, 12_000, "") == Err(TH_LOOSE_NO_NOTE) {
        caught += 1;
    }
    // 4) 判定日志回退（seq 重复）
    let mut t3 = standard_threshold();
    if t3.judge(3, 10_000, 100) == Err(TH_LOG_BACK) {
        caught += 1;
    }
    // 5) 封账改写
    let mut t4 = standard_threshold();
    t4.seal();
    if t4.judge(9, 10_000, 100) == Err(TH_SEALED)
        && t4.tighten(9, 10_000, "x") == Err(TH_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判定日志渲染（回放面）。
pub fn render_log(t: &ThresholdBook) -> String {
    let mut out = String::new();
    for r in t.log.iter() {
        out.push_str(&format!(
            "JUDGE|{}|ratio={}|th={}|{}\n",
            r.seq, r.ratio_pbp, r.threshold_pbp, V_NAMES[r.verdict]
        ));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56036");
    // J1-1 P1 判定语义（10000 pass / 11910 fail / 11000 恰阈 pass）
    let t = standard_threshold();
    let ok = t.log[0].verdict == V_PASS
        && t.log[1].verdict == V_FAIL
        && t.log[2].verdict == V_PASS
        && t.log[2].ratio_pbp == 11_000;
    s.add("56036-J1-P1判定语义", ok, if ok { "" } else { "判定语义漂移" });
    // J1-2 P95 口径与样本下限（30 过 / 29 拒）
    let mut t2 = ThresholdBook::new(11_000).expect("t2");
    let ok = t2.judge(1, 10_000, SAMPLE_MIN).is_ok()
        && t2.judge(2, 10_000, SAMPLE_MIN - 1) == Err(TH_SAMPLE_SHORT);
    s.add("56036-J1-样本下限", ok, if ok { "" } else { "下限漂移" });
    // J2-1 收紧默认执行留痕（11000 → 10500，ADR 行 tighten=true）
    let mut t3 = standard_threshold();
    t3.tighten(1, 10_500, "先例集 #6 默认收紧").expect("tighten");
    let ok = t3.threshold_pbp == 10_500
        && t3.adr.len() == 1
        && t3.adr[0].tighten
        && t3.adr[0].note.contains("收紧");
    s.add("56036-J2-收紧默认留痕", ok, if ok { "" } else { "收紧钩子缺" });
    // J2-2 放宽须显式理由（无理由拒 / 有理由过）
    let mut t4 = standard_threshold();
    let no_note = t4.loosen(1, 12_000, "");
    let with_note = t4.loosen(2, 12_000, "P1 阈值口径 ADR-7 放宽至 12000");
    let ok = no_note == Err(TH_LOOSE_NO_NOTE)
        && with_note.is_ok()
        && t4.threshold_pbp == 12_000
        && !t4.adr[0].tighten;
    s.add("56036-J2-放宽须理由", ok, if ok { "" } else { "放宽口径松" });
    // J2-3 伪收紧拒（新阈 ≥ 旧阈走 tighten 拒）
    let mut t5 = standard_threshold();
    let ok = t5.tighten(1, 11_000, "x") == Err(TH_LOOSE_NO_NOTE);
    s.add("56036-J2-伪收紧拒", ok, if ok { "" } else { "伪收紧穿透" });
    // J2-4 同 ADR 号重复拒
    let mut t6 = standard_threshold();
    t6.tighten(1, 10_500, "a").expect("t1");
    let ok = t6.tighten(1, 10_000, "b") == Err(TH_DUP_ADR);
    s.add("56036-J2-ADR号唯一", ok, if ok { "" } else { "ADR 撞号" });
    // J3-1 判定日志可复查（回放一致 + 三行）
    let ok = standard_threshold().replay_consistent();
    s.add("56036-J3-日志回放一致", ok, if ok { "" } else { "回放漂移" });
    // J3-2 日志渲染机读（JUDGE 行三元齐）
    let text = render_log(&standard_threshold());
    let ok = text.lines().count() == 3
        && text.contains("JUDGE|2|ratio=11910|th=11000|fail");
    s.add("56036-J3-日志机读", ok, if ok { "" } else { "渲染面漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56036-J3-注入核必抓", ok, &detail);
    // J1-3 零 ratio 判定拒
    let ok = ThresholdBook::new(11_000).expect("z").judge(1, 0, 100) == Err(TH_RATIO_ZERO);
    s.add("56036-J1-零ratio拒", ok, if ok { "" } else { "零 ratio 穿透" });
    // J2-5 判后阈值即时生效（收紧后旧 pass 变 fail）
    let mut t7 = standard_threshold();
    t7.tighten(1, 10_500, "收紧复验").expect("tc");
    let v = t7.judge(9, 10_800, 100).expect("v");
    let ok = v == V_FAIL;
    s.add("56036-J2-阈值即时生效", ok, if ok { "" } else { "生效时序漂移" });
    // J2-6 封账冻结
    let mut t8 = standard_threshold();
    t8.seal();
    let ok = t8.loosen(9, 12_000, "x") == Err(TH_SEALED);
    s.add("56036-J2-封账冻结", ok, if ok { "" } else { "封账后可改" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56036_all_checks_green() {
        let set = run_unx56036_checks();
        assert!(set.all_passed(), "UNX-F56036 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56036_boundary() {
        let mut t = ThresholdBook::new(11_000).expect("t");
        assert_eq!(t.judge(1, 11_000, 30).expect("v"), V_PASS);
        assert_eq!(t.judge(2, 11_001, 30).expect("v2"), V_FAIL);
    }
}
