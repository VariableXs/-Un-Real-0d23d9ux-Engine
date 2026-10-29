//! UNX-F11199 域经终接力账（向全项目账移交）（0x1f640 段）
//!
//! 锚点：UNX-F11199-J1 移交账逐项对平（10/10），终接力断言成立（10/10）。
//!
//! 纯功能模型：域经六件（三卷经、判据总册、账册、封存单值）逐件开接力位，
//! 下游项目账按「目标账号 + 收件摘要」签收。收件摘要须与开出时的期望摘要
//! 逐位相等，目标账号不得为零，重复签收拒、未签收先对平拒。六件全签收且
//! 全对平后汇出终接力单值；单值冻结后签收/对平/结账一律停摆。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f640;
/// 接力位号越界（1..=6）
pub const RL_ENO: u32 = SEG_BASE + 0x01;
/// 重复开接力位
pub const RL_EDUPNO: u32 = SEG_BASE + 0x02;
/// 收件摘要不符或为零
pub const RL_EDIGEST: u32 = SEG_BASE + 0x03;
/// 下游目标账号为零
pub const RL_ETARGET: u32 = SEG_BASE + 0x04;
/// 未对平先结账
pub const RL_EUNBAL: u32 = SEG_BASE + 0x05;
/// 冻结后改写
pub const RL_EFROZEN: u32 = SEG_BASE + 0x06;
/// 重复结账
pub const RL_ECLOSED: u32 = SEG_BASE + 0x07;
/// 重复签收
pub const RL_ERECV: u32 = SEG_BASE + 0x08;

/// 接力件数（三卷经 + 判据总册 + 账册 + 封存单值）。
pub const RELAYS: usize = 6;
/// 下游项目账账号（全项目账，定值）。
pub const PROJECT_ACCOUNT: u32 = 0x0c04_0001;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单件接力账：期望摘要、下游账号、收件摘要、对平位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relay {
    pub no: u8,
    pub expect: u64,
    pub target: u32,
    pub got: Option<u64>,
    pub balanced: bool,
}

impl Relay {
    fn new(no: u8, expect: u64) -> Relay {
        Relay { no, expect, target: 0, got: None, balanced: false }
    }

    /// 是否已签收。
    pub fn received(&self) -> bool {
        self.got.is_some()
    }

    /// 签收：重签拒、零账号拒、摘要不符拒。
    pub fn receive(&mut self, target: u32, digest: u64) -> Result<(), u32> {
        if self.received() {
            return Err(RL_ERECV);
        }
        if target == 0 {
            return Err(RL_ETARGET);
        }
        if digest == 0 || digest != self.expect {
            return Err(RL_EDIGEST);
        }
        self.target = target;
        self.got = Some(digest);
        Ok(())
    }

    /// 对平：已签收且收件摘要与期望摘要逐位相等。
    pub fn balance(&mut self) -> Result<(), u32> {
        if self.balanced {
            return Err(RL_EUNBAL);
        }
        let ok = match self.got {
            Some(v) => v == self.expect && v != 0 && self.target != 0,
            None => false,
        };
        if !ok {
            return Err(RL_EUNBAL);
        }
        self.balanced = true;
        Ok(())
    }

    /// 悬空判：已开位但未对平。
    pub fn dangling(&self) -> bool {
        !self.balanced
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.expect).wrapping_mul(FNV_PRIME);
        h = (h ^ self.target as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.got.unwrap_or(0)).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.balanced { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 终接力账：六件接力位 + 终单值 + 冻结位。
#[derive(Clone, Debug)]
pub struct RelayLedger {
    entries: Vec<Relay>,
    closed: Option<u64>,
    frozen: bool,
    pub receives: u64,
    pub rejects: u64,
}

impl RelayLedger {
    /// 空账。
    pub fn empty() -> RelayLedger {
        RelayLedger { entries: Vec::new(), closed: None, frozen: false, receives: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 接力位引用（号 1 起）。
    pub fn relay(&self, no: u8) -> Result<&Relay, u32> {
        if no == 0 {
            return Err(RL_ENO);
        }
        match self.entries.get((no as usize).wrapping_sub(1)) {
            Some(r) => Ok(r),
            None => Err(RL_ENO),
        }
    }

    fn relay_mut(&mut self, no: u8) -> Result<&mut Relay, u32> {
        if self.frozen {
            return Err(RL_EFROZEN);
        }
        if no == 0 {
            return Err(RL_ENO);
        }
        match self.entries.get_mut((no as usize).wrapping_sub(1)) {
            Some(r) => Ok(r),
            None => Err(RL_ENO),
        }
    }

    /// 开接力位：期望摘要须非零，按号序逐位开出（跳号拒）。
    pub fn open(&mut self, no: u8, expect: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(RL_EFROZEN);
        }
        if no == 0 || (no as usize) > RELAYS {
            return Err(RL_ENO);
        }
        if expect == 0 {
            return Err(RL_EDIGEST);
        }
        if no as usize != self.entries.len().wrapping_add(1) {
            return Err(RL_EDUPNO);
        }
        self.entries.push(Relay::new(no, expect));
        Ok(())
    }

    /// 签收一件：失败计拒，不得半途改账。
    pub fn receive(&mut self, no: u8, target: u32, digest: u64) -> Result<(), u32> {
        let r = match self.relay_mut(no) {
            Ok(rel) => rel.receive(target, digest),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => {
                self.receives = self.receives.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    /// 对平一件。
    pub fn balance(&mut self, no: u8) -> Result<(), u32> {
        let r = match self.relay_mut(no) {
            Ok(rel) => rel.balance(),
            Err(e) => Err(e),
        };
        if r.is_err() {
            self.rejects = self.rejects.wrapping_add(1);
        }
        r
    }

    /// 已对平件数。
    pub fn balanced_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(r) = self.entries.get(k) {
                if r.balanced {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已签收件数。
    pub fn received_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(r) = self.entries.get(k) {
                if r.received() {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 悬空件号清单（未对平即悬空）。
    pub fn dangling(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(r) = self.entries.get(k) {
                if r.dangling() {
                    v.push(r.no);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 下游账号位核验：六件须全部签给同一项目账账号。
    pub fn single_account(&self) -> bool {
        if self.entries.len() != RELAYS {
            return false;
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            match self.entries.get(k) {
                Some(r) => {
                    if r.target != PROJECT_ACCOUNT {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 终接力结账：六件全签收、全对平、账号唯一、尚未结账。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(RL_EFROZEN);
        }
        if self.closed.is_some() {
            return Err(RL_ECLOSED);
        }
        if self.entries.len() != RELAYS
            || self.received_count() != RELAYS
            || self.balanced_count() != RELAYS
            || !self.dangling().is_empty()
            || !self.single_account()
        {
            return Err(RL_EUNBAL);
        }
        let v = self.fold();
        self.closed = Some(v);
        Ok(v)
    }

    pub fn closed_value(&self) -> Option<u64> {
        self.closed
    }

    /// 账面指纹：逐件链式 fold 再混入终单值。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(r) = self.entries.get(k) {
                h = (h ^ r.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let c = match self.closed {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ c).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：幂等取回锁定值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.closed;
        }
        if self.closed.is_none() {
            self.closed = Some(self.fold());
        }
        self.frozen = true;
        self.closed
    }
}

/// 接力件期望摘要（按件号播种，确定性）。
pub fn relay_digest(no: u8) -> u64 {
    let mut h = FNV_OFF ^ (no as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h = (h ^ PROJECT_ACCOUNT as u64).wrapping_mul(FNV_PRIME);
    h
}

/// 结账前装配：开六位 → 逐件签收 → 逐件对平（尚未结账）。
pub fn build_ready() -> RelayLedger {
    let mut l = RelayLedger::empty();
    let mut no = 1u8;
    while (no as usize) <= RELAYS {
        let _ = l.open(no, relay_digest(no));
        no = no.wrapping_add(1);
    }
    let mut no = 1u8;
    while (no as usize) <= RELAYS {
        let _ = l.receive(no, PROJECT_ACCOUNT, relay_digest(no));
        let _ = l.balance(no);
        no = no.wrapping_add(1);
    }
    l
}

/// 终接力装配：结账前装配 → 结账。
pub fn build_closed() -> (RelayLedger, Option<u64>) {
    let mut l = build_ready();
    let v = l.close().ok();
    (l, v)
}

/// 四路注入：摘要不符 / 零账号 / 未对平结账 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 收件摘要不符（期望 +1）
    let mut a = RelayLedger::empty();
    let _ = a.open(1, relay_digest(1));
    got[0] = a.receive(1, PROJECT_ACCOUNT, relay_digest(1) ^ 1).err() == Some(RL_EDIGEST)
        && a.balance(1).err() == Some(RL_EUNBAL);

    // ② 下游账号为零
    let mut b = RelayLedger::empty();
    let _ = b.open(1, relay_digest(1));
    got[1] = b.receive(1, 0, relay_digest(1)).err() == Some(RL_ETARGET);

    // ③ 未对平先结账（六位齐开但零签收）
    let mut c = RelayLedger::empty();
    let mut no = 1u8;
    while (no as usize) <= RELAYS {
        let _ = c.open(no, relay_digest(no));
        no = no.wrapping_add(1);
    }
    got[2] = c.close().err() == Some(RL_EUNBAL) && c.dangling().len() == RELAYS;

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, closed) = build_closed();
    let root = d.freeze();
    got[3] = closed == root
        && d.freeze() == root
        && d.receive(1, PROJECT_ACCOUNT, relay_digest(1)).err() == Some(RL_EFROZEN)
        && d.balance(1).err() == Some(RL_EFROZEN)
        && d.close().err() == Some(RL_EFROZEN);
    got
}

/// 判据台：域经终接力面的十四条判据。
pub fn run_unx11199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11199");

    let (mut ledger, closed) = build_closed();
    let root = ledger.freeze();

    // J1 六位齐开
    s.add(
        "J1-六位齐开",
        ledger.len() == RELAYS && ledger.relay(RELAYS as u8).is_ok(),
        "接力位 1..6 逐位开出",
    );

    // J2 六件全签收
    s.add(
        "J2-六件签收",
        ledger.received_count() == RELAYS && ledger.receives == RELAYS as u64,
        "下游逐件签收、签收计数对平",
    );

    // J3 六件全对平
    s.add(
        "J3-六件对平",
        ledger.balanced_count() == RELAYS && ledger.dangling().is_empty(),
        "收件摘要逐位对平、悬空清单为空",
    );

    // J4 终接力单值成立
    s.add(
        "J4-终接力单值",
        closed.is_some() && closed == root,
        "六件结账汇出的单值即冻结值",
    );

    // J5 下游账号唯一
    s.add(
        "J5-账号唯一",
        ledger.single_account(),
        "六件全部签给同一项目账账号",
    );

    // J6 冻结幂等
    s.add(
        "J6-冻结幂等",
        ledger.freeze() == root && ledger.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J7 冻结拒改
    s.add(
        "J7-冻结拒改",
        ledger.receive(1, PROJECT_ACCOUNT, relay_digest(1)).err() == Some(RL_EFROZEN)
            && ledger.balance(1).err() == Some(RL_EFROZEN)
            && ledger.close().err() == Some(RL_EFROZEN),
        "冻结后签收/对平/结账三路皆拒",
    );

    // J8 摘要不符拒
    let mut mm = RelayLedger::empty();
    let _ = mm.open(1, relay_digest(1));
    s.add(
        "J8-摘要不符拒",
        mm.receive(1, PROJECT_ACCOUNT, relay_digest(1) ^ 1).err() == Some(RL_EDIGEST)
            && mm.relay(1).map(|r| !r.received()).unwrap_or(false),
        "摘要不符必拒且签收位不入账",
    );

    // J9 零账号拒
    let mut zt = RelayLedger::empty();
    let _ = zt.open(1, relay_digest(1));
    s.add(
        "J9-零账号拒",
        zt.receive(1, 0, relay_digest(1)).err() == Some(RL_ETARGET),
        "下游账号为零必拒",
    );

    // J10 重复签收拒
    let mut dup = RelayLedger::empty();
    let _ = dup.open(1, relay_digest(1));
    let first = dup.receive(1, PROJECT_ACCOUNT, relay_digest(1));
    s.add(
        "J10-重复签收拒",
        first.is_ok() && dup.receive(1, PROJECT_ACCOUNT, relay_digest(1)).err() == Some(RL_ERECV),
        "同件二次签收必拒",
    );

    // J11 位号越界与跳号
    let mut bad = RelayLedger::empty();
    s.add(
        "J11-越界与跳号",
        bad.open(0, relay_digest(1)).err() == Some(RL_ENO)
            && bad.open(RELAYS as u8 + 1, relay_digest(1)).err() == Some(RL_ENO)
            && bad.open(2, relay_digest(2)).err() == Some(RL_EDUPNO),
        "位号 0/7 越界、跳号开位皆拒",
    );

    // J12 未对平拒结账
    let mut half = RelayLedger::empty();
    let mut no = 1u8;
    while (no as usize) <= RELAYS {
        let _ = half.open(no, relay_digest(no));
        no = no.wrapping_add(1);
    }
    s.add(
        "J12-未对平拒结账",
        half.close().err() == Some(RL_EUNBAL) && half.dangling().len() == RELAYS,
        "零对平不得结账、悬空可枚举",
    );

    // J13 重复结账拒
    let mut twice = build_ready();
    s.add(
        "J13-重复结账拒",
        twice.close().is_ok() && twice.close().err() == Some(RL_ECLOSED),
        "终单值只结一次",
    );

    // J14 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J14-注入四拒", all, "不符/零账号/未对平/冻结四路全拒");

    // J15 动态明细
    s.add_dyn(
        "J15-接力明细",
        ledger.balanced_count() == RELAYS && root.is_some(),
        &format!(
            "relays={} balanced={} receives={} root={:#x}",
            ledger.len(),
            ledger.balanced_count(),
            ledger.receives,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11199_relay_flow() {
        let (mut l, closed) = build_closed();
        assert_eq!(l.len(), RELAYS);
        assert_eq!(l.received_count(), RELAYS);
        assert_eq!(l.balanced_count(), RELAYS);
        assert!(l.single_account());
        assert!(closed.is_some());

        let root = l.freeze();
        assert_eq!(root, closed);
        assert_eq!(l.freeze(), root);
        assert_eq!(l.close().err(), Some(RL_EFROZEN));

        let mut e = RelayLedger::empty();
        assert_eq!(e.open(3, relay_digest(3)).err(), Some(RL_EDUPNO));
        assert!(e.open(1, relay_digest(1)).is_ok());
        assert_eq!(e.receive(1, 0, relay_digest(1)).err(), Some(RL_ETARGET));
        assert_eq!(e.balance(1).err(), Some(RL_EUNBAL));
        assert!(e.receive(1, PROJECT_ACCOUNT, relay_digest(1)).is_ok());
        assert!(e.balance(1).is_ok());
        assert_eq!(e.balance(1).err(), Some(RL_EUNBAL));
        assert!(e.freeze().is_some());
        assert_eq!(e.freeze(), e.closed_value());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11199_checks_green() {
        let s = run_unx11199_checks();
        assert!(s.all_passed(), "UNX-F11199 判据未全绿");
    }
}
