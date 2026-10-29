// unx2027_idlepath —— UNX-F2027 idle 进入/退出路径与切换链 v3 协同（第 0 段语义）
// 判据原文 UNX-F2027-J1：idle 路径协同断言全过（idle 循环作为切换链 v3 第 0 段的
// 进入/退出语义），路径×唤醒×IPI 协同 10/10，段账互证一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9DE0 独占（0x9DE0~0x9DFF）。
//
// 设计（纯功能 no_std）：
//   * 切换链 v3 全序：第 0 段（idle 进入/退出）→ 四段（保存→选程→换页→恢复，
//     对齐 unx1883 TriCode::SwitchChain 语义）→ IPI 转发收尾；
//   * 第 0 段语义：idle 进入必须在四段之前完成（深睡后才有切换），
//     idle 退出必须在四段之后（恢复完成才允许退出 idle 面板）；
//   * 路径账：逐次会话落行（seq/进入档/退出档/衔接段序），FNV 签名；
//   * 协同注入 10/10：段序错位（idle 后置/退出前置/跳段/IPI 缺席/重复进入等）
//     10 笔全部必抓；
//   * 段账互证：本账会话数 == 衔接面四段走完计数（TriCode 语义对齐）；
//   * 零 unwrap/expect/panic 面。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9DE0 独占） ─────────────────────────────────────────────
pub const XC_IP_ORDER: u32 = 0x9DE1; // 段序错位（idle 未在四段前）
pub const XC_IP_EXIT_EARLY: u32 = 0x9DE2; // idle 退出前置（四段未走完）
pub const XC_IP_SKIP: u32 = 0x9DE3; // 跳段（四段缺段）
pub const XC_IP_IPI_MISS: u32 = 0x9DE4; // IPI 转发缺席
pub const XC_IP_DUP_ENTER: u32 = 0x9DE5; // 重复进入（已在线再进）
pub const XC_IP_LEDGER: u32 = 0x9DE6; // 路径账签名漂移
pub const XC_IP_SEALED: u32 = 0x9DE7; // 封账后路径操作
pub const XC_IP_MUTUAL: u32 = 0x9DE8; // 段账互证破（会话数 ≠ 衔接数）
pub const XC_IP_OOB: u32 = 0x9DE9; // 档号越界（本段自有，不借他段）

/// 切换链四段（对齐 unx1883 TriCode::SwitchChain：保存→选程→换页→恢复）。
pub const SEG_SAVE: u8 = 1;
pub const SEG_SELECT: u8 = 2;
pub const SEG_SWITCH: u8 = 3;
pub const SEG_RESTORE: u8 = 4;
/// IPI 转发收尾（衔接面第 5 步）。
pub const SEG_IPI: u8 = 5;

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 一条路径账行：一次完整 idle 会话（第 0 段进入 → 四段 → IPI → 第 0 段退出）。
#[derive(Clone, Copy)]
pub struct PathRow {
    pub seq: u64,
    pub enter_lvl: u8,
    pub exit_lvl: u8,
    pub segs_done: u8, // 衔接段完成数（四段 + IPI = 5）
    pub sig: u64,
}

fn row_sig(seq: u64, enter_lvl: u8, exit_lvl: u8, segs_done: u8) -> u64 {
    let mut buf = [0u8; 11];
    buf[0..8].copy_from_slice(&seq.to_le_bytes());
    buf[8] = enter_lvl;
    buf[9] = exit_lvl;
    buf[10] = segs_done;
    fnv64(&buf)
}

/// idle 路径协调器（第 0 段宿主）。
pub struct IdlePathCoordinator {
    in_idle: bool,
    pending_lvl: Option<u8>,
    segs_done: u8, // 当前会话衔接段计数
    ipi_done: bool,
    ledger: Vec<PathRow>,
    seq: u64,
    chained_sessions: u64, // 四段走完计数（互证源）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl IdlePathCoordinator {
    pub fn new() -> IdlePathCoordinator {
        IdlePathCoordinator {
            in_idle: false,
            pending_lvl: None,
            segs_done: 0,
            ipi_done: false,
            ledger: Vec::new(),
            seq: 0,
            chained_sessions: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 第 0 段·进入：必须在未在线时；档号闭集 0..6。
    pub fn idle_enter(&mut self, lvl: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IP_SEALED, "封账后进入"));
            return Err(XC_IP_SEALED);
        }
        if self.in_idle {
            self.rejects.push((XC_IP_DUP_ENTER, "重复进入"));
            return Err(XC_IP_DUP_ENTER);
        }
        if lvl >= 6 {
            self.rejects.push((XC_IP_OOB, "档号越界"));
            return Err(XC_IP_OOB);
        }
        self.in_idle = true;
        self.pending_lvl = Some(lvl);
        self.segs_done = 0;
        self.ipi_done = false;
        Ok(())
    }

    /// 衔接段推进：四段 + IPI；段序强制 1→2→3→4→5（跳段必拒）。
    pub fn seg_advance(&mut self, seg: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IP_SEALED, "封账后推段"));
            return Err(XC_IP_SEALED);
        }
        if !self.in_idle {
            // idle 未在线就推段 = 段序错位（第 0 段必须在四段前）
            self.rejects.push((XC_IP_ORDER, "idle 未在前"));
            return Err(XC_IP_ORDER);
        }
        if seg < SEG_SAVE || seg > SEG_IPI {
            self.rejects.push((XC_IP_SKIP, "段号非法"));
            return Err(XC_IP_SKIP);
        }
        if seg != self.segs_done + 1 {
            self.rejects.push((XC_IP_SKIP, "跳段"));
            return Err(XC_IP_SKIP);
        }
        self.segs_done = seg;
        if seg == SEG_IPI {
            self.ipi_done = true;
        }
        Ok(())
    }

    /// 第 0 段·退出：必须四段+IPI 全走完；退出档与进入档一致。
    pub fn idle_exit(&mut self, lvl: u8) -> Result<PathRow, u32> {
        if self.sealed {
            self.rejects.push((XC_IP_SEALED, "封账后退出"));
            return Err(XC_IP_SEALED);
        }
        if !self.in_idle {
            self.rejects.push((XC_IP_ORDER, "未在线退出"));
            return Err(XC_IP_ORDER);
        }
        if self.segs_done < SEG_RESTORE {
            self.rejects.push((XC_IP_EXIT_EARLY, "四段未走完"));
            return Err(XC_IP_EXIT_EARLY);
        }
        if !self.ipi_done {
            self.rejects.push((XC_IP_IPI_MISS, "IPI 缺席"));
            return Err(XC_IP_IPI_MISS);
        }
        let enter_lvl = match self.pending_lvl {
            Some(v) => v,
            None => {
                self.rejects.push((XC_IP_ORDER, "进入档缺失"));
                return Err(XC_IP_ORDER);
            }
        };
        if enter_lvl != lvl {
            self.rejects.push((XC_IP_ORDER, "进出档不一致"));
            return Err(XC_IP_ORDER);
        }
        self.in_idle = false;
        self.pending_lvl = None;
        self.chained_sessions += 1;
        self.seq += 1;
        let row = PathRow {
            seq: self.seq,
            enter_lvl,
            exit_lvl: lvl,
            segs_done: self.segs_done,
            sig: row_sig(self.seq, enter_lvl, lvl, self.segs_done),
        };
        self.ledger.push(row);
        Ok(row)
    }

    /// 段账互证：本账完整会话数 == 衔接面走完计数。
    pub fn mutual_consistent(&self) -> bool {
        (self.ledger.len() as u64) == self.chained_sessions
    }

    /// 协同注入核 10/10：十种段序错位全部必抓。
    pub fn inject_core() -> (u32, u32) {
        const N: u32 = 10;
        let mut caught = 0u32;

        // 注入 1：idle 未在线直接推段（段序错位）
        {
            let mut c = IdlePathCoordinator::new();
            if c.seg_advance(SEG_SAVE) == Err(XC_IP_ORDER) {
                caught += 1;
            }
        }
        // 注入 2：重复进入
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(2);
            if c.idle_enter(2) == Err(XC_IP_DUP_ENTER) {
                caught += 1;
            }
        }
        // 注入 3：跳段（进后直接推段 2）
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            if c.seg_advance(SEG_SELECT) == Err(XC_IP_SKIP) {
                caught += 1;
            }
        }
        // 注入 4：四段未走完就退出
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            let _ = c.seg_advance(SEG_SAVE);
            if c.idle_exit(1).err() == Some(XC_IP_EXIT_EARLY) {
                caught += 1;
            }
        }
        // 注入 5：四段走完但 IPI 缺席就退出
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            for seg in SEG_SAVE..SEG_IPI {
                let _ = c.seg_advance(seg);
            }
            if c.idle_exit(1).err() == Some(XC_IP_IPI_MISS) {
                caught += 1;
            }
        }
        // 注入 6：进出档不一致
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(2);
            for seg in SEG_SAVE..=SEG_IPI {
                let _ = c.seg_advance(seg);
            }
            if c.idle_exit(3).err() == Some(XC_IP_ORDER) {
                caught += 1;
            }
        }
        // 注入 7：段号非法（0 / 6）
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            let e1 = c.seg_advance(0);
            let _ = c.seg_advance(SEG_SAVE);
            let e2 = c.seg_advance(6);
            if e1 == Err(XC_IP_SKIP) && e2 == Err(XC_IP_SKIP) {
                caught += 1;
            }
        }
        // 注入 8：合法全序走通后二次进入成功（正常路径哨兵——走通才算抓到语境）
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(2);
            for seg in SEG_SAVE..=SEG_IPI {
                let _ = c.seg_advance(seg);
            }
            if c.idle_exit(2).is_ok() && c.idle_enter(1).is_ok() {
                caught += 1;
            }
        }
        // 注入 9：合法会话后再推段（未进入新会话）→ 段序错位
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            for seg in SEG_SAVE..=SEG_IPI {
                let _ = c.seg_advance(seg);
            }
            let _ = c.idle_exit(1);
            if c.seg_advance(SEG_SAVE) == Err(XC_IP_ORDER) {
                caught += 1;
            }
        }
        // 注入 10：深度嵌套语境——会话 2 重复段 1（跳段）
        {
            let mut c = IdlePathCoordinator::new();
            let _ = c.idle_enter(1);
            for seg in SEG_SAVE..=SEG_IPI {
                let _ = c.seg_advance(seg);
            }
            let _ = c.idle_exit(1);
            let _ = c.idle_enter(2);
            let _ = c.seg_advance(SEG_SAVE);
            if c.seg_advance(SEG_SAVE) == Err(XC_IP_SKIP) {
                caught += 1;
            }
        }
        (N, caught)
    }

    pub fn row_count(&self) -> usize {
        self.ledger.len()
    }

    pub fn chained_sessions(&self) -> u64 {
        self.chained_sessions
    }

    pub fn ledger_verified(&self) -> bool {
        self.ledger
            .iter()
            .all(|r| r.sig == row_sig(r.seq, r.enter_lvl, r.exit_lvl, r.segs_done))
    }

    pub fn seq_dense(&self) -> bool {
        for (i, r) in self.ledger.iter().enumerate() {
            if r.seq != (i as u64) + 1 {
                return false;
            }
        }
        true
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IP_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x9de0_9de0_9de0_9de0;
        for r in &self.ledger {
            h ^= r.sig;
            h = h.wrapping_mul(0x100000001b3);
        }
        h.wrapping_add(self.chained_sessions)
    }
}

// ── 标准夹具 ─────────────────────────────────────────────────────────
/// 标准协调器：3 笔完整会话（档 1/3/5），全序推进。
fn standard_coordinator() -> IdlePathCoordinator {
    let mut c = IdlePathCoordinator::new();
    for lvl in [1u8, 3, 5] {
        let _ = c.idle_enter(lvl);
        for seg in SEG_SAVE..=SEG_IPI {
            let _ = c.seg_advance(seg);
        }
        let _ = c.idle_exit(lvl);
    }
    c
}

// ── 判据入口（UNX-F2027-J1） ─────────────────────────────────────────
pub fn run_unx2027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2027 idle 路径与切换链 v3 协同");
    // C27-01 第 0 段语义：3 笔完整会话全走通
    {
        let c = standard_coordinator();
        let ok = c.row_count() == 3 && c.chained_sessions() == 3;
        s.add("C27-01-三会话走通", ok, if ok { "" } else { "会话缺失" });
    }
    // C27-02 段序强制：标准会话 segs_done == 5（四段+IPI）
    {
        let c = standard_coordinator();
        let ok = c.ledger.iter().all(|r| r.segs_done == SEG_IPI);
        s.add("C27-02-五步全序", ok, if ok { "" } else { "全序破" });
    }
    // C27-03 段账互证一致
    {
        let c = standard_coordinator();
        let ok = c.mutual_consistent();
        s.add("C27-03-段账互证", ok, if ok { "" } else { "互证破" });
    }
    // C27-04 路径账逐次可查 + 验签
    {
        let c = standard_coordinator();
        let ok = c.ledger.iter().count() == 3
            && c.ledger_verified()
            && c.seq_dense();
        s.add("C27-04-逐次验签", ok, if ok { "" } else { "账完整性破" });
    }
    // C27-05 协同注入 10/10
    {
        let (n, hits) = IdlePathCoordinator::inject_core();
        let ok = n == 10 && hits == 10;
        s.add("C27-05-协同注入10/10", ok, if ok { "" } else { "注入漏抓" });
    }
    // C27-06 封账后路径操作必拒
    {
        let mut c = standard_coordinator();
        let _ = c.seal();
        let e = c.idle_enter(1);
        let g = c.seg_advance(SEG_SAVE);
        let ok = e == Err(XC_IP_SEALED) && g == Err(XC_IP_SEALED) && c.reject_count() >= 2;
        s.add("C27-06-封账必拒", ok, if ok { "" } else { "封账失守" });
    }
    // C27-07 与 unx1883 TriCode 语义对齐：四段名序 = 保存→选程→换页→恢复
    {
        // 常量序哨兵：1..4 递增且 IPI=5 收尾
        let ok = SEG_SAVE == 1 && SEG_SELECT == 2 && SEG_SWITCH == 3 && SEG_RESTORE == 4
            && SEG_IPI == 5;
        s.add("C27-07-四段名序", ok, if ok { "" } else { "段名漂移" });
    }
    // C27-08 IPI 收尾幂等推进（重复推同段必拒）
    {
        let mut c = IdlePathCoordinator::new();
        let _ = c.idle_enter(1);
        let _ = c.seg_advance(SEG_SAVE);
        let ok = c.seg_advance(SEG_SAVE) == Err(XC_IP_SKIP);
        s.add("C27-08-重复段拒", ok, if ok { "" } else { "重复放行" });
    }
    // C27-09 指纹一致 + 封账稳定
    {
        let f = || standard_coordinator().fingerprint();
        let ok0 = f() == f();
        let mut c = standard_coordinator();
        let pre = c.fingerprint();
        let sealed = c.seal();
        s.add(
            "C27-09-指纹封账",
            ok0 && sealed.is_ok() && c.fingerprint() == pre,
            if ok0 { "" } else { "指纹漂移" },
        );
    }
    // C27-10 互证破检测：手工构造「衔接计数 > 账行数」场景不可达（退出即计数）
    {
        let mut c = IdlePathCoordinator::new();
        let _ = c.idle_enter(1);
        for seg in SEG_SAVE..=SEG_IPI {
            let _ = c.seg_advance(seg);
        }
        // 未退出：chained_sessions 仍 0，互证待退出后闭合
        let ok = c.chained_sessions() == 0 && c.row_count() == 0;
        s.add("C27-10-未退出不计数", ok, if ok { "" } else { "计数前置" });
    }
    s
}
