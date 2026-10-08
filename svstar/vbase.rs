//! vbase — Varix STAR I · 泳道四 D 服务守护域（AI-V1 · F111-F130）共享底盘。
//!
//! 二十项功能共用六件基础设施，全部零外部依赖、宿主测试直跑、内核
//! 镜像（no_std + alloc）可编译：
//!
//! - [`sha256`] 纯自研 SHA-256——F121 快照块哈希 / F127 产物哈希与签名 /
//!   F128 数据面签名 / F129 证据哈希链共用同一条散列通路（一处一事实）；
//! - [`hex`] 定容十六进制编码——哈希产物可读化（诊断面诚实记账）；
//! - [`contrast`] WCAG 2.1 相对亮度与对比度比——F113 高对比度 ≥7:1
//!   全表实测的唯一算法源；
//! - [`kelvin`] 色温→RGB（Tanner Helland 公开近似算法）——F116 夜间模式
//!   伽马表与色度计对拍的换算层；
//! - [`semver`] 语义版本三元组解析与比较——F126 版本化承诺 / F127 格式
//!   版本戳 / F130 登记册版本列共用；
//! - [`jsonw`] 定容 JSON 转义写出器——F126 schema 示例包 / F128 星图
//!   JSON 快照的结构化产出面；
//! - [`sanitize`] 脱敏三查（路径用户段/序列号/密钥类）——F120 导出 /
//!   F129 提交线的固定三查判据（规则一处一事实，两处消费不各写一套）。
//!
//! 时间纪律：与 STAR-K2 sbase 同款——**一切时间由调用方以参数注入**，
//! 模块不持真实时钟，宿主测试确定复现。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// SHA-256（FIPS 180-4，纯自研——F127 主册「ed25519/minisign 公共密码学」
// 评估的散列地基；签名面语义在 vxapp 模块内实装，算法替换点登记 F130）
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 上下文：流式喂块，产出 32 字节摘要。
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

impl Sha256 {
    pub const fn new() -> Sha256 {
        Sha256 {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0; 64],
            buf_len: 0,
            total: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
        self.h[5] = self.h[5].wrapping_add(f);
        self.h[6] = self.h[6].wrapping_add(g);
        self.h[7] = self.h[7].wrapping_add(h);
    }

    /// 喂入一段数据。
    pub fn update(&mut self, data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        let mut pos = 0;
        while pos < data.len() {
            let take = (64 - self.buf_len).min(data.len() - pos);
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[pos..pos + take]);
            self.buf_len += take;
            pos += take;
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
    }

    /// 收口产出 32 字节摘要（上下文复位，可复用）。
    pub fn finalize(&mut self) -> [u8; 32] {
        let bit_len = self.total.wrapping_mul(8);
        // padding：0x80 + 零 + 64bit 长度
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let pad_len = if self.buf_len < 56 { 56 - self.buf_len } else { 120 - self.buf_len };
        self.update_pad(&pad[..pad_len]);
        let len_bytes = bit_len.to_be_bytes();
        self.update_pad(&len_bytes);
        debug_assert_eq!(self.buf_len, 0);
        let mut out = [0u8; 32];
        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_be_bytes());
        }
        out
    }

    /// padding 专喂：走 update 同一套缓冲逻辑但不计入 total。
    fn update_pad(&mut self, data: &[u8]) {
        let mut pos = 0;
        while pos < data.len() {
            let take = (64 - self.buf_len).min(data.len() - pos);
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[pos..pos + take]);
            self.buf_len += take;
            pos += take;
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
    }
}

/// 一次性摘要便捷口。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

/// 双轮散列链（F129 证据上链语义：`h_i = H(h_{i-1} || chunk)`）。
pub fn chain_hash(prev: &[u8; 32], chunk: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(chunk);
    h.finalize()
}

// ---------------------------------------------------------------------------
// hex — 定容十六进制编码
// ---------------------------------------------------------------------------

/// 32 字节摘要 → 64 字符小写 hex（写入调用方缓冲，返回写入字节数；
/// 缓冲不足如实返回 0 不写半截——诚实失败）。
pub fn hex32(digest: &[u8; 32], out: &mut [u8]) -> usize {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    if out.len() < 64 {
        return 0;
    }
    for (i, b) in digest.iter().enumerate() {
        out[i * 2] = HEXD[(b >> 4) as usize];
        out[i * 2 + 1] = HEXD[(b & 0xf) as usize];
    }
    64
}

/// 32 字节摘要 → String（诊断/报告面用；热路径禁用）。
pub fn hex32_str(digest: &[u8; 32]) -> String {
    let mut buf = [0u8; 64];
    let n = hex32(digest, &mut buf);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

// ---------------------------------------------------------------------------
// contrast — WCAG 2.1 相对亮度与对比度比（F113 唯一算法源）
// ---------------------------------------------------------------------------

/// sRGB 8bit 通道 → 线性化（WCAG 2.1 公式）。
fn srgb_channel(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// 相对亮度 L（WCAG 2.1：0.2126R + 0.7152G + 0.0722B，线性域）。
pub fn relative_luminance(rgb: (u8, u8, u8)) -> f64 {
    0.2126 * srgb_channel(rgb.0) + 0.7152 * srgb_channel(rgb.1) + 0.0722 * srgb_channel(rgb.2)
}

/// 对比度比 (L1+0.05)/(L2+0.05)，返回放大 100 倍的整数（7:1 = 700）——
/// 判线比较用整数语义（≥700 过 WCAG AAA），避免浮点直等。
pub fn contrast_ratio_x100(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    ((hi + 0.05) / (lo + 0.05) * 100.0).round() as u32
}

// ---------------------------------------------------------------------------
// kelvin — 色温→RGB（Tanner Helland 公开近似算法，F116 标注）
// ---------------------------------------------------------------------------

/// 开尔文色温（1000K-40000K 有效域）→ 8bit RGB 三元组。
/// 越界输入钳到边界（不静默吞：调用方可由返回值判端点）。
pub fn kelvin_to_rgb(k: u32) -> (u8, u8, u8) {
    let t = (k.max(1000).min(40000) as f64) / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698727446 * (t - 60.0).powf(-0.1332047592)
    };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    (
        r.max(0.0).min(255.0).round() as u8,
        g.max(0.0).min(255.0).round() as u8,
        b.max(0.0).min(255.0).round() as u8,
    )
}

// ---------------------------------------------------------------------------
// semver — 语义版本三元组（F126/F127/F130 共用）
// ---------------------------------------------------------------------------

/// 解析 `major.minor.patch`（允许尾部冗余段忽略）。非法输入返回 None——
/// 调用方按「用户错」处理，不静默归零。
pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = [0u32; 3];
    let mut seen = 0;
    for seg in s.split('.') {
        if seen >= 3 {
            break;
        }
        if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // 段值超 u32 视为非法（防注入超长数字）。
        match seg.parse::<u32>() {
            Ok(v) => {
                parts[seen] = v;
                seen += 1;
            }
            Err(_) => return None,
        }
    }
    if seen != 3 {
        return None;
    }
    Some((parts[0], parts[1], parts[2]))
}

/// 版本比较：a > b 返回 Greater；破格判定（主版本变更）由调用方据 major 差。
pub fn semver_cmp(a: (u32, u32, u32), b: (u32, u32, u32)) -> core::cmp::Ordering {
    a.cmp(&b)
}

// ---------------------------------------------------------------------------
// jsonw — 定容 JSON 转义写出器（F126/F128 结构化产出面）
// ---------------------------------------------------------------------------

/// 把字符串转义为 JSON 字符串体（含引号）。控制字符与引号/反斜杠全转义；
/// 非 ASCII 原样透传（UTF-8 合法）。热路径外使用（返回 String）。
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&alloc::format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 最小 JSON 快照构建器：按序追加字段，收口产出合法 JSON 对象文本。
/// 只做「扁平对象 + 嵌套数组值」两种形态（F126/F128 快照够用；复杂结构
/// 由调用方多次拼接——保持写出器零递归零 panic）。
#[derive(Default)]
pub struct JsonObj {
    fields: Vec<String>,
}

impl JsonObj {
    pub fn new() -> JsonObj {
        JsonObj { fields: Vec::new() }
    }

    pub fn str_field(&mut self, key: &str, val: &str) {
        self.fields.push(alloc::format!(
            "{}:{}",
            json_escape(key),
            json_escape(val)
        ));
    }

    pub fn num_field(&mut self, key: &str, val: u64) {
        self.fields
            .push(alloc::format!("{}:{}", json_escape(key), val));
    }

    pub fn bool_field(&mut self, key: &str, val: bool) {
        self.fields
            .push(alloc::format!("{}:{}", json_escape(key), if val { "true" } else { "false" }));
    }

    /// 数组字段：元素为已转义字符串列表。
    pub fn str_array_field(&mut self, key: &str, vals: &[String]) {
        let mut s = String::from("[");
        for (i, v) in vals.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&json_escape(v));
        }
        s.push(']');
        self.fields.push(alloc::format!("{}:{}", json_escape(key), s));
    }

    /// 原样数组字段：元素为已序列化 JSON 片段（嵌套对象/数组透传
    /// ——starmap 快照的星卡数组语义；调用方保证片段合法）。
    pub fn raw_array_field(&mut self, key: &str, vals: &[String]) {
        let mut s = String::from("[");
        for (i, v) in vals.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(v);
        }
        s.push(']');
        self.fields.push(alloc::format!("{}:{}", json_escape(key), s));
    }

    /// 收口：`{"k":v,...}`。
    pub fn finish(self) -> String {
        let mut s = String::from("{");
        for (i, f) in self.fields.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(f);
        }
        s.push('}');
        s
    }
}

// ---------------------------------------------------------------------------
// sanitize — 脱敏三查（F120 导出 / F129 提交线固定三查，规则唯一源）
// ---------------------------------------------------------------------------

/// 三查命中类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveKind {
    /// 路径用户段（C:\Users\<name>\…）。
    UserSegment,
    /// 序列号样式（连续 ≥8 位字母数字含 SN 前缀语境）。
    SerialLike,
    /// 密钥类（key/token/secret/password 语境 + 赋值形态）。
    KeyLike,
}

/// 对一行文本跑三查。命中返回 Some(类别)（首个命中即返回——按序
/// 用户段→序列号→密钥类，顺序即主册列举序）。
pub fn sanitize_hit(line: &str) -> Option<SensitiveKind> {
    let lower = line.to_ascii_lowercase();
    // 查一：路径用户段。匹配 "users\<名>\" 或 "users/<名>/"。
    if let Some(pos) = lower.find("users") {
        let rest = &line[pos + 5..];
        let rest_b = rest.as_bytes();
        if rest_b.len() >= 3
            && (rest_b[0] == b'\\' || rest_b[0] == b'/')
        {
            let mut name_len = 0;
            for &c in &rest_b[1..] {
                if c == b'\\' || c == b'/' {
                    break;
                }
                name_len += 1;
            }
            // 用户段后还有分隔符 → 是真实路径用户目录（排除 "users" 词尾）。
            if name_len > 0 && rest_b.len() > 1 + name_len {
                return Some(SensitiveKind::UserSegment);
            }
        }
    }
    // 查二：序列号样式。sn/serial/序列号语境 + 等值后连续 ≥8 位。
    for kw in ["sn=", "sn:", "serial", "序列号"] {
        if let Some(pos) = lower.find(kw) {
            let tail = &line[(pos + kw.len()).min(line.len())..];
            let digits: String = tail
                .chars()
                .skip_while(|c| !c.is_ascii_alphanumeric())
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if digits.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 8 {
                return Some(SensitiveKind::SerialLike);
            }
        }
    }
    // 查三：密钥类。key/token/secret/password 语境 + 赋值/冒号形态且值非空。
    for kw in ["key", "token", "secret", "password", "密钥", "口令"] {
        if let Some(pos) = lower.find(kw) {
            let tail = &line[(pos + kw.len()).min(line.len())..];
            let trimmed = tail.trim_start();
            if trimmed.starts_with('=') || trimmed.starts_with(':') {
                let val = trimmed[1..].trim();
                if !val.is_empty() {
                    return Some(SensitiveKind::KeyLike);
                }
            }
        }
    }
    None
}

/// 脱敏替换：命中段统一替换为 `[已脱敏]`（F120 导出包 manifest 声明同款措辞）。
pub fn sanitize_line(line: &str) -> String {
    match sanitize_hit(line) {
        Some(_) => String::from("[已脱敏]"),
        None => String::from(line),
    }
}

// ---------------------------------------------------------------------------
// 定容字符串截断（诊断 detail 诚实截断语义）
// ---------------------------------------------------------------------------

/// 截到 `max` 字符（按 char 边界，不劈 UTF-8），超出追加省略号计数。
pub fn trunc_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return String::from(s);
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// 自检（vbase 自身判据：算法正确性是全域地基——红了整域停工）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

pub fn run_vbase_checks() -> CheckSet {
    let mut set = CheckSet::new("vbase-aiv1");

    // SHA-256 标准测试向量（FIPS 180-4 附录 B）："abc"。
    let d = sha256(b"abc");
    set.add(
        "sha256 abc vector",
        &d == &[
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ],
        "",
    );

    // 空串向量。
    let d = sha256(b"");
    set.add(
        "sha256 empty vector",
        &d == &[
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ],
        "",
    );

    // 流式喂入（分块）与一次性等值。
    let mut h = Sha256::new();
    h.update(b"ab");
    h.update(b"c");
    set.add("sha256 streaming equals oneshot", h.finalize() == sha256(b"abc"), "");

    // hex 编码定容诚实失败。
    let mut buf = [0u8; 63];
    set.add("hex short buffer rejects", hex32(&d, &mut buf) == 0, "");

    // WCAG 对比度：黑白 = 21:1；黑灰(#777) ≈ 4.69（AAA 线 700 不过；
    // 4.48 是 #777 对白底的官方示例值——两种对拍分开记）。
    set.add(
        "contrast black/white 2100",
        contrast_ratio_x100((0, 0, 0), (255, 255, 255)) == 2100,
        "",
    );
    set.add(
        "contrast black/gray777 ~469",
        (contrast_ratio_x100((0, 0, 0), (0x77, 0x77, 0x77)) as i32 - 469).abs() <= 2,
        "",
    );

    // 色温端点：6500K 近纯白、1900K 暖（B 分量显著低于 R）。
    let (r6, _g6, b6) = kelvin_to_rgb(6500);
    let (_, _, b1) = kelvin_to_rgb(1900);
    let (r1, _, _) = kelvin_to_rgb(1900);
    set.add(
        "kelvin 6500 near white, 1900 warm",
        b6 >= 240 && r6 >= 250 && r1 > b1 && b1 < 80,
        "",
    );

    // semver 非法输入显式拒绝。
    set.add(
        "semver parse",
        parse_semver("1.2.3") == Some((1, 2, 3))
            && parse_semver("1.2") == None
            && parse_semver("a.b.c") == None
            && parse_semver("999999999999.0.0") == None,
        "",
    );

    // JSON 转义。
    set.add(
        "json escape",
        json_escape("a\"b\\c\nd") == "\"a\\\"b\\\\c\\nd\"",
        "",
    );

    // 脱敏三查逐条命中与放行。
    set.add(
        "sanitize user segment",
        sanitize_hit(r"C:\Users\variable\doc.txt") == Some(SensitiveKind::UserSegment),
        "",
    );
    set.add(
        "sanitize serial",
        sanitize_hit("SN=AB12CD34EF56") == Some(SensitiveKind::SerialLike),
        "",
    );
    set.add(
        "sanitize key",
        sanitize_hit("api_key=9f8e7d6c") == Some(SensitiveKind::KeyLike),
        "",
    );
    set.add(
        "sanitize clean line passes",
        sanitize_hit("帧率 80fps 正常") == None,
        "",
    );
    set.add(
        "sanitize replace",
        sanitize_line("SN=AB12CD34EF56") == "[已脱敏]",
        "",
    );

    // 哈希链：链式与整体一次散列可区分（链式语义成立）。
    let h0 = sha256(b"genesis");
    let h1 = chain_hash(&h0, b"chunk-1");
    let h2 = chain_hash(&h1, b"chunk-2");
    set.add(
        "chain hash progressive",
        h1 != h0 && h2 != h1 && h2 != h0,
        "",
    );

    // 截断不劈 UTF-8。
    set.add(
        "trunc utf8 safe",
        trunc_chars("中文测试文本", 3) == "中文测…",
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vbase_self_checks_green() {
        let set = run_vbase_checks();
        assert!(set.all_passed(), "vbase 自检存在红项——算法地基不牢，全域停工");
    }

    #[test]
    fn sha256_long_input() {
        // 长度跨越多块 + 尾部 padding 双分支（56 边界两侧）。
        let a = sha256(&[0x61u8; 55]);
        let b = sha256(&[0x61u8; 56]);
        let c = sha256(&[0x61u8; 64]);
        assert_ne!(a, b);
        assert_ne!(b, c);
        // 55/56/64 各自确定且可复现。
        assert_eq!(a, sha256(&[0x61u8; 55]));
    }

    #[test]
    fn contrast_known_values() {
        // WCAG 官方示例：#777 vs #fff ≈ 4.48:1。
        let r = contrast_ratio_x100((0x77, 0x77, 0x77), (255, 255, 255));
        assert!((r as i32 - 448).abs() <= 2, "got {}", r);
        // 同色对比 = 1:1。
        assert_eq!(contrast_ratio_x100((16, 16, 16), (16, 16, 16)), 100);
    }

    #[test]
    fn sanitize_edge_cases() {
        // "users" 词尾不误命中。
        assert_eq!(sanitize_hit("the users are happy"), None);
        // 空值 key 不命中。
        assert_eq!(sanitize_hit("key="), None);
        // 序列号不足 8 位不命中。
        assert_eq!(sanitize_hit("SN=AB12"), None);
    }

    #[test]
    fn jsonobj_roundtrip() {
        let mut o = JsonObj::new();
        o.str_field("name", "星卡");
        o.num_field("stars", 3);
        o.bool_field("ok", true);
        o.str_array_field("tags", &[String::from("a"), String::from("b")]);
        let s = o.finish();
        assert_eq!(s, "{\"name\":\"星卡\",\"stars\":3,\"ok\":true,\"tags\":[\"a\",\"b\"]}");
    }
}
