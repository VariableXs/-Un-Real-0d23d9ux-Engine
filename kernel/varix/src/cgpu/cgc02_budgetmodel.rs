//! CGPU-F0322 · 帧预算模型（CGPU-C 域 · 帧预算仲裁域 · C02 组 · 目标 420 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0322`
//!
//! **判据（锚点原文）**：四池、三级、双表述、守恒校验、判据。
//!
//! **职责定位（锚点原文）**：12.5ms 帧预算分解（呈现合成/界面响应/后台
//! 任务/网络与余量四池——池比例可配但默认保交互）；预算层级（帧预算→
//! 池预算→账户预算三级）；预算双表述（微秒精度+占比）；模型数学化
//! （预算守恒：Σ池=帧预算、Σ账户≤池——超载检测器）。
//!
//! ## 一、四池分解（判据一）
//!
//! 帧预算四池（[`PoolKind`] 闭集）：呈现合成（交换链/合成器——错过
//! 即掉帧的硬实时面）/界面响应（输入到像素的交互链路——手感面）/
//! 后台任务（能延迟的批处理——可挤压面）/网络与余量（抖动吸收+未来
//! 余量——保险面）。默认比例写死（[`DEFAULT_POOL_RATIO_PPM`]：呈现
//! 400000/界面 250000/后台 200000/网络余量 150000，百万分比口径）；
//! **比例可配但默认保交互**——配置面校验四池和恰为 1_000_000，配置
//! 错误显性拒不静默归一（静默归一会把用户的配置意图改写成另一套
//! 预算而无人知晓）。
//!
//! ## 二、三级层级（判据二）
//!
//! 帧预算→池预算→账户预算三级：帧预算（12.5ms=12500μs，80Hz 承诺
//! 的精确化）；池预算（帧×比例）；账户预算（池内申请——F0323+ 的
//! 申请面按账户扣减）。层级是单向分解：上级定了下级才有界，下级
//! 永远不能改写上级（下级超载向上暴露，不向上挪用）。
//!
//! ## 三、双表述（判据三）
//!
//! 每级预算两种表述：微秒精度（u64，仲裁比较的唯一权威——比较永远
//! 用 μs 不用占比，浮点占比只作呈现）+ 占比（ppm 百万分比，人读与
//! 配置面）。双表述互为换算（[`BudgetQty`]），换算方向单一：ppm→μs
//! 由模型算，μs→ppm 只在报告面出现（禁止拿呈现占比反推预算——
//! 舍入会吃掉微秒）。
//!
//! ## 四、守恒校验与超载检测（判据四）
//!
//! 数学化两条守恒式：Σ池=帧预算（分解面——ppm 和恰 1e6 闭式保证，
//! μs 面按比例分摊后**余量全部归网络池**——余量不给大池兜底而是
//! 进保险面，分摊确定性）；Σ账户≤池（消费面——超载显性检出：
//! [`BudgetModel::charge`] 拒绝超池申请并返回缺额，不静默借贷）。
//! 超载检测器（[`overload_scan`]）：遍历全部池返回超载清单——
//! 检测是查询不是修复，修复是仲裁（F0323+）的职责。
//!
//! **对接**：F0321（C 域开工——本模块是其预算模型面）；F0323+
//! （仲裁器——超载的处置面）；F0482/F0483（计时与账本——实际耗时
//! 的回流对照）。零 panic 面、零 IO、零墙钟、no_std、零浮点。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BUDGET_MODEL_VERSION: &str = "C02-budgetmodel-v1";

/// 帧预算（μs；80Hz=12500——12.5ms 承诺的精确化）。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 百万分比满额。
pub const PPM: u64 = 1_000_000;

/// 默认池比例（ppm；呈现/界面/后台/网络余量——默认保交互）。
pub const DEFAULT_POOL_RATIO_PPM: [u64; 4] = [400_000, 250_000, 200_000, 150_000];

// ---------------------------------------------------------------------------
// 二、诊断码（0x35 段独占：C02 预算模型面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x35 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 池比例配置错误（Σppm ≠ 1_000_000）。
    BadRatioSum,
    /// 账户申请超池余额（缺额在错误载荷外另行返回）。
    PoolOverdrawn,
    /// 池序越界（闭集外下标——防御性）。
    PoolIndexOutOfRange,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::BadRatioSum => "池比例和≠1000000ppm：配置显性拒不静默归一，修正四池比例后再建模",
            XcCode::PoolOverdrawn => "账户申请超池余额：本模型只检出不借贷，超载交仲裁面（F0323+）处置",
            XcCode::PoolIndexOutOfRange => "池序越界：四池闭集 0..3，建议按 PoolKind::ordinal 取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、四池闭集与预算量（判据一、三）
// ---------------------------------------------------------------------------

/// 预算池闭集（四元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolKind {
    /// 呈现合成（硬实时——错过即掉帧）。
    Present,
    /// 界面响应（交互链路——手感）。
    UiResponse,
    /// 后台任务（可延迟批处理）。
    Background,
    /// 网络与余量（抖动吸收+保险）。
    NetReserve,
}

impl PoolKind {
    /// 序号（0-3）。
    pub const fn ordinal(self) -> usize {
        match self {
            PoolKind::Present => 0,
            PoolKind::UiResponse => 1,
            PoolKind::Background => 2,
            PoolKind::NetReserve => 3,
        }
    }

    /// 池名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            PoolKind::Present => "呈现合成",
            PoolKind::UiResponse => "界面响应",
            PoolKind::Background => "后台任务",
            PoolKind::NetReserve => "网络与余量",
        }
    }
}

/// 预算量双表述（判据三：μs 是权威、ppm 是呈现；换算单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetQty {
    /// 微秒精度（仲裁比较的唯一权威）。
    pub us: u64,
    /// 占比（ppm；呈现面）。
    pub ppm: u64,
}

impl BudgetQty {
    /// ppm → 双表述（μs = frame×ppm/1e6 向下取整——分摊误差不许
    /// 超发，宁可少给）。
    pub const fn from_ppm(frame_us: u64, ppm: u64) -> BudgetQty {
        BudgetQty {
            us: frame_us.saturating_mul(ppm) / PPM,
            ppm,
        }
    }

    /// μs → 呈现占比（只在报告面用；此方向的结果禁入仲裁）。
    pub const fn to_ppm(self, frame_us: u64) -> u64 {
        if frame_us == 0 {
            return 0;
        }
        self.us.saturating_mul(PPM) / frame_us
    }
}

// ---------------------------------------------------------------------------
// 四、三级层级与守恒分解（判据二、四）
// ---------------------------------------------------------------------------

/// 池账（一级池的 μs 预算 + 已扣减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolLedger {
    /// 池种类。
    pub kind: PoolKind,
    /// 池预算（μs；frame×ppm 向下取整）。
    pub budget_us: u64,
    /// 配置占比（ppm）。
    pub ratio_ppm: u64,
    /// 已扣减（μs）。
    pub charged_us: u64,
}

impl PoolLedger {
    /// 余额。
    pub const fn remaining(&self) -> u64 {
        self.budget_us.saturating_sub(self.charged_us)
    }

    /// 超载判定。
    pub const fn overloaded(&self) -> bool {
        self.charged_us > self.budget_us
    }
}

/// 帧预算模型（三级分解 + 消费 + 守恒校验的承载）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetModel {
    /// 帧预算（μs）。
    pub frame_us: u64,
    /// 四池账（下标=PoolKind::ordinal）。
    pools: [PoolLedger; 4],
}

impl BudgetModel {
    /// 按比例建模型（比例和恰 1e6 才受理；μs 分摊余量归网络池——
    /// 向下取整的残余不给大池兜底而是进保险面，分摊确定性）。
    pub fn from_ratio(frame_us: u64, ratio: [u64; 4]) -> Result<BudgetModel, XcCode> {
        let sum = ratio[0]
            .saturating_add(ratio[1])
            .saturating_add(ratio[2])
            .saturating_add(ratio[3]);
        if sum != PPM {
            return Err(XcCode::BadRatioSum);
        }
        let mut allocated = 0u64;
        let mut i = 0usize;
        while i < 4 {
            allocated = allocated.saturating_add(frame_us.saturating_mul(ratio[i]) / PPM);
            i += 1;
        }
        // 前三池按比例，网络池 = 帧预算 − 前三池实分（吸收取整残差）。
        let p0 = frame_us.saturating_mul(ratio[0]) / PPM;
        let p1 = frame_us.saturating_mul(ratio[1]) / PPM;
        let p2 = frame_us.saturating_mul(ratio[2]) / PPM;
        let p3 = frame_us.saturating_sub(p0.saturating_add(p1).saturating_add(p2));
        let _ = allocated;
        Ok(BudgetModel {
            frame_us,
            pools: [
                PoolLedger { kind: PoolKind::Present, budget_us: p0, ratio_ppm: ratio[0], charged_us: 0 },
                PoolLedger { kind: PoolKind::UiResponse, budget_us: p1, ratio_ppm: ratio[1], charged_us: 0 },
                PoolLedger { kind: PoolKind::Background, budget_us: p2, ratio_ppm: ratio[2], charged_us: 0 },
                PoolLedger { kind: PoolKind::NetReserve, budget_us: p3, ratio_ppm: ratio[3], charged_us: 0 },
            ],
        })
    }

    /// 默认比例建模型（12.5ms × 默认四池）。
    pub fn with_defaults() -> Result<BudgetModel, XcCode> {
        BudgetModel::from_ratio(FRAME_BUDGET_US, DEFAULT_POOL_RATIO_PPM)
    }

    /// 账户申请扣减（判据四的 Σ账户≤池：超池显性拒并返回缺额——
    /// 不静默借贷不挤压他池；他池让渡是仲裁面的显性动作）。
    pub fn charge(&mut self, pool: PoolKind, account_id: u32, want_us: u64)
        -> Result<(BudgetQty, u32), (XcCode, u64, u32)>
    {
        let o = pool.ordinal();
        let rem = match self.pools.get(o) {
            Some(p) => p.remaining(),
            None => return Err((XcCode::PoolIndexOutOfRange, 0, account_id)),
        };
        if want_us > rem {
            return Err((XcCode::PoolOverdrawn, want_us.saturating_sub(rem), account_id));
        }
        match self.pools.get_mut(o) {
            Some(p) => p.charged_us = p.charged_us.saturating_add(want_us),
            None => return Err((XcCode::PoolIndexOutOfRange, 0, account_id)),
        }
        let ppm = BudgetQty { us: want_us, ppm: 0 }.to_ppm(self.frame_us);
        Ok((BudgetQty { us: want_us, ppm }, account_id))
    }

    /// 池账只读（判据/仲裁消费）。
    pub fn pool(&self, pool: PoolKind) -> Result<&PoolLedger, XcCode> {
        self.pools.get(pool.ordinal()).ok_or(XcCode::PoolIndexOutOfRange)
    }

    /// 分解守恒校验（判据四第一条：Σ池=帧预算——建模型后的自证）。
    pub const fn decomposition_conserves(&self) -> bool {
        let sum = self.pools[0]
            .budget_us
            .saturating_add(self.pools[1].budget_us)
            .saturating_add(self.pools[2].budget_us)
            .saturating_add(self.pools[3].budget_us);
        sum == self.frame_us
    }

    /// 超载扫描（判据四第二条的消费面查询：返回超载池清单——
    /// 检测是查询不是修复，修复是仲裁面的职责）。
    pub fn overload_scan(&self) -> Vec<PoolKind> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < 4 {
            if let Some(p) = self.pools.get(i) {
                if p.overloaded() {
                    out.push(p.kind);
                }
            }
            i += 1;
        }
        out
    }

    /// 模型摘要（人读；双表述呈现）。
    pub fn summary(&self) -> String {
        let mut s = String::from("帧预算 ");
        s.push_str(&self.frame_us.to_string());
        s.push_str("μs｜");
        let mut i = 0usize;
        while i < 4 {
            if let Some(p) = self.pools.get(i) {
                s.push_str(p.kind.zh());
                s.push(' ');
                s.push_str(&p.budget_us.to_string());
                s.push_str("μs(");
                s.push_str(&p.ratio_ppm.to_string());
                s.push_str("ppm) 余 ");
                s.push_str(&p.remaining().to_string());
                if i < 3 {
                    s.push_str("｜");
                }
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、账户账与帧收口（三级层级的账户面深化）
// ---------------------------------------------------------------------------

/// 账户行（池内账户：account_id + 扣减账——三级的最底层）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRow {
    /// 账户 id（消费方分配）。
    pub account_id: u32,
    /// 归属池。
    pub pool: PoolKind,
    /// 累计扣减（μs）。
    pub charged_us: u64,
    /// 本帧封顶（μs；0=未设顶）。
    pub cap_us: u64,
}

impl AccountRow {
    /// 本帧还可申请（cap 为 0 视为无独立封顶——受池余额约束）。
    pub const fn room(&self) -> Option<u64> {
        if self.cap_us == 0 {
            None
        } else {
            Some(self.cap_us.saturating_sub(self.charged_us))
        }
    }
}

/// 账户账（挂在模型下：申请即记账，帧结束统一退账）。
#[derive(Clone, Debug, Default)]
pub struct AccountBook {
    rows: Vec<AccountRow>,
}

impl AccountBook {
    /// 开户（同 id 重复开户幂等拒——一账户一行）。
    pub fn open(&mut self, account_id: u32, pool: PoolKind, cap_us: u64) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.account_id == account_id {
                    return Err(XcCode::PoolIndexOutOfRange); // 语义：键冲突按配置错拒
                }
            }
            i += 1;
        }
        self.rows.push(AccountRow {
            account_id,
            pool,
            charged_us: 0,
            cap_us,
        });
        Ok(())
    }

    /// 记账（申请成功后同步账户账；cap 校验在前）。
    pub fn record(&mut self, account_id: u32, us: u64) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get_mut(i) {
                if r.account_id == account_id {
                    if let Some(room) = r.room() {
                        if us > room {
                            return Err(XcCode::PoolOverdrawn);
                        }
                    }
                    r.charged_us = r.charged_us.saturating_add(us);
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(XcCode::PoolIndexOutOfRange)
    }

    /// 帧收口退账（判据四的守恒恢复面：Σ账户清零、池 charged 清零——
    /// 退账返回总退额，供账本对账）。
    pub fn close_frame(&mut self, m: &mut BudgetModel) -> u64 {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get_mut(i) {
                total = total.saturating_add(r.charged_us);
                r.charged_us = 0;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < 4 {
            if let Some(p) = m.pools.get_mut(j) {
                p.charged_us = 0;
            }
            j += 1;
        }
        total
    }

    /// 账户数。
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 实际耗时对照行（判据四的对照面输入：池实际耗时注入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActualRow {
    /// 池。
    pub pool: PoolKind,
    /// 实际耗时（μs；F0482 计时面注入）。
    pub actual_us: u64,
}

/// 预算-实际对照（超耗清单：actual > budget 的池点名——对照是
/// 查询，处置是仲裁面）。
pub fn reconcile(model: &BudgetModel, actuals: &[ActualRow]) -> Vec<PoolKind> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < actuals.len() {
        if let Some(a) = actuals.get(i) {
            if let Ok(p) = model.pool(a.pool) {
                if a.actual_us > p.budget_us {
                    out.push(a.pool);
                }
            }
        }
        i += 1;
    }
    out
}

/// 比例配置校验报告（判据一的配置面深化：给人读的配置体检——
/// 和恰 1e6 之外再查交互保底：呈现+界面 ≥ 500000ppm（默认保交互
/// 的机器可检表述））。
pub fn ratio_health(ratio: [u64; 4]) -> String {
    let sum = ratio[0]
        .saturating_add(ratio[1])
        .saturating_add(ratio[2])
        .saturating_add(ratio[3]);
    let mut s = String::from("比例体检：和 ");
    s.push_str(&sum.to_string());
    if sum != PPM {
        s.push_str("≠1e6 显性拒");
        return s;
    }
    let interactive = ratio[0].saturating_add(ratio[1]);
    s.push_str("，交互池 ");
    s.push_str(&interactive.to_string());
    if interactive < 500_000 {
        s.push_str(" < 保底线 500000——默认保交互被配置侵蚀，建议复核");
    } else {
        s.push_str("——交互保底达标");
    }
    s
}
