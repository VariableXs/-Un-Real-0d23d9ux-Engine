//! UNX-F11031 unix socket 挂号联签（bind/listen/accept 挂点账）（0x1e3e0 段）
//!
//! 锚点：UNX-F11031-J1 三段挂点千次对平（10/10），连接注册账守恒（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·unix socket）。
//!
//! 结构：监听端账（名 + 绑定位 + 监听 backlog + 挂起连接队列）+ bind/
//! listen/connect/accept 四挂点（重绑/未绑/队满必拒）+ 连接守恒器（accept
//! 出队即配对、断开回笼）+ 千次对账器 + fold 与冻结 + 注入核（未绑收连/
//! 重名绑/队满连接/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e3e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e3e0;
pub const US_NOBIND: u32 = SEG_BASE + 0x01; // 未绑收连
pub const US_DUPBIND: u32 = SEG_BASE + 0x02; // 重名绑
pub const US_BACKLOG: u32 = SEG_BASE + 0x03; // 队满连接
pub const US_NOCONN: u32 = SEG_BASE + 0x04; // 空队 accept
pub const US_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const US_NOLEAF: u32 = SEG_BASE + 0x06; // 未监听 accept

/// 单端挂起队列深。
pub const BACKLOG_N: usize = 4;
/// 千次连接轮数。
pub const US_ROUNDS: u32 = 1000;
/// unix sock 账域标（fold 种子）。
pub const USOCK_TAG: u64 = 0x1e3e_31;
/// 监听端对平面规模（10/10）。
pub const LSNS_NEEDED: usize = 10;

/// 单监听端：名 + 绑定位 + 监听位 + 挂起队列 + 历史计数。
#[derive(Clone, Debug)]
pub struct LsnEnd {
    pub name: String,
    pub bound: bool,
    pub listening: bool,
    pub pending: Vec<u32>,
    pub connects: u32,
    pub accepts: u32,
}

impl LsnEnd {
    pub fn render(&self) -> String {
        format!(
            "lsn|{}|b{}|l{}|p{}|c{}|a{}",
            self.name,
            self.bound as u32,
            self.listening as u32,
            self.pending.len(),
            self.connects,
            self.accepts
        )
    }
}

/// unix socket 挂号账：bind → listen → connect/accept 循环封账。
pub struct UsockBook {
    pub lsns: Vec<LsnEnd>,
    pub next_conn: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl UsockBook {
    pub fn new() -> UsockBook {
        UsockBook { lsns: Vec::new(), next_conn: 1, frozen: false, root: None }
    }

    /// bind 挂点：重名绑必拒。
    pub fn bind(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(US_FROZEN);
        }
        if self.lsns.iter().any(|x| x.name == name && x.bound) {
            return Err(US_DUPBIND);
        }
        match self.find(name) {
            Some(k) => {
                self.lsns[k].bound = true;
                Ok(())
            }
            None => {
                self.lsns.push(LsnEnd {
                    name: name.to_string(),
                    bound: true,
                    listening: false,
                    pending: Vec::new(),
                    connects: 0,
                    accepts: 0,
                });
                Ok(())
            }
        }
    }

    /// listen 挂点：未绑必拒，置监听位。
    pub fn listen(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(US_FROZEN);
        }
        let mut k = 0usize;
        while k < self.lsns.len() {
            if self.lsns[k].name == name {
                if !self.lsns[k].bound {
                    return Err(US_NOBIND);
                }
                self.lsns[k].listening = true;
                return Ok(());
            }
            k += 1;
        }
        Err(US_NOBIND)
    }

    /// connect 挂点：未绑未听必拒；队满必拒；连接号入队。
    pub fn connect(&mut self, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(US_FROZEN);
        }
        let mut k = 0usize;
        while k < self.lsns.len() {
            if self.lsns[k].name == name {
                if !self.lsns[k].bound || !self.lsns[k].listening {
                    return Err(US_NOBIND);
                }
                if self.lsns[k].pending.len() >= BACKLOG_N {
                    return Err(US_BACKLOG);
                }
                let c = self.next_conn;
                self.next_conn = self.next_fd_step();
                self.lsns[k].pending.push(c);
                self.lsns[k].connects = self.lsns[k].connects.wrapping_add(1);
                return Ok(c);
            }
            k += 1;
        }
        Err(US_NOBIND)
    }

    /// 连接号步进。
    fn next_fd_step(&mut self) -> u32 {
        self.next_conn.wrapping_add(1)
    }

    /// accept 挂点：未听必拒；空队必拒；出队配对。
    pub fn accept(&mut self, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(US_FROZEN);
        }
        let mut k = 0usize;
        while k < self.lsns.len() {
            if self.lsns[k].name == name {
                if !self.lsns[k].listening {
                    return Err(US_NOLEAF);
                }
                if self.lsns[k].pending.is_empty() {
                    return Err(US_NOCONN);
                }
                let c = self.lsns[k].pending.remove(0);
                self.lsns[k].accepts = self.lsns[k].accepts.wrapping_add(1);
                return Ok(c);
            }
            k += 1;
        }
        Err(US_NOBIND)
    }

    /// 按名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.lsns.len() {
            if self.lsns[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 连接守恒：逐端 connect = accept 且挂起队列清空。
    pub fn conserved(&self) -> bool {
        let mut ok = self.lsns.len() == LSNS_NEEDED;
        let mut k = 0usize;
        while k < self.lsns.len() {
            if self.lsns[k].connects != self.lsns[k].accepts
                || !self.lsns[k].pending.is_empty()
            {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = USOCK_TAG;
        let mut k = 0usize;
        while k < self.lsns.len() {
            let x = &self.lsns[k];
            h = (h ^ x.bound as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.listening as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.connects as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.accepts as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.pending.len() as u64).wrapping_mul(0x100000001b3);
            let bytes = x.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
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

/// 监听名确定性样本。
pub fn us_name(no: usize) -> String {
    format!("/srv{}", no)
}

/// 千次对账全链：十监听端 bind+listen；千轮在轮转端 connect+accept 成对。
pub fn build_usockbook() -> (UsockBook, bool) {
    let mut b = UsockBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < LSNS_NEEDED {
        if b.bind(&us_name(i)).is_err() {
            ok = false;
        }
        if b.listen(&us_name(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < US_ROUNDS {
        let k = (r % LSNS_NEEDED as u32) as usize;
        let nm = us_name(k);
        if b.connect(&nm).is_err() {
            ok = false;
        }
        if b.accept(&nm).is_err() {
            ok = false;
        }
        r += 1;
    }
    let b_ok = ok && b.conserved();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未绑收连必拒
    let mut a = UsockBook::new();
    let _ = a.bind("/x");
    // 已绑未听收连判未听，未绑名收连判未绑，两路均必拒
    got[0] = a.connect("/x").err() == Some(US_NOBIND)
        && a.accept("/x").err() == Some(US_NOLEAF)
        && a.accept("/nope").err() == Some(US_NOBIND);
    // 2) 重名绑必拒
    let mut b = UsockBook::new();
    let _ = b.bind("/d");
    got[1] = b.bind("/d").err() == Some(US_DUPBIND);
    // 3) 队满连接必拒：第 BACKLOG_N+1 连必拒
    let mut c = UsockBook::new();
    let _ = c.bind("/f");
    let _ = c.listen("/f");
    let mut filled = true;
    let mut n = 0usize;
    while n < BACKLOG_N {
        if c.connect("/f").is_err() {
            filled = false;
        }
        n += 1;
    }
    got[2] = filled && c.connect("/f").err() == Some(US_BACKLOG);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_usockbook();
    d.freeze();
    got[3] = d.connect(&us_name(0)).err() == Some(US_FROZEN)
        && d.bind("/late").err() == Some(US_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11031");

    // J1 三段挂点千次对平：十端千轮 connect+accept
    let (mut book, green) = build_usockbook();
    s.add("J1-千次对平", green && book.lsns.len() == 10, "十端千轮三段挂点对平");

    // J2 连接注册账守恒：逐端 connect = accept 且队列清空
    s.add("J2-连接守恒", book.conserved(), "逐端收发配对挂起清空");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.connect(&us_name(0)).err() == Some(US_FROZEN)
        && book.accept(&us_name(0)).err() == Some(US_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后连收均必拒");

    // J5 注入检出：未绑收连 + 重名绑
    let inj = inject_four();
    s.add("J5-未绑重绑抓", inj[0] && inj[1], "未绑收连与重名绑两路必抓");

    // J6 注入检出：队满连接 + 冻结后写
    s.add("J6-队满冻结检出", inj[2] && inj[3], "队满连接与冻结后写必抓");

    // J7 backlog 深度：恰 BACKLOG_N 条挂起可入队
    let j7 = {
        let mut c = UsockBook::new();
        let _ = c.bind("/b");
        let _ = c.listen("/b");
        let mut n = 0usize;
        let mut ok = true;
        while n < BACKLOG_N {
            if c.connect("/b").is_err() {
                ok = false;
            }
            n += 1;
        }
        ok && c.find("/b").map(|k| c.lsns[k].pending.len()) == Some(BACKLOG_N)
    };
    s.add("J7-backlog 深度", j7, "挂起队列恰达深度可收");

    // J8 连接号序：accept 出队按 connect 入队序
    let j8 = {
        let mut c = UsockBook::new();
        let _ = c.bind("/o");
        let _ = c.listen("/o");
        let c1 = c.connect("/o");
        let c2 = c.connect("/o");
        let a1 = c.accept("/o");
        let a2 = c.accept("/o");
        a1 == c1 && a2 == c2
    };
    s.add("J8-连接号序", j8, "accept 按入队序出队配对");

    // J9 空队 accept 必拒
    let j9 = {
        let mut c = UsockBook::new();
        let _ = c.bind("/e");
        let _ = c.listen("/e");
        c.accept("/e").err() == Some(US_NOCONN)
    };
    s.add("J9-空队 accept 拒", j9, "空挂起队列 accept 必拒");

    // J10 bind 后未 listen 收连必拒
    let j10 = {
        let mut c = UsockBook::new();
        let _ = c.bind("/n");
        c.connect("/n").err() == Some(US_NOBIND)
    };
    s.add("J10-未听收连拒", j10, "只绑未听收连必拒");

    // J11 计数均衡：各端 connect/accept 各百次
    let j11 = {
        let per = US_ROUNDS / LSNS_NEEDED as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.lsns.len() {
            if book.lsns[k].connects != per || book.lsns[k].accepts != per {
                even = false;
            }
            k += 1;
        }
        even && per == 100
    };
    s.add("J11-计数均衡", j11, "十端各百次连接配对");

    // J12 渲染面：监听端渲染含名与计数
    let j12 = {
        let x = LsnEnd {
            name: "/r".to_string(),
            bound: true,
            listening: true,
            pending: Vec::new(),
            connects: 7,
            accepts: 7,
        };
        let r = x.render();
        r.contains("/r") && r.contains("c7") && r.contains("a7")
    };
    s.add("J12-渲染明细", j12, "监听端渲染含名计数");

    // J13 样本确定性：名同入同出
    let n4a = us_name(4);
    let n4b = us_name(4);
    s.add("J13-样本确定性", n4a == n4b, "监听名样本同入同出");

    // J14 守恒注入：漏 accept 一端守恒断言为假
    let j14 = {
        let mut c = UsockBook::new();
        let _ = c.bind("/m");
        let _ = c.listen("/m");
        let _ = c.connect("/m");
        let _ = c.connect("/m");
        let _ = c.accept("/m");
        c.find("/m").map(|k| c.lsns[k].pending.len()) == Some(1) && !c.conserved()
    };
    s.add("J14-守恒注入", j14, "漏配对守恒断言为假");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.conserved(),
        &format!("root={:#x} lsns=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11031_usock_flow() {
        let (mut b, built) = build_usockbook();
        assert!(built);
        assert!(b.accept(&us_name(0)).err() == Some(US_NOCONN));
        b.freeze();
        assert!(b.connect(&us_name(1)).err() == Some(US_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11031_checks_green() {
        let s = run_unx11031_checks();
        assert!(s.all_passed(), "UNX-F11031 判据未全绿");
    }
}
