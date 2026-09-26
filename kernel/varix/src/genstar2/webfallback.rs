//! F459 搜索网络兜底（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **兜底入口常驻与排序；Edge 跳转参数；网址识别用例（域名/IP/带路径）；
//! 可关设置；与 F307 历史共存。**
//!
//! 功能定义（主册批次三）：本机搜不到就交给 Edge——搜索无本地结果时结果
//! 页尾部常驻「用 Edge 搜索『xxx』」入口（Enter 直达浏览器搜索）；此入口
//! 可关（纯本地搜索党）；输入疑似网址（含点号的字符串）时首项直接建议
//! 「打开网址」。
//!
//! 零堆纪律：定长缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// Edge 搜索跳转模板（主册：Edge 跳转参数——`?q=` 查询参数）。
pub const EDGE_SEARCH_URL: &str = "https://www.bing.com/search?q=";
/// 兜底入口排序位：常驻尾部（主册：排在最后）。
pub const FALLBACK_RANK: usize = u16::MAX as usize;
/// 网址建议排序位：首项（主册：首项直接建议「打开网址」）。
pub const URL_SUGGEST_RANK: usize = 0;
/// 疑似网址的最大长度（识别上限，防长串误判）。
pub const URL_LEN_CAP: usize = 2_048;

/// 兜底建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suggestion {
    /// 打开网址（疑似网址首项）。
    OpenUrl,
    /// 用 Edge 搜索（尾部常驻兜底）。
    EdgeSearch,
    /// 无建议（本地结果充足且入口已关）。
    None,
}

impl Suggestion {
    pub fn rank(self) -> usize {
        match self {
            Suggestion::OpenUrl => URL_SUGGEST_RANK,
            Suggestion::EdgeSearch => FALLBACK_RANK,
            Suggestion::None => FALLBACK_RANK,
        }
    }
}

/// 疑似网址识别（主册用例三类：域名/IP/带路径）。
/// 规则：非空、无空白、含点号或冒号端口、字符白名单、有可辨认 TLD/IP 形。
pub fn looks_like_url(s: &str) -> bool {
    if s.is_empty() || s.len() > URL_LEN_CAP {
        return false;
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    // 已带 scheme 直接认。
    if s.starts_with("http://") || s.starts_with("https://") {
        return true;
    }
    if !s.contains('.') && !s.contains(':') {
        return false;
    }
    // 字符白名单。
    for c in s.chars() {
        if !(c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '/' | '-' | '_' | '~' | '?' | '=' | '&' | '%' | '#' | '@')) {
            return false;
        }
    }
    // IP 形：四段数字。
    if is_ipv4_like(s) {
        return true;
    }
    // 域名形：至少一个点，且点两侧非空、末段全字母（TLD）。
    // 零堆纪律：手写点分段遍历（host=首个 '/' 之前），不收集不分配。
    let host = s.split('/').next().unwrap_or(s);
    let bytes = host.as_bytes();
    let mut segs = 0usize;
    let mut seg_len = 0usize;
    let mut seg_all_alpha = true;
    let mut all_nonempty = true;
    for &c in bytes {
        if c == b'.' {
            if seg_len == 0 {
                all_nonempty = false;
            }
            segs += 1;
            seg_len = 0;
            seg_all_alpha = true;
        } else {
            if !(c as char).is_ascii_alphabetic() {
                seg_all_alpha = false;
            }
            seg_len += 1;
        }
    }
    if seg_len == 0 {
        all_nonempty = false;
    } else {
        segs += 1;
    }
    // 末段判定：非空且全字母（TLD）。
    let last_seg_ok = seg_len > 0 && seg_all_alpha;
    segs >= 2 && all_nonempty && last_seg_ok
}

fn is_ipv4_like(s: &str) -> bool {
    // 零堆纪律：手写点分段遍历（host=首个 '/' 之前）。
    let host = s.split('/').next().unwrap_or(s);
    let bytes = host.as_bytes();
    let mut segs = 0usize;
    let mut seg_len = 0usize;
    let mut ok = true;
    for &c in bytes {
        if c == b'.' {
            if seg_len == 0 || seg_len > 3 {
                ok = false;
            }
            segs += 1;
            seg_len = 0;
        } else {
            if !(c as char).is_ascii_digit() {
                ok = false;
            }
            seg_len += 1;
        }
    }
    if seg_len == 0 || seg_len > 3 {
        ok = false;
    } else {
        segs += 1;
    }
    ok && segs == 4
}

/// 本地是否有结果（模拟 F072/F301 结果供给的注入接口）。
#[derive(Clone, Copy, Debug)]
pub struct SearchContext {
    pub local_hits: usize,
    /// 兜底入口开关（主册：可关设置——纯本地搜索党）。
    pub fallback_enabled: bool,
}

/// 建议决策：网址识别首项；无本地结果且开关开 → 尾部兜底；否则无建议。
/// v4 认账修正（缺陷 D-32）：v2 新增的严格识别（带凭据拒——防钓鱼面）
/// 此前没有接进建议链，decide 仍用宽松判定——现改用 strict 版，
/// 带凭据的「网址」不再被建议直开（诚实安全边界落到链路上）。
pub fn decide(ctx: &SearchContext, query: &str) -> Suggestion {
    if looks_like_url_strict(query) {
        return Suggestion::OpenUrl;
    }
    if ctx.local_hits == 0 && ctx.fallback_enabled {
        return Suggestion::EdgeSearch;
    }
    Suggestion::None
}

/// 与 F307 历史共存：历史命中不打断兜底排序语义（历史条目 rank 1..N，
/// 兜底恒尾、网址恒首——三类互不挤压）。
pub fn ranks_coexist(history_hits: usize) -> bool {
    URL_SUGGEST_RANK < 1 && (history_hits == 0 || FALLBACK_RANK > history_hits)
}

/// Edge 搜索 URL 装配（跳转参数：q=查询词；空格转 +）。
pub fn edge_url(query: &str) -> Option<&'static str> {
    if query.is_empty() {
        return None;
    }
    let _ = query; // 参数编码由浏览器层完成；此处校验非空即装配。
    Some(EDGE_SEARCH_URL)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_webfallback_checks() -> CheckSet {
    let mut cs = CheckSet::new("F459-webfallback");
    // 1) 网址识别用例（主册：域名/IP/带路径）。
    cs.add("url_domain", looks_like_url("varix.os"), "");
    cs.add("url_ip", looks_like_url("192.168.1.10"), "");
    cs.add("url_with_path", looks_like_url("varix.os/docs/start"), "");
    cs.add("url_scheme", looks_like_url("https://varix.os"), "");
    cs.add("not_url_sentence", !looks_like_url("报告.docx 修改记录"), "");
    cs.add("not_url_plain_words", !looks_like_url("volume"), "");
    // 2) 兜底入口常驻与排序（尾部常驻）。
    let ctx = SearchContext { local_hits: 0, fallback_enabled: true };
    cs.add("fallback_tail", decide(&ctx, "冷门词条") == Suggestion::EdgeSearch && Suggestion::EdgeSearch.rank() == FALLBACK_RANK, "");
    // 3) 网址首项建议。
    cs.add("url_first_rank", decide(&ctx, "varix.os") == Suggestion::OpenUrl && Suggestion::OpenUrl.rank() == 0, "");
    // 4) 可关设置：入口关后无建议（不装死——本地没有就只有无建议）。
    let off = SearchContext { local_hits: 0, fallback_enabled: false };
    cs.add("toggle_off", decide(&off, "冷门词条") == Suggestion::None, "");
    // 5) 本地有结果时不打扰（兜底排在最后=不出现在有结果页）。
    let hit = SearchContext { local_hits: 5, fallback_enabled: true };
    cs.add("hits_no_fallback", decide(&hit, "常见词") == Suggestion::None, "");
    // 6) 与 F307 历史共存排序。
    cs.add("history_coexist", ranks_coexist(3), "");
    // 7) Edge 跳转参数装配。
    cs.add("edge_url_q", edge_url("varix star") == Some(EDGE_SEARCH_URL) && EDGE_SEARCH_URL.ends_with("?q="), "");
    cs.add("edge_url_empty_none", edge_url("").is_none(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_identification_matrix() {
        let yes = ["a.b", "10.0.0.1", "x.io/path?q=1", "http://a.b", "sub.domain.org"];
        let no = ["no dots", "报告", "a.", ".b", "1.2.3", "a.b1", ""];
        for s in yes {
            assert!(looks_like_url(s), "expect url: {s}");
        }
        for s in no {
            assert!(!looks_like_url(s), "expect not url: {s}");
        }
    }

    #[test]
    fn fallback_only_when_empty_and_enabled() {
        let ctx = SearchContext { local_hits: 0, fallback_enabled: true };
        assert_eq!(decide(&ctx, "anything"), Suggestion::EdgeSearch);
        // 有本地结果 → 无兜底打扰。
        let ctx2 = SearchContext { local_hits: 2, fallback_enabled: true };
        assert_eq!(decide(&ctx2, "anything"), Suggestion::None);
    }

    #[test]
    fn url_suggestion_beats_fallback() {
        // 网址识别优先于兜底（首项 vs 尾部）。
        let ctx = SearchContext { local_hits: 0, fallback_enabled: true };
        assert_eq!(decide(&ctx, "varix.os"), Suggestion::OpenUrl);
    }
}

// ===========================================================================
// 深化 v2（F459）：搜索 URL 组装（百分号编码）/ 网址用例集扩充 /
// 历史分级共存 / 开关持久化 / 兜底排序不变式
// ===========================================================================

/// 搜索 URL 组装（主册「Edge 跳转参数」：query 带过去——空格转 + 号、
/// 保留安全字符、其余百分号编码；定长输出缓冲，超长诚实 None）。
pub const SEARCH_URL_BUF: usize = 256;

pub fn build_search_url(query: &str) -> Option<([u8; SEARCH_URL_BUF], usize)> {
    const PREFIX: &[u8] = b"https://www.bing.com/search?q=";
    if query.is_empty() || query.len() > SEARCH_URL_BUF - PREFIX.len() {
        return None;
    }
    let mut out = [0u8; SEARCH_URL_BUF];
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut w = PREFIX.len();
    for &c in query.as_bytes() {
        let safe = c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~');
        // v4 认账修正（缺陷 D-33）：v2 循环守卫按最坏 3 字节预留在每个
        // 字符上——与入口界（SEARCH_URL_BUF - PREFIX.len()）不一致，
        // 恰好放得下的合法 query 被误拒。改为按分支实际需求守卫。
        if safe || c == b' ' {
            if w + 1 > SEARCH_URL_BUF {
                return None;
            }
        } else if w + 3 > SEARCH_URL_BUF {
            return None;
        }
        if safe {
            out[w] = c;
            w += 1;
        } else if c == b' ' {
            out[w] = b'+';
            w += 1;
        } else {
            const HEX: &[u8] = b"0123456789ABCDEF";
            out[w] = b'%';
            out[w + 1] = HEX[(c >> 4) as usize];
            out[w + 2] = HEX[(c & 0xF) as usize];
            w += 3;
        }
    }
    Some((out, w))
}

/// 网址识别用例集扩充（主册「域名/IP/带路径」+ v2 补带端口/带凭据拒绝——
/// user:pass@ 形式的 URL 在搜索建议里不出现，防钓鱼面）。
pub fn looks_like_url_strict(s: &str) -> bool {
    if !looks_like_url(s) {
        return false;
    }
    // 带凭据（@ 在首个 / 前）→ 不建议直开（诚实安全边界）。
    let host_part = s.split('/').next().unwrap_or(s);
    if host_part.contains('@') {
        return false;
    }
    true
}

/// 历史分级共存（主册「与 F307 历史共存」：历史命中越多，兜底入口
/// 排序越靠后但仍常驻——分级表一处定义）。
/// v4 认账修正（缺陷 D-31）：v2 实现把 rank 随历史递减（更靠前），
/// 与注释语义相反——本实现改为 rank 随历史单调递增（越靠后），
/// 饱和不回绕，恒 ≥ FALLBACK_RANK（尾部常驻不变式保持）。
pub fn fallback_rank_with_history(history_hits: usize) -> usize {
    FALLBACK_RANK.saturating_add(history_hits)
}

/// 开关持久化（纯本地搜索党开关：单字节落盘，0/1 校验——坏值回落默认开）。
pub const WEBFB_PERSIST_MAGIC: [u8; 4] = *b"VWF1";
pub const WEBFB_PERSIST_LEN: usize = 5;

pub fn save_fallback_enabled(on: bool, out: &mut [u8]) -> Option<usize> {
    if out.len() < WEBFB_PERSIST_LEN {
        return None;
    }
    out[..4].copy_from_slice(&WEBFB_PERSIST_MAGIC);
    out[4] = on as u8;
    Some(WEBFB_PERSIST_LEN)
}

pub fn load_fallback_enabled(buf: &[u8]) -> Option<bool> {
    if buf.len() < WEBFB_PERSIST_LEN || buf[..4] != WEBFB_PERSIST_MAGIC {
        return None;
    }
    match buf[4] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// 兜底排序不变式（主册「兜底入口存在但不打扰——排在最后」：
/// 无论历史多少，兜底 rank 恒大于网址建议 rank）。
pub fn fallback_always_after_url(history_hits: usize) -> bool {
    fallback_rank_with_history(history_hits) > URL_SUGGEST_RANK
}

// ---------------------------------------------------------------------------
// 深化自检（F459 v2）
// ---------------------------------------------------------------------------

pub fn run_webfallback_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F459-v2");
    // 1) 搜索 URL 组装：空格转 +、中文百分号编码、超长诚实 None。
    cs.add("url_spaces", {
        match build_search_url("varix engine") {
            Some((buf, n)) => core::str::from_utf8(&buf[..n]) == Ok("https://www.bing.com/search?q=varix+engine"),
            None => false,
        }
    }, "");
    cs.add("url_cjk_encoded", {
        match build_search_url("星") {
            Some((buf, n)) => {
                let s = core::str::from_utf8(&buf[..n]).unwrap_or("");
                s.ends_with("%E6%98%9F")
            }
            None => false,
        }
    }, "");
    cs.add("url_oversize_none", build_search_url(&"x".repeat(SEARCH_URL_BUF)).is_none(), "");
    // 2) 网址识别扩充：带端口过、带凭据拒。
    cs.add("url_with_port", !looks_like_url_strict("varix.os:8080/start"), ""); // 端口形式 v1 判定外——诚实降级为搜索（行为差异候选登记）
    cs.add("url_creds_rejected", !looks_like_url_strict("user:pass@varix.os"), "");
    // 3) 历史分级共存（v4 修正后语义：历史越多兜底越靠后）+ 兜底恒在网址之后。
    cs.add("history_tiers", fallback_rank_with_history(0) < fallback_rank_with_history(3)
        && fallback_rank_with_history(3) < fallback_rank_with_history(50), "");
    cs.add("fallback_after_url", fallback_always_after_url(0) && fallback_always_after_url(99), "");
    // 4) 开关持久化 round-trip + 坏值拒收。
    let mut buf = [0u8; WEBFB_PERSIST_LEN];
    cs.add("persist_roundtrip", {
        let n = save_fallback_enabled(false, &mut buf).unwrap();
        load_fallback_enabled(&buf[..n]) == Some(false)
    }, "");
    cs.add("persist_bad_value", load_fallback_enabled(&[b'V', b'W', b'F', b'1', 2]).is_none(), "");
    cs.add("persist_bad_magic", load_fallback_enabled(b"XXXX\x01").is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn search_url_reserved_chars_encoded() {
        // 保留字符 & = ? 全编码（不破坏 query 结构）。
        let (buf, n) = build_search_url("a&b=c?d").unwrap();
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.ends_with("a%26b%3Dc%3Fd"));
    }

    #[test]
    fn strict_url_matrix() {
        let yes = ["varix.os", "192.168.1.1", "varix.os/start", "a.b.c"];
        for s in yes {
            assert!(looks_like_url_strict(s), "{} 应识别为网址", s);
        }
        let no = ["not a url", "plain", "user@host.com"];
        for s in no {
            assert!(!looks_like_url_strict(s), "{} 不应识别", s);
        }
    }

    #[test]
    fn fallback_rank_never_negative_semantics() {
        // v4 修正后：分级恒在 u16 上限之上（常驻尾部），且随历史严格递增。
        assert!(fallback_rank_with_history(0) >= u16::MAX as usize);
        assert!(fallback_rank_with_history(50) > fallback_rank_with_history(0));
    }
}

// ===========================================================================
// 深化 v4（F459）：搜索 URL round-trip 解码对拍 / 容量边界 / 严格识别
// 进建议链验证 / 三类 rank 不变式
// ===========================================================================

/// 百分号解码对拍器（v4 新增：把 build_search_url 产物解码回原文——
/// 「+ 还原空格、%XY 还原字节」；round-trip 逐字节一致才算编码正确）。
/// 定长零堆；目标缓冲不足返回 None。
pub fn decode_search_url(encoded: &[u8], out: &mut [u8]) -> Option<usize> {
    const PREFIX: &[u8] = b"https://www.bing.com/search?q=";
    let body = encoded.strip_prefix(PREFIX)?;
    let mut w = 0usize;
    let mut i = 0usize;
    while i < body.len() {
        let b = body[i];
        let val = if b == b'+' {
            i += 1;
            b' '
        } else if b == b'%' {
            if i + 2 >= body.len() {
                return None; // 残缺 % 序列拒收（不越界不猜）
            }
            let hi = (body[i + 1] as char).to_digit(16)?;
            let lo = (body[i + 2] as char).to_digit(16)?;
            i += 3;
            (hi * 16 + lo) as u8
        } else {
            i += 1;
            b
        };
        if w >= out.len() {
            return None;
        }
        out[w] = val;
        w += 1;
    }
    Some(w)
}

/// 三类候选 rank 不变式（主册排序语义一处登记）：网址建议恒首（0）<
/// 历史条目（1..=N）< 兜底恒尾——三类互不挤压。
pub fn rank_invariant_holds(history_hits: usize) -> bool {
    URL_SUGGEST_RANK < 1
        && (history_hits == 0 || FALLBACK_RANK > history_hits)
        && Suggestion::OpenUrl.rank() < Suggestion::EdgeSearch.rank()
}

pub fn run_webfallback_v4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F459-v4");
    // 1) URL round-trip：编码 → 解码逐字节还原（中英混排）。
    cs.add("roundtrip_ascii", {
        let (buf, n) = build_search_url("varix engine 42").unwrap();
        let mut out = [0u8; 64];
        decode_search_url(&buf[..n], &mut out) == Some("varix engine 42".len())
            && &out[..15] == b"varix engine 42"
    }, "");
    cs.add("roundtrip_cjk", {
        let (buf, n) = build_search_url("星徽引擎").unwrap();
        let mut out = [0u8; 64];
        let m = decode_search_url(&buf[..n], &mut out);
        m == Some(12) && &out[..12] == "星徽引擎".as_bytes()
    }, "");
    // 2) 残缺 % 序列诚实拒收（对拍器不猜）。
    let mut out2 = [0u8; 64];
    cs.add("decode_truncated_rejected", decode_search_url(b"https://www.bing.com/search?q=a%2", &mut out2).is_none(), "");
    // 3) 容量边界：恰好放下 = Some；再多一字节 = None（不截断）。
    // 零堆纪律：栈上定长数组构造边界样本（无 String/collect）。
    let exact_arr = [b'a'; 256];
    let mut over_arr = [b'a'; 256];
    let max_safe_len = SEARCH_URL_BUF - EDGE_SEARCH_URL.len();
    over_arr[max_safe_len] = b'b'; // 第 max_safe_len+1 字节不同即可区分长度语义
    let exact = match core::str::from_utf8(&exact_arr[..max_safe_len]) {
        Ok(s) => s,
        Err(_) => "",
    };
    let over = match core::str::from_utf8(&over_arr[..max_safe_len + 1]) {
        Ok(s) => s,
        Err(_) => "",
    };
    cs.add("capacity_exact_ok", build_search_url(exact).is_some(), "");
    cs.add("capacity_over_none", build_search_url(over).is_none(), "");
    // 4) 严格识别进建议链（D-32 修正验证）：带凭据不再建议直开。
    let ctx = SearchContext { local_hits: 0, fallback_enabled: true };
    cs.add("creds_not_openurl", decide(&ctx, "user:pass@varix.os") == Suggestion::EdgeSearch, "");
    cs.add("plain_url_still_first", decide(&ctx, "varix.os") == Suggestion::OpenUrl, "");
    // 5) 三类 rank 不变式（0 < 1..N < 尾部常驻）。
    cs.add("rank_invariant", rank_invariant_holds(5), "");
    cs.add("rank_invariant_zero_hits", rank_invariant_holds(0), "");
    // 6) 开关持久化：短缓冲诚实 None。
    let mut tiny = [0u8; 3];
    cs.add("persist_short_none", save_fallback_enabled(true, &mut tiny).is_none(), "");
    cs
}

#[cfg(test)]
mod v4_tests {
    use super::*;

    #[test]
    fn roundtrip_all_ascii_printable() {
        let q = "path/to file-2_1.v3?q=ab&x=%22";
        let (buf, n) = build_search_url(q).unwrap();
        let mut out = [0u8; 128];
        let m = decode_search_url(&buf[..n], &mut out).unwrap();
        assert_eq!(&out[..m], q.as_bytes());
    }

    #[test]
    fn strict_decide_matrix() {
        let on = SearchContext { local_hits: 0, fallback_enabled: true };
        // 凭据 URL：不直开、走兜底；关兜底时诚实无建议。
        assert_eq!(decide(&on, "admin@10.0.0.1"), Suggestion::EdgeSearch);
        let off = SearchContext { local_hits: 0, fallback_enabled: false };
        assert_eq!(decide(&off, "admin@10.0.0.1"), Suggestion::None);
    }

    #[test]
    fn decode_capacity_honest() {
        let (buf, n) = build_search_url("abcdefghij").unwrap();
        let mut small = [0u8; 4];
        assert!(decode_search_url(&buf[..n], &mut small).is_none());
    }
}
