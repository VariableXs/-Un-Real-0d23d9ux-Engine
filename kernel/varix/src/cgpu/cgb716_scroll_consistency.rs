//! CGPU-F0716 · 滚动一致性（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0716，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动一致性——对双向滚动位置归约器做性质级一致性验证：
//!   往返不变（下滚 N 再上滚 N 还原）、幂等（重复查询无副作用）、
//!   双端钳制（位置永远落在 [0,H]）与同流双实现位置一致。
//!
//! 设计纪律：
//! - 双实现同流对拍：规约实现（逐行步进）与快速实现（整跳钳制）
//!   对同一命令流终态位置必须相等（一致性恒等式）。
//! - 性质三闭集：RoundTrip / Idempotent / ClampBound，
//!   每性质一轮判定，结果三态（Pass/Fail/Skip）。
//! - 命令流确定性：由固定常量表驱动（非随机），逐条注释来源。
//! - 位置账：每条流验证后登记（流长，终态），终态和守恒只对
//!   钳制恒等式负责：0 ≤ pos ≤ H。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x47c0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x47c0，批内 CGPU-F0716 独占）。
pub const SEG_BASE: u16 = 0x47c0;

/// 高度非法（0 或超上限）。
pub const E_HEIGHT: u16 = SEG_BASE + 1;
/// 双实现终态不一致。
pub const E_MISMATCH: u16 = SEG_BASE + 2;
/// 位置越界（钳制恒等式被击穿）。
pub const E_CLAMP: u16 = SEG_BASE + 3;
/// 往返性质失败。
pub const E_ROUNDTRIP: u16 = SEG_BASE + 4;
/// 幂等性质失败。
pub const E_IDEMPOTENT: u16 = SEG_BASE + 5;
/// 命令流过长。
pub const E_STREAM: u16 = SEG_BASE + 6;

/// 内容高度上限（行）。
pub const H_MAX: u32 = 1 << 20;
/// 命令流上限。
pub const STREAM_MAX: usize = 512;

/// 双向滚动命令：正=向下滚，负=向上滚（i32 语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BiCmd {
    /// 有符号位移（行）。
    pub d: i32,
}

/// 规约实现：逐行步进，每行检查边界（慢而直白）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefScroller {
    pos: i64,
    h: i64,
}

impl RefScroller {
    pub fn new(h: u32) -> Result<Self, u16> {
        if h == 0 || h > H_MAX {
            return Err(E_HEIGHT);
        }
        Ok(Self { pos: 0, h: h as i64 })
    }
    /// 应用一条命令，返回新位置。
    pub fn apply(&mut self, c: BiCmd) -> i64 {
        let step = c.d as i64;
        if step >= 0 {
            for _ in 0..step {
                if self.pos < self.h {
                    self.pos += 1;
                }
            }
        } else {
            for _ in 0..(-step) {
                if self.pos > 0 {
                    self.pos -= 1;
                }
            }
        }
        self.pos
    }
    pub fn pos(&self) -> i64 {
        self.pos
    }
}

/// 快速实现：整跳 saturating 钳制（语义等价的另一写法）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastScroller {
    pos: i64,
    h: i64,
}

impl FastScroller {
    pub fn new(h: u32) -> Result<Self, u16> {
        if h == 0 || h > H_MAX {
            return Err(E_HEIGHT);
        }
        Ok(Self { pos: 0, h: h as i64 })
    }
    pub fn apply(&mut self, c: BiCmd) -> i64 {
        let target = self.pos + c.d as i64;
        if target < 0 {
            self.pos = 0;
        } else if target > self.h {
            self.pos = self.h;
        } else {
            self.pos = target;
        }
        self.pos
    }
    pub fn pos(&self) -> i64 {
        self.pos
    }
}

/// 同流双实现对拍：终态必须一致且落在 [0,H]。
pub fn consistency_run(h: u32, stream: &[BiCmd]) -> Result<i64, u16> {
    if stream.len() > STREAM_MAX {
        return Err(E_STREAM);
    }
    let mut r = RefScroller::new(h)?;
    let mut f = FastScroller::new(h)?;
    for c in stream {
        r.apply(*c);
        f.apply(*c);
    }
    if r.pos() != f.pos() {
        return Err(E_MISMATCH);
    }
    let p = r.pos();
    if p < 0 || p > h as i64 {
        return Err(E_CLAMP);
    }
    Ok(p)
}

/// 性质三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 失败（错误码随行）。
    Fail,
    /// 跳过（前置条件不满足，如空流）。
    Skip,
}

/// 性质判定行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropRow {
    /// 性质名。
    pub prop: &'static str,
    /// 判定。
    pub v: Verdict,
    /// 伴随值（终态或失败码）。
    pub aux: u64,
}

/// 往返性质：+N 后 -N 回原位（N 为流首条绝对值）。
pub fn prop_roundtrip(h: u32, n: i32) -> Result<PropRow, u16> {
    if n == 0 {
        return Ok(PropRow { prop: "roundtrip", v: Verdict::Skip, aux: 0 });
    }
    let mut r = RefScroller::new(h)?;
    let p1 = r.apply(BiCmd { d: n });
    let p2 = r.apply(BiCmd { d: -n });
    if p2 != 0 {
        return Ok(PropRow { prop: "roundtrip", v: Verdict::Fail, aux: E_ROUNDTRIP as u64 });
    }
    let _ = p1;
    Ok(PropRow { prop: "roundtrip", v: Verdict::Pass, aux: 0 })
}

/// 幂等性质：连续 3 次查询命令（d=0）位置不变。
pub fn prop_idempotent(h: u32) -> Result<PropRow, u16> {
    let mut r = FastScroller::new(h)?;
    r.apply(BiCmd { d: 7 });
    let p0 = r.pos();
    for _ in 0..3 {
        let p = r.apply(BiCmd { d: 0 });
        if p != p0 {
            return Ok(PropRow { prop: "idempotent", v: Verdict::Fail, aux: E_IDEMPOTENT as u64 });
        }
    }
    Ok(PropRow { prop: "idempotent", v: Verdict::Pass, aux: p0 as u64 })
}

/// 钳制性质：极端流（超量下滚/上滚）后位置仍在 [0,H]。
pub fn prop_clamp(h: u32) -> Result<PropRow, u16> {
    let stream = [
        BiCmd { d: i32::MAX },
        BiCmd { d: i32::MIN },
        BiCmd { d: 1 << 20 },
        BiCmd { d: -(1 << 20) },
        BiCmd { d: 3 },
    ];
    let p = consistency_run(h, &stream)?;
    if p < 0 || p > h as i64 {
        return Ok(PropRow { prop: "clamp", v: Verdict::Fail, aux: E_CLAMP as u64 });
    }
    Ok(PropRow { prop: "clamp", v: Verdict::Pass, aux: p as u64 })
}

/// 一致性会话：高度 + 已验证流账本。
#[derive(Clone, Debug)]
pub struct ConsistencySession {
    h: u32,
    /// 已验证流（流长，终态）。
    ledger: Vec<(usize, i64)>,
    /// 三性质最近判定。
    props: [PropRow; 3],
}

impl ConsistencySession {
    pub fn new(h: u32) -> Result<Self, u16> {
        if h == 0 || h > H_MAX {
            return Err(E_HEIGHT);
        }
        Ok(Self {
            h,
            ledger: Vec::new(),
            props: [
                PropRow { prop: "roundtrip", v: Verdict::Skip, aux: 0 },
                PropRow { prop: "idempotent", v: Verdict::Skip, aux: 0 },
                PropRow { prop: "clamp", v: Verdict::Skip, aux: 0 },
            ],
        })
    }
    /// 验证一条流并登记。
    pub fn verify_stream(&mut self, stream: &[BiCmd]) -> Result<i64, u16> {
        let p = consistency_run(self.h, stream)?;
        self.ledger.push((stream.len(), p));
        Ok(p)
    }
    /// 跑全部三性质。
    pub fn run_props(&mut self, n: i32) -> Result<(), u16> {
        self.props[0] = prop_roundtrip(self.h, n)?;
        self.props[1] = prop_idempotent(self.h)?;
        self.props[2] = prop_clamp(self.h)?;
        Ok(())
    }
    /// 性质全过判定。
    pub fn all_green(&self) -> bool {
        self.props.iter().all(|p| p.v == Verdict::Pass)
    }
    /// 账本长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    /// 高度。
    pub fn height(&self) -> u32 {
        self.h
    }
    /// 性质只读视图。
    pub fn props(&self) -> &[PropRow; 3] {
        &self.props
    }
}

/// 固定常量命令流（来源：滚动组一致性规格样例表，逐条注释）。
pub const SAMPLE_STREAM: [BiCmd; 8] = [
    BiCmd { d: 100 },   // 样例1：下滚 100 行
    BiCmd { d: -40 },   // 样例2：上滚 40 行（净 +60）
    BiCmd { d: 0 },     // 样例3：查询空转
    BiCmd { d: 5000 },  // 样例4：超界下滚（触发钳制）
    BiCmd { d: -9999 }, // 样例5：超界上滚（回顶）
    BiCmd { d: 1 },     // 样例6：单步
    BiCmd { d: -1 },    // 样例7：回退单步
    BiCmd { d: 256 },   // 样例8：常规页滚
];

/// 自例程：主路径（同流对拍 + 三性质 + 会话账）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：样例流在 h=1024 上对拍。
    let p = consistency_run(1024, &SAMPLE_STREAM)?;
    // 手算：0+100=100；-40→60；0→60；+5000→钳到 1024；-9999→0；+1→1；-1→0；+256→256。
    if p != 256 {
        return Err(E_MISMATCH);
    }
    // 主路径 2：三性质。
    let mut s = ConsistencySession::new(1024)?;
    s.run_props(300)?;
    if !s.all_green() {
        return Err(E_ROUNDTRIP);
    }
    let props = s.props();
    if props[1].aux != 7 {
        return Err(E_IDEMPOTENT); // 幂等性质 aux=查询终态 7
    }
    // 主路径 3：会话流登记。
    s.verify_stream(&SAMPLE_STREAM)?;
    if s.ledger_len() != 1 {
        return Err(E_STREAM);
    }
    // 错误路径 1：高度 0。
    if RefScroller::new(0) != Err(E_HEIGHT) || FastScroller::new(0) != Err(E_HEIGHT) {
        return Err(E_HEIGHT);
    }
    // 错误路径 2：高度超上限。
    if ConsistencySession::new(H_MAX + 1).err() != Some(E_HEIGHT) {
        return Err(E_HEIGHT);
    }
    // 错误路径 3：人为制造不一致——用两个不同高度的实现对拍：
    // 直接调用底层：Ref 在 h=100 上到顶 100，Fast 在 h=50 上到顶 50。
    let mut r1 = RefScroller::new(100)?;
    let mut f1 = FastScroller::new(50)?;
    let c = BiCmd { d: 80 };
    r1.apply(c);
    f1.apply(c);
    if r1.pos() == f1.pos() {
        return Err(E_MISMATCH); // 预期不同（100 vs 50），若相同说明实现坏了
    }
    // 错误路径 4：流过长。
    let long: Vec<BiCmd> = (0..STREAM_MAX + 1).map(|_| BiCmd { d: 1 }).collect();
    if consistency_run(1024, &long) != Err(E_STREAM) {
        return Err(E_STREAM);
    }
    // 错误路径 5：钳制恒等式在合法实现上永不击穿（负验证：位置不可能 <0 或 >h）。
    let extreme = [BiCmd { d: i32::MIN }, BiCmd { d: i32::MAX }];
    let pe = consistency_run(64, &extreme)?;
    if pe != 0 && pe != 64 {
        return Err(E_CLAMP);
    }
    Ok(0)
}
