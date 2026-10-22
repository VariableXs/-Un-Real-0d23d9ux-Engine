//! unx1602_cputopo —— UNX-F1602 CPU 拓扑表消费与每核坐标账。
//! 锚点（CoRun Unxreal 主册 L3538）：UNX-F1602 · CPU 拓扑表消费与每核坐标账 · 目标 300 行 · 已深化
//! 判据原文：UNX-F1602-J1 消费 A1 拓扑账构建（包/核/线程）四元组坐标表，坐标数与
//!           MADT enabled 核数一致，SMT 兄弟线程对映射账零遗漏可导出
//! 组件：四元组坐标（pkg/core/thread/apic_id · CtCoord 冻结）
//!      + 拓扑表消费器（A1 拓扑账 → 坐标表构建 · 逐核登记）
//!      + MADT 对账（坐标数 == MADT enabled 核数 · 一致断言）
//!      + SMT 兄弟对映射账（同核兄弟线程对 · 零遗漏 + 可导出）
//!      + 重复/越界注入核（重复 APIC id / 线程越界 · 必拒）
//! 判据落地：四元组齐备 + MADT 一致 + SMT 对零遗漏 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CT_VERSION: &str = "unx1602_cputopo-v1";

/// 诊断段基址（0x51a0 段 · 本模块独占）。
pub const CT_SEG_BASE: u16 = 0x51a0;

/// 四元组坐标（冻结 v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtCoord {
    /// 包号。
    pub pkg: u32,
    /// 核号（包内）。
    pub core: u32,
    /// 线程号（核内 · SMT 0/1）。
    pub thread: u32,
    /// APIC id（全局唯一键）。
    pub apic: u32,
}

impl CtCoord {
    /// 坐标合法（SMT 线程号 < 2 · 各号非越界）。
    pub fn valid(&self, max_pkg: u32, max_core: u32) -> bool {
        self.thread < 2 && self.pkg < max_pkg && self.core < max_core
    }
}

/// A1 拓扑账（消费源 · 声明面）。
#[derive(Clone, Copy, Debug)]
pub struct CtSource {
    /// 包数。
    pub packages: u32,
    /// 每包核数。
    pub cores_per_pkg: u32,
    /// 每核线程数（SMT 1/2）。
    pub threads_per_core: u32,
    /// MADT enabled 核数。
    pub madt_enabled: u32,
    /// APIC id 基址（线性分配起点）。
    pub apic_base: u32,
}

impl CtSource {
    /// 拓扑守恒：总核数 = 包 × 核/包。
    pub fn total_cores(&self) -> u32 {
        self.packages * self.cores_per_pkg
    }

    /// 总线程数。
    pub fn total_threads(&self) -> u32 {
        self.total_cores() * self.threads_per_core
    }
}

/// 拓扑消费器：源账 → 坐标表。
pub struct CtBuilder {
    /// 坐标表（每线程一条）。
    pub coords: Vec<CtCoord>,
    /// 构建拒绝数。
    pub rejects: u32,
    /// 源账。
    pub src: Option<CtSource>,
}

impl CtBuilder {
    /// 空构建器。
    pub fn new() -> Self {
        CtBuilder {
            coords: Vec::new(),
            rejects: 0,
            src: None,
        }
    }

    /// 消费源账：全拓扑展开为四元组坐标表（含 SMT 兄弟对）。
    pub fn consume(&mut self, src: CtSource) -> bool {
        if src.packages == 0 || src.cores_per_pkg == 0 || src.threads_per_core == 0 {
            self.rejects += 1;
            return false;
        }
        if src.threads_per_core > 2 {
            self.rejects += 1;
            return false;
        }
        self.coords.clear();
        self.src = Some(src);
        let mut apic = src.apic_base;
        for p in 0..src.packages {
            for c in 0..src.cores_per_pkg {
                for t in 0..src.threads_per_core {
                    // APIC id 线性分配：同核兄弟连续 id。
                    let coord = CtCoord {
                        pkg: p,
                        core: c,
                        thread: t,
                        apic,
                    };
                    // 重复 APIC id 拒绝。
                    if self.coords.iter().any(|x| x.apic == apic) {
                        self.rejects += 1;
                        return false;
                    }
                    self.coords.push(coord);
                    apic += 1;
                }
            }
        }
        true
    }

    /// MADT 对账：坐标线程数 == MADT enabled。
    pub fn madt_match(&self) -> bool {
        match self.src {
            None => false,
            Some(s) => self.coords.len() as u32 == s.madt_enabled,
        }
    }

    /// 四元组完备：每条坐标全字段有效。
    pub fn coords_valid(&self) -> bool {
        match self.src {
            None => false,
            Some(s) => !self.coords.is_empty()
                && self
                    .coords
                    .iter()
                    .all(|c| c.valid(s.packages, s.cores_per_pkg)),
        }
    }

    /// APIC id 唯一（零重复）。
    pub fn apic_unique(&self) -> bool {
        for i in 0..self.coords.len() {
            for j in i + 1..self.coords.len() {
                if self.coords[i].apic == self.coords[j].apic {
                    return false;
                }
            }
        }
        true
    }
}

/// SMT 兄弟线程对映射账。
#[derive(Clone, Copy, Debug)]
pub struct CtSibling {
    /// 核坐标（pkg, core）。
    pub pkg: u32,
    pub core: u32,
    /// 兄弟线程 APIC id 对（thread0, thread1）。
    pub apic_t0: u32,
    pub apic_t1: u32,
}

/// SMT 对账构建器。
pub struct CtSmtMap {
    /// 兄弟对账。
    pub pairs: Vec<CtSibling>,
    /// 漏对数（应恒 0）。
    pub missed: u32,
}

impl CtSmtMap {
    /// 空对账。
    pub fn new() -> Self {
        CtSmtMap {
            pairs: Vec::new(),
            missed: 0,
        }
    }

    /// 从坐标表构建 SMT 对（仅 SMT=2 源有效）。
    pub fn build(&mut self, b: &CtBuilder) -> bool {
        self.pairs.clear();
        self.missed = 0;
        let src = match b.src {
            Some(s) => s,
            None => return false,
        };
        if src.threads_per_core != 2 {
            // 非 SMT 拓扑：对账为空是正确语义。
            return b.coords.is_empty();
        }
        // 期望对数 = 总核数。
        let expect = src.total_cores();
        for p in 0..src.packages {
            for c in 0..src.cores_per_pkg {
                let t0 = b.coords.iter().find(|x| x.pkg == p && x.core == c && x.thread == 0);
                let t1 = b.coords.iter().find(|x| x.pkg == p && x.core == c && x.thread == 1);
                match (t0, t1) {
                    (Some(a), Some(s)) => self.pairs.push(CtSibling {
                        pkg: p,
                        core: c,
                        apic_t0: a.apic,
                        apic_t1: s.apic,
                    }),
                    _ => self.missed += 1,
                }
            }
        }
        self.pairs.len() as u32 == expect && self.missed == 0
    }

    /// 对账可导出：单行账非空。
    pub fn export_line(&self) -> String {
        let mut line = String::new();
        for (i, pr) in self.pairs.iter().enumerate() {
            if i >= 8 {
                break;
            }
            if !line.is_empty() {
                line.push(',');
            }
            line.push_str(&format!(
                "p{}c{}:({},{})",
                pr.pkg, pr.core, pr.apic_t0, pr.apic_t1
            ));
        }
        format!("smt[{}] {}", self.pairs.len(), line)
    }
}

/// 负向注入场景。
pub enum CtInject {
    /// 零包源（构建必拒）。
    ZeroPkg,
    /// SMT 越界（threads=3 必拒）。
    SmtOver,
    /// MADT 失配（enabled 与坐标数不等 → 对账必红）。
    MadtMismatch,
    /// 重复 APIC id（线性分配被破坏 → 唯一性必红）。
    DupApic,
}

/// 注入核。
pub struct CtInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截/检出数。
    pub caught: u32,
}

impl CtInjector {
    /// 空注入核。
    pub fn new() -> Self {
        CtInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self, b: &mut CtBuilder) {
        // 场一：零包源。
        if !b.consume(CtSource {
            packages: 0,
            cores_per_pkg: 4,
            threads_per_core: 2,
            madt_enabled: 0,
            apic_base: 0,
        }) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：SMT 越界。
        if !b.consume(CtSource {
            packages: 1,
            cores_per_pkg: 4,
            threads_per_core: 3,
            madt_enabled: 12,
            apic_base: 0,
        }) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：MADT 失配（enabled 少报）。
        b.consume(CtSource {
            packages: 2,
            cores_per_pkg: 2,
            threads_per_core: 2,
            madt_enabled: 7, // 实际 8
            apic_base: 0,
        });
        if !b.madt_match() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：重复 APIC id（人为污染坐标表）。
        b.consume(CtSource {
            packages: 1,
            cores_per_pkg: 2,
            threads_per_core: 2,
            madt_enabled: 4,
            apic_base: 0,
        });
        let saved = b.coords.clone();
        if let Some(first) = b.coords.first().cloned() {
            let last = b.coords.len() - 1;
            b.coords[last].apic = first.apic;
        }
        let dup = !b.apic_unique();
        b.coords = saved;
        if dup {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据门面（判据四件：四元组/MADT 一致/SMT 零遗漏/注入必拒）。
pub struct CtGate {
    /// 构建器。
    pub b: CtBuilder,
    /// SMT 对账。
    pub smt: CtSmtMap,
    /// 注入核。
    pub inj: CtInjector,
}

impl CtGate {
    /// 新门面。
    pub fn new() -> Self {
        CtGate {
            b: CtBuilder::new(),
            smt: CtSmtMap::new(),
            inj: CtInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.b.coords_valid()
            && self.b.madt_match()
            && self.b.apic_unique()
            && !self.smt.pairs.is_empty()
            && self.smt.missed == 0
            && self.inj.all_caught();
        let msg = format!(
            "{}: coords={} madt={} smt_pairs={} missed={} dup_apic={} inj={}/{} [{}] seg=0x{:04x}",
            CT_VERSION,
            self.b.coords.len(),
            self.b.madt_match(),
            self.smt.pairs.len(),
            self.smt.missed,
            !self.b.apic_unique(),
            self.inj.caught,
            self.inj.cases,
            self.smt.export_line(),
            CT_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：2 包 × 2 核 × 2 线程消费 + SMT 对账 + 四场注入。
pub fn run_t602_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1602-t602");
    let mut g = CtGate::new();
    // 消费 2×2×2 = 8 线程拓扑。
    let consumed = g.b.consume(CtSource {
        packages: 2,
        cores_per_pkg: 2,
        threads_per_core: 2,
        madt_enabled: 8,
        apic_base: 0,
    });
    // SMT 对账（4 对）。
    let smt_ok = g.smt.build(&g.b);
    // 四场注入。
    g.inj.inject_all(&mut g.b);
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F1602-J1", ok && consumed && smt_ok, &msg);
    set.add("CT-coords-valid", g.b.coords_valid(), "");
    set.add("CT-madt-match", g.b.madt_match(), "");
    set.add("CT-smt-zero-miss", smt_ok && g.smt.missed == 0, "");
    set.add("CT-inject-4/4", g.inj.all_caught(), "");
    set
}
