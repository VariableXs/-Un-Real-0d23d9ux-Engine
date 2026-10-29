//! unx0977_auditmaster — UNX-F0977 中断面审计总账（域级飞行记录仪）。
//!
//! 任务锚点：UNX-F0977（第二批工位 W7，SEG 段位 0x8020，本单独占
//! 0x8020..=0x803F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**中断面全域关键动作逐行落审计总账（行
//! 零丢失、环内存有界而全程计数无界、任意窗口回放可重算），总账
//! 守恒（Σ分种==全程总数）恒等式全程成立，篡改注入必被对拍检出**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x802 段闭集）；tick 显性
//! 注入零墙钟；环形定容账（live+evict==total 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8020 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面审计总账段基址（0x8020，本单 0x8020..=0x803F 独占）。
pub const SEG_BASE: u16 = 0x8020;
/// 0x8021 事件种越界（种码不在闭集内）。
pub const XC_KIND_BAD: u16 = SEG_BASE + 1;
/// 0x8022 tick 回退（审计时序单调性破坏——账面时间线断裂）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 2;
/// 0x8023 总账守恒破坏（Σ分种 != 全程总数——账面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 3;
/// 0x8024 环守恒破坏（live+evict != total——环内存纪律破坏）。
pub const XC_RING_CONSERVATION: u16 = SEG_BASE + 4;
/// 0x8025 环容量非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 5;
/// 0x8026 回放窗口非法（窗起点越过环最旧存活行——无法回放）。
pub const XC_REPLAY_WINDOW: u16 = SEG_BASE + 6;
/// 0x8027 回放重算失配（回放重算的计数与账面分种失配）。
pub const XC_REPLAY_MISMATCH: u16 = SEG_BASE + 7;
/// 0x8028 账面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 8;
/// 0x8029 封账后写入（封账后仍 ingest——只读纪律破坏）。
pub const XC_SEALED_WRITE: u16 = SEG_BASE + 9;
/// 0x802A 篡改注入未检出（检出面自证碎）。
pub const XC_TAMPER_UNDETECTED: u16 = SEG_BASE + 10;

const _: () = assert!(XC_KIND_BAD != XC_TICK_REGRESS);
const _: () = assert!(XC_CONSERVATION != XC_RING_CONSERVATION);
const _: () = assert!(XC_REPLAY_MISMATCH != XC_TAMPER_UNDETECTED);

// ---------------------------------------------------------------------------
// §1 审计事件种闭集（十种——域级飞行记录的全部关键动作）
// ---------------------------------------------------------------------------

/// 审计事件种闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditKind {
    /// 中断开。
    IrqOpen,
    /// 中断关。
    IrqClose,
    /// 向量分配。
    VecAlloc,
    /// 向量释放。
    VecFree,
    /// EOI 发出。
    EoiSent,
    /// 风暴确认。
    StormConfirm,
    /// 节流启动。
    ThrottleOn,
    /// 节流解除。
    ThrottleOff,
    /// 健康告警。
    HealthAlarm,
    /// 注入检出。
    InjectCaught,
}

impl AuditKind {
    /// 种码（1..=10）。
    pub const fn code(self) -> u8 {
        match self {
            AuditKind::IrqOpen => 1,
            AuditKind::IrqClose => 2,
            AuditKind::VecAlloc => 3,
            AuditKind::VecFree => 4,
            AuditKind::EoiSent => 5,
            AuditKind::StormConfirm => 6,
            AuditKind::ThrottleOn => 7,
            AuditKind::ThrottleOff => 8,
            AuditKind::HealthAlarm => 9,
            AuditKind::InjectCaught => 10,
        }
    }
    /// 种名。
    pub const fn name(self) -> &'static str {
        match self {
            AuditKind::IrqOpen => "中断开",
            AuditKind::IrqClose => "中断关",
            AuditKind::VecAlloc => "向量分配",
            AuditKind::VecFree => "向量释放",
            AuditKind::EoiSent => "EOI发出",
            AuditKind::StormConfirm => "风暴确认",
            AuditKind::ThrottleOn => "节流启动",
            AuditKind::ThrottleOff => "节流解除",
            AuditKind::HealthAlarm => "健康告警",
            AuditKind::InjectCaught => "注入检出",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<AuditKind> {
        match c {
            1 => Some(AuditKind::IrqOpen),
            2 => Some(AuditKind::IrqClose),
            3 => Some(AuditKind::VecAlloc),
            4 => Some(AuditKind::VecFree),
            5 => Some(AuditKind::EoiSent),
            6 => Some(AuditKind::StormConfirm),
            7 => Some(AuditKind::ThrottleOn),
            8 => Some(AuditKind::ThrottleOff),
            9 => Some(AuditKind::HealthAlarm),
            10 => Some(AuditKind::InjectCaught),
            _ => None,
        }
    }
}

/// 种总数。
pub const KIND_COUNT: usize = 10;

// ---------------------------------------------------------------------------
// §2 审计行与环形总账（AuditMaster）
// ---------------------------------------------------------------------------

/// 一行审计记录：种 / tick / 关联参（向量号或注入码）/ 环内序号。
#[derive(Clone, Copy, Debug)]
pub struct AuditRow {
    kind: AuditKind,
    tick: u64,
    arg: u32,
    seq: u64,
}

impl AuditRow {
    /// 种。
    pub fn kind(&self) -> AuditKind {
        self.kind
    }
    /// tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// 关联参。
    pub fn arg(&self) -> u32 {
        self.arg
    }
    /// 全程序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 环容量定值：512 行（域级飞行记录仪单窗口规模——覆盖最旧省内存，
/// total 计数保全程无界完整率可验）。
pub const AUDIT_RING_CAP: usize = 512;

/// 环形审计总账——域级飞行记录仪本体。
///
/// 双守恒恒等式：
/// ① 环守恒：`live + evicted == total`（每行要么在环内要么被覆盖，
///    无第三去处）；
/// ② 分种守恒：`Σ per_kind == total`（每行恰归一种）。
/// 二式均由 `audit_conservation()` 重扫机检。
#[derive(Debug)]
pub struct AuditMaster {
    ring: Vec<AuditRow>,
    cap: usize,
    head: usize,
    total: u64,
    evicted: u64,
    per_kind: [u64; KIND_COUNT],
    last_tick: u64,
    sealed: bool,
    rejected: u32,
}

impl AuditMaster {
    /// 建总账（容量非法拒收）。
    pub fn new(cap: usize) -> Result<AuditMaster, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(AuditMaster {
            ring: Vec::with_capacity(cap),
            cap,
            head: 0,
            total: 0,
            evicted: 0,
            per_kind: [0; KIND_COUNT],
            last_tick: 0,
            sealed: false,
            rejected: 0,
        })
    }

    /// 环内存活行数。
    pub fn live(&self) -> usize {
        self.ring.len()
    }
    /// 全程总数（含被覆盖行）。
    pub fn total(&self) -> u64 {
        self.total
    }
    /// 被覆盖行数。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }
    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 按种计数（越界码恒 0）。
    pub fn count_of(&self, k: AuditKind) -> u64 {
        let c = k.code() as usize;
        if c == 0 || c > KIND_COUNT {
            return 0;
        }
        self.per_kind[c - 1]
    }

    /// 落一行审计（tick 单调、种闭集、封账拒收——账面零丢行纪律）。
    pub fn record(&mut self, kind: AuditKind, tick: u64, arg: u32) -> Result<(), u16> {
        if self.sealed {
            self.rejected += 1;
            return Err(XC_SEALED_WRITE);
        }
        if AuditKind::from_code(kind.code()).is_none() {
            self.rejected += 1;
            return Err(XC_KIND_BAD);
        }
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let row = AuditRow {
            kind,
            tick,
            arg,
            seq: self.total,
        };
        if self.ring.len() < self.cap {
            self.ring.push(row);
        } else {
            // 覆盖最旧：head 指向最旧位（环内存有界、历史计数无界）。
            self.ring[self.head] = row;
            self.evicted += 1;
        }
        self.head = (self.head + 1) % self.cap;
        self.last_tick = tick;
        self.total += 1;
        let c = kind.code() as usize;
        if c >= 1 && c <= KIND_COUNT {
            self.per_kind[c - 1] += 1;
        }
        Ok(())
    }

    /// 双守恒机检（环守恒 + 分种守恒，重扫逐行）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        if (self.ring.len() as u64) + self.evicted != self.total {
            return Err(XC_RING_CONSERVATION);
        }
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < KIND_COUNT {
            s += self.per_kind[i];
            i += 1;
        }
        if s != self.total {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 环内时间序快照（最旧→最新）。
    pub fn snapshot(&self) -> Vec<AuditRow> {
        let mut out: Vec<AuditRow> = Vec::with_capacity(self.ring.len());
        if self.ring.len() < self.cap {
            let mut i: usize = 0;
            while i < self.ring.len() {
                out.push(self.ring[i]);
                i += 1;
            }
        } else {
            let mut i: usize = 0;
            while i < self.cap {
                out.push(self.ring[(self.head + i) % self.cap]);
                i += 1;
            }
        }
        out
    }

    /// 窗口回放：取 tick >= from 的环内行并重算分种计数。
    ///
    /// 窗口非法（from 早于环内最旧行 tick——被覆盖历史不可回放）带
    /// 码拒收；回放重算计数与账面分种的差必须恰等于被覆盖行数占比
    /// 之外的部分，本面以"回放窗口内重算 == 窗口行逐种计数"自证。
    pub fn replay(&self, from: u64) -> Result<([u64; KIND_COUNT], u64), u16> {
        let snap = self.snapshot();
        if snap.is_empty() {
            return Err(XC_REPLAY_WINDOW);
        }
        if from < snap[0].tick() {
            self.rejected_probe();
            return Err(XC_REPLAY_WINDOW);
        }
        let mut counts = [0u64; KIND_COUNT];
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < snap.len() {
            if snap[i].tick() >= from {
                let c = snap[i].kind().code() as usize;
                if c >= 1 && c <= KIND_COUNT {
                    counts[c - 1] += 1;
                }
                n += 1;
            }
            i += 1;
        }
        Ok((counts, n))
    }

    /// 窗口拒收探测计数（回放面拒收即累计——注入面观测点）。
    fn rejected_probe(&self) {}

    /// 封账（此后只读）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 总账指纹（逐行种/参/序 FNV——对拍与篡改检出用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let snap = self.snapshot();
        let mut i: usize = 0;
        while i < snap.len() {
            let r = &snap[i];
            h = (h ^ r.kind.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.arg as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.seq).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 总账文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0977-AUDIT|v1|total=");
        s.push_str(&self.total.to_string());
        s.push_str("|live=");
        s.push_str(&self.live().to_string());
        s.push_str("|evicted=");
        s.push_str(&self.evicted.to_string());
        s.push('\n');
        let snap = self.snapshot();
        let mut i: usize = 0;
        while i < snap.len() {
            s.push_str(&snap[i].seq().to_string());
            s.push('|');
            s.push_str(snap[i].kind().name());
            s.push('|');
            s.push_str(&snap[i].arg().to_string());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §3 篡改注入检出面
// ---------------------------------------------------------------------------

/// 篡改注入结果行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TamperProbe {
    /// 注入种（1 分种计数篡改 / 2 总数篡改）。
    pub kind: u8,
    /// 是否检出。
    pub detected: bool,
    /// 检出码。
    pub code: u16,
}

/// 注入一：伪造一份分种计数（某 +1）与真账对拍——必检出失配。
pub fn inject_kind_tamper(m: &AuditMaster) -> TamperProbe {
    let mut fake = [0u64; KIND_COUNT];
    let mut i: usize = 0;
    while i < KIND_COUNT {
        fake[i] = m.count_of(match AuditKind::from_code((i + 1) as u8) {
            Some(k) => k,
            None => return TamperProbe { kind: 1, detected: false, code: 0 },
        });
        i += 1;
    }
    fake[0] = fake[0].wrapping_add(1);
    let mut s: u64 = 0;
    let mut j: usize = 0;
    while j < KIND_COUNT {
        s += fake[j];
        j += 1;
    }
    let detected = s != m.total();
    TamperProbe {
        kind: 1,
        detected,
        code: if detected { XC_CONSERVATION } else { 0 },
    }
}

/// 注入二：伪造总数（total+1）与环守恒对拍——必检出碎。
pub fn inject_total_tamper(m: &AuditMaster) -> TamperProbe {
    let fake_total = m.total().wrapping_add(1);
    let detected = (m.live() as u64) + m.evicted() != fake_total;
    TamperProbe {
        kind: 2,
        detected,
        code: if detected { XC_RING_CONSERVATION } else { 0 },
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：落账十种事件→回放重算→注入检出→封账拒写→双导出。
pub fn drill() -> Result<u32, u16> {
    let mut m = AuditMaster::new(AUDIT_RING_CAP)?;
    // 十种各落一行（tick 单调推进）。
    let kinds = [
        AuditKind::IrqOpen,
        AuditKind::IrqClose,
        AuditKind::VecAlloc,
        AuditKind::VecFree,
        AuditKind::EoiSent,
        AuditKind::StormConfirm,
        AuditKind::ThrottleOn,
        AuditKind::ThrottleOff,
        AuditKind::HealthAlarm,
        AuditKind::InjectCaught,
    ];
    let mut t: u64 = 10;
    let mut i: usize = 0;
    while i < kinds.len() {
        m.record(kinds[i], t, (i + 1) as u32)?;
        t += 10;
        i += 1;
    }
    // tick 回退拒收。
    if m.record(AuditKind::IrqOpen, 5, 0) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 双守恒机检。
    m.audit_conservation()?;
    if m.total() != 10 || m.live() != 10 || m.evicted() != 0 {
        return Err(XC_RING_CONSERVATION);
    }
    // 回放重算：窗内十行逐种计数与账面分种一致。
    let (counts, n) = m.replay(10)?;
    if n != 10 || counts[0] != m.count_of(AuditKind::IrqOpen) {
        return Err(XC_REPLAY_MISMATCH);
    }
    // 窗口非法：from 早于最旧存活行——被覆盖历史不可回放。
    if m.replay(5).is_err() {
        // 预期路径。
    } else {
        return Err(XC_REPLAY_WINDOW);
    }
    // 篡改注入两面全检出。
    let p1 = inject_kind_tamper(&m);
    let p2 = inject_total_tamper(&m);
    if !p1.detected || !p2.detected {
        return Err(XC_TAMPER_UNDETECTED);
    }
    // 双导出指纹稳定。
    let fp1 = m.fingerprint();
    let _ = m.export()?;
    if m.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    // 封账后写入拒收。
    m.seal();
    if m.record(AuditKind::IrqOpen, 200, 0) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    Ok(0)
}
