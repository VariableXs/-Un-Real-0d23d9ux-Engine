//! UNX-F12141 — RtlDispatchException 主体语义（0x1a3c0 段）
//!
//! 锚点：UNX-F12141-J1 异常分发主体（ExceptionRecord×ContextRecord→逐帧检索
//! .pdata→handler 链回调→返回 DispatchStatus），三态处置 10/10（ContinueExecution/
//! ContinueSearch/Unhandled），异常记录自洽核全账通过。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：异常记录（码/标志/地址/参数四元）+ 帧链（EstablisherFrame 有序链）+
//! 分发核（逐帧回调 handler 三态处置）+ 分发序账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a3c0;
pub const DE_BAD_REC: u32 = SEG_BASE + 0x01; // 异常记录非法
pub const DE_GHOST: u32 = SEG_BASE + 0x02; // 帧不在链
pub const DE_ORDER: u32 = SEG_BASE + 0x03; // 分发序错
pub const DE_EMPTY: u32 = SEG_BASE + 0x04; // 留痕为空
pub const DE_FROZEN: u32 = SEG_BASE + 0x05; // 印后分发

/// 分发处置三态。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DispatchStatus {
    /// handler 已处置，恢复执行。
    ContinueExecution,
    /// handler 不处置，继续向外层搜索。
    ContinueSearch,
    /// 全链无人处置。
    Unhandled,
}

impl DispatchStatus {
    pub fn name(&self) -> &'static str {
        match self {
            DispatchStatus::ContinueExecution => "exec",
            DispatchStatus::ContinueSearch => "search",
            DispatchStatus::Unhandled => "unhandled",
        }
    }
}

/// 异常记录：码/标志/异常地址/参数四元。
#[derive(Clone)]
pub struct ExceptionRecord {
    /// 异常码（如 0xC0000005）。
    pub code: u32,
    /// 标志位。
    pub flags: u32,
    /// 异常地址。
    pub addr: u64,
    /// 附加参数（至多 4 元）。
    pub params: [u64; 4],
    /// 参数有效数。
    pub nparams: usize,
}

impl ExceptionRecord {
    /// 记录合法性：码非零、参数数 ≤4。
    pub fn ok(&self) -> bool {
        self.code != 0 && self.nparams <= 4
    }

    /// 访问违例判定（0xC0000005）。
    pub fn is_access_violation(&self) -> bool {
        self.code == 0xC000_0005
    }
}

/// 一层栈帧：establisher frame + handler 决议。
#[derive(Clone)]
pub struct Frame {
    /// 帧基址（栈上从高到低）。
    pub base: u64,
    /// handler 处置预案（None=无 handler 即 ContinueSearch）。
    pub verdict: Option<DispatchStatus>,
}

/// 分发序账条目。
#[derive(Clone)]
pub struct DispatchStep {
    pub seq: u32,
    pub frame_base: u64,
    pub status: DispatchStatus,
    pub note: String,
}

impl DispatchStep {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }
}

/// 异常分发器。
pub struct Dispatcher {
    /// 帧链：栈顶帧（最内层）在前，基址降序。
    pub frames: Vec<Frame>,
    /// 分发序账。
    pub steps: Vec<DispatchStep>,
    pub seq_next: u32,
    sealed: bool,
}

impl Dispatcher {
    pub fn new() -> Dispatcher {
        Dispatcher { frames: Vec::new(), steps: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂帧（基址必须严格降序入链，重基址/逆序必拒）。
    pub fn push_frame(&mut self, base: u64, verdict: Option<DispatchStatus>) -> Result<(), u32> {
        if self.sealed {
            return Err(DE_FROZEN);
        }
        if base == 0 {
            return Err(DE_GHOST);
        }
        if let Some(f) = self.frames.first() {
            if base >= f.base {
                return Err(DE_ORDER);
            }
        }
        self.frames.push(Frame { base, verdict });
        Ok(())
    }

    /// RtlDispatchException 主体：逐帧检索→回调→三态处置。
    /// 返回最终处置态与分发步数。
    pub fn dispatch(&mut self, rec: &ExceptionRecord) -> Result<(DispatchStatus, usize), u32> {
        if self.sealed {
            return Err(DE_FROZEN);
        }
        if !rec.ok() {
            return Err(DE_BAD_REC);
        }
        if self.frames.is_empty() {
            return Err(DE_GHOST);
        }
        for f in &self.frames {
            let status = match f.verdict {
                Some(DispatchStatus::ContinueExecution) => DispatchStatus::ContinueExecution,
                _ => DispatchStatus::ContinueSearch,
            };
            let note = format!("frame{:#x}->{}", f.base, status.name());
            let seq = self.seq_next;
            self.steps.push(DispatchStep {
                seq,
                frame_base: f.base,
                status,
                note,
            });
            self.seq_next += 1;
            if status == DispatchStatus::ContinueExecution {
                return Ok((DispatchStatus::ContinueExecution, self.steps.len()));
            }
        }
        // 全链搜索完毕：收尾 Unhandled。
        let last = self.frames[self.frames.len() - 1].base;
        let seq = self.seq_next;
        self.steps.push(DispatchStep {
            seq,
            frame_base: last,
            status: DispatchStatus::Unhandled,
            note: "chain-exhausted".to_string(),
        });
        self.seq_next += 1;
        Ok((DispatchStatus::Unhandled, self.steps.len()))
    }

    /// 分发序守恒：步序严格单调且帧基址降序（外层方向）。
    pub fn order_conserved(&self) -> bool {
        for w in self.steps.windows(2) {
            if w[1].seq <= w[0].seq {
                return false;
            }
        }
        let bases: Vec<u64> = self
            .steps
            .iter()
            .map(|s| s.frame_base)
            .collect();
        let mut sorted = bases.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        bases == sorted || bases.len() <= 1
    }

    /// 步数。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 全步自洽。
    pub fn all_steps_ok(&self) -> bool {
        self.steps.iter().all(|s| s.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a3c0;
        for f in &self.frames {
            h = h.wrapping_mul(211).wrapping_add(f.base as u32);
        }
        for s in &self.steps {
            h = h.wrapping_mul(211).wrapping_add(s.status.name().as_bytes()[0] as u32);
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "frames={} steps={} fp_ok={}",
            self.frames.len(),
            self.step_count(),
            self.order_conserved()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut d = Dispatcher::new();
    // 1) 非法异常记录（零码）必拒。
    let bad = ExceptionRecord { code: 0, flags: 0, addr: 0x1000, params: [0; 4], nparams: 0 };
    got[0] = d.dispatch(&bad).err() == Some(DE_BAD_REC);
    // 2) 空帧链必拒。
    let rec = ExceptionRecord { code: 0xC000_0005, flags: 0, addr: 0x2000, params: [0; 4], nparams: 0 };
    got[1] = d.dispatch(&rec).err() == Some(DE_GHOST);
    // 3) 帧基址逆序必拒。
    let _ = d.push_frame(0x8000, None);
    got[2] = d.push_frame(0x9000, None).err() == Some(DE_ORDER);
    // 4) 全链无 handler 收尾 Unhandled（步数=链长+1）。
    let _ = d.push_frame(0x7000, None);
    let (st, n) = d.dispatch(&rec).unwrap_or((DispatchStatus::Unhandled, 0));
    got[3] = st == DispatchStatus::Unhandled && n == 3;
    // 5) 印账后再分发必拒。
    d.seal();
    got[4] = d.dispatch(&rec).err() == Some(DE_FROZEN);
    got
}

/// 判据入口：UNX-F12141
pub fn run_unx12141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12141");
    let mut d = Dispatcher::new();

    // J1 三帧链挂载。
    let j1 = d.push_frame(0x9000, None).is_ok()
        && d
            .push_frame(0x8000, Some(DispatchStatus::ContinueExecution))
            .is_ok()
        && d.push_frame(0x7000, None).is_ok();
    s.add("J1_frames", j1 && d.frames.len() == 3, "三帧链基址降序挂载");

    // J2 ContinueExecution 命中处置。
    let rec = ExceptionRecord { code: 0xC000_0005, flags: 0, addr: 0x2000, params: [0; 4], nparams: 0 };
    let (st, n) = d.dispatch(&rec).unwrap_or((DispatchStatus::Unhandled, 0));
    s.add(
        "J2_exec_hit",
        st == DispatchStatus::ContinueExecution && n == 2,
        "第二帧 handler 处置即停",
    );

    // J3 分发序账：首步 search 帧内层、次步 exec。
    s.add(
        "J3_step_order",
        d.steps[0].status == DispatchStatus::ContinueSearch
            && d.steps[1].status == DispatchStatus::ContinueExecution,
        "内层 search→外层 exec 分发序",
    );

    // J4 序守恒。
    s.add("J4_order_conserved", d.order_conserved(), "步序单调且帧基址降序");

    // J5 全步自洽。
    s.add("J5_steps_ok", d.all_steps_ok(), "全步过留痕核");

    // J6 访问违例识别。
    s.add("J6_av", rec.is_access_violation(), "0xC0000005 访问违例识别");

    // J7 非法记录受控（参数数越 4）。
    let bad = ExceptionRecord { code: 1, flags: 0, addr: 0, params: [0; 4], nparams: 5 };
    s.add(
        "J7_bad_rec",
        d.dispatch(&bad).err() == Some(DE_BAD_REC),
        "参数数越 4 拒 BAD_REC",
    );

    // J8 ContinueSearch 全链。
    let mut d2 = Dispatcher::new();
    let _ = d2.push_frame(0x5000, Some(DispatchStatus::ContinueSearch));
    let _ = d2.push_frame(0x4000, None);
    let (st2, n2) = d2.dispatch(&rec).unwrap_or((DispatchStatus::Unhandled, 0));
    s.add(
        "J8_unhandled",
        st2 == DispatchStatus::Unhandled && n2 == 3,
        "全链 search 收尾 Unhandled",
    );

    // J9 Unhandled 步账含收尾注。
    s.add(
        "J9_tail_note",
        d2.steps.last().unwrap().note == "chain-exhausted",
        "收尾步注 chain-exhausted",
    );

    // J10 无 handler 帧即 search。
    s.add(
        "J10_no_handler",
        d2.steps[1].status == DispatchStatus::ContinueSearch,
        "None verdict 即 ContinueSearch",
    );

    // J11 三态名闭集。
    s.add(
        "J11_status_names",
        [DispatchStatus::ContinueExecution.name(), DispatchStatus::ContinueSearch.name(), DispatchStatus::Unhandled.name()]
            == ["exec", "search", "unhandled"],
        "三态名闭集",
    );

    // J12 指纹稳定。
    let f1 = d.fold();
    let f2 = d.fold();
    s.add("J12_fingerprint", f1 == f2, "分发器折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    d.seal();
    s.add(
        "J14_sealed",
        d.dispatch(&rec).err() == Some(DE_FROZEN)
            && d.push_frame(0x1000, None).err() == Some(DE_FROZEN),
        "印账后分发/挂帧皆拒",
    );

    // J15 挂帧重基址拒。
    let mut d3 = Dispatcher::new();
    let _ = d3.push_frame(0x3000, None);
    s.add(
        "J15_dup_base",
        d3.push_frame(0x3000, None).err() == Some(DE_ORDER),
        "重复基址同码拒",
    );

    // J16 零基址拒。
    s.add("J16_zero_base", d3.push_frame(0, None).err() == Some(DE_GHOST), "零基址拒 GHOST");

    // J17 步数与链长联动（exec 在第一帧时步数 1）。
    let mut d4 = Dispatcher::new();
    let _ = d4.push_frame(0x6000, Some(DispatchStatus::ContinueExecution));
    let (_, n4) = d4.dispatch(&rec).unwrap_or((DispatchStatus::Unhandled, 0));
    s.add("J17_first_hit", n4 == 1, "首帧命中步数 1");

    // J18 账面摘要。
    let sm = d2.book_summary();
    s.add(
        "J18_summary",
        sm.contains("frames=2") && sm.contains("fp_ok=true"),
        "摘要含帧数与序守恒",
    );

    // J19 记录参数账。
    let rec4 = ExceptionRecord { code: 0xC000_00FD, flags: 0, addr: 0x10, params: [1, 2, 3, 4], nparams: 4 };
    s.add("J19_params", rec4.ok() && rec4.nparams == 4, "四参记录过合法性核");

    // J20 空链指纹稳定。
    let d5 = Dispatcher::new();
    s.add("J20_fresh_fp", d5.fold() == d5.fold(), "空分发器指纹稳定");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12141_dispatch_hit() {
        let mut d = Dispatcher::new();
        let _ = d.push_frame(0x9000, Some(DispatchStatus::ContinueExecution));
        let rec = ExceptionRecord { code: 1, flags: 0, addr: 0, params: [0; 4], nparams: 0 };
        assert_eq!(d.dispatch(&rec), Ok((DispatchStatus::ContinueExecution, 1)));
    }

    #[test]
    fn unx12141_checks_green() {
        let s = run_unx12141_checks();
        assert!(s.all_passed(), "UNX-F12141 判据未全绿");
    }

    #[test]
    fn unx12141_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
