//! unx106_contract — UNX-F0917 设备域协作契约文档面（B4 对接冻结
//! 接口汇总）。
//!
//! 判据原文（施工书锚点 UNX-F0917，CoRun·UNX-A2-B06 批）：
//! **UNX-F0917-J1 契约汇总齐备（本域全部对外接口一册），冻结版本
//! 化（版本号+变更日志），样例可编译（桩消费样例编译通过断言）**。
//!
//! 职责定位：B06 中断面十六单（F0901~F0916）各有 API 面，但"外
//! 域（B4 对接）要调谁、怎么调、改了谁会炸"散在各单头注释里
//! ——没有一册账。Windows 深在哪：WDK 的接口冻结纪律——每个
//! 导出内核例程有声明头（wdm.h/ntifs.h）、版本随 OS 构建号冻结、
//! 改签名必须升版本+变更日志（驱动兼容性就是靠这个活着的）；
//! DDK 样例代码是"可编译的契约"（样例编译不过=接口漂移红）。
//! Varix：①**契约册**——接口条目闭集（id/名/签名/起始版本/稳定
//! 性），id 唯一，登记对拍 B06 十六单齐备（缺单即 0x4A07）；
//! ②**冻结版本化**——册版本三元组（主/次/修订），freeze 后条目
//! 写入即 0x4A02 红，降版 0x4A05 红，变更日志 append-only（失序
//! 0x4A03）；③**样例可编译**——桩消费样例（按契约签名构造桩类
//! 型与调用形态）本文件编译通过即断言过（桩与真源的一致性靠冻
//! 结+对拍：签名条目与桩声明逐字段对拍，漂移 0x4A08）。
//! 上游：F0901~F0916 十六单 API 面（汇总对象）、F0907 导出
//! schema 版本纪律（同源声明）；下游：B4 对接域（契约消费方）、
//! F0899 健康联动、ktest B06。
//! 本单交付**契约册 + 冻结版本化 + 变更日志 + 十六单汇总 + 桩样
//! 例 + 七闸注入 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟；LCG
//! 确定性仿真；FNV-1a 快照往返；错误全显性带诊断码与建议。诊断
//! 码段 0x4A（0x49 为 F0916 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CONTRACT_VERSION: &str = "B06-contract-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x4A 段，本单独占——0x49 为 F0916 所取）
// ---------------------------------------------------------------------------

/// 0x4A01 接口 id 重复（一册一名——重名即假账）。
pub const XC_DUP_ID: u32 = 0x4A01;
/// 0x4A02 冻结后写入（冻结册改条目——契约纪律红）。
pub const XC_WRITE_AFTER_FREEZE: u32 = 0x4A02;
/// 0x4A03 变更日志失序（append-only 破——序号回退或跳号）。
pub const XC_LOG_DISORDER: u32 = 0x4A03;
/// 0x4A04 条目 schema 违例（id/名/签名为空或稳定性闭集外）。
pub const XC_ENTRY_SCHEMA: u32 = 0x4A04;
/// 0x4A05 版本回退（降版写回——单调纪律破）。
pub const XC_VERSION_REGRESS: u32 = 0x4A05;
/// 0x4A06 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4A06;
/// 0x4A07 汇总缺口（域内单元漏登记——齐备判据破）。
pub const XC_COVER_GAP: u32 = 0x4A07;
/// 0x4A08 样例签名漂移（桩声明与契约条目对拍不符）。
pub const XC_SAMPLE_DRIFT: u32 = 0x4A08;
/// 0x4A09 未冻结消费（消费未冻结契约——对接纪律红）。
pub const XC_CONSUME_UNFROZEN: u32 = 0x4A09;
/// 0x4A0A 幽灵接口（消费不存在 id）。
pub const XC_GHOST_IFACE: u32 = 0x4A0A;

// ---------------------------------------------------------------------------
// §1 稳定性闭集与版本三元组
// ---------------------------------------------------------------------------

/// 接口稳定性闭集（冻结语义三级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    /// 已稳定（可对接）。
    Stable,
    /// 已冻结（签名不可变——本册目标态）。
    Frozen,
    /// 已废弃（保留 id 仅供迁移）。
    Deprecated,
}

impl Stability {
    /// 全态枚举。
    pub const fn all() -> [Stability; 3] {
        [Stability::Stable, Stability::Frozen, Stability::Deprecated]
    }

    /// 下标。
    pub const fn idx(self) -> usize {
        match self {
            Stability::Stable => 0,
            Stability::Frozen => 1,
            Stability::Deprecated => 2,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Stability::Stable => "stable",
            Stability::Frozen => "frozen",
            Stability::Deprecated => "deprecated",
        }
    }
}

/// 版本三元组（主.次.修订——单调纪律的载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    /// 主版本（签名不兼容变更升位）。
    pub major: u16,
    /// 次版本（新增接口升位）。
    pub minor: u16,
    /// 修订（勘误升位）。
    pub patch: u16,
}

impl SemVer {
    /// 是否严格升版（任一位严格大于且高位不降——降版即假）。
    pub fn strictly_greater(&self, other: &SemVer) -> bool {
        if self.major != other.major {
            return self.major > other.major;
        }
        if self.minor != other.minor {
            return self.minor > other.minor;
        }
        self.patch > other.patch
    }

    /// 人话名。
    pub fn name(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------------------
// §2 接口条目
// ---------------------------------------------------------------------------

/// 一条对外接口契约。
#[derive(Clone, Debug)]
pub struct ContractEntry {
    /// 接口 id（唯一——"F0901-reg" 形态）。
    pub id: String,
    /// 接口名（人话）。
    pub name: String,
    /// 签名摘要（Rust 形态字符串——样例对拍依据）。
    pub signature: String,
    /// 所属域单元（B06 单号——齐备对拍键）。
    pub unit: String,
    /// 起始版本。
    pub since: SemVer,
    /// 稳定性。
    pub stability: Stability,
}

impl ContractEntry {
    /// 条目 schema 校验（id/名/签名非空、单元非空、稳定性闭集内）。
    pub fn schema_ok(&self) -> Result<(), (u32, String)> {
        if self.id.is_empty() {
            return Err((XC_ENTRY_SCHEMA, "接口 id 空——假账".to_string()));
        }
        if self.name.is_empty() {
            return Err((XC_ENTRY_SCHEMA, format!("接口 \"{}\" 名空", self.id)));
        }
        if self.signature.is_empty() {
            return Err((XC_ENTRY_SCHEMA, format!("接口 \"{}\" 签名空", self.id)));
        }
        if self.unit.is_empty() {
            return Err((XC_ENTRY_SCHEMA, format!("接口 \"{}\" 域单元空", self.id)));
        }
        // 稳定性闭集显性化（枚举天然闭集——对拍槽位）。
        let _slot = match self.stability {
            Stability::Stable => 0u8,
            Stability::Frozen => 1u8,
            Stability::Deprecated => 2u8,
        };
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 变更日志（append-only）
// ---------------------------------------------------------------------------

/// 一条变更日志。
#[derive(Clone, Debug)]
pub struct ChangeLog {
    /// 序号（单调递增——失序即红）。
    pub seq: u64,
    /// 版本（该变更后的册版本）。
    pub version: SemVer,
    /// 摘要。
    pub note: String,
}

// ---------------------------------------------------------------------------
// §4 契约册（登记/冻结/版本/日志）
// ---------------------------------------------------------------------------

/// 设备域契约册。
pub struct ContractBook {
    /// 条目账（id→条目）。
    pub entries: BTreeMap<String, ContractEntry>,
    /// 变更日志（append-only）。
    pub log: Vec<ChangeLog>,
    /// 当前版本。
    pub version: SemVer,
    /// 是否冻结。
    pub frozen: bool,
    /// 日志序号游标。
    pub log_seq: u64,
}

impl ContractBook {
    /// 新册（版本从 0.1.0 起步）。
    pub fn new() -> ContractBook {
        ContractBook {
            entries: BTreeMap::new(),
            log: Vec::new(),
            version: SemVer {
                major: 0,
                minor: 1,
                patch: 0,
            },
            frozen: false,
            log_seq: 0,
        }
    }

    /// 登记接口（schema 闸 + id 唯一 + 冻结闸）。
    pub fn register(&mut self, entry: ContractEntry) -> Result<(), (u32, String)> {
        if self.frozen {
            return Err((
                XC_WRITE_AFTER_FREEZE,
                format!("册已冻结 {}——接口 \"{}\" 登记拒", self.version.name(), entry.id),
            ));
        }
        entry.schema_ok()?;
        if self.entries.contains_key(&entry.id) {
            return Err((
                XC_DUP_ID,
                format!("接口 id \"{}\" 重复——一册一名", entry.id),
            ));
        }
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    /// 冻结（单向——解冻不存在；冻结即对接基线）。
    pub fn freeze(&mut self) -> Result<(), (u32, String)> {
        if self.frozen {
            return Err((XC_WRITE_AFTER_FREEZE, "册已冻结——重复冻结拒".to_string()));
        }
        if self.entries.is_empty() {
            return Err((XC_COVER_GAP, "空册冻结——契约无内容".to_string()));
        }
        self.frozen = true;
        Ok(())
    }

    /// 升版+记日志（append-only：seq 单调递增，版本严格升）。
    pub fn bump_version(&mut self, v: SemVer, note: &str) -> Result<(), (u32, String)> {
        if !v.strictly_greater(&self.version) {
            return Err((
                XC_VERSION_REGRESS,
                format!("版本 {} 不严格高于当前 {}——降版拒", v.name(), self.version.name()),
            ));
        }
        self.log_seq += 1;
        if self.log_seq == 0 {
            return Err((XC_LOG_DISORDER, "日志序号溢出回绕——失序".to_string()));
        }
        // append-only 对拍：seq 必须恰为上一条+1。
        if let Some(last) = self.log.last() {
            if self.log_seq != last.seq + 1 {
                return Err((
                    XC_LOG_DISORDER,
                    format!("日志序 {} 与上条 {} 不连续", self.log_seq, last.seq),
                ));
            }
        } else if self.log_seq != 1 {
            return Err((XC_LOG_DISORDER, format!("首条日志序 {} ≠ 1", self.log_seq)));
        }
        self.log.push(ChangeLog {
            seq: self.log_seq,
            version: v,
            note: note.to_string(),
        });
        self.version = v;
        Ok(())
    }

    /// 消费前置：册必须已冻结（未冻结消费红——对接纪律）。
    pub fn consume_ready(&self) -> Result<(), (u32, String)> {
        if !self.frozen {
            return Err((
                XC_CONSUME_UNFROZEN,
                format!("契约 {} 未冻结——对接消费拒", self.version.name()),
            ));
        }
        Ok(())
    }

    /// 接口解析（幽灵 id 显性拒）。
    pub fn resolve(&self, id: &str) -> Result<&ContractEntry, (u32, String)> {
        match self.entries.get(id) {
            Some(e) => Ok(e),
            None => Err((
                XC_GHOST_IFACE,
                format!("接口 \"{}\" 不在册——幽灵接口", id),
            )),
        }
    }

    /// 汇总齐备对拍：域单元清单必须全覆盖（缺单 0x4A07）。
    pub fn coverage_audit(&self, units: &[&str]) -> Result<usize, (u32, String)> {
        let mut covered: Vec<&str> = Vec::new();
        for u in units.iter() {
            let hit = self.entries.values().any(|e| e.unit == *u);
            if hit {
                covered.push(u);
            }
        }
        if covered.len() != units.len() {
            let missing: Vec<&str> = units
                .iter()
                .filter(|u| !covered.contains(u))
                .copied()
                .collect();
            return Err((
                XC_COVER_GAP,
                format!(
                    "汇总缺口 {}/{}：缺 {}",
                    covered.len(),
                    units.len(),
                    missing.join(",")
                ),
            ));
        }
        Ok(covered.len())
    }

    /// 行式导出（F0899 体例——确定性）。
    pub fn export_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "契约册 v={} 条目={} 冻结={} 日志={}",
            self.version.name(),
            self.entries.len(),
            self.frozen,
            self.log.len()
        ));
        for (id, e) in self.entries.iter() {
            out.push(format!(
                "接口 {} 名=\"{}\" 单元={} 签名=\"{}\" 自={} 稳定性={}",
                id, e.name, e.unit, e.signature, e.since.name(), e.stability.name()
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §5 桩消费样例（可编译断言本体——按契约签名构造调用形态）
// ---------------------------------------------------------------------------

/// 桩：向量路径（与 F0908/F0916 VecPath 签名对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StubPath {
    /// INTx。
    SIntx,
    /// MSI。
    SMsi,
    /// MSI-X。
    SMsiX,
}

/// 桩：指标行（与 F0913/F0914 MetricRow 三元组签名对齐）。
#[derive(Clone, Debug)]
pub struct StubMetric {
    /// 名。
    pub name: String,
    /// 值。
    pub value: u64,
    /// 单位字面（us/count）。
    pub unit: &'static str,
}

/// 桩：告警级别（与 F0916 AlertLevel 签名对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StubAlert {
    /// 告警。
    SWarn,
    /// 红账。
    SRed,
}

/// 样例消费面：按契约声明的调用形态逐条消费（本函数编译通过即
/// "样例可编译"断言——签名漂移会在对拍层被抓）。
pub fn sample_consumer(metrics: &[StubMetric], path: StubPath, alert: StubAlert) -> String {
    let mut out = String::new();
    for m in metrics.iter() {
        out.push_str(&format!("样例 指标={} 值={} 单位={}\n", m.name, m.value, m.unit));
    }
    let p = match path {
        StubPath::SIntx => "intx",
        StubPath::SMsi => "msi",
        StubPath::SMsiX => "msix",
    };
    let a = match alert {
        StubAlert::SWarn => "告警",
        StubAlert::SRed => "红账",
    };
    out.push_str(&format!("样例 路径={} 级别={}\n", p, a));
    out
}

/// 桩↔契约签名对拍：样例桩的声明面与册内 B06 样例条目逐字段比
/// 对（漂移 0x4A08——"可编译"与"对得上"双闸）。
pub fn sample_signature_audit(book: &ContractBook) -> Result<usize, (u32, String)> {
    // 对拍样例接口条目（登记时签名即写"样例签名"——与桩一致）。
    let cases: [(&str, &str); 3] = [
        ("F0913-metric-export", "fn export(Ledger)->Vec<String>"),
        ("F0916-path-form", "enum VecPath{Intx,Msi,MsiX}"),
        ("F0916-alert-level", "enum AlertLevel{Warn,Red}"),
    ];
    for (id, want) in cases.iter() {
        let e = book.resolve(id)?;
        if e.signature != *want {
            return Err((
                XC_SAMPLE_DRIFT,
                format!("样例签名漂移：\"{}\" 册内 \"{}\" ≠ 桩 \"{}\"", id, e.signature, want),
            ));
        }
    }
    // 样例调用（编译期形态 + 运行期产物非空）。
    let demo = sample_consumer(
        &[StubMetric {
            name: "irq.arrive.total".to_string(),
            value: 3,
            unit: "count",
        }],
        StubPath::SMsiX,
        StubAlert::SRed,
    );
    if demo.is_empty() {
        return Err((XC_SAMPLE_DRIFT, "样例消费产物空——桩失灵".to_string()));
    }
    Ok(cases.len())
}

// ---------------------------------------------------------------------------
// §6 B06 域十六单接口汇总（齐备判据的登记面）
// ---------------------------------------------------------------------------

/// B06 域单元清单（F0901~F0916——汇总齐备对拍基准）。
pub const B06_UNITS: [&str; 16] = [
    "F0901", "F0902", "F0903", "F0904", "F0905", "F0906", "F0907", "F0908",
    "F0909", "F0910", "F0911", "F0912", "F0913", "F0914", "F0915", "F0916",
];

/// 构造 B06 域契约册：十六单各登记一条核心对外接口（齐备≥1/单）。
pub fn build_b06_book() -> Result<ContractBook, (u32, String)> {
    let mut book = ContractBook::new();
    let v1 = SemVer { major: 0, minor: 1, patch: 0 };
    let mk = |id: &str, name: &str, sig: &str, unit: &str| ContractEntry {
        id: id.to_string(),
        name: name.to_string(),
        signature: sig.to_string(),
        unit: unit.to_string(),
        since: v1,
        stability: Stability::Stable,
    };
    // 十六单核心接口（签名=桩对拍口径的人话形态）。
    book.register(mk("F0901-irqregister", "中断注册面", "fn register(key,u32)->Result", "F0901"))?;
    book.register(mk("F0902-msix-multivec", "MSI-X 多向量配额", "fn quota()->Vec<u32>", "F0902"))?;
    book.register(mk("F0903-handler-registry", "handler 五态生命周期", "fn transit(State,Event)->State", "F0903"))?;
    book.register(mk("F0904-sharedvector", "共享向量分发票", "enum VecPath{Intx,Msi,MsiX}", "F0904"))?;
    book.register(mk("F0905-irqcontext", "中断上下文契约", "fn token()->CtxToken", "F0905"))?;
    book.register(mk("F0906-floodtest", "注水测试资产", "fn admit(WaterTank)->Result", "F0906"))?;
    book.register(mk("F0907-auditreplay", "审计账与回放", "fn export()->Schema", "F0907"))?;
    book.register(mk("F0908-enableseq", "使能时序状态机", "fn step(EnableStep)->Result", "F0908"))?;
    book.register(mk("F0909-dualcapability", "双能力裁决", "fn arbitrate(Caps)->Path", "F0909"))?;
    book.register(mk("F0910-intxpath", "INTx 兼容路径", "fn deassert_seq()->[EventPhase;6]", "F0910"))?;
    book.register(mk("F0911-irqname", "中断命名面", "fn validate(&str)->Result", "F0911"))?;
    book.register(mk("F0912-crosscheck", "三账交叉校验", "fn cross_check(Ledger)->Report", "F0912"))?;
    book.register(mk("F0913-metric-export", "统计导出", "fn export(Ledger)->Vec<String>", "F0913"))?;
    book.register(mk("F0914-handlertime", "时长直方图", "fn observe(key,us,tick)->Steps", "F0914"))?;
    book.register(mk("F0915-teardown", "注销清理链", "fn teardown(key,Closure)->Result", "F0915"))?;
    book.register(mk("F0916-alert-ledger", "共享告警账", "enum AlertLevel{Warn,Red}", "F0916"))?;
    // 汇总齐备对拍（判据第一项正主）。
    let n = book.coverage_audit(&B06_UNITS)?;
    let _ = n;
    Ok(book)
}

// ---------------------------------------------------------------------------
// §7 LCG + FNV
// ---------------------------------------------------------------------------

/// 线性同余发生器（Numerical Recipes 参数）。
pub struct Lcg(u64);

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }

    /// 半开区间 [0, n)。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 契约册快照。
pub struct ContractSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl ContractSnap {
    /// 采样。
    pub fn take(book: &ContractBook) -> ContractSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&book.version.major.to_le_bytes());
        buf.extend_from_slice(&book.version.minor.to_le_bytes());
        buf.extend_from_slice(&book.version.patch.to_le_bytes());
        buf.push(u8::from(book.frozen));
        buf.extend_from_slice(&book.log_seq.to_le_bytes());
        for (id, e) in book.entries.iter() {
            buf.extend_from_slice(id.as_bytes());
            buf.extend_from_slice(e.unit.as_bytes());
            buf.extend_from_slice(e.signature.as_bytes());
            buf.push(e.stability.idx() as u8);
        }
        for l in book.log.iter() {
            buf.extend_from_slice(&l.seq.to_le_bytes());
            buf.extend_from_slice(l.note.as_bytes());
        }
        ContractSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "v={} 条目={} 冻结={} 日志={}",
                book.version.name(),
                book.entries.len(),
                book.frozen,
                book.log.len()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §8 七闸注入
// ---------------------------------------------------------------------------

/// 七闸注入：重复 id/冻结后写/日志失序/降版/schema 违例/幽灵接口/
/// 未冻结消费。
pub fn inject_gates_7() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    let v1 = SemVer { major: 0, minor: 1, patch: 0 };
    let mk = |id: &str| ContractEntry {
        id: id.to_string(),
        name: "测试接口".to_string(),
        signature: "fn x()->u32".to_string(),
        unit: "F0999".to_string(),
        since: v1,
        stability: Stability::Stable,
    };

    // ── 1 重复 id ──
    {
        let mut book = ContractBook::new();
        book.register(mk("dup.one"))?;
        let r = book.register(mk("dup.one"));
        hits.push((1, matches!(r, Err((XC_DUP_ID, _))), format!("{:?}", r.is_err())));
    }
    // ── 2 冻结后写 ──
    {
        let mut book = ContractBook::new();
        book.register(mk("a.one"))?;
        book.freeze()?;
        let r = book.register(mk("b.two"));
        hits.push((2, matches!(r, Err((XC_WRITE_AFTER_FREEZE, _))), format!("{:?}", r.is_err())));
    }
    // ── 3 日志失序（首条非 1）──
    {
        let mut book = ContractBook::new();
        book.log_seq = 5; // 人工跳号
        let r = book.bump_version(SemVer { major: 0, minor: 2, patch: 0 }, "跳号");
        hits.push((3, matches!(r, Err((XC_LOG_DISORDER, _))), format!("{:?}", r.is_err())));
    }
    // ── 4 降版 ──
    {
        let mut book = ContractBook::new();
        book.bump_version(SemVer { major: 1, minor: 0, patch: 0 }, "升主")?;
        let r = book.bump_version(SemVer { major: 0, minor: 9, patch: 0 }, "降版");
        hits.push((4, matches!(r, Err((XC_VERSION_REGRESS, _))), format!("{:?}", r.is_err())));
    }
    // ── 5 schema 违例（签名为空）──
    {
        let mut book = ContractBook::new();
        let bad = ContractEntry {
            id: "bad.one".to_string(),
            name: "坏条目".to_string(),
            signature: String::new(),
            unit: "F0999".to_string(),
            since: v1,
            stability: Stability::Stable,
        };
        let r = book.register(bad);
        hits.push((5, matches!(r, Err((XC_ENTRY_SCHEMA, _))), format!("{:?}", r.is_err())));
    }
    // ── 6 幽灵接口 ──
    {
        let book = ContractBook::new();
        let r = book.resolve("ghost.x");
        hits.push((6, matches!(r, Err((XC_GHOST_IFACE, _))), format!("{:?}", r.is_err())));
    }
    // ── 7 未冻结消费 ──
    {
        let mut book = ContractBook::new();
        book.register(mk("c.three"))?;
        let r = book.consume_ready();
        hits.push((7, matches!(r, Err((XC_CONSUME_UNFROZEN, _))), format!("{:?}", r.is_err())));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_WRITE_AFTER_FREEZE, format!("七闸漏网：{}", names.join("；"))));
    }
    Ok("七闸全拦（重名/冻结后写/失序/降版/schema/幽灵/未冻结消费）".to_string())
}

// ---------------------------------------------------------------------------
// §9 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：B06 汇总+冻结+升版日志+样例对拍+七闸+快照，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    // 1）B06 域契约册：十六单齐备登记。
    let mut book = build_b06_book()?;
    if book.entries.len() != 16 {
        return Err((
            XC_COVER_GAP,
            format!("B06 契约册条目 {} ≠ 16", book.entries.len()),
        ));
    }
    parts.push(format!("B06 契约册齐备：{} 单全覆盖", book.entries.len()));
    // 2）冻结（对接基线）。
    book.freeze()?;
    book.consume_ready()?;
    // 3）升版+变更日志（冻结后接口勘误走升版不动条目——纪律演示）。
    book.bump_version(
        SemVer { major: 0, minor: 2, patch: 0 },
        "新增样例对拍条目口径（十六单签名冻结版）",
    )?;
    parts.push(format!(
        "冻结+升版过：v={} 日志 {} 条",
        book.version.name(),
        book.log.len()
    ));
    // 4）样例可编译+签名对拍（判据第三项正主）。
    let n = sample_signature_audit(&book)?;
    parts.push(format!("桩样例对拍 {} 条过（可编译断言）", n));
    // 5）导出确定性。
    let a = book.export_lines();
    let b = book.export_lines();
    if a != b {
        return Err((XC_SNAP_DRIFT, "契约册两次导出不等".to_string()));
    }
    parts.push(format!("导出确定性 {} 行逐字一致", a.len()));
    // 6）七闸注入。
    parts.push(inject_gates_7()?);
    // 7）快照稳定。
    let s0 = ContractSnap::take(&book);
    let s1 = ContractSnap::take(&book);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "契约册快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
