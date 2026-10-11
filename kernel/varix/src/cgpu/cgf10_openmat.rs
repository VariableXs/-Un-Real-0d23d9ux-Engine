//! cgf10_openmat — CGPU-F0808 认证数据开放（认证矩阵是公共资产，不是黑箱）。
//!
//! 判据原文（施工书锚点 CGPU-F0808）：开放：认证结果公开可查（认证矩阵的全
//! 量公开——每硬件的等级/依据/日期；透明纪律）；开放格式（JSON 查询与导出）；
//! 开放边界（不含用户隐私数据——认证数据全是硬件与度量）；数据防篡改（认证
//! 数据签名——复用 B07/F0266 验签理念）。判据：全量公开、开放格式、隐私边
//! 界、防篡改、判据。
//!
//! 职责定位：认证矩阵条目仓（等级/依据/日期/度量四元组全量可查）+ no_std
//! JSON 序列化导出器（零依赖手写，转义完备）+ 字段白名单查询面（开放边界
//! 落地——白名单外字段名一律拒绝并计数，而不是静默忽略）+ FNV-HMAC 式签名
//! 与验签（写时签名、读时验签，篡改即抛诊断码）。与 cgf05_certsuite（考
//! 卷）、cgf08_certvalid（时效）分工：彼产出判定与有效期，此把已判定结果
//! 开放出去。诊断码段 0x5001~0x5005。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const OPEN_MAT_VERSION: &str = "F10-openmat-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x50 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_TAMPER_DETECTED: u32 = 0x5001; // 验签失败（数据被篡改）
pub const XC_FIELD_NOT_ALLOWED: u32 = 0x5002; // 字段不在白名单（开放边界拦截）
pub const XC_ENTRY_NOT_FOUND: u32 = 0x5003; // 硬件条目不存在
pub const XC_JSON_MALFORMED: u32 = 0x5004; // 导出 JSON 结构损坏（自检路径）
pub const XC_SIG_KEY_ABSENT: u32 = 0x5005; // 签名密钥未装载（拒绝写签名空承诺）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 签名域分隔符：防跨域签名重放（等级签名 ≠ 全文签名）。
pub const SIG_DOMAIN_FULL: u64 = 0x4F50_5542_4C49_4301; // "OPUBLIC" 语义词
pub const SIG_DOMAIN_QUERY: u64 = 0x4F50_5542_4C49_4302;

/// 白名单字段（判据"开放边界"——认证数据全是硬件与度量，字段闭集）。
pub const ALLOWED_FIELDS: [&str; 8] = [
    "hardware_id",
    "model_name",
    "grade",
    "basis",
    "cert_date",
    "valid_until",
    "metrics",
    "signature",
];

/// 用户隐私字段黑名单示例（任何匹配即硬拒——双保险在白名单之上）。
pub const FORBIDDEN_SUBSTR: [&str; 6] = [
    "user", "owner", "email", "phone", "machine_name", "serial_person",
];

/// 等级闭集（与 cgf07_certgrade 一致的四级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertGrade {
    Gold,
    Silver,
    Bronze,
    Uncertified,
}

impl CertGrade {
    pub const fn code(self) -> &'static str {
        match self {
            CertGrade::Gold => "gold",
            CertGrade::Silver => "silver",
            CertGrade::Bronze => "bronze",
            CertGrade::Uncertified => "uncertified",
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            CertGrade::Gold => "金级",
            CertGrade::Silver => "银级",
            CertGrade::Bronze => "铜级",
            CertGrade::Uncertified => "未认证",
        }
    }
}

/// 认证矩阵条目（判据"每硬件的等级/依据/日期"）。
#[derive(Clone, Debug)]
pub struct CertMatrixEntry {
    /// 硬件标识（vid:pid:rev 规范串，不含个体序列号——隐私边界）。
    pub hardware_id: String,
    /// 型号名（营销名，公开语义）。
    pub model_name: String,
    /// 认证等级。
    pub grade: CertGrade,
    /// 认证依据（考卷版本+判据条目号串）。
    pub basis: String,
    /// 认证日期（ISO 日期串）。
    pub cert_date: String,
    /// 有效期至（ISO 日期串，空串=长期）。
    pub valid_until: String,
    /// 度量（key=value 分号串，全为性能/一致性度量）。
    pub metrics: String,
    /// 全文签名（写时计算，读时验证）。
    pub signature: u64,
}

impl CertMatrixEntry {
    /// 签名载荷（域分隔+全字段拼接——FNV-HMAC 式）。
    pub fn signing_payload(&self) -> u64 {
        let mut h: u64 = SIG_DOMAIN_FULL;
        h = fnv_mix(h, self.hardware_id.as_bytes());
        h = fnv_mix(h, self.model_name.as_bytes());
        h = fnv_mix(h, self.grade.code().as_bytes());
        h = fnv_mix(h, self.basis.as_bytes());
        h = fnv_mix(h, self.cert_date.as_bytes());
        h = fnv_mix(h, self.valid_until.as_bytes());
        h = fnv_mix(h, self.metrics.as_bytes());
        h
    }

    /// 字段是否可公开导出（白名单+黑名单双保险）。
    pub fn field_exportable(field: &str) -> bool {
        let mut allowed = false;
        let mut i = 0;
        while i < ALLOWED_FIELDS.len() {
            if ALLOWED_FIELDS[i] == field {
                allowed = true;
                break;
            }
            i += 1;
        }
        if !allowed {
            return false;
        }
        // 黑名单子串扫描（防未来误把隐私字段加进白名单）。
        let mut j = 0;
        while j < FORBIDDEN_SUBSTR.len() {
            if contains_ci(field, FORBIDDEN_SUBSTR[j]) {
                return false;
            }
            j += 1;
        }
        true
    }
}

/// FNV-1a 混合步骤。
pub const fn fnv_step(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x1000_0000_01b3)
}

pub fn fnv_mix(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < data.len() {
        h = fnv_step(h, data[i]);
        i += 1;
    }
    h
}

/// 大小写不敏感子串包含（白名单黑名单扫描用）。
pub fn contains_ci(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.len() == 0 || hb.len() < nb.len() {
        return nb.len() == 0;
    }
    let mut i = 0;
    while i + nb.len() <= hb.len() {
        let mut matched = true;
        let mut j = 0;
        while j < nb.len() {
            if hb[i + j].to_ascii_lowercase() != nb[j].to_ascii_lowercase() {
                matched = false;
                break;
            }
            j += 1;
        }
        if matched {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// JSON 导出器（开放格式判据——no_std 手写，零依赖）
// ---------------------------------------------------------------------------

/// JSON 字符串转义（引号/反斜杠/控制字符）。
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                out.push('\\');
                out.push('"');
            }
            b'\\' => {
                out.push('\\');
                out.push('\\');
            }
            b'\n' => {
                out.push('\\');
                out.push('n');
            }
            b'\r' => {
                out.push('\\');
                out.push('r');
            }
            b'\t' => {
                out.push('\\');
                out.push('t');
            }
            b if b < 0x20 => {
                // 控制字符统一 \u00XX。
                out.push('\\');
                out.push('u');
                out.push('0');
                out.push('0');
                let hi = b / 16;
                let lo = b % 16;
                out.push(hex_digit(hi));
                out.push(hex_digit(lo));
            }
            b => {
                out.push(b as char); // UTF-8 字节直通（JSON 合法）
            }
        }
        i += 1;
    }
    out
}

pub const fn hex_digit(v: u8) -> char {
    match v {
        0..=9 => (b'0' + v) as char,
        _ => (b'A' + (v - 10)) as char,
    }
}

/// 条目导出为 JSON 对象（只导白名单字段）。
pub fn entry_to_json(e: &CertMatrixEntry) -> String {
    let mut out = String::with_capacity(256);
    out.push('{');
    out.push_str("\"hardware_id\":\"");
    out.push_str(&json_escape(&e.hardware_id));
    out.push_str("\",\"model_name\":\"");
    out.push_str(&json_escape(&e.model_name));
    out.push_str("\",\"grade\":\"");
    out.push_str(e.grade.code());
    out.push_str("\",\"basis\":\"");
    out.push_str(&json_escape(&e.basis));
    out.push_str("\",\"cert_date\":\"");
    out.push_str(&json_escape(&e.cert_date));
    out.push_str("\",\"valid_until\":\"");
    out.push_str(&json_escape(&e.valid_until));
    out.push_str("\",\"metrics\":\"");
    out.push_str(&json_escape(&e.metrics));
    out.push_str("\",\"signature\":\"");
    out.push_str(&u64_hex_real(e.signature));
    out.push_str("\"}");
    out
}

/// u64 转 16 字符大写十六进制（签名串表示）。
pub fn u64_hex_real(v: u64) -> String {
    let mut buf = [0u8; 16];
    let mut i = 16;
    let mut x = v;
    while i > 0 {
        i -= 1;
        buf[i] = hex_digit((x & 0xF) as u8) as u8;
        x >>= 4;
    }
    let mut s = String::with_capacity(16);
    let mut j = 0;
    while j < 16 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 矩阵仓：全量公开 + 查询 + 防篡改
// ---------------------------------------------------------------------------

/// 认证矩阵开放仓。
pub struct OpenMatrix {
    entries: Vec<CertMatrixEntry>,
    sig_key: Option<u64>,
    /// 遥测计数。
    pub blocked_field_queries: u64,
    pub tamper_events: u64,
    pub export_count: u64,
    pub query_count: u64,
}

impl OpenMatrix {
    pub fn new() -> Self {
        OpenMatrix {
            entries: Vec::new(),
            sig_key: None,
            blocked_field_queries: 0,
            tamper_events: 0,
            export_count: 0,
            query_count: 0,
        }
    }

    /// 装载签名密钥（未装载则拒绝写入——防"签名空承诺"）。
    pub fn load_key(&mut self, key: u64) {
        self.sig_key = Some(key);
    }

    fn keyed(&self, payload: u64) -> Result<u64, u32> {
        match self.sig_key {
            Some(k) => Ok(fnv_mix(payload.wrapping_add(k), b"VX-CERT-SIG")),
            None => Err(XC_SIG_KEY_ABSENT),
        }
    }

    /// 登记条目（写时签名）。
    pub fn publish(&mut self, mut e: CertMatrixEntry) -> Result<u64, u32> {
        if !CertMatrixEntry::field_exportable("hardware_id") {
            // 白名单自检异常（理论不可达，防御性）。
            return Err(XC_FIELD_NOT_ALLOWED);
        }
        // 隐私边界：硬件串含黑名单子串直接拒收。
        let mut f = 0;
        while f < FORBIDDEN_SUBSTR.len() {
            if contains_ci(&e.hardware_id, FORBIDDEN_SUBSTR[f])
                || contains_ci(&e.model_name, FORBIDDEN_SUBSTR[f])
            {
                return Err(XC_FIELD_NOT_ALLOWED);
            }
            f += 1;
        }
        let payload = e.signing_payload();
        e.signature = self.keyed(payload)?;
        // 同 hardware_id 覆盖写（矩阵语义：每硬件一条）。
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].hardware_id == e.hardware_id {
                self.entries[i] = e.clone();
                return Ok(self.entries[i].signature);
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(self.entries[self.entries.len() - 1].signature)
    }

    /// 按硬件 ID 查询（读时验签）。
    pub fn query(&mut self, hardware_id: &str) -> Result<(CertGrade, String, String), u32> {
        self.query_count += 1;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].hardware_id == hardware_id {
                let payload = self.entries[i].signing_payload();
                let expect = self.keyed(payload)?;
                if expect != self.entries[i].signature {
                    self.tamper_events += 1;
                    return Err(XC_TAMPER_DETECTED);
                }
                let e = &self.entries[i];
                return Ok((e.grade, e.basis.clone(), e.cert_date.clone()));
            }
            i += 1;
        }
        Err(XC_ENTRY_NOT_FOUND)
    }

    /// 白名单字段查询（开放边界的运行时执行点）。
    pub fn query_field(&mut self, hardware_id: &str, field: &str) -> Result<String, u32> {
        if !CertMatrixEntry::field_exportable(field) {
            self.blocked_field_queries += 1;
            return Err(XC_FIELD_NOT_ALLOWED);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].hardware_id == hardware_id {
                let e = &self.entries[i];
                return Ok(match field {
                    "hardware_id" => e.hardware_id.clone(),
                    "model_name" => e.model_name.clone(),
                    "grade" => String::from(e.grade.code()),
                    "basis" => e.basis.clone(),
                    "cert_date" => e.cert_date.clone(),
                    "valid_until" => e.valid_until.clone(),
                    "metrics" => e.metrics.clone(),
                    "signature" => u64_hex_real(e.signature),
                    _ => return Err(XC_FIELD_NOT_ALLOWED),
                });
            }
            i += 1;
        }
        Err(XC_ENTRY_NOT_FOUND)
    }

    /// 全量 JSON 导出（判据"全量公开"）。
    pub fn export_all(&mut self) -> Result<String, u32> {
        self.export_count += 1;
        let mut out = String::with_capacity(64 + self.entries.len() * 256);
        out.push('[');
        // 先全量验签（任一篡改即中止导出——开放以完整为前提）。
        let mut i = 0;
        while i < self.entries.len() {
            let payload = self.entries[i].signing_payload();
            let expect = self.keyed(payload)?;
            if expect != self.entries[i].signature {
                self.tamper_events += 1;
                return Err(XC_TAMPER_DETECTED);
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.entries.len() {
            out.push_str(&entry_to_json(&self.entries[j]));
            if j + 1 < self.entries.len() {
                out.push(',');
            }
            j += 1;
        }
        out.push(']');
        Ok(out)
    }

    /// 条目总数。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    // -----------------------------------------------------------------------
    // 自检（判据五组：全量公开 / 开放格式 / 隐私边界 / 防篡改 / 空态）
    // -----------------------------------------------------------------------

    /// 判据组一：全量公开——三条登记后 export_all 返回含全部 ID 的 JSON。
    pub fn selfcheck_full_disclosure(&mut self) -> Result<(), u32> {
        let mut m = OpenMatrix::new();
        m.load_key(0xABCD_1234);
        let _ = m.publish(fx_entry("1002:7340:01", CertGrade::Gold))?;
        let _ = m.publish(fx_entry("10de:2704:00", CertGrade::Silver))?;
        let _ = m.publish(fx_entry("8086:46a6:02", CertGrade::Bronze))?;
        let json = m.export_all()?;
        if !contains_ci(&json, "1002:7340:01") || !contains_ci(&json, "10de:2704:00") {
            return Err(XC_JSON_MALFORMED);
        }
        if !contains_ci(&json, "\"grade\":\"gold\"") {
            return Err(XC_JSON_MALFORMED);
        }
        if m.len() != 3 {
            return Err(XC_JSON_MALFORMED);
        }
        Ok(())
    }

    /// 判据组二：开放格式——JSON 转义完备（引号/反斜杠/控制字符）。
    pub fn selfcheck_json_escape(&mut self) -> Result<(), u32> {
        let s = json_escape("a\"b\\c\nd\u{1}");
        if !contains_ci(&s, "\\\"") || !contains_ci(&s, "\\\\") || !contains_ci(&s, "\\n") {
            return Err(XC_JSON_MALFORMED);
        }
        // \u001 序列存在性（大写十六进制）。
        if !contains_ci(&s, "\\u001") {
            return Err(XC_JSON_MALFORMED);
        }
        Ok(())
    }

    /// 判据组三：隐私边界——黑名单字段拒绝计数、白名单外字段拒绝。
    pub fn selfcheck_privacy(&mut self) -> Result<(), u32> {
        let mut m = OpenMatrix::new();
        m.load_key(1);
        let _ = m.publish(fx_entry("10de:2704:00", CertGrade::Gold))?;
        // 白名单内字段可查。
        let g = m.query_field("10de:2704:00", "grade")?;
        if g != "gold" {
            return Err(XC_FIELD_NOT_ALLOWED);
        }
        // 白名单外字段拒绝。
        let r1 = m.query_field("10de:2704:00", "owner_email");
        if r1 != Err(XC_FIELD_NOT_ALLOWED) || m.blocked_field_queries != 1 {
            return Err(XC_FIELD_NOT_ALLOWED);
        }
        // 含隐私子串的条目拒绝登记。
        let mut bad = fx_entry("10de:2704:00", CertGrade::Gold);
        bad.model_name = String::from("MyUser-GPU");
        let r2 = m.publish(bad);
        if r2 != Err(XC_FIELD_NOT_ALLOWED) {
            return Err(XC_FIELD_NOT_ALLOWED);
        }
        Ok(())
    }

    /// 判据组四：防篡改——改字段后验签必须失败。
    pub fn selfcheck_tamper(&mut self) -> Result<(), u32> {
        let mut m = OpenMatrix::new();
        m.load_key(0xFEED);
        let _ = m.publish(fx_entry("1002:7340:01", CertGrade::Gold))?;
        // 正常查询应通过。
        let q1 = m.query("1002:7340:01");
        if q1.is_err() {
            return Err(XC_TAMPER_DETECTED);
        }
        // 篡改等级。
        let mut i = 0;
        while i < m.entries.len() {
            if m.entries[i].hardware_id == "1002:7340:01" {
                m.entries[i].grade = CertGrade::Bronze; // 未重签
                break;
            }
            i += 1;
        }
        let q2 = m.query("1002:7340:01");
        if q2 != Err(XC_TAMPER_DETECTED) || m.tamper_events != 1 {
            return Err(XC_TAMPER_DETECTED);
        }
        // 无密钥写入拒绝（签名空承诺防护）。
        let mut m2 = OpenMatrix::new();
        let r3 = m2.publish(fx_entry("1002:7340:01", CertGrade::Gold));
        if r3 != Err(XC_SIG_KEY_ABSENT) {
            return Err(XC_SIG_KEY_ABSENT);
        }
        Ok(())
    }

    /// 判据组五：空态与覆盖——空仓导出合法空数组；同 ID 覆盖不重复。
    pub fn selfcheck_empty_and_upsert(&mut self) -> Result<(), u32> {
        let mut m = OpenMatrix::new();
        m.load_key(7);
        let empty = m.export_all()?;
        if empty != "[]" {
            return Err(XC_JSON_MALFORMED);
        }
        let _ = m.publish(fx_entry("8086:46a6:02", CertGrade::Silver))?;
        let _ = m.publish(fx_entry("8086:46a6:02", CertGrade::Gold))?; // 覆盖
        if m.len() != 1 {
            return Err(XC_JSON_MALFORMED);
        }
        let (g, _, _) = m.query("8086:46a6:02")?;
        if g != CertGrade::Gold {
            return Err(XC_JSON_MALFORMED);
        }
        // 查不存在条目。
        let r = m.query("dead:beef:00");
        if r != Err(XC_ENTRY_NOT_FOUND) {
            return Err(XC_ENTRY_NOT_FOUND);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_full_disclosure()?;
        self.selfcheck_json_escape()?;
        self.selfcheck_privacy()?;
        self.selfcheck_tamper()?;
        self.selfcheck_empty_and_upsert()?;
        Ok(())
    }
}

/// 测试样例条目工厂（判据驱动，非生产路径）。
pub fn fx_entry(hwid: &str, g: CertGrade) -> CertMatrixEntry {
    CertMatrixEntry {
        hardware_id: String::from(hwid),
        model_name: String::from("Varix Reference GPU"),
        grade: g,
        basis: String::from("suite-v1:F0369,F0806"),
        cert_date: String::from("2026-10-11"),
        valid_until: String::from("2027-10-11"),
        metrics: String::from("fps80_p99=1.4ms;jitter_cv=3.1%"),
        signature: 0,
    }
}
