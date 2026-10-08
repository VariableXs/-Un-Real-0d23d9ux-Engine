//! F024 深化批次三 · 证书扩展与名称约束面（compatstar2/deep2 · G-A-24）。
//!
//! 主层 tlsstore.rs 覆盖证书库/链验证/SNI/OCSP；批次二深化覆盖密钥用法
//! 三位子面。本批补齐主册【功能定义】「全语义对齐」的注入/边界面：
//! EKU OID 语义表（serverAuth/clientAuth/codeSigning/timeStamping 四类，
//! id-kp OID 值段真实值 1.3.6.1.5.5.7.3.{1,2,3,8}）、SAN 多名解析（DNS
//! 大小写归一/IP v4/非法类型计数）、密钥用法位掩码验证链（RFC 5280
//! keyUsage 六位真实值 + 用法→必需位判定函数）、名称约束边界（通配符
//! 仅限最左整标签、多通配符拒绝、IDN 前缀 xn-- 与通配符组合保留拒绝）。
//!
//! 判据对账：主册 G-A-24【设计细节】TLS 证书段 + RFC 5280（id-kp 注册
//! 段、keyUsage 位序）/RFC 6125（通配符仅限最左标签）语义对拍。
//!
//! 零堆纪律：SAN 定长 16 槽、名称缓冲定长 64 字节，无 Vec/String/Box/
//! format!，拒绝一律 Err(&'static str) 或计数账面。

use crate::checks::CheckSet;

/// id-kp-serverAuth（RFC 5280 Appendix A 注册段真实值）。
pub const OID_EKU_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
/// id-kp-clientAuth。
pub const OID_EKU_CLIENT_AUTH: &str = "1.3.6.1.5.5.7.3.2";
/// id-kp-codeSigning。
pub const OID_EKU_CODE_SIGNING: &str = "1.3.6.1.5.5.7.3.3";
/// id-kp-timeStamping。
pub const OID_EKU_TIME_STAMPING: &str = "1.3.6.1.5.5.7.3.8";

/// EKU 角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EkuRole {
    ServerAuth,
    ClientAuth,
    CodeSigning,
    TimeStamping,
}

/// EKU OID 语义表（OID → 角色，RFC 5280 注册值）。
pub const EKU_TABLE: [(&str, EkuRole); 4] = [
    (OID_EKU_SERVER_AUTH, EkuRole::ServerAuth),
    (OID_EKU_CLIENT_AUTH, EkuRole::ClientAuth),
    (OID_EKU_CODE_SIGNING, EkuRole::CodeSigning),
    (OID_EKU_TIME_STAMPING, EkuRole::TimeStamping),
];

/// EKU 查表；未注册 OID 显性 None（不猜语义）。
pub fn eku_lookup(oid: &str) -> Option<EkuRole> {
    EKU_TABLE.iter().find(|(o, _)| *o == oid).map(|(_, r)| *r)
}

// keyUsage 六位真实值（RFC 5280 §4.2.1.3 bit 序，MSB-first 编码）。
pub const KU_DIGITAL_SIGNATURE: u8 = 0x80;
pub const KU_NON_REPUDIATION: u8 = 0x40;
pub const KU_KEY_ENCIPHERMENT: u8 = 0x20;
pub const KU_DATA_ENCIPHERMENT: u8 = 0x10;
pub const KU_KEY_AGREEMENT: u8 = 0x08;
pub const KU_KEY_CERT_SIGN: u8 = 0x04;

/// 用法目的（验证链节点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KuPurpose {
    TlsServer,
    TlsClient,
    CodeSign,
    TimeStamp,
}

/// 用法 → 必需位判定表（RFC 5280 + MS CryptoAPI 语义对拍）。
pub fn required_ku(purpose: KuPurpose) -> u8 {
    match purpose {
        KuPurpose::TlsServer => KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT, // RSA 密钥交换面
        KuPurpose::TlsClient => KU_DIGITAL_SIGNATURE,
        KuPurpose::CodeSign => KU_DIGITAL_SIGNATURE,
        KuPurpose::TimeStamp => KU_NON_REPUDIATION,
    }
}

/// 位掩码验证：必需位全置才放行；不足显性 Err（不静默降级）。
pub fn ku_check(mask: u8, purpose: KuPurpose) -> Result<(), &'static str> {
    let need = required_ku(purpose);
    if mask & need == need { Ok(()) } else { Err("ku-insufficient") }
}

/// SAN 名称定长缓冲。
pub const SAN_NAME_CAP: usize = 64;
/// SAN 列表定长（域内模型口径）。
pub const SAN_SLOTS: usize = 16;
/// GeneralName dNSName 上下文标签（wincrypt CERT_ALT_NAME_DNS_NAME）。
pub const SAN_KIND_DNS: u32 = 2;
/// GeneralName iPAddress 上下文标签（wincrypt CERT_ALT_NAME_IP_ADDRESS）。
pub const SAN_KIND_IP: u32 = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SanKind {
    Dns,
    Ipv4,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SanEntry {
    pub kind: SanKind,
    /// DNS 名归一化小写缓冲（IPv4 时不用）。
    pub dns: [u8; SAN_NAME_CAP],
    pub dns_len: usize,
    /// IPv4 四元组（DNS 时不用）。
    pub ip: [u8; 4],
}

/// SAN 多名列表：定长 16，非法类型/超容计数显性化。
pub struct SanList {
    entries: [Option<SanEntry>; SAN_SLOTS],
    pub n: usize,
    pub illegal_types: u32,
    pub rejected: u32,
}

fn parse_ip4(s: &str) -> Result<[u8; 4], &'static str> {
    let mut ip = [0u8; 4];
    let mut i = 0usize;
    for part in s.split('.') {
        if i >= 4 { return Err("san-ip-format"); }
        let b = part.as_bytes();
        if b.is_empty() || b.len() > 3 { return Err("san-ip-format"); }
        let mut v: u16 = 0;
        for &c in b {
            if !c.is_ascii_digit() { return Err("san-ip-format"); }
            v = v * 10 + (c - b'0') as u16;
        }
        if v > 255 { return Err("san-ip-octet"); }
        ip[i] = v as u8;
        i += 1;
    }
    if i != 4 { return Err("san-ip-format"); }
    Ok(ip)
}

impl SanList {
    pub const fn new() -> Self {
        SanList { entries: [None; SAN_SLOTS], n: 0, illegal_types: 0, rejected: 0 }
    }

    /// 追加一名：DNS 大小写归一化入定长缓冲；IP 严格四段解析；
    /// 非法类型/超容/非法名一律计数并显性 Err。
    pub fn add(&mut self, kind_id: u32, raw: &str) -> Result<usize, &'static str> {
        let entry = match kind_id {
            SAN_KIND_DNS => {
                if raw.is_empty() || raw.len() > SAN_NAME_CAP { self.rejected += 1; return Err("san-name-invalid"); }
                let mut dns = [0u8; SAN_NAME_CAP];
                for (i, &c) in raw.as_bytes().iter().enumerate() {
                    dns[i] = c.to_ascii_lowercase();
                }
                SanEntry { kind: SanKind::Dns, dns, dns_len: raw.len(), ip: [0; 4] }
            }
            SAN_KIND_IP => {
                let ip = parse_ip4(raw)?;
                SanEntry { kind: SanKind::Ipv4, dns: [0; SAN_NAME_CAP], dns_len: 0, ip }
            }
            _ => { self.illegal_types += 1; return Err("san-kind-illegal"); }
        };
        if self.n >= SAN_SLOTS { self.rejected += 1; return Err("san-full"); }
        self.entries[self.n] = Some(entry);
        self.n += 1;
        Ok(self.n - 1)
    }

    pub fn get(&self, i: usize) -> Option<SanEntry> {
        if i < self.n {
            self.entries[i]
        } else {
            None
        }
    }
}

/// 名称约束边界：通配符仅限最左整标签；多通配符拒绝；通配符对 IDN
/// punycode 标签（xn-- 前缀，RFC 5890 保留段）组合保留拒绝。
pub fn wildcard_valid(name: &str) -> Result<(), &'static str> {
    let mut star_labels = 0usize;
    let mut leftmost_star = false;
    let mut second: Option<&str> = None;
    for (i, label) in name.split('.').enumerate() {
        if label.contains('*') {
            star_labels += 1;
            if label != "*" { return Err("wildcard-partial-label"); }
            if i == 0 { leftmost_star = true; }
        }
        if i == 1 { second = Some(label); }
    }
    if star_labels > 1 { return Err("wildcard-multi"); }
    if star_labels == 1 && !leftmost_star { return Err("wildcard-not-leftmost"); }
    if leftmost_star && second.map(|s| s.len() >= 4 && s.as_bytes()[..4] == *b"xn--").unwrap_or(false) { return Err("idn-reserved"); }
    Ok(())
}

/// 域自检（深化批次三）。
pub fn run_f024e_checks() -> crate::checks::CheckSet {
    let mut cs = CheckSet::new("F024-tlsstore-d3");
    // 1) EKU OID 表真实值（RFC 5280 id-kp 注册段）。
    cs.add(
        "eku_oids_real",
        OID_EKU_SERVER_AUTH == "1.3.6.1.5.5.7.3.1"
            && OID_EKU_CLIENT_AUTH == "1.3.6.1.5.5.7.3.2"
            && OID_EKU_CODE_SIGNING == "1.3.6.1.5.5.7.3.3"
            && OID_EKU_TIME_STAMPING == "1.3.6.1.5.5.7.3.8",
        "",
    );
    // 2) 查表命中/未注册 OID 显性 None。
    cs.add(
        "eku_lookup",
        eku_lookup("1.3.6.1.5.5.7.3.1") == Some(EkuRole::ServerAuth)
            && eku_lookup("1.3.6.1.5.5.7.3.8") == Some(EkuRole::TimeStamping)
            && eku_lookup("1.3.6.1.5.5.7.3.99").is_none(),
        "",
    );
    // 3) keyUsage 六位真实值（RFC 5280 bit 序）。
    cs.add(
        "ku_bit_values",
        KU_DIGITAL_SIGNATURE == 0x80
            && KU_NON_REPUDIATION == 0x40
            && KU_KEY_ENCIPHERMENT == 0x20
            && KU_DATA_ENCIPHERMENT == 0x10
            && KU_KEY_AGREEMENT == 0x08
            && KU_KEY_CERT_SIGN == 0x04,
        "",
    );
    // 4) 用法→必需位判定：TLS 服务端需 0xA0；仅 0x80 显性不足。
    cs.add(
        "ku_tls_server_chain",
        required_ku(KuPurpose::TlsServer) == 0xA0 && ku_check(0xA0, KuPurpose::TlsServer).is_ok()
            && ku_check(0x80, KuPurpose::TlsServer) == Err("ku-insufficient"),
        "",
    );
    // 5) 验证链多节点：时间戳需 nonRepudiation（0x40），0xA0 不满足。
    cs.add(
        "ku_purpose_chain",
        ku_check(0x80, KuPurpose::TlsClient).is_ok() && ku_check(0x40, KuPurpose::TimeStamp).is_ok()
            && ku_check(0xA0, KuPurpose::TimeStamp) == Err("ku-insufficient"),
        "",
    );
    // 6) SAN DNS 大小写归一化入定长缓冲。
    let mut sl = SanList::new();
    let i0 = sl.add(SAN_KIND_DNS, "WWW.Example.COM").expect("合法 DNS 名必成");
    let e0 = sl.get(i0).expect("槽位必在");
    cs.add("san_dns_normalize", e0.kind == SanKind::Dns && &e0.dns[..e0.dns_len] == b"www.example.com", "");
    // 7) SAN IPv4 严格解析入四元组。
    let mut sl2 = SanList::new();
    let i1 = sl2.add(SAN_KIND_IP, "10.0.0.42").expect("合法 IP 必成");
    cs.add("san_ipv4", sl2.get(i1).expect("槽位必在").ip == [10, 0, 0, 42], "");
    // 8) 非法类型计数（既非 dNSName 也非 iPAddress）。
    let mut sl3 = SanList::new();
    cs.add(
        "san_illegal_type",
        sl3.add(9, "x") == Err("san-kind-illegal") && sl3.illegal_types == 1 && sl3.n == 0, "",
    );
    // 9) SAN 定长 16：装满后第 17 名显性拒绝并记账。
    let mut full = SanList::new();
    let mut ok = true;
    for k in 0..SAN_SLOTS + 1 {
        let r = full.add(SAN_KIND_DNS, "host.example");
        ok = ok && if k < SAN_SLOTS { r.is_ok() } else { r == Err("san-full") };
    }
    cs.add("san_capacity_ledger", ok && full.n == SAN_SLOTS && full.rejected == 1, "");
    // 10) 通配符边界：最左整标签放行；部分/非最左/多通配拒绝。
    cs.add(
        "wildcard_matrix",
        wildcard_valid("*.example.com").is_ok()
            && wildcard_valid("a*.example.com") == Err("wildcard-partial-label")
            && wildcard_valid("sub.*.example.com") == Err("wildcard-not-leftmost")
            && wildcard_valid("*.*.example.com") == Err("wildcard-multi"),
        "",
    );
    // 11) IDN 保留：通配符 × xn-- 组合拒绝；纯 xn-- 名不受影响。
    cs.add(
        "wildcard_idn_reserved",
        wildcard_valid("*.xn--p1ai") == Err("idn-reserved") && wildcard_valid("xn--p1ai").is_ok(),
        "",
    );
    // 12) SAN 空名显性拒绝（归一化入口的边界）。
    let mut sl4 = SanList::new();
    cs.add("san_empty_name_reject", sl4.add(SAN_KIND_DNS, "") == Err("san-name-invalid") && sl4.rejected == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_reject_matrix() {
        let cases = [
            ("a*.example.com", Err("wildcard-partial-label")),
            ("sub.*.example.com", Err("wildcard-not-leftmost")),
            ("*.*.example.com", Err("wildcard-multi")),
            ("*.xn--p1ai", Err("idn-reserved")),
        ];
        for (name, want) in cases {
            assert_eq!(wildcard_valid(name), want, "通配符矩阵必须与 RFC 6125 一致");
        }
    }

    #[test]
    fn eku_unknown_oid_is_none() {
        assert_eq!(eku_lookup("2.5.29.37.0"), None, "anyExtendedKeyUsage 不在本表语义面");
        assert_eq!(eku_lookup("1.3.6.1.5.5.7.3.4"), None, "emailProtection 不在四类表内");
    }

    #[test]
    fn san_ip_strict_rejects() {
        let mut sl = SanList::new();
        assert_eq!(sl.add(SAN_KIND_IP, "256.1.1.1"), Err("san-ip-octet"));
        assert_eq!(sl.add(SAN_KIND_IP, "1.2.3"), Err("san-ip-format"));
        assert_eq!(sl.illegal_types, 0);
        assert_eq!(sl.rejected, 0);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f024e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
