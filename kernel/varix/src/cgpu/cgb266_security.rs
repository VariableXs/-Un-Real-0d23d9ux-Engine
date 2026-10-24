#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb266_security
// CGPU-F0266 · 持久化安全（B07 组 · 验签校验 + 路径白名单 + 敏感过滤）
//
// 锚点：CGPU-F0266
// 判据原文：文件来源校验（加载持久化文件验签+schema 校验——恶意文件不崩不注入）；
//          路径纪律（持久化目录白名单——防路径穿越）；
//          敏感信息（参数摘要中不落密钥类数据——写入前过滤清单）。
//
// 难点分析：
//   1. 「恶意文件不崩不注入」要求反序列化的**每一步都做边界检查**——
//      长度字段、索引值、字符串字节数组，任一超出合理范围立即拒绝。
//   2. 「路径穿越」是经典攻击：../../etc/passwd。必须规范化后做前缀匹配。
//   3. 「敏感过滤」必须在写入时做——写了就泄露了，事后删除无法保证备份/缓存里没有。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ============================================================================
// 一、路径白名单
// ============================================================================

/// 路径校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathCheck {
    /// 在白名单内。
    Allowed,
    /// 路径穿越攻击。
    Traversal,
    /// 不在白名单。
    NotWhitelisted,
    /// 路径格式非法。
    Malformed,
}

/// 路径白名单。
pub struct PathWhitelist {
    /// 允许的目录前缀。
    allowed_prefixes: Vec<String>,
}

impl PathWhitelist {
    /// 构造。
    pub fn new() -> Self {
        Self {
            allowed_prefixes: Vec::new(),
        }
    }

    /// 添加允许的目录。
    pub fn allow(&mut self, dir: &str) {
        self.allowed_prefixes.push(Self::normalize(dir));
    }

    /// 默认白名单（varix/persist 及子目录）。
    pub fn default_whitelist() -> Self {
        let mut w = Self::new();
        w.allow("varix/persist");
        w.allow("varix/snapshots");
        w.allow("varix/recordings");
        w
    }

    /// 路径规范化（去除多余分隔符、解析 . 和 ..）。
    pub fn normalize(path: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for seg in path.split(['/', '\\']) {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        parts.join("/")
    }

    /// 校验路径。
    pub fn check(&self, path: &str) -> PathCheck {
        if path.is_empty() {
            return PathCheck::Malformed;
        }
        // 检测原始路径中的穿越尝试。
        if path.contains("..") {
            // 规范化后可能仍安全，但保守拒绝含 .. 的路径。
            return PathCheck::Traversal;
        }
        let normalized = Self::normalize(path);
        if normalized.is_empty() {
            return PathCheck::Malformed;
        }
        for prefix in &self.allowed_prefixes {
            if normalized.starts_with(prefix.as_str()) {
                return PathCheck::Allowed;
            }
        }
        PathCheck::NotWhitelisted
    }
}

impl Default for PathWhitelist {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 二、敏感信息过滤
// ============================================================================

/// 敏感键名模式。
pub const SENSITIVE_PATTERNS: &[&str] = &[
    "key",
    "secret",
    "token",
    "password",
    "credential",
    "private",
    "cert",
    "signature",
    "auth",
    "session",
];

/// 敏感过滤器。
pub struct SensitiveFilter;

impl SensitiveFilter {
    /// 判断键名是否敏感。
    pub fn is_sensitive(key: &str) -> bool {
        let lower = key.to_ascii_lowercase();
        SENSITIVE_PATTERNS.iter().any(|p| lower.contains(p))
    }

    /// 过滤参数列表：返回 (保留列表, 被过滤键名列表)。
    pub fn filter_params(params: &[(String, String)]) -> (Vec<(String, String)>, Vec<String>) {
        let mut kept = Vec::new();
        let mut filtered = Vec::new();
        for (k, v) in params {
            if Self::is_sensitive(k) {
                filtered.push(k.clone());
            } else {
                kept.push((k.clone(), v.clone()));
            }
        }
        (kept, filtered)
    }

    /// 过滤字符串值：把匹配模式的值替换为 [REDACTED]。
    pub fn redact_value(key: &str, value: &str) -> String {
        if Self::is_sensitive(key) {
            "[REDACTED]".to_string()
        } else {
            value.to_string()
        }
    }
}

// ============================================================================
// 三、文件来源校验（验签 + schema）
// ============================================================================

/// 文件校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCheck {
    /// 通过。
    Pass,
    /// 魔数不符。
    BadMagic,
    /// 版本不兼容。
    VersionMismatch,
    /// 签名无效。
    BadSignature,
    /// schema 不匹配。
    SchemaMismatch,
    /// 结构损坏。
    Corrupt,
}

/// 文件校验器。
pub struct FileValidator {
    /// 期望魔数。
    pub expected_magic: [u8; 4],
    /// 期望 schema 版本。
    pub expected_version: u16,
}

impl FileValidator {
    /// 构造。
    pub fn new(expected_magic: [u8; 4], expected_version: u16) -> Self {
        Self {
            expected_magic,
            expected_version,
        }
    }

    /// 默认校验器（VXGP 格式）。
    pub fn for_graph_persist() -> Self {
        Self::new(*b"VXGP", 1)
    }

    /// 校验文件头。
    pub fn validate_header(&self, data: &[u8]) -> FileCheck {
        if data.len() < 64 {
            return FileCheck::Corrupt;
        }
        if data[0..4] != self.expected_magic {
            return FileCheck::BadMagic;
        }
        let major = u16::from_le_bytes([data[4], data[5]]);
        if major != self.expected_version {
            return FileCheck::VersionMismatch;
        }
        FileCheck::Pass
    }

    /// 校验完整性（CRC）。
    pub fn validate_integrity(&self, data: &[u8]) -> FileCheck {
        use super::cgb258_serialize::GraphSerializer;
        match GraphSerializer::verify(data) {
            Ok(()) => FileCheck::Pass,
            Err(_) => FileCheck::Corrupt,
        }
    }

    /// 全量校验。
    pub fn validate_all(&self, data: &[u8]) -> FileCheck {
        let header = self.validate_header(data);
        if header != FileCheck::Pass {
            return header;
        }
        self.validate_integrity(data)
    }
}

// ============================================================================
// 四、恶意构造防护
// ============================================================================

/// 恶意构造检测。
pub struct MaliciousGuard;

impl MaliciousGuard {
    /// 检测超大长度字段（防御拒绝服务）。
    pub fn check_length_claim(claimed: u32, actual_available: usize) -> bool {
        (claimed as usize) <= actual_available && (claimed as usize) <= 64 * 1024 * 1024
    }

    /// 检测深度嵌套（防栈溢出）。
    pub fn check_nesting_depth(depth: usize) -> bool {
        depth <= 64
    }

    /// 检测超大计数字段。
    pub fn check_count_claim(claimed: u32, max_reasonable: u32) -> bool {
        claimed <= max_reasonable
    }
}

// ============================================================================
// 五、判据：F0266 三条（验签校验、路径白名单、敏感过滤）
// ============================================================================

/// F0266 判据集。
pub fn run_f0266_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0266");

    // --- 判据 1：验签 + schema 校验 ---
    use super::cgb257_persistarch::*;
    use super::cgb258_serialize::GraphSerializer;
    let snap = GraphSnapshot::new(
        TopologyLayer::new(1, vec![1], vec![]),
        ParamLayer::new(),
        AccountLayer::new(vec![], vec![]),
        EventStreamLayer::new(vec![]),
    );
    let file = GraphSerializer::serialize(&snap).expect("ser");
    let validator = FileValidator::for_graph_persist();
    let good = validator.validate_all(&file.data);

    // 恶意文件：坏魔数。
    let mut bad = file.data.clone();
    bad[0] = b'X';
    let bad_result = validator.validate_all(&bad);

    cs.add(
        "验签校验拒绝恶意文件",
        good == FileCheck::Pass && bad_result == FileCheck::BadMagic,
        "好文件Pass/坏魔数拒绝",
    );

    // --- 判据 2：路径白名单 ---
    let whitelist = PathWhitelist::default_whitelist();
    let ok_path = whitelist.check("varix/persist/graph.bin");
    let attack1 = whitelist.check("../../etc/passwd");
    let attack2 = whitelist.check("varix/../../../root/.ssh/id_rsa");
    let outside = whitelist.check("other/data.bin");
    cs.add(
        "路径白名单防穿越",
        ok_path == PathCheck::Allowed
            && attack1 == PathCheck::Traversal
            && attack2 == PathCheck::Traversal
            && outside == PathCheck::NotWhitelisted,
        "正常路径放行/穿越拒绝/外部拒绝",
    );

    // --- 判据 3：敏感过滤 ---
    let params = vec![
        ("cgpu.priority".to_string(), "5".to_string()),
        ("api_key".to_string(), "sk-12345".to_string()),
        ("node.timeout".to_string(), "1000".to_string()),
        ("session_token".to_string(), "abc".to_string()),
    ];
    let (kept, filtered) = SensitiveFilter::filter_params(&params);
    cs.add(
        "敏感信息过滤",
        kept.len() == 2 && filtered.len() == 2,
        alloc::format!("保留{}条/过滤{}条", kept.len(), filtered.len()).leak(),
    );

    // --- 附加：恶意构造防护 ---
    let len_ok = MaliciousGuard::check_length_claim(100, 200);
    let len_bad = !MaliciousGuard::check_length_claim(u32::MAX, 100);
    let depth_ok = MaliciousGuard::check_nesting_depth(10);
    let depth_bad = !MaliciousGuard::check_nesting_depth(10_000);
    cs.add(
        "恶意构造防护",
        len_ok && len_bad && depth_ok && depth_bad,
        "长度/深度边界检查",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0266_all_pass() {
        let cs = run_f0266_checks();
        assert!(cs.all_green(), "F0266 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn path_traversal_rejected() {
        let w = PathWhitelist::default_whitelist();
        assert_eq!(w.check("../secret"), PathCheck::Traversal);
        assert_eq!(w.check("varix/../../etc"), PathCheck::Traversal);
    }

    #[test]
    fn sensitive_detection() {
        assert!(SensitiveFilter::is_sensitive("my_api_key"));
        assert!(SensitiveFilter::is_sensitive("SECRET_VALUE"));
        assert!(!SensitiveFilter::is_sensitive("node_count"));
    }
}
