//! VE-F4017 · 国际化 API 冻结
//!
//! i18n API 冻结：API 四族（Locale / 格式化 / 方向 / 复数选择）签名表
//! ——冻结；消费双签；生成单源复用；兼容承诺（十年）；API 覆盖断言
//! （十项映射 API 全覆盖——覆盖硬门）。
//! 数据结构：四族签名；消费双签；生成复用；覆盖断言。
//! 错误路径：回执未达→不冻结；手写文档→迁移；断兼容→ADR；覆盖缺→补。
//! 性能：冻结 O(签名)；双签 O(消费方)；生成 O(接口)；覆盖 O(断言)。
//! 跨批对接：F3350/F3950 模式复用声明；消费方对端；F3817 冻结协同。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6640）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6640;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T23ACode(pub u16);

impl T23ACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const RECEIPT_LATE: Self = Self(SEG_BASE + 1);
    pub const HANDWRITE: Self = Self(SEG_BASE + 2);
    pub const COVER_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RECEIPT_LATE => "回执未达→不冻结",
            Self::HANDWRITE => "手写文档→迁移",
            Self::COVER_MISS => "覆盖缺→补",
            _ => "未知",
        }
    }
}

// ── API 四族 ─────────────────────────────────────────────────────────

/// 四族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFamily {
    /// Locale 解析。
    Locale,
    /// 格式化。
    Format,
    /// 方向。
    Direction,
    /// 复数选择。
    Plural,
}

pub const API_FAMILIES: [ApiFamily; 4] = [
    ApiFamily::Locale, ApiFamily::Format, ApiFamily::Direction, ApiFamily::Plural,
];

impl ApiFamily {
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::Locale => "Locale",
            ApiFamily::Format => "格式化",
            ApiFamily::Direction => "方向",
            ApiFamily::Plural => "复数选择",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            ApiFamily::Locale => 0,
            ApiFamily::Format => 1,
            ApiFamily::Direction => 2,
            ApiFamily::Plural => 3,
        }
    }
}

/// 签名条目。
#[derive(Debug, Clone)]
pub struct Signature {
    /// 接口名。
    pub api: String,
    /// 所属族。
    pub family: ApiFamily,
    /// 签名串（冻结指纹源）。
    pub sig: String,
    /// 手写文档标记（复用红线：须迁移生成）。
    pub handwritten: bool,
}

/// 签名表（可冻结）。
#[derive(Debug, Clone)]
pub struct SigBook {
    pub sigs: Vec<Signature>,
    pub frozen: bool,
}

impl SigBook {
    pub fn new() -> Self {
        Self { sigs: Vec::new(), frozen: false }
    }

    pub fn put(&mut self, api: &str, family: ApiFamily, sig: &str, handwritten: bool) -> bool {
        if self.frozen || self.sigs.iter().any(|s| s.api == api) {
            return false;
        }
        self.sigs.push(Signature {
            api: String::from(api),
            family,
            sig: String::from(sig),
            handwritten,
        });
        true    }

    /// 手写检出（迁移对象）。
    pub fn handwritten_list(&self) -> Vec<String> {
        self.sigs.iter().filter(|s| s.handwritten).map(|s| s.api.clone()).collect()
    }

    /// 冻结哈希（全签名 FNV）。
    pub fn freeze_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for s in self.sigs.iter() {
            for b in s.sig.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

// ── 消费双签 ─────────────────────────────────────────────────────────

/// 消费方回执（双签：接口提供方 + 消费方，禁同人）。
#[derive(Debug, Clone)]
pub struct ConsumeSign {
    pub provider: String,
    pub consumer: String,
}

/// 双签册。
#[derive(Debug, Clone)]
pub struct ConsumeBook {
    pub signs: Vec<(&'static str, ConsumeSign)>,
}

impl ConsumeBook {
    pub fn new() -> Self {
        Self { signs: Vec::new() }
    }

    /// 登记双签（禁同人）。
    pub fn sign(&mut self, api: &'static str, provider: &str, consumer: &str) -> bool {
        if provider.is_empty() || consumer.is_empty() || provider == consumer {
            return false;
        }
        if self.signs.iter().any(|(a, _)| *a == api) {
            return false;
        }
        self.signs.push((api, ConsumeSign {
            provider: String::from(provider),
            consumer: String::from(consumer),
        }));
        true
    }

    /// 指定 API 回执已达。
    pub fn received(&self, api: &str) -> bool {
        self.signs.iter().any(|(a, _)| *a == api)
    }

    /// 未达回执清单（不冻结依据）。
    pub fn pending(&self, all: &[&str]) -> Vec<String> {
        all.iter().filter(|a| !self.received(a)).map(|a| String::from(*a)).collect()
    }
}

// ── 覆盖断言（十项映射全覆盖）───────────────────────────────────────

pub const TEN_MAPPINGS: [&str; 10] = [
    "Locale", "方向", "排版", "字体", "格式",
    "复数", "密度", "输入法", "测试", "调试",
];

/// 覆盖断言：每映射至少一个 API。
#[derive(Debug, Clone)]
pub struct CoverageAssert {
    pub covered: Vec<&'static str>,
}

impl CoverageAssert {
    pub fn new() -> Self {
        Self { covered: Vec::new() }
    }

    pub fn cover(&mut self, mapping: &'static str) -> bool {
        if !TEN_MAPPINGS.contains(&mapping) || self.covered.contains(&mapping) {
            return false;
        }
        self.covered.push(mapping);
        true
    }

    pub fn gaps(&self) -> Vec<&'static str> {
        TEN_MAPPINGS.iter().filter(|m| !self.covered.contains(m)).map(|m| *m).collect()
    }

    pub fn full(&self) -> bool {
        self.gaps().is_empty()
    }
}

// ── 冻结闸 ───────────────────────────────────────────────────────────

/// 冻结：手写清零 + 回执全达 + 覆盖满 → 冻结。
pub fn freeze_gate(sigs: &SigBook, cons: &ConsumeBook, cov: &CoverageAssert) -> T23ACode {
    if !sigs.handwritten_list().is_empty() {
        return T23ACode::HANDWRITE;
    }
    let names: Vec<&str> = sigs.sigs.iter().map(|s| s.api.as_str()).collect();
    if !cons.pending(&names).is_empty() {
        return T23ACode::RECEIPT_LATE;
    }
    if !cov.full() {
        return T23ACode::COVER_MISS;
    }
    T23ACode::OK
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves257_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves257_s11apifreeze");

    // C4017-01 诊断码段不越窗。
    chk(&mut set, "C4017-01",
        T23ACode::COVER_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x6640, "err");

    // C4017-02 四族齐备。
    chk(&mut set, "C4017-02",
        API_FAMILIES.len() == 4
            && API_FAMILIES[0].name() == "Locale"
            && API_FAMILIES[3].name() == "复数选择", "err");

    // C4017-03 签名登记 + 重名拒。
    let mut s = SigBook::new();
    chk(&mut set, "C4017-03",
        s.put("locale_parse", ApiFamily::Locale, "fn(&str)->Locale", false)
            && !s.put("locale_parse", ApiFamily::Locale, "dup", false), "err");

    // C4017-04 手写检出。
    s.put("fmt_date", ApiFamily::Format, "fn(date)->str", true);
    chk(&mut set, "C4017-04", s.handwritten_list() == vec![String::from("fmt_date")], "err");

    // C4017-05 双签：同人拒。
    let mut c = ConsumeBook::new();
    chk(&mut set, "C4017-05", !c.sign("locale_parse", "t域", "t域"), "err");

    // C4017-06 双签异人登记 + 回执达。
    chk(&mut set, "C4017-06",
        c.sign("locale_parse", "t域", "n域") && c.received("locale_parse"), "err");

    // C4017-07 回执未达 → 不冻结。
    let names = vec!["locale_parse", "fmt_date"];
    chk(&mut set, "C4017-07", c.pending(&names).iter().map(|s| s.as_str()).collect::<Vec<&str>>() == vec!["fmt_date"], "err");

    // C4017-08 冻结闸：手写优先拦截。
    let cov0 = CoverageAssert::new();
    chk(&mut set, "C4017-08", freeze_gate(&s, &c, &cov0) == T23ACode::HANDWRITE, "err");

    // C4017-09 手写迁移（改为生成）后 → 回执拦。
    if let Some(x) = s.sigs.iter_mut().find(|x| x.api == "fmt_date") {
        x.handwritten = false;
    }
    chk(&mut set, "C4017-09", freeze_gate(&s, &c, &cov0) == T23ACode::RECEIPT_LATE, "err");

    // C4017-10 回执补齐 → 覆盖拦。
    c.sign("fmt_date", "t域", "n域");
    chk(&mut set, "C4017-10", freeze_gate(&s, &c, &cov0) == T23ACode::COVER_MISS, "err");

    // C4017-11 十映射覆盖登记 + 缺口。
    let mut cov = CoverageAssert::new();
    for m in TEN_MAPPINGS.iter().take(9) {
        cov.cover(m);
    }
    chk(&mut set, "C4017-11", cov.gaps() == vec!["调试"] && !cov.full(), "err");

    // C4017-12 补齐 → 冻结过。
    cov.cover("调试");
    chk(&mut set, "C4017-12", cov.full() && freeze_gate(&s, &c, &cov) == T23ACode::OK, "err");

    // C4017-13 冻结哈希确定性 + 区分度。
    let mut s2 = SigBook::new();
    s2.put("a", ApiFamily::Locale, "sig-a", false);
    let mut s3 = SigBook::new();
    s3.put("a", ApiFamily::Locale, "sig-b", false);
    chk(&mut set, "C4017-13",
        s2.freeze_hash() == s2.freeze_hash() && s2.freeze_hash() != s3.freeze_hash(), "err");

    // C4017-14 冻结后写入拒。
    s.frozen = true;
    chk(&mut set, "C4017-14", !s.put("new_api", ApiFamily::Plural, "x", false), "err");

    // C4017-15 族槽双射。
    let slots: Vec<usize> = API_FAMILIES.iter().map(|f| f.slot()).collect();
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4017-15", uniq, "err");

    // C4017-16 十映射名互异。
    let mut uniq2 = true;
    for i in 0..10 {
        for j in (i + 1)..10 {
            if TEN_MAPPINGS[i] == TEN_MAPPINGS[j] { uniq2 = false; }
        }
    }
    chk(&mut set, "C4017-16", uniq2 && TEN_MAPPINGS.len() == 10, "err");

    // C4017-17 越名覆盖拒绝。
    let mut cov2 = CoverageAssert::new();
    chk(&mut set, "C4017-17", !cov2.cover("伪映射"), "err");

    // C4017-18 重复覆盖拒绝。
    cov2.cover("Locale");
    chk(&mut set, "C4017-18", !cov2.cover("Locale"), "err");

    // C4017-19 回执码语义。
    chk(&mut set, "C4017-19", T23ACode::RECEIPT_LATE.explain().contains("不冻结"), "err");

    // C4017-20 错误码互异。
    let codes = [T23ACode::OK.0, T23ACode::RECEIPT_LATE.0, T23ACode::HANDWRITE.0, T23ACode::COVER_MISS.0];
    chk(&mut set, "C4017-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_blocks_handwritten() {
        let mut s = SigBook::new();
        s.put("a", ApiFamily::Locale, "s", true);
        let c = ConsumeBook::new();
        let cov = CoverageAssert::new();
        assert_eq!(freeze_gate(&s, &c, &cov), T23ACode::HANDWRITE);
    }

    #[test]
    fn dual_sign_no_same() {
        let mut c = ConsumeBook::new();
        assert!(!c.sign("a", "x", "x"));
        assert!(c.sign("a", "x", "y"));
    }

    #[test]
    fn coverage_full_gate() {
        let mut cov = CoverageAssert::new();
        for m in TEN_MAPPINGS.iter() {
            cov.cover(m);
        }
        assert!(cov.full());
    }
}
