// unx1209_u1209 —— UNX-F1209 Polling 态与现存轮询栈对账账（三路径运行期映射）
// 判据原文 UNX-F1209-J1：现存三路径（clock/serial/ps2）运行期映射登记率
// 100%（盘点点→运行期采集点逐一对上），映射与实际活动一致率 100%（孤儿/
// 幽灵双检零），对账周期可配默认 ≤1s。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9bb0 独占（0x9bb0~0x9bef）。
//
// 设计（纯功能 no_std）：
//   * 三路径闭集：Clock / Serial / Ps2；
//   * 盘点-运行期双向映射：盘点点（静态声明）↔ 运行期采集点（动态登记）；
//   * 孤儿/幽灵双检：孤儿 = 运行期有盘点无；幽灵 = 盘点有运行期无；
//   * 对账周期可配：默认 1000ms（≤1s 判据），越界拒；
//   * 一致率账：映射登记率 × 双检零 ⇒ 100%。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9bb0 独占） ─────────────────────────────────────────────
pub const XC_RC_SEALED: u32 = 0x9BB1; // 封账后登记
pub const XC_RC_BADPATH: u32 = 0x9BB2; // 路径越三闭集
pub const XC_RC_DUP: u32 = 0x9BB3; // 采集点重复
pub const XC_RC_PERIOD: u32 = 0x9BB4; // 对账周期越界
pub const XC_RC_ORPHAN: u32 = 0x9BB5; // 孤儿采集点
pub const XC_RC_GHOST: u32 = 0x9BB6; // 幽灵盘点点
pub const XC_RC_FULL: u32 = 0x9BB7; // 容量满

pub const PATH_CLOCK: u32 = 0;
pub const PATH_SERIAL: u32 = 1;
pub const PATH_PS2: u32 = 2;
pub const PATH_N: usize = 3;

pub const DEF_PERIOD_MS: u64 = 1000; // 默认对账周期（判据 ≤1s）

/// 盘点点（静态声明：路径 + 点号 + 语义）。
#[derive(Clone, Copy)]
pub struct AuditPt {
    pub path: u32,
    pub pt_no: u32,
    pub semantic: u32, // 语义码（如 PIT 频率/COM 就绪/KBD 缓冲）
}

/// 运行期采集点（动态登记：路径 + 点号 + 活动 tick）。
#[derive(Clone, Copy)]
pub struct LivePt {
    pub path: u32,
    pub pt_no: u32,
    pub last_tick: u64,
    pub hits: u64,
}

impl LivePt {
    fn key(&self) -> (u32, u32) {
        (self.path, self.pt_no)
    }
}

impl AuditPt {
    fn key(&self) -> (u32, u32) {
        (self.path, self.pt_no)
    }
}

/// 对账账。
pub struct ReconBook {
    cap: usize,
    audit: Vec<AuditPt>,
    live: Vec<LivePt>,
    period_ms: u64,
    recon_rounds: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ReconBook {
    pub fn new(cap: usize) -> ReconBook {
        ReconBook {
            cap,
            audit: vec![],
            live: vec![],
            period_ms: DEF_PERIOD_MS,
            recon_rounds: 0,
            sealed: false,
            rejects: vec![],
        }
    }

    /// 配对账周期（≤1000ms 才收）。
    pub fn set_period(&mut self, ms: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "封账后配置"));
            return Err(XC_RC_SEALED);
        }
        if ms == 0 || ms > 1000 {
            self.rejects.push((XC_RC_PERIOD, "对账周期越界"));
            return Err(XC_RC_PERIOD);
        }
        self.period_ms = ms;
        Ok(())
    }

    pub fn period_ms(&self) -> u64 {
        self.period_ms
    }

    /// 盘点点登记（静态声明）。
    pub fn audit_add(&mut self, path: u32, pt_no: u32, semantic: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "封账后登记"));
            return Err(XC_RC_SEALED);
        }
        if path as usize >= PATH_N {
            self.rejects.push((XC_RC_BADPATH, "路径越三闭集"));
            return Err(XC_RC_BADPATH);
        }
        if self.audit.len() >= self.cap {
            self.rejects.push((XC_RC_FULL, "容量满"));
            return Err(XC_RC_FULL);
        }
        let p = AuditPt { path, pt_no, semantic };
        if self.audit.iter().any(|a| a.key() == p.key()) {
            self.rejects.push((XC_RC_DUP, "采集点重复"));
            return Err(XC_RC_DUP);
        }
        self.audit.push(p);
        Ok(())
    }

    /// 运行期采集点登记（动态活动）。
    pub fn live_add(&mut self, path: u32, pt_no: u32, tick: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "封账后登记"));
            return Err(XC_RC_SEALED);
        }
        if path as usize >= PATH_N {
            self.rejects.push((XC_RC_BADPATH, "路径越三闭集"));
            return Err(XC_RC_BADPATH);
        }
        if let Some(l) = self.live.iter_mut().find(|l| l.key() == (path, pt_no)) {
            l.last_tick = tick;
            l.hits += 1;
            return Ok(());
        }
        if self.live.len() >= self.cap {
            self.rejects.push((XC_RC_FULL, "容量满"));
            return Err(XC_RC_FULL);
        }
        self.live.push(LivePt { path, pt_no, last_tick: tick, hits: 1 });
        Ok(())
    }

    /// 孤儿检：运行期有、盘点无的键。
    pub fn orphans(&self) -> Vec<(u32, u32)> {
        self.live
            .iter()
            .filter(|l| !self.audit.iter().any(|a| a.key() == l.key()))
            .map(|l| l.key())
            .collect()
    }

    /// 幽灵检：盘点有、运行期无的键。
    pub fn ghosts(&self) -> Vec<(u32, u32)> {
        self.audit
            .iter()
            .filter(|a| !self.live.iter().any(|l| l.key() == a.key()))
            .map(|a| a.key())
            .collect()
    }

    /// 映射登记率（千分比）：盘点点逐一对上运行期。
    pub fn map_rate_bps(&self) -> u32 {
        if self.audit.is_empty() {
            return 0;
        }
        let hit = self.audit.iter().filter(|a| self.live.iter().any(|l| l.key() == a.key())).count();
        (hit as u64 * 10_000 / self.audit.len() as u64) as u32
    }

    /// 对账一轮（双检零才过）。
    pub fn reconcile(&mut self) -> Result<(usize, usize), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "封账后对账"));
            return Err(XC_RC_SEALED);
        }
        let o = self.orphans();
        let g = self.ghosts();
        self.recon_rounds += 1;
        if !o.is_empty() {
            self.rejects.push((XC_RC_ORPHAN, "孤儿采集点"));
            return Err(XC_RC_ORPHAN);
        }
        if !g.is_empty() {
            self.rejects.push((XC_RC_GHOST, "幽灵盘点点"));
            return Err(XC_RC_GHOST);
        }
        Ok((0, 0))
    }

    /// 三路径全覆盖：每路径至少一个盘点点。
    pub fn all_paths(&self) -> bool {
        (0..PATH_N as u32).all(|p| self.audit.iter().any(|a| a.path == p))
    }

    pub fn recon_rounds(&self) -> u64 {
        self.recon_rounds
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    pub fn live_len(&self) -> usize {
        self.live.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：三路径映射编排（判据主体路径） ────────────────────────────

/// 标准三路径编排：每路径 3 盘点点 → 全部运行期对上 → 对账零孤儿幽灵。
pub fn three_path_run() -> Result<(u32, u64), u32> {
    let mut bk = ReconBook::new(16);
    bk.set_period(1000)?;
    // 盘点：clock×3 / serial×3 / ps2×3。
    for path in 0..3u32 {
        for pt in 0..3u32 {
            bk.audit_add(path, pt, path * 10 + pt)?;
        }
    }
    // 运行期：全部对上（tick 递增）。
    for path in 0..3u32 {
        for pt in 0..3u32 {
            bk.live_add(path, pt, (100 + path * 10 + pt) as u64)?;
        }
    }
    let (o, g) = bk.reconcile()?;
    let rate = bk.map_rate_bps();
    if o != 0 || g != 0 || rate != 10_000 || !bk.all_paths() {
        return Err(XC_RC_ORPHAN);
    }
    Ok((rate, bk.recon_rounds()))
}

/// 孤儿/幽灵双检演练：注入两侧破坏均被抓。
pub fn dual_check_run() -> Result<(bool, bool), u32> {
    // 孤儿：运行期多一个。
    let mut b1 = ReconBook::new(8);
    b1.audit_add(PATH_CLOCK, 0, 1)?;
    b1.live_add(PATH_CLOCK, 0, 1)?;
    b1.live_add(PATH_SERIAL, 9, 2)?; // 无盘点 → 孤儿
    let orphan_caught = b1.reconcile() == Err(XC_RC_ORPHAN);
    // 幽灵：盘点多一个。
    let mut b2 = ReconBook::new(8);
    b2.audit_add(PATH_CLOCK, 0, 1)?;
    b2.audit_add(PATH_PS2, 5, 2)?; // 无运行期 → 幽灵
    b2.live_add(PATH_CLOCK, 0, 1)?;
    let ghost_caught = b2.reconcile() == Err(XC_RC_GHOST);
    Ok((orphan_caught, ghost_caught))
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①路径越闭集 ②采集点重复 ③对账周期越界 ④封账后对账
// ⑤孤儿检 ⑥幽灵检。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 路径越闭集 → XC_RC_BADPATH
    total += 1;
    let mut b = ReconBook::new(8);
    if b.audit_add(9, 0, 1) == Err(XC_RC_BADPATH) {
        hit += 1;
    }

    // ② 采集点重复 → XC_RC_DUP
    total += 1;
    let mut b2 = ReconBook::new(8);
    let _ = b2.audit_add(PATH_CLOCK, 0, 1);
    if b2.audit_add(PATH_CLOCK, 0, 2) == Err(XC_RC_DUP) {
        hit += 1;
    }

    // ③ 对账周期越界 → XC_RC_PERIOD
    total += 1;
    let mut b3 = ReconBook::new(8);
    if b3.set_period(2000) == Err(XC_RC_PERIOD) {
        hit += 1;
    }

    // ④ 封账后对账 → XC_RC_SEALED
    total += 1;
    let mut b4 = ReconBook::new(8);
    b4.seal();
    if b4.reconcile() == Err(XC_RC_SEALED) {
        hit += 1;
    }

    // ⑤ 孤儿检：运行期多一个必抓。
    total += 1;
    let mut b5 = ReconBook::new(8);
    let _ = b5.audit_add(PATH_CLOCK, 0, 1);
    let _ = b5.live_add(PATH_CLOCK, 0, 1);
    let _ = b5.live_add(PATH_SERIAL, 7, 2);
    if b5.reconcile() == Err(XC_RC_ORPHAN) {
        hit += 1;
    }

    // ⑥ 幽灵检：盘点多一个必抓。
    total += 1;
    let mut b6 = ReconBook::new(8);
    let _ = b6.audit_add(PATH_CLOCK, 0, 1);
    let _ = b6.audit_add(PATH_PS2, 3, 2);
    let _ = b6.live_add(PATH_CLOCK, 0, 1);
    if b6.reconcile() == Err(XC_RC_GHOST) {
        hit += 1;
    }

    (hit, total)
}
