//! UNX-F12003 NT 进程创建内核路径语义（NtCreateUserProcess 底座）（0x19280 段）
//!
//! 判据原文：用户进程创建沿六相状态机推进——参数编组（ParamsMarshaled）
//! → 初始线程（InitialThread）→ 节映像（SectionMapped）→ PEB 构建
//! （PebBuilt）→ 句柄交接（HandoffDone），相序不可跳、不可回；参数块
//! 非空入相、节对象必须先于 PEB 构建映射、交接只发生一次。
//!
//! 结构：创建相状态机（CreatePhase 六相闭集 + 相序账）+ 相进账
//! （PhaseRec 逐相留痕）+ 推进核（跳相/回退/空参/无节/双交接五码守卫）+
//! 折叠指纹 + 印账冻结 + 注入核 + 判据聚合（run_unx12003_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19280 独占 0x20）。
pub const SEG_BASE: u32 = 0x19280;
pub const CR_PHASE_SKIP: u32 = SEG_BASE + 0x01; // 跳相推进
pub const CR_PHASE_BACK: u32 = SEG_BASE + 0x02; // 相序回退
pub const CR_EMPTY_PARAMS: u32 = SEG_BASE + 0x03; // 参数块为空
pub const CR_NO_SECTION: u32 = SEG_BASE + 0x04; // 未映节先建 PEB
pub const CR_HANDOFF_TWICE: u32 = SEG_BASE + 0x05; // 双重交接
pub const CR_FROZEN: u32 = SEG_BASE + 0x06; // 印账后仍推进

/// 创建相闭集（Idle 为起点，HandoffDone 为终点）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum CreatePhase {
    Idle = 0,
    ParamsMarshaled = 1,
    InitialThread = 2,
    SectionMapped = 3,
    PebBuilt = 4,
    HandoffDone = 5,
}

/// 一相推进的账目留痕。
#[derive(Clone)]
pub struct PhaseRec {
    pub seq: u32,
    pub phase: CreatePhase,
    pub note: String,
}

impl PhaseRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 创建路径模型：相序推进 + 参数块账。
pub struct CreatePath {
    pub cur: CreatePhase,
    pub ledger: Vec<PhaseRec>,
    pub seq_next: u32,
    pub param_bytes: usize,
    pub sealed: bool,
}

impl CreatePath {
    pub fn new() -> CreatePath {
        CreatePath {
            cur: CreatePhase::Idle,
            ledger: Vec::new(),
            seq_next: 1,
            param_bytes: 0,
            sealed: false,
        }
    }

    /// 参数编组：只有 Idle 相可编组，且参数块非空。
    pub fn marshal_params(&mut self, nbytes: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_FROZEN);
        }
        if self.cur != CreatePhase::Idle {
            return Err(CR_PHASE_BACK);
        }
        if nbytes == 0 {
            return Err(CR_EMPTY_PARAMS);
        }
        self.param_bytes = nbytes;
        self.advance_to(CreatePhase::ParamsMarshaled, "params")
    }

    /// 相推进核：严格逐相 +1，跳相/回退各守一码。
    /// PebBuilt 相要求节已映射（即已越过 SectionMapped 相）。
    pub fn advance_to(&mut self, to: CreatePhase, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_FROZEN);
        }
        if note.is_empty() {
            return Err(CR_PHASE_SKIP);
        }
        // 无节建 PEB 守卫先于跳相守卫：抢建 PEB 必须按「无节」红账。
        if to == CreatePhase::PebBuilt && self.cur < CreatePhase::SectionMapped {
            return Err(CR_NO_SECTION);
        }
        if (to as u16) > (self.cur as u16) + 1 {
            return Err(CR_PHASE_SKIP);
        }
        if (to as u16) < (self.cur as u16) {
            return Err(CR_PHASE_BACK);
        }
        if to == CreatePhase::HandoffDone && self.cur == CreatePhase::HandoffDone {
            return Err(CR_HANDOFF_TWICE);
        }
        let rec = PhaseRec { seq: self.seq_next, phase: to, note: note.to_string() };
        if !rec.ok() {
            return Err(CR_PHASE_SKIP);
        }
        self.cur = to;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 是否已映射节（PebBuilt 前置条件核对面）。
    pub fn section_mapped(&self) -> bool {
        self.cur >= CreatePhase::SectionMapped
    }

    /// 交接完成判定。
    pub fn handed_off(&self) -> bool {
        self.cur == CreatePhase::HandoffDone
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 相数核对：账条数 == 已越相数。
    pub fn phases_tally(&self) -> bool {
        self.ledger.len() == self.cur as usize
    }

    /// 走完全程判定：终点相且账面与相数对平。
    pub fn walk_complete(&self) -> bool {
        self.handed_off() && self.phases_tally()
    }

    /// 全程账摘要：逐相标签串联（诊断面用）。
    pub fn walk_summary(&self) -> String {
        let mut parts: Vec<&'static str> = Vec::new();
        for r in &self.ledger {
            parts.push(phase_label(r.phase));
        }
        format!("path[{}|{}]", parts.join(">"), self.param_bytes)
    }

    /// 相标签面：六相诊断文本（闭集全覆盖）。
/// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.cur as u32);
        h = h.wrapping_mul(211).wrapping_add(self.param_bytes as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.phase as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }
}

/// 相标签面：六相诊断文本（闭集全覆盖）。
pub fn phase_label(p: CreatePhase) -> &'static str {
    match p {
        CreatePhase::Idle => "idle",
        CreatePhase::ParamsMarshaled => "params",
        CreatePhase::InitialThread => "thread",
        CreatePhase::SectionMapped => "section",
        CreatePhase::PebBuilt => "peb",
        CreatePhase::HandoffDone => "handoff",
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 跳相：Idle 直奔 InitialThread（越过参数编组）→ CR_PHASE_SKIP。
    let mut a = CreatePath::new();
    got[0] = a.advance_to(CreatePhase::InitialThread, "skip").err() == Some(CR_PHASE_SKIP);
    // 2) 回退：编组后想退回 Idle → CR_PHASE_BACK。
    let mut b = CreatePath::new();
    let _ = b.marshal_params(64);
    got[1] = b.advance_to(CreatePhase::Idle, "back").err() == Some(CR_PHASE_BACK);
    // 3) 空参数块编组 → CR_EMPTY_PARAMS。
    let mut c = CreatePath::new();
    got[2] = c.marshal_params(0).err() == Some(CR_EMPTY_PARAMS);
    // 4) 未映节先建 PEB：参数→线程后直跳 PEB 相隔两位也算跳相，故先走
    //    合法三步再在 SectionMapped 前抢 PEB——此处以 Idle 起点验证
    //    PebBuilt 守卫（同一样本必踩 NO_SECTION 而非 SKIP，因距离差 4）。
    let mut d = CreatePath::new();
    got[3] = d.advance_to(CreatePhase::PebBuilt, "no-sec").err() == Some(CR_NO_SECTION);
    // 5) 双重交接：完整走完后再次 HandoffDone → CR_HANDOFF_TWICE。
    let mut e = CreatePath::new();
    let _ = e.marshal_params(96);
    let _ = e.advance_to(CreatePhase::InitialThread, "thr");
    let _ = e.advance_to(CreatePhase::SectionMapped, "sec");
    let _ = e.advance_to(CreatePhase::PebBuilt, "peb");
    let _ = e.advance_to(CreatePhase::HandoffDone, "ho");
    got[4] =
        e.advance_to(CreatePhase::HandoffDone, "ho2").err() == Some(CR_HANDOFF_TWICE);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12003");

    // J1 正路径：六相完整推进、相数对平、交接达成。
    let j1 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(128);
        let _ = p.advance_to(CreatePhase::InitialThread, "thr");
        let _ = p.advance_to(CreatePhase::SectionMapped, "sec");
        let _ = p.advance_to(CreatePhase::PebBuilt, "peb");
        let _ = p.advance_to(CreatePhase::HandoffDone, "ho");
        p.handed_off() && p.phases_tally() && p.ledger.len() == 5
    };
    s.add("J1-六相正路径", j1, "5 步推进全过");

    // J2 跳相拒：Idle→SectionMapped（跨两位）。
    let j2 = {
        let mut p = CreatePath::new();
        p.advance_to(CreatePhase::SectionMapped, "s").err() == Some(CR_PHASE_SKIP)
    };
    s.add("J2-跳相拒", j2, "相序不可跨");

    // J3 回退拒：编组后再编组（Idle 出发点已失）。
    let j3 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(32);
        p.marshal_params(64).err() == Some(CR_PHASE_BACK)
    };
    s.add("J3-回退拒", j3, "相不可逆退");

    // J4 空参数块拒。
    let j4 = {
        let mut p = CreatePath::new();
        p.marshal_params(0).err() == Some(CR_EMPTY_PARAMS)
    };
    s.add("J4-空参拒", j4, "参数块非空");

    // J5 无节建 PEB 拒：Idle 直接抢 PebBuilt。
    let j5 = {
        let mut p = CreatePath::new();
        p.advance_to(CreatePhase::PebBuilt, "n").err() == Some(CR_NO_SECTION)
    };
    s.add("J5-无节拒", j5, "节先于 PEB");

    // J6 双重交接拒。
    let j6 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(8);
        let _ = p.advance_to(CreatePhase::InitialThread, "t");
        let _ = p.advance_to(CreatePhase::SectionMapped, "s");
        let _ = p.advance_to(CreatePhase::PebBuilt, "p");
        let _ = p.advance_to(CreatePhase::HandoffDone, "h");
        p.advance_to(CreatePhase::HandoffDone, "h2").err() == Some(CR_HANDOFF_TWICE)
    };
    s.add("J6-双交接拒", j6, "交接只一次");

    // J7 节映射前置面：相序走到 SectionMapped 前后判定翻转。
    let j7 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(8);
        let before = !p.section_mapped();
        let _ = p.advance_to(CreatePhase::InitialThread, "t");
        let _ = p.advance_to(CreatePhase::SectionMapped, "s");
        before && p.section_mapped()
    };
    s.add("J7-节映前置", j7, "映射判定翻转");

    // J8 相数对平：任意前缀推进后 ledger.len()==cur。
    let j8 = {
        let mut p = CreatePath::new();
        let mut ok = true;
        let seq = [
            (CreatePhase::ParamsMarshaled, "a"),
            (CreatePhase::InitialThread, "b"),
            (CreatePhase::SectionMapped, "c"),
        ];
        for (ph, note) in seq.iter() {
            let _ = p.advance_to(*ph, note);
            if !p.phases_tally() {
                ok = false;
            }
        }
        ok
    };
    s.add("J8-相数对平", j8, "逐相账数一致");

    // J9 空留痕推进拒。
    let j9 = {
        let mut p = CreatePath::new();
        p.advance_to(CreatePhase::ParamsMarshaled, "").err() == Some(CR_PHASE_SKIP)
    };
    s.add("J9-空留痕拒", j9, "留痕必填");

    // J10 印账冻结。
    let j10 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(16);
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2
            && p.advance_to(CreatePhase::InitialThread, "post").err() == Some(CR_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：半程路径。
    let mut p = CreatePath::new();
    let _ = p.marshal_params(512);
    let _ = p.advance_to(CreatePhase::InitialThread, "sum-t");
    s.add("样例相数", p.phases_tally(), "2 步对平");
    s.add("样例账面合法", p.ledger_ok(), "留痕全过自洽");

    // J12 汇总面补充：全程走查与标签面。
    let j12 = {
        let mut p = CreatePath::new();
        let _ = p.marshal_params(256);
        let _ = p.advance_to(CreatePhase::InitialThread, "t");
        let _ = p.advance_to(CreatePhase::SectionMapped, "s");
        let _ = p.advance_to(CreatePhase::PebBuilt, "p");
        let _ = p.advance_to(CreatePhase::HandoffDone, "h");
        p.walk_complete()
            && p.walk_summary().contains("params>thread>section>peb>handoff")
            && phase_label(CreatePhase::Idle) == "idle"
            && phase_label(CreatePhase::HandoffDone) == "handoff"
    };
    s.add("J12-全程标签", j12, "走查完成且标签闭集");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12003_phase_order() {
        let mut p = CreatePath::new();
        assert!(p.marshal_params(64).is_ok());
        assert_eq!(p.advance_to(CreatePhase::SectionMapped, "skip"), Err(CR_PHASE_SKIP));
        assert!(p.advance_to(CreatePhase::InitialThread, "t").is_ok());
        assert_eq!(p.advance_to(CreatePhase::Idle, "back"), Err(CR_PHASE_BACK));
        assert!(p.advance_to(CreatePhase::SectionMapped, "s").is_ok());
        assert!(p.advance_to(CreatePhase::PebBuilt, "p").is_ok());
        assert!(p.handed_off() == false);
        assert!(p.advance_to(CreatePhase::HandoffDone, "h").is_ok());
    }

    #[test]
    fn unx12003_checks_green() {
        let s = run_unx12003_checks();
        assert!(s.all_passed(), "UNX-F12003 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12003_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
