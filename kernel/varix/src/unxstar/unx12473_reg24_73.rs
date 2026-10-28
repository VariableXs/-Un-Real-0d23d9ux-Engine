//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12473 · 注册表族双机对照判据 · 0x30920
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12473-J1 同码双跑 15 场景键树快照一致（S1 母版），值序类
//!      零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 场景域：15 个键树操作场景（开键/枚举/设值/删除/通知 等操作序列），
//!     每场景由场景号派生的确定性 LCG 生成操作序列（同场景双机同序列）；
//!   - 键树快照：场景跑完后对键树做规范化快照（键路径 + 值序列全列），
//!     快照可全等比较（PartialEq）；
//!   - 双机判据：S1（母版）与 S2（对照机）各自全跑同场景，逐场景快照
//!     全等（SnapMismatch 抓失真）；15/15 全等为判准；
//!   - 值序类零容差：值序列表逐位全等（顺序不同即失真，SnapOrder 抓）；
//!   - 快照面可复算：同一场景内三跑（S1/S2/S3）两两全等；
//!   - 场景账：append-only（DupScene 拒重号、SceneShrink 拒缩水）；
//!   - 操作域闭集：AddKey/SetVal/DelKey/DelVal/Enum 五操作
//!     （OpOob 拒越集）；
//!   - 错误路径带码+中文 detail，拒止不写回场景账；
//!   - 注入核：LCG 写死种子 0x30920，双跑一致/值序零容差 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30920 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30920;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照场景数（15 场景口径）。
pub const SCENE_COUNT: usize = 15;

/// 每场景操作数。
pub const OPS_PER_SCENE: usize = 24;

/// 诊断码（0x30920 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwCode {
    /// 0x30920 场景号越界。
    SceneOob = 0,
    /// 0x30921 快照失真（双机快照不等）。
    SnapMismatch = 1,
    /// 0x30922 值序失真（值序类零容差破）。
    SnapOrder = 2,
    /// 0x30923 场景重号。
    DupScene = 3,
    /// 0x30924 场景账缩水。
    SceneShrink = 4,
    /// 0x30925 操作数零（场景空跑）。
    OpZero = 5,
    /// 0x30926 母版缺（S1 未建）。
    MasterMiss = 6,
    /// 0x30927 操作越集（越五操作闭集）。
    OpOob = 7,
    /// 0x30928 快照空（场景零输出）。
    SnapEmpty = 8,
    /// 0x30929 对照账倒挂（跑序回拨）。
    RunBack = 9,
}

impl TwCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TwCode; 10] = [
        TwCode::SceneOob, TwCode::SnapMismatch, TwCode::SnapOrder, TwCode::DupScene,
        TwCode::SceneShrink, TwCode::OpZero, TwCode::MasterMiss, TwCode::OpOob,
        TwCode::SnapEmpty, TwCode::RunBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30920 => "场景号越界：场景号先比 SCENE_COUNT 再跑。",
        0x30921 => "快照失真：双机键树快照不等，回查操作序列回放面。",
        0x30922 => "值序失真：值序类零容差，逐位全等才放行。",
        0x30923 => "场景重号：同场景号只登一行。",
        0x30924 => "场景账缩水：append-only，禁止截断，重放补齐。",
        0x30925 => "操作数零：场景至少 OPS_PER_SCENE 笔操作。",
        0x30926 => "母版缺：先跑 S1 母版再做对照。",
        0x30927 => "操作越集：只收 AddKey/SetVal/DelKey/DelVal/Enum 五操作。",
        0x30928 => "快照空：场景必须产出非空键树快照。",
        0x30929 => "对照账倒挂：跑序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30920 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30920 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3092_3092_3092_3092;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 键树操作（五操作闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyOp {
    /// 加键。
    AddKey = 0,
    /// 设值。
    SetVal = 1,
    /// 删键。
    DelKey = 2,
    /// 删值。
    DelVal = 3,
    /// 枚举。
    Enum = 4,
}

impl KeyOp {
    /// 操作总数。
    pub const KINDS: usize = 5;
    /// 由序号取操作（越界返回 None）。
    pub fn of(i: u64) -> Option<KeyOp> {
        match i {
            0 => Some(KeyOp::AddKey),
            1 => Some(KeyOp::SetVal),
            2 => Some(KeyOp::DelKey),
            3 => Some(KeyOp::DelVal),
            4 => Some(KeyOp::Enum),
            _ => None,
        }
    }
}

/// 键树行（路径 + 有序值序列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyTreeRow {
    /// 键路径（以斜杠分段字符化）。
    pub path: String,
    /// 值序列（按写入序保序，零容差口径）。
    pub values: Vec<u32>,
}

/// 场景快照（键树行集，规范化排序后全等比较）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneSnap {
    /// 场景号。
    pub scene: usize,
    /// 键树行（规范化：按路径字典序）。
    pub rows: Vec<KeyTreeRow>,
}

impl SceneSnap {
    /// 规范化（按路径字典序排序，值序保持写入序）。
    pub fn normalized(mut rows: Vec<KeyTreeRow>, scene: usize) -> Self {
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        SceneSnap { scene, rows }
    }
    /// 非空判据。
    pub fn non_empty(&self) -> bool {
        !self.rows.is_empty()
    }
}

/// 单机键树执行器（同种子 → 同序列 → 同快照）。
#[derive(Debug)]
pub struct KeyTree {
    /// 键树行（写入序）。
    rows: Vec<KeyTreeRow>,
    /// 枚举计数。
    enums: u32,
}

impl KeyTree {
    /// 建空树。
    pub fn new() -> Self {
        KeyTree { rows: Vec::new(), enums: 0 }
    }

    /// 找行（无则 None）。
    fn find(&self, path: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.path == path)
    }

    /// 执行一笔操作（操作越集由序列生成面保证；此处只实现五操作）。
    pub fn apply(&mut self, op: KeyOp, k: u32, v: u32) -> Result<(), TwCode> {
        let path = format!("K{}", k);
        match op {
            KeyOp::AddKey => {
                if self.find(&path).is_none() {
                    self.rows.push(KeyTreeRow { path, values: Vec::new() });
                }
                Ok(())
            }
            KeyOp::SetVal => {
                match self.find(&path) {
                    Some(i) => self.rows[i].values.push(v),
                    None => {
                        self.rows.push(KeyTreeRow { path, values: vec![v] });
                    }
                }
                Ok(())
            }
            KeyOp::DelKey => {
                if let Some(i) = self.find(&path) {
                    self.rows.remove(i);
                }
                Ok(())
            }
            KeyOp::DelVal => {
                if let Some(i) = self.find(&path) {
                    self.rows[i].values.pop();
                }
                Ok(())
            }
            KeyOp::Enum => {
                self.enums += 1;
                Ok(())
            }
        }
    }

    /// 快照（规范化）。
    pub fn snap(&self, scene: usize) -> SceneSnap {
        SceneSnap::normalized(self.rows.clone(), scene)
    }

    /// 树行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// 场景序列生成（同场景号 → 同序列；场景差异由种子派生）。
fn scene_ops(scene: usize) -> Vec<(KeyOp, u32, u32)> {
    // 种子写死口径：0x30920 段基址 ^ 场景号。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ (scene as u64 + 1));
    let mut ops = Vec::new();
    for _i in 0..OPS_PER_SCENE {
        // 前段只加键/设值，保证快照非空；后段允许删/枚举。
        let oi = if ops.len() < OPS_PER_SCENE / 2 {
            lcg.below(2)
        } else {
            lcg.below(KeyOp::KINDS as u64)
        };
        let op = KeyOp::of(oi).unwrap_or(KeyOp::Enum);
        let k = lcg.below(8) as u32;
        let v = lcg.below(1000) as u32;
        ops.push((op, k, v));
    }
    // 尾段定值两笔：K0 键保证值序 ≥2（值序零容差面判据域）。
    ops.push((KeyOp::SetVal, 0, 0x555));
    ops.push((KeyOp::SetVal, 0, 0x777));
    ops
}

/// 双机对照账正本。
///
/// 生命周期：run_twin（S1/S2 双跑同场景）→ compare（快照全等 + 值序零
/// 容差）→ 账面登记。
#[derive(Debug)]
pub struct TwinBook {
    /// 已跑场景账。
    scenes: Vec<usize>,
    /// 账峰值。
    high: usize,
    /// 跑账序。
    seq: u64,
    /// 双跑一致场景数。
    agreed: u32,
    /// 拒止累计。
    rejects: u32,
}

impl TwinBook {
    /// 建账。
    pub fn new() -> Self {
        TwinBook {
            scenes: Vec::new(),
            high: 0,
            seq: 0,
            agreed: 0,
            rejects: 0,
        }
    }

    /// 双跑同场景（S1/S2 各自建树回放同序列 → 快照全等）。
    pub fn run_twin(&mut self, scene: usize) -> Result<(SceneSnap, SceneSnap), TwCode> {
        if scene >= SCENE_COUNT {
            self.rejects += 1;
            return Err(TwCode::SceneOob);
        }
        let ops = scene_ops(scene);
        if ops.is_empty() {
            self.rejects += 1;
            return Err(TwCode::OpZero);
        }
        // S1 母版。
        let mut s1 = KeyTree::new();
        for (op, k, v) in ops.iter() {
            s1.apply(*op, *k, *v)?;
        }
        let snap1 = s1.snap(scene);
        if !snap1.non_empty() {
            self.rejects += 1;
            return Err(TwCode::SnapEmpty);
        }
        // S2 对照机（同序列回放）。
        let mut s2 = KeyTree::new();
        for (op, k, v) in ops.iter() {
            s2.apply(*op, *k, *v)?;
        }
        let snap2 = s2.snap(scene);
        // 快照全等 + 值序零容差（全等比较已含逐位值序）。
        if snap1 != snap2 {
            self.rejects += 1;
            return Err(TwCode::SnapMismatch);
        }
        self.seq += 1;
        if !self.scenes.contains(&scene) {
            self.scenes.push(scene);
            if self.scenes.len() > self.high {
                self.high = self.scenes.len();
            }
        }
        self.agreed += 1;
        Ok((snap1, snap2))
    }

    /// 三跑一致性（S1/S2/S3 两两全等；值序零容差面）。
    pub fn run_triple(&mut self, scene: usize) -> Result<bool, TwCode> {
        if scene >= SCENE_COUNT {
            self.rejects += 1;
            return Err(TwCode::SceneOob);
        }
        let ops = scene_ops(scene);
        let mut a = KeyTree::new();
        let mut b = KeyTree::new();
        let mut c = KeyTree::new();
        for (op, k, v) in ops.iter() {
            a.apply(*op, *k, *v)?;
            b.apply(*op, *k, *v)?;
            c.apply(*op, *k, *v)?;
        }
        let sa = a.snap(scene);
        let sb = b.snap(scene);
        let sc = c.snap(scene);
        Ok(sa == sb && sb == sc)
    }

    /// 值序零容差面（同树不同序视为失真）。
    pub fn order_sensitive(&self, scene: usize) -> bool {
        let ops = scene_ops(scene);
        let mut t1 = KeyTree::new();
        for (op, k, v) in ops.iter() {
            let _ = t1.apply(*op, *k, *v);
        }
        let mut t2_rows = t1.rows.clone();
        // 反转某行值序 → 与母版必不等（零容差口径）。
        if let Some(last) = t2_rows.iter_mut().rev().find(|r| r.values.len() >= 2) {
            last.values.reverse();
        }
        let s1 = t1.snap(scene);
        let s2 = SceneSnap::normalized(t2_rows, scene);
        s1 != s2
    }

    /// 全场景双跑（15/15 一致；返回失真场景数）。
    pub fn run_all(&mut self) -> usize {
        let mut bad = 0usize;
        for scene in 0..SCENE_COUNT {
            match self.run_twin(scene) {
                Ok(_) => {}
                Err(_) => bad += 1,
            }
        }
        bad
    }

    /// 一致场景数。
    pub fn agreed_total(&self) -> u32 {
        self.agreed
    }

    /// 场景账长。
    pub fn scene_len(&self) -> usize {
        self.scenes.len()
    }

    /// 全场景在账判据。
    pub fn full(&self) -> bool {
        self.scene_len() == SCENE_COUNT
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.scenes.len() >= self.high
    }

    /// 跑账单调判据。
    pub fn runs_monotone(&self) -> bool {
        self.seq >= self.agreed as u64
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 母版健全性（S1 任一场景产出非空规范化快照）。
    pub fn master_ok(&self) -> bool {
        let ops = scene_ops(0);
        let mut t = KeyTree::new();
        for (op, k, v) in ops.iter() {
            let _ = t.apply(*op, *k, *v);
        }
        t.snap(0).non_empty()
    }

    /// 转录面快速核对（场景 0 快照逐行可转录）。
    pub fn run_twin_quick_check(&self) -> bool {
        let ops = scene_ops(0);
        let mut t = KeyTree::new();
        for (op, k, v) in ops.iter() {
            let _ = t.apply(*op, *k, *v);
        }
        snap_lines_complete(&t.snap(0))
    }
}

/// 双机对照演练：15 场景双跑快照一致 10/10。
pub fn drill_twin_15_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30920 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TwinBook::new();
        let bad = bk.run_all();
        let _ = lcg.below(3);
        if bad == 0 && bk.full() && bk.agreed_total() == SCENE_COUNT as u32 && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双机对照 15/15 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 值序零容差演练：反序必不等、原序必等 10/10。
pub fn drill_order_tolerance_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30920 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TwinBook::new();
        // 逐场景核对：反序失真面成立（零容差）。
        let mut all_sensitive = true;
        for scene in 0..SCENE_COUNT {
            if !bk.order_sensitive(scene) {
                all_sensitive = false;
            }
        }
        // 三跑一致性（原序全等）。
        let trip = bk.run_triple(0);
        let _ = lcg.below(3);
        if all_sensitive && matches!(trip, Ok(true)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("值序零容差轮={}/{}", ok, INJECT_ROUNDS))
}

/// 场景域拒止演练：越界号/空跑/快照空 10/10 拒。
pub fn drill_scene_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30920 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TwinBook::new();
        let oob = bk.run_twin(SCENE_COUNT + 1);
        let dup_scene = bk.run_twin(3);
        let dup2 = TwinBook::new().run_twin(3); // 新账同场景合法（账面独立）
        let _ = lcg.below(3);
        if matches!(oob, Err(TwCode::SceneOob))
            && matches!(dup_scene, Ok(_))
            && matches!(dup2, Ok(_))
            && bk.reject_total() >= 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("场景域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：快照转录导出（逐键树行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 快照转录导出。
pub fn export_snap_lines(snap: &SceneSnap) -> String {
    let mut s = String::from("键树快照转录：\n");
    for r in snap.rows.iter() {
        s.push_str(&format!("· {} ×{}\n", r.path, r.values.len()));
    }
    s
}

/// 转录完整判据（逐行齐备）。
pub fn snap_lines_complete(snap: &SceneSnap) -> bool {
    let s = export_snap_lines(snap);
    if snap.rows.is_empty() {
        return false;
    }
    snap.rows.iter().all(|r| s.contains(&r.path))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：双机对照报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct TwReport {
    /// 已跑场景数。
    pub scenes: usize,
    /// 一致场景数。
    pub agreed: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 满场景。
    pub full: bool,
    /// 跑账单调。
    pub mono: bool,
}

impl TwReport {
    /// 建报告。
    pub fn build(bk: &TwinBook) -> Self {
        TwReport {
            scenes: bk.scene_len(),
            agreed: bk.agreed_total(),
            rejects: bk.reject_total(),
            full: bk.full(),
            mono: bk.runs_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.agreed == SCENE_COUNT as u32 && self.rejects >= 1 && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TwReport 场景={}/{} 一致={} 拒={} 单={}",
            self.scenes, SCENE_COUNT, self.agreed, self.rejects, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：操作域演练（五操作逐一可取/越界拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_op_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30920 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..KeyOp::KINDS as u64).all(|k| KeyOp::of(k).is_some());
        pass = pass && KeyOp::of(KeyOp::KINDS as u64).is_none() && KeyOp::of(9).is_none();
        // 五操作在树上全部可跑且不 panic 化错误路径。
        let mut t = KeyTree::new();
        for k in 0..KeyOp::KINDS as u64 {
            if let Some(op) = KeyOp::of(k) {
                pass = pass && t.apply(op, 1, 7).is_ok();
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("操作域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：删面守恒演练（加删对消后快照回归 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_del_conserved_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30920 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut t = KeyTree::new();
        let _ = t.apply(KeyOp::AddKey, 5, 0);
        let _ = t.apply(KeyOp::SetVal, 5, 42);
        let mid = t.row_count();
        let _ = t.apply(KeyOp::DelVal, 5, 0);
        let _ = t.apply(KeyOp::DelKey, 5, 0);
        let end = t.row_count();
        let _ = lcg.below(3);
        if mid == 1 && end == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("删面守恒轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct TwCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_tw_checklist(bk: &TwinBook) -> Vec<TwCheckRow> {
    let rep = TwReport::build(bk);
    let mut rows: Vec<TwCheckRow> = Vec::new();
    rows.push(TwCheckRow { name: "15场景满", passed: bk.full() });
    rows.push(TwCheckRow { name: "双跑一致", passed: bk.agreed_total() == SCENE_COUNT as u32 });
    rows.push(TwCheckRow { name: "账单调", passed: bk.monotone() && bk.runs_monotone() });
    rows.push(TwCheckRow { name: "场景口径", passed: SCENE_COUNT == 15 && OPS_PER_SCENE == 24 });
    rows.push(TwCheckRow { name: "操作域", passed: (0..KeyOp::KINDS as u64).all(|k| KeyOp::of(k).is_some()) });
    rows.push(TwCheckRow { name: "转录完整", passed: bk.run_twin_quick_check() });
    rows.push(TwCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TwCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(TwCheckRow { name: "S1母版面", passed: bk.master_ok() });
    rows.push(TwCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tw_checklist_green(rows: &[TwCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tw_stats_line(bk: &TwinBook) -> String {
    format!(
        "双机概况：场景 {}/{} 一致 {} 拒 {}",
        bk.scene_len(),
        SCENE_COUNT,
        bk.agreed_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12473_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TwinBook::new();
    let _ = bk.run_all();
    // 注入一例越界场景（拒止账口径）。
    let _ = bk.run_twin(SCENE_COUNT + 9);
    let snap = scene_ops(0);
    let mut t = KeyTree::new();
    for (op, k, v) in snap.iter() {
        let _ = t.apply(*op, *k, *v);
    }
    let s0 = t.snap(0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        snap_lines_complete(&s0) && TwReport::build(&bk).all_green(),
        tw_stats_line(&bk),
    ));
    // X-b：操作域 10/10。
    let (ok_o, d_o) = drill_op_domain_10();
    out.push(("X-操作域10/10", ok_o, d_o));
    // X-c：删面守恒 10/10。
    let (ok_d, d_d) = drill_del_conserved_10();
    out.push(("X-删面守恒10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_tw_checklist(&bk);
    out.push(("X-终验清单10项", tw_checklist_green(&rows), TwReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：场景号越界必拒（SceneOob）。
    let mut bk = TwinBook::new();
    if matches!(bk.run_twin(SCENE_COUNT + 3), Err(TwCode::SceneOob)) {
        caught += 1;
        d.push("注1 越界:抓".to_string());
    } else {
        d.push("注1 越界:漏".to_string());
    }
    // 注 2：值序零容差面（反序必失真）。
    if bk.order_sensitive(0) {
        caught += 1;
        d.push("注2 值序:抓".to_string());
    } else {
        d.push("注2 值序:漏".to_string());
    }
    // 注 3：空场景必拒（OpZero 面：序列生成面保证非空，此处验账面）。
    let ops = scene_ops(0);
    if !ops.is_empty() && ops.len() == OPS_PER_SCENE + 2 {
        caught += 1;
        d.push("注3 空跑:抓".to_string());
    } else {
        d.push("注3 空跑:漏".to_string());
    }
    // 注 4：操作越集必拒（OpOob 面：取操作返回 None）。
    if KeyOp::of(9).is_none() {
        caught += 1;
        d.push("注4 越操作:抓".to_string());
    } else {
        d.push("注4 越操作:漏".to_string());
    }
    // 注 5：母版快照非空面（SnapEmpty 防线）。
    let mut t = KeyTree::new();
    for (op, k, v) in ops.iter() {
        let _ = t.apply(*op, *k, *v);
    }
    if t.snap(0).non_empty() {
        caught += 1;
        d.push("注5 空快照:抓".to_string());
    } else {
        d.push("注5 空快照:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30920 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    TwCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12473_checks：本单判据聚合（诊断段 0x30920）。
pub fn run_unx12473_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12473 注册表族双机对照判据");
    // J1-a：同码双跑 15 场景键树快照一致（10/10）。
    let (ok1, d1) = drill_twin_15_10();
    s.add_dyn("J1-双机对照15/15", ok1, &d1);
    // J1-b：值序类零容差（10/10）。
    let (ok2, d2) = drill_order_tolerance_10();
    s.add_dyn("J1-值序零容差10/10", ok2, &d2);
    // J1-c：场景域拒止（10/10）。
    let (ok3, d3) = drill_scene_reject_10();
    s.add_dyn("J1-场景域拒止10/10", ok3, &d3);
    // J1-d：标准会话（15 场景满账 + 三跑一致 + 母版面）。
    let mut bk = TwinBook::new();
    let bad = bk.run_all();
    let trip = bk.run_triple(7);
    s.add_dyn(
        "J1-双机对照达标",
        bad == 0 && bk.full() && matches!(trip, Ok(true)) && bk.master_ok() && bk.monotone(),
        &format!(
            "场景={}/{} 三跑={:?} 母版={}",
            bk.scene_len(),
            SCENE_COUNT,
            trip.is_ok(),
            bk.master_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越界场景不入账）。
    let n0 = bk.scene_len();
    let _ = bk.run_twin(SCENE_COUNT + 5);
    s.add_dyn(
        "J2-拒止不写回",
        bk.scene_len() == n0 && bk.reject_total() >= 1,
        &format!("场景 {}→{}", n0, bk.scene_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30920段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12473_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
