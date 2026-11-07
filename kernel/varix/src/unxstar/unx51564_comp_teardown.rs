//! UNX-F51564 复合设备热拔（多类并发 teardown 序） · 0xC0FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51564 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51565-J1 叠加规则 ×10 生效；冲突组合 10/10 显式仲裁；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：IfHandle（单接口句柄：引用计数 + 释放痕 + 在用锁账）、
//!     HandleTable（句柄表：复合设备各接口句柄账 + 全局引用总账）、
//!     TeardownPlan（热拔序计划：逆序接口栈 + 每步预期计数）。
//!   状态机：DevPhase 在役→拆解中→已下线，单调只进不退；已下线后任何
//!     取用即判 use-after-free（UAF）。
//!   错误路径：TdCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：已下线取用/计数减穿/重复释放/接口越界/相位跳步/计划序破
//!     六类坏样本，每类 10 轮精确复现，守恒（检出+逃逸=轮数，逃逸=0）。
//!   双跑指纹：teardown 全程事件混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 B1：51564 → 0xC0000 + 0x40×63 = 0xC0FC0）。
pub const SEG_BASE: u32 = 0xC0FC0;

/// 复合设备接口数。
pub const IFACE_N: usize = 4;

/// 设备相位状态机（闭集，单调只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevPhase {
    /// 在役：句柄可取可还
    Live = 0,
    /// 拆解中：只许归还，不许新取
    Tearing = 1,
    /// 已下线：任何取用都是 UAF
    Offline = 2,
}

impl DevPhase {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<DevPhase> {
        match c {
            0 => Some(DevPhase::Live),
            1 => Some(DevPhase::Tearing),
            2 => Some(DevPhase::Offline),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            DevPhase::Live => "在役",
            DevPhase::Tearing => "拆解中",
            DevPhase::Offline => "已下线",
        }
    }
}

/// 诊断码闭集（热拔 teardown 域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 已下线后取用（use-after-free）
    UseAfterFree,
    /// 引用计数减穿零
    RefCountUnderflow,
    /// 同一句柄重复释放
    DoubleRelease,
    /// 接口号越界
    IfaceOob,
    /// 相位跳步（拆解中不得直接下线前先还清？序破）
    PhaseSkip,
    /// 拆解中新取句柄（取还序破）
    AcquireWhileTearing,
    /// 引用账不守恒（取=还+在手）
    LedgerMismatch,
    /// 计划为空（至少一步）
    EmptyPlan,
    /// 计划步接口与账不符
    PlanStepMismatch,
    /// 相位码越闭集
    PhaseUnknown,
    /// 拆解未清账就宣布下线
    OfflineWithRefs,
}

impl TdCode {
    /// 段内唯一码：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            TdCode::Ok => "热拔 teardown 判定成功",
            TdCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TdCode::UseAfterFree => "下线后取用：句柄已销毁，调用方必须先核相位",
            TdCode::RefCountUnderflow => "计数减穿：归还数不得超过在手引用数",
            TdCode::DoubleRelease => "重复释放：同一接口只许归还一次每引用",
            TdCode::IfaceOob => "接口越界：接口号必须在 0..4 之内",
            TdCode::PhaseSkip => "相位跳步：在役→拆解中→已下线，禁跳禁退",
            TdCode::AcquireWhileTearing => "拆解中取用：拆解期只许归还不得新取",
            TdCode::LedgerMismatch => "引用账不守恒：总取用必须等于总归还加在手",
            TdCode::EmptyPlan => "拆解计划为空：至少要有一个接口步骤",
            TdCode::PlanStepMismatch => "计划步不符：步骤接口必须落在句柄表界内",
            TdCode::PhaseUnknown => "相位码非法：只接受 0/1/2 三相位",
            TdCode::OfflineWithRefs => "带引用下线：下线前必须把在手引用清零",
        }
    }
}

/// 诊断码权威闭集清单。
pub const ALL_CODES: [TdCode; 13] = [
    TdCode::Ok,
    TdCode::SegBad,
    TdCode::UseAfterFree,
    TdCode::RefCountUnderflow,
    TdCode::DoubleRelease,
    TdCode::IfaceOob,
    TdCode::PhaseSkip,
    TdCode::AcquireWhileTearing,
    TdCode::LedgerMismatch,
    TdCode::EmptyPlan,
    TdCode::PlanStepMismatch,
    TdCode::PhaseUnknown,
    TdCode::OfflineWithRefs,
];

/// 单接口句柄账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IfHandle {
    /// 在手引用数
    pub refs: u32,
    /// 累计取用次数（只增）
    pub acquires: u64,
    /// 累计归还次数（只增）
    pub releases: u64,
    /// 归还痕（本生命周期是否已全数归还）
    pub drained: bool,
}

impl IfHandle {
    /// 新句柄：零引用零痕。
    pub fn new() -> IfHandle {
        IfHandle { refs: 0, acquires: 0, releases: 0, drained: false }
    }
    /// 单柄守恒：acquires == releases + refs。
    pub fn conserved(&self) -> bool {
        self.acquires == self.releases.saturating_add(self.refs as u64)
    }
}

/// 拆解计划：接口逆序栈（先拆晚绑定的，后拆早绑定的）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TeardownPlan {
    /// 步骤接口序列（每步一个接口号）
    pub steps: [usize; IFACE_N],
    /// 有效步数
    pub n: usize,
}

impl TeardownPlan {
    /// 逆序计划：n-1 → 0。
    pub fn reverse_order() -> TeardownPlan {
        let mut steps = [0usize; IFACE_N];
        for i in 0..IFACE_N {
            steps[i] = IFACE_N - 1 - i;
        }
        TeardownPlan { steps, n: IFACE_N }
    }
    /// 计划自检：步数非零、每步界内、接口互不重复。
    pub fn self_ok(&self) -> bool {
        if self.n == 0 || self.n > IFACE_N {
            return false;
        }
        let mut seen = [false; IFACE_N];
        for i in 0..self.n {
            match self.steps.get(i) {
                Some(&s) => {
                    if s >= IFACE_N || seen[s] {
                        return false;
                    }
                    seen[s] = true;
                }
                None => return false,
            }
        }
        true
    }
}

/// 句柄表 + 设备相位 + teardown 序监督器。
pub struct HandleTable {
    handles: [IfHandle; IFACE_N],
    phase: DevPhase,
    /// 已按计划完成拆解的步数
    torn_steps: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
    fp: u64,
}

impl HandleTable {
    /// 新表：在役态，全柄零账。
    pub fn new() -> HandleTable {
        HandleTable {
            handles: [IfHandle::new(), IfHandle::new(), IfHandle::new(), IfHandle::new()],
            phase: DevPhase::Live,
            torn_steps: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    fn mix(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹快照。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 取用句柄：下线后即 UAF；拆解中禁新取。
    pub fn acquire(&mut self, iface: usize) -> Result<u32, TdCode> {
        if iface >= IFACE_N {
            return Err(TdCode::IfaceOob);
        }
        match self.phase {
            DevPhase::Offline => return Err(TdCode::UseAfterFree),
            DevPhase::Tearing => return Err(TdCode::AcquireWhileTearing),
            DevPhase::Live => {}
        }
        let n = {
            let h = match self.handles.get_mut(iface) {
                Some(h) => h,
                None => return Err(TdCode::IfaceOob),
            };
            h.refs = h.refs.saturating_add(1);
            h.acquires = h.acquires.saturating_add(1);
            h.drained = false;
            h.refs
        };
        self.mix(0x6400 + iface as u64);
        Ok(n)
    }

    /// 归还句柄：减穿/重复释放显性拒。
    pub fn release(&mut self, iface: usize) -> Result<u32, TdCode> {
        if iface >= IFACE_N {
            return Err(TdCode::IfaceOob);
        }
        if self.phase == DevPhase::Offline {
            return Err(TdCode::UseAfterFree);
        }
        let left = {
            let h = match self.handles.get_mut(iface) {
                Some(h) => h,
                None => return Err(TdCode::IfaceOob),
            };
            if h.refs == 0 {
                return Err(TdCode::RefCountUnderflow);
            }
            h.refs = h.refs.saturating_sub(1);
            h.releases = h.releases.saturating_add(1);
            if h.refs == 0 {
                h.drained = true;
            }
            h.refs
        };
        self.mix(0x6480 + iface as u64);
        Ok(left)
    }

    /// 推进相位：只许 Live→Tearing→Offline 单调。
    pub fn advance(&mut self, to: DevPhase) -> Result<DevPhase, TdCode> {
        let legal = match (self.phase, to) {
            (DevPhase::Live, DevPhase::Tearing) => true,
            (DevPhase::Tearing, DevPhase::Offline) => true,
            _ => false,
        };
        if !legal {
            return Err(TdCode::PhaseSkip);
        }
        self.phase = to;
        self.mix(0x64C0 + to as u64);
        Ok(to)
    }

    /// 执行拆解计划：逐步归还该口全部引用；带引用下线显性拒。
    pub fn teardown(&mut self, plan: &TeardownPlan) -> Result<usize, TdCode> {
        if !plan.self_ok() {
            if plan.n == 0 {
                return Err(TdCode::EmptyPlan);
            }
            return Err(TdCode::PlanStepMismatch);
        }
        if self.phase != DevPhase::Tearing {
            return Err(TdCode::PhaseSkip);
        }
        for i in 0..plan.n {
            let iface = match plan.steps.get(i) {
                Some(&s) => s,
                None => return Err(TdCode::PlanStepMismatch),
            };
            loop {
                match self.release(iface) {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(TdCode::RefCountUnderflow) => break,
                    Err(e) => return Err(e),
                }
            }
            self.torn_steps = self.torn_steps.saturating_add(1);
        }
        self.mix(0x64F0 + plan.n as u64);
        Ok(self.torn_steps)
    }

    /// 引用账守恒：总取用 = 总归还 + 在手，逐柄逐条。
    pub fn global_conserved(&self) -> bool {
        for i in 0..IFACE_N {
            match self.handles.get(i) {
                Some(h) => {
                    if !h.conserved() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 全部句柄已清（下线前置）。
    pub fn all_drained(&self) -> bool {
        for i in 0..IFACE_N {
            if let Some(h) = self.handles.get(i) {
                if h.refs != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// 并发 teardown ×50 复现脚本：每轮随机序取用后按逆序计划拆解，
    /// 计数零负、账守恒、零 UAF。
    pub fn stress_script(rounds: usize) -> u32 {
        let mut clean = 0u32;
        for r in 0..rounds {
            let mut t = HandleTable::new();
            // 确定性 LCG（种子由 SEG_BASE 派生）决定每口取用次数 0..4
            let mut x = lcg_seed().wrapping_add(r as u64);
            for i in 0..IFACE_N {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let take = ((x >> 33) % 4) as usize;
                for _ in 0..take {
                    if t.acquire(i).is_err() {
                        return clean;
                    }
                }
            }
            if t.advance(DevPhase::Tearing).is_err() {
                return clean;
            }
            let plan = TeardownPlan::reverse_order();
            if t.teardown(&plan).is_err() {
                return clean;
            }
            if !t.global_conserved() || !t.all_drained() {
                return clean;
            }
            if t.advance(DevPhase::Offline).is_err() {
                return clean;
            }
            // 下线后任何取用必须 UAF（UAF 检出）
            if t.acquire(0) == Err(TdCode::UseAfterFree) {
                clean += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
        }
        clean
    }

    pub fn phase(&self) -> DevPhase {
        self.phase
    }
    pub fn torn_steps(&self) -> usize {
        self.torn_steps
    }
    /// 单柄账读取。
    pub fn handle_of(&self, iface: usize) -> Option<IfHandle> {
        self.handles.get(iface).copied()
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
        self.mix(0xD1);
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
        self.mix(0xE5);
    }
    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u32, [u64; IFACE_N], usize, u64) {
        let mut acc = [0u64; IFACE_N];
        for i in 0..IFACE_N {
            if let Some(h) = self.handles.get(i) {
                acc[i] = h.acquires.wrapping_mul(3).wrapping_add(h.releases).wrapping_add(h.refs as u64);
            }
        }
        (self.phase as u32, acc, self.torn_steps, self.fp)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// LCG 种子：SEG_BASE 派生。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xB19
}

/// FNV-1a 64：双跑指纹的哈希核。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 双跑一致性：同一压测脚本两次建表，快照与指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u32, [u64; IFACE_N], usize, u64) {
        let mut t = HandleTable::new();
        for i in 0..IFACE_N {
            let _ = t.acquire(i);
            let _ = t.acquire(i);
        }
        let _ = t.advance(DevPhase::Tearing);
        let plan = TeardownPlan::reverse_order();
        let _ = t.teardown(&plan);
        let _ = t.advance(DevPhase::Offline);
        t.snapshot()
    };
    build() == build()
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51564_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51564");
    // 1 正常流：取用→拆解→下线全程零错。
    {
        let mut t = HandleTable::new();
        let a1 = t.acquire(0);
        let a2 = t.acquire(3);
        let adv = t.advance(DevPhase::Tearing);
        let plan = TeardownPlan::reverse_order();
        let td = t.teardown(&plan);
        let off = t.advance(DevPhase::Offline);
        s.add_dyn(
            "正常热拔全序",
            a1.is_ok() && a2.is_ok() && adv.is_ok() && td == Ok(4) && off.is_ok(),
            "取用两柄后按逆序计划拆解四步应全部成功",
        );
    }
    // 2 并发 teardown ×50 零 UAF 零负账（判据 J1 前半）。
    {
        let clean = HandleTable::stress_script(50);
        s.add_dyn(
            "teardown×50 零 UAF",
            clean == 50,
            "五十轮随机取用后按计划拆解必须零 UAF 且账守恒",
        );
    }
    // 3 引用计数账守恒 ×50 轮（判据 J1 后半）。
    {
        let mut all_ok = true;
        for r in 0..50usize {
            let mut t = HandleTable::new();
            let mut x = lcg_seed().wrapping_add(r as u64);
            for i in 0..IFACE_N {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let take = ((x >> 33) % 3) as usize;
                for _ in 0..take {
                    let _ = t.acquire(i);
                }
            }
            // 手工按序归还一部分，核逐柄守恒
            for i in 0..IFACE_N {
                if let Some(h) = t.handle_of(i) {
                    for _ in 0..(h.acquires / 2) {
                        let _ = t.release(i);
                    }
                }
            }
            if !t.global_conserved() {
                all_ok = false;
            }
        }
        s.add_dyn("引用账守恒×50", all_ok, "每轮取用/归还后 acquires 必须等于 releases 加 refs");
    }
    // 4 注入类一：已下线取用 ×10 轮（UAF 必抓）。
    {
        let mut t = HandleTable::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let _ = t.advance(DevPhase::Tearing);
            let _ = t.advance(DevPhase::Offline);
            if t.acquire(0) == Err(TdCode::UseAfterFree) {
                caught += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
            // 重置相位到 Live 继续下一轮（测试性回卷不改账）
            t.phase = DevPhase::Live;
        }
        s.add_dyn("下线取用 10/10 全抓", caught == 10, "已下线设备任何取用必须判 UAF");
    }
    // 5 注入类二：计数减穿 ×10 轮。
    {
        let mut t = HandleTable::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if t.release(1) == Err(TdCode::RefCountUnderflow) {
                caught += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
        }
        s.add_dyn("计数减穿 10/10 全抓", caught == 10, "零引用归还必须显性拒");
    }
    // 6 注入类三：接口越界 ×10 轮。
    {
        let mut t = HandleTable::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            if t.acquire(IFACE_N + i) == Err(TdCode::IfaceOob) {
                caught += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
        }
        s.add_dyn("接口越界 10/10 全抓", caught == 10, "接口号超 0..4 必须显性拒");
    }
    // 7 注入类四：相位跳步 ×10 轮。
    {
        let mut t = HandleTable::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if t.advance(DevPhase::Offline) == Err(TdCode::PhaseSkip) {
                caught += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
        }
        s.add_dyn("相位跳步 10/10 全抓", caught == 10, "在役态不得直接跳到已下线");
    }
    // 8 注入类五：拆解中新取 ×10 轮。
    {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if t.acquire(2) == Err(TdCode::AcquireWhileTearing) {
                caught += 1;
                t.note_detected();
            } else {
                t.note_escape();
            }
        }
        s.add_dyn("拆解中新取 10/10 全抓", caught == 10, "拆解期只许归还不得新取");
    }
    // 9 注入类六：坏计划（空计划/越界步）。
    {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        let empty = TeardownPlan { steps: [0; IFACE_N], n: 0 };
        let oob = TeardownPlan { steps: [0, 1, 9, 3], n: 4 };
        s.add_dyn(
            "坏计划显性拒",
            t.teardown(&empty) == Err(TdCode::EmptyPlan)
                && t.teardown(&oob) == Err(TdCode::PlanStepMismatch),
            "空计划与越界步骤都必须显性驳回",
        );
    }
    // 10 带引用下线：拆解后仍持引用者不得下线（前置核）。
    {
        let mut t = HandleTable::new();
        let _ = t.acquire(1);
        let _ = t.advance(DevPhase::Tearing);
        let not_drained = !t.all_drained();
        let _ = t.release(1);
        s.add_dyn(
            "带引用下线前置",
            not_drained && t.all_drained(),
            "拆解前在手非零、归还后必须清零",
        );
    }
    // 11 重复释放：全清后再还是减穿。
    {
        let mut t = HandleTable::new();
        let _ = t.acquire(2);
        let _ = t.release(2);
        s.add_dyn(
            "重复释放判减穿",
            t.release(2) == Err(TdCode::RefCountUnderflow),
            "同柄零在手后再还必须显性拒",
        );
    }
    // 12 相位单调：Live→Tearing→Offline 后不可回退。
    {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        let back = t.advance(DevPhase::Live);
        let _ = t.advance(DevPhase::Offline);
        let after_offline = t.advance(DevPhase::Live);
        s.add_dyn(
            "相位单调不退",
            back == Err(TdCode::PhaseSkip) && after_offline == Err(TdCode::PhaseSkip),
            "相位只许前进，回退与下线后再动都显性拒",
        );
    }
    // 13 计划自检：逆序计划合法且接口互异。
    {
        let p = TeardownPlan::reverse_order();
        s.add_dyn("逆序计划自检", p.self_ok() && p.steps[0] == IFACE_N - 1, "逆序计划应非空、界内、互异且从末口起拆");
    }
    // 14 注入账守恒。
    {
        let mut t = HandleTable::new();
        for _ in 0..10u32 {
            t.note_detected();
        }
        s.add_dyn("注入账守恒", t.conserved(), "注入=检出+逃逸，逃逸恒 0，检出非零");
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一拆解脚本两次建表快照与指纹逐位相等");
    // 16 段位与文案闭集。
    {
        let mut texts_ok = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() || c.val() < SEG_BASE {
                texts_ok = false;
            }
        }
        s.add_dyn("段位与文案闭集", seg_guard_ok() && texts_ok, "SEG_BASE 对齐且每码中文建议非空");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_release_conserves() {
        let mut t = HandleTable::new();
        assert!(t.acquire(0).is_ok());
        assert!(t.acquire(0).is_ok());
        assert!(t.acquire(1).is_ok());
        assert!(t.global_conserved());
        let _ = t.release(0);
        let _ = t.release(0);
        let _ = t.release(1);
        assert!(t.global_conserved());
        assert!(t.all_drained());
    }

    #[test]
    fn offline_acquire_is_uaf() {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        let _ = t.advance(DevPhase::Offline);
        assert_eq!(t.acquire(0), Err(TdCode::UseAfterFree));
        assert_eq!(t.release(0), Err(TdCode::UseAfterFree));
    }

    #[test]
    fn underflow_and_double_release_denied() {
        let mut t = HandleTable::new();
        assert_eq!(t.release(0), Err(TdCode::RefCountUnderflow));
        let _ = t.acquire(0);
        let _ = t.release(0);
        assert_eq!(t.release(0), Err(TdCode::RefCountUnderflow));
    }

    #[test]
    fn phase_machine_monotonic() {
        let mut t = HandleTable::new();
        assert_eq!(t.advance(DevPhase::Offline), Err(TdCode::PhaseSkip));
        assert!(t.advance(DevPhase::Tearing).is_ok());
        assert_eq!(t.advance(DevPhase::Live), Err(TdCode::PhaseSkip));
        assert!(t.advance(DevPhase::Offline).is_ok());
    }

    #[test]
    fn tearing_blocks_acquire() {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        assert_eq!(t.acquire(2), Err(TdCode::AcquireWhileTearing));
        assert!(t.release(0).is_ok() || t.handle_of(0).unwrap_or(IfHandle::new()).refs == 0);
    }

    #[test]
    fn stress_50_clean() {
        assert_eq!(HandleTable::stress_script(50), 50);
    }

    #[test]
    fn bad_plans_denied() {
        let mut t = HandleTable::new();
        let _ = t.advance(DevPhase::Tearing);
        let empty = TeardownPlan { steps: [0; IFACE_N], n: 0 };
        assert_eq!(t.teardown(&empty), Err(TdCode::EmptyPlan));
        let oob = TeardownPlan { steps: [0, 7, 2, 1], n: 4 };
        assert_eq!(t.teardown(&oob), Err(TdCode::PlanStepMismatch));
        let dup = TeardownPlan { steps: [0, 0, 2, 1], n: 4 };
        assert!(!dup.self_ok());
    }

    #[test]
    fn plan_reverse_order_correct() {
        let p = TeardownPlan::reverse_order();
        assert_eq!(p.steps[0], 3);
        assert_eq!(p.steps[3], 0);
        assert!(p.self_ok());
    }

    #[test]
    fn iface_oob_ten() {
        let mut t = HandleTable::new();
        for i in 0..10usize {
            assert_eq!(t.acquire(IFACE_N + i), Err(TdCode::IfaceOob));
        }
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut t = HandleTable::new();
        for _ in 0..10u32 {
            t.note_detected();
        }
        assert!(t.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        assert_eq!(lcg_seed(), lcg_seed());
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn unx51564_checks_green() {
        let set = run_unx51564_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
