//! VE-F0483 · 变体一致性验证（VE-C 域 · 变体组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0483`
//!
//! **锚点原文**：「变体一致性验证：同源不同参数变体的语义一致性抽样对拍
//! （参数只应影响预期维度，其他维度输出应等价）、变体与基础版对拍（参数
//! 回退语义等价验证）；不一致定位到参数维度。判据：抽样对拍、维度定位、
//! 保守判定、覆盖门槛、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **抽样对拍**（判据一）。同源不同参数变体抽样对拍（预期维度外输出
//!    应等价——参数只影响声明的维度面）。
//!
//! 2. **维度定位**（判据二）。不一致定位到参数维度（哪个参数引发的差异
//!    可溯——维度账逐参对拍）。
//!
//! 3. **保守判定**（判据三）。等价判定歧义 → 保守判不一致（宁可误报阻断
//!    也不放过）。
//!
//! 4. **覆盖门槛**（判据四）。抽样覆盖不足 → 加样阻断（最低覆盖门槛：
//!    [`MIN_SAMPLES`]）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0483";

/// 最低抽样覆盖门槛。
pub const MIN_SAMPLES: usize = 8;

// ---------------------------------------------------------------------------
// 对拍面（输出投影 + 维度账）
// ---------------------------------------------------------------------------

/// 变体输出投影（对拍的比较面：各维度值）。
#[derive(Clone, Debug, PartialEq)]
pub struct VarOutput {
    /// 维度名 → 输出值摘要。
    pub dims: Vec<(String, String)>,
}

impl VarOutput {
    /// 等价判定（保守面）：任一维度差异或维度集不同 → 不一致。
    pub fn equiv(&self, other: &VarOutput) -> bool {
        if self.dims.len() != other.dims.len() {
            return false;
        }
        self.dims
            .iter()
            .zip(other.dims.iter())
            .all(|((a1, v1), (a2, v2))| a1 == a2 && v1 == v2)
    }
    /// 首个差异维度（定位面）。
    pub fn first_diff(&self, other: &VarOutput) -> Option<String> {
        for ((a1, v1), (a2, v2)) in self.dims.iter().zip(other.dims.iter()) {
            if a1 != a2 {
                return Some(format!("维度集不同：{} vs {}", a1, a2));
            }
            if v1 != v2 {
                return Some(a1.clone());
            }
        }
        None
    }
}

/// 对拍记录：(抽样序, 维度, 是否一致)。
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRec {
    pub sample: usize,
    pub dim: String,
}

/// 一致性验证器。
#[derive(Debug, Default)]
pub struct Consistency {
    pub diffs: Vec<DiffRec>,
    pub samples: usize,
    /// 保守判定账（歧义 → 判不一致的登记）。
    pub ambiguous: Vec<String>,
}

impl Consistency {
    pub fn new() -> Self {
        Self::default()
    }
    /// 单维对拍：预期维度允许差异，其余维度必须等价。
    /// ambiguous=true → 保守判不一致（宁可误报）。
    pub fn sample_pair(
        &mut self,
        idx: usize,
        expected_dim: &str,
        a: &VarOutput,
        b: &VarOutput,
        ambiguous: bool,
    ) -> bool {
        self.samples += 1;
        if ambiguous {
            // 保守判定：歧义即不一致（入账可溯）。
            self.ambiguous.push(format!("抽样 {} 等价歧义 → 保守判不一致", idx));
            self.diffs.push(DiffRec { sample: idx, dim: "<ambiguous>".to_string() });
            return false;
        }
        // 预期维度排除后的等价判定。
        let strip = |o: &VarOutput| -> Vec<(String, String)> {
            o.dims.iter().filter(|(d, _)| d != expected_dim).cloned().collect()
        };
        let (sa, sb) = (strip(a), strip(b));
        let eq = sa.len() == sb.len()
            && sa.iter().zip(sb.iter()).all(|(x, y)| x == y);
        if !eq {
            // 维度定位：找首个差异维。
            let dim = a
                .first_diff(b)
                .unwrap_or_else(|| "<unknown>".to_string());
            self.diffs.push(DiffRec { sample: idx, dim: dim.clone() });
        }
        eq
    }
    /// 覆盖门槛：抽样数达 MIN_SAMPLES 才可下结论。
    pub fn covered(&self) -> bool {
        self.samples >= MIN_SAMPLES
    }
    /// 一致结论：覆盖达标 + 零差异 + 零歧义。
    pub fn verdict(&self) -> Result<(), String> {
        if !self.covered() {
            return Err(format!("抽样覆盖不足：{} < {}（加样阻断）", self.samples, MIN_SAMPLES));
        }
        if !self.diffs.is_empty() {
            let d = &self.diffs[0];
            return Err(format!("不一致定位：抽样 {} 维度 {}", d.sample, d.dim));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec83_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let out = |color: &str, lit: &str| VarOutput {
        dims: vec![
            ("color".to_string(), color.to_string()),
            ("lighting".to_string(), lit.to_string()),
        ],
    };

    // 一、抽样对拍（预期维度外等价）。
    {
        let mut c = Consistency::new();
        // SHADOWS 参数只应影响 lighting 维：color 维必须等价。
        let ok = c.sample_pair(0, "lighting", &out("r", "on"), &out("r", "off"), false);
        v.push((
            "F0483-对拍-预期维排除",
            ok && c.diffs.is_empty(),
        ));
        // color 维差异（预期外）→ 不一致 + 定位。
        let ok2 = c.sample_pair(1, "lighting", &out("r", "on"), &out("g", "on"), false);
        v.push((
            "F0483-对拍-预期外差异拦",
            !ok2 && c.diffs.len() == 1 && c.diffs[0].dim == "color",
        ));
    }

    // 二、维度定位。
    {
        let mut c = Consistency::new();
        // 三维输出中第二维差异 → 定位到该维。
        let a = VarOutput {
            dims: vec![
                ("d1".to_string(), "x".to_string()),
                ("d2".to_string(), "y".to_string()),
                ("d3".to_string(), "z".to_string()),
            ],
        };
        let b = VarOutput {
            dims: vec![
                ("d1".to_string(), "x".to_string()),
                ("d2".to_string(), "y!".to_string()),
                ("d3".to_string(), "z".to_string()),
            ],
        };
        let _ = c.sample_pair(0, "none", &a, &b, false);
        v.push((
            "F0483-定位-差异维可溯",
            c.diffs.len() == 1 && c.diffs[0].dim == "d2",
        ));
        // verdict 带定位（覆盖门槛先行——补足抽样后再下结论）。
        for i in 1..MIN_SAMPLES {
            let _ = c.sample_pair(i, "none", &a, &b, false);
        }
        let r = c.verdict();
        v.push((
            "F0483-定位-verdict带维",
            matches!(&r, Err(msg) if msg.contains("d2")),
        ));
    }

    // 三、保守判定。
    {
        let mut c = Consistency::new();
        // 等价歧义 → 保守判不一致（入账）。
        let ok = c.sample_pair(0, "x", &out("r", "on"), &out("r", "on"), true);
        v.push((
            "F0483-保守-歧义判不一致",
            !ok && c.ambiguous.len() == 1 && c.diffs[0].dim == "<ambiguous>",
        ));
    }

    // 四、覆盖门槛。
    {
        // 零抽样 → 覆盖不足阻断（加样）。
        let c = Consistency::new();
        let r = c.verdict();
        v.push((
            "F0483-门槛-不足阻断",
            matches!(&r, Err(msg) if msg.contains("覆盖不足")),
        ));
        // 边界：恰 MIN_SAMPLES 且零差异 → 过。
        let mut ok_c = Consistency::new();
        let base = out("r", "on");
        for i in 0..MIN_SAMPLES {
            ok_c.sample_pair(i, "lighting", &base, &base, false);
        }
        v.push((
            "F0483-门槛-达标放行",
            ok_c.covered() && ok_c.verdict().is_ok(),
        ));
        // 差一即阻（门槛有牙）。
        let mut short = Consistency::new();
        for i in 0..(MIN_SAMPLES - 1) {
            short.sample_pair(i, "lighting", &base, &base, false);
        }
        v.push(("F0483-门槛-差一即阻", !short.covered() && short.verdict().is_err()));
    }

    // 五、判据自检。
    {
        v.push(("F0483-判据-锚点入册", ANCHOR == "VE-F0483"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec83）。
pub fn run_vec83_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec83");
    for (name, ok) in vec83_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec83_raw_checks as run_vec83_checks;
    #[test]
    fn vec83_consistency_checks() {
        for (name, ok) in run_vec83_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
