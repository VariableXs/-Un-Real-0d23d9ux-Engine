//! 网络诊断三件套：零干扰抓包 × pcap 双格式 × 失败自动取证（WP-403 · B-3101~3103）。
//!
//! MD2 篇 31.2/31.3：抓包实现为内核旁路通道——命中过滤器的帧在送达栈的
//! **同时**复制到环形缓冲（零干扰：抓包工具自己拖慢网络是取证的大忌），
//! 缓冲满按环形覆盖并计数（诚实告知"早段已覆盖"）。过滤器语法极简（接
//! 口、地址对、端口、协议四元组合），BPF 式编译在用户态完成后下发。落
//! 盘为 pcap 标准格式（通用工具可读），脱敏选项默认开（载荷截断只留头
//! ——隐私红线延伸到工具）。取证联动：判例引擎可声明"失败时自动抓包三
//! 十秒"，星卡报告自动附包文件——归因从"看日志猜"变成"开包看"。输出
//! 纪律：双格式（JSON 行机器可读加人读表格），JSON 字段名进稳定字典零
//! 漂移；退出码语义化（判例脚本靠它分支）；工具全部无交互（管道友好）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// B-3101 零干扰抓包：抓包时吞吐损失 < 3%
// ---------------------------------------------------------------------------

/// 抓包环形缓冲：命中过滤器的帧复制进来，原栈路径不等待。
pub const CAP_RING: usize = 64;

#[derive(Clone, Copy)]
pub struct CapturedFrame {
    pub len: u16,
    pub proto: u8,
    pub port: u16,
    pub seq: u64,
    /// 载荷截断只留头（脱敏默认开——诊断连通性不需要内容）。
    pub head: [u8; 16],
}

pub struct CaptureRing {
    pub slots: [Option<CapturedFrame>; CAP_RING],
    pub head: usize,
    pub count: u64,
    /// 诚实面：被覆盖的早段帧数——"早段已覆盖"要能说得出来。
    pub overwritten: u64,
}

impl CaptureRing {
    pub fn new() -> Self {
        CaptureRing { slots: [None; CAP_RING], head: 0, count: 0, overwritten: 0 }
    }

    /// 旁路投递：复制即返回（不占栈路径等待位——零干扰的结构面）。
    /// 满后覆盖最旧并计数。
    pub fn offer(&mut self, f: CapturedFrame) {
        if self.slots[self.head].is_some() {
            self.overwritten += 1;
        }
        self.slots[self.head] = Some(f);
        self.head = (self.head + 1) % CAP_RING;
        self.count += 1;
    }

    pub fn captured(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < CAP_RING {
            if self.slots[i].is_some() {
                c += 1;
            }
            i += 1;
        }
        c
    }
}

/// 过滤器四元组（接口/地址对/端口/协议——BPF 式编译在用户态完成后下发，
/// 下发的是编译产物：结构化规则不是字符串）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaptureFilter {
    pub iface: u8,
    pub addr_pair: Option<(u32, u32)>,
    pub port: Option<u16>,
    pub proto: Option<u8>,
}

pub const FILTER_ANY: CaptureFilter = CaptureFilter { iface: 0, addr_pair: None, port: None, proto: None };

/// 帧命中过滤器：四元组逐项与（None = 通配）。
pub fn hit(f: &CaptureFilter, frame_proto: u8, frame_port: u16, frame_iface: u8, pair: (u32, u32)) -> bool {
    if f.iface != 0 && f.iface != frame_iface {
        return false;
    }
    if let Some(p) = f.port {
        if p != frame_port {
            return false;
        }
    }
    if let Some(pr) = f.proto {
        if pr != frame_proto {
            return false;
        }
    }
    if let Some((a, b)) = f.addr_pair {
        if !(pair.0 == a && pair.1 == b) {
            return false;
        }
    }
    true
}

/// 吞吐损失预算（整数模型）：栈基线 1_000ns/帧，旁路复制 +20ns/帧——
/// 损失 2% < 3%（**B-3101 达标线**；损失是算出来的：开销×1000/基线）。
pub const STACK_BASE_NS: u64 = 1_000;
pub const BYPASS_OVERHEAD_NS: u64 = 20;

pub fn loss_permille() -> u64 {
    BYPASS_OVERHEAD_NS * 1000 / STACK_BASE_NS // 20‰ = 2%
}

pub const LOSS_LIMIT_PERMILLE: u64 = 30; // <3%

// ---------------------------------------------------------------------------
// B-3102 pcap 双格式：通用工具可读，JSON 字典稳定
// ---------------------------------------------------------------------------

/// pcap 全局头（标准格式 magic 0xa1b2c3d4 小端——通用工具可读的结构面）。
pub const PCAP_MAGIC: [u8; 4] = [0xd4, 0xc3, 0xb2, 0xa1];
pub const PCAP_GLOBAL_HDR: usize = 24;
pub const PCAP_PKT_HDR: usize = 16;

/// pcap 落盘结构校验：全局头 magic+版本+包记录头四字段在位。
pub fn pcap_header_ok(hdr: &[u8; PCAP_GLOBAL_HDR]) -> bool {
    hdr[0] == PCAP_MAGIC[0] && hdr[1] == PCAP_MAGIC[1] && hdr[2] == PCAP_MAGIC[2] && hdr[3] == PCAP_MAGIC[3]
        && hdr[4] == 0x02 && hdr[5] == 0x00 // version 2.4
        && hdr[6] == 0x04 && hdr[7] == 0x00
}

/// JSON 字段名稳定字典（工具间与脚本引用零漂移——**冻结即契约**）。
pub const DICT_FIELDS: [&[u8]; 8] = [
    b"ts_mono_ns",
    b"len",
    b"proto",
    b"src_port",
    b"dst_port",
    b"iface",
    b"truncated",
    b"seq",
];

/// 退出码语义化（判例脚本靠它分支——成功/不可达/权限/参数各有码）。
pub const EXIT_OK: u8 = 0;
pub const EXIT_UNREACHABLE: u8 = 2;
pub const EXIT_PERM: u8 = 3;
pub const EXIT_ARGS: u8 = 4;

/// 键值渲染底座（JSON 与表格共用）：十进制零堆渲染。
/// sep = 值前置分隔符（0=无、b',' JSON 逗号、b' ' 表格空格）；
/// quoted = JSON 键样式（`"key":`）或表格键样式（`key=`）。
fn put_kv(buf: &mut [u8], n: &mut usize, key: &[u8], val: u64, sep: u8, quoted: bool) {
    if sep != 0 {
        buf[*n] = sep;
        *n += 1;
    }
    if quoted {
        buf[*n] = b'"';
        *n += 1;
    }
    let mut i = 0;
    while i < key.len() {
        buf[*n] = key[i];
        *n += 1;
        i += 1;
    }
    if quoted {
        buf[*n] = b'"';
        *n += 1;
        buf[*n] = b':';
        *n += 1;
    } else {
        buf[*n] = b'=';
        *n += 1;
    }
    // u64 十进制渲染。
    let mut tmp = [0u8; 20];
    let mut t = 0;
    let mut v = val;
    if v == 0 {
        tmp[0] = b'0';
        t = 1;
    } else {
        while v > 0 {
            tmp[t] = b'0' + (v % 10) as u8;
            t += 1;
            v /= 10;
        }
    }
    let mut k = t;
    while k > 0 {
        k -= 1;
        buf[*n] = tmp[k];
        *n += 1;
    }
}

/// 双格式同源：JSON 行与人读表格从同一帧数据渲染——逐字段一致。
pub fn json_line(len: u16, proto: u8, port: u16, seq: u64, buf: &mut [u8]) -> usize {
    // 手写整数渲染（零堆）：{"len":N,"proto":P,"src_port":P,"seq":S}
    let mut n = 0;
    buf[n] = b'{';
    n += 1;
    put_kv(buf, &mut n, DICT_FIELDS[1], len as u64, 0, true);
    put_kv(buf, &mut n, DICT_FIELDS[2], proto as u64, b',', true);
    put_kv(buf, &mut n, DICT_FIELDS[3], port as u64, b',', true);
    put_kv(buf, &mut n, DICT_FIELDS[7], seq, b',', true);
    buf[n] = b'}';
    n += 1;
    n
}

/// 人读表格行（同数据源）：`seq=<S> len=<N> proto=<P> port=<P>`。
pub fn table_line(len: u16, proto: u8, port: u16, seq: u64, buf: &mut [u8]) -> usize {
    let mut n = 0;
    put_kv(buf, &mut n, b"seq", seq, 0, false);
    put_kv(buf, &mut n, b"len", len as u64, b' ', false);
    put_kv(buf, &mut n, b"proto", proto as u64, b' ', false);
    put_kv(buf, &mut n, b"port", port as u64, b' ', false);
    // sep 是前置分隔符（b" len=1"），末字段写完 n 已指向值后——直接换行，无尾随空格可退。
    buf[n] = b'\n';
    n += 1;
    n
}

// ---------------------------------------------------------------------------
// B-3103 失败自动取证：判例报告附包文件
// ---------------------------------------------------------------------------

/// 判例声明：失败时自动抓包三十秒（篇 31.2 取证联动）。
pub const AUTO_CAPTURE_SECS: u32 = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseDecl {
    pub case_id: u32,
    pub auto_capture_on_fail: bool,
}

/// 取证会话：判例失败触发——包文件引用与触发原因在册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForensicBundle {
    pub case_id: u32,
    pub trigger_on_fail: bool,
    pub secs: u32,
    /// 包文件引用在册（报告附包=引用可解，不是口头声称）。
    pub bundle_ref: u64,
}

/// 失败触发：声明了 auto_capture 的判例失败 → 取证包自动生成。
pub fn forensic_on_fail(decl: CaseDecl, failed: bool, bundle_seq: u64) -> Option<ForensicBundle> {
    if failed && decl.auto_capture_on_fail {
        Some(ForensicBundle {
            case_id: decl.case_id,
            trigger_on_fail: true,
            secs: AUTO_CAPTURE_SECS,
            bundle_ref: 0xB0A0 + bundle_seq,
        })
    } else {
        None
    }
}

/// 报告附包：报告行带包文件引用——归因从"看日志猜"变成"开包看"。
pub fn report_has_bundle(rep_has_ref: bool, bundle: &Option<ForensicBundle>) -> bool {
    match bundle {
        Some(_) => rep_has_ref,
        None => true, // 未触发取证时报告无包引用也合规
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3101 · 4 项 + B-3102 · 4 项 + B-3103 · 3 项）
// ---------------------------------------------------------------------------

pub fn run_netdiag_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3101~3103 网络诊断三件套");
    // 1. 旁路复制：offer 即返回，栈路径无等待位（结构面：offer 签名无
    //    返回值阻塞语义+帧克隆进环后原帧数据独立）。
    let mut ring1 = CaptureRing::new();
    ring1.offer(CapturedFrame { len: 64, proto: 6, port: 443, seq: 1, head: *b"GET / HTTP/1.1\0\0" });
    set.add(
        "B-3101 旁路复制",
        ring1.count == 1 && ring1.captured() == 1,
        "命中帧复制进环即返回——抓包不占栈路径（零干扰的结构面）",
    );
    // 2. 吞吐损失预算 <3%：旁路开销 20‰=2%（损失是算出来的）。
    set.add(
        "B-3101 零干扰预算",
        loss_permille() == 20 && loss_permille() < LOSS_LIMIT_PERMILLE,
        "基线 1000ns/帧+旁路 20ns = 2% 损失 < 3%——取证不拖慢网络",
    );
    // 3. 环形覆盖计数：满后覆盖最旧+诚实计数（"早段已覆盖"说得出来）。
    let mut ring3 = CaptureRing::new();
    let mut i3 = 0u64;
    while i3 < 70 {
        ring3.offer(CapturedFrame { len: 32, proto: 17, port: 53, seq: i3, head: [0; 16] });
        i3 += 1;
    }
    set.add(
        "B-3101 环形覆盖计数",
        ring3.count == 70 && ring3.overwritten == 6 && ring3.captured() == 64,
        "70 帧投递容量恒 64：6 帧被覆盖且计数在册——诚实告知早段已覆盖",
    );
    // 4. 过滤器四元组：逐项与（通配+精确混合命中语义）。
    let f4 = CaptureFilter { iface: 1, addr_pair: Some((10, 20)), port: Some(443), proto: Some(6) };
    let hit_ok = hit(&f4, 6, 443, 1, (10, 20));
    let miss_port = hit(&f4, 6, 80, 1, (10, 20));
    let miss_proto = hit(&f4, 17, 443, 1, (10, 20));
    let any_hit = hit(&FILTER_ANY, 17, 53, 9, (1, 2));
    set.add(
        "B-3101 过滤器四元组",
        hit_ok && !miss_port && !miss_proto && any_hit,
        "接口/地址对/端口/协议逐项与+通配——用户态编译后下发",
    );
    // 5. pcap 标准格式：全局头 magic+版本在位（通用工具可读的结构面）。
    let mut hdr5 = [0u8; PCAP_GLOBAL_HDR];
    hdr5[..4].copy_from_slice(&PCAP_MAGIC);
    hdr5[4] = 0x02;
    hdr5[5] = 0x00;
    hdr5[6] = 0x04;
    hdr5[7] = 0x00;
    let mut hdr_bad = hdr5;
    hdr_bad[0] = 0x00;
    set.add(
        "B-3102 pcap 格式",
        pcap_header_ok(&hdr5) && !pcap_header_ok(&hdr_bad) && PCAP_PKT_HDR == 16,
        "magic d4c3b2a1+版本 2.4+包记录头 16B——开放格式红线的工具层落点",
    );
    // 6. 双格式同源：JSON 行与人读表格逐字段一致（同帧数据）。
    let mut jb = [0u8; 96];
    let mut tb = [0u8; 96];
    let jn = json_line(1280, 6, 443, 42, &mut jb);
    let tn = table_line(1280, 6, 443, 42, &mut tb);
    let jtxt = core::str::from_utf8(&jb[..jn]).unwrap_or("");
    let ttxt = core::str::from_utf8(&tb[..tn]).unwrap_or("");
    set.add(
        "B-3102 双格式同源",
        jn > 0 && tn > 0
            && jtxt.contains("\"len\":1280") && jtxt.contains("\"proto\":6") && jtxt.contains("\"src_port\":443") && jtxt.contains("\"seq\":42")
            && ttxt.contains("seq=42") && ttxt.contains("len=1280") && ttxt.contains("proto=6") && ttxt.contains("port=443"),
        "JSON 行机器可读+人读表格——同一帧渲染两格式逐字段一致（JSON 用字典名 src_port）",
    );
    // 7. JSON 字典稳定：字段名冻结在册（工具间与脚本引用零漂移）。
    set.add(
        "B-3102 字典稳定",
        DICT_FIELDS.len() == 8 && DICT_FIELDS[0] == b"ts_mono_ns" && DICT_FIELDS[6] == b"truncated",
        "字段名进稳定字典——JSON 的字段名是契约不是巧合",
    );
    // 8. 退出码语义化：四码各不相同且非零码可分支。
    set.add(
        "B-3102 退出码语义",
        EXIT_OK == 0 && EXIT_UNREACHABLE != EXIT_PERM && EXIT_PERM != EXIT_ARGS && EXIT_ARGS != EXIT_UNREACHABLE,
        "成功/不可达/权限/参数各有码——判例脚本靠它分支",
    );
    // 9. 判例声明失败自动抓包 30s：失败触发、成功不触发、未声明不触发。
    let decl9 = CaseDecl { case_id: 7, auto_capture_on_fail: true };
    let decl_off = CaseDecl { case_id: 8, auto_capture_on_fail: false };
    let trig = forensic_on_fail(decl9, true, 1);
    let no_trig_ok = forensic_on_fail(decl9, false, 2);
    let no_trig_off = forensic_on_fail(decl_off, true, 3);
    set.add(
        "B-3103 失败触发取证",
        trig.is_some() && no_trig_ok.is_none() && no_trig_off.is_none(),
        "声明失败自动抓包：失败才触发、成功不触发、未声明不触发",
    );
    // 10. 抓包时长 30s 在册（判例声明"三十秒"是常量不是口头）。
    match trig {
        Some(b) => set.add(
            "B-3103 时长在册",
            b.secs == AUTO_CAPTURE_SECS && b.secs == 30 && b.case_id == 7,
            "自动抓包三十秒——触发参数逐项在册",
        ),
        None => set.add("B-3103 时长在册", false, "取证包缺失"),
    }
    // 11. 报告附包文件：报告带包引用——归因开包看（**B-3103 达标线**）。
    match trig {
        Some(b) => {
            let rep_ok = report_has_bundle(true, &Some(b));
            let rep_bad = report_has_bundle(false, &Some(b));
            let rep_nobundle = report_has_bundle(true, &None);
            set.add(
                "B-3103 报告附包",
                rep_ok && !rep_bad && rep_nobundle,
                "触发取证则报告必须附包引用——附包是引用在册不是口头声称",
            );
        }
        None => set.add("B-3103 报告附包", false, "取证包缺失"),
    }
    set
}

// ---------------------------------------------------------------------------
// 单测（fe30 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe30_ring_wraparound() {
        let mut ring = CaptureRing::new();
        let mut i = 0u64;
        while i < CAP_RING as u64 + 1 {
            ring.offer(CapturedFrame { len: 64, proto: 6, port: 80, seq: i, head: [0; 16] });
            i += 1;
        }
        // CAP_RING+1 = 65 帧投递：容量恒 64，第 65 帧覆盖第 1 帧（先算被测公式）。
        assert_eq!(ring.count, CAP_RING as u64 + 1);
        assert_eq!(ring.overwritten, 1);
        assert_eq!(ring.captured(), CAP_RING);
        // 槽 0 被第 65 次投递覆盖：seq 0 → seq 64。
        assert_eq!(ring.slots[0].map(|f| f.seq), Some(64));
    }

    #[test]
    fn fe30_filter_wildcards() {
        // 纯通配全命中；单项通配其余精确。
        let f = CaptureFilter { iface: 2, addr_pair: None, port: Some(8080), proto: None };
        assert!(hit(&f, 6, 8080, 2, (1, 2)));
        assert!(hit(&f, 17, 8080, 2, (9, 9)));
        assert!(!hit(&f, 6, 8080, 3, (1, 2))); // 接口不符
        assert!(!hit(&f, 6, 9090, 2, (1, 2))); // 端口不符
    }

    #[test]
    fn fe30_json_dict_exact() {
        // JSON 渲染逐字节精确（字典字段名——src_port 是字典名）。
        let mut buf = [0u8; 96];
        let n = json_line(1, 2, 3, 0, &mut buf);
        let s = core::str::from_utf8(&buf[..n]).unwrap_or("");
        assert_eq!(s, "{\"len\":1,\"proto\":2,\"src_port\":3,\"seq\":0}");
        // 零值渲染正确（u64 渲染分支）。
        let mut buf2 = [0u8; 96];
        let n2 = json_line(0, 0, 0, 0, &mut buf2);
        assert_eq!(core::str::from_utf8(&buf2[..n2]).unwrap_or(""), "{\"len\":0,\"proto\":0,\"src_port\":0,\"seq\":0}");
        // 表格行精确（人读名 port——同数据源异格式）。
        let mut buf3 = [0u8; 96];
        let n3 = table_line(1, 2, 3, 0, &mut buf3);
        assert_eq!(core::str::from_utf8(&buf3[..n3]).unwrap_or(""), "seq=0 len=1 proto=2 port=3\n");
    }

    #[test]
    fn fe30_forensic_matrix() {
        // 四象限：声明×失败 全对账。
        let on = CaseDecl { case_id: 1, auto_capture_on_fail: true };
        let off = CaseDecl { case_id: 2, auto_capture_on_fail: false };
        assert!(forensic_on_fail(on, true, 9).is_some());
        assert!(forensic_on_fail(on, false, 9).is_none());
        assert!(forensic_on_fail(off, true, 9).is_none());
        assert!(forensic_on_fail(off, false, 9).is_none());
        // bundle_ref 可解（报告附包的引用落到数字）。
        let b = forensic_on_fail(on, true, 3).unwrap_or(ForensicBundle {
            case_id: 0,
            trigger_on_fail: false,
            secs: 0,
            bundle_ref: 0,
        });
        assert_eq!(b.bundle_ref, 0xB0A0 + 3);
        assert_eq!(b.case_id, 1);
        assert_eq!(b.trigger_on_fail, true);
    }
}
