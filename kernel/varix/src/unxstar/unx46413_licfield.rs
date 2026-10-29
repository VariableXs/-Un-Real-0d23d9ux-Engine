//! UNX-F46413 工具链组件许可字段账（gcc GPL/clang LLVM-Apache/rust MIT-Apache/git GPLv2/CMake BSD 五件逐件核）（0x400C0 段）
//!
//! 锚点：UNX-F46413-J1 五件许可字段完整率 100% 预核（AI-95 会签前置）；
//! 缺字段拒绝入镜像构建清单注入 ×3 生效。
//! 域：CoRun·UNX-S1-B01（工具链域·许可合规面）。
//!
//! 结构：五件许可账（组件 × 许可证 × 条款要点 × 分发面四字段）+ 完整率
//! 预核（四字段逐件齐才可入镜像清单）+ 镜像构建清单准入闸（缺字段拒绝
//! 显性化）+ 注入核（缺字段/未知组件/空条款/重复登记/清单封后改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400C0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400C0;
pub const LC_FIELD_MISS: u32 = SEG_BASE + 0x01; // 许可字段缺失
pub const LC_COMP_OOB: u32 = SEG_BASE + 0x02; // 组件号越闭集
pub const LC_TERMS_EMPTY: u32 = SEG_BASE + 0x03; // 条款要点为空
pub const LC_DUP: u32 = SEG_BASE + 0x04; // 重复登记
pub const LC_MANIFEST_SEAL: u32 = SEG_BASE + 0x05; // 清单封后改写

/// 五件闭集。
pub const LC_GCC: usize = 0;
pub const LC_CLANG: usize = 1;
pub const LC_RUSTC: usize = 2;
pub const LC_GIT: usize = 3;
pub const LC_CMAKE: usize = 4;
pub const LC_COUNT: usize = 5;

pub const LC_NAMES: [&str; LC_COUNT] = ["gcc", "clang", "rustc", "git", "cmake"];

/// 五件钉版许可证（与 F46401 钉版账同源版本序）。
pub const PIN_LICENSES: [&str; LC_COUNT] = ["GPL-3.0-or-later", "Apache-2.0-WITH-LLVM-exception", "MIT-Apache", "GPLv2", "BSD-3-Clause"];

/// 许可账目：四字段（组件/许可证/条款要点/分发面）。
#[derive(Clone)]
pub struct LicEntry {
    pub comp: usize,
    pub license: String,
    pub terms: String,
    pub distribution: String,
}

impl LicEntry {
    /// 四字段齐核。
    pub fn complete(&self) -> bool {
        !self.license.is_empty() && !self.terms.is_empty() && !self.distribution.is_empty()
    }

    /// 账目指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for b in format!("{}|{}|{}|{}", LC_NAMES[self.comp], self.license, self.terms, self.distribution).as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 许可账（逐件登记，同件唯一）。
pub struct LicBook {
    pub entries: Vec<LicEntry>,
    pub sealed: bool,
}

impl LicBook {
    pub fn new() -> Self {
        LicBook { entries: Vec::new(), sealed: false }
    }

    /// 登记（越闭集/空条款/重复/封账全拒）。
    pub fn put(&mut self, comp: usize, license: &str, terms: &str, distribution: &str) -> Result<(), u32> {
        if comp >= LC_COUNT {
            return Err(LC_COMP_OOB);
        }
        if license.is_empty() || distribution.is_empty() {
            return Err(LC_FIELD_MISS);
        }
        if terms.is_empty() {
            return Err(LC_TERMS_EMPTY);
        }
        if self.sealed {
            return Err(LC_MANIFEST_SEAL);
        }
        if self.entries.iter().any(|e| e.comp == comp) {
            return Err(LC_DUP);
        }
        self.entries.push(LicEntry {
            comp,
            license: license.to_string(),
            terms: terms.to_string(),
            distribution: distribution.to_string(),
        });
        Ok(())
    }

    /// 五件齐核。
    pub fn full(&self) -> bool {
        self.entries.len() == LC_COUNT && self.entries.iter().all(|e| e.complete())
    }

    /// 镜像构建清单准入：全件字段齐才放行；缺件清单显性返回。
    pub fn manifest_admit(&self) -> Result<Vec<String>, u32> {
        if self.sealed {
            return Err(LC_MANIFEST_SEAL);
        }
        let mut missing: Vec<String> = Vec::new();
        for c in 0..LC_COUNT {
            match self.entries.iter().find(|e| e.comp == c) {
                None => missing.push(LC_NAMES[c].to_string()),
                Some(e) => {
                    if !e.complete() {
                        missing.push(LC_NAMES[c].to_string());
                    }
                }
            }
        }
        if missing.is_empty() {
            let mut list = Vec::new();
            for e in self.entries.iter() {
                list.push(format!("{}|{}", LC_NAMES[e.comp], e.license));
            }
            Ok(list)
        } else {
            Err(LC_FIELD_MISS)
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账指纹（逐件复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            h ^= e.fingerprint();
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 标准夹具：五件全登记（钉版许可证 + 条款要点 + 分发面）。
pub fn standard_book() -> LicBook {
    let mut b = LicBook::new();
    let terms = [
        "运行时库随二进制分发例外条款适用",
        "LLVM 例外条款适用（可执行文件不再传染）",
        "MIT 与 Apache 双许可任选其一",
        "GPLv2-only 逐字条款适用",
        "BSD 三条款保留版权声明",
    ];
    let dist = ["随镜像分发", "随镜像分发", "随镜像分发", "随镜像分发", "随镜像分发"];
    for c in 0..LC_COUNT {
        b.put(c, PIN_LICENSES[c], terms[c], dist[c]).expect("fixture lic");
    }
    b
}

/// 注入核：6 注入必抓（缺字段/越闭集/空条款/重复/清单封后改写/缺件准入拒绝）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺字段必抓（license 空）
    let mut b = LicBook::new();
    if b.put(LC_GCC, "", "条款", "分发") == Err(LC_FIELD_MISS) {
        caught += 1;
    }
    // 2) 越闭集必抓
    if b.put(9, "MIT", "条款", "分发") == Err(LC_COMP_OOB) {
        caught += 1;
    }
    // 3) 空条款必抓
    if b.put(LC_GCC, "GPL", "", "分发") == Err(LC_TERMS_EMPTY) {
        caught += 1;
    }
    // 4) 重复登记必抓
    b.put(LC_GCC, "GPL", "条款", "分发").expect("first");
    if b.put(LC_GCC, "GPLv2", "条款2", "分发") == Err(LC_DUP) {
        caught += 1;
    }
    // 5) 缺件准入拒绝（只登记 2 件 → manifest 拒绝）
    let part = {
        let mut pb = LicBook::new();
        pb.put(LC_GCC, "GPL", "条款", "分发").expect("p1");
        pb.put(LC_CLANG, "Apache", "条款", "分发").expect("p2");
        pb
    };
    if part.manifest_admit() == Err(LC_FIELD_MISS) {
        caught += 1;
    }
    // 6) 清单封后改写必抓
    let mut full = standard_book();
    full.seal();
    if full.put(LC_GIT, "GPLv2", "条款", "分发") == Err(LC_MANIFEST_SEAL) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46413_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46413");
    // J1-1 五件许可字段完整率 100%
    let b = standard_book();
    let ok = b.full();
    s.add("46413-J1-五件字段完整", ok, if ok { "" } else { "字段不齐" });
    // J1-2 镜像清单准入：五件全放行且许可证与钉版账一致
    let admitted = b.manifest_admit().expect("admit");
    let ok = admitted.len() == 5
        && admitted.iter().zip(PIN_LICENSES.iter()).all(|(row, lic)| row.ends_with(lic));
    s.add("46413-J1-镜像清单准入", ok, if ok { "" } else { "清单准入漂移" });
    // J1-3 许可证与钉版逐件一致
    let ok = b.entries.iter().all(|e| PIN_LICENSES[e.comp] == e.license);
    s.add("46413-J1-钉版一致", ok, if ok { "" } else { "许可证漂移" });
    // J1-4 账指纹稳定
    let ok = b.fingerprint() == standard_book().fingerprint();
    s.add("46413-J1-账指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46413-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46413_all_checks_green() {
        let set = run_unx46413_checks();
        assert!(set.all_passed(), "UNX-F46413 checks must be green: {}", set.red_items_text());
    }
}
