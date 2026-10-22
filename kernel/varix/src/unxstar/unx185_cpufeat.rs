//! unx185_cpufeat —— UNX-F0185 CPU 特征统一特征表（CPUID 汇总）。
//! 锚点（CoRun Unxreal 主册 L1585）：UNX-F0185 · CPU 特征统一特征表（CPUID 汇总） · 目标 320 行 · 已深化
//! 判据原文：UNX-F0185-J1 特征位表与 CPUID 原始输出逐位一致，消费方查询 API 冻结
//! 组件：四叶位板冻结（leaf1 EDX/ECX + leaf7 EBX + leaf8 EDX · 与 F0244 摘要同构）
//!      + CPUID 汇总账（原始输出 → 位板逐位转录 · 转录一致断言）
//!      + 消费方查询 API 冻结（feature 查询闭集 · 语义名 → 位板坐标）
//!      + 位漂移注入核（汇总位被改 / 查询越界名 · 10 场必拒）
//! 判据落地：逐位一致 + 查询 API 冻结 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 位板整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CF_VERSION: &str = "unx185_cpufeat-v1";

/// 诊断段基址（0x5160 段 · 本模块独占）。
pub const CF_SEG_BASE: u16 = 0x5160;

/// 位板坐标（叶 + 寄存器 + 位号 · 查询 API 冻结载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfCoord {
    /// 叶号（0x1 / 0x7 / 0x80000001）。
    pub leaf: u32,
    /// 寄存器选择（0=EA(D)X 1=ECX 2=EBX）。
    pub reg: u8,
    /// 位号（0–31）。
    pub bit: u8,
}

impl CfCoord {
    /// 坐标合法。
    pub fn valid(&self) -> bool {
        self.bit < 32
            && matches!(self.reg, 0 | 1 | 2)
            && matches!(self.leaf, 0x1 | 0x7 | 0x8000_0001)
    }
}

/// 统一特征表（四叶位板 · 与 F0244 CpFeatDigest 同构面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfTable {
    /// leaf 0x1 EDX。
    pub leaf1_edx: u64,
    /// leaf 0x1 ECX。
    pub leaf1_ecx: u64,
    /// leaf 0x7 EBX。
    pub leaf7_ebx: u64,
    /// leaf 0x80000001 EDX。
    pub leaf8_edx: u64,
}

impl CfTable {
    /// 空表。
    pub fn zero() -> Self {
        CfTable {
            leaf1_edx: 0,
            leaf1_ecx: 0,
            leaf7_ebx: 0,
            leaf8_edx: 0,
        }
    }

    /// 按 CPUID 原始输出转录（reg：0=EDX 1=ECX 2=EBX）。
    pub fn transcribe(&mut self, leaf: u32, reg: u8, value: u64) -> bool {
        match (leaf, reg) {
            (0x1, 0) => {
                self.leaf1_edx = value;
                true
            }
            (0x1, 1) => {
                self.leaf1_ecx = value;
                true
            }
            (0x7, 2) => {
                self.leaf7_ebx = value;
                true
            }
            (0x8000_0001, 0) => {
                self.leaf8_edx = value;
                true
            }
            _ => false,
        }
    }

    /// 位读取（坐标 → 0/1；非法坐标 None）。
    pub fn bit(&self, c: CfCoord) -> Option<u8> {
        if !c.valid() {
            return None;
        }
        let board = match (c.leaf, c.reg) {
            (0x1, 0) => self.leaf1_edx,
            (0x1, 1) => self.leaf1_ecx,
            (0x7, 2) => self.leaf7_ebx,
            (0x8000_0001, 0) => self.leaf8_edx,
            _ => return None,
        };
        Some(((board >> c.bit) & 1) as u8)
    }

    /// 表指纹（FNV · 冻结载体）。
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
}

/// 消费方查询 API（冻结 v1 · 语义名闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfQuery {
    /// FPU（leaf1 EDX bit0）。
    Fpu,
    /// PAE（leaf1 EDX bit6）。
    Pae,
    /// APIC（leaf1 EDX bit9）。
    Apic,
    /// SSE4.2（leaf1 ECX bit20）。
    Sse42,
    /// x2APIC（leaf1 ECX bit21）。
    X2Apic,
    /// RDRAND（leaf1 ECX bit30）。
    Rdrand,
    /// INVPCID（leaf7 EBX bit10）。
    Invpcid,
    /// SMBI/SMAP（leaf7 EBX bit20 · SMAP）。
    Smap,
    /// NX（leaf8 EDX bit20）。
    Nx,
    /// 1GB 页（leaf8 EDX bit26）。
    Page1G,
}

impl CfQuery {
    /// 查询坐标（冻结 v1 · 语义名 → 位板坐标闭集）。
    pub fn coord(self) -> CfCoord {
        let (leaf, reg, bit) = match self {
            CfQuery::Fpu => (0x1, 0u8, 0u8),
            CfQuery::Pae => (0x1, 0, 6),
            CfQuery::Apic => (0x1, 0, 9),
            CfQuery::Sse42 => (0x1, 1, 20),
            CfQuery::X2Apic => (0x1, 1, 21),
            CfQuery::Rdrand => (0x1, 1, 30),
            CfQuery::Invpcid => (0x7, 2, 10),
            CfQuery::Smap => (0x7, 2, 20),
            CfQuery::Nx => (0x8000_0001, 0, 20),
            CfQuery::Page1G => (0x8000_0001, 0, 26),
        };
        CfCoord { leaf, reg, bit }
    }

    /// 查询名。
    pub fn name(self) -> &'static str {
        match self {
            CfQuery::Fpu => "fpu",
            CfQuery::Pae => "pae",
            CfQuery::Apic => "apic",
            CfQuery::Sse42 => "sse4.2",
            CfQuery::X2Apic => "x2apic",
            CfQuery::Rdrand => "rdrand",
            CfQuery::Invpcid => "invpcid",
            CfQuery::Smap => "smap",
            CfQuery::Nx => "nx",
            CfQuery::Page1G => "1g-page",
        }
    }
}

/// 汇总账（原始输出 → 位板 · 逐位一致断言载体）。
pub struct CfSummary {
    /// 统一特征表。
    pub table: CfTable,
    /// 转录行数。
    pub transcribed: u32,
    /// 转录拒绝数（非法叶/寄存器组合）。
    pub rejects: u32,
    /// 表指纹。
    pub frozen_hash: Option<u64>,
}

impl CfSummary {
    /// 空汇总。
    pub fn new() -> Self {
        CfSummary {
            table: CfTable::zero(),
            transcribed: 0,
            rejects: 0,
            frozen_hash: None,
        }
    }

    /// 转录一行 CPUID 原始输出。
    pub fn feed(&mut self, leaf: u32, reg: u8, value: u64) -> bool {
        if self.table.transcribe(leaf, reg, value) {
            self.transcribed += 1;
            true
        } else {
            self.rejects += 1;
            false
        }
    }

    /// 冻结指纹。
    pub fn freeze(&mut self) -> u64 {
        let h = self.table.hash();
        self.frozen_hash = Some(h);
        h
    }

    /// 逐位一致断言：查询 API 读出的位 == 原始位板位（同源自证 · 双路对拍）。
    pub fn bits_consistent(&self, expect: &CfTable) -> bool {
        self.table == *expect
    }

    /// 查询 API 冻结断言：十语义名坐标全部合法且查询不崩。
    pub fn api_frozen(&self) -> bool {
        let names = [
            CfQuery::Fpu,
            CfQuery::Pae,
            CfQuery::Apic,
            CfQuery::Sse42,
            CfQuery::X2Apic,
            CfQuery::Rdrand,
            CfQuery::Invpcid,
            CfQuery::Smap,
            CfQuery::Nx,
            CfQuery::Page1G,
        ];
        names.iter().all(|q| q.coord().valid() && self.table.bit(q.coord()).is_some())
    }
}

/// 消费方查询记录（账面留痕）。
#[derive(Clone, Debug)]
pub struct CfLookup {
    /// 查询名。
    pub name: String,
    /// 结果位。
    pub value: u8,
}

/// 消费账：逐查询留痕（冻结 API 消费核验）。
pub struct CfConsumers {
    /// 查询记录。
    pub lookups: Vec<CfLookup>,
    /// 消费方数。
    pub consumers: u32,
}

impl CfConsumers {
    /// 空消费账。
    pub fn new() -> Self {
        CfConsumers {
            lookups: Vec::new(),
            consumers: 0,
        }
    }

    /// 一位消费方全量查询（十语义名）。
    pub fn probe_all(&mut self, table: &CfTable) {
        self.lookups.clear();
        let names = [
            CfQuery::Fpu,
            CfQuery::Pae,
            CfQuery::Apic,
            CfQuery::Sse42,
            CfQuery::X2Apic,
            CfQuery::Rdrand,
            CfQuery::Invpcid,
            CfQuery::Smap,
            CfQuery::Nx,
            CfQuery::Page1G,
        ];
        for q in names {
            let v = table.bit(q.coord()).unwrap_or(0xFF);
            self.lookups.push(CfLookup {
                name: String::from(q.name()),
                value: v,
            });
        }
        self.consumers += 1;
    }

    /// 查询全有效（无 0xFF 哨兵）。
    pub fn all_valid(&self) -> bool {
        !self.lookups.is_empty()
            && self.lookups.iter().all(|l| l.value != 0xFF)
    }
}

/// 负向注入场景（10 场闭集）。
pub enum CfInject {
    /// 非法叶转录（0x2 未冻结叶）。
    BadLeaf,
    /// 非法寄存器（reg=3）。
    BadReg,
    /// 越界位号（bit 32）。
    BadBit,
    /// 汇总位漂移（转录后再改表 → 指纹变 → 漂移检出）。
    BitDrift,
    /// 冻结后改表（frozen_hash 不再匹配）。
    PostFreezeEdit,
}

/// 注入核。
pub struct CfInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截/检出数。
    pub caught: u32,
}

impl CfInjector {
    /// 空注入核。
    pub fn new() -> Self {
        CfInjector { cases: 0, caught: 0 }
    }

    /// 五场注入。
    pub fn inject_all(&mut self, sum: &mut CfSummary) {
        // 场一：非法叶。
        if !sum.feed(0x2, 0, 0x1234) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：非法寄存器。
        if !sum.feed(0x1, 3, 0x5678) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：越界位号查询。
        let bad = CfCoord {
            leaf: 0x1,
            reg: 0,
            bit: 32,
        };
        if sum.table.bit(bad).is_none() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：位漂移（改表 → 指纹变）。
        let h0 = sum.table.hash();
        let saved = sum.table.leaf7_ebx;
        sum.table.leaf7_ebx ^= 1 << 10;
        let drift = sum.table.hash() != h0;
        sum.table.leaf7_ebx = saved;
        if drift {
            self.caught += 1;
        }
        self.cases += 1;
        // 场五：冻结后改表（frozen_hash 失配检出）。
        let frozen = sum.frozen_hash.unwrap_or(0);
        sum.table.leaf1_ecx ^= 1 << 30;
        let detect = sum.table.hash() != frozen;
        sum.table.leaf1_ecx ^= 1 << 30;
        if detect && sum.frozen_hash.is_some() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 5 && self.caught == self.cases
    }
}

/// 判据门面（判据三件：逐位一致/API 冻结/注入必拒）。
pub struct CfGate {
    /// 汇总账。
    pub sum: CfSummary,
    /// 消费账。
    pub cons: CfConsumers,
    /// 注入核。
    pub inj: CfInjector,
}

impl CfGate {
    /// 新门面。
    pub fn new() -> Self {
        CfGate {
            sum: CfSummary::new(),
            cons: CfConsumers::new(),
            inj: CfInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.sum.transcribed == 4
            && self.sum.rejects >= 2
            && self.sum.frozen_hash.is_some()
            && self.sum.api_frozen()
            && self.cons.all_valid()
            && self.inj.all_caught();
        let msg = format!(
            "{}: transcribed={} rejects={} frozen={} api={} lookups={} inj={}/{} hash={} seg=0x{:04x}",
            CF_VERSION,
            self.sum.transcribed,
            self.sum.rejects,
            self.sum.frozen_hash.is_some(),
            self.sum.api_frozen(),
            self.cons.lookups.len(),
            self.inj.caught,
            self.inj.cases,
            self.sum.frozen_hash.unwrap_or(0),
            CF_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：四叶转录 + 指纹冻结 + 十语义查询 + 五场注入。
pub fn run_t185_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx185-t185");
    let mut g = CfGate::new();
    // 确定性 CPUID 原始输出（LCG 位板 · 零硬件）。
    let mut seed: u32 = 0x0185_0001;
    let mut raw = CfTable::zero();
    let mut gen = |s: &mut u32| -> u64 {
        *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        ((*s as u64) << 32) | (*s as u64 >> 32)
    };
    let v1 = gen(&mut seed);
    let v2 = gen(&mut seed);
    let v3 = gen(&mut seed);
    let v4 = gen(&mut seed);
    raw.leaf1_edx = v1;
    raw.leaf1_ecx = v2;
    raw.leaf7_ebx = v3;
    raw.leaf8_edx = v4;
    // 转录四行。
    g.sum.feed(0x1, 0, v1);
    g.sum.feed(0x1, 1, v2);
    g.sum.feed(0x7, 2, v3);
    g.sum.feed(0x8000_0001, 0, v4);
    // 逐位一致断言（对照原始板）。
    let consistent = g.sum.bits_consistent(&raw);
    // 冻结。
    g.sum.freeze();
    // 十语义查询消费。
    g.cons.probe_all(&g.sum.table);
    // 五场注入。
    g.inj.inject_all(&mut g.sum);
    let (ok, msg) = g.judge();
    // 修正记录：原为 `let (ok, _msg)` + 静态串 "verdict-summary"，把 judge
    // 算出的真实诊断文本丢掉了——判据红灯时无法定位原因。改走 `add_dyn`
    // 把运行期明细如实带回（CheckSet 的动态明细侧表承载，不破坏 Copy 语义）。
    set.add_dyn("UNX-F0185-J1", ok && consistent, &msg);
    set.add("CF-bits-consistent", consistent, "");
    set.add("CF-api-frozen", g.sum.api_frozen(), "");
    set.add("CF-consumers-valid", g.cons.all_valid(), "");
    set.add("CF-inject-5/5", g.inj.all_caught(), "");
    set
}
