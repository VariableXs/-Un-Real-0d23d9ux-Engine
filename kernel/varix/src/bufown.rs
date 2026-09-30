//! UNREAL-X-15000 · WP-201 · B-504 缓冲所有权状态机（MD2 篇 5.2 × 判据表）。
//!
//! 所有权定案（MD2 行 335）：客户端缓冲是共享内存对象，合成器与应用各自持有
//! 映射，所有权语义清晰——**提交后缓冲归合成器读到下一帧回调为止，客户端在此
//! 窗口内不许复用**（双缓冲起步，三缓冲给需要高帧率的动画应用，数量进表面
//! 配额）。这条所有权纪律防住一类经典腐坏：应用边画边被合成，用户看到半帧。
//!
//! 判据（MD2 行 360）：B-504 缓冲所有权——**并发压测零半帧**。
//! 典型缺陷（MD2 行 1621）：客户端复用未回收缓冲——半帧上屏，压测才现形。
//!
//! 状态迁移表（代码即冻结文本，变更走 ADR）：
//!   Free  --commit-->      Held   （原子提交：attach+damage+commit 组合生效）
//!   Held  --frame_done-->  Free   （合成器发 frame_callback 归还）
//!   Free  --destroy-->     Gone   （回收记账，配额即时归还）
//!   Held  --destroy-->     Dying  （不撕正在合成的缓冲；回调后 Gone）
//!   Dying --frame_done-->  Gone   （回收记账）
//!   Held  --client_write-> 违规计数（半帧腐坏源，压测必须零）
//! 零堆、整数运算、宿主全测。判据号 B-504 入 CheckSet 命名。

// ---------------------------------------------------------------------------
// 常量与状态
// ---------------------------------------------------------------------------

/// 每表面缓冲上限（双缓冲起步，三缓冲给高帧率动画）。
pub const MAX_BUFS_PER_SURFACE: usize = 3;

/// 缓冲状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufState {
    /// 客户端拥有，可写。
    Free,
    /// 合成器持有（commit 后至 frame_callback 前）。
    Held,
    /// 合成中还，但已请求销毁；回调后回收。
    Dying,
    /// 已回收，配额已归还。
    Gone,
}

/// 错误码。
pub const E_OK: u16 = 0;
pub const E_BAD_SLOT: u16 = 1;
pub const E_QUOTA: u16 = 2;
pub const E_WRONG_STATE: u16 = 3;
pub const E_NOT_COMMITTED: u16 = 4;
pub const E_DOUBLE_COMMIT: u16 = 5;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_BAD_SLOT => "缓冲槽位不存在，建议核对表面配额内编号",
        E_QUOTA => "表面配额已满，建议等 frame_callback 归还后再申请",
        E_WRONG_STATE => "所有权窗口内操作非法——提交后到回调前缓冲归合成器，客户端禁写禁复用",
        E_NOT_COMMITTED => "缓冲未提交，合成器无从读取，建议先 commit",
        E_DOUBLE_COMMIT => "缓冲已提交且未归还，重复提交被拒——原子语义防半帧",
        _ => "未知所有权错误，建议重建表面",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Buffer {
    pub state: BufState,
    pub shm_id: u32,
    /// 代数：每次回收递增，防陈旧引用（客户端拿着旧槽号回来 = 拒）。
    pub gen: u32,
}

/// 单表面缓冲所有权状态机。
#[derive(Clone, Copy, Debug)]
pub struct SurfaceOwnership {
    bufs: [Option<Buffer>; MAX_BUFS_PER_SURFACE],
    /// 表面配额（2 双缓冲起步 / 3 动画档）。
    pub quota: usize,
    /// 已 commit 待合成的缓冲槽（原子提交登记）。
    pub pending_commit: Option<usize>,
    // —— 记账（篇 14.1 账本挂点）——
    pub commits: u64,
    pub callbacks: u64,
    pub destroys: u64,
    /// Held 中客户端写 = 半帧腐坏（B-504 压测必须零）。
    pub reuse_violations: u64,
    /// Held 中销毁请求（合法但延迟回收）。
    pub destroy_while_held: u64,
    /// 配额拒绝次数。
    pub quota_rejected: u64,
    /// 双重提交拒绝次数。
    pub double_commit_rejected: u64,
}

impl SurfaceOwnership {
    pub fn new(quota: usize) -> SurfaceOwnership {
        SurfaceOwnership {
            bufs: [None; MAX_BUFS_PER_SURFACE],
            quota: quota.clamp(2, MAX_BUFS_PER_SURFACE),
            pending_commit: None,
            commits: 0,
            callbacks: 0,
            destroys: 0,
            reuse_violations: 0,
            destroy_while_held: 0,
            quota_rejected: 0,
            double_commit_rejected: 0,
        }
    }

    /// 活跃（未回收）缓冲数——配额实时可查。
    pub fn active(&self) -> usize {
        self.bufs.iter().filter(|b| b.map_or(false, |b| b.state != BufState::Gone)).count()
    }

    fn slot_state(&self, slot: usize) -> Option<BufState> {
        self.bufs.get(slot).and_then(|b| b.map(|b| b.state))
    }

    /// 客户端申请缓冲：配额内分配，返回槽号。
    pub fn attach(&mut self, shm_id: u32) -> Result<usize, u16> {
        if self.active() >= self.quota {
            self.quota_rejected += 1;
            return Err(E_QUOTA);
        }
        let slot = self
            .bufs
            .iter()
            .position(|b| b.is_none() || b.map_or(false, |b| b.state == BufState::Gone))
            .ok_or(E_QUOTA)?;
        let gen = self.bufs[slot].map_or(1, |b| b.gen + 1);
        self.bufs[slot] = Some(Buffer { state: BufState::Free, shm_id, gen });
        Ok(slot)
    }

    /// 客户端原子提交：Free → Held，登记待合成。
    pub fn commit(&mut self, slot: usize) -> u16 {
        match self.slot_state(slot) {
            None | Some(BufState::Gone) => return E_BAD_SLOT,
            Some(BufState::Held) => {
                self.double_commit_rejected += 1;
                return E_DOUBLE_COMMIT;
            }
            Some(BufState::Dying) => return E_WRONG_STATE,
            Some(BufState::Free) => {}
        }
        if self.pending_commit.is_some() {
            return E_DOUBLE_COMMIT;
        }
        self.bufs[slot].as_mut().unwrap().state = BufState::Held;
        self.pending_commit = Some(slot);
        self.commits += 1;
        E_OK
    }

    /// 合成器开帧：取待合成缓冲开始读。
    pub fn begin_compose(&mut self) -> Option<usize> {
        self.pending_commit.take()
    }

    /// 合成器发 frame_callback：Held → Free（或 Dying → Gone 回收）。
    pub fn frame_done(&mut self, slot: usize) -> u16 {
        match self.slot_state(slot) {
            Some(BufState::Held) => {
                self.bufs[slot].as_mut().unwrap().state = BufState::Free;
                self.callbacks += 1;
                E_OK
            }
            Some(BufState::Dying) => {
                self.bufs[slot].as_mut().unwrap().state = BufState::Gone;
                self.callbacks += 1;
                self.destroys += 1;
                E_OK
            }
            None | Some(BufState::Gone) => E_BAD_SLOT,
            Some(BufState::Free) => E_NOT_COMMITTED,
        }
    }

    /// 客户端写检查：仅 Free 可写。Held 中写 = 半帧腐坏，计数上报。
    pub fn client_write(&mut self, slot: usize, gen: u32) -> u16 {
        match self.slot_state(slot) {
            Some(BufState::Free) if self.bufs[slot].map_or(false, |b| b.gen == gen) => E_OK,
            Some(BufState::Held) => {
                self.reuse_violations += 1;
                E_WRONG_STATE
            }
            Some(BufState::Dying) => {
                self.reuse_violations += 1;
                E_WRONG_STATE
            }
            _ => E_BAD_SLOT,
        }
    }

    /// 销毁：Free → Gone 即时回收；Held → Dying 延迟到回调后。
    pub fn destroy(&mut self, slot: usize) -> u16 {
        match self.slot_state(slot) {
            Some(BufState::Free) => {
                self.bufs[slot].as_mut().unwrap().state = BufState::Gone;
                self.destroys += 1;
                E_OK
            }
            Some(BufState::Held) => {
                self.bufs[slot].as_mut().unwrap().state = BufState::Dying;
                self.destroy_while_held += 1;
                E_OK
            }
            Some(BufState::Dying) => E_OK,
            None | Some(BufState::Gone) => E_BAD_SLOT,
        }
    }
}

// ---------------------------------------------------------------------------
// 交错压测（B-504 判据"并发压测零半帧"的宿主落地：确定性交错模型）
// ---------------------------------------------------------------------------

/// 压测动作（合成器/客户端两参与者的操作交错）。
#[derive(Clone, Copy, Debug)]
pub enum Drill {
    ClientWrite { slot: usize, gen: u32 },
    Commit { slot: usize },
    BeginCompose,
    FrameDone { slot: usize },
    Attach { shm_id: u32 },
}

/// 确定性 LCG（无浮点、跨平台一致）。
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// 一轮压测：按给定动作序列驱动状态机，返回账本。
/// 守纪律客户端（只在 Free 且代数匹配时写）必须零半帧。
pub fn run_drill(quota: usize, seed: u64, steps: usize, disciplined: bool) -> SurfaceOwnership {
    let mut so = SurfaceOwnership::new(quota);
    let mut rng = Lcg(seed);
    let mut live: [(usize, u32); MAX_BUFS_PER_SURFACE] = [(usize::MAX, 0); MAX_BUFS_PER_SURFACE];
    let mut nlive = 0usize;
    let mut held: Option<(usize, u32)> = None;
    let _ = disciplined;

    for _ in 0..steps {
        // 每步先合成的机会：有 pending 就有机会开帧，开帧后有机会回调
        if held.is_none() && so.pending_commit.is_some() && rng.pick(2) == 0 {
            if let Some(slot) = so.begin_compose() {
                let g = so.bufs[slot].map_or(0, |b| b.gen);
                held = Some((slot, g));
                continue;
            }
        }
        if let Some((slot, _g)) = held {
            if rng.pick(3) != 0 {
                let _ = so.frame_done(slot);
                // 归还后更新 live 表
                for l in live.iter_mut().take(nlive) {
                    if l.0 == slot {
                        // gen 不变（归还代数不变）
                    }
                }
                held = None;
                continue;
            }
        }
        // 客户端动作
        match rng.pick(4) {
            0 => {
                // 写：守纪律 = 只写 Free；不守纪律 = 乱写（违规被抓）
                if nlive > 0 {
                    let i = rng.pick(nlive);
                    let (slot, gen) = live[i];
                    if disciplined {
                        let st = so.slot_state(slot);
                        if st == Some(BufState::Free) {
                            let _ = so.client_write(slot, gen);
                        }
                    } else {
                        let _ = so.client_write(slot, gen);
                    }
                }
            }
            1 => {
                // commit：守纪律 = 只 commit Free
                if nlive > 0 {
                    let i = rng.pick(nlive);
                    let (slot, _gen) = live[i];
                    let st = so.slot_state(slot);
                    if !disciplined || st == Some(BufState::Free) {
                        if so.commit(slot) == E_OK {
                            for l in live.iter_mut().take(nlive) {
                                if l.0 == slot {
                                    if let Some(b) = so.bufs[slot] {
                                        l.1 = b.gen;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            2 => {
                // attach 新缓冲（配额内）
                if nlive < quota {
                    if let Ok(slot) = so.attach(0xA000 + nlive as u32) {
                        if let Some(b) = so.bufs[slot] {
                            live[nlive] = (slot, b.gen);
                            nlive += 1;
                        }
                    }
                }
            }
            _ => {
                // destroy：守纪律 = 不销毁 Held（销毁 Dying/Free 均合法）
                if nlive > 0 {
                    let i = rng.pick(nlive);
                    let (slot, _gen) = live[i];
                    let st = so.slot_state(slot);
                    if !disciplined || st != Some(BufState::Held) {
                        let _ = so.destroy(slot);
                        // Gone 的从 live 表摘除
                        if st == Some(BufState::Free) {
                            live[i] = live[nlive - 1];
                            nlive -= 1;
                        }
                    }
                }
            }
        }
    }
    // 收尾：把余下的 Held 全部归还，状态机回到静止
    if let Some((slot, _g)) = held {
        let _ = so.frame_done(slot);
    }
    so
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-504 入命名）
// ---------------------------------------------------------------------------

pub fn run_bufown_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-bufown");

    // —— 状态迁移表 ——
    let mut so = SurfaceOwnership::new(2);
    let s0 = so.attach(100).unwrap_or(usize::MAX);
    set.add(
        "B-504 attach 配额内分配",
        s0 == 0 && so.active() == 1 && so.quota == 2,
        "双缓冲起步",
    );
    let c1 = so.commit(s0);
    set.add(
        "B-504 commit Free→Held",
        c1 == E_OK && so.pending_commit == Some(s0) && so.commits == 1,
        "原子提交登记待合成",
    );
    let dc = so.commit(s0);
    set.add(
        "B-504 双重提交拒绝",
        dc == E_DOUBLE_COMMIT && so.double_commit_rejected == 1,
        "未归还前重复提交被拒——防半帧",
    );
    let got = so.begin_compose();
    set.add(
        "B-504 合成器开帧取 pending",
        got == Some(s0) && so.pending_commit.is_none(),
        "pending 取走即清空",
    );
    let w_held = so.client_write(s0, 1);
    set.add(
        "B-504 Held 中写=违规计数",
        w_held == E_WRONG_STATE && so.reuse_violations == 1,
        "所有权窗口内客户端禁写——半帧腐坏源",
    );
    let cb = so.frame_done(s0);
    set.add(
        "B-504 回调 Held→Free",
        cb == E_OK && so.slot_state(s0) == Some(BufState::Free) && so.callbacks == 1,
        "frame_callback 归还客户端",
    );
    let w_free = so.client_write(s0, 1);
    set.add(
        "B-504 Free 恢复可写",
        w_free == E_OK && so.reuse_violations == 1,
        "归还后客户端继续画",
    );

    // —— 配额 ——
    let mut q3 = SurfaceOwnership::new(3);
    let a = q3.attach(1);
    let b = q3.attach(2);
    let c = q3.attach(3);
    let d = q3.attach(4);
    set.add(
        "B-504 三缓冲动画档",
        a == Ok(0) && b == Ok(1) && c == Ok(2),
        "配额 3 档可开",
    );
    set.add(
        "B-504 超配额拒绝计数",
        d == Err(E_QUOTA) && q3.quota_rejected == 1 && q3.active() == 3,
        "数量进表面配额",
    );
    let mut q2 = SurfaceOwnership::new(2);
    let x1 = q2.attach(1).unwrap_or(usize::MAX);
    let x2 = q2.attach(2).unwrap_or(usize::MAX);
    let x3 = q2.attach(3);
    set.add(
        "B-504 双缓冲配额 2",
        x1 != usize::MAX && x2 != usize::MAX && x3 == Err(E_QUOTA),
        "起步档双缓冲",
    );

    // —— 销毁与回收 ——
    let mut dz = SurfaceOwnership::new(2);
    let f1 = dz.attach(7).unwrap_or(9);
    let f2 = dz.attach(8).unwrap_or(9);
    let d_free = dz.destroy(f1);
    set.add(
        "B-504 Free 销毁即时回收",
        d_free == E_OK && dz.slot_state(f1) == Some(BufState::Gone) && dz.destroys == 1,
        "配额即时归还",
    );
    let re = dz.attach(9);
    set.add(
        "B-504 回收槽位复用",
        re == Ok(f1) && dz.active() == 2,
        "Gone 槽可再分配",
    );
    let stale = dz.client_write(f1, 1);
    set.add(
        "B-504 陈旧代数引用拒绝",
        stale == E_BAD_SLOT,
        "代数防 ABA——旧引用不落新缓冲",
    );
    let _ = dz.commit(f2);
    let _ = dz.begin_compose();
    let d_held = dz.destroy(f2);
    set.add(
        "B-504 Held 销毁延迟到回调",
        d_held == E_OK && dz.slot_state(f2) == Some(BufState::Dying) && dz.destroy_while_held == 1,
        "不撕正在合成的缓冲",
    );
    let cb_d = dz.frame_done(f2);
    set.add(
        "B-504 Dying 回调后 Gone",
        cb_d == E_OK && dz.slot_state(f2) == Some(BufState::Gone) && dz.destroys == 2,
        "回收记账完成",
    );

    // —— 并发压测（判据：零半帧）——
    let clean = run_drill(2, 0xB504_0001, 4096, true);
    set.add(
        "B-504 并发压测零半帧（守纪律）",
        clean.reuse_violations == 0 && clean.commits > 0 && clean.callbacks > 0,
        "4096 步交错零违规",
    );
    let clean3 = run_drill(3, 0xB504_0002, 4096, true);
    set.add(
        "B-504 三缓冲压测零半帧",
        clean3.reuse_violations == 0 && clean3.quota == 3,
        "动画档同纪律",
    );
    let evil = run_drill(2, 0xB504_0003, 4096, false);
    let mut caught = evil.reuse_violations > 0;
    if !caught {
        for seed in 1..8u64 {
            if run_drill(2, seed, 4096, false).reuse_violations > 0 {
                caught = true;
                break;
            }
        }
    }
    set.add(
        "B-504 越窗写被计数抓获",
        caught,
        "不守纪律客户端必现形——压测能抓住腐坏",
    );
    let multi = [
        run_drill(2, 1, 1024, true),
        run_drill(2, 2, 1024, true),
        run_drill(3, 3, 1024, true),
        run_drill(3, 4, 1024, true),
    ];
    set.add(
        "B-504 多种子压测零半帧",
        multi.iter().all(|s| s.reuse_violations == 0),
        "四种子各 1024 步全零",
    );

    // —— 错误叙事与不变量 ——
    set.add(
        "B-504 错误叙事体系",
        describe(E_WRONG_STATE).contains("所有权窗口") && describe(E_QUOTA).contains("frame_callback"),
        "每个失败有下一步建议",
    );
    let inv = run_drill(2, 0xBEEF, 2048, true);
    set.add(
        "B-504 账本守恒",
        inv.commits >= inv.callbacks && inv.active() <= inv.quota,
        "提交数≥回调数、活跃≤配额恒成立",
    );
    let mut half = SurfaceOwnership::new(2);
    let h1 = half.attach(1).unwrap_or(9);
    let _ = half.commit(h1);
    let notyet = half.frame_done(h1 + 99);
    set.add(
        "B-504 回调越界槽拒绝",
        notyet == E_BAD_SLOT,
        "槽位域校验",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bufown_full_lifecycle() {
        let mut so = SurfaceOwnership::new(2);
        let s = so.attach(42).unwrap();
        assert_eq!(so.commit(s), E_OK);
        assert_eq!(so.begin_compose(), Some(s));
        assert_eq!(so.client_write(s, 1), E_WRONG_STATE);
        assert_eq!(so.frame_done(s), E_OK);
        assert_eq!(so.client_write(s, 1), E_OK);
        assert_eq!(so.reuse_violations, 1);
    }

    #[test]
    fn bufown_quota_matrix() {
        for q in 2..=3 {
            let mut so = SurfaceOwnership::new(q);
            for i in 0..q {
                assert_eq!(so.attach(100 + i as u32), Ok(i));
            }
            assert_eq!(so.attach(999), Err(E_QUOTA));
        }
        // 配额钳制：1 → 2（起步下限）
        assert_eq!(SurfaceOwnership::new(1).quota, 2);
        assert_eq!(SurfaceOwnership::new(0).quota, 2);
        assert_eq!(SurfaceOwnership::new(9).quota, MAX_BUFS_PER_SURFACE);
    }

    #[test]
    fn bufown_gen_guard() {
        let mut so = SurfaceOwnership::new(2);
        let s = so.attach(1).unwrap();
        assert_eq!(so.client_write(s, 1), E_OK);
        let _ = so.destroy(s);
        assert_eq!(so.attach(2), Ok(s)); // 复用槽位
        // 旧代数引用拒绝
        assert_eq!(so.client_write(s, 1), E_BAD_SLOT);
        assert_eq!(so.client_write(s, 2), E_OK);
    }

    #[test]
    fn bufown_dying_path() {
        let mut so = SurfaceOwnership::new(2);
        let s = so.attach(1).unwrap();
        let _ = so.commit(s);
        let _ = so.begin_compose();
        assert_eq!(so.destroy(s), E_OK);
        assert_eq!(so.slot_state(s), Some(BufState::Dying));
        // Dying 中 commit 拒绝
        assert_eq!(so.commit(s), E_WRONG_STATE);
        // 回调后 Gone，配额归还
        assert_eq!(so.frame_done(s), E_OK);
        assert_eq!(so.slot_state(s), Some(BufState::Gone));
        assert_eq!(so.active(), 0);
    }

    #[test]
    fn bufown_drill_zero_half_frame() {
        for seed in [0u64, 1, 42, 0xB504, u64::MAX / 3] {
            let so = run_drill(2, seed, 2048, true);
            assert_eq!(so.reuse_violations, 0, "seed {seed} 半帧违规");
            assert!(so.active() <= so.quota);
        }
    }

    #[test]
    fn bufown_drill_catches_violation() {
        // 不守纪律的客户端必然被抓（多种子 4096 步至少一次越窗写）
        let mut caught = false;
        for seed in [7u64, 1, 2, 3, 4, 5, 6, 8, 9, 10] {
            if run_drill(2, seed, 4096, false).reuse_violations > 0 {
                caught = true;
                break;
            }
        }
        assert!(caught, "压测必须能现形");
        // 检测器确定性证明：Held/Dying 中写必计违规
        let mut so = SurfaceOwnership::new(2);
        let s = so.attach(1).unwrap();
        let _ = so.commit(s);
        let _ = so.begin_compose();
        assert_eq!(so.client_write(s, 1), E_WRONG_STATE);
        assert_eq!(so.reuse_violations, 1);
        let _ = so.frame_done(s);
        let _ = so.commit(s);
        let _ = so.begin_compose();
        let _ = so.destroy(s); // → Dying
        assert_eq!(so.client_write(s, 1), E_WRONG_STATE);
        assert_eq!(so.reuse_violations, 2);
    }

    #[test]
    fn bufown_all_checks_pass() {
        let set = run_bufown_checks();
        assert!(set.len() >= 20, "B-504 CheckSet 应≥20 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-504 check {} failed: {}", c.name, c.detail);
        }
    }
}
