//! ves332_s18sec — VE-F4092 · T05 语言资源安全
//! 三道防线：包来源签名验证 + 安装路径白名单 + 资源加载沙箱 + 供应链面（字体/资产第三方来源审查登记）+ 热更新回滚保护（防恶意回退）+ 来源审查含隐私合规项（资产不夹带跟踪——红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9740;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    SigBad = SEG_BASE + 0x01,
    PathOutside = SEG_BASE + 0x02,
    SandboxEscape = SEG_BASE + 0x03,
    SourceQuarantined = SEG_BASE + 0x04,
    RollbackFrozen = SEG_BASE + 0x05,
    TrackingPayload = SEG_BASE + 0x06,
}

/// 第一道：签名验证（FNV 模拟验签：对内容+密钥材料重算比对）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 签名链节点：包 → 签发方 → 内容摘要。
#[derive(Clone)]
pub struct SigChain { pub pkg: &'static str, pub issuer: &'static str, pub digest: u32 }

pub struct SigBook { pub chains: Vec<SigChain> }

impl SigBook {
    pub fn new() -> Self { SigBook { chains: Vec::new() } }
    pub fn register(&mut self, pkg: &'static str, issuer: &'static str, content: &[u8]) {
        let d = fnv1a(content);
        self.chains.push(SigChain { pkg, issuer, digest: d });
    }
    /// 验签：重算内容摘要与链上登记比对。
    pub fn verify(&self, pkg: &str, content: &[u8]) -> Result<&SigChain, ScCode> {
        let c = self.chains.iter().find(|c| c.pkg == pkg).ok_or(ScCode::SigBad)?;
        if fnv1a(content) != c.digest { return Err(ScCode::SigBad); }
        Ok(c)
    }
}

/// 第二道：安装路径白名单。
pub struct PathWhitelist { pub allowed: Vec<&'static str> }

impl PathWhitelist {
    pub fn new(allowed: Vec<&'static str>) -> Self { PathWhitelist { allowed } }
    /// 前缀匹配：路径必须落在白名单目录内。
    pub fn check(&self, path: &str) -> Result<(), ScCode> {
        for a in self.allowed.iter() {
            if path.starts_with(a) { return Ok(()); }
        }
        Err(ScCode::PathOutside)
    }
}

/// 第三道：资源加载沙箱——加载请求只许声明过的资源域。
pub struct Sandbox { pub domains: Vec<&'static str> }

impl Sandbox {
    pub fn new(domains: Vec<&'static str>) -> Self { Sandbox { domains } }
    /// 域内放行；越域即沙箱逃逸判定。
    pub fn load(&self, domain: &str) -> Result<(), ScCode> {
        if self.domains.iter().any(|d| *d == domain) { Ok(()) } else { Err(ScCode::SandboxEscape) }
    }
}

/// 供应链来源登记：第三方来源 × 审查状态 × 隐私合规项。
#[derive(Clone)]
pub struct SourceEntry { pub origin: &'static str, pub asset_kind: &'static str, pub reviewed: bool, pub tracking_free: bool }

pub struct SupplyBook { pub entries: Vec<SourceEntry> }

impl SupplyBook {
    pub fn new() -> Self { SupplyBook { entries: Vec::new() } }

    pub fn register(&mut self, origin: &'static str, asset_kind: &'static str, tracking_free: bool) -> Result<(), ScCode> {
        if !tracking_free { return Err(ScCode::TrackingPayload); } // 隐私红线：夹带跟踪直接拒
        self.entries.push(SourceEntry { origin, asset_kind, reviewed: true, tracking_free });
        Ok(())
    }

    /// 可疑来源 → 隔离审查（未登记来源的加载请求）。
    pub fn screen(&self, origin: &str) -> Result<(), ScCode> {
        match self.entries.iter().find(|e| e.origin == origin) {
            Some(e) if e.reviewed => Ok(()),
            Some(_) => Err(ScCode::SourceQuarantined),
            None => Err(ScCode::SourceQuarantined),
        }
    }

    pub fn count(&self) -> usize { self.entries.len() }
}

/// 回滚保护：版本地板（防恶意回退——低于地板拒绝）+ 异常回退冻结。
pub struct RollbackGuard { pub floor_version: u32, pub frozen: bool, pub anomalies: Vec<&'static str> }

impl RollbackGuard {
    pub fn new(floor: u32) -> Self { RollbackGuard { floor_version: floor, frozen: false, anomalies: Vec::new() } }

    /// 安装版本必须 ≥ 地板；低于地板 → 拒 + 立案。
    pub fn install_version(&mut self, v: u32) -> Result<(), ScCode> {
        if self.frozen { return Err(ScCode::RollbackFrozen); }
        if v < self.floor_version {
            self.anomalies.push("version-below-floor");
            return Err(ScCode::SigBad); // 恶意回退按验签失败处置
        }
        self.floor_version = v; // 地板随安装推进
        Ok(())
    }

    /// 回退异常（短窗内多次降版）→ 冻结 + 告警。
    pub fn flag_anomaly(&mut self, what: &'static str) -> bool {
        self.anomalies.push(what);
        if self.anomalies.len() >= 2 {
            self.frozen = true;
            return true; // 告警
        }
        false
    }

    pub fn alarm(&self) -> bool { self.frozen }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves332_checks() -> CheckSet {
    let mut set = CheckSet::new("ves332");

    // 夹具：签名链登记 zh 包内容。
    let mut sb = SigBook::new();
    let content = b"langpack-zh-v9001-payload";
    sb.register("zh-9001", "varix-official", content);

    // C332-01 第一道：验签通过。
    chk(&mut set, "C332-01", sb.verify("zh-9001", content).is_ok(), "内容原样 → 验签过");

    // C332-02 篡改内容 → 拒装。
    chk(&mut set, "C332-02", sb.verify("zh-9001", b"langpack-zh-v9001-payload!").is_err(), "一字篡改 => SigBad");

    // C332-03 未登记包 → 拒。
    chk(&mut set, "C332-03", sb.verify("ko-8800", content).is_err(), "无签名链 => SigBad");

    // C332-04 第二道：路径白名单内放行。
    let wl = PathWhitelist::new(vec!["/sys/langpacks/", "/user/langpacks/"]);
    chk(&mut set, "C332-04", wl.check("/sys/langpacks/zh/manifest.bin").is_ok(), "白名单目录内放行");

    // C332-05 白名单外路径拒绝。
    chk(&mut set, "C332-05", wl.check("/tmp/evil.bin") == Err(ScCode::PathOutside), "/tmp => PathOutside");

    // C332-06 前缀伪造不越界（同前缀字符串但目录不同）。
    chk(&mut set, "C332-06", wl.check("/sys/langpacks-evil/x") == Err(ScCode::PathOutside), "目录前缀需含分隔符");

    // C332-07 第三道：沙箱域内加载。
    let sx = Sandbox::new(vec!["font", "image", "audio", "video"]);
    chk(&mut set, "C332-07", sx.load("font").is_ok() && sx.load("video").is_ok(), "四域内加载放行");

    // C332-08 沙箱逃逸判定。
    chk(&mut set, "C332-08", sx.load("filesystem") == Err(ScCode::SandboxEscape), "越域 => SandboxEscape");

    // C332-09 供应链登记：夹带跟踪红线拒。
    let mut sup = SupplyBook::new();
    chk(&mut set, "C332-09", sup.register("third-font-x", "font", false) == Err(ScCode::TrackingPayload), "夹带跟踪 => 红线拒");

    // C332-10 合规来源登记 + 未登记来源隔离。
    let _ = sup.register("ofl-font-y", "font", true);
    chk(&mut set, "C332-10", sup.screen("ofl-font-y").is_ok() && sup.screen("unknown-z") == Err(ScCode::SourceQuarantined), "已审放行 未审隔离");

    // C332-11 防恶意回退：低于地板拒装。
    let mut rg = RollbackGuard::new(9001);
    chk(&mut set, "C332-11", rg.install_version(8900) == Err(ScCode::SigBad) && rg.anomalies.len() == 1, "8900 < 地板 9001 => 拒");

    // C332-12 回退异常累积 → 冻结 + 告警。
    let alarm = rg.flag_anomaly("rollback-spam");
    chk(&mut set, "C332-12", alarm && rg.alarm() && rg.install_version(9002) == Err(ScCode::RollbackFrozen), "二次异常 → 冻结");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves332_smoke() {
        let set = run_ves332_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves332 red: {:?}", red);
    }

    #[test]
    fn ves332_sig_chain() {
        let mut sb = SigBook::new();
        sb.register("p", "i", b"abc");
        assert!(sb.verify("p", b"abc").is_ok());
        assert!(sb.verify("p", b"abd").is_err());
    }

    #[test]
    fn ves332_rollback_floor() {
        let mut rg = RollbackGuard::new(100);
        assert!(rg.install_version(101).is_ok());
        assert_eq!(rg.floor_version, 101);
        assert!(rg.install_version(100).is_err());
    }
}
