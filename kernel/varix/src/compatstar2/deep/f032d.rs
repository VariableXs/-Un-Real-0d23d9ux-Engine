//! F032 深化批次二 · 包管理器语义面（compatstar2/deep · G-A-32）。
//!
//! 批次一深化覆盖 semver 比较/五镜像表/隔离解析；本批补齐：pip freeze 行
//! 解析（`name==version` 注释跳过——「验证安装」的清单核对面）、PEP 503
//! 包名规范化（`-_.` 游程归一为单连字符——解析按会话的键规范）、node
//! engines 子集判定（`>=14`/`^14` 三操作符——文档化子集）、工具链磁盘占用
//! 聚合（磁盘预检的第二数据源）、会话 PATH 摘除（取消激活 = 摘前缀——
//! 显式激活的对称出口）。
//!
//! 零堆纪律：定长缓冲，无 alloc。

use crate::checks::CheckSet;

/// 包名规范化缓冲容量。
pub const PKG_NAME_MAX: usize = 32;

/// pip freeze 行解析：`name==version` → (name, version)；
/// 空行与 `#` 注释跳过；无 `==` 的行如实 None。
pub fn parse_freeze_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (name, version) = trimmed.split_once("==")?;
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name.trim(), version.trim()))
}

/// PEP 503 包名规范化：小写化 + `-_.' 游程折叠为单 `-`。
/// 返回写入长度；超长截断不越界（缓冲容量纪律）。
pub fn normalize_pkg_name(name: &str, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut prev_sep = true; // 首位分隔符丢弃
    for &b in name.as_bytes() {
        let is_sep = matches!(b, b'-' | b'_' | b'.');
        let c = if b.is_ascii_uppercase() { b + 32 } else { b };
        if is_sep {
            if !prev_sep && n < out.len() {
                out[n] = b'-';
                n += 1;
            }
            prev_sep = true;
        } else {
            if n < out.len() {
                out[n] = c;
                n += 1;
            }
            prev_sep = false;
        }
    }
    // 尾分隔符丢弃。
    if n > 0 && out[n - 1] == b'-' {
        n -= 1;
    }
    n
}

/// node engines 判定（文档化子集）：
/// `>=N` / `>N` / `^N`（主版本相等即可）/ `=N`。
pub fn engines_ok(spec: &str, major: u32) -> bool {
    if let Some(rest) = spec.strip_prefix(">=") {
        return rest.parse::<u32>().map(|v| major >= v).unwrap_or(false);
    }
    if let Some(rest) = spec.strip_prefix('>') {
        return rest.parse::<u32>().map(|v| major > v).unwrap_or(false);
    }
    if let Some(rest) = spec.strip_prefix('^') {
        return rest.parse::<u32>().map(|v| major == v).unwrap_or(false);
    }
    if let Some(rest) = spec.strip_prefix('=') {
        return rest.parse::<u32>().map(|v| major == v).unwrap_or(false);
    }
    false // 未声明操作符 = 不满足（保守正确）
}

/// 工具链磁盘占用聚合（磁盘预检第二数据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiskUsage {
    pub dirs: u32,
    pub files: u64,
    pub bytes: u64,
}

impl DiskUsage {
    pub const fn zero() -> Self {
        DiskUsage { dirs: 0, files: 0, bytes: 0 }
    }
    pub fn add(&mut self, other: &DiskUsage) {
        self.dirs += other.dirs;
        self.files += other.files;
        self.bytes += other.bytes;
    }
}

/// 会话 PATH 摘除：从会话路径串中摘除 `prefix;` 段（取消激活 = 显式激活
/// 的对称出口）。返回写入长度；未命中原样回写。
pub fn strip_path_prefix(prefix: &str, session: &str, out: &mut [u8]) -> usize {
    let needle_len = prefix.len() + 1; // prefix + ';'
    let mut n = 0usize;
    let mut i = 0usize;
    let sb = session.as_bytes();
    while i < sb.len() {
        let hit = i + needle_len <= sb.len()
            && &sb[i..i + prefix.len()] == prefix.as_bytes()
            && sb[i + prefix.len()] == b';';
        if hit {
            i += needle_len; // 摘除该段
        } else {
            if n < out.len() {
                out[n] = sb[i];
            }
            n += 1;
            i += 1;
        }
    }
    n
}

/// 域自检（深化批次二）。
pub fn run_f032d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F032-runtimes-d2");
    // 1) freeze 行解析：正常/注释/空行/缺 == 四态。
    cs.add(
        "freeze_line_parse",
        parse_freeze_line("requests==2.31.0") == Some(("requests", "2.31.0"))
            && parse_freeze_line("# comment only").is_none()
            && parse_freeze_line("").is_none()
            && parse_freeze_line("just-a-name").is_none(),
        "",
    );
    // 2) PEP 503 规范化：混合分隔符折叠、大小写归一。
    let mut buf = [0u8; PKG_NAME_MAX];
    let n1 = normalize_pkg_name("Django_Rest-Framework", &mut buf);
    cs.add("pep503_normalize", &buf[..n1] == b"django-rest-framework", "");
    let n2 = normalize_pkg_name("A..B__C--D", &mut buf);
    cs.add("pep503_collapse_runs", &buf[..n2] == b"a-b-c-d", "");
    // 3) engines 子集：>=14 过 15 拒 13；^14 匹配 14 不匹配 15；无操作符拒。
    cs.add(
        "engines_subset",
        engines_ok(">=14", 15) && !engines_ok(">=14", 13)
            && engines_ok("^14", 14) && !engines_ok("^14", 15)
            && engines_ok(">18", 19) && !engines_ok("14", 14),
        "",
    );
    // 4) 磁盘占用聚合：两目录相加（dirs/files/bytes 三账同加）。
    let mut total = DiskUsage::zero();
    total.add(&DiskUsage { dirs: 3, files: 120, bytes: 120 << 20 });
    total.add(&DiskUsage { dirs: 1, files: 80, bytes: 80 << 20 });
    cs.add("disk_usage_aggregate", total.dirs == 4 && total.files == 200 && total.bytes == 200 << 20, "");
    // 5) PATH 摘除：命中段摘除、未命中原样、保留其余段。
    let mut out = [0u8; 64];
    let n3 = strip_path_prefix("Toolchains\\python\\3.12.4", "Toolchains\\python\\3.12.4;C:\\bin", &mut out);
    cs.add("path_strip_hit", &out[..n3] == b"C:\\bin", "");
    let n4 = strip_path_prefix("Toolchains\\go\\1.22", "C:\\bin", &mut out);
    cs.add("path_strip_miss", &out[..n4] == b"C:\\bin", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_whitespace_tolerant() {
        assert_eq!(parse_freeze_line("  numpy == 1.26.4  "), Some(("numpy", "1.26.4")));
    }

    #[test]
    fn normalize_trailing_separator_dropped() {
        let mut buf = [0u8; PKG_NAME_MAX];
        let n = normalize_pkg_name("flask_", &mut buf);
        assert_eq!(&buf[..n], b"flask", "尾分隔符丢弃");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f032d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
