//! CGPU-F1121 · H 域开工与显存预算池架构（CGPU-H 域 · 显存管理 · 批次 H01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1121`
//!
//! H 域开工：显存管理域（F1121-F1280，官方七主题：预算池/压缩/换页/碎片
//! 治理/泄漏防线/跨进程显存/显存遥测）；显存预算池架构——**显存分配的全面
//! 预算化**：所有显存分配走预算池（进程池/系统池/保留池三级）；C 域 F0338
//! 显存维度兑现声明（F0404 集成的池侧落地）；与 G 域联动（各厂商显存特性
//! 差异表消费——F0964/F0852）；显存哲学：显存有界是稳定的前提——无界即
//! 事故（复用 E04 哲学）。
//!
//! ## 要点一：七主题是闭集，域界是宣言
//!
//! H 域十一个批次的七主题以枚举冻结——加主题必须显性改本枚举并过判据，
//! 「顺手加一个」没有语法通道。域区间 F1121-F1280 常量登记，越界任务号
//! 不属于本域。
//!
//! ## 要点二：全面预算化 = 无预算即无分配
//!
//! 三级池（进程/系统/保留）是显存的唯一出口：`try_allocate` 超配额**显性
//! 拒绝**——不静默给满、不静默降质（降质是上层策略对拒绝的反应，池侧
//! 只说真话）。Σ三级配额 ≤ 总显存守恒在构造期一次性验证，守恒破坏的
//! 池组根本构造不出来。
//!
//! ## 要点三：账实守恒贯穿全生命周期
//!
//! used = Σ在途分配逐字节记账；释放即时归还（C 域回收联动条款的池侧
//! 落地）；重复释放拒绝；audit() 对账任何不齐立即立案——账本是显存
//! 遥测（主题七）与泄漏防线（主题五）的事实来源。
//!
//! ## 要点四：C 域 F0338 兑现是**声明+落地**双面
//!
//! [`MemDimVow`] 三条款闭集冻结（分配前预算校验/超用告警/回收归还），
//! 且每条都有可观察的池侧行为兑现点——只有声明没有落地是空头支票，
//! 只有落地没有声明是对接面失踪。
//!
//! ## 要点五：G 域差异表消费——Unknown 也是诚实
//!
//! 三厂商显存特性槽位一律 [`TriState`]：差异表（F0964/F0852）未冻结前
//! 消费面只能给 `Unknown` 保守缺省——把「不知道」说成「支持/不支持」
//! 都是替厂商撒谎。
//!
//! ## 要点六：诊断码独占 0x50xx 段
//!
//! 与 C 域 0x21 细分段、VE 域各段互不重叠；每码专属 reason；判据用
//! `!=` 防自判死。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、H 域开工宣言：七主题闭集与域界
// ---------------------------------------------------------------------------

/// H 域起始任务号（本单）。
pub const H_DOMAIN_FIRST: u32 = 1121;
/// H 域终止任务号（含）。
pub const H_DOMAIN_LAST: u32 = 1280;

/// 显存管理域七主题（官方闭集——加主题必须显性改枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicKind {
    /// 主题一：预算池（本单——三级池全面预算化）。
    BudgetPool,
    /// 主题二：显存压缩。
    Compression,
    /// 主题三：换页。
    Paging,
    /// 主题四：碎片治理。
    Fragmentation,
    /// 主题五：泄漏防线。
    LeakDefense,
    /// 主题六：跨进程显存。
    CrossProcess,
    /// 主题七：显存遥测。
    Telemetry,
}

/// 七主题总数（判据侧独立写死对拍）。
pub const TOPIC_COUNT: usize = 7;

impl TopicKind {
    /// 全部主题按官方顺序枚举。
    pub const ALL: [TopicKind; TOPIC_COUNT] = [
        TopicKind::BudgetPool,
        TopicKind::Compression,
        TopicKind::Paging,
        TopicKind::Fragmentation,
        TopicKind::LeakDefense,
        TopicKind::CrossProcess,
        TopicKind::Telemetry,
    ];

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            TopicKind::BudgetPool => "显存预算池",
            TopicKind::Compression => "显存压缩",
            TopicKind::Paging => "换页",
            TopicKind::Fragmentation => "碎片治理",
            TopicKind::LeakDefense => "泄漏防线",
            TopicKind::CrossProcess => "跨进程显存",
            TopicKind::Telemetry => "显存遥测",
        }
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// 二、显存哲学（三条冻结，复用 E04 哲学）
// ---------------------------------------------------------------------------

/// 显存哲学条款（字面量冻结——判据侧独立写死全文对拍，改动即红）。
pub const PHILOSOPHY_AXIOMS: [&str; 3] = [
    "显存有界是稳定的前提",
    "无界即事故（复用 E04 哲学）",
    "预算先行：无预算即无分配",
];

// ---------------------------------------------------------------------------
// 三、错误契约（独占 0x50xx 段）
// ---------------------------------------------------------------------------

/// H01 诊断码。独占 `0x50xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct H01Code(pub u16);

impl H01Code {
    /// 超池配额拒绝（无界即事故——显性拒绝，不静默给满）。
    pub const OVER_QUOTA: H01Code = H01Code(0x5001);
    /// 重复释放（同一句柄两次归还——账实守恒破坏）。
    pub const DOUBLE_FREE: H01Code = H01Code(0x5002);
    /// Σ配额超总显存（守恒破坏，池组构造期拒绝）。
    pub const CONSERVATION: H01Code = H01Code(0x5003);
    /// 未知句柄（释放/查询目标不存在）。
    pub const UNKNOWN_HANDLE: H01Code = H01Code(0x5004);
    /// 账实不符（audit 对账失败——泄漏防线立案信号）。
    pub const LEDGER_MISMATCH: H01Code = H01Code(0x5005);
    /// 非法参数（零字节申请等）。
    pub const BAD_INPUT: H01Code = H01Code(0x5006);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            H01Code::OVER_QUOTA => "显存申请超池配额：显性拒绝（无界即事故）".into(),
            H01Code::DOUBLE_FREE => "显存重复释放：同一句柄只能归还一次".into(),
            H01Code::CONSERVATION => "三级池配额之和超总显存：守恒破坏，拒绝构造".into(),
            H01Code::UNKNOWN_HANDLE => "未知显存句柄：目标不存在".into(),
            H01Code::LEDGER_MISMATCH => "显存账实不符：泄漏防线立案信号".into(),
            H01Code::BAD_INPUT => "显存申请参数非法（零字节等）".into(),
            H01Code(_) => "未知 H01 显存预算池诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、三级池闭集
// ---------------------------------------------------------------------------

/// 三级池（锚点原文：进程池/系统池/保留池）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolKind {
    /// 进程池：各引擎会话。
    Process,
    /// 系统池：系统 UI/合成器。
    System,
    /// 保留池：应急+驱动保留（不可为零——应急通道必须有底）。
    Reserved,
}

impl PoolKind {
    /// 全部三级。
    pub const ALL: [PoolKind; 3] = [PoolKind::Process, PoolKind::System, PoolKind::Reserved];

    /// 人话标签（用途语义随标签落地）。
    pub fn label(self) -> String {
        match self {
            PoolKind::Process => "进程池（各引擎会话）",
            PoolKind::System => "系统池（系统 UI/合成器）",
            PoolKind::Reserved => "保留池（应急+驱动保留）",
        }
        .to_string()
    }

    /// 池下标（O(1) 定位，显式映射）。
    pub const fn index(self) -> usize {
        match self {
            PoolKind::Process => 0,
            PoolKind::System => 1,
            PoolKind::Reserved => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、预算池（核心：try_allocate / release / water_level 全 O(1)）
// ---------------------------------------------------------------------------

/// 在途分配记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alloc {
    /// 分配句柄（池内单调递增，从 1 起——0 不是合法句柄）。
    pub handle: u64,
    /// 字节数。
    pub bytes: u64,
    /// 所有者模块标签（记账埋点——F1122 全分配点埋点的池侧雏形）。
    pub owner: u32,
}

/// 单分配池容量上界（在途记录条数——满即拒绝并如实，防无界增长）。
pub const MAX_ALLOCS: usize = 256;

/// 水位档位阈值（百分比）——判据侧独立写死对拍。
pub const WATER_HIGH_PCT: u64 = 80;
/// 水位危急档阈值（百分比）。
pub const WATER_CRITICAL_PCT: u64 = 95;

/// 水位档位（闭集四档，O(1) 判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterLevel {
    /// 空闲（used == 0）。
    Empty,
    /// 常规。
    Normal,
    /// 高水位（≥ [`WATER_HIGH_PCT`]）——告警联动源。
    High,
    /// 危急（≥ [`WATER_CRITICAL_PCT`]）——泄漏防线立案源。
    Critical,
}

/// 单级预算池。
#[derive(Debug, Clone)]
pub struct BudgetPool {
    kind: PoolKind,
    quota: u64,
    used: u64,
    peak: u64,
    next_handle: u64,
    allocs: Vec<Alloc>,
}

impl BudgetPool {
    /// 构造（配额可为 0——表示该池禁用分配；保留池例外由 PoolSet 把关）。
    pub fn new(kind: PoolKind, quota: u64) -> BudgetPool {
        BudgetPool {
            kind,
            quota,
            used: 0,
            peak: 0,
            next_handle: 1,
            allocs: Vec::new(),
        }
    }

    /// 配额。
    pub fn quota(&self) -> u64 {
        self.quota
    }

    /// 在用字节。
    pub fn used(&self) -> u64 {
        self.used
    }

    /// 峰值（遥测主题七的数据面）。
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// 在途分配数。
    pub fn alloc_count(&self) -> usize {
        self.allocs.len()
    }

    /// 申请（超配额显性拒绝；零字节拒绝；容量满拒绝——全 O(1)）。
    pub fn try_allocate(&mut self, owner: u32, bytes: u64) -> Result<u64, H01Code> {
        if bytes == 0 {
            return Err(H01Code::BAD_INPUT);
        }
        let new_used = self
            .used
            .checked_add(bytes)
            .ok_or(H01Code::OVER_QUOTA)?;
        if new_used > self.quota {
            return Err(H01Code::OVER_QUOTA); // 无界即事故：不给满不静默
        }
        if self.allocs.len() >= MAX_ALLOCS {
            return Err(H01Code::OVER_QUOTA); // 容量满也如实走同码，防句柄无界
        }
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1).max(1);
        self.allocs.push(Alloc { handle, bytes, owner });
        self.used = new_used;
        if self.used > self.peak {
            self.peak = self.used;
        }
        Ok(handle)
    }

    /// 释放（重复释放拒绝；即时归还——C 域回收联动条款池侧落地）。
    pub fn release(&mut self, handle: u64) -> Result<u64, H01Code> {
        let mut idx = None;
        for (i, a) in self.allocs.iter().enumerate() {
            if a.handle == handle {
                idx = Some(i);
                break;
            }
        }
        match idx {
            None => Err(H01Code::UNKNOWN_HANDLE),
            Some(i) => {
                let a = self.allocs[i];
                self.allocs.remove(i);
                self.used = self.used.saturating_sub(a.bytes);
                Ok(a.bytes)
            }
        }
    }

    /// 水位档位（O(1)：两次比较）。
    pub fn water_level(&self) -> WaterLevel {
        if self.used == 0 {
            return WaterLevel::Empty;
        }
        if self.quota == 0 {
            return WaterLevel::Critical; // 零配额池有占用即危急
        }
        // 千分比整数比较，避免除法余数边界歧义
        let permille = self.used.saturating_mul(1000) / self.quota;
        if permille >= WATER_CRITICAL_PCT * 10 {
            WaterLevel::Critical
        } else if permille >= WATER_HIGH_PCT * 10 {
            WaterLevel::High
        } else {
            WaterLevel::Normal
        }
    }

    /// 查询句柄（存在返回字节数）。
    pub fn lookup(&self, handle: u64) -> Option<u64> {
        self.allocs.iter().find(|a| a.handle == handle).map(|a| a.bytes)
    }
}

// ---------------------------------------------------------------------------
// 六、PoolSet：三级池总成（Σ配额 ≤ 总显存守恒 + 账实对账）
// ---------------------------------------------------------------------------

/// 三级池总成。
#[derive(Debug, Clone)]
pub struct PoolSet {
    total_vram: u64,
    pools: [BudgetPool; 3],
}

impl PoolSet {
    /// 构造期守恒验证：Σ配额 ≤ 总显存（checked，防回绕）；保留池必须
    /// 有底（应急+驱动保留不可为零）——破坏即拒绝构造。
    pub fn new(total_vram: u64, quotas: [u64; 3]) -> Result<PoolSet, H01Code> {
        let mut sum: u64 = 0;
        for q in quotas {
            sum = sum.checked_add(q).ok_or(H01Code::CONSERVATION)?;
        }
        if sum > total_vram {
            return Err(H01Code::CONSERVATION);
        }
        if quotas[PoolKind::Reserved.index()] == 0 {
            return Err(H01Code::CONSERVATION); // 应急通道必须有底
        }
        Ok(PoolSet {
            total_vram,
            pools: [
                BudgetPool::new(PoolKind::Process, quotas[0]),
                BudgetPool::new(PoolKind::System, quotas[1]),
                BudgetPool::new(PoolKind::Reserved, quotas[2]),
            ],
        })
    }

    /// 总显存。
    pub fn total_vram(&self) -> u64 {
        self.total_vram
    }

    /// 池访问（O(1) 下标定位）。
    pub fn pool(&self, kind: PoolKind) -> &BudgetPool {
        &self.pools[kind.index()]
    }

    /// 池访问（可变）。
    pub fn pool_mut(&mut self, kind: PoolKind) -> &mut BudgetPool {
        &mut self.pools[kind.index()]
    }

    /// 申请（直接按池种类路由）。
    pub fn allocate(&mut self, kind: PoolKind, owner: u32, bytes: u64) -> Result<u64, H01Code> {
        self.pools[kind.index()].try_allocate(owner, bytes)
    }

    /// 释放（池侧即时归还）。
    pub fn release(&mut self, kind: PoolKind, handle: u64) -> Result<u64, H01Code> {
        self.pools[kind.index()].release(handle)
    }

    /// 全局在用 = Σ池.used（checked）。
    pub fn total_used(&self) -> Result<u64, H01Code> {
        let mut sum = 0u64;
        for p in &self.pools {
            sum = sum.checked_add(p.used()).ok_or(H01Code::LEDGER_MISMATCH)?;
        }
        Ok(sum)
    }

    /// 账实对账：Σ池.used ≤ total_vram 且每池 used = Σ其分配字节数。
    /// 任何不齐立案 [`H01Code::LEDGER_MISMATCH`]（泄漏防线事实来源）。
    pub fn audit(&self) -> Result<(), H01Code> {
        for p in &self.pools {
            let mut sum: u64 = 0;
            for a in p.allocs.iter() {
                sum = sum.checked_add(a.bytes).ok_or(H01Code::LEDGER_MISMATCH)?;
            }
            if sum != p.used() {
                return Err(H01Code::LEDGER_MISMATCH);
            }
        }
        let used = self.total_used()?;
        if used > self.total_vram {
            return Err(H01Code::LEDGER_MISMATCH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、C 域 F0338 显存维度兑现声明（F0404 集成的池侧落地）
// ---------------------------------------------------------------------------

/// C 域兑现条款闭集（显存维度三条款——F0338 对接面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VowClause {
    /// 条款一：显存分配前先过预算校验——超预算拒绝或降质（池侧落地 =
    /// [`PoolSet::allocate`] 的 OVER_QUOTA 显性拒绝路径）。
    BudgetCheckBeforeAlloc,
    /// 条款二：显存超用异常→预算告警（池侧落地 = [`BudgetPool::water_level`]
    /// 的 High/Critical 档位联动源）。
    LeakAlarmOnOveruse,
    /// 条款三：显存回收→预算即时归还（池侧落地 = [`BudgetPool::release`]
    /// 的 used 即时回退）。
    ReturnOnReclaim,
}

impl VowClause {
    /// 全部条款。
    pub const ALL: [VowClause; 3] = [
        VowClause::BudgetCheckBeforeAlloc,
        VowClause::LeakAlarmOnOveruse,
        VowClause::ReturnOnReclaim,
    ];

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            VowClause::BudgetCheckBeforeAlloc => "分配前预算校验（超预算拒绝或降质）",
            VowClause::LeakAlarmOnOveruse => "超用异常→预算告警",
            VowClause::ReturnOnReclaim => "回收→预算即时归还",
        }
        .to_string()
    }
}

/// C 域 F0338 显存维度兑现声明（冻结版本指纹可对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemDimVow {
    /// 契约版本（条款集变更即递增）。
    pub version: u32,
}

/// C 域契约当前版本。
pub const VOW_VERSION: u32 = 1;

impl MemDimVow {
    /// 冻结声明。
    pub const fn frozen() -> MemDimVow {
        MemDimVow { version: VOW_VERSION }
    }

    /// 指纹（FNV-1a over 条款标签——判据侧独立实现同口径对拍）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in VowClause::ALL {
            for b in c.label().as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= 0xFF;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^ (self.version.wrapping_mul(0x9E37_79B9))
    }
}

// ---------------------------------------------------------------------------
// 八、G 域联动：厂商显存特性差异消费面（F0964/F0852 前向声明）
// ---------------------------------------------------------------------------

/// 厂商闭集（G 域三方）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorKind {
    /// NVIDIA。
    Nvidia,
    /// AMD。
    Amd,
    /// Intel。
    Intel,
}

/// 三态：支持/不支持/未知——差异表未冻结前一律 Unknown（不假宣称）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    /// 表已声明支持。
    Supported,
    /// 表已声明不支持。
    Unsupported,
    /// 表未冻结或未登记——保守未知。
    Unknown,
}

/// 厂商显存特性槽（三槽固定：可_resize_BAR/压缩/换页粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorMemTrait {
    /// Resizable BAR。
    pub resizable_bar: TriState,
    /// 显存压缩（主题二上游）。
    pub compress: TriState,
    /// 换页粒度（字节；None = 未知）。
    pub page_granularity: Option<u32>,
}

/// 差异表消费入口（F0964/F0852 前向声明：表未冻结前全部保守 Unknown）。
///
/// 这是消费面不是推测面：接上真差异表时只改这里一处，全部槽位来自表、
/// 表里没有的保持 Unknown——把「不知道」说成确定值是替厂商撒谎。
pub fn vendor_trait(_v: VendorKind) -> VendorMemTrait {
    VendorMemTrait {
        resizable_bar: TriState::Unknown,
        compress: TriState::Unknown,
        page_granularity: None,
    }
}

// ---------------------------------------------------------------------------
// 九、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 三级池守恒构造与超额拒绝() {
        let mut ps = PoolSet::new(1000, [600, 300, 100]).unwrap();
        assert!(ps.allocate(PoolKind::Process, 1, 600).is_ok());
        assert_eq!(
            ps.allocate(PoolKind::Process, 1, 1),
            Err(H01Code::OVER_QUOTA)
        );
        assert!(PoolSet::new(1000, [700, 300, 100]).is_err());
    }

    #[test]
    fn 保留池不可为零() {
        assert!(PoolSet::new(1000, [900, 100, 0]).is_err());
    }

    #[test]
    fn 释放即时归还且重复释放拒绝() {
        let mut ps = PoolSet::new(1000, [600, 300, 100]).unwrap();
        let h = ps.allocate(PoolKind::Process, 7, 300).unwrap();
        assert_eq!(ps.release(PoolKind::Process, h), Ok(300));
        assert_eq!(ps.release(PoolKind::Process, h), Err(H01Code::UNKNOWN_HANDLE));
        assert_eq!(ps.pool(PoolKind::Process).used(), 0);
    }

    #[test]
    fn 水位档位边界() {
        let mut p = BudgetPool::new(PoolKind::Process, 1000);
        assert_eq!(p.water_level(), WaterLevel::Empty);
        let _ = p.try_allocate(1, 799);
        assert_eq!(p.water_level(), WaterLevel::Normal);
        let _ = p.release(1);
        let _ = p.try_allocate(1, 800);
        assert_eq!(p.water_level(), WaterLevel::High);
        let _ = p.release(2);
        let _ = p.try_allocate(1, 950);
        assert_eq!(p.water_level(), WaterLevel::Critical);
    }

    #[test]
    fn audit账实守恒() {
        let mut ps = PoolSet::new(1000, [600, 300, 100]).unwrap();
        let a = ps.allocate(PoolKind::System, 2, 200).unwrap();
        let _ = ps.allocate(PoolKind::Reserved, 2, 50).unwrap();
        assert!(ps.audit().is_ok());
        let _ = ps.release(PoolKind::System, a);
        assert!(ps.audit().is_ok());
    }

    #[test]
    fn 差异表未冻结前一律Unknown() {
        for v in [VendorKind::Nvidia, VendorKind::Amd, VendorKind::Intel] {
            let t = vendor_trait(v);
            assert_eq!(t.resizable_bar, TriState::Unknown);
            assert_eq!(t.compress, TriState::Unknown);
            assert_eq!(t.page_granularity, None);
        }
    }
}
