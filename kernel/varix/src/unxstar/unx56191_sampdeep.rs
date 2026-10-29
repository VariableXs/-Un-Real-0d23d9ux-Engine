//! UNX-F56191 启动样本存证深化件（0x617E0 段）
//!
//! 锚点：UNX-F56191-J1 深化存证账（样本存证深化版——样本号 × 四段
//! 时长 × 16 子段指纹 × 环境摘要逐字段落账）；J2 存证语义（段和一致
//! + 子段指纹齐 + 环境摘要非空三性质）；J3 存证指纹（存证折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·样本存证深化面）。
//!
//! 结构：深化样本结构 + 存证核（和闸 + 指纹齐闸 + 摘要闸）+ 存证指
//! 纹 + 注入核（和不等/指纹缺/摘要空/样本重号/印后追加必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x617E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x617E0;
pub const SS_SUM_MISS: u32 = SEG_BASE + 0x01; // 段和不一致
pub const SS_FP_MISS: u32 = SEG_BASE + 0x02; // 子段指纹缺
pub const SS_ENV_EMPTY: u32 = SEG_BASE + 0x03; // 环境摘要空
pub const SS_DUP_SAMPLE: u32 = SEG_BASE + 0x04; // 样本重号
pub const SS_FROZEN: u32 = SEG_BASE + 0x05; // 印后追加

/// 深化启动样本存证。
#[derive(Clone)]
pub struct DeepSample {
    pub sample_no: u32,
    pub seg_ms: [u64; 4],
    /// 16 子段折叠指纹（每子段一个）。
    pub sub_fps: [u32; 16],
    pub env_digest: String,
    pub declared_total: u64,
}

impl DeepSample {
    /// 段和。
    pub fn seg_sum(&self) -> u64 {
        self.seg_ms.iter().sum()
    }

    /// 和闸。
    pub fn sum_ok(&self) -> bool {
        self.seg_sum() == self.declared_total
    }

    /// 指纹齐闸：16 子段指纹全非零。
    pub fn fps_full(&self) -> bool {
        self.sub_fps.iter().all(|&f| f != 0)
    }

    /// 摘要闸：环境摘要非空。
    pub fn env_ok(&self) -> bool {
        !self.env_digest.is_empty()
    }

    /// 存证合格总核。
    pub fn evidence_ok(&self) -> bool {
        self.sum_ok() && self.fps_full() && self.env_ok()
    }

    /// 存证指纹。
    pub fn sample_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.sample_no;
        for m in self.seg_ms {
            h = h.wrapping_mul(0x0100_0193) ^ (m as u32);
            h ^= (m >> 32) as u32;
        }
        for f in self.sub_fps {
            h = h.wrapping_mul(0x0100_0193) ^ f;
        }
        for b in self.env_digest.bytes() {
            h = h.wrapping_mul(0x0100_0193) ^ b as u32;
        }
        h = h.wrapping_mul(0x0100_0193);
        h
    }
}

/// 深化存证库。
pub struct SampleVault {
    samples: Vec<DeepSample>,
    sealed: bool,
}

impl SampleVault {
    pub fn new() -> Self {
        SampleVault {
            samples: Vec::new(),
            sealed: false,
        }
    }

    /// 入库：合格闸 + 重号拒 + 印后拒。
    pub fn deposit(&mut self, s: DeepSample) -> Result<(), u32> {
        if self.sealed {
            return Err(SS_FROZEN);
        }
        if !s.evidence_ok() {
            if !s.sum_ok() {
                return Err(SS_SUM_MISS);
            }
            if !s.fps_full() {
                return Err(SS_FP_MISS);
            }
            return Err(SS_ENV_EMPTY);
        }
        if self.samples.iter().any(|x| x.sample_no == s.sample_no) {
            return Err(SS_DUP_SAMPLE);
        }
        self.samples.push(s);
        Ok(())
    }

    pub fn sample(&self, no: u32) -> Option<&DeepSample> {
        self.samples.iter().find(|s| s.sample_no == no)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 库内总时长均值。
    pub fn mean_total(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|s| s.declared_total).sum();
        sum / self.samples.len() as u64
    }

    /// 库指纹（全部样本折叠）。
    pub fn vault_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for s in &self.samples {
            h = h.wrapping_mul(0x0100_0193) ^ s.sample_fp();
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封库印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.samples.is_empty() || self.samples.iter().any(|s| !s.evidence_ok()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_vault(&self) -> String {
        let mut out = String::new();
        for s in &self.samples {
            out.push_str(&format!(
                "S{}|sum={}|env={}\n",
                s.sample_no,
                s.seg_sum(),
                s.env_digest
            ));
        }
        if self.sealed {
            out.push_str("VAULT-SEAL\n");
        }
        out
    }
}

impl Default for SampleVault {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造一个合格深化样本。
fn good_sample(no: u32) -> DeepSample {
    let mut fps = [0u32; 16];
    for (i, f) in fps.iter_mut().enumerate() {
        *f = 0x1000_0000u32 + (no as u32) * 0x100 + i as u32;
    }
    DeepSample {
        sample_no: no,
        seg_ms: [800, 1800, 2600, 800],
        sub_fps: fps,
        env_digest: format!("env-{no}-b40"),
        declared_total: 6_000,
    }
}

/// 标准夹具：三样本库。
fn standard_vault() -> SampleVault {
    let mut v = SampleVault::new();
    for no in 1..=3u32 {
        let _ = v.deposit(good_sample(no));
    }
    v
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：段和不一致
    let mut s = good_sample(9);
    s.declared_total = 6_100;
    let mut v = SampleVault::new();
    if v.deposit(s) == Err(SS_SUM_MISS) {
        caught += 1;
    }
    // 注 2：子段指纹缺
    let mut s = good_sample(9);
    s.sub_fps[7] = 0;
    let mut v = SampleVault::new();
    if v.deposit(s) == Err(SS_FP_MISS) {
        caught += 1;
    }
    // 注 3：环境摘要空
    let mut s = good_sample(9);
    s.env_digest.clear();
    let mut v = SampleVault::new();
    if v.deposit(s) == Err(SS_ENV_EMPTY) {
        caught += 1;
    }
    // 注 4：样本重号
    let mut v = standard_vault();
    if v.deposit(good_sample(2)) == Err(SS_DUP_SAMPLE) {
        caught += 1;
    }
    // 注 5：印后追加
    let mut v = standard_vault();
    let _ = v.seal();
    if v.deposit(good_sample(9)) == Err(SS_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 深化存证账；J2 存证语义；J3 存证指纹。
pub fn run_unx56191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56191");
    // J1-1 三样本全入库
    let v = standard_vault();
    let ok = v.len() == 3 && (1..=3).all(|n| v.sample(n).is_some());
    s.add("56191-J1-三样本全入库", ok, if ok { "" } else { "存证账不全" });
    // J1-2 渲染导出齐
    let mut v2 = standard_vault();
    let _ = v2.seal();
    let text = v2.render_vault();
    let ok = text.contains("S1|sum=6000|env=env-1-b40") && text.contains("VAULT-SEAL");
    s.add("56191-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 单样本合格总核
    let g = good_sample(1);
    let ok = g.evidence_ok();
    s.add("56191-J2-单样本合格", ok, if ok { "" } else { "样本不合格" });
    // J2-2 和闸
    let ok = g.seg_sum() == 6_000 && g.sum_ok();
    s.add("56191-J2-和闸", ok, if ok { "" } else { "和漂移" });
    // J2-3 指纹齐闸
    let ok = g.fps_full();
    s.add("56191-J2-指纹齐闸", ok, if ok { "" } else { "指纹缺" });
    // J2-4 摘要闸
    let ok = g.env_ok();
    s.add("56191-J2-摘要闸", ok, if ok { "" } else { "摘要空" });
    // J2-5 库均值（6000）
    let ok = v.mean_total() == 6_000;
    s.add("56191-J2-库均值", ok, if ok { "" } else { "均值漂移" });
    // J2-6 重号拒
    let mut v3 = standard_vault();
    let ok = v3.deposit(good_sample(1)) == Err(SS_DUP_SAMPLE);
    s.add("56191-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J3-1 样本指纹确定
    let ok = good_sample(7).sample_fp() == good_sample(7).sample_fp();
    s.add("56191-J3-样本指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 段变签变
    let mut s2 = good_sample(7);
    let fp0 = s2.sample_fp();
    s2.seg_ms[0] += 1;
    let ok = s2.sample_fp() != fp0;
    s.add("56191-J3-段变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 环境变签变
    let mut s3 = good_sample(7);
    let fp0 = s3.sample_fp();
    s3.env_digest.push('x');
    let ok = s3.sample_fp() != fp0;
    s.add("56191-J3-环境变签变", ok, if ok { "" } else { "环境折叠退化" });
    // J3-4 库指纹确定 + 封库印
    let mut v4 = standard_vault();
    let fp0 = v4.vault_fp();
    let sealed_ok = v4.seal() && v4.sealed();
    let ok = v4.vault_fp() != fp0 && sealed_ok;
    s.add("56191-J3-库指纹与封印", ok, if ok { "" } else { "库指纹/封印漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56191-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56191_all_checks_green() {
        let set = run_unx56191_checks();
        assert!(set.all_passed(), "UNX-F56191 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56191_vault_stable() {
        let mut v = standard_vault();
        assert!(v.seal());
    }
}
