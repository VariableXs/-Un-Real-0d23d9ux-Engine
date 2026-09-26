
// ---------------------------------------------------------------------------
// F009 · 深化批次四：蜂巢查看器（键值复制 + 十六进制值查看）+ 系统模板只读
// 共享（全进程一份）
//
// 主册依据（G-A-09【设计细节】）：「蜂巢查看器树视图支持键值复制与十六进制
// 值查看」；「系统模板蜂巢启动时 mmap 只读共享（全进程共用一份）」。
// ---------------------------------------------------------------------------

/// 值的十六进制视图（诊断页/查看器消费：`AA BB CC` 空格分隔，缓冲不足
/// 如实截断——截断结果不冒充完整值）。
pub fn hex_view(val: &[u8], buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0usize;
    for (i, &b) in val.iter().enumerate() {
        if i > 0 {
            if n >= buf.len() {
                return n;
            }
            buf[n] = b' ';
            n += 1;
        }
        if n + 2 > buf.len() {
            return n;
        }
        buf[n] = HEX[(b >> 4) as usize];
        buf[n + 1] = HEX[(b & 0xF) as usize];
        n += 2;
    }
    n
}

/// 查看器行复制（键值复制动线：`键 = 值(hex)` 写入缓冲，供剪贴板 F017）。
pub fn viewer_row(key: &str, val: &[u8], buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    for &b in key.as_bytes() {
        if n >= buf.len() {
            return n;
        }
        buf[n] = b;
        n += 1;
    }
    for &s in b" = ".iter() {
        if n >= buf.len() {
            return n;
        }
        buf[n] = s;
        n += 1;
    }
    let mut hexbuf = [0u8; 192];
    let hn = hex_view(val, &mut hexbuf);
    for &b in &hexbuf[..hn] {
        if n >= buf.len() {
            return n;
        }
        buf[n] = b;
        n += 1;
    }
    n
}

/// 系统模板只读共享（mmap 语义的进程面模型：全进程一份，写请求恒拒绝）。
#[derive(Clone, Copy, Debug)]
pub struct TemplateShare {
    /// 共享映射的进程引用数。
    pub refs: u32,
    /// 写模板请求拒绝计数（审计面——「全局模板零写入」判据的本面锚）。
    pub write_refusals: u32,
}

impl TemplateShare {
    pub const fn new() -> TemplateShare {
        TemplateShare { refs: 0, write_refusals: 0 }
    }

    pub fn attach(&mut self) -> u32 {
        self.refs += 1;
        self.refs
    }

    pub fn detach(&mut self) -> u32 {
        self.refs = self.refs.saturating_sub(1);
        self.refs
    }

    /// 模板写请求：恒拒绝（「全局模板零写入」判据在共享面的落点——哈希不变）。
    pub fn write_template_request(&mut self) -> bool {
        self.write_refusals += 1;
        false
    }
}

/// F009 深化批次四自检。
pub fn run_reghive_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep3");
    // 1) 十六进制视图：`DE AD BE EF` 精确产出；短缓冲如实截断。
    let mut buf = [0u8; 32];
    let n1 = hex_view(&[0xDE, 0xAD, 0xBE, 0xEF], &mut buf);
    let full_ok = &buf[..n1] == b"DE AD BE EF";
    let mut small = [0u8; 4];
    let n2 = hex_view(&[0xDE, 0xAD, 0xBE, 0xEF], &mut small);
    cs.add(
        "hex_view_exact_and_truncated",
        full_ok && n2 == 4,
        "",
    );
    // 2) 查看器行复制：键 + 十六进制值成行（复制动线端到端）。
    let mut row = [0u8; 64];
    let n3 = viewer_row("Software\\MyApp\\Theme", &[0x01, 0x02], &mut row);
    cs.add(
        "viewer_row_copy",
        n3 > 0 && core::str::from_utf8(&row[..n3]).unwrap_or("").starts_with("Software\\MyApp\\Theme = 01 02"),
        "",
    );
    // 3) 模板共享：多进程 attach 计数；写模板恒拒绝计数；detach 全清。
    let mut sh = TemplateShare::new();
    let r1 = sh.attach();
    let r2 = sh.attach();
    let w = sh.write_template_request();
    let w2 = sh.write_template_request();
    let r3 = sh.detach();
    let r4 = sh.detach();
    cs.add(
        "template_share_readonly_shared",
        r1 == 1 && r2 == 2 && !w && !w2 && sh.write_refusals == 2 && r3 == 1 && r4 == 0,
        "",
    );
    cs
}
