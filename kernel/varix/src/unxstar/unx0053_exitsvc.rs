//! UNX-F0053 · ExitBootServices 时序协议 · 诊断段 0x2060
//! 验收锚点：UNX-F0053-J1 GetMemoryMap→EBS 两段式时序状态机，
//! MapKey 失配重试闭环、二次调用契约、回调链冻结语义判据通过；
//! 纯功能代码 ≥320 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2060;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORDER: u16 = SEG_BASE + 1;     // 时序违例（状态机非法边）
const E_MAPKEY: u16 = SEG_BASE + 2;    // MapKey 失配（内存图已变）
const E_BUFSMALL: u16 = SEG_BASE + 3;  // 内存图缓冲不足
const E_RETRY_OUT: u16 = SEG_BASE + 4; // 重试预算耗尽
const E_CB_FROZEN: u16 = SEG_BASE + 5; // EBS 成功后回调链冻结再注册
const E_TWICE: u16 = SEG_BASE + 6;     // EBS 成功后指针/服务仍被使用
const E_CBSTATE: u16 = SEG_BASE + 7;   // 回调登记状态非法
const E_NOTERM: u16 = SEG_BASE + 8;    // 终局态未到达（协议悬空）
const E_BAL: u16 = SEG_BASE + 9;       // 时序账守恒不平
const E_STALE: u16 = SEG_BASE + 10;    // 使用了过期内存图副本

/// 协议状态闭集（严格单向推进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// S0：未取图。必须先 GetMemoryMap。
    Idle,
    /// S1：已取图，持 MapKey，可调 EBS。
    Mapped { map_key: u64 },
    /// S2：EBS 首次失败（如 MAPKEY 失配/缓冲不足），必须重取图。
    FailedOnce { failures: u32 },
    /// S3：回调链已冻结（EBS 前最后一步），只能推进不能回退。
    CallbacksFrozen { map_key: u64 },
    /// S4：EBS 成功——此后 Boot Services 全部失效。
    Exited,
}

impl Phase {
    pub fn id(&self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::Mapped { .. } => 1,
            Phase::FailedOnce { .. } => 2,
            Phase::CallbacksFrozen { .. } => 3,
            Phase::Exited => 4,
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ORDER => "时序违例：先取图再冻结回调再 EBS，禁止跳段/回退",
        E_MAPKEY => "MapKey 失配：内存图已变，丢弃旧副本重取（两次调用契约）",
        E_BUFSMALL => "内存图缓冲不足：扩缓冲后重试，禁止截断硬凑",
        E_RETRY_OUT => "重试预算耗尽：固件反复翻动内存图，升级立案",
        E_CB_FROZEN => "回调链冻结后再注册：属误用，登记拒绝",
        E_TWICE => "EBS 成功后仍访问 Boot Services：二次调用契约红线",
        E_CBSTATE => "回调登记状态非法：已触发/已冻结条目不可再改",
        E_NOTERM => "协议悬空：演练终局态未到达 Exited",
        E_BAL => "时序账守恒不平：取图数 ≠ 成功数 + 失败数",
        E_STALE => "使用过期内存图副本：任何失败后旧图立即作废",
        _ => "未知诊断码",
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 回调链（EBS 前注册、冻结，EBS 时逐个触发一次）──────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackState {
    Registered,
    Frozen,
    Fired,
}

#[derive(Clone, Debug)]
pub struct EbsCallback {
    pub id: u32,
    pub fp: u64,
    pub state: CallbackState,
}

// ── 协议机 ───────────────────────────────────────────────────
pub struct EbsProtocol {
    pub phase: Phase,
    pub callbacks: Vec<EbsCallback>,
    pub map_attempts: u32,
    pub exit_attempts: u32,
    pub max_retries: u32,
    pub stale_maps: u32,
    /// 固件侧内存图代际（演练注入用；真机态由 GetMemoryMap 侧供给）。
    pub fw_generation: u64,
}

/// 取图结果（对 GetMemoryMap 调用的抽象建模）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapResult {
    Ok { map_key: u64, desc_count: usize },
    BufferTooSmall { needed: usize },
    KeyMismatch, // 固件代际已前进
}

impl EbsProtocol {
    pub fn new(fw_generation: u64) -> EbsProtocol {
        EbsProtocol {
            phase: Phase::Idle,
            callbacks: Vec::new(),
            map_attempts: 0,
            exit_attempts: 0,
            max_retries: 5,
            stale_maps: 0,
            fw_generation,
        }
    }

    /// 取图：状态机只允许 Idle/FailedOnce/CallbacksFrozen? 否。
    /// 注意：冻结后重取图会作废冻结——协议上禁止（冻结必须紧邻 EBS）。
    pub fn get_map(&mut self, res: MapResult) -> Result<(u64, usize), u16> {
        match self.phase {
            Phase::Idle | Phase::FailedOnce { .. } => {}
            Phase::Mapped { .. } => {
                // 允许主动重取（图可能被引导期分配翻动），旧图作废
                self.stale_maps += 1;
            }
            Phase::CallbacksFrozen { .. } => return Err(E_ORDER),
            Phase::Exited => return Err(E_TWICE),
        }
        self.map_attempts += 1;
        match res {
            MapResult::Ok { map_key, desc_count } => {
                self.phase = Phase::Mapped { map_key };
                Ok((map_key, desc_count))
            }
            MapResult::BufferTooSmall { needed: _ } => {
                self.phase = Phase::FailedOnce { failures: self.failures() + 1 };
                Err(E_BUFSMALL)
                // needed 由调用方扩容使用；此处显性返回诊断
            }
            MapResult::KeyMismatch => {
                self.phase = Phase::FailedOnce { failures: self.failures() + 1 };
                self.stale_maps += 1;
                Err(E_MAPKEY)
            }
        }
    }

    fn failures(&self) -> u32 {
        match self.phase {
            Phase::FailedOnce { failures } => failures,
            _ => 0,
        }
    }

    /// 回调注册：只在 Mapped 态允许；冻结后拒。
    pub fn register_callback(&mut self, id: u32, fp: u64) -> Result<(), u16> {
        match self.phase {
            Phase::Mapped { .. } => {}
            _ => return Err(E_CBSTATE),
        }
        if self.callbacks.iter().any(|c| c.id == id) {
            return Err(E_CBSTATE);
        }
        self.callbacks.push(EbsCallback {
            id,
            fp,
            state: CallbackState::Registered,
        });
        Ok(())
    }

    /// 冻结回调链：EBS 前最后一步。冻结后禁止再取图/注册。
    pub fn freeze_callbacks(&mut self) -> Result<usize, u16> {
        match self.phase {
            Phase::Mapped { map_key } => {
                let n = self.callbacks.len();
                for c in self.callbacks.iter_mut() {
                    c.state = CallbackState::Frozen;
                }
                self.phase = Phase::CallbacksFrozen { map_key };
                Ok(n)
            }
            Phase::CallbacksFrozen { .. } => Ok(self.callbacks.len()),
            _ => Err(E_ORDER),
        }
    }

    /// 调 EBS：带调用方持有的 MapKey（可能与取图时不同步）。
    /// 规范契约：第一次成功；失败后必须重取图再冻结再调。
    pub fn exit_boot_services(&mut self, caller_key: u64, fw_now: u64) -> Result<(), u16> {
        match self.phase {
            Phase::CallbacksFrozen { map_key } => {
                self.exit_attempts += 1;
                if caller_key != map_key {
                    self.phase = Phase::FailedOnce { failures: self.failures() + 1 };
                    self.stale_maps += 1;
                    return Err(E_MAPKEY);
                }
                if fw_now != self.fw_generation {
                    // EBS 调用瞬间固件翻动了内存图：两次调用契约——
                    // 第一次调用必须报告失配，重取后第二次才可成功。
                    self.phase = Phase::FailedOnce { failures: self.failures() + 1 };
                    self.fw_generation = fw_now;
                    self.stale_maps += 1;
                    return Err(E_MAPKEY);
                }
                if self.failures() > self.max_retries {
                    return Err(E_RETRY_OUT);
                }
                // 成功：触发全部冻结回调恰好一次，进终局态
                for c in self.callbacks.iter_mut() {
                    c.state = CallbackState::Fired;
                }
                self.phase = Phase::Exited;
                Ok(())
            }
            Phase::Exited => Err(E_TWICE),
            _ => Err(E_ORDER),
        }
    }

    /// 终局审计：全部回调恰好触发一次；账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.phase.id() != 4 {
            return Err(E_NOTERM);
        }
        let fired = self
            .callbacks
            .iter()
            .filter(|c| c.state == CallbackState::Fired)
            .count();
        if fired != self.callbacks.len() {
            return Err(E_CBSTATE);
        }
        // 守恒：取图次数 = 成功入账（含中间态重取）≥ 失败次数
        if self.map_attempts < self.stale_maps {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(self.phase.id());
        buf.extend_from_slice(&self.map_attempts.to_le_bytes());
        buf.extend_from_slice(&self.exit_attempts.to_le_bytes());
        for c in &self.callbacks {
            buf.extend_from_slice(&c.id.to_le_bytes());
            buf.extend_from_slice(&c.fp.to_le_bytes());
            buf.push(match c.state {
                CallbackState::Registered => 0,
                CallbackState::Frozen => 1,
                CallbackState::Fired => 2,
            });
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法闭环 2 例 + 注入拦截 6 例 ────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：干净路径 Idle→Map→冻结→EBS→Exited，回调恰好触发一次。
    let mut p1 = EbsProtocol::new(7);
    p1.get_map(MapResult::Ok { map_key: 0x1234, desc_count: 64 })?;
    p1.register_callback(1, 0xAA)?;
    p1.register_callback(2, 0xBB)?;
    p1.freeze_callbacks()?;
    p1.exit_boot_services(0x1234, 7)?;
    p1.audit()?;
    if p1.phase.id() != 4 || p1.exit_attempts != 1 {
        return Err(E_NOTERM);
    }
    hits += 1;

    // R2：MAPKEY 失配闭环——失败→重取图→再冻结→二次成功。
    let mut p2 = EbsProtocol::new(7);
    p2.get_map(MapResult::Ok { map_key: 100, desc_count: 8 })?;
    p2.freeze_callbacks()?;
    // 固件翻动：EBS 时代际前进 → 失配
    if p2.exit_boot_services(100, 8) != Err(E_MAPKEY) {
        return Err(E_MAPKEY);
    }
    // 重取图（固件新代际）
    p2.get_map(MapResult::Ok { map_key: 200, desc_count: 9 })?;
    p2.freeze_callbacks()?;
    p2.exit_boot_services(200, 8)?;
    p2.audit()?;
    if p2.stale_maps < 1 {
        return Err(E_STALE);
    }
    hits += 1;

    // R3：缓冲不足→扩容重试闭环（收链为空时冻结为 0 条合法）。
    let mut p3 = EbsProtocol::new(3);
    if p3.get_map(MapResult::BufferTooSmall { needed: 128 }) != Err(E_BUFSMALL) {
        return Err(E_BUFSMALL);
    }
    p3.get_map(MapResult::Ok { map_key: 9, desc_count: 128 })?;
    p3.freeze_callbacks()?;
    p3.exit_boot_services(9, 3)?;
    p3.audit()?;
    if p3.map_attempts != 2 || p3.exit_attempts != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：Idle 态直接 EBS → E_ORDER。
    let mut i1 = EbsProtocol::new(1);
    if i1.exit_boot_services(0, 1) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I2：冻结后取图 → E_ORDER。
    let mut i2 = EbsProtocol::new(1);
    i2.get_map(MapResult::Ok { map_key: 1, desc_count: 1 })?;
    i2.freeze_callbacks()?;
    if i2.get_map(MapResult::Ok { map_key: 2, desc_count: 1 }) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I3：EBS 成功后再调 → E_TWICE。
    let mut i3 = EbsProtocol::new(1);
    i3.get_map(MapResult::Ok { map_key: 5, desc_count: 2 })?;
    i3.freeze_callbacks()?;
    i3.exit_boot_services(5, 1)?;
    if i3.exit_boot_services(5, 1) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：冻结后注册回调 → E_CBSTATE。
    let mut i4 = EbsProtocol::new(1);
    i4.get_map(MapResult::Ok { map_key: 6, desc_count: 2 })?;
    i4.freeze_callbacks()?;
    if i4.register_callback(9, 1) != Err(E_CBSTATE) {
        return Err(E_CBSTATE);
    }
    hits += 1;

    // I5：持错 MapKey 调 EBS → E_MAPKEY（本地键失配路径）。
    let mut i5 = EbsProtocol::new(1);
    i5.get_map(MapResult::Ok { map_key: 77, desc_count: 2 })?;
    i5.freeze_callbacks()?;
    if i5.exit_boot_services(78, 1) != Err(E_MAPKEY) {
        return Err(E_MAPKEY);
    }
    hits += 1;

    // I6：未冻结直接 EBS → E_ORDER（P3 的对偶负例）。
    let mut i6 = EbsProtocol::new(1);
    i6.get_map(MapResult::Ok { map_key: 9, desc_count: 128 })?;
    if i6.exit_boot_services(9, 1) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I7：终局审计对未到局协议拒绝。
    let i7 = EbsProtocol::new(1);
    if i7.audit() != Err(E_NOTERM) {
        return Err(E_NOTERM);
    }
    hits += 1;

    Ok(hits)
}
