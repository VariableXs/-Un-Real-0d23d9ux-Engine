
// ---------------------------------------------------------------------------
// F020 · 深化批次七：dump 尺寸人话标签（诊断中心列表的显示面）
//
// 主册依据（G-A-20【交互设计】）：「详情页就是 minidump 摘要」的列表态——
// 尺寸人话（B/KB/MB 三级，一位小数——不裸抛字节数）。
// ---------------------------------------------------------------------------

/// 尺寸标签渲染（<1KB → `N B`；<1MB → `N.N KB`；其余 → `N.N MB`；缓冲不足
/// 截断）。
pub fn dump_size_label(bytes: u64, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut push = |s: &[u8], buf: &mut [u8], n: &mut usize| {
        for &b in s {
            if *n < buf.len() {
                buf[*n] = b;
                *n += 1;
            }
        }
    };
    if bytes < 1024 {
        // N B（≤4 位十进制）。
        let mut digits = [0u8; 20];
        let mut dn = 0;
        let mut v = bytes;
        if v == 0 {
            digits[0] = b'0';
            dn = 1;
        }
        while v > 0 {
            digits[dn] = b'0' + (v % 10) as u8;
            dn += 1;
            v /= 10;
        }
        for i in (0..dn).rev() {
            push(&[digits[i]], buf, &mut n);
        }
        push(b" B", buf, &mut n);
        return n;
    }
    // KB/MB 一位小数（十分位四舍五入进位处理： tenths = (bytes*10+half)/unit）。
    let (unit_name, unit, tenths) = if bytes < 1024 * 1024 {
        (b" KB", 1024u64, (bytes * 10 + 512) / 1024)
    } else {
        (b" MB", 1024 * 1024, (bytes * 10 + 512 * 1024) / (1024 * 1024))
    };
    let whole = tenths / 10;
    let frac = tenths % 10;
    let mut digits = [0u8; 20];
    let mut dn = 0;
    let mut v = whole;
    if v == 0 {
        digits[0] = b'0';
        dn = 1;
    }
    while v > 0 {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
    }
    for i in (0..dn).rev() {
        push(&[digits[i]], buf, &mut n);
    }
    push(&[b'.', b'0' + frac as u8], buf, &mut n);
    push(unit_name, buf, &mut n);
    n
}

/// F020 深化批次七自检。
pub fn run_excface_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep6");
    // 1) 三级标签：512B / 2048B=2.0KB / 3MB。
    let mut buf = [0u8; 32];
    let n1 = dump_size_label(512, &mut buf);
    let t1 = core::str::from_utf8(&buf[..n1]).unwrap_or("");
    let n2 = dump_size_label(2048, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    let n3 = dump_size_label(3 * 1024 * 1024, &mut buf);
    let t3 = core::str::from_utf8(&buf[..n3]).unwrap_or("");
    cs.add(
        "dump_size_three_levels",
        t1 == "512 B" && t2 == "2.0 KB" && t3 == "3.0 MB",
        "",
    );
    // 2) 四舍五入进位：1536B = 1.5KB；1024*1024+512KB 边界 → 1.5 MB。
    let n4 = dump_size_label(1536, &mut buf);
    let t4 = core::str::from_utf8(&buf[..n4]).unwrap_or("");
    let mb_half = 1024 * 1024 + 512 * 1024;
    let n5 = dump_size_label(mb_half, &mut buf);
    let t5 = core::str::from_utf8(&buf[..n5]).unwrap_or("");
    cs.add(
        "dump_size_rounding",
        t4 == "1.5 KB" && t5 == "1.5 MB",
        "",
    );
    // 3) 零字节如实 "0 B"。
    let n6 = dump_size_label(0, &mut buf);
    let t6 = core::str::from_utf8(&buf[..n6]).unwrap_or("");
    cs.add("dump_size_zero", t6 == "0 B", "");
    cs
}
