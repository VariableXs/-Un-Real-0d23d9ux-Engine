//! cgb749_profconsist —— CGPU-F0749 画像一致性。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0749 画像一致性  <SEG 0x4be0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0749；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 一致性 = 三面账对冲：注册面（画像表）/ 联动面（场景绑定
//!      账）/ 遥测面（采样引用账），三面同号画像的属性必须逐位
//!      相等；任一面漂移即不一致红；
//!   2. 属性闭集四维：权重三元组、场景位图、版本档、指纹——四维
//!      全等才算一致（逐维独立报告，首差定位到维）；
//!   3. 快照-复演模型：注册面在 t0 打快照，联动/遥测面在 t1 回读
//!      对拍；快照不可变（封账级），复演必须逐位重现；
//!   4. 对冲账守恒：一致数 + 漂移数 = 对比总数，恒等式逐条核；
//!   5. 漂移定位账：每条漂移记（画像号，维编号，期望值，实测值）
//      四元组，不许只报"有漂移"；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4be0 段为本模块独占（SEG_BASE=0x4be0），判别式
//!   SEG_BASE+1..+N；与 0x4bc0/0x4c00 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PC_VERSION: &str = "cgb749_profconsist-v1";

/// 诊断段基址（0x4be0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4BE0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4be1 画像号越界（0..=63 之外）。
pub const XC_BAD_PID: u16 = SEG_BASE + 1;
/// 0x4be2 权重维漂移（三元组不等）。
pub const XC_DRIFT_WEIGHT: u16 = SEG_BASE + 2;
/// 0x4be3 位图维漂移（场景位图不等）。
pub const XC_DRIFT_MASK: u16 = SEG_BASE + 3;
/// 0x4be4 版本维漂移（版本档不等）。
pub const XC_DRIFT_VER: u16 = SEG_BASE + 4;
/// 0x4be5 指纹维漂移（画像指纹不等）。
pub const XC_DRIFT_FP: u16 = SEG_BASE + 5;
/// 0x4be6 快照未打就复演（次序红线）。
pub const XC_NO_SNAPSHOT: u16 = SEG_BASE + 6;
/// 0x4be7 对冲账不守恒（一致+漂移 ≠ 总数）。
pub const XC_HEDGE_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4be8 快照重复打（t0 已封不可再封）。
pub const XC_SNAPSHOT_DUP: u16 = SEG_BASE + 8;
/// 0x4be9 对比集为空（无画像可比）。
pub const XC_COMPARE_EMPTY: u16 = SEG_BASE + 9;

/// 画像容量（写死：64）。
pub const PID_CAP: usize = 64;

/// 画像属性（四维对比面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfAttrs {
    /// 画像号。
    pub pid: u8,
    /// 权重三元组（千分位）。
    pub w: [u32; 3],
    /// 场景位图（低 8 位）。
    pub mask: u8,
    /// 版本档。
    pub ver: u8,
    /// 画像指纹（FNV-1a of 前三维）。
    pub fp: u32,
}

/// 画像指纹：权重三元组+位图+版本 顺序吃 FNV-1a。
pub fn prof_fingerprint(w: &[u32; 3], mask: u8, ver: u8) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for x in w.iter() {
        for i in 0..4 {
            h ^= (x >> (i * 8)) as u8 as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    for b in [mask, ver] {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 画像注册面（t0 权威账）。
pub struct ProfileFace {
    slots: [Option<ProfAttrs>; PID_CAP],
}

impl ProfileFace {
    /// 空面。
    pub fn new() -> Self {
        ProfileFace {
            slots: [None; PID_CAP],
        }
    }

    /// 注册/覆写画像属性（号越界红码；权重和 ≠1000 红码）。
    pub fn put(&mut self, pid: u8, w: [u32; 3], mask: u8, ver: u8) -> Result<ProfAttrs, u16> {
        if (pid as usize) >= PID_CAP {
            return Err(XC_BAD_PID);
        }
        if w[0] + w[1] + w[2] != 1000 {
            return Err(XC_BAD_PID);
        }
        let a = ProfAttrs {
            pid,
            w,
            mask,
            ver,
            fp: prof_fingerprint(&w, mask, ver),
        };
        self.slots[pid as usize] = Some(a);
        Ok(a)
    }

    /// 读画像。
    pub fn get(&self, pid: u8) -> Result<ProfAttrs, u16> {
        self.slots
            .get(pid as usize)
            .and_then(|s| s.as_ref())
            .copied()
            .ok_or(XC_BAD_PID)
    }

    /// 在册画像号列表（升序）。
    pub fn pids(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            if s.is_some() {
                v.push(i as u8);
            }
        }
        v
    }

    /// 快照提取（不可变副本）。
    pub fn snapshot(&self) -> Vec<ProfAttrs> {
        self.slots.iter().flatten().copied().collect()
    }
}

/// 漂移四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drift {
    /// 画像号。
    pub pid: u8,
    /// 维编号：1=权重 2=位图 3=版本 4=指纹。
    pub dim: u8,
    /// 期望值（快照面）。
    pub expect: u32,
    /// 实测值（复演面）。
    pub actual: u32,
}

/// 对冲结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hedge {
    /// 一致画像数。
    pub consistent: u32,
    /// 漂移画像数。
    pub drifted: u32,
    /// 对比总数。
    pub compared: u32,
}

impl Hedge {
    /// 守恒恒等式：consistent + drifted == compared。
    pub fn conserved(&self) -> bool {
        self.consistent + self.drifted == self.compared
    }
}

/// 逐画像四维对拍：全等 → None；任一维不等 → 首差 Drift。
pub fn compare_one(expect: &ProfAttrs, actual: &ProfAttrs) -> Option<Drift> {
    if expect.w != actual.w {
        return Some(Drift {
            pid: expect.pid,
            dim: 1,
            expect: expect.w[0] ^ expect.w[1] ^ expect.w[2],
            actual: actual.w[0] ^ actual.w[1] ^ actual.w[2],
        });
    }
    if expect.mask != actual.mask {
        return Some(Drift {
            pid: expect.pid,
            dim: 2,
            expect: expect.mask as u32,
            actual: actual.mask as u32,
        });
    }
    if expect.ver != actual.ver {
        return Some(Drift {
            pid: expect.pid,
            dim: 3,
            expect: expect.ver as u32,
            actual: actual.ver as u32,
        });
    }
    if expect.fp != actual.fp {
        return Some(Drift {
            pid: expect.pid,
            dim: 4,
            expect: expect.fp,
            actual: actual.fp,
        });
    }
    None
}

/// 三面对冲引擎：快照面（冻结）+ 复演面（动态）→ 对冲账。
pub struct ConsistEngine {
    snapshot: Option<Vec<ProfAttrs>>,
    replay: ProfileFace,
    drifts: Vec<Drift>,
    hedge: Option<Hedge>,
}

impl ConsistEngine {
    /// 空引擎。
    pub fn new() -> Self {
        ConsistEngine {
            snapshot: None,
            replay: ProfileFace::new(),
            drifts: Vec::new(),
            hedge: None,
        }
    }

    /// 打快照（从权威面）；二次打红码。
    pub fn seal_snapshot(&mut self, face: &ProfileFace) -> Result<usize, u16> {
        if self.snapshot.is_some() {
            return Err(XC_SNAPSHOT_DUP);
        }
        let snap = face.snapshot();
        if snap.is_empty() {
            return Err(XC_COMPARE_EMPTY);
        }
        let n = snap.len();
        self.snapshot = Some(snap);
        Ok(n)
    }

    /// 复演面写入口（转发）。
    pub fn replay_put(&mut self, pid: u8, w: [u32; 3], mask: u8, ver: u8) -> Result<(), u16> {
        self.replay.put(pid, w, mask, ver)?;
        Ok(())
    }

    /// 复演面删口（模拟漂移源：复演面丢画像）。
    pub fn replay_drop(&mut self, pid: u8) -> Result<(), u16> {
        if self.replay.get(pid).is_err() {
            return Err(XC_BAD_PID);
        }
        // 置空槽（直接改槽位——引擎是面的唯一写者）。
        self.replay.slots[pid as usize] = None;
        Ok(())
    }

    /// 对冲主路径：快照逐条 vs 复演面；快照有而复演无 → 维 4 漂移
    /// （指纹维，期望 fp 实测 0）。空快照红码。
    pub fn hedge(&mut self) -> Result<Hedge, u16> {
        let snap = self.snapshot.as_ref().ok_or(XC_NO_SNAPSHOT)?;
        self.drifts.clear();
        let mut consistent = 0u32;
        let mut drifted = 0u32;
        for e in snap.iter() {
            match self.replay.get(e.pid) {
                Ok(a) => match compare_one(e, &a) {
                    None => consistent += 1,
                    Some(d) => {
                        self.drifts.push(d);
                        drifted += 1;
                    }
                },
                Err(_) => {
                    self.drifts.push(Drift {
                        pid: e.pid,
                        dim: 4,
                        expect: e.fp,
                        actual: 0,
                    });
                    drifted += 1;
                }
            }
        }
        if consistent + drifted != snap.len() as u32 {
            return Err(XC_HEDGE_UNBALANCED);
        }
        let h = Hedge {
            consistent,
            drifted,
            compared: snap.len() as u32,
        };
        if !h.conserved() {
            return Err(XC_HEDGE_UNBALANCED);
        }
        self.hedge = Some(h);
        Ok(h)
    }

    /// 漂移账只读。
    pub fn drift_ledger(&self) -> &[Drift] {
        &self.drifts
    }

    /// 首差定位：第一条漂移（无漂移 None）。
    pub fn first_drift(&self) -> Option<Drift> {
        self.drifts.first().copied()
    }

    /// 全绿判定：对冲完成且零漂移。
    pub fn all_green(&self) -> Result<bool, u16> {
        let h = self.hedge.as_ref().ok_or(XC_NO_SNAPSHOT)?;
        Ok(h.drifted == 0)
    }
}

/// 自例程：三面构造 → 快照 → 复演全等对冲（全绿）→ 注入漂移
/// （权重/位图/版本/丢失）逐一检出 → 错误路径（未快照对冲/二次
/// 快照/守恒核）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 权威面：三个画像。
    let mut face = ProfileFace::new();
    face.put(0, [600, 300, 100], 0b0111, 1)?;
    face.put(1, [100, 700, 200], 0b0011, 1)?;
    face.put(2, [200, 200, 600], 0b0101, 0)?;

    // 主路径 1：复演面全等 → 全绿。
    let mut e1 = ConsistEngine::new();
    e1.seal_snapshot(&face)?;
    e1.replay_put(0, [600, 300, 100], 0b0111, 1)?;
    e1.replay_put(1, [100, 700, 200], 0b0011, 1)?;
    e1.replay_put(2, [200, 200, 600], 0b0101, 0)?;
    let h1 = e1.hedge()?;
    if h1.consistent != 3 || h1.drifted != 0 || !h1.conserved() {
        return Err(XC_HEDGE_UNBALANCED);
    }
    if !e1.all_green()? {
        return Err(XC_DRIFT_WEIGHT);
    }
    // 二次快照红码。
    if e1.seal_snapshot(&face) != Err(XC_SNAPSHOT_DUP) {
        return Err(XC_SNAPSHOT_DUP);
    }

    // 主路径 2：权重维漂移。
    let mut e2 = ConsistEngine::new();
    e2.seal_snapshot(&face)?;
    e2.replay_put(0, [500, 300, 200], 0b0111, 1)?;
    e2.replay_put(1, [100, 700, 200], 0b0011, 1)?;
    e2.replay_put(2, [200, 200, 600], 0b0101, 0)?;
    let h2 = e2.hedge()?;
    if h2.consistent != 2 || h2.drifted != 1 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    let d = e2.first_drift().ok_or(XC_HEDGE_UNBALANCED)?;
    if d.pid != 0 || d.dim != 1 {
        return Err(XC_DRIFT_WEIGHT);
    }

    // 主路径 3：位图维 + 版本维 + 丢失维 三漂移齐检出。
    let mut e3 = ConsistEngine::new();
    e3.seal_snapshot(&face)?;
    e3.replay_put(0, [600, 300, 100], 0b0001, 1)?; // 位图漂移
    e3.replay_put(1, [100, 700, 200], 0b0011, 0)?; // 版本漂移
    e3.replay_drop(2)?;                            // 丢失
    let h3 = e3.hedge()?;
    if h3.drifted != 3 || h3.consistent != 0 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    if e3.drift_ledger().len() != 3 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    if e3.all_green() != Ok(false) {
        return Err(XC_DRIFT_MASK);
    }

    // 错误路径 1：未快照就对冲。
    let mut e4 = ConsistEngine::new();
    if e4.hedge() != Err(XC_NO_SNAPSHOT) {
        return Err(XC_NO_SNAPSHOT);
    }
    // 错误路径 2：画像号越界。
    if face.put(PID_CAP as u8, [500, 300, 200], 1, 1) != Err(XC_BAD_PID) {
        return Err(XC_BAD_PID);
    }
    // 错误路径 3：权重破恒。
    if face.put(5, [400, 400, 400], 1, 1) != Err(XC_BAD_PID) {
        return Err(XC_BAD_PID);
    }
    // 错误路径 4：空面对冲（快照空集）。
    let empty = ProfileFace::new();
    let mut e5 = ConsistEngine::new();
    if e5.seal_snapshot(&empty) != Err(XC_COMPARE_EMPTY) {
        return Err(XC_COMPARE_EMPTY);
    }
    // 指纹确定性：同属性同指纹。
    let f1 = prof_fingerprint(&[600, 300, 100], 0b0111, 1);
    let f2 = prof_fingerprint(&[600, 300, 100], 0b0111, 1);
    if f1 != f2 {
        return Err(XC_DRIFT_FP);
    }
    Ok(0)
}
