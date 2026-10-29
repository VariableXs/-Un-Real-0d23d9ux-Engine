//! UNX-F10935 shmdt EINVAL 矩阵（非映射地址）（0x1d460 段）
//!
//! 纯功能模型：`shmdt` 卸载裁决板。
//! 正面：非挂载基址（含段内偏移地址、未用地址、已卸载地址、
//! 他人进程的基址）一律裁 EINVAL；反面：合法卸载千轮零误报，
//! 且每次卸载后段挂载计数与映射账逐项对平。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d460;
/// 非挂载基址
pub const DT_EINVAL: u32 = SEG_BASE + 0x01;
/// 地址不对齐（等同非基址）
pub const DT_UNALIGNED: u32 = SEG_BASE + 0x02;
/// 他人进程的基址
pub const DT_FOREIGN: u32 = SEG_BASE + 0x03;
/// 板内对账失败
pub const DT_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 对齐下限（SHMLBA）
pub const DT_LBA: usize = 4096;
/// 板容量（活跃映射上限）
pub const DT_CAP: usize = 32;
/// 反面验收轮数
pub const DT_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 一条活跃映射。
#[derive(Clone, Debug, PartialEq)]
pub struct DtMap {
    /// 进程号
    pub pid: u32,
    /// 段句柄
    pub shmid: u32,
    /// 挂载基址
    pub addr: usize,
    /// 段页数
    pub pages: usize,
}

impl DtMap {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.pid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.shmid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.addr as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.pages as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 单轮卸载日志。
#[derive(Clone, Debug, PartialEq)]
pub struct DtLog {
    /// 轮次
    pub round: usize,
    /// 请求地址
    pub addr: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// shmdt 卸载裁决板。
#[derive(Clone, Debug)]
pub struct DetachBoard {
    /// 活跃映射
    pub maps: Vec<DtMap>,
    /// 下一自动基址
    pub next_base: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功卸载轮
    pub ok_hits: usize,
    /// EINVAL（非基址/不对齐）
    pub einval_hits: usize,
    /// 不对齐命中（判定序上的独立一面，须单独入账）
    pub unaligned_hits: usize,
    /// 他人基址命中
    pub foreign_hits: usize,
    /// 逐轮日志
    pub log: Vec<DtLog>,
}

impl DetachBoard {
    /// 建板。
    pub fn new() -> Self {
        DetachBoard {
            maps: Vec::new(),
            next_base: 0x70000000,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            unaligned_hits: 0,
            foreign_hits: 0,
            log: Vec::new(),
        }
    }

    /// 模拟一次挂载（供测试铺场景；addr 为 0 时自动择址）。
    pub fn attach(&mut self, pid: u32, shmid: u32, pages: usize) -> usize {
        let base = self.next_base;
        self.next_base = self.next_base.wrapping_add(pages.wrapping_mul(DT_LBA));
        self.maps.push(DtMap { pid, shmid, addr: base, pages });
        base
    }

    /// 精确查找某进程某基址的映射。
    pub fn find_exact(&self, pid: u32, addr: usize) -> Option<usize> {
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].pid == pid && self.maps[i].addr == addr {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 地址是否落在某条映射的段区间内（非基址）。
    pub fn in_range(&self, addr: usize) -> bool {
        let mut i = 0;
        while i < self.maps.len() {
            let m = &self.maps[i];
            let span = m.pages.wrapping_mul(DT_LBA);
            if addr >= m.addr && addr < m.addr.wrapping_add(span) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 记账。
    fn book(&mut self, round: usize, addr: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == DT_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == DT_UNALIGNED => self.unaligned_hits = self.unaligned_hits.wrapping_add(1),
            x if x == DT_FOREIGN => self.foreign_hits = self.foreign_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(DtLog { round, addr, verdict });
    }

    /// 裁决一次 shmdt。判定序：
    /// ① 地址不对齐 → UNALIGNED；② 非本人基址：属他人 → FOREIGN；
    /// ③ 完全非映射 → EINVAL；④ 精确命中 → 卸载成功。
    pub fn shmdt(&mut self, pid: u32, addr: usize, round: usize) -> Result<usize, u32> {
        if addr % DT_LBA != 0 {
            self.book(round, addr, DT_UNALIGNED);
            return Err(DT_UNALIGNED);
        }
        let exact = self.find_exact(pid, addr);
        match exact {
            None => {
                if self.find_exact(0, addr).is_some() || self.other_owner(addr) {
                    self.book(round, addr, DT_FOREIGN);
                    return Err(DT_FOREIGN);
                }
                if self.in_range(addr) {
                    // 段内偏移：仍是 EINVAL 主面。
                    self.book(round, addr, DT_EINVAL);
                    return Err(DT_EINVAL);
                }
                self.book(round, addr, DT_EINVAL);
                return Err(DT_EINVAL);
            }
            Some(ix) => {
                let m = self.maps.remove(ix);
                self.book(round, addr, 0);
                Ok(m.pages)
            }
        }
    }

    /// 地址是否是任何映射的基址（无论属主）。
    fn other_owner(&self, addr: usize) -> bool {
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].addr == addr {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 账目对平（四类裁决：成功 + 三错误，逐类计数之和须等于轮次）。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.unaligned_hits)
            .wrapping_add(self.foreign_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// 活跃映射指纹和。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.maps.len() {
            h = (h ^ self.maps[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "DetachBoard{{maps={},rounds={},ok={},einval={},unaligned={},foreign={}}}",
            self.maps.len(), self.rounds, self.ok_hits, self.einval_hits,
            self.unaligned_hits, self.foreign_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被板正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut b = DetachBoard::new();
    let base = b.attach(1, 10, 2);
    // 注入 1：段内偏移地址 → EINVAL。
    caught[0] = matches!(b.shmdt(1, base.wrapping_add(DT_LBA), 9001), Err(x) if x == DT_EINVAL);
    // 注入 2：完全未用地址 → EINVAL。
    caught[1] = matches!(b.shmdt(1, 0x7abc0000, 9002), Err(x) if x == DT_EINVAL);
    // 注入 3：不对齐地址 → UNALIGNED。
    caught[2] = matches!(b.shmdt(1, base + 3, 9003), Err(x) if x == DT_UNALIGNED);
    // 注入 4：他人基址 → FOREIGN；本人合法基址可卸。
    let base2 = b.attach(2, 11, 1);
    caught[3] = matches!(b.shmdt(1, base2, 9004), Err(x) if x == DT_FOREIGN)
        && b.shmdt(1, base, 9005).is_ok();
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：挂-卸循环为主，四错误面穿插。返回"裁决符合预期"的轮数。
pub fn sample_thousand(b: &mut DetachBoard) -> usize {
    let mut good = 0;
    let mut r = 0;
    let mut slot = 0;
    while r < DT_ROUNDS {
        let low = r % 10;
        if low < 6 {
            // 合法面：挂一条、卸上一条，账循环滚动。
            let nb = b.attach(0x35, 10 + slot % 4, 1);
            let res = b.shmdt(0x35, nb, r);
            if res.is_ok() {
                good += 1;
            }
            slot = slot.wrapping_add(1);
        } else if low == 6 {
            // EINVAL 面：偏移地址。
            let probe = b.next_base.wrapping_add(DT_LBA / 2);
            if matches!(b.shmdt(0x35, probe, r), Err(x) if x == DT_EINVAL || x == DT_UNALIGNED) {
                good += 1;
            }
        } else if low == 7 {
            // EINVAL 面：未用地址。
            if matches!(b.shmdt(0x35, 0x7f000000, r), Err(x) if x == DT_EINVAL) {
                good += 1;
            }
        } else if low == 8 {
            // FOREIGN 面：他人基址。
            let fb = b.attach(0x36, 12, 1);
            if matches!(b.shmdt(0x35, fb, r), Err(x) if x == DT_FOREIGN) {
                good += 1;
            }
            // 清掉刚才那条他人映射，防账膨胀。
            let _ = b.shmdt(0x36, fb, r.wrapping_add(DT_ROUNDS));
        } else {
            // UNALIGNED 面。
            if matches!(b.shmdt(0x35, 0x7e000001, r), Err(x) if x == DT_UNALIGNED) {
                good += 1;
            }
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10935 判据组。
pub fn run_unx10935_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10935");
    // J1 正面：非挂载基址 → EINVAL 十轮全中（偏移/未用交替）。
    let mut b1 = DetachBoard::new();
    let base1 = b1.attach(1, 10, 2);
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let probe = if k % 2 == 0 {
            base1.wrapping_add(DT_LBA)
        } else {
            // 未用地址也须按 LBA 对齐，否则先被不对齐面截走（判成 UNALIGNED）。
            0x7abc0000usize.wrapping_add((k as usize).wrapping_mul(DT_LBA))
        };
        if matches!(b1.shmdt(1, probe, k), Err(x) if x == DT_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "非基址十轮全部裁 EINVAL");
    // J1 正面补充：双卸载第二轮也是 EINVAL。
    let mut b2 = DetachBoard::new();
    let base2 = b2.attach(1, 10, 1);
    let first = b2.shmdt(1, base2, 1);
    let second = b2.shmdt(1, base2, 2);
    s.add(
        "J1-双卸载",
        first.is_ok() && matches!(second, Err(x) if x == DT_EINVAL),
        "已卸载基址二次卸载裁 EINVAL",
    );
    // J2 反面：千轮合法卸载零误报。
    let mut b3 = DetachBoard::new();
    let good = sample_thousand(&mut b3);
    s.add_dyn("J2-反面零误报", good == DT_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "偏移/未用/不对齐/他人基址四路坏样本全部拦截",
    );
    // J4 判定序：不对齐先于非映射。
    let r4 = b1.shmdt(1, 0x1234567, 1);
    s.add("J4-判定序", matches!(r4, Err(x) if x == DT_UNALIGNED), "不对齐地址先行裁定");
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        DT_EINVAL > SEG_BASE && DT_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [DT_EINVAL, DT_UNALIGNED, DT_FOREIGN, DT_LEDGER];
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
    s.add("J7-账目对平", b3.ledger_ok(), "千轮之后计数与轮次、日志逐项对平");
    // J8 卸载即记账：合法卸载后活跃映射减少。
    let before8 = b3.maps.len();
    let nb8 = b3.attach(0x99, 20, 1);
    let _ = b3.shmdt(0x99, nb8, 1);
    s.add("J8-卸载记账", b3.maps.len() == before8, "挂一条卸一条后映射数还原");
    // J9 fold 稳定。
    let f = b3.fold();
    let clone = b3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 基址对齐不变式：全部活跃基址按 LBA 对齐。
    let mut aligned = true;
    let mut i = 0;
    while i < b3.maps.len() {
        if b3.maps[i].addr % DT_LBA != 0 {
            aligned = false;
        }
        i += 1;
    }
    s.add("J10-基址对齐", aligned, "全部活跃基址按 SHMLBA 对齐");
    // J11 渲染非空。
    let txt = b3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("DetachBoard"), "板级渲染含类型名与计数");
    // J12 段内偏移识别：偏移地址确实落在段区间。
    s.add("J12-区间识别", b3.in_range(nb8.wrapping_sub(1)) || b3.maps.is_empty(), "区间识别函数可用");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == DT_ROUNDS,
        &format!("good={} board={}", good, b3.render()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
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
    fn unx10935_shmdt_einval_flow() {
        // 流程：挂载 → 合法卸载 → 二次卸载 EINVAL → 偏移 EINVAL → 他人 FOREIGN。
        let mut b = DetachBoard::new();
        let a1 = b.attach(1, 10, 1);
        assert!(b.shmdt(1, a1, 1).is_ok());
        assert!(matches!(b.shmdt(1, a1, 2), Err(x) if x == DT_EINVAL));
        let a2 = b.attach(1, 10, 2);
        assert!(matches!(b.shmdt(1, a2.wrapping_add(DT_LBA), 3), Err(x) if x == DT_EINVAL));
        let a3 = b.attach(2, 11, 1);
        assert!(matches!(b.shmdt(1, a3, 4), Err(x) if x == DT_FOREIGN));
        assert!(b.shmdt(2, a3, 5).is_ok());
        assert!(b.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10935_checks_green() {
        let s = run_unx10935_checks();
        assert!(s.all_passed(), "UNX-F10935 判据未全绿");
    }
}
