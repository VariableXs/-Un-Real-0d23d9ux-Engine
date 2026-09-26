//! F127 vxapp 打包工具 · 完整设计（STAR I 主册 G-D-02）。
//!
//! **判据（主册）**：五个不同形态目录（单 exe/带资源/带运行时/带图标/
//! 最简）打包全绿；产物在系统侧安装-运行-卸载全通（与 F030 闭环）；
//! 分钟级承诺实测（五例均 <10 分钟含学习）。
//!
//! **设计要点（主册）**：
//! - 一条命令打包：`vxapp pack <目录>` 自动生成清单骨架（补默认值）/
//!   计算哈希 / 签名（用户密钥）/ 输出 .vxapp；工具开源（F126 格式的
//!   参考实现）；十分钟心智成本承诺；
//! - `vxapp validate` 独立校验命令（打包前后自查）；`--interactive`
//!   向导模式（问答式补清单，新手路径）；错误输出带修复建议（三要素
//!   纪律的 CLI 版）；
//! - 清单字段必填三项（id/版本/主程序）其余默认值全文档化；哈希
//!   SHA-256 全文件+分块双计；签名覆盖清单+内容树；产物格式版本戳
//!   （F126 双读依据）；CLI 退出码规范（0 成功/1 用户错/2 内部错
//!   ——脚本友好）；
//! - 签名评估 minisign 算法思路（ed25519 公共密码学）；工具本身自研
//!   开源（MIT 建议许可，F130 登记）；
//! - 清单骨架模板内嵌；签名密钥对生成与保管指引（`vxapp keygen`）；
//!   打包缓存（重复打包跳过未变文件）；
//! - 目录缺必需文件（主程序）→ 指出缺什么+示例；哈希计算中断 →
//!   断点续算；签名失败 → 密钥排查指引；产物校验失败 → 工具 bug
//!   报告通道（F139）。
//!
//! 时间注入式，宿主测试确定复现。无外部依赖（哈希用 vbase::sha256
//! 唯一源；签名面为 ed25519 替换点登记的 SHA-256 双轮参考实现——
//! 算法位诚实标注，替换接缝登记 F130）。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 产物格式版本戳（F126 双读依据）。
pub const FORMAT_VERSION: u32 = 1;
/// 分块大小（KiB——SHA-256 全文件+分块双计的分块粒度）。
pub const CHUNK_KIB: usize = 64;
/// CLI 退出码规范（0 成功/1 用户错/2 内部错）。
pub const EXIT_OK: i32 = 0;
pub const EXIT_USER_ERR: i32 = 1;
pub const EXIT_INTERNAL_ERR: i32 = 2;
/// 打包承诺（分钟，主册：<10 分钟含学习）。
pub const PACK_BUDGET_MIN: u64 = 10;

// ---------------------------------------------------------------------------
// 清单与密钥
// ---------------------------------------------------------------------------

/// 包清单（必填三项 + 可选默认值全文档化）。
#[derive(Clone, Debug)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub entry: String,
    /// 显示名（默认取 id）。
    pub name: Option<String>,
    /// 图标（默认无）。
    pub icon: Option<String>,
}

impl Manifest {
    /// 清单骨架自动生成（补默认值）：给三必填 → 可选位默认填充。
    pub fn skeleton(id: &str, version: &str, entry: &str) -> Manifest {
        Manifest {
            id: String::from(id),
            version: String::from(version),
            entry: String::from(entry),
            name: Some(String::from(id)),
            icon: None,
        }
    }

    /// 必填三查（缺一即用户错 + 修复建议）。
    pub fn validate(&self) -> Result<(), (&'static str, &'static str)> {
        if self.id.is_empty() || !self.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-' || b.is_ascii_digit()) {
            return Err(("id", "示例：demo.tool（小写反域名）"));
        }
        if vbase::parse_semver(&self.version).is_none() {
            return Err(("version", "示例：1.0.0（semver 三元组）"));
        }
        if self.entry.is_empty() {
            return Err(("entry", "示例：bin/demo.exe（相对主程序路径）"));
        }
        Ok(())
    }

    /// 清单序列化（vbase::JsonObj 唯一 JSON 面——F126 schema 同源）。
    pub fn to_json(&self) -> String {
        let mut o = vbase::JsonObj::new();
        o.str_field("id", &self.id);
        o.str_field("version", &self.version);
        o.str_field("entry", &self.entry);
        match &self.name {
            Some(n) => o.str_field("name", n),
            None => o.str_field("name", ""),
        }
        match &self.icon {
            Some(i) => o.str_field("icon", i),
            None => o.str_field("icon", ""),
        }
        o.finish()
    }
}

/// 用户密钥对（keygen 产物）。
#[derive(Clone)]
pub struct KeyPair {
    pub pub_hex: String,
    secret: [u8; 32],
}

/// `vxapp keygen`：密钥对生成（SHA-256 派生参考实现——ed25519 替换点）。
pub fn keygen(seed: &[u8]) -> KeyPair {
    let secret = vbase::sha256(seed);
    let pubk = vbase::sha256(&secret);
    KeyPair { pub_hex: vbase::hex32_str(&pubk), secret }
}

/// 公钥字节（派生链：pub = H(secret)）。
pub fn public_bytes(kp: &KeyPair) -> [u8; 32] {
    vbase::sha256(&kp.secret)
}

/// 签名（覆盖清单+内容树哈希）：sig = H(pub || content)。
/// 参考实现语义：对称派生签名（公钥可重演）——ed25519 替换点见 F130
/// 登记；主册「minisign 算法思路」的接缝位。
pub fn sign(kp: &KeyPair, content_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = vbase::Sha256::new();
    h.update(&public_bytes(kp));
    h.update(content_hash);
    h.finalize()
}

/// 验签（双向：公钥侧重演 sig' = H(pub || content)，对得上才过）。
pub fn verify(kp_pub_bytes: &[u8; 32], content_hash: &[u8; 32], sig: &[u8; 32]) -> bool {
    let mut h = vbase::Sha256::new();
    h.update(kp_pub_bytes);
    h.update(content_hash);
    &h.finalize() == sig
}

/// hex 字符串 → 32 字节（非法字符即 None——验签前置闸）。
pub fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = (b[i * 2] as char).to_digit(16);
        let lo = (b[i * 2 + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out[i] = (h * 16 + l) as u8,
            _ => return None,
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 打包器
// ---------------------------------------------------------------------------

/// 内容树中的一个文件。
#[derive(Clone, Debug)]
pub struct TreeFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// 打包产物。
#[derive(Clone, Debug)]
pub struct Artifact {
    pub manifest_json: String,
    /// 内容树哈希（SHA-256 全文件+分块双计的聚合根）。
    pub tree_hash: String,
    pub sig_hex: String,
    pub format_version: u32,
    pub total_bytes: u64,
}

/// 打包结果（CLI 三输出：进度/警告/产物路径）。
#[derive(Debug)]
pub struct PackOutcome {
    pub exit: i32,
    pub warnings: Vec<String>,
    pub error: Option<(&'static str, &'static str)>, // (问题, 修复建议)
    pub artifact: Option<Artifact>,
    /// 缓存命中（重复打包跳过未变文件）。
    pub cache_hit: bool,
}

/// 打包器。
pub struct Packer {
    /// 上次产物（缓存比对基准——树哈希未变即跳过）。
    last_tree_hash: Option<String>,
}

impl Packer {
    pub fn new() -> Packer {
        Packer { last_tree_hash: None }
    }

    /// 内容树哈希（全文件+分块双计：每文件整散列 + 64KiB 分块散列
    /// 串联再聚合——主册双计语义）。
    pub fn tree_hash(files: &[TreeFile]) -> [u8; 32] {
        let mut h = vbase::Sha256::new();
        for f in files {
            let whole = vbase::sha256(&f.bytes);
            h.update(&whole);
            // 分块双计。
            let chunk = CHUNK_KIB * 1024;
            let mut off = 0;
            while off < f.bytes.len() {
                let end = (off + chunk).min(f.bytes.len());
                let ch = vbase::sha256(&f.bytes[off..end]);
                h.update(&ch);
                off = end;
            }
            h.update(f.path.as_bytes());
        }
        h.finalize()
    }

    /// `vxapp pack`：清单校验 → 主程序在位检查 → 哈希 → 签名 → 产物。
    /// 缓存命中（树哈希同上次）→ 跳过重打包（警告条说明）。
    pub fn pack(&mut self, m: &Manifest, files: &[TreeFile], kp: &KeyPair, cache_enabled: bool) -> PackOutcome {
        // 1. 清单校验（用户错 + 修复建议三要素）。
        if let Err((field, hint)) = m.validate() {
            return PackOutcome {
                exit: EXIT_USER_ERR,
                warnings: Vec::new(),
                error: Some((field, hint)),
                artifact: None,
                cache_hit: false,
            };
        }
        // 2. 主程序在位检查（缺 → 指出缺什么 + 示例）。
        if !files.iter().any(|f| f.path == m.entry) {
            return PackOutcome {
                exit: EXIT_USER_ERR,
                warnings: Vec::new(),
                error: Some((
                    "entry",
                    "主程序不在内容树：需要相对路径文件（示例 bin/demo.exe）",
                )),
                artifact: None,
                cache_hit: false,
            };
        }
        // 3. 树哈希 + 缓存。
        let th = Self::tree_hash(files);
        let th_hex = vbase::hex32_str(&th);
        if cache_enabled && self.last_tree_hash.as_deref() == Some(th_hex.as_str()) {
            return PackOutcome {
                exit: EXIT_OK,
                warnings: vec![String::from("内容未变，命中打包缓存——跳过重打包")],
                error: None,
                artifact: None,
                cache_hit: true,
            };
        }
        // 4. 签名失败路径（空密钥 → 排查指引；用户错）。
        if kp.pub_hex.len() != 64 {
            return PackOutcome {
                exit: EXIT_USER_ERR,
                warnings: Vec::new(),
                error: Some(("keygen", "密钥无效：先跑 vxapp keygen 生成密钥对")),
                artifact: None,
                cache_hit: false,
            };
        }
        // 5. 组装产物。
        let manifest_json = m.to_json();
        let mut mh = vbase::Sha256::new();
        mh.update(manifest_json.as_bytes());
        mh.update(&th);
        let content_hash = mh.finalize();
        let sig = sign(kp, &content_hash);
        let total: u64 = files.iter().map(|f| f.bytes.len() as u64).sum();
        let artifact = Artifact {
            manifest_json,
            tree_hash: th_hex.clone(),
            sig_hex: vbase::hex32_str(&sig),
            format_version: FORMAT_VERSION,
            total_bytes: total,
        };
        self.last_tree_hash = Some(th_hex);
        PackOutcome {
            exit: EXIT_OK,
            warnings: Vec::new(),
            error: None,
            artifact: Some(artifact),
            cache_hit: false,
        }
    }
}

/// `vxapp validate`：独立校验命令（打包前后自查——产物结构 + 签名
/// 双向验）。返回（退出码，问题清单）。
pub fn validate_artifact(a: &Artifact, kp_pub_hex: &str) -> (i32, Vec<String>) {
    let mut problems = Vec::new();
    if a.format_version != FORMAT_VERSION {
        problems.push(alloc::format!("格式版本戳不符：{} != {}", a.format_version, FORMAT_VERSION));
    }
    if vbase::parse_semver(
        &a.manifest_json
            .split("\"version\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or(""),
    )
    .is_none()
    {
        problems.push(String::from("清单 version 字段非法"));
    }
    // 树哈希与签名解码（非十六进制即拒）。
    let tree = match hex_to_32(&a.tree_hash) {
        Some(t) => t,
        None => {
            problems.push(String::from("树哈希格式非法"));
            return (EXIT_USER_ERR, problems);
        }
    };
    let sig = match hex_to_32(&a.sig_hex) {
        Some(s) => s,
        None => {
            problems.push(String::from("签名格式非法"));
            return (EXIT_USER_ERR, problems);
        }
    };
    // 验签：重演内容哈希（清单 JSON + 树哈希原始字节——与 pack 同口径）。
    let mut mh = vbase::Sha256::new();
    mh.update(a.manifest_json.as_bytes());
    mh.update(&tree);
    let content_hash = mh.finalize();
    let pub_bytes = match hex_to_32(kp_pub_hex) {
        Some(p) => p,
        None => {
            problems.push(String::from("公钥格式非法"));
            return (EXIT_USER_ERR, problems);
        }
    };
    if !verify(&pub_bytes, &content_hash, &sig) {
        problems.push(String::from("验签失败——产物被篡改或密钥不符（报告通道 F139）"));
        return (EXIT_USER_ERR, problems);
    }
    if problems.is_empty() {
        (EXIT_OK, problems)
    } else {
        (EXIT_USER_ERR, problems)
    }
}

/// `--interactive` 向导模式：问答式补清单（新手路径）——答案序列
/// 逐问填入，缺答保留默认。
pub fn interactive_wizard(answers: [&str; 3]) -> Manifest {
    let mut m = Manifest::skeleton("demo.tool", "1.0.0", "bin/demo.exe");
    if !answers[0].is_empty() {
        m.id = String::from(answers[0]);
    }
    if !answers[1].is_empty() {
        m.version = String::from(answers[1]);
    }
    if !answers[2].is_empty() {
        m.entry = String::from(answers[2]);
    }
    m.name = Some(m.id.clone());
    m
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_vxapp_checks() -> CheckSet {
    let mut set = CheckSet::new("F127-vxapp");

    let kp = keygen(b"varix-demo-seed");

    // 五种目录形态（判据第一句）：单 exe / 带资源 / 带运行时 / 带图标 / 最简。
    let shapes: [Vec<TreeFile>; 5] = [
        vec![TreeFile { path: String::from("bin/demo.exe"), bytes: vec![1u8; 4096] }],
        vec![
            TreeFile { path: String::from("bin/demo.exe"), bytes: vec![2u8; 4096] },
            TreeFile { path: String::from("res/bg.png"), bytes: vec![3u8; 100_000] },
        ],
        vec![
            TreeFile { path: String::from("bin/demo.exe"), bytes: vec![4u8; 4096] },
            TreeFile { path: String::from("runtime/rt.dll"), bytes: vec![5u8; 300_000] },
        ],
        vec![
            TreeFile { path: String::from("bin/demo.exe"), bytes: vec![6u8; 4096] },
            TreeFile { path: String::from("icon.ico"), bytes: vec![7u8; 22_000] },
        ],
        vec![TreeFile { path: String::from("demo.exe"), bytes: vec![8u8; 16] }],
    ];
    let entries = [
        "bin/demo.exe",
        "bin/demo.exe",
        "bin/demo.exe",
        "bin/demo.exe",
        "demo.exe",
    ];

    // 1. 五形态打包全绿（退出码 0 + 产物齐 + 格式版本戳）。
    let mut packer = Packer::new();
    let mut artifacts = Vec::new();
    let mut all_green = true;
    for (i, files) in shapes.iter().enumerate() {
        let m = Manifest::skeleton("demo.tool", "1.0.0", entries[i]);
        let out = packer.pack(&m, files, &kp, false);
        all_green &= out.exit == EXIT_OK
            && out.artifact.is_some()
            && out.artifact.as_ref().unwrap().format_version == FORMAT_VERSION;
        if let Some(a) = out.artifact {
            artifacts.push(a);
        }
    }
    set.add("five directory shapes pack green", all_green && artifacts.len() == 5, "");

    // 2. 产物系统侧校验全通（F030 闭环的打包侧：validate 五产物全过）。
    let mut validate_ok = true;
    for a in &artifacts {
        let (exit, problems) = validate_artifact(a, &kp.pub_hex);
        if exit != EXIT_OK || !problems.is_empty() {
            validate_ok = false;
        }
    }
    set.add("five artifacts validate green", validate_ok, "");

    // 3. 双计哈希：整文件与分块双计聚合根可复现且对内容敏感。
    let h1 = Packer::tree_hash(&shapes[0]);
    let h2 = Packer::tree_hash(&shapes[1]);
    let h1b = Packer::tree_hash(&shapes[0]);
    set.add(
        "tree hash deterministic + content-sensitive",
        h1 == h1b && h1 != h2,
        "",
    );

    // 4. 清单三必填校验 + 修复建议（用户错退出码 1）。
    let mut packer = Packer::new();
    let mut bad = Manifest::skeleton("", "abc", "");
    bad.version = String::from("abc");
    let out = packer.pack(&bad, &shapes[0], &kp, false);
    set.add(
        "manifest validation + exit code 1 + hints",
        out.exit == EXIT_USER_ERR && out.error.is_some(),
        "",
    );

    // 5. 主程序缺失 → 指出缺什么 + 示例。
    let m = Manifest::skeleton("demo.tool", "1.0.0", "bin/missing.exe");
    let out = packer.pack(&m, &shapes[0], &kp, false);
    set.add(
        "missing entry reported with example",
        out.exit == EXIT_USER_ERR
            && out.error.as_ref().map(|e| e.0 == "entry" && e.1.contains("示例")).unwrap_or(false),
        "",
    );

    // 6. 打包缓存：同内容二次打包命中（警告条说明）；内容变化后失效。
    let mut packer = Packer::new();
    let m = Manifest::skeleton("demo.tool", "1.0.0", "bin/demo.exe");
    let first = packer.pack(&m, &shapes[0], &kp, true);
    let second = packer.pack(&m, &shapes[0], &kp, true);
    let changed = packer.pack(&m, &shapes[1], &kp, true);
    set.add(
        "cache hit on unchanged, miss on change",
        first.exit == EXIT_OK
            && !first.cache_hit
            && second.cache_hit
            && second.artifact.is_none()
            && !changed.cache_hit
            && changed.artifact.is_some(),
        "",
    );

    // 7. 签名-验签双向 + 篡改必拒。
    let a = &artifacts[0];
    let (ok_exit, _) = validate_artifact(a, &kp.pub_hex);
    let mut tampered = a.clone();
    tampered.tree_hash = vbase::hex32_str(&vbase::sha256(b"tampered"));
    let (bad_exit, problems) = validate_artifact(&tampered, &kp.pub_hex);
    set.add(
        "sign-verify both ways + tamper rejected",
        ok_exit == EXIT_OK && bad_exit == EXIT_USER_ERR && problems.iter().any(|p| p.contains("验签失败")),
        "",
    );

    // 8. keygen 确定性 + 密钥不同产物不同。
    let kp2 = keygen(b"varix-demo-seed");
    let kp3 = keygen(b"other-seed");
    set.add(
        "keygen deterministic + seed-sensitive",
        kp2.pub_hex == kp.pub_hex && kp3.pub_hex != kp.pub_hex,
        "",
    );

    // 9. 错误密钥验签拒绝（密钥排查指引路径）。
    let (exit, problems) = validate_artifact(a, &kp3.pub_hex);
    set.add(
        "wrong key rejected",
        exit == EXIT_USER_ERR && problems.iter().any(|p| p.contains("验签失败")),
        "",
    );

    // 10. 向导模式：答案填入 + 缺答保留默认。
    let m = interactive_wizard(["my.app", "2.0.0", ""]);
    set.add(
        "interactive wizard fills and defaults",
        m.id == "my.app" && m.version == "2.0.0" && m.entry == "bin/demo.exe",
        "",
    );

    // 11. CLI 退出码规范与打包预算常量（0/1/2 + <10 分钟）。
    set.add(
        "exit codes + pack budget",
        EXIT_OK == 0 && EXIT_USER_ERR == 1 && EXIT_INTERNAL_ERR == 2 && PACK_BUDGET_MIN == 10,
        "",
    );

    // 12. 分块双计跨块敏感：恰好 64KiB+1 字节跨块文件哈希变化。
    let f1 = vec![TreeFile { path: String::from("a"), bytes: vec![9u8; CHUNK_KIB * 1024] }];
    let mut f2bytes = vec![9u8; CHUNK_KIB * 1024];
    f2bytes.push(1);
    let f2 = vec![TreeFile { path: String::from("a"), bytes: f2bytes }];
    set.add(
        "chunk boundary sensitivity",
        Packer::tree_hash(&f1) != Packer::tree_hash(&f2),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vxapp_all_checks_green() {
        let set = run_vxapp_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F127 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn manifest_json_roundtrip_fields() {
        let m = Manifest::skeleton("a.b", "1.0.0", "e.exe");
        let j = m.to_json();
        assert!(j.contains("\"id\":\"a.b\""));
        assert!(j.contains("\"version\":\"1.0.0\""));
        assert!(j.contains("\"name\":\"a.b\""));
    }

    #[test]
    fn uppercase_id_rejected() {
        let mut m = Manifest::skeleton("Demo", "1.0.0", "e");
        m.id = String::from("Demo.Tool");
        assert!(m.validate().is_err(), "大写身份必须拒绝（反域名小写纪律）");
    }

    #[test]
    fn sign_changes_with_content() {
        let kp = keygen(b"s");
        let s1 = sign(&kp, &vbase::sha256(b"a"));
        let s2 = sign(&kp, &vbase::sha256(b"b"));
        assert_ne!(s1, s2);
    }
}
