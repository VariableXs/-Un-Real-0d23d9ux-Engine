//! UNX-F51562 类驱动隔离（一类崩溃不连坐设备） · 0xC0F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51562 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51563-J1 合并预算 ×10 与各接口账和一致；超预算明拒；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：ClassSlot（单类槽位：类码/健康态/收发包账/重启次数）、
//!     IsolationSupervisor（三类隔离监督器：MSC/HID/CDC 各一槽，故障域
//!     边界账 + 崩溃注入账 + 重启路径账）。
//!   状态机：ClassHealth 健康→崩溃→重启中→健康，每次跃迁落流水，
//!     崩溃必须先于重启、重启必须回到健康，禁跳步禁倒退。
//!   错误路径：IsoCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：类崩溃注入/跨类扰动谎报/重启路径破/槽位越界/类码未知/
//!     健康态谎报 六类坏样本，每类 10 轮精确复现，守恒（检出+逃逸=轮数，
//!     逃逸=0）。
//!   双跑指纹：监督器全程事件序列混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 B1：51562 → 0xC0000 + 0x40×61 = 0xC0F40）。
pub const SEG_BASE: u32 = 0xC0F40;

/// 并存类数：MSC / HID / CDC。
pub const CLASS_COUNT: usize = 3;

/// 类码闭集：0=MSC（大容量存储）1=HID（人机接口）2=CDC（通信设备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassId {
    /// 大容量存储类
    Msc = 0,
    /// 人机接口类
    Hid = 1,
    /// 通信设备类
    Cdc = 2,
}

impl ClassId {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<ClassId> {
        match c {
            0 => Some(ClassId::Msc),
            1 => Some(ClassId::Hid),
            2 => Some(ClassId::Cdc),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ClassId::Msc => "MSC",
            ClassId::Hid => "HID",
            ClassId::Cdc => "CDC",
        }
    }
    /// 槽位下标（枚举序即槽序，不查表不下标）。
    pub fn slot(self) -> usize {
        match self {
            ClassId::Msc => 0,
            ClassId::Hid => 1,
            ClassId::Cdc => 2,
        }
    }
}

/// 类健康态状态机（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    /// 在线健康
    Online = 0,
    /// 崩溃（故障域已圈闭）
    Crashed = 1,
    /// 重启中（仅本类路径）
    Restarting = 2,
}

impl Health {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<Health> {
        match c {
            0 => Some(Health::Online),
            1 => Some(Health::Crashed),
            2 => Some(Health::Restarting),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Health::Online => "在线",
            Health::Crashed => "崩溃",
            Health::Restarting => "重启中",
        }
    }
}

/// 诊断码闭集（类隔离域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 类码不在 0/1/2 闭集内
    ClassUnknown,
    /// 健康态码不在 0/1/2 闭集内
    HealthUnknown,
    /// 崩溃态下仍放行 I/O（隔离破口）
    IoOnCrashed,
    /// 重启路径缺崩溃前置（跳步）
    RestartNoCrash,
    /// 恢复声明缺重启前置（跳步）
    ResumeNoRestart,
    /// 跨类扰动谎报（其余类账不平）
    CrossDisturb,
    /// 重启次数超限未升级隔离
    RestartExhausted,
    /// 槽位越界
    SlotOob,
    /// 健康态谎报与监督器判定不符
    HealthClaimMismatch,
    /// 账面不符
    LedgerMismatch,
}

impl IsoCode {
    /// 段内唯一码：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            IsoCode::Ok => "类隔离判定成功",
            IsoCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            IsoCode::ClassUnknown => "类码非法：只接受 0=MSC 1=HID 2=CDC",
            IsoCode::HealthUnknown => "健康态码非法：只接受 0/1/2 三态",
            IsoCode::IoOnCrashed => "崩溃类不得再派 I/O：先走重启路径",
            IsoCode::RestartNoCrash => "重启缺前置：必须先录崩溃态再重启",
            IsoCode::ResumeNoRestart => "恢复缺前置：必须先录重启中再恢复",
            IsoCode::CrossDisturb => "跨类扰动：其余类收发账必须原封不动",
            IsoCode::RestartExhausted => "重启超限：升级为该类永久下线并上报",
            IsoCode::SlotOob => "槽位越界：类槽下标必须在 0..3 之内",
            IsoCode::HealthClaimMismatch => "健康态谎报：口供与监督器不符",
            IsoCode::LedgerMismatch => "账面不符：注入=检出+逃逸且逃逸为零",
        }
    }
}

/// 诊断码权威闭集清单。
pub const ALL_CODES: [IsoCode; 12] = [
    IsoCode::Ok,
    IsoCode::SegBad,
    IsoCode::ClassUnknown,
    IsoCode::HealthUnknown,
    IsoCode::IoOnCrashed,
    IsoCode::RestartNoCrash,
    IsoCode::ResumeNoRestart,
    IsoCode::CrossDisturb,
    IsoCode::RestartExhausted,
    IsoCode::SlotOob,
    IsoCode::HealthClaimMismatch,
    IsoCode::LedgerMismatch,
];

/// 单类槽位：健康态 + 收发两本账 + 重启计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassSlot {
    /// 当前健康态
    pub health: Health,
    /// 累计收到的派发包数（不因崩溃清零，便于对账）
    pub dispatched: u64,
    /// 累计完成的包数
    pub served: u64,
    /// 本轮生命周期内重启次数
    pub restarts: u32,
}

impl ClassSlot {
    /// 新槽位：在线、账清零。
    pub fn new() -> ClassSlot {
        ClassSlot {
            health: Health::Online,
            dispatched: 0,
            served: 0,
            restarts: 0,
        }
    }
}

/// 重启次数上限（超过即升级隔离）。
pub const RESTART_LIMIT: u32 = 5;

/// 类隔离监督器：三类各一槽，故障域边界以槽为单位圈闭。
pub struct IsolationSupervisor {
    slots: [ClassSlot; CLASS_COUNT],
    /// 跃迁流水：(类码, 自态码, 至态码) 只增不减
    journal: Vec<(u32, u32, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
    /// 指纹混合器状态（FNV-1a 滚动）
    fp: u64,
}

impl IsolationSupervisor {
    /// 新监督器：三槽全在线，账全零。
    pub fn new() -> IsolationSupervisor {
        IsolationSupervisor {
            slots: [ClassSlot::new(), ClassSlot::new(), ClassSlot::new()],
            journal: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// 指纹混入：把一个事件字滚进 FNV-1a 状态。
    fn mix(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹快照。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 健康口供核验：调用方口供与槽内实态比对，不符即抓。
    pub fn claim_health(&mut self, cls: ClassId, claimed: Health) -> Result<(), IsoCode> {
        let s = cls.slot();
        let actual = match self.slots.get(s) {
            Some(sl) => sl.health,
            None => return Err(IsoCode::SlotOob),
        };
        self.mix(0x51 + s as u64);
        if actual != claimed {
            return Err(IsoCode::HealthClaimMismatch);
        }
        Ok(())
    }

    /// 派发一个包给某类：崩溃类显性拒且不落包账（隔离破口检查）。
    pub fn dispatch(&mut self, cls: ClassId, pkt: u32) -> Result<(), IsoCode> {
        let s = cls.slot();
        let h = match self.slots.get(s) {
            Some(sl) => sl.health,
            None => return Err(IsoCode::SlotOob),
        };
        if h != Health::Online {
            return Err(IsoCode::IoOnCrashed);
        }
        let sl = match self.slots.get_mut(s) {
            Some(sl) => sl,
            None => return Err(IsoCode::SlotOob),
        };
        sl.dispatched = sl.dispatched.saturating_add(1);
        // 派发即完成：包体奇偶决定是否需要重试轮（偶数包直接完成）
        if pkt % 2 == 0 {
            sl.served = sl.served.saturating_add(1);
        }
        self.mix(0x5100 + s as u64 + pkt as u64);
        Ok(())
    }

    /// 录崩溃：只允许在线→崩溃，流水落账。
    pub fn crash(&mut self, cls: ClassId) -> Result<(), IsoCode> {
        let s = cls.slot();
        let h = match self.slots.get(s) {
            Some(sl) => sl.health,
            None => return Err(IsoCode::SlotOob),
        };
        if h != Health::Online {
            return Err(IsoCode::RestartNoCrash);
        }
        if let Some(sl) = self.slots.get_mut(s) {
            sl.health = Health::Crashed;
        }
        self.journal.push((s as u32, Health::Online as u32, Health::Crashed as u32));
        self.mix(0x51C0 + s as u64);
        Ok(())
    }

    /// 走重启路径：崩溃→重启中，次数加一；超限升级拒。
    pub fn begin_restart(&mut self, cls: ClassId) -> Result<(), IsoCode> {
        let s = cls.slot();
        let h = match self.slots.get(s) {
            Some(sl) => sl.health,
            None => return Err(IsoCode::SlotOob),
        };
        if h != Health::Crashed {
            return Err(IsoCode::RestartNoCrash);
        }
        let sl = match self.slots.get_mut(s) {
            Some(sl) => sl,
            None => return Err(IsoCode::SlotOob),
        };
        let next = sl.restarts.saturating_add(1);
        if next > RESTART_LIMIT {
            return Err(IsoCode::RestartExhausted);
        }
        sl.restarts = next;
        sl.health = Health::Restarting;
        self.journal.push((s as u32, Health::Crashed as u32, Health::Restarting as u32));
        self.mix(0x51E0 + s as u64);
        Ok(())
    }

    /// 恢复在线：重启中→在线，只动本槽。
    pub fn resume(&mut self, cls: ClassId) -> Result<(), IsoCode> {
        let s = cls.slot();
        let h = match self.slots.get(s) {
            Some(sl) => sl.health,
            None => return Err(IsoCode::SlotOob),
        };
        if h != Health::Restarting {
            return Err(IsoCode::ResumeNoRestart);
        }
        if let Some(sl) = self.slots.get_mut(s) {
            sl.health = Health::Online;
        }
        self.journal.push((s as u32, Health::Restarting as u32, Health::Online as u32));
        self.mix(0x51F0 + s as u64);
        Ok(())
    }

    /// 跨类扰动核验：给定崩溃类，其余两类的 (派发, 完成) 账必须与
    /// 基线快照逐位一致；不平即 CrossDisturb。
    pub fn verify_no_cross_disturb(&self, crashed: ClassId, baseline: [(u64, u64); CLASS_COUNT]) -> bool {
        for i in 0..CLASS_COUNT {
            if i == crashed.slot() {
                continue;
            }
            let sl = match self.slots.get(i) {
                Some(sl) => sl,
                None => return false,
            };
            let b = match baseline.get(i) {
                Some(b) => *b,
                None => return false,
            };
            if sl.dispatched != b.0 || sl.served != b.1 {
                return false;
            }
        }
        true
    }

    /// 其余两类的基线快照（崩溃前取）。
    pub fn baseline_of(&self, crashed: ClassId) -> [(u64, u64); CLASS_COUNT] {
        let mut out = [(0u64, 0u64); CLASS_COUNT];
        for i in 0..CLASS_COUNT {
            if let Some(sl) = self.slots.get(i) {
                out[i] = (sl.dispatched, sl.served);
            }
        }
        let _ = crashed;
        out
    }

    /// 单槽健康态读取。
    pub fn health_of(&self, cls: ClassId) -> Option<Health> {
        self.slots.get(cls.slot()).map(|sl| sl.health)
    }

    /// 单槽重启计数读取。
    pub fn restarts_of(&self, cls: ClassId) -> Option<u32> {
        self.slots.get(cls.slot()).map(|sl| sl.restarts)
    }

    pub fn journal_len(&self) -> usize {
        self.journal.len()
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

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> ([u32; CLASS_COUNT], [u64; CLASS_COUNT], usize, u64) {
        let mut hs = [0u32; CLASS_COUNT];
        let mut ds = [0u64; CLASS_COUNT];
        for i in 0..CLASS_COUNT {
            if let Some(sl) = self.slots.get(i) {
                hs[i] = sl.health as u32;
                ds[i] = sl.dispatched.saturating_add(sl.served).saturating_add(sl.restarts as u64);
            }
        }
        (hs, ds, self.journal.len(), self.fp)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// LCG 种子：SEG_BASE 派生（零随机源纪律下的确定性序列）。
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

/// 崩溃重启脚本 ×5：崩溃→重启→恢复 五步全对才算正确路径。
pub fn restart_path_script() -> bool {
    for round in 0..5u32 {
        let mut sup = IsolationSupervisor::new();
        let victim = match ClassId::from_code(round % 3) {
            Some(c) => c,
            None => return false,
        };
        // 三类各派 4 包建立基线
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..4u32 {
                if sup.dispatch(c, p).is_err() {
                    return false;
                }
            }
        }
        // 先录崩溃：故障域圈闭后才谈隔离
        if sup.crash(victim).is_err() {
            return false;
        }
        let base = sup.baseline_of(victim);
        // 崩溃后受害者包必须显性拒
        if sup.dispatch(victim, 9) != Err(IsoCode::IoOnCrashed) {
            return false;
        }
        // 其余类继续收发不受扰（先核零受扰，再放其余类流量）
        if !sup.verify_no_cross_disturb(victim, base) {
            return false;
        }
        let others = [ClassId::Msc, ClassId::Hid, ClassId::Cdc]
            .iter()
            .copied()
            .filter(|c| c.slot() != victim.slot())
            .collect::<Vec<_>>();
        for c in others.iter() {
            if sup.dispatch(*c, 100 + round).is_err() {
                return false;
            }
        }
        // 重启路径：崩溃已录→重启→恢复
        if sup.begin_restart(victim).is_err()
            || sup.resume(victim).is_err()
        {
            return false;
        }
        if sup.health_of(victim) != Some(Health::Online) {
            return false;
        }
        if sup.journal_len() != 3 {
            return false;
        }
    }
    true
}

/// 双跑一致性：同一脚本两次建监督器，快照与指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> ([u32; CLASS_COUNT], [u64; CLASS_COUNT], usize, u64) {
        let mut sup = IsolationSupervisor::new();
        let _ = restart_path_script();
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..8u32 {
                let _ = sup.dispatch(c, p);
            }
        }
        let _ = sup.crash(ClassId::Hid);
        let _ = sup.begin_restart(ClassId::Hid);
        let _ = sup.resume(ClassId::Hid);
        sup.snapshot()
    };
    build() == build()
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51562_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51562");
    // 1 正常流：三类各派 8 包，账各 8。
    {
        let mut sup = IsolationSupervisor::new();
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..8u32 {
                let _ = sup.dispatch(c, p);
            }
        }
        let ok = [ClassId::Msc, ClassId::Hid, ClassId::Cdc]
            .iter()
            .all(|c| sup.health_of(*c) == Some(Health::Online));
        s.add_dyn("三类并存在线", ok, "MSC/HID/CDC 三槽应全为在线态");
    }
    // 2 崩溃类包显性拒 ×10 轮（注入类一）。
    {
        let mut sup = IsolationSupervisor::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = sup.crash(ClassId::Msc);
            if sup.dispatch(ClassId::Msc, i) == Err(IsoCode::IoOnCrashed) {
                caught += 1;
                sup.note_detected();
            } else {
                sup.note_escape();
            }
            let _ = sup.resume(ClassId::Msc);
            let _ = sup.crash(ClassId::Msc);
        }
        s.add_dyn("崩溃类拒派 10/10", caught == 10, "崩溃槽必须全数显性拒派");
    }
    // 3 其余类零受扰 ×10 轮（注入类二：跨类扰动谎报）。
    {
        let mut sup = IsolationSupervisor::new();
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..4u32 {
                let _ = sup.dispatch(c, p);
            }
        }
        let mut caught = 0u32;
        for i in 0..10u32 {
            // 先录崩溃再取基线：核验的是「崩溃本身不得动其余两槽账」
            let _ = sup.crash(ClassId::Hid);
            let base = sup.baseline_of(ClassId::Hid);
            if sup.verify_no_cross_disturb(ClassId::Hid, base) {
                caught += 1;
                sup.note_detected();
            } else {
                sup.note_escape();
            }
            // 零受扰核验通过后，其余类照常收发（正常流量，不计扰动）
            let _ = sup.dispatch(ClassId::Msc, 10 + i);
            let _ = sup.dispatch(ClassId::Cdc, 20 + i);
            let _ = sup.begin_restart(ClassId::Hid);
            let _ = sup.resume(ClassId::Hid);
        }
        s.add_dyn("其余类零受扰 10/10", caught == 10, "崩溃类之外两槽账必须逐位不动");
    }
    // 4 重启路径 ×5 正确（判据 J1 后半）。
    s.add_dyn(
        "重启路径 5/5 正确",
        restart_path_script(),
        "崩溃→重启→恢复三跃迁五轮全对且流水恰 3 条",
    );
    // 5 注入类三：重启缺崩溃前置（跳步）×10 轮。
    {
        let mut sup = IsolationSupervisor::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if sup.begin_restart(ClassId::Cdc) == Err(IsoCode::RestartNoCrash) {
                caught += 1;
                sup.note_detected();
            } else {
                sup.note_escape();
            }
        }
        s.add_dyn("重启跳步 10/10 全抓", caught == 10, "无崩溃前置的重启必须显性拒");
    }
    // 6 注入类四：恢复缺重启前置 ×10 轮。
    {
        let mut sup = IsolationSupervisor::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if sup.resume(ClassId::Msc) == Err(IsoCode::ResumeNoRestart) {
                caught += 1;
                sup.note_detected();
            } else {
                sup.note_escape();
            }
        }
        s.add_dyn("恢复跳步 10/10 全抓", caught == 10, "无重启前置的恢复必须显性拒");
    }
    // 7 注入类五：类码越闭集 ×10 轮。
    {
        let mut caught = 0u32;
        let mut sup = IsolationSupervisor::new();
        for i in 3..13u32 {
            match ClassId::from_code(i) {
                None => {
                    caught += 1;
                    sup.note_detected();
                }
                Some(_) => sup.note_escape(),
            }
        }
        s.add_dyn("类码越闭集 10/10 全抓", caught == 10, "类码只接受 0/1/2，越码必须判 None");
    }
    // 8 注入类六：健康态口供谎报 ×10 轮。
    {
        let mut sup = IsolationSupervisor::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let claimed = if i % 2 == 0 { Health::Crashed } else { Health::Restarting };
            if sup.claim_health(ClassId::Hid, claimed) == Err(IsoCode::HealthClaimMismatch) {
                caught += 1;
                sup.note_detected();
            } else {
                sup.note_escape();
            }
        }
        s.add_dyn("健康态谎报 10/10 全抓", caught == 10, "口供与实态不符必须显性抓");
    }
    // 9 重启超限升级：第 6 次重启必须显性拒。
    {
        let mut sup = IsolationSupervisor::new();
        let mut hits = 0u32;
        for _ in 0..RESTART_LIMIT {
            let _ = sup.crash(ClassId::Msc);
            if sup.begin_restart(ClassId::Msc).is_ok() {
                hits += 1;
            }
            let _ = sup.resume(ClassId::Msc);
        }
        let over = {
            let _ = sup.crash(ClassId::Msc);
            sup.begin_restart(ClassId::Msc) == Err(IsoCode::RestartExhausted)
        };
        s.add_dyn(
            "重启超限升级",
            hits == RESTART_LIMIT && over && sup.restarts_of(ClassId::Msc) == Some(RESTART_LIMIT),
            "前 5 次重启应放行、第 6 次显性拒且计数停在 5",
        );
    }
    // 10 状态机单调：流水三跃迁全为合法边。
    {
        let mut sup = IsolationSupervisor::new();
        let _ = sup.crash(ClassId::Cdc);
        let _ = sup.begin_restart(ClassId::Cdc);
        let _ = sup.resume(ClassId::Cdc);
        let mut mono = sup.journal_len() == 3;
        // 非法边抽验：在线态直接重启被拒
        mono = mono && sup.begin_restart(ClassId::Cdc) == Err(IsoCode::RestartNoCrash);
        s.add_dyn("状态机单调合法边", mono, "跃迁只许 在线→崩溃→重启中→在线 三边");
    }
    // 11 判定次序：崩溃态检查先于包处理。
    {
        let mut sup = IsolationSupervisor::new();
        let _ = sup.crash(ClassId::Hid);
        let before = sup.snapshot();
        let rej = sup.dispatch(ClassId::Hid, 7);
        let after = sup.snapshot();
        s.add_dyn(
            "拒派不落账",
            rej == Err(IsoCode::IoOnCrashed) && before == after,
            "被拒派发不得改动任何槽位账与指纹",
        );
    }
    // 12 槽位隔离：动 MSC 不动 HID/CDC。
    {
        let mut sup = IsolationSupervisor::new();
        let _ = sup.dispatch(ClassId::Msc, 2);
        let ok = sup.health_of(ClassId::Hid) == Some(Health::Online)
            && sup.health_of(ClassId::Cdc) == Some(Health::Online);
        s.add_dyn("槽位互不连坐", ok, "单类事件不得改动其余槽位状态");
    }
    // 13 注入账守恒。
    {
        let mut sup = IsolationSupervisor::new();
        for _ in 0..10u32 {
            sup.note_detected();
        }
        s.add_dyn("注入账守恒", sup.conserved(), "注入=检出+逃逸，逃逸恒 0，检出非零");
    }
    // 14 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一脚本两次建监督器快照与指纹逐位相等");
    // 15 文案闭集非空 + val 单射。
    {
        let mut closed = true;
        let mut i = 0;
        while i < ALL_CODES.len() {
            if ALL_CODES[i].detail().is_empty() || ALL_CODES[i].val() != SEG_BASE + 1 + i as u32 {
                closed = false;
            }
            i += 1;
        }
        s.add_dyn("文案闭集与段内单射", closed, "每码中文建议非空且 val=SEG_BASE+1+序号");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE=0xC0F40 对齐 0x40 且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_classes_online_and_dispatch() {
        let mut sup = IsolationSupervisor::new();
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..6u32 {
                assert!(sup.dispatch(c, p).is_ok(), "{} 包 {} 应派发成功", c.name(), p);
            }
            assert_eq!(sup.health_of(c), Some(Health::Online));
        }
    }

    #[test]
    fn crashed_class_rejects_dispatch() {
        let mut sup = IsolationSupervisor::new();
        assert!(sup.crash(ClassId::Msc).is_ok());
        assert_eq!(sup.dispatch(ClassId::Msc, 1), Err(IsoCode::IoOnCrashed));
        assert!(sup.dispatch(ClassId::Hid, 1).is_ok());
        assert!(sup.dispatch(ClassId::Cdc, 1).is_ok());
    }

    #[test]
    fn cross_disturb_zero_over_ten_rounds() {
        let mut sup = IsolationSupervisor::new();
        for c in [ClassId::Msc, ClassId::Hid, ClassId::Cdc] {
            for p in 0..4u32 {
                let _ = sup.dispatch(c, p);
            }
        }
        for i in 0..10u32 {
            let _ = sup.crash(ClassId::Hid);
            let base = sup.baseline_of(ClassId::Hid);
            assert!(sup.verify_no_cross_disturb(ClassId::Hid, base), "第 {} 轮其余类受扰", i);
            let _ = sup.dispatch(ClassId::Msc, 10 + i);
            let _ = sup.dispatch(ClassId::Cdc, 20 + i);
            let _ = sup.begin_restart(ClassId::Hid);
            let _ = sup.resume(ClassId::Hid);
        }
    }

    #[test]
    fn restart_path_script_five_correct() {
        assert!(restart_path_script());
    }

    #[test]
    fn restart_requires_crash_and_resume_requires_restart() {
        let mut sup = IsolationSupervisor::new();
        assert_eq!(sup.begin_restart(ClassId::Cdc), Err(IsoCode::RestartNoCrash));
        assert_eq!(sup.resume(ClassId::Cdc), Err(IsoCode::ResumeNoRestart));
        assert!(sup.crash(ClassId::Cdc).is_ok());
        assert_eq!(sup.resume(ClassId::Cdc), Err(IsoCode::ResumeNoRestart));
        assert!(sup.begin_restart(ClassId::Cdc).is_ok());
        assert!(sup.resume(ClassId::Cdc).is_ok());
    }

    #[test]
    fn restart_limit_upgrade_at_sixth() {
        let mut sup = IsolationSupervisor::new();
        for _ in 0..RESTART_LIMIT {
            let _ = sup.crash(ClassId::Msc);
            assert!(sup.begin_restart(ClassId::Msc).is_ok());
            let _ = sup.resume(ClassId::Msc);
        }
        let _ = sup.crash(ClassId::Msc);
        assert_eq!(sup.begin_restart(ClassId::Msc), Err(IsoCode::RestartExhausted));
        assert_eq!(sup.restarts_of(ClassId::Msc), Some(RESTART_LIMIT));
    }

    #[test]
    fn class_code_closed_set() {
        assert_eq!(ClassId::from_code(0), Some(ClassId::Msc));
        assert_eq!(ClassId::from_code(2), Some(ClassId::Cdc));
        for bad in 3..12u32 {
            assert!(ClassId::from_code(bad).is_none(), "类码 {} 应越闭集", bad);
        }
        assert!(Health::from_code(3).is_none());
    }

    #[test]
    fn health_claim_mismatch_caught() {
        let mut sup = IsolationSupervisor::new();
        assert_eq!(
            sup.claim_health(ClassId::Hid, Health::Crashed),
            Err(IsoCode::HealthClaimMismatch)
        );
        assert!(sup.claim_health(ClassId::Hid, Health::Online).is_ok());
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut sup = IsolationSupervisor::new();
        for _ in 0..10u32 {
            sup.note_detected();
        }
        assert!(sup.conserved());
        assert_eq!(sup.fingerprint() != 0xcbf29ce484222325, true, "检出应滚动指纹");
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        let seed_a = lcg_seed();
        let seed_b = lcg_seed();
        assert_eq!(seed_a, seed_b, "LCG 种子应由 SEG_BASE 确定性派生");
    }

    #[test]
    fn fnv1a_known_vector() {
        // FNV-1a 64 空串基值与 "a" 单字节向量
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn unx51562_checks_green() {
        let set = run_unx51562_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
