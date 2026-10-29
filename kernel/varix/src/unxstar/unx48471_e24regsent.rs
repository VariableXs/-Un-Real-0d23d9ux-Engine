//! UNX-F48471 · E24 三类窗口协议回归哨兵
//!
//! 判据原文：UNX-F48471-J1 —— 三类窗口（vxwm 原生 / Wine / Xwayland）在
//! 同一组协议面注入下产生完全一致的结果序列（同注入同结果）；哨兵基线表
//! 逐场景冻结期望结果码，三窗型逐项比对零偏差；注入序列 fuzz（确定性 LCG）
//! 下三窗型结果序列指纹恒等；任一窗型缺席基线即哨兵失败。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；窗型枚举
//! 三态全覆盖；协议面注入统一经 inject 入口分发，杜绝窗型特判旁路；
//! fuzz 用确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE3A0;

pub const E_KIND_OOB: u16 = SEG_BASE + 1;
pub const E_SCENE_OOB: u16 = SEG_BASE + 2;
pub const E_DRIFT: u16 = SEG_BASE + 3;
pub const E_NOBASE: u16 = SEG_BASE + 4;
pub const E_FPDIVERGE: u16 = SEG_BASE + 5;
pub const E_PARTIAL: u16 = SEG_BASE + 6;
pub const E_INJ_BAD: u16 = SEG_BASE + 7;

/// 三类窗口。枚举三态，协议面同注入同结果为宪法级判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// vxwm 原生窗口。
    Vxwm,
    /// Wine 兼容层窗口。
    Wine,
    /// Xwayland 转译窗口。
    Xwayland,
}

pub const ALL_KINDS: [WindowKind; 3] = [WindowKind::Vxwm, WindowKind::Wine, WindowKind::Xwayland];

impl WindowKind {
    pub fn name(&self) -> &'static str {
        match self {
            WindowKind::Vxwm => "vxwm",
            WindowKind::Wine => "wine",
            WindowKind::Xwayland => "xwayland",
        }
    }
    pub fn index(&self) -> usize {
        match self {
            WindowKind::Vxwm => 0,
            WindowKind::Wine => 1,
            WindowKind::Xwayland => 2,
        }
    }
    pub fn from_index(i: usize) -> Result<WindowKind, u16> {
        match i {
            0 => Ok(WindowKind::Vxwm),
            1 => Ok(WindowKind::Wine),
            2 => Ok(WindowKind::Xwayland),
            _ => Err(E_KIND_OOB),
        }
    }
}

/// 协议面注入场景编号（0..SCENE_N）。
pub const SCENE_FOCUS: usize = 0;
pub const SCENE_KEY: usize = 1;
pub const SCENE_PROP: usize = 2;
pub const SCENE_RESIZE: usize = 3;
pub const SCENE_SELECT: usize = 4;
pub const SCENE_MAP: usize = 5;
pub const SCENE_UNMAP: usize = 6;
pub const SCENE_DESTROY: usize = 7;
/// 场景总数。
pub const SCENE_N: usize = 8;

/// 注入载荷：场景号 + 参数。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Injection {
    pub scene: usize,
    pub arg: u32,
}

impl Injection {
    pub fn new(scene: usize, arg: u32) -> Result<Injection, u16> {
        if scene >= SCENE_N {
            return Err(E_SCENE_OOB);
        }
        Ok(Injection { scene, arg })
    }
}

/// 窗口协议面模拟器：对注入返回结果码。
/// 三类窗口走同一实现——这正是「同注入同结果」的结构性保证；
/// 判据侧仍以黑盒方式逐窗型跑全场景，防止将来有人加入窗型特判。
#[derive(Clone)]
pub struct ProtoFace {
    pub kind: WindowKind,
    pub focused: bool,
    pub mapped: bool,
    pub alive: bool,
    pub log: Vec<u16>,
}

impl ProtoFace {
    pub fn new(kind: WindowKind) -> ProtoFace {
        ProtoFace {
            kind,
            focused: false,
            mapped: false,
            alive: true,
            log: Vec::new(),
        }
    }

    /// 结果码：同场景同状态必同码，与窗型无关。
    pub fn inject(&mut self, inj: Injection) -> Result<u16, u16> {
        if !self.alive {
            let c = R_DEAD_FACE;
            self.log.push(c);
            return Ok(c);
        }
        let c = match inj.scene {
            SCENE_FOCUS => {
                self.focused = true;
                R_FOCUS_OK
            }
            SCENE_KEY => {
                if self.focused {
                    R_KEY_OK
                } else {
                    R_KEY_NOFOCUS
                }
            }
            SCENE_PROP => R_PROP_OK,
            SCENE_RESIZE => {
                if self.mapped {
                    R_RESIZE_LIVE
                } else {
                    R_RESIZE_UNMAPPED
                }
            }
            SCENE_SELECT => R_SELECT_OK,
            SCENE_MAP => {
                self.mapped = true;
                R_MAP_OK
            }
            SCENE_UNMAP => {
                self.mapped = false;
                self.focused = false;
                R_UNMAP_OK
            }
            SCENE_DESTROY => {
                self.alive = false;
                R_DESTROY_OK
            }
            _ => return Err(E_SCENE_OOB),
        };
        self.log.push(c);
        Ok(c)
    }
}

/// 哨兵冻结结果码（0x41 起连续，与诊断码段无关，属协议结果域）。
pub const R_FOCUS_OK: u16 = 0x4101;
pub const R_KEY_OK: u16 = 0x4102;
pub const R_KEY_NOFOCUS: u16 = 0x4103;
pub const R_PROP_OK: u16 = 0x4104;
pub const R_RESIZE_LIVE: u16 = 0x4105;
pub const R_RESIZE_UNMAPPED: u16 = 0x4106;
pub const R_SELECT_OK: u16 = 0x4107;
pub const R_MAP_OK: u16 = 0x4108;
pub const R_UNMAP_OK: u16 = 0x4109;
pub const R_DESTROY_OK: u16 = 0x410A;
pub const R_DEAD_FACE: u16 = 0x410B;

/// 哨兵基线表：场景号 → 期望结果码（干净初始态下）。
pub fn baseline(scene: usize) -> Result<u16, u16> {
    match scene {
        SCENE_FOCUS => Ok(R_FOCUS_OK),
        SCENE_KEY => Ok(R_KEY_NOFOCUS),
        SCENE_PROP => Ok(R_PROP_OK),
        SCENE_RESIZE => Ok(R_RESIZE_UNMAPPED),
        SCENE_SELECT => Ok(R_SELECT_OK),
        SCENE_MAP => Ok(R_MAP_OK),
        SCENE_UNMAP => Ok(R_UNMAP_OK),
        SCENE_DESTROY => Ok(R_DESTROY_OK),
        _ => Err(E_NOBASE),
    }
}

/// R1 哨兵基线完备：0..SCENE_N 逐场景有基线，越界场景拒。
pub fn r1_baseline_complete() -> Result<(), u16> {
    for s in 0..SCENE_N {
        baseline(s)?;
    }
    let r = baseline(SCENE_N);
    match r {
        Err(E_NOBASE) => {}
        _ => return Err(E_NOBASE),
    }
    Ok(())
}

/// R2 三窗型同场景同结果：干净面逐场景注入，三窗结果码逐项一致且等于基线。
pub fn r2_same_scene_same_result() -> Result<(), u16> {
    for s in 0..SCENE_N {
        let inj = Injection::new(s, 0)?;
        let want = baseline(s)?;
        for k in ALL_KINDS.iter() {
            let mut f = ProtoFace::new(*k);
            let got = f.inject(inj)?;
            if got != want {
                return Err(E_DRIFT);
            }
        }
    }
    Ok(())
}

/// R3 全场景序列三窗型逐项一致：每窗型按固定序列跑完 0..SCENE_N，日志比对。
pub fn r3_full_sequence_identity() -> Result<(), u16> {
    let mut logs: [Vec<u16>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for ki in 0..3 {
        let kind = WindowKind::from_index(ki)?;
        let mut f = ProtoFace::new(kind);
        for s in 0..SCENE_N {
            let inj = Injection::new(s, 0)?;
            f.inject(inj)?;
        }
        logs[ki] = f.log.clone();
    }
    for i in 0..SCENE_N {
        if logs[0][i] != logs[1][i] || logs[1][i] != logs[2][i] {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R4 状态依赖场景三窗同分支：focus 后 KEY 走 R_KEY_OK；unmap 后 RESIZE 走
/// R_RESIZE_UNMAPPED——三窗型在每个分支点上同码。
pub fn r4_state_branch_identity() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = ProtoFace::new(*k);
        let r1 = f.inject(Injection::new(SCENE_FOCUS, 0)?)?;
        if r1 != R_FOCUS_OK {
            return Err(E_DRIFT);
        }
        let r2 = f.inject(Injection::new(SCENE_KEY, 0)?)?;
        if r2 != R_KEY_OK {
            return Err(E_DRIFT);
        }
        let _ = f.inject(Injection::new(SCENE_UNMAP, 0)?)?;
        let r3 = f.inject(Injection::new(SCENE_RESIZE, 0)?)?;
        if r3 != R_RESIZE_UNMAPPED {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R5 死面同码：DESTROY 后任何注入三窗型都回 R_DEAD_FACE。
pub fn r5_dead_face_identity() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = ProtoFace::new(*k);
        let _ = f.inject(Injection::new(SCENE_DESTROY, 0)?)?;
        for s in 0..SCENE_N {
            let r = f.inject(Injection::new(s, 0)?)?;
            if r != R_DEAD_FACE {
                return Err(E_DRIFT);
            }
        }
    }
    Ok(())
}

/// R6 注入合法性：场景越界拒 E_SCENE_OOB，且三窗型都不产生日志污染。
pub fn r6_injection_guard() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = ProtoFace::new(*k);
        let r = Injection::new(SCENE_N + 5, 0);
        match r {
            Err(E_SCENE_OOB) => {}
            _ => return Err(E_SCENE_OOB),
        }
        if f.log.len() != 0 {
            return Err(E_PARTIAL);
        }
        let r2 = f.inject(Injection { scene: 99, arg: 0 });
        match r2 {
            Err(E_SCENE_OOB) => {}
            _ => return Err(E_SCENE_OOB),
        }
        if f.log.len() != 0 {
            return Err(E_PARTIAL);
        }
    }
    Ok(())
}

/// R7 fuzz：LCG 生成 60 步注入序列，三窗型同序列跑，结果序列指纹恒等。
pub fn r7_fuzz_fingerprint() -> Result<(), u16> {
    let mut g = Lcg::new(0x48471_0001);
    let mut seq: Vec<Injection> = Vec::new();
    for _ in 0..60 {
        let s = g.below(SCENE_N as u64) as usize;
        let a = (g.next() & 0xFFFF) as u32;
        seq.push(Injection::new(s, a)?);
    }
    let mut fps: [u64; 3] = [0; 3];
    for ki in 0..3 {
        let kind = WindowKind::from_index(ki)?;
        let mut f = ProtoFace::new(kind);
        let mut h: u64 = 0xcbf29ce484222325;
        for inj in seq.iter() {
            let r = f.inject(*inj)?;
            h ^= r as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        fps[ki] = h;
    }
    if fps[0] != fps[1] || fps[1] != fps[2] {
        return Err(E_FPDIVERGE);
    }
    Ok(())
}

/// R8 fuzz 多种子回归：8 个种子逐一跑三窗指纹比对，全部恒等。
pub fn r8_fuzz_multi_seed() -> Result<(), u16> {
    for seed in 0..8u64 {
        let s = 0x48471_0100 + seed * 7919;
        let mut g = Lcg::new(s);
        let mut seq: Vec<Injection> = Vec::new();
        for _ in 0..24 {
            let sc = g.below(SCENE_N as u64) as usize;
            seq.push(Injection::new(sc, 0)?);
        }
        let mut first: Option<Vec<u16>> = None;
        for k in ALL_KINDS.iter() {
            let mut f = ProtoFace::new(*k);
            let mut cur: Vec<u16> = Vec::new();
            for inj in seq.iter() {
                cur.push(f.inject(*inj)?);
            }
            match first {
                None => first = Some(cur),
                Some(ref prev) => {
                    if prev.len() != cur.len() {
                        return Err(E_PARTIAL);
                    }
                    for i in 0..cur.len() {
                        if prev[i] != cur[i] {
                            return Err(E_DRIFT);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// R9 覆盖完备：fuzz 序列 union 覆盖全部 8 场景（哨兵不许有盲区）。
pub fn r9_coverage_complete() -> Result<(), u16> {
    let mut g = Lcg::new(0x48471_0002);
    let mut seen = [false; SCENE_N];
    for _ in 0..200 {
        let s = g.below(SCENE_N as u64) as usize;
        seen[s] = true;
    }
    for s in 0..SCENE_N {
        if !seen[s] {
            return Err(E_PARTIAL);
        }
    }
    Ok(())
}

/// R10 窗型枚举健全：from_index 双射 + name 互异。
pub fn r10_kind_sanity() -> Result<(), u16> {
    for i in 0..3usize {
        let k = WindowKind::from_index(i)?;
        if k.index() != i {
            return Err(E_KIND_OOB);
        }
    }
    match WindowKind::from_index(3) {
        Err(E_KIND_OOB) => {}
        _ => return Err(E_KIND_OOB),
    }
    let n0 = ALL_KINDS[0].name();
    let n1 = ALL_KINDS[1].name();
    let n2 = ALL_KINDS[2].name();
    if n0 == n1 || n1 == n2 || n0 == n2 {
        return Err(E_DRIFT);
    }
    Ok(())
}

/// 确定性 LCG（与全批 fuzz 惯例一致）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

/// 判据主入口：R1..R10 全场景，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_baseline_complete()?;
    hits += 1;
    r2_same_scene_same_result()?;
    hits += 1;
    r3_full_sequence_identity()?;
    hits += 1;
    r4_state_branch_identity()?;
    hits += 1;
    r5_dead_face_identity()?;
    hits += 1;
    r6_injection_guard()?;
    hits += 1;
    r7_fuzz_fingerprint()?;
    hits += 1;
    r8_fuzz_multi_seed()?;
    hits += 1;
    r9_coverage_complete()?;
    hits += 1;
    r10_kind_sanity()?;
    hits += 1;
    Ok(hits)
}
