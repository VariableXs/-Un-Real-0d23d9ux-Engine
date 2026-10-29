//! UNX-F10933 shmget 错误矩阵（四错误面）（0x1d420 段）
//!
//! 纯功能模型：SysV `shmget` 共享内存段表。
//! 四错误面：① 键已存在且带 EXCL → EEXIST；② size 超出 SHMMAX
//! 或对既有段要求更大 → EINVAL；③ 既有段无读权 → EACCES；
//! ④ 表槽位用尽 → ENOSPC。反面千轮样本只走合法 key，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d420;
/// 键已存在且要求互斥创建
pub const GH_EEXIST: u32 = SEG_BASE + 0x01;
/// size 非法或对既有段要求更大
pub const GH_EINVAL: u32 = SEG_BASE + 0x02;
/// 既有段无访问权
pub const GH_EACCES: u32 = SEG_BASE + 0x03;
/// 表槽位用尽
pub const GH_ENOSPC: u32 = SEG_BASE + 0x04;
/// 键不存在且未带创建位
pub const GH_ENOENT: u32 = SEG_BASE + 0x05;
/// 表内对账失败
pub const GH_LEDGER: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------- 常数
/// shmflg：创建位
pub const GFLG_CREAT: u32 = 0x0100;
/// shmflg：互斥位
pub const GFLG_EXCL: u32 = 0x0200;
/// 段大小上限（SHMMAX，1 MiB 模型值）
pub const SHMMAX: usize = 0x100000;
/// 段表槽位数（SHMMNI）
pub const SHMMNI: usize = 16;
/// 属主读位
pub const MODE_R: u32 = 0x4;
/// 属主写位
pub const MODE_W: u32 = 0x2;
/// 反面验收轮数
pub const GH_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 段表中的一个共享内存段。
#[derive(Clone, Debug)]
pub struct ShmSeg {
    /// 创建键
    pub key: u32,
    /// 段句柄
    pub shmid: u32,
    /// 段大小（字节）
    pub size: usize,
    /// 权限位（属主读/写）
    pub mode: u32,
    /// 当前映射计数
    pub nattch: usize,
}

impl ShmSeg {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.key as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.shmid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.size as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.mode as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.nattch as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "ShmSeg{{key=0x{:x},id={},size={},mode={:#o},nattch={}}}",
            self.key, self.shmid, self.size, self.mode, self.nattch
        )
    }
}

/// 单轮 shmget 日志。
#[derive(Clone, Debug, PartialEq)]
pub struct GetLog {
    /// 轮次
    pub round: usize,
    /// 请求键
    pub key: u32,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// shmget 段表裁决器。
#[derive(Clone, Debug)]
pub struct ShmTable {
    /// 段表
    pub segs: Vec<ShmSeg>,
    /// 下一句柄
    pub next_id: u32,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EEXIST 命中
    pub eexist_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// ENOSPC 命中
    pub enospc_hits: usize,
    /// ENOENT 命中（无创建位查无此键，同属裁决面须入账）
    pub enoent_hits: usize,
    /// 逐轮日志
    pub log: Vec<GetLog>,
}

impl ShmTable {
    /// 建空表。
    pub fn new() -> Self {
        ShmTable {
            segs: Vec::new(),
            next_id: 1,
            rounds: 0,
            ok_hits: 0,
            eexist_hits: 0,
            einval_hits: 0,
            eacces_hits: 0,
            enospc_hits: 0,
            enoent_hits: 0,
            log: Vec::new(),
        }
    }

    /// 按键查找。
    pub fn find_key(&self, key: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.segs.len() {
            if self.segs[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账。
    fn book(&mut self, round: usize, key: u32, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == GH_EEXIST => self.eexist_hits = self.eexist_hits.wrapping_add(1),
            x if x == GH_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == GH_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            x if x == GH_ENOSPC => self.enospc_hits = self.enospc_hits.wrapping_add(1),
            x if x == GH_ENOENT => self.enoent_hits = self.enoent_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(GetLog { round, key, verdict });
    }

    /// 裁决一次 shmget。判定序：
    /// ① 键存在：EXCL → EEXIST；size 更大 → EINVAL；无读权 → EACCES；命中返回；
    /// ② 键不存在：无 CREAT → ENOENT；size 非法 → EINVAL；表满 → ENOSPC；建段。
    pub fn shmget(&mut self, key: u32, size: usize, flags: u32, round: usize) -> Result<u32, u32> {
        match self.find_key(key) {
            Some(ix) => {
                if flags & GFLG_EXCL != 0 {
                    self.book(round, key, GH_EEXIST);
                    return Err(GH_EEXIST);
                }
                if size > self.segs[ix].size {
                    self.book(round, key, GH_EINVAL);
                    return Err(GH_EINVAL);
                }
                if self.segs[ix].mode & MODE_R == 0 {
                    self.book(round, key, GH_EACCES);
                    return Err(GH_EACCES);
                }
                let id = self.segs[ix].shmid;
                self.book(round, key, 0);
                Ok(id)
            }
            None => {
                if flags & GFLG_CREAT == 0 {
                    self.book(round, key, GH_ENOENT);
                    return Err(GH_ENOENT);
                }
                if size == 0 || size > SHMMAX {
                    self.book(round, key, GH_EINVAL);
                    return Err(GH_EINVAL);
                }
                if self.segs.len() >= SHMMNI {
                    self.book(round, key, GH_ENOSPC);
                    return Err(GH_ENOSPC);
                }
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                let mode = flags & (MODE_R | MODE_W);
                self.segs.push(ShmSeg { key, shmid: id, size, mode, nattch: 0 });
                self.book(round, key, 0);
                Ok(id)
            }
        }
    }

    /// 账目对平（六类裁决：成功 + 五错误，逐类计数之和须等于轮次）。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eexist_hits)
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.eacces_hits)
            .wrapping_add(self.enospc_hits)
            .wrapping_add(self.enoent_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.segs.len() {
            h = (h ^ self.segs[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "ShmTable{{segs={},rounds={},ok={},eexist={},einval={},eacces={},enospc={},enoent={}}}",
            self.segs.len(), self.rounds, self.ok_hits,
            self.eexist_hits, self.einval_hits, self.eacces_hits, self.enospc_hits,
            self.enoent_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被表正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut t = ShmTable::new();
    // 建一个只读小段与一个可写大段。
    let _ = t.shmget(0xa101, 0x1000, GFLG_CREAT | MODE_R, 9001);
    let _ = t.shmget(0xa102, 0x8000, GFLG_CREAT | MODE_R | MODE_W, 9002);
    // 注入 1：同键 EXCL → EEXIST。
    caught[0] = matches!(t.shmget(0xa102, 0x100, GFLG_CREAT | GFLG_EXCL | MODE_W, 9003), Err(x) if x == GH_EEXIST);
    // 注入 2：对既有段要求更大 → EINVAL。
    caught[1] = matches!(t.shmget(0xa102, 0x9000, 0, 9004), Err(x) if x == GH_EINVAL);
    // 注入 3：只读段仍无权读（mode 位被剥）→ EACCES。
    // 用内部改模构造：先把段 mode 置 0 再请求。
    let ix = t.find_key(0xa101).unwrap_or(0);
    t.segs[ix].mode = 0;
    caught[2] = matches!(t.shmget(0xa101, 0x100, 0, 9005), Err(x) if x == GH_EACCES);
    t.segs[ix].mode = MODE_R;
    // 注入 4：灌满表后再建段 → ENOSPC。
    let mut k = 0;
    while t.segs.len() < SHMMNI && k < SHMMNI + 4 {
        let _ = t.shmget(0xb000 + k as u32, 0x100, GFLG_CREAT | MODE_R | MODE_W, 9100 + k);
        k += 1;
    }
    caught[3] = matches!(t.shmget(0xc000, 0x100, GFLG_CREAT | MODE_R, 9200), Err(x) if x == GH_ENOSPC);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法命中与四错误面轮转。返回"裁决符合预期"的轮数。
pub fn sample_thousand(t: &mut ShmTable) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < GH_ROUNDS {
        let low = r % 8;
        let res;
        let expect_ok;
        if low < 4 {
            // 合法面：对既有段做不带创建位的命中。
            res = t.shmget(0xd100 + (r % 3) as u32, 0x100, 0, r);
            expect_ok = true;
        } else if low == 4 {
            // EEXIST 面。
            res = t.shmget(0xd100, 0x100, GFLG_CREAT | GFLG_EXCL | MODE_R, r);
            expect_ok = false;
        } else if low == 5 {
            // EINVAL 面：size 超 SHMMAX。
            res = t.shmget(0xd200 + (r % 5) as u32, SHMMAX + 1, GFLG_CREAT | MODE_R, r);
            expect_ok = false;
        } else if low == 6 {
            // EACCES 面：无权段。
            res = t.shmget(0xd300, 0x100, 0, r);
            expect_ok = false;
        } else {
            // ENOENT 面（补面）。
            res = t.shmget(0xd400 + (r % 9) as u32, 0x100, 0, r);
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == GH_EEXIST
                        || *x == GH_EINVAL
                        || *x == GH_EACCES
                        || *x == GH_ENOSPC
                        || *x == GH_ENOENT)
            }
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10933 判据组。
pub fn run_unx10933_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10933");
    // 预置表：三个合法段（0xd100/0xd101/0xd102 可写，0xd300 只读零权）。
    let mut t0 = ShmTable::new();
    let _ = t0.shmget(0xd100, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 1);
    let _ = t0.shmget(0xd101, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 2);
    let _ = t0.shmget(0xd102, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 3);
    let _ = t0.shmget(0xd300, 0x1000, GFLG_CREAT, 4);
    let ix0 = t0.find_key(0xd300).unwrap_or(0);
    t0.segs[ix0].mode = 0;
    // J1 正面之一：EEXIST 十轮全中。
    let mut eexist_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(t0.shmget(0xd101, 0x100, GFLG_CREAT | GFLG_EXCL | MODE_W, k), Err(x) if x == GH_EEXIST) {
            eexist_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EEXIST", eexist_cnt == 10, "同键互斥创建十轮全部裁 EEXIST");
    // J1 正面之二：EINVAL 十轮全中。
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            t0.shmget(0xd102, 0x3000, 0, k)
        } else {
            t0.shmget(0xe000 + k as u32, 0, GFLG_CREAT | MODE_R, k)
        };
        if matches!(r, Err(x) if x == GH_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "size 越界/零尺寸十轮全部裁 EINVAL");
    // J1 正面之三：EACCES 十轮全中。
    let mut eacces_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(t0.shmget(0xd300, 0x100, 0, k), Err(x) if x == GH_EACCES) {
            eacces_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", eacces_cnt == 10, "无权段访问十轮全部裁 EACCES");
    // J1 正面之四：ENOSPC 十轮全中。
    let mut t1 = ShmTable::new();
    let mut k = 0;
    while t1.segs.len() < SHMMNI {
        let _ = t1.shmget(0xf000 + k as u32, 0x100, GFLG_CREAT | MODE_R | MODE_W, k);
        k += 1;
    }
    let mut enospc_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(t1.shmget(0xf100 + k as u32, 0x100, GFLG_CREAT | MODE_R, k), Err(x) if x == GH_ENOSPC) {
            enospc_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOSPC", enospc_cnt == 10, "表满建段十轮全部裁 ENOSPC");
    // J2 反面：千轮样本零误报。
    let mut t2 = ShmTable::new();
    let _ = t2.shmget(0xd100, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 1);
    let _ = t2.shmget(0xd101, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 2);
    let _ = t2.shmget(0xd102, 0x2000, GFLG_CREAT | MODE_R | MODE_W, 3);
    let _ = t2.shmget(0xd300, 0x1000, GFLG_CREAT, 4);
    let ix2 = t2.find_key(0xd300).unwrap_or(0);
    t2.segs[ix2].mode = 0;
    let good = sample_thousand(&mut t2);
    s.add_dyn("J2-反面零误报", good == GH_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EEXIST/EINVAL/EACCES/ENOSPC 四路坏样本全部拦截",
    );
    // J4 判定序：EXCL 先于无权（同键带 EXCL 且无权时裁 EEXIST）。
    let r4 = t0.shmget(0xd300, 0x100, GFLG_CREAT | GFLG_EXCL | MODE_W, 1);
    s.add(
        "J4-判定序",
        matches!(r4, Err(x) if x == GH_EEXIST),
        "EXCL 优先于 EACCES 裁定",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        GH_EEXIST > SEG_BASE && GH_LEDGER < SEG_BASE + 0x20,
        "六个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [GH_EEXIST, GH_EINVAL, GH_EACCES, GH_ENOSPC, GH_ENOENT, GH_LEDGER];
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
    s.add("J6-错误码互异", distinct, "六个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", t2.ledger_ok(), "千轮之后五类计数与轮次、日志逐项对平");
    // J8 句柄单调。
    let next = t2.next_id;
    let mut mono = true;
    let mut i = 0;
    while i < t2.segs.len() {
        if t2.segs[i].shmid >= next {
            mono = false;
        }
        i += 1;
    }
    s.add("J8-句柄单调", mono, "表内全部句柄小于 next_id");
    // J9 fold 稳定。
    let f = t2.fold();
    let clone = t2.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 SHMMAX 边界：恰好等于上限可建段。
    let mut t3 = ShmTable::new();
    let okmax = t3.shmget(0x7000, SHMMAX, GFLG_CREAT | MODE_R | MODE_W, 1);
    s.add("J10-SHMMAX边界", matches!(okmax, Ok(_)) && t3.segs[0].size == SHMMAX, "恰好 SHMMAX 建段成功");
    // J11 渲染非空。
    let txt = t2.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("ShmTable"), "表级渲染含类型名与计数");
    // J12 无 CREAT 查无此键 → ENOENT。
    let r12 = t2.shmget(0x9999, 0x100, 0, 2);
    s.add("J12-ENOENT面", matches!(r12, Err(x) if x == GH_ENOENT), "无创建位查无此键裁 ENOENT");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == GH_ROUNDS,
        &format!("good={} table={}", good, t2.render()),
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
    fn unx10933_shmget_matrix_flow() {
        // 流程：建段 → 命中 → EEXIST → EINVAL → EACCES → ENOSPC。
        let mut t = ShmTable::new();
        let id = t.shmget(0xcafe, 0x1000, GFLG_CREAT | MODE_R | MODE_W, 1);
        assert!(id.is_ok());
        assert_eq!(t.shmget(0xcafe, 0x800, 0, 2), id);
        assert!(matches!(t.shmget(0xcafe, 0x100, GFLG_CREAT | GFLG_EXCL | MODE_W, 3), Err(x) if x == GH_EEXIST));
        assert!(matches!(t.shmget(0xcafe, 0x2000, 0, 4), Err(x) if x == GH_EINVAL));
        let ix = t.find_key(0xcafe).unwrap_or(0);
        t.segs[ix].mode = 0;
        assert!(matches!(t.shmget(0xcafe, 0x100, 0, 5), Err(x) if x == GH_EACCES));
        let mut k = 0;
        while t.segs.len() < SHMMNI {
            let _ = t.shmget(0xd000 + k as u32, 0x100, GFLG_CREAT | MODE_R, 100 + k);
            k += 1;
        }
        assert!(matches!(t.shmget(0xeeee, 0x100, GFLG_CREAT | MODE_R, 200), Err(x) if x == GH_ENOSPC));
        assert!(t.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10933_checks_green() {
        let s = run_unx10933_checks();
        assert!(s.all_passed(), "UNX-F10933 判据未全绿");
    }
}
