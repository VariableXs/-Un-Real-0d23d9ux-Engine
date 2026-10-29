//! UNX-F10930 semop EIDRM/EINVAL/EFBIG 矩阵（0x1d3c0 段）
//!
//! 纯功能模型：带句柄表的 SysV `semop` 引用裁决门。
//! 与 F10929 不同点：本门挂在句柄簿之上，调用方以 `semid` 引用集合，
//! 判定序为：① semid 不在簿 → EINVAL；② 集合已 RMID → EIDRM；
//! ③ sem_num 越过集合界 → EFBIG（Linux 语义：越界走 EFBIG 面）。
//! 反面千轮样本只走合法路径，要求零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
pub const SEG_BASE: u32 = 0x1d3c0;
/// semid 不在句柄簿（EINVAL 面）
pub const SR_EINVAL: u32 = SEG_BASE + 0x01;
/// 集合已被 RMID 标记（EIDRM 面）
pub const SR_EIDRM: u32 = SEG_BASE + 0x02;
/// sem_num 越过集合信号量个数（EFBIG 面）
pub const SR_EFBIG: u32 = SEG_BASE + 0x03;
/// 操作会使值落负（非本卡主面，占位裁定）
pub const SR_NEG: u32 = SEG_BASE + 0x04;
/// 空操作数组
pub const SR_EMPTY: u32 = SEG_BASE + 0x05;
/// nsops 超过单次上限
pub const SR_E2BIG: u32 = SEG_BASE + 0x06;
/// 句柄簿对账失败
pub const SR_LEDGER: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------- 常数
/// 单次 semop 最大条数
pub const REF_SEMOPM: usize = 16;
/// 句柄簿容量
pub const BOOK_CAP: usize = 8;
/// 反面验收轮数
pub const REF_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 句柄簿中的一个信号量集合。
#[derive(Clone, Debug)]
pub struct SemObj {
    /// 句柄（semid）
    pub semid: u32,
    /// 创建键
    pub key: u32,
    /// 信号量个数
    pub nsems: usize,
    /// 当前值
    pub vals: Vec<i32>,
    /// 是否已 RMID
    pub removed: bool,
}

impl SemObj {
    /// 建集合。
    pub fn new(semid: u32, key: u32, nsems: usize, init: i32) -> Self {
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nsems {
            vals.push(init);
            i += 1;
        }
        SemObj { semid, key, nsems, vals, removed: false }
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.semid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.key as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.nsems as u64)).wrapping_mul(0x100000001b3);
        let mut i = 0;
        while i < self.vals.len() {
            h = (h ^ (self.vals[i] as i64 as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.removed as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < self.vals.len() {
            acc += self.vals[i] as i64;
            i += 1;
        }
        format!(
            "SemObj{{id={},key=0x{:x},nsems={},sum={},rm={}}}",
            self.semid, self.key, self.nsems, acc, self.removed as u32
        )
    }
}

/// 单轮裁决日志。
#[derive(Clone, Debug, PartialEq)]
pub struct RefLog {
    /// 轮次
    pub round: usize,
    /// 目标句柄
    pub semid: u32,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// 引用裁决门：句柄簿 + 判定序 + 计数账。
#[derive(Clone, Debug)]
pub struct RefGate {
    /// 句柄簿
    pub book: Vec<SemObj>,
    /// 下一句柄号
    pub next_id: u32,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL（坏句柄）
    pub einval_hits: usize,
    /// EIDRM（已删集合）
    pub eidrm_hits: usize,
    /// EFBIG（越界）
    pub efbig_hits: usize,
    /// 逐轮日志
    pub log: Vec<RefLog>,
}

impl RefGate {
    /// 建空门。
    pub fn new() -> Self {
        RefGate {
            book: Vec::new(),
            next_id: 1,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            eidrm_hits: 0,
            efbig_hits: 0,
            log: Vec::new(),
        }
    }

    /// 建集合并返回句柄；簿满返回 None。
    pub fn create(&mut self, key: u32, nsems: usize, init: i32) -> Option<u32> {
        if self.book.len() >= BOOK_CAP {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.book.push(SemObj::new(id, key, nsems, init));
        Some(id)
    }

    /// 句柄查找。
    pub fn find(&self, semid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.book.len() {
            if self.book[i].semid == semid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 读集合首信号量当前值（无此句柄返 0）。
    pub fn val0(&self, semid: u32) -> i32 {
        match self.find(semid) {
            Some(ix) => self.book[ix].vals[0],
            None => 0,
        }
    }

    /// RMID：标记删除（映射存活语义由后续单核验）。
    pub fn rmid(&mut self, semid: u32) -> bool {
        match self.find(semid) {
            Some(ix) => {
                self.book[ix].removed = true;
                true
            }
            None => false,
        }
    }

    /// 记账。
    fn book_round(&mut self, round: usize, semid: u32, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SR_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == SR_EIDRM => self.eidrm_hits = self.eidrm_hits.wrapping_add(1),
            x if x == SR_EFBIG => self.efbig_hits = self.efbig_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(RefLog { round, semid, verdict });
    }

    /// 裁决一次以句柄引用的 semop。判定序：
    /// ① 句柄查无 → EINVAL；② 已 RMID → EIDRM；③ 空数组 → EMPTY；
    /// ④ nsops 超上限 → E2BIG；⑤ 逐条越界 → EFBIG；⑥ 预演落负 → NEG；
    /// ⑦ 全过 → 提交。
    pub fn judge(&mut self, semid: u32, ops: &[(usize, i32)], round: usize) -> Result<usize, u32> {
        let ix = match self.find(semid) {
            Some(ix) => ix,
            None => {
                self.book_round(round, semid, SR_EINVAL);
                return Err(SR_EINVAL);
            }
        };
        if self.book[ix].removed {
            self.book_round(round, semid, SR_EIDRM);
            return Err(SR_EIDRM);
        }
        if ops.is_empty() {
            self.book_round(round, semid, SR_EMPTY);
            return Err(SR_EMPTY);
        }
        if ops.len() > REF_SEMOPM {
            self.book_round(round, semid, SR_E2BIG);
            return Err(SR_E2BIG);
        }
        let nsems = self.book[ix].nsems;
        let mut k = 0;
        while k < ops.len() {
            if ops[k].0 >= nsems {
                self.book_round(round, semid, SR_EFBIG);
                return Err(SR_EFBIG);
            }
            k += 1;
        }
        let mut staged = self.book[ix].vals.clone();
        let mut k = 0;
        while k < ops.len() {
            let cur = staged[ops[k].0];
            let after = cur.wrapping_add(ops[k].1);
            if after < 0 {
                self.book_round(round, semid, SR_NEG);
                return Err(SR_NEG);
            }
            staged[ops[k].0] = after;
            k += 1;
        }
        self.book[ix].vals = staged;
        self.book_round(round, semid, 0);
        Ok(ops.len())
    }

    /// 账目对平：四类计数与轮次、日志一致。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.eidrm_hits)
            .wrapping_add(self.efbig_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// 门级指纹：全部集合指纹叠判定账。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.book.len() {
            h = (h ^ self.book[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "RefGate{{objs={},rounds={},ok={},einval={},eidrm={},efbig={}}}",
            self.book.len(), self.rounds, self.ok_hits,
            self.einval_hits, self.eidrm_hits, self.efbig_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = RefGate::new();
    let id = g.create(0xa930, 3, 2).unwrap_or(0);
    // 注入 1：坏句柄 → EINVAL。
    caught[0] = matches!(g.judge(0xdead, &[(0, -1)], 9001), Err(x) if x == SR_EINVAL);
    // 注入 2：RMID 后再引用 → EIDRM。
    assert!(g.rmid(id));
    caught[1] = matches!(g.judge(id, &[(0, 1)], 9002), Err(x) if x == SR_EIDRM);
    // 注入 3：越界 sem_num → EFBIG（在另一个存活集合上）。
    let id2 = g.create(0xa931, 2, 4).unwrap_or(0);
    caught[2] = matches!(g.judge(id2, &[(2, -1)], 9003), Err(x) if x == SR_EFBIG);
    // 注入 4：落负预演 → NEG 且值零残留。
    let before = g.book[g.find(id2).unwrap_or(0)].vals.clone();
    let r4 = g.judge(id2, &[(0, -9)], 9004);
    caught[3] = matches!(r4, Err(x) if x == SR_NEG)
        && g.book[g.find(id2).unwrap_or(0)].vals == before;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：三集合（中途删一），返回"裁决符合预期"的轮数。
pub fn sample_thousand(g: &mut RefGate) -> usize {
    let ida = g.create(0x1a, 3, 4).unwrap_or(0);
    let idb = g.create(0x1b, 2, 2).unwrap_or(0);
    let idc = g.create(0x1c, 4, 1).unwrap_or(0);
    let mut good = 0;
    let mut r = 0;
    while r < REF_ROUNDS {
        let low = r % 10;
        // 目标三元组带上 sem_num：越界面必须真的把越界下标送进裁决门。
        let (target, idx, op, expect_ok) = if low == 0 {
            // 坏句柄面。
            (0x7fffu32, 0usize, -1i32, false)
        } else if low == 1 && r >= 300 {
            // 300 轮后 idc 已删 → EIDRM 面。
            (idc, 0usize, 1i32, false)
        } else if low == 2 {
            // EFBIG 面：对 idb（nsems=2）用下标 5。
            (idb, 5usize, -1i32, false)
        } else {
            // 合法面：有余量则扣减，枯竭则回补（同为合法路径）；idb 单调回补。
            if low % 2 == 0 {
                if g.val0(ida) >= 1 {
                    (ida, 0usize, -1i32, true)
                } else {
                    (ida, 0usize, 2i32, true)
                }
            } else {
                (idb, 0usize, 1i32, true)
            }
        };
        // 中途删除 idc，制造 EIDRM 场景。
        if r == 300 {
            assert!(g.rmid(idc));
        }
        let res = g.judge(target, &[(idx, op)], r);
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == SR_EINVAL
                        || *x == SR_EIDRM
                        || *x == SR_EFBIG
                        || *x == SR_NEG
                        || *x == SR_EMPTY)
            }
        };
        if matched {
            good += 1;
        }
        // 定期再回补 ida 一次，保证扣减路径占比仍高。
        if r % 48 == 0 && g.find(ida).is_some() && !g.book[g.find(ida).unwrap_or(0)].removed {
            let _ = g.judge(ida, &[(0usize, 2)], r.wrapping_add(REF_ROUNDS));
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10930 判据组。
pub fn run_unx10930_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10930");
    // J1 正面之一：已删集合 → EIDRM 十轮全中。
    let mut g1 = RefGate::new();
    let ida = g1.create(0x21, 2, 3).unwrap_or(0);
    assert!(g1.rmid(ida));
    let mut eidrm_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.judge(ida, &[(0, 1)], k), Err(x) if x == SR_EIDRM) {
            eidrm_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EIDRM", eidrm_cnt == 10, "已删集合十轮全部裁 EIDRM");
    // J1 正面之二：非法 semnum → EFBIG 十轮全中。
    let mut g2 = RefGate::new();
    let idb = g2.create(0x22, 3, 1).unwrap_or(0);
    let mut efbig_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g2.judge(idb, &[(3 + k, -1)], k), Err(x) if x == SR_EFBIG) {
            efbig_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EFBIG", efbig_cnt == 10, "非法 semnum 十轮全部裁 EFBIG");
    // J1 正面之三：坏句柄 → EINVAL 十轮全中。
    let mut g3 = RefGate::new();
    let _ = g3.create(0x23, 2, 1);
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g3.judge(0x5150u32 + k as u32, &[(0, 1)], k), Err(x) if x == SR_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "坏句柄十轮全部裁 EINVAL");
    // J2 反面：千轮存活集合零误报。
    let mut g4 = RefGate::new();
    let good = sample_thousand(&mut g4);
    s.add_dyn("J2-反面零误报", good == REF_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EINVAL/EIDRM/EFBIG/落负四路坏样本全部拦截",
    );
    // J4 EIDRM 先于 EFBIG：删后越界引用裁 EIDRM。
    let mut g5 = RefGate::new();
    let id5 = g5.create(0x24, 2, 1).unwrap_or(0);
    assert!(g5.rmid(id5));
    s.add(
        "J4-判定序",
        matches!(g5.judge(id5, &[(9, 1)], 1), Err(x) if x == SR_EIDRM),
        "已删集合的越界引用按 EIDRM 先行裁定",
    );
    // J5 错误码分段。
    let seg_ok = SR_EINVAL > SEG_BASE && SR_LEDGER < SEG_BASE + 0x20;
    s.add("J5-错误码分段", seg_ok, "七个错误码全部落在段基址页内");
    // J6 错误码互异。
    let codes = [SR_EINVAL, SR_EIDRM, SR_EFBIG, SR_NEG, SR_EMPTY, SR_E2BIG, SR_LEDGER];
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
    s.add("J6-错误码互异", distinct, "七个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", g4.ledger_ok(), "千轮之后四类计数与轮次、日志逐项对平");
    // J8 句柄簿单调：next_id 严格递增。
    let next = g4.next_id;
    let mut monotone = true;
    let mut i = 0;
    while i < g4.book.len() {
        if g4.book[i].semid >= next {
            monotone = false;
        }
        i += 1;
    }
    s.add("J8-句柄单调", monotone, "簿内全部句柄小于 next_id");
    // J9 fold 稳定。
    let f = g4.fold();
    let clone = g4.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 合法路径复核。
    let mut g6 = RefGate::new();
    let id6 = g6.create(0x25, 1, 5).unwrap_or(0);
    let ok6 = g6.judge(id6, &[(0, -3)], 1);
    s.add(
        "J10-合法扣减",
        matches!(ok6, Ok(1)) && g6.book[0].vals[0] == 2,
        "充足资源下 -3 扣减成功且值落为 2",
    );
    // J11 渲染非空。
    let txt = g4.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("RefGate"), "门级渲染含类型名与计数");
    // J12 EFBIG 优先于落负：越界下标即便会落负也先裁 EFBIG。
    let r12 = g6.judge(id6, &[(7, -9)], 2);
    s.add("J12-越界优先", matches!(r12, Err(x) if x == SR_EFBIG), "越界与落负同现时按判定序裁 EFBIG");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == REF_ROUNDS,
        &format!("good={} gate={}", good, g4.render()),
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
    fn unx10930_semop_eidrm_flow() {
        // 流程：建集合 → 合法扣减 → RMID → EIDRM → 新集合越界 EFBIG。
        let mut g = RefGate::new();
        let id = match g.create(0xcafe, 2, 4) {
            Some(x) => x,
            None => 0,
        };
        assert!(id != 0 || g.book.len() == 1);
        assert_eq!(g.judge(id, &[(0, -2)], 1), Ok(1));
        assert!(g.rmid(id));
        assert!(matches!(g.judge(id, &[(0, 1)], 2), Err(x) if x == SR_EIDRM));
        let id2 = g.create(0xcafd, 2, 1).unwrap_or(0);
        assert!(matches!(g.judge(id2, &[(2, 1)], 3), Err(x) if x == SR_EFBIG));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10930_checks_green() {
        let s = run_unx10930_checks();
        assert!(s.all_passed(), "UNX-F10930 判据未全绿");
    }
}
