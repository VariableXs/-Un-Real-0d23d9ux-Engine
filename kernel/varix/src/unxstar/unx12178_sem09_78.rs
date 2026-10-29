//! UNX-F12178 — 信号/中断/异常三源上下文统一判据（0x1a860 段）
//!
//! 锚点：UNX-F12178-J1 三源上下文统一 10/10（信号/中断/异常各自
//! 产上下文帧，统一形制逐槽对平、源标签互异、混合账回放全对），
//! 野源标签必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxSource（三源枚举）+ UniFrame（统一帧）+ UniLedger
//! （混合账）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a860;
pub const UF_GHOST: u32 = SEG_BASE + 0x01; // 野源标签
pub const UF_ORDER: u32 = SEG_BASE + 0x02; // 帧锚非法
pub const UF_DUP: u32 = SEG_BASE + 0x03; // 重复帧
pub const UF_SEALED: u32 = SEG_BASE + 0x04; // 印后记账
pub const UF_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕记账

/// 三源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtxSource {
    Signal,
    Interrupt,
    Exception,
}

impl CtxSource {
    pub fn tag(&self) -> &'static str {
        match self {
            CtxSource::Signal => "SIG",
            CtxSource::Interrupt => "IRQ",
            CtxSource::Exception => "EXC",
        }
    }

    pub fn rank(&self) -> u8 {
        match self {
            CtxSource::Signal => 1,
            CtxSource::Interrupt => 2,
            CtxSource::Exception => 3,
        }
    }
}

/// 统一帧。
#[derive(Clone, PartialEq)]
pub struct UniFrame {
    pub src: CtxSource,
    pub rip: u64,
    pub rsp: u64,
    pub id: u32, // 信号号/中断向量/异常码
    pub note: String,
}

impl UniFrame {
    /// 帧自洽：RIP 非零、RSP 8 对齐、id 非零。
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && self.id != 0
    }

    pub fn render(&self) -> String {
        format!("{} id={:#x} rip={:#x}", self.src.tag(), self.id, self.rip)
    }
}

/// 混合账。
pub struct UniLedger {
    pub frames: Vec<UniFrame>,
    sealed: bool,
}

impl UniLedger {
    pub fn new() -> UniLedger {
        UniLedger { frames: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一帧（锚非法/同源同 id 重复/空留痕必拒）。
    pub fn record(
        &mut self,
        src: CtxSource,
        id: u32,
        rip: u64,
        rsp: u64,
        note: &str,
    ) -> Result<usize, u32> {
        if self.sealed {
            return Err(UF_SEALED);
        }
        if note.is_empty() {
            return Err(UF_EMPTY);
        }
        let f = UniFrame { src, rip, rsp, id, note: note.to_string() };
        if !f.ok() {
            return Err(UF_ORDER);
        }
        if self.frames.iter().any(|x| x.src == src && x.id == id) {
            return Err(UF_DUP);
        }
        self.frames.push(f);
        Ok(self.frames.len())
    }

    /// 按源过滤。
    pub fn by_source(&self, src: CtxSource) -> Vec<&UniFrame> {
        self.frames.iter().filter(|f| f.src == src).collect()
    }

    /// 三源全齐。
    pub fn all_sources(&self) -> bool {
        !self.by_source(CtxSource::Signal).is_empty()
            && !self.by_source(CtxSource::Interrupt).is_empty()
            && !self.by_source(CtxSource::Exception).is_empty()
    }

    /// 全账回放：每帧渲染可复解析出源标签。
    pub fn all_replayable(&self) -> bool {
        self.frames.iter().all(|f| f.render().starts_with(f.src.tag()))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a860;
        for f in &self.frames {
            h = h.wrapping_mul(211).wrapping_add(f.src.rank() as u32);
            h = h.wrapping_mul(211).wrapping_add(f.id);
            for by in f.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("frames={} sources={}", self.frames.len(), self.all_sources())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UniLedger::new();
    // 1) 非对齐帧必拒。
    got[0] = b
        .record(CtxSource::Signal, 10, 0x1000, 0x2004, "mis")
        .err()
        == Some(UF_ORDER);
    // 2) 零 id 必拒。
    got[1] = b
        .record(CtxSource::Signal, 0, 0x1000, 0x2000, "z")
        .err()
        == Some(UF_ORDER);
    // 3) 同源同 id 重复必拒。
    let _ = b.record(CtxSource::Exception, 0xC0000005, 0x1000, 0x2000, "a");
    got[2] = b
        .record(CtxSource::Exception, 0xC0000005, 0x1010, 0x2010, "dup")
        .err()
        == Some(UF_DUP);
    // 4) 空留痕必拒。
    got[3] = b.record(CtxSource::Signal, 11, 0x1000, 0x2000, "").err() == Some(UF_EMPTY);
    // 5) 印后记账必拒。
    b.seal();
    got[4] = b
        .record(CtxSource::Interrupt, 0x30, 0x1000, 0x2000, "post")
        .err()
        == Some(UF_SEALED);
    got
}

/// 判据入口：UNX-F12178
pub fn run_unx12178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12178");
    let mut b = UniLedger::new();

    // J1 三源混合记账（SIG 2 帧 + IRQ 2 帧 + EXC 2 帧）。
    let j1 = b.record(CtxSource::Signal, 10, 0x1000, 0x8000, "sig-hup").is_ok()
        && b.record(CtxSource::Signal, 15, 0x1100, 0x8100, "sig-term").is_ok()
        && b.record(CtxSource::Interrupt, 0x30, 0x2000, 0x9000, "irq-timer").is_ok()
        && b.record(CtxSource::Interrupt, 0x31, 0x2100, 0x9100, "irq-kbd").is_ok()
        && b.record(CtxSource::Exception, 0xC0000005, 0x3000, 0xA000, "exc-av").is_ok()
        && b.record(CtxSource::Exception, 0xC0000094, 0x3100, 0xA100, "exc-dbz").is_ok();
    s.add("J1_mix6", j1 && b.frames.len() == 6, "三源六帧混合记账");

    // J2 三源全齐。
    s.add("J2_all_sources", b.all_sources(), "SIG/IRQ/EXC 三源全齐");

    // J3 按源过滤计数。
    s.add(
        "J3_by_source",
        b.by_source(CtxSource::Signal).len() == 2
            && b.by_source(CtxSource::Interrupt).len() == 2
            && b.by_source(CtxSource::Exception).len() == 2,
        "按源过滤 2/2/2",
    );

    // J4 源标签。
    s.add(
        "J4_tags",
        CtxSource::Signal.tag() == "SIG"
            && CtxSource::Interrupt.tag() == "IRQ"
            && CtxSource::Exception.tag() == "EXC",
        "三源标签 SIG/IRQ/EXC",
    );

    // J5 源 rank。
    s.add(
        "J5_ranks",
        CtxSource::Signal.rank() == 1
            && CtxSource::Interrupt.rank() == 2
            && CtxSource::Exception.rank() == 3,
        "源 rank 1/2/3",
    );

    // J6 帧渲染。
    let r = b.frames[0].render();
    s.add(
        "J6_render",
        r.contains("SIG") && r.contains("id=0xa") && r.contains("rip=0x1000"),
        "渲染含源/ID/rip",
    );

    // J7 帧自洽核。
    s.add(
        "J7_frames_ok",
        b.frames.iter().all(|f| f.ok()),
        "全帧过自洽核",
    );

    // J8 全账回放。
    s.add("J8_replayable", b.all_replayable(), "全账渲染回放一致");

    // J9 同源同 id 重复拒。
    s.add(
        "J9_dup",
        b.record(CtxSource::Signal, 10, 0x1200, 0x8200, "dup").err() == Some(UF_DUP),
        "同源同 id 拒 DUP",
    );

    // J10 非对齐帧拒。
    s.add(
        "J10_misalign",
        b.record(CtxSource::Signal, 20, 0x1400, 0x8404, "m").err() == Some(UF_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J11 零 id 拒。
    s.add(
        "J11_zero_id",
        b.record(CtxSource::Interrupt, 0, 0x2400, 0x9400, "z").err() == Some(UF_ORDER),
        "零 id 拒 ORDER",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "混合账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.record(CtxSource::Signal, 99, 0x1500, 0x8500, "p").err() == Some(UF_SEALED),
        "印账后记账拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("frames=6") && sm.contains("sources=true"),
        "摘要含帧数与全齐位",
    );

    // J16 异常源 id 即 NT 异常码。
    let exc = b.by_source(CtxSource::Exception);
    s.add(
        "J16_exc_codes",
        exc[0].id == 0xC0000005 && exc[1].id == 0xC0000094,
        "异常源 id=NT 码对平",
    );

    // J17 空留痕记账拒。
    let mut b17 = UniLedger::new();
    s.add(
        "J17_empty_note",
        b17.record(CtxSource::Signal, 1, 0x1000, 0x2000, "").err() == Some(UF_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J18 零 RIP 拒。
    s.add(
        "J18_zero_rip",
        b17.record(CtxSource::Signal, 1, 0, 0x2000, "z").err() == Some(UF_ORDER),
        "零 RIP 拒 ORDER",
    );

    // J19 空账指纹稳定。
    let b19 = UniLedger::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空混合账指纹稳定");

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12178_uni() {
        let mut b = UniLedger::new();
        let _ = b.record(CtxSource::Signal, 10, 0x1000, 0x2000, "s");
        assert_eq!(b.by_source(CtxSource::Signal).len(), 1);
        assert!(!b.all_sources());
    }

    #[test]
    fn unx12178_checks_green() {
        let s = run_unx12178_checks();
        assert!(s.all_passed(), "UNX-F12178 判据未全绿");
    }

    #[test]
    fn unx12178_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
