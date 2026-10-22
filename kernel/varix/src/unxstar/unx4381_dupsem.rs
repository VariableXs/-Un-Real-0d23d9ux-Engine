// unx4381 —— UNX-F4381 dup 基础语义
// 判据原文 UNX-F4381-J1：dup 返回最小可用新 fd 且与旧 fd 指向同一 file 对象
// （f_count+1 守恒账），新旧 fd 独立位图位（1,000 次映射核对）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xD180 独占（0xD180~0xD1FF）。
//
// 设计（纯功能 no_std）：
//   * fd 表模型：位图（占用位）+ fd → FileRef（引用计数 f_count）；
//   * dup 语义：最小可用新 fd + 同一 file 对象 + f_count+1 守恒账；
//   * 独立位图位：dup 后新旧 fd 各占一位（close 其一不影响另一）；
//   * 1,000 次映射核对：确定性 LCG 千次 dup/close 循环账平；
//   * 注入核：fd 越界/重复关闭/计数漂移/封表后 dup 四类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xD180 独占） ─────────────────────────────────────────────
pub const XC_DUP_OOB: u32 = 0xD181; // fd 越界
pub const XC_DUP_DBL: u32 = 0xD182; // 重复关闭
pub const XC_DUP_CNT: u32 = 0xD183; // 计数漂移
pub const XC_DUP_SEAL: u32 = 0xD184; // 封表后 dup

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// fd 表容量。
pub const MAX_FD: usize = 64;

/// file 对象（引用计数共享目标）。
pub struct FileObj {
    pub id: u64,
    pub f_count: u64,
}

/// fd 表。
pub struct FdTable {
    bitmap: [bool; MAX_FD],
    /// fd → 共享 file 对象索引（files 池下标）。
    map: [Option<usize>; MAX_FD],
    files: Vec<FileObj>,
    open_cnt: u64,
    dup_cnt: u64,
    close_cnt: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl FdTable {
    pub fn new() -> Self {
        FdTable {
            bitmap: [false; MAX_FD],
            map: [None; MAX_FD],
            files: Vec::new(),
            open_cnt: 0,
            dup_cnt: 0,
            close_cnt: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 打开新 file（分配最小可用 fd）。
    pub fn open(&mut self, file_id: u64) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_DUP_SEAL, "封表后 open"));
            return Err(XC_DUP_SEAL);
        }
        let fd = self.alloc_min()?;
        let idx = self.files.len();
        self.files.push(FileObj { id: file_id, f_count: 1 });
        self.map[fd] = Some(idx);
        self.open_cnt += 1;
        Ok(fd)
    }

    /// 最小可用 fd。
    fn alloc_min(&mut self) -> Result<usize, u32> {
        for i in 0..MAX_FD {
            if !self.bitmap[i] {
                self.bitmap[i] = true;
                return Ok(i);
            }
        }
        self.rejects.push((XC_DUP_OOB, "fd 耗尽"));
        Err(XC_DUP_OOB)
    }

    /// dup：最小可用新 fd + 同一 file 对象 + f_count+1。
    pub fn dup(&mut self, old: usize) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_DUP_SEAL, "封表后 dup"));
            return Err(XC_DUP_SEAL);
        }
        if old >= MAX_FD || !self.bitmap[old] {
            self.rejects.push((XC_DUP_OOB, "旧 fd 无效"));
            return Err(XC_DUP_OOB);
        }
        let new_fd = self.alloc_min()?;
        let idx = self.map[old];
        self.map[new_fd] = idx;
        if let Some(i) = idx {
            self.files[i].f_count += 1;
        }
        self.dup_cnt += 1;
        Ok(new_fd)
    }

    /// close：清位 + f_count−1（重复关闭拒）。
    pub fn close(&mut self, fd: usize) -> Result<u64, u32> {
        if fd >= MAX_FD || !self.bitmap[fd] {
            self.rejects.push((XC_DUP_DBL, "重复关闭/无效 fd"));
            return Err(XC_DUP_DBL);
        }
        self.bitmap[fd] = false;
        let ret = if let Some(i) = self.map[fd].take() {
            self.files[i].f_count -= 1;
            self.files[i].f_count
        } else {
            0
        };
        self.close_cnt += 1;
        Ok(ret)
    }

    /// f_count 守恒账：Σ f_count == open + dup − close。
    pub fn fcount_conserved(&self) -> bool {
        let sum: u64 = self.files.iter().map(|f| f.f_count).sum();
        let occupied: u64 = self.bitmap.iter().filter(|b| **b).count() as u64;
        sum == occupied && sum == self.open_cnt + self.dup_cnt - self.close_cnt
    }

    /// 同一 file 对象核对：两 fd 指向同 file。
    pub fn same_file(&self, a: usize, b: usize) -> bool {
        a < MAX_FD && b < MAX_FD && self.map[a].is_some() && self.map[a] == self.map[b]
    }

    /// 独立位图位：两 fd 各占一位。
    pub fn bits_independent(&self, a: usize, b: usize) -> bool {
        self.bitmap[a] && self.bitmap[b]
    }

    /// 关其一对另一无影响。
    pub fn close_one_other_survives(&mut self, a: usize, b: usize) -> bool {
        let f_a = self.map[a];
        let _ = self.close(a);
        self.bitmap[b] && self.map[b] == f_a
    }

    pub fn open_count(&self) -> u64 {
        self.open_cnt
    }
    pub fn dup_count(&self) -> u64 {
        self.dup_cnt
    }
    pub fn close_count(&self) -> u64 {
        self.close_cnt
    }
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_DUP_SEAL);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 1,000 次映射核对：千次随机 dup/close 循环，账全程平。
pub fn mapping_1000(seed: u64) -> (bool, u64) {
    let mut rng = Lcg(seed | 1);
    let mut t = FdTable::new();
    let _ = t.open(1); // fd0
    let mut live: Vec<usize> = vec![0];
    let mut ops = 0;
    for i in 0..1000 {
        if i % 3 != 2 || live.len() <= 1 {
            // dup 随机活 fd
            let pick = (rng.next() % live.len() as u64) as usize;
            let old = live[pick];
            if let Ok(nf) = t.dup(old) {
                live.push(nf);
                ops += 1;
            }
        } else {
            // close 随机活 fd（保底 fd0）
            let pick = 1 + (rng.next() % (live.len() - 1) as u64) as usize;
            let fd = live.swap_remove(pick);
            if t.close(fd).is_ok() {
                ops += 1;
            }
        }
        if !t.fcount_conserved() {
            return (false, ops);
        }
    }
    (t.fcount_conserved(), ops)
}

/// 注入核：fd 越界/重复关闭/计数漂移/封表后 dup 四类必抓。
pub struct DupInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl DupInjectCore {
    pub fn new() -> Self {
        DupInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_fd_oob(&mut self) {
        self.total += 1;
        let mut t = FdTable::new();
        let d = t.dup(MAX_FD + 5);
        let c = t.close(MAX_FD + 5);
        if d.is_err() && c.is_err() && t.reject_count() == 2 {
            self.caught += 1;
        }
    }
    pub fn inject_double_close(&mut self) {
        self.total += 1;
        let mut t = FdTable::new();
        let fd = t.open(1).unwrap_or(0);
        let c1 = t.close(fd);
        let c2 = t.close(fd);
        if c1.is_ok() && c2.is_err() && t.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_count_drift(&mut self) {
        self.total += 1;
        let mut t = FdTable::new();
        let fd = t.open(9).unwrap_or(0);
        let _ = t.dup(fd);
        // 漂移注入：篡改 f_count
        t.files[0].f_count += 1;
        if !t.fcount_conserved() {
            self.caught += 1;
        }
    }
    pub fn inject_dup_after_seal(&mut self) {
        self.total += 1;
        let mut t = FdTable::new();
        let fd = t.open(1).unwrap_or(0);
        let _ = t.seal();
        let r = t.dup(fd);
        if r.is_err() && t.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx4381_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx4381");

    // C81-01 dup 返回最小可用新 fd
    {
        let mut t = FdTable::new();
        let fd0 = t.open(1).unwrap_or(0);
        let fd1 = t.open(2).unwrap_or(0);
        let nd = t.dup(fd0).unwrap_or(0);
        let ok = fd0 == 0 && fd1 == 1 && nd == 2;
        s.add("C81-01-最小 fd", ok, if ok { "" } else { "分配非最小" });
    }
    // C81-02 同一 file 对象 + f_count 守恒
    {
        let mut t = FdTable::new();
        let fd0 = t.open(7).unwrap_or(0);
        let _ = t.dup(fd0);
        let ok = t.same_file(fd0, 1)
            && t.file_count() == 1
            && t.fcount_conserved()
            && t.dup_count() == 1;
        s.add("C81-02-同对象", ok, if ok { "" } else { "对象分裂/计数漂移" });
    }
    // C81-03 独立位图位 + 关其一不影响另一
    {
        let mut t = FdTable::new();
        let fd0 = t.open(3).unwrap_or(0);
        let _ = t.dup(fd0);
        let bits = t.bits_independent(0, 1);
        let surv = t.close_one_other_survives(0, 1);
        let cons = t.fcount_conserved();
        s.add(
            "C81-03-位独立",
            bits && surv && cons,
            if bits && surv { "" } else { "位联动误伤" },
        );
    }
    // C81-04 1,000 次映射核对全程账平
    {
        let (ok, ops) = mapping_1000(0xF00D);
        s.add(
            "C81-04-千次账平",
            ok && ops >= 1000 - 400,
            if ok { "" } else { "账平断" },
        );
    }
    // C81-05 fd 耗尽闸（64 全占后 open/dup 拒）
    {
        let mut t = FdTable::new();
        let fd0 = t.open(1).unwrap_or(0);
        let mut filled = 1;
        while t.dup(fd0).is_ok() {
            filled += 1;
            if filled > MAX_FD {
                break;
            }
        }
        let ok = filled == MAX_FD && t.reject_count() >= 1;
        s.add("C81-05-耗尽闸", ok, if ok { "" } else { "耗尽失守" });
    }
    // C81-06 封表拒 dup
    {
        let mut t = FdTable::new();
        let fd0 = t.open(1).unwrap_or(0);
        let _ = t.seal();
        let dup = t.seal();
        let r = t.dup(fd0);
        let ok = dup.is_err() && r.is_err() && t.reject_count() == 2;
        s.add("C81-06-封表拒", ok, if ok { "" } else { "封表失守" });
    }
    // C81-07 注入核 4/4
    {
        let mut inj = DupInjectCore::new();
        inj.inject_fd_oob();
        inj.inject_double_close();
        inj.inject_count_drift();
        inj.inject_dup_after_seal();
        s.add(
            "C81-07-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
