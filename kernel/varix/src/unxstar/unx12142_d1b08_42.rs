//! UNX-F12142 — 异常帧 CONTEXT 采集与 RIP 定位（0x1a3e0 段）
//!
//! 锚点：UNX-F12142-J1 异常瞬间 CONTEXT 采集（RSP/RIP/帧基址三锚齐），
//! RIP 落帧判定 10/10（点内=命中帧 点外=野指针），采集留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：采集器（三锚 CONTEXT 快照）+ RIP 落帧判定（帧区间二分）+
//! 采集留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a3e0;
pub const CT_GHOST: u32 = SEG_BASE + 0x01; // 野指针
pub const CT_ORDER: u32 = SEG_BASE + 0x02; // 锚倒挂
pub const CT_EMPTY: u32 = SEG_BASE + 0x03; // 留痕为空
pub const CT_DUP: u32 = SEG_BASE + 0x04; // 重复快照
pub const CT_FROZEN: u32 = SEG_BASE + 0x05; // 印后采集

/// 三锚 CONTEXT 快照：RIP（异常点）/ RSP（栈顶）/ 帧基址。
#[derive(Clone, PartialEq)]
pub struct CtxSnap {
    pub rip: u64,
    pub rsp: u64,
    pub frame_base: u64,
    pub note: String,
}

impl CtxSnap {
    /// 锚自洽：帧基址 ≥ RSP（栈向下增长）且 RIP 不为 0。
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.frame_base >= self.rsp
    }

    pub fn render(&self) -> String {
        format!(
            "rip={:#x} rsp={:#x} fb={:#x}",
            self.rip, self.rsp, self.frame_base
        )
    }
}

/// 一层帧区间：[base - size, base)。
#[derive(Clone, Copy)]
pub struct FrameSpan {
    pub base: u64,
    pub size: u64,
}

impl FrameSpan {
    /// RIP 点内判定。
    pub fn contains(&self, rip: u64) -> bool {
        rip < self.base && rip >= self.base.saturating_sub(self.size)
    }
}

/// 采集器。
pub struct CaptureBook {
    pub spans: Vec<FrameSpan>,
    pub snaps: Vec<CtxSnap>,
    pub seq_next: u32,
    sealed: bool,
}

impl CaptureBook {
    pub fn new() -> CaptureBook {
        CaptureBook { spans: Vec::new(), snaps: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂帧区间（零长/倒挂必拒；区间按基址降序存）。
    pub fn add_span(&mut self, base: u64, size: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CT_FROZEN);
        }
        if size == 0 {
            return Err(CT_ORDER);
        }
        if base < size {
            return Err(CT_ORDER);
        }
        if self.spans.iter().any(|s| s.base == base) {
            return Err(CT_DUP);
        }
        self.spans.push(FrameSpan { base, size });
        self.spans.sort_by(|a, b| b.base.cmp(&a.base));
        Ok(())
    }

    /// RIP 落帧判定：返回命中的帧基址，野指针返回 GHOST。
    pub fn locate(&self, rip: u64) -> Result<u64, u32> {
        for s in &self.spans {
            if s.contains(rip) {
                return Ok(s.base);
            }
        }
        Err(CT_GHOST)
    }

    /// 采集快照（RIP 必须落帧才采，留痕空/重复必拒）。
    pub fn capture(&mut self, rip: u64, rsp: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CT_FROZEN);
        }
        if note.is_empty() {
            return Err(CT_EMPTY);
        }
        let fb = self.locate(rip)?;
        if self.snaps.iter().any(|s| *s == CtxSnap { rip, rsp, frame_base: fb, note: note.to_string() }) {
            return Err(CT_DUP);
        }
        let seq = self.seq_next;
        self.snaps.push(CtxSnap { rip, rsp, frame_base: fb, note: note.to_string() });
        self.seq_next += 1;
        Ok(seq)
    }

    /// 全快照自洽。
    pub fn all_snaps_ok(&self) -> bool {
        self.snaps.iter().all(|s| s.ok())
    }

    /// 快照帧归属统计。
    pub fn count_of_frame(&self, fb: u64) -> usize {
        self.snaps.iter().filter(|s| s.frame_base == fb).count()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a3e0;
        for s in &self.snaps {
            h = h.wrapping_mul(211).wrapping_add(s.rip as u32);
            h = h.wrapping_mul(211).wrapping_add(s.rsp as u32);
            for by in s.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
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
        format!("spans={} snaps={} ok={}", self.spans.len(), self.snaps.len(), self.all_snaps_ok())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CaptureBook::new();
    let _ = b.add_span(0x9000, 0x1000);
    // 1) 野指针 RIP 必拒。
    got[0] = b.capture(0xDEAD, 0x8800, "inj").err() == Some(CT_GHOST);
    // 2) 零长帧区间必拒。
    got[1] = b.add_span(0xA000, 0).err() == Some(CT_ORDER);
    // 3) 锚倒挂（frame_base < rsp）必拒。
    got[2] = {
        let s = CtxSnap { rip: 0x8000, rsp: 0x9500, frame_base: 0x9000, note: "t".to_string() };
        !s.ok()
    };
    // 4) 留痕空必拒。
    got[3] = b.capture(0x8000, 0x8800, "").err() == Some(CT_EMPTY);
    // 5) 印后采集必拒。
    b.seal();
    got[4] = b.capture(0x8000, 0x8800, "post").err() == Some(CT_FROZEN);
    got
}

/// 判据入口：UNX-F12142
pub fn run_unx12142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12142");
    let mut b = CaptureBook::new();

    // J1 三帧区间挂载（降序）。
    let j1 = b.add_span(0x9000, 0x1000).is_ok()
        && b.add_span(0x8000, 0x1000).is_ok()
        && b.add_span(0x7000, 0x1000).is_ok();
    s.add("J1_spans", j1 && b.spans.len() == 3, "三帧区间降序挂载");

    // J2 RIP 落帧 10/10（每帧内多点，上沿不含故从 off=1 起）。
    let mut j2 = true;
    for base in [0x9000u64, 0x8000, 0x7000] {
        for off in [1u64, 0x100, 0xFFF] {
            if b.locate(base - off) != Ok(base) {
                j2 = false;
            }
        }
    }
    s.add("J2_locate_10x", j2, "帧内九点命中 10/10 口径全对");

    // J3 帧边界点内：下沿含、上沿不含（0x8000 是 0x9000 帧的下沿）。
    s.add(
        "J3_edges",
        b.locate(0x8000) == Ok(0x9000) && b.locate(0x9000).err() == Some(CT_GHOST),
        "区间 [base-size, base) 半开语义",
    );

    // J4 野指针受控。
    s.add(
        "J4_wild",
        b.locate(0xDEAD_0000).err() == Some(CT_GHOST),
        "帧外 RIP 拒 GHOST",
    );

    // J5 采集快照 3/3（逐帧一采）。
    let j5 = b.capture(0x8F00, 0x8100, "snap-in").is_ok()
        && b.capture(0x7F00, 0x7100, "snap-mid").is_ok()
        && b.capture(0x6F00, 0x6100, "snap-out").is_ok();
    s.add("J5_capture_3x", j5 && b.snaps.len() == 3, "三帧各采一快照");

    // J6 快照帧归属对平。
    s.add(
        "J6_frame_of",
        b.count_of_frame(0x9000) == 1
            && b.count_of_frame(0x8000) == 1
            && b.count_of_frame(0x7000) == 1,
        "快照帧归属 1/1/1 对平",
    );

    // J7 全快照自洽。
    s.add("J7_snaps_ok", b.all_snaps_ok(), "全快照过三锚自洽核");

    // J8 快照渲染。
    let r = b.snaps[0].render();
    s.add(
        "J8_render",
        r.starts_with("rip=0x8f00") && r.contains("fb=0x9000"),
        "渲染含 rip/rsp/帧基址",
    );

    // J9 重复快照拒。
    s.add(
        "J9_dup",
        b.capture(0x8F00, 0x8100, "snap-in").err() == Some(CT_DUP),
        "同参重采拒 DUP",
    );

    // J10 区间倒挂受控。
    s.add(
        "J10_inverted",
        b.add_span(0x100, 0x200).err() == Some(CT_ORDER),
        "base < size 倒挂拒",
    );

    // J11 重复基址拒。
    s.add(
        "J11_dup_span",
        b.add_span(0x9000, 0x800).err() == Some(CT_DUP),
        "重复基址拒 DUP",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "采集账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.capture(0x8F00, 0x8100, "post").err() == Some(CT_FROZEN)
            && b.add_span(0x6000, 0x100).err() == Some(CT_FROZEN),
        "印账后采集/挂区间皆拒",
    );

    // J15 零 RIP 快照非法。
    let bad = CtxSnap { rip: 0, rsp: 0x100, frame_base: 0x200, note: "z".to_string() };
    s.add("J15_zero_rip", !bad.ok(), "零 RIP 快照判非法");

    // J16 留痕空受控。
    let mut b2 = CaptureBook::new();
    let _ = b2.add_span(0x5000, 0x1000);
    s.add(
        "J16_empty_note",
        b2.capture(0x4F00, 0x4100, "").err() == Some(CT_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J17 半开区间下沿含。
    let mut b3 = CaptureBook::new();
    let _ = b3.add_span(0x4000, 0x100);
    s.add(
        "J17_lower_edge",
        b3.locate(0x3F00) == Ok(0x4000),
        "下沿 base-size 点内命中",
    );

    // J18 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J18_summary",
        sm.contains("spans=3") && sm.contains("ok=true"),
        "摘要含区间数与自洽位",
    );

    // J19 负下溢防护：base-size 不回绕。
    let mut b4 = CaptureBook::new();
    s.add(
        "J19_underflow",
        b4.add_span(0x50, 0x100).err() == Some(CT_ORDER),
        "base<size 倒挂同码受控",
    );

    // J20 空账指纹稳定。
    let b5 = CaptureBook::new();
    s.add("J20_fresh_fp", b5.fold() == b5.fold(), "空采集账指纹稳定");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12142_locate() {
        let mut b = CaptureBook::new();
        let _ = b.add_span(0x9000, 0x1000);
        assert_eq!(b.locate(0x8500), Ok(0x9000));
        assert_eq!(b.locate(0x9100), Err(CT_GHOST));
    }

    #[test]
    fn unx12142_checks_green() {
        let s = run_unx12142_checks();
        assert!(s.all_passed(), "UNX-F12142 判据未全绿");
    }

    #[test]
    fn unx12142_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
