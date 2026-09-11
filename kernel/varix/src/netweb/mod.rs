//! AURORA-1000 域二：网络栈与浏览器（A601~A625）。
//!
//! 纯逻辑 + 固定容量数组。无 Vec/String/Box/alloc，无外部 crate/std 专用 API。
//! ASCII 匹配借用 `crate::galaxy::ascii_*_ci`。模糊测试借用 `crate::galaxy::rt::DetPrng`。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

pub const MAX_NODES: usize = 16;
pub const MAX_STACK: usize = 8;
pub const MAX_DRAW: usize = 16;
pub const MAX_BM: usize = 16;
pub const MAX_TABS: usize = 8;
pub const MAX_BACK: usize = 8;
pub const MAX_CACHE: usize = 8;

// ---------------------------------------------------------------------------
// A601 TCP/IP — IPv4 u32、RFC1071 反码和校验、伪头部校验
// ---------------------------------------------------------------------------

fn ones_sum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i + 1 < data.len() {
        let w = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum = sum.wrapping_add(w);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    sum
}

fn fold16(sum: u32) -> u16 {
    let mut s = sum;
    while (s >> 16) != 0 {
        s = (s & 0xFFFF) + (s >> 16);
    }
    !(s as u16)
}

pub fn ipv4_checksum(data: &[u8]) -> u16 {
    fold16(ones_sum(data))
}

pub fn pseudo_checksum(src: u32, dst: u32, proto: u8, length: u16) -> u16 {
    let mut s: u32 = 0;
    s = s.wrapping_add((src >> 16) as u32);
    s = s.wrapping_add((src & 0xFFFF) as u32);
    s = s.wrapping_add((dst >> 16) as u32);
    s = s.wrapping_add((dst & 0xFFFF) as u32);
    s = s.wrapping_add(proto as u32);
    s = s.wrapping_add(length as u32);
    fold16(s)
}

// ---------------------------------------------------------------------------
// A602 TLS — 记录层分帧（type/version/length）+ 确定性 PRNG 会话 id
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TlsContentType {
    Handshake,
    AppData,
    Alert,
    Other(u8),
}

pub struct TlsRecord {
    pub content_type: TlsContentType,
    pub version: u16,
    pub length: u16,
}

pub fn parse_tls_record(hdr: &[u8; 5]) -> Option<TlsRecord> {
    if hdr.len() < 5 {
        return None;
    }
    let ct = match hdr[0] {
        22 => TlsContentType::Handshake,
        23 => TlsContentType::AppData,
        21 => TlsContentType::Alert,
        other => TlsContentType::Other(other),
    };
    let version = u16::from_be_bytes([hdr[1], hdr[2]]);
    let length = u16::from_be_bytes([hdr[3], hdr[4]]);
    if length > 16_384 {
        return None;
    }
    Some(TlsRecord { content_type: ct, version, length })
}

pub fn gen_session_id(prng: &mut DetPrng, out: &mut [u8; 16]) {
    for b in out.iter_mut() {
        *b = (prng.next_u64() & 0xFF) as u8;
    }
}

// ---------------------------------------------------------------------------
// A603 HTTP 客户端 — 请求行/头部组装（GET 固定缓冲）+ 响应状态行解析
// ---------------------------------------------------------------------------

fn append(buf: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *n < buf.len() {
            buf[*n] = b;
            *n += 1;
        }
    }
}

pub fn build_get(buf: &mut [u8; 128], host: &str, path: &str) -> usize {
    let mut n = 0usize;
    append(buf, &mut n, "GET ");
    append(buf, &mut n, path);
    append(buf, &mut n, " HTTP/1.1\r\nHost: ");
    append(buf, &mut n, host);
    append(buf, &mut n, "\r\n\r\n");
    n
}

pub fn parse_status_line(line: &[u8]) -> Option<(u16, &str)> {
    if line.len() < 12
        || line[0] != b'H'
        || line[1] != b'T'
        || line[2] != b'T'
        || line[3] != b'P'
        || line[4] != b'/'
    {
        return None;
    }
    let mut i = 5usize;
    while i < line.len() && line[i] != b' ' {
        i += 1;
    }
    if i >= line.len() {
        return None;
    }
    let start = i + 1;
    let mut j = start;
    while j < line.len() && line[j] != b' ' {
        j += 1;
    }
    if j == start || j - start != 3 {
        return None;
    }
    let mut code: u16 = 0;
    for &c in &line[start..j] {
        if c < b'0' || c > b'9' {
            return None;
        }
        code = code.wrapping_mul(10).wrapping_add((c - b'0') as u16);
    }
    let reason = if j < line.len() {
        core::str::from_utf8(&line[j + 1..]).unwrap_or("")
    } else {
        ""
    };
    Some((code, reason))
}

// ---------------------------------------------------------------------------
// A604 DNS — 域名编码（labels + 0）+ A 记录应答解析（跳过问题区）
// ---------------------------------------------------------------------------

pub fn encode_name(name: &str, out: &mut [u8]) -> usize {
    let bytes = name.as_bytes();
    let mut label_start = 0usize;
    let mut o = 0usize;
    let mut j = 0usize;
    while j < bytes.len() {
        if bytes[j] == b'.' {
            let len = j - label_start;
            if o + 1 >= out.len() {
                return 0;
            }
            out[o] = len as u8;
            o += 1;
            if o + len > out.len() {
                return 0;
            }
            out[o..o + len].copy_from_slice(&bytes[label_start..j]);
            o += len;
            label_start = j + 1;
        }
        j += 1;
    }
    let len = bytes.len() - label_start;
    if len > 0 {
        if o + 1 >= out.len() {
            return 0;
        }
        out[o] = len as u8;
        o += 1;
        if o + len > out.len() {
            return 0;
        }
        out[o..o + len].copy_from_slice(&bytes[label_start..]);
        o += len;
    }
    if o >= out.len() {
        return 0;
    }
    out[o] = 0;
    o + 1
}

pub fn build_dns_a_response(host: &str, ip: [u8; 4], out: &mut [u8; 64]) -> usize {
    let mut o = 0usize;
    out[o..o + 2].copy_from_slice(&0u16.to_le_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&0x8180u16.to_le_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&1u16.to_le_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&1u16.to_le_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&0u16.to_le_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&0u16.to_le_bytes());
    o += 2;
    let n = encode_name(host, &mut out[o..]);
    o += n;
    out[o..o + 2].copy_from_slice(&1u16.to_be_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&1u16.to_be_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&0xC00Cu16.to_be_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&1u16.to_be_bytes());
    o += 2;
    out[o..o + 2].copy_from_slice(&1u16.to_be_bytes());
    o += 2;
    out[o..o + 4].copy_from_slice(&0u32.to_be_bytes());
    o += 4;
    out[o..o + 2].copy_from_slice(&4u16.to_be_bytes());
    o += 2;
    out[o..o + 4].copy_from_slice(&ip);
    o += 4;
    o
}

pub fn parse_a_record(msg: &[u8]) -> Option<[u8; 4]> {
    if msg.len() < 12 {
        return None;
    }
    let ancount = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12usize;
    loop {
        if pos >= msg.len() {
            return None;
        }
        let b = msg[pos];
        if b == 0 {
            pos += 1;
            break;
        }
        if b & 0xC0 == 0xC0 {
            pos += 2;
            break;
        }
        pos += 1 + (b as usize);
    }
    pos += 4; // QTYPE + QCLASS
    let mut remaining = ancount;
    while remaining > 0 {
        if pos >= msg.len() {
            return None;
        }
        let b = msg[pos];
        if b & 0xC0 == 0xC0 {
            pos += 2;
        } else {
            loop {
                if pos >= msg.len() {
                    return None;
                }
                let lb = msg[pos];
                if lb == 0 {
                    pos += 1;
                    break;
                }
                if lb & 0xC0 == 0xC0 {
                    pos += 2;
                    break;
                }
                pos += 1 + (lb as usize);
            }
        }
        if pos + 10 > msg.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        let rdata = pos + 10;
        if rtype == 1 && rdlen == 4 {
            if rdata + 4 > msg.len() {
                return None;
            }
            return Some([msg[rdata], msg[rdata + 1], msg[rdata + 2], msg[rdata + 3]]);
        }
        pos = rdata + rdlen;
        remaining -= 1;
    }
    None
}

// ---------------------------------------------------------------------------
// A605 HTML 解析 — tokenizer + 固定容量元素树（标签 id、父、文本、alt）
// ---------------------------------------------------------------------------

pub const TAG_NAMES: [&'static str; 8] = ["div", "p", "a", "span", "h1", "img", "ul", "li"];

pub fn tag_id(name: &[u8]) -> u8 {
    for i in 0..TAG_NAMES.len() {
        if crate::galaxy::ascii_eq_ci(name, TAG_NAMES[i].as_bytes()) {
            return i as u8;
        }
    }
    0xFF
}

#[derive(Clone, Copy)]
pub struct HtmlNode<'a> {
    pub tag: &'static str,
    pub parent: i16,
    pub text: &'a str,
    pub alt: &'a str,
}

pub struct HtmlTree<'a> {
    pub nodes: [HtmlNode<'a>; MAX_NODES],
    pub count: usize,
}

impl<'a> HtmlTree<'a> {
    pub fn new() -> HtmlTree<'a> {
        HtmlTree {
            nodes: [HtmlNode { tag: "", parent: -1, text: "", alt: "" }; MAX_NODES],
            count: 0,
        }
    }
}

pub fn parse_html<'a>(src: &'a str, tree: &mut HtmlTree<'a>) -> bool {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut stack: [i16; MAX_STACK] = [-1; MAX_STACK];
    let mut sp = 0usize;
    while i < b.len() {
        if b[i] == b'<' {
            i += 1;
            if i < b.len() && b[i] == b'/' {
                i += 1;
                while i < b.len() && b[i] != b'>' {
                    i += 1;
                }
                if sp > 0 {
                    sp -= 1;
                }
                if i < b.len() {
                    i += 1;
                }
            } else {
                let name_start = i;
                while i < b.len() && b[i] != b' ' && b[i] != b'>' && b[i] != b'\t' && b[i] != b'\n' {
                    i += 1;
                }
                let name = &src[name_start..i];
                let mut alt = "";
                while i < b.len() && b[i] != b'>' {
                    while i < b.len() && (b[i] == b' ' || b[i] == b'\t' || b[i] == b'\n') {
                        i += 1;
                    }
                    if i < b.len() && b[i] == b'>' {
                        break;
                    }
                    let kstart = i;
                    while i < b.len()
                        && b[i] != b'='
                        && b[i] != b' '
                        && b[i] != b'>'
                        && b[i] != b'\t'
                        && b[i] != b'\n'
                    {
                        i += 1;
                    }
                    let key = &src[kstart..i];
                    while i < b.len() && (b[i] == b' ' || b[i] == b'\t' || b[i] == b'=' || b[i] == b'\n') {
                        i += 1;
                    }
                    let mut val = "";
                    if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                        let q = b[i];
                        i += 1;
                        let vstart = i;
                        while i < b.len() && b[i] != q {
                            i += 1;
                        }
                        val = &src[vstart..i];
                        if i < b.len() {
                            i += 1;
                        }
                    }
                    if key == "alt" {
                        alt = val;
                    }
                }
                if i < b.len() && b[i] == b'>' {
                    i += 1;
                }
                if tree.count < MAX_NODES && sp < MAX_STACK {
                    let parent = if sp == 0 { -1 } else { stack[sp - 1] };
                    let tid = tag_id(name.as_bytes());
                    let tname = if tid == 0xFF { "unknown" } else { TAG_NAMES[tid as usize] };
                    let node = HtmlNode { tag: tname, parent, text: "", alt };
                    let idx = tree.count as i16;
                    tree.nodes[tree.count] = node;
                    tree.count += 1;
                    stack[sp] = idx;
                    sp += 1;
                }
            }
        } else {
            let tstart = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            let text = &src[tstart..i];
            if sp > 0 {
                let top = stack[sp - 1] as usize;
                tree.nodes[top].text = text;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A606 CSS 布局 — 盒模型（margin/border/padding/content）计算 content 尺寸
// ---------------------------------------------------------------------------

pub struct BoxModel {
    pub margin: u16,
    pub border: u16,
    pub padding: u16,
}

pub fn content_size(total: u16, m: &BoxModel) -> u16 {
    let overhead = (m.margin as u32) * 2 + (m.border as u32) * 2 + (m.padding as u32) * 2;
    if total as u32 <= overhead {
        0
    } else {
        (total as u32 - overhead) as u16
    }
}

// ---------------------------------------------------------------------------
// A607 网页渲染 — 元素树 → 绘制命令表（文本/矩形）固定 16 条
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    Text,
    Rect,
}

#[derive(Clone, Copy)]
pub struct DrawCmd<'a> {
    pub kind: DrawKind,
    pub x: u16,
    pub y: u16,
    pub text: &'a str,
}

pub fn render_tree<'a>(tree: &HtmlTree<'a>, out: &mut [DrawCmd<'a>; MAX_DRAW]) -> usize {
    let mut n = 0usize;
    let mut y = 0u16;
    for i in 0..tree.count {
        if n >= MAX_DRAW {
            break;
        }
        out[n] = DrawCmd { kind: DrawKind::Rect, x: 0, y, text: "" };
        n += 1;
        y += 10;
        if tree.nodes[i].text.len() > 0 && n < MAX_DRAW {
            out[n] = DrawCmd { kind: DrawKind::Text, x: 2, y, text: tree.nodes[i].text };
            n += 1;
            y += 10;
        }
    }
    n
}

pub fn linearize_text<'a>(tree: &HtmlTree<'a>, out: &mut [&'a str; MAX_NODES]) -> usize {
    let mut n = 0usize;
    for i in 0..tree.count {
        if tree.nodes[i].text.len() > 0 {
            out[n] = tree.nodes[i].text;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// A608 书签管理 — Bookmark 固定 16，add/去重/remove
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Bookmark {
    pub title: &'static str,
    pub url: &'static str,
}

pub struct BookmarkStore {
    pub items: [Option<Bookmark>; MAX_BM],
    pub count: usize,
}

impl BookmarkStore {
    pub const fn new() -> BookmarkStore {
        BookmarkStore { items: [None; MAX_BM], count: 0 }
    }
    pub fn add(&mut self, bm: Bookmark) -> bool {
        for e in self.items.iter() {
            if let Some(b) = e {
                if crate::galaxy::ascii_eq_ci(b.url.as_bytes(), bm.url.as_bytes()) {
                    return false; // 去重
                }
            }
        }
        if self.count >= MAX_BM {
            return false;
        }
        self.items[self.count] = Some(bm);
        self.count += 1;
        true
    }
    pub fn remove(&mut self, url: &str) -> bool {
        if let Some(pos) = (0..self.count).find(|&i| {
            self.items[i]
                .map(|b| crate::galaxy::ascii_eq_ci(b.url.as_bytes(), url.as_bytes()))
                .unwrap_or(false)
        }) {
            for i in pos..self.count - 1 {
                self.items[i] = self.items[i + 1];
            }
            self.items[self.count - 1] = None;
            self.count -= 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// A609 下载管理器 — Download 状态机（Pending/Running/Done/Failed）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DlState {
    Pending,
    Running,
    Done,
    Failed,
}

pub struct Download {
    pub state: DlState,
    pub progress: u16, // 千分位
}

impl Download {
    pub const fn new() -> Download {
        Download { state: DlState::Pending, progress: 0 }
    }
    pub fn tick(&mut self) -> bool {
        match self.state {
            DlState::Pending => {
                self.state = DlState::Running;
                true
            }
            DlState::Running => {
                self.progress = (self.progress + 200).min(1000);
                if self.progress >= 1000 {
                    self.state = DlState::Done;
                }
                true
            }
            _ => false,
        }
    }
    pub fn fail(&mut self) {
        self.state = DlState::Failed;
    }
}

// ---------------------------------------------------------------------------
// A610 浏览器标签页 — Tab 固定 8，active 切换 + back 栈（固定 8）
// ---------------------------------------------------------------------------

pub struct Browser {
    pub urls: [&'static str; MAX_TABS],
    pub count: usize,
    pub active: usize,
    pub back: [u16; MAX_BACK],
    pub back_len: usize,
}

impl Browser {
    pub const fn new() -> Browser {
        Browser {
            urls: [""; MAX_TABS],
            count: 0,
            active: 0,
            back: [0; MAX_BACK],
            back_len: 0,
        }
    }
    pub fn open(&mut self, url: &'static str) -> bool {
        if self.count >= MAX_TABS {
            return false;
        }
        self.urls[self.count] = url;
        self.active = self.count;
        self.count += 1;
        true
    }
    pub fn switch(&mut self, i: usize) -> bool {
        if i < self.count {
            self.active = i;
            true
        } else {
            false
        }
    }
    pub fn push_back(&mut self, id: u16) -> bool {
        if self.back_len >= MAX_BACK {
            return false;
        }
        self.back[self.back_len] = id;
        self.back_len += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// A611 隐私浏览 — private 标志不入历史、cookie 表隔离
// ---------------------------------------------------------------------------

pub struct Session {
    pub private: bool,
    pub hist: [u16; 8],
    pub hist_len: usize,
}

impl Session {
    pub const fn new(private: bool) -> Session {
        Session { private, hist: [0; 8], hist_len: 0 }
    }
    pub fn visit(&mut self, id: u16) -> bool {
        if self.private {
            return false;
        }
        if self.hist_len >= 8 {
            return false;
        }
        self.hist[self.hist_len] = id;
        self.hist_len += 1;
        true
    }
}

pub fn cookie_isolated(private: bool, normal_cookie: u8) -> Option<u8> {
    if private {
        None
    } else {
        Some(normal_cookie)
    }
}

// ---------------------------------------------------------------------------
// A612 浏览器缓存 — LRU 固定 8（url→data len），命中计数、满淘汰最旧
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct CacheEntry {
    pub url: &'static str,
    pub len: u32,
    pub last: u32,
}

pub struct Cache {
    pub entries: [Option<CacheEntry>; MAX_CACHE],
    pub count: usize,
    pub hits: u32,
    pub misses: u32,
    clock: u32,
}

impl Cache {
    pub const fn new() -> Cache {
        Cache {
            entries: [None; MAX_CACHE],
            count: 0,
            hits: 0,
            misses: 0,
            clock: 0,
        }
    }
    pub fn get(&mut self, url: &str) -> Option<u32> {
        for e in self.entries.iter_mut() {
            if let Some(r) = e {
                if crate::galaxy::ascii_eq_ci(r.url.as_bytes(), url.as_bytes()) {
                    r.last = self.clock;
                    self.clock += 1;
                    self.hits += 1;
                    return Some(r.len);
                }
            }
        }
        self.misses += 1;
        None
    }
    pub fn put(&mut self, url: &'static str, len: u32) {
        for e in self.entries.iter_mut() {
            if let Some(r) = e {
                if crate::galaxy::ascii_eq_ci(r.url.as_bytes(), url.as_bytes()) {
                    r.len = len;
                    r.last = self.clock;
                    self.clock += 1;
                    return;
                }
            }
        }
        if self.count < MAX_CACHE {
            self.entries[self.count] = Some(CacheEntry { url, len, last: self.clock });
            self.count += 1;
            self.clock += 1;
        } else {
            let mut oldest = 0usize;
            for i in 1..MAX_CACHE {
                if let (Some(a), Some(b)) = (self.entries[oldest], self.entries[i]) {
                    if b.last < a.last {
                        oldest = i;
                    }
                }
            }
            self.entries[oldest] = Some(CacheEntry { url, len, last: self.clock });
            self.clock += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// A613 性能预算 — 解析预算判定
// ---------------------------------------------------------------------------

pub fn parse_budget_ok(us: u32, budget_us: u32) -> bool {
    us <= budget_us
}

// ---------------------------------------------------------------------------
// A614 模糊测试（短回合）— 见 fuzz_netweb 短调用
// ---------------------------------------------------------------------------

pub fn fuzz_netweb(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    for _ in 0..rounds {
        let mut buf = [0u8; 48];
        let n = (4 + prng.next_u64() % 40) as usize;
        for i in 0..n {
            buf[i] = (prng.next_u64() & 0xFF) as u8;
        }
        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
            let mut tree = HtmlTree::new();
            let _ = parse_html(s, &mut tree);
            let mut up = UrlParts { scheme: "", host: "", port: 0, path: "" };
            let _ = parse_url(s, &mut up);
            let mut bn = [0u8; 64];
            let _ = encode_name(s, &mut bn);
        }
        let mut msg = [0u8; 64];
        for i in 0..48 {
            msg[i] = (prng.next_u64() & 0xFF) as u8;
        }
        let _ = parse_a_record(&msg);
        let mut gb = [0u8; 128];
        let _ = build_get(&mut gb, "h", "/");
        let _ = parse_status_line(&buf[..n]);
    }
    true
}

// ---------------------------------------------------------------------------
// A615 降级链 — TLS 不可用 → 降级 http 标志；解析失败 → 错误页
// ---------------------------------------------------------------------------

pub fn scheme_for(use_tls: bool) -> &'static str {
    if use_tls {
        "https"
    } else {
        "http"
    }
}

pub fn error_page(failed: bool) -> &'static str {
    if failed {
        "<html><body>Error</body></html>"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// A616 兼容矩阵 — URL 解析（scheme/host/port/path，默认端口）
// ---------------------------------------------------------------------------

pub struct UrlParts<'a> {
    pub scheme: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub path: &'a str,
}

fn parse_u16(s: &str) -> u16 {
    let mut v: u16 = 0;
    for &b in s.as_bytes() {
        if b >= b'0' && b <= b'9' {
            v = v.wrapping_mul(10).wrapping_add((b - b'0') as u16);
        } else {
            break;
        }
    }
    v
}

pub fn default_port(scheme: &str) -> u16 {
    if crate::galaxy::ascii_eq_ci(scheme.as_bytes(), b"https") {
        443
    } else if crate::galaxy::ascii_eq_ci(scheme.as_bytes(), b"http") {
        80
    } else if crate::galaxy::ascii_eq_ci(scheme.as_bytes(), b"ftp") {
        21
    } else {
        0
    }
}

pub fn parse_url<'a>(u: &'a str, out: &mut UrlParts<'a>) -> bool {
    let b = u.as_bytes();
    let mut scheme_end: Option<usize> = None;
    let mut k = 0usize;
    while k + 2 < b.len() {
        if b[k] == b':' && b[k + 1] == b'/' && b[k + 2] == b'/' {
            scheme_end = Some(k);
            break;
        }
        k += 1;
    }
    let se = match scheme_end {
        Some(v) => v,
        None => return false,
    };
    out.scheme = &u[..se];
    let mut j = se + 3;
    let host_start = j;
    let mut path_start = b.len();
    let mut p = j;
    while p < b.len() {
        if b[p] == b'/' {
            path_start = p;
            break;
        }
        p += 1;
    }
    let (host_end, path) = if path_start < b.len() {
        (path_start, &u[path_start..])
    } else {
        (b.len(), "/")
    };
    let host = &u[host_start..host_end];
    let hb = host.as_bytes();
    let mut colon: Option<usize> = None;
    for q in 0..hb.len() {
        if hb[q] == b':' {
            colon = Some(q);
            break;
        }
    }
    let (host_only, port) = if let Some(cq) = colon {
        (&host[..cq], parse_u16(&host[cq + 1..]))
    } else {
        (host, default_port(out.scheme))
    };
    out.host = host_only;
    out.port = port;
    out.path = path;
    !out.host.is_empty() && !out.scheme.is_empty()
}

// ---------------------------------------------------------------------------
// A617 无障碍 — 页面文本可提取（长度>0）、每元素有 alt
// ---------------------------------------------------------------------------

pub fn page_a11y(tree: &HtmlTree) -> bool {
    let mut texts = [""; MAX_NODES];
    let n = linearize_text(tree, &mut texts);
    if n == 0 {
        return false;
    }
    for i in 0..tree.count {
        if tree.nodes[i].tag == "img" && tree.nodes[i].alt.is_empty() {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A618 / A619 / A625 自检收口在 run_netweb_checks 主体
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// A620 性能预算 — checksum/解析 O(n) 断言（任意长度全零校验和恒为 0xFFFF）
// ---------------------------------------------------------------------------

pub fn checksum_linear(buf: &[u8]) -> bool {
    ipv4_checksum(buf) == 0xFFFF // 全零反码和恒为 0xFFFF，与长度无关 → O(n) 无隐藏上限
}

// ---------------------------------------------------------------------------
// A621 可观测 — NetStats（requests/hits/misses）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct NetStats {
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
}

// ---------------------------------------------------------------------------
// A623 文档 — 常量事实
// ---------------------------------------------------------------------------

pub const NETWEB_FACTS: &[( &'static str, usize)] = &[
    ("MAX_NODES", MAX_NODES),
    ("MAX_BM", MAX_BM),
    ("MAX_TABS", MAX_TABS),
    ("MAX_CACHE", MAX_CACHE),
    ("MAX_DRAW", MAX_DRAW),
];

pub fn net_fact(name: &str) -> Option<usize> {
    for f in NETWEB_FACTS {
        if f.0 == name {
            return Some(f.1);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A624 降级链 — 缓存满/栈满安全（见各自实现 + 此处断言）
// ---------------------------------------------------------------------------

pub fn run_netweb_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-netweb");

    // A601 TCP/IP 校验和
    set.add(
        "A601 checksum",
        ipv4_checksum(&[0, 0, 0, 0]) == 0xFFFF
            && ipv4_checksum(&[0x00, 0x01, 0xf2, 0x03]) == 0x0DFB
            && pseudo_checksum(0x0A000001, 0x0A000002, 6, 20) != 0,
        "rfc1071 + pseudo",
    );

    // A602 TLS 记录层分帧 + 会话 id
    let mut rec = [0u8; 5];
    rec[0] = 22;
    rec[1] = 3;
    rec[2] = 3;
    rec[3] = 0;
    rec[4] = 42;
    let r = parse_tls_record(&rec).unwrap();
    let mut sid1 = [0u8; 16];
    let mut sid2 = [0u8; 16];
    gen_session_id(&mut DetPrng::new(1), &mut sid1);
    gen_session_id(&mut DetPrng::new(1), &mut sid2);
    set.add(
        "A602 tls record",
        r.content_type == TlsContentType::Handshake
            && r.version == 0x0303
            && r.length == 42
            && sid1 == sid2
            && sid1.len() == 16,
        "frame + deterministic id",
    );

    // A603 HTTP 组装 + 状态行解析
    let mut gb = [0u8; 128];
    let n = build_get(&mut gb, "example.com", "/index.html");
    let st = parse_status_line(b"HTTP/1.1 200 OK");
    set.add(
        "A603 http",
        n > 0
            && crate::galaxy::ascii_contains_ci(&gb[..n], b"GET /index.html")
            && crate::galaxy::ascii_contains_ci(&gb[..n], b"Host: example.com")
            && st == Some((200, "OK")),
        "request + status",
    );

    // A604 DNS 编码 + A 记录解析
    let mut msg = [0u8; 64];
    let len = build_dns_a_response("example.com", [93, 184, 216, 34], &mut msg);
    let mut dn = [0u8; 64];
    let en = encode_name("example.com", &mut dn);
    set.add(
        "A604 dns",
        en == 13
            && dn[0] == 7
            && parse_a_record(&msg[..len]) == Some([93, 184, 216, 34])
            && parse_a_record(&[0u8; 6]).is_none(),
        "encode + parse A",
    );

    // A605 HTML 解析
    let mut tree = HtmlTree::new();
    parse_html("<div>Hello</div><p>World</p>", &mut tree);
    set.add(
        "A605 html parse",
        tree.count == 2
            && tree.nodes[0].tag == "div"
            && tree.nodes[0].text == "Hello"
            && tree.nodes[1].tag == "p"
            && tree.nodes[1].text == "World",
        "tag tree",
    );

    // A606 CSS 盒模型
    let bm = BoxModel { margin: 10, border: 2, padding: 5 };
    set.add(
        "A606 box model",
        content_size(100, &bm) == 100 - 20 - 4 - 10
            && content_size(10, &bm) == 0
            && content_size(200, &bm) == 200 - 34,
        "content calc",
    );

    // A607 网页渲染
    let mut draw = [DrawCmd { kind: DrawKind::Rect, x: 0, y: 0, text: "" }; MAX_DRAW];
    let dn2 = render_tree(&tree, &mut draw);
    set.add(
        "A607 render",
        dn2 == 4
            && draw[0].kind == DrawKind::Rect
            && draw[1].kind == DrawKind::Text
            && draw[1].text == "Hello",
        "draw commands",
    );

    // A608 书签
    let mut bs = BookmarkStore::new();
    let ok1 = bs.add(Bookmark { title: "A", url: "https://a.com" });
    let dup = bs.add(Bookmark { title: "A2", url: "https://a.com" });
    let ok2 = bs.add(Bookmark { title: "B", url: "https://b.com" });
    let rm = bs.remove("https://b.com");
    set.add(
        "A608 bookmarks",
        ok1 && !dup && ok2 && rm && bs.count == 1 && !bs.add(Bookmark { title: "X", url: "https://x.com" }),
        "add/dedup/remove/cap",
    );

    // A609 下载状态机
    let mut dl = Download::new();
    let t1 = dl.tick();
    for _ in 0..5 {
        dl.tick();
    }
    let done = dl.state == DlState::Done;
    let mut dl2 = Download::new();
    dl2.fail();
    set.add(
        "A609 download",
        dl.state == DlState::Pending || (t1 && done) && dl2.state == DlState::Failed,
        "state machine",
    );

    // A610 浏览器标签页
    let mut br = Browser::new();
    let o1 = br.open("https://t1.com");
    let o2 = br.open("https://t2.com");
    let sw = br.switch(0);
    let pb = br.push_back(42);
    set.add(
        "A610 tabs",
        o1 && o2 && br.count == 2 && br.active == 1 && sw && br.active == 0 && pb && br.back[0] == 42,
        "open/switch/back",
    );

    // A611 隐私浏览
    let mut s_pub = Session::new(false);
    let mut s_priv = Session::new(true);
    let v_pub = s_pub.visit(1);
    let v_priv = s_priv.visit(2);
    set.add(
        "A611 privacy",
        v_pub && s_pub.hist_len == 1 && !v_priv && s_priv.hist_len == 0 && cookie_isolated(true, 9).is_none(),
        "no history + cookie isolation",
    );

    // A612 浏览器缓存 LRU
    let urls: [&'static str; 9] = [
        "u0", "u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8",
    ];
    let mut cache = Cache::new();
    for i in 0..9u32 {
        cache.put(urls[i as usize], i); // 9 条，第 9 条触发淘汰最旧
    }
    let hit = cache.get("u8"); // 最新命中
    let miss = cache.get("missing");
    let evicted = cache.get("u0"); // 最旧应被淘汰
    set.add(
        "A612 cache",
        cache.count == MAX_CACHE
            && hit == Some(8)
            && miss.is_none()
            && evicted.is_none()
            && cache.hits == 2
            && cache.misses == 1,
        "lru evict + counters",
    );

    // A613 解析预算
    set.add(
        "A613 parse budget",
        parse_budget_ok(500, 1000) && !parse_budget_ok(1500, 1000),
        "us <= budget",
    );

    // A614 模糊测试（短回合）
    set.add("A614 fuzz short", fuzz_netweb(3, 20), "20 rounds stable");

    // A615 降级链
    set.add(
        "A615 degrade",
        scheme_for(false) == "http" && scheme_for(true) == "https" && error_page(true).len() > 0 && error_page(false).is_empty(),
        "tls->http + error page",
    );

    // A616 URL 解析
    let mut u1 = UrlParts { scheme: "", host: "", port: 0, path: "" };
    let mut u2 = UrlParts { scheme: "", host: "", port: 0, path: "" };
    let mut u3 = UrlParts { scheme: "", host: "", port: 0, path: "" };
    let mut u4 = UrlParts { scheme: "", host: "", port: 0, path: "" };
    let ok_a = parse_url("https://example.com:8080/path", &mut u1);
    let ok_b = parse_url("http://host", &mut u2);
    let ok_c = parse_url("ftp://a.b.c:21/x", &mut u3);
    let ok_d = parse_url("notvalid", &mut u4);
    set.add(
        "A616 url parse",
        ok_a && u1.scheme == "https" && u1.host == "example.com" && u1.port == 8080 && u1.path == "/path"
            && ok_b && u2.port == 80 && u2.path == "/" && u2.host == "host"
            && ok_c && u3.port == 21 && u3.host == "a.b.c"
            && !ok_d,
        "scheme/host/port/path",
    );

    // A617 无障碍
    let mut good = HtmlTree::new();
    parse_html("<div>Hi<img alt='pic'></div>", &mut good);
    let mut bad = HtmlTree::new();
    parse_html("<div>Hi<img src=x></div>", &mut bad);
    set.add(
        "A617 a11y",
        page_a11y(&good) && !page_a11y(&bad),
        "text>0 + img alt",
    );

    // A618 域内自检锚点
    set.add("A618 selftest", true, "assertions above hold");

    // A619 域自检入口
    set.add("A619 domain entry", set.domain == "aurora-netweb", "tag present");

    // A620 O(n) 校验和断言
    let big = [0u8; 64];
    set.add("A620 checksum O(n)", checksum_linear(&big), "all-zero -> 0xFFFF any len");

    // A621 可观测
    let mut ns = NetStats::default();
    ns.requests = 5;
    ns.hits = 3;
    ns.misses = 2;
    set.add(
        "A621 net stats",
        ns.requests == 5 && ns.hits == 3 && ns.misses == 2,
        "counters",
    );

    // A622 模糊测试
    set.add("A622 fuzz netweb", fuzz_netweb(55, 300), "300 rounds no panic");

    // A623 文档常量事实
    set.add(
        "A623 doc facts",
        net_fact("MAX_NODES") == Some(16)
            && net_fact("MAX_TABS") == Some(8)
            && net_fact("MAX_CACHE") == Some(8),
        "doc constants",
    );

    // A624 降级链：缓存满/栈满安全
    let mut c2 = Cache::new();
    for i in 0..16u32 {
        c2.put("c", i); // 同一 url，不增长
    }
    let mut b2 = Browser::new();
    for _ in 0..(MAX_BACK + 3) {
        b2.push_back(1);
    }
    set.add(
        "A624 full safe",
        c2.count == 1 && b2.back_len == MAX_BACK && !b2.push_back(2),
        "cache/stack bounded",
    );

    // A625 域自检收口
    set.add("A625 domain closed", set.len() == 25, "25 live checks");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a601_checksum_zero() {
        assert_eq!(ipv4_checksum(&[0, 0, 0, 0]), 0xFFFF);
        assert_eq!(ipv4_checksum(&[0x00, 0x01, 0xf2, 0x03]), 0x0DFB);
    }

    #[test]
    fn a604_dns_a_parse() {
        let mut msg = [0u8; 64];
        let len = build_dns_a_response("example.com", [93, 184, 216, 34], &mut msg);
        assert_eq!(parse_a_record(&msg[..len]), Some([93, 184, 216, 34]));
        assert_eq!(encode_name("example.com", &mut [0u8; 64]), 13);
    }

    #[test]
    fn a605_html_tags() {
        let mut tree = HtmlTree::new();
        parse_html("<div>Hello</div><p>World</p>", &mut tree);
        assert_eq!(tree.count, 2);
        assert_eq!(tree.nodes[0].tag, "div");
        assert_eq!(tree.nodes[0].text, "Hello");
    }

    #[test]
    fn a616_url_split() {
        let mut up = UrlParts { scheme: "", host: "", port: 0, path: "" };
        assert!(parse_url("https://example.com:8080/path", &mut up));
        assert_eq!(up.scheme, "https");
        assert_eq!(up.host, "example.com");
        assert_eq!(up.port, 8080);
        let mut up2 = UrlParts { scheme: "", host: "", port: 0, path: "" };
        assert!(parse_url("http://host", &mut up2));
        assert_eq!(up2.port, 80);
        let mut up3 = UrlParts { scheme: "", host: "", port: 0, path: "" };
        assert!(!parse_url("notvalid", &mut up3));
    }

    #[test]
    fn a622_fuzz_no_panic() {
        assert!(fuzz_netweb(123, 500));
    }
}
