//! UNX-F12054 PEB 写白名单字段防护
//! 分段: 0x198e0 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：环境块字段按"可写主体"分级：
//!   RW_KERNEL  — 仅内核写入合法（如 NumberProcessors）
//!   RW_PROCESS — 进程自身可写（如 ProcessParameters 指针）
//!   RW_BOTH    — 双方皆可（罕见，如特定调试字段）
//! 写请求携带 { 主体, 字段, epoch }，防护器按分级表裁决：
//!   - 主体无权 → W54_ROLE；
//!   - 同一 epoch 重复写同一字段 → W54_REPLAY（防回放）；
//!   - 封存后任何写 → W54_SEALED。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x198E0;
pub const SEG_W54_ROLE: u32 = SEG_BASE + 0x01;
pub const SEG_W54_REPLAY: u32 = SEG_BASE + 0x02;
pub const SEG_W54_SEALED: u32 = SEG_BASE + 0x03;
pub const SEG_W54_FIELD: u32 = SEG_BASE + 0x04;
pub const SEG_W54_ORDER: u32 = SEG_BASE + 0x05;

/// 写主体
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    Kernel = 1,
    Process = 2,
}

/// PEB 字段（可写子集）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PebField {
    NumProc = 1,
    ProcParams = 2,
    MaxODS = 3,
    AltStore = 4,
}

impl PebField {
    /// 字段可写分级
    pub fn acl(self) -> Writer {
        match self {
            PebField::NumProc => Writer::Kernel,
            PebField::MaxODS => Writer::Kernel,
            PebField::ProcParams => Writer::Process,
            PebField::AltStore => Writer::Process,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PebField::NumProc => "NumProc",
            PebField::ProcParams => "ProcParams",
            PebField::MaxODS => "MaxODS",
            PebField::AltStore => "AltStore",
        }
    }
}

/// 一次写请求
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReq {
    pub who: Writer,
    pub field: PebField,
    pub epoch: u32,
}

/// 防护器：维护已接受写的水位表
#[derive(Debug, Clone)]
pub struct WriteGate {
    pub watermark: Vec<(PebField, u32)>, // 字段 → 已接受最大 epoch
    pub sealed: bool,
}

impl WriteGate {
    pub fn new() -> WriteGate {
        WriteGate { watermark: Vec::new(), sealed: false }
    }

    /// 审批一次写请求。
    /// 顺序：封存 → 字段未知 → 角色无权 → 回放（epoch ≤ 水位）。
    pub fn approve(&mut self, req: &WriteReq) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_W54_SEALED);
        }
        // 字段合法（枚举内必合法，此处防御未知变体场景用 ACL 查询）
        let _ = req.field.acl();
        if req.who != req.field.acl() {
            return Err(SEG_W54_ROLE);
        }
        // 回放：水位表内已有 >= epoch 的记录
        for (f, w) in &self.watermark {
            if *f == req.field && *w >= req.epoch {
                return Err(SEG_W54_REPLAY);
            }
        }
        // 接受：提升水位
        let mut updated = false;
        for e in self.watermark.iter_mut() {
            if e.0 == req.field {
                e.1 = req.epoch;
                updated = true;
                break;
            }
        }
        if !updated {
            self.watermark.push((req.field, req.epoch));
        }
        Ok(())
    }

    /// 字段水位查询
    pub fn level(&self, f: PebField) -> u32 {
        let mut v = 0;
        for (ff, w) in &self.watermark {
            if *ff == f && *w > v {
                v = *w;
            }
        }
        v
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 写者域标签面（诊断用）。
pub fn writer_label(w: Writer) -> &'static str {
    match w {
        Writer::Kernel => "KRNL",
        Writer::Process => "PROC",
    }
}

/// 字段 ACL 摘要：字段名 + 允许写者（诊断面用）。
pub fn acl_summary(f: PebField) -> String {
    let who = match f.acl() {
        Writer::Kernel => "KRNL",
        Writer::Process => "PROC",
    };
    format!("acl[{}|{}]", f.label(), who)
}

/// epoch 间距核：新请求相对水位的推进量（未写过字段回 0）。
pub fn epoch_gap(gate: &WriteGate, f: PebField, epoch: u32) -> u32 {
    let w = gate.level(f);
    if epoch > w {
        epoch - w
    } else {
        0
    }
}

/// 已写过的字段清点。
pub fn touched_count(gate: &WriteGate) -> usize {
    gate.watermark.len()
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(gate: &WriteGate) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    h = (h.wrapping_mul(211).wrapping_add(gate.sealed as u64)) & 0xFFFF_FFFF;
    for (f, w) in &gate.watermark {
        h = (h.wrapping_mul(211).wrapping_add(*f as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(*w as u64)) & 0xFFFF_FFFF;
    }
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 进程写内核字段 → ROLE
    let mut g = WriteGate::new();
    let r1 = WriteReq { who: Writer::Process, field: PebField::NumProc, epoch: 1 };
    if g.approve(&r1) == Err(SEG_W54_ROLE) {
        ok[0] = true;
    }
    // 样2: 内核写进程字段 → ROLE
    let mut g2 = WriteGate::new();
    let r2 = WriteReq { who: Writer::Kernel, field: PebField::ProcParams, epoch: 1 };
    if g2.approve(&r2) == Err(SEG_W54_ROLE) {
        ok[1] = true;
    }
    // 样3: 同 epoch 重放 → REPLAY
    let mut g3 = WriteGate::new();
    let r3 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 5 };
    let _ = g3.approve(&r3);
    let r3b = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 5 };
    if g3.approve(&r3b) == Err(SEG_W54_REPLAY) {
        ok[2] = true;
    }
    // 样4: 回退 epoch → REPLAY
    let r4 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 3 };
    if g3.approve(&r4) == Err(SEG_W54_REPLAY) {
        ok[3] = true;
    }
    // 样5: 封存后写 → SEALED
    let mut g4 = WriteGate::new();
    g4.seal();
    let r5 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 1 };
    if g4.approve(&r5) == Err(SEG_W54_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12054_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12054");
    // J1 内核写 NumProc 合法
    let mut g = WriteGate::new();
    let r = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 1 };
    cs.add("J1", g.approve(&r).is_ok(), "kernel numproc");
    // J2 水位提升
    cs.add("J2", g.level(PebField::NumProc) == 1, "watermark 1");
    // J3 更高 epoch 接受
    let r2 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 2 };
    cs.add("J3", g.approve(&r2).is_ok() && g.level(PebField::NumProc) == 2, "epoch 2 ok");
    // J4 同 epoch 回放
    let r2b = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 2 };
    cs.add("J4", g.approve(&r2b).err() == Some(SEG_W54_REPLAY), "replay same");
    // J5 回退 epoch
    let r3 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 1 };
    cs.add("J5", g.approve(&r3).err() == Some(SEG_W54_REPLAY), "replay back");
    // J6 进程写 ProcParams 合法
    let mut g2 = WriteGate::new();
    let r4 = WriteReq { who: Writer::Process, field: PebField::ProcParams, epoch: 10 };
    cs.add("J6", g2.approve(&r4).is_ok(), "proc params ok");
    // J7 进程写内核字段拒绝
    let r5 = WriteReq { who: Writer::Process, field: PebField::MaxODS, epoch: 1 };
    cs.add("J7", g2.approve(&r5).err() == Some(SEG_W54_ROLE), "proc maxods role");
    // J8 内核写进程字段拒绝
    let r6 = WriteReq { who: Writer::Kernel, field: PebField::AltStore, epoch: 1 };
    cs.add("J8", g2.approve(&r6).err() == Some(SEG_W54_ROLE), "kernel altstore role");
    // J9 多字段水位独立
    let r7 = WriteReq { who: Writer::Kernel, field: PebField::MaxODS, epoch: 3 };
    let _ = g2.approve(&r7);
    cs.add("J9", g2.level(PebField::MaxODS) == 3 && g2.level(PebField::NumProc) == 0, "independent wm");
    // J10 未写过的字段水位 0
    cs.add("J10", g.level(PebField::ProcParams) == 0, "untouched wm 0");
    // J11 封存后写
    let mut g3 = WriteGate::new();
    g3.seal();
    let r8 = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 9 };
    cs.add("J11", g3.approve(&r8).err() == Some(SEG_W54_SEALED), "sealed write");
    // J12 字段标签
    cs.add("J12", PebField::NumProc.label() == "NumProc", "field label");
    // J13 四字段 ACL 覆盖
    let acl_ok = PebField::NumProc.acl() == Writer::Kernel
        && PebField::MaxODS.acl() == Writer::Kernel
        && PebField::ProcParams.acl() == Writer::Process
        && PebField::AltStore.acl() == Writer::Process;
    cs.add("J13", acl_ok, "acl table");
    // J14 注入五样
    let inj = inject_five();
    cs.add("J14", inj.iter().all(|&b| b), "inject five");

    // J15 汇总面补充：ACL 摘要 / 域标签 / epoch 间距 / 触面清点。
    let j15 = {
        let mut g = WriteGate::new();
        let _ = g.approve(&WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 10 });
        let _ = g.approve(&WriteReq { who: Writer::Process, field: PebField::ProcParams, epoch: 3 });
        writer_label(Writer::Kernel) == "KRNL"
            && writer_label(Writer::Process) == "PROC"
            && acl_summary(PebField::NumProc) == "acl[NumProc|KRNL]"
            && acl_summary(PebField::AltStore) == "acl[AltStore|PROC]"
            && epoch_gap(&g, PebField::NumProc, 15) == 5
            && epoch_gap(&g, PebField::NumProc, 8) == 0
            && epoch_gap(&g, PebField::MaxODS, 1) == 1
            && touched_count(&g) == 2
    };
    cs.add("J15-诊断面四件", j15, "标签/摘要/间距/清点全对");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_split() {
        let mut g = WriteGate::new();
        let k = WriteReq { who: Writer::Kernel, field: PebField::NumProc, epoch: 1 };
        let p = WriteReq { who: Writer::Process, field: PebField::NumProc, epoch: 1 };
        assert!(g.approve(&k).is_ok());
        assert_eq!(g.approve(&p).err(), Some(SEG_W54_ROLE));
    }

    #[test]
    fn test_replay_window() {
        let mut g = WriteGate::new();
        let _ = g.approve(&WriteReq { who: Writer::Kernel, field: PebField::MaxODS, epoch: 4 });
        assert!(g.approve(&WriteReq { who: Writer::Kernel, field: PebField::MaxODS, epoch: 5 }).is_ok());
        assert_eq!(g.approve(&WriteReq { who: Writer::Kernel, field: PebField::MaxODS, epoch: 4 }).err(), Some(SEG_W54_REPLAY));
    }

    #[test]
    fn test_gap_and_summary() {
        let mut g = WriteGate::new();
        let _ = g.approve(&WriteReq { who: Writer::Kernel, field: PebField::MaxODS, epoch: 7 });
        assert_eq!(epoch_gap(&g, PebField::MaxODS, 10), 3);
        assert_eq!(epoch_gap(&g, PebField::MaxODS, 5), 0);
        assert_eq!(touched_count(&g), 1);
        assert_eq!(acl_summary(PebField::ProcParams), "acl[ProcParams|PROC]");
        assert_eq!(writer_label(Writer::Process), "PROC");
    }

    #[test]
    fn test_fold_stable() {
        let mut g = WriteGate::new();
        let _ = g.approve(&WriteReq { who: Writer::Process, field: PebField::ProcParams, epoch: 7 });
        let a = fold(&g);
        let b = fold(&g);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
