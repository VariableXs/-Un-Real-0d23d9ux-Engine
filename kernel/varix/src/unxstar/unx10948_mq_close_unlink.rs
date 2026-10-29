//! UNX-F10948 mq_close/mq_unlink 错误矩阵（0x1d600 段）
//!
//! 纯功能模型：POSIX 消息队列关闭/删除裁决台。
//! 三错误面：① 关闭不属于自己的描述符 → EBADF；② 删除不存在的
//! 名字 → ENOENT；③ 非属主删除 → EACCES。
//! 生命周期：unlink 摘名但存量描述符仍可用，最后一个 close 后队列消亡。
//! 反面千轮样本只走合法关闭/删除，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d600;
/// 关闭坏/异主描述符
pub const CL_EBADF: u32 = SEG_BASE + 0x01;
/// 删除不存在的名字
pub const CL_ENOENT: u32 = SEG_BASE + 0x02;
/// 非属主删除
pub const CL_EACCES: u32 = SEG_BASE + 0x03;
/// 台内对账失败
pub const CL_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 反面验收轮数
pub const CL_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 队列条目：名 + 属主 + 存量描述符。
#[derive(Clone, Debug)]
pub struct ClQueue {
    /// 队列名
    pub name: String,
    /// 属主（创建者 pid）
    pub owner: u32,
    /// 存量描述符（pid, fd）
    pub fds: Vec<(u32, u32)>,
    /// 是否已摘名
    pub unlinked: bool,
}

impl ClQueue {
    /// 存量描述符数。
    pub fn live_fds(&self) -> usize {
        self.fds.len()
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let bytes = self.name.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            h = (h ^ (bytes[i] as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.owner as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.fds.len() as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.unlinked as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct ClLog {
    /// 轮次
    pub round: usize,
    /// 操作名
    pub op: String,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_close/mq_unlink 裁决台。
#[derive(Clone, Debug)]
pub struct CloseDesk {
    /// 队列表
    pub queues: Vec<ClQueue>,
    /// 已消亡队列数
    pub dead: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// ENOENT 命中
    pub enoent_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// 逐轮日志
    pub log: Vec<ClLog>,
}

impl CloseDesk {
    /// 建台。
    pub fn new() -> Self {
        CloseDesk {
            queues: Vec::new(),
            dead: 0,
            rounds: 0,
            ok_hits: 0,
            ebadf_hits: 0,
            enoent_hits: 0,
            eacces_hits: 0,
            log: Vec::new(),
        }
    }

    /// 建队列（模拟 mq_open 的创建侧）。
    pub fn create(&mut self, name: &str, owner: u32, fd: u32) {
        let mut fds = Vec::new();
        fds.push((owner, fd));
        self.queues.push(ClQueue {
            name: name.to_string(),
            owner,
            fds,
            unlinked: false,
        });
    }

    /// 再开一条描述符。
    pub fn attach(&mut self, name: &str, pid: u32, fd: u32) -> bool {
        let mut i = 0;
        while i < self.queues.len() {
            if self.queues[i].name == name && !self.queues[i].unlinked {
                self.queues[i].fds.push((pid, fd));
                return true;
            }
            i += 1;
        }
        false
    }

    /// 队列查找。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.queues.len() {
            if self.queues[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账。
    fn book(&mut self, round: usize, op: &str, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == CL_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            x if x == CL_ENOENT => self.enoent_hits = self.enoent_hits.wrapping_add(1),
            x if x == CL_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(ClLog { round, op: op.to_string(), verdict });
    }

    /// mq_close：判定序 ① 队列查无 → EBADF；
    /// ② (pid, fd) 不在存量账 → EBADF；③ 移除；账空且已摘名 → 消亡。
    pub fn mq_close(&mut self, pid: u32, name: &str, fd: u32, round: usize) -> Result<(), u32> {
        let ix = match self.find(name) {
            Some(ix) => ix,
            None => {
                self.book(round, "close", CL_EBADF);
                return Err(CL_EBADF);
            }
        };
        let mut slot = None;
        let mut i = 0;
        while i < self.queues[ix].fds.len() {
            if self.queues[ix].fds[i] == (pid, fd) {
                slot = Some(i);
            }
            i += 1;
        }
        match slot {
            None => {
                self.book(round, "close", CL_EBADF);
                Err(CL_EBADF)
            }
            Some(s) => {
                let _ = self.queues[ix].fds.remove(s);
                let gone = self.queues[ix].fds.is_empty() && self.queues[ix].unlinked;
                if gone {
                    let _ = self.queues.remove(ix);
                    self.dead = self.dead.wrapping_add(1);
                }
                self.book(round, "close", 0);
                Ok(())
            }
        }
    }

    /// mq_unlink：判定序 ① 查无名 → ENOENT；② 非属主 → EACCES；
    /// ③ 摘名；无存量描述符 → 立即消亡。
    pub fn mq_unlink(&mut self, pid: u32, name: &str, round: usize) -> Result<(), u32> {
        let ix = match self.find(name) {
            Some(ix) => ix,
            None => {
                self.book(round, "unlink", CL_ENOENT);
                return Err(CL_ENOENT);
            }
        };
        if self.queues[ix].owner != pid {
            self.book(round, "unlink", CL_EACCES);
            return Err(CL_EACCES);
        }
        self.queues[ix].unlinked = true;
        if self.queues[ix].fds.is_empty() {
            let _ = self.queues.remove(ix);
            self.dead = self.dead.wrapping_add(1);
        }
        self.book(round, "unlink", 0);
        Ok(())
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.ebadf_hits)
            .wrapping_add(self.enoent_hits)
            .wrapping_add(self.eacces_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.queues.len() {
            h = (h ^ self.queues[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.dead as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "CloseDesk{{queues={},dead={},rounds={},ok={},ebadf={},enoent={},eacces={}}}",
            self.queues.len(), self.dead, self.rounds, self.ok_hits,
            self.ebadf_hits, self.enoent_hits, self.eacces_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = CloseDesk::new();
    d.create("/q", 1, 0);
    // 注入 1：异主描述符关闭 → EBADF。
    caught[0] = matches!(d.mq_close(2, "/q", 0, 9001), Err(x) if x == CL_EBADF);
    // 注入 2：不存在名字的 close → EBADF；unlink → ENOENT。
    caught[1] = matches!(d.mq_unlink(1, "/ghost", 9002), Err(x) if x == CL_ENOENT);
    // 注入 3：非属主删除 → EACCES。
    caught[2] = matches!(d.mq_unlink(9, "/q", 9003), Err(x) if x == CL_EACCES);
    // 注入 4：好的 fd 号但未在账 → EBADF。
    caught[3] = matches!(d.mq_close(1, "/q", 77, 9004), Err(x) if x == CL_EBADF);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：建-关-删循环为主，三错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut CloseDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    let mut slot = 0;
    while r < CL_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 5 {
            // 合法面：建 → 关 →（可能）删。
            let name = "/cyc";
            d.create(name, 0x48, slot);
            let ok1 = d.mq_close(0x48, name, slot, r).is_ok();
            slot = slot.wrapping_add(1);
            let ok2 = d.mq_unlink(0x48, name, r).is_ok();
            matched = ok1 && ok2;
        } else if low == 5 {
            // EBADF 面。
            matched = matches!(d.mq_close(0x49, "/none", 1, r), Err(x) if x == CL_EBADF);
        } else if low == 6 {
            // ENOENT 面。
            matched = matches!(d.mq_unlink(0x48, "/ghost", r), Err(x) if x == CL_ENOENT);
        } else if low == 7 {
            // EACCES 面。
            d.create("/own", 0x48, slot);
            let hit = matches!(d.mq_unlink(0x50, "/own", r), Err(x) if x == CL_EACCES);
            // 属主自己收尾。
            let okc = d.mq_close(0x48, "/own", slot, r).is_ok();
            let oku = d.mq_unlink(0x48, "/own", r).is_ok();
            slot = slot.wrapping_add(1);
            matched = hit && okc && oku;
        } else if low < 9 {
            // 合法面：多描述符存活期内摘名，逐个关闭后消亡。
            d.create("/late", 0x48, slot);
            let _ = d.attach("/late", 0x51, slot.wrapping_add(1));
            let ok1 = d.mq_unlink(0x48, "/late", r).is_ok();
            let ok2 = d.mq_close(0x48, "/late", slot, r).is_ok();
            let ok3 = d.mq_close(0x51, "/late", slot.wrapping_add(1), r).is_ok();
            slot = slot.wrapping_add(2);
            matched = ok1 && ok2 && ok3;
        } else {
            // EBADF 面：重复关闭（第二次 fd 已不在账）。
            d.create("/dbl", 0x48, slot);
            let ok1 = d.mq_close(0x48, "/dbl", slot, r).is_ok();
            let hit = matches!(d.mq_close(0x48, "/dbl", slot, r), Err(x) if x == CL_EBADF);
            let ok2 = d.mq_unlink(0x48, "/dbl", r).is_ok();
            slot = slot.wrapping_add(1);
            matched = ok1 && hit && ok2;
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10948 判据组。
pub fn run_unx10948_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10948");
    let mk = || {
        let mut d = CloseDesk::new();
        d.create("/a", 1, 0);
        d
    };
    // J1 正面之一：EBADF 十轮全中。
    let mut d1 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d1.mq_close(9, "/a", 0, k)
        } else {
            d1.mq_close(1, "/ghost", 0, k)
        };
        if matches!(r, Err(x) if x == CL_EBADF) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EBADF", c == 10, "异主/无名关闭十轮全部裁 EBADF");
    // J1 正面之二：ENOENT 十轮全中。
    let mut d2 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d2.mq_unlink(1, "/ghost", k), Err(x) if x == CL_ENOENT) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOENT", c == 10, "删除不存在的名字十轮全部裁 ENOENT");
    // J1 正面之三：EACCES 十轮全中。
    let mut d3 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d3.mq_unlink(2 + k as u32, "/a", k), Err(x) if x == CL_EACCES) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", c == 10, "非属主删除十轮全部裁 EACCES");
    // J2 反面：千轮合法关闭/删除零误报。
    let mut d4 = CloseDesk::new();
    let good = sample_thousand(&mut d4);
    s.add_dyn("J2-反面零误报", good == CL_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/ENOENT/EACCES/未在账四路坏样本全部拦截",
    );
    // J4 摘名不夺权：unlink 后存量描述符仍可关。
    let mut d5 = CloseDesk::new();
    d5.create("/k", 1, 0);
    let _ = d5.attach("/k", 2, 1);
    let ok4 = d5.mq_unlink(1, "/k", 1).is_ok();
    let still = d5.find("/k").is_some();
    let ok5 = d5.mq_close(2, "/k", 1, 2).is_ok();
    let ok6 = d5.mq_close(1, "/k", 0, 3).is_ok();
    s.add(
        "J4-摘名不夺权",
        ok4 && still && ok5 && ok6 && d5.find("/k").is_none(),
        "unlink 后旧描述符逐一关闭，最后队列消亡",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        CL_EBADF > SEG_BASE && CL_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [CL_EBADF, CL_ENOENT, CL_EACCES, CL_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J6-错误码互异", distinct, "四个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", d4.ledger_ok(), "千轮之后四类计数与轮次、日志逐项对平");
    // J8 fold 稳定。
    let f = d4.fold();
    let clone = d4.clone();
    s.add("J8-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J9 消亡账：样本结束后表内无 /cyc 残留。
    s.add("J9-消亡账", d4.dead > 0 && d4.find("/cyc").is_none(), "循环建的队列全部消亡");
    // J10 渲染非空。
    let txt = d4.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("CloseDesk"), "台级渲染含类型名与计数");
    // J11 无描述符直删。
    let mut d6 = CloseDesk::new();
    d6.create("/z", 1, 0);
    let _ = d6.mq_close(1, "/z", 0, 1);
    let ok11 = d6.mq_unlink(1, "/z", 2).is_ok();
    s.add(
        "J11-直删即亡",
        ok11 && d6.find("/z").is_none() && d6.dead == 1,
        "描述符清零后 unlink 立即消亡",
    );
    // J12 动态：千轮明细。
    s.add_dyn(
        "J12-千轮明细",
        good == CL_ROUNDS,
        &format!("good={} desk={}", good, d4.render()),
    );
    // J13 动态：注入明细。
    s.add_dyn(
        "J13-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10948_mq_close_unlink_flow() {
        // 流程：建队 → 摘名 → 旧 fd 可关 → 消亡 → ENOENT → EACCES → EBADF。
        let mut d = CloseDesk::new();
        d.create("/q", 1, 0);
        let _ = d.attach("/q", 2, 1);
        assert!(d.mq_unlink(1, "/q", 1).is_ok());
        assert!(d.mq_close(2, "/q", 1, 2).is_ok());
        assert!(d.mq_close(1, "/q", 0, 3).is_ok());
        assert!(d.find("/q").is_none());
        assert!(matches!(d.mq_unlink(1, "/q", 4), Err(x) if x == CL_ENOENT));
        let mut d2 = CloseDesk::new();
        d2.create("/r", 1, 0);
        assert!(matches!(d2.mq_unlink(2, "/r", 5), Err(x) if x == CL_EACCES));
        assert!(matches!(d2.mq_close(2, "/r", 0, 6), Err(x) if x == CL_EBADF));
        assert!(d.ledger_ok() && d2.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10948_checks_green() {
        let s = run_unx10948_checks();
        assert!(s.all_passed(), "UNX-F10948 判据未全绿");
    }
}
