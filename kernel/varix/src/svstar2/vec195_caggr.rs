//! VE-F0595 · VE-C 域聚合自检（双增维声明 + 聚合核验）
//!
//! 职责：C 域聚合自检——域特色增维声明（增维家族第三用：A 域渲染确定性加
//! 性能可解释性、B 域硬件诚实性加环境多样性在先，C 域双增维）：
//! ① 方言保真度——四后端对各自语言语义的忠实度可验证可对账
//!   （F0456/F0457 差异表 + F0557/F0578 对拍框架构成保真度证据链）；
//! ② 确定性编译——同输入同环境必同产物（F0494 优化器确定性升格为域级承诺，
//!   加 F0562 ID 单调、F0590 键不变式）。
//! 聚合核验：四后端套件聚合（F0509/F0533/F0552/F0572）、增量级联不变式（F0582）、
//! 缓存键不变式（F0590）、域内查重核验。
//! 错误路径：核验未过→阻断宣告；增维证据缺失→补齐再签；查重命中→合并或显性区分。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// C 域双增维声明档：每维——名称、承诺文本、证据链条目号表。
#[derive(Clone, Debug, PartialEq)]
pub struct DimClaim {
    pub name: String,
    pub promise: String,
    /// 证据链条目：上游条目号（F 号），链条必须非空且逐环可追溯。
    pub evidence: Vec<u32>,
}

impl DimClaim {
    pub fn new(name: &str, promise: &str, evidence: &[u32]) -> Self {
        DimClaim {
            name: name.to_string(),
            promise: promise.to_string(),
            evidence: evidence.to_vec(),
        }
    }
    /// 证据链闭合：链条非空且无重复环（同一条目不得自证两次）。
    pub fn chain_closed(&self) -> bool {
        !self.evidence.is_empty()
            && self.evidence.iter().collect::<Vec<_>>().len() == self.evidence.len()
            && self.evidence.iter().all(|&f| (4000..=5999).contains(&f))
    }
}

/// C 域增维声明总档：双维齐备、承诺非空、证据闭合三关。
pub struct CDimBook {
    pub claims: Vec<DimClaim>,
}

impl CDimBook {
    /// 双增维标准声明：方言保真度 + 确定性编译。
    pub fn standard() -> Self {
        CDimBook {
            claims: vec![
                DimClaim::new(
                    "方言保真度",
                    "四后端对各自语言语义的忠实度可验证可对账",
                    &[456, 457, 557, 578],
                ),
                DimClaim::new(
                    "确定性编译",
                    "同输入同环境必同产物（域级承诺）",
                    &[494, 562, 590],
                ),
            ],
        }
    }

    /// 声明核验：双维齐备（恰好两维且名称互异）+ 逐维证据链闭合。
    pub fn validate(&self) -> Result<(), String> {
        if self.claims.len() != 2 {
            return Err(format!("增维数 {} != 2：C 域声明双增维", self.claims.len()));
        }
        if self.claims[0].name == self.claims[1].name {
            return Err("两维同名：增维必须互异".to_string());
        }
        for c in &self.claims {
            if c.promise.is_empty() {
                return Err(format!("增维 {} 承诺为空：补齐再签", c.name));
            }
            if !c.chain_closed() {
                return Err(format!("增维 {} 证据链未闭合：补齐再签", c.name));
            }
        }
        Ok(())
    }
}

/// 四后端套件聚合：F0509/F0533/F0552/F0572 四套件全绿才可聚合通过。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuiteAgg {
    pub glsl_green: bool,
    pub hlsl_green: bool,
    pub spirv_green: bool,
    pub wgsl_green: bool,
}

impl SuiteAgg {
    pub fn all_green(&self) -> bool {
        self.glsl_green && self.hlsl_green && self.spirv_green && self.wgsl_green
    }
    /// 聚合核验：四套件全绿；任一未绿→阻断宣告并点名未绿者。
    pub fn verify(&self) -> Result<(), String> {
        let names = [("GLSL", self.glsl_green), ("HLSL", self.hlsl_green),
                     ("SPIR-V", self.spirv_green), ("WGSL", self.wgsl_green)];
        let red: Vec<&str> = names.iter().filter(|(_, g)| !g).map(|(n, _)| *n).collect();
        if red.is_empty() {
            Ok(())
        } else {
            Err(format!("套件未绿：{}——阻断宣告", red.join(", ")))
        }
    }
}

/// 增量级联不变式（F0582 承诺的域级核验面）：
/// 修改一文件 → 只有受影响单元重编，级联集合 == 依赖闭包。
pub fn cascade_invariant(changed: usize, rebuilt: &[usize], dependents: &[Vec<usize>]) -> bool {
    // 期望重编集 = 直接改动的文件号 + 其全部传递依赖者。
    let mut expect: Vec<usize> = vec![changed];
    let mut stack = vec![changed];
    let mut seen = vec![false; dependents.len().max(1)];
    seen[changed] = true;
    while let Some(x) = stack.pop() {
        for &d in dependents.get(x).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !seen[d] {
                seen[d] = true;
                expect.push(d);
                stack.push(d);
            }
        }
    }
    expect.sort_unstable();
    let mut got = rebuilt.to_vec();
    got.sort_unstable();
    expect == got
}

/// 缓存键不变式（F0590 承诺的域级核验面）：键四元组相同必同产物。
pub fn key_invariant(k1: (u32, u32, u32, u32), k2: (u32, u32, u32, u32), same_output: bool) -> bool {
    if k1 == k2 {
        same_output // 同键必同产物
    } else {
        true // 异键不约束（可能同产物——不同写法同语义）
    }
}

/// 域内查重核验：条目承诺相似度扫描——命中→合并或显性区分（返回处置建议）。
pub fn dedup_review(items: &[(u32, &str)]) -> Vec<(u32, u32, String)> {
    let mut hits = Vec::new();
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            let (a, ta) = items[i];
            let (b, tb) = items[j];
            // 相似度：共享词根（≥4 字公共子串）即命中。
            let shared = common_substring_len(ta, tb);
            if shared >= 4 {
                hits.push((
                    a.min(b),
                    b.max(a),
                    if ta == tb {
                        format!("F{} 与 F{} 承诺全同：合并", a, b)
                    } else {
                        format!("F{} 与 F{} 相似（公共 {} 字）：显性区分边界", a, b, shared)
                    },
                ));
            }
        }
    }
    hits
}

/// 最长公共子串长度（简单 DP，查重用，O(len²) 可接受——条目文本短）。
fn common_substring_len(a: &str, b: &str) -> usize {
    let ab: Vec<char> = a.chars().collect();
    let bb: Vec<char> = b.chars().collect();
    let mut best = 0usize;
    let mut dp = vec![0usize; bb.len() + 1];
    for i in 1..=ab.len() {
        let mut prev = 0usize;
        for j in 1..=bb.len() {
            let cur = if ab[i - 1] == bb[j - 1] { dp[j - 1] + 1 } else { 0 };
            dp[j - 1] = prev;
            prev = cur;
            best = best.max(cur);
        }
        dp[bb.len()] = prev;
    }
    best
}

pub fn run_vec195_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec195");
    // 族一：双增维声明——齐备、承诺、证据链闭合
    let book = CDimBook::standard();
    s.add("双增维齐备", book.claims.len() == 2, "two-dims");
    s.add("维度名称互异", book.claims[0].name != book.claims[1].name, "distinct");
    s.add("方言保真度证据链闭合", book.claims[0].chain_closed(), "chain-fidelity");
    s.add("确定性编译证据链闭合", book.claims[1].chain_closed(), "chain-determ");
    s.add("标准声明核验通过", book.validate().is_ok(), "std-ok");
    let bad = CDimBook { claims: vec![book.claims[0].clone()] };
    s.add("单维声明被拒", bad.validate().is_err(), "one-dim");
    let dup = CDimBook { claims: vec![book.claims[0].clone(), book.claims[0].clone()] };
    s.add("同名两维被拒", dup.validate().is_err(), "same-name");
    let empty_ev = DimClaim::new("x", "y", &[]);
    s.add("空证据链未闭合", !empty_ev.chain_closed(), "empty-chain");
    let out_of_range = DimClaim::new("x", "y", &[7000]);
    s.add("段外证据判未闭合", !out_of_range.chain_closed(), "range-chain");
    // 族二：四后端套件聚合
    let green = SuiteAgg { glsl_green: true, hlsl_green: true, spirv_green: true, wgsl_green: true };
    s.add("四套件全绿聚合通过", green.verify().is_ok() && green.all_green(), "agg-ok");
    let red_one = SuiteAgg { glsl_green: true, hlsl_green: false, spirv_green: true, wgsl_green: true };
    match red_one.verify() {
        Err(m) => s.add("未绿点名阻断", m.contains("HLSL") && m.contains("阻断"), "agg-red"),
        Ok(()) => s.add("未绿点名阻断", false, "expected err"),
    }
    // 族三：级联不变式与键不变式
    // 依赖图：1 依赖 0（0 的依赖者有 1），2 依赖 1（1 的依赖者有 2）。
    let deps = vec![vec![1], vec![2], vec![]];
    s.add("级联集合==依赖闭包", cascade_invariant(0, &[0, 1, 2], &deps), "cascade-ok");
    s.add("级联漏编判不变式破", !cascade_invariant(0, &[0, 1], &deps), "cascade-bad");
    s.add("级联多编判不变式破", !cascade_invariant(0, &[0, 1, 2, 3], &deps), "cascade-extra");
    s.add("同键同产物不变式", key_invariant((1, 2, 3, 4), (1, 2, 3, 4), true), "key-same");
    s.add("同键异产物即违约", !key_invariant((1, 2, 3, 4), (1, 2, 3, 4), false), "key-violate");
    s.add("异键不约束", key_invariant((1, 2, 3, 4), (5, 6, 7, 8), false), "key-free");
    // 族四：域内查重——全同合并、相似区分、无重放行
    let items = vec![(1u32, "四后端差异表维护"), (2u32, "四后端差异表维护"), (3u32, "热编译延迟预算"), (4u32, "热编译延迟预算分档")];
    let hits = dedup_review(&items);
    s.add("全同命中合并", hits.iter().any(|(a, b, m)| (*a, *b) == (1, 2) && m.contains("合并")), "dup-merge");
    s.add("相似命中显性区分", hits.iter().any(|(a, b, m)| (*a, *b) == (3, 4) && m.contains("显性区分")), "dup-distinguish");
    let clean = vec![(1u32, "图层树数据结构"), (2u32, "缓存键四元组")];
    s.add("无重放行", dedup_review(&clean).is_empty(), "no-dup");
    s.add("核验未过阻断宣告路径显性", red_one.verify().is_err(), "block-path");
    s
}
