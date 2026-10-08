//! F025 深化批次二 · PDF 结构字节面（compatstar2/deep · G-A-25）。
//!
//! 批次一深化覆盖 PDF 对象模型/内容流/页范围/xref；本批补齐：文件头尾
//! 字节结构（%PDF-1.x 头 + startxref/%%EOF 尾——外部阅读器互操作判据的
//! 结构前提）、字体子集前缀（ABCDEF+ 嵌入约定——「文件更小」的实现面）、
//! 页树计数不变量（Pages/Kids/Count 一致性）、MediaBox/CropBox 语义
//! （CropBox 不得越出 MediaBox）、Info 字典日期（D:YYYYMMDDHHmmSS 格式）。
//!
//! 零堆纪律：定长缓冲写出，无 alloc。

use crate::checks::CheckSet;

/// PDF 头签名（MS 兼容阅读器识别线）。
pub const PDF_HEADER: &[u8] = b"%PDF-1.";

/// 文件头识别：以 %PDF-1. 开头即合法 PDF 版本化文件。
pub fn header_ok(bytes: &[u8]) -> bool {
    bytes.len() >= PDF_HEADER.len() && &bytes[..PDF_HEADER.len()] == PDF_HEADER
}

/// 尾部结构写出：`startxref\n<offset>\n%%EOF\n`（互操作抽查的结构前提）。
/// 返回写入长度；offset 逐位十进制（零分配）。
pub fn write_startxref(offset: u64, out: &mut [u8]) -> usize {
    const LEAD: &[u8] = b"startxref\n";
    let mut n = 0usize;
    for &b in LEAD {
        out[n] = b;
        n += 1;
    }
    if offset == 0 {
        out[n] = b'0';
        n += 1;
    } else {
        let mut digits = [0u8; 20];
        let mut i = 0usize;
        let mut v = offset;
        while v > 0 {
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
        while i > 0 {
            i -= 1;
            out[n] = digits[i];
            n += 1;
        }
    }
    const TAIL: &[u8] = b"\n%%EOF\n";
    for &b in TAIL {
        out[n] = b;
        n += 1;
    }
    n
}

/// 字体子集前缀：字体名 FNV-1a → 6 个 A-P 大写字母 + '+'（嵌入约定：
/// 同名同前缀、异名几乎必异——确定性对拍面）。
pub fn subset_prefix(font: &str, out: &mut [u8; 7]) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in font.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    for i in 0..6 {
        out[i] = b'A' + (h & 0x0F) as u8;
        h >>= 4;
    }
    out[6] = b'+';
    7
}

/// 页树计数不变量：声明 Count == 各 Kid 叶子之和（扁平模型）。
pub fn page_tree_count_valid(kids: &[u16], declared: u16) -> bool {
    let sum: u32 = kids.iter().map(|&k| k as u32).sum();
    sum == declared as u32
}

/// MediaBox/CropBox（pt 整型口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoxPt {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl BoxPt {
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }
    /// A4 判定：595×842（整型口径——主册 A4 pt 常量）。
    pub fn is_a4(&self) -> bool {
        self.width() == 595 && self.height() == 842
    }
}

/// CropBox 合法性：不得越出 MediaBox（可见区域 ⊆ 页面区域）。
pub fn crop_within_media(media: &BoxPt, crop: &BoxPt) -> bool {
    crop.x0 >= media.x0 && crop.y0 >= media.y0 && crop.x1 <= media.x1 && crop.y1 <= media.y1
}

/// Info 字典日期写出：`D:YYYYMMDDHHmmSS`（PDF 规范日期格式；零分配）。
/// 返回写入长度（16）。
pub fn info_dict_date(epoch_s: i64, out: &mut [u8]) -> usize {
    const HEAD: &[u8] = b"D:";
    let mut n = 0usize;
    for &b in HEAD {
        out[n] = b;
        n += 1;
    }
    let days = epoch_s.div_euclid(86_400);
    let secs = epoch_s.rem_euclid(86_400);
    let (y, m, d) = crate::compatstar2::deep::f022d::civil_from_days(days);
    let fields = [y, m as i64, d as i64, secs / 3600, (secs % 3600) / 60, secs % 60];
    let widths = [4usize, 2, 2, 2, 2, 2]; // 年 4 位、余 2 位补零
    for (f, w) in fields.iter().zip(widths.iter()) {
        let mut digits = [0u8; 20];
        let mut i = 0usize;
        let mut v = *f;
        while v > 0 {
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
        while i < *w {
            digits[i] = b'0'; // 前导补零
            i += 1;
        }
        while i > 0 {
            i -= 1;
            out[n] = digits[i];
            n += 1;
        }
    }
    n
}

/// 域自检（深化批次二）。
pub fn run_f025d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F025-printpdf-d2");
    // 1) 文件头识别：合法版本头过、裸 %PDF-（无版本）拒。
    cs.add(
        "header_detect",
        header_ok(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3") && !header_ok(b"%PDF-x") && !header_ok(b"GIF89a"),
        "",
    );
    // 2) 尾部结构：startxref + 偏移 + %%EOF 逐字节形状。
    let mut tail = [0u8; 48];
    let n = write_startxref(1024, &mut tail);
    cs.add("startxref_shape", &tail[..n] == b"startxref\n1024\n%%EOF\n", "");
    // 3) 子集前缀：同字体名确定性一致、长度 7（6 字母 + '+'）。
    let mut p1 = [0u8; 7];
    let mut p2 = [0u8; 7];
    subset_prefix("VARIX-Song", &mut p1);
    subset_prefix("VARIX-Song", &mut p2);
    cs.add("subset_prefix_deterministic", p1 == p2 && p1[6] == b'+' && p1[..6].iter().all(|&c| (b'A'..=b'P').contains(&c)), "");
    // 4) 页树计数：Kids 和 == Count 过；不一致拒。
    cs.add(
        "page_tree_count",
        page_tree_count_valid(&[4, 6, 2], 12) && !page_tree_count_valid(&[4, 6, 2], 10),
        "",
    );
    // 5) MediaBox/CropBox：A4 判定 + Crop 越界拒。
    let a4 = BoxPt { x0: 0, y0: 0, x1: 595, y1: 842 };
    let inside = BoxPt { x0: 50, y0: 50, x1: 545, y1: 792 };
    let outside = BoxPt { x0: 0, y0: 0, x1: 600, y1: 842 };
    cs.add(
        "mediabox_cropbox",
        a4.is_a4() && crop_within_media(&a4, &inside) && !crop_within_media(&a4, &outside),
        "",
    );
    // 6) Info 日期：epoch 0 → D:19700101000000（16 字符）。
    let mut dbuf = [0u8; 24];
    let dn = info_dict_date(0, &mut dbuf);
    cs.add("info_date_shape", dn == 16 && &dbuf[..dn] == b"D:19700101000000", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startxref_offset_zero() {
        let mut tail = [0u8; 48];
        let n = write_startxref(0, &mut tail);
        assert_eq!(&tail[..n], b"startxref\n0\n%%EOF\n");
    }

    #[test]
    fn info_date_known_point() {
        // 2026-09-27 00:00:00 UTC = 20723 天 → D:20260927000000。
        let epoch = 20_723i64 * 86_400;
        let mut buf = [0u8; 24];
        let n = info_dict_date(epoch, &mut buf);
        assert_eq!(&buf[..n], b"D:20260927000000");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f025d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
