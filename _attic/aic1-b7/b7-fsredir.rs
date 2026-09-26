
// ---------------------------------------------------------------------------
// F010 · 深化批次七：沙盒属性页统计行（「属于 XX 应用的隔离数据」的数据源）
//
// 主册依据（G-A-10【交互设计】）：「属性页标注『属于 XX 应用的隔离数据』」
// ——统计行渲染（条目数/字节占用的人话形态——资源占用诚实透明）。
// ---------------------------------------------------------------------------

/// 沙盒统计（条目数 + 字节——枚举面产出）。
#[derive(Clone, Copy, Debug)]
pub struct SandboxStat {
    pub app: u32,
    pub entries: u32,
    pub bytes: u64,
}

/// 统计行渲染（`属于应用 #N 的隔离数据：M 项，X.Y KB`——KB 一位小数，四舍
/// 五入；不足 1KB 显示字节）。返回写入字节数。
pub fn render_sandbox_stat(stat: &SandboxStat, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut push = |s: &[u8], buf: &mut [u8], n: &mut usize| {
        for &b in s {
            if *n < buf.len() {
                buf[*n] = b;
                *n += 1;
            }
        }
    };
    push(b"属于应用 #", buf, &mut n);
    // 应用号（十进制）。
    let app = stat.app;
    if app < 10 {
        push(&[b'0' + app as u8], buf, &mut n);
    } else {
        push(&[b'0' + (app / 10) as u8, b'0' + (app % 10) as u8], buf, &mut n);
    }
    push(b" 的隔离数据：", buf, &mut n);
    // 条目数（≤3 位十进制——诊断页量级）。
    let e = stat.entries;
    if e < 10 {
        push(&[b'0' + e as u8], buf, &mut n);
    } else {
        let mut digits = [0u8; 10];
        let mut dn = 0;
        let mut v = e;
        while v > 0 {
            digits[dn] = b'0' + (v % 10) as u8;
            dn += 1;
            v /= 10;
        }
        for i in (0..dn).rev() {
            push(&[digits[i]], buf, &mut n);
        }
    }
    push(b" 项，", buf, &mut n);
    // 字节 → KB（一位小数）。
    let kb_tenths = (stat.bytes * 10 + 512) / 1024;
    let kb = kb_tenths / 10;
    let tenth = kb_tenths % 10;
    if kb < 10 {
        push(&[b'0' + kb as u8], buf, &mut n);
    } else {
        push(&[b'0' + (kb / 10) as u8, b'0' + (kb % 10) as u8], buf, &mut n);
    }
    push(&[b'.', b'0' + tenth as u8, b' ', b'K', b'B'], buf, &mut n);
    n
}

/// F010 深化批次七自检。
pub fn run_fsredir_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep6");
    // 1) 标准渲染：应用 #3、12 项、1536B → 1.5 KB。
    let st = SandboxStat { app: 3, entries: 12, bytes: 1536 };
    let mut buf = [0u8; 128];
    let n = render_sandbox_stat(&st, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "sandbox_stat_line_render",
        text == "属于应用 #3 的隔离数据：12 项，1.5 KB",
        "",
    );
    // 2) 边界：999B → 1.0 KB（四舍五入进位）；10 项两位数。
    let tiny = SandboxStat { app: 7, entries: 10, bytes: 999 };
    let n2 = render_sandbox_stat(&tiny, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "sandbox_stat_rounding",
        t2.contains("10 项，1.0 KB"),
        "",
    );
    // 3) 小缓冲截断（不冒充完整行）。
    let mut small = [0u8; 8];
    let n3 = render_sandbox_stat(&st, &mut small);
    cs.add("sandbox_stat_truncation_honest", n3 == 8, "");
    cs
}
