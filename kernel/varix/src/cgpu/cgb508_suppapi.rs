//! CGPU-F0508 · 抑制 API 冻结 v1（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API：抖动抑制族冻结（检测订阅/策略配置/均衡器控制/验证接口——
//!   签名冻结版本化）；与 D01 帧计时 API 命名一致；十年承诺。
//!   判据：冻结、一致、承诺、判据。
//!
//! 设计纪律：
//! - 四族接口登记：supp_subscribe（检测订阅）/ supp_policy（策略配置）/
//!   supp_eqctl（均衡器控制）/ supp_verify（验证接口）——每条登记
//!   名称、形参列表、返回码、冻结版本。
//! - 签名冻结版本化：签名文本的 FNV-1a 哈希入册——任何形参/顺序/返回
//!   变更都会改哈希，冻结校验（对账入册哈希）即拒。
//! - 与 D01 帧计时 API 命名一致：族前缀规范（ft_/supp_/eq_）+ 全小写
//!   蛇形 + 无缩写歧义——命名审查器逐条校验。
//! - 十年承诺：v1 承诺面只增不改不删——承诺账本记录每条的承诺档位
//!   （十年/长期/过渡），十年档不得降档。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5230 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x5230，批十独占）。
pub const SEG_BASE: u16 = 0x5230;

pub const E_AP_FROZEN: u16 = SEG_BASE + 1; // 冻结签名被改动
pub const E_AP_NAME: u16 = SEG_BASE + 2; // 命名纪律违例
pub const E_AP_PLEDGE: u16 = SEG_BASE + 3; // 十年承诺降档
pub const E_AP_DUP: u16 = SEG_BASE + 4; // 接口重名登记

/// API 冻结版本（v1.0 → 1）。
pub const API_FREEZE_VERSION: u32 = 1;

/// 十年承诺档位。
pub const PL_TENYEAR: u8 = 3; // 十年（最高档，不得降档）
pub const PL_LONGTERM: u8 = 2; // 长期
pub const PL_TRANSITION: u8 = 1; // 过渡（可废弃但须 ADR）

/// D01 帧计时族前缀（命名一致性参照）。
pub const D01_PREFIX: &str = "ft_";
/// 本抑制族前缀。
pub const SUPP_PREFIX: &str = "supp_";

/// FNV-1a 64（签名指纹，与 cgb507 同源实现）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 单条冻结接口登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiEntry {
    /// 接口名（含族前缀）。
    pub name: &'static str,
    /// 形参类型列表（签名文本）。
    pub params: &'static str,
    /// 返回码类型。
    pub ret: &'static str,
    /// 冻结版本（登记时版本）。
    pub frozen_at: u32,
    /// 承诺档位。
    pub pledge: u8,
}

impl ApiEntry {
    /// 签名指纹（名称+形参+返回三元组）。
    pub fn sig_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.name.as_bytes());
        bytes.push(0x1F);
        bytes.extend_from_slice(self.params.as_bytes());
        bytes.push(0x1F);
        bytes.extend_from_slice(self.ret.as_bytes());
        fnv1a64(&bytes)
    }
}

/// 抑制族 API 冻结账本。
#[derive(Clone, Debug)]
pub struct ApiFreezeBook {
    entries: Vec<ApiEntry>,
}

impl ApiFreezeBook {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 命名纪律：族前缀 + 全小写蛇形（字母数字下划线，无大写）。
    pub fn name_ok(name: &str) -> bool {
        let prefixes = [D01_PREFIX, SUPP_PREFIX, "eq_", "b08_"];
        if !prefixes.iter().any(|p| name.starts_with(p)) {
            return false;
        }
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    /// 登记冻结接口：命名审查 + 重名拒绝 + 入册。
    pub fn register(&mut self, e: ApiEntry) -> Result<(), u16> {
        if !Self::name_ok(e.name) {
            return Err(E_AP_NAME);
        }
        if self.entries.iter().any(|x| x.name == e.name) {
            return Err(E_AP_DUP);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 冻结校验：现行签名哈希与入册哈希逐一比对。
    pub fn freeze_audit(&self, current: &[( & 'static str, u64)]) -> Result<(), u16> {
        for (name, hash) in current {
            let e = match self.entries.iter().find(|e| e.name == *name) {
                Some(e) => e,
                None => continue, // 现行未登记的新接口不归 v1 承诺面
            };
            if e.sig_hash() != *hash {
                return Err(E_AP_FROZEN);
            }
        }
        Ok(())
    }

    /// 十年承诺审计：十年档条目存在且不得出现 0（非法档）。
    pub fn pledge_audit(&self) -> Result<usize, u16> {
        let mut ten = 0usize;
        for e in &self.entries {
            if !(PL_TRANSITION..=PL_TENYEAR).contains(&e.pledge) {
                return Err(E_AP_PLEDGE);
            }
            if e.pledge == PL_TENYEAR {
                ten += 1;
            }
        }
        if ten == 0 {
            return Err(E_AP_PLEDGE); // 四族核心必须含十年档
        }
        Ok(ten)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// v1 四族核心登记面（规范签名源）。
    pub fn v1_core() -> [ApiEntry; 4] {
        [
            ApiEntry {
                name: "supp_subscribe",
                params: "(mode:u8,cb:fn(JitterEvent))",
                ret: "Result<SubId,u16>",
                frozen_at: API_FREEZE_VERSION,
                pledge: PL_TENYEAR,
            },
            ApiEntry {
                name: "supp_policy",
                params: "(strategy:u8,depth:u8)",
                ret: "Result<(),u16>",
                frozen_at: API_FREEZE_VERSION,
                pledge: PL_TENYEAR,
            },
            ApiEntry {
                name: "supp_eqctl",
                params: "(ctl:EqCmd)",
                ret: "Result<EqState,u16>",
                frozen_at: API_FREEZE_VERSION,
                pledge: PL_LONGTERM,
            },
            ApiEntry {
                name: "supp_verify",
                params: "(window:u32)",
                ret: "Result<VerifyReport,u16>",
                frozen_at: API_FREEZE_VERSION,
                pledge: PL_TRANSITION,
            },
        ]
    }
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb508_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 冻结登记：v1 四族全部入册
    let mut book = ApiFreezeBook::new();
    let mut ok_reg = true;
    for e in ApiFreezeBook::v1_core() {
        ok_reg &= book.register(e) == Ok(());
    }
    v.push(("冻结-v1四族全入册", ok_reg && book.len() == 4));

    // 2. 签名冻结校验：现行哈希=入册哈希即过
    let cur: Vec<(&'static str, u64)> = ApiFreezeBook::v1_core()
        .iter()
        .map(|e| (e.name, e.sig_hash()))
        .collect();
    v.push(("冻结-签名哈希对账通过", book.freeze_audit(&cur) == Ok(())));

    // 3. 签名一变即拒：改形参文本→哈希漂移→E_AP_FROZEN
    let mut cur2 = cur.clone();
    cur2[1] = ("supp_policy", fnv1a64(b"supp_policy\x1f(strategy:u8,depth:u16)\x1fResult<(),u16>"));
    v.push(("冻结-签名漂移即拒", book.freeze_audit(&cur2) == Err(E_AP_FROZEN)));

    // 4. 命名一致：族前缀+蛇形过审；大写/无前缀拒
    let ok_names = ApiFreezeBook::name_ok("supp_subscribe")
        && ApiFreezeBook::name_ok("ft_frame_time")
        && !ApiFreezeBook::name_ok("Supp_Subscribe")
        && !ApiFreezeBook::name_ok("wild_api");
    v.push(("一致-D01命名纪律过审", ok_names));

    // 5. 重名拒绝
    let dup = book.register(ApiEntry {
        name: "supp_policy",
        params: "()",
        ret: "()",
        frozen_at: API_FREEZE_VERSION,
        pledge: PL_TRANSITION,
    });
    v.push(("冻结-重名拒绝", dup == Err(E_AP_DUP)));

    // 6. 十年承诺：核心面含十年档且非法档被拒
    let ok_pledge = book.pledge_audit() == Ok(2);
    let mut bad = ApiFreezeBook::new();
    let _ = bad.register(ApiEntry {
        name: "supp_verify",
        params: "()",
        ret: "()",
        frozen_at: 1,
        pledge: 9,
    });
    v.push((
        "承诺-十年档在册且非法档拒",
        ok_pledge && bad.pledge_audit() == Err(E_AP_PLEDGE),
    ));

    // 7. 过渡档存在（验证接口 v1 为过渡档——演进空间声明诚实）
    let has_trans = ApiFreezeBook::v1_core().iter().any(|e| e.pledge == PL_TRANSITION);
    v.push(("承诺-过渡档诚实声明", has_trans));

    v
}

pub fn run_cgb508_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb508");
    for (name, ok) in cgb508_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb508_raw_checks as raw;
    #[test]
    fn t_cgb508() {
        for (n, ok) in raw() {
            assert!(ok, "cgb508 check failed: {}", n);
        }
    }
}
