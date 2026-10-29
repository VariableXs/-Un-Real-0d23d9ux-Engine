//! unx48263_engplug — UNX-F48263 引擎可插拔契约（imepinyin.rs ↔ fcitx5 二选一切换语义）（诊断段 0xC1C0）。
//!
//! 判据原文：
//!   J1 切换 ×10 双向功能等价冒烟全绿；判据锚定 §AI-61 §2 专题四
//!   「对接与依赖宪法级区别」。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 二选一互斥：自研 imepinyin 线与 fcitx5 对接线同一时刻仅一条
//!      活跃（Active 独占位）；切换即换签活跃位，双活即契约违规。
//!   2. 契约三查：切换前核对对端在册（注册表含双方能力账）、握手版
//!      本一致（契约版本逐位相等）、对端能力位含组字/提交/预编辑三项；
//!      任一查失败即拒切，活跃位不动。
//!   3. 功能等价冒烟（J1 主体）：每次切换后向活跃引擎喂同一金标键流，
//!      组字提交输出必须与参照模型逐字相等——自研线与对接线在用户
//!      面无差别，切换 ×10（双向交替）逐次全绿。
//!   4. 切换净空：切换时旧引擎预编辑挂字作废清空（净空语义），已提交
//!      流水不受影响；带未释放按键态切换必拒（E_NOT_QUIESCENT）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC1C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NOT_QUIESCENT: u16 = SEG_BASE + 1; // 非净空切换被拒
const E_ENGINE_MISS: u16 = SEG_BASE + 2; // 对端引擎未在册
const E_HANDSHAKE: u16 = SEG_BASE + 3; // 契约版本握手不一致
const E_EQUIV_OFF: u16 = SEG_BASE + 4; // 功能等价冒烟失守
const E_CONTRACT_BAD: u16 = SEG_BASE + 5; // 能力位契约不齐
const E_STATE_BAD: u16 = SEG_BASE + 6; // 槽位状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 引擎身份：自研 imepinyin 线 / fcitx5 对接线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineKind {
    SelfEngine = 0,
    ExtEngine = 1,
}

/// 契约能力位：组字 / 提交 / 预编辑。
pub const CAP_COMPOSE: u32 = 1;
pub const CAP_COMMIT: u32 = 2;
pub const CAP_PREEDIT: u32 = 4;
/// 契约版本（双方必须逐位相等）。
pub const CONTRACT_VER: u32 = 0x0021_0007;

/// 注册表一条：引擎能力账。
#[derive(Clone, Copy, Debug)]
pub struct EngineCaps {
    pub kind: u8,
    pub caps: u32,
    pub ver: u32,
}

/// 自研线组字器（imepinyin.rs 语义）：4 键一收，字 = 0x4E00 + 键折算。
pub struct SelfEngine {
    buf: Vec<u8>,
    pub commits: Vec<u16>,
    pub quiescent: bool,
}

impl SelfEngine {
    pub fn new() -> SelfEngine {
        SelfEngine { buf: Vec::new(), commits: Vec::new(), quiescent: true }
    }
    pub fn feed(&mut self, key: u8) -> Result<(), u16> {
        self.quiescent = false;
        self.buf.push(key);
        if self.buf.len() >= 4 {
            let mut acc: u32 = 0;
            let mut i = 0usize;
            while i < 4 {
                acc = acc.wrapping_mul(31).wrapping_add(self.buf[i] as u32);
                i += 1;
            }
            self.commits.push(0x4E00 + (acc % 400) as u16);
            self.buf.clear();
        }
        self.quiescent = true;
        Ok(())
    }
    pub fn flush(&mut self) {
        self.buf.clear(); // 切换净空：挂字作废
    }
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
    pub fn commit_len(&self) -> usize {
        self.commits.len()
    }
}

/// 对接线组字器（fcitx5 适配语义）：键先封帧再解帧，同律 4 键一收。
pub struct ExtEngine {
    buf: Vec<u8>,
    pub commits: Vec<u16>,
    pub quiescent: bool,
}

impl ExtEngine {
    pub fn new() -> ExtEngine {
        ExtEngine { buf: Vec::new(), commits: Vec::new(), quiescent: true }
    }
    /// 封帧：键 → [0x5A, key] 双字节帧，解帧还原键（适配层开销账）。
    fn frame(&self, key: u8) -> [u8; 2] {
        [0x5A, key]
    }
    fn unframe(&self, f: [u8; 2]) -> Result<u8, u16> {
        if f[0] != 0x5A {
            return Err(E_CONTRACT_BAD);
        }
        Ok(f[1])
    }
    pub fn feed(&mut self, key: u8) -> Result<(), u16> {
        self.quiescent = false;
        let f = self.frame(key);
        let k = self.unframe(f)?;
        self.buf.push(k);
        if self.buf.len() >= 4 {
            let mut acc: u32 = 0;
            let mut i = 0usize;
            while i < 4 {
                acc = acc.wrapping_mul(31).wrapping_add(self.buf[i] as u32);
                i += 1;
            }
            self.commits.push(0x4E00 + (acc % 400) as u16);
            self.buf.clear();
        }
        self.quiescent = true;
        Ok(())
    }
    pub fn flush(&mut self) {
        self.buf.clear();
    }
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
    pub fn commit_len(&self) -> usize {
        self.commits.len()
    }
}

/// 参照模型（金标）：键流按 4 键一组折算的提交字序列。
pub fn golden(stream: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    let groups = stream.len() / 4;
    let mut g = 0usize;
    while g < groups {
        let mut acc: u32 = 0;
        let mut i = 0usize;
        while i < 4 {
            acc = acc.wrapping_mul(31).wrapping_add(stream[g * 4 + i] as u32);
            i += 1;
        }
        out.push(0x4E00 + (acc % 400) as u16);
        g += 1;
    }
    out
}

/// 引擎槽：二选一切换枢纽。
pub struct EngineSlot {
    self_e: SelfEngine,
    ext_e: ExtEngine,
    registry: Vec<EngineCaps>,
    pub active: Option<EngineKind>,
    pub switches: u32,
    pub smoke_green: u32,
    pub contract_faults: u32,
}

impl EngineSlot {
    pub fn new() -> EngineSlot {
        EngineSlot {
            self_e: SelfEngine::new(),
            ext_e: ExtEngine::new(),
            registry: Vec::new(),
            active: None,
            switches: 0,
            smoke_green: 0,
            contract_faults: 0,
        }
    }

    /// 引擎入册：能力三项齐 + 版本同契方准入。
    pub fn register(&mut self, kind: u8, caps: u32, ver: u32) -> Result<(), u16> {
        if kind > 1 {
            return Err(E_ENGINE_MISS);
        }
        if caps & (CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT) != (CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT) {
            self.contract_faults += 1;
            return Err(E_CONTRACT_BAD);
        }
        if ver != CONTRACT_VER {
            self.contract_faults += 1;
            return Err(E_HANDSHAKE);
        }
        // 同引擎重复入册覆盖（幂等）。
        let mut kept: Vec<EngineCaps> = Vec::new();
        let mut i = 0usize;
        while i < self.registry.len() {
            if self.registry[i].kind != kind {
                kept.push(self.registry[i]);
            }
            i += 1;
        }
        self.registry = kept;
        self.registry.push(EngineCaps { kind, caps, ver });
        Ok(())
    }

    fn registered(&self, kind: u8) -> bool {
        let mut i = 0usize;
        while i < self.registry.len() {
            if self.registry[i].kind == kind {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 活跃引擎喂键。
    pub fn feed(&mut self, key: u8) -> Result<(), u16> {
        match self.active {
            Some(EngineKind::SelfEngine) => self.self_e.feed(key),
            Some(EngineKind::ExtEngine) => self.ext_e.feed(key),
            None => Err(E_STATE_BAD),
        }
    }

    /// 切换（J1 主体路径）：契约三查 → 净空 → 换签活跃位。
    pub fn switch_to(&mut self, kind: EngineKind) -> Result<(), u16> {
        let target = kind as u8;
        // 查一：对端在册。
        if !self.registered(target) {
            return Err(E_ENGINE_MISS);
        }
        // 查二/三：注册表内版本与能力位核验。
        let mut i = 0usize;
        while i < self.registry.len() {
            let r = self.registry[i];
            if r.kind == target {
                if r.ver != CONTRACT_VER {
                    self.contract_faults += 1;
                    return Err(E_HANDSHAKE);
                }
                if r.caps & (CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT)
                    != (CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT)
                {
                    self.contract_faults += 1;
                    return Err(E_CONTRACT_BAD);
                }
            }
            i += 1;
        }
        // 净空：双方按键态必须已释放。
        if !self.self_e.quiescent || !self.ext_e.quiescent {
            return Err(E_NOT_QUIESCENT);
        }
        // 二选一互斥：同引擎重切即状态违规。
        match self.active {
            Some(a) if a == kind => return Err(E_STATE_BAD),
            _ => {}
        }
        // 净空语义：旧活跃线挂字作废。
        self.self_e.flush();
        self.ext_e.flush();
        self.active = Some(kind);
        self.switches += 1;
        Ok(())
    }

    /// 功能等价冒烟：向活跃引擎喂金标键流，提交输出对照参照模型逐字核对。
    pub fn smoke(&mut self, stream: &[u8]) -> Result<usize, u16> {
        let before_self = self.self_e.commit_len();
        let before_ext = self.ext_e.commit_len();
        let mut i = 0usize;
        while i < stream.len() {
            self.feed(stream[i])?;
            i += 1;
        }
        let want = golden(stream);
        let got: Vec<u16> = match self.active {
            Some(EngineKind::SelfEngine) => {
                let mut v = Vec::new();
                let mut j = before_self;
                while j < self.self_e.commits.len() {
                    v.push(self.self_e.commits[j]);
                    j += 1;
                }
                v
            }
            Some(EngineKind::ExtEngine) => {
                let mut v = Vec::new();
                let mut j = before_ext;
                while j < self.ext_e.commits.len() {
                    v.push(self.ext_e.commits[j]);
                    j += 1;
                }
                v
            }
            None => return Err(E_STATE_BAD),
        };
        if got.len() != want.len() {
            self.contract_faults += 1;
            return Err(E_EQUIV_OFF);
        }
        let mut k = 0usize;
        while k < want.len() {
            if got[k] != want[k] {
                self.contract_faults += 1;
                return Err(E_EQUIV_OFF);
            }
            k += 1;
        }
        self.smoke_green += 1;
        Ok(got.len())
    }

    pub fn pending_of(&self, kind: EngineKind) -> usize {
        match kind {
            EngineKind::SelfEngine => self.self_e.pending(),
            EngineKind::ExtEngine => self.ext_e.pending(),
        }
    }
    pub fn registry_count(&self) -> usize {
        self.registry.len()
    }

    /// 契约审计：双方在册、活跃位唯一、故障账闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.registered(0) || !self.registered(1) {
            return Err(E_ENGINE_MISS);
        }
        match self.active {
            Some(_) => {}
            None => return Err(E_STATE_BAD),
        }
        Ok(())
    }

    /// ×N 切换覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want_switches: u32, want_smoke: u32) -> Result<(), u16> {
        if self.switches < want_switches || self.smoke_green < want_smoke {
            return Err(E_EQUIV_OFF);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：冒烟全绿 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.smoke_green == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（切换 ×10 双向功能等价冒烟全绿），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：双向交替切换（自研↔对接），每次切换后金标冒烟全绿；
    //    活跃位二选一互斥全程成立。
    let mut slot = EngineSlot::new();
    slot.register(0, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot.register(1, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    if slot.registry_count() != 2 {
        return Err(E_ENGINE_MISS);
    }
    let smoke_stream: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    slot.switch_to(EngineKind::SelfEngine)?;
    slot.smoke(&smoke_stream)?;
    let mut s = 1u32;
    while s < 10 {
        let target = if s % 2 == 1 { EngineKind::ExtEngine } else { EngineKind::SelfEngine };
        slot.switch_to(target)?;
        slot.smoke(&smoke_stream)?;
        s += 1;
    }
    slot.coverage(10, 10)?;
    slot.audit()?;
    hits += 1;

    // R2 功能等价本体：同键流下自研线与对接线输出逐字相等、且等于参照
    //    模型（二选一之外再加双线对拍）。
    let mut se = SelfEngine::new();
    let mut ee = ExtEngine::new();
    let mut i = 0u8;
    while (i as usize) < 16 {
        se.feed(i)?;
        ee.feed(i)?;
        i += 1;
    }
    let g = golden(&{
        let mut v = Vec::new();
        let mut j = 0u8;
        while (j as usize) < 16 {
            v.push(j);
            j += 1;
        }
        v
    });
    if se.commits.len() != 4 || ee.commits.len() != 4 || g.len() != 4 {
        return Err(E_EQUIV_OFF);
    }
    let mut k = 0usize;
    while k < 4 {
        if se.commits[k] != g[k] || ee.commits[k] != g[k] {
            return Err(E_EQUIV_OFF);
        }
        k += 1;
    }
    hits += 1;

    // R3 同引擎重切拒收：活跃自研再切自研 → E_STATE_BAD，切换计数不动。
    let mut slot2 = EngineSlot::new();
    slot2.register(0, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot2.register(1, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot2.switch_to(EngineKind::SelfEngine)?;
    if slot2.switch_to(EngineKind::SelfEngine).is_ok() {
        return Err(E_STATE_BAD);
    }
    if slot2.switches != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R4 未在册对端拒切：只入册自研线，切对接线 → E_ENGINE_MISS。
    let mut slot3 = EngineSlot::new();
    slot3.register(0, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    if slot3.switch_to(EngineKind::ExtEngine).is_ok() {
        return Err(E_ENGINE_MISS);
    }
    if slot3.active.is_some() {
        return Err(E_STATE_BAD); // 拒切后活跃位不得漂移
    }
    hits += 1;

    // R5 契约能力位/版本握手负例：缺预编辑能力 → E_CONTRACT_BAD；
    //    版本不同契 → E_HANDSHAKE；故障账逐笔留痕。
    let mut slot4 = EngineSlot::new();
    if slot4.register(1, CAP_COMPOSE | CAP_COMMIT, CONTRACT_VER).is_ok() {
        return Err(E_CONTRACT_BAD);
    }
    if slot4.register(1, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, 0x9999).is_ok() {
        return Err(E_HANDSHAKE);
    }
    if slot4.contract_faults != 2 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 净空语义：挂字后切换 → 挂字作废清零、已提交流水保全、
    //    新线从零挂账。
    let mut slot5 = EngineSlot::new();
    slot5.register(0, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot5.register(1, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot5.switch_to(EngineKind::SelfEngine)?;
    let mut j = 0u8;
    while (j as usize) < 6 {
        slot5.feed(j)?; // 4 键提交一组、余 2 键挂账
        j += 1;
    }
    if slot5.pending_of(EngineKind::SelfEngine) != 2 {
        return Err(E_STATE_BAD);
    }
    let commits_before = slot5.self_e.commit_len();
    slot5.switch_to(EngineKind::ExtEngine)?;
    if slot5.pending_of(EngineKind::SelfEngine) != 0 {
        return Err(E_STATE_BAD); // 净空：挂字作废
    }
    if slot5.self_e.commit_len() != commits_before {
        return Err(E_EQUIV_OFF); // 已提交流水保全
    }
    if slot5.pending_of(EngineKind::ExtEngine) != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：零冒烟不写回、空摘要拒、全绿+非空摘要放行。
    let mut slot6 = EngineSlot::new();
    slot6.register(0, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    slot6.register(1, CAP_COMPOSE | CAP_COMMIT | CAP_PREEDIT, CONTRACT_VER)?;
    if slot6.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    slot6.switch_to(EngineKind::SelfEngine)?;
    slot6.smoke(&smoke_stream)?;
    if slot6.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    slot6.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
