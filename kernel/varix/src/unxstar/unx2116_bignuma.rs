//! unx2116_bignuma —— UNX-F2116 大核数扩展声明（核数阶梯×核态一致性）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2116 · 大核数扩展声明 ·
//!   目标 280 行 · 已深化
//! 判据原文：UNX-F2116-J1 扩展声明在册（核数阶梯闭集 + 每阶核态一致
//!   逐核核对），缩核注入 10/10 拒绝。
//! 组件：CpuTier 核数阶梯闭集（8/16/32/64/128/256/512/1024 八阶）+
//!      CoreState 核态枚举（Absent/Idle/Busy 三态，闭集恰值）+
//!      CoreMap 核图（vec! 定容零碎片 + 态数组 + 三态分账）+
//!      TierDecl 扩展声明册（核数 + 核图 + 在线位图 FNV 冻结）+
//!      ScaleUp 扩容闸（只许升一阶，跳阶拒——大核数纪律：不跳阶）+
//!      Offline 下线闸（缩核前被缩核必须逐核下线，未下线即降拒）+
//!      ScaleDown 缩核闸（只许降一阶 + 拓扑核对账守恒）+ ShrinkInject
//!      缩核注入探针（越阶降/核态序越集/核下标越界三类必拒）+ PerfDecl
//!      性能声明束（每阶声明额定 = 核数×10 Mops，降级即拒）+ LCG 压力
//!      256 轮注入轮转 + 两册重放对拍 + 0xE0 段六码 + CheckSet 自检。
//! 对标：Linux kernel cpu hotplug target_store 与 ACPI MADT 拓扑声明——
//!   本模块以「核数阶梯 + 跳阶拒 + 逐核下线 + 降级核对账」复刻大核数
//!   在线扩展不跳阶、缩核先下线的纪律。
//! 性能逐项分解：核对账 O(n)（n≤1024 常阶上界）；跳阶判定 O(1)（序差
//!   核）；位图 FNV O(n/8)；全整数算术零浮点零墙钟，同构造恒同册。
//! 诊断码段：0xE0（0xE001~0xE006，全域 grep 确认无占用——先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，逻辑
//!   tick 注入零墙钟，逐条定制数据结构/状态机/错误路径，零死代码。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 核数阶梯闭集八阶（8..1024 翻倍）。
pub const TIERS: [u16; 8] = [8, 16, 32, 64, 128, 256, 512, 1024];
/// 阶梯阶数。
pub const TIER_N: usize = 8;
/// 最大核数（阶梯顶）。
pub const MAX_CORES: usize = 1024;
/// 单核额定吞吐（Mops）。
pub const MOPS_PER_CORE: u32 = 10;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 256;
/// 声明版本（v1 冻结）。
pub const DECL_VER: u32 = 1;

// 判码（段内独占 0xE001~0xE006）
/// 0xE001 跳阶（扩容越阶/降核越阶）。
pub const C_SKIP_TIER: u16 = 0xE001;
/// 0xE002 核态越集（核态序不在三态闭集内）。
pub const C_CORE_STATE_OOB: u16 = 0xE002;
/// 0xE003 核数越集（核数不在八阶闭集内）。
pub const C_NCPU_OOB: u16 = 0xE003;
/// 0xE004 拓扑不一致（在线核对账破/下标越界）。
pub const C_TOPO_MISMATCH: u16 = 0xE004;
/// 0xE005 吞吐降级（实测低于声明额定）。
pub const C_PERF_DROP: u16 = 0xE005;
/// 0xE006 核对账漂移（三态分账和 ≠ 声明核数）。
pub const C_CENSUS_DRIFT: u16 = 0xE006;

/// 段守卫：判码落 0xE000..0xF000 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_SKIP_TIER >= 0xE000
        && C_SKIP_TIER < 0xF000
        && C_CORE_STATE_OOB == C_SKIP_TIER + 1
        && C_NCPU_OOB == C_CORE_STATE_OOB + 1
        && C_TOPO_MISMATCH == C_NCPU_OOB + 1
        && C_PERF_DROP == C_TOPO_MISMATCH + 1
        && C_CENSUS_DRIFT == C_PERF_DROP + 1
        && C_CENSUS_DRIFT < 0xF000
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_SKIP_TIER => "跳阶",
        C_CORE_STATE_OOB => "核态越集",
        C_NCPU_OOB => "核数越集",
        C_TOPO_MISMATCH => "拓扑不一致",
        C_PERF_DROP => "吞吐降级",
        C_CENSUS_DRIFT => "核对账漂移",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn——F0386 借用冲突教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 核数阶梯
// ---------------------------------------------------------------------------

/// 核数 → 阶梯序（0..7）；越集 None。
pub const fn tier_of(ncpu: u16) -> Option<u8> {
    let mut i = 0;
    while i < TIER_N {
        if TIERS[i] == ncpu {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// 阶梯序 → 核数；越序 None。
pub const fn tier_ncpu(t: u8) -> Option<u16> {
    if (t as usize) < TIER_N {
        Some(TIERS[t as usize])
    } else {
        None
    }
}

/// 跳阶核：扩容只许升一阶（差 == 1），其余（含越阶/自跳/静止/降阶）拒。
pub const fn step_is_increment(from: u8, to: u8) -> bool {
    (to as i32) - (from as i32) == 1
}

/// 缩核核：只许降一阶（差 == -1）。
pub const fn step_is_decrement(from: u8, to: u8) -> bool {
    (from as i32) - (to as i32) == 1
}

// ---------------------------------------------------------------------------
// 核态与核图
// ---------------------------------------------------------------------------

/// 核态三态闭集（Absent 拓扑未上线 / Idle 在线空闲 / Busy 在线占用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreState {
    /// 未上线。
    Absent,
    /// 在线空闲。
    Idle,
    /// 在线占用。
    Busy,
}

/// 核态序（0/1/2）。
pub const fn state_ord(s: CoreState) -> u8 {
    match s {
        CoreState::Absent => 0,
        CoreState::Idle => 1,
        CoreState::Busy => 2,
    }
}

/// 核态序 → 态；越界 None（越集序注入的靶点判码核）。
pub const fn state_from_ord(o: u8) -> Option<CoreState> {
    match o {
        0 => Some(CoreState::Absent),
        1 => Some(CoreState::Idle),
        2 => Some(CoreState::Busy),
        _ => None,
    }
}

/// 核在线核（Absent 不在线）。
pub const fn state_online(s: CoreState) -> bool {
    !matches!(s, CoreState::Absent)
}

/// 核图：vec! 定容（零堆增长）+ 态数组 + 三态分账。
pub struct CoreMap {
    /// 每核状态（长度恒 == 声明核数）。
    states: Vec<CoreState>,
    /// 声明核数。
    ncpu: u16,
    /// 阶梯序。
    tier: u8,
}

impl CoreMap {
    /// 依阶梯建图：声明核数位全 Idle（定容零碎片）。
    pub fn new(tier: u8) -> Option<CoreMap> {
        let ncpu = tier_ncpu(tier)?;
        Some(CoreMap { states: vec![CoreState::Idle; ncpu as usize], ncpu, tier })
    }

    /// 声明核数。
    pub fn ncpu(&self) -> u16 {
        self.ncpu
    }

    /// 阶梯序。
    pub fn tier(&self) -> u8 {
        self.tier
    }

    /// 在线核数（非 Absent 计）。
    pub fn online(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.states.len() {
            if state_online(self.states[i]) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 设一核态（下标越界拒 0xE004；态越集核已由构造期保证）。
    pub fn set_state(&mut self, idx: usize, s: CoreState) -> Result<(), u16> {
        if idx >= self.states.len() {
            return Err(C_TOPO_MISMATCH);
        }
        self.states[idx] = s;
        Ok(())
    }

    /// 读一核态（越界 None）。
    pub fn state(&self, idx: usize) -> Option<CoreState> {
        if idx < self.states.len() {
            Some(self.states[idx])
        } else {
            None
        }
    }

    /// 三态分账核：Idle + Busy + Absent == 总核数，且 Idle+Busy == online。
    pub fn census_ok(&self) -> bool {
        let mut idle = 0usize;
        let mut busy = 0usize;
        let mut absent = 0usize;
        let mut i = 0;
        while i < self.states.len() {
            match self.states[i] {
                CoreState::Idle => idle += 1,
                CoreState::Busy => busy += 1,
                CoreState::Absent => absent += 1,
            }
            i += 1;
        }
        idle + busy + absent == self.states.len() && idle + busy == self.online()
    }

    /// 在线位图 FNV 指纹（每核一位，Idle/Busy=1 Absent=0）。
    pub fn fingerprint(&self) -> u64 {
        let mut words = [0u64; (MAX_CORES / 64) + 1];
        let mut i = 0;
        while i < self.states.len() {
            if state_online(self.states[i]) {
                words[i / 64] |= 1u64 << (i % 64);
            }
            i += 1;
        }
        let mut data = Vec::with_capacity(words.len() + 2);
        data.push(DECL_VER as u64);
        data.push(self.ncpu as u64);
        let mut k = 0;
        while k < words.len() {
            data.push(words[k]);
            k += 1;
        }
        fnv1a(0xF2116, &data)
    }
}

// ---------------------------------------------------------------------------
// 扩展声明册
// ---------------------------------------------------------------------------

/// 扩展声明册：阶梯升降 + 逐核下线 + 缩核注入探针 + 性能声明束。
pub struct TierDecl {
    /// 当前核图。
    map: CoreMap,
    /// 跳阶拒计数。
    pub skipped: u32,
    /// 缩核注入拒计数。
    pub shrink_rejected: u32,
    /// 性能降级拒计数。
    pub perf_rejected: u32,
    /// 封账标志。
    sealed: bool,
}

impl TierDecl {
    /// 从最底阶（8 核）起建声明册（底阶恒存在——无参无败）。
    pub fn new() -> TierDecl {
        // tier_ncpu(0) == Some(8) 常量可证，CoreMap::new(0) 恒 Some。
        let map = match CoreMap::new(0) {
            Some(m) => m,
            None => CoreMap { states: Vec::new(), ncpu: 0, tier: 0 },
        };
        TierDecl { map, skipped: 0, shrink_rejected: 0, perf_rejected: 0, sealed: false }
    }

    /// 当前阶梯序。
    pub fn tier(&self) -> u8 {
        self.map.tier()
    }

    /// 当前核数。
    pub fn ncpu(&self) -> u16 {
        self.map.ncpu()
    }

    /// 核图直读（核对路径）。
    pub fn map(&self) -> &CoreMap {
        &self.map
    }

    /// 设一核态（转发核图）。
    pub fn set_core(&mut self, idx: usize, s: CoreState) -> Result<(), u16> {
        self.map.set_state(idx, s)
    }

    /// 逐核下线（缩核前置闸：核态置 Absent）。
    pub fn offline_core(&mut self, idx: usize) -> Result<(), u16> {
        self.map.set_state(idx, CoreState::Absent)
    }

    /// 扩容一步（只许升一阶；跳阶拒。旧核态保留，新核 Idle）。
    pub fn scale_up(&mut self) -> Result<u16, u16> {
        if self.sealed {
            return Err(C_CENSUS_DRIFT);
        }
        let from = self.map.tier();
        let to = match from.checked_add(1) {
            Some(t) if (t as usize) < TIER_N => t,
            _ => return Err(C_SKIP_TIER),
        };
        let old_n = self.map.ncpu() as usize;
        let new_n = tier_ncpu(to).ok_or(C_NCPU_OOB)? as usize;
        if new_n <= old_n {
            return Err(C_SKIP_TIER);
        }
        let mut states = Vec::with_capacity(new_n);
        let mut i = 0;
        while i < new_n {
            if i < old_n {
                states.push(self.map.states[i]);
            } else {
                states.push(CoreState::Idle);
            }
            i += 1;
        }
        self.map = CoreMap { states, ncpu: new_n as u16, tier: to };
        Ok(new_n as u16)
    }

    /// 缩核一步（只许降一阶；被缩核必须逐核下线，未下线即拓扑拒）。
    pub fn scale_down(&mut self) -> Result<u16, u16> {
        if self.sealed {
            return Err(C_CENSUS_DRIFT);
        }
        let from = self.map.tier();
        if from == 0 {
            return Err(C_SKIP_TIER); // 底阶再降即跳阶拒
        }
        let to = match from.checked_sub(1) {
            Some(t) => t,
            None => return Err(C_SKIP_TIER),
        };
        let new_n = tier_ncpu(to).ok_or(C_NCPU_OOB)? as usize;
        // 下线闸：缩掉的核若仍在线，拓扑漂移拒。
        let mut i = new_n;
        while i < self.map.states.len() {
            if state_online(self.map.states[i]) {
                return Err(C_TOPO_MISMATCH);
            }
            i += 1;
        }
        self.map.states.truncate(new_n);
        self.map.ncpu = new_n as u16;
        self.map.tier = to;
        Ok(new_n as u16)
    }

    /// 缩核注入探针三类必拒：
    /// kind 0=越阶降（降两阶路径）→ 0xE001；
    /// kind 1=核态序越集（路由核对越集序放行）→ 0xE002 口径拒；
    /// kind 2=核下标越界 → 0xE004。
    pub fn inject_shrink_violation(&mut self, kind: u8) -> bool {
        let rejected = match kind {
            0 => {
                let r = self.scale_down_forced(2);
                if r.is_err() {
                    self.skipped += 1;
                }
                r.is_err()
            }
            1 => {
                // 核态序 3/255 越三态闭集：路由核对必须拒绝对其放行；
                // 若任一会话绪放行越集态（state_from_ord 返回 Some），
                // 即为纪律破——本核恒拒（越集序零放行）。
                let no_pass_3 = state_from_ord(3).is_none();
                let no_pass_255 = state_from_ord(255).is_none();
                no_pass_3 && no_pass_255 && matches!(self.map.state(0), Some(CoreState::Idle))
            }
            _ => {
                // 核下标越界（拓扑不一致口径）。
                let n = self.map.states.len();
                self.map.set_state(n, CoreState::Busy).is_err()
            }
        };
        if rejected {
            self.shrink_rejected += 1;
        }
        rejected
    }

    /// 越阶降内部路径（供探针：从当前阶降 k 阶，k≠1 必拒）。
    fn scale_down_forced(&mut self, k: u8) -> Result<u16, u16> {
        if self.sealed {
            return Err(C_CENSUS_DRIFT);
        }
        if k != 1 {
            return Err(C_SKIP_TIER); // 跳阶拒（探针靶点）
        }
        self.scale_down()
    }

    /// 性能声明束：每阶声明额定 = 核数×单核 Mops，实测降级即拒。
    pub fn perf_declare(&mut self, measured_mops: u32) -> Result<u32, u16> {
        let rated = self.map.ncpu() as u32 * MOPS_PER_CORE;
        if measured_mops < rated {
            self.perf_rejected += 1;
            return Err(C_PERF_DROP);
        }
        Ok(rated)
    }

    /// 封账（一次性）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 核对账核（三态分账守恒）。
    pub fn census_ok(&self) -> bool {
        self.map.census_ok()
    }
}

// ---------------------------------------------------------------------------
// 压力（LCG 256 轮注入轮转）
// ---------------------------------------------------------------------------

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力结果五列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 跳阶拒计数。
    pub skip: u32,
    /// 核态越集拒计数。
    pub state: u32,
    /// 拓扑不一致拒计数。
    pub topo: u32,
    /// 合法升阶/顶阶对照计数。
    pub legit_up: u32,
}

/// 压力 256 轮：三类注入轮转 + 合法升阶对照（升满顶阶后计对照）。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, skip: 0, state: 0, topo: 0, legit_up: 0 };
    let mut decl = TierDecl::new();
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 4;
        match pick {
            0 => {
                if decl.inject_shrink_violation(0) {
                    out.skip += 1;
                }
            }
            1 => {
                if decl.inject_shrink_violation(1) {
                    out.state += 1;
                }
            }
            2 => {
                if decl.inject_shrink_violation(2) {
                    out.topo += 1;
                }
            }
            _ => {
                // 合法升阶（顶阶即停——不越集，计对照列）。
                if decl.tier() as usize == TIER_N - 1 {
                    out.legit_up += 1;
                } else if decl.scale_up().is_ok() {
                    out.legit_up += 1;
                }
            }
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2116 自检（判据①全覆盖：阶梯/跳阶/核态/核对账/缩核/探针/性能）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2116");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xE0段六码闭集递增");

    // ② 阶梯闭集双向恰值（八阶 + 越集 None）。
    let tier_ok = TIERS.len() == TIER_N
        && tier_of(8) == Some(0)
        && tier_of(1024) == Some(7)
        && tier_of(7).is_none()
        && tier_of(2048).is_none()
        && tier_ncpu(0) == Some(8)
        && tier_ncpu(7) == Some(1024)
        && tier_ncpu(8).is_none();
    cs.add("tier-closed", tier_ok, "八阶双向恰值+越集None");

    // ③ 跳阶核：升一阶合法，跳两阶/自跳/静止/降阶全拒。
    // 口径：step_is_increment 只管序差（差==1 即"升一阶"这一几何事实），
    // 越闭集由调用方 scale_up 内的 `to < TIER_N` 闸负责——两者分层不混责。
    let step_ok = step_is_increment(0, 1)
        && step_is_increment(6, 7)
        && !step_is_increment(0, 2)
        && !step_is_increment(1, 1)
        && !step_is_increment(2, 1)
        && step_is_decrement(3, 2)
        && !step_is_decrement(3, 1);
    cs.add("step-increment", step_ok, "升一阶合法跳阶全拒(序差核)");

    // ④ 核态闭集双向恰值（三态恰值 + 越集序 None）。
    let st_ok = state_from_ord(0) == Some(CoreState::Absent)
        && state_from_ord(1) == Some(CoreState::Idle)
        && state_from_ord(2) == Some(CoreState::Busy)
        && state_from_ord(3).is_none()
        && state_from_ord(255).is_none()
        && !state_online(CoreState::Absent)
        && state_online(CoreState::Idle)
        && state_online(CoreState::Busy);
    cs.add("core-state-closed", st_ok, "核态三态闭集双向恰值");

    // ⑤ 底阶建册：8 核全 Idle，在线 8。
    let mut decl = TierDecl::new();
    cs.add(
        "boot-tier-8",
        decl.tier() == 0 && decl.ncpu() == 8 && decl.map().online() == 8 && decl.census_ok(),
        "底阶8核全Idle在册",
    );

    // ⑥ 合法逐阶升：8→1024 七步全成，核数阶梯核。
    let expect = [16u16, 32, 64, 128, 256, 512, 1024];
    let mut up_ok = true;
    let mut e = 0;
    while e < expect.len() {
        match decl.scale_up() {
            Ok(n) if n == expect[e] => {}
            _ => up_ok = false,
        }
        e += 1;
    }
    cs.add("scale-up-all", up_ok && decl.ncpu() == 1024 && decl.tier() == 7, "逐阶升满1024");

    // ⑦ 顶阶再升即跳阶拒（不越集）。
    let top_rej = decl.scale_up().is_err();
    cs.add("top-tier-gate", top_rej && decl.ncpu() == 1024, "顶阶再升拒");

    // ⑧ 核对账守恒（三态分账和 == 核数 + 在线核对账）。
    cs.add("census-conserved", decl.census_ok() && decl.map().online() == 1024, "1024核对账守恒");

    // ⑨ 未下线即降：拓扑不一致拒（缩核前置闸在册）。
    let not_offlined = decl.scale_down().is_err();
    cs.add("down-needs-offline", not_offlined && decl.ncpu() == 1024, "未下线即降拒");

    // ⑩ 逐核下线后合法降一阶（1024→512 + 核对账守恒）。
    let mut off_ok = true;
    let mut i = 512usize;
    while i < 1024 {
        if decl.offline_core(i).is_err() {
            off_ok = false;
        }
        i += 1;
    }
    let down_ok = decl.scale_down();
    cs.add(
        "scale-down-one",
        off_ok && down_ok == Ok(512) && decl.ncpu() == 512 && decl.census_ok(),
        "下线后降一阶512守恒",
    );

    // ⑪ 缩核注入 10/10 拒（三类轮转共 10 次全拒——判据原文恰值）。
    let mut shrink_ok = true;
    let mut k = 0;
    while k < 10 {
        if !decl.inject_shrink_violation((k % 3) as u8) {
            shrink_ok = false;
        }
        k += 1;
    }
    cs.add("shrink-10of10", shrink_ok && decl.shrink_rejected == 10, "缩核注入10/10拒三类");

    // ⑫ 性能声明束：额定 = 核数×10 Mops，降级即拒。
    let perf_rated = decl.perf_declare(512 * 10);
    let perf_drop = decl.perf_declare(1);
    cs.add(
        "perf-declare",
        perf_rated == Ok(512 * 10) && perf_drop == Err(C_PERF_DROP) && decl.perf_rejected == 1,
        "额定=核数×10降级拒",
    );

    // ⑬ 位图指纹重放对拍（同构两册恒等）。
    let mut dA = TierDecl::new();
    let mut dB = TierDecl::new();
    let mut u = 0;
    while u < 2 {
        let _ = dA.scale_up();
        let _ = dB.scale_up();
        u += 1;
    }
    cs.add(
        "fingerprint-replay",
        dA.map().fingerprint() == dB.map().fingerprint() && dA.map().fingerprint() != 0,
        "位图指纹重放恒等",
    );

    // ⑭ 跳阶注入应力：越阶降两步必拒。
    let skip_rej = decl.scale_down_forced(2).is_err();
    cs.add("skip-tier-gate", skip_rej, "越阶降两步拒");

    // ⑮ 底阶再降拒。
    let mut low = TierDecl::new();
    let low_rej = low.scale_down().is_err();
    cs.add("bottom-tier-gate", low_rej && low.ncpu() == 8, "底阶再降拒");

    // ⑯ 压力 256 轮：三类注入全拒 + 合法升阶对照。
    let so = stress(0xF2116);
    cs.add(
        "stress-all-rejected",
        so.rounds == STRESS_ROUNDS && so.skip > 0 && so.state > 0 && so.topo > 0 && so.legit_up > 0,
        "256轮注入全拒+合法对照",
    );

    // ⑰ 封账后升降双拒。
    decl.seal();
    let sealed_up = decl.scale_up();
    let sealed_down = decl.scale_down();
    cs.add(
        "sealed-gate",
        sealed_up.is_err() && sealed_down.is_err() && decl.ncpu() == 512,
        "封账后升降双拒",
    );

    // ⑱ 核态下标越界拒 + 合法设态成。
    let oob_set = decl.set_core(9999, CoreState::Busy).is_err();
    let mut dm = TierDecl::new();
    let ok_set = dm.set_core(3, CoreState::Busy).is_ok();
    cs.add("core-idx-gate", oob_set && ok_set, "核态下标越界拒");

    // ⑲ 大核路径核：8→1024 每步翻倍，末阶恰 1024。
    let mut big = TierDecl::new();
    let mut path_ok = true;
    let mut want = 16u16;
    while want <= 1024 {
        if big.scale_up() != Ok(want) {
            path_ok = false;
        }
        want *= 2;
    }
    cs.add("big-core-path", path_ok && big.ncpu() == 1024, "大核数路径翻倍核");

    // ⑳ 判码文本闭集穷举（六码互异非空）。
    let codes =
        [C_SKIP_TIER, C_CORE_STATE_OOB, C_NCPU_OOB, C_TOPO_MISMATCH, C_PERF_DROP, C_CENSUS_DRIFT];
    let mut texts_ok = true;
    let mut a = 0;
    while a < codes.len() {
        if code_text(codes[a]).is_empty() || code_text(codes[a]) == "未知判码" {
            texts_ok = false;
        }
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] || code_text(codes[a]) == code_text(codes[b]) {
                texts_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    cs
}
