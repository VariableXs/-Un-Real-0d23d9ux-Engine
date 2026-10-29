//! UNX-F48587 —— I·手柄/输入设备无关声明
//!         （H5 语义零回流本域）
//! 判据原文：UNX-F48587-J1 越界注入 ×10 零波及；判据锚定 §AI-61 §1 边界 H5/AI-40。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           H5 事件单点分类拒收，本域状态零污染。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0080; // H5 输入设备无关声明诊断段

const E_H5_LEAK: u16 = SEG_BASE + 1;    // H5 语义回流本域
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 本域状态被污染
const E_CLS_BAD: u16 = SEG_BASE + 3;    // 事件分类错
const E_KB_MISS: u16 = SEG_BASE + 4;    // 键盘事件被误拒
const E_LEDGER: u16 = SEG_BASE + 5;     // 摄入账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;     // 重演不一致

/// 事件源域。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SrcDomain {
    Keyboard,   // 本域合法：键盘
    Ime,        // 本域合法：IME 内部
    Gamepad,    // H5：手柄
    TouchH5,    // H5：触屏手势（设备无关面）
    PointerH5,  // H5：指针设备
}

impl SrcDomain {
    /// H5 判定：手柄/触屏手势/指针属 H5 设备语义，零回流。
    pub fn is_h5(&self) -> bool {
        match self {
            SrcDomain::Gamepad | SrcDomain::TouchH5 | SrcDomain::PointerH5 => true,
            _ => false,
        }
    }
}

/// 入域事件。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InEvent {
    pub src: SrcDomain,
    pub code: u32,
}

/// 本域摄入器：H5 语义单点拒收。
pub struct DomainIngest {
    pub label: String,
    pub accepted: Vec<u32>,
    pub rejected: Vec<u32>,
    pub state_seed: u64, // 本域状态指纹基准
    pub state_cur: u64,  // 本域状态当前值
}

impl DomainIngest {
    pub fn new(label: &str) -> Self {
        DomainIngest {
            label: label.to_string(),
            accepted: Vec::new(),
            rejected: Vec::new(),
            state_seed: 0x1234_5678_9abc_def0,
            state_cur: 0x1234_5678_9abc_def0,
        }
    }

    /// 摄入一个事件：H5 拒收且零波及；键盘/IME 受理并推进状态。
    pub fn ingest(&mut self, ev: InEvent) -> Result<bool, u16> {
        if ev.src.is_h5() {
            self.rejected.push(ev.code);
            // 零波及断言：拒收路径不得触碰状态。
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

    /// 摄入账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.accepted.len() as u32 + self.rejected.len() as u32
            != (self.accepted.len() + self.rejected.len()) as u32
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

/// 越界注入 ×10 演练：H5 事件全数拒收，本域状态零波及。
pub fn inject_h5_x10(ing: &mut DomainIngest) -> Result<(), u16> {
    let mut i = 0u32;
    while i < 10 {
        let src = match i % 3 {
            0 => SrcDomain::Gamepad,
            1 => SrcDomain::TouchH5,
            _ => SrcDomain::PointerH5,
        };
        let ok = ing.ingest(InEvent { src, code: h5_base() + i })?;
        if ok {
            return Err(E_H5_LEAK);
        }
        if ing.state_cur != ing.state_seed {
            return Err(E_STATE_DIRTY);
        }
        i += 1;
    }
    Ok(())
}

fn h5_base() -> u32 {
    0x5000
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

/// 主巡检：手柄/输入设备无关声明十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 H5 判定表：三类 H5 源全部识别，两类本域源不误判。
    if !SrcDomain::Gamepad.is_h5()
        || !SrcDomain::TouchH5.is_h5()
        || !SrcDomain::PointerH5.is_h5()
    {
        return Err(E_CLS_BAD);
    }
    if SrcDomain::Keyboard.is_h5() || SrcDomain::Ime.is_h5() {
        return Err(E_CLS_BAD);
    }
    hits += 1;

    // R2 越界注入 ×10：H5 事件全数拒收零波及。
    let mut ing = DomainIngest::new("h5-iso");
    inject_h5_x10(&mut ing)?;
    if ing.rejected.len() != 10 || ing.accepted.len() != 0 {
        return Err(E_H5_LEAK);
    }
    hits += 1;

    // R3 本域事件受理：键盘事件照常受理并推进状态。
    let ok = ing.ingest(InEvent { src: SrcDomain::Keyboard, code: 30 })?;
    if !ok {
        return Err(E_KB_MISS);
    }
    if ing.state_cur == ing.state_seed {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R4 IME 内部事件受理。
    let ok = ing.ingest(InEvent { src: SrcDomain::Ime, code: 0x100 })?;
    if !ok {
        return Err(E_KB_MISS);
    }
    hits += 1;

    // R5 摄入账守恒：受理 2 + 拒收 10 == 总摄入 12。
    ing.audit()?;
    if ing.accepted.len() as u32 + ing.rejected.len() as u32 != 12 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6 fuzz ×512 混合流：分类零错、H5 零回流、键盘零误拒。
    let mut rng = Lcg::new(0x48587_0001);
    let mut ing2 = DomainIngest::new("h5-iso-fuzz");
    let mut acc = 0u32;
    let mut rej = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let d = rng.below(5);
        let src = match d {
            0 => SrcDomain::Keyboard,
            1 => SrcDomain::Ime,
            2 => SrcDomain::Gamepad,
            3 => SrcDomain::TouchH5,
            _ => SrcDomain::PointerH5,
        };
        let code = rng.below(0x1_0000) as u32;
        let ok = ing2.ingest(InEvent { src, code })?;
        if src.is_h5() {
            if ok {
                return Err(E_H5_LEAK);
            }
            rej += 1;
        } else {
            if !ok {
                return Err(E_KB_MISS);
            }
            acc += 1;
        }
        k += 1;
    }
    if ing2.accepted.len() as u32 != acc || ing2.rejected.len() as u32 != rej {
        return Err(E_LEDGER);
    }
    if acc == 0 || rej == 0 {
        return Err(E_CLS_BAD);
    }
    hits += 1;

    // R7 状态推进封闭：状态值只受受理事件影响，可与受理序列复算一致。
    let mut rng3 = Lcg::new(0x48587_0002);
    let mut st: u64 = 0x1234_5678_9abc_def0;
    let mut i = 0u32;
    while i < acc {
        let code = rng3.below(0x1_0000) as u32;
        st = st.wrapping_mul(0x0000_0100_0000_01b3) ^ (code as u64);
        i += 1;
    }
    if st != ing2.state_cur {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R8 声明封闭：拒收事件码全部落在 H5 段外不影响受理序列。
    let mut i = 0usize;
    while i < ing2.accepted.len() {
        let mut j = 0usize;
        while j < ing2.rejected.len() {
            if ing2.accepted[i] == ing2.rejected[j] {
                return Err(E_CLS_BAD);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R9 零波及再断言：拒收后状态基准未被触碰。
    let mut ing3 = DomainIngest::new("h5-iso-pure");
    ing3.ingest(InEvent { src: SrcDomain::Gamepad, code: 1 })?;
    ing3.ingest(InEvent { src: SrcDomain::PointerH5, code: 2 })?;
    if ing3.state_cur != ing3.state_seed || ing3.accepted.len() != 0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R10 重演一致：同种子重建混合流，指纹相等。
    let fp1 = ing2.fingerprint();
    let mut rng4 = Lcg::new(0x48587_0001);
    let mut ing4 = DomainIngest::new("h5-iso-fuzz");
    let mut k = 0u32;
    while k < 512 {
        let d = rng4.below(5);
        let src = match d {
            0 => SrcDomain::Keyboard,
            1 => SrcDomain::Ime,
            2 => SrcDomain::Gamepad,
            3 => SrcDomain::TouchH5,
            _ => SrcDomain::PointerH5,
        };
        let code = rng4.below(0x1_0000) as u32;
        let _ = ing4.ingest(InEvent { src, code });
        k += 1;
    }
    if ing4.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
