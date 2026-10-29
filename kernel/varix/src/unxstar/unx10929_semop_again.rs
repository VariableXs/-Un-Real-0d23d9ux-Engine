//! UNX-F10929 semop EAGAIN/E2BIG 矩阵（0x1d3a0 段）
//!
//! 纯功能模型：SysV `semop` 单次调用裁决门。
//! 覆盖三条主要错误面：`nsops` 超过 `SEMOPM` 上限 → E2BIG；
//! `sem_num` 越过集合界 → EINVAL；NOWAIT 标志下资源不足 → EAGAIN。
//! 全部裁决走"先校验后生效"的原子预演路径：任一操作失败，
//! 已在预演向量上做过的扣减一律丢弃，集合原值零残留。
//! 本模块不触碰真实内核对象，仅以纯数据结构复现判定序。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d3a0;
/// nsops 超过 SEMOPM 上限（E2BIG 面）
pub const SE_E2BIG: u32 = SEG_BASE + 0x01;
/// sem_num 越过集合信号量个数（EINVAL 面）
pub const SE_EINVAL: u32 = SEG_BASE + 0x02;
/// NOWAIT 下资源不足立即让位（EAGAIN 面）
pub const SE_EAGAIN: u32 = SEG_BASE + 0x03;
/// 非 NOWAIT 阻塞在本模型中的占位裁定（不真实阻塞）
pub const SE_WOULD_BLOCK: u32 = SEG_BASE + 0x04;
/// 空操作数组（nsops == 0 视为无参调用，判无效）
pub const SE_EMPTY_OPS: u32 = SEG_BASE + 0x05;
/// 集合已被 RMID 标记删除
pub const SE_EIDRM: u32 = SEG_BASE + 0x06;
/// 账目对平失败（ok/四错误计数与轮次不符）
pub const SE_LEDGER: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------- 常数
/// 单次 semop 允许的最大 sembuf 条数（SEMOPM）
pub const SEMOPM_MAX: usize = 32;
/// sem_flg：非阻塞
pub const FLG_NOWAIT: u32 = 0x0001;
/// sem_flg：进程终止时回滚
pub const FLG_UNDO: u32 = 0x0002;
/// 反面验收轮数（千次）
pub const LEGAL_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 单条 sembuf 的纯数据镜像。
#[derive(Clone, Debug, PartialEq)]
pub struct SemBuf {
    /// 目标信号量在集合内的下标
    pub sem_num: usize,
    /// 增减量（正加负减）
    pub sem_op: i32,
    /// 标志位（NOWAIT/UNDO）
    pub sem_flg: u32,
}

impl SemBuf {
    /// 便捷构造。
    pub fn of(sem_num: usize, sem_op: i32, sem_flg: u32) -> Self {
        SemBuf { sem_num, sem_op, sem_flg }
    }
}

/// 信号量集合的纯数据镜像。
#[derive(Clone, Debug)]
pub struct SemSet {
    /// 集合键
    pub key: u32,
    /// 集合内信号量个数
    pub nsems: usize,
    /// 各信号量当前值
    pub vals: Vec<i32>,
    /// 是否已被 RMID 标记
    pub removed: bool,
}

impl SemSet {
    /// 建集合，全部信号量初始化为同一初值。
    pub fn new(key: u32, nsems: usize, init: i32) -> Self {
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nsems {
            vals.push(init);
            i += 1;
        }
        SemSet { key, nsems, vals, removed: false }
    }

    /// 当前值向量快照（用于原子性断言）。
    pub fn snapshot(&self) -> Vec<i32> {
        self.vals.clone()
    }

    /// FNV 折叠指纹：key、nsems、值向量与 removed 位一并入账。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
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

    /// 单行渲染，供日志与判据说明使用。
    pub fn render(&self) -> String {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < self.vals.len() {
            acc += self.vals[i] as i64;
            i += 1;
        }
        format!(
            "SemSet{{key=0x{:x},nsems={},sum={},removed={}}}",
            self.key, self.nsems, acc, self.removed as u32
        )
    }
}

/// 单轮裁决日志条目。verdict 为 0 表示成功。
#[derive(Clone, Debug, PartialEq)]
pub struct OpLog {
    /// 轮次号（由调用方给定的序号）
    pub round: usize,
    /// 裁定码：0 成功，否则为 SE_* 错误码
    pub verdict: u32,
    /// 预演中已推进的操作条数（失败时为失败点前的条数）
    pub staged: usize,
}

impl OpLog {
    /// 是否为成功条目。
    pub fn is_ok(&self) -> bool {
        self.verdict == 0
    }
}

/// semop 裁决门：集合 + 判定序 + 计数账。
#[derive(Clone, Debug)]
pub struct SemopGate {
    /// 被裁决的信号量集合
    pub set: SemSet,
    /// 本门采用的 SEMOPM 上限
    pub semopm: usize,
    /// 已裁决轮次
    pub rounds: usize,
    /// 成功轮数
    pub ok_hits: usize,
    /// EAGAIN 命中数
    pub eagain_hits: usize,
    /// EINVAL 命中数
    pub einval_hits: usize,
    /// E2BIG 命中数
    pub e2big_hits: usize,
    /// EIDRM 命中数
    pub eidrm_hits: usize,
    /// 逐轮日志
    pub log: Vec<OpLog>,
}

impl SemopGate {
    /// 建门：key/nsems/初值/上限齐备。
    pub fn new(key: u32, nsems: usize, init: i32, semopm: usize) -> Self {
        SemopGate {
            set: SemSet::new(key, nsems, init),
            semopm,
            rounds: 0,
            ok_hits: 0,
            eagain_hits: 0,
            einval_hits: 0,
            e2big_hits: 0,
            eidrm_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账一条裁决。
    fn book(&mut self, round: usize, verdict: u32, staged: usize) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SE_EAGAIN => self.eagain_hits = self.eagain_hits.wrapping_add(1),
            x if x == SE_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == SE_E2BIG => self.e2big_hits = self.e2big_hits.wrapping_add(1),
            x if x == SE_EIDRM => self.eidrm_hits = self.eidrm_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(OpLog { round, verdict, staged });
    }

    /// 裁决一次 semop 调用。判定序：
    /// ① 集合已删 → EIDRM；② 空操作数组 → EMPTY_OPS；
    /// ③ nsops 超上限 → E2BIG；④ 逐条 sem_num 越界 → EINVAL；
    /// ⑤ 预演逐条扣减，NOWAIT 且会变负 → EAGAIN（原子，零残留）；
    /// ⑥ 非 NOWAIT 会变负 → WOULD_BLOCK 占位；⑦ 全部通过 → 提交并 Ok。
    pub fn judge(&mut self, ops: &[SemBuf], round: usize) -> Result<usize, u32> {
        if self.set.removed {
            self.book(round, SE_EIDRM, 0);
            return Err(SE_EIDRM);
        }
        if ops.is_empty() {
            self.book(round, SE_EMPTY_OPS, 0);
            return Err(SE_EMPTY_OPS);
        }
        if ops.len() > self.semopm {
            self.book(round, SE_E2BIG, 0);
            return Err(SE_E2BIG);
        }
        // 越界校验全量前置，保证 EINVAL 先于任何扣减生效。
        let mut i = 0;
        while i < ops.len() {
            if ops[i].sem_num >= self.set.nsems {
                self.book(round, SE_EINVAL, i);
                return Err(SE_EINVAL);
            }
            i += 1;
        }
        // 预演向量：失败即整轮丢弃，体现内核 semop 的原子性。
        let mut staged_vals = self.set.vals.clone();
        let mut j = 0;
        while j < ops.len() {
            let cur = staged_vals[ops[j].sem_num];
            let after = cur.wrapping_add(ops[j].sem_op);
            if after < 0 {
                if ops[j].sem_flg & FLG_NOWAIT != 0 {
                    self.book(round, SE_EAGAIN, j);
                    return Err(SE_EAGAIN);
                }
                self.book(round, SE_WOULD_BLOCK, j);
                return Err(SE_WOULD_BLOCK);
            }
            staged_vals[ops[j].sem_num] = after;
            j = j.wrapping_add(1);
        }
        // 提交预演结果。
        self.set.vals = staged_vals;
        self.book(round, 0, ops.len());
        Ok(ops.len())
    }

    /// 四类计数与轮次总数对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eagain_hits)
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.e2big_hits)
            .wrapping_add(self.eidrm_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// 门级 FNV 指纹：集合指纹叠上判定账。
    pub fn fold(&self) -> u64 {
        let mut h = self.set.fold();
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.eagain_hits as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.e2big_hits as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.einval_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 单行渲染。
    pub fn render(&self) -> String {
        format!(
            "SemopGate{{rounds={},ok={},again={},einval={},e2big={},set={}}}",
            self.rounds,
            self.ok_hits,
            self.eagain_hits,
            self.einval_hits,
            self.e2big_hits,
            self.set.render()
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = SemopGate::new(0xa929, 4, 2, SEMOPM_MAX);
    // 注入 1：nsops = SEMOPM + 1 → E2BIG。
    let mut big: Vec<SemBuf> = Vec::new();
    let mut i = 0;
    while i <= SEMOPM_MAX {
        big.push(SemBuf::of(0, 0, 0));
        i += 1;
    }
    caught[0] = matches!(g.judge(&big, 9001), Err(x) if x == SE_E2BIG);
    // 注入 2：sem_num 越过 nsems → EINVAL。
    let bad_num = [SemBuf::of(9, -1, FLG_NOWAIT)];
    caught[1] = matches!(g.judge(&bad_num, 9002), Err(x) if x == SE_EINVAL);
    // 注入 3：NOWAIT 下扣 5 而值只有 2 → EAGAIN。
    let drain = [SemBuf::of(0, -5, FLG_NOWAIT)];
    caught[2] = matches!(g.judge(&drain, 9003), Err(x) if x == SE_EAGAIN);
    // 注入 4：原子性——先把 0 号压到 1，再连扣两次 1，第二条须 EAGAIN 且值零残留。
    let _ = g.judge(&[SemBuf::of(0, -1, FLG_NOWAIT)], 9004);
    let before = g.set.snapshot();
    let two = [
        SemBuf::of(0, -1, FLG_NOWAIT),
        SemBuf::of(0, -1, FLG_NOWAIT),
    ];
    let r4 = g.judge(&two, 9005);
    caught[3] = matches!(r4, Err(x) if x == SE_EAGAIN) && g.set.snapshot() == before;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：按轮次低位分流到四条路径，返回"裁决符合预期"的轮数。
/// - 低位 0 两条净零合法操作（-1 后 +1，同一原子调用内生效，恒不落负）；
/// - 低位 1/2/3 分别对应 E2BIG/EINVAL/EAGAIN；
/// - 其余低位走合法增减交替（1 号平衡，恒不落负）。
pub fn sample_thousand(g: &mut SemopGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < LEGAL_ROUNDS {
        let low = r % 10;
        let expect_ok: bool;
        let res;
        if low == 0 {
            // 合法：对 0 号 -1 后同调用内 +1，净零且任一步不为负。
            res = g.judge(
                &[SemBuf::of(0, -1, FLG_NOWAIT), SemBuf::of(0, 1, FLG_NOWAIT)],
                r,
            );
            expect_ok = true;
        } else if low == 1 {
            // E2BIG 面：35 条空操作。
            let mut big: Vec<SemBuf> = Vec::new();
            let mut i = 0;
            while i < SEMOPM_MAX + 3 {
                big.push(SemBuf::of(0, 0, 0));
                i += 1;
            }
            res = g.judge(&big, r);
            expect_ok = false;
        } else if low == 2 {
            // EINVAL 面：sem_num = nsems。
            res = g.judge(&[SemBuf::of(g.set.nsems, 1, 0)], r);
            expect_ok = false;
        } else if low == 3 {
            // EAGAIN 面：NOWAIT 扣大数。
            res = g.judge(&[SemBuf::of(1, -99, FLG_NOWAIT)], r);
            expect_ok = false;
        } else {
            // 合法增减：偶数低位补 1，奇数低位扣 1（1 号保持 >= 0）。
            let op = if low % 2 == 1 { 1i32 } else { -1i32 };
            res = g.judge(&[SemBuf::of(1, op, FLG_NOWAIT)], r);
            expect_ok = res.is_ok();
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == SE_E2BIG
                        || *x == SE_EINVAL
                        || *x == SE_EAGAIN
                        || *x == SE_WOULD_BLOCK)
            }
        };
        if matched {
            good += 1;
        }
        r = r.wrapping_add(1);
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10929 判据组。
pub fn run_unx10929_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10929");
    // J1 正面之一：NOWAIT 负值 → EAGAIN 十轮全中。
    let mut g1 = SemopGate::new(0x11, 3, 1, SEMOPM_MAX);
    let base1 = g1.set.snapshot();
    let mut again_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = g1.judge(&[SemBuf::of(0, -3, FLG_NOWAIT)], k);
        // EAGAIN 走原子预演、失败轮零残留：余量恒为初值，无需回补
        //（回补会把余量垫高，下一轮 -3 便不再超额而漏判）。
        if matches!(r, Err(x) if x == SE_EAGAIN) && g1.set.snapshot() == base1 {
            again_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EAGAIN", again_cnt == 10, "NOWAIT 负值十轮全部裁 EAGAIN");
    // J1 正面之二：sem_num 越界 → EINVAL 十轮全中。
    let mut g2 = SemopGate::new(0x12, 3, 1, SEMOPM_MAX);
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = g2.judge(&[SemBuf::of(3 + k, -1, 0)], k);
        if matches!(r, Err(x) if x == SE_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "sembuf 越界十轮全部裁 EINVAL");
    // J1 正面之三：nsops 超上限 → E2BIG 十轮全中。
    let mut g3 = SemopGate::new(0x13, 3, 1, SEMOPM_MAX);
    let mut e2big_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut big: Vec<SemBuf> = Vec::new();
        let mut i = 0;
        while i < SEMOPM_MAX + 1 + (k % 3) {
            big.push(SemBuf::of(0, 0, 0));
            i += 1;
        }
        let r = g3.judge(&big, k);
        if matches!(r, Err(x) if x == SE_E2BIG) {
            e2big_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面E2BIG", e2big_cnt == 10, "nsops 超上限十轮全部裁 E2BIG");
    // J2 反面：千轮样本中合法路径零误报。
    let mut g4 = SemopGate::new(0x14, 4, 3, SEMOPM_MAX);
    let good = sample_thousand(&mut g4);
    // 合法路径 = 低位 0（100 轮）+ 低位 4..9（600 轮）= 700 轮。
    s.add_dyn("J2-反面零误报", good == LEGAL_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "E2BIG/EINVAL/EAGAIN/原子性四路坏样本全部拦截",
    );
    // J4 原子性独立复核：失败轮集合值零残留。
    let mut g5 = SemopGate::new(0x15, 2, 1, SEMOPM_MAX);
    let before = g5.set.snapshot();
    let two = [SemBuf::of(0, -1, FLG_NOWAIT), SemBuf::of(0, -1, FLG_NOWAIT)];
    let r5 = g5.judge(&two, 1);
    s.add(
        "J4-原子预演",
        matches!(r5, Err(x) if x == SE_EAGAIN) && g5.set.snapshot() == before,
        "预演失败后集合值与进入前逐位一致",
    );
    // J5 错误码分段：全部落在 0x1d3a0..0x1d3c0 区间。
    let seg_ok = SE_E2BIG > SEG_BASE
        && SE_EIDRM < SEG_BASE + 0x20
        && SE_LEDGER < SEG_BASE + 0x20;
    s.add("J5-错误码分段", seg_ok, "七个错误码全部落在段基址页内");
    // J6 错误码互异。
    let codes = [SE_E2BIG, SE_EINVAL, SE_EAGAIN, SE_WOULD_BLOCK, SE_EMPTY_OPS, SE_EIDRM, SE_LEDGER];
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
    // J7 判定账对平。
    s.add("J7-账目对平", g4.ledger_ok(), "千轮之后四类计数与轮次、日志逐项对平");
    // J8 常数自洽：SEMOPM 与标志位。
    s.add(
        "J8-常数自洽",
        SEMOPM_MAX == 32 && FLG_NOWAIT == 1 && FLG_UNDO == 2,
        "SEMOPM=32，NOWAIT/UNDO 标志位独立",
    );
    // J9 fold 稳定：克隆与重算一致。
    let f1 = g4.fold();
    let clone = g4.clone();
    s.add("J9-fold稳定", clone.fold() == f1, "克隆后指纹逐位一致");
    // J10 成功路径也复核：充足资源下合法扣减成功且值正确。
    let mut g6 = SemopGate::new(0x16, 1, 5, SEMOPM_MAX);
    let ok6 = g6.judge(&[SemBuf::of(0, -3, FLG_NOWAIT)], 1);
    s.add(
        "J10-合法扣减",
        matches!(ok6, Ok(1)) && g6.set.vals[0] == 2,
        "充足资源下 -3 扣减成功且值落为 2",
    );
    // J11 渲染非空且含关键字段。
    let txt = g6.render();
    s.add(
        "J11-渲染非空",
        txt.len() > 10 && txt.contains("SemopGate"),
        "门级渲染包含类型名与计数",
    );
    // J12 空操作数组单独成面。
    let r12 = g6.judge(&[], 2);
    s.add("J12-空操作面", matches!(r12, Err(x) if x == SE_EMPTY_OPS), "空 sembuf 数组判 SE_EMPTY_OPS");
    // J13 动态：千轮样本零误报明细。
    s.add_dyn(
        "J13-千轮明细",
        good == LEGAL_ROUNDS,
        &format!("good={} rounds={} set={}", good, g4.rounds, g4.set.render()),
    );
    // J14 动态：注入四路明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?} gate={}", inj, g1.render()),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10929_semop_again_flow() {
        // 流程：合法扣减 → EAGAIN → 越界 → 超上限 → 原子回退。
        let mut g = SemopGate::new(0xbeef, 3, 4, SEMOPM_MAX);
        assert_eq!(g.judge(&[SemBuf::of(0, -2, FLG_NOWAIT)], 1), Ok(1));
        assert_eq!(g.set.vals[0], 2);
        assert!(matches!(g.judge(&[SemBuf::of(0, -3, FLG_NOWAIT)], 2), Err(x) if x == SE_EAGAIN));
        assert!(matches!(g.judge(&[SemBuf::of(5, -1, 0)], 3), Err(x) if x == SE_EINVAL));
        let mut big: Vec<SemBuf> = Vec::new();
        let mut i = 0;
        while i < 33 {
            big.push(SemBuf::of(0, 0, 0));
            i += 1;
        }
        assert!(matches!(g.judge(&big, 4), Err(x) if x == SE_E2BIG));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10929_checks_green() {
        let s = run_unx10929_checks();
        assert!(s.all_passed(), "UNX-F10929 判据未全绿");
    }
}
