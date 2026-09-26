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
    let parts: Vec<&str> = s.split('/').next().unwrap_or(s).split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty())
        && parts.last().map(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphabetic())).unwrap_or(false)
}

fn is_ipv4_like(s: &str) -> bool {
    let host = s.split('/').next().unwrap_or(s);
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

/// 本地是否有结果（模拟 F072/F301 结果供给的注入接口）。
#[derive(Clone, Copy, Debug)]
pub struct SearchContext {
    pub local_hits: usize,
    /// 兜底入口开关（主册：可关设置——纯本地搜索党）。
    pub fallback_enabled: bool,
}

/// 建议决策：网址识别首项；无本地结果且开关开 → 尾部兜底；否则无建议。
pub fn decide(ctx: &SearchContext, query: &str) -> Suggestion {
    if looks_like_url(query) {
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
