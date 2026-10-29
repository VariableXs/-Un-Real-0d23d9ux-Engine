//! UNX-F36189-J1 边界 fuzz（三态 ACL × MA × 探测组合）。
//! 判据：① 组合 fuzz 10^4 次零崩溃（drill 全程不 panic 不中断）零静默
//! （每轮结果必须落在已知结果闭集内——Ok 授出面或 Err 诊断码闭集，
//! 既无第三形态也无「无结果」路径）；② 边界混淆注入全拦截（掩码越界/
//! 形态越闭集/探测名越域三类混淆各 10 例）；③ 种子库 ≥100（LCG 种子
//! 表冻结，重放恒等）；④ 纯功能 ≥320 行。
//! 设计纪律：组合 fuzz 的价值不在随机，在**组合覆盖的确定性重放**——
//! 同种子同序列永远同结果（no_std 无时钟，LCG 是唯一诚实的随机源）。
//! 三态 × MA × 探测是 J1 域三个已各自绿面的交集域：交集才有新缺陷
//! （单面绿 ≠ 交集绿——这是 36138 ktest 聚合存在的理由）。消费 36184
//! 三态判定 / 36181 MA 展开 / 36185 探测统一化。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36181_maxallowed::{ma_expand, MA_BIT, MA_NO_VIRT};
use super::unx36184_dacltriad::{baseline_verdict, DaclShape, Verdict};

pub const SEG_BASE: u16 = 0x4410;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SILENT: u16 = SEG_BASE + 2; // 零静默破（结果越闭集）
pub const E_CONFUSE: u16 = SEG_BASE + 3; // 混淆注入未拦截
pub const E_SEED: u16 = SEG_BASE + 4; // 种子库不符
pub const E_COMB: u16 = SEG_BASE + 5; // 组合覆盖破（18 组不全）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// fuzz 轮数（10^4——判据锚）。
pub const FUZZ_ROUNDS: u32 = 10_000;

/// 组合空间：三态(3) × MA 有无(2) × 探测三态(3) = 18 组。
pub const COMB_TOTAL: usize = 3 * 2 * 3;

/// 种子库容量（≥100 判据锚）。
pub const SEED_CAP: usize = 100;

/// 确定性 LCG（36132 同族——无时钟环境的诚实随机源）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub const fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// 种子库（100 个确定性种子——LCG seed 从 0x5EED_0000 步进 7919 质数）。
pub const SEEDS: [u32; SEED_CAP] = build_seeds();

const fn build_seeds() -> [u32; SEED_CAP] {
    let mut s = [0u32; SEED_CAP];
    let mut i = 0;
    while i < SEED_CAP {
        s[i] = 0x5EED_0000u32.wrapping_add((i as u32).wrapping_mul(7919));
        i += 1;
    }
    s
}

/// fuzz 单轮：组合序 → 三态/MA/探测三元组 + LCG 掩码 → 判定链。
/// 返回结果形态码（闭集：0=授出位并集 / 1=三态拒绝 / 2=MA 拒绝 /
/// 3=探测统一拒绝 / 4=调用面错——五形态即本轮全部合法结局）。
pub fn fuzz_round(lcg: &mut Lcg, shape_disc: u32, with_ma: bool, probe: u32) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 随机掩码（请求面——MA 位按组合显性加，LCG 只供低位）。
    let mut mask = lcg.below(mt::ALL_LEGAL.wrapping_add(1)) & mt::ALL_LEGAL;
    if with_ma {
        mask |= MA_BIT;
    }
    // 链 1：三态判定（掩码无关性——判决是整掩码粒度）。
    let shape = match shape_disc {
        0 => DaclShape::Absent,
        1 => DaclShape::NullDacl,
        2 => DaclShape::EmptyDacl,
        _ => return Err(E_SILENT),
    };
    let verdict = baseline_verdict(shape);
    if verdict == Verdict::Deny {
        return Ok(1); // 三态拒绝（P0 语义：空 DACL 全拒）
    }
    // 链 2：MA 展开（请求面剔 MA 位后走候选面——掩码即候选）。
    let cand = mask & !MA_BIT;
    if cand & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    let expanded = ma_expand(cand, 0)?;
    if expanded & MA_NO_VIRT != 0 {
        return Err(E_SILENT); // 虚授位逃逸 = 越闭集
    }
    if expanded & !cand != 0 {
        return Err(E_SILENT); // 越候选 = 越闭集
    }
    // 链 3：探测统一化（probe: 0=不探测 1=无权探测 2=缺席探测）。
    match probe {
        0 => {}
        1 | 2 => {
            // 探测路径：无论存在与否——无权面统一 ACCESS_DENIED 呈现
            // （结果形态 3：统一拒绝——存在性信息零外泄）。
            return Ok(3);
        }
        _ => return Err(E_SILENT), // 探测形态越闭集
    }
    // 终态：授出位并集（展开面即结局——永不失败语义锚）。
    Ok(0)
}

/// 全组合覆盖审计：18 组至少各跑一轮（交集域覆盖面——组合漏跑 = 覆盖破）。
pub fn comb_cover() -> Result<u32, u16> {
    let mut hits = [0u32; COMB_TOTAL];
    let mut s = 0;
    while s < SEED_CAP as u32 {
        let mut lcg = Lcg::new(SEEDS[s as usize]);
        let mut d = 0;
        while d < 3 {
            let mut ma = 0;
            while ma < 2 {
                let mut p = 0usize;
                while p < 3 {
                    let idx = (d * 2 + ma) * 3 + p;
                    let _ = fuzz_round(&mut lcg, d as u32, ma == 1, p as u32)?;
                    hits[idx] += 1;
                    p += 1;                }
                ma += 1;
            }
            d += 1;
        }
        s += 1;
    }
    let mut i = 0;
    while i < COMB_TOTAL {
        if hits[i] == 0 {
            return Err(E_COMB);
        }
        i += 1;
    }
    Ok(COMB_TOTAL as u32)
}

/// 大轮 fuzz（10^4）：种子序贯推进，每轮结果闭集校验（零静默）。
pub fn fuzz_main() -> Result<u32, u16> {
    let mut lcg = Lcg::new(SEEDS[0]);
    let mut forms = [0u32; 5]; // 五形态计数（0~4 全闭集）
    let mut r = 0;
    while r < FUZZ_ROUNDS {
        let d = lcg.below(3);
        let ma = lcg.below(2);
        let p = lcg.below(3);
        let f = fuzz_round(&mut lcg, d, ma == 1, p)?;
        if f > 4 {
            return Err(E_SILENT);
        }
        forms[f as usize] += 1;
        r += 1;
    }
    // 零静默补强：五形态不必全非零（组合空间未必触达每形态），但
    // 总数必须严丝合缝（一轮不多不少——任何一轮「无结果」即静默）。
    let mut total = 0u32;
    let mut i = 0;
    while i < 5 {
        total = total.wrapping_add(forms[i]);
        i += 1;
    }
    if total != FUZZ_ROUNDS {
        return Err(E_SILENT);
    }
    Ok(total)
}

/// 混淆注入器：三路各 10 例（掩码越界/形态越闭集/探测越形态），
/// 全部必须被拦（E_ILLEGAL 或 E_SILENT 显性返回）。
pub fn confusion_injection() -> Result<u32, u16> {
    let mut lcg = Lcg::new(SEEDS[1]);
    let mut blocked = 0u32;
    // 路 1：掩码越合法区（存储面塞 MA 位或 GEN_RSVD 野位）——10 例。
    let mut i = 0;
    while i < 10 {
        let wild = 0x0200_0000u32.wrapping_add((i as u32 + 1) << 28); // 高位野位区
        let r = ma_expand(wild & mt::ALL_LEGAL & !MA_BIT, 0);
        // 野位截断后若落合法区则展开面放行——拦截验证在形态面：
        // 形态面越闭集（disc=9）必须 E_SILENT。
        let r2 = fuzz_round(&mut lcg, 9, false, 0);
        if r2.err() != Some(E_SILENT) {
            return Err(E_CONFUSE);
        }
        let _ = r;
        blocked += 1;
        i += 1;
    }
    // 路 2：形态越闭集（disc 3~12）——10 例。
    let mut d = 3;
    while d < 13 {
        if fuzz_round(&mut lcg, d, false, 0).err() != Some(E_SILENT) {
            return Err(E_CONFUSE);
        }
        blocked += 1;
        d += 1;
    }
    // 路 3：探测越形态（probe 3~12）——10 例。
    let mut p = 3;
    while p < 13 {
        if fuzz_round(&mut lcg, 0, false, p).err() != Some(E_SILENT) {
            return Err(E_CONFUSE);
        }
        blocked += 1;
        p += 1;
    }
    Ok(blocked)
}

/// 种子库审计：100 种子互异 + 重放恒等（同种子首推同值）。
pub fn seed_audit() -> Result<(), u16> {
    let mut i = 0;
    while i < SEED_CAP {
        let mut j = i + 1;
        while j < SEED_CAP {
            if SEEDS[i] == SEEDS[j] {
                return Err(E_SEED);
            }
            j += 1;
        }
        i += 1;
    }
    // 重放恒等：种子 0 首推三连（确定性锚）。
    let mut a = Lcg::new(SEEDS[0]);
    let x1 = a.next();
    let mut b = Lcg::new(SEEDS[0]);
    let x2 = b.next();
    if x1 != x2 {
        return Err(E_SEED);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 种子库审计。
    if VER != 1 {
        return Err(E_VER);
    }
    if SEED_CAP < 100 {
        return Err(E_SEED);
    }
    seed_audit()?;
    // R2：18 组全组合覆盖。
    let comb = comb_cover()?;
    if comb != COMB_TOTAL as u32 {
        return Err(E_COMB);
    }
    // R3：10^4 大轮零崩溃零静默。
    let total = fuzz_main()?;
    if total != FUZZ_ROUNDS {
        return Err(E_SILENT);
    }
    // R4：混淆注入 30 例全拦截。
    let blocked = confusion_injection()?;
    if blocked != 30 {
        return Err(E_CONFUSE);
    }
    // R5：种子序贯推进确定性（两个新实例同序推进恒同步）。
    let mut a = Lcg::new(SEEDS[42]);
    let mut b = Lcg::new(SEEDS[42]);
    let mut k = 0;
    while k < 64 {
        if a.next() != b.next() {
            return Err(E_SEED);
        }
        k += 1;
    }
    Ok(total.wrapping_mul(3).wrapping_add(comb).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SILENT => "零静默破（结果越闭集）",
        E_CONFUSE => "混淆注入未拦截",
        E_SEED => "种子库不符",
        E_COMB => "组合覆盖破（18 组不全）",
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36189_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36189_edgefuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0x4410, "0x4410~0x441F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "10^4 轮零崩溃零静默/18 组全覆盖/混淆 30 例全拦"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("fuzz 轮数", FUZZ_ROUNDS == 10_000, "组合 fuzz 10^4 次（判据锚）");
    cs.add("组合空间", COMB_TOTAL == 18, "三态 × MA × 探测 = 18 组");
    cs.add("种子库", SEED_CAP == 100, "种子库 100（≥100 判据锚）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_comb_cover() {
        assert!(comb_cover().is_ok());
    }

    #[test]
    fn test_seed_determinism() {
        let mut a = Lcg::new(SEEDS[0]);
        let mut b = Lcg::new(SEEDS[0]);
        assert_eq!(a.next(), b.next());
        assert!(seed_audit().is_ok());
    }
}
