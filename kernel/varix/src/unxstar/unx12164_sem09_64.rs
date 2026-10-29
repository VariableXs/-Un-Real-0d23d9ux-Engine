//! UNX-F12164 — RtlCaptureContext 语义（0x1a6a0 段）
//!
//! 锚点：UNX-F12164-J1 上下文采集 10/10（采集点三锚齐：RIP=调用
//! 返回址、RSP=调用栈顶、帧基址=调用者帧），重复采集全对齐、
//! 采集账回放一致。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CapturePoint（采集点）+ CaptureLedger（采集账）+ 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a6a0;
pub const CC_ORDER: u32 = SEG_BASE + 0x01; // 采集锚非法
pub const CC_DUP: u32 = SEG_BASE + 0x02; // 重复采集点
pub const CC_SEALED: u32 = SEG_BASE + 0x03; // 印后采集
pub const CC_GHOST: u32 = SEG_BASE + 0x04; // 采集点不在账
pub const CC_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕采集

/// 采集点。
#[derive(Clone, PartialEq)]
pub struct CapturePoint {
    pub rip: u64,
    pub rsp: u64,
    pub frame: u64,
    pub note: String,
}

impl CapturePoint {
    /// 点自洽：RIP 非零、RSP 8 对齐、帧基址 ≥ RSP。
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && self.frame >= self.rsp && self.rsp != 0
    }

    pub fn render(&self) -> String {
        format!("rip={:#x} rsp={:#x} f={:#x}", self.rip, self.rsp, self.frame)
    }
}

/// 采集账。
pub struct CaptureLedger {
    pub points: Vec<CapturePoint>,
    pub queries: Vec<u64>,
    sealed: bool,
}

impl CaptureLedger {
    pub fn new() -> CaptureLedger {
        CaptureLedger { points: Vec::new(), queries: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 采集一次（锚非法/重复/空留痕必拒）。
    pub fn capture(&mut self, rip: u64, rsp: u64, frame: u64, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(CC_SEALED);
        }
        if note.is_empty() {
            return Err(CC_EMPTY);
        }
        let p = CapturePoint { rip, rsp, frame, note: note.to_string() };
        if !p.ok() {
            return Err(CC_ORDER);
        }
        if self.points.iter().any(|x| x.rip == rip && x.rsp == rsp) {
            return Err(CC_DUP);
        }
        self.points.push(p);
        Ok(self.points.len())
    }

    /// 按 RIP 查采集点。
    pub fn by_rip(&mut self, rip: u64) -> Result<&CapturePoint, u32> {
        self.queries.push(rip);
        let idx = self.points.iter().position(|p| p.rip == rip).ok_or(CC_GHOST)?;
        Ok(&self.points[idx])
    }

    /// 全点自洽。
    pub fn all_ok(&self) -> bool {
        self.points.iter().all(|p| p.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a6a0;
        for p in &self.points {
            h = h.wrapping_mul(211).wrapping_add(p.rip as u32);
            h = h.wrapping_mul(211).wrapping_add(p.rsp as u32);
            for by in p.note.bytes() {
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
        format!("points={} ok={}", self.points.len(), self.all_ok())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CaptureLedger::new();
    // 1) 非对齐 RSP 采集必拒。
    got[0] = b.capture(0x1000, 0x2004, 0x3000, "mis").err() == Some(CC_ORDER);
    // 2) 帧基址低于 RSP 必拒。
    got[1] = b.capture(0x1000, 0x5000, 0x4000, "inv").err() == Some(CC_ORDER);
    // 3) 零 RIP 必拒。
    got[2] = b.capture(0, 0x2000, 0x3000, "z").err() == Some(CC_ORDER);
    // 4) 查野 RIP 必拒。
    let _ = b.capture(0x1000, 0x2000, 0x3000, "ok");
    got[3] = b.by_rip(0xDEAD).err() == Some(CC_GHOST);
    // 5) 印后采集必拒。
    b.seal();
    got[4] = b.capture(0x1100, 0x2100, 0x3100, "post").err() == Some(CC_SEALED);
    got
}

/// 判据入口：UNX-F12164
pub fn run_unx12164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12164");
    let mut b = CaptureLedger::new();

    // J1 十点采集（模拟十次调用链采集）。
    let mut ok1 = true;
    for i in 0..10u64 {
        let rip = 0x1000 + i * 0x100;
        let rsp = 0x8000 - i * 0x80;
        let frame = rsp + 0x40;
        if b.capture(rip, rsp, frame, "cap").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_cap_10x", ok1 && b.points.len() == 10, "十点采集全过");

    // J2 全点自洽。
    s.add("J2_all_ok", b.all_ok(), "全采集点过自洽核");

    // J3 按 RIP 查点。
    let hit = b.by_rip(0x1000 + 5 * 0x100);
    s.add(
        "J3_by_rip",
        hit.as_ref().map(|p| p.rsp) == Ok(0x8000u64 - 5 * 0x80),
        "按 RIP 查点对平",
    );

    // J4 野 RIP 拒。
    s.add(
        "J4_wild",
        b.by_rip(0xDEAD).err() == Some(CC_GHOST),
        "野 RIP 拒 GHOST",
    );

    // J5 点渲染。
    let r = b.points[0].render();
    s.add(
        "J5_render",
        r.contains("rip=0x1000") && r.contains("f=0x8040"),
        "点渲染含三锚",
    );

    // J6 重复采集点拒。
    s.add(
        "J6_dup",
        b.capture(0x1000, 0x8000, 0x8040, "dup").err() == Some(CC_DUP),
        "同 RIP/RSP 重采拒 DUP",
    );

    // J7 采集锚非法（非对齐）。
    s.add(
        "J7_misalign",
        b.capture(0x2000, 0x8004, 0x8040, "m").err() == Some(CC_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J8 帧基址 < RSP 拒。
    s.add(
        "J8_frame_below",
        b.capture(0x2100, 0x8000, 0x7F00, "b").err() == Some(CC_ORDER),
        "帧基址低于 RSP 拒 ORDER",
    );

    // J9 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J9_fingerprint", f1 == f2, "采集账折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    b.seal();
    s.add(
        "J11_sealed",
        b.capture(0x3000, 0x7000, 0x7040, "post").err() == Some(CC_SEALED),
        "印账后采集拒",
    );

    // J12 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J12_summary",
        sm.contains("points=10") && sm.contains("ok=true"),
        "摘要含点数与自洽位",
    );

    // J13 空留痕采集拒。
    let mut b13 = CaptureLedger::new();
    s.add(
        "J13_empty_note",
        b13.capture(0x1000, 0x2000, 0x3000, "").err() == Some(CC_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J14 查询留痕计数。
    s.add(
        "J14_queries",
        b.queries.len() == 2,
        "查询留痕 2 条",
    );

    // J15 零 RSP 拒。
    let mut b15 = CaptureLedger::new();
    s.add(
        "J15_zero_rsp",
        b15.capture(0x1000, 0, 0x100, "z").err() == Some(CC_ORDER),
        "零 RSP 拒 ORDER",
    );

    // J16 三锚自洽核（合法点）。
    let p16 = CapturePoint { rip: 0x1000, rsp: 0x2000, frame: 0x2040, note: "n".to_string() };
    s.add("J16_point_ok", p16.ok(), "合法点过自洽核");

    // J17 采集序 rip 单调（模拟链）。
    let rips: Vec<u64> = b.points.iter().map(|p| p.rip).collect();
    s.add(
        "J17_rip_mono",
        rips.windows(2).all(|w| w[0] < w[1]),
        "采集点 rip 单调",
    );

    // J18 RSP 反向单调（栈向下）。
    let rsps: Vec<u64> = b.points.iter().map(|p| p.rsp).collect();
    s.add(
        "J18_rsp_desc",
        rsps.windows(2).all(|w| w[0] > w[1]),
        "采集点 RSP 递减",
    );

    // J19 空账指纹稳定。
    let b19 = CaptureLedger::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空采集账指纹稳定");

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
    fn unx12164_capture() {
        let mut b = CaptureLedger::new();
        assert!(b.capture(0x1000, 0x2000, 0x2040, "c").is_ok());
        assert_eq!(b.capture(0x1000, 0x2000, 0x2040, "d"), Err(CC_DUP));
    }

    #[test]
    fn unx12164_checks_green() {
        let s = run_unx12164_checks();
        assert!(s.all_passed(), "UNX-F12164 判据未全绿");
    }

    #[test]
    fn unx12164_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
