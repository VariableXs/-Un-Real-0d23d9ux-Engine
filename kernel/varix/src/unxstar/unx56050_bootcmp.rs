//! UNX-F56050 启动对照行生成器（0x60620 段）
//!
//! 锚点：UNX-F56050-J1 cmp_entry 全字段（分段列扩展：全程 ratio + 五
//! 段逐段 ratio 双口径落账）；J2 字段完整性断言（十四基础字段 + 分段
//! 扩展列逐位非缺省）；J3 verdict 双口径判定（全程 ratio 判总体 + 段
//! ratio 逐段留证）三判据全绿。判据锚定 §AI-71.2 专题三。
//! 域：CoRun·UNX-F2-B40（启动分段域·对照行面）。
//!
//! 结构：启动对照行账目（全程字段 + 五段扩展列）+ ratio 双口径生成核
//! （全程/逐段万分比）+ 字段完整性断言 + verdict 判定核 + 注入核（零
//! 哈希/空版本/零分位/段缺列/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60620 独占 0x20）。
pub const SEG_BASE: u32 = 0x60620;
pub const BC_HASH_ZERO: u32 = SEG_BASE + 0x01; // env_hash 零哨兵
pub const BC_VER_EMPTY: u32 = SEG_BASE + 0x02; // 版本串空
pub const BC_P_ZERO: u32 = SEG_BASE + 0x03; // 分位零值
pub const BC_SEG_MISS: u32 = SEG_BASE + 0x04; // 分段列缺
pub const BC_SEALED: u32 = SEG_BASE + 0x05; // 对照账已封账拒写
pub const BC_RATIO_BAD: u32 = SEG_BASE + 0x06; // ratio 越物理界
pub const BC_FIELD_MISS: u32 = SEG_BASE + 0x07; // 基础字段缺

/// 五段数（与 F56041 同源）。
pub const G_COUNT: usize = 5;

/// P1 倍率（万分比 11000 判过）。
pub const P1_MULT_PBP: u64 = 11_000;

/// verdict 闭集。
pub const V_PASS: usize = 0;
pub const V_FAIL: usize = 1;
pub const V_NAMES: [&str; 2] = ["pass", "fail"];

pub fn verdict_of(ratio_pbp: u64) -> usize {
    if ratio_pbp <= P1_MULT_PBP {
        V_PASS
    } else {
        V_FAIL
    }
}

/// 启动对照行：基础十四字段 + 五段扩展列（双侧段 P95）。
#[derive(Clone)]
pub struct BootCmpEntry {
    pub metric: String,
    pub env_hash: u32,
    pub varix_ver: String,
    pub win_ver: String,
    pub varix_total_p95: u64,
    pub win_total_p95: u64,
    pub varix_seg_p95: [u64; G_COUNT],
    pub win_seg_p95: [u64; G_COUNT],
    pub ratio_total_pbp: u64,
    pub ratio_seg_pbp: [u64; G_COUNT],
    pub verdict: usize,
    pub sample_n: u32,
    pub date: String,
    pub ai_sign: String,
}

impl BootCmpEntry {
    /// 构造即全字段校验（缺段列/零值即拒）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metric: &str,
        env_hash: u32,
        varix_ver: &str,
        win_ver: &str,
        varix_total_p95: u64,
        win_total_p95: u64,
        varix_seg_p95: [u64; G_COUNT],
        win_seg_p95: [u64; G_COUNT],
        sample_n: u32,
        date: &str,
        ai_sign: &str,
    ) -> Result<Self, u32> {
        if env_hash == 0 {
            return Err(BC_HASH_ZERO);
        }
        if metric.is_empty() || varix_ver.is_empty() || win_ver.is_empty() {
            return Err(BC_VER_EMPTY);
        }
        if date.is_empty() || ai_sign.is_empty() {
            return Err(BC_FIELD_MISS);
        }
        if varix_total_p95 == 0 || win_total_p95 == 0 {
            return Err(BC_P_ZERO);
        }
        for g in 0..G_COUNT {
            if varix_seg_p95[g] == 0 || win_seg_p95[g] == 0 {
                return Err(BC_SEG_MISS);
            }
        }
        if sample_n < 30 {
            return Err(BC_FIELD_MISS);
        }
        let ratio_total_pbp = win_total_p95 * 10_000 / varix_total_p95;
        if ratio_total_pbp > 100_000 {
            return Err(BC_RATIO_BAD);
        }
        let mut ratio_seg_pbp = [0u64; G_COUNT];
        for g in 0..G_COUNT {
            ratio_seg_pbp[g] = win_seg_p95[g] * 10_000 / varix_seg_p95[g];
        }
        let verdict = verdict_of(ratio_total_pbp);
        Ok(BootCmpEntry {
            metric: metric.to_string(),
            env_hash,
            varix_ver: varix_ver.to_string(),
            win_ver: win_ver.to_string(),
            varix_total_p95,
            win_total_p95,
            varix_seg_p95,
            win_seg_p95,
            ratio_total_pbp,
            ratio_seg_pbp,
            verdict,
            sample_n,
            date: date.to_string(),
            ai_sign: ai_sign.to_string(),
        })
    }

    /// J2：字段完整性断言（全程 + 段列逐位非缺省）。
    pub fn fields_complete(&self) -> bool {
        !self.metric.is_empty()
            && self.env_hash != 0
            && !self.varix_ver.is_empty()
            && !self.win_ver.is_empty()
            && self.varix_total_p95 > 0
            && self.win_total_p95 > 0
            && self.ratio_total_pbp > 0
            && self.verdict < V_NAMES.len()
            && self.sample_n >= 30
            && !self.date.is_empty()
            && !self.ai_sign.is_empty()
            && self.ratio_seg_pbp.iter().all(|r| *r > 0)
            && self.varix_seg_p95.iter().all(|v| *v > 0)
            && self.win_seg_p95.iter().all(|v| *v > 0)
    }

    /// 机读行（BOOT|metric|hash|ver×2|total×2|ratio|verdict|段 ratio×5|n|date|sign）。
    pub fn render(&self) -> String {
        let mut seg_text = String::new();
        for g in 0..G_COUNT {
            seg_text.push_str(&format!("|{}", self.ratio_seg_pbp[g]));
        }
        format!(
            "BOOT|{}|{:08x}|{}|{}|{}|{}|{}|{}{}|{}|{}|{}",
            self.metric,
            self.env_hash,
            self.varix_ver,
            self.win_ver,
            self.varix_total_p95,
            self.win_total_p95,
            self.ratio_total_pbp,
            V_NAMES[self.verdict],
            seg_text,
            self.sample_n,
            self.date,
            self.ai_sign
        )
    }
}

/// 对照账。
pub struct BootCmpBook {
    pub rows: Vec<BootCmpEntry>,
    pub sealed: bool,
}

impl BootCmpBook {
    pub fn new() -> Self {
        BootCmpBook { rows: Vec::new(), sealed: false }
    }

    /// 落行（同 metric 重复拒 + 封账拒）。
    pub fn put(&mut self, e: BootCmpEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(BC_SEALED);
        }
        for r in self.rows.iter() {
            if r.metric == e.metric {
                return Err(BC_FIELD_MISS);
            }
        }
        self.rows.push(e);
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：boot 行（全程 varix 1900 / win 2200；五段逐段确定值）。
/// ratio_total = 2200×10000/1900 = 11578 → fail。
pub fn standard_row() -> BootCmpEntry {
    let vseg = [900u64, 400, 200, 250, 150];
    let wseg = [950u64, 450, 220, 270, 160];
    BootCmpEntry::new(
        "boot", 0x00ABCDEF, "varix-1.0", "win-11",
        1_900, 2_200, vseg, wseg,
        100, "2026-10-27", "AI-WBK2-1027",
    )
    .expect("fixture row")
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零哈希
    let mut vseg = [1u64; 5];
    let mut wseg = [1u64; 5];
    vseg[0] = 100;
    wseg[0] = 100;
    if BootCmpEntry::new("boot", 0, "a", "b", 100, 110, vseg, wseg, 30, "d", "s").err()
        == Some(BC_HASH_ZERO)
    {
        caught += 1;
    }
    // 2) 段列缺（段 2 零值）
    let mut vseg2 = vseg;
    vseg2[2] = 0;
    if BootCmpEntry::new("boot", 1, "a", "b", 100, 110, vseg2, wseg, 30, "d", "s").err()
        == Some(BC_SEG_MISS)
    {
        caught += 1;
    }
    // 3) 分位零值（全程）
    if BootCmpEntry::new("boot", 1, "a", "b", 0, 110, vseg, wseg, 30, "d", "s").err()
        == Some(BC_P_ZERO)
    {
        caught += 1;
    }
    // 4) 样本不足
    if BootCmpEntry::new("boot", 1, "a", "b", 100, 110, vseg, wseg, 29, "d", "s").err()
        == Some(BC_FIELD_MISS)
    {
        caught += 1;
    }
    // 5) ratio 越界（win 是 varix 的 100×）
    if BootCmpEntry::new("boot", 1, "a", "b", 10, 1_000, vseg, wseg, 30, "d", "s").err()
        == Some(BC_RATIO_BAD)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56050_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56050");
    // J1-1 全字段落账（全程 + 五段扩展列）
    let r = standard_row();
    let ok = r.fields_complete() && r.metric == "boot" && r.sample_n == 100;
    s.add("56050-J1-全字段落账", ok, if ok { "" } else { "字段缺" });
    // J1-2 双侧分段列齐（varix 五段 900/400/200/250/150）
    let ok = r.varix_seg_p95 == [900, 400, 200, 250, 150]
        && r.win_seg_p95 == [950, 450, 220, 270, 160];
    s.add("56050-J1-分段列齐", ok, if ok { "" } else { "分段列缺" });
    // J2-1 ratio 双口径（全程 11578；段 0 = 950×10000/900 = 10555）
    let ok = r.ratio_total_pbp == 11_578 && r.ratio_seg_pbp[0] == 10_555;
    s.add("56050-J2-ratio双口径", ok, if ok { "" } else { "ratio 漂移" });
    // J2-2 段 ratio 逐段语义（段 2 = 220×10000/200 = 11000）
    let ok = r.ratio_seg_pbp[2] == 11_000;
    s.add("56050-J2-段ratio语义", ok, if ok { "" } else { "段 ratio 漂移" });
    // J3-1 verdict 双口径（全程 fail 留证 + 段 0 pass 留证）
    let ok = r.verdict == V_FAIL
        && verdict_of(r.ratio_seg_pbp[0]) == V_PASS
        && verdict_of(11_000) == V_PASS
        && verdict_of(11_001) == V_FAIL;
    s.add("56050-J3-verdict双口径", ok, if ok { "" } else { "verdict 漂移" });
    // J3-2 字段完整性断言逐位
    let ok = !r.ai_sign.is_empty() && !r.date.is_empty() && r.env_hash == 0x00ABCDEF;
    s.add("56050-J3-完整性逐位", ok, if ok { "" } else { "完整性破" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56050-J3-注入核必抓", ok, &detail);
    // J1-3 机读行段数（BOOT 头 + metric/hash/双版本/双分位/ratio/verdict
    // + 5 段 ratio + n/date/sign = 17 段）
    let text = r.render();
    let ok = text.starts_with("BOOT|boot|00abcdef|") && text.split('|').count() == 17;
    s.add("56050-J1-机读行段数", ok, if ok { "" } else { "渲染段漂移" });
    // J2-3 对照账落行（重复 metric 拒）
    let mut b = BootCmpBook::new();
    b.put(standard_row()).expect("put");
    let ok = b.rows.len() == 1 && b.put(standard_row()).is_err();
    s.add("56050-J2-对照账落行", ok, if ok { "" } else { "账目漂移" });
    // J1-4 段数与 F56041 同源（5 段）
    let ok = G_COUNT == 5;
    s.add("56050-J1-段数同源", ok, if ok { "" } else { "段数漂移" });
    // J2-4 封账冻结
    let mut b2 = BootCmpBook::new();
    b2.seal();
    let ok = b2.put(standard_row()).is_err();
    s.add("56050-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56050_all_checks_green() {
        let set = run_unx56050_checks();
        assert!(set.all_passed(), "UNX-F56050 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56050_ratio_math() {
        let r = standard_row();
        assert_eq!(r.ratio_total_pbp, 11_578);
        assert_eq!(r.ratio_seg_pbp[0], 10_555);
    }
}
