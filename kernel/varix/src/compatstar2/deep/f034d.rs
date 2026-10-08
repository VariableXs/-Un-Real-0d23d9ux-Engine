//! F034 深化批次二 · 编码器与换行统计面（compatstar2/deep · G-A-34）。
//!
//! 批次一深化覆盖 GBK 区间/CP437/代理对/BOM；本批补齐：UTF-8 编码器
//! （码点 → 字节序列——「另存为 UTF-8」写出侧，与批次一解码侧闭环）、
//! Big5/EUC-KR 双字节区间校验（8 码页登记表的区间深化）、换行三态统计
//! （CRLF/LF/CR 分计——编辑器保存不偷换换行的依据）、转换账本不变量
//! （入字节/出字符/替换三账自洽）。
//!
//! 零堆纪律：定长缓冲，无 alloc。

use crate::checks::CheckSet;

/// UTF-8 编码：码点 → 字节序列（1-4 字节；代理区与超界如实拒绝返回 0）。
/// 返回写入长度；与批次一 `codepage::is_valid_utf8` 构成编码-解码闭环。
pub fn utf8_encode(cp: u32, out: &mut [u8]) -> usize {
    if cp > 0x10_FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return 0; // 代理区/超界：编码器如实拒绝（不静默产坏字节）
    }
    if cp < 0x80 {
        if out.is_empty() {
            return 0;
        }
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        if out.len() < 2 {
            return 0;
        }
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else if cp < 0x1_0000 {
        if out.len() < 3 {
            return 0;
        }
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    } else {
        if out.len() < 4 {
            return 0;
        }
        out[0] = 0xF0 | (cp >> 18) as u8;
        out[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        out[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[3] = 0x80 | (cp & 0x3F) as u8;
        4
    }
}

/// Big5 双字节区间（lead 0x81-0xFE；trail 0x40-0x7E 或 0xA1-0xFE——Big5 规范）。
pub fn big5_double_byte_valid(lead: u8, trail: u8) -> bool {
    (0x81..=0xFE).contains(&lead) && ((0x40..=0x7E).contains(&trail) || (0xA1..=0xFE).contains(&trail))
}

/// EUC-KR 双字节区间（lead 0x81-0xFD；trail 0x41-0xFE 除 0x7F——KS X 1001 口径）。
pub fn euckr_double_byte_valid(lead: u8, trail: u8) -> bool {
    (0x81..=0xFD).contains(&lead) && (0x41..=0xFE).contains(&trail) && trail != 0x7F
}

/// 换行三态统计：CRLF/LF/CR 分计（保存不偷换换行的依据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NewlineStats {
    pub crlf: u32,
    pub lf: u32,
    pub cr: u32,
}

/// 扫描字节流：CRLF 记一格并跳过两字节；裸 LF、裸 CR 各记一格。
pub fn scan_newlines(bytes: &[u8]) -> NewlineStats {
    let mut st = NewlineStats { crlf: 0, lf: 0, cr: 0 };
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                st.crlf += 1;
                i += 2;
            }
            b'\r' => {
                st.cr += 1;
                i += 1;
            }
            b'\n' => {
                st.lf += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    st
}

/// 转换账本：入字节/出字符/替换三账（不变量：替换 ≤ 出字符；替换超 1‰
/// 告警复用批次一 ConvertReport 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConvertLedger {
    pub in_bytes: u64,
    pub out_chars: u64,
    pub replacements: u64,
}

impl ConvertLedger {
    /// 账本自洽：三账非负恒真 + 替换不超产出。
    pub fn invariant_ok(&self) -> bool {
        self.replacements <= self.out_chars
    }
    /// 混合内容提示：有替换且替换率超 10‰ → 「可能选错编码」提示线。
    pub fn suspicious(&self) -> bool {
        self.out_chars > 0 && self.replacements * 1000 > self.out_chars * 10
    }
}

/// 域自检（深化批次二）。
pub fn run_f034d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F034-codepage-d2");
    // 1) 编码闭环：'中' U+4E2D → E4 B8 AD；😀 U+1F600 → F0 9F 98 80；
    //    编码产物全部通过批次一严格校验（逐步快照——缓冲复用先取值）。
    let mut buf = [0u8; 4];
    let n1 = utf8_encode(0x4E2D, &mut buf);
    let zh_ok = n1 == 3 && buf[0] == 0xE4 && buf[1] == 0xB8 && buf[2] == 0xAD;
    let n2 = utf8_encode(0x1F600, &mut buf);
    cs.add(
        "utf8_encode_roundtrip",
        zh_ok
            && n2 == 4 && buf == [0xF0, 0x9F, 0x98, 0x80]
            && crate::compatstar2::codepage::is_valid_utf8(&buf[..n2]),
        "",
    );
    // 2) 编码器拒绝：代理区 U+D800 与超界 U+110000 如实拒（0 长度）。
    cs.add("utf8_encode_reject", utf8_encode(0xD800, &mut buf) == 0 && utf8_encode(0x11_0000, &mut buf) == 0, "");
    // 3) Big5 区间：合法对/次字节 0x80 洞（0x7F-0xA0 间）拒。
    cs.add(
        "big5_range",
        big5_double_byte_valid(0xA4, 0x40) && big5_double_byte_valid(0xA4, 0xA1)
            && !big5_double_byte_valid(0xA4, 0x80) && !big5_double_byte_valid(0x80, 0x40),
        "",
    );
    // 4) EUC-KR 区间：合法对/0x7F 拒/lead 越界拒。
    cs.add(
        "euckr_range",
        euckr_double_byte_valid(0xB0, 0xA1) && !euckr_double_byte_valid(0xB0, 0x7F) && !euckr_double_byte_valid(0xFE, 0xA1),
        "",
    );
    // 5) 换行三态：b"a\nb\r\nc\r" → lf 1 / crlf 1 / cr 1（保存不偷换依据）。
    let st = scan_newlines(b"a\nb\r\nc\r");
    cs.add("newline_scan", st == NewlineStats { crlf: 1, lf: 1, cr: 1 }, "");
    // 6) 转换账本：替换 ≤ 产出不变量 + 10‰ 告警线复用
    //    （ok 样本 1/200 = 5‰ 低于线；bad 样本 2/90 ≈ 22‰ 超线）。
    let ok = ConvertLedger { in_bytes: 100, out_chars: 200, replacements: 1 };
    let bad = ConvertLedger { in_bytes: 100, out_chars: 90, replacements: 2 };
    cs.add(
        "convert_ledger",
        ok.invariant_ok() && !ok.suspicious() && bad.invariant_ok() && bad.suspicious()
            && !ConvertLedger { in_bytes: 10, out_chars: 0, replacements: 0 }.suspicious(),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode(b'A' as u32, &mut buf), 1);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn crlf_only_file() {
        let st = scan_newlines(b"x\r\ny\r\n");
        assert_eq!(st, NewlineStats { crlf: 2, lf: 0, cr: 0 }, "纯 CRLF 文件不误计裸 LF/CR");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f034d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
