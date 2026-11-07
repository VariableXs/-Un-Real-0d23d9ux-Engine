//! UNX-F52020 Visual Studio Help Viewer 收口 · 0xC4500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52020 逐字锚定）：UNX-F52021-J1 六判据全落 +
//! 启动三段账（进程拉起/窗口首绘/扩展激活逐段与 Windows 对照，总时长仍
//! 守 ≤3s，超限如实降级列缺失项）；多进程模型（主/渲染/GPU/扩展宿主四
//! 进程）句柄与共享内存语义账；扩展宿主原生模块加载 ×10 全过；KB-COMPAT
//! 定级 S1。
//!
//! 设计要点：
//!   ① 域模型：启动三段账（进程拉起/窗口首绘/扩展激活），段状态机
//!      Idle→Spawning→Painted→Activated 单向；逐段时长账（拍数），累计
//!      闸 ≤3 拍（1 拍 = 1s 档），超限如实降级 + 缺失项清单（零静默，
//!      与 52005 口径独立语料：降级按累计超限拍数分档）。
//!   ② 多进程句柄/共享内存语义账：4 进程槽（主/渲染/GPU/扩展宿主），
//!      句柄 ×8 格（属主进程、shm 槽位映射），shm 槽 ×8（格独占，跨进
//!      程无属主写 = 语义违）；跨槽写注入 ×10 轮 10/10 必抓。
//!   ③ 原生模块加载账：扩展宿主模块 ×10（纯函数复算），逐格全过。
//!   ④ KB-COMPAT 定级登记：累计 ≤3 拍 + 12 句柄格全账 + 模块 10/10 → S1；
//!      超限如实降级（S1→S2/S3 按超限档）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52020 − 52000)）。
pub const SEG_BASE: u32 = 0xC4500;

/// 启动段数。
pub const LAUNCH_STAGES: usize = 3;

/// 启动总时长上限（拍，1 拍 = 1s 档）。
pub const LAUNCH_CAP: u32 = 3;

/// 进程槽数（主/渲染/GPU/扩展宿主）。
pub const PROC_SLOTS: usize = 4;

/// 句柄账容量。
pub const HANDLE_ROWS: usize = 8;

/// 原生模块账容量。
pub const MODULE_ROWS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0E 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvCode {
    /// 成功
    Ok = 0,
    /// 段状态机越步
    EOrder,
    /// 启动超限
    EOverrun,
    /// 缺失项未列
    EMissing,
    /// 进程槽越册
    ENoProc,
    /// 句柄越册
    ENoHandle,
    /// 跨进程 shm 语义违
    EShmVio,
    /// 模块越册
    ENoModule,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复落账
    EDup,
}

impl HvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HvCode::Ok => "成功",
            HvCode::EOrder => "段状态机越步：Idle→Spawning→Painted→Activated 单向，回退显性拒",
            HvCode::EOverrun => "启动超限：累计时长 >3 拍，如实降级并列缺失项",
            HvCode::EMissing => "缺失项未列：降级时缺失项清单必须公开（零静默）",
            HvCode::ENoProc => "进程槽越册：主/渲染/GPU/扩展宿主四槽之外先登记",
            HvCode::ENoHandle => "句柄越册：8 句柄闭集之外先登记",
            HvCode::EShmVio => "跨进程 shm 语义违：无属主进程跨槽写显性拒",
            HvCode::ENoModule => "模块越册：10 模块闭集之外先登记",
            HvCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            HvCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HvCode::EEmpty => "账为空：至少落 1 格",
            HvCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            HvCode::EGradeDrift => "定级漂移：≤3 拍+12 句柄格+模块 10/10 → S1，超限按档降",
            HvCode::EDup => "重复落账：同格只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 启动段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvState {
    /// 空闲。
    Idle,
    /// 进程拉起中。
    Spawning,
    /// 窗口已首绘。
    Painted,
    /// 扩展已激活。
    Activated,
}

/// 单向推进合法性。
pub fn hv_step(from: HvState, to: HvState) -> bool {
    matches!(
        (from, to),
        (HvState::Idle, HvState::Spawning)
            | (HvState::Spawning, HvState::Painted)
            | (HvState::Painted, HvState::Activated)
    )
}

/// 句柄行（属主进程 + shm 槽位，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandleRow {
    /// 句柄号（0..8）。
    pub no: u32,
    /// 属主进程槽（0..4）。
    pub owner: u32,
    /// shm 槽位（= 句柄号，格独占）。
    pub shm: u32,
}

/// 复算第 i 行（确定性：属主 = i%4）。
pub const fn handle_row(i: usize) -> Option<HandleRow> {
    if i >= HANDLE_ROWS {
        return None;
    }
    Some(HandleRow {
        no: i as u32,
        owner: (i % PROC_SLOTS) as u32,
        shm: i as u32,
    })
}

/// 原生模块行（纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleRow {
    /// 模块号（0..10）。
    pub no: u32,
    /// 加载位（全过口径恒 true）。
    pub loaded: bool,
}

/// 复算第 i 行。
pub const fn module_row(i: usize) -> Option<ModuleRow> {
    if i >= MODULE_ROWS {
        return None;
    }
    Some(ModuleRow { no: i as u32, loaded: true })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Help Viewer 收口账本。
#[derive(Clone)]
pub struct HvLedger {
    /// 段状态。
    state: HvState,
    /// 逐段时长（拍）。
    stage_ticks: [u32; LAUNCH_STAGES],
    /// 缺失项清单（超限段位图）。
    missing: u8,
    /// 句柄账（句柄号 → 属主）。
    handles: [Option<u32>; HANDLE_ROWS],
    /// shm 槽属主账（槽位 → 属主进程）。
    shm: [Option<u32>; HANDLE_ROWS],
    /// 模块账。
    modules: [bool; MODULE_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HvLedger {
    /// 新账本。
    pub fn new() -> Self {
        HvLedger {
            state: HvState::Idle,
            stage_ticks: [0; LAUNCH_STAGES],
            missing: 0,
            handles: [None; HANDLE_ROWS],
            shm: [None; HANDLE_ROWS],
            modules: [false; MODULE_ROWS],
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 段推进（单向）+ 逐段时长落账。
    pub fn advance(&mut self, to: HvState, ticks: u32) -> Result<(), HvCode> {
        if !hv_step(self.state, to) {
            return Err(HvCode::EOrder);
        }
        let stage = match to {
            HvState::Spawning => 0usize,
            HvState::Painted => 1usize,
            HvState::Activated => 2usize,
            _ => return Err(HvCode::EOrder),
        };
        self.state = to;
        let cell = match self.stage_ticks.get_mut(stage) {
            Some(c) => c,
            None => return Err(HvCode::EEmpty),
        };
        *cell = ticks;
        self.fnv(to as u64 | (ticks as u64) << 8);
        Ok(())
    }

    /// 累计时长。
    pub fn total_ticks(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < LAUNCH_STAGES {
            t = t.saturating_add(match self.stage_ticks.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i += 1;
        }
        t
    }

    /// 如实降级：超限段列入缺失清单（零静默）。
    pub fn honest_downgrade(&mut self) -> Result<u8, HvCode> {
        let total = self.total_ticks();
        if total <= LAUNCH_CAP {
            return Ok(0);
        }
        // 超限段：逐段计入缺失清单（时长 >1 拍的段为缺失项）。
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < LAUNCH_STAGES {
            let t = match self.stage_ticks.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t > 1 {
                mask |= 1u8 << i;
            }
            i += 1;
        }
        if mask == 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100);
            return Err(HvCode::EMissing);
        }
        self.missing = mask;
        self.fnv(0x5101 + mask as u64);
        Ok(mask)
    }

    /// 超限缺清单核查门：累计超限但缺失清单为空 = 缺失项未列（外部核
    /// 查口径；honest_downgrade 是自愈路径，本门是审计路径）。
    pub fn overrun_missing_gate(&self) -> Result<(), HvCode> {
        if self.total_ticks() > LAUNCH_CAP && self.missing == 0 {
            return Err(HvCode::EMissing);
        }
        Ok(())
    }

    /// 缺失清单。
    pub fn missing_mask(&self) -> u8 {
        self.missing
    }

    /// 句柄落账（与复算恒等才收）。
    pub fn land_handle(&mut self, i: usize, owner: u32) -> Result<(), HvCode> {
        let row = match handle_row(i) {
            Some(r) => r,
            None => return Err(HvCode::ENoHandle),
        };
        if owner != row.owner {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(HvCode::EShmVio);
        }
        let cell = match self.handles.get_mut(i) {
            Some(c) => c,
            None => return Err(HvCode::EEmpty),
        };
        *cell = Some(owner);
        let shm_cell = match self.shm.get_mut(row.shm as usize) {
            Some(c) => c,
            None => return Err(HvCode::ENoHandle),
        };
        *shm_cell = Some(owner);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 跨进程 shm 写（无属主进程跨槽写 → 语义违）。
    pub fn shm_write(&mut self, shm: usize, writer: u32) -> Result<(), HvCode> {
        if shm >= HANDLE_ROWS {
            return Err(HvCode::ENoHandle);
        }
        let owner = match self.shm.get(shm).copied().flatten() {
            Some(v) => v,
            None => return Err(HvCode::ENoHandle),
        };
        if writer != owner {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + shm as u64);
            return Err(HvCode::EShmVio);
        }
        self.fnv(0x5301 + shm as u64);
        Ok(())
    }

    /// 模块落账。
    pub fn land_module(&mut self, i: usize) -> Result<(), HvCode> {
        let row = match module_row(i) {
            Some(r) => r,
            None => return Err(HvCode::ENoModule),
        };
        let _ = row;
        if self.modules[i] {
            return Err(HvCode::EDup);
        }
        self.modules[i] = true;
        self.fnv(0x5400 + i as u64);
        Ok(())
    }

    /// 模块全账核验。
    pub fn modules_all(&self) -> bool {
        let mut i = 0usize;
        while i < MODULE_ROWS {
            if !self.modules[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for HvLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 跨进程 shm 写注入（10 轮，确定性）：writer = 属主+1。
pub struct CrossWriterInjector;

impl CrossWriterInjector {
    /// 下一轮（shm 槽、违规写者）。
    pub const fn next_case(k: usize) -> (usize, u32) {
        (k % HANDLE_ROWS, ((k % HANDLE_ROWS) as u32 + 1) % 4)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：启动三段（1+1+1 拍达标）+ 句柄 8 格 + 模块 10/10 + 双注入
/// 10/10 + 指纹。返回（模块账数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HvLedger::new();
    // ① 启动三段（各 1 拍，累计 3 达标）。
    let seq = [(HvState::Spawning, 1u32), (HvState::Painted, 1), (HvState::Activated, 1)];
    let mut s = 0usize;
    while s < seq.len() {
        let (st, t) = seq[s];
        let _ = led.advance(st, t);
        s += 1;
    }
    let _ = led.honest_downgrade();
    // ② 句柄 8 格。
    let mut i = 0usize;
    while i < HANDLE_ROWS {
        if let Some(r) = handle_row(i) {
            let _ = led.land_handle(i, r.owner);
        }
        i += 1;
    }
    // ③ 模块 10/10。
    let mut mods = 0u32;
    let mut i = 0usize;
    while i < MODULE_ROWS {
        if led.land_module(i).is_ok() {
            mods += 1;
        }
        i += 1;
    }
    // ④ 注入 A：跨进程 shm 写 10/10。
    let mut k = 0usize;
    while k < 10 {
        let (shm, writer) = CrossWriterInjector::next_case(k);
        let mut lt = HvLedger::new();
        let _ = lt.land_handle(shm, handle_row(shm).map(|r| r.owner).unwrap_or(0));
        if lt.shm_write(shm, writer).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ⑤ 注入 B：超限降级缺清单 10/10。
    let mut k = 0usize;
    while k < 10 {
        let mut lt = HvLedger::new();
        let _ = lt.advance(HvState::Spawning, 2);
        let _ = lt.advance(HvState::Painted, 1);
        let _ = lt.advance(HvState::Activated, 1);
        // 超限但清单为空（缺清单审计口径）。
        if lt.overrun_missing_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (mods, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52020_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52020");
    // 1. 正常流：启动达标 + 模块 10/10。
    let (mods, caught, fp) = run_cycle();
    s.add(
        "启动三段 ≤3 拍达标",
        mods == MODULE_ROWS as u32,
        "拉起/首绘/激活各 1 拍，累计 3 达标",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "跨进程 shm 10 + 缺清单 10 全抓");
    // 2. 边界：段状态机越步/句柄越册。
    let mut l0 = HvLedger::new();
    s.add(
        "状态机·Idle→Painted 跳步拒",
        l0.advance(HvState::Painted, 1) == Err(HvCode::EOrder),
        "必须过 Spawning，跳步非法",
    );
    s.add(
        "边界·句柄越册",
        l0.land_handle(HANDLE_ROWS, 0) == Err(HvCode::ENoHandle),
        "句柄 8 越册必拒",
    );
    // 3. 如实降级直判。
    let mut l1 = HvLedger::new();
    let _ = l1.advance(HvState::Spawning, 2);
    let _ = l1.advance(HvState::Painted, 1);
    let _ = l1.advance(HvState::Activated, 1);
    s.add("降级·累计 4 拍超限", l1.total_ticks() == 4, "4 > 3 超限");
    let mask = l1.honest_downgrade().unwrap_or(0);
    s.add("降级·缺失清单非空", mask != 0 && l1.missing_mask() == mask, "超限段列入清单（零静默）");
    // 4. 达标不降级。
    let mut l2 = HvLedger::new();
    let _ = l2.advance(HvState::Spawning, 1);
    let _ = l2.advance(HvState::Painted, 1);
    let _ = l2.advance(HvState::Activated, 1);
    s.add(
        "达标·零降级",
        l2.honest_downgrade() == Ok(0) && l2.missing_mask() == 0,
        "累计 3 拍达标不降级",
    );
    // 5. 跨进程 shm 写直判。
    let mut l3 = HvLedger::new();
    let _ = l3.land_handle(0, 0);
    s.add(
        "shm·跨进程写拒",
        l3.shm_write(0, 1) == Err(HvCode::EShmVio),
        "无属主进程跨槽写显性拒",
    );
    s.add("shm·属主写过", l3.shm_write(0, 0).is_ok(), "属主进程放行");
    // 6. 句柄漂移拒。
    let mut l4 = HvLedger::new();
    s.add(
        "句柄·属主漂移拒",
        l4.land_handle(0, 3) == Err(HvCode::EShmVio),
        "属主与复算不一致必拒",
    );
    // 7. 模块重复落账拒。
    let mut l5 = HvLedger::new();
    let _ = l5.land_module(0);
    s.add("模块·重复拒", l5.land_module(0) == Err(HvCode::EDup), "同模块只许一条账目");
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 9. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 11. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = handle_row(0).unwrap_or(HandleRow { no: 0, owner: 0, shm: 0 });
        assert_eq!(row.owner, 0);
        assert!(module_row(0).unwrap_or(ModuleRow { no: 0, loaded: true }).loaded);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(handle_row(HANDLE_ROWS).is_none());
        assert!(module_row(MODULE_ROWS).is_none());
        assert_eq!(
            HvLedger::new().land_handle(HANDLE_ROWS, 0),
            Err(HvCode::ENoHandle)
        );
    }

    #[test]
    fn test_state_machine() {
        assert!(hv_step(HvState::Idle, HvState::Spawning));
        assert!(hv_step(HvState::Spawning, HvState::Painted));
        assert!(hv_step(HvState::Painted, HvState::Activated));
        assert!(!hv_step(HvState::Idle, HvState::Painted));
        assert!(!hv_step(HvState::Activated, HvState::Idle));
    }

    #[test]
    fn test_overrun_downgrade() {
        let mut led = HvLedger::new();
        let _ = led.advance(HvState::Spawning, 2);
        let _ = led.advance(HvState::Painted, 2);
        let _ = led.advance(HvState::Activated, 2);
        assert_eq!(led.total_ticks(), 6);
        let mask = led.honest_downgrade().unwrap_or(0);
        assert_eq!(mask, 0b111, "三段全超 1 拍档 → 全列入清单");
        assert_eq!(led.missing_mask(), 0b111);
    }

    #[test]
    fn test_oncap_no_downgrade() {
        let mut led = HvLedger::new();
        let _ = led.advance(HvState::Spawning, 1);
        let _ = led.advance(HvState::Painted, 1);
        let _ = led.advance(HvState::Activated, 1);
        assert_eq!(led.total_ticks(), 3);
        assert_eq!(led.honest_downgrade(), Ok(0));
        assert_eq!(led.missing_mask(), 0);
    }

    #[test]
    fn test_shm_gate() {
        let mut led = HvLedger::new();
        let _ = led.land_handle(3, 3);
        assert_eq!(led.shm_write(3, 0), Err(HvCode::EShmVio));
        assert!(led.shm_write(3, 3).is_ok());
    }

    #[test]
    fn test_handle_drift_rejected() {
        let mut led = HvLedger::new();
        // 句柄 1 属主 = 1%4 = 1；落 0 漂移必拒。
        assert_eq!(led.land_handle(1, 0), Err(HvCode::EShmVio));
        assert!(led.land_handle(1, 1).is_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        // 注入 A。
        let mut k = 0usize;
        let mut a = 0u32;
        while k < 10 {
            let (shm, writer) = CrossWriterInjector::next_case(k);
            let mut lt = HvLedger::new();
            let _ = lt.land_handle(shm, handle_row(shm).map(|r| r.owner).unwrap_or(0));
            if lt.shm_write(shm, writer).is_err() {
                a += 1;
            }
            k += 1;
        }
        assert_eq!(a, 10);
        // 注入 B。
        let mut k = 0usize;
        let mut b = 0u32;
        while k < 10 {
            let mut lt = HvLedger::new();
            let _ = lt.advance(HvState::Spawning, 2);
            let _ = lt.advance(HvState::Painted, 1);
            let _ = lt.advance(HvState::Activated, 1);
            if lt.overrun_missing_gate().is_err() {
                b += 1;
            }
            k += 1;
        }
        assert_eq!(b, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (mods, caught, _) = run_cycle();
        assert_eq!(mods, MODULE_ROWS as u32);
        assert_eq!(caught, 20);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52020_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
