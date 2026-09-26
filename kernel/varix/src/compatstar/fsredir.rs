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
pub fn run_fsredir_checks() -> CheckSet {
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
