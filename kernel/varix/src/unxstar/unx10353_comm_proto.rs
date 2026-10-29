//! UNX-F10353 演习通信协议（波次内触达规则）（0x1b6a0 段）
//!
//! 锚点：UNX-F10353-J1 演习通信协议——波次内消息走广播/直达两信道，
//! 广播须面向全体（收方=0xFF），直达须指向已知在册面且不得自发自收，
//! 任一消息送达即出三面回执位，重复送达与错人代收一律拒绝，全信达毕
//! 零积压方算触达闭环。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：信账（消息=号+信道+发面+收方+三面回执）+ 信道核（广播全体/
//! 直达点名）+ 回执核（只认对的人且只收一次）+ 积压核（未达即积压）+
//! 注入核（重复送达/错人代收/自发自收/广播误点名四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b6a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b6a0;
pub const CP_CHAN: u32 = SEG_BASE + 0x01; // 信道未知
pub const CP_BC_TGT: u32 = SEG_BASE + 0x02; // 广播收方非全体
pub const CP_SELF: u32 = SEG_BASE + 0x03; // 直达自发自收
pub const CP_WHO: u32 = SEG_BASE + 0x04; // 错人代收回执
pub const CP_DUP: u32 = SEG_BASE + 0x05; // 重复送达
pub const CP_NOMSG: u32 = SEG_BASE + 0x06; // 信号未在册
pub const CP_CAP: u32 = SEG_BASE + 0x07; // 账超容
pub const CP_NOTREADY: u32 = SEG_BASE + 0x08; // 有积压即封

/// 信道。
pub const CH_BC: u8 = 1;
pub const CH_DIRECT: u8 = 2;
/// 广播收方哨兵。
pub const TO_ALL: u8 = 0xFF;
/// 三面收执位（回执下标即面号）。
pub const FACE_N: u8 = 3;

/// 信数锚（混合信道至少五封）。
pub const MSGS_NEEDED: usize = 5;
/// 账定容。
pub const CAP: usize = 14;

/// 消息：号 + 信道 + 发面 + 收方 + 三面回执。
#[derive(Clone)]
pub struct Msg {
    pub mid: u32,
    pub chan: u8,
    pub from: u8,
    pub to: u8,
    pub got: [bool; 3],
}

impl Msg {
    /// 某面是否已收。
    pub fn got_by(&self, face: u8) -> bool {
        if face < FACE_N {
            self.got[face as usize]
        } else {
            false
        }
    }

    /// 是否全员已达（直达只看点名位）。
    pub fn fully_delivered(&self) -> bool {
        if self.chan == CH_BC {
            let mut f = 0;
            while f < FACE_N {
                if f != self.from && !self.got[f as usize] {
                    return false;
                }
                f += 1;
            }
            true
        } else {
            self.to < FACE_N && self.got[self.to as usize]
        }
    }
}

/// 演习信账。
#[derive(Clone)]
pub struct CommProto {
    pub msgs: Vec<Msg>,
    pub sealed: bool,
}

impl CommProto {
    /// 空账。
    pub fn new() -> Self {
        CommProto {
            msgs: Vec::new(),
            sealed: false,
        }
    }

    /// 发信：信道核+收方核+发面核。
    pub fn send(&mut self, mid: u32, chan: u8, from: u8, to: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(CP_NOTREADY);
        }
        if chan != CH_BC && chan != CH_DIRECT {
            return Err(CP_CHAN);
        }
        if from >= FACE_N {
            return Err(CP_WHO);
        }
        if chan == CH_BC {
            if to != TO_ALL {
                return Err(CP_BC_TGT);
            }
        } else {
            if to >= FACE_N {
                return Err(CP_WHO);
            }
            if to == from {
                return Err(CP_SELF);
            }
        }
        if self.msgs.len() >= CAP {
            return Err(CP_CAP);
        }
        let mut got = [false; 3];
        if chan == CH_BC {
            // 发面自身视为即时知悉。
            got[from as usize] = true;
        }
        self.msgs.push(Msg { mid, chan, from, to, got });
        Ok(())
    }

    /// 找信。
    pub fn find(&self, mid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.msgs.len() {
            if self.msgs[i].mid == mid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 送达回执：直达只认点名收方，广播三面（发面外）逐面回执，一次为限。
    pub fn deliver(&mut self, mid: u32, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(CP_NOTREADY);
        }
        let idx = match self.find(mid) {
            Some(i) => i,
            None => return Err(CP_NOMSG),
        };
        if face >= FACE_N {
            return Err(CP_WHO);
        }
        let m = &mut self.msgs[idx];
        if m.chan == CH_DIRECT && m.to != face {
            return Err(CP_WHO);
        }
        if m.got_by(face) {
            return Err(CP_DUP);
        }
        self.msgs[idx].got[face as usize] = true;
        Ok(())
    }

    /// 积压：未达全体的信数。
    pub fn backlog(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.msgs.len() {
            if !self.msgs[i].fully_delivered() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某面收信清单。
    pub fn inbox_of(&self, face: u8) -> Vec<u32> {
        let mut out = Vec::new();
        if face >= FACE_N {
            return out;
        }
        let mut i = 0;
        while i < self.msgs.len() {
            if self.msgs[i].got_by(face) {
                out.push(self.msgs[i].mid);
            }
            i += 1;
        }
        out
    }

    /// 触达闭环：在册信数达锚且零积压。
    pub fn proto_ok(&self) -> bool {
        self.msgs.len() >= MSGS_NEEDED && self.backlog() == 0
    }

    /// 封账：有积压即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(CP_NOTREADY);
        }
        if !self.proto_ok() {
            return Err(CP_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：号/信道/回执逐信叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6a01b6;
        let mut i = 0;
        while i < self.msgs.len() {
            let m = &self.msgs[i];
            h = (h ^ m.mid).wrapping_mul(0x01000193);
            h = (h ^ (m.chan as u32)).wrapping_mul(0x01000193);
            let mut f = 0;
            while f < 3 {
                let bit = if m.got[f] { 1u32 } else { 0 };
                h = (h ^ bit).wrapping_mul(0x01000193);
                f += 1;
            }
            i += 1;
        }
        h
    }
}

/// 演习发信样例：二广播三直达。
pub fn sample_sends() -> [(u32, u8, u8, u8); 5] {
    [
        (501, CH_BC, 0, TO_ALL),
        (502, CH_DIRECT, 0, 1),
        (503, CH_DIRECT, 1, 2),
        (504, CH_BC, 2, TO_ALL),
        (505, CH_DIRECT, 2, 0),
    ]
}

/// 注入核：四路坏信必抓。
/// 0: 广播误点名；1: 直达自发自收；2: 错人代收；3: 重复送达。
pub fn inject_four() -> [bool; 4] {
    let mut p = CommProto::new();
    let _ = p.send(601, CH_BC, 0, TO_ALL);
    let _ = p.send(602, CH_DIRECT, 0, 1);
    let _ = p.send(603, CH_DIRECT, 1, 2);
    let _ = p.send(604, CH_BC, 2, TO_ALL);
    let _ = p.send(605, CH_DIRECT, 2, 0);
    let d0 = p.send(606, CH_BC, 1, 2).err() == Some(CP_BC_TGT);
    let d1 = p.send(607, CH_DIRECT, 1, 1).err() == Some(CP_SELF);
    let d2 = p.deliver(602, 2).err() == Some(CP_WHO);
    let d3 = {
        let _ = p.deliver(602, 1);
        p.deliver(602, 1).err() == Some(CP_DUP)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10353_checks。
pub fn run_unx10353_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10353");
    // J1 正路径：五封样例入账。
    let mut p = CommProto::new();
    let mut j1 = true;
    let mut k = 0;
    while k < sample_sends().len() {
        let (mid, chan, from, to) = sample_sends()[k];
        if p.send(mid, chan, from, to).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-五信入账", j1 && p.msgs.len() == 5, "混合信道五封全收");
    // J2 注入：广播误点名必抓。
    let mut p2 = CommProto::new();
    let j2 = p2.send(611, CH_BC, 0, 1).err() == Some(CP_BC_TGT);
    s.add("J2-广播点名抓", j2, "广播收方非全体即 CP_BC_TGT");
    // J3 注入：直达自发自收必抓。
    let mut p3 = CommProto::new();
    let j3 = p3.send(612, CH_DIRECT, 1, 1).err() == Some(CP_SELF);
    s.add("J3-自收抓", j3, "直达自发自收即 CP_SELF");
    // J4 注入：错人代收必抓。
    let mut p4 = CommProto::new();
    let _ = p4.send(613, CH_DIRECT, 0, 2);
    let j4 = p4.deliver(613, 1).err() == Some(CP_WHO);
    s.add("J4-代收抓", j4, "非点名面回执即 CP_WHO");
    // J5 注入：重复送达必抓。
    let mut p5 = CommProto::new();
    let _ = p5.send(614, CH_DIRECT, 0, 2);
    let _ = p5.deliver(614, 2);
    let j5 = p5.deliver(614, 2).err() == Some(CP_DUP);
    s.add("J5-重达抓", j5, "重复回执即 CP_DUP");
    // J6 边界：未在册信回执拒。
    let mut p6 = CommProto::new();
    let j6 = p6.deliver(999, 0).err() == Some(CP_NOMSG);
    s.add("J6-无信拒", j6, "未在册信号即 CP_NOMSG");
    // J7 边界：坏信道拒。
    let mut p7 = CommProto::new();
    let j7 = p7.send(615, 9, 0, TO_ALL).err() == Some(CP_CHAN);
    s.add("J7-坏道拒", j7, "信道未知即 CP_CHAN");
    // J8 正路径：广播三面回执齐即达。
    let mut p8 = CommProto::new();
    let _ = p8.send(616, CH_BC, 0, TO_ALL);
    let _ = p8.deliver(616, 1);
    let _ = p8.deliver(616, 2);
    let j8 = p8.msgs[0].fully_delivered() && p8.inbox_of(1).len() == 1;
    s.add("J8-广播达", j8, "广播发面外两回执齐即达");
    // J9 正路径：直达点名一回执即达。
    let mut p9 = CommProto::new();
    let _ = p9.send(617, CH_DIRECT, 1, 0);
    let _ = p9.deliver(617, 0);
    let j9 = p9.msgs[0].fully_delivered() && p9.inbox_of(0).len() == 1;
    s.add("J9-直达达", j9, "点名回执到位即达");
    // J10 边界：有积压封账拒。
    let mut p10 = CommProto::new();
    let _ = p10.send(618, CH_DIRECT, 0, 1);
    let j10 = p10.seal().err() == Some(CP_NOTREADY) && p10.backlog() == 1;
    s.add("J10-压封拒", j10, "积压未清封账即 CP_NOTREADY");
    // J11 正路径：五信全达零积压封账成。
    let mut p11 = CommProto::new();
    let mut k11 = 0;
    while k11 < sample_sends().len() {
        let (mid, chan, from, to) = sample_sends()[k11];
        let _ = p11.send(mid, chan, from, to);
        if chan == CH_DIRECT {
            let _ = p11.deliver(mid, to);
        } else {
            let mut f = 0;
            while f < FACE_N {
                if f != from {
                    let _ = p11.deliver(mid, f);
                }
                f += 1;
            }
        }
        k11 += 1;
    }
    let j11 = p11.proto_ok() && p11.seal().is_ok();
    s.add("J11-闭环封", j11, "全达零积压封账通过");
    // J12 注入：封账后再发必抓。
    let mut p12 = p11.clone();
    let j12 = p12.send(619, CH_BC, 0, TO_ALL).err() == Some(CP_NOTREADY);
    s.add("J12-封后发抓", j12, "封账后再发即 CP_NOTREADY");
    // J13 动态：回执总位与积压数。
    let mut receipts = 0;
    let mut i = 0;
    while i < p11.msgs.len() {
        let mut f = 0;
        while f < 3 {
            if p11.msgs[i].got[f] {
                receipts += 1;
            }
            f += 1;
        }
        i += 1;
    }
    let j13 = p11.backlog() == 0 && receipts == 9;
    s.add_dyn(
        "J13-回执对平",
        j13,
        &format!(
            "五信回执 {} 位零积压，指纹 {:08x}",
            receipts,
            p11.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "误点/自收/代收/重达四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut p = CommProto::new();
        let mut k = 0;
        while k < sample_sends().len() {
            let (mid, chan, from, to) = sample_sends()[k];
            assert!(p.send(mid, chan, from, to).is_ok());
            if chan == CH_DIRECT {
                assert!(p.deliver(mid, to).is_ok());
            } else {
                let mut f = 0;
                while f < FACE_N {
                    if f != from {
                        assert!(p.deliver(mid, f).is_ok());
                    }
                    f += 1;
                }
            }
            k += 1;
        }
        assert!(p.proto_ok());
        assert!(p.seal().is_ok());
        assert!(p.send(700, CH_BC, 0, TO_ALL).err() == Some(CP_NOTREADY));
        assert!(p.fold() != 0);
        assert!(p.inbox_of(0).contains(&505));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10353_checks();
        assert!(s.all_passed());
    }
}
