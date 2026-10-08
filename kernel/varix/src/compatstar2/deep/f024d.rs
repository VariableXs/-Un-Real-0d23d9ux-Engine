//! F024 深化批次二 · 通配符/证书时间/OCSP 缓存面（compatstar2/deep · G-A-24）。
//!
//! 批次一深化覆盖根名册/密钥用途位/链策略；本批补齐：SAN 通配符匹配
//! （单层语义——`*.example.com` 不跨级）、ASN.1 UTCTime 解析（证书有效期
//! 的字节级入口）、SHA-256 指纹十六进制展示（证书页「全串可复制」的格式面）、
//! OCSP 响应缓存（软失败策略的 TTL 承载）、PEM 头识别（F008 导入选择器的
//! 快速分流）。
//!
//! 零堆纪律：定长缓冲与表，无 alloc。

use crate::checks::CheckSet;

/// OCSP 缓存容量。
pub const OCSP_CACHE_SLOTS: usize = 16;
/// PEM 头签名。
pub const PEM_BEGIN: &[u8] = b"-----BEGIN";

/// SAN 通配符匹配（RFC 6125 / webpki 单层语义）：`*.example.com` 匹配
/// `a.example.com`；不匹配 `a.b.example.com`（跨级）与 `example.com`（缺标签）。
pub fn wildcard_match(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => {
            if !host.ends_with(suffix) {
                return false;
            }
            // 前缀 = 主机名去掉后缀的剩余段（含分隔点）：必须恰为
            // 「一个标签 + 点」——非空、以点收尾、点前无点。
            let prefix = &host[..host.len() - suffix.len()];
            match prefix.strip_suffix('.') {
                Some(label) => !label.is_empty() && !label.contains('.'),
                None => false,
            }
        }
        None => pattern == host, // 非通配 = 精确匹配
    }
}

/// ASN.1 UTCTime 解析（YYMMDDHHMMSSZ；YY < 50 → 20YY，否则 19YY——ASN.1 规则）。
/// 返回 epoch 秒。历法核复用 f022d（一处一事实：历法只有一份）。
pub fn asn1_utc_time_to_epoch(s: &[u8]) -> Option<i64> {
    if s.len() != 13 || s[12] != b'Z' {
        return None;
    }
    let num = |r: &[u8]| -> Option<i64> {
        if r.iter().any(|b| !b.is_ascii_digit()) {
            return None;
        }
        Some((r[0] - b'0') as i64 * 10 + (r[1] - b'0') as i64)
    };
    let yy = num(&s[0..2])?;
    let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
    let month = num(&s[2..4])? as u32;
    let day = num(&s[4..6])? as u32;
    let (hh, mi, ss) = (num(&s[6..8])?, num(&s[8..10])?, num(&s[10..12])?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hh > 23 || mi > 59 || ss > 60 {
        return None;
    }
    let days = crate::compatstar2::deep::f022d::days_from_civil(year, month, day);
    Some(days * 86_400 + hh * 3600 + mi * 60 + ss)
}

/// 指纹十六进制展示（冒号分隔大写——证书页格式面）：`AA:BB:…`。
/// 8 字节入 → 8×3-1 = 23 字符出。
pub fn fingerprint_hex(fp: &[u8; 8], out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0usize;
    for (i, &b) in fp.iter().enumerate() {
        if i > 0 {
            out[n] = b':';
            n += 1;
        }
        out[n] = HEX[(b >> 4) as usize];
        out[n + 1] = HEX[(b & 0xF) as usize];
        n += 2;
    }
    n
}

/// OCSP 响应缓存条目。
#[derive(Clone, Copy)]
pub struct OcspEntry {
    pub resp_hash: u64,
    pub ttl_s: u32,
    pub good: bool,
}

/// OCSP 响应缓存：命中复用、TTL 到期淘汰（软失败策略的离线承载）。
pub struct OcspCache {
    pub entries: [Option<OcspEntry>; OCSP_CACHE_SLOTS],
    pub count: usize,
    pub hits: u32,
}

impl OcspCache {
    pub const fn new() -> Self {
        OcspCache { entries: [None; OCSP_CACHE_SLOTS], count: 0, hits: 0 }
    }
    pub fn store(&mut self, resp_hash: u64, ttl_s: u32, good: bool) {
        for e in self.entries.iter_mut().take(self.count) {
            if let Some(x) = e {
                if x.resp_hash == resp_hash {
                    x.ttl_s = ttl_s; // 刷新
                    x.good = good;
                    return;
                }
            }
        }
        if self.count < OCSP_CACHE_SLOTS {
            self.entries[self.count] = Some(OcspEntry { resp_hash, ttl_s, good });
            self.count += 1;
        }
    }
    /// 查询：命中且未过期 → Some(good)；过期淘汰；未命中 → None。
    pub fn lookup(&mut self, resp_hash: u64) -> Option<bool> {
        for i in 0..self.count {
            if let Some(x) = self.entries[i] {
                if x.resp_hash == resp_hash {
                    if x.ttl_s > 0 {
                        self.hits += 1;
                        return Some(x.good);
                    }
                    self.entries[i] = None; // 到期淘汰
                    // 压缩（滑窗，保序）。
                    for j in i..self.count - 1 {
                        self.entries[j] = self.entries[j + 1];
                    }
                    self.entries[self.count - 1] = None;
                    self.count -= 1;
                    return None;
                }
            }
        }
        None
    }
    pub fn tick(&mut self, dt_s: u32) {
        for e in self.entries.iter_mut().flatten() {
            e.ttl_s = e.ttl_s.saturating_sub(dt_s);
        }
    }
}

/// PEM 头快速识别（导入分流：PEM 走文本解析、DER 走二进制——F008 面）。
pub fn is_pem(bytes: &[u8]) -> bool {
    bytes.len() >= PEM_BEGIN.len() && &bytes[..PEM_BEGIN.len()] == PEM_BEGIN
}

/// 域自检（深化批次二）。
pub fn run_f024d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F024-tlsstore-d2");
    // 1) 通配符四态：单层命中、跨级拒、裸域拒、精确匹配。
    cs.add(
        "wildcard_single_level",
        wildcard_match("*.example.com", "a.example.com")
            && !wildcard_match("*.example.com", "a.b.example.com")
            && !wildcard_match("*.example.com", "example.com")
            && wildcard_match("cdn.example.com", "cdn.example.com"),
        "",
    );
    // 2) ASN.1 时间：2026-09-27 12:00:00Z 解析成功且历法回读一致（闰年后回
    //    读到同一天——历法核对拍）。
    let t = asn1_utc_time_to_epoch(b"260927120000Z").expect("合法 UTCTime");
    let (_, m, d) = crate::compatstar2::deep::f022d::civil_from_days(t.div_euclid(86_400));
    cs.add("asn1_time_parse", (m, d) == (9, 27) && t.rem_euclid(86_400) == 12 * 3600, "");
    // 3) ASN.1 非法：长度错/非 Z 尾/月 13。
    cs.add(
        "asn1_time_reject",
        asn1_utc_time_to_epoch(b"26092712000Z").is_none()
            && asn1_utc_time_to_epoch(b"260927120000X").is_none()
            && asn1_utc_time_to_epoch(b"261327120000Z").is_none(),
        "",
    );
    // 4) 指纹格式：8 字节 → 23 字符冒号大写。
    let mut buf = [0u8; 32];
    let n = fingerprint_hex(&[0xDE, 0xAD, 0xBE, 0xEF, 0, 1, 2, 3], &mut buf);
    cs.add("fingerprint_hex_shape", n == 23 && &buf[..5] == b"DE:AD", "");
    // 5) OCSP 缓存：存→命中→TTL 到期淘汰→未命中。
    let mut oc = OcspCache::new();
    oc.store(0xFEED, 10, true);
    let hit = oc.lookup(0xFEED) == Some(true);
    oc.tick(11);
    cs.add("ocsp_cache_ttl", hit && oc.lookup(0xFEED).is_none() && oc.count == 0 && oc.hits == 1, "");
    // 6) PEM 识别。
    cs.add("pem_detect", is_pem(b"-----BEGIN CERTIFICATE-----") && !is_pem(b"\x30\x82\x03\x21"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_edge_labels() {
        assert!(!wildcard_match("*.example.com", ".example.com"), "空前缀不匹配（不吞裸域）");
        assert!(!wildcard_match("*.com", "example.org"), "后缀不齐即拒");
        assert!(wildcard_match("*.a.example.com", "x.a.example.com"), "通配符本身可含多级后缀");
    }

    #[test]
    fn asn1_yy_pivot() {
        // YY=49 → 2049；YY=51 → 1951（ASN.1 分界规则 50）。
        let t49 = asn1_utc_time_to_epoch(b"490101000000Z").unwrap();
        let (y, _, _) = crate::compatstar2::deep::f022d::civil_from_days(t49 / 86_400);
        assert_eq!(y, 2049);
        let t51 = asn1_utc_time_to_epoch(b"510101000000Z").unwrap();
        let (y2, _, _) = crate::compatstar2::deep::f022d::civil_from_days(t51 / 86_400);
        assert_eq!(y2, 1951);
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f024d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
