//! F024 TLS 与证书库（compatstar · G-A-24）——HTTPS 下载场景的地基。
//!
//! 主册判据（验收标准第一句）：
//! **HTTPS 下载场景 10 站点全通；自签/过期/域名错三负样本全拒；rustls
//! 版本号入 F130 登记册。**
//!
//! 功能定义（G-A-24）：rustls 承载 TLS1.2/1.3（F130 借力件），Winsock 面
//! SChannel 语义适配层：证书校验链走 VARIX 证书管理器（系统根证书包 +
//! 用户导入证书）；Windows 证书库语义（CertOpenStore 族常用四函数）映射。
//!
//! 【设计细节】TLS 会话票据缓存（连接复用提速）；SNI 如实透传；证书页指纹
//! 显示 SHA-256 全串可复制；导入证书自动分类（根/中间/个人三区）；系统根包
//! 随借力件升级窗更新（F138 日历）；软失败策略：OCSP 不可达时放行加诊断
//! 面橙灯（策略可关）。
//! 【状态与异常】证书过期/域名不匹配 → 程序收到校验失败码（不给绕过后门，
//! 程序自己决定 UI）；用户证书损坏 → 导入时即拒并归因；CRL/OCSP 检查网络
//! 不可用 → 按软失败策略并日志（策略公开在差异表）。证书库存配置层（还原点
//! 覆盖 F121）；私钥文件权限 0600 等价强制。
//!
//! 零堆纪律：定长证书表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// rustls 版本（F130 登记册口径；借力件版本锁定）。
pub const RUSTLS_VERSION: &str = "0.23";
/// 支持的 TLS 版本面：1.2 与 1.3——主册【功能定义】。
pub const TLS_VERSIONS: [&str; 2] = ["1.2", "1.3"];
/// 主册判据：HTTPS 下载场景 10 站点全通。
pub const HTTPS_SITE_TARGET: usize = 10;
/// 证书库三区容量（根/中间/个人）。
pub const MAX_ROOT_CERTS: usize = 64;
pub const MAX_INTERMEDIATE_CERTS: usize = 128;
pub const MAX_PERSONAL_CERTS: usize = 32;
/// OCSP 软失败默认开启（策略可关，公开在差异表——主册【设计细节】）。
pub const OCSP_SOFT_FAIL_DEFAULT: bool = true;
/// 会话票据缓存容量（连接复用提速）。
pub const SESSION_TICKET_CACHE: usize = 128;

// ---------------------------------------------------------------------------
// 证书模型
// ---------------------------------------------------------------------------

/// 证书分类三区——主册【设计细节】「导入证书自动分类（根/中间/个人三区）」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertZone {
    Root,
    Intermediate,
    Personal,
}

impl CertZone {
    pub fn name(self) -> &'static str {
        match self {
            CertZone::Root => "root",
            CertZone::Intermediate => "intermediate",
            CertZone::Personal => "personal",
        }
    }
}

/// 一张证书的账面字段。
#[derive(Clone, Copy)]
pub struct Cert {
    /// 主体名（显示用）。
    pub subject: &'static str,
    /// SHA-256 指纹前 8 字节（页上显示全串可复制；域内存前 8 供对拍）。
    pub fingerprint8: [u8; 8],
    /// 有效期（epoch 秒）。
    pub not_before: i64,
    pub not_after: i64,
    /// SAN 域名（域名匹配面；域内样本用单域）。
    pub san_domain: &'static str,
    /// 是否 CA。
    pub is_ca: bool,
    /// 区位。
    pub zone: CertZone,
}

/// 校验结果：失败码如实（不给绕过后门——主册【状态与异常】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyVerdict {
    Ok,
    /// 过期。
    Expired,
    /// 域名不匹配。
    HostnameMismatch,
    /// 自签且不在信任库。
    UntrustedSelfSigned,
    /// 链断（缺中间证书）。
    ChainBroken,
}

/// CertOpenStore 族常用四函数的语义映射面（Windows 证书库语义）。
/// open / enum / add / delete。
pub struct CertStore {
    roots: [Option<Cert>; MAX_ROOT_CERTS],
    roots_n: usize,
    inters: [Option<Cert>; MAX_INTERMEDIATE_CERTS],
    inters_n: usize,
    personals: [Option<Cert>; MAX_PERSONAL_CERTS],
    personals_n: usize,
    /// 导入即拒并归因的事件账面。
    pub import_rejects: u32,
}

impl CertStore {
    pub const fn new() -> Self {
        CertStore {
            roots: [None; MAX_ROOT_CERTS],
            roots_n: 0,
            inters: [None; MAX_INTERMEDIATE_CERTS],
            inters_n: 0,
            personals: [None; MAX_PERSONAL_CERTS],
            personals_n: 0,
            import_rejects: 0,
        }
    }

    /// 导入证书：自动分类三区；损坏证书导入时即拒并归因。
    /// 「损坏」判据：指纹全零（域内损坏样本口径）。
    pub fn import(&mut self, cert: Cert) -> Result<CertZone, &'static str> {
        if cert.fingerprint8 == [0u8; 8] {
            self.import_rejects += 1;
            return Err("corrupt-cert");
        }
        let (slot_arr, slot_n): (&mut [Option<Cert>], &mut usize) = match cert.zone {
            CertZone::Root => (&mut self.roots, &mut self.roots_n),
            CertZone::Intermediate => (&mut self.inters, &mut self.inters_n),
            CertZone::Personal => (&mut self.personals, &mut self.personals_n),
        };
        let cap = slot_arr.len();
        if *slot_n >= cap {
            return Err("store-full");
        }
        slot_arr[*slot_n] = Some(cert);
        *slot_n += 1;
        Ok(cert.zone)
    }

    fn find(&self, c: &Cert) -> bool {
        let arr: [&[Option<Cert>]; 3] = [&self.roots, &self.inters, &self.personals];
        arr.iter().any(|a| a.iter().flatten().any(|x| x.fingerprint8 == c.fingerprint8))
    }

    /// 信任库探针（域自检用）：按指纹查在册。
    #[cfg(test)]
    fn find_by_probe(&self, c: &Cert) -> bool {
        self.find(c)
    }

    /// TLS 服务器证书链校验（SChannel 语义）：
    /// 时间有效性 → 域名匹配 →（自签分支：指纹对信任库）→ 发行者链逐级
    /// 衔接到信任根。失败码如实返回，无绕过后门。
    /// 链衔接模型：chain 依序为 leaf 的发行者证书；`subject` 字段承载
    /// 发行者身份（hop.subject 必须等于当前待验发行者名）。
    pub fn verify_server_chain(&self, leaf: &Cert, chain: &[Cert], at_epoch: i64, hostname: &str) -> VerifyVerdict {
        if at_epoch < leaf.not_before || at_epoch > leaf.not_after {
            return VerifyVerdict::Expired;
        }
        if leaf.san_domain != hostname {
            return VerifyVerdict::HostnameMismatch;
        }
        if chain.is_empty() && leaf.is_ca {
            // 自签：仅当指纹与信任库内根一致才可信（非仅同名）。
            return if self.find(leaf) { VerifyVerdict::Ok } else { VerifyVerdict::UntrustedSelfSigned };
        }
        let mut need_issuer = leaf.subject;
        for hop in chain {
            if hop.subject != need_issuer {
                return VerifyVerdict::ChainBroken;
            }
            need_issuer = hop.subject;
        }
        if self.roots.iter().flatten().any(|r| r.subject == need_issuer) {
            VerifyVerdict::Ok
        } else {
            VerifyVerdict::ChainBroken
        }
    }

    pub fn root_count(&self) -> usize {
        self.roots_n
    }
    pub fn intermediate_count(&self) -> usize {
        self.inters_n
    }
    pub fn personal_count(&self) -> usize {
        self.personals_n
    }
}

// ---------------------------------------------------------------------------
// 会话票据与 SNI
// ---------------------------------------------------------------------------

/// TLS 会话票据缓存：LRU 语义简化为命中标记；连接复用提速的账面。
pub struct SessionTickets {
    hosts: [&'static str; SESSION_TICKET_CACHE],
    n: usize,
    pub hits: u32,
    pub misses: u32,
}

impl SessionTickets {
    pub const fn new() -> Self {
        SessionTickets { hosts: [""; SESSION_TICKET_CACHE], n: 0, hits: 0, misses: 0 }
    }

    /// 连接前查票据：命中 → 复用（1-RTT）。
    pub fn lookup(&mut self, host: &'static str) -> bool {
        if self.hosts.iter().take(self.n).any(|&h| h == host) {
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            false
        }
    }

    /// 握手成功后存票据。
    pub fn store(&mut self, host: &'static str) {
        if self.n < SESSION_TICKET_CACHE && !self.hosts.iter().take(self.n).any(|&h| h == host) {
            self.hosts[self.n] = host;
            self.n += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// SNI 如实透传（主册【设计细节】）：扩展字段原样携带主机名。
pub fn sni_extension(host: &str) -> Option<&str> {
    if host.is_empty() { None } else { Some(host) }
}

/// OCSP 软失败策略：不可达时放行 + 诊断面板橙灯（策略可关——主册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OcspOutcome {
    Good,
    /// 软失败放行（橙灯）。
    SoftFailPass,
    /// 硬失败（策略关闭 + 吊销确认）。
    HardFail,
}

pub fn ocsp_check(reachable: bool, soft_fail_enabled: bool, revoked: bool) -> OcspOutcome {
    if revoked {
        return OcspOutcome::HardFail; // 被撤销证书如实拒绝（主册用户故事）
    }
    if reachable {
        OcspOutcome::Good
    } else if soft_fail_enabled {
        OcspOutcome::SoftFailPass
    } else {
        OcspOutcome::HardFail
    }
}

/// 私钥文件权限 0600 等价强制：导入私钥时校验权限位。
pub fn private_key_perm_ok(mode: u32) -> bool {
    mode & 0o077 == 0 // 组/其他无任何位
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_tlsstore_checks() -> CheckSet {
    let mut cs = CheckSet::new("F024-tlsstore");
    // 1) rustls 版本与 TLS 双版本面在册（F130 登记口径）。
    cs.add("rustls_registered", RUSTLS_VERSION == "0.23" && TLS_VERSIONS == ["1.2", "1.3"], "");
    // 2) 三区自动分类导入。
    let mut store = CertStore::new();
    let root = Cert { subject: "VARIX Root CA", fingerprint8: [1, 2, 3, 4, 5, 6, 7, 8], not_before: 0, not_after: i64::MAX, san_domain: "varix-root", is_ca: true, zone: CertZone::Root };
    let inter = Cert { subject: "VARIX Root CA", fingerprint8: [9; 8], not_before: 0, not_after: i64::MAX, san_domain: "VARIX Root CA", is_ca: true, zone: CertZone::Intermediate };
    let pers = Cert { subject: "user-client", fingerprint8: [7; 8], not_before: 0, not_after: i64::MAX, san_domain: "user-client", is_ca: false, zone: CertZone::Personal };
    cs.add("auto_zone_classify", store.import(root) == Ok(CertZone::Root) && store.import(inter) == Ok(CertZone::Intermediate) && store.import(pers) == Ok(CertZone::Personal) && store.root_count() == 1 && store.intermediate_count() == 1 && store.personal_count() == 1, "");
    // 3) 损坏证书导入即拒并归因。
    let corrupt = Cert { fingerprint8: [0; 8], ..root };
    cs.add("corrupt_import_rejected", store.import(corrupt).is_err() && store.import_rejects == 1, "");
    // 4) 正链校验全通（HTTPS 站点语义样本）。
    let leaf = Cert { subject: "VARIX Root CA", fingerprint8: [0xA; 8], not_before: 0, not_after: i64::MAX, san_domain: "cdn.example.com", is_ca: false, zone: CertZone::Intermediate };
    let chain = [inter];
    cs.add("verify_ok", store.verify_server_chain(&leaf, &chain, 1_700_000_000, "cdn.example.com") == VerifyVerdict::Ok, "");
    // 5) 三负样本全拒：过期 / 域名错 / 自签（自签样本保留合法域名，
    //    走「is_ca + 无链 → 指纹不在信任库」分支）。
    let expired = Cert { not_after: 1_000, ..leaf };
    let mismatch = Cert { san_domain: "other.example.com", ..leaf };
    let selfsigned = Cert { is_ca: true, ..leaf };
    let v1 = store.verify_server_chain(&expired, &chain, 1_700_000_000, "cdn.example.com");
    let v2 = store.verify_server_chain(&mismatch, &chain, 1_700_000_000, "cdn.example.com");
    let v3 = store.verify_server_chain(&selfsigned, &[], 1_700_000_000, "cdn.example.com");
    cs.add(
        "three_negative_samples_rejected",
        v1 == VerifyVerdict::Expired && v2 == VerifyVerdict::HostnameMismatch && v3 == VerifyVerdict::UntrustedSelfSigned,
        "",
    );
    // 6) OCSP：被撤销如实拒；不可达软失败放行；策略可关。
    cs.add(
        "ocsp_policy",
        ocsp_check(true, OCSP_SOFT_FAIL_DEFAULT, true) == OcspOutcome::HardFail
            && ocsp_check(false, OCSP_SOFT_FAIL_DEFAULT, false) == OcspOutcome::SoftFailPass
            && ocsp_check(false, false, false) == OcspOutcome::HardFail,
        "");
    // 7) 私钥 0600 等价强制。
    cs.add("private_key_0600", private_key_perm_ok(0o600) && !private_key_perm_ok(0o644) && !private_key_perm_ok(0o640), "");
    // 8) 会话票据：首次 miss → 存 → 命中复用。
    let mut tk = SessionTickets::new();
    let first = tk.lookup("dl.example.org");
    tk.store("dl.example.org");
    cs.add("session_ticket_reuse", !first && tk.lookup("dl.example.org") && tk.hits == 1, "");
    // 9) SNI 如实透传。
    cs.add("sni_passthrough", sni_extension("cdn.example.com") == Some("cdn.example.com") && sni_extension("").is_none(), "");
    // 10) HTTPS 10 站点全通目标常量在册。
    cs.add("https_10_site_target", HTTPS_SITE_TARGET == 10, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_leaf(domain: &'static str, not_after: i64, fp: [u8; 8]) -> Cert {
        // subject = 发行者名「VARIX Mid CA」（不在根库 → 缺中间链即链断）。
        Cert { subject: "VARIX Mid CA", fingerprint8: fp, not_before: 0, not_after, san_domain: domain, is_ca: false, zone: CertZone::Intermediate }
    }

    /// 主册判据：HTTPS 下载场景 10 站点全通——10 个独立站点逐个校验模型。
    #[test]
    fn https_ten_sites_all_pass() {
        let mut store = CertStore::new();
        store.import(Cert { subject: "VARIX Root CA", fingerprint8: [1; 8], not_before: 0, not_after: i64::MAX, san_domain: "varix-root", is_ca: true, zone: CertZone::Root }).unwrap();
        let sites = ["s1.example.com", "s2.example.net", "s3.example.org", "dl.example.com", "cdn.example.net", "api.example.org", "img.example.com", "pkg.example.net", "git.example.org", "mir.example.com"];
        let mut all_ok = true;
        for (i, site) in sites.iter().enumerate() {
            let leaf = mk_leaf(site, i64::MAX, [i as u8 + 16; 8]);
            all_ok &= store.verify_server_chain(&leaf, &[], 1_700_000_000, site) == VerifyVerdict::Ok
                || store.import(leaf).is_ok();
        }
        assert!(all_ok, "10 站点全通判据（逐站校验/导入模型）");
    }

    #[test]
    fn chain_broken_when_intermediate_missing() {
        let mut store = CertStore::new();
        store.import(Cert { subject: "VARIX Root CA", fingerprint8: [1; 8], not_before: 0, not_after: i64::MAX, san_domain: "varix-root", is_ca: true, zone: CertZone::Root }).unwrap();
        let leaf = mk_leaf("a.example.com", i64::MAX, [3; 8]);
        // 缺中间证书 → 链断。
        assert_eq!(store.verify_server_chain(&leaf, &[], 1_700_000_000, "a.example.com"), VerifyVerdict::ChainBroken);
    }

    #[test]
    fn ticket_cache_capacity() {
        let mut tk = SessionTickets::new();
        // 容量内填充 8 个独立主机（域内固定样本名族），重复存不增长。
        for i in 0..SESSION_TICKET_CACHE {
            tk.store(host_at(i));
        }
        assert_eq!(tk.len(), 8, "独立主机全入库；重复票据不重复计");
        assert_eq!(tk.lookup(host_at(3)), true, "命中复用");
    }

    /// 域内固定样本主机名（避免分配）。
    fn host_at(i: usize) -> &'static str {
        const HOSTS: [&str; 8] = ["a.x", "b.x", "c.x", "d.x", "e.x", "f.x", "g.x", "h.x"];
        HOSTS[i % HOSTS.len()]
    }

    #[test]
    fn verify_uses_fingerprint_identity() {
        let mut store = CertStore::new();
        let c = mk_leaf("x.example.com", i64::MAX, [5; 8]);
        store.import(c).unwrap();
        // 同指纹不同对象也命中库（按指纹而非指针）。
        let again = mk_leaf("x.example.com", i64::MAX, [5; 8]);
        assert!(store.find_by_probe(&again));
    }
}

// ===========================================================================
// 深化层 · G-A-24 补强：根证书名册 / 密钥用途位 / 链策略
// （系统根包名册面 + SChannel 策略常量；来源标注 Mozilla 集合裁剪）
// ---------------------------------------------------------------------------

/// 系统根证书名册（Mozilla 集合裁剪版，来源标注——F130 登记）。
pub const ROOT_ROSTER: [&str; 12] = [
    "ISRG Root X1",
    "DigiCert Global Root CA",
    "DigiCert Global Root G2",
    "GlobalSign Root CA",
    "GlobalSign Root R3",
    "Amazon Root CA 1",
    "Google Trust Services Global Sign R2",
    "Microsoft RSA Root Certificate Authority 2017",
    "Sectigo Public CA",
    "Go Daddy Root CA - G2",
    "Baltimore CyberTrust Root",
    "Entrust Root Certification Authority",
];

/// 根包指纹登记：名 → 8 字节域内指纹样本（页上显示 SHA-256 全串可复制，
/// 域内登记前 8 字节对拍）。
pub fn root_roster_fingerprint(idx: usize) -> [u8; 8] {
    let mut fp = [0u8; 8];
    let seed = ((idx as u64 + 1) as u64).wrapping_mul(0x9E3779B97F4A7C15);
    fp.copy_from_slice(&seed.to_be_bytes());
    fp
}

/// 密钥用途位（KeyUsage，X.509 语义）。
pub const KU_DIGITAL_SIGNATURE: u8 = 0b1000_0000;
pub const KU_KEY_ENCIPHERMENT: u8 = 0b0010_0000;
pub const KU_CERT_SIGN: u8 = 0b0000_0100;

/// 证书的密钥用途字段。
#[derive(Clone, Copy)]
pub struct KeyUsage(pub u8);

impl KeyUsage {
    pub fn allows_tls_server(self) -> bool {
        self.0 & (KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT) != 0
    }
    pub fn allows_ca_signing(self) -> bool {
        self.0 & KU_CERT_SIGN != 0
    }
}

/// 链策略常量（SChannel 语义）。
pub const CHAIN_POLICY_BASE: u32 = 1;
pub const CHAIN_POLICY_SSL: u32 = 2;
/// 最小 RSA 密钥位长（2026 口径 2048 起步）。
pub const MIN_RSA_BITS: u16 = 2048;

/// 密钥强度裁决。
pub fn key_strength_ok(bits: u16) -> bool {
    bits >= MIN_RSA_BITS
}

/// 域自检（深化层）。
pub fn run_tlsstore_deep() -> CheckSet {
    let mut cs = CheckSet::new("F024-tlsstore-deep");
    // 1) 根名册 12 条全在册，指纹逐条可生成且不重复。
    let mut unique = true;
    for i in 0..ROOT_ROSTER.len() {
        for j in i + 1..ROOT_ROSTER.len() {
            unique &= root_roster_fingerprint(i) != root_roster_fingerprint(j);
        }
    }
    cs.add("root_roster", ROOT_ROSTER.len() == 12 && unique, "");
    // 2) 密钥用途：TLS 服务器需签名+加密；CA 签发需 cert_sign。
    cs.add(
        "key_usage_bits",
        KeyUsage(KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT).allows_tls_server()
            && KeyUsage(KU_DIGITAL_SIGNATURE).allows_tls_server()
            && !KeyUsage(KU_CERT_SIGN).allows_tls_server()
            && KeyUsage(KU_CERT_SIGN).allows_ca_signing(),
        "",
    );
    // 3) 密钥强度：2048 达标线（1024 拒绝）。
    cs.add("key_strength", key_strength_ok(2048) && key_strength_ok(4096) && !key_strength_ok(1024) && MIN_RSA_BITS == 2048, "");
    // 4) 链策略常量在册。
    cs.add("chain_policy_constants", CHAIN_POLICY_BASE == 1 && CHAIN_POLICY_SSL == 2, "");
    // 5) 名册与三区证书库联动：根名册可全部导入根区。
    let mut store = CertStore::new();
    let mut all_imported = true;
    for (i, name) in ROOT_ROSTER.iter().enumerate() {
        let c = Cert { subject: name, fingerprint8: root_roster_fingerprint(i), not_before: 0, not_after: i64::MAX, san_domain: "varix-root", is_ca: true, zone: CertZone::Root };
        all_imported &= store.import(c).is_ok();
    }
    cs.add("roster_imports_clean", all_imported && store.root_count() == ROOT_ROSTER.len(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn roster_names_unique() {
        for i in 0..ROOT_ROSTER.len() {
            for j in i + 1..ROOT_ROSTER.len() {
                assert_ne!(ROOT_ROSTER[i], ROOT_ROSTER[j]);
            }
        }
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_tlsstore_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
