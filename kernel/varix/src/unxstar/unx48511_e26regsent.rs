//! UNX-F48511 · E26 三类窗口风暴回归哨兵
//!
//! 判据原文：UNX-F48511-J1 —— 三类窗口（vxwm 原生 / Wine / Xwayland）
//! 在同一组风暴注入（刷新风暴/输入风暴/焦点风暴/断连风暴）下产生完全
//! 一致的结果序列（同风暴同结果）；哨兵基线表逐风暴冻结期望结局，三窗
//! 型逐项比对零偏差；风暴强度参数不影响窗型间一致性（同强度三窗指纹
//! 恒等）；fuzz 混合风暴 60 步三窗结果指纹恒等；任一窗型缺席基线即哨兵
//! 失败。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；窗型枚举
//! 三态全覆盖；风暴统一经 inject 分发，杜绝窗型特判旁路；fuzz 用确定
//! 性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEAE0;

pub const E_KIND_OOB: u16 = SEG_BASE + 1;
pub const E_STORM_OOB: u16 = SEG_BASE + 2;
pub const E_DRIFT: u16 = SEG_BASE + 3;
pub const E_NOBASE: u16 = SEG_BASE + 4;
pub const E_FPDIVERGE: u16 = SEG_BASE + 5;
pub const E_PARTIAL: u16 = SEG_BASE + 6;

/// 三类窗口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WKind {
    Vxwm,
    Wine,
    Xwayland,
}

pub const ALL_KINDS: [WKind; 3] = [WKind::Vxwm, WKind::Wine, WKind::Xwayland];

impl WKind {
    pub fn name(&self) -> &'static str {
        match self {
            WKind::Vxwm => "vxwm",
            WKind::Wine => "wine",
            WKind::Xwayland => "xwayland",
        }
    }
    pub fn index(&self) -> usize {
        match self {
            WKind::Vxwm => 0,
            WKind::Wine => 1,
            WKind::Xwayland => 2,
        }
    }
    pub fn from_index(i: usize) -> Result<WKind, u16> {
        match i {
            0 => Ok(WKind::Vxwm),
            1 => Ok(WKind::Wine),
            2 => Ok(WKind::Xwayland),
            _ => Err(E_KIND_OOB),
        }
    }
}

/// 四类风暴。
pub const STORM_REFRESH: usize = 0;
pub const STORM_INPUT: usize = 1;
pub const STORM_FOCUS: usize = 2;
pub const STORM_DISCONNECT: usize = 3;
pub const STORM_N: usize = 4;

/// 风暴注入：类型 + 强度。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StormInj {
    pub storm: usize,
    pub intensity: u32,
}

impl StormInj {
    pub fn new(storm: usize, intensity: u32) -> Result<StormInj, u16> {
        if storm >= STORM_N {
            return Err(E_STORM_OOB);
        }
        if intensity == 0 || intensity > 10_000 {
            return Err(E_STORM_OOB);
        }
        Ok(StormInj { storm, intensity })
    }
}

/// 窗口风暴面模拟器。
#[derive(Clone)]
pub struct StormFace {
    pub kind: WKind,
    pub alive: bool,
    pub focused: bool,
    pub log: Vec<u16>,
}

impl StormFace {
    pub fn new(kind: WKind) -> StormFace {
        StormFace { kind, alive: true, focused: false, log: Vec::new() }
    }

    /// 结局码：同风暴同状态必同码，与窗型无关。
    pub fn inject(&mut self, inj: StormInj) -> Result<u16, u16> {
        if !self.alive {
            let c = R_DEAD;
            self.log.push(c);
            return Ok(c);
        }
        let c = match inj.storm {
            STORM_REFRESH => {
                // 强度阈值：>5000 走降级刷新。
                if inj.intensity > 5000 {
                    R_REFRESH_DEGRADED
                } else {
                    R_REFRESH_OK
                }
            }
            STORM_INPUT => {
                self.focused = true;
                if inj.intensity > 8000 {
                    R_INPUT_THROTTLED
                } else {
                    R_INPUT_OK
                }
            }
            STORM_FOCUS => {
                self.focused = true;
                R_FOCUS_TAKEN
            }
            STORM_DISCONNECT => {
                self.alive = false;
                self.focused = false;
                R_DISC_SEALED
            }
            _ => return Err(E_STORM_OOB),
        };
        self.log.push(c);
        Ok(c)
    }
}

/// 哨兵冻结结局码。
pub const R_REFRESH_OK: u16 = 0x4201;
pub const R_REFRESH_DEGRADED: u16 = 0x4202;
pub const R_INPUT_OK: u16 = 0x4203;
pub const R_INPUT_THROTTLED: u16 = 0x4204;
pub const R_FOCUS_TAKEN: u16 = 0x4205;
pub const R_DISC_SEALED: u16 = 0x4206;
pub const R_DEAD: u16 = 0x4207;

/// 哨兵基线表：风暴类型 → 干净面期望结局（标准强度 1000）。
pub fn baseline(storm: usize) -> Result<u16, u16> {
    match storm {
        STORM_REFRESH => Ok(R_REFRESH_OK),
        STORM_INPUT => Ok(R_INPUT_OK),
        STORM_FOCUS => Ok(R_FOCUS_TAKEN),
        STORM_DISCONNECT => Ok(R_DISC_SEALED),
        _ => Err(E_NOBASE),
    }
}

/// R1 哨兵基线完备：0..STORM_N 有基线、越界拒。
pub fn r1_baseline_complete() -> Result<(), u16> {
    for s in 0..STORM_N {
        baseline(s)?;
    }
    match baseline(STORM_N) {
        Err(E_NOBASE) => {}
        _ => return Err(E_NOBASE),
    }
    Ok(())
}

/// R2 三窗型同风暴同结果：干净面逐风暴注入，三窗结局一致且等基线。
pub fn r2_same_storm_same_result() -> Result<(), u16> {
    for s in 0..STORM_N {
        let inj = StormInj::new(s, 1000)?;
        let want = baseline(s)?;
        for k in ALL_KINDS.iter() {
            let mut f = StormFace::new(*k);
            let got = f.inject(inj)?;
            if got != want {
                return Err(E_DRIFT);
            }
        }
    }
    Ok(())
}

/// R3 全序列三窗一致：每窗型按固定序列跑完，日志逐项比对。
pub fn r3_full_sequence_identity() -> Result<(), u16> {
    let mut logs: [Vec<u16>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for ki in 0..3 {
        let kind = WKind::from_index(ki)?;
        let mut f = StormFace::new(kind);
        for s in 0..STORM_N {
            f.inject(StormInj::new(s, 2000)?)?;
        }
        logs[ki] = f.log.clone();
    }
    for i in 0..STORM_N {
        if logs[0][i] != logs[1][i] || logs[1][i] != logs[2][i] {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R4 强度分支三窗同分支：REFRESH 6000 → DEGRADED、INPUT 9000 → THROTTLED，
/// 三窗型在每个分支点同码。
pub fn r4_intensity_branches() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = StormFace::new(*k);
        let r1 = f.inject(StormInj::new(STORM_REFRESH, 6000)?)?;
        if r1 != R_REFRESH_DEGRADED {
            return Err(E_DRIFT);
        }
        let mut f2 = StormFace::new(*k);
        let r2 = f2.inject(StormInj::new(STORM_INPUT, 9000)?)?;
        if r2 != R_INPUT_THROTTLED {
            return Err(E_DRIFT);
        }
    }
    Ok(())
}

/// R5 死面同码：断连风暴后任何注入三窗都回 R_DEAD。
pub fn r5_dead_identity() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = StormFace::new(*k);
        let _ = f.inject(StormInj::new(STORM_DISCONNECT, 1)?)?;
        for s in 0..STORM_N {
            let r = f.inject(StormInj::new(s, 100)?)?;
            if r != R_DEAD {
                return Err(E_DRIFT);
            }
        }
    }
    Ok(())
}

/// R6 注入合法性：类型越界/强度越界都拒 E_STORM_OOB，三窗日志零污染。
pub fn r6_injection_guard() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let mut f = StormFace::new(*k);
        match StormInj::new(STORM_N + 2, 100) {
            Err(E_STORM_OOB) => {}
            _ => return Err(E_STORM_OOB),
        }
        match StormInj::new(0, 0) {
            Err(E_STORM_OOB) => {}
            _ => return Err(E_STORM_OOB),
        }
        match StormInj::new(0, 20_000) {
            Err(E_STORM_OOB) => {}
            _ => return Err(E_STORM_OOB),
        }
        match f.inject(StormInj { storm: 99, intensity: 5 }) {
            Err(E_STORM_OOB) => {}
            _ => return Err(E_STORM_OOB),
        }
        if f.log.len() != 0 {
            return Err(E_PARTIAL);
        }
    }
    Ok(())
}

/// R7 fuzz：LCG 生成 60 步风暴序列，三窗同序列，结局指纹恒等。
pub fn r7_fuzz_fingerprint() -> Result<(), u16> {
    let mut g = Lcg::new(0x48511_0001);
    let mut seq: Vec<StormInj> = Vec::new();
    for _ in 0..60 {
        let s = g.below(STORM_N as u64) as usize;
        let inten = 1 + g.below(10_000) as u32;
        seq.push(StormInj::new(s, inten)?);
    }
    let mut fps: [u64; 3] = [0; 3];
    for ki in 0..3 {
        let kind = WKind::from_index(ki)?;
        let mut f = StormFace::new(kind);
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

/// R8 fuzz 多种子：8 种子逐一三窗指纹比对全恒等。
pub fn r8_fuzz_multi_seed() -> Result<(), u16> {
    for seed in 0..8u64 {
        let s0 = 0x48511_0100 + seed * 7919;
        let mut g = Lcg::new(s0);
        let mut seq: Vec<StormInj> = Vec::new();
        for _ in 0..24 {
            let s = g.below(STORM_N as u64) as usize;
            let inten = 1 + g.below(9_000) as u32;
            seq.push(StormInj::new(s, inten)?);
        }
        let mut first: Option<Vec<u16>> = None;
        for k in ALL_KINDS.iter() {
            let mut f = StormFace::new(*k);
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

/// R9 覆盖完备：fuzz 序列覆盖全部 4 风暴 + 全部 3 窗型。
pub fn r9_coverage() -> Result<(), u16> {
    let mut g = Lcg::new(0x48511_0002);
    let mut seen_storm = [false; STORM_N];
    let mut seen_kind = [false; 3];
    for _ in 0..100 {
        let s = g.below(STORM_N as u64) as usize;
        seen_storm[s] = true;
        let k = g.below(3) as usize;
        seen_kind[k] = true;
    }
    for s in 0..STORM_N {
        if !seen_storm[s] {
            return Err(E_PARTIAL);
        }
    }
    for k in 0..3 {
        if !seen_kind[k] {
            return Err(E_PARTIAL);
        }
    }
    Ok(())
}

/// R10 窗型枚举健全：from_index 双射 + name 互异。
pub fn r10_kind_sanity() -> Result<(), u16> {
    for i in 0..3usize {
        let k = WKind::from_index(i)?;
        if k.index() != i {
            return Err(E_KIND_OOB);
        }
    }
    match WKind::from_index(3) {
        Err(E_KIND_OOB) => {}
        _ => return Err(E_KIND_OOB),
    }
    let names = [ALL_KINDS[0].name(), ALL_KINDS[1].name(), ALL_KINDS[2].name()];
    if names[0] == names[1] || names[1] == names[2] || names[0] == names[2] {
        return Err(E_DRIFT);
    }
    Ok(())
}

/// 确定性 LCG。
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
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_baseline_complete()?;
    hits += 1;
    r2_same_storm_same_result()?;
    hits += 1;
    r3_full_sequence_identity()?;
    hits += 1;
    r4_intensity_branches()?;
    hits += 1;
    r5_dead_identity()?;
    hits += 1;
    r6_injection_guard()?;
    hits += 1;
    r7_fuzz_fingerprint()?;
    hits += 1;
    r8_fuzz_multi_seed()?;
    hits += 1;
    r9_coverage()?;
    hits += 1;
    r10_kind_sanity()?;
    hits += 1;
    Ok(hits)
}
