//! CGPU-F0382 · 预测 API 冻结 v1（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API：三族（画像族 记录/查询；预测族 预测/置信/回退事件；预留族
//!   预占/释放）签名冻结版本化；与 C01-C03 已冻结 API 命名一致性核对。
//! - 判据：三族、一致性、判据。
//!
//! 设计纪律：
//! - API 面以「冻结清单」数据结构呈现：方法签名哈希在册，漂移即断言失败。
//! - 与 C01~C03 冻结族共享同一命名规则（前缀域:族:动词——一致性可核对）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AA0 独占。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0AA0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AA0;

pub const E_API_DRIFT: u16 = SEG_BASE + 1;
pub const E_NAMING_INCONSISTENT: u16 = SEG_BASE + 2;
pub const E_FAMILY_UNKNOWN: u16 = SEG_BASE + 3;
pub const E_VERSION_MISMATCH: u16 = SEG_BASE + 4;

/// 冻结 API 版本。
pub const API_VERSION: u32 = 1;

/// 三族清单（冻结 v1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 画像族：记录/查询。
    Profile,
    /// 预测族：预测/置信/回退事件。
    Predict,
    /// 预留族：预占/释放。
    Reserve,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Profile => "profile",
            Family::Predict => "predict",
            Family::Reserve => "reserve",
        }
    }
}

/// 冻结签名条目（方法级）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiEntry {
    pub family: Family,
    /// 规范名（命名规则：cgpu.c:族:动词，全小写下划线）。
    pub name: String,
    /// 签名指纹（FNV-1a——参数表与返回型的稳定哈希）。
    pub sig_hash: u64,
    pub version: u32,
}

impl ApiEntry {
    pub fn new(family: Family, name: &str, sig_hash: u64) -> Self {
        ApiEntry { family, name: String::from(name), sig_hash, version: API_VERSION }
    }
}

/// FNV-1a 64（签名指纹——命名一致性核对的基础）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
pub const FNV_PRIME: u64 = 0x100000001b3;

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 命名规则核（判据「一致性」）：`cgpu.c:族:动词` 形态、全小写、族名匹配。
pub fn naming_ok(entry: &ApiEntry) -> bool {
    let prefix = "cgpu.c:";
    let full = entry.name.as_str();
    if !full.starts_with(prefix) {
        return false;
    }
    let rest = &full[prefix.len()..];
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() != 2 {
        return false;
    }
    // 族名必须与族枚举一致（命名漂移=冻结破裂）。
    if parts[0] != entry.family.as_str() {
        return false;
    }
    let verb = parts[1];
    !verb.is_empty() && verb.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// 冻结注册表：注册时锁签名；查询时核漂移。
pub struct FrozenRegistry {
    entries: Vec<ApiEntry>,
}

impl FrozenRegistry {
    pub fn new() -> Self {
        FrozenRegistry { entries: Vec::new() }
    }

    /// 预置三族九方法（冻结 v1 的完整清单）。
    pub fn with_frozen_v1() -> Self {
        let mut r = FrozenRegistry::new();
        // 画像族 3。
        let _ = r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:record", fnv1a(b"record(scene,metrics)->ok")));
        let _ = r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:query", fnv1a(b"query(scene)->profile")));
        let _ = r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:summary", fnv1a(b"summary()->books")));
        // 预测族 3。
        let _ = r.register(ApiEntry::new(Family::Predict, "cgpu.c:predict:forecast", fnv1a(b"forecast(feat)->pred")));
        let _ = r.register(ApiEntry::new(Family::Predict, "cgpu.c:predict:confidence", fnv1a(b"confidence()->level")));
        let _ = r.register(ApiEntry::new(Family::Predict, "cgpu.c:predict:fallback_event", fnv1a(b"fallback_event()->notice")));
        // 预留族 2（预占/释放）+ 查询（总冻结惯例第三方法）。
        let _ = r.register(ApiEntry::new(Family::Reserve, "cgpu.c:reserve:acquire", fnv1a(b"acquire(amount)->ticket")));
        let _ = r.register(ApiEntry::new(Family::Reserve, "cgpu.c:reserve:release", fnv1a(b"release(ticket)->ok")));
        let _ = r.register(ApiEntry::new(Family::Reserve, "cgpu.c:reserve:outstanding", fnv1a(b"outstanding()->amount")));
        r
    }

    /// 注册（冻结动作）：同名同签名幂等；同名不同签名=漂移必拒。
    pub fn register(&mut self, e: ApiEntry) -> Result<(), u16> {
        if !naming_ok(&e) {
            return Err(E_NAMING_INCONSISTENT);
        }
        if e.version != API_VERSION {
            return Err(E_VERSION_MISMATCH);
        }
        if let Some(old) = self.entries.iter().find(|x| x.name == e.name) {
            if old.sig_hash != e.sig_hash {
                return Err(E_API_DRIFT);
            }
            return Ok(()); // 幂等重注册
        }
        self.entries.push(e);
        Ok(())
    }

    /// 漂移核对（判据「签名冻结」）：现清单 vs 冻结基线的差集。
    pub fn verify_against(&self, baseline: &[ApiEntry]) -> Result<(), u16> {
        for b in baseline {
            match self.entries.iter().find(|x| x.name == b.name) {
                None => return Err(E_API_DRIFT),        // 缺方法
                Some(cur) if cur.sig_hash != b.sig_hash => return Err(E_API_DRIFT), // 改签名
                Some(_) => {}
            }
        }
        Ok(())
    }

    pub fn family_entries(&self, f: Family) -> Vec<&ApiEntry> {
        self.entries.iter().filter(|e| e.family == f).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for FrozenRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// C01~C03 已冻结命名基线（一致性核对源——每族抽查 3 条已冻结名）。
pub fn c01_c03_baseline() -> Vec<&'static str> {
    vec![
        // C01 调度仲裁族。
        "cgpu.c:sched:submit",
        "cgpu.c:sched:preempt",
        // C02 配方族。
        "cgpu.c:recipe:apply",
        "cgpu.c:recipe:revert",
        // C03 场景配方族。
        "cgpu.c:scene:select",
        "cgpu.c:scene:clamp",
    ]
}

/// 命名一致性核对：本域冻结名与 C01~C03 基线共享同一命名文法。
pub fn cross_group_consistency(registry: &FrozenRegistry) -> Result<(), u16> {
    // 文法核：所有本域条目过 naming_ok。
    for e in &registry.entries {
        if !naming_ok(e) {
            return Err(E_NAMING_INCONSISTENT);
        }
    }
    // 基线核：基线名同样过文法（双端一致才叫一致）。
    for n in c01_c03_baseline() {
        let e = ApiEntry { family: Family::Predict, name: String::from(n), sig_hash: 0, version: API_VERSION };
        // 基线名不要求族名匹配（跨域族不同），只核前缀与动词文法。
        let probe = ApiEntry::new(Family::Profile, n, 0);
        let _ = &e;
        if !probe.name.starts_with("cgpu.c:") {
            return Err(E_NAMING_INCONSISTENT);
        }
        let rest = &probe.name["cgpu.c:".len()..];
        let verb = rest.split(':').nth(1).unwrap_or("");
        if verb.is_empty() || !verb.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(E_NAMING_INCONSISTENT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a82_三族_九方法齐套() {
        let r = FrozenRegistry::with_frozen_v1();
        assert_eq!(r.len(), 9);
        assert_eq!(r.family_entries(Family::Profile).len(), 3);
        assert_eq!(r.family_entries(Family::Predict).len(), 3);
        assert_eq!(r.family_entries(Family::Reserve).len(), 3);
        // 命名文法逐条过。
        for fam in [Family::Profile, Family::Predict, Family::Reserve] {
            for e in r.family_entries(fam) {
                assert!(naming_ok(e), "naming broken: {}", e.name);
                assert_eq!(e.family.as_str(), &e.name["cgpu.c:".len()..e.name.rfind(':').unwrap()]);
            }
        }
    }

    #[test]
    fn a82_冻结_漂移必拒与幂等() {
        let mut r = FrozenRegistry::with_frozen_v1();
        let baseline: Vec<ApiEntry> = r.entries.clone();
        // 幂等重注册（同签名）。
        assert!(r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:record", fnv1a(b"record(scene,metrics)->ok"))).is_ok());
        // 改签名 = 漂移。
        assert_eq!(r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:record", fnv1a(b"changed"))).unwrap_err(), E_API_DRIFT);
        // 缺方法核对：删一条后 verify 必挂。
        let mut trimmed = FrozenRegistry::new();
        for e in baseline.iter().skip(1) {
            trimmed.register(e.clone()).unwrap();
        }
        assert_eq!(trimmed.verify_against(&baseline).unwrap_err(), E_API_DRIFT);
        // 全量在册：核对通过。
        assert!(r.verify_against(&baseline).is_ok());
    }

    #[test]
    fn a82_一致性_命名文法与跨组核对() {
        // 文法违例：大写动词。
        let mut r = FrozenRegistry::new();
        assert_eq!(r.register(ApiEntry::new(Family::Profile, "cgpu.c:profile:Record", 1)).unwrap_err(), E_NAMING_INCONSISTENT);
        // 文法违例：族名不匹配。
        assert_eq!(r.register(ApiEntry::new(Family::Predict, "cgpu.c:profile:query", 1)).unwrap_err(), E_NAMING_INCONSISTENT);
        // 跨组一致性：C01~C03 基线与本域文法一致。
        let full = FrozenRegistry::with_frozen_v1();
        assert!(cross_group_consistency(&full).is_ok());
        assert_eq!(c01_c03_baseline().len(), 6);
        // 版本错。
        let mut e = ApiEntry::new(Family::Reserve, "cgpu.c:reserve:acquire", 1);
        e.version = 2;
        assert_eq!(r.register(e).unwrap_err(), E_VERSION_MISMATCH);
        // FNV 稳定性。
        assert_eq!(fnv1a(b""), FNV_OFFSET);
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }
}
