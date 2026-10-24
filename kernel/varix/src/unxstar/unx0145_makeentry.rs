//! UNX-F0145 · 单命令构建入口（make image）· 诊断段 0x2540
//!
//! 判据原文（UNX-F0145-J1）：单命令语义——一条命令触发完整阶段链
//! （编译→链接→打包→校验），阶段账按序记账，跳阶段/重入即红。
//!
//! 设计要点：构建入口闸门（BuildGate）持有阶段闭集与严格序账；
//! 单命令 = 一次 run() 驱动全链；重入保护（进行中拒二次 run）；
//! 阶段账守恒：Σ阶段 = 链长 × 成功命令数。零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2540;

const E_STATE: u16 = SEG_BASE + 1; // 重入/状态机非法迁移
const E_ORDER: u16 = SEG_BASE + 2; // 阶段乱序
const E_BAL: u16 = SEG_BASE + 3; // 阶段账守恒被破坏
const E_STAGE: u16 = SEG_BASE + 4; // 阶段名越纲
const E_FAIL: u16 = SEG_BASE + 5; // 阶段执行失败
const E_SEAL: u16 = SEG_BASE + 6; // 闸门已冻结

/// 构建阶段闭集（严格序）。
pub const STAGES: [&str; 4] = ["compile", "link", "pack", "verify"];

/// 阶段执行结果行。
#[derive(Clone)]
pub struct StageRow {
    pub stage: &'static str,
    pub idx: usize,
    pub ok: bool,
    pub note: String,
}

/// 单命令构建闸门。
pub struct BuildGate {
    pub running: bool,
    pub rows: Vec<StageRow>,
    pub next_stage: usize,
    pub runs_done: u32,
    pub runs_failed: u32,
    pub frozen: bool,
}

/// 阶段执行器（由宿主注入；返回 Err 即链断）。
pub type StageFn = fn(&mut Vec<u8>) -> Result<(), u16>;

impl BuildGate {
    pub fn new() -> Result<Self, u16> {
        Ok(BuildGate {
            running: false,
            rows: Vec::new(),
            next_stage: 0,
            runs_done: 0,
            runs_failed: 0,
            frozen: false,
        })
    }

    /// 单命令入口：一键驱动 编译→链接→打包→校验 全链。
    ///
    /// 任何阶段失败即整链失败（fail-fast），已过阶段保留在账。
    pub fn run(&mut self, artifact: &mut Vec<u8>, stages: &[StageFn]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.running {
            return Err(E_STATE); // 重入保护
        }
        if stages.len() != STAGES.len() {
            return Err(E_STAGE); // 阶段闭集长度不符
        }
        self.running = true;
        self.rows.clear();
        self.next_stage = 0;
        let mut outcome = Ok(());
        for (i, f) in stages.iter().enumerate() {
            match f(artifact) {
                Ok(()) => {
                    self.rows.push(StageRow {
                        stage: STAGES[i],
                        idx: i,
                        ok: true,
                        note: "过".to_string(),
                    });
                }
                Err(c) => {
                    self.rows.push(StageRow {
                        stage: STAGES[i],
                        idx: i,
                        ok: false,
                        note: format!("{:04X}", c),
                    });
                    outcome = Err(E_FAIL);
                    break;
                }
            }
            self.next_stage = i + 1;
        }
        if outcome.is_ok() {
            self.runs_done += 1;
            if self.rows.len() != STAGES.len() {
                self.running = false;
                return Err(E_BAL); // 成功链却阶段账不满——装配矛盾
            }
        } else {
            self.runs_failed += 1;
        }
        self.running = false;
        outcome
    }

    /// 阶段账守恒：行序严格递增且与闭集对齐。
    pub fn audit(&self) -> Result<(), u16> {
        for (i, r) in self.rows.iter().enumerate() {
            if r.stage != STAGES[r.idx] {
                return Err(E_BAL);
            }
            if r.idx != i {
                return Err(E_ORDER); // 行序与阶段序错位
            }
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染构建一览。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "构建闸门 成{} 败{} 进行:{}\n",
            self.runs_done, self.runs_failed, self.running
        ));
        for r in &self.rows {
            out.push_str(&format!("{}({}) {} {}\n", r.stage, r.idx, r.note, r.ok));
        }
        out
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 4 例 ─────────────────────────

/// 四个阶段样例执行器：逐阶段往 artifact 盖段标。
fn s_compile(a: &mut Vec<u8>) -> Result<(), u16> {
    a.extend_from_slice(b"C");
    Ok(())
}
fn s_link(a: &mut Vec<u8>) -> Result<(), u16> {
    a.extend_from_slice(b"L");
    Ok(())
}
fn s_pack(a: &mut Vec<u8>) -> Result<(), u16> {
    a.extend_from_slice(b"P");
    Ok(())
}
fn s_verify(a: &mut Vec<u8>) -> Result<(), u16> {
    a.extend_from_slice(b"V");
    Ok(())
}
/// 打包阶段注红的失败样例。
fn s_pack_bad(a: &mut Vec<u8>) -> Result<(), u16> {
    a.extend_from_slice(b"P");
    Err(0x51A1)
}

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：单命令全链——四阶段依序过，产物盖 CLPV。
    let mut g = BuildGate::new()?;
    let mut art = Vec::new();
    let stages: Vec<StageFn> = vec![s_compile, s_link, s_pack, s_verify];
    g.run(&mut art, &stages)?;
    if art != b"CLPV".to_vec() {
        return Err(E_BAL);
    }
    g.audit()?;
    hits += 1;

    // L2：连续两次单命令——重入解除后账翻倍。
    g.run(&mut art, &stages)?;
    if g.runs_done != 2 || art != b"CLPVCLPV".to_vec() {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：fail-fast——pack 注红，verify 不再执行。
    let mut g3 = BuildGate::new()?;
    let mut a3 = Vec::new();
    let bad: Vec<StageFn> = vec![s_compile, s_link, s_pack_bad, s_verify];
    if g3.run(&mut a3, &bad) != Err(E_FAIL) {
        return Err(E_FAIL);
    }
    if a3 != b"CLP".to_vec() {
        return Err(E_BAL);
    }
    if g3.rows.len() != 3 {
        return Err(E_BAL);
    }
    if g3.rows[2].ok {
        return Err(E_BAL);
    }
    g3.audit()?; // 失败链前缀仍是对齐账
    hits += 1;

    // I1：重入拦截（running 态拒二次 run）。
    let mut i1 = BuildGate::new()?;
    i1.running = true; // 手工置进行中
    let mut a1 = Vec::new();
    if i1.run(&mut a1, &stages) != Err(E_STATE) {
        return Err(E_STATE);
    }
    i1.running = false;
    hits += 1;

    // I2：阶段表长度不符拦截。
    let mut i2 = BuildGate::new()?;
    let short: Vec<StageFn> = stages.iter().take(3).copied().collect();
    if i2.run(&mut a1, &short) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I3：冻结拒跑。
    let mut i3 = BuildGate::new()?;
    i3.freeze();
    if i3.run(&mut a1, &stages) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I4：守恒注入——成功链但阶段账被抽走一行。
    let mut i4 = BuildGate::new()?;
    i4.run(&mut a1, &stages)?;
    i4.rows.remove(1); // 破账：成功却少一行
    if i4.audit() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
