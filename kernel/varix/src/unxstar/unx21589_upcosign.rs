//! UNX-F21589 · 上游会签复核（AI-26 四组冻结接口）
//!
//! 诊断段 0x91041~0x91048（F2-B40 域收官段 9/100）。
//!
//! 判据原文（施工书 UNX-F21589-J1）：会签复核 20 例，接口版本未漂移。
//!
//! 设计要点（为什么复核不返回 `Err` 而是返回「带结论的报告」）：
//! 20 例里出现漂移是会签的**正常业务结果**，不是调用失败。若用 `Err` 表达
//! 「有 3 例签名不符」，调用方只拿得到第一条故障，另外 17 例结论全丢，台账
//! 就永远缺页。故分两层：`review` 的 `Err` 只表达**结构性不可复核**（样本数
//! 不是 20 / 四组登记不全），样本级问题落进报告 `faults`，由 `conclusion`
//! 汇总三态——「否决」也是一份可归档的结论，不是一次异常。
//!
//! 判序固定为「登记面 → 主版本 → 次版本 → 签名」：名字没登记时版本比对毫无
//! 意义；主版本变了签名必然跟着变，先报主版本才不会让运维追假线索。结论由
//! 证据推导但允许人工请求：`ratify` 对三种自相矛盾的请求都拒，且引证证据码
//! 而非自造码（自造码等于把跨模块对账键私有化）。

use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 会签样本数（施工书：20 例）。
pub const SAMPLE_TARGET: usize = 20;
/// 冻结接口组数（AI-26 四组）。
pub const GROUP_TARGET: u8 = 4;

/// 会签诊断码（0x91041~0x91048 八码闭集）。
///
/// 显式判别式：码值是跨模块对账键，隐式递增会随变体插入整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignCode {
    CosignOk = 0x91041,
    IfaceUnknown = 0x91042,
    MajorDrift = 0x91043,
    SignatureMismatch = 0x91044,
    MinorRegress = 0x91045,
    GroupIncomplete = 0x91046,
    ConditionUnsatisfied = 0x91047,
    VetoWithoutCause = 0x91048,
}

impl CosignCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            CosignCode::CosignOk => "会签通过，接口版本未漂移",
            CosignCode::IfaceUnknown => "会签样本指向未登记接口",
            CosignCode::MajorDrift => "接口主版本已漂移",
            CosignCode::SignatureMismatch => "签名指纹与冻结值不符",
            CosignCode::MinorRegress => "接口次版本回退",
            CosignCode::GroupIncomplete => "四组登记面或 20 例样本面不完整",
            CosignCode::ConditionUnsatisfied => "有条件通过缺少条件项",
            CosignCode::VetoWithoutCause => "否决结论没有故障引证",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            CosignCode::CosignOk => "20 例逐项命中冻结版本与签名",
            CosignCode::IfaceUnknown => "被复核的名字不在四组冻结登记表内",
            CosignCode::MajorDrift => "主版本变化意味着契约不兼容",
            CosignCode::SignatureMismatch => "签名是最强契约锚点，变了就是改了",
            CosignCode::MinorRegress => "次版本只许增，减少说明上游回滚了已发布接口",
            CosignCode::GroupIncomplete => "四组缺组或样本数不是 20，复核无从谈起",
            CosignCode::ConditionUnsatisfied => "有条件通过必须以条件清单为载体",
            CosignCode::VetoWithoutCause => "否决必须指向具体故障，否则不可申诉",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            CosignCode::CosignOk => "归档会签报告并转入下游复核",
            CosignCode::IfaceUnknown => "核对接口名拼写，或先补登记再复核",
            CosignCode::MajorDrift => "冻结主版本，另开新版接口并重新会签",
            CosignCode::SignatureMismatch => "比对上游头文件，确认是否无声改签名",
            CosignCode::MinorRegress => "要求上游恢复已发布次版本",
            CosignCode::GroupIncomplete => "补齐四组登记与 20 例样本后重跑",
            CosignCode::ConditionUnsatisfied => "写入条件项，或把结论降为否决",
            CosignCode::VetoWithoutCause => "引证故障条目，或把结论改为通过",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 接口分组（四组 + 未登记哨兵）。
///
/// 留 `Unregistered = 0` 而非 `Option<Group>`：定位字段在故障里必须恒有值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Unregistered = 0,
    Core = 1,
    Mem = 2,
    Sched = 3,
    Irq = 4,
}

impl Group {
    /// 组名（报告与定位用）。
    pub const fn label(self) -> &'static str {
        match self {
            Group::Unregistered => "unregistered",
            Group::Core => "g1-core",
            Group::Mem => "g2-mem",
            Group::Sched => "g3-sched",
            Group::Irq => "g4-irq",
        }
    }
    /// 组号（位图与下标用）。
    pub const fn code(self) -> u8 {
        self as u8
    }
    /// 是否属于四组。
    pub const fn is_frozen(self) -> bool {
        self.code() >= 1 && self.code() <= GROUP_TARGET
    }
}

/// 会签故障：码 + 组 + 接口名，三件套足以定位到行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CosignFault {
    pub code: u32,
    pub group: Group,
    pub iface: &'static str,
}

impl CosignFault {
    const fn new(code: CosignCode, group: Group, iface: &'static str) -> CosignFault {
        CosignFault { code: code.code(), group, iface }
    }
    /// 码还原（闭集外按最保守的「登记面不完整」处理，不 panic）。
    pub const fn as_code(self) -> CosignCode {
        match self.code {
            0x91041 => CosignCode::CosignOk,
            0x91042 => CosignCode::IfaceUnknown,
            0x91043 => CosignCode::MajorDrift,
            0x91044 => CosignCode::SignatureMismatch,
            0x91045 => CosignCode::MinorRegress,
            0x91047 => CosignCode::ConditionUnsatisfied,
            0x91048 => CosignCode::VetoWithoutCause,
            _ => CosignCode::GroupIncomplete,
        }
    }
    /// 一行定位文本（组名 + 接口名）。
    pub fn line(&self) -> String {
        let mut out = String::with_capacity(48);
        out.push_str(self.group.label());
        out.push('/');
        out.push_str(self.iface);
        out
    }
}

/// 冻结接口登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenIface {
    pub group: Group,
    pub name: &'static str,
    pub major: u16,
    pub minor: u16,
    pub sig: u64,
    /// 冻结日（单调日序号，非墙钟）。
    pub day_seq: u32,
    /// 消费方清单：登记即冻结，运行时不得增删。
    pub consumers: &'static [&'static str],
}

impl FrozenIface {
    /// 构造登记项（校验在 `FreezeRegistry::register` 统一做）。
    pub const fn new(
        group: Group,
        name: &'static str,
        major: u16,
        minor: u16,
        sig: u64,
        day_seq: u32,
        consumers: &'static [&'static str],
    ) -> FrozenIface {
        FrozenIface { group, name, major, minor, sig, day_seq, consumers }
    }
    /// 自洽：组内、名非空、主版本非零、有消费方、冻结日有效。
    pub const fn is_well_formed(&self) -> bool {
        self.group.is_frozen()
            && !self.name.is_empty()
            && self.major > 0
            && self.day_seq > 0
            && !self.consumers.is_empty()
    }
}

/// 会签样本（一例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub iface: &'static str,
    pub want_major: u16,
    pub want_minor: u16,
    pub got_major: u16,
    pub got_minor: u16,
    pub got_sig: u64,
}

impl Sample {
    /// 构造样本。
    pub const fn new(
        iface: &'static str,
        want_major: u16,
        want_minor: u16,
        got_major: u16,
        got_minor: u16,
        got_sig: u64,
    ) -> Sample {
        Sample { iface, want_major, want_minor, got_major, got_minor, got_sig }
    }
}

/// 漂移类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drift {
    None,
    /// 次版本增：允许，记录。
    MinorAhead,
    /// 次版本减：拒。
    MinorBack,
    /// 主版本变：拒。
    Major,
    /// 名字未登记：拒，且不参与版本比较。
    Unknown,
}

impl Drift {
    /// 名称（报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            Drift::None => "none",
            Drift::MinorAhead => "minor-ahead",
            Drift::MinorBack => "minor-back",
            Drift::Major => "major",
            Drift::Unknown => "unknown",
        }
    }
    /// 是否阻断会签。
    pub const fn is_blocking(self) -> bool {
        matches!(self, Drift::Major | Drift::MinorBack | Drift::Unknown)
    }
}

/// 单例复核结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleVerdict {
    pub group: Group,
    pub iface: &'static str,
    pub drift: Drift,
    pub sig_ok: bool,
}

/// 会签结论三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conclusion {
    Pass,
    /// 存在允许的次版本增量，附条件项。
    Conditional,
    Veto,
}

impl Conclusion {
    /// 名称（导出文本用）。
    pub const fn label(self) -> &'static str {
        match self {
            Conclusion::Pass => "pass",
            Conclusion::Conditional => "conditional",
            Conclusion::Veto => "veto",
        }
    }
}

/// 会签报告。字段全公开：允许外部构造敌意报告来检验 `ratify` 的闸门。
pub struct CosignReport {
    pub verdicts: Vec<SampleVerdict>,
    pub faults: Vec<CosignFault>,
    pub conclusion: Conclusion,
    pub conditions: Vec<&'static str>,
    pub registry_fp: u64,
}

impl CosignReport {
    /// 故障条数。
    pub fn fault_count(&self) -> usize {
        self.faults.len()
    }
    /// 报告指纹：登记表指纹 + 逐例 (组, 漂移, 签名命中)。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.registry_fp);
        for v in self.verdicts.iter() {
            h = fnv_u64(h, v.group.code() as u64);
            h = fnv_u64(h, v.drift as u64);
            h = fnv_u64(h, if v.sig_ok { 1 } else { 0 });
        }
        h
    }
    /// 导出文本（字段序固定，便于逐行 diff；`list` 只列非 `none` 的漂移）。
    pub fn export(&self) -> String {
        let mut list = String::with_capacity(64);
        let mut ahead = 0u32;
        for v in self.verdicts.iter() {
            if matches!(v.drift, Drift::MinorAhead) {
                ahead += 1;
            }
            if !matches!(v.drift, Drift::None) {
                if !list.is_empty() {
                    list.push(',');
                }
                list.push_str(v.iface);
                list.push(':');
                list.push_str(v.drift.label());
            }
        }
        let mut out = String::with_capacity(192);
        out.push_str("unx-f21589 verdicts=");
        out.push_str(&self.verdicts.len().to_string());
        out.push_str(" faults=");
        out.push_str(&self.fault_count().to_string());
        out.push_str(" drifts=");
        out.push_str(&ahead.to_string());
        out.push_str(" conditions=");
        out.push_str(&self.conditions.len().to_string());
        out.push_str(" conclusion=");
        out.push_str(self.conclusion.label());
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint()));
        out.push_str(" list=[");
        out.push_str(&list);
        out.push(']');
        out
    }
}

/// 四组冻结登记表。
pub struct FreezeRegistry {
    items: Vec<FrozenIface>,
    /// 冻结代次：每次全表冻结 +1，代替墙钟进入指纹。
    epoch: u16,
}

impl FreezeRegistry {
    /// 新建空表。
    pub const fn new(epoch: u16) -> FreezeRegistry {
        FreezeRegistry { items: Vec::new(), epoch }
    }
    /// 登记一个冻结接口。
    ///
    /// 重复名与非法组都记 `GroupIncomplete`：两者都意味着「这张登记表不成立」，
    /// 定位靠 `iface` 字段——为登记卫生问题单开码，会让码段被杂务占满。
    pub fn register(&mut self, item: FrozenIface) -> Result<(), CosignFault> {
        if !item.is_well_formed() {
            return Err(CosignFault::new(CosignCode::GroupIncomplete, item.group, item.name));
        }
        if self.lookup(item.name).is_some() {
            return Err(CosignFault::new(CosignCode::GroupIncomplete, item.group, item.name));
        }
        self.items.push(item);
        Ok(())
    }
    /// 按名查登记项。
    pub fn lookup(&self, name: &str) -> Option<&FrozenIface> {
        for it in self.items.iter() {
            if it.name == name {
                return Some(it);
            }
        }
        None
    }
    /// 按下标取登记项（夹具生成样本用）。
    pub fn item(&self, idx: usize) -> Option<&FrozenIface> {
        self.items.get(idx)
    }
    /// 登记项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 已覆盖组数（位图计数，只看 1..=4）。
    pub fn groups_present(&self) -> u8 {
        let mut mask = 0u8;
        for it in self.items.iter() {
            let g = it.group.code();
            if g >= 1 && g <= GROUP_TARGET {
                mask |= 1u8 << g;
            }
        }
        mask.count_ones() as u8
    }
    /// 登记表指纹：逐项喂 (组号, 名字字节, 版本, 签名, 冻结日, 消费方字节)。
    ///
    /// 名字按字节喂而非按 `&str` 指针：指针随加载地址变化，指纹会失去可比性。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.epoch as u64);
        for it in self.items.iter() {
            h = fnv_u64(h, it.group.code() as u64);
            h = feed_str(h, it.name);
            h = fnv_u64(h, it.major as u64);
            h = fnv_u64(h, it.minor as u64);
            h = fnv_u64(h, it.sig);
            h = fnv_u64(h, it.day_seq as u64);
            for c in it.consumers.iter() {
                h = feed_str(h, c);
            }
        }
        h
    }
    /// 复核 20 例会签样本：结构性问题走 `Err`，样本级问题进报告。
    pub fn review(&self, samples: &[Sample]) -> Result<CosignReport, CosignFault> {
        if samples.len() != SAMPLE_TARGET {
            return Err(gate(CosignCode::GroupIncomplete, "sample-count"));
        }
        if self.groups_present() != GROUP_TARGET {
            return Err(gate(CosignCode::GroupIncomplete, "group-coverage"));
        }
        let mut verdicts = Vec::with_capacity(SAMPLE_TARGET);
        let mut faults: Vec<CosignFault> = Vec::new();
        let mut ahead = false;
        for s in samples.iter() {
            match self.lookup(s.iface) {
                None => {
                    faults.push(CosignFault::new(CosignCode::IfaceUnknown, Group::Unregistered, s.iface));
                    verdicts.push(SampleVerdict {
                        group: Group::Unregistered,
                        iface: s.iface,
                        drift: Drift::Unknown,
                        sig_ok: false,
                    });
                }
                Some(it) => {
                    let drift = classify(s, it);
                    let sig_ok = s.got_sig == it.sig;
                    // 先版本后签名：版本已漂移时签名必然跟着变，报签名只会误导。
                    let code = match drift {
                        Drift::Major => Some(CosignCode::MajorDrift),
                        Drift::MinorBack => Some(CosignCode::MinorRegress),
                        _ if !sig_ok => Some(CosignCode::SignatureMismatch),
                        _ => None,
                    };
                    if let Some(c) = code {
                        faults.push(CosignFault::new(c, it.group, s.iface));
                    }
                    ahead = ahead || matches!(drift, Drift::MinorAhead);
                    verdicts.push(SampleVerdict { group: it.group, iface: s.iface, drift, sig_ok });
                }
            }
        }
        let conditions: Vec<&'static str> = if ahead {
            vec!["次版本增量的消费方需在下一会签周期前完成适配确认"]
        } else {
            Vec::new()
        };
        let conclusion = if !faults.is_empty() {
            Conclusion::Veto
        } else if ahead {
            Conclusion::Conditional
        } else {
            Conclusion::Pass
        };
        Ok(CosignReport {
            verdicts,
            faults,
            conclusion,
            conditions,
            registry_fp: self.fingerprint(),
        })
    }
}

/// 单例漂移分类。主版本先判：主版本变了，同主版本内的次版本比较已无意义。
fn classify(s: &Sample, it: &FrozenIface) -> Drift {
    if s.got_major != s.want_major || it.major != s.want_major {
        return Drift::Major;
    }
    if s.got_minor < s.want_minor {
        return Drift::MinorBack;
    }
    if s.got_minor > s.want_minor {
        return Drift::MinorAhead;
    }
    Drift::None
}

/// 核定会签结论（闸门）。
///
/// 三个分支都用证据说话：请求通过与故障清单矛盾时报**第一条故障的码**，
/// 不自造「与证据不符」——规范里没有的码等于私有化对账键。
pub fn ratify(report: &CosignReport, requested: Conclusion) -> Result<Conclusion, CosignFault> {
    match requested {
        Conclusion::Pass => {
            if let Some(f) = report.faults.first() {
                return Err(*f);
            }
            if !report.conditions.is_empty() {
                return Err(gate(CosignCode::ConditionUnsatisfied, "pass-with-conditions"));
            }
            Ok(Conclusion::Pass)
        }
        Conclusion::Conditional => {
            if report.conditions.is_empty() {
                return Err(gate(CosignCode::ConditionUnsatisfied, "no-condition"));
            }
            if let Some(f) = report.faults.first() {
                return Err(*f);
            }
            Ok(Conclusion::Conditional)
        }
        Conclusion::Veto => {
            if report.faults.is_empty() {
                return Err(gate(CosignCode::VetoWithoutCause, "no-fault"));
            }
            Ok(Conclusion::Veto)
        }
    }
}

/// 结构性故障（组无归属，定位靠标签字符串）。
fn gate(code: CosignCode, tag: &'static str) -> CosignFault {
    CosignFault::new(code, Group::Unregistered, tag)
}

/// 接口签名指纹：对接口名做 FNV-1a 64。
///
/// 真实头文件签名在 no_std 里取不到，故以「名字 → 指纹」的确定映射充当锚点：
/// 名字或算法被改动，指纹必变，判据即可捕捉。
pub fn signature_of(name: &str) -> u64 {
    feed_str(0xcbf2_9ce4_8422_2325, name)
}

/// 把一个字符串按字节喂进 FNV-1a（名字、消费方名共用）。
pub fn feed_str(mut h: u64, s: &str) -> u64 {
    for b in s.as_bytes() {
        h = fnv_u64(h, *b as u64);
    }
    h
}

/// FNV-1a 64 单步。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    for shift in (0..64u32).step_by(4).rev() {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
    }
    s
}

/// 夹具包：登记表 + 20 例样本。
pub struct CosignFixture {
    pub registry: FreezeRegistry,
    pub samples: Vec<Sample>,
}

/// 标准登记表：四组各一条，各带两个消费方。
pub fn standard_registry() -> Result<FreezeRegistry, CosignFault> {
    let mut reg = FreezeRegistry::new(7);
    let items = [
        FrozenIface::new(Group::Core, "ai26.core.dispatch", 1, 4, signature_of("ai26.core.dispatch"), 101, &["ai29.fs", "ai30.net"]),
        FrozenIface::new(Group::Mem, "ai26.mem.arena", 2, 1, signature_of("ai26.mem.arena"), 102, &["ai66.vfs", "ai70.mm"]),
        FrozenIface::new(Group::Sched, "ai26.sched.tick", 3, 6, signature_of("ai26.sched.tick"), 103, &["ai61.sched", "ai67.task"]),
        FrozenIface::new(Group::Irq, "ai26.irq.route", 4, 2, signature_of("ai26.irq.route"), 104, &["ai69.irq", "ai87.dma"]),
    ];
    items.iter().try_for_each(|it| reg.register(*it))?;
    Ok(reg)
}

/// 按登记表轮转生成 20 例样本（四组各 5 例）。
pub fn standard_samples(reg: &FreezeRegistry) -> Vec<Sample> {
    let mut out = Vec::with_capacity(SAMPLE_TARGET);
    let n = reg.len();
    if n == 0 {
        return out;
    }
    let mut k = 0usize;
    while out.len() < SAMPLE_TARGET {
        if let Some(it) = reg.item(k % n) {
            out.push(Sample::new(it.name, it.major, it.minor, it.major, it.minor, it.sig));
        }
        k += 1;
    }
    out
}

/// 标准夹具。
pub fn standard_fixture() -> Result<CosignFixture, CosignFault> {
    let registry = standard_registry()?;
    let samples = standard_samples(&registry);
    Ok(CosignFixture { registry, samples })
}

/// 夹具不可构造时返回空夹具。
///
/// 第 1 条判据已单独报「夹具失败」，其余判据在空夹具上自然全红——每条再写
/// 一遍 `else` 兜底，只会把第一条的真因刷掉，诊断反而更差。
fn fixture_or_empty() -> CosignFixture {
    match standard_fixture() {
        Ok(f) => f,
        Err(_) => CosignFixture { registry: FreezeRegistry::new(0), samples: Vec::new() },
    }
}

/// 按接口名施加一次变异（返回新样本集，不改原夹具）。
///
/// 只保留一份「按名定位」实现：判据可以自由决定怎么变异，但若各自写一遍
/// 定位循环，五条判据就会长出五种定位语义。
fn mutated(f: &CosignFixture, iface: &str, apply: impl Fn(&mut Sample)) -> Vec<Sample> {
    let mut s = f.samples.clone();
    s.iter_mut().filter(|i| i.iface == iface).for_each(|i| apply(i));
    s
}

/// 造一张「只有结论、没有证据」的报告（检验核定闸门的负向）。
fn evidence_less(conclusion: Conclusion, with_condition: bool) -> CosignReport {
    let conditions = if with_condition { vec!["条件一"] } else { Vec::new() };
    CosignReport { verdicts: Vec::new(), faults: Vec::new(), conclusion, conditions, registry_fp: 1 }
}

/// 判据入口（UNX-F21589-J1 及其配套十三条）。
///
/// 判据常量独立写死：`20` / `4` / `0x91043` / `0x91044` 以字面量出现。
pub fn run_unx21589_upcosign_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21589");
    let f = fixture_or_empty();

    // 1) 契约与夹具先行（码值 + 三要素 + 定位文本 + 四组登记面）。
    let fl = CosignFault::new(CosignCode::SignatureMismatch, Group::Irq, "ai26.irq.route");
    cs.add(
        "f21589.contract_and_fixture",
        standard_fixture().is_ok()
            && f.registry.len() == 4
            && f.registry.groups_present() == 4
            && CosignCode::CosignOk.code() == 0x91041
            && CosignCode::VetoWithoutCause.code() == 0x91048
            && !CosignCode::MajorDrift.what().is_empty()
            && !CosignCode::MajorDrift.why().is_empty()
            && !CosignCode::MajorDrift.advice().is_empty()
            && fl.line().contains("g4-irq")
            && fl.as_code() == CosignCode::SignatureMismatch,
        "夹具不可构造，或登记面/码值/三要素/定位文本不达标",
    );

    // 2) 正向：20 例全命中 → 通过、零故障、指纹与登记表一致。
    let clean = match f.registry.review(&f.samples) {
        Ok(r) => r.verdicts.len() == 20 && r.fault_count() == 0 && r.conclusion == Conclusion::Pass
            && r.conditions.is_empty() && r.registry_fp == f.registry.fingerprint(),
        Err(_) => false,
    };
    cs.add("f21589.clean_pass", clean, "标准 20 例未通过或结论/条数不符");

    // 3) 样本数不贴边（19 / 21）→ 结构性拒绝。
    let short: Vec<Sample> = f.samples.iter().take(19).copied().collect();
    let mut long = f.samples.clone();
    if let Some(first) = f.samples.first() {
        long.push(*first);
    }
    let a = f.registry.review(&short).err().map(|e| e.code);
    let b = f.registry.review(&long).err().map(|e| e.code);
    cs.add("f21589.sample_count_enforced", a == Some(0x91046) && b == Some(0x91046), "19 或 21 例未被结构性拒绝");

    // 4) 四组缺一 → 拒；重复登记与非法组登记同样被拒。
    let mut reg = FreezeRegistry::new(1);
    let items = [
        FrozenIface::new(Group::Core, "probe.a", 1, 0, 11, 1, &["c1"]),
        FrozenIface::new(Group::Mem, "probe.b", 1, 0, 12, 1, &["c1"]),
        FrozenIface::new(Group::Sched, "probe.c", 1, 0, 13, 1, &["c1"]),
    ];
    let reg_ok = items.iter().all(|it| reg.register(*it).is_ok());
    let dup = reg.register(FrozenIface::new(Group::Core, "probe.a", 1, 0, 11, 1, &["c1"]));
    let stray = reg.register(FrozenIface::new(Group::Unregistered, "probe.d", 1, 0, 14, 1, &["c1"]));
    let three = standard_samples(&reg);
    cs.add(
        "f21589.group_coverage_enforced",
        reg_ok
            && dup.is_err()
            && stray.is_err()
            && three.len() == 20
            && reg.review(&three).err().map(|e| e.code) == Some(0x91046),
        "三组登记未判不完整，或重复/非法登记未被拒",
    );

    // 5) 主版本漂移 → 否决，码 0x91043，5 例全定位到 g2-mem。
    let major = mutated(&f, "ai26.mem.arena", |x| x.got_major += 1);
    let ok = match f.registry.review(&major) {
        Ok(r) if r.conclusion == Conclusion::Veto && r.fault_count() == 5 => {
            r.faults.iter().all(|x| x.code == 0x91043 && x.group == Group::Mem)
                && r.verdicts.iter().filter(|v| v.drift.is_blocking()).count() == 5
                && !Drift::MinorAhead.is_blocking()
        }
        _ => false,
    };
    cs.add("f21589.major_drift_veto", ok, "主版本漂移未否决或未定位到 g2-mem");

    // 6) 签名不符 → 否决，码 0x91044，定位到 g3-sched。
    let sig_bad = mutated(&f, "ai26.sched.tick", |x| x.got_sig = x.got_sig.wrapping_add(1));
    let ok = match f.registry.review(&sig_bad) {
        Ok(r) if r.conclusion == Conclusion::Veto && !r.faults.is_empty() => {
            r.faults.iter().all(|x| x.code == 0x91044 && x.group == Group::Sched)
        }
        _ => false,
    };
    cs.add("f21589.signature_mismatch", ok, "签名不符未否决或未定位到 g3-sched");

    // 7) 次版本增 → 允许，5 例被记录并转有条件通过。
    let ahead = mutated(&f, "ai26.irq.route", |x| x.got_minor += 1);
    let ok = match f.registry.review(&ahead) {
        Ok(r) => {
            r.conclusion == Conclusion::Conditional
                && r.fault_count() == 0
                && r.verdicts.iter().filter(|v| matches!(v.drift, Drift::MinorAhead)).count() == 5
                && r.conditions.len() == 1
                && r.export().contains("minor-ahead")
        }
        Err(_) => false,
    };
    cs.add("f21589.minor_bump_allowed", ok, "次版本增未被允许或未转有条件通过");

    // 8) 次版本减 → 否决，码 0x91045。
    let back = mutated(&f, "ai26.core.dispatch", |x| x.got_minor = x.want_minor - 1);
    let ok = match f.registry.review(&back) {
        Ok(r) => {
            r.conclusion == Conclusion::Veto
                && r.faults.iter().any(|x| x.code == 0x91045)
                && r.export().contains("minor-back")
        }
        Err(_) => false,
    };
    cs.add("f21589.minor_regress_veto", ok, "次版本回退未否决或码不符");

    // 9) 未登记接口 → 0x91042，该例标记 unknown、不参与版本比较。
    let mut ghost = f.samples.clone();
    if let Some(first) = ghost.first_mut() {
        first.iface = "ai26.ghost.nonexistent";
    }
    let ok = match f.registry.review(&ghost) {
        Ok(r) => {
            r.conclusion == Conclusion::Veto
                && r.faults.iter().any(|x| x.code == 0x91042 && x.iface == "ai26.ghost.nonexistent")
                && r.export().contains("unknown")
        }
        Err(_) => false,
    };
    cs.add("f21589.unknown_iface", ok, "未登记接口未被判为未知");

    // 10) 有条件通过但无条件项 = 自相矛盾，必拒（0x91047）；带条件则放行。
    let refused = ratify(&evidence_less(Conclusion::Conditional, false), Conclusion::Conditional)
        .err()
        .map(|e| e.code)
        == Some(0x91047);
    let allowed =
        ratify(&evidence_less(Conclusion::Conditional, true), Conclusion::Conditional) == Ok(Conclusion::Conditional);
    cs.add("f21589.condition_required", refused && allowed, "无条件的有条件通过未被拒，或带条件反而被拒");

    // 11) 无故障却否决 = 不可申诉，必拒（0x91048）。
    let no_cause = ratify(&evidence_less(Conclusion::Veto, false), Conclusion::Veto)
        .err()
        .map(|e| e.code);
    cs.add("f21589.veto_needs_cause", no_cause == Some(0x91048), "无故障引证的否决未被拒");

    // 12) 核定随证据走：有故障时请求通过被拒，且引证的正是证据码 0x91043。
    let ev = mutated(&f, "ai26.core.dispatch", |x| x.got_major += 2);
    let ok = f.registry.review(&ev).map_or(false, |r| {
        ratify(&r, Conclusion::Pass).err().map(|e| e.code) == Some(0x91043)
            && ratify(&r, Conclusion::Veto) == Ok(Conclusion::Veto)
    });
    cs.add("f21589.ratify_follows_evidence", ok, "核定未随证据走：请求通过未被拒");

    // 13) 确定性：两次独立装配的登记表指纹、报告指纹、导出文本逐位相同。
    let other = fixture_or_empty();
    let (same, text) = match (f.registry.review(&f.samples), other.registry.review(&other.samples)) {
        (Ok(x), Ok(y)) => (x.fingerprint() == y.fingerprint() && x.export() == y.export(), x.export()),
        _ => (false, String::new()),
    };
    let ok = same
        && f.registry.fingerprint() == other.registry.fingerprint()
        && text.contains("verdicts=20")
        && text.contains("conclusion=pass");
    cs.add("f21589.deterministic_export", ok, "两次复核不一致或导出字段缺失");

    cs
}
