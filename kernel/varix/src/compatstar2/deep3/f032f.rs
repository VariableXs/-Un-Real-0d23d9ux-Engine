//! F032 深化批次四 · 运行时环境账面（compatstar2/deep3 · G-A-32）。
//!
//! 批次一/二/三已覆盖 F032 的安装协议面、执行治理面与包版本解析面；本批
//! 补主册【功能定义】「全语义对齐」的序列化/账本/容错面：五语言版本槽
//! 矩阵（python/node/java/go/rust × 各 2 版本槽 = 10 格占用账）、活动
//! 解释器切换账（显式激活/取消激活/双激活拒绝三态）、venv 隔离模型
//! （路径前缀解析：venv 内可执行文件优先，越界引用拒绝）、包缓存字节
//! LRU（按字节配额逐出最旧，逐出账）。
//!
//! 判据对账：主册【设计细节】「版本并存激活机制：会话内显式切换或设置页
//! 点选默认（显式无魔法）」「包缓存全局共享（哈希去重）」未落地面为源，
//! 一处一事实（pyenv/nvm 显式激活语义、pip 缓存配额逐出语义对拍）。
//!
//! 零堆纪律：定长槽矩阵/切换账/影子表/缓存槽，无 alloc。

use crate::checks::CheckSet;

/// 语言数（主册【功能定义】五语言）。
pub const LANG_N: usize = 5;
/// 每语言版本槽数（并存口径：双槽）。
pub const SLOTS: usize = 2;
/// 五语言名表。
pub const LANGS: [&'static str; LANG_N] = ["python", "node", "java", "go", "rust"];
/// 五语言各 2 槽的判例版本种子（判例对拍用，非真实发布清单）。
pub const SEED_VERSIONS: [[&'static str; SLOTS]; LANG_N] = [
    ["3.12.4", "3.11.9"],   // python
    ["22.11.0", "20.18.0"], // node
    ["21.0.4", "17.0.11"],  // java
    ["1.23.2", "1.22.8"],   // go
    ["1.82.0", "1.80.1"],   // rust
];
/// venv 路径前缀（隔离解析口径）。
pub const VENV_PREFIX: &'static str = "venv/";
/// 系统工具链路径前缀（venv 激活时越界）。
pub const SYS_PREFIX: &'static str = "system/";
/// 裸命令 → venv 影子路径表（venv 内可执行文件优先）。
pub const SHADOWS: [(&'static str, &'static str); 4] = [
    ("python", "venv/python.exe"), ("node", "venv/node.exe"),
    ("pip", "venv/scripts/pip.exe"), ("npm", "venv/scripts/npm.exe"),
];
/// 活动解释器状态：无 / 激活。
pub const ST_NONE: u8 = 0;
pub const ST_ACTIVE: u8 = 1;
/// 包缓存槽容量。
pub const CACHE_SLOTS: usize = 8;

/// 版本槽矩阵：5 语言 × 2 槽；装/卸/重复版本拒绝全记账。
pub struct VersionMatrix {
    vers: [[&'static str; SLOTS]; LANG_N],
    used: [usize; LANG_N],
    /// 全矩阵已占槽数（0..=10）。
    pub total_used: usize,
}

impl VersionMatrix {
    pub const fn new() -> Self {
        VersionMatrix { vers: [[""; SLOTS]; LANG_N], used: [0; LANG_N], total_used: 0 }
    }
    /// 安装一版入空闲槽；槽满/重复版本/坏语言显性 Err。
    pub fn install(&mut self, lang: usize, ver: &'static str) -> Result<usize, &'static str> {
        if lang >= LANG_N { return Err("bad-lang"); }
        if self.used[lang] >= SLOTS { return Err("slot-full"); }
        for s in 0..SLOTS {
            if self.vers[lang][s] == ver { return Err("dup-version"); }
        }
        let slot = self.used[lang];
        self.vers[lang][slot] = ver;
        self.used[lang] += 1;
        self.total_used += 1;
        Ok(slot)
    }
    /// 卸载一槽（交换尾槽填补）；坏位如实拒绝。
    pub fn uninstall(&mut self, lang: usize, slot: usize) -> bool {
        if lang >= LANG_N || slot >= SLOTS || self.used[lang] == 0 { return false; }
        self.used[lang] -= 1;
        self.vers[lang][slot] = self.vers[lang][self.used[lang]];
        self.vers[lang][self.used[lang]] = "";
        self.total_used -= 1;
        true
    }
    /// 槽内容视图（坏位为空串）。
    pub fn slot_ver(&self, lang: usize, slot: usize) -> &'static str {
        if lang < LANG_N && slot < SLOTS { self.vers[lang][slot] } else { "" }
    }
    /// 某语言已用槽数。
    pub fn used(&self, lang: usize) -> usize {
        if lang < LANG_N { self.used[lang] } else { 0 }
    }
}

/// 活动解释器切换账：同一会话至多一个活动解释器（显式无魔法）。
pub struct ActiveSwitch {
    state: u8,
    active_lang: usize,
    active_slot: usize,
    /// 显式激活 / 取消激活 / 双激活拒绝 / 无激活取消拒绝 四账。
    pub activations: u32,
    pub deactivations: u32,
    pub double_active_rejects: u32,
    pub no_active_rejects: u32,
}

impl ActiveSwitch {
    pub const fn new() -> Self {
        ActiveSwitch { state: ST_NONE, active_lang: 0, active_slot: 0, activations: 0, deactivations: 0, double_active_rejects: 0, no_active_rejects: 0 }
    }
    /// 显式激活；已激活 → 拒绝并记账（Err("already-active")）。
    pub fn activate(&mut self, lang: usize, slot: usize) -> Result<(), &'static str> {
        if lang >= LANG_N || slot >= SLOTS { return Err("bad-slot"); }
        if self.state == ST_ACTIVE {
            self.double_active_rejects += 1;
            return Err("already-active");
        }
        self.state = ST_ACTIVE;
        self.active_lang = lang;
        self.active_slot = slot;
        self.activations += 1;
        Ok(())
    }
    /// 取消激活；无激活 → 拒绝并记账。
    pub fn deactivate(&mut self) -> bool {
        if self.state != ST_ACTIVE {
            self.no_active_rejects += 1;
            return false;
        }
        self.state = ST_NONE;
        self.deactivations += 1;
        true
    }
    /// 当前活动槽（无激活 → None）。
    pub fn active(&self) -> Option<(usize, usize)> {
        if self.state == ST_ACTIVE { Some((self.active_lang, self.active_slot)) } else { None }
    }
}

/// venv 隔离解析：激活时裸命令 → venv 影子路径优先；venv/ 前缀直通；
/// system/ 前缀 → 越界拒绝（隔离红线）；未激活时 venv/ 引用同样拒绝。
pub struct VenvIsolation {
    pub active: bool,
    /// venv 优先解析计数。
    pub venv_priority: u32,
    /// 越界引用拒绝计数。
    pub oob_rejects: u32,
}

impl VenvIsolation {
    pub const fn new() -> Self {
        VenvIsolation { active: false, venv_priority: 0, oob_rejects: 0 }
    }
    /// 隔离解析主入口（三态显性：直通 / 影子优先 / 拒绝）。
    pub fn resolve(&mut self, exe: &'static str) -> Result<&'static str, &'static str> {
        if !self.active {
            if exe.starts_with(VENV_PREFIX) { return Err("venv-not-active"); }
            return Ok(exe);
        }
        if exe.starts_with(VENV_PREFIX) {
            self.venv_priority += 1;
            return Ok(exe);
        }
        if exe.starts_with(SYS_PREFIX) {
            self.oob_rejects += 1;
            return Err("venv-oob");
        }
        for &(bare, shadow) in SHADOWS.iter() {
            if exe == bare {
                self.venv_priority += 1;
                return Ok(shadow);
            }
        }
        Err("unknown-command")
    }
}

/// 包缓存：字节配额内 LRU；逐出最旧并记账；超配额单项显性拒绝。
pub struct PkgCache {
    keys: [&'static str; CACHE_SLOTS],
    sizes: [u32; CACHE_SLOTS],
    stamp: [u64; CACHE_SLOTS],
    live: [bool; CACHE_SLOTS],
    tick: u64,
    /// 字节配额。
    pub quota_bytes: u32,
    /// 当前占用字节。
    pub used_bytes: u32,
    /// 逐出计数。
    pub evictions: u32,
    evict_log: [&'static str; CACHE_SLOTS],
    pub evict_n: usize,
}

impl PkgCache {
    pub const fn new(quota: u32) -> Self {
        PkgCache { keys: [""; CACHE_SLOTS], sizes: [0; CACHE_SLOTS], stamp: [0; CACHE_SLOTS], live: [false; CACHE_SLOTS], tick: 0, quota_bytes: quota, used_bytes: 0, evictions: 0, evict_log: [""; CACHE_SLOTS], evict_n: 0 }
    }
    /// 放入/更新：已存在 → 触摸新鲜化并调账；新键 → 先逐出最旧腾位。
    pub fn put(&mut self, key: &'static str, size: u32) -> Result<(), &'static str> {
        for i in 0..CACHE_SLOTS {
            if self.live[i] && self.keys[i] == key {
                self.used_bytes += size;
                self.used_bytes -= self.sizes[i];
                self.sizes[i] = size;
                self.stamp[i] = self.tick;
                self.tick += 1;
                return Ok(());
            }
        }
        if size > self.quota_bytes { return Err("quota-exceeded"); }
        while self.used_bytes + size > self.quota_bytes {
            let mut oldest = usize::MAX;
            for i in 0..CACHE_SLOTS {
                if self.live[i] && (oldest == usize::MAX || self.stamp[i] < self.stamp[oldest]) { oldest = i; }
            }
            if oldest == usize::MAX { return Err("quota-exceeded"); }
            self.live[oldest] = false;
            self.used_bytes -= self.sizes[oldest];
            self.evictions += 1;
            if self.evict_n < CACHE_SLOTS { self.evict_log[self.evict_n] = self.keys[oldest]; self.evict_n += 1; }
        }
        for i in 0..CACHE_SLOTS {
            if !self.live[i] {
                self.keys[i] = key;
                self.sizes[i] = size;
                self.stamp[i] = self.tick;
                self.tick += 1;
                self.live[i] = true;
                self.used_bytes += size;
                return Ok(());
            }
        }
        Err("cache-full")
    }
    /// 取用：命中 → 触摸新鲜化并返回字节数；未命中显性 Err。
    pub fn get(&mut self, key: &'static str) -> Result<u32, &'static str> {
        for i in 0..CACHE_SLOTS {
            if self.live[i] && self.keys[i] == key {
                self.stamp[i] = self.tick;
                self.tick += 1;
                return Ok(self.sizes[i]);
            }
        }
        Err("cache-miss")
    }
    /// 逐出账视图。
    pub fn evict_log(&self) -> &[&'static str] { &self.evict_log[..self.evict_n] }
}

/// 域自检（深化批次四）。
pub fn run_f032f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F032-runtime-env-d4");
    // 1) 五语言 × 2 槽 = 10 格占用账。
    let mut m = VersionMatrix::new();
    let mut fill_ok = true;
    for lang in 0..LANG_N {
        fill_ok &= m.install(lang, SEED_VERSIONS[lang][0]).is_ok();
        fill_ok &= m.install(lang, SEED_VERSIONS[lang][1]).is_ok();
    }
    cs.add("matrix_fill_10_slots", fill_ok && m.total_used == 10, "");
    // 2) 槽满显性拒绝。
    cs.add("slot_full_rejected", m.install(0, "3.13.0") == Err("slot-full"), "");
    // 3) 卸载腾槽后重装成功、槽内容如实（交换尾槽填补）、同语言重复版本拒绝。
    let uninstalled = m.uninstall(0, 0);
    let reinstall = m.install(0, "3.13.0");
    cs.add("uninstall_frees_slot", uninstalled && reinstall == Ok(1) && m.slot_ver(0, 0) == "3.11.9" && m.slot_ver(0, 1) == "3.13.0", "");
    let refree = m.uninstall(0, 1);
    cs.add("dup_version_rejected", refree && m.install(0, "3.11.9") == Err("dup-version"), "");
    // 4) 显式激活/取消激活三态账。
    let mut a = ActiveSwitch::new();
    let act = a.activate(2, 1);
    let snapshot = a.active() == Some((2, 1));
    let deact = a.deactivate();
    cs.add("activate_deactivate_cycle", act.is_ok() && snapshot && deact && a.active().is_none() && a.activations == 1 && a.deactivations == 1, "");
    // 5) 双激活拒绝（显式无魔法），原激活保持。
    let _ = a.activate(0, 0);
    cs.add("double_activate_rejected", a.activate(1, 0) == Err("already-active") && a.double_active_rejects == 1 && a.active() == Some((0, 0)), "");
    // 6) 无激活时取消 → 拒绝并记账。
    let mut a2 = ActiveSwitch::new();
    cs.add("deactivate_without_active_rejected", !a2.deactivate() && a2.no_active_rejects == 1, "");
    // 7) venv 激活：裸命令影子优先 + venv/ 前缀直通。
    let mut v = VenvIsolation::new();
    v.active = true;
    let shadow = v.resolve("python");
    let direct = v.resolve("venv/custom.exe");
    cs.add("venv_shadow_priority", shadow == Ok("venv/python.exe") && direct == Ok("venv/custom.exe") && v.venv_priority == 2, "");
    // 8) venv 激活时 system/ 越界引用拒绝。
    cs.add("venv_oob_rejected", v.resolve("system/python.exe") == Err("venv-oob") && v.oob_rejects == 1, "");
    // 9) 未激活：system 直通、venv/ 引用拒绝。
    let mut v2 = VenvIsolation::new();
    cs.add("venv_inactive_passthrough_guard", v2.resolve("system/python.exe") == Ok("system/python.exe") && v2.resolve("venv/x.exe") == Err("venv-not-active"), "");
    // 10) 包缓存字节 LRU：get 触摸改逐出对象（b 先于 a 逐出）。
    let mut c = PkgCache::new(100);
    let _ = c.put("a", 50);
    let _ = c.put("b", 30);
    let _ = c.get("a");
    let _ = c.put("c", 30);
    let _ = c.put("d", 30);
    cs.add("cache_lru_eviction_ledger", c.evict_log() == ["b", "a"] && c.evictions == 2 && c.used_bytes == 60 && c.get("c") == Ok(30), "");
    // 11) 超配额单项显性拒绝。
    cs.add("cache_quota_oversize_rejected", c.put("e", 150) == Err("quota-exceeded"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_full_rejects_then_recovers() {
        let mut m = VersionMatrix::new();
        for lang in 0..LANG_N {
            assert!(m.install(lang, SEED_VERSIONS[lang][0]).is_ok());
            assert!(m.install(lang, SEED_VERSIONS[lang][1]).is_ok());
        }
        assert_eq!(m.install(4, "1.83.0"), Err("slot-full"), "10 格占满如实拒绝");
        assert!(m.uninstall(4, 1));
        assert!(m.install(4, "1.83.0").is_ok(), "卸载腾槽后重装成功");
    }

    #[test]
    fn lru_touch_changes_victim() {
        let mut c = PkgCache::new(100);
        let _ = c.put("x", 60);
        let _ = c.put("y", 30);
        let _ = c.get("x"); // 触摸 x → 最旧变为 y
        let _ = c.put("z", 30);
        assert_eq!(c.evict_log(), ["y"], "LRU 逐出最旧而非 FIFO 首项");
        assert_eq!(c.used_bytes, 90);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f032f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
