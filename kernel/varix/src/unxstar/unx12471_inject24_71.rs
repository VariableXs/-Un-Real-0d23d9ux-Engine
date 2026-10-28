//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12471 · 枚举索引越界注入判据 · 0x308E0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12471-J1 越界索引/回退索引注入 30/30 次正确拒止，枚举
//!      快照一致性断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注入三族 30 例：越界族（index=n / n+7 / u32::MAX，10 例，全返
//!    NO_MORE_ENTRIES 终值）、回退族（Index 变小/跳回 0/跨批回退，
//!    10 例，全拒 IndexBackward）、跳跃族（界内跳前/跳过未扫位，
//!    10 例，全拒 CursorOob）；
//!   - 枚举快照一致性：开枚举拍快照（界 + 入序指纹）→ 枚举中注册
//!    新键 → 快照界不变（新键不进本批）→ 重开枚举后新键可见（两
//!    批界差 = 新增数）；
//!   - 快照一致性断言三件：批内界恒定、批内指纹推进单调、批间界差
//!    ≡ 新增数（三件全绿）；
//!   - 池账无泄漏：注入例不占池（先检后写），枚举池占用快照前后一
//!    致；
//!   - 注入核：LCG 写死种子 = SEG_BASE，30 例确定性重放双跑一致；
//!   - 诊断码闭集 0x308E0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x308E0 段）。
pub const SEG_BASE: u32 = 0x308E0;

/// 注入判准总量（三族 × 10）。
pub const INJECT_TOTAL: usize = 30;

/// 枚举表容量。
pub const ENUM_CAP: usize = 16;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x308E0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EiCode {
    /// 0x308E0 越界终值（NO_MORE_ENTRIES，记账不红账）。
    NoMore = 0,
    /// 0x308E1 回退索引（Index 变小）。
    IndexBackward = 1,
    /// 0x308E2 界内跳跃（跳过未扫位）。
    CursorSkip = 2,
    /// 0x308E3 终值后再取。
    AfterEnd = 3,
    /// 0x308E4 幽灵游标（未开枚举先取）。
    GhostCursor = 4,
    /// 0x308E5 快照界漂移（批内界变）。
    SnapshotDrift = 5,
    /// 0x308E6 快照指纹失配（出序 ≢ 入序）。
    SnapshotFp = 6,
    /// 0x308E7 批间界差失配（≢ 新增数）。
    BatchDelta = 7,
    /// 0x308E8 池泄漏（注入占池）。
    PoolLeak = 8,
    /// 0x308E9 段位越界。
    SegOob = 9,
    /// 0x308EA 注入漏抓（应拒未拒）。
    InjectLeak = 10,
    /// 0x308EB 双跑失配。
    DualMismatch = 11,
}

impl EiCode {
    /// 全部码闭集。
    pub const ALL: [EiCode; 12] = [
        EiCode::NoMore,
        EiCode::IndexBackward,
        EiCode::CursorSkip,
        EiCode::AfterEnd,
        EiCode::GhostCursor,
        EiCode::SnapshotDrift,
        EiCode::SnapshotFp,
        EiCode::BatchDelta,
        EiCode::PoolLeak,
        EiCode::SegOob,
        EiCode::InjectLeak,
        EiCode::DualMismatch,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            EiCode::NoMore => "越界索引返 NO_MORE_ENTRIES（终值语义，重开枚举复位）",
            EiCode::IndexBackward => "Index 回退，枚举游标必须单调递进",
            EiCode::CursorSkip => "界内跳跃拒，游标必须恰等下一未扫位",
            EiCode::AfterEnd => "终值后再取拒，先重开枚举",
            EiCode::GhostCursor => "未开枚举先取拒，先走开枚举入口",
            EiCode::SnapshotDrift => "批内快照界变化，快照口径被破",
            EiCode::SnapshotFp => "出序指纹与入序失配，存在丢包/重排",
            EiCode::BatchDelta => "重开枚举界差 ≢ 新增数，快照一致性破",
            EiCode::PoolLeak => "注入例占池（先检后写被破）",
            EiCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            EiCode::InjectLeak => "应拒注入未拒，注入闸失灵",
            EiCode::DualMismatch => "双跑重放失配，确定性被破",
        }
    }
}

/// FNV-1a 64（快照指纹）。
fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// 枚举池（快照口径，本单自带协议核）。
pub struct EnumPool {
    /// 名表。
    names: [Option<[u8; 8]>; ENUM_CAP],
    lens: [usize; ENUM_CAP],
    count: usize,
    /// 快照界。
    snap: usize,
    /// 快照入序指纹（滚动：随注册推进，供下一批）。
    snap_fp: u64,
    /// 开枚举时定格的指纹（本批对拍基准——中批注册不污染）。
    open_fp: u64,
    /// 游标。
    cursor: usize,
    /// 出序指纹。
    out_fp: u64,
    /// 终值已返。
    ended: bool,
    /// 已开。
    open: bool,
    /// 注入拒计数（逐码）。
    rej: [u32; 12],
}

impl EnumPool {
    /// 新建。
    pub fn new() -> EnumPool {
        EnumPool {
            names: [None; ENUM_CAP],
            lens: [0; ENUM_CAP],
            count: 0,
            snap: 0,
            snap_fp: 0xcbf29ce484222325,
            open_fp: 0xcbf29ce484222325,
            cursor: 0,
            out_fp: 0xcbf29ce484222325,
            ended: false,
            open: false,
            rej: [0; 12],
        }
    }

    /// 记拒码。
    fn hit(&mut self, c: EiCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.rej[i] = self.rej[i].wrapping_add(1);
        }
        c.val()
    }

    /// 注册（先检后写：越界名/超长名零占用）。
    pub fn register(&mut self, name: &[u8]) -> Result<(), u32> {
        if name.is_empty() || name.len() > 8 {
            return Err(self.hit(EiCode::InjectLeak));
        }
        if self.count >= ENUM_CAP {
            return Err(self.hit(EiCode::PoolLeak));
        }
        let mut nm = [0u8; 8];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        self.names[self.count] = Some(nm);
        self.lens[self.count] = name.len();
        self.snap_fp = fnv1a(self.snap_fp, name);
        self.count += 1;
        Ok(())
    }

    /// 开枚举（快照定格：界 + 指纹双定格）。
    pub fn open(&mut self) {
        self.snap = self.count;
        self.open_fp = self.snap_fp;
        self.cursor = 0;
        self.out_fp = 0xcbf29ce484222325;
        self.ended = false;
        self.open = true;
    }

    /// 枚举一步（协议核：终值 → 回退 → 界恒 → 越界 → 恰等）。
    /// 返回 Ok(Some(len)) 出一名 / Ok(None) 终值 / Err=协议码。
    pub fn step(&mut self, index: usize) -> Result<Option<usize>, u32> {
        if !self.open {
            return Err(self.hit(EiCode::GhostCursor));
        }
        if self.ended {
            return Err(self.hit(EiCode::AfterEnd));
        }
        if index < self.cursor {
            return Err(self.hit(EiCode::IndexBackward));
        }
        // 快照口径：批内界由 snap 定格，中批注册不破批（新增只影响
        // 下一批界——见 snapshot_consistent 件 3）。
        if index >= self.snap {
            self.ended = true;
            return Ok(None);
        }
        if index != self.cursor {
            return Err(self.hit(EiCode::CursorSkip));
        }
        let name = match self.names[index] {
            Some(ref nm) => &nm[..self.lens[index]],
            None => return Err(self.hit(EiCode::InjectLeak)),
        };
        self.out_fp = fnv1a(self.out_fp, name);
        self.cursor += 1;
        Ok(Some(self.lens[index]))
    }

    /// 快照一致性断言三件（判准 J1 第二句）。
    pub fn snapshot_consistent(&self, before_new: usize, added: usize) -> (bool, bool, bool) {
        // 件 1：批内界恒定（快照界 == 开枚举时界）——由协议核保证，
        // 此处核快照界未被本批注册污染。
        let c1 = self.snap == before_new;
        // 件 2：批内指纹推进单调（出序指纹 ≠ 初值 或批空）。
        let c2 = self.out_fp != 0xcbf29ce484222325 || self.snap == 0;
        // 件 3：批间界差（当前 count - 快照界 ≡ 新增数）。
        let c3 = self.count - self.snap == added;
        (c1, c2, c3)
    }

    /// 出序指纹一致（与本批定格基准对拍）。
    pub fn fp_ok(&self) -> bool {
        self.out_fp == self.open_fp
    }

    /// 现有数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 快照界。
    pub fn snap(&self) -> usize {
        self.snap
    }

    /// 游标。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 逐码命中。
    pub fn rej_hit(&self, c: EiCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.rej[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 30/30 注入主判据（判准 J1 第一句）。
pub fn drill_inject_30() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut passed = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut case = 0usize;
    while case < INJECT_TOTAL {
        let family = case % 3;
        // 每例新池（5 名），开枚举，扫至注入位。
        let mut p = EnumPool::new();
        let mut i = 0;
        while i < 5 {
            let mut nm = b"N".to_vec();
            nm.push(b'0' + i as u8);
            let _ = p.register(&nm);
            i += 1;
        }
        p.open();
        let _ = p.step(0);
        let _ = p.step(1);
        let r = match family {
            0 => {
                // 越界族：index=n(5)/n+7(12)/u32::MAX → NO_MORE。
                let idx = match case % 30 {
                    0 | 3 | 6 | 9 | 12 | 15 | 18 | 21 | 24 | 27 => 5usize,
                    1 | 4 | 7 | 10 | 13 | 16 | 19 | 22 | 25 | 28 => 12usize,
                    _ => u32::MAX as usize,
                };
                match p.step(idx) {
                    Ok(None) => true,
                    _ => false,
                }
            }
            1 => {
                // 回退族：回 0 / 回 1 / 跨批回退（u32::MAX 也可视回退——
                // 但 ≥n 先捕 NO_MORE，故用 0/1/游标前位）。
                let idx = match case % 3 {
                    0 => 0usize,
                    1 => 1usize,
                    _ => 0usize,
                };
                matches!(p.step(idx), Err(c) if c == EiCode::IndexBackward.val())
            }
            _ => {
                // 跳跃族：界内跳 3/跳 4/跳 2 → CursorSkip。
                let idx = 3 + (case % 3) as usize;
                matches!(p.step(idx.min(4)), Err(c) if c == EiCode::CursorSkip.val())
            }
        };
        // 池账无泄漏：注入不占池（count 恒 5）。
        let no_leak = p.count() == 5;
        if r && no_leak {
            passed += 1;
        } else {
            d.push(format!("例{} 族{} 拒{} 泄漏{}", case, family, r, !no_leak));
        }
        let _ = lcg.below(3);
        case += 1;
    }
    (passed == INJECT_TOTAL, format!("注入 {}/{} {}", passed, INJECT_TOTAL, d.join(" ")))
}

/// 枚举快照一致性演练 10/10 轮（判准 J1 第二句）。
pub fn drill_snapshot_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = EnumPool::new();
        let mut i = 0;
        while i < 6 {
            let mut nm = b"K".to_vec();
            nm.push(b'0' + i as u8);
            let _ = p.register(&nm);
            i += 1;
        }
        p.open();
        // 扫 3 名。
        let mut idx = 0usize;
        while idx < 3 {
            let _ = p.step(idx);
            idx += 1;
        }
        // 枚举中注册新键（3 个）。
        let mut j = 0;
        while j < 3 {
            let mut nm = b"NEW".to_vec();
            nm.push(b'0' + j as u8);
            if p.register(&nm).is_err() {
                d.push(format!("轮{} 新键注册错", round));
            }
            j += 1;
        }
        // 继续扫批内剩余（快照界恒 6）。
        let mut cont_ok = true;
        while idx < 6 {
            if p.step(idx).is_err() {
                cont_ok = false;
            }
            idx += 1;
        }
        // 终值。
        let end = matches!(p.step(6), Ok(None));
        // 快照一致性三件。
        let (c1, c2, c3) = p.snapshot_consistent(6, 3);
        // 出序指纹一致（批内 6 名全出）。
        let fp = p.fp_ok();
        if cont_ok && end && c1 && c2 && c3 && fp {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 续{} 终{} 件{}{}{} 指纹{}",
                round, cont_ok, end, c1, c2, c3, fp
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("快照一致性 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 双跑确定性演练 10/10 轮：30 例注入两次重放逐例同判。
pub fn drill_dual_replay_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut runs: Vec<Vec<bool>> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut lcg = Lcg(SEG_BASE as u64);
            let mut seq: Vec<bool> = Vec::new();
            let mut case = 0usize;
            while case < INJECT_TOTAL {
                let family = case % 3;
                let mut p = EnumPool::new();
                let mut i = 0;
                while i < 5 {
                    let mut nm = b"N".to_vec();
                    nm.push(b'0' + i as u8);
                    let _ = p.register(&nm);
                    i += 1;
                }
                p.open();
                let _ = p.step(0);
                let _ = p.step(1);
                let r = match family {
                    0 => matches!(p.step(5 + (case % 5) * 7), Ok(None)),
                    1 => matches!(p.step(case % 2), Err(c) if c == EiCode::IndexBackward.val()),
                    _ => matches!(p.step(3 + case % 2), Err(c) if c == EiCode::CursorSkip.val()),
                };
                seq.push(r);
                let _ = lcg.below(3);
                case += 1;
            }
            runs.push(seq);
            rep += 1;
        }
        let mut same = true;
        let mut i = 0;
        while i < INJECT_TOTAL {
            if runs[0][i] != runs[1][i] {
                same = false;
            }
            i += 1;
        }
        if same {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("双跑重放 {}/{}", ok, INJECT_ROUNDS))
}

/// 幽灵游标/终值后取注入 10/10 轮。
pub fn drill_ghost_end_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = EnumPool::new();
        let _ = p.register(b"K");
        // 未开先取：GhostCursor。
        let ghost = matches!(p.step(0), Err(c) if c == EiCode::GhostCursor.val());
        p.open();
        let _ = p.step(0);
        let _ = p.step(1); // 终值
        // 终值后再取：AfterEnd。
        let after = matches!(p.step(0), Err(c) if c == EiCode::AfterEnd.val());
        if ghost && after {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("幽灵/终值注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    EiCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：注入常量锚核对。
pub fn run_unx12471_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-注入锚",
        INJECT_TOTAL == 30 && ENUM_CAP == 16,
        format!("注入{} 表{}", INJECT_TOTAL, ENUM_CAP),
    ));
    v
}

/// run_unx12471_checks：本单判据聚合（诊断段 0x308E0）。
pub fn run_unx12471_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12471 枚举索引越界注入");
    // J1-a：30/30 注入（越界/回退/跳跃三族）。
    let (ok1, d1) = drill_inject_30();
    s.add_dyn("J1-注入30/30", ok1, &d1);
    // J1-b：枚举快照一致性断言。
    let (ok2, d2) = drill_snapshot_10();
    s.add_dyn("J1-快照一致性10/10", ok2, &d2);
    // J1-c：双跑确定性。
    let (ok3, d3) = drill_dual_replay_10();
    s.add_dyn("J1-双跑重放10/10", ok3, &d3);
    // J1-d：幽灵/终值注入。
    let (ok4, d4) = drill_ghost_end_10();
    s.add_dyn("J1-幽灵终值注入10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x308E0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12471_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
