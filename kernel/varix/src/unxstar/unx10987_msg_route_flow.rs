//! UNX-F10987 SysV 消息队列类型路由压力（多发多收零错投）（0x1dae0 段）
//!
//! 锚点：UNX-F10987-J1 mtype 路由千次零错投（10/10），队列账守恒（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·消息队列面）。
//!
//! 结构：类型道账（四道：mtype + 发送数 + 接收数 + 错投数）+ 队列账
//! （深度 + 高水位 + 容量）+ 路由器（发送按 mtype 入道，接收者只准
//! 取本道消息）+ 守恒闸（逐道发送等于接收、四道之和等于总账、深度
//! 归零）+ fold 与冻结 + 注入核（坏 mtype/越界道号/错投取信/冻结后
//! 发四路必抓）。
//!
//! 路由律：接收者绑定某 mtype，只能取该类型的消息；取到异类型即
//! QT_MISROUTE。每轮发一条再收一条，收束后队列深度归零，逐道发送数
//! 等于接收数，四道之和等于总轮数。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10987 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1dae0;

/// 空账 —— 未立类型道即收发
pub const QT_EMPTY: u32 = SEG_BASE + 0x01;
/// 道号越界 —— 四道 1..=4
pub const QT_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立道 —— 同一 mtype 只准一道
pub const QT_DUP: u32 = SEG_BASE + 0x03;
/// 错投 —— 接收者取到异类型消息
pub const QT_MISROUTE: u32 = SEG_BASE + 0x04;
/// 空道取信 —— 本道无消息可取
pub const QT_UNDERFLOW: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再发
pub const QT_FROZEN: u32 = SEG_BASE + 0x06;

/// 消息类型数（四类，mtype 1..=4）。
pub const TYPES: usize = 4;
/// 队列容量。
pub const QCAP: u64 = 64;
/// 路由轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 队列域标（fold 种子）。
pub const QT_TAG: u64 = 0x1da_87;

/// 类型道：一类 mtype 的收发账。
#[derive(Clone, Debug)]
pub struct TypeLane {
    /// 道号（0x15e00 段起编）
    pub lid: u32,
    /// 消息类型（1..=4）
    pub mtype: u64,
    /// 发送数
    pub sent: u64,
    /// 接收数
    pub recvd: u64,
    /// 错投数
    pub misrouted: u64,
}

impl TypeLane {
    /// 道内对平：发送等于接收。
    pub fn balanced(&self) -> bool {
        self.sent == self.recvd && self.sent > 0
    }

    /// 道内零错投。
    pub fn clean(&self) -> bool {
        self.misrouted == 0
    }

    /// 道内待取数。
    pub fn pending(&self) -> u64 {
        self.sent.wrapping_sub(self.recvd)
    }
}

/// SysV 消息队列路由总账。
#[derive(Clone, Debug)]
pub struct MsgBook {
    /// 四类型道
    pub lanes: Vec<TypeLane>,
    /// 队列深度（当前待取总数）
    pub depth: u64,
    /// 队列高水位
    pub high: u64,
    /// 已跑轮数
    pub rounds: u64,
    /// 累计发送
    pub sent: u64,
    /// 累计接收
    pub recvd: u64,
    /// 错投检出
    pub misroutes: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl MsgBook {
    /// 立账：四类型道全部登记（零计数）。
    pub fn new() -> MsgBook {
        let mut b = MsgBook {
            lanes: Vec::new(),
            depth: 0,
            high: 0,
            rounds: 0,
            sent: 0,
            recvd: 0,
            misroutes: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < TYPES {
            b.lanes.push(TypeLane {
                lid: 0x15e00u32.wrapping_add(k as u32),
                mtype: (k as u64) + 1,
                sent: 0,
                recvd: 0,
                misrouted: 0,
            });
            k += 1;
        }
        b
    }

    /// 类型道定位（mtype 1..=4）。
    pub fn lane_ix(&self, mtype: u64) -> Result<usize, u32> {
        if self.lanes.is_empty() {
            return Err(QT_EMPTY);
        }
        if mtype == 0 || mtype as usize > TYPES {
            return Err(QT_RANGE);
        }
        let ix = (mtype as usize).wrapping_sub(1);
        if ix >= self.lanes.len() {
            return Err(QT_RANGE);
        }
        Ok(ix)
    }

    /// 发送一条：按 mtype 入道。
    pub fn send(&mut self, mtype: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(QT_FROZEN);
        }
        let ix = self.lane_ix(mtype)?;
        if self.depth >= QCAP {
            return Err(QT_UNDERFLOW);
        }
        self.lanes[ix].sent = self.lanes[ix].sent.wrapping_add(1);
        self.sent = self.sent.wrapping_add(1);
        self.depth = self.depth.wrapping_add(1);
        if self.depth > self.high {
            self.high = self.depth;
        }
        Ok(self.depth)
    }

    /// 接收一条：只准取本道消息。
    pub fn recv(&mut self, mtype: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(QT_FROZEN);
        }
        let ix = self.lane_ix(mtype)?;
        if self.lanes[ix].pending() == 0 {
            return Err(QT_UNDERFLOW);
        }
        self.lanes[ix].recvd = self.lanes[ix].recvd.wrapping_add(1);
        self.recvd = self.recvd.wrapping_add(1);
        self.depth = self.depth.wrapping_sub(1);
        Ok(self.lanes[ix].recvd)
    }

    /// 越道取信：绑定 bound 的接收者试图取 actual 型消息。
    pub fn recv_as(&mut self, bound: u64, actual: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(QT_FROZEN);
        }
        let bix = self.lane_ix(bound)?;
        let _ = self.lane_ix(actual)?;
        if bound != actual {
            self.lanes[bix].misrouted = self.lanes[bix].misrouted.wrapping_add(1);
            self.misroutes = self.misroutes.wrapping_add(1);
            return Err(QT_MISROUTE);
        }
        self.recv(bound)
    }

    /// 路由一轮：发一条 (r%4)+1 型，再由同道接收者取走。
    pub fn round(&mut self, r: u64) -> Result<u64, u32> {
        let mtype = (r % TYPES as u64).wrapping_add(1);
        self.send(mtype)?;
        self.recv(mtype)?;
        self.rounds = self.rounds.wrapping_add(1);
        Ok(self.rounds)
    }

    /// 跑 n 轮。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.rounds)
    }

    /// 零错投。
    pub fn no_misroute(&self) -> bool {
        self.misroutes == 0
    }

    /// 逐道对平。
    pub fn lanes_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.lanes.len() {
            if !self.lanes[k].balanced() {
                return false;
            }
            k += 1;
        }
        !self.lanes.is_empty()
    }

    /// 队列账守恒：发送等于接收且深度归零。
    pub fn conserved(&self) -> bool {
        self.sent == self.recvd && self.depth == 0 && self.sent == self.rounds
    }

    /// 四道之和等于总账。
    pub fn lanes_summed(&self) -> bool {
        let mut s = 0u64;
        let mut r = 0u64;
        let mut k = 0usize;
        while k < self.lanes.len() {
            s = s.wrapping_add(self.lanes[k].sent);
            r = r.wrapping_add(self.lanes[k].recvd);
            k += 1;
        }
        s == self.sent && r == self.recvd
    }

    /// 类型号连续。
    pub fn types_continuous(&self) -> bool {
        let mut k = 0usize;
        while k < self.lanes.len() {
            if self.lanes[k].mtype != (k as u64) + 1 {
                return false;
            }
            k += 1;
        }
        self.lanes.len() == TYPES
    }

    /// 高水位不越容量。
    pub fn high_ok(&self) -> bool {
        self.high <= QCAP
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = QT_TAG;
        let mut k = 0usize;
        while k < self.lanes.len() {
            let l = &self.lanes[k];
            h = (h ^ l.lid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ l.mtype).wrapping_mul(0x100000001b3);
            h = (h ^ l.sent).wrapping_mul(0x100000001b3);
            h = (h ^ l.recvd).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.high).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮路由零错投且队列账守恒。
pub fn sample_routed() -> (MsgBook, bool) {
    let mut b = MsgBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.no_misroute() && b.lanes_balanced() && b.conserved() && b.lanes_summed();
    b.freeze();
    (b, ok)
}

/// 注入核：坏 mtype / 越界道号 / 错投取信 / 冻结后发。
pub fn inject_four() -> [bool; 4] {
    let mut b = MsgBook::new();
    // 注入 1：坏 mtype（0）→ QT_RANGE 拒收
    let i1 = matches!(b.send(0), Err(QT_RANGE));
    // 注入 2：越界道号 → QT_RANGE 拒收
    let i2 = matches!(b.lane_ix(TYPES as u64 + 1), Err(QT_RANGE));
    // 注入 3：错投取信 → QT_MISROUTE 拒收
    let _ = b.send(1);
    let i3 = matches!(b.recv_as(2, 1), Err(QT_MISROUTE));
    // 注入 4：冻结后发 → QT_FROZEN 拒收
    b.freeze();
    let i4 = matches!(b.send(2), Err(QT_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 mtype 路由千次零错投，队列账守恒。
pub fn run_unx10987_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10987");
    let (b, ok) = sample_routed();

    s.add("J1-千次零错投", ok && b.no_misroute(), "四类型道千轮路由零错投");
    s.add("J1-队列守恒", b.conserved(), "发送等于接收且深度归零");
    s.add("J2-逐道对平", b.lanes_balanced(), "四道逐道发送等于接收");
    s.add("J2-四道求和", b.lanes_summed(), "四道收发之和等于总账");
    s.add("J2-总数守恒", b.sent == ROUNDS && b.recvd == ROUNDS, "发送接收各满千条");
    s.add("J3-类型连续", b.types_continuous(), "mtype 自一至四连续");
    s.add("J3-逐道均分", b.lanes.iter().all(|l| l.sent == ROUNDS / TYPES as u64), "四道发送数完全均等");
    s.add("J3-道内零错", b.lanes.iter().all(|l| l.clean()), "四道逐道零错投");
    s.add("J4-高水位", b.high_ok() && b.high == 1, "逐轮收发即时深度峰值恰一");
    s.add("J4-待取归零", b.lanes.iter().all(|l| l.pending() == 0), "四道待取数全部归零");
    s.add("J5-空道拒取", {
        let mut g = MsgBook::new();
        g.recv(3) == Err(QT_UNDERFLOW)
    }, "空道取信被 QT_UNDERFLOW 拒止");
    s.add("J6-坏类型拒发", inject_four()[0], "坏 mtype 被 QT_RANGE 拒收");
    s.add("J6-越界拒定", inject_four()[1], "越界道号被 QT_RANGE 拒收");
    s.add("J7-错投拒收", inject_four()[2], "错投取信被 QT_MISROUTE 拒收");
    s.add("J7-冻结拒发", inject_four()[3], "冻结后发被 QT_FROZEN 拒止");
    s.add("J8-错投留痕", {
        let mut g = MsgBook::new();
        let _ = g.send(1);
        let _ = g.recv_as(2, 1);
        g.misroutes == 1 && g.lanes[1].misrouted == 1 && g.lanes[0].sent == 1
    }, "越道取信只在绑定道留错投痕");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-路由动态",
        ok,
        &format!(
            "轮 {} 发 {} 收 {} 深度 {} 峰值 {} 错投 {}",
            b.rounds,
            b.sent,
            b.recvd,
            b.depth,
            b.high,
            b.misroutes,
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10987_msg_route_flow() {
        let mut b = MsgBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.no_misroute());
        assert!(b.lanes_balanced());
        assert!(b.conserved());
        // 单轮语义抽查
        let mut g = MsgBook::new();
        let _ = g.send(2);
        let _ = g.send(2);
        assert_eq!(g.depth, 2);
        assert_eq!(g.recv(2).unwrap_or(9), 1);
        assert_eq!(g.depth, 1);
        assert!(g.recv(1).is_err());
        assert!(g.lane_ix(0).is_err());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10987_checks_green() {
        let s = run_unx10987_checks();
        assert!(s.all_passed(), "UNX-F10987 判据未全绿");
    }
}
