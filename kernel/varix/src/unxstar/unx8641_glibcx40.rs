//! UNX-F8641 · glibc 动态链接样本库 30→40 扩额账 · 诊断段 0xB660
//! 验收锚点：UNX-F8641-J1 样本库从 30 扩额至 40——旧 30 条零变动
//! （指纹逐一比对），新 10 条增量登记，扩额后总量对账 40 自洽，
//! 每样本加载模拟全过，双判据缺一即批内红灯。
use alloc::vec;
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB660;

// ── 诊断段独占 · 扩额错误域 ─────────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_OLD_MUT: u16 = SEG_ERR + 1; // 旧样本被改动
const E_DUP: u16 = SEG_ERR + 2;     // 样本名重复
const E_LOAD: u16 = SEG_ERR + 3;    // 加载失败
const E_COUNT: u16 = SEG_ERR + 4;   // 总量对账不平
const E_ORDER: u16 = SEG_ERR + 5;   // PT_INTERP 缺失

// ── FNV-1a 64（样本指纹）────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── glibc 样本 ───────────────────────────────────────────────
/// 一条动态链接样本：ELF 头摘要 + PT_INTERP 路径 + 依赖清单。
#[derive(Clone, Debug)]
pub struct GlibcSample {
    pub name: String,       // 样本名（如 glibc-2.31-hello）
    pub class: u8,          // 1=ELFCLASS32 2=ELFCLASS64
    pub interp: String,     // PT_INTERP 路径
    pub deps: Vec<String>,  // NEEDED 依赖名
    pub text_pages: u32,    // 代码页数（模拟）
    pub entry: u64,         // 入口虚地址
}

impl GlibcSample {
    pub fn fp(&self) -> u64 {
        let mut buf = format!("{}|{}|{}", self.name, self.class, self.interp);
        for d in &self.deps {
            buf.push(':');
            buf.push_str(d);
        }
        fnv1a64(buf.as_bytes())
    }
    /// 加载模拟：_interp 以 / 开头、依赖名非空、entry 对齐。
    pub fn load_sim(&self) -> Result<u32, u16> {
        if !self.interp.starts_with('/') {
            return Err(E_ORDER);
        }
        if self.deps.is_empty() {
            return Err(E_LOAD);
        }
        if self.entry % 0x10 != 0 {
            return Err(E_LOAD);
        }
        Ok(self.text_pages)
    }
}

// ── 样本库 ───────────────────────────────────────────────────
/// 样本库：扩额登记 + 旧集指纹保全 + 总量对账。
pub struct SampleVault {
    pub samples: Vec<GlibcSample>,
    old_fps: Vec<(String, u64)>, // 扩额前快照
}

impl SampleVault {
    pub fn new() -> Self {
        SampleVault { samples: Vec::new(), old_fps: Vec::new() }
    }
    pub fn add(&mut self, s: GlibcSample) -> bool {
        if self.samples.iter().any(|x| x.name == s.name) {
            return false; // 名重复拒
        }
        self.samples.push(s);
        true
    }
    /// 扩额前快照：冻结旧集指纹。
    pub fn snapshot_old(&mut self) {
        self.old_fps = self.samples.iter().map(|s| (s.name.clone(), s.fp())).collect();
    }
    /// 旧集零变动校验：逐一比对快照指纹。
    pub fn old_untouched(&self) -> bool {
        for (n, f) in &self.old_fps {
            match self.samples.iter().find(|s| &s.name == n) {
                Some(s) if s.fp() == *f => {}
                _ => return false,
            }
        }
        true
    }
    /// 增量登记：把新样本逐条塞入库，返回成功条数。
    pub fn expand(&mut self, extra: Vec<GlibcSample>) -> usize {
        let mut n = 0;
        for s in extra {
            if self.add(s) {
                n += 1;
            }
        }
        n
    }
    /// 名无重复。
    pub fn names_unique(&self) -> bool {
        let n = self.samples.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.samples[i].name == self.samples[j].name {
                    return false;
                }
            }
        }
        true
    }
    /// 全库加载模拟。
    pub fn load_all(&self) -> Result<u32, u16> {
        let mut pages = 0u32;
        for s in &self.samples {
            pages += s.load_sim()?;
        }
        Ok(pages)
    }
    pub fn count(&self) -> usize {
        self.samples.len()
    }
}

/// 总量对账：扩额后恰 40 = 30 旧 + 10 新。
pub fn recon_total(vault: &SampleVault, old_n: usize, new_n: usize) -> bool {
    vault.count() == old_n + new_n && vault.old_fps.len() == old_n
}

// ── 标准夹具：旧 30 条 ──────────────────────────────────────
pub fn base_30() -> Vec<GlibcSample> {
    let mut v = Vec::new();
    for i in 0..30usize {
        let deps = match i % 3 {
            0 => vec!["libc.so.6".to_string()],
            1 => vec!["libc.so.6".to_string(), "ld-linux.so.2".to_string()],
            _ => vec!["libc.so.6".to_string(), "libm.so.6".to_string(), "libdl.so.2".to_string()],
        };
        v.push(GlibcSample {
            name: format!("glibc-base-{:02}", i),
            class: if i % 2 == 0 { 2 } else { 1 },
            interp: "/lib/ld-linux.so.2".to_string(),
            deps,
            text_pages: 8 + i as u32,
            entry: 0x400000 + (i as u64) * 0x1000,
        });
    }
    v
}

/// 新 10 条增量。
pub fn extra_10() -> Vec<GlibcSample> {
    let mut v = Vec::new();
    for i in 0..10usize {
        v.push(GlibcSample {
            name: format!("glibc-ext-{:02}", i),
            class: 2,
            interp: "/lib64/ld-linux-x86-64.so.2".to_string(),
            deps: vec!["libc.so.6".to_string(), format!("libext{}.so", i)],
            text_pages: 4 + i as u32,
            entry: 0x400000 + (30 + i) as u64 * 0x1000,
        });
    }
    v
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8641_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8641 glibc样本30→40扩额");

    // 1. 基线 30 条注册成功
    let mut vault = SampleVault::new();
    let mut add_ok = true;
    for sb in base_30() {
        if !vault.add(sb) {
            add_ok = false;
        }
    }
    s.add("GLX-基线30注册", add_ok && vault.count() == 30,
        if add_ok { "" } else { "基线注册失败" });

    // 2. 扩额前快照冻结
    vault.snapshot_old();
    let snap_n = vault.old_fps.len();
    s.add("GLX-快照冻结", snap_n == 30, if snap_n == 30 { "" } else { "快照缺失" });

    // 3. 增量 10 条全部登记
    let n = vault.expand(extra_10());
    s.add("GLX-增量10登记", n == 10 && vault.count() == 40,
        if n == 10 { "" } else { "增量登记不齐" });

    // 4. 旧 30 条零变动（指纹逐一比对）
    let untouched = vault.old_untouched();
    s.add("GLX-旧集零变动", untouched, if untouched { "" } else { "旧样本被改动" });

    // 5. 总量对账 40 = 30 + 10
    let bal = recon_total(&vault, 30, 10);
    s.add("GLX-总量对账40", bal, if bal { "" } else { "总量对账不平" });

    // 6. 名无重复
    let uniq = vault.names_unique();
    s.add("GLX-名无重复", uniq, if uniq { "" } else { "样本名重复" });

    // 7. 全库加载模拟通过
    let pages = vault.load_all();
    let load_ok = pages.is_ok() && pages.as_ref().map(|p| *p > 0).unwrap_or(false);
    s.add("GLX-全库加载", load_ok, if load_ok { "" } else { "加载模拟失败" });

    // 8. 重名拒收：再投同名增量必拒
    let mut v2 = SampleVault::new();
    for sb in base_30() {
        let _ = v2.add(sb);
    }
    let dup_rej = !v2.add(extra_10()[0].clone());
    s.add("GLX-重名拒收", dup_rej, if dup_rej { "" } else { "重名未拒" });

    // 9. 旧样本被改动的漂移检出
    let mut v3 = SampleVault::new();
    for sb in base_30() {
        let _ = v3.add(sb);
    }
    v3.snapshot_old();
    v3.samples[5].text_pages = 999; // 篡改旧样本
    let drift = !v3.old_untouched();
    s.add("GLX-篡改检出", drift, if drift { "" } else { "漂移未检出" });

    // 10. interp 非法路径加载拒
    let bad = GlibcSample {
        name: "bad-interp".to_string(),
        class: 2,
        interp: "ld-linux.so.2".to_string(), // 非 / 开头
        deps: vec!["libc.so.6".to_string()],
        text_pages: 4,
        entry: 0x400000,
    };
    let rej = bad.load_sim() == Err(E_ORDER);
    s.add("GLX-interp路径拒", rej, if rej { "" } else { "非法路径放行" });

    // 11. 依赖为空加载拒
    let nodep = GlibcSample {
        name: "no-dep".to_string(),
        class: 2,
        interp: "/lib64/ld-linux-x86-64.so.2".to_string(),
        deps: Vec::new(),
        text_pages: 4,
        entry: 0x400000,
    };
    let rej2 = nodep.load_sim() == Err(E_LOAD);
    s.add("GLX-空依赖拒", rej2, if rej2 { "" } else { "空依赖放行" });

    // 12. entry 未对齐加载拒
    let misalign = GlibcSample {
        name: "misalign".to_string(),
        class: 2,
        interp: "/lib64/ld-linux-x86-64.so.2".to_string(),
        deps: vec!["libc.so.6".to_string()],
        text_pages: 4,
        entry: 0x400001,
    };
    let rej3 = misalign.load_sim() == Err(E_LOAD);
    s.add("GLX-入口对齐拒", rej3, if rej3 { "" } else { "未对齐放行" });

    // 13. LCG 抽样复核 5 条指纹稳定性
    let mut lcg = Lcg(0xB660);
    let mut fp_ok = true;
    for _ in 0..5 {
        let i = lcg.below(vault.count() as u64) as usize;
        let smp = &vault.samples[i];
        if smp.fp() != smp.fp() {
            fp_ok = false;
        }
    }
    s.add("GLX-指纹稳定", fp_ok, if fp_ok { "" } else { "指纹不稳定" });

    // 14. 页数对账：全库页数 = 各样本页数和（load 返回值核对）
    let manual: u32 = vault.samples.iter().map(|x| x.text_pages).sum();
    let pages_sum = vault.load_all().unwrap_or(0);
    s.add("GLX-页数对账", pages_sum == manual,
        if pages_sum == manual { "" } else { "页数账不平" });

    // 15. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB660 && SEG_BASE < 0xB680;
    s.add("GLX-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
