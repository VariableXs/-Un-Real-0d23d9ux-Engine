//! UNX-F11150 B30 清账（多进程 IPC 压测判据终重放）（0x1f320 段）
//!
//! 纯功能模型：B30 批的判据是四机制 IPC 压测重放——管道、共享内存、消息队列、
//! 事件计数四路各跑同一口径轮次，逐条重放压测判据。清账要求四机制齐备、轮次
//! 同为 256、失败数归零、判据回放全绿；终值一经冻结，B30 不再收账。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f320;
/// 机制号越界（只认 0..4）
pub const IP_ENO: u32 = SEG_BASE + 0x01;
/// 机制重记
pub const IP_EDUP: u32 = SEG_BASE + 0x02;
/// 机制序倒挂
pub const IP_EORDER: u32 = SEG_BASE + 0x03;
/// 轮次为零或失败数超轮次（形状违规）
pub const IP_ESHAPE: u32 = SEG_BASE + 0x04;
/// 四机制未齐即清账
pub const IP_ESHORT: u32 = SEG_BASE + 0x05;
/// 机制间轮次不齐
pub const IP_EROUND: u32 = SEG_BASE + 0x06;
/// 压测判据未全绿
pub const IP_EFAIL: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const IP_EFROZEN: u32 = SEG_BASE + 0x08;

/// 四机制路数。
pub const MECHANISMS: usize = 4;
/// 每机制压测轮次口径。
pub const ROUNDS_PER_MECH: u32 = 256;
/// 每机制判据条数。
pub const CRITERIA_PER_MECH: u32 = 5;
/// B30 批内判据总数（4×5）。
pub const B30_CRITERIA: u32 = 20;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 机制名（静态，供明细行核对）。
pub const MECH_NAMES: [&str; MECHANISMS] = ["pipe", "shm", "msgq", "evtctr"];

/// 单机制压测结果。
#[derive(Clone, Copy, Debug)]
pub struct MechProbe {
    pub mech: u8,
    pub rounds: u32,
    pub failures: u32,
    pub verdict: bool,
}

impl MechProbe {
    /// 该机制压测判据是否全绿。
    pub fn cleaned(&self) -> bool {
        self.rounds == ROUNDS_PER_MECH && self.failures == 0 && self.verdict
    }
}

/// B30 四机制压测清账。
#[derive(Clone, Debug)]
pub struct IpcLedger {
    probes: Vec<MechProbe>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl IpcLedger {
    /// 空账。
    pub fn new() -> IpcLedger {
        IpcLedger {
            probes: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已记机制路数。
    pub fn len(&self) -> usize {
        self.probes.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// 取某机制记录。
    pub fn probe(&self, mech: usize) -> Option<MechProbe> {
        let mut i = 0usize;
        while i < self.probes.len() {
            if self.probes[i].mech as usize == mech {
                return Some(self.probes[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一路机制：冻结 → 越界 → 重记 → 倒序 → 形状。
    ///
    /// 重记先于倒序裁定：0 号已记后重来一次只报 IP_EDUP，不会顺带报
    /// IP_EORDER，两个错误码因此各自可达。
    pub fn record(
        &mut self,
        mech: usize,
        rounds: u32,
        failures: u32,
        verdict: bool,
    ) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(IP_EFROZEN);
        }
        if mech >= MECHANISMS {
            return self.fail(IP_ENO);
        }
        if self.probe(mech).is_some() {
            return self.fail(IP_EDUP);
        }
        if let Some(last) = self.probes.last() {
            if mech <= last.mech as usize {
                return self.fail(IP_EORDER);
            }
        }
        if rounds == 0 || failures > rounds {
            return self.fail(IP_ESHAPE);
        }
        self.probes.push(MechProbe {
            mech: mech as u8,
            rounds,
            failures,
            verdict,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl IpcLedger {
    /// 轮次总和。
    pub fn total_rounds(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.probes.len() {
            t = t.wrapping_add(self.probes[i].rounds as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 失败数总和。
    pub fn total_failures(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.probes.len() {
            t = t.wrapping_add(self.probes[i].failures as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 缺记机制表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut m = 0usize;
        while m < MECHANISMS {
            if self.probe(m).is_none() {
                out.push(m as u32);
            }
            m = m.wrapping_add(1);
        }
        out
    }

    /// 机制序是否严格递增（在册部分）。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.probes.len() {
            let prev = self.probes[i - 1].mech;
            if self.probes[i].mech <= prev {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 轮次是否齐（四路同口径）。
    pub fn rounds_uniform(&self) -> bool {
        let mut i = 0usize;
        while i < self.probes.len() {
            if self.probes[i].rounds != ROUNDS_PER_MECH {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 压测判据是否全绿。
    pub fn all_clean(&self) -> bool {
        let mut i = 0usize;
        while i < self.probes.len() {
            if !self.probes[i].cleaned() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 判据重放条数：每机制 5 条（轮次、零失败、判据为真、机制号在册、在册本身）。
    pub fn criteria_replayed(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.probes.len() {
            let p = self.probes[i];
            if p.rounds == ROUNDS_PER_MECH {
                n = n.wrapping_add(1);
            }
            if p.failures == 0 {
                n = n.wrapping_add(1);
            }
            if p.verdict {
                n = n.wrapping_add(1);
            }
            if (p.mech as usize) < MECHANISMS {
                n = n.wrapping_add(1);
            }
            n = n.wrapping_add(1);
            i = i.wrapping_add(1);
        }
        n
    }

    /// 清账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.probes.len() {
            let p = self.probes[i];
            h = (h ^ p.mech as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ p.rounds as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ p.failures as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ p.verdict as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.probes.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_rounds()).wrapping_mul(FNV_PRIME);
        (h ^ self.total_failures()).wrapping_mul(FNV_PRIME)
    }

    /// 终清账：齐 → 轮次同 → 判据全绿。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(IP_EFROZEN);
        }
        if self.probes.len() != MECHANISMS || !self.gaps().is_empty() {
            return self.fail(IP_ESHORT);
        }
        if !self.rounds_uniform() || !self.order_strict() {
            return self.fail(IP_EROUND);
        }
        if !self.all_clean() || self.criteria_replayed() != B30_CRITERIA {
            return self.fail(IP_EFAIL);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：四机制各 256 轮、零失败、判据为真，再清账。
pub fn build_b30() -> (IpcLedger, bool) {
    let mut l = IpcLedger::new();
    let mut m = 0usize;
    while m < MECHANISMS {
        if l.record(m, ROUNDS_PER_MECH, 0, true).is_err() {
            return (l, false);
        }
        m = m.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：机制越界 / 重记与倒序 / 形状与轮次 / 终账后再记。
pub fn inject_four() -> [bool; 4] {
    // ① 机制号越界
    let mut a0 = IpcLedger::new();
    let r0 = a0.record(MECHANISMS, ROUNDS_PER_MECH, 0, true).err() == Some(IP_ENO)
        && a0.record(MECHANISMS.wrapping_add(9), 1, 0, true).err() == Some(IP_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = IpcLedger::new();
    let s0 = a1.record(0, ROUNDS_PER_MECH, 0, true).is_ok();
    let s1 = a1.record(1, ROUNDS_PER_MECH, 0, true).is_ok();
    let r1 = s0
        && s1
        && a1.record(0, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EDUP)
        && a1.record(3, ROUNDS_PER_MECH, 0, true).is_ok()
        && a1.record(2, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EORDER);

    // ③ 形状违规、短账、轮次不齐
    let mut a2 = IpcLedger::new();
    let shape = a2.record(0, 0, 0, true).err() == Some(IP_ESHAPE)
        && a2.record(0, 8, 9, true).err() == Some(IP_ESHAPE);
    let short = a2.settle().err() == Some(IP_ESHORT);
    let mut a3 = IpcLedger::new();
    let _ = a3.record(0, ROUNDS_PER_MECH, 0, true);
    let _ = a3.record(1, ROUNDS_PER_MECH.wrapping_mul(2), 0, true);
    let _ = a3.record(2, ROUNDS_PER_MECH, 0, true);
    let _ = a3.record(3, ROUNDS_PER_MECH, 0, true);
    let round = a3.settle().err() == Some(IP_EROUND);
    let r2 = shape && short && round;

    // ④ 终账后再记 / 再清账
    let (mut a4, ok4) = build_b30();
    let locked = a4.freeze().is_some();
    let r3 = ok4
        && locked
        && a4.record(0, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EFROZEN)
        && a4.settle().err() == Some(IP_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11150 B30 多进程 IPC 压测判据终重放。
pub fn run_unx11150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11150");
    let (mut l, ok) = build_b30();

    // X1 四机制齐备
    s.add_dyn(
        "X1-四机制齐备",
        ok && l.len() == MECHANISMS && l.gaps().is_empty(),
        &format!("mech={}/{} gaps={}", l.len(), MECHANISMS, l.gaps().len()),
    );

    // X2 轮次对平
    let want_rounds = (MECHANISMS as u64).wrapping_mul(ROUNDS_PER_MECH as u64);
    s.add_dyn(
        "X2-轮次对平",
        l.total_rounds() == want_rounds,
        &format!("rounds={}/{}", l.total_rounds(), want_rounds),
    );

    // X3 失败归零
    s.add_dyn(
        "X3-失败归零",
        l.total_failures() == 0,
        &format!("failures={}", l.total_failures()),
    );

    // X4 判据逐条重放
    s.add_dyn(
        "X4-判据逐条重放",
        l.criteria_replayed() == B30_CRITERIA,
        &format!("criteria={}/{}", l.criteria_replayed(), B30_CRITERIA),
    );

    // X5 机制序与轮次齐
    s.add(
        "X5-机制序与轮次齐",
        l.order_strict() && l.rounds_uniform(),
        "四路机制按 pipe/shm/msgq/evtctr 递增且轮次同口径",
    );

    // X6 终值即指纹
    let root = l.settled_value();
    s.add(
        "X6-终值即指纹",
        root == Some(l.fold()),
        "终清账落下的单值等于四路探测的 FNV 指纹",
    );

    // X7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X8 终账后拒记拒清
    s.add(
        "X8-终账后拒记拒清",
        l.is_frozen()
            && l.record(0, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EFROZEN)
            && l.settle().err() == Some(IP_EFROZEN),
        "冻结后 record/settle 都落 IP_EFROZEN",
    );

    // X9 机制越界拒止（新账）
    let mut g = IpcLedger::new();
    s.add(
        "X9-机制越界拒止",
        g.record(MECHANISMS, 1, 0, true).err() == Some(IP_ENO) && g.is_empty() && g.rejects == 1,
        "四路之外的机制号一律 IP_ENO",
    );

    // X10 重记先于倒序
    let mut d = IpcLedger::new();
    let _ = d.record(0, ROUNDS_PER_MECH, 0, true);
    let _ = d.record(1, ROUNDS_PER_MECH, 0, true);
    let _ = d.record(3, ROUNDS_PER_MECH, 0, true);
    s.add(
        "X10-重记先于倒序",
        d.record(0, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EDUP)
            && d.record(2, ROUNDS_PER_MECH, 0, true).err() == Some(IP_EORDER),
        "重记报 EDUP、真倒序才报 EORDER",
    );

    // X11 形状违规拒止
    let mut sh = IpcLedger::new();
    s.add(
        "X11-形状违规拒止",
        sh.record(0, 0, 0, true).err() == Some(IP_ESHAPE)
            && sh.record(0, 8, 9, true).err() == Some(IP_ESHAPE)
            && sh.is_empty(),
        "零轮次与失败数超轮次都落 IP_ESHAPE",
    );

    // X12 清账三拒
    let mut q1 = IpcLedger::new();
    let _ = q1.record(0, ROUNDS_PER_MECH, 0, true);
    let mut q2 = IpcLedger::new();
    let _ = q2.record(0, ROUNDS_PER_MECH, 0, true);
    let _ = q2.record(1, ROUNDS_PER_MECH.wrapping_mul(2), 0, true);
    let _ = q2.record(2, ROUNDS_PER_MECH, 0, true);
    let _ = q2.record(3, ROUNDS_PER_MECH, 0, true);
    let mut q3 = IpcLedger::new();
    let _ = q3.record(0, ROUNDS_PER_MECH, 0, true);
    let _ = q3.record(1, ROUNDS_PER_MECH, 1, true);
    let _ = q3.record(2, ROUNDS_PER_MECH, 0, true);
    let _ = q3.record(3, ROUNDS_PER_MECH, 0, false);
    s.add(
        "X12-清账三拒",
        q1.settle().err() == Some(IP_ESHORT)
            && q2.settle().err() == Some(IP_EROUND)
            && q3.settle().err() == Some(IP_EFAIL),
        "未齐/轮次不齐/判据未绿各按其码",
    );

    // X13 注入四拒与计数
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b30();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-注入四拒与账目",
        all && ok2 && fresh.accepts == MECHANISMS as u64 && fresh.rejects == 0 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11150_flow() {
        let (mut l, ok) = build_b30();
        assert!(ok);
        assert_eq!(l.len(), MECHANISMS);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.all_clean());
        assert_eq!(l.total_rounds(), 1024);
        assert_eq!(l.criteria_replayed(), B30_CRITERIA);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(0, ROUNDS_PER_MECH, 0, true), Err(IP_EFROZEN));
        assert_eq!(l.settle(), Err(IP_EFROZEN));
    }

    #[test]
    fn f11150_gates() {
        let mut l = IpcLedger::new();
        assert!(l.record(0, ROUNDS_PER_MECH, 0, true).is_ok());
        assert!(l.record(1, ROUNDS_PER_MECH, 0, true).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(MECHANISMS, ROUNDS_PER_MECH, 0, true), Err(IP_ENO));
        assert_eq!(l.record(1, ROUNDS_PER_MECH, 0, true), Err(IP_EDUP));
        assert_eq!(l.record(1, 0, 0, true), Err(IP_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 3));

        let mut o = IpcLedger::new();
        let _ = o.record(0, ROUNDS_PER_MECH, 0, true);
        let _ = o.record(1, ROUNDS_PER_MECH, 0, true);
        let _ = o.record(3, ROUNDS_PER_MECH, 0, true);
        assert_eq!(o.record(2, ROUNDS_PER_MECH, 0, true), Err(IP_EORDER));
        assert_eq!((o.accepts, o.rejects), (3, 1));

        let mut sh = IpcLedger::new();
        assert_eq!(sh.record(0, 0, 0, true), Err(IP_ESHAPE));
        assert_eq!(sh.record(0, 4, 5, true), Err(IP_ESHAPE));
        assert_eq!((sh.accepts, sh.rejects), (0, 2));

        let mut q = IpcLedger::new();
        let _ = q.record(0, ROUNDS_PER_MECH, 0, true);
        let _ = q.record(1, ROUNDS_PER_MECH, 0, true);
        assert_eq!(q.settle(), Err(IP_ESHORT));
        assert_eq!((q.accepts, q.rejects), (2, 1));

        let mut r = IpcLedger::new();
        let _ = r.record(0, ROUNDS_PER_MECH, 0, true);
        let _ = r.record(1, ROUNDS_PER_MECH.wrapping_mul(2), 0, true);
        let _ = r.record(2, ROUNDS_PER_MECH, 0, true);
        let _ = r.record(3, ROUNDS_PER_MECH, 0, true);
        assert_eq!(r.settle(), Err(IP_EROUND));
        assert_eq!((r.accepts, r.rejects), (4, 1));

        let mut f = IpcLedger::new();
        let _ = f.record(0, ROUNDS_PER_MECH, 0, true);
        let _ = f.record(1, ROUNDS_PER_MECH, 1, true);
        let _ = f.record(2, ROUNDS_PER_MECH, 0, true);
        let _ = f.record(3, ROUNDS_PER_MECH, 0, false);
        assert_eq!(f.settle(), Err(IP_EFAIL));
        assert_eq!((f.accepts, f.rejects), (4, 1));

        let (mut d, ok) = build_b30();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(0, ROUNDS_PER_MECH, 0, true), Err(IP_EFROZEN));
        assert_eq!(d.settle(), Err(IP_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (4, 2));
    }

    #[test]
    fn f11150_checks_green() {
        assert!(run_unx11150_checks().all_passed());
    }
}
