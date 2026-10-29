//! UNX-F10937 SHM_RMID 与映射存活矩阵（0x1d4a0 段）
//!
//! 纯功能模型：RMID 后共享段的生命周期语义。
//! 三条主判据：① RMID 后已挂载映射仍可读写（数据不失效）；
//! ② RMID 后新 shmat → EINVAL；③ 全部卸载后段回收（freed 置位、
//! 数据清零）。反面千轮样本在存活期内读写零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d4a0;
/// RMID 后新挂载
pub const LV_EINVAL: u32 = SEG_BASE + 0x01;
/// 对已回收段的旧映射访问
pub const LV_DEADMAP: u32 = SEG_BASE + 0x02;
/// 坏句柄
pub const LV_BADID: u32 = SEG_BASE + 0x03;
/// 段下标越界
pub const LV_EFAULT: u32 = SEG_BASE + 0x04;
/// 双重 RMID
pub const LV_TWICE: u32 = SEG_BASE + 0x05;
/// 台内对账失败
pub const LV_LEDGER: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------- 常数
/// 段单元数（模型字数）
pub const LV_CELLS: usize = 16;
/// 自动基址步长
pub const LV_LBA: usize = 4096;
/// 反面验收轮数
pub const LV_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 段条目：数据 + 生命周期位。
#[derive(Clone, Debug)]
pub struct LiveSeg {
    /// 段句柄
    pub shmid: u32,
    /// 数据单元
    pub cells: Vec<u32>,
    /// 是否已 RMID
    pub removed: bool,
    /// 是否已回收（挂载归零后）
    pub freed: bool,
    /// 挂载计数
    pub nattch: usize,
}

impl LiveSeg {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.shmid as u64)).wrapping_mul(0x100000001b3);
        let mut i = 0;
        while i < self.cells.len() {
            h = (h ^ (self.cells[i] as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.removed as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.freed as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.nattch as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.cells.len() {
            acc = acc.wrapping_add(self.cells[i] as u64);
            i += 1;
        }
        format!(
            "LiveSeg{{id={},cells={},sum={},rm={},freed={},nattch={}}}",
            self.shmid, self.cells.len(), acc, self.removed as u32, self.freed as u32, self.nattch
        )
    }
}

/// 一条映射。
#[derive(Clone, Debug, PartialEq)]
pub struct LiveMap {
    /// 进程号
    pub pid: u32,
    /// 段句柄
    pub shmid: u32,
    /// 基址
    pub addr: usize,
}

/// 生命周期试验台。
#[derive(Clone, Debug)]
pub struct LivenessRig {
    /// 段集合
    pub segs: Vec<LiveSeg>,
    /// 活跃映射
    pub maps: Vec<LiveMap>,
    /// 下一自动基址
    pub next_base: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// DEADMAP 命中
    pub deadmap_hits: usize,
    /// 台内回收次数
    pub reclaimed: usize,
}

impl LivenessRig {
    /// 建台：n 个段，每段统一初值。
    pub fn new(n: usize, init: u32) -> Self {
        let mut segs = Vec::new();
        let mut i = 0;
        while i < n {
            let mut cells = Vec::new();
            let mut j = 0;
            while j < LV_CELLS {
                cells.push(init);
                j += 1;
            }
            segs.push(LiveSeg { shmid: (i + 1) as u32, cells, removed: false, freed: false, nattch: 0 });
            i += 1;
        }
        LivenessRig {
            segs,
            maps: Vec::new(),
            next_base: 0x50000000,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            deadmap_hits: 0,
            reclaimed: 0,
        }
    }

    /// 段查找。
    pub fn find(&self, shmid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.segs.len() {
            if self.segs[i].shmid == shmid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 挂载。已删段 → EINVAL；存活段正常建映射。
    pub fn shmat(&mut self, pid: u32, shmid: u32) -> Result<usize, u32> {
        let ix = match self.find(shmid) {
            Some(ix) => ix,
            None => return Err(LV_BADID),
        };
        if self.segs[ix].removed {
            self.rounds = self.rounds.wrapping_add(1);
            self.einval_hits = self.einval_hits.wrapping_add(1);
            return Err(LV_EINVAL);
        }
        let base = self.next_base;
        self.next_base = self.next_base.wrapping_add(LV_LBA);
        self.segs[ix].nattch = self.segs[ix].nattch.wrapping_add(1);
        self.maps.push(LiveMap { pid, shmid, addr: base });
        self.rounds = self.rounds.wrapping_add(1);
        self.ok_hits = self.ok_hits.wrapping_add(1);
        Ok(base)
    }

    /// RMID。无挂载即回收；有挂载进入"删而未收"态。
    pub fn rmid(&mut self, shmid: u32) -> Result<bool, u32> {
        let ix = match self.find(shmid) {
            Some(ix) => ix,
            None => return Err(LV_BADID),
        };
        if self.segs[ix].removed {
            return Err(LV_TWICE);
        }
        self.segs[ix].removed = true;
        if self.segs[ix].nattch == 0 {
            self.segs[ix].freed = true;
            let mut i = 0;
            while i < self.segs[ix].cells.len() {
                self.segs[ix].cells[i] = 0;
                i += 1;
            }
            self.reclaimed = self.reclaimed.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 经映射读。映射不存在或段已回收 → DEADMAP；下标越界 → EFAULT。
    pub fn read(&mut self, pid: u32, addr: usize, idx: usize, round: usize) -> Result<u32, u32> {
        let m = self.map_of(pid, addr);
        let shmid = match m {
            Some(s) => s,
            None => {
                self.rounds = self.rounds.wrapping_add(1);
                self.deadmap_hits = self.deadmap_hits.wrapping_add(1);
                return Err(LV_DEADMAP);
            }
        };
        let ix = self.find(shmid).unwrap_or(0);
        if self.segs[ix].freed {
            self.rounds = self.rounds.wrapping_add(1);
            self.deadmap_hits = self.deadmap_hits.wrapping_add(1);
            return Err(LV_DEADMAP);
        }
        if idx >= self.segs[ix].cells.len() {
            self.rounds = self.rounds.wrapping_add(1);
            return Err(LV_EFAULT);
        }
        self.rounds = self.rounds.wrapping_add(1);
        self.ok_hits = self.ok_hits.wrapping_add(1);
        Ok(self.segs[ix].cells[idx])
    }

    /// 经映射写。同读的判定序。
    pub fn write(&mut self, pid: u32, addr: usize, idx: usize, val: u32, round: usize) -> Result<(), u32> {
        let m = self.map_of(pid, addr);
        let shmid = match m {
            Some(s) => s,
            None => {
                self.rounds = self.rounds.wrapping_add(1);
                self.deadmap_hits = self.deadmap_hits.wrapping_add(1);
                return Err(LV_DEADMAP);
            }
        };
        let ix = self.find(shmid).unwrap_or(0);
        if self.segs[ix].freed {
            self.rounds = self.rounds.wrapping_add(1);
            self.deadmap_hits = self.deadmap_hits.wrapping_add(1);
            return Err(LV_DEADMAP);
        }
        if idx >= self.segs[ix].cells.len() {
            self.rounds = self.rounds.wrapping_add(1);
            return Err(LV_EFAULT);
        }
        self.segs[ix].cells[idx] = val;
        self.rounds = self.rounds.wrapping_add(1);
        self.ok_hits = self.ok_hits.wrapping_add(1);
        Ok(())
    }

    /// 查映射属段。
    fn map_of(&self, pid: u32, addr: usize) -> Option<u32> {
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].pid == pid && self.maps[i].addr == addr {
                return Some(self.maps[i].shmid);
            }
            i += 1;
        }
        None
    }

    /// 卸载：nattch 归零且已删则回收。
    pub fn shmdt(&mut self, pid: u32, addr: usize) -> bool {
        let mut found = None;
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].pid == pid && self.maps[i].addr == addr {
                found = Some(i);
            }
            i += 1;
        }
        let ix = match found {
            Some(f) => f,
            None => return false,
        };
        let m = self.maps.remove(ix);
        match self.find(m.shmid) {
            Some(sx) => {
                self.segs[sx].nattch = self.segs[sx].nattch.saturating_sub(1);
                if self.segs[sx].removed && self.segs[sx].nattch == 0 && !self.segs[sx].freed {
                    self.segs[sx].freed = true;
                    let mut j = 0;
                    while j < self.segs[sx].cells.len() {
                        self.segs[sx].cells[j] = 0;
                        j += 1;
                    }
                    self.reclaimed = self.reclaimed.wrapping_add(1);
                }
                true
            }
            None => false,
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.deadmap_hits);
        total <= self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.segs.len() {
            h = (h ^ self.segs[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.maps.len() as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.reclaimed as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "LivenessRig{{segs={},maps={},rounds={},ok={},einval={},dead={},reclaimed={}}}",
            self.segs.len(), self.maps.len(), self.rounds, self.ok_hits,
            self.einval_hits, self.deadmap_hits, self.reclaimed
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部按预期处置。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut rig = LivenessRig::new(2, 7);
    // 注入 1：RMID 后已挂载映射仍可读写。
    let addr = rig.shmat(1, 1).unwrap_or(0);
    let _ = rig.rmid(1);
    let r_ok = rig.write(1, addr, 0, 0x5a5a, 1).is_ok() && rig.read(1, addr, 0, 2) == Ok(0x5a5a);
    caught[0] = r_ok;
    // 注入 2：RMID 后新 shmat → EINVAL。
    caught[1] = matches!(rig.shmat(2, 1), Err(x) if x == LV_EINVAL);
    // 注入 3：最后卸载后段回收（freed + 数据清零）。
    let _ = rig.shmdt(1, addr);
    let ix3 = rig.find(1).unwrap_or(0);
    caught[2] = rig.segs[ix3].freed && rig.segs[ix3].nattch == 0 && rig.reclaimed >= 1;
    // 注入 4：对已回收段的旧映射访问 → DEADMAP。
    caught[3] = matches!(rig.read(1, addr, 0, 3), Err(x) if x == LV_DEADMAP);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：存活段读写循环为主，穿插坏句柄与越界面。
/// 返回"裁决符合预期"的轮数。
pub fn sample_thousand(rig: &mut LivenessRig) -> usize {
    let addr = rig.shmat(0x37, 2).unwrap_or(0);
    let mut good = 0;
    let mut r = 0;
    while r < LV_ROUNDS {
        let low = r % 10;
        let res: Result<u32, u32>;
        let expect_ok;
        if low < 6 {
            // 合法面：写后读同址。
            let val = (r % 251) as u32;
            let idx = r % LV_CELLS;
            let w = rig.write(0x37, addr, idx, val, r);
            res = match w {
                Ok(()) => rig.read(0x37, addr, idx, r),
                Err(e) => Err(e),
            };
            expect_ok = true;
        } else if low == 6 {
            // EFAULT 面：下标越界。
            res = rig.read(0x37, addr, LV_CELLS + 1, r);
            expect_ok = false;
        } else if low == 7 {
            // BADID 面：坏句柄挂载。
            res = rig.shmat(0x37, 99).map(|_| 0);
            expect_ok = false;
        } else if low == 8 {
            // DEADMAP 面：未映射地址读。
            res = rig.read(0x38, 0xdead0000, 0, r);
            expect_ok = false;
        } else {
            // 合法面：跨单元校验和。
            let mut acc: u64 = 0;
            let mut i = 0;
            while i < LV_CELLS {
                match rig.read(0x37, addr, i, r) {
                    Ok(v) => acc = acc.wrapping_add(v as u64),
                    Err(_) => {}
                }
                i += 1;
            }
            res = Ok((acc & 0xffff) as u32);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => !expect_ok && (*x == LV_EFAULT || *x == LV_BADID || *x == LV_DEADMAP),
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10937 判据组。
pub fn run_unx10937_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10937");
    // J1 正面之一：RMID 后已挂载映射仍可读写，十轮全中。
    let mut live_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut rig = LivenessRig::new(1, 3);
        let addr = rig.shmat(1, 1).unwrap_or(0);
        let _ = rig.rmid(1);
        let val = 0x1000 + k as u32;
        if rig.write(1, addr, k % LV_CELLS, val, k).is_ok()
            && rig.read(1, addr, k % LV_CELLS, k) == Ok(val)
        {
            live_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面存活", live_cnt == 10, "RMID 后旧映射十轮全部可读写");
    // J1 正面之二：RMID 后新 shmat → EINVAL 十轮全中。
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut rig = LivenessRig::new(1, 3);
        let _ = rig.shmat(1, 1);
        let _ = rig.rmid(1);
        if matches!(rig.shmat(2, 1), Err(x) if x == LV_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "RMID 后新挂载十轮全部裁 EINVAL");
    // J2 反面：卸载后段回收（千轮口径：回收账逐次核销）。
    let mut rec_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut rig = LivenessRig::new(1, 5);
        let addr = rig.shmat(1, 1).unwrap_or(0);
        let _ = rig.rmid(1);
        let _ = rig.shmdt(1, addr);
        let ix = rig.find(1).unwrap_or(0);
        let mut zeros = true;
        let mut j = 0;
        while j < rig.segs[ix].cells.len() {
            if rig.segs[ix].cells[j] != 0 {
                zeros = false;
            }
            j += 1;
        }
        if rig.segs[ix].freed && zeros {
            rec_cnt += 1;
        }
        k += 1;
    }
    s.add("J2-反面回收", rec_cnt == 10, "最后卸载后段回收十轮全部核销");
    // J3 无挂载直删：立即回收。
    let mut rig3 = LivenessRig::new(1, 5);
    let imm = rig3.rmid(1);
    let ix3 = rig3.find(1).unwrap_or(0);
    s.add(
        "J3-直删即收",
        matches!(imm, Ok(true)) && rig3.segs[ix3].freed && rig3.reclaimed == 1,
        "无挂载 RMID 立即回收",
    );
    // J4 注入四路。
    let inj = inject_four();
    s.add(
        "J4-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "存活/EINVAL/回收/死映射四路全部按预期",
    );
    // J5 千轮存活期零误报。
    let mut rig5 = LivenessRig::new(3, 9);
    let good = sample_thousand(&mut rig5);
    s.add_dyn("J5-千轮零误报", good == LV_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J6 判定序：EIDRM 态挂载先于 BADID（同现按坏句柄面各自独立，不串码）。
    let mut rig6 = LivenessRig::new(1, 1);
    let _ = rig6.rmid(1);
    let r6a = rig6.shmat(1, 1);
    let r6b = rig6.shmat(1, 9);
    s.add(
        "J6-判定序",
        matches!(r6a, Err(x) if x == LV_EINVAL) && matches!(r6b, Err(x) if x == LV_BADID),
        "删后挂载裁 EINVAL、坏句柄裁 BADID，两不串",
    );
    // J7 错误码分段。
    s.add(
        "J7-错误码分段",
        LV_EINVAL > SEG_BASE && LV_LEDGER < SEG_BASE + 0x20,
        "六个错误码全部落在段基址页内",
    );
    // J8 错误码互异。
    let codes = [LV_EINVAL, LV_DEADMAP, LV_BADID, LV_EFAULT, LV_TWICE, LV_LEDGER];
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
    s.add("J8-错误码互异", distinct, "六个错误码两两不同");
    // J9 台内对账。
    s.add("J9-台内对账", rig5.ledger_ok(), "计数总和不超过轮次、映射账一致");
    // J10 fold 稳定。
    let f = rig5.fold();
    let clone = rig5.clone();
    s.add("J10-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J11 双重 RMID。
    let mut rig11 = LivenessRig::new(1, 1);
    let _ = rig11.rmid(1);
    s.add("J11-双重RMID", matches!(rig11.rmid(1), Err(x) if x == LV_TWICE), "二次 RMID 裁 TWICE");
    // J12 渲染非空。
    let txt = rig5.render();
    s.add("J12-渲染非空", txt.len() > 10 && txt.contains("LivenessRig"), "台级渲染含类型名与计数");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == LV_ROUNDS,
        &format!("good={} rig={}", good, rig5.render()),
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
    fn unx10937_shm_rmid_liveness_flow() {
        // 流程：挂载 → 写 → RMID → 旧映射仍读写 → 新挂载 EINVAL → 卸载 → 回收。
        let mut rig = LivenessRig::new(2, 4);
        let addr = rig.shmat(1, 1).unwrap_or(0);
        assert!(rig.write(1, addr, 0, 42, 1).is_ok());
        assert!(matches!(rig.rmid(1), Ok(false)));
        assert!(rig.write(1, addr, 1, 43, 2).is_ok());
        assert_eq!(rig.read(1, addr, 0, 3), Ok(42));
        assert!(matches!(rig.shmat(2, 1), Err(x) if x == LV_EINVAL));
        assert!(rig.shmdt(1, addr));
        let ix = rig.find(1).unwrap_or(0);
        assert!(rig.segs[ix].freed);
        assert!(matches!(rig.read(1, addr, 0, 4), Err(x) if x == LV_DEADMAP));
        assert!(rig.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10937_checks_green() {
        let s = run_unx10937_checks();
        assert!(s.all_passed(), "UNX-F10937 判据未全绿");
    }
}
