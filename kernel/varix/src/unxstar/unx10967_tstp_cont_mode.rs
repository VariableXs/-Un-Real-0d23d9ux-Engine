//! UNX-F10967 SIGTSTP/SIGCONT 挂起恢复联测（模式保存恢复）（0x1d860 段）
//!
//! 卡片判据（UNX-F10967-J1）：
//! - 挂起前模式快照/恢复后逐位还原（各 10/10），千次往返零漂移（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模 SIGTSTP/SIGCONT 挂起恢复链：
//! 挂起时对终端模式做逐位快照，恢复时逐位还原；快照-恢复千次
//! 往返摘要零漂移；非会话首/坏 fd 显性报错。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10967-10900)*0x20 = 0x1d860）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d860;

/// SP_EBADF：fd 无效
pub const SP_EBADF: u32 = SEG_BASE + 0x01;
/// SP_EINVAL：模式位越界
pub const SP_EINVAL: u32 = SEG_BASE + 0x02;
/// SP_EPERM：非会话首操作挂起
pub const SP_EPERM: u32 = SEG_BASE + 0x03;
/// SP_ENOSNAP：无快照可恢复
pub const SP_ENOSNAP: u32 = SEG_BASE + 0x04;
/// SP_EDRIFT：恢复后逐位比对漂移（自检面）
pub const SP_EDRIFT: u32 = SEG_BASE + 0x05;
/// SP_ESTATE：状态机错乱（挂起中再挂起）
pub const SP_ESTATE: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// SIGTSTP 信号号（建模值）
pub const SIGTSTP: u32 = 20;
/// SIGCONT 信号号（建模值）
pub const SIGCONT: u32 = 18;
/// 模式位有效掩码
pub const MODE_MASK: u32 = 0x00ff;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 终端模式快照
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeSnap {
    pub flags: u32,
    pub seq: u64,
}

impl ModeSnap {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.flags as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.seq).wrapping_mul(0x100000001b3);
        h
    }
}

/// 挂起恢复联测台
#[derive(Clone, Debug)]
pub struct SuspendDesk {
    /// 当前模式位
    pub flags: u32,
    /// 会话首标记
    pub is_leader: bool,
    /// fd 存活
    pub fd_alive: bool,
    /// 挂起中标记
    pub suspended: bool,
    /// 挂起前快照
    pub snap: Option<ModeSnap>,
    /// 快照计数
    pub n_snap: u32,
    /// 恢复计数
    pub n_restore: u32,
    /// 错误计数
    pub n_eperm: u32,
    pub n_ebadf: u32,
    pub n_einval: u32,
}

impl SuspendDesk {
    pub fn new() -> Self {
        SuspendDesk {
            flags: 0x0f,
            is_leader: true,
            fd_alive: true,
            suspended: false,
            snap: None,
            n_snap: 0,
            n_restore: 0,
            n_eperm: 0,
            n_ebadf: 0,
            n_einval: 0,
        }
    }

    /// 修改模式位（挂起模拟外部环境改动）
    pub fn set_flags(&mut self, fd: i32, flags: u32) -> Result<(), u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(SP_EBADF);
        }
        if flags & !MODE_MASK != 0 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(SP_EINVAL);
        }
        self.flags = flags;
        Ok(())
    }

    /// SIGTSTP：挂起并做逐位快照
    pub fn tstp(&mut self, fd: i32) -> Result<ModeSnap, u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(SP_EBADF);
        }
        if !self.is_leader {
            self.n_eperm = self.n_eperm.wrapping_add(1);
            return Err(SP_EPERM);
        }
        if self.suspended {
            return Err(SP_ESTATE);
        }
        let s = ModeSnap { flags: self.flags, seq: self.n_snap as u64 + 1 };
        self.snap = Some(s);
        self.suspended = true;
        self.n_snap = self.n_snap.wrapping_add(1);
        Ok(s)
    }

    /// SIGCONT：恢复并逐位还原
    pub fn cont(&mut self, fd: i32) -> Result<u32, u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(SP_EBADF);
        }
        if !self.is_leader {
            self.n_eperm = self.n_eperm.wrapping_add(1);
            return Err(SP_EPERM);
        }
        match self.snap.take() {
            None => Err(SP_ENOSNAP),
            Some(s) => {
                self.flags = s.flags;
                self.suspended = false;
                self.n_restore = self.n_restore.wrapping_add(1);
                Ok(s.flags)
            }
        }
    }

    /// 逐位还原自检：恢复后 flags 与快照逐位一致
    pub fn roundtrip_ok(&self, snap_flags: u32) -> bool {
        self.flags == snap_flags && self.snap.is_none() && !self.suspended
    }

    /// 千次往返零漂移：快照/恢复不改变位面且账自洽
    pub fn drift_ok(&self, start_flags: u32) -> bool {
        // 挂起-恢复一次不改 flags：n_snap == n_restore 且当前 flags 未变
        if self.n_snap != self.n_restore {
            return false;
        }
        // 快照序列连续：无滞留快照
        self.snap.is_none() && start_flags == self.flags
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_rounds: u32) -> bool {
        self.n_snap == expect_rounds && self.n_restore == expect_rounds
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.flags as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_snap as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_restore as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.suspended as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "suspend:flags={:#x} snap={} restore={} susp={}",
            self.flags, self.n_snap, self.n_restore, self.suspended
        )
    }
}

impl Default for SuspendDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = SuspendDesk::new();
    // 路1：无快照恢复 → ENOSNAP
    let r1 = matches!(d.cont(0), Err(x) if x == SP_ENOSNAP);
    // 路2：非会话首挂起 → EPERM
    d.is_leader = false;
    let r2 = matches!(d.tstp(0), Err(x) if x == SP_EPERM);
    d.is_leader = true;
    // 路3：越界模式位 → EINVAL
    let r3 = matches!(d.set_flags(0, 0x100), Err(x) if x == SP_EINVAL);
    // 路4：挂起中再挂起 → ESTATE
    let _ = d.tstp(0);
    let r4 = matches!(d.tstp(0), Err(x) if x == SP_ESTATE);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮在挂起前改一次模式位，快照-恢复逐位还原；
/// 恢复后位面与快照一致记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut d = SuspendDesk::new();
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        // 挂起前模式位随轮变化
        let want = ((r % 256) as u32) ^ 0x5a;
        if d.set_flags(0, want).is_err() {
            return good;
        }
        let snap = match d.tstp(0) { Ok(s) => s, Err(_) => return good };
        // 挂起期间模拟一次无效改动（恢复必须覆盖它）
        let _ = d.set_flags(0, 0x33);
        let restored = match d.cont(0) { Ok(f) => f, Err(_) => return good };
        if restored == want && snap.flags == want && d.roundtrip_ok(want) {
            good = good.wrapping_add(1);
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10967_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10967");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ENOSNAP/EPERM/EINVAL/ESTATE 坏样本全部命中");

    // J1 快照 10/10（同一台面十轮挂起/恢复，快照序号逐轮递增）
    let mut snap10 = 0u32;
    let mut d = SuspendDesk::new();
    for i in 0..10u32 {
        let want = 0x10 + i;
        if d.set_flags(0, want).is_err() {
            continue;
        }
        if let Ok(sn) = d.tstp(0) {
            if sn.flags == want && sn.seq == i as u64 + 1 {
                snap10 = snap10.wrapping_add(1);
            }
        }
        let _ = d.cont(0);
    }
    s.add("J1-快照10/10", snap10 == 10, "挂起前逐位快照十轮全对");

    // 恢复逐位还原 10/10
    let mut res10 = 0u32;
    for i in 0..10u32 {
        let mut d = SuspendDesk::new();
        let want = 0xa0 + i;
        let _ = d.set_flags(0, want);
        let _ = d.tstp(0);
        let _ = d.set_flags(0, 0x00);
        if d.cont(0) == Ok(want) && d.roundtrip_ok(want) {
            res10 = res10.wrapping_add(1);
        }
    }
    s.add("J2-逐位还原10/10", res10 == 10, "恢复后位面与快照逐位一致");

    // 千次往返零漂移
    let good = sample_thousand();
    let drift = good == 1000;
    s.add("J3-千次往返零漂移", drift, "千轮快照-恢复位面零漂移");

    // 错误码分段
    let codes = [SP_EBADF, SP_EINVAL, SP_EPERM, SP_ENOSNAP, SP_EDRIFT, SP_ESTATE];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J4-错误码分段", seg_ok, "六码全部落在 0x1d860 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J5-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut d2 = SuspendDesk::new();
    for _ in 0..10 {
        let _ = d2.tstp(0);
        let _ = d2.cont(0);
    }
    s.add("J6-账目对平", d2.ledger_ok(10), "快照/恢复各 10 次对平");

    // 漂移自检：快照/恢复不改变位面
    s.add("J7-漂移自检", d2.drift_ok(0x0f) && d2.flags == 0x0f, "十次往返后位面保持初始值");

    // 信号号建模
    s.add(
        "J8-信号号",
        SIGTSTP == 20 && SIGCONT == 18,
        "SIGTSTP=20 SIGCONT=18",
    );

    // 判定序：EBADF 先于 EPERM
    let mut d3 = SuspendDesk::new();
    d3.fd_alive = false;
    d3.is_leader = false;
    let seq_ok = matches!(d3.tstp(0), Err(x) if x == SP_EBADF);
    s.add("J9-判定序", seq_ok, "坏 fd 先于会话首判定");

    // fold 稳定
    let h1 = d2.fold_all();
    let h2 = d2.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d2.render();
    s.add(
        "J11-渲染非空",
        r.starts_with("suspend:") && r.contains("flags=0xf"),
        "摘要含位面与往返账",
    );

    // 状态机恢复后可再挂起
    let again = d2.tstp(0).is_ok() && d2.cont(0).is_ok() && !d2.suspended;
    s.add("J12-再挂起面", again, "恢复后状态机可复用");

    // 动态判据
    s.add_dyn(
        "J13-往返账",
        d2.n_snap == 11 && d2.n_restore == 11,
        &crate::alloc::format!("snap={} restore={}", d2.n_snap, d2.n_restore),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-挂起态",
        !d2.suspended && d2.snap.is_none(),
        &crate::alloc::format!("suspended={} snap_none={}", d2.suspended, d2.snap.is_none()),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10967_tstp_cont_mode_flow() {
        let mut d = SuspendDesk::new();
        let _ = d.set_flags(0, 0x2a);
        let sn = d.tstp(0).unwrap();
        assert_eq!(sn.flags, 0x2a);
        let _ = d.set_flags(0, 0x00);
        assert_eq!(d.cont(0), Ok(0x2a));
        assert!(d.roundtrip_ok(0x2a));
        assert!(matches!(d.cont(0), Err(x) if x == SP_ENOSNAP));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10967_checks_green() {
        let s = run_unx10967_checks();
        assert!(s.all_passed(), "UNX-F10967 判据未全绿");
    }
}
