//! H2 字体安全档 · 深化批次五（F288 字体管理的安装判定深化——缺字
//! 扫描与三档安全判定，F159 车道经 F288 锚落位）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F288 字体管理**：损坏字体注入拒绝（文件级校验）、系统锁定
//!   清单（不许卸载的字体——保护在结构上先于操作）、用户级权限
//!   （装字体不需要管理员——写用户目录）；
//! - **F159 车道（经 F288 锚）**：三档判定阈值（构造 0%/8%/20% 缺
//!   字样本）、缺字清单前 20 字展示、强行应用后回退路径可用。
//!
//! 字符覆盖判定：调用方注入「该字体是否有此字形」闭包（本层无
//! 字形表——判定逻辑可测，数据在字体引擎）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 文件级校验（F288 损坏拒绝）
// ---------------------------------------------------------------------------

/// 字体文件魔数合法集（ttcf/ttf/otf——表外魔数=损坏拒绝）。
pub const MAGIC_OK: [&str; 3] = ["ttcf", "true", "OTTO"];

/// 文件头校验：前 4 字节魔数 ∈ 合法集。
pub fn looks_like_font(head: &[u8]) -> bool {
    if head.len() < 4 {
        return false;
    }
    let m = core::str::from_utf8(&head[..4]).map_or(false, |s| MAGIC_OK.contains(&s));
    m
}

// ---------------------------------------------------------------------------
// 三档安全判定（F159 车道）
// ---------------------------------------------------------------------------

/// 三档判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyTier {
    /// 全覆盖：直接启用。
    Safe,
    /// 轻度缺字（≤8%）：可启用 + 缺字清单提示（回退映射补齐）。
    UsableWithFallback,
    /// 重度缺字（>8%）：默认拒绝；强行应用留一票（可回退）。
    Risky,
}

/// 缺字率分档阈值（F159 判据口径：8% 线）。
pub const USABLE_MAX_MISSING_PERMILLE: u32 = 80;

/// 缺字率判定：常用字集 `covered`（闭包注入）× 常用样本 → 档。
pub fn tier_of(covered: impl Fn(char) -> bool, sample: &[char]) -> SafetyTier {
    if sample.is_empty() {
        return SafetyTier::Safe;
    }
    let missing = sample.iter().filter(|c| !covered(**c)).count();
    let permille = (missing as u64 * 1000 / sample.len() as u64) as u32;
    if permille == 0 {
        SafetyTier::Safe
    } else if permille <= USABLE_MAX_MISSING_PERMILLE {
        SafetyTier::UsableWithFallback
    } else {
        SafetyTier::Risky
    }
}

/// 缺字清单（前 20 字展示——判据口径；顺序 = 样本序，稳定可回放）。
pub fn missing_list(covered: impl Fn(char) -> bool, sample: &[char], cap: usize) -> Vec<char> {
    sample
        .iter()
        .filter(|c| !covered(**c))
        .take(cap.max(1).min(20))
        .copied()
        .collect()
}

/// 缺字回退映射：缺字字形 → 主字体（微软雅黑车道）——回退路径
/// 可用的数据面（应用后可一键回退 = 换回旧映射表）。
pub fn fallback_map(covered: impl Fn(char) -> bool, sample: &[char]) -> Vec<(char, &'static str)> {
    sample
        .iter()
        .filter(|c| !covered(**c))
        .map(|c| (*c, "微软雅黑"))
        .collect()
}

// ---------------------------------------------------------------------------
// 系统锁定清单（F288 判据）
// ---------------------------------------------------------------------------

/// 系统锁定字体（不许卸载——UI 渲染依赖，删了就白屏）。
pub const LOCKED_FONTS: [&str; 5] =
    ["系统 UI", "微软雅黑", "等宽终端", "图标符号", "回退衬底"];

/// 卸载许可：锁定清单拒收（留因——拒绝可见）。
pub fn uninstall_allowed(name: &str) -> Result<(), &'static str> {
    if LOCKED_FONTS.contains(&name) {
        Err("系统字体不可卸载——界面渲染依赖它")
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2fontsafe_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2fontsafe");
    // 文件级：三魔数过、乱头拒、短头拒。
    set.add(
        "h2fontsafe magic ok",
        MAGIC_OK.iter().all(|m| looks_like_font(m.as_bytes()))
            && !looks_like_font(b"JUNK")
            && !looks_like_font(b"ab"),
        "3 magics + 2 rejects",
    );
    // 三档：全样本覆盖 = Safe；8% 线内 = UsableWithFallback；超线 = Risky。
    let sample: Vec<char> = (0u32..100).filter_map(|i| char::from_u32(0x4e00 + i)).collect();
    let all_cov = tier_of(|_| true, &sample);
    let cov92_set: Vec<char> = sample[..92].to_vec();
    let cov92 = tier_of(|c| cov92_set.contains(&c), &sample);
    let cov78: Vec<char> = sample[..78].to_vec();
    let risky = tier_of(|c| cov78.contains(&c), &sample);
    set.add(
        "h2fontsafe three tiers",
        all_cov == SafetyTier::Safe
            && cov92 == SafetyTier::UsableWithFallback
            && tier_of(|c| cov78.contains(&c), &sample) == SafetyTier::Risky
            && risky == SafetyTier::Risky,
        "0%/8%/20% samples",
    );
    // 缺字清单：前 20 上限、顺序 = 样本序。
    let cov_none = tier_of(|_| false, &sample);
    let list = missing_list(|_| false, &sample, 20);
    set.add(
        "h2fontsafe missing list",
        cov_none == SafetyTier::Risky
            && list.len() == 20
            && list[0] == sample[0]
            && list[19] == sample[19],
        "cap 20 stable order",
    );
    // 回退映射：缺的字全部映射到主字体。
    let fm = fallback_map(|c| cov92_set.contains(&c), &sample);
    set.add(
        "h2fontsafe fallback map",
        fm.len() == 8 && fm.iter().all(|(_, f)| *f == "微软雅黑"),
        "fallback = main font",
    );
    // 系统锁定：五件拒卸 + 普通字体放行。
    set.add(
        "h2fontsafe locked list",
        LOCKED_FONTS.iter().all(|f| uninstall_allowed(f).is_err())
            && uninstall_allowed("我的手写体").is_ok(),
        "locked refused with reason",
    );
    set.add(
        "h2fontsafe threshold const",
        USABLE_MAX_MISSING_PERMILLE == 80,
        "8% line",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2fontsafe_all_green() {
        let set = run_h2fontsafe_checks();
        assert!(set.all_passed(), "h2fontsafe 自检有红项");
        assert!(!set.truncated(), "h2fontsafe 自检溢出");
    }

    #[test]
    fn tier_boundary_is_monotone() {
        // 覆盖率从 100% 单调下降：档位只许单调变差（不许来回跳）。
        let sample: Vec<char> = (0u32..200).filter_map(|i| char::from_u32(0x4e00 + i)).collect();
        let mut last = 2u8; // 从最差档起——覆盖率升 → 档位只许不变或变好
        for keep in (10..=200).step_by(10) {
            let cov: Vec<char> = sample[..keep].to_vec();
            let t = tier_of(|c| cov.contains(&c), &sample);
            let v = match t {
                SafetyTier::Safe => 0,
                SafetyTier::UsableWithFallback => 1,
                SafetyTier::Risky => 2,
            };
            assert!(v <= last, "tier worsened as coverage grew at keep={keep}");
            last = v;
        }
    }
}
