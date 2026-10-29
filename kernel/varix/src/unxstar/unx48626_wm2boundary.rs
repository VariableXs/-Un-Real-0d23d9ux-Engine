//! UNX-F48626 —— I·F2 窗口管理边界复签（窗口本体语义零回流）
//! 判据原文：UNX-F48626-J1 越界注入 ×10 零波及；判据锚定 §AI-61 §1 边界 F2。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           F2 窗管语义单点拒收，本域状态零污染。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0560; // F2 窗管边界诊断段

const E_WM_LEAK: u16 = SEG_BASE + 1;    // 窗管语义回流
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 本域状态污染
const E_CLS_BAD: u16 = SEG_BASE + 3;    // 事件分类错
const E_IME_MISS: u16 = SEG_BASE + 4;   // 本域事件被误拒
const E_LEDGER: u16 = SEG_BASE + 5;     // 边界账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;     // 重演不一致
const E_GEO_BAD: u16 = SEG_BASE + 7;    // 几何域违例

/// 事件源域（F2 边界面）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SrcF2 {
    ImeInput = 0,   // 本域：IME 输入
    ImeState = 1,   // 本域：IME 状态
    WmMove = 2,     // F2：窗口移动
    WmResize = 3,   // F2：窗口缩放
    WmStack = 4,    // F2：窗口层叠
    WmIconify = 5,  // F2：窗口图标化
}

impl SrcF2 {
    /// F2 判定：窗管四类语义零回流。
    pub fn is_wm(&self) -> bool {
        match self {
            SrcF2::WmMove | SrcF2::WmResize | SrcF2::WmStack | SrcF2::WmIconify => true,
            _ => false,
        }
    }
}

/// 边界事件。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct F2Event {
    pub src: SrcF2,
    pub code: u32,
    pub gx: i32, // 几何参数（窗管语义用）
    pub gy: i32,
}

/// 本域边界摄入器。
pub struct F2Ingest {
    pub label: String,
    pub accepted: Vec<u32>,
    pub rejected: Vec<u32>,
    pub state_seed: u64,
    pub state_cur: u64,
}

impl F2Ingest {
    pub fn new(label: &str) -> Self {
        F2Ingest {
            label: label.to_string(),
            accepted: Vec::new(),
            rejected: Vec::new(),
            state_seed: 0x0f2e_0000_dead_beef,
            state_cur: 0x0f2e_0000_dead_beef,
        }
    }

    /// 摄入：F2 窗管语义拒收零波及；本域事件受理。
    pub fn ingest(&mut self, ev: F2Event) -> Result<bool, u16> {
        if ev.src.is_wm() {
            if ev.gx < -0x8000 || ev.gx > 0x8000 || ev.gy < -0x8000 || ev.gy > 0x8000 {
                return Err(E_GEO_BAD);
            }
            self.rejected.push(ev.code);
            if self.state_cur != self.state_seed {
                return Err(E_STATE_DIRTY);
            }
            return Ok(false);
        }
        self.accepted.push(ev.code);
        self.state_cur = self
            .state_cur
            .wrapping_mul(0x0000_0100_0000_01b3)
            ^ (ev.code as u64);
        Ok(true)
    }

    pub fn audit(&self, exp_acc: u32, exp_rej: u32) -> Result<(), u16> {
        if self.accepted.len() as u32 != exp_acc
            || self.rejected.len() as u32 != exp_rej
        {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = self.state_cur;
        h ^= (self.accepted.len() as u64) << 32;
        h ^= self.rejected.len() as u64;
        h
    }
}

/// 越界注入 ×10：窗管四类语义轮注入，全拒零波及。
pub fn inject_wm_x10(ing: &mut F2Ingest) -> Result<(), u16> {
    let mut i = 0u32;
    while i < 10 {
        let src = match i % 4 {
            0 => SrcF2::WmMove,
            1 => SrcF2::WmResize,
            2 => SrcF2::WmStack,
            _ => SrcF2::WmIconify,
        };
        let ok = ing.ingest(F2Event { src, code: 0xf200 + i, gx: 10, gy: 20 })?;
        if ok {
            return Err(E_WM_LEAK);
        }
        if ing.state_cur != ing.state_seed {
            return Err(E_STATE_DIRTY);
        }
        i += 1;
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：F2 窗管边界复签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 F2 判定表：四类窗管语义识别，两类本域源不误判。
    if !SrcF2::WmMove.is_wm()
        || !SrcF2::WmResize.is_wm()
        || !SrcF2::WmStack.is_wm()
        || !SrcF2::WmIconify.is_wm()
    {
        return Err(E_CLS_BAD);
    }
    if SrcF2::ImeInput.is_wm() || SrcF2::ImeState.is_wm() {
        return Err(E_CLS_BAD);
    }
    hits += 1;

    // R2 越界注入 ×10：全拒零波及。
    let mut ing = F2Ingest::new("f2-boundary");
    inject_wm_x10(&mut ing)?;
    if ing.rejected.len() != 10 || ing.accepted.len() != 0 {
        return Err(E_WM_LEAK);
    }
    hits += 1;

    // R3 本域受理：IME 输入/状态事件照常受理。
    let ok = ing.ingest(F2Event { src: SrcF2::ImeInput, code: 1, gx: 0, gy: 0 })?;
    if !ok {
        return Err(E_IME_MISS);
    }
    let ok = ing.ingest(F2Event { src: SrcF2::ImeState, code: 2, gx: 0, gy: 0 })?;
    if !ok {
        return Err(E_IME_MISS);
    }
    if ing.state_cur == ing.state_seed {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R4 账守恒：受理 2 + 拒收 10。
    ing.audit(2, 10)?;
    hits += 1;

    // R5 几何域守卫：几何参数超域须拒（窗管面也不得携带脏几何）。
    let mut ing2 = F2Ingest::new("geo");
    let r = ing2.ingest(F2Event {
        src: SrcF2::WmMove,
        code: 9,
        gx: 0x9000,
        gy: 0,
    });
    if r.err() != Some(E_GEO_BAD) {
        return Err(E_GEO_BAD);
    }
    hits += 1;

    // R6 纯拒收零污染：只拒收后状态基准不变。
    let mut ing3 = F2Ingest::new("pure");
    ing3.ingest(F2Event { src: SrcF2::WmStack, code: 3, gx: 1, gy: 1 })?;
    ing3.ingest(F2Event { src: SrcF2::WmIconify, code: 4, gx: 2, gy: 2 })?;
    if ing3.state_cur != ing3.state_seed || ing3.accepted.len() != 0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R7 fuzz ×512 混合流：分类零错、F2 零回流、本域零误拒。
    let mut rng = Lcg::new(0x48626_0001);
    let mut ing4 = F2Ingest::new("fuzz");
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let d = rng.below(6);
        let src = match d {
            0 => SrcF2::ImeInput,
            1 => SrcF2::ImeState,
            2 => SrcF2::WmMove,
            3 => SrcF2::WmResize,
            4 => SrcF2::WmStack,
            _ => SrcF2::WmIconify,
        };
        let code = rng.below(0x1_0000) as u32;
        let ok = ing4.ingest(F2Event { src, code, gx: 0, gy: 0 })?;
        if src.is_wm() {
            if ok {
                return Err(E_WM_LEAK);
            }
            rej += 1;
        } else {
            if !ok {
                return Err(E_IME_MISS);
            }
            acc += 1;
        }
        k += 1;
    }
    ing4.audit(acc, rej)?;
    if acc == 0 || rej == 0 {
        return Err(E_CLS_BAD);
    }
    hits += 1;

    // R8 受理/拒收互斥：事件码不跨账。
    let mut i = 0usize;
    while i < ing4.accepted.len() {
        let mut j = 0usize;
        while j < ing4.rejected.len() {
            if ing4.accepted[i] == ing4.rejected[j] {
                return Err(E_CLS_BAD);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R9 状态推进复算：状态值与受理序列复算一致。
    let mut rng3 = Lcg::new(0x48626_0002);
    let mut st: u64 = 0x0f2e_0000_dead_beef;
    let mut i = 0u32;
    while i < acc {
        let code = rng3.below(0x1_0000) as u32;
        st = st.wrapping_mul(0x0000_0100_0000_01b3) ^ (code as u64);
        i += 1;
    }
    if st != ing4.state_cur {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R10 重演一致：同种子重建混合流指纹相等。
    let fp1 = ing4.fingerprint();
    let mut rng4 = Lcg::new(0x48626_0001);
    let mut ing5 = F2Ingest::new("fuzz");
    let mut k = 0u32;
    while k < 512 {
        let d = rng4.below(6);
        let src = match d {
            0 => SrcF2::ImeInput,
            1 => SrcF2::ImeState,
            2 => SrcF2::WmMove,
            3 => SrcF2::WmResize,
            4 => SrcF2::WmStack,
            _ => SrcF2::WmIconify,
        };
        let code = rng4.below(0x1_0000) as u32;
        let _ = ing5.ingest(F2Event { src, code, gx: 0, gy: 0 });
        k += 1;
    }
    if ing5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
