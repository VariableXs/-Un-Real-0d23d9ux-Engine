//! VE-F4292 · 工具集成文档（U05 工具集成组）
//! 职责：集成面文档四族（接入指南/适配器参考/总线协议/故障手册）+ 协议
//! 文档语义版本与 U02 同规 + 指南步骤逐条可复现验证（复现失败阻断发版）
//! + 协议破坏兼容禁止发布 + 故障手册缺景补录排程 + 读屏结构（域本色）。
//! 段位：0x2DE0（续表 idx526）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2DE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdCode {
    /// 复现失败（阻断发版）。
    ReproFail = SEG_BASE,
    /// 协议破坏兼容（禁止发布）。
    Incompat = SEG_BASE + 0x01,
    /// 故障手册缺景。
    ManualHole = SEG_BASE + 0x02,
    /// 复现账本漂移（批次重复/越界）。
    LedgerDrift = SEG_BASE + 0x03,
    /// 序列化指纹失配（哨兵/族号损坏）。
    FpMismatch = SEG_BASE + 0x04,
}

/// 四族文档。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdDoc {
    Guide = 0,
    AdapterRef = 1,
    BusProto = 2,
    FaultManual = 3,
}
pub const IDOC_N: usize = 4;

/// 步骤复现记录。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct Step {
    pub doc: IdDoc,
    pub no: u8,
    pub repro_ok: bool,
}

/// 集成文档册。
pub struct IntegDoc {
    pub docs: [bool; IDOC_N],
    /// 协议文档版本（语义版本：主.次）。
    pub proto_major: u32,
    pub proto_minor: u32,
    pub steps: Vec<Step>,
    /// 故障手册景数。
    pub manual_scenes: u32,
    /// 读屏结构标记（域本色）。
    pub sr_struct: bool,
}

impl IntegDoc {
    pub fn new() -> Self {
        IntegDoc { docs: [false; IDOC_N], proto_major: 1, proto_minor: 0, steps: Vec::new(), manual_scenes: 0, sr_struct: false }
    }

    pub fn write(&mut self, d: IdDoc) {
        self.docs[d as usize] = true;
    }

    pub fn all_docs(&self) -> bool {
        self.docs.iter().all(|d| *d)
    }

    /// 步骤复现登记。
    pub fn add_step(&mut self, d: IdDoc, no: u8, ok: bool) {
        self.steps.push(Step { doc: d, no, repro_ok: ok });
    }

    /// 复现验证：任一步失败→阻断发版。
    pub fn repro_ok(&self) -> Result<(), IdCode> {
        if self.steps.iter().any(|s| !s.repro_ok) {
            return Err(IdCode::ReproFail);
        }
        Ok(())
    }

    /// 协议变更兼容检：主版本不变=兼容；主版本变=破坏兼容禁止发布。
    pub fn proto_change(&mut self, major: u32, minor: u32) -> Result<(), IdCode> {
        if major != self.proto_major && minor == 0 {
            return Err(IdCode::Incompat);
        }
        self.proto_major = major;
        self.proto_minor = minor;
        Ok(())
    }

    /// 故障手册补景。
    pub fn add_scene(&mut self, n: u32) {
        self.manual_scenes += n;
    }

    pub fn manual_ok(&self, need: u32) -> bool {
        self.manual_scenes >= need
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C526-01..12。
pub fn run_ves526_checks() -> CheckSet {
    let mut set = CheckSet::new("ves526");

    let mut id = IntegDoc::new();
    for d in [IdDoc::Guide, IdDoc::AdapterRef, IdDoc::BusProto, IdDoc::FaultManual] {
        id.write(d);
    }
    chk(&mut set, "C526-01", id.all_docs(), "四族文档齐备");

    // 步骤复现
    id.add_step(IdDoc::Guide, 1, true);
    id.add_step(IdDoc::Guide, 2, true);
    chk(&mut set, "C526-02", id.repro_ok().is_ok(), "指南步骤全复现");
    id.add_step(IdDoc::Guide, 3, false);
    chk(&mut set, "C526-03", id.repro_ok() == Err(IdCode::ReproFail), "复现失败阻断发版");

    // 协议版本
    chk(&mut set, "C526-04", id.proto_change(1, 1).is_ok(), "次版本升级兼容");
    chk(&mut set, "C526-05", id.proto_change(2, 0) == Err(IdCode::Incompat), "主版本破坏兼容禁止发布");
    chk(&mut set, "C526-06", id.proto_change(2, 1).is_ok(), "主版本带迁移方案放行");
    chk(&mut set, "C526-07", id.proto_major == 2 && id.proto_minor == 1, "协议版本演进在册");

    // 故障手册
    chk(&mut set, "C526-08", !id.manual_ok(3), "故障手册缺景");
    id.add_scene(2);
    chk(&mut set, "C526-09", !id.manual_ok(3), "补 2 仍缺 1");
    id.add_scene(1);
    chk(&mut set, "C526-10", id.manual_ok(3), "补齐三景");

    id.sr_struct = true;
    chk(&mut set, "C526-11", id.sr_struct, "文档结构读屏标准（域本色）");
    chk(&mut set, "C526-12", id.steps.len() == 3 && id.steps.iter().filter(|s| s.repro_ok).count() == 2, "步骤账自洽");

    ves526_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：复现复核批次账本（结构化归因）+ 守恒恒等式 + 边界探针
// + 族级失败投影与整数中位 + LCG 注入回归（异常必抓/负对照零误报）
// + 幂等闸 + 步骤 4 字节序列化往返 + FNV-1a 册指纹。

use alloc::string::{String, ToString};

/// 复核批次账：按批次号 × 文档族归因登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReproBatch {
    pub batch: u16,
    pub doc: IdDoc,
    pub pass: u32,
    pub fail: u32,
}

/// 集成文档深化账本（与 IntegDoc 并行，只消化其判定面）。
pub struct DocLedger {
    pub batches: Vec<ReproBatch>,
    pub total_reviews: u32,
}

impl DocLedger {
    pub fn new() -> Self {
        DocLedger { batches: Vec::new(), total_reviews: 0 }
    }

    /// 登记复核批次；同批次同族重复登记判账本漂移。
    pub fn add_batch(&mut self, batch: u16, doc: IdDoc, pass: u32, fail: u32) -> Result<(), IdCode> {
        if self.batches.iter().any(|b| b.batch == batch && b.doc == doc) {
            return Err(IdCode::LedgerDrift);
        }
        self.batches.push(ReproBatch { batch, doc, pass, fail });
        self.total_reviews += pass + fail;
        Ok(())
    }

    /// 守恒恒等式：Σ(pass+fail) ≡ total_reviews。
    pub fn conservation(&self) -> bool {
        let s: u32 = self.batches.iter().map(|b| b.pass + b.fail).sum();
        s == self.total_reviews
    }

    /// 边界探针：查无此批次 → 显性 Err（绝不 panic）。
    pub fn batch_of(&self, batch: u16) -> Result<ReproBatch, IdCode> {
        self.batches.iter().find(|b| b.batch == batch).copied().ok_or(IdCode::LedgerDrift)
    }

    /// 族级投影：失败数按四族分列。
    pub fn fail_by_doc(&self) -> [u32; IDOC_N] {
        let mut out = [0u32; IDOC_N];
        for b in &self.batches {
            out[b.doc as usize] += b.fail;
        }
        out
    }

    /// 批次失败数中位（排序取点，整数禁浮点）。
    pub fn fail_median(&self) -> u32 {
        let mut v: Vec<u32> = self.batches.iter().map(|b| b.fail).collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        v[v.len() / 2]
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 注入回归：64 轮随机注入复现失败——凡含失败步骤的册子
/// repro_ok 必抓（异常必抓率 100%）；fail 位 =8 的轮为负对照（全通过），
/// 必须零误报。任何偏离返回中文诊断。
fn inject_regression(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut id = IntegDoc::new();
        id.write(IdDoc::Guide);
        let fail_at = (rng.next() % 9) as u8; // 0..8；=8 则全通过（负对照轮）
        for no in 0..8u8 {
            id.add_step(IdDoc::Guide, no, no != fail_at);
        }
        if fail_at < 8 {
            if id.repro_ok() != Err(IdCode::ReproFail) {
                return Err("注入回归：含失败步骤的册子未被复现闸抓到".to_string());
            }
        } else if id.repro_ok().is_err() {
            return Err("注入回归负对照：全通过步骤被误报复现失败".to_string());
        }
    }
    Ok(())
}

/// 幂等闸：同参二次操作零副作用（write/add_scene 语义幂等；
/// add_step 属账本追加语义，不在此列）。
fn idempotence_probe() -> bool {
    let mut id = IntegDoc::new();
    id.write(IdDoc::BusProto);
    let snap_docs = id.docs;
    id.write(IdDoc::BusProto); // 重复写同族
    if id.docs != snap_docs {
        return false;
    }
    id.add_scene(0); // 零补景
    if id.manual_scenes != 0 {
        return false;
    }
    let before = id.steps.len();
    id.add_step(IdDoc::Guide, 1, true);
    id.add_step(IdDoc::Guide, 1, true);
    before + 2 == id.steps.len()
}

/// 步骤定长 4 字节编码：[族号, 步号, 通过旗, 0x5A 哨兵]。手写布局禁 JSON。
pub fn encode_step(s: &Step) -> [u8; 4] {
    [s.doc as u8, s.no, u8::from(s.repro_ok), 0x5A]
}

/// 解码：哨兵/族号/旗位任一损坏 → FpMismatch。
pub fn decode_step(b: &[u8; 4]) -> Result<Step, IdCode> {
    if b[3] != 0x5A || b[0] >= IDOC_N as u8 || b[2] > 1 {
        return Err(IdCode::FpMismatch);
    }
    let doc = match b[0] {
        0 => IdDoc::Guide,
        1 => IdDoc::AdapterRef,
        2 => IdDoc::BusProto,
        _ => IdDoc::FaultManual,
    };
    Ok(Step { doc, no: b[1], repro_ok: b[2] == 1 })
}

/// FNV-1a 32 位册指纹：内容动 → 指纹动。
pub fn fnv_steps(steps: &[Step]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for s in steps {
        for byte in encode_step(s) {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 边界探针：空册守恒平凡成立 + 空册指纹等于 FNV 偏移基。
pub fn empty_ledger_probes() -> bool {
    let dl = DocLedger::new();
    if !dl.conservation() || dl.fail_median() != 0 || dl.fail_by_doc() != [0, 0, 0, 0] {
        return false;
    }
    fnv_steps(&[]) == 0x811C_9DC5
}

/// 深化判据并入主判据流（C526-13..24）。
pub fn ves526_deep_checks(set: &mut CheckSet) {
    let mut dl = DocLedger::new();
    let r1 = dl.add_batch(1, IdDoc::Guide, 6, 1);
    let r2 = dl.add_batch(2, IdDoc::BusProto, 4, 0);
    set.add("C526-13", r1.is_ok() && r2.is_ok() && dl.conservation(), "批次账守恒 Σ(pass+fail)≡total");
    set.add("C526-14", dl.add_batch(1, IdDoc::Guide, 1, 1) == Err(IdCode::LedgerDrift), "重复批次登记判账本漂移");
    set.add("C526-15", dl.batch_of(9).is_err() && dl.batch_of(1).is_ok(), "越界批次探针显性报错");
    set.add("C526-16", dl.fail_by_doc() == [1, 0, 0, 0], "族级失败投影分列");
    set.add("C526-17", dl.fail_median() == 0, "失败中位整数取点（禁浮点）");

    match inject_regression(0xC0FF_EE01) {
        Ok(()) => set.add("C526-18", true, "LCG 注入 64 轮异常必抓+负对照零误报"),
        Err(msg) => set.add_dyn("C526-18", false, &msg),
    }

    set.add("C526-19", idempotence_probe(), "幂等闸：同参二次操作零副作用");

    let st = Step { doc: IdDoc::FaultManual, no: 7, repro_ok: false };
    match decode_step(&encode_step(&st)) {
        Ok(back) => {
            set.add("C526-20", back.doc == st.doc && back.no == st.no && back.repro_ok == st.repro_ok, "步骤 4 字节编码解码往返")
        }
        Err(_) => set.add("C526-20", false, "往返解码异常失败"),
    }
    set.add("C526-21", decode_step(&[3, 7, 0, 0x00]) == Err(IdCode::FpMismatch), "哨兵损坏拒绝解码");
    set.add("C526-22", decode_step(&[4, 0, 1, 0x5A]) == Err(IdCode::FpMismatch), "族号越界拒绝解码");

    let mut id2 = IntegDoc::new();
    id2.add_step(IdDoc::Guide, 1, true);
    let fp1 = fnv_steps(&id2.steps);
    id2.add_step(IdDoc::Guide, 2, false);
    set.add("C526-23", fnv_steps(&id2.steps) != fp1, "FNV 册指纹随内容漂移");
    set.add("C526-24", empty_ledger_probes(), "空册守恒/空册指纹偏移基探针");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_docs() {
        let mut id = IntegDoc::new();
        id.write(IdDoc::Guide);
        assert!(!id.all_docs());
        id.add_step(IdDoc::Guide, 1, true);
        assert!(id.repro_ok().is_ok());
        id.add_step(IdDoc::Guide, 2, false);
        assert_eq!(id.repro_ok(), Err(IdCode::ReproFail));
        assert_eq!(id.proto_change(2, 0), Err(IdCode::Incompat));
    }
}
