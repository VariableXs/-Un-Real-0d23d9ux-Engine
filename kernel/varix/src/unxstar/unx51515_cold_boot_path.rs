//! UNX-F51515 U 盘即插即用冷启动路径（枚举→卷层挂载端到端账） · 0xC03C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51516-J1 三错误注入 ×3×10 恢复全通
//!
//! 设计要点：
//!   ① 冷启动按六级阶段梯建模：上电→总线复位→设备枚举→LUN 就绪→卷探针
//!      →卷挂载，逐格推进；倒退（StageRegression）/跳格（StageSkip）/
//!      挂载后再走段（PathAlreadyMounted）显性拒。
//!   ② 错误注入按「咬点」归属阶段：描述符短读咬枚举段、扇区零尺寸咬
//!      LUN 就绪段、卷签名不符咬卷探针段；故障只在其咬点阶段生效，
//!      咬满预定次数后自动放行，放行即恢复（×3 次尝试内）。
//!   ③ 端到端账只增不减：每段成功推进落一条 StageRec（boot_id/阶段/
//!      逻辑 tick/该段重试次数）；重启 abort 只递增 boot_id（单调），
//!      不清账——跨重启全程可回放。
//!   ④ 判定次序：推进侧：挂载完成守卫 → 阶段单调守卫 → 账容量守卫 →
//!      咬合 → 落账；注入侧：挂载守卫 → 故障互斥 → 咬数校验（零拒/上限拒）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类错误 ×10 轮
//!      全部在 ×3 次尝试内恢复，恢复数恒等于检出数。
//!   ⑥ 零浮点、零 IO、零墙钟：时序用单调逻辑 tick（checked_add 防回绕）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC03C0;

/// 每个故障允许的咬合次数上限（咬 2 次、第 3 次尝试放行 = ×3 恢复；冻结）。
pub const MAX_BITES: u32 = 2;
/// 端到端账容量（冻结）。
pub const ACCOUNT_CAP: usize = 64;
/// 冻结扇区尺寸（字节）。
pub const SECTOR_SIZE_BYTES: u32 = 512;
/// 每类错误的判据轮数（冻结）。
pub const ROUNDS_PER_FAULT: u32 = 10;

/// 诊断码闭集（冷启动路径域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 注入咬次数为零
    ZeroBites,
    /// 注入咬次数超上限
    BitesTooMany,
    /// 已有活跃故障未恢复，拒绝叠加注入
    FaultBusy,
    /// 描述符短读（枚举段咬点）
    DescriptorShort,
    /// 卷探针签名不符（卷探针段咬点）
    ProbeSignature,
    /// 扇区尺寸为零（LUN 就绪段咬点）
    SectorZero,
    /// 阶段倒退
    StageRegression,
    /// 阶段跳格
    StageSkip,
    /// 路径已挂载仍继续走段
    PathAlreadyMounted,
    /// 未挂载却请求重启
    AbortNotMounted,
    /// 端到端账已满
    AccountFull,
    /// 逻辑 tick 回绕溢出
    TickOverflow,
    /// 账面守恒失败
    LedgerMismatch,
}

impl BootCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BootCode::Ok => "冷启动推进成功",
            BootCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            BootCode::ZeroBites => "咬数为零：注入至少咬 1 次或不注入",
            BootCode::BitesTooMany => "咬数超限：×3 尝试内必须放行恢复",
            BootCode::FaultBusy => "故障互斥：等当前故障恢复后再注入",
            BootCode::DescriptorShort => "描述符短读：枚举段重试，核对描述符长度",
            BootCode::ProbeSignature => "卷签名不符：卷探针段重试，核对卷头签名",
            BootCode::SectorZero => "扇区尺寸为零：LUN 段重试，核对几何参数",
            BootCode::StageRegression => "阶段倒退：冷启动阶段只能前进不能回退",
            BootCode::StageSkip => "阶段跳格：必须逐格推进冷启动阶段",
            BootCode::PathAlreadyMounted => "已挂载：路径收口，重启后才能再走段",
            BootCode::AbortNotMounted => "未挂载：只有挂载后才能重启",
            BootCode::AccountFull => "端到端账满：账只增不减，导出后清账再演练",
            BootCode::TickOverflow => "逻辑 tick 溢出：时序账回绕，检查推进量",
            BootCode::LedgerMismatch => "账面不符：注入/检出/恢复守恒被破坏",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [BootCode; 15] = [
    BootCode::Ok,
    BootCode::SegBad,
    BootCode::ZeroBites,
    BootCode::BitesTooMany,
    BootCode::FaultBusy,
    BootCode::DescriptorShort,
    BootCode::ProbeSignature,
    BootCode::SectorZero,
    BootCode::StageRegression,
    BootCode::StageSkip,
    BootCode::PathAlreadyMounted,
    BootCode::AbortNotMounted,
    BootCode::AccountFull,
    BootCode::TickOverflow,
    BootCode::LedgerMismatch,
];

/// 冷启动阶段梯（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 上电
    PowerOn = 0,
    /// 总线复位
    BusReset = 1,
    /// 设备枚举完成
    Enumerated = 2,
    /// LUN 就绪
    LunReady = 3,
    /// 卷探针通过
    VolumeProbed = 4,
    /// 卷层挂载完成（端到端收口）
    Mounted = 5,
}

impl BootStage {
    pub fn order(self) -> u32 {
        self as u32
    }
}

/// 注入错误类别（每类归属一个咬点阶段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 描述符短读（咬枚举段）
    DescriptorShort,
    /// 卷签名不符（咬卷探针段）
    ProbeSignature,
    /// 扇区零尺寸（咬 LUN 就绪段）
    SectorZero,
}

impl FaultKind {
    /// 咬点阶段：故障只在该阶段的推进上生效。
    pub fn bite_stage(self) -> BootStage {
        match self {
            FaultKind::DescriptorShort => BootStage::Enumerated,
            FaultKind::ProbeSignature => BootStage::VolumeProbed,
            FaultKind::SectorZero => BootStage::LunReady,
        }
    }

    /// 对应诊断码。
    pub fn code(self) -> BootCode {
        match self {
            FaultKind::DescriptorShort => BootCode::DescriptorShort,
            FaultKind::ProbeSignature => BootCode::ProbeSignature,
            FaultKind::SectorZero => BootCode::SectorZero,
        }
    }
}

/// 活跃故障状态（私有）：剩余咬合次数 + 是否已计检出。
#[derive(Clone, Copy, Debug)]
struct FaultState {
    kind: FaultKind,
    left: u32,
    counted: bool,
}

/// 单段端到端账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageRec {
    /// 所属启动轮次（重启单调递增）
    pub boot_id: u32,
    pub stage: BootStage,
    /// 推进落账时的逻辑 tick
    pub tick: u32,
    /// 该段的重试次数（咬合次数）
    pub retries: u32,
}

/// 冷启动端到端引擎。
pub struct ColdBootEngine {
    boot_id: u32,
    stage: BootStage,
    tick: u32,
    fault: Option<FaultState>,
    stage_retries: u32,
    account: Vec<StageRec>,
    injected: u64,
    detected: u64,
    recovered: u64,
    mounted_count: u64,
}

impl ColdBootEngine {
    /// 新引擎：boot_id 从 1 起，停在上电段。
    pub fn new() -> ColdBootEngine {
        ColdBootEngine {
            boot_id: 1,
            stage: BootStage::PowerOn,
            tick: 0,
            fault: None,
            stage_retries: 0,
            account: Vec::new(),
            injected: 0,
            detected: 0,
            recovered: 0,
            mounted_count: 0,
        }
    }

    pub fn stage(&self) -> BootStage {
        self.stage
    }

    pub fn boot_id(&self) -> u32 {
        self.boot_id
    }

    pub fn mounted(&self) -> bool {
        self.stage == BootStage::Mounted
    }

    pub fn mounted_count(&self) -> u64 {
        self.mounted_count
    }

    pub fn account_len(&self) -> usize {
        self.account.len()
    }

    pub fn account(&self) -> &[StageRec] {
        &self.account
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn recovered(&self) -> u64 {
        self.recovered
    }

    pub fn escaped(&self) -> u64 {
        self.injected.saturating_sub(self.detected)
    }

    /// 守恒：注入 = 检出 + 逃逸，且每个检出故障都恢复（恢复 = 检出）。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected == self.recovered && self.detected > 0
    }

    /// 注入一个故障（咬点归属其阶段）。判定次序：挂载守卫 → 故障互斥 →
    /// 咬数校验（零拒 → 上限拒）。
    pub fn inject(&mut self, kind: FaultKind, bites: u32) -> Result<(), BootCode> {
        if self.stage == BootStage::Mounted {
            return Err(BootCode::PathAlreadyMounted);
        }
        if self.fault.is_some() {
            return Err(BootCode::FaultBusy);
        }
        if bites == 0 {
            return Err(BootCode::ZeroBites);
        }
        if bites > MAX_BITES {
            return Err(BootCode::BitesTooMany);
        }
        self.fault = Some(FaultState { kind, left: bites, counted: false });
        self.injected += 1;
        Ok(())
    }

    /// 下一阶段（挂载段之后为 None）。
    pub fn next_stage(&self) -> Option<BootStage> {
        match self.stage {
            BootStage::PowerOn => Some(BootStage::BusReset),
            BootStage::BusReset => Some(BootStage::Enumerated),
            BootStage::Enumerated => Some(BootStage::LunReady),
            BootStage::LunReady => Some(BootStage::VolumeProbed),
            BootStage::VolumeProbed => Some(BootStage::Mounted),
            BootStage::Mounted => None,
        }
    }

    /// 推进到 target。判定次序：挂载守卫 → 单调守卫 → 账容量守卫 →
    /// 咬合 → 推进落账。
    pub fn step_to(&mut self, target: BootStage) -> Result<BootStage, BootCode> {
        if self.stage == BootStage::Mounted {
            return Err(BootCode::PathAlreadyMounted);
        }
        let cur = self.stage.order();
        let tgt = target.order();
        if tgt < cur {
            return Err(BootCode::StageRegression);
        }
        if tgt != cur + 1 {
            return Err(BootCode::StageSkip);
        }
        if self.account.len() >= ACCOUNT_CAP {
            return Err(BootCode::AccountFull);
        }
        if let Some(f) = self.fault {
            if f.kind.bite_stage() == target {
                if f.left > 0 {
                    let mut nf = f;
                    nf.left -= 1;
                    if !nf.counted {
                        nf.counted = true;
                        self.detected += 1;
                    }
                    self.fault = Some(nf);
                    self.stage_retries += 1;
                    return Err(f.kind.code());
                }
                // 咬满放行：恢复成立。
                self.fault = None;
                self.recovered += 1;
            }
        }
        let nt = match self.tick.checked_add(1) {
            Some(t) => t,
            None => return Err(BootCode::TickOverflow),
        };
        self.stage = target;
        self.tick = nt;
        self.account.push(StageRec {
            boot_id: self.boot_id,
            stage: target,
            tick: self.tick,
            retries: self.stage_retries,
        });
        self.stage_retries = 0;
        if self.stage == BootStage::Mounted {
            self.mounted_count += 1;
        }
        Ok(target)
    }

    /// 重启：只在挂载后允许；boot_id 单调递增，账不清（跨重启可回放）。
    pub fn abort(&mut self) -> Result<u32, BootCode> {
        if self.stage != BootStage::Mounted {
            return Err(BootCode::AbortNotMounted);
        }
        self.stage = BootStage::PowerOn;
        self.fault = None;
        self.stage_retries = 0;
        self.boot_id = match self.boot_id.checked_add(1) {
            Some(v) => v,
            None => self.boot_id,
        };
        Ok(self.boot_id)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 从当前阶段一路推进到挂载（含咬合重试）；步数护栏 12（6 段 + 最多 2 咬）。
fn run_to_mount(e: &mut ColdBootEngine) {
    let mut guard = 0u32;
    while !e.mounted() && guard < 12 {
        guard += 1;
        let nxt = match e.next_stage() {
            Some(t) => t,
            None => break,
        };
        let _ = e.step_to(nxt);
    }
}

/// 判据演练：三类错误注入 ×3 尝试 ×10 轮恢复全通。
/// 每类故障每轮重注入（咬 2 次 → 第 3 次尝试放行），返回（全通, 恢复总数）。
pub fn fault_recovery_x3x10() -> (bool, u64) {
    let kinds = [FaultKind::DescriptorShort, FaultKind::ProbeSignature, FaultKind::SectorZero];
    let mut all_ok = true;
    let mut total_recovered = 0u64;
    for kind in kinds.iter() {
        let mut e = ColdBootEngine::new();
        for r in 0..ROUNDS_PER_FAULT {
            if r > 0 && e.abort().is_err() {
                all_ok = false;
            }
            if e.inject(*kind, MAX_BITES).is_err() {
                all_ok = false;
            }
            run_to_mount(&mut e);
            if !e.mounted() {
                all_ok = false;
            }
        }
        if e.recovered() != ROUNDS_PER_FAULT as u64 || !e.conserved() {
            all_ok = false;
        }
        total_recovered += e.recovered();
    }
    (all_ok, total_recovered)
}

/// 双跑一致性：同一脚本（注入 + 一轮重启）两次建账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<StageRec> {
        let mut e = ColdBootEngine::new();
        let _ = e.inject(FaultKind::SectorZero, 1);
        run_to_mount(&mut e);
        let _ = e.abort();
        run_to_mount(&mut e);
        e.account.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51515_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51515");
    // 1 正常流：无故障六级段梯端到端。
    {
        let mut e = ColdBootEngine::new();
        run_to_mount(&mut e);
        let mut retries_zero = true;
        for r in e.account().iter() {
            if r.retries != 0 {
                retries_zero = false;
            }
        }
        s.add(
            "正常冷启动端到端",
            e.mounted() && e.account_len() == 6 && retries_zero,
            "枚举→卷挂载六级逐格走通，账 6 条、零重试",
        );
    }
    // 2 边界零：零咬数拒 + 上电零点起步账空。
    {
        let mut e = ColdBootEngine::new();
        let zero = e.inject(FaultKind::DescriptorShort, 0) == Err(BootCode::ZeroBites);
        s.add(
            "零咬数与零点起步",
            zero && e.next_stage() == Some(BootStage::BusReset) && e.account_len() == 0,
            "咬数 0 显性拒；上电零点起步、端到端账空起步",
        );
    }
    // 3 边界上限/上限+1。
    {
        let mut e = ColdBootEngine::new();
        let ok_max = e.inject(FaultKind::ProbeSignature, MAX_BITES).is_ok();
        let mut e2 = ColdBootEngine::new();
        let over = e2.inject(FaultKind::ProbeSignature, MAX_BITES + 1) == Err(BootCode::BitesTooMany);
        s.add(
            "咬数上限与上限+1",
            ok_max && over,
            "上限恰好注入、上限+1 显性拒（×3 尝试界）",
        );
    }
    // 4~6 三类错误 ×10 轮恢复全通（每类一个引擎，跨重启账只增不减）。
    {
        let mut e = ColdBootEngine::new();
        let mut all = true;
        for r in 0..ROUNDS_PER_FAULT {
            if r > 0 && e.abort().is_err() {
                all = false;
            }
            if e.inject(FaultKind::DescriptorShort, MAX_BITES).is_err() {
                all = false;
            }
            run_to_mount(&mut e);
            if !e.mounted() {
                all = false;
            }
        }
        s.add(
            "描述符短读 ×10 恢复",
            all && e.recovered() == 10 && e.conserved(),
            "每轮 ×3 尝试内恢复，恢复数=检出数",
        );
    }
    {
        let mut e = ColdBootEngine::new();
        let mut all = true;
        for r in 0..ROUNDS_PER_FAULT {
            if r > 0 && e.abort().is_err() {
                all = false;
            }
            if e.inject(FaultKind::SectorZero, MAX_BITES).is_err() {
                all = false;
            }
            run_to_mount(&mut e);
            if !e.mounted() {
                all = false;
            }
        }
        s.add(
            "扇区零尺寸 ×10 恢复",
            all && e.recovered() == 10 && e.conserved(),
            "LUN 段咬点恢复全通，注入=检出=恢复",
        );
    }
    {
        let mut e = ColdBootEngine::new();
        let mut all = true;
        for r in 0..ROUNDS_PER_FAULT {
            if r > 0 && e.abort().is_err() {
                all = false;
            }
            if e.inject(FaultKind::ProbeSignature, MAX_BITES).is_err() {
                all = false;
            }
            run_to_mount(&mut e);
            if !e.mounted() {
                all = false;
            }
        }
        s.add(
            "卷签名不符 ×10 恢复",
            all && e.recovered() == 10 && e.conserved(),
            "卷探针段咬点恢复全通，注入=检出=恢复",
        );
    }
    // 7 判定次序：故障互斥先于咬数校验；挂载守卫先于阶段单调守卫。
    {
        let mut e = ColdBootEngine::new();
        let _ = e.inject(FaultKind::DescriptorShort, MAX_BITES);
        let busy = e.inject(FaultKind::SectorZero, 0) == Err(BootCode::FaultBusy);
        run_to_mount(&mut e);
        let mounted_guard = e.step_to(BootStage::BusReset) == Err(BootCode::PathAlreadyMounted);
        s.add(
            "判序：互斥与挂载守卫在先",
            busy && mounted_guard,
            "故障互斥先于咬数校验；挂载守卫先于单调守卫",
        );
    }
    // 8 状态机拒止：跳格/倒退/未挂载重启。
    {
        let mut e = ColdBootEngine::new();
        let skip = e.step_to(BootStage::Enumerated) == Err(BootCode::StageSkip);
        let _ = e.step_to(BootStage::BusReset);
        let regress = e.step_to(BootStage::PowerOn) == Err(BootCode::StageRegression);
        let abort = e.abort() == Err(BootCode::AbortNotMounted);
        s.add(
            "阶段梯拒止",
            skip && regress && abort,
            "跳格/倒退/未挂载重启一律显性拒",
        );
    }
    // 9 容量耗尽：端到端账满显性拒且账量停在冻结容量。
    {
        let mut e = ColdBootEngine::new();
        let mut hit_full = false;
        let mut guard = 0u32;
        while guard < 400 {
            guard += 1;
            if e.mounted() {
                if e.abort().is_err() {
                    break;
                }
                continue;
            }
            let nxt = match e.next_stage() {
                Some(t) => t,
                None => break,
            };
            if e.step_to(nxt) == Err(BootCode::AccountFull) {
                hit_full = true;
                break;
            }
        }
        s.add(
            "端到端账容量耗尽",
            hit_full && e.account_len() == ACCOUNT_CAP,
            "账满显性拒（AccountFull），账量停在冻结容量",
        );
    }
    // 10 恢复账：咬 1 次放行，咬点段重试数落账。
    {
        let mut e = ColdBootEngine::new();
        let _ = e.inject(FaultKind::SectorZero, 1);
        run_to_mount(&mut e);
        let mut retries_ok = false;
        for r in e.account().iter() {
            if r.stage == BootStage::LunReady {
                retries_ok = r.retries == 1;
            }
        }
        s.add(
            "恢复账重试落账",
            e.mounted() && retries_ok,
            "咬 1 次放行：LUN 段重试数=1 落进端到端账",
        );
    }
    // 11 双跑一致。
    s.add("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建账逐字段相等");
    // 12 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 13 三错误注入 ×3×10 恢复全通（守恒总账）。
    {
        let (all_ok, recovered) = fault_recovery_x3x10();
        s.add(
            "三错误 ×3×10 恢复全通",
            all_ok && recovered == 30,
            "三类错误各 10 轮 ×3 尝试内恢复，恢复总数=30",
        );
    }
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cold_boot_end_to_end() {
        let mut e = ColdBootEngine::new();
        let mut guard = 0u32;
        while !e.mounted() && guard < 12 {
            guard += 1;
            let nxt = match e.next_stage() {
                Some(t) => t,
                None => break,
            };
            assert!(e.step_to(nxt).is_ok());
        }
        assert!(e.mounted());
        // 六级阶段梯：五次推进（PowerOn 起步即记账首态，账 = 推进次数）。
        assert_eq!(e.account_len(), 5);
        assert_eq!(e.boot_id(), 1);
        assert!(e.account().get(0).is_some());
        if let Some(first) = e.account().get(0) {
            assert_eq!(first.stage, BootStage::BusReset);
            assert_eq!(first.retries, 0);
        }
    }

    #[test]
    fn zero_and_upper_bites() {
        let mut e = ColdBootEngine::new();
        assert_eq!(e.inject(FaultKind::SectorZero, 0), Err(BootCode::ZeroBites));
        assert!(e.inject(FaultKind::SectorZero, MAX_BITES).is_ok());
        let mut e2 = ColdBootEngine::new();
        assert_eq!(
            e2.inject(FaultKind::SectorZero, MAX_BITES + 1),
            Err(BootCode::BitesTooMany)
        );
    }

    #[test]
    fn stage_machine_rejects_skip_and_regress() {
        let mut e = ColdBootEngine::new();
        assert_eq!(e.step_to(BootStage::Enumerated), Err(BootCode::StageSkip));
        assert_eq!(e.step_to(BootStage::Mounted), Err(BootCode::StageSkip));
        assert!(e.step_to(BootStage::BusReset).is_ok());
        assert_eq!(e.step_to(BootStage::PowerOn), Err(BootCode::StageRegression));
        assert_eq!(e.abort(), Err(BootCode::AbortNotMounted));
    }

    #[test]
    fn fault_recovery_all_three_x10() {
        let kinds = [FaultKind::DescriptorShort, FaultKind::ProbeSignature, FaultKind::SectorZero];
        for kind in kinds.iter() {
            let mut e = ColdBootEngine::new();
            for r in 0..ROUNDS_PER_FAULT {
                if r > 0 {
                    assert!(e.abort().is_err() == false, "挂载后重启应成功");
                }
                assert!(e.inject(*kind, MAX_BITES).is_ok());
                run_to_mount(&mut e);
                assert!(e.mounted(), "第 {} 轮应在 ×3 尝试内恢复挂载", r);
            }
            assert_eq!(e.recovered(), 10);
            assert!(e.conserved());
            assert_eq!(e.escaped(), 0);
        }
    }

    #[test]
    fn retries_recorded_in_account() {
        let mut e = ColdBootEngine::new();
        let _ = e.inject(FaultKind::ProbeSignature, 1);
        run_to_mount(&mut e);
        assert!(e.mounted());
        let mut found = false;
        for r in e.account().iter() {
            if r.stage == BootStage::VolumeProbed {
                found = true;
                assert_eq!(r.retries, 1, "咬 1 次放行：卷探针段重试数=1");
            }
        }
        assert!(found);
    }

    #[test]
    fn account_capacity_exhaustion() {
        let mut e = ColdBootEngine::new();
        let mut hit_full = false;
        let mut guard = 0u32;
        while guard < 400 {
            guard += 1;
            if e.mounted() {
                if e.abort().is_err() {
                    break;
                }
                continue;
            }
            let nxt = match e.next_stage() {
                Some(t) => t,
                None => break,
            };
            if e.step_to(nxt) == Err(BootCode::AccountFull) {
                hit_full = true;
                break;
            }
        }
        assert!(hit_full);
        assert_eq!(e.account_len(), ACCOUNT_CAP);
    }

    #[test]
    fn judge_orders() {
        // 故障互斥先于咬数校验。
        let mut e = ColdBootEngine::new();
        let _ = e.inject(FaultKind::DescriptorShort, MAX_BITES);
        assert_eq!(e.inject(FaultKind::SectorZero, 0), Err(BootCode::FaultBusy));
        // 挂载守卫先于单调守卫（挂载后走任何段都报挂载守卫码）。
        run_to_mount(&mut e);
        assert_eq!(e.step_to(BootStage::BusReset), Err(BootCode::PathAlreadyMounted));
    }

    #[test]
    fn dual_run_identical_account() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(ALL_CODES.len(), 15);
    }
}
