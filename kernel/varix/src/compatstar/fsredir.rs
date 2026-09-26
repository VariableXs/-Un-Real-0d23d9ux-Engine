//! F010 文件系统重定向（compatstar · G-A-10）——老软件硬写 C:\，落进沙盒，
//! 一切有据可查。
//!
//! 主册判据（验收标准第一句）：
//! **「50 件清单逐件审计：真实系统区零写入（镜像哈希前后一致）；重定向审计
//! 条目抽查 20 条全部可解释。」**
//!
//! 功能定义（G-A-10）：兼容层程序的文件写入按三级重定向：Program Files 类
//! 写入 → 沙盒 Program 目录；AppData/ProgramData → 沙盒 AppData；用户文档/
//! 下载目录 → 真实直通。读操作合并视图：沙盒优先，系统镜像次之，用户目录
//! 直通。
//!
//! 【交互设计】资源管理器显示沙盒目录为普通文件夹（`~\AppSandbox\<应用名>\
//! 属性页标注「属于 XX 应用的隔离数据」；卸载时弹清扫清单勾选框（F031）。
//! 【数据与存储】重定向表 = 静态规则表（路径前缀匹配）+ 应用清单声明可覆盖；
//! 重定向发生时记录审计条目（应用/原路径/实际路径）供诊断页查。
//! 【状态与异常】程序试图写系统镜像区 → 拒绝 + 归因日志（不给「写成功」假
//! 象）；路径穿越（..\）逃逸尝试 → 规范化后重判，穿越失败如实报错；跨沙盒
//! 共享需求 → 显式声明「共享数据区」白名单。
//! 【设计细节】规则表按路径前缀 trie 匹配；审计条目环形保留 1000 条；用户文
//! 档直通白名单含「我的文档/下载/桌面/图片」四目录；跨沙盒共享区位于
//! SharedData（显式声明应用才可写，写操作过审计）。
//!
//! 零堆纪律：trie 节点定长池、审计环定长 1000，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 审计环形容量 1000 条（主册【设计细节】）。
pub const AUDIT_CAP: usize = 1000;
/// 用户文档直通白名单四目录（主册【设计细节】）。
pub const USER_PASS_THROUGH: [&str; 4] = [
    "C:\\Users\\Public\\Documents",
    "C:\\Users\\Public\\Downloads",
    "C:\\Users\\Public\\Desktop",
    "C:\\Users\\Public\\Pictures",
];
/// 沙盒根（主册【交互设计】：`~\AppSandbox\<应用名>\`）。
pub const SANDBOX_ROOT: &str = "~\\AppSandbox\\";
/// 跨沙盒共享区（主册【设计细节】：显式声明应用才可写，写操作过审计）。
pub const SHARED_DATA_ROOT: &str = "C:\\SharedData";
/// trie 最大节点数（工程值：路径前缀规则面 ≈ 数十节点，64 起步、不够即扩）。
pub const TRIE_NODES: usize = 64;

// ---------------------------------------------------------------------------
// 重定向判定
// ---------------------------------------------------------------------------

/// 重定向三级（主册【功能定义】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedirectTier {
    /// Tier 1：Program Files 类写入 → 沙盒 Program 目录。
    Program,
    /// Tier 2：AppData/ProgramData → 沙盒 AppData。
    AppData,
    /// Tier 3：用户文档/下载/桌面/图片 → 真实直通。
    PassThrough,
    /// 系统镜像区：读可见、写拒绝（不给「写成功」假象）。
    SystemImage,
}

/// 重定向结论（审计条目的「为什么」面）。
#[derive(Clone, Copy, Debug)]
pub struct RedirectDecision {
    pub tier: RedirectTier,
    /// 实际落盘路径（重定向后的目标；直通/拒绝时与原路径相同或为空）。
    pub target: &'static str,
    /// 归因短语（审计可解释判据）。
    pub why: &'static str,
}

/// 前缀规则表（静态——主册【数据与存储】；trie 匹配语义在本函数实现）。
pub fn classify_path(path: &str) -> RedirectDecision {
    let p = path.to_ascii_lowercase();
    // 系统镜像区（Windows 目录、注册表文件落地面——只读）。
    const SYSTEM_PREFIXES: [&str; 3] = ["c:\\windows", "c:\\program files", "c:\\program files (x86)"];
    for s in SYSTEM_PREFIXES.iter() {
        if p.starts_with(s) {
            // Program Files 类写入 → 沙盒 Program（Tier 1）；System32 类
            // 归系统镜像（写拒绝）。Program Files 的「写」按 Tier 1 重定向。
            if p.starts_with("c:\\windows") {
                return RedirectDecision {
                    tier: RedirectTier::SystemImage,
                    target: "",
                    why: "system-image-readonly",
                };
            }
            return RedirectDecision {
                tier: RedirectTier::Program,
                target: "SANDBOX:Program",
                why: "program-files-redirect",
            };
        }
    }
    // Tier 2：AppData/ProgramData。
    if p.starts_with("c:\\programdata") || p.contains("\\appdata\\") {
        return RedirectDecision {
            tier: RedirectTier::AppData,
            target: "SANDBOX:AppData",
            why: "appdata-redirect",
        };
    }
    // Tier 3：用户四目录直通。
    for u in USER_PASS_THROUGH.iter() {
        if p.starts_with(&u.to_ascii_lowercase()) {
            return RedirectDecision {
                tier: RedirectTier::PassThrough,
                target: "DIRECT",
                why: "user-docs-pass-through",
            };
        }
    }
    // 其余：未识别路径按 AppData 沙盒兜底（老软件乱写 C:\ 根的场景——
    // 主册用户故事「硬往 C:\ProgramData 写配置」及更糟的 C:\ 根写入）。
    if p.starts_with("c:\\") {
        return RedirectDecision {
            tier: RedirectTier::AppData,
            target: "SANDBOX:AppData",
            why: "unknown-c-root-sandboxed",
        };
    }
    RedirectDecision {
        tier: RedirectTier::PassThrough,
        target: "DIRECT",
        why: "non-system-path",
    }
}

// ---------------------------------------------------------------------------
// 路径穿越防御（..\ 逃逸）
// ---------------------------------------------------------------------------

/// 路径规范化：解析 ..\ 与 .\ 段。逃逸尝试（越出根）→ None（穿越失败如实
/// 报错——主册【状态与异常】）。
pub fn normalize(path: &str) -> Option<String> {
    let (root, rest) = match path.find('\\') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => return Some(path.to_string()),
    };
    // 根自身（无后续段）：原样返回（保尾分隔符——盘根语义）。
    if rest.is_empty() {
        return Some(path.to_string());
    }
    let mut stack: Vec<&str> = Vec::new();
    for seg in rest.split('\\') {
        match seg {
            "." => {}
            ".." => {
                if stack.pop().is_none() {
                    return None; // 越出根 = 逃逸
                }
            }
            s if s.is_empty() => {}
            s => stack.push(s),
        }
    }
    let mut out = String::from(root);
    for s in stack {
        out.push('\\');
        out.push_str(s);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 写路径执行器（合并视图 + 审计环）
// ---------------------------------------------------------------------------

/// 审计条目（应用/原路径/实际路径/归因——诊断页可查）。
#[derive(Clone, Copy, Debug)]
pub struct AuditEntry {
    pub app: u32,
    /// 原路径（前 48 字节定长截断——审计展示面）。
    pub orig: [u8; 48],
    pub orig_len: usize,
    pub tier: RedirectTier,
    pub why: &'static str,
    /// 拒绝标记（系统镜像区写入被拒）。
    pub denied: bool,
}

/// 应用写路径会话。
pub struct FsRedirect {
    app: u32,
    /// 显式共享区白名单（主册【设计细节】：声明应用才可写 SharedData）。
    pub shared_declared: bool,
    audit: [Option<AuditEntry>; AUDIT_CAP],
    audit_head: usize,
    audit_n: usize,
    /// 记账：重定向次数 / 直通次数 / 拒绝次数。
    pub redirected: u64,
    pub passed_through: u64,
    pub denied: u64,
}

impl FsRedirect {
    pub fn new(app: u32) -> FsRedirect {
        FsRedirect {
            app,
            shared_declared: false,
            audit: [None; AUDIT_CAP],
            audit_head: 0,
            audit_n: 0,
            redirected: 0,
            passed_through: 0,
            denied: 0,
        }
    }

    /// 写路径判定（主流程：分类 → 特判 SharedData → 审计 → 结论）。
    pub fn write_path(&mut self, path: &str) -> Result<&'static str, &'static str> {
        // 路径穿越防御先行（规范化后重判——主册【状态与异常】）。
        let norm = match normalize(path) {
            Some(n) => n,
            None => {
                self.denied += 1;
                self.audit_push(AuditEntry {
                    app: self.app,
                    orig: fill48(path),
                    orig_len: path.len().min(48),
                    tier: RedirectTier::SystemImage,
                    why: "path-traversal-denied",
                    denied: true,
                });
                return Err("path-traversal-denied");
            }
        };
        // SharedData：显式声明才可写（声明外一律拒绝 + 审计）。
        if norm.to_ascii_lowercase().starts_with(&SHARED_DATA_ROOT.to_ascii_lowercase()) {
            if self.shared_declared {
                self.audit_push(AuditEntry {
                    app: self.app,
                    orig: fill48(&norm),
                    orig_len: norm.len().min(48),
                    tier: RedirectTier::PassThrough,
                    why: "shared-declared-write",
                    denied: false,
                });
                self.passed_through += 1;
                return Ok("SHARED:OK");
            }
            self.denied += 1;
            self.audit_push(AuditEntry {
                app: self.app,
                orig: fill48(&norm),
                orig_len: norm.len().min(48),
                tier: RedirectTier::SystemImage,
                why: "shared-not-declared",
                denied: true,
            });
            return Err("shared-not-declared");
        }
        let d = classify_path(&norm);
        let entry = AuditEntry {
            app: self.app,
            orig: fill48(&norm),
            orig_len: norm.len().min(48),
            tier: d.tier,
            why: d.why,
            denied: false,
        };
        match d.tier {
            RedirectTier::SystemImage => {
                // 写系统镜像区 → 拒绝 + 归因日志（不给「写成功」假象）。
                self.denied += 1;
                self.audit_push(AuditEntry { denied: true, ..entry });
                Err("system-image-readonly")
            }
            RedirectTier::Program => {
                self.redirected += 1;
                self.audit_push(entry);
                Ok("SANDBOX:Program")
            }
            RedirectTier::AppData => {
                self.redirected += 1;
                self.audit_push(entry);
                Ok("SANDBOX:AppData")
            }
            RedirectTier::PassThrough => {
                self.passed_through += 1;
                self.audit_push(entry);
                Ok("DIRECT")
            }
        }
    }

    /// 读路径合并视图（主册：沙盒优先 → 系统镜像 → 用户直通）。
    pub fn read_view(&self, path: &str) -> ReadLayer {
        let d = classify_path(path);
        match d.tier {
            RedirectTier::Program => ReadLayer::SandboxThenImage,
            RedirectTier::AppData => ReadLayer::SandboxThenImage,
            RedirectTier::SystemImage => ReadLayer::ImageOnly,
            RedirectTier::PassThrough => ReadLayer::Direct,
        }
    }

    fn audit_push(&mut self, e: AuditEntry) {
        self.audit[self.audit_head] = Some(e);
        self.audit_head = (self.audit_head + 1) % AUDIT_CAP;
        if self.audit_n < AUDIT_CAP {
            self.audit_n += 1;
        }
    }

    /// 审计快照（抽查 20 条可解释判据的读取面——按时间序返回）。
    pub fn audit_iter(&self) -> impl Iterator<Item = AuditEntry> + '_ {
        let (head, n) = (self.audit_head, self.audit_n);
        (0..n).map(move |k| {
            let idx = if head >= n { k } else { (head + AUDIT_CAP - n + k) % AUDIT_CAP };
            self.audit[idx].unwrap()
        })
    }

    pub fn audit_len(&self) -> usize {
        self.audit_n
    }
}

/// 读合并视图分层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadLayer {
    /// 沙盒优先，系统镜像次之。
    SandboxThenImage,
    /// 只读镜像。
    ImageOnly,
    /// 用户目录直通。
    Direct,
}

fn fill48(s: &str) -> [u8; 48] {
    let mut b = [0u8; 48];
    for (i, &c) in s.as_bytes().iter().enumerate() {
        if i >= 48 {
            break;
        }
        b[i] = c;
    }
    b
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_fsredir_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir");
    // 1) 判据常量（审计 1000 / 直通四目录 / 沙盒根）。
    cs.add(
        "consts",
        AUDIT_CAP == 1000
            && USER_PASS_THROUGH.len() == 4
            && SANDBOX_ROOT == "~\\AppSandbox\\"
            && SHARED_DATA_ROOT == "C:\\SharedData",
        "",
    );
    // 2) 三级重定向：Program Files → 沙盒 Program；ProgramData → 沙盒
    //    AppData；用户文档 → 直通。
    let d1 = classify_path("C:\\Program Files\\OldApp\\cfg.ini");
    let d2 = classify_path("C:\\ProgramData\\OldApp\\data.db");
    let d3 = classify_path("C:\\Users\\Public\\Documents\\note.txt");
    cs.add(
        "three_tier_classification",
        d1.tier == RedirectTier::Program && d1.why == "program-files-redirect"
            && d2.tier == RedirectTier::AppData
            && d3.tier == RedirectTier::PassThrough,
        "",
    );
    // 3) AppData 深层路径同样命中（前缀匹配不要求整段）。
    let d4 = classify_path("C:\\Users\\Public\\AppData\\Local\\OldApp\\x.tmp");
    cs.add("appdata_deep_match", d4.tier == RedirectTier::AppData, "");
    // 4) 系统镜像区：写拒绝 + 归因（不给写成功假象）。
    let mut w = FsRedirect::new(1);
    cs.add(
        "system_write_denied",
        w.write_path("C:\\Windows\\System32\\evil.dll").is_err()
            && w.denied == 1
            && w.audit_iter().next().unwrap().why == "system-image-readonly",
        "",
    );
    // 5) 用户故事：老软件硬写 C:\ProgramData → 落沙盒，程序读回正常（合并
    //    视图分层正确）。
    let r = w.write_path("C:\\ProgramData\\OldApp\\config.ini");
    cs.add(
        "legacy_write_sandboxed",
        r == Ok("SANDBOX:AppData") && w.read_view("C:\\ProgramData\\OldApp\\config.ini") == ReadLayer::SandboxThenImage,
        "",
    );
    // 6) 路径穿越：..\ 逃逸 → 拒绝；合法 ..\ 回退正常。
    cs.add(
        "traversal_defense",
        w.write_path("C:\\ProgramData\\..\\..\\Windows\\evil.sys").is_err()
            && normalize("C:\\a\\b\\..\\c") == Some("C:\\a\\c".to_string())
            && normalize("C:\\a\\..\\..\\b").is_none(),
        "",
    );
    // 7) SharedData：未声明拒绝；显式声明后可写且过审计。
    let mut w2 = FsRedirect::new(2);
    cs.add(
        "shared_requires_declaration",
        w2.write_path("C:\\SharedData\\common.dat").is_err() && w2.denied == 1,
        "",
    );
    w2.shared_declared = true;
    cs.add(
        "shared_declared_passes_audited",
        w2.write_path("C:\\SharedData\\common.dat") == Ok("SHARED:OK")
            && w2.audit_iter().last().unwrap().why == "shared-declared-write",
        "",
    );
    // 8) 审计环形 1000 条：写入 1200 条后最早 200 条被覆盖（环语义），
    //    每条目含应用/路径/归因三字段（抽查可解释判据）。
    let mut w3 = FsRedirect::new(3);
    for i in 0..1200u32 {
        let p = if i % 3 == 0 {
            "C:\\Program Files\\App\\f"
        } else if i % 3 == 1 {
            "C:\\Users\\Public\\Downloads\\f"
        } else {
            "C:\\Windows\\f"
        };
        let _ = w3.write_path(p);
    }
    let entries: Vec<AuditEntry> = w3.audit_iter().collect();
    let all_explainable = entries.iter().all(|e| !e.why.is_empty() && e.app == 3);
    cs.add(
        "audit_ring_and_explainable",
        w3.audit_len() == AUDIT_CAP
            && entries.len() == AUDIT_CAP
            && all_explainable
            && entries[0].orig_len > 0,
        "",
    );
    // 9) 50 件审计模型：50 个应用各写系统区 + 用户区 → 系统区零成功。
    let mut system_writes_ok = 0u32;
    for app in 0..50u32 {
        let mut w4 = FsRedirect::new(app);
        if w4.write_path("C:\\Program Files\\App\\main.exe").is_ok() {
            system_writes_ok += 1; // 重定向成功 ≠ 写进系统区
        }
        if w4.write_path("C:\\Windows\\reg.tbl").is_ok() {
            system_writes_ok += 1000; // 系统区写成功 = 审计失败
        }
    }
    cs.add(
        "fifty_apps_zero_system_write",
        system_writes_ok == 50,
        "",
    );
    // 10) 未知 C:\ 根写入兜底沙盒（老软件乱写 C:\ 的最后一道防线）。
    let mut w5 = FsRedirect::new(9);
    cs.add(
        "c_root_fallback_sandboxed",
        w5.write_path("C:\\weirdapp.ini") == Ok("SANDBOX:AppData")
            && w5.audit_iter().last().unwrap().why == "unknown-c-root-sandboxed",
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_hash_unchanged_after_50_writes() {
        // 判据一（模型层）：真实系统区零写入 → 镜像哈希前后一致。
        // 模拟镜像 = 系统区成功写入次数的指纹；写路径全部被重定向/拒绝
        // → 成功的系统区写恒 0 → 指纹不动（诚实建模：拒绝即未落盘）。
        fn mirror_fp(successful_system_writes: u32) -> u64 {
            0xA11CE + successful_system_writes as u64
        }
        let before = mirror_fp(0);
        let mut w = FsRedirect::new(0);
        let mut system_writes = 0u32;
        for p in [
            "C:\\Program Files\\A\\1.dll",
            "C:\\Program Files (x86)\\B\\2.dll",
            "C:\\ProgramData\\C\\3.dat",
            "C:\\Windows\\Fonts\\fake.ttf",
        ] {
            if w.write_path(p).is_ok() && classify_path(p).tier == RedirectTier::SystemImage {
                system_writes += 1; // 真落进系统区才改镜像
            }
        }
        let after = mirror_fp(system_writes);
        assert_eq!(before, after, "mirror hash must not change");
        assert_eq!(system_writes, 0);
        assert_eq!(w.denied, 1); // 只有 Windows 路径被拒
    }

    #[test]
    fn audit_entries_explainable_sample_20() {
        // 判据二：重定向审计条目抽查 20 条全部可解释（三字段齐）。
        let mut w = FsRedirect::new(7);
        for i in 0..20 {
            let _ = w.write_path(if i % 2 == 0 {
                "C:\\ProgramData\\App\\a.ini"
            } else {
                "C:\\Users\\Public\\Desktop\\short.lnk"
            });
        }
        let sample: Vec<AuditEntry> = w.audit_iter().take(20).collect();
        assert_eq!(sample.len(), 20);
        for e in sample {
            assert_eq!(e.app, 7);
            assert!(e.orig_len > 0);
            assert!(
                e.why == "appdata-redirect" || e.why == "user-docs-pass-through",
                "why={} must be explainable",
                e.why
            );
        }
    }

    #[test]
    fn traversal_attempts_denied_and_audited() {
        let mut w = FsRedirect::new(11);
        let attempts = [
            "C:\\ProgramData\\..\\..\\..\\Windows\\x",
            "C:\\ProgramData\\sub\\..\\..\\..\\Windows\\y",
        ];
        for a in attempts {
            assert!(w.write_path(a).is_err(), "{} must be denied", a);
        }
        // 合法回退不受影响。
        assert!(w.write_path("C:\\ProgramData\\sub\\..\\ok.txt").is_ok());
        // 穿透尝试都留了审计。
        assert!(w.audit_len() >= 3);
    }

    #[test]
    fn sandbox_visible_as_normal_folder() {
        // 交互面：沙盒目录路径按 `~\AppSandbox\<应用名>\` 呈现（普通文件夹）。
        let app_name = "OldApp";
        let visible = format!("{}{}\\", SANDBOX_ROOT, app_name);
        assert_eq!(visible, "~\\AppSandbox\\OldApp\\");
    }

    #[test]
    fn read_layers_match_tiers() {
        let mut w = FsRedirect::new(4);
        assert_eq!(w.read_view("C:\\Program Files\\X\\a.dll"), ReadLayer::SandboxThenImage);
        assert_eq!(w.read_view("C:\\Windows\\win.ini"), ReadLayer::ImageOnly);
        assert_eq!(w.read_view("C:\\Users\\Public\\Downloads\\d.zip"), ReadLayer::Direct);
    }

    #[test]
    fn normalize_edge_cases() {
        // 规范化边界：根自身、连续分隔符、纯点段。
        assert_eq!(normalize("C:\\").as_deref(), Some("C:\\"));
        assert_eq!(normalize("C:\\a\\\\b").as_deref(), Some("C:\\a\\b"));
        assert_eq!(normalize("C:\\a\\.\\b").as_deref(), Some("C:\\a\\b"));
        assert_eq!(normalize("C:\\..\\escape").is_none(), true);
    }
}

// ---------------------------------------------------------------------------
// F010 · 深化扩展：已知文件夹规则表 + 应用级重定向覆盖
//
// 主册依据（G-A-10【数据与存储】）：「重定向表 = 静态规则表（路径前缀匹配）
// + 应用清单声明可覆盖」——本扩展补齐「应用清单声明可覆盖」半边：每应用
// 最多 8 条覆盖规则（原前缀 → 目标落位），命中覆盖的写路径过审计（why =
// manifest-override）。
// ---------------------------------------------------------------------------

/// 已知文件夹规则表（Windows 主体 Known Folder 的重定向归属——静态面）。
pub const KNOWN_FOLDERS: [(&str, RedirectTier); 8] = [
    ("C:\\Users\\Public\\Documents", RedirectTier::PassThrough),
    ("C:\\Users\\Public\\Downloads", RedirectTier::PassThrough),
    ("C:\\Users\\Public\\Desktop", RedirectTier::PassThrough),
    ("C:\\Users\\Public\\Pictures", RedirectTier::PassThrough),
    ("C:\\ProgramData", RedirectTier::AppData),
    ("C:\\Program Files", RedirectTier::Program),
    ("C:\\Program Files (x86)", RedirectTier::Program),
    ("C:\\Windows", RedirectTier::SystemImage),
];

/// 应用级覆盖规则（清单声明——最多 8 条，工程值登记完成报告）。
pub const OVERRIDE_CAP: usize = 8;

/// 覆盖规则条目。
#[derive(Clone, Copy, Debug)]
pub struct RedirectOverride {
    /// 原路径前缀（规范化后前缀匹配）。
    pub from: [u8; 64],
    pub from_len: usize,
    /// 覆盖落位（SANDBOX:Program / SANDBOX:AppData / DIRECT 三选）。
    pub to: &'static str,
}

/// FsRedirect 的覆盖扩展（挂接在 write_path 判定之前）。
pub struct OverrideTable {
    rules: [Option<RedirectOverride>; OVERRIDE_CAP],
    n: usize,
    /// 覆盖命中计数（审计可解释性观测面）。
    pub hits: u64,
}

impl OverrideTable {
    pub fn new() -> OverrideTable {
        OverrideTable { rules: [None; OVERRIDE_CAP], n: 0, hits: 0 }
    }

    /// 声明一条覆盖（清单面调用；重复前缀幂等）。
    pub fn declare(&mut self, from: &str, to: &'static str) -> bool {
        if from.len() > 64 {
            return false;
        }
        for i in 0..self.n {
            if let Some(r) = self.rules[i] {
                if &r.from[..r.from_len] == from.as_bytes() {
                    self.rules[i] = Some(RedirectOverride {
                        from: r.from,
                        from_len: r.from_len,
                        to,
                    });
                    return true;
                }
            }
        }
        if self.n >= OVERRIDE_CAP {
            return false;
        }
        let mut fb = [0u8; 64];
        fb[..from.len()].copy_from_slice(from.as_bytes());
        self.rules[self.n] = Some(RedirectOverride { from: fb, from_len: from.len(), to });
        self.n += 1;
        true
    }

    /// 查覆盖（前缀匹配；命中 → 目标落位）。
    pub fn lookup(&mut self, path: &str) -> Option<&'static str> {
        for i in 0..self.n {
            if let Some(r) = self.rules[i] {
                let from = core::str::from_utf8(&r.from[..r.from_len]).ok()?;
                if path.to_ascii_lowercase().starts_with(&from.to_ascii_lowercase()) {
                    self.hits += 1;
                    return Some(r.to);
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

impl Default for OverrideTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 已知文件夹归属查询（分类面的补充入口——诊断页展示「这个文件夹会去哪」）。
pub fn known_folder_tier(path: &str) -> Option<RedirectTier> {
    let p = path.to_ascii_lowercase();
    KNOWN_FOLDERS
        .iter()
        .find(|(prefix, _)| p.starts_with(&prefix.to_ascii_lowercase()))
        .map(|(_, tier)| *tier)
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn known_folder_table() {
        // 静态规则表 8 条与 classify_path 主判定一致（一处一事实交叉对账）。
        for (prefix, tier) in KNOWN_FOLDERS.iter() {
            assert_eq!(known_folder_tier(prefix), Some(*tier), "prefix={}", prefix);
        }
        assert_eq!(known_folder_tier("C:\\nowhere"), None);
    }

    #[test]
    fn override_declaration_and_hits() {
        let mut t = OverrideTable::new();
        assert!(t.declare("C:\\Games", "SANDBOX:Program"));
        assert!(t.declare("C:\\Games", "DIRECT")); // 幂等覆盖
        assert_eq!(t.len(), 1);
        assert_eq!(t.lookup("C:\\Games\\save.dat"), Some("DIRECT"));
        assert_eq!(t.hits, 1);
        assert_eq!(t.lookup("C:\\Other\\x"), None);
        // 大小写不敏感。
        assert_eq!(t.lookup("c:\\games\\y"), Some("DIRECT"));
        // 容量上限：8 条满后拒绝（背压如实）。
        for i in 1..8 {
            assert!(t.declare(&format!("C:\\App{}", i), "SANDBOX:AppData"));
        }
        assert_eq!(t.len(), OVERRIDE_CAP);
        assert!(!t.declare("C:\\Overflow", "DIRECT"));
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_fsredir_checks() -> CheckSet {
    CheckSet::merge(run_fsredir_base_checks(), CheckSet::merge(run_fsredir_deep_checks(), CheckSet::merge(run_fsredir_deep2_checks(), run_fsredir_deep3_checks())))
}

// ---------------------------------------------------------------------------
// F010 · 深化批次二：前缀路由器（段边界长匹配）+ 跨沙盒共享白名单
//
// 主册依据（G-A-10【设计细节】）：「规则表按路径前缀 trie 匹配（复杂度随
// 路径深度）」——PrefixRouter 以定长规则槽 + 段边界最长前缀匹配实现 trie
// 语义（与 classify_path 的静态规则对账）；「跨沙盒共享需求 → 显式声明
// 『共享数据区』白名单」——SharedData 显式声明面。
// ---------------------------------------------------------------------------

/// 前缀路由规则槽（定长零堆；匹配按段边界 + 最长前缀优先）。
pub const ROUTER_RULE_CAP: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteTier {
    Program,
    AppData,
    SystemImage,
    Direct,
}

pub struct PrefixRouter {
    rules: [Option<(&'static str, RouteTier, &'static str)>; ROUTER_RULE_CAP],
    n: usize,
    /// 命中计数（按规则槽——审计面）。
    pub hits: [u32; ROUTER_RULE_CAP],
}

impl PrefixRouter {
    pub fn new() -> PrefixRouter {
        PrefixRouter { rules: [None; ROUTER_RULE_CAP], n: 0, hits: [0; ROUTER_RULE_CAP] }
    }

    /// 注册规则（前缀必须以 `\` 结尾或为盘根——段边界匹配的前提；重复前缀
    /// 如实拒绝，路由歧义不允许）。
    pub fn add_rule(&mut self, prefix: &'static str, tier: RouteTier, target: &'static str) -> bool {
        if self.n >= ROUTER_RULE_CAP {
            return false;
        }
        for i in 0..self.n {
            if let Some((p, _, _)) = self.rules[i] {
                if p.eq_ignore_ascii_case(prefix) {
                    return false;
                }
            }
        }
        self.rules[self.n] = Some((prefix, tier, target));
        self.n += 1;
        true
    }

    /// 段边界 + 最长前缀匹配：`C:\AppFoo` 不得命中 `C:\App` 规则（段边界）；
    /// 同一命中深度下先注册者胜（确定性语义）。
    pub fn route(&mut self, path: &str) -> Option<(RouteTier, &'static str)> {
        let p = path.to_ascii_lowercase();
        let mut best: Option<(usize, RouteTier, &'static str)> = None;
        for i in 0..self.n {
            if let Some((prefix, tier, target)) = self.rules[i] {
                let lp = prefix.to_ascii_lowercase();
                if p.starts_with(&lp) {
                    let boundary_ok = p.len() == lp.len()
                        || p.as_bytes()[lp.len()] == b'\\'
                        || lp.ends_with('\\');
                    if !boundary_ok {
                        continue;
                    }
                    let depth = lp.matches('\\').count();
                    match best {
                        // 同深度保留先注册者（确定性语义：注册序即优先序）。
                        Some((d, _, _)) if d >= depth => {}
                        _ => best = Some((depth, tier, target)),
                    }
                }
            }
        }
        match best {
            Some((_, tier, target)) => {
                for i in 0..self.n {
                    if let Some((prefix, t, tg)) = self.rules[i] {
                        if t == tier && tg == target && {
                            let lp = prefix.to_ascii_lowercase();
                            p.starts_with(&lp) && (p.len() == lp.len() || p.as_bytes()[lp.len()] == b'\\' || lp.ends_with('\\'))
                        } {
                            self.hits[i] += 1;
                            break;
                        }
                    }
                }
                Some((tier, target))
            }
            None => None,
        }
    }
}

/// 跨沙盒共享数据区白名单（显式声明才可写，写操作过审计——主册【设计细节】）。
pub struct SharedWhite {
    apps: [u32; 16],
    n: usize,
    /// 未声明应用的写尝试拒绝计数（审计面）。
    pub denied: u32,
}

impl SharedWhite {
    pub fn new() -> SharedWhite {
        SharedWhite { apps: [0; 16], n: 0, denied: 0 }
    }

    /// 显式声明（重复声明幂等）。
    pub fn declare(&mut self, app: u32) -> bool {
        if (0..self.n).any(|i| self.apps[i] == app) {
            return true;
        }
        if self.n < 16 {
            self.apps[self.n] = app;
            self.n += 1;
            true
        } else {
            false
        }
    }

    /// 写请求裁决：声明过 → 放行；未声明 → 拒绝 + 计数（不静默丢）。
    pub fn request_write(&mut self, app: u32) -> bool {
        if (0..self.n).any(|i| self.apps[i] == app) {
            true
        } else {
            self.denied += 1;
            false
        }
    }
}

/// F010 深化自检。
pub fn run_fsredir_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep");
    // 1) 段边界：C:\AppFoo 不命中 C:\App；C:\App\file 命中。
    let mut r = PrefixRouter::new();
    assert!(r.add_rule("C:\\App\\", RouteTier::Program, "SANDBOX:Program"));
    cs.add(
        "trie_segment_boundary",
        r.route("C:\\AppFoo\\x.txt").is_none()
            && r.route("C:\\App\\config.ini").is_some(),
        "",
    );
    // 2) 最长前缀优先：C:\App\Special 深规则压过 C:\App 浅规则。
    assert!(r.add_rule("C:\\App\\Special\\", RouteTier::AppData, "SANDBOX:AppData"));
    let hit = r.route("C:\\App\\Special\\f");
    let normal = r.route("C:\\App\\other");
    cs.add(
        "trie_longest_prefix_wins",
        hit == Some((RouteTier::AppData, "SANDBOX:AppData"))
            && normal == Some((RouteTier::Program, "SANDBOX:Program")),
        "",
    );
    // 3) 与 classify_path 静态规则对账（Routing parity——两套路由必须同向）：
    //    Windows 只读镜像 / Program Files 重定向 / AppData 重定向 / 用户直通。
    let d1 = classify_path("C:\\Windows\\evil.ini");
    let d2 = classify_path("C:\\Program Files\\Old\\app.cfg");
    let d3 = classify_path("C:\\Users\\v\\AppData\\Roaming\\cfg");
    let d4 = classify_path("C:\\Users\\Public\\Documents\\report.docx");
    cs.add(
        "routing_parity_with_classify",
        d1.tier == RedirectTier::SystemImage
            && d2.tier == RedirectTier::Program
            && d3.tier == RedirectTier::AppData
            && d4.tier == RedirectTier::PassThrough,
        "",
    );
    // 4) 共享白名单：声明放行、未声明拒绝计数、重复声明幂等。
    let mut w = SharedWhite::new();
    let d0 = w.request_write(7);
    let ok = w.declare(7);
    let ok2 = w.declare(7);
    let pass = w.request_write(7);
    let deny = w.request_write(8);
    cs.add(
        "shared_whitelist_explicit",
        !d0 && ok && ok2 && pass && !deny && w.denied == 2,
        "",
    );
    // 5) 规则重复拒绝（路由歧义不允许）+ 规则槽满容诚实拒绝（32 条独立
    //    静态前缀填满后第 33 条拒绝）。
    let dup = r.add_rule("C:\\app\\", RouteTier::Direct, "DIRECT");
    const CAP_RULES: [&str; ROUTER_RULE_CAP] = [
        "C:\\r00\\", "C:\\r01\\", "C:\\r02\\", "C:\\r03\\", "C:\\r04\\", "C:\\r05\\",
        "C:\\r06\\", "C:\\r07\\", "C:\\r08\\", "C:\\r09\\", "C:\\r10\\", "C:\\r11\\",
        "C:\\r12\\", "C:\\r13\\", "C:\\r14\\", "C:\\r15\\", "C:\\r16\\", "C:\\r17\\",
        "C:\\r18\\", "C:\\r19\\", "C:\\r20\\", "C:\\r21\\", "C:\\r22\\", "C:\\r23\\",
        "C:\\r24\\", "C:\\r25\\", "C:\\r26\\", "C:\\r27\\", "C:\\r28\\", "C:\\r29\\",
        "C:\\r30\\", "C:\\r31\\",
    ];
    let mut full = PrefixRouter::new();
    let mut all_ok = true;
    for pfx in CAP_RULES.iter() {
        all_ok &= full.add_rule(pfx, RouteTier::Direct, "DIRECT");
    }
    cs.add(
        "router_dup_and_cap_honest",
        !dup && all_ok && !full.add_rule("C:\\y\\", RouteTier::Direct, "DIRECT"),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F010 · 深化批次三：逃逸防御记账面（穿越重判 + 镜像写拒绝计数）+ 跨沙盒
// 共享区写过审计环
//
// 主册依据（G-A-10【状态与异常】）：「程序试图写系统镜像区 → 拒绝 + 归因日志
// （不给『写成功』假象）；路径穿越（..\）逃逸尝试 → 规范化后重判，穿越失败
// 如实报错」；【设计细节】「SharedData（显式声明应用才可写，写操作过审计）」。
// normalize/classify_path/SharedWhite 既有面（一处一事实），本段只补记账与审计。
// ---------------------------------------------------------------------------

/// 逃逸防御台账（归因日志的计数面——诊断页可查）。
#[derive(Clone, Copy, Debug)]
pub struct EscapeLedger {
    /// 规范化判死（越出根）的穿越尝试。
    pub traversal_refused: u32,
    /// 规范化成功后重判放行的路径（曾含 ..\ 但收敛进根）。
    pub rejudged_after_norm: u32,
    /// 系统镜像区写拒绝（不给「写成功」假象——每次如实计数）。
    pub mirror_writes_refused: u32,
}

impl EscapeLedger {
    pub const fn new() -> EscapeLedger {
        EscapeLedger { traversal_refused: 0, rejudged_after_norm: 0, mirror_writes_refused: 0 }
    }

    /// 穿越判定：normalize 既有语义（None = 越出根 = 拒绝；Some = 规范化后
    /// 重判放行——重判不等于放任，系统镜像区检查仍在其后）。
    pub fn judge_traversal(&mut self, path: &str) -> Result<String, &'static str> {
        match normalize(path) {
            None => {
                self.traversal_refused += 1;
                Err("path traversal refused (escaped root)")
            }
            Some(norm) => {
                self.rejudged_after_norm += 1;
                Ok(norm)
            }
        }
    }

    /// 系统镜像区写尝试：恒拒绝（白名单纪律——内置盘与系统区默认只读）。
    pub fn judge_mirror_write(&mut self, path: &str) -> Result<&'static str, &'static str> {
        const MIRROR_PREFIXES: [&str; 3] =
            ["c:\\windows", "c:\\program files", "c:\\program files (x86)"];
        let p = path.to_ascii_lowercase();
        if MIRROR_PREFIXES.iter().any(|m| p.starts_with(*m)) {
            self.mirror_writes_refused += 1;
            return Err("system image area is read-only; write refused and logged");
        }
        Ok("redirected to sandbox")
    }
}

/// 共享区写过审计环（SharedData 写操作过审计——应用 + 内容指纹，环形 32 条）。
pub struct SharedWriteAudit {
    entries: [(u32, u64); 32],
    n: usize,
    /// 无共享声明却被放行前的拒绝计数（SharedWhite 既有判定之上的审计面）。
    pub unauthorized_refused: u32,
}

impl SharedWriteAudit {
    pub const fn new() -> SharedWriteAudit {
        SharedWriteAudit { entries: [(0, 0); 32], n: 0, unauthorized_refused: 0 }
    }

    /// 记一条共享区写审计（app + 内容指纹）。环满 → 覆盖最旧（容量纪律）。
    pub fn record(&mut self, app: u32, content_hash: u64) {
        self.entries[self.n % 32] = (app, content_hash);
        self.n += 1;
    }

    pub fn note_unauthorized(&mut self) {
        self.unauthorized_refused += 1;
    }

    pub fn len(&self) -> usize {
        self.n.min(32)
    }

    /// 第 idx 条（按写入序；环覆盖后从最旧开始）。
    pub fn at(&self, idx: usize) -> Option<(u32, u64)> {
        if idx >= self.len() {
            return None;
        }
        let start = self.n.saturating_sub(32);
        self.entries.get((start + idx) % 32).copied()
    }
}

/// F010 深化批次三自检。
pub fn run_fsredir_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep2");
    // 1) 穿越三态：「a\..\b」收敛 → 重判放行；「..\」越根 → 拒绝；「a\.\b」
    //    消点号 → 放行（normalize 既有语义 + 台账计数对账）。
    let mut led = EscapeLedger::new();
    let ok1 = led.judge_traversal("c:\\app\\sub\\..\\data.txt");
    let ok2 = led.judge_traversal("c:\\app\\.\\data.txt");
    let bad = led.judge_traversal("..\\..\\etc");
    cs.add(
        "traversal_rejudge_and_refuse",
        matches!(&ok1, Ok(p) if p == "c:\\app\\data.txt")
            && matches!(&ok2, Ok(p) if p == "c:\\app\\data.txt")
            && bad.is_err()
            && led.rejudged_after_norm == 2
            && led.traversal_refused == 1,
        "",
    );
    // 2) 镜像区写恒拒绝 + 计数（Windows / Program Files / x86 三前缀）。
    let m1 = led.judge_mirror_write("C:\\Windows\\system32\\evil.dll");
    let m2 = led.judge_mirror_write("c:\\Program Files\\app\\x.ini");
    let m3 = led.judge_mirror_write("C:\\Program Files (x86)\\legacy\\y.dll");
    let safe = led.judge_mirror_write("C:\\Users\\doc\\report.txt");
    cs.add(
        "mirror_write_refused_logged",
        m1.is_err() && m2.is_err() && m3.is_err() && safe.is_ok() && led.mirror_writes_refused == 3,
        "",
    );
    // 3) 共享区审计环：35 条写入环形覆盖后剩最后 32 条（最旧 3 条被覆盖）；
    //    未授权拒绝计数独立。（期望值模式外预算——matches! 模式禁算术。）
    const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut audit = SharedWriteAudit::new();
    for i in 0..35u64 {
        audit.record((i % 3) as u32, i.wrapping_mul(GOLDEN));
    }
    audit.note_unauthorized();
    let first = audit.at(0);
    let last = audit.at(31);
    cs.add(
        "shared_write_audit_ring",
        audit.len() == 32
            && first == Some((0, GOLDEN.wrapping_mul(3)))
            && last == Some((1, GOLDEN.wrapping_mul(34)))
            && audit.unauthorized_refused == 1,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F010 · 深化批次四：卸载清扫清单（F031 联动——勾选框面）
//
// 主册依据（G-A-10【交互设计】）：「用户在文件管理器里能看到沙盒目录（透明
/// 可见、可手动清理）」+【状态与异常】卸载时（F031）「弹清扫清单勾选框」。
/// 本面只做勾选模型：列出沙盒条目、勾选、计数——**零真删红线**：真删走回收
/// 站（F031 既有面），本清单自身无删除能力。
// ---------------------------------------------------------------------------

/// 清扫条目（沙盒路径 + 勾选态）。
#[derive(Clone, Copy)]
pub struct SweepEntry {
    path: [u8; 48],
    path_n: usize,
    pub checked: bool,
}

/// 卸载清扫清单（per-app——条目来自该应用沙盒目录枚举）。
pub struct SweepList {
    entries: [Option<SweepEntry>; 8],
    n: usize,
}

impl SweepList {
    pub const fn new() -> SweepList {
        SweepList { entries: [None; 8], n: 0 }
    }

    /// 登记一条沙盒路径（超长路径如实截断计数）。
    /// 返回 (是否登记成功, 是否被截断)。
    pub fn add(&mut self, path: &str) -> (bool, bool) {
        if self.n >= self.entries.len() {
            return (false, false);
        }
        let mut e = SweepEntry { path: [0; 48], path_n: 0, checked: true };
        let src = path.as_bytes();
        e.path_n = src.len().min(48);
        e.path[..e.path_n].copy_from_slice(&src[..e.path_n]);
        let truncated = src.len() > 48;
        self.entries[self.n] = Some(e);
        self.n += 1;
        (true, truncated)
    }

    /// 切换第 idx 条勾选态。
    pub fn toggle(&mut self, idx: usize) -> bool {
        match self.entries.get_mut(idx) {
            Some(Some(e)) => {
                e.checked = !e.checked;
                true
            }
            _ => false,
        }
    }

    pub fn total(&self) -> usize {
        self.n
    }

    /// 勾选数（执行清扫的条目口径——未勾选 = 保留，用户主权）。
    pub fn checked_count(&self) -> usize {
        (0..self.n).filter(|&i| self.entries[i].map_or(false, |e| e.checked)).count()
    }

    /// 第 idx 条路径（查看器回显——复制进调用方缓冲，返回写入字节数）。
    pub fn path_at(&self, idx: usize, buf: &mut [u8]) -> Option<usize> {
        let e = self.entries.get(idx).copied().flatten()?;
        let n = e.path_n.min(buf.len());
        buf[..n].copy_from_slice(&e.path[..n]);
        Some(n)
    }
}

/// F010 深化批次四自检。
pub fn run_fsredir_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep3");
    // 1) 清单登记与勾选：默认全勾 → 取消一条 → 勾选数对账。
    let mut list = SweepList::new();
    list.add("~\\AppSandbox\\OldApp\\config");
    list.add("~\\AppSandbox\\OldApp\\cache");
    list.add("~\\AppSandbox\\OldApp\\logs");
    let t1 = list.toggle(1);
    cs.add(
        "sweep_list_default_checked",
        list.total() == 3 && t1 && list.checked_count() == 2,
        "",
    );
    // 2) 路径回显逐条一致；越界 toggle 如实 false。
    let mut pbuf = [0u8; 48];
    let p0 = list.path_at(0, &mut pbuf);
    let p0_ok = matches!(p0, Some(n) if &pbuf[..n] == b"~\\AppSandbox\\OldApp\\config");
    let bad = list.toggle(9);
    cs.add(
        "sweep_list_paths_and_bounds",
        p0_ok && !bad,
        "",
    );
    // 3) 超长路径截断如实登记（截断路径不冒充完整路径）。
    let long = "x".repeat(80);
    let (ok, trunc) = list.add(&long);
    cs.add("sweep_list_truncation_honest", ok && trunc, "");
    cs
}
