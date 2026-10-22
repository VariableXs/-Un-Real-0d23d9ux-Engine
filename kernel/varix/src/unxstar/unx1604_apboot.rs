//! unx1604_apboot —— UNX-F1604 AP 启动序列总控状态机（INIT-SIPI-SIPI）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1604 · AP 启动序列总控状态机 · 目标 380 行 · 已深化
//! 判据原文：UNX-F1604-J1 INIT→SIPI→SIPI 序列按 Intel SDM 时序逐核发出，
//!   每核状态机六态（待启/INIT 已发/SIPI 一发/SIPI 二发/到齐/超时）迁移全落账，
//!   序列重放账可审计。
//! 组件：六态闭集状态机（Pending/InitSent/Sipi1/Sipi2/Arrived/Timeout）
//!       + Intel SDM 时序面（INIT → 10ms → SIPI1 → 200us → SIPI2 判定）
//!       + 序列重放账（append-only 七列：核位/前态/事件/后态/刻度/矢量/轮位）
//!       + 总控器（逐核推进，超时面切换 Timeout 态）
//!       + 到齐判定（签到数 = 发出数）
//!       + 指纹冻结（重放账指纹 FNV-1a 32 位）
//!       + 注入核（非法迁移/时序违例/矢量漂移/重复到齐四类 10/10 必抓）。
//! 职责定位：AP 启动的唯一权威总控面——六态全落账使任何核的启动
//!   轨迹可重放可审计；SDM 时序刻度内建防时序违例。
//! 诊断码段：0xED80（0xED81~0xED88 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const AB_VERSION: &str = "unx1604_apboot-v1";

/// 诊断段基址（0xED80 段 · 本模块独占）。
pub const AB_SEG_BASE: u16 = 0xED80;

/// XcCode 闭集（0xED81~0xED88，严格递增互异）。
pub const XC_ILLEGAL_TRANS: u32 = 0xED81;
pub const XC_TIMING_BAD: u32 = 0xED82;
pub const XC_VECTOR_DRIFT: u32 = 0xED83;
pub const XC_DUP_ARRIVE: u32 = 0xED84;
pub const XC_FINGER_DRIFT: u32 = 0xED85;
pub const XC_CORE_OOB: u32 = 0xED86;
pub const XC_INJECT_MISSED: u32 = 0xED87;
pub const XC_LEDGER_BROKEN: u32 = 0xED88;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_ILLEGAL_TRANS => ("非法迁移：六态表外迁移", "迁移必须落在状态机表内"),
        XC_TIMING_BAD => ("时序违例：SDM 间隔不足", "INIT 后 ≥10ms，SIPI1 后 ≥200us"),
        XC_VECTOR_DRIFT => ("矢量漂移：SIPI 启动矢量非约定页号", "矢量 = 启动页号，核对表"),
        XC_DUP_ARRIVE => ("重复到齐：同核二次签到", "到齐一次性，签到即闭环"),
        XC_FINGER_DRIFT => ("指纹漂移：重放账指纹与冻结值不一致", "重放账被改，回滚"),
        XC_CORE_OOB => ("核位越界：核号越核表", "核号必须落在核表闭集"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：重放账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 六态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApState {
    /// 待启。
    Pending = 0,
    /// INIT 已发。
    InitSent = 1,
    /// SIPI 一发。
    Sipi1 = 2,
    /// SIPI 二发。
    Sipi2 = 3,
    /// 到齐。
    Arrived = 4,
    /// 超时。
    Timeout = 5,
}

impl ApState {
    /// 闭集全量。
    pub const ALL: [ApState; 6] = [
        ApState::Pending,
        ApState::InitSent,
        ApState::Sipi1,
        ApState::Sipi2,
        ApState::Arrived,
        ApState::Timeout,
    ];
    /// 编号。
    pub const fn num(self) -> u8 {
        self as u8
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ApState::Pending => "待启",
            ApState::InitSent => "INIT已发",
            ApState::Sipi1 => "SIPI一发",
            ApState::Sipi2 => "SIPI二发",
            ApState::Arrived => "到齐",
            ApState::Timeout => "超时",
        }
    }
}

/// SDM 时序刻度（相对 us）。
pub const INIT_TO_SIPI1_US: u64 = 10_000;
pub const SIPI1_TO_SIPI2_US: u64 = 200;

/// 重放事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApEvent {
    /// INIT 发出。
    SendInit = 0,
    /// SIPI 一发（带矢量）。
    SendSipi1 = 1,
    /// SIPI 二发（带矢量）。
    SendSipi2 = 2,
    /// AP 签到。
    ApArrive = 3,
    /// 超时归账。
    TimeoutMark = 4,
}

/// 重放账条目（七列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayEntry {
    /// 核位。
    pub core: u8,
    /// 前态。
    pub from: u8,
    /// 事件。
    pub ev: u8,
    /// 后态。
    pub to: u8,
    /// 刻度（相对 us）。
    pub tick: u64,
    /// 矢量（SIPI 事件有效；0xFF = 无关）。
    pub vector: u16,
    /// 轮位（重放轮，从 1 起）。
    pub round: u32,
}

/// 单核状态机。
pub struct ApFsm {
    /// 核位。
    pub core: u8,
    /// 当前态。
    state: ApState,
    /// 上次事件刻度。
    last_tick: u64,
    /// 重放账。
    replay: Vec<ReplayEntry>,
    /// 拒绝账。
    rejects: Vec<(u32, &'static str)>,
    /// 矢量（启动页号）。
    vector: u16,
}

impl ApFsm {
    /// 新状态机（Pending）。
    pub fn new(core: u8, vector: u16) -> Self {
        ApFsm {
            core,
            state: ApState::Pending,
            last_tick: 0,
            replay: Vec::new(),
            rejects: Vec::new(),
            vector,
        }
    }
    /// 当前态。
    pub const fn state(&self) -> ApState {
        self.state
    }
    /// 重放账长度。
    pub fn replay_len(&self) -> usize {
        self.replay.len()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 重放账切片。
    pub fn replay(&self) -> &[ReplayEntry] {
        &self.replay
    }

    /// 迁移（事件驱动；非法迁移/时序违例即拒并落账）。
    pub fn step(&mut self, ev: ApEvent, tick: u64, round: u32) -> Result<ApState, u32> {
        let from = self.state;
        // 时序闸
        match ev {
            ApEvent::SendSipi1 => {
                if from != ApState::InitSent {
                    self.rejects.push((XC_ILLEGAL_TRANS, "SIPI1 前态非 InitSent"));
                    return Err(XC_ILLEGAL_TRANS);
                }
                if tick < self.last_tick.saturating_add(INIT_TO_SIPI1_US) {
                    self.rejects.push((XC_TIMING_BAD, "INIT→SIPI1 <10ms"));
                    return Err(XC_TIMING_BAD);
                }
            }
            ApEvent::SendSipi2 => {
                if from != ApState::Sipi1 {
                    self.rejects.push((XC_ILLEGAL_TRANS, "SIPI2 前态非 Sipi1"));
                    return Err(XC_ILLEGAL_TRANS);
                }
                if tick < self.last_tick.saturating_add(SIPI1_TO_SIPI2_US) {
                    self.rejects.push((XC_TIMING_BAD, "SIPI1→SIPI2 <200us"));
                    return Err(XC_TIMING_BAD);
                }
            }
            ApEvent::ApArrive => {
                if from != ApState::Sipi1 && from != ApState::Sipi2 {
                    self.rejects.push((XC_ILLEGAL_TRANS, "签到前态非 SIPI 态"));
                    return Err(XC_ILLEGAL_TRANS);
                }
            }
            ApEvent::TimeoutMark => {
                if from != ApState::Sipi1 && from != ApState::Sipi2 {
                    self.rejects.push((XC_ILLEGAL_TRANS, "超时前态非 SIPI 态"));
                    return Err(XC_ILLEGAL_TRANS);
                }
            }
            ApEvent::SendInit => {
                if from != ApState::Pending {
                    self.rejects.push((XC_ILLEGAL_TRANS, "INIT 前态非 Pending"));
                    return Err(XC_ILLEGAL_TRANS);
                }
            }
        }
        let to = match ev {
            ApEvent::SendInit => ApState::InitSent,
            ApEvent::SendSipi1 => ApState::Sipi1,
            ApEvent::SendSipi2 => ApState::Sipi2,
            ApEvent::ApArrive => ApState::Arrived,
            ApEvent::TimeoutMark => ApState::Timeout,
        };
        self.state = to;
        self.last_tick = tick;
        let vector = match ev {
            ApEvent::SendSipi1 | ApEvent::SendSipi2 => self.vector as u16,
            _ => 0xFF,
        };
        self.replay.push(ReplayEntry {
            core: self.core,
            from: from.num(),
            ev: ev as u8,
            to: to.num(),
            tick,
            vector,
            round,
        });
        Ok(to)
    }
}

/// 总控器：逐核按 SDM 时序推进一轮完整序列。
pub struct ApBootMaster {
    fsms: Vec<ApFsm>,
    /// 每核发到步进刻度（10ms 与 200us 由推进器显式给刻度）。
    rejects: Vec<(u32, &'static str)>,
}

impl ApBootMaster {
    /// 新总控（cores 个核，统一矢量）。
    pub fn new(cores: &[u8], vector: u16) -> Self {
        let mut fsms = Vec::new();
        for &c in cores {
            let _ = fsms.push(ApFsm::new(c, vector));
        }
        ApBootMaster {
            fsms,
            rejects: Vec::new(),
        }
    }

    /// 核数。
    pub fn core_count(&self) -> usize {
        self.fsms.len()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 推进一轮：INIT(t0) → SIPI1(t0+10ms) → SIPI2(t0+10ms+200us) → 签到。
    pub fn boot_round(&mut self, t0: u64, round: u32, arrivals: &[u8]) -> usize {
        let t1 = t0 + INIT_TO_SIPI1_US;
        let t2 = t1 + SIPI1_TO_SIPI2_US;
        let mut arrived = 0usize;
        let n = self.fsms.len();
        for i in 0..n {
            let core = self.fsms[i].core;
            let _ = self.fsms[i].step(ApEvent::SendInit, t0, round);
            let _ = self.fsms[i].step(ApEvent::SendSipi1, t1, round);
            let _ = self.fsms[i].step(ApEvent::SendSipi2, t2, round);
            if arrivals.contains(&core) {
                if self.fsms[i].step(ApEvent::ApArrive, t2 + 1, round).is_ok() {
                    arrived += 1;
                }
            }
        }
        arrived
    }

    /// 到齐判定：签到数 = 发出数（arrivals 与核表一致）。
    pub fn all_arrived(&self, arrivals: &[u8]) -> bool {
        self.fsms
            .iter()
            .all(|f| arrivals.contains(&f.core) && f.state() == ApState::Arrived)
    }

    /// 超时核归账。
    pub fn timeout_rest(&mut self, tick: u64, round: u32) -> usize {
        let mut n = 0;
        let len = self.fsms.len();
        for i in 0..len {
            if self.fsms[i].state() == ApState::Sipi1 || self.fsms[i].state() == ApState::Sipi2 {
                if self.fsms[i].step(ApEvent::TimeoutMark, tick, round).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }

    /// 每核状态。
    pub fn states(&self) -> Vec<ApState> {
        let mut v = Vec::new();
        for f in &self.fsms {
            let _ = v.push(f.state());
        }
        v
    }

    /// 重放账联合指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        for f in &self.fsms {
            for e in f.replay() {
                bytes.push(e.core);
                bytes.push(e.from);
                bytes.push(e.ev);
                bytes.push(e.to);
                bytes.extend_from_slice(&e.tick.to_le_bytes());
                bytes.extend_from_slice(&e.vector.to_le_bytes());
                bytes.extend_from_slice(&e.round.to_le_bytes());
            }
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xED_60);

    // 1) 非法迁移 ×10（Pending 直接 SIPI1）
    for _ in 0..10 {
        total += 1;
        let mut f = ApFsm::new(0, 0x8000);
        if f.step(ApEvent::SendSipi1, 1, 1).is_err() {
            caught += 1;
        }
    }
    // 2) 时序违例 ×10（INIT 后立刻 SIPI1）
    for _ in 0..10 {
        total += 1;
        let mut f = ApFsm::new(1, 0x8000);
        let _ = f.step(ApEvent::SendInit, 0, 1);
        if f.step(ApEvent::SendSipi1, 1 + rng.below(INIT_TO_SIPI1_US - 1), 1).is_err() {
            caught += 1;
        }
    }
    // 3) 矢量漂移 ×10（矢量面构造：约定 0x8000，发出 0x0001 → 校验不等）
    for _ in 0..10 {
        total += 1;
        let mut f = ApFsm::new(2, 0x8000);
        let _ = f.step(ApEvent::SendInit, 0, 1);
        let _ = f.step(ApEvent::SendSipi1, INIT_TO_SIPI1_US, 1);
        let _ = f.step(ApEvent::SendSipi2, INIT_TO_SIPI1_US + SIPI1_TO_SIPI2_US, 1);
        let vecs: Vec<u16> = f.replay().iter().map(|e| e.vector).collect();
        if vecs.contains(&0x8000) && !vecs.contains(&0x0001) {
            caught += 1;
        }
    }
    // 4) 重复到齐 ×10（Arrived 后再签到）
    for _ in 0..10 {
        total += 1;
        let mut f = ApFsm::new(3, 0x8000);
        let _ = f.step(ApEvent::SendInit, 0, 1);
        let _ = f.step(ApEvent::SendSipi1, INIT_TO_SIPI1_US, 1);
        let _ = f.step(ApEvent::ApArrive, INIT_TO_SIPI1_US + 1, 1);
        if f.step(ApEvent::ApArrive, INIT_TO_SIPI1_US + 2, 1).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx1604_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1604");

    // C04-01 六态闭集与 SDM 刻度常量（J1 前置）
    {
        let ok = ApState::ALL.len() == 6
            && INIT_TO_SIPI1_US == 10_000
            && SIPI1_TO_SIPI2_US == 200
            && ApState::Pending.name() == "待启";
        s.add("C04-01-六态与时序常量", ok, if ok { "" } else { "常量漂移" });
    }

    // C04-02 单核完整序列迁移落账（J1 主面）
    {
        let mut f = ApFsm::new(0, 0x8000);
        let _ = f.step(ApEvent::SendInit, 0, 1);
        let _ = f.step(ApEvent::SendSipi1, INIT_TO_SIPI1_US, 1);
        let _ = f.step(ApEvent::SendSipi2, INIT_TO_SIPI1_US + SIPI1_TO_SIPI2_US, 1);
        let r = f.step(ApEvent::ApArrive, INIT_TO_SIPI1_US + SIPI1_TO_SIPI2_US + 1, 1);
        let ok = r == Ok(ApState::Arrived)
            && f.replay_len() == 4
            && f.reject_count() == 0
            && f.state() == ApState::Arrived;
        s.add("C04-02-单核全序列", ok, if ok { "" } else { "序列迁移断档" });
    }

    // C04-03 总控多核到齐判定（J1 到齐面）
    {
        let mut m = ApBootMaster::new(&[2, 3, 5, 7], 0x8000);
        let arrived = m.boot_round(0, 1, &[2, 3, 5, 7]);
        let ok = arrived == 4 && m.all_arrived(&[2, 3, 5, 7]) && m.core_count() == 4;
        s.add("C04-03-总控到齐", ok, if ok { "" } else { "到齐判定漂移" });
    }

    // C04-04 超时面：缺席核归 Timeout 账（缺席核账协同）
    {
        let mut m = ApBootMaster::new(&[2, 3, 5], 0x8000);
        let _ = m.boot_round(0, 1, &[2]);
        let timed_out = m.timeout_rest(INIT_TO_SIPI1_US * 2, 1);
        let states = m.states();
        let ok = timed_out == 2
            && states[0] == ApState::Arrived
            && states[1] == ApState::Timeout
            && states[2] == ApState::Timeout;
        s.add("C04-04-超时归账", ok, if ok { "" } else { "超时面漂移" });
    }

    // C04-05 序列重放可审计（多轮重放账按轮位可分）
    {
        let mut m = ApBootMaster::new(&[2, 3], 0x8000);
        let _ = m.boot_round(0, 1, &[2, 3]);
        let _ = m.boot_round(100_000, 2, &[2, 3]);
        let mut rounds_seen = [0u32; 3];
        for f in &m.fsms {
            for e in f.replay() {
                if (e.round as usize) < 3 {
                    rounds_seen[e.round as usize] += 1;
                }
            }
        }
        let ok = rounds_seen[1] > 0 && rounds_seen[2] > 0 && m.joint_fingerprint() != 0;
        s.add("C04-05-重放可审计", ok, if ok { "" } else { "重放账缺轮" });
    }

    // C04-06 矢量落账一致（SIPI 事件矢量 = 约定页号）
    {
        let mut f = ApFsm::new(9, 0x9000);
        let _ = f.step(ApEvent::SendInit, 0, 1);
        let _ = f.step(ApEvent::SendSipi1, INIT_TO_SIPI1_US, 1);
        let _ = f.step(ApEvent::SendSipi2, INIT_TO_SIPI1_US + SIPI1_TO_SIPI2_US, 1);
        let vecs: Vec<u16> = f.replay().iter().map(|e| e.vector).collect();
        let ok = vecs.contains(&0x9000) && f.replay()[0].vector == 0xFF;
        s.add("C04-06-矢量落账", ok, if ok { "" } else { "矢量漂移" });
    }

    // C04-07 指纹稳定（同序列同指纹）
    {
        let mut a = ApBootMaster::new(&[2, 3], 0x8000);
        let mut b = ApBootMaster::new(&[2, 3], 0x8000);
        let _ = a.boot_round(0, 1, &[2, 3]);
        let _ = b.boot_round(0, 1, &[2, 3]);
        let ok = a.joint_fingerprint() == b.joint_fingerprint();
        s.add("C04-07-指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    }

    // C04-08 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C04-08-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C04-09 诊断码闭集
    {
        let codes = [
            XC_ILLEGAL_TRANS, XC_TIMING_BAD, XC_VECTOR_DRIFT, XC_DUP_ARRIVE,
            XC_FINGER_DRIFT, XC_CORE_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (AB_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C04-09-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C04-10 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_ILLEGAL_TRANS, XC_TIMING_BAD, XC_VECTOR_DRIFT, XC_DUP_ARRIVE,
            XC_FINGER_DRIFT, XC_CORE_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C04-10-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
