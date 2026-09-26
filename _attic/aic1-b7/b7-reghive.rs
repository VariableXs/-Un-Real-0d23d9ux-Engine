
// ---------------------------------------------------------------------------
// F009 · 深化批次七：十六进制分页查看（蜂巢查看器的大值分页面）
//
// 主册依据（G-A-09【设计细节】）：「蜂巢查看器……十六进制值查看」的续面——
// 大值分页：每页 16 字节、`偏移: 字节...` 行形态（offset 4 位 hex），页越界
// 如实空页。
// ---------------------------------------------------------------------------

/// 每页字节数。
pub const HEXDUMP_PAGE_BYTES: usize = 16;

/// 一页十六进制渲染（`0040: DE AD ...` 单行；行首 offset 4 位 hex；缓冲不足
/// 截断）。页越界 → 0 字节（空页如实）。
pub fn hexdump_page(val: &[u8], page: usize, buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let start = page * HEXDUMP_PAGE_BYTES;
    if start >= val.len() {
        return 0;
    }
    let end = (start + HEXDUMP_PAGE_BYTES).min(val.len());
    let mut n = 0usize;
    // 行首偏移（4 位大写 hex）。
    let off = start;
    for shift in [12u32, 8, 4, 0] {
        if n < buf.len() {
            buf[n] = HEX[((off >> shift) & 0xF) as usize];
            n += 1;
        }
    }
    if n < buf.len() {
        buf[n] = b':';
        n += 1;
    }
    for &b in &val[start..end] {
        if n + 3 > buf.len() {
            return n;
        }
        buf[n] = b' ';
        buf[n + 1] = HEX[(b >> 4) as usize];
        buf[n + 2] = HEX[(b & 0xF) as usize];
        n += 3;
    }
    n
}

/// F009 深化批次七自检。
pub fn run_reghive_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep6");
    // 1) 满页：16 字节 → 行首偏移 0000 + 16 对 hex。
    let data: alloc::vec::Vec<u8> = (0..32u8).collect();
    let mut buf = [0u8; 128];
    let n1 = hexdump_page(&data, 0, &mut buf);
    let t1 = core::str::from_utf8(&buf[..n1]).unwrap_or("");
    cs.add(
        "hexdump_page_zero_offset",
        n1 == 4 + 1 + 16 * 3 && t1.starts_with("0000: 00 01 02 03") && t1.ends_with("0F"),
        "",
    );
    // 2) 第二页：偏移 0010 + 剩余 16 字节。
    let n2 = hexdump_page(&data, 1, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "hexdump_page_one_offset",
        t2.starts_with("0010: 10 11 12") && t2.ends_with("1F"),
        "",
    );
    // 3) 尾页不满 + 页越界空页（如实零字节）。
    let tail: alloc::vec::Vec<u8> = (0..20u8).collect();
    let n3 = hexdump_page(&tail, 1, &mut buf);
    let t3 = core::str::from_utf8(&buf[..n3]).unwrap_or("");
    let empty = hexdump_page(&tail, 5, &mut buf);
    cs.add(
        "hexdump_page_tail_and_beyond",
        n3 == 4 + 1 + 4 * 3 && t3.ends_with("13") && empty == 0,
        "",
    );
    cs
}
