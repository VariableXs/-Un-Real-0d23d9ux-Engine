//! unx803_irqpool —— UNX-F0803 设备 IRQ 向量区 0x20–0x8F 池化与首配分配。
//! 锚点（CoRun Unxreal 主册）：UNX-F0803 · 设备 IRQ 向量区 0x20–0x8F 池化与
//!       首配分配 · 目标 300 行 · 已深化
//! 判据原文：UNX-F0803-J1 设备池向量申请-归还状态机一致（万次操作零悬空），
//!           越界申请被拒并留账
//! 组件：向量池模型（0x20..0x8F 闭集 112 槽：空闲/占用/保留三态）+
//!       首配分配器（首个空闲槽 O(n) 扫描 + 分配留账）+ 归还状态机
//!       （占用 → 空闲；空闲归还/越界归还拒绝留账）+ 申请-归还账
//!       （op 序号/向量/设备号/操作四列 append-only）+ 悬空零容忍核
//!       （终态账 vs 池位图逐槽一致）+ 万次压测泵（LCG 随机申请/归还
//!       序列 + 一致性断言）+ 注入核（越界/重复归还/重复申请/保留槽
//!       申请四类注入 10/10 必抓）。
//! 职责定位：设备中断向量资源的唯一权威分配面——0x20..0x8F 闭集使
//!   向量空间可枚举可审计；首配分配保证确定性（同序列同结果）；
//!   万次零悬空使池化语义经压测钉死（F0804 MSI 消费本池）。
//! 性能逐项分解：申请 O(池)；归还 O(1)；一致性核 O(池)；压测泵
//!   O(万次×池/2 均摊)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xD350（0xD351~0xD359 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;

/// 模块版本。
pub const IP_VERSION: &str = "unx803_irqpool-v1";

/// 诊断段基址（0xD350 段 · 本模块独占）。
pub const IP_SEG_BASE: u16 = 0xD350;

/// XcCode 闭集（0xD351~0xD359）。
pub const XC_VEC_RANGE: u32 = 0xD351;
pub const XC_VEC_BUSY: u32 = 0xD352;
pub const XC_VEC_NOT_MINE: u32 = 0xD353;
pub const XC_DOUBLE_FREE: u32 = 0xD354;
pub const XC_RESERVED: u32 = 0xD355;
pub const XC_POOL_DANGLING: u32 = 0xD356;
pub const XC_LEDGER_BROKEN: u32 = 0xD357;
pub const XC_POOL_EXHAUST: u32 = 0xD358;
pub const XC_INJECT_MISSED: u32 = 0xD359;

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_VEC_RANGE => ("向量越界（0x20..0x8F 闭集之外）", "改从池内首配分配"),
        XC_VEC_BUSY => ("向量已被占用（重复申请）", "改配下一空闲槽"),
        XC_VEC_NOT_MINE => ("归还向量不属于该设备（错主归还）", "归还前核对设备归属账"),
        XC_DOUBLE_FREE => ("向量重复归还（空闲态归还）", "归还前查池位图状态"),
        XC_RESERVED => ("保留槽申请被拒（调试/伪中断专用）", "改配普通槽位"),
        XC_POOL_DANGLING => ("池位图与终态账不一致（悬空）", "全量对账后修复位图"),
        XC_LEDGER_BROKEN => ("申请-归还账序号跳号", "检查账务封装是否被绕过"),
        XC_POOL_EXHAUST => ("向量池耗尽（112 槽全占）", "回收空闲或走 MSI 扩展"),
        XC_INJECT_MISSED => ("注入故障未被池闸检出", "修复池闸比对逻辑后重跑"),
        _ => ("未知诊断码", "查 0xD350 段闭集登记表"),
    }
}

/// 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x0803_4950)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, n) 确定性取值（LCG 均匀映射）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }
}

/// 池边界闭集。
pub const POOL_LO: u8 = 0x20;
pub const POOL_HI: u8 = 0x8F;
/// 池容量（112 槽）。
pub const POOL_CAP: usize = (POOL_HI - POOL_LO + 1) as usize;

/// 槽三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 空闲。
    Free,
    /// 占用（记设备号）。
    Busy(u32),
    /// 保留（最后 4 槽：调试/伪中断专用）。
    Reserved,
}

/// 操作账行。
#[derive(Clone, Copy, Debug)]
pub struct IpOp {
    pub seq: u64,
    /// 1=申请 2=归还。
    pub kind: u8,
    pub vec: u8,
    pub dev: u32,
    /// 1=成功 0=拒绝。
    pub ok: u8,
}

/// 向量池。
pub struct IrqPool {
    slots: [SlotState; POOL_CAP],
    ledger: Vec<IpOp>,
    next_seq: u64,
    pub alloc_count: u64,
    pub free_count: u64,
    pub reject_count: u64,
}

impl IrqPool {
    pub fn new() -> Self {
        let mut slots = [SlotState::Free; POOL_CAP];
        // 保留槽：0x8C..0x8F。
        for i in (POOL_CAP - 4)..POOL_CAP {
            slots[i] = SlotState::Reserved;
        }
        IrqPool {
            slots,
            ledger: Vec::new(),
            next_seq: 1,
            alloc_count: 0,
            free_count: 0,
            reject_count: 0,
        }
    }

    fn idx(vec: u8) -> Option<usize> {
        if vec < POOL_LO || vec > POOL_HI {
            None
        } else {
            Some((vec - POOL_LO) as usize)
        }
    }

    /// 首配申请：从 0x20 起第一个 Free 槽分配给 dev。
    pub fn alloc(&mut self, dev: u32) -> Result<u8, u32> {
        for i in 0..POOL_CAP {
            if self.slots[i] == SlotState::Free {
                self.slots[i] = SlotState::Busy(dev);
                let vec = POOL_LO + i as u8;
                self.record(1, vec, dev, 1);
                self.alloc_count += 1;
                return Ok(vec);
            }
        }
        self.reject_count += 1;
        self.record(1, 0, dev, 0);
        Err(XC_POOL_EXHAUST)
    }

    /// 定向申请（越界/保留/占用均拒）。
    pub fn alloc_at(&mut self, vec: u8, dev: u32) -> Result<(), u32> {
        let i = match Self::idx(vec) {
            Some(i) => i,
            None => {
                self.reject_count += 1;
                self.record(1, vec, dev, 0);
                return Err(XC_VEC_RANGE);
            }
        };
        match self.slots[i] {
            SlotState::Reserved => {
                self.reject_count += 1;
                self.record(1, vec, dev, 0);
                Err(XC_RESERVED)
            }
            SlotState::Busy(_) => {
                self.reject_count += 1;
                self.record(1, vec, dev, 0);
                Err(XC_VEC_BUSY)
            }
            SlotState::Free => {
                self.slots[i] = SlotState::Busy(dev);
                self.alloc_count += 1;
                self.record(1, vec, dev, 1);
                Ok(())
            }
        }
    }

    /// 归还（空闲态归还/非本主归还均拒）。
    pub fn free(&mut self, vec: u8, dev: u32) -> Result<(), u32> {
        let i = match Self::idx(vec) {
            Some(i) => i,
            None => {
                self.reject_count += 1;
                self.record(2, vec, dev, 0);
                return Err(XC_VEC_RANGE);
            }
        };
        match self.slots[i] {
            SlotState::Free | SlotState::Reserved => {
                self.reject_count += 1;
                self.record(2, vec, dev, 0);
                Err(XC_DOUBLE_FREE)
            }
            SlotState::Busy(owner) => {
                if owner != dev {
                    self.reject_count += 1;
                    self.record(2, vec, dev, 0);
                    return Err(XC_VEC_NOT_MINE);
                }
                self.slots[i] = SlotState::Free;
                self.free_count += 1;
                self.record(2, vec, dev, 1);
                Ok(())
            }
        }
    }

    fn record(&mut self, kind: u8, vec: u8, dev: u32, ok: u8) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ledger.push(IpOp { seq, kind, vec, dev, ok });
    }

    /// 账序号连续核。
    pub fn ledger_ok(&self) -> bool {
        self.ledger
            .iter()
            .enumerate()
            .all(|(i, op)| op.seq == i as u64 + 1)
    }

    /// 悬空核：位图占用槽的设备号在账中必须有对应成功申请。
    pub fn no_dangling(&self) -> bool {
        for i in 0..POOL_CAP {
            if let SlotState::Busy(dev) = self.slots[i] {
                let vec = POOL_LO + i as u8;
                let has_alloc = self
                    .ledger
                    .iter()
                    .any(|op| op.kind == 1 && op.vec == vec && op.dev == dev && op.ok == 1);
                if !has_alloc {
                    return false;
                }
            }
        }
        true
    }

    /// 空闲槽计数。
    pub fn free_slots(&self) -> usize {
        self.slots.iter().filter(|s| **s == SlotState::Free).count()
    }

    /// 占用槽计数。
    pub fn busy_slots(&self) -> usize {
        self.slots.iter().filter(|s| matches!(s, SlotState::Busy(_))).count()
    }

    /// 万次压测泵：随机申请/归还，终态一致性断言。
    pub fn stress_10k(&mut self, seed: u64) -> Result<(), u32> {
        let mut rng = Lcg::new(seed);
        let mut held: Vec<(u8, u32)> = Vec::new();
        for op in 0..10_000u64 {
            let roll = rng.below(100);
            if roll < 55 || held.is_empty() {
                // 申请。
                if let Ok(v) = self.alloc((op % 97) as u32) {
                    held.push((v, (op % 97) as u32));
                }
            } else {
                // 归还（随机取持有一项）。
                let k = rng.below(held.len() as u64) as usize;
                let (v, d) = held[k];
                held.swap_remove(k);
                if self.free(v, d).is_err() {
                    return Err(XC_POOL_DANGLING);
                }
            }
        }
        // 收尾归还全部。
        while let Some((v, d)) = held.pop() {
            if self.free(v, d).is_err() {
                return Err(XC_POOL_DANGLING);
            }
        }
        if self.busy_slots() != 0 || self.free_slots() != POOL_CAP - 4 {
            return Err(XC_POOL_DANGLING);
        }
        if !self.no_dangling() || !self.ledger_ok() {
            return Err(XC_POOL_DANGLING);
        }
        Ok(())
    }
}

impl Default for IrqPool {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入核：越界/重复归还/重复申请/保留槽申请四类，池闸必抓 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all_caught = true;

    // 注入一：越界申请（0x10 / 0x90）。
    for i in 0..3u32 {
        let mut p = IrqPool::new();
        let bad = if i == 0 { 0x10u8 } else { 0x90u8 };
        let r = p.alloc_at(bad, 1);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_VEC_RANGE;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "越界#{} vec={:#04x} caught={} code={:04X}h\n",
            i + 1,
            bad,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入二：重复归还（free 后再 free）。
    for i in 0..3u32 {
        let mut p = IrqPool::new();
        let v = p.alloc(7).unwrap_or(0);
        let _ = p.free(v, 7);
        let r = p.free(v, 7);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_DOUBLE_FREE;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "重复归还#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入三：重复申请（同槽定向二次申请）。
    for i in 0..2u32 {
        let mut p = IrqPool::new();
        let _ = p.alloc_at(0x21, 3);
        let r = p.alloc_at(0x21, 4);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_VEC_BUSY;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "重复申请#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入四：保留槽申请（0x8D）。
    for i in 0..2u32 {
        let mut p = IrqPool::new();
        let r = p.alloc_at(0x8D, 9);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_RESERVED;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "保留槽#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    (all_caught, log)
}

/// 验收入口：判据 J1 全链核（万次零悬空 + 越界留账 + 注入 10/10）。
pub fn drill_irqpool() -> (bool, &'static str) {
    let mut p = IrqPool::new();
    // 判据一：万次压测零悬空（两枚种子）。
    if p.stress_10k(0x0803_0001).is_err() {
        return (false, "F0803 万次压测一致性破缺");
    }
    let mut p2 = IrqPool::new();
    if p2.stress_10k(0x0803_0002).is_err() {
        return (false, "F0803 万次压测种子二破缺");
    }
    // 判据二：越界申请被拒并留账（账含 ok=0 行）。
    let mut p3 = IrqPool::new();
    let _ = p3.alloc_at(0x90, 1);
    if p3.reject_count == 0 || !p3.ledger_ok() {
        return (false, "F0803 越界留账缺失");
    }
    // 判据三：确定性首配（同序列同向量序列）。
    let mut a = IrqPool::new();
    let mut b = IrqPool::new();
    for d in 0..8u32 {
        let va = a.alloc(d).unwrap_or(0);
        let vb = b.alloc(d).unwrap_or(0);
        if va != vb || va != POOL_LO + d as u8 {
            return (false, "F0803 首配确定性破缺");
        }
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F0803 注入核未全抓");
    }
    (true, "F0803 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_irqpool().0);
    }
}
