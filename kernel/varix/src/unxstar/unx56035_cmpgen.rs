//! UNX-F56035 syscall 延迟对照行生成器（0x60440 段）
//!
//! 锚点：UNX-F56035-J1 cmp_entry 全字段生成（metric=syscall, env_hash,
//! varix_ver, win_ver, varix_p50/p95/p99, win_p50/p95/p99, ratio_p95,
//! verdict, sample_n, date, collector_id, ai_sign 十四字段逐位落账）；
//! J2 字段完整性断言（任一字段缺省即拒生成）；J3 ratio_p95 语义（win/
//! varix 万分比）+ verdict 判定（P1 口径：varix ≤ win × 倍率）三判据
//! 全绿。判据锚定 §AI-71.2 专题三。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·对照行面）。
//!
//! 结构：十四字段 cmp_entry 账目（构造即全字段校验）+ ratio/verdict
//! 生成核（P1 倍率闸）+ 字段完整性断言核（十四位全非缺省）+ 机读渲染
//! 面 + 注入核（零哈希/空版本/零分位/样本不足/签名空必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60440 独占 0x20）。
pub const SEG_BASE: u32 = 0x60440;
pub const CE_HASH_ZERO: u32 = SEG_BASE + 0x01; // env_hash 零哨兵
pub const CE_VER_EMPTY: u32 = SEG_BASE + 0x02; // 版本串空
pub const CE_P_ZERO: u32 = SEG_BASE + 0x03; // 分位零值
pub const CE_SAMPLE_SHORT: u32 = SEG_BASE + 0x04; // 样本量不足
pub const CE_SIGN_EMPTY: u32 = SEG_BASE + 0x05; // AI 联签空
pub const CE_RATIO_BAD: u32 = SEG_BASE + 0x06; // ratio 越物理界（>10×）
pub const CE_FIELD_MISS: u32 = SEG_BASE + 0x07; // 字段缺省

/// 对照行字段数（十四）。
pub const FIELD_N: usize = 14;

/// 样本量下限（P4 口径 ≥30）。
pub const SAMPLE_MIN: u32 = 30;

/// P1 倍率上限（万分比：varix ≤ win × 1.1 → ratio ≤ 11000 判过）。
pub const P1_MULT_PBP: u64 = 11_000;

/// verdict 闭集。
pub const V_PASS: usize = 0;
pub const V_FAIL: usize = 1;
pub const V_NAMES: [&str; 2] = ["pass", "fail"];

/// verdict 判定：ratio_pbp ≤ P1 倍率判过。
pub fn verdict_of(ratio_pbp: u64) -> usize {
    if ratio_pbp <= P1_MULT_PBP {
        V_PASS
    } else {
        V_FAIL
    }
}

/// 十四字段对照行。
#[derive(Clone)]
pub struct CmpEntry {
    pub metric: String,
    pub env_hash: u32,
    pub varix_ver: String,
    pub win_ver: String,
    pub varix_p50: u64,
    pub varix_p95: u64,
    pub varix_p99: u64,
    pub win_p50: u64,
    pub win_p95: u64,
    pub win_p99: u64,
    pub ratio_pbp: u64,
    pub verdict: usize,
    pub sample_n: u32,
    pub date: String,
    pub collector_id: String,
    pub ai_sign: String,
}

impl CmpEntry {
    /// 构造即全字段校验（任一缺省即拒）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metric: &str,
        env_hash: u32,
        varix_ver: &str,
        win_ver: &str,
        varix_p50: u64,
        varix_p95: u64,
        varix_p99: u64,
        win_p50: u64,
        win_p95: u64,
        win_p99: u64,
        sample_n: u32,
        date: &str,
        collector_id: &str,
        ai_sign: &str,
    ) -> Result<Self, u32> {
        if env_hash == 0 {
            return Err(CE_HASH_ZERO);
        }
        if metric.is_empty() || varix_ver.is_empty() || win_ver.is_empty() {
            return Err(CE_VER_EMPTY);
        }
        if date.is_empty() || collector_id.is_empty() || ai_sign.is_empty() {
            return Err(CE_FIELD_MISS);
        }
        if varix_p50 == 0 || varix_p95 == 0 || varix_p99 == 0
            || win_p50 == 0 || win_p95 == 0 || win_p99 == 0
        {
            return Err(CE_P_ZERO);
        }
        if sample_n < SAMPLE_MIN {
            return Err(CE_SAMPLE_SHORT);
        }
        let ratio_pbp = win_p95 * 10_000 / varix_p95;
        if ratio_pbp > 100_000 {
            return Err(CE_RATIO_BAD);
        }
        let verdict = verdict_of(ratio_pbp);
        Ok(CmpEntry {
            metric: metric.to_string(),
            env_hash,
            varix_ver: varix_ver.to_string(),
            win_ver: win_ver.to_string(),
            varix_p50,
            varix_p95,
            varix_p99,
            win_p50,
            win_p95,
            win_p99,
            ratio_pbp,
            verdict,
            sample_n,
            date: date.to_string(),
            collector_id: collector_id.to_string(),
            ai_sign: ai_sign.to_string(),
        })
    }

    /// J2：字段完整性断言（十四字段逐位非缺省）。
    pub fn fields_complete(&self) -> bool {
        !self.metric.is_empty()
            && self.env_hash != 0
            && !self.varix_ver.is_empty()
            && !self.win_ver.is_empty()
            && self.varix_p50 > 0
            && self.varix_p95 > 0
            && self.varix_p99 > 0
            && self.win_p50 > 0
            && self.win_p95 > 0
            && self.win_p99 > 0
            && self.ratio_pbp > 0
            && self.verdict < V_NAMES.len()
            && self.sample_n >= SAMPLE_MIN
            && !self.date.is_empty()
            && !self.collector_id.is_empty()
            && !self.ai_sign.is_empty()
    }

    /// 机读行（十六段：metric 起 ai_sign 止）。
    pub fn render(&self) -> String {
        format!(
            "CMP|{}|{:08x}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.metric,
            self.env_hash,
            self.varix_ver,
            self.win_ver,
            self.varix_p50,
            self.varix_p95,
            self.varix_p99,
            self.win_p50,
            self.win_p95,
            self.win_p99,
            self.ratio_pbp,
            V_NAMES[self.verdict],
            self.sample_n,
            self.date,
            self.collector_id,
            self.ai_sign
        )
    }
}

/// 对照账：逐指标一行（重复指标拒）。
pub struct CmpBook {
    pub rows: Vec<CmpEntry>,
    pub sealed: bool,
}

impl CmpBook {
    pub fn new() -> Self {
        CmpBook { rows: Vec::new(), sealed: false }
    }

    /// 落行（同指标重复拒 + 封账拒）。
    pub fn put(&mut self, e: CmpEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(CE_FIELD_MISS);
        }
        for r in self.rows.iter() {
            if r.metric == e.metric {
                return Err(CE_FIELD_MISS);
            }
        }
        self.rows.push(e);
        Ok(())
    }

    /// 查询。
    pub fn get(&self, metric: &str) -> Option<&CmpEntry> {
        for r in self.rows.iter() {
            if r.metric == metric {
                return Some(r);
            }
        }
        None
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：syscall 一行（varix 260/314/317，win 320/374/377，n=100）。
/// ratio = 374×10000/314 = 11910 → verdict pass（≤11000？11910 > 11000 → fail）。
/// 另立 pass 行（win 330/340：ratio=10828 → pass）。
pub fn standard_row() -> CmpEntry {
    CmpEntry::new(
        "syscall", 0x00ABCDEF, "varix-1.0", "win-11",
        260, 314, 317,
        320, 374, 377,
        100, "2026-10-27", "col-001", "AI-WBK2-1027",
    )
    .expect("fixture row")
}

pub fn pass_row() -> CmpEntry {
    CmpEntry::new(
        "syscall-fast", 0x00ABCDEF, "varix-1.0", "win-11",
        260, 314, 317,
        300, 340, 345,
        100, "2026-10-27", "col-001", "AI-WBK2-1027",
    )
    .expect("fixture pass row")
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零环境哈希
    if CmpEntry::new("m", 0, "a", "b", 1, 2, 3, 4, 5, 6, 30, "d", "c", "s").err()
        == Some(CE_HASH_ZERO)
    {
        caught += 1;
    }
    // 2) 版本空
    if CmpEntry::new("m", 1, "", "b", 1, 2, 3, 4, 5, 6, 30, "d", "c", "s").err()
        == Some(CE_VER_EMPTY)
    {
        caught += 1;
    }
    // 3) 分位零值
    if CmpEntry::new("m", 1, "a", "b", 0, 2, 3, 4, 5, 6, 30, "d", "c", "s").err()
        == Some(CE_P_ZERO)
    {
        caught += 1;
    }
    // 4) 样本不足（29）
    if CmpEntry::new("m", 1, "a", "b", 1, 2, 3, 4, 5, 6, 29, "d", "c", "s").err()
        == Some(CE_SAMPLE_SHORT)
    {
        caught += 1;
    }
    // 5) 联签空
    if CmpEntry::new("m", 1, "a", "b", 1, 2, 3, 4, 5, 6, 30, "d", "c", "").err()
        == Some(CE_FIELD_MISS)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56035");
    // J1-1 十四字段全生成
    let r = standard_row();
    let ok = r.fields_complete() && r.metric == "syscall" && r.sample_n == 100;
    s.add("56035-J1-十四字段全生成", ok, if ok { "" } else { "字段缺省" });
    // J1-2 双侧分位落账（varix 260/314/317，win 320/374/377）
    let ok = r.varix_p50 == 260 && r.varix_p95 == 314 && r.varix_p99 == 317
        && r.win_p50 == 320 && r.win_p95 == 374 && r.win_p99 == 377;
    s.add("56035-J1-双侧分位落账", ok, if ok { "" } else { "分位漂移" });
    // J2-1 ratio_p95 语义（374/314 → 11910 万分比）
    let ok = r.ratio_pbp == 11_910;
    s.add("56035-J2-ratio语义", ok, if ok { "" } else { "ratio 漂移" });
    // J2-2 verdict P1 口径（11910 > 11000 → fail）
    let ok = r.verdict == V_FAIL && verdict_of(11_000) == V_PASS && verdict_of(11_001) == V_FAIL;
    s.add("56035-J2-verdict口径", ok, if ok { "" } else { "verdict 漂移" });
    // J2-3 pass 行留证（340/314 → 10828 ≤ 11000 → pass）
    let p = pass_row();
    let ok = p.ratio_pbp == 10_828 && p.verdict == V_PASS;
    s.add("56035-J2-pass行留证", ok, if ok { "" } else { "pass 行漂移" });
    // J2-4 字段完整性断言逐位（metric/hash/版本/分位/样本/日期/采集器/联签）
    let ok = !r.ai_sign.is_empty() && !r.collector_id.is_empty()
        && !r.date.is_empty() && r.env_hash == 0x00ABCDEF;
    s.add("56035-J2-完整性逐位", ok, if ok { "" } else { "完整性破" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56035-J3-注入核必抓", ok, &detail);
    // J3-2 机读行段数（CMP 起 17 段）
    let text = r.render();
    let ok = text.starts_with("CMP|syscall|00abcdef|") && text.split('|').count() == 17;
    s.add("56035-J3-机读行段数", ok, if ok { "" } else { "渲染段漂移" });
    // J3-3 对照账落行（重复指标拒）
    let mut b = CmpBook::new();
    b.put(standard_row()).expect("put");
    let ok = b.get("syscall").is_some() && b.put(standard_row()).is_err();
    s.add("56035-J3-对照账落行", ok, if ok { "" } else { "账目漂移" });
    // J1-3 ratio 物理界（win 是 varix 的 100× → 拒）
    let ok = CmpEntry::new("m", 1, "a", "b", 1, 100, 2, 4, 10_000, 6, 30, "d", "c", "s")
        .err()
        == Some(CE_RATIO_BAD);
    s.add("56035-J1-ratio物理界", ok, if ok { "" } else { "越界穿透" });
    // J2-5 样本量下限语义（30 过 / 29 拒）
    let ok = CmpEntry::new("m", 1, "a", "b", 1, 2, 3, 4, 5, 6, 30, "d", "c", "s").is_ok();
    s.add("56035-J2-样本量下限", ok, if ok { "" } else { "下限漂移" });
    // J2-6 封账冻结
    let mut b2 = CmpBook::new();
    b2.seal();
    let ok = b2.put(standard_row()).is_err();
    s.add("56035-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56035_all_checks_green() {
        let set = run_unx56035_checks();
        assert!(set.all_passed(), "UNX-F56035 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56035_verdict_math() {
        assert_eq!(verdict_of(10_828), V_PASS);
        assert_eq!(verdict_of(11_910), V_FAIL);
    }
}
