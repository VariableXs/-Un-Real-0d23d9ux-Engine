//! UNX-F11032 socket 数据面挂点联签（send/recv 挂号对账）（0x1e400 段）
//!
//! 锚点：UNX-F11032-J1 收发挂点千次账对平（10/10），字节守恒零丢失（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·socket 数据面）。
//!
//! 结构：连接对账（端名 + 发字节数 + 收字节数 + 在途缓冲）+ send/recv 挂
//! 点（在途缓冲上限必拒、空收必拒）+ 字节守恒器（发 = 收 + 在途）+ 千次
//! 对账器（确定性轮转收发清空）+ fold 与冻结 + 注入核（空收/超缓冲/未注
//! 册端/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e400 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e400;
pub const DP_EMPTY: u32 = SEG_BASE + 0x01; // 空收
pub const DP_OVERBUF: u32 = SEG_BASE + 0x02; // 在途超缓冲
pub const DP_NOEND: u32 = SEG_BASE + 0x03; // 未注册端
pub const DP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const DP_ZLEN: u32 = SEG_BASE + 0x05; // 零长发送
pub const DP_DUP: u32 = SEG_BASE + 0x06; // 端重号

/// 在途缓冲字节上限。
pub const BUF_CAP: u32 = 64;
/// 千次收发轮数。
pub const DP_ROUNDS: u32 = 1000;
/// 数据面账域标（fold 种子）。
pub const DP_TAG: u64 = 0x1e40_32;
/// 端对平面规模（10/10）。
pub const ENDS_NEEDED: usize = 10;

/// 单连接端：名 + 发收字节 + 在途缓冲。
#[derive(Clone, Debug)]
pub struct DataEnd {
    pub name: String,
    pub sent: u32,
    pub recvd: u32,
    pub inflight: Vec<u32>,
}

impl DataEnd {
    /// 在途字节数。
    pub fn inflight_bytes(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.inflight.len() {
            n = n.wrapping_add(self.inflight[k]);
            k += 1;
        }
        n
    }

    /// 字节守恒：发 = 收 + 在途。
    pub fn conserved(&self) -> bool {
        self.sent == self.recvd.wrapping_add(self.inflight_bytes())
    }

    pub fn render(&self) -> String {
        format!("dp|{}|s{}|r{}|f{}", self.name, self.sent, self.recvd, self.inflight.len())
    }
}

/// socket 数据面挂号账：注册端 → send/recv 挂点 → 字节守恒封账。
pub struct DpBook {
    pub ends: Vec<DataEnd>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DpBook {
    pub fn new() -> DpBook {
        DpBook { ends: Vec::new(), frozen: false, root: None }
    }

    /// 注册端：重号必拒。
    pub fn enroll(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(DP_FROZEN);
        }
        if self.ends.iter().any(|x| x.name == name) {
            return Err(DP_DUP);
        }
        self.ends.push(DataEnd {
            name: name.to_string(),
            sent: 0,
            recvd: 0,
            inflight: Vec::new(),
        });
        Ok(())
    }

    /// send 挂点：零长/超缓冲必拒；字节入在途。
    pub fn send(&mut self, name: &str, len: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(DP_FROZEN);
        }
        if len == 0 {
            return Err(DP_ZLEN);
        }
        let mut k = 0usize;
        while k < self.ends.len() {
            if self.ends[k].name == name {
                let cur = self.ends[k].inflight_bytes();
                if cur.wrapping_add(len) > BUF_CAP {
                    return Err(DP_OVERBUF);
                }
                self.ends[k].inflight.push(len);
                self.ends[k].sent = self.ends[k].sent.wrapping_add(len);
                return Ok(self.ends[k].sent);
            }
            k += 1;
        }
        Err(DP_NOEND)
    }

    /// recv 挂点：空收必拒；按段出在途。
    pub fn recv(&mut self, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(DP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.ends.len() {
            if self.ends[k].name == name {
                if self.ends[k].inflight.is_empty() {
                    return Err(DP_EMPTY);
                }
                let len = self.ends[k].inflight.remove(0);
                self.ends[k].recvd = self.ends[k].recvd.wrapping_add(len);
                return Ok(len);
            }
            k += 1;
        }
        Err(DP_NOEND)
    }

    /// 按名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.ends.len() {
            if self.ends[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 全账守恒：逐端守恒且端数齐。
    pub fn conserved_all(&self) -> bool {
        let mut ok = self.ends.len() == ENDS_NEEDED;
        let mut k = 0usize;
        while k < self.ends.len() {
            if !self.ends[k].conserved() {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = DP_TAG;
        let mut k = 0usize;
        while k < self.ends.len() {
            let x = &self.ends[k];
            h = (h ^ x.sent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.recvd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.inflight.len() as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.inflight_bytes() as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 端名确定性样本。
pub fn dp_name(no: usize) -> String {
    format!("/ch{}", no)
}

/// 长度确定性样本。
pub fn dp_len(no: u32) -> u32 {
    (no % 11 + 2) as u32
}

/// 千次对账全链：十端注册；千轮在轮转端 send+recv 成对（在途即清）。
pub fn build_dpbook() -> (DpBook, bool) {
    let mut b = DpBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < ENDS_NEEDED {
        if b.enroll(&dp_name(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < DP_ROUNDS {
        let k = (r % ENDS_NEEDED as u32) as usize;
        let nm = dp_name(k);
        let l = dp_len(r);
        if b.send(&nm, l).is_err() {
            ok = false;
        }
        if b.recv(&nm).is_err() {
            ok = false;
        }
        r += 1;
    }
    let b_ok = ok && b.conserved_all();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空收必拒
    let mut a = DpBook::new();
    let _ = a.enroll("/e");
    got[0] = a.recv("/e").err() == Some(DP_EMPTY);
    // 2) 在途超缓冲必拒
    let mut b = DpBook::new();
    let _ = b.enroll("/f");
    got[1] = b.send("/f", BUF_CAP).is_ok() && b.send("/f", 1).err() == Some(DP_OVERBUF);
    // 3) 未注册端与零长必拒
    let mut c = DpBook::new();
    got[2] = c.send("/none", 8).err() == Some(DP_NOEND)
        && {
            let _ = c.enroll("/z");
            c.send("/z", 0).err() == Some(DP_ZLEN)
        };
    // 4) 冻结后写账必抓
    let (mut d, _) = build_dpbook();
    d.freeze();
    got[3] = d.send(&dp_name(0), 4).err() == Some(DP_FROZEN)
        && d.recv(&dp_name(0)).err() == Some(DP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11032_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11032");

    // J1 收发挂点千次账对平：十端千轮成对收发
    let (mut book, green) = build_dpbook();
    s.add("J1-千次对平", green && book.ends.len() == 10, "十端千轮成对收发对平");

    // J2 字节守恒零丢失：逐端发 = 收 + 在途且在途清空
    let j2 = {
        let mut ok = book.conserved_all();
        let mut k = 0usize;
        while k < book.ends.len() {
            if !book.ends[k].inflight.is_empty() {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J2-字节守恒", j2, "逐端守恒且在途清零");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.send(&dp_name(0), 4).err() == Some(DP_FROZEN)
        && book.recv(&dp_name(0)).err() == Some(DP_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后收发均必拒");

    // J5 注入检出：空收 + 超缓冲
    let inj = inject_four();
    s.add("J5-空收超缓抓", inj[0] && inj[1], "空收与在途超缓冲两路必抓");

    // J6 注入检出：未注册零长 + 冻结后写
    s.add("J6-未注冻结检出", inj[2] && inj[3], "未注册端零长与冻结后写必抓");

    // J7 在途语义：只发不收则守恒靠在途闭合
    let j7 = {
        let mut c = DpBook::new();
        let _ = c.enroll("/i");
        let _ = c.send("/i", 20);
        let _ = c.send("/i", 30);
        let x = c.find("/i");
        match x {
            Some(k) => c.ends[k].sent == 50 && c.ends[k].inflight_bytes() == 50 && c.ends[k].conserved(),
            None => false,
        }
    };
    s.add("J7-在途语义", j7, "未收字节落在途守恒闭合");

    // J8 分段出队：在途按入队序出
    let j8 = {
        let mut c = DpBook::new();
        let _ = c.enroll("/o");
        let _ = c.send("/o", 12);
        let _ = c.send("/o", 24);
        c.recv("/o") == Ok(12) && c.recv("/o") == Ok(24)
    };
    s.add("J8-分段出队", j8, "在途按入队序出队");

    // J9 缓冲边界：恰满 BUF_CAP 可收一笔
    let j9 = {
        let mut c = DpBook::new();
        let _ = c.enroll("/b");
        let g = c.send("/b", BUF_CAP);
        g.is_ok() && c.send("/b", 1).is_err()
    };
    s.add("J9-缓冲边界", j9, "恰满可收超一必拒");

    // J10 计数均衡：各端收发字节均等
    let j10 = {
        let mut even = true;
        let mut k = 0usize;
        while k < book.ends.len() {
            if book.ends[k].sent != book.ends[k].recvd {
                even = false;
            }
            k += 1;
        }
        even
    };
    s.add("J10-计数均衡", j10, "逐端收发字节均等");

    // J11 端重号拒
    let j11 = {
        let mut c = DpBook::new();
        let _ = c.enroll("/d");
        c.enroll("/d").err() == Some(DP_DUP)
    };
    s.add("J11-端重号拒", j11, "端重注册必拒");

    // J12 渲染面：端渲染含名与计数
    let j12 = {
        let x = DataEnd {
            name: "/r".to_string(),
            sent: 30,
            recvd: 20,
            inflight: Vec::new(),
        };
        let r = x.render();
        r.contains("/r") && r.contains("s30") && r.contains("r20")
    };
    s.add("J12-渲染明细", j12, "端渲染含名收发计数");

    // J13 样本确定性：名与长度同入同出
    let n5a = dp_name(5);
    let n5b = dp_name(5);
    let l7a = dp_len(7);
    let l7b = dp_len(7);
    s.add("J13-样本确定性", n5a == n5b && l7a == l7b, "名与长度样本同入同出");

    // J14 丢失注入：构造发收不符端断言为假
    let j14 = {
        let x = DataEnd {
            name: "/l".to_string(),
            sent: 100,
            recvd: 90,
            inflight: Vec::new(),
        };
        !x.conserved()
    };
    s.add("J14-丢失注入", j14, "发收不符守恒断言为假");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.conserved_all(),
        &format!("root={:#x} ends=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11032_dp_flow() {
        let (mut b, built) = build_dpbook();
        assert!(built);
        assert!(b.recv(&dp_name(0)).err() == Some(DP_EMPTY));
        b.freeze();
        assert!(b.send(&dp_name(1), 4).err() == Some(DP_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11032_checks_green() {
        let s = run_unx11032_checks();
        assert!(s.all_passed(), "UNX-F11032 判据未全绿");
    }
}
