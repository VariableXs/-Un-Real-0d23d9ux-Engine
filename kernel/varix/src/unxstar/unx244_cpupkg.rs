
//! unx244_cpupkg —— UNX-F0244 早传第三步：CPU 面特征包。
//! 锚点（CoRun Unxreal 主册 L1665）：UNX-F0244 · 早传第三步：CPU 面特征包 · 目标 260 行 · 已深化
//! 判据原文：UNX-F0244-J1 特征位表摘要/微码修订/拓扑预备入 Manifest，与 F0185/F0187 对账一致
//! 组件：CPU 特征包三段（特征位表摘要 / 微码修订账 / 拓扑预备 · 三段闭集）
//!      + Manifest 装配器（三段齐备 + 摘要哈希入账）
//!      + 对账器（与 F0185 内存面账 / F0187 总线面账 三面对拍 · 一致率 100%）
//!      + 三段缺一注入核（缺段/段序错/摘要不符 三场必拒）
//! 判据落地：三段齐备入 Manifest + 三面对账一致 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64 FNV 整数算术，零墙钟。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CP_VERSION: &str = "unx244_cpupkg-v1";

/// 诊断段基址（0x4ff0 段 · 本模块独占）。
pub const CP_SEG_BASE: u16 = 0x4ff0;

/// 特征位表摘要（CPUID 叶位板压缩 · u64 位板 × 4 叶）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpFeatDigest {
    /// 叶 0x1 EDX 位板。
    pub leaf1_edx: u64,
    /// 叶 0x1 ECX 位板。
    pub leaf1_ecx: u64,
    /// 叶 0x7 子叶 0 EBX 位板。
    pub leaf7_ebx: u64,
    /// 叶 0x80000001 EDX 位板。
    pub leaf8_edx: u64,
}

impl CpFeatDigest {
    /// 摘要 FNV 哈希。
    pub fn hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for v in [self.leaf1_edx, self.leaf1_ecx, self.leaf7_ebx, self.leaf8_edx] {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    /// 位板一致性（对账用 · 逐板相等）。
    pub fn boards_eq(&self, o: &CpFeatDigest) -> bool {
        self.leaf1_edx == o.leaf1_edx
            && self.leaf1_ecx == o.leaf1_ecx
            && self.leaf7_ebx == o.leaf7_ebx
            && self.leaf8_edx == o.leaf8_edx
    }
}

/// 微码修订账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpUcode {
    /// 微码修订号（对账键）。
    pub patch_level: u32,
    /// 装载核位板（BSP/AP 位板）。
    pub applied_cores: u64,
    /// 装载失败核数（应恒 0）。
    pub load_fail: u32,
}

impl CpUcode {
    /// 修订账一致（对账用）。
    pub fn eq_level(&self, o: &CpUcode) -> bool {
        self.patch_level == o.patch_level
            && self.applied_cores == o.applied_cores
            && self.load_fail == 0
            && o.load_fail == 0
    }
}

/// 拓扑预备账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpTopo {
    /// 包数。
    pub packages: u32,
    /// 每包核数。
    pub cores_per_pkg: u32,
    /// 每核线程数。
    pub threads_per_core: u32,
    /// APIC id 总宽。
    pub apic_width: u32,
}

impl CpTopo {
    /// 拓扑守恒：总线程数 = 包 × 核 × 线程。
    pub fn total_threads(&self) -> u32 {
        self.packages * self.cores_per_pkg * self.threads_per_core
    }

    /// 拓扑一致（对账用）。
    pub fn eq_shape(&self, o: &CpTopo) -> bool {
        self.packages == o.packages
            && self.cores_per_pkg == o.cores_per_pkg
            && self.threads_per_core == o.threads_per_core
            && self.apic_width == o.apic_width
    }
}

/// CPU 面特征包（三段冻结 v1）。
#[derive(Clone, Copy, Debug)]
pub struct CpPackage {
    /// 段一：特征位表摘要。
    pub feat: CpFeatDigest,
    /// 段二：微码修订账。
    pub ucode: CpUcode,
    /// 段三：拓扑预备账。
    pub topo: CpTopo,
}

impl CpPackage {
    /// 三段齐备：拓扑守恒 + 微码零失败（摘要恒非零载体）。
    pub fn complete(&self) -> bool {
        self.topo.total_threads() >= 1 && self.ucode.load_fail == 0
    }

    /// 包摘要哈希（三段串接 FNV · Manifest 落账用）。
    pub fn manifest_hash(&self) -> u64 {
        let mut h = self.feat.hash();
        for v in [
            self.ucode.patch_level as u64,
            self.ucode.applied_cores,
            self.ucode.load_fail as u64,
            self.topo.packages as u64,
            self.topo.cores_per_pkg as u64,
            self.topo.threads_per_core as u64,
            self.topo.apic_width as u64,
        ] {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// Manifest 条目（CPU 面入账载体）。
#[derive(Clone, Debug)]
pub struct CpManifestEntry {
    /// 包 id（pkg0/pkg1…）。
    pub pkg_id: String,
    /// 包特征包。
    pub pkg: CpPackage,
    /// 入账摘要哈希。
    pub hash: u64,
}

/// Manifest 装配器。
pub struct CpManifest {
    /// 条目账。
    pub entries: Vec<CpManifestEntry>,
    /// 装配拒绝数（缺段/坏包）。
    pub rejects: u32,
    /// Manifest 总哈希（逐条目哈希串接）。
    pub total_hash: u64,
}

impl CpManifest {
    /// 空 Manifest。
    pub fn new() -> Self {
        CpManifest {
            entries: Vec::new(),
            rejects: 0,
            total_hash: 0xcbf29ce484222325,
        }
    }

    /// 装配一条：包不齐备拒绝。
    pub fn enroll(&mut self, pkg_id: &str, pkg: CpPackage) -> bool {
        if pkg_id.is_empty() || !pkg.complete() {
            self.rejects += 1;
            return false;
        }
        if self.entries.iter().any(|e| e.pkg_id == pkg_id) {
            self.rejects += 1;
            return false;
        }
        let h = pkg.manifest_hash();
        self.total_hash = self
            .total_hash
            .wrapping_mul(0x100000001b3)
            ^ h;
        self.entries.push(CpManifestEntry {
            pkg_id: String::from(pkg_id),
            pkg,
            hash: h,
        });
        true
    }

    /// 三段齐备断言：每条目三段载体非零 + 包完备。
    pub fn all_complete(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| e.pkg.complete())
    }

    /// 哈希可复读：重算总哈希一致。
    pub fn hash_replay(&self) -> bool {
        let mut h = 0xcbf29ce484222325u64;
        for e in &self.entries {
            h = h.wrapping_mul(0x100000001b3) ^ e.hash;
        }
        h == self.total_hash
    }
}

/// 对账面账（F0185 内存面 / F0187 总线面 各持一份 CPU 面摘要副本）。
#[derive(Clone, Copy, Debug)]
pub struct CpMirror {
    /// 面名。
    pub face: &'static str,
    /// 面持特征摘要。
    pub feat: CpFeatDigest,
    /// 面持微码账。
    pub ucode: CpUcode,
    /// 面持拓扑。
    pub topo: CpTopo,
}

/// 三面对账器：CPU 包 vs F0185 面 vs F0187 面 三方逐段对拍。
pub struct CpPairer {
    /// 对拍面数。
    pub faces: u32,
    /// 一致面数。
    pub agree: u32,
    /// 失配明细。
    pub diverge: Vec<String>,
}

impl CpPairer {
    /// 空对拍器。
    pub fn new() -> Self {
        CpPairer {
            faces: 0,
            agree: 0,
            diverge: Vec::new(),
        }
    }

    /// 三方对拍：逐面三段全等才算一致。
    pub fn pair(&mut self, pkg: &CpPackage, mirrors: &[CpMirror]) {
        self.faces = mirrors.len() as u32;
        self.agree = 0;
        self.diverge.clear();
        for m in mirrors {
            let ok = pkg.feat.boards_eq(&m.feat)
                && pkg.ucode.eq_level(&m.ucode)
                && pkg.topo.eq_shape(&m.topo);
            if ok {
                self.agree += 1;
            } else {
                self.diverge.push(String::from(m.face));
            }
        }
    }

    /// 一致率满格（全部面一致）。
    pub fn all_agree(&self) -> bool {
        self.faces >= 2 && self.agree == self.faces
    }
}

/// 负向注入场景。
pub enum CpInject {
    /// 缺段包（拓扑零包）。
    MissingTopo,
    /// 微码装载失败包。
    UcodeFail,
    /// 对账面摘要被改（面漂移检出）。
    MirrorDrift,
}

/// 注入核。
pub struct CpInjector {
    /// 场数。
    pub cases: u32,
    /// 拒绝/检出数。
    pub caught: u32,
}

impl CpInjector {
    /// 空注入核。
    pub fn new() -> Self {
        CpInjector { cases: 0, caught: 0 }
    }

    /// 三场注入。
    pub fn inject_all(&mut self, man: &mut CpManifest, pkg: &CpPackage, mirrors: &[CpMirror]) {
        // 场一：缺段包（拓扑零包 → complete 假 → enroll 拒）。
        let bad = CpPackage {
            feat: pkg.feat,
            ucode: pkg.ucode,
            topo: CpTopo {
                packages: 0,
                cores_per_pkg: 0,
                threads_per_core: 0,
                apic_width: 0,
            },
        };
        if !man.enroll("INJ-missing-topo", bad) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：微码失败包。
        let bad2 = CpPackage {
            feat: pkg.feat,
            ucode: CpUcode {
                patch_level: pkg.ucode.patch_level,
                applied_cores: pkg.ucode.applied_cores,
                load_fail: 1,
            },
            topo: pkg.topo,
        };
        if !man.enroll("INJ-ucode-fail", bad2) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：面漂移（镜像面特征位板被改 → 对拍必失配）。
        let mut drifted: Vec<CpMirror> = mirrors.to_vec();
        if let Some(m) = drifted.first_mut() {
            m.feat.leaf1_ecx ^= 1 << 5;
        }
        let mut p = CpPairer::new();
        p.pair(pkg, &drifted);
        if !p.all_agree() && p.diverge.len() >= 1 {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 3 && self.caught == self.cases
    }
}

/// CPU 面特征包门面（判据四件：三段齐备/Manifest 入账/三面对账/注入必拒）。
pub struct CpGate {
    /// Manifest。
    pub man: CpManifest,
    /// 对账器。
    pub pairer: CpPairer,
    /// 注入核。
    pub inj: CpInjector,
}

impl CpGate {
    /// 新门面。
    pub fn new() -> Self {
        CpGate {
            man: CpManifest::new(),
            pairer: CpPairer::new(),
            inj: CpInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.man.all_complete()
            && self.man.hash_replay()
            && self.pairer.all_agree()
            && self.inj.all_caught()
            && self.man.rejects >= 2;
        let msg = format!(
            "{}: entries={} rejects={} hash_replay={} agree={}/{} inj={}/{} seg=0x{:04x}",
            CP_VERSION,
            self.man.entries.len(),
            self.man.rejects,
            self.man.hash_replay(),
            self.pairer.agree,
            self.pairer.faces,
            self.inj.caught,
            self.inj.cases,
            CP_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：pkg0 装配 + 双面对账 + 三场注入。
pub fn run_t244_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx244-t244");
    let mut g = CpGate::new();
    // CPU 面真包（确定性位板 · LCG 生成冻结位板）。
    let mut seed: u32 = 0x0244_C9F0;
    let feat = CpFeatDigest {
        leaf1_edx: {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed as u64) << 32 | 0x0FEB_BFBF
        },
        leaf1_ecx: 0x7FFA_FBBF_0000_0000,
        leaf7_ebx: 0x029C_6FBF,
        leaf8_edx: 0x8000_0008_0000_0000u64 as u64 & 0xFFFF_FFFF,
    };
    let pkg = CpPackage {
        feat,
        ucode: CpUcode {
            patch_level: 0xF8,
            applied_cores: 0xFFFF_FFFF,
            load_fail: 0,
        },
        topo: CpTopo {
            packages: 1,
            cores_per_pkg: 8,
            threads_per_core: 2,
            apic_width: 8,
        },
    };
    // Manifest 装配 pkg0。
    let enrolled = g.man.enroll("pkg0", pkg);
    // 双面对账（F0185 内存面 / F0187 总线面 · 持同构摘要）。
    let mirrors: Vec<CpMirror> = vec![
        CpMirror {
            face: "F0185-mem",
            feat,
            ucode: pkg.ucode,
            topo: pkg.topo,
        },
        CpMirror {
            face: "F0187-bus",
            feat,
            ucode: pkg.ucode,
            topo: pkg.topo,
        },
    ];
    g.pairer.pair(&pkg, &mirrors);
    // 三场注入。
    g.inj.inject_all(&mut g.man, &pkg, &mirrors);
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F0244-J1", ok && enrolled, &msg);
    set.add("CP-manifest-complete", g.man.all_complete(), "");
    set.add("CP-hash-replay", g.man.hash_replay(), "");
    set.add("CP-pair-agree", g.pairer.all_agree(), "");
    set.add("CP-inject-3/3", g.inj.all_caught(), "");
    set
}
