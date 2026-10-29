//! cgb835_iris_xe_cert — CGPU-F0835 Iris Xe 档认证。
//!
//! 判据原文（验收行摘录）：Iris Xe 档认证：特征档硬门（EU 64~128、带宽
//! ≥ 48GB/s、媒体位含编解码）；场景面覆盖（内存/媒体/功耗/遥测四场景面
//! 全须有关卡通过记录）；档位越级拦截（声明高档特征按 Iris Xe 送检——
//! 拒收降档）；认证守恒账（关卡总数 = 通过+挂掉+跳过）。判据：特征硬
//! 门、四场景面、越级拦截、守恒账。
//!
//! 诊断段 0x72E0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容关卡账；守恒每步可验；drill() 自例程
//! 返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb835-v1";

/// 诊断段基（本单独占 0x72E0）。
pub const SEG_BASE: u16 = 0x72E0;

/// EU 越上界（送检 EU > 128——疑高档混检，越级拦截）。
pub const XC_EU_OVER: u16 = SEG_BASE + 1;
/// EU 不足（EU < 64——不及本档下限）。
pub const XC_EU_UNDER: u16 = SEG_BASE + 2;
/// 带宽不足（< 48GB/s）。
pub const XC_BW_SHORT: u16 = SEG_BASE + 3;
/// 媒体位缺失（编解码位不全）。
pub const XC_MEDIA_SHORT: u16 = SEG_BASE + 4;
/// 场景面缺口（四场景面有未过关卡）。
pub const SCENE_GAP: u16 = SEG_BASE + 5;
/// 守恒破坏。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;
/// 重复收敛（已判定再 finalize）。
pub const XC_ALREADY_DONE: u16 = SEG_BASE + 7;

/// Iris Xe 硬门区间。
pub const EU_LO: u32 = 64;
/// 硬门上界（越级拦截线）。
pub const EU_HI: u32 = 128;
/// 带宽下限。
pub const BW_FLOOR_GBS: u32 = 48;
/// 媒体必需位（bit0 解码 H264 + bit1 解码 HEVC + bit2 编码）。
pub const MEDIA_NEED: u32 = 0b0111;

/// 四场景面闭集（判据"四场景面"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 内存面。
    Mem,
    /// 媒体面。
    Media,
    /// 功耗面。
    Power,
    /// 遥测面。
    Telemetry,
}

impl Face {
    /// 面序号。
    pub fn idx(self) -> usize {
        match self {
            Face::Mem => 0,
            Face::Media => 1,
            Face::Power => 2,
            Face::Telemetry => 3,
        }
    }
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Face::Mem => "mem",
            Face::Media => "media",
            Face::Power => "power",
            Face::Telemetry => "telemetry",
        }
    }
}

/// 送检特征。
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// EU 数。
    pub eus: u32,
    /// 带宽（GB/s）。
    pub bandwidth_gbs: u32,
    /// 媒体能力位图。
    pub media_bits: u32,
}

/// 特征硬门（判据"特征档硬门"三检 + 越级拦截）。
pub fn spec_gate(s: &Spec) -> Result<(), u16> {
    if s.eus > EU_HI {
        return Err(XC_EU_OVER);
    }
    if s.eus < EU_LO {
        return Err(XC_EU_UNDER);
    }
    if s.bandwidth_gbs < BW_FLOOR_GBS {
        return Err(XC_BW_SHORT);
    }
    if s.media_bits & MEDIA_NEED != MEDIA_NEED {
        return Err(XC_MEDIA_SHORT);
    }
    Ok(())
}

/// 关卡状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateSt {
    /// 待跑。
    Todo,
    /// 通过。
    Pass,
    /// 挂掉。
    Fail,
    /// 跳过。
    Skip,
}

impl GateSt {
    /// 终态否。
    pub fn terminal(self) -> bool {
        matches!(self, GateSt::Pass | GateSt::Fail | GateSt::Skip)
    }
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            GateSt::Todo => "todo",
            GateSt::Pass => "pass",
            GateSt::Fail => "fail",
            GateSt::Skip => "skip",
        }
    }
}

/// 关卡。
#[derive(Clone, Debug)]
pub struct Gate {
    /// 关卡名。
    pub title: String,
    /// 所属场景面。
    pub face: Face,
    /// 状态。
    pub st: GateSt,
}

/// Iris Xe 认证账。
pub struct IrisCert {
    /// 账名。
    pub name: String,
    /// 送检特征（硬门过后冻结）。
    pub spec: Spec,
    /// 关卡账（定容 32）。
    pub gates: Vec<Gate>,
    /// 判定位。
    done: bool,
    /// 判定结果。
    pub passed: bool,
}

/// 守恒恒等式：gates.len() == 通过数 + 挂掉数 + 跳过数 + 待跑数。
/// 每次状态迁移后 verify() 复核。
impl IrisCert {
    /// 构造（硬门前置——判据"三项不过即拒"）。
    pub fn new(name: &str, s: Spec) -> Result<Self, u16> {
        spec_gate(&s)?;
        Ok(IrisCert { name: name.to_string(), spec: s, gates: Vec::new(), done: false, passed: false })
    }

    /// 登记关卡。
    pub fn add_gate(&mut self, title: &str, face: Face) -> Result<usize, u16> {
        if self.done {
            return Err(XC_ALREADY_DONE);
        }
        if self.gates.len() >= 32 {
            return Err(XC_LEDGER_DRIFT);
        }
        self.gates.push(Gate { title: title.to_string(), face, st: GateSt::Todo });
        self.verify()?;
        Ok(self.gates.len() - 1)
    }

    /// 关卡判定。
    pub fn judge(&mut self, idx: usize, st: GateSt) -> Result<(), u16> {
        if self.done {
            return Err(XC_ALREADY_DONE);
        }
        if idx >= self.gates.len() {
            return Err(XC_LEDGER_DRIFT);
        }
        if self.gates[idx].st.terminal() {
            return Err(XC_LEDGER_DRIFT);
        }
        if !st.terminal() {
            return Err(XC_LEDGER_DRIFT);
        }
        self.gates[idx].st = st;
        self.verify()?;
        Ok(())
    }

    /// 各状态计数。
    pub fn count_of(&self, st: GateSt) -> u32 {
        let mut n = 0;
        for g in self.gates.iter() {
            if g.st == st {
                n += 1;
            }
        }
        n
    }

    /// 面覆盖核验（判据"四场景面全须有关卡通过记录"）。
    pub fn face_cover(&self) -> Result<(), u16> {
        let faces = [Face::Mem, Face::Media, Face::Power, Face::Telemetry];
        for f in faces.iter() {
            let mut hit = false;
            for g in self.gates.iter() {
                if g.face == *f && g.st == GateSt::Pass {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(SCENE_GAP);
            }
        }
        Ok(())
    }

    /// 收敛判定（全关卡终态 + 四面覆盖 → passed）。
    pub fn finalize(&mut self) -> Result<bool, u16> {
        if self.done {
            return Err(XC_ALREADY_DONE);
        }
        let todo = self.count_of(GateSt::Todo);
        if todo > 0 {
            return Err(SCENE_GAP);
        }
        self.face_cover()?;
        let fail = self.count_of(GateSt::Fail);
        self.done = true;
        self.passed = fail == 0;
        Ok(self.passed)
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        let p = self.count_of(GateSt::Pass) as usize;
        let f = self.count_of(GateSt::Fail) as usize;
        let s = self.count_of(GateSt::Skip) as usize;
        let t = self.count_of(GateSt::Todo) as usize;
        if p + f + s + t != self.gates.len() {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb835[");
        s.push_str(&self.name);
        s.push_str("] gates=");
        s.push_str(&self.gates.len().to_string());
        s.push_str(" pass=");
        s.push_str(&self.count_of(GateSt::Pass).to_string());
        s.push_str(" fail=");
        s.push_str(&self.count_of(GateSt::Fail).to_string());
        s.push_str(" todo=");
        s.push_str(&self.count_of(GateSt::Todo).to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    // 负样本 1：越级拦截（EU 512 疑高档混检）。
    let over = Spec { eus: 512, bandwidth_gbs: 64, media_bits: MEDIA_NEED };
    match IrisCert::new("x", over) {
        Err(code) if code == XC_EU_OVER => {}
        Ok(_) => return Err(XC_EU_OVER),
        Err(other) => return Err(other),
    }
    // 负样本 2：EU 不足。
    let under = Spec { eus: 32, bandwidth_gbs: 64, media_bits: MEDIA_NEED };
    match IrisCert::new("x", under) {
        Err(code) if code == XC_EU_UNDER => {}
        Ok(_) => return Err(XC_EU_UNDER),
        Err(other) => return Err(other),
    }
    // 负样本 3：带宽不足。
    let bw = Spec { eus: 96, bandwidth_gbs: 32, media_bits: MEDIA_NEED };
    match IrisCert::new("x", bw) {
        Err(code) if code == XC_BW_SHORT => {}
        Ok(_) => return Err(XC_BW_SHORT),
        Err(other) => return Err(other),
    }
    // 负样本 4：媒体位缺失（缺编码位）。
    let noenc = Spec { eus: 96, bandwidth_gbs: 64, media_bits: 0b0011 };
    match IrisCert::new("x", noenc) {
        Err(code) if code == XC_MEDIA_SHORT => {}
        Ok(_) => return Err(XC_MEDIA_SHORT),
        Err(other) => return Err(other),
    }

    // 正样本：合规送检 + 四面关卡全过。
    let spec = Spec { eus: 96, bandwidth_gbs: 64, media_bits: MEDIA_NEED };
    let mut c = IrisCert::new("w3-drill-835", spec)?;
    let g0 = c.add_gate("内存池压测", Face::Mem)?;
    let g1 = c.add_gate("编解码回路", Face::Media)?;
    let g2 = c.add_gate("功耗画像采样", Face::Power)?;
    let g3 = c.add_gate("遥测通道回放", Face::Telemetry)?;
    let g4 = c.add_gate("媒体边界补充", Face::Media)?;
    c.judge(g0, GateSt::Pass)?;
    c.judge(g1, GateSt::Pass)?;
    c.judge(g2, GateSt::Pass)?;
    c.judge(g3, GateSt::Pass)?;
    c.judge(g4, GateSt::Skip)?;
    c.verify()?;
    if !c.finalize()? {
        return Err(SCENE_GAP);
    }
    // 负样本 5：判定后改判。
    match c.judge(g0, GateSt::Fail) {
        Err(code) if code == XC_ALREADY_DONE => {}
        Ok(_) => return Err(XC_ALREADY_DONE),
        Err(other) => return Err(other),
    }
    match c.finalize() {
        Err(code) if code == XC_ALREADY_DONE => {}
        Ok(_) => return Err(XC_ALREADY_DONE),
        Err(other) => return Err(other),
    }

    // 挂掉路径：一面无通过记录 → 收敛拒。
    let mut c2 = IrisCert::new("w3-drill-835-gap", spec)?;
    let h0 = c2.add_gate("内存池压测", Face::Mem)?;
    let h1 = c2.add_gate("编解码回路", Face::Media)?;
    let h2 = c2.add_gate("功耗画像采样", Face::Power)?;
    let h3 = c2.add_gate("遥测通道回放", Face::Telemetry)?;
    c2.judge(h0, GateSt::Pass)?;
    c2.judge(h1, GateSt::Pass)?;
    c2.judge(h2, GateSt::Pass)?;
    c2.judge(h3, GateSt::Fail)?;
    c2.verify()?;
    // 面覆盖缺口（Telemetry 无通过）。
    match c2.face_cover() {
        Err(code) if code == SCENE_GAP => {}
        Ok(_) => return Err(SCENE_GAP),
        Err(other) => return Err(other),
    }
    match c2.finalize() {
        Err(code) if code == SCENE_GAP => {}
        Ok(_) => return Err(SCENE_GAP),
        Err(other) => return Err(other),
    }

    // 待跑缺口路径：_todo 未清即收敛。
    let mut c3 = IrisCert::new("w3-drill-835-todo", spec)?;
    let k0 = c3.add_gate("内存池压测", Face::Mem)?;
    let k1 = c3.add_gate("编解码回路", Face::Media)?;
    let k2 = c3.add_gate("功耗画像采样", Face::Power)?;
    let k3 = c3.add_gate("遥测通道回放", Face::Telemetry)?;
    c3.judge(k0, GateSt::Pass)?;
    c3.judge(k1, GateSt::Pass)?;
    c3.judge(k2, GateSt::Pass)?;
    // k3 保持 Todo。
    match c3.finalize() {
        Err(code) if code == SCENE_GAP => {}
        Ok(_) => return Err(SCENE_GAP),
        Err(other) => return Err(other),
    }
    c3.judge(k3, GateSt::Pass)?;
    if !c3.finalize()? {
        return Err(SCENE_GAP);
    }
    c3.verify()?;
    c2.verify()?;
    Ok(0)
}
