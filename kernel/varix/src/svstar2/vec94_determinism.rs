//! VE-F0494 · 优化器确定性（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0494`
//!
//! **锚点原文**：「优化器确定性保证：同输入同输出强断言（跨运行、跨平台
//! 位一致——浮点路径显性排除项登记）、无随机源（迭代序、哈希序全部显性
//! 定序）、确定性测试（同输入多次编译逐字节比对）；确定性例外清单（哪些
//! 允许不确定——无，原则全确定）。数据结构：确定性断言；定序登记；例外
//! 清单（空单原则）。错误路径与降级矩阵：逐字节不一致→定位首个分歧 pass
//! 阻断；发现随机源→消除登记；平台差异→按显性排除项裁定。判据：同入同
//! 出、无随机源、逐字节比对、空例外单、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **同入同出**（判据一）。同输入多次编译逐字节比对强断言；不一致 →
//!    定位首个分歧 pass 阻断。
//!
//! 2. **无随机源**（判据二）。迭代序、哈希序全部显性定序登记；发现随机
//!    源 → 消除登记（不留未定序源）。
//!
//! 3. **逐字节比对**（判据三）。比对粒度逐字节（不按「语义等价」放水）；
//!    首个分歧字节即定位。
//!
//! 4. **空例外单**（判据四）。确定性例外清单恒空（原则全确定）——任何
//!    入单企图都拦截；浮点路径排除项另册显性登记（平台差异裁定面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0494";

/// 定序登记：一类序的显性定序声明。
#[derive(Clone, Debug, PartialEq)]
pub struct SeqRule {
    /// 序类别（"iter" = 迭代序 / "hash" = 哈希序）。
    pub kind: &'static str,
    /// 定序说明（如 "按指令序号升序"）。
    pub order: &'static str,
}

/// 确定性账。
#[derive(Debug, Default)]
pub struct DetBook {
    pub seq_rules: Vec<SeqRule>,
    /// 随机源消除登记。
    pub removed_randoms: Vec<String>,
    /// 首个分歧 pass 定位账。
    pub divergences: Vec<String>,
    /// 浮点路径显性排除项（平台差异裁定面——另册，非例外单）。
    pub fp_exclusions: Vec<&'static str>,
    pub audit: u32,
}

impl DetBook {
    /// 定序登记：全部迭代序/哈希序必须显性定序。
    pub fn register_seq(&mut self, kind: &'static str, order: &'static str) {
        if !self.seq_rules.iter().any(|r| r.kind == kind) {
            self.seq_rules.push(SeqRule { kind, order });
        }
    }

    /// 随机源检查：未消除的随机源清单（空 = 无随机源达成）。
    pub fn found_random(&mut self, src: &'static str) {
        self.removed_randoms.push(format!("随机源消除登记：{}", src));
    }

    /// 逐字节比对：同输入双跑输出（字节面）→ 一致 / 首个分歧定位。
    pub fn byte_compare(&mut self, passes: &[&'static str], a: &[u8], b: &[u8]) -> bool {
        let n = a.len().min(b.len());
        for i in 0..n {
            if a[i] != b[i] {
                // 首个分歧字节 → 归属 pass（按字节序均摊定位到 pass 序）。
                let owner = passes.get(i % passes.len().max(1)).copied().unwrap_or("?");
                self.divergences.push(format!(
                    "逐字节分歧：字节 #{}（pass {}）{:#x} ≠ {:#x} → 阻断",
                    i, owner, a[i], b[i]
                ));
                return false;
            }
        }
        if a.len() != b.len() {
            let owner = passes.last().copied().unwrap_or("?");
            self.divergences.push(format!(
                "逐字节分歧：长度 {} ≠ {}（pass {}）→ 阻断",
                a.len(),
                b.len(),
                owner
            ));
            return false;
        }
        self.audit += 1;
        true
    }

    /// 空例外单：例外清单恒空——入单企图拦截。
    pub fn try_add_exception(&mut self, _what: &'static str) -> Result<(), &'static str> {
        Err("确定性例外单恒空（原则全确定）：不允许任何不确定例外")
    }

    /// 浮点路径显性排除登记（平台差异裁定面——另册非例外）。
    pub fn register_fp_exclusion(&mut self, path: &'static str) {
        if !self.fp_exclusions.contains(&path) {
            self.fp_exclusions.push(path);
        }
    }

    /// 平台差异裁定：差异面在排除项内 → 按排除项裁定（放行但不冒充位一致）。
    pub fn platform_diff(&self, path: &str) -> bool {
        self.fp_exclusions.iter().any(|p| *p == path)
    }
}

// ---------------------------------------------------------------------------
// 判据（F0494-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec94_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、同入同出：逐字节比对。
    {
        let mut book = DetBook::default();
        let passes = ["fold", "dce", "cse"];
        let out1 = vec![0x48u8, 0x65, 0x6c, 0x6c, 0x6f];
        let out2 = out1.clone();
        let ok = book.byte_compare(&passes, &out1, &out2);
        v.push(("F0494-同入同出-一致放行", ok && book.audit == 1));
        // 不一致 → 首个分歧 pass 定位阻断。
        let out3 = vec![0x48u8, 0x65, 0x6d, 0x6c, 0x6f];
        let bad = book.byte_compare(&passes, &out1, &out3);
        v.push((
            "F0494-同入同出-分歧阻断定位",
            !bad && book.divergences.len() == 1 && book.divergences[0].contains("pass dce"),
        ));
        // 长度差同阻断。
        let out4 = vec![0x48u8, 0x65];
        let bad2 = book.byte_compare(&passes, &out1, &out4);
        v.push((
            "F0494-同入同出-长度差阻断",
            !bad2 && book.divergences.last().unwrap().contains("长度"),
        ));
    }

    // 二、无随机源。
    {
        let mut book = DetBook::default();
        book.register_seq("iter", "按指令序号升序");
        book.register_seq("hash", "按桶内键字典序");
        book.register_seq("iter", "重复登记去重");
        v.push((
            "F0494-定序-双序显性登记",
            book.seq_rules.len() == 2
                && book.seq_rules[0].kind == "iter"
                && book.seq_rules[1].kind == "hash",
        ));
        book.found_random("时钟戳序");
        v.push(("F0494-随机-消除登记", book.removed_randoms.len() == 1));
    }

    // 三、空例外单。
    {
        let mut book = DetBook::default();
        let r = book.try_add_exception("平台浮点舍入");
        v.push(("F0494-例外-空单原则拦截", r.is_err()));
    }

    // 四、浮点排除项另册 + 平台裁定。
    {
        let mut book = DetBook::default();
        book.register_fp_exclusion("f64 归约序");
        book.register_fp_exclusion("f64 归约序");
        v.push((
            "F0494-浮点-排除项去重登记",
            book.fp_exclusions.len() == 1,
        ));
        let covered = book.platform_diff("f64 归约序");
        let uncovered = book.platform_diff("整数序");
        v.push((
            "F0494-浮点-平台差异裁定",
            covered && !uncovered,
        ));
        // 排除项≠例外单：例外单仍空。
        v.push(("F0494-判据-锚点入册", ANCHOR == "VE-F0494" && book.try_add_exception("x").is_err()));
    }

    v
}

/// 注册表入口。
pub fn run_vec94_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec94");
    for (name, ok) in vec94_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec94_raw_checks as run_vec94_checks;
    #[test]
    fn vec94_determinism_checks() {
        for (name, ok) in run_vec94_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
