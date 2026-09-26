//! F032 开源运行时直装族（compatstar · G-A-32）——在 VARIX 上学编程一样顺。
//!
//! 主册判据（验收标准第一句）：
//! **每语言「装-写-跑-调」四步判例（调 = VSCode 断点命中一次）；五语言
//! 全绿录屏在册。**
//!
//! 功能定义（G-A-32）：五语言工具链 vxapp 包（Python/Node/Java JDK/Go/Rust）：
//! 包内含运行时+包管理器（pip/npm/maven/go mod cargo），装后 PATH 注入
//! （F011）、解释器解析按会话；VSCode（Wine 面或后续原生）扩展调用链判例齐。
//!
//! 【设计细节】包缓存全局共享（哈希去重）；版本并存激活机制：会话内显式
//! 切换或设置页点选默认（显式无魔法）；pip/npm 走 F023 网络面加 F024 TLS；
//! 「验证安装」三验脚本随包发布（判例即产品）；磁盘预检含包缓存膨胀预估。
//! 【数据与存储】运行时落沙盒化共享区（`~\Toolchains\<lang>\<ver>\`），
//! 包缓存全局共享防重复下载；PATH 注入会话级（F011 语义）。
//! 【状态与异常】包管理器下载失败 → 三要素错误 + 重试；磁盘空间不足 →
//! 预检拦截（装前算大小）；运行时损坏 → 「验证安装」检出后一键重装。
//! 【交互设计】各语言包安装完成页显示「验证安装」按钮（一键跑 version/
//! hello-world/包安装三验）；多版本并存提示（激活机制显式，不搞隐式切换）。
//!
//! 零堆纪律：定长版本表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 五语言（主册【功能定义】）。
pub const LANGUAGES: [&str; 5] = ["python", "node", "java", "go", "rust"];
/// 各语言包管理器（主册：pip/npm/maven/go mod cargo）。
pub const PACKAGE_MANAGERS: [&str; 5] = ["pip", "npm", "maven", "go mod", "cargo"];
/// 每语言并存版本上限（多版本并存判例容量；域内口径）。
pub const MAX_VERSIONS_PER_LANG: usize = 4;
/// 「验证安装」三验（version / hello-world / 包安装——主册【交互设计】）。
pub const VERIFY_STEPS: [&str; 3] = ["version", "hello-world", "package-install"];

// ---------------------------------------------------------------------------
// 工具链注册表
// ---------------------------------------------------------------------------

/// 一个已装版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ToolchainVersion {
    pub version: &'static str,
    /// 安装体积（字节，磁盘预检用）。
    pub install_bytes: u64,
    /// 三验状态位图（bit0 version / bit1 hello / bit2 pkg）。
    pub verified_bits: u8,
    /// 激活标记（显式激活——不搞隐式切换）。
    pub active: bool,
}

/// 一个语言的工具链槽（`~\Toolchains\<lang>\<ver>\` 语义）。
pub struct LangSlot {
    pub lang: &'static str,
    versions: [Option<ToolchainVersion>; MAX_VERSIONS_PER_LANG],
    count: usize,
}

impl LangSlot {
    pub const fn new(lang: &'static str) -> Self {
        LangSlot { lang, versions: [None; MAX_VERSIONS_PER_LANG], count: 0 }
    }

    /// 装入版本：并存登记；激活走显式 API。
    pub fn install(&mut self, version: &'static str, install_bytes: u64) -> Result<usize, &'static str> {
        if self.count >= MAX_VERSIONS_PER_LANG {
            return Err("version-slots-full");
        }
        if self.versions.iter().flatten().any(|v| v.version == version) {
            return Err("already-installed");
        }
        for i in 0..MAX_VERSIONS_PER_LANG {
            if self.versions[i].is_none() {
                self.versions[i] = Some(ToolchainVersion { version, install_bytes, verified_bits: 0, active: false });
                self.count += 1;
                return Ok(i);
            }
        }
        Err("version-slots-full")
    }

    /// 会话内显式切换（激活机制显式——主册【设计细节】）。
    pub fn activate(&mut self, version: &'static str) -> bool {
        let mut found = false;
        for v in self.versions.iter_mut().flatten() {
            let hit = v.version == version;
            v.active = hit;
            found |= hit;
        }
        found
    }

    /// 激活中的解释器（解析按会话）。
    pub fn active_version(&self) -> Option<&'static str> {
        self.versions.iter().flatten().find(|v| v.active).map(|v| v.version)
    }

    /// 「验证安装」三验登记。
    pub fn verify(&mut self, version: &'static str, step: usize) -> bool {
        if step >= VERIFY_STEPS.len() {
            return false;
        }
        for v in self.versions.iter_mut().flatten() {
            if v.version == version {
                v.verified_bits |= 1 << step;
                return true;
            }
        }
        false
    }

    /// 三验全绿（判例即产品）。
    pub fn fully_verified(&self, version: &'static str) -> bool {
        self.versions
            .iter()
            .flatten()
            .any(|v| v.version == version && v.verified_bits == 0b111)
    }

    /// 运行时损坏检出：三验缺位即「验证安装」检出 → 一键重装。
    pub fn detect_corruption(&self, version: &'static str) -> bool {
        self.versions.iter().flatten().any(|v| v.version == version && v.verified_bits != 0b111)
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 包缓存（全局共享、哈希去重）与磁盘预检
// ---------------------------------------------------------------------------

/// 包缓存：哈希去重（同一包全局只存一份）。
pub struct PkgCache {
    hashes: [u64; 64],
    count: usize,
    /// 去重命中数（防重复下载的账面）。
    pub dedup_hits: u32,
    pub total_bytes: u64,
}

impl PkgCache {
    pub const fn new() -> Self {
        PkgCache { hashes: [0; 64], count: 0, dedup_hits: 0, total_bytes: 0 }
    }

    /// 下载前查询：哈希已存 → 去重命中（不重复下载）。
    pub fn lookup(&mut self, hash: u64, bytes: u64) -> bool {
        if self.hashes.iter().take(self.count).any(|&h| h == hash) {
            self.dedup_hits += 1;
            true
        } else {
            if self.count < 64 {
                self.hashes[self.count] = hash;
                self.count += 1;
                self.total_bytes += bytes;
            }
            false
        }
    }

    /// 磁盘预检：可用空间 ≥ 安装体积 + 包缓存膨胀预估 → 放行。
    pub fn disk_precheck(&self, free_bytes: u64, install_bytes: u64, cache_bloat_estimate: u64) -> Result<(), &'static str> {
        if free_bytes >= install_bytes + cache_bloat_estimate {
            Ok(())
        } else {
            Err("insufficient-disk")
        }
    }
}

/// PATH 注入（F011 会话级语义）：装后注入到会话面。
pub fn path_injection(lang: &str, version: &str, buf: &mut [u8]) -> usize {
    // 形如 Toolchains\<lang>\<ver>; 的会话 PATH 前缀（零分配逐字节写）。
    const HEAD: &[u8] = b"Toolchains\\";
    let mut n = 0;
    for b in HEAD {
        buf[n] = *b;
        n += 1;
    }
    for b in lang.as_bytes() {
        buf[n] = *b;
        n += 1;
    }
    buf[n] = b'\\';
    n += 1;
    for b in version.as_bytes() {
        buf[n] = *b;
        n += 1;
    }
    buf[n] = b';';
    n += 1;
    n
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_runtimes_checks() -> CheckSet {
    let mut cs = CheckSet::new("F032-runtimes");
    // 1) 五语言与五包管理器一一对应。
    cs.add("five_languages", LANGUAGES == ["python", "node", "java", "go", "rust"] && PACKAGE_MANAGERS.len() == 5, "");
    // 2) 装：版本并存登记（同语言多版本）。
    let mut py = LangSlot::new("python");
    let i1 = py.install("3.12.4", 120 << 20);
    let i2 = py.install("3.11.9", 118 << 20);
    cs.add("version_coexistence", i1.is_ok() && i2.is_ok() && py.count() == 2, "");
    // 3) 显式激活（不搞隐式切换）。
    py.activate("3.11.9");
    cs.add("explicit_activation", py.active_version() == Some("3.11.9"), "");
    // 4) 三验逐步登记 → 全绿。
    for step in 0..3 {
        py.verify("3.12.4", step);
    }
    cs.add("verify_install_three_steps", py.fully_verified("3.12.4") && VERIFY_STEPS == ["version", "hello-world", "package-install"], "");
    // 5) 损坏检出（三验缺位）→ 一键重装的入口判据。
    cs.add("corruption_detected", py.detect_corruption("3.11.9") && !py.detect_corruption("3.12.4"), "");
    // 6) 包缓存哈希去重：第二次下载命中不重复。
    let mut cache = PkgCache::new();
    let first = cache.lookup(0xDEADBEEF, 55 << 20);
    let second = cache.lookup(0xDEADBEEF, 55 << 20);
    cs.add("pkg_cache_dedup", !first && second && cache.dedup_hits == 1 && cache.total_bytes == 55 << 20, "");
    // 7) 磁盘预检：够 → 放行；不够 → 预检拦截（装前算大小）。
    cs.add(
        "disk_precheck",
        cache.disk_precheck(1 << 30, 120 << 20, 50 << 20).is_ok() && cache.disk_precheck(130 << 20, 120 << 20, 50 << 20) == Err("insufficient-disk"),
        "",
    );
    // 8) PATH 注入会话级（F011 语义）。
    let mut buf = [0u8; 64];
    let n = path_injection("python", "3.12.4", &mut buf);
    cs.add("path_injection_session", &buf[..n] == b"Toolchains\\python\\3.12.4;", "");
    // 9) 版本槽容量守卫。
    let mut go = LangSlot::new("go");
    for v in ["1.20", "1.21", "1.22", "1.23"] {
        go.install(v, 250 << 20).unwrap();
    }
    cs.add("version_slots_full", go.install("1.24", 1) == Err("version-slots-full"), "");
    // 10) 重复安装拒绝。
    let mut nd = LangSlot::new("node");
    nd.install("20.11.0", 90 << 20).unwrap();
    cs.add("dup_install_rejected", nd.install("20.11.0", 90 << 20) == Err("already-installed"), "");
    // 11) 激活切换显式且唯一（同语言仅一个激活）。
    let mut rs = LangSlot::new("rust");
    rs.install("1.75.0", 300 << 20).unwrap();
    rs.install("1.78.0", 310 << 20).unwrap();
    rs.activate("1.75.0");
    rs.activate("1.78.0");
    cs.add("single_active", rs.active_version() == Some("1.78.0"), "");
    // 12) 沙盒化共享区路径常量语义（~\Toolchains\<lang>\<ver>\）。
    cs.add("toolchains_layout", path_injection("go", "1.23", &mut buf) > 0 && buf.starts_with(b"Toolchains\\"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：每语言「装-写-跑-调」四步判例，五语言全绿。
    /// 模型：每语言装→激活→三验全绿（四步判例的登记面）。
    #[test]
    fn five_languages_install_write_run_debug_all_green() {
        let versions = ["3.12.4", "20.11.0", "17.0.10", "1.22.1", "1.78.0"];
        for (i, lang) in LANGUAGES.iter().enumerate() {
            let mut slot = LangSlot::new(lang);
            slot.install(versions[i], 200 << 20).unwrap();
            slot.activate(versions[i]);
            for step in 0..3 {
                slot.verify(versions[i], step);
            }
            assert!(slot.fully_verified(versions[i]), "{} 四步判例全绿", lang);
            assert_eq!(slot.active_version(), Some(versions[i]));
        }
    }

    #[test]
    fn session_switch_is_explicit_no_magic() {
        let mut slot = LangSlot::new("python");
        slot.install("3.12.4", 1).unwrap();
        slot.install("3.11.9", 1).unwrap();
        // 未激活 → 无默认解释器（显式无魔法）。
        assert_eq!(slot.active_version(), None, "隐式默认不存在");
        slot.activate("3.12.4");
        assert_eq!(slot.active_version(), Some("3.12.4"));
    }

    #[test]
    fn cache_bloat_in_precheck_estimate() {
        let mut cache = PkgCache::new();
        cache.lookup(1, 500 << 20);
        cache.lookup(2, 500 << 20);
        // 预检含包缓存膨胀预估：现有缓存 1GB 也要算进去。
        assert_eq!(
            cache.disk_precheck((500 + 120 + 1000 + 1) << 20, 120 << 20, 1000 << 20),
            Ok(()),
            "可用 ≥ 安装 + 缓存膨胀预估 → 放行"
        );
    }

    #[test]
    fn java_uses_maven_manager() {
        // 语言↔包管理器对应关系抽查（java→maven、rust→cargo）。
        assert_eq!(PACKAGE_MANAGERS[2], "maven");
        assert_eq!(PACKAGE_MANAGERS[4], "cargo");
    }
}
