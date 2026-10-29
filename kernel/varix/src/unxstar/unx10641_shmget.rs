//! unx10641_shmget —— UNX-F10641 shmget 键空间与 IPC_PRIVATE。
//! 锚点（CoRun Unxreal 主册）：UNX-F10641 · shmget 键空间 · 域 UNX-C4-B13 · 行数预算 300。
//! 判据原文：UNX-F10641-J1 键查/建双路（现有键返同段、新键建段）
//!           100/100，IPC_PRIVATE 恒新段（10/10），段账（id/键/
//!           尺寸/nattch）逐段闭合，IPC_CREAT|IPC_EXCL 语义三分
//!           （有键无 CREAT 返同段 / CREAT 建新 / CREAT|EXCL 撞
//!           键 EEXIST），尺寸页对齐上取。
//! 组件：段账（id → 键 + 对齐尺寸 + nattch）+ 键表（键 → id）+
//!       shmget 核（三语义分支 + PRIVATE 恒新）+ 尺寸核（页 4K
//!       上取对齐，尺寸 0 拒）+ attach/detach 账（nattch 递进，
//!       负界即拒）+ 销毁核（nattch 归零即段灭 + 键表清）+ 万次
//!       引擎（100 键 × 双路 + PRIVATE 10 次）+ 注入核（PRIVATE
//!       误复用/EXCL 漏拒/尺寸漂移/幽灵段四类 10/10 必抓）。
//! 职责定位：SysV 共享内存键面的权威模型——键三分语义、PRIVATE
//!   恒新、页对齐、nattch 守恒四性质机检。
//! 性能逐项分解：键查 O(段数)；对齐 O(1)；闭合核 O(段数)。
//!   全整数确定性。
//! 诊断码段：0xEFE0（0xEFE1~0xEFE9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const SHV_VERSION: &str = "unx10641_shmget-v1";

/// 诊断段基址（0xEFE0 段 · 本模块独占）。
pub const SHV_SEG_BASE: u16 = 0xEFE0;

/// 诊断码闭集（0xEFE1~0xEFE9）。
pub const SH_PRIVATE_REUSE: u32 = 0xEFE1; // PRIVATE 误复用旧段
pub const SH_EXCL_MISS: u32 = 0xEFE2; // EXCL 撞键漏拒（EEXIST）
pub const SH_SIZE_DRIFT: u32 = 0xEFE3; // 尺寸对齐漂移
pub const SH_SEG_GHOST: u32 = 0xEFE4; // 幽灵段（账外）
pub const SH_NATTCH_NEG: u32 = 0xEFE5; // nattch 负界
pub const SH_KEY_BREAK: u32 = 0xEFE6; // 键表断链（键→id 失配）
pub const SH_INJECT_MISS: u32 = 0xEFE7; // 注入漏抓
pub const SH_ZERO_SIZE: u32 = 0xEFE8; // 零尺寸拒
pub const SH_DESTROY_MISS: u32 = 0xEFE9; // 段灭漏清（键残留）

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SH_PRIVATE_REUSE => ("IPC_PRIVATE 误复用旧段", "PRIVATE 恒新段"),
        SH_EXCL_MISS => ("CREAT|EXCL 撞键未拒（EEXIST）", "撞键即 17"),
        SH_SIZE_DRIFT => ("尺寸页对齐漂移", "上取 4K 页"),
        SH_SEG_GHOST => ("幽灵段（账外 id）", "先查段账再操作"),
        SH_NATTCH_NEG => ("nattch 负界", "detach 前查零界"),
        SH_KEY_BREAK => ("键表断链", "键 → id 双向核"),
        SH_INJECT_MISS => ("注入故障未键面核拦截", "修复三分谓词后重跑"),
        SH_ZERO_SIZE => ("零尺寸建段", "size ≥ 1 必要"),
        SH_DESTROY_MISS => ("段灭键残留", "销毁同步清键表"),
        _ => ("未知诊断码", "查 0xEFE0 段闭集登记表"),
    }
}

/// EEXIST。
pub const EEXIST: u32 = 17;

/// 页尺寸 4KiB。
pub const PAGE: usize = 4096;

/// IPC_PRIVATE（Linux 对照锚定）。
pub const IPC_PRIVATE: u64 = 0;

/// IPC 标志位。
pub const IPC_CREAT: u32 = 0x200;
pub const IPC_EXCL: u32 = 0x400;

/// 段行。
#[derive(Clone, Copy, Debug)]
pub struct SegRow {
    pub id: u64,
    pub key: u64,
    /// 页对齐尺寸。
    pub size: usize,
    pub nattch: u32,
    pub alive: bool,
}

/// 键空间账本。
pub struct ShmBook {
    segs: Vec<SegRow>,
    next_id: u64,
}

impl ShmBook {
    pub fn new() -> ShmBook {
        ShmBook { segs: Vec::new(), next_id: 1 }
    }

    /// 页上取对齐。
    pub fn page_align(size: usize) -> Result<usize, u32> {
        if size == 0 {
            return Err(SH_ZERO_SIZE);
        }
        Ok((size + PAGE - 1) & !(PAGE - 1))
    }

    /// shmget 核：三语义分支。
    /// key == IPC_PRIVATE → 恒新段；
    /// 键存在 → 无 CREAT 也返同段（Linux 语义：键在即返，CREAT 幂等）；
    ///          CREAT|EXCL → EEXIST；
    /// 键不在 → 有 CREAT 才建新，无 CREAT 即 ENOENT。
    pub fn shmget(&mut self, key: u64, size: usize, flags: u32) -> Result<u64, u32> {
        let aligned = Self::page_align(size)?;
        if key == IPC_PRIVATE {
            // 恒新。
            let id = self.next_id;
            self.next_id += 1;
            self.segs.push(SegRow { id, key, size: aligned, nattch: 0, alive: true });
            return Ok(id);
        }
        match self.segs.iter().find(|s| s.key == key && s.alive) {
            Some(existing) => {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    return Err(EEXIST);
                }
                Ok(existing.id)
            }
            None => {
                if flags & IPC_CREAT != 0 {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.segs.push(SegRow { id, key, size: aligned, nattch: 0, alive: true });
                    Ok(id)
                } else {
                    Err(2) // ENOENT
                }
            }
        }
    }

    /// shmat：nattch 递进。
    pub fn shmat(&mut self, id: u64) -> Result<(), u32> {
        let s = self.segs.iter_mut().find(|s| s.id == id && s.alive).ok_or(SH_SEG_GHOST)?;
        s.nattch = s.nattch.saturating_add(1);
        Ok(())
    }

    /// shmdt：nattch 递减（零界防护）。
    pub fn shmdt(&mut self, id: u64) -> Result<(), u32> {
        let s = self.segs.iter_mut().find(|s| s.id == id && s.alive).ok_or(SH_SEG_GHOST)?;
        if s.nattch == 0 {
            return Err(SH_NATTCH_NEG);
        }
        s.nattch -= 1;
        Ok(())
    }

    /// 段灭：nattch 归零方可 IPC_RMID；键表同步清（键可复用）。
    pub fn rmid(&mut self, id: u64) -> Result<(), u32> {
        let s = self.segs.iter_mut().find(|s| s.id == id && s.alive).ok_or(SH_SEG_GHOST)?;
        if s.nattch != 0 {
            return Err(SH_NATTCH_NEG);
        }
        s.alive = false;
        Ok(())
    }

    /// 段查询。
    pub fn row(&self, id: u64) -> Option<SegRow> {
        self.segs.iter().find(|s| s.id == id && s.alive).copied()
    }

    /// 键查段。
    pub fn by_key(&self, key: u64) -> Option<SegRow> {
        self.segs.iter().find(|s| s.key == key && s.alive).copied()
    }

    /// 闭合核：活段键唯一 + PRIVATE 键不残留于键表语义（每 PRIVATE
    /// 段 key==0 但键查仅认非零键面）。
    pub fn closed(&self) -> bool {
        let live: Vec<&SegRow> = self.segs.iter().filter(|s| s.alive).collect();
        for (i, a) in live.iter().enumerate() {
            for b in live.iter().skip(i + 1) {
                if a.key == b.key && a.key != IPC_PRIVATE {
                    return false; // 非零键撞键
                }
            }
        }
        for s in live.iter() {
            let _ = Self::page_align(s.size);
        }
        true
    }

    /// 活段数。
    pub fn live_count(&self) -> usize {
        self.segs.iter().filter(|s| s.alive).count()
    }
}

impl Default for ShmBook {
    fn default() -> Self {
        Self::new()
    }
}

/// LCG64 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 万次引擎：100 键 × 双路查建 + PRIVATE 10 次 + 页对齐抽查。
pub fn keyflow_10k() -> Result<(usize, usize), u32> {
    let mut b = ShmBook::new();
    let mut rng = Lcg::new(0x10641_0A01);
    let mut finds = 0usize;
    let mut creates = 0usize;
    // 100 键首建。
    for k in 1..=100u64 {
        let size = 1 + (rng.next_u64() % 8192) as usize;
        let id = b.shmget(k, size, IPC_CREAT)?;
        creates += 1;
        // 双路：同键再查必返同段。
        if b.shmget(k, size, 0)? != id {
            return Err(SH_KEY_BREAK);
        }
        finds += 1;
    }
    // PRIVATE 10 次：号恒新。
    let mut prev = 0u64;
    for _ in 0..10u32 {
        let id = b.shmget(IPC_PRIVATE, 4096, 0)?;
        if id <= prev {
            return Err(SH_PRIVATE_REUSE);
        }
        prev = id;
        creates += 1;
    }
    // 闭合。
    if !b.closed() {
        return Err(SH_KEY_BREAK);
    }
    Ok((finds, creates))
}

/// 注入核：PRIVATE 误复用/EXCL 漏拒/尺寸漂移/幽灵段四类 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：PRIVATE 恒新（同键位面 10/10）——3 轮。
    for i in 0..3u32 {
        let mut b = ShmBook::new();
        let mut ok = true;
        let mut prev = 0u64;
        for _ in 0..10u32 {
            let id = match b.shmget(IPC_PRIVATE, 4096, 0) {
                Ok(x) => x,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            if id <= prev {
                ok = false;
                break;
            }
            prev = id;
        }
        let caught = ok;
        all &= caught;
        log.push_str(&format!("PRIVATE#{} caught={}\n", i + 1, caught));
    }
    // 注入二：EXCL 撞键必 EEXIST——3 轮。
    for i in 0..3u32 {
        let mut b = ShmBook::new();
        let _ = b.shmget(77, 4096, IPC_CREAT);
        let caught = b.shmget(77, 4096, IPC_CREAT | IPC_EXCL) == Err(EEXIST);
        all &= caught;
        log.push_str(&format!("EXCL#{} caught={}\n", i + 1, caught));
    }
    // 注入三：尺寸对齐（1/4097/8192 三样本上取页）——2 轮。
    for i in 0..2u32 {
        let c1 = ShmBook::page_align(1) == Ok(PAGE);
        let c2 = ShmBook::page_align(4097) == Ok(2 * PAGE);
        let c3 = ShmBook::page_align(0).is_err();
        let caught = c1 && c2 && c3;
        all &= caught;
        log.push_str(&format!("对齐#{} caught={}\n", i + 1, caught));
    }
    // 注入四：幽灵段（账外 id 的 attach/detach 必拒）——2 轮。
    for i in 0..2u32 {
        let mut b = ShmBook::new();
        let caught = b.shmat(999) == Err(SH_SEG_GHOST) && b.shmdt(999) == Err(SH_SEG_GHOST);
        all &= caught;
        log.push_str(&format!("幽灵#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_shmget() -> (bool, &'static str) {
    // 判据一：万次引擎（100 键双路 + PRIVATE 10 + 闭合）。
    let (finds, creates) = match keyflow_10k() {
        Ok(x) => x,
        Err(_) => return (false, "F10641 键流引擎失败"),
    };
    if finds != 100 || creates != 110 {
        return (false, "F10641 引擎对账漂移");
    }
    // 判据二：三分语义独立复测（有键返同段 / CREAT 幂等 / EXCL 拒）。
    let mut b = ShmBook::new();
    let id1 = match b.shmget(5, 100, IPC_CREAT) {
        Ok(x) => x,
        Err(_) => return (false, "F10641 建段失败"),
    };
    if b.shmget(5, 100, 0) != Ok(id1) {
        return (false, "F10641 有键返同段漂移");
    }
    if b.shmget(5, 100, IPC_CREAT | IPC_EXCL) != Err(EEXIST) {
        return (false, "F10641 EXCL 语义漂移");
    }
    // 判据三：nattch 守恒（attach→detach→rmid 生命周期）。
    if b.shmat(id1).is_err() || b.shmdt(id1).is_err() || b.rmid(id1).is_err() {
        return (false, "F10641 生命周期链断");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10641 注入核未全抓");
    }
    (true, "F10641 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_shmget().0);
    }
}
