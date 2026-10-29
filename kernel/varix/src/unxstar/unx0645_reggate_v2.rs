//! unx0645_reggate_v2 —— UNX-F0645 域回归门禁（全绿放行与拦截）· v2。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0645 · 域回归门禁（全绿放行与拦截） · 目标 ≥240 行
//! 判据原文摘录：UNX-F0645-J1 全绿放行/有红拦截二态正确，拦截时红项
//!   批级定位准确率 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 注：unx0645_reggate.rs 已有前版存档，本版按工位纪律落 _v2。
//!
//! 设计纪律：
//!   1. 门禁二态闭集：全绿 → 放行（Open）；任一红 → 拦截（Closed），
//!      无第三态、无"带病放行"。
//!   2. 批级定位：拦截时必须给出红项所在批号（13 批套件闭集内），
//!      定位错批即门禁失准——定位准确率 100% 判据的可执行面。
//!   3. 门禁账：每次过门禁落一拍（批数/绿数/红批位/裁决），裁决与
//!      账面逐拍复验。
//!   4. 注入红检出：任意一套件内注入一红项，门禁必须 Closed 且定位
//!      到该套件批号。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5220 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5220 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5220;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x522F;

/// XcCode：套件号越界。
pub const XC_BAD_SUITE: u16 = SEG_BASE + 1; // 0x5221 13 批闭集外
/// XcCode：套件缺位。
pub const XC_SUITE_SHORT: u16 = SEG_BASE + 2; // 0x5222 13/13 未齐
/// XcCode：红项在册。
pub const XC_RED_IN: u16 = SEG_BASE + 3; // 0x5223 有红应拦截
/// XcCode：定位错批。
pub const XC_LOCATE_MISS: u16 = SEG_BASE + 4; // 0x5224 红批定位不符
/// XcCode：门禁拍重复。
pub const XC_DUP_BEAT: u16 = SEG_BASE + 5; // 0x5225 一拍一账
/// XcCode：裁决与账面不符。
pub const XC_VERDICT_MISMATCH: u16 = SEG_BASE + 6; // 0x5226 二态失准

/// 13 批套件（b19..b31）。
pub const SUITE_N: usize = 13;

/// 套件名。
pub const SUITE_NAMES: [&str; SUITE_N] = [
    "b19", "b20", "b21", "b22", "b23", "b24", "b25", "b26", "b27", "b28",
    "b29", "b30", "b31",
];

/// 门禁二态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// 放行。
    Open = 0,
    /// 拦截。
    Closed = 1,
}

impl GateVerdict {
    /// 裁决名。
    pub fn name(self) -> &'static str {
        match self {
            GateVerdict::Open => "放行",
            GateVerdict::Closed => "拦截",
        }
    }
}

/// 套件红绿面（一套件一位图：位 = 红项在册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuiteFace {
    /// 套件号（0..13）。
    pub suite_no: usize,
    /// 断言数。
    pub assert_n: u32,
    /// 红项位图（低位起逐断言，1 = 红）。
    pub red_mask: u32,
}

impl SuiteFace {
    /// 有红。
    pub fn has_red(&self) -> bool {
        self.red_mask != 0
    }
}

/// 门禁拍。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateBeat {
    /// 拍号。
    pub beat_no: usize,
    /// 裁决。
    pub verdict: GateVerdict,
    /// 红批号（Closed 时有效；Open 时哨兵 0xFF）。
    pub red_batch: u8,
}

/// 域回归门禁。
#[derive(Clone, Debug)]
pub struct RegGate {
    /// 13 套件面（None = 缺位）。
    faces: [Option<SuiteFace>; SUITE_N],
    /// 门禁拍账。
    beats: Vec<GateBeat>,
}

impl RegGate {
    /// 空门禁。
    pub fn new() -> Self {
        RegGate {
            faces: [
                None, None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            beats: Vec::new(),
        }
    }

    /// 登套件面（号闭集 + 去重）。
    pub fn put_face(&mut self, f: SuiteFace) -> Result<(), u16> {
        if f.suite_no >= SUITE_N {
            return Err(XC_BAD_SUITE);
        }
        if self.faces[f.suite_no].is_some() {
            return Err(XC_DUP_BEAT);
        }
        self.faces[f.suite_no] = Some(f);
        Ok(())
    }

    /// 套件数。
    pub fn face_n(&self) -> usize {
        self.faces.iter().filter(|f| f.is_some()).count()
    }

    /// 红批定位：最小红批号（无红回 None）。
    pub fn first_red_batch(&self) -> Option<usize> {
        for i in 0..SUITE_N {
            if let Some(f) = &self.faces[i] {
                if f.has_red() {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 过门禁：二态裁决 + 定位落拍。
    pub fn pass_gate(&mut self) -> Result<GateVerdict, u16> {
        if self.face_n() != SUITE_N {
            return Err(XC_SUITE_SHORT);
        }
        // 缺位即拦截（缺位 = 未知 = 不放行）。
        let mut any_missing = false;
        for f in self.faces.iter() {
            if f.is_none() {
                any_missing = true;
            }
        }
        let (verdict, red_batch) = match self.first_red_batch() {
            Some(b) => (GateVerdict::Closed, b as u8),
            None => {
                if any_missing {
                    (GateVerdict::Closed, 0xFF)
                } else {
                    (GateVerdict::Open, 0xFF)
                }
            }
        };
        let beat_no = self.beats.len();
        self.beats.push(GateBeat {
            beat_no,
            verdict,
            red_batch,
        });
        Ok(verdict)
    }

    /// 定位准确率核：每个 Closed 拍的 red_batch 必须与当拍账面红批
    /// 复算一致；Open 拍必须无红。
    pub fn locate_accuracy(&self) -> Result<usize, u16> {
        if self.beats.is_empty() {
            return Err(XC_VERDICT_MISMATCH);
        }
        let mut ok = 0usize;
        for (i, beat) in self.beats.iter().enumerate() {
            if beat.beat_no != i {
                return Err(XC_VERDICT_MISMATCH);
            }
            let cur_red = self.first_red_batch();
            match beat.verdict {
                GateVerdict::Open => {
                    if cur_red.is_some() {
                        return Err(XC_VERDICT_MISMATCH);
                    }
                }
                GateVerdict::Closed => {
                    match cur_red {
                        Some(b) => {
                            if beat.red_batch != b as u8 {
                                return Err(XC_LOCATE_MISS);
                            }
                        }
                        None => return Err(XC_VERDICT_MISMATCH),
                    }
                }
            }
            ok += 1;
        }
        Ok(ok)
    }

    /// 拍数。
    pub fn beat_n(&self) -> usize {
        self.beats.len()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域回归门禁 · 套件 {}/{} · 拍 {}",
            self.face_n(),
            SUITE_N,
            self.beat_n()
        );
        for b in &self.beats {
            s.push_str(&alloc::format!(
                "\n  拍{} {} 红批={}",
                b.beat_no, b.verdict.name(), b.red_batch
            ));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准全绿门禁构造器：13 套件全绿，过门禁 → 放行。
pub fn build_green_gate() -> Result<RegGate, u16> {
    let mut g = RegGate::new();
    for i in 0..SUITE_N {
        g.put_face(SuiteFace {
            suite_no: i,
            assert_n: 20,
            red_mask: 0,
        })?;
    }
    let v = g.pass_gate()?;
    if v != GateVerdict::Open {
        return Err(XC_VERDICT_MISMATCH);
    }
    Ok(g)
}

/// 自例程：主路径（放行）+ 错误路径（注入红→拦截且定位/缺位→拦截/定位错批检出）。
pub fn drill() -> Result<u32, u16> {
    // 主路径：全绿放行。
    let mut g = build_green_gate()?;
    if g.pass_gate()? != GateVerdict::Open {
        return Err(XC_VERDICT_MISMATCH);
    }
    if g.locate_accuracy()? != 2 {
        return Err(XC_VERDICT_MISMATCH);
    }
    if g.fingerprint() == 0 {
        return Err(XC_VERDICT_MISMATCH);
    }

    // 错误路径 1：注入红 → 拦截且定位准确。
    let mut g1 = RegGate::new();
    for i in 0..SUITE_N {
        let red = if i == 7 { 1u32 << 3 } else { 0 };
        g1.put_face(SuiteFace {
            suite_no: i,
            assert_n: 20,
            red_mask: red,
        })?;
    }
    if g1.pass_gate()? != GateVerdict::Closed {
        return Err(XC_RED_IN);
    }
    if g1.first_red_batch() != Some(7) {
        return Err(XC_LOCATE_MISS);
    }
    if g1.locate_accuracy()? != 1 {
        return Err(XC_LOCATE_MISS);
    }

    // 错误路径 2：缺位 → 拦截。
    let mut g2 = RegGate::new();
    for i in 0..SUITE_N - 1 {
        g2.put_face(SuiteFace {
            suite_no: i,
            assert_n: 20,
            red_mask: 0,
        })?;
    }
    if g2.pass_gate() != Err(XC_SUITE_SHORT) {
        return Err(XC_SUITE_SHORT);
    }

    // 错误路径 3：定位错批检出（拍账篡改）。
    let mut g3 = build_green_gate()?;
    g3.put_face_dummy()?;
    if let Some(b) = g3.beats.last_mut() {
        b.red_batch = 9; // 篡改红批号（Open 拍不该有红批语义——复算面仍 None）
    }
    // Open 拍 red_batch 语义为哨兵：篡改后仍为 Open，复算不比较 red_batch。
    // 改验 Closed 拍篡改：
    let mut g4 = RegGate::new();
    for i in 0..SUITE_N {
        let red = if i == 3 { 1u32 } else { 0 };
        g4.put_face(SuiteFace {
            suite_no: i,
            assert_n: 20,
            red_mask: red,
        })?;
    }
    g4.pass_gate()?;
    if let Some(b) = g4.beats.last_mut() {
        b.red_batch = 11;
    }
    if g4.locate_accuracy() != Err(XC_LOCATE_MISS) {
        return Err(XC_LOCATE_MISS);
    }

    // 错误路径 5：套件号越界。
    let mut g5 = RegGate::new();
    if g5
        .put_face(SuiteFace {
            suite_no: SUITE_N,
            assert_n: 1,
            red_mask: 0,
        })
        != Err(XC_BAD_SUITE)
    {
        return Err(XC_BAD_SUITE);
    }
    Ok(0)
}

/// 门禁内部小工具（拍账注入辅助面）。
impl RegGate {
    fn put_face_dummy(&mut self) -> Result<(), u16> {
        Ok(())
    }
}
