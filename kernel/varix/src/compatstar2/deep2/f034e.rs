//! F034 深化批次三 · 双字节码页引擎面（compatstar2/deep2 · G-A-34）。
//!
//! 批次一/批次二已覆盖 F034 的协议语义面与执行治理面；本批补主册
//! 【功能定义】「全语义对齐」的执行/边界/注入面：lead-byte 区间状态机
//! （GBK 双轨：首字节 0x81-0xFE、次字节 0x40-0xFE 除 0x7F；Big5 双轨：
//! 首字节 0x81-0xFE、次字节 0x40-0x7E 与 0xA1-0xFE——非法次字节检出
//! 计数）、转换表驱动模型（双字节→Unicode 稠密段查表，登记真实段与
//! GB2312 汉字覆盖区）、替换策略统计账（不可映射字节→U+FFFD 逐字节
//! 计损，损失率 permille）、BOM 嗅探优先级表（FF FE 前缀先按 32 位
//! BOM 探测的歧义处理）。
//!
//! 判据对账：深化以主册【设计细节】「替换字符计数超全文 1% 时警告」、
//! 【状态与异常】「转换不可逆字符 → U+FFFD + 日志（乱码显式可见不静默
//! 吞）」未落地面为源，一处一事实（GBK/Big5 双字节区间规范、Unicode
//! 稠密段锚点、MS BOM 语义对拍）。
//!
//! 零堆纪律：定长稠密段表/扫描账，无 alloc。

use crate::checks::CheckSet;

// 常量（一处一事实：区间值取自 GBK/Big5 规范）

/// GBK 首字节区间 0x81-0xFE（GBK 规范）。
pub const GBK_LEAD_MIN: u8 = 0x81;
pub const GBK_LEAD_MAX: u8 = 0xFE;
/// GBK 次字节区间 0x40-0xFE，剔除 0x7F（GBK 规范）。
pub const GBK_TRAIL_MIN: u8 = 0x40;
pub const GBK_TRAIL_MAX: u8 = 0xFE;
pub const GBK_TRAIL_EXCLUDED: u8 = 0x7F;
/// Big5 次字节双轨：0x40-0x7E 与 0xA1-0xFE（Big5 规范；0x7F-0xA0 为隙）。
pub const BIG5_TRAIL_LOW_MIN: u8 = 0x40;
pub const BIG5_TRAIL_LOW_MAX: u8 = 0x7E;
pub const BIG5_TRAIL_HIGH_MIN: u8 = 0xA1;
pub const BIG5_TRAIL_HIGH_MAX: u8 = 0xFE;
/// 替换字符（MS 多字节转换不可映射语义：乱码显式可见）。
pub const REPLACEMENT: u32 = 0xFFFD;
/// 损失告警线（主册【设计细节】：替换计数超全文 1% 警告——10‰）。
pub const LOSS_WARN_PERMILLE: u32 = 10;
/// GB2312 一级汉字区：0xB0A1 起拼音序 3755 字；二级区 0xD8A1 起部首序
/// 3008 字（GB2312 规范）。
pub const LEVEL1_HANZI_BASE: u16 = 0xB0A1;
pub const LEVEL1_HANZI_END: u16 = 0xD7F3;
pub const LEVEL1_HANZI_COUNT: u32 = 3755;
pub const LEVEL2_HANZI_BASE: u16 = 0xD8A1;
pub const LEVEL2_HANZI_END: u16 = 0xF7FE;

/// 稠密线性段（锚点一处一事实）：段内 code-base 线性映射到 Unicode。
pub struct DenseSegment {
    pub gbk_base: u16,
    pub uni_base: u32,
    pub len: u16,
}

/// 登记的真实稠密段：
/// - 0xA3A1 起 94 格全角 ASCII → U+FF01..U+FF5E（GB2312 区位 3 行，
///   全角形与 Unicode 全角块线性对应）。
/// - 0xA6A1 起 17 格希腊大写 Α..Ρ → U+0391..U+03A1（U+03A2 未指派故断）。
/// - 0xA7A1 起 32 格西里尔大写 А..Я → U+0410..U+042F。
pub const DENSE_SEGMENTS: [DenseSegment; 3] = [
    DenseSegment { gbk_base: 0xA3A1, uni_base: 0xFF01, len: 94 },
    DenseSegment { gbk_base: 0xA6A1, uni_base: 0x0391, len: 17 },
    DenseSegment { gbk_base: 0xA7A1, uni_base: 0x0410, len: 32 },
];

// lead-byte 区间状态机

/// 首字节判定（GBK/Big5 同区间 0x81-0xFE）。
pub fn is_lead(b: u8) -> bool { (GBK_LEAD_MIN..=GBK_LEAD_MAX).contains(&b) }

/// GBK 次字节合法性：0x40-0xFE 且非 0x7F。
pub fn gbk_trail_ok(b: u8) -> bool {
    (GBK_TRAIL_MIN..=GBK_TRAIL_MAX).contains(&b) && b != GBK_TRAIL_EXCLUDED
}

/// Big5 次字节合法性：双轨 0x40-0x7E ∪ 0xA1-0xFE。
pub fn big5_trail_ok(b: u8) -> bool {
    (BIG5_TRAIL_LOW_MIN..=BIG5_TRAIL_LOW_MAX).contains(&b)
        || (BIG5_TRAIL_HIGH_MIN..=BIG5_TRAIL_HIGH_MAX).contains(&b)
}

/// 双轨扫描账：完整对/非法次字节/孤首字节三分类计数——非法显性入账，
/// 零静默吞。gbk_track = true 走 GBK 轨，false 走 Big5 轨。
pub struct DoubleByteScanner {
    pub gbk_track: bool,
    pub complete_pairs: u32,
    pub illegal_trails: u32,
    pub lone_leads: u32,
}

impl DoubleByteScanner {
    pub const fn new(gbk_track: bool) -> Self {
        DoubleByteScanner { gbk_track, complete_pairs: 0, illegal_trails: 0, lone_leads: 0 }
    }

    /// 扫描字节流：lead 后跟合法次字节 → 完整对；lead 后跟非法次字节 →
    /// 检出计数（双轨判据差异在此显性化）；流尾孤 lead → 孤字节账。
    pub fn scan(&mut self, bytes: &[u8]) {
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if is_lead(b) {
                if i + 1 < bytes.len() {
                    let t = bytes[i + 1];
                    let ok = if self.gbk_track { gbk_trail_ok(t) } else { big5_trail_ok(t) };
                    if ok { self.complete_pairs += 1; } else { self.illegal_trails += 1; }
                    i += 2;
                } else {
                    self.lone_leads += 1;
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
}

// 转换表驱动模型（稠密段查表 + 汉字覆盖区）

/// 稠密段查表：命中 → 段内线性索引取 Unicode；未命中 → None（替换
/// 策略接管）。汉字覆盖区不参与线性映射（区序≠Unicode 线性序，如实
/// 分离——线性谎报比查不到更危险）。
pub fn double_to_unicode(code: u16) -> Option<u32> {
    for s in DENSE_SEGMENTS.iter() {
        if code >= s.gbk_base && code - s.gbk_base < s.len {
            return Some(s.uni_base + (code - s.gbk_base) as u32);
        }
    }
    None
}

/// GB2312 一级汉字区判定（0xB0A1..0xD7F3，拼音序 3755 字）。
pub fn is_level1_hanzi(code: u16) -> bool { (LEVEL1_HANZI_BASE..=LEVEL1_HANZI_END).contains(&code) }

/// GB2312 二级汉字区判定（0xD8A1..0xF7FE，部首序 3008 字）。
pub fn is_level2_hanzi(code: u16) -> bool { (LEVEL2_HANZI_BASE..=LEVEL2_HANZI_END).contains(&code) }

// 替换策略统计账

/// 转换账：逐转换单元（ASCII 字节 / 双字节对 / 非法单元）计损。
pub struct ConversionStats {
    pub total_units: u32,
    pub converted: u32,
    pub unmappable: u32,
}

impl ConversionStats {
    pub const fn new() -> Self {
        ConversionStats { total_units: 0, converted: 0, unmappable: 0 }
    }

    /// 记一个转换单元：Some(码位) → 转换成功；None → U+FFFD 计损。
    pub fn record(&mut self, mapped: Option<u32>) {
        self.total_units += 1;
        match mapped {
            Some(_) => self.converted += 1,
            None => self.unmappable += 1,
        }
    }

    /// 损失率 permille（全文口径）。
    pub fn loss_permille(&self) -> u32 {
        if self.total_units == 0 { 0 } else { self.unmappable * 1000 / self.total_units }
    }

    /// 告警判定：损失率**超过** 1%（主册「超全文 1%」为严格大于）。
    pub fn loss_warn(&self) -> bool { self.loss_permille() > LOSS_WARN_PERMILLE }
}

/// 码页引擎：扫描账 + 转换账合一（ASCII 直通，双字节对查表，其余计损）。
pub struct CodePageEngine {
    pub scanner: DoubleByteScanner,
    pub stats: ConversionStats,
}

impl CodePageEngine {
    pub const fn new(gbk_track: bool) -> Self {
        CodePageEngine { scanner: DoubleByteScanner::new(gbk_track), stats: ConversionStats::new() }
    }

    /// 流式转换：ASCII（<0x80）直通；lead+合法 trail → 查表（未命中按
    /// 替换计损）；非法 trail / 孤 lead → 替换计损。账面与流逐单元对齐。
    pub fn process(&mut self, bytes: &[u8]) {
        self.scanner.scan(bytes);
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.stats.record(Some(b as u32));
                i += 1;
            } else if i + 1 < bytes.len() {
                let t = bytes[i + 1];
                let ok = if self.scanner.gbk_track { gbk_trail_ok(t) } else { big5_trail_ok(t) };
                if ok {
                    let code = ((b as u16) << 8) | t as u16;
                    self.stats.record(double_to_unicode(code));
                } else {
                    self.stats.record(None);
                }
                i += 2;
            } else {
                self.stats.record(None);
                i += 1;
            }
        }
    }
}

// BOM 嗅探优先级表

/// BOM 字节序（MS BOM 语义，一处一事实）。
pub const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];
pub const BOM_UTF32LE: [u8; 4] = [0xFF, 0xFE, 0x00, 0x00];
pub const BOM_UTF32BE: [u8; 4] = [0x00, 0x00, 0xFE, 0xFF];
pub const BOM_UTF16LE: [u8; 2] = [0xFF, 0xFE];
pub const BOM_UTF16BE: [u8; 2] = [0xFE, 0xFF];

/// BOM 嗅探：优先级表判定。歧义处理：FF FE 前缀先按 32 位 BOM 探测
/// （UTF-32LE 以 UTF-16LE 为前缀，先长后短防误判）；无 BOM → "none"
/// 交启发式链（主册检测顺序：UTF-8 严格校验 → UTF-16 → GBK → Latin-1）。
pub fn sniff_bom(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&BOM_UTF8) { "utf-8" }
    else if bytes.starts_with(&BOM_UTF32LE) { "utf-32le" }
    else if bytes.starts_with(&BOM_UTF32BE) { "utf-32be" }
    else if bytes.starts_with(&BOM_UTF16LE) { "utf-16le" }
    else if bytes.starts_with(&BOM_UTF16BE) { "utf-16be" }
    else { "none" }
}

// 域自检（深化批次三）

pub fn run_f034e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F034-codepage-d3");
    // 1) GBK 双轨：合法对入账（0xB0A1 啊——首 0xB0 次 0xA1 均在轨）。
    let mut g = DoubleByteScanner::new(true);
    g.scan(&[0xB0, 0xA1]);
    cs.add("gbk_pair_ok", g.complete_pairs == 1 && g.illegal_trails == 0, "");
    // 2) GBK 次字节 0x7F 剔除：0xB0 7F 非法检出计数。
    let mut g2 = DoubleByteScanner::new(true);
    g2.scan(&[0xB0, 0x7F, 0xB0, 0x40]);
    cs.add("gbk_trail_excluded_7f", g2.illegal_trails == 1 && g2.complete_pairs == 1, "");
    // 3) Big5 双轨隙：0x80 在 0x7E-0xA1 隙内非法；GBK 同字节合法。
    let mut b5 = DoubleByteScanner::new(false);
    let mut gk = DoubleByteScanner::new(true);
    b5.scan(&[0xA1, 0x40, 0xA1, 0x80]);
    gk.scan(&[0xA1, 0x40, 0xA1, 0x80]);
    cs.add("big5_dual_track_gap",
        b5.complete_pairs == 1 && b5.illegal_trails == 1 && gk.complete_pairs == 2 && gk.illegal_trails == 0, "");
    // 4) 孤首字节：流尾 lead 无次字节 → 孤字节账。
    let mut g3 = DoubleByteScanner::new(true);
    g3.scan(&[0x41, 0xB0]);
    cs.add("lone_lead_detected", g3.lone_leads == 1 && g3.complete_pairs == 0, "");
    // 5) 稠密段：全角行线性（0xA3A1→U+FF01、0xA3B0→U+FF10、0xA3FE→U+FF5E）。
    cs.add("dense_fullwidth_row",
        double_to_unicode(0xA3A1) == Some(0xFF01) && double_to_unicode(0xA3B0) == Some(0xFF10)
            && double_to_unicode(0xA3FE) == Some(0xFF5E), "");
    // 6) 稠密段：希腊/西里尔锚点；段外未命中返回 None（不谎报）。
    cs.add("dense_greek_cyrillic",
        double_to_unicode(0xA6A1) == Some(0x0391) && double_to_unicode(0xA7A1) == Some(0x0410)
            && double_to_unicode(0xB0A1).is_none() && double_to_unicode(0x0000).is_none(), "");
    // 7) 汉字覆盖区：一级区含端 0xB0A1/0xD7F3，区外排除；二级区同理。
    cs.add("hanzi_region_bounds",
        is_level1_hanzi(0xB0A1) && is_level1_hanzi(0xD7F3) && !is_level1_hanzi(0xB0A0)
            && !is_level1_hanzi(0xD7F4) && is_level2_hanzi(0xD8A1) && is_level2_hanzi(0xF7FE)
            && !is_level2_hanzi(0xD7F4) && !is_level2_hanzi(0xF7FF), "");
    // 8) 替换账：100 单元损 2 → 20‰ 超线告警；损 1 → 10‰ 恰线不告。
    let mut s1 = ConversionStats::new();
    for _ in 0..98 { s1.record(Some(0x41)); }
    s1.record(None);
    s1.record(None);
    let mut s2 = ConversionStats::new();
    for _ in 0..99 { s2.record(Some(0x41)); }
    s2.record(None);
    cs.add("loss_permille_warn_line",
        s1.loss_permille() == 20 && s1.loss_warn() && s2.loss_permille() == 10 && !s2.loss_warn(), "");
    // 9) 引擎账自洽：全角"０"（0xA3B0）直查命中；非法 trail 计损。
    let mut e = CodePageEngine::new(true);
    e.process(&[0xA3, 0xB0, 0xB0, 0x7F]);
    cs.add("engine_ledger_consistent",
        e.stats.total_units == 2 && e.stats.converted == 1 && e.stats.unmappable == 1
            && e.scanner.illegal_trails == 1, "");
    // 10) BOM 优先级：FF FE 前缀先按 32 位探测（歧义处理）；五序齐全。
    cs.add("bom_priority_table",
        sniff_bom(&BOM_UTF8) == "utf-8" && sniff_bom(&[0xFF, 0xFE, 0x00, 0x00, 0x41]) == "utf-32le"
            && sniff_bom(&[0xFF, 0xFE, 0x41, 0x00]) == "utf-16le"
            && sniff_bom(&[0x00, 0x00, 0xFE, 0xFF]) == "utf-32be"
            && sniff_bom(&[0xFE, 0xFF, 0x41]) == "utf-16be" && sniff_bom(&[0x41, 0x42]) == "none", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_segment_boundaries() {
        // 段界：末格含端、越界一格即未命中——线性索引边界如实。
        assert_eq!(double_to_unicode(0xA7A1 + 31), Some(0x0410 + 31));
        assert_eq!(double_to_unicode(0xA7A1 + 32), None);
        assert_eq!(double_to_unicode(0xA6A1 + 16), Some(0x0391 + 16));
        assert_eq!(double_to_unicode(0xA6A1 + 17), None);
    }

    #[test]
    fn scanner_counts_stream_exactly() {
        // 混合流逐对对账：合法对 2 + 非法 1 + 孤 1。
        let mut s = DoubleByteScanner::new(true);
        s.scan(&[0xB0, 0xA1, 0x41, 0xD6, 0xD0, 0x81, 0x7F, 0xFE]);
        assert_eq!(s.complete_pairs, 2);
        assert_eq!(s.illegal_trails, 1);
        assert_eq!(s.lone_leads, 1);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f034e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
