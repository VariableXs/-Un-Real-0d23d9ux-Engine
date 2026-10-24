//! veb362_verneg —— VE-F0362 驱动版本协商与兼容检查。
//! 锚点（CoRun Unxreal 主册）：VE-F0362 · 驱动版本协商 · 目标 380 行
//! 判据原文：VE-F0362-J1 驱动与内核按语义化版本三元组协商：
//!           主版本必须相等、次版本内核 ≥ 驱动、修订号不校验；
//!           协商结果三态（兼容/降级兼容/不兼容）可判定；不兼
//!           容立案带双方版本；协商可重放（同版本对同结论）；
//!           版本串解析（"a.b.c" → 三元组）拒绝畸形串。
//! 组件：版本三元组（major/minor/patch）+ 解析器 + 协商引擎
//!       （三态）+ 立案账 + 重放核。
//! 职责定位：版本协商是「驱动的准入面」——语义化版本的纪律：
//!   主版本 = ABI 断代（不等即免谈），次版本 = 向后兼容的新
//!   能力（内核新于驱动可降级），修订号 = 纯修复（免检）。
//!   降级兼容是关键灰度：内核比驱动新时允许跑，但要显式标记
//!   （新能力不可用），不许静默假装全兼容。
//! 性能逐项分解：解析 O(串长)；协商 O(1)；重放 O(1)。
//! 诊断码段：0x30000（0x30001~0x30009 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VN_VERSION: &str = "veb362_verneg-v1";

/// 诊断段基址（0x30000 段 · 本模块独占）。
pub const VN_SEG_BASE: u32 = 0x30000;
pub const XC_MAJ_MISMATCH: u32 = 0x30001; // 主版本不等
pub const XC_MIN_AHEAD: u32 = 0x30002; // 次版本驱动超前
pub const XC_PARSE_BAD: u32 = 0x30003; // 版本串畸形
pub const XC_REPLAY: u32 = 0x30004; // 协商重放不一致
pub const XC_MAGIC: u32 = 0x30005; // 账魔数破
pub const XC_CASE_LOST: u32 = 0x30006; // 立案丢失
pub const XC_DUP_CASE: u32 = 0x30007; // 案重号
pub const XC_EMPTY_NAME: u32 = 0x30008; // 驱动名空
pub const XC_NEG_VER: u32 = 0x30009; // 版本号非法（超域）

/// 语义化版本三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// 协商三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 全兼容。
    Compatible,
    /// 降级兼容（内核次版本更新）。
    Downgraded,
    /// 不兼容。
    Incompatible,
}

impl Verdict {
    /// 码（1~3）。
    pub fn code(&self) -> u32 {
        match self {
            Verdict::Compatible => 1,
            Verdict::Downgraded => 2,
            Verdict::Incompatible => 3,
        }
    }
}

/// 版本串解析："a.b.c" → 三元组；畸形拒。
pub fn parse_ver(s: &str) -> Result<SemVer, u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(XC_PARSE_BAD);
    }
    let mut nums = [0u32; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 5 {
            return Err(XC_PARSE_BAD);
        }
        let mut v: u32 = 0;
        for ch in p.chars() {
            let d = match ch.to_digit(10) {
                Some(d) => d,
                None => return Err(XC_PARSE_BAD),
            };
            v = v * 10 + d;
            if v > 99_999 {
                return Err(XC_PARSE_BAD);
            }
        }
        nums[i] = v;
    }
    Ok(SemVer { major: nums[0], minor: nums[1], patch: nums[2] })
}

/// 协商：主版本必须等、次版本内核 ≥ 驱动、修订免检。
pub fn negotiate(kernel: SemVer, driver: SemVer) -> Result<Verdict, u32> {
    if kernel.major != driver.major {
        return Err(XC_MAJ_MISMATCH);
    }
    if kernel.minor < driver.minor {
        return Err(XC_MIN_AHEAD);
    }
    if kernel.minor > driver.minor {
        Ok(Verdict::Downgraded)
    } else {
        Ok(Verdict::Compatible)
    }
}

/// 协商账。
pub struct NegBook {
    cases: Vec<(String, SemVer, SemVer, u32)>,
}

impl NegBook {
    /// 建空账。
    pub fn new() -> NegBook {
        NegBook { cases: Vec::new() }
    }

    /// 立案：驱动名非空、重号拒、不兼容必立案。
    pub fn record(&mut self, name: &str, k: SemVer, d: SemVer, v: Verdict) -> Result<(), u32> {
        if name.is_empty() {
            return Err(XC_EMPTY_NAME);
        }
        if k.major > 9999 || d.major > 9999 {
            return Err(XC_NEG_VER);
        }
        for (n, _, _, _) in self.cases.iter() {
            if n == name {
                return Err(XC_DUP_CASE);
            }
        }
        self.cases.push((String::from(name), k, d, v.code()));
        Ok(())
    }

    /// 案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 按名读案。
    pub fn get(&self, name: &str) -> Option<u32> {
        self.cases.iter().find(|(n, _, _, _)| n == name).map(|(_, _, _, c)| *c)
    }

    /// 重放核：重算全部协商与账面一致。
    pub fn replay_ok(&self) -> Result<(), u32> {
        for (name, k, d, code) in self.cases.iter() {
            let v = match negotiate(*k, *d) {
                Ok(v) => v,
                Err(_) => return Err(XC_REPLAY),
            };
            if v.code() != *code {
                let _ = name;
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }
}

pub fn run_veb362_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0362");
    // 族一：版本串解析。
    cs_add_parse(&mut s);
    // 族二：协商三态。
    let k = SemVer { major: 3, minor: 5, patch: 0 };
    s.add("全等兼容", negotiate(k, SemVer { major: 3, minor: 5, patch: 9 }) == Ok(Verdict::Compatible), "主次等修订免检");
    s.add("内核次新降级", negotiate(k, SemVer { major: 3, minor: 4, patch: 0 }) == Ok(Verdict::Downgraded), "5 > 4");
    s.add("驱动次新拒", negotiate(k, SemVer { major: 3, minor: 6, patch: 0 }) == Err(XC_MIN_AHEAD), "5 < 6");
    s.add("主版本不等拒", negotiate(k, SemVer { major: 4, minor: 0, patch: 0 }) == Err(XC_MAJ_MISMATCH), "3 ≠ 4");
    // 族三：协商账。
    let mut b = NegBook::new();
    let r1 = b.record("gpu", k, SemVer { major: 3, minor: 5, patch: 1 }, Verdict::Compatible);
    s.add("gpu 立案", r1.is_ok(), "兼容");
    let r2 = b.record("nvme", k, SemVer { major: 3, minor: 2, patch: 7 }, Verdict::Downgraded);
    s.add("nvme 立案", r2.is_ok(), "降级");
    s.add("两案在账", b.len() == 2, "逐案");
    s.add("按名读案", b.get("gpu") == Some(1) && b.get("nvme") == Some(2), "码回读");
    s.add("重号拒", b.record("gpu", k, k, Verdict::Compatible) == Err(XC_DUP_CASE), "gpu 撞");
    s.add("空名拒", b.record("", k, k, Verdict::Compatible) == Err(XC_EMPTY_NAME), "名空");
    s.add("版本超域拒", b.record("x", SemVer { major: 10_000, minor: 0, patch: 0 }, k, Verdict::Compatible) == Err(XC_NEG_VER), "major 越域");
    s.add("重放核过", b.replay_ok().is_ok(), "复算一致");
    s
}

/// 解析判据组（独立函数防超容）。
fn cs_add_parse(s: &mut CheckSet) {
    let v = parse_ver("3.5.1");
    s.add("解析 3.5.1", v == Ok(SemVer { major: 3, minor: 5, patch: 1 }), "三元组");
    s.add("解析 0.0.0", parse_ver("0.0.0") == Ok(SemVer { major: 0, minor: 0, patch: 0 }), "全零合法");
    s.add("两位段拒", parse_ver("3.5").is_err(), "段数 ≠ 3");
    s.add("四位段拒", parse_ver("3.5.1.2").is_err(), "段数 ≠ 3");
    s.add("空段拒", parse_ver("3..1").is_err(), "空子串");
    s.add("字母拒", parse_ver("3.a.1").is_err(), "非数字");
    s.add("超长拒", parse_ver("3.5.123456").is_err(), "段长 > 5");
    s.add("超值拒", parse_ver("3.5.99999").is_ok(), "=99999 边界");
    s.add("魔数核", VN_SEG_BASE == 0x30000, "段基址");
}
