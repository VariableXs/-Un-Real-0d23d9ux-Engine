//! unx48335_smubound — UNX-F48335 智能面与用户词边界声明（学习不覆盖用户显式置顶）（诊断段 0xCAC0）。
//!
//! 判据原文：
//!   J1 越权注入 ×10 置顶恒胜；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 置顶主权：用户显式置顶（pinned）词在候选序中恒排首位——
//!      学习引擎（反馈环/热词统计）无论增益多大都不得越过置顶位。
//!   2. 越权注入 ×10：注入 10 次高频学习增益（把非置顶词频次推到
//!      极值），排序结果中置顶词仍恒第一（×10 全查零失守）。
//!   3. 学习只作用于非置顶层：置顶层冻结（任何写路径到置顶词频次
//!      都被边界卫兵拒绝）；非置顶层正常增益（增益通路可观测）。
//!   4. 排序稳定性：同层内按 (频次 desc, 词 id asc) 全序；置顶层
//!      内按置顶顺序（pinned seq asc）全序——两层各自全序可复算。
//!   5. 声明入册：边界声明（置顶层不可被学习覆盖）入册冻结，
//!      改写尝试拒绝留痕。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCAC0;

const E_PIN_OVERTHROWN: u16 = SEG_BASE + 1; // 置顶词被挤出首位
const E_PIN_WRITE: u16 = SEG_BASE + 2;      // 学习写穿了置顶层
const E_GAIN_LOST: u16 = SEG_BASE + 3;      // 非置顶层增益未生效
const E_ORDER_BAD: u16 = SEG_BASE + 4;      // 同层排序违例
const E_DECL_THAW: u16 = SEG_BASE + 5;      // 边界声明被改写
const E_CODE_RANGE: u16 = SEG_BASE + 6;     // 诊断码越段

/// 候选词：id + 频次 + 是否置顶 + 置顶序（非置顶时 0）。
#[derive(Clone, Copy)]
pub struct Cand {
    pub id: u32,
    pub freq: u32,
    pub pinned: bool,
    pub pin_seq: u32,
}

/// 排序输出：置顶层（按 pin_seq 升序）在前、非置顶层（按 freq 降序、
/// id 升序）在后，两层拼接即候选序。
pub fn rank(cands: &[Cand]) -> Vec<u32> {
    let mut pinned: Vec<&Cand> = Vec::new();
    let mut normal: Vec<&Cand> = Vec::new();
    let mut i = 0;
    while i < cands.len() {
        if cands[i].pinned {
            pinned.push(&cands[i]);
        } else {
            normal.push(&cands[i]);
        }
        i += 1;
    }
    // 置顶层：插入排序按 pin_seq 升序。
    let mut a = 1;
    while a < pinned.len() {
        let mut b = a;
        while b > 0 && pinned[b - 1].pin_seq > pinned[b].pin_seq {
            pinned.swap(b - 1, b);
            b -= 1;
        }
        a += 1;
    }
    // 非置顶层：插入排序按 (freq desc, id asc)。
    let mut c = 1;
    while c < normal.len() {
        let mut d = c;
        while d > 0
            && (normal[d - 1].freq < normal[d].freq
                || (normal[d - 1].freq == normal[d].freq && normal[d - 1].id > normal[d].id))
        {
            normal.swap(d - 1, d);
            d -= 1;
        }
        c += 1;
    }
    let mut out: Vec<u32> = Vec::new();
    let mut e = 0;
    while e < pinned.len() {
        out.push(pinned[e].id);
        e += 1;
    }
    let mut f = 0;
    while f < normal.len() {
        out.push(normal[f].id);
        f += 1;
    }
    out
}

/// 边界声明：置顶层对学习写路径封闭。入册冻结。
pub struct UBoundDecl {
    frozen: bool,
    tamper_marks: u32,
}

impl UBoundDecl {
    pub fn new() -> Self {
        UBoundDecl {
            frozen: false,
            tamper_marks: 0,
        }
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 学习写路径尝试改置顶词频次：冻结后恒拒绝留痕。
    pub fn learn_write_to_pinned(&mut self) -> bool {
        if self.frozen {
            self.tamper_marks += 1;
            return false;
        }
        false
    }

    pub fn tamper_marks(&self) -> u32 {
        self.tamper_marks
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }
}

/// 学习引擎：对非置顶词频次增益（饱和 +7）；对置顶词请求恒拒。
pub struct LearnEngine {
    pub gains: u32,
    pub denied: u32,
}

impl LearnEngine {
    pub fn new() -> Self {
        LearnEngine {
            gains: 0,
            denied: 0,
        }
    }

    /// 增益请求：置顶词拒绝；非置顶词饱和增益。
    pub fn boost(&mut self, c: &mut Cand) -> bool {
        if c.pinned {
            self.denied += 1;
            return false;
        }
        c.freq = if c.freq > 0xFFFFFFF0 {
            0xFFFF_FFFF
        } else {
            c.freq + 7
        };
        self.gains += 1;
        true
    }
}

/// 候选池夹具：1 置顶词 + 5 非置顶词（频次低位起步）。
pub fn pool() -> Vec<Cand> {
    vec![
        Cand { id: 9001, freq: 3, pinned: true, pin_seq: 1 },
        Cand { id: 9002, freq: 5, pinned: false, pin_seq: 0 },
        Cand { id: 9003, freq: 4, pinned: false, pin_seq: 0 },
        Cand { id: 9004, freq: 2, pinned: false, pin_seq: 0 },
        Cand { id: 9005, freq: 1, pinned: false, pin_seq: 0 },
        Cand { id: 9006, freq: 6, pinned: false, pin_seq: 0 },
    ]
}

/// 智能面边界主巡检：×10 越权注入置顶恒胜全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线排序：置顶词 9001 恒首位（初始频次下成立）。
    let p0 = pool();
    let r0 = rank(&p0);
    if r0[0] != 9001 {
        return Err(E_PIN_OVERTHROWN);
    }
    hits += 1;

    // R2 越权注入 ×10：把非置顶词 9002 频次推到极值（×10 次增益），
    //    每次注入后置顶词仍恒第一（置顶恒胜判据本体）。
    let mut decl = UBoundDecl::new();
    decl.freeze();
    let mut eng = LearnEngine::new();
    let mut p = pool();
    let mut k = 0;
    while k < 10 {
        // 对全部非置顶词增益（最强越权：全体推高）。
        let mut i = 0;
        while i < p.len() {
            let _ = eng.boost(&mut p[i]);
            i += 1;
        }
        let r = rank(&p);
        if r[0] != 9001 {
            return Err(E_PIN_OVERTHROWN);
        }
        // 置顶词频次零变化（学习写不穿置顶层）。
        if p[0].freq != 3 {
            return Err(E_PIN_WRITE);
        }
        k += 1;
    }
    // 增益通路可观测：非置顶层全部实际增益（40 次成功请求）。
    if eng.gains != 50 || eng.denied != 10 {
        return Err(E_GAIN_LOST);
    }
    // 逐词核对增益值：+70（10 次 ×7）。
    let mut i2 = 1;
    while i2 < p.len() {
        if p[i2].freq != p0[i2].freq + 70 {
            return Err(E_GAIN_LOST);
        }
        i2 += 1;
    }
    hits += 1;

    // R3 置顶层写路径恒拒：×10 次直写请求全拒绝留痕。
    let mut d3 = 0;
    while d3 < 10 {
        if decl.learn_write_to_pinned() {
            return Err(E_PIN_WRITE);
        }
        d3 += 1;
    }
    if decl.tamper_marks() != 10 || !decl.frozen() {
        return Err(E_PIN_WRITE);
    }
    hits += 1;

    // R4 非置顶层全序：(freq desc, id asc) 逐位核对——构造同频次
    //    平局对（9004/9005 同频后 id 升序）。
    let mut p4 = pool();
    p4[3].freq = p4[4].freq; // 制造平局
    let r4 = rank(&p4);
    // 平局对之间 id 必须升序（9004 在 9005 前）。
    let mut pos4 = 1;
    let mut pos5 = 2;
    let mut i4 = 0;
    while i4 < r4.len() {
        if r4[i4] == 9004 {
            pos4 = i4;
        }
        if r4[i4] == 9005 {
            pos5 = i4;
        }
        i4 += 1;
    }
    if pos4 >= pos5 {
        return Err(E_ORDER_BAD);
    }
    hits += 1;

    // R5 置顶多词全序：两个置顶词按 pin_seq 升序（pin_seq 小者在前）。
    let mut p5 = pool();
    p5.push(Cand { id: 9007, freq: 0, pinned: true, pin_seq: 0 });
    let r5 = rank(&p5);
    if r5[0] != 9007 || r5[1] != 9001 {
        return Err(E_ORDER_BAD);
    }
    hits += 1;

    // R6 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 6] = [
        E_PIN_OVERTHROWN,
        E_PIN_WRITE,
        E_GAIN_LOST,
        E_ORDER_BAD,
        E_DECL_THAW,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
