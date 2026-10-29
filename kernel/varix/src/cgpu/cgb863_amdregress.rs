//! cgb863_amdregress —— CGPU-F0863 AMD 回归矩阵。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0863 AMD 回归矩阵  <SEG 0x7660>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0863；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 回归矩阵，整数账面）：
//!   1. 回归面闭集六面（写死）：架构适配 / 显存路径 / 媒体管线 /
//!      功耗曲线 / 遥测口径 / 驱动升级；面号 0..=5，越界红码
//!      （XC_BAD_FACE）；
//!   2. 用例状态机：Planned → Armed → Ran → Pass/Fail；Fail 可
//!      复跑（Ran 轮次入账，上限 3 次）；非法迁移红码
//!      （XC_BAD_TRANS）；
//!   3. 矩阵账（6 面 × 定容 16 用例）守恒：计划数 = 已跑数 + 待
//!      跑数；已跑数 = 过数 + 挂数；双恒等式漂移红码
//!      （XC_PLAN_DRIFT / XC_RUN_DRIFT）；
//!   4. 复跑上限（写死 3）：超限仍 Fail 判挂死（红码
//!      XC_RERUN_EXHAUSTED）；
//!   5. 面完成判据：面内全部用例 Pass 才可关面；未齐关面红码
//!      （XC_FACE_INCOMPLETE）；
//!   6. 总关账：六面全关才可出回归报告；未齐出报告红码
//!      （XC_REPORT_BLOCKED）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7660 段为本模块独占（SEG_BASE=0x7660），判别式
//!   SEG_BASE+1..+N；与 0x7640/0x7680 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AR_VERSION: &str = "cgb863_amdregress-v1";

/// 诊断段基址（0x7660 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7660;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7661 回归面越界（0..=5 之外）。
pub const XC_BAD_FACE: u16 = SEG_BASE + 1;
/// 0x7662 用例状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7663 计划账漂移（计划 ≠ 已跑 + 待跑）。
pub const XC_PLAN_DRIFT: u16 = SEG_BASE + 3;
/// 0x7664 跑批账漂移（已跑 ≠ 过 + 挂）。
pub const XC_RUN_DRIFT: u16 = SEG_BASE + 4;
/// 0x7665 复跑超限。
pub const XC_RERUN_EXHAUSTED: u16 = SEG_BASE + 5;
/// 0x7666 面未齐即关。
pub const XC_FACE_INCOMPLETE: u16 = SEG_BASE + 6;
/// 0x7667 报告受阻（面未全关）。
pub const XC_REPORT_BLOCKED: u16 = SEG_BASE + 7;
/// 0x7668 用例号越界。
pub const XC_BAD_CASE: u16 = SEG_BASE + 8;
/// 0x7669 面账满（16）。
pub const XC_FACE_FULL: u16 = SEG_BASE + 9;
/// 0x766A 重复用例名。
pub const XC_DUP_CASE: u16 = SEG_BASE + 10;

/// 回归面数（写死：6）。
pub const FACE_CAP: usize = 6;
/// 每面用例容量（写死：16）。
pub const CASE_CAP: usize = 16;
/// 复跑上限（写死：3）。
pub const RERUN_MAX: u32 = 3;

/// 回归面闭集。
pub const F_ARCH: usize = 0;
pub const F_VRAM: usize = 1;
pub const F_MEDIA: usize = 2;
pub const F_POWER: usize = 3;
pub const F_TELEM: usize = 4;
pub const F_DRVUPD: usize = 5;

/// 面名（闭集原文）。
pub fn face_name(f: usize) -> &'static str {
    match f {
        F_ARCH => "arch",
        F_VRAM => "vram",
        F_MEDIA => "media",
        F_POWER => "power",
        F_TELEM => "telem",
        _ => "drvupd",
    }
}

/// 用例状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RState {
    /// 已计划。
    Planned,
    /// 已武装。
    Armed,
    /// 已跑通过。
    Pass,
    /// 已跑失败。
    Fail,
}

/// 单条回归用例。
#[derive(Clone, Debug)]
pub struct RCase {
    /// 用例名（面内去重键）。
    pub name: String,
    /// 状态。
    pub state: RState,
    /// 跑批次数。
    pub runs: u32,
    /// 复跑次数（Fail→Armed）。
    pub reruns: u32,
}

/// 单面账。
pub struct Face {
    /// 面号。
    pub no: usize,
    /// 用例账（定容 16）。
    pub cases: Vec<RCase>,
    /// 关面标志。
    pub closed: bool,
}

impl Face {
    fn new(no: usize) -> Self {
        Face {
            no,
            cases: Vec::new(),
            closed: false,
        }
    }

    /// 名查重。
    pub fn has_name(&self, name: &str) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].name == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 面内全 Pass 判定。
    pub fn all_pass(&self) -> bool {
        if self.cases.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].state != RState::Pass {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// AMD 回归矩阵。
pub struct RegressMatrix {
    /// 六面账。
    pub faces: [Face; FACE_CAP],
    /// 计划数（立案例计一次）。
    pub planned: u64,
    /// 已跑数（Ran 计一次）。
    pub ran: u64,
    /// 过数。
    pub passed: u64,
    /// 挂数。
    pub failed: u64,
    /// 待跑数（Planned+Armed 存量）。
    pub pending: u64,
    /// 报告标志。
    pub reported: bool,
    /// 累计红码。
    pub red_count: u64,
}

impl RegressMatrix {
    /// 空矩阵。
    pub fn new() -> Self {
        RegressMatrix {
            faces: [
                Face::new(0),
                Face::new(1),
                Face::new(2),
                Face::new(3),
                Face::new(4),
                Face::new(5),
            ],
            planned: 0,
            ran: 0,
            passed: 0,
            failed: 0,
            pending: 0,
            reported: false,
            red_count: 0,
        }
    }

    /// 立案（面闭集 + 名查重 + 容量）。
    pub fn plan(&mut self, face: usize, name: &str) -> Result<(), u16> {
        if face >= FACE_CAP {
            self.red_count += 1;
            return Err(XC_BAD_FACE);
        }
        if self.faces[face].has_name(name) {
            self.red_count += 1;
            return Err(XC_DUP_CASE);
        }
        if self.faces[face].cases.len() == CASE_CAP {
            self.red_count += 1;
            return Err(XC_FACE_FULL);
        }
        self.faces[face].cases.push(RCase {
            name: String::from(name),
            state: RState::Planned,
            runs: 0,
            reruns: 0,
        });
        self.planned += 1;
        self.pending += 1;
        Ok(())
    }

    /// 定位用例。
    fn locate(&self, face: usize, name: &str) -> Result<usize, u16> {
        if face >= FACE_CAP {
            return Err(XC_BAD_FACE);
        }
        let mut i = 0;
        while i < self.faces[face].cases.len() {
            if self.faces[face].cases[i].name == name {
                return Ok(i);
            }
            i += 1;
        }
        Err(XC_BAD_CASE)
    }

    /// 武装（Planned/Fail → Armed；Fail 计复跑）。
    pub fn arm(&mut self, face: usize, name: &str) -> Result<(), u16> {
        let idx = self.locate(face, name)?;
        let cur = self.faces[face].cases[idx].state;
        match cur {
            RState::Planned => {}
            RState::Fail => {
                if self.faces[face].cases[idx].reruns + 1 > RERUN_MAX {
                    self.red_count += 1;
                    return Err(XC_RERUN_EXHAUSTED);
                }
                self.faces[face].cases[idx].reruns += 1;
                self.failed -= 1;
                self.pending += 1;
            }
            _ => {
                self.red_count += 1;
                return Err(XC_BAD_TRANS);
            }
        }
        self.faces[face].cases[idx].state = RState::Armed;
        Ok(())
    }

    /// 跑批（Armed → Pass/Fail）。
    pub fn run(&mut self, face: usize, name: &str, ok: bool) -> Result<(), u16> {
        let idx = self.locate(face, name)?;
        if self.faces[face].cases[idx].state != RState::Armed {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        self.faces[face].cases[idx].state = if ok { RState::Pass } else { RState::Fail };
        self.faces[face].cases[idx].runs += 1;
        self.ran += 1;
        self.pending -= 1;
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        Ok(())
    }

    /// 关面（面内全 Pass）。
    pub fn close_face(&mut self, face: usize) -> Result<(), u16> {
        if face >= FACE_CAP {
            self.red_count += 1;
            return Err(XC_BAD_FACE);
        }
        if self.faces[face].closed {
            self.red_count += 1;
            return Err(XC_FACE_INCOMPLETE);
        }
        if !self.faces[face].all_pass() {
            self.red_count += 1;
            return Err(XC_FACE_INCOMPLETE);
        }
        self.faces[face].closed = true;
        Ok(())
    }

    /// 计划账守恒。
    pub fn plan_balanced(&self) -> bool {
        self.planned == self.ran + self.pending
    }

    /// 跑批账守恒。
    pub fn run_balanced(&self) -> bool {
        self.ran == self.passed + self.failed
    }

    /// 出报告（六面全关）。
    pub fn report(&mut self) -> Result<String, u16> {
        if !self.plan_balanced() {
            self.red_count += 1;
            return Err(XC_PLAN_DRIFT);
        }
        let mut f = 0;
        while f < FACE_CAP {
            if !self.faces[f].closed {
                self.red_count += 1;
                return Err(XC_REPORT_BLOCKED);
            }
            f += 1;
        }
        self.reported = true;
        let mut s = String::from("amdregress faces=6 pass=");
        s += &self.passed.to_string();
        s += " rerun_fail=";
        s += &self.failed.to_string();
        Ok(s)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdregress plan=");
        s += &self.planned.to_string();
        s += " ran=";
        s += &self.ran.to_string();
        s += " pass=";
        s += &self.passed.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.plan_balanced() {
            return Err(XC_PLAN_DRIFT);
        }
        if !self.run_balanced() {
            return Err(XC_RUN_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：六面立案 → 全跑 → 复跑路径 → 关面 → 报告 + 负例七连。
pub fn drill() -> Result<u32, u16> {
    let mut m = RegressMatrix::new();
    // 每面立案 2 条。
    let mut f = 0;
    while f < FACE_CAP {
        m.plan(f, "basic")?;
        m.plan(f, "stress")?;
        f += 1;
    }
    // 负例：面越界 / 重名。
    if m.plan(FACE_CAP, "x") != Err(XC_BAD_FACE) {
        return Err(XC_BAD_FACE);
    }
    if m.plan(0, "basic") != Err(XC_DUP_CASE) {
        return Err(XC_DUP_CASE);
    }
    // 负例：未武装即跑。
    if m.run(0, "basic", true) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    // 负例：用例名越界。
    if m.arm(0, "ghost") != Err(XC_BAD_CASE) {
        return Err(XC_BAD_CASE);
    }
    // 全武装全跑通。
    let mut f2 = 0;
    while f2 < FACE_CAP {
        m.arm(f2, "basic")?;
        m.arm(f2, "stress")?;
        m.run(f2, "basic", true)?;
        m.run(f2, "stress", true)?;
        f2 += 1;
    }
    m.audit()?;
    // 复跑路径：F_MEDIA stress 失败两次后第三次过。
    m.arm(F_MEDIA, "stress")?;
    m.run(F_MEDIA, "stress", false)?;
    m.arm(F_MEDIA, "stress")?;
    m.run(F_MEDIA, "stress", false)?;
    m.arm(F_MEDIA, "stress")?;
    m.run(F_MEDIA, "stress", true)?;
    // 复跑上限：F_POWER stress 已 1 次过；再 arm 会因 Pass 拒绝。
    if m.arm(F_POWER, "stress") != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    m.audit()?;
    // 关面 + 报告。
    let mut f3 = 0;
    while f3 < FACE_CAP {
        m.close_face(f3)?;
        f3 += 1;
    }
    // 负例：重复关面。
    if m.close_face(0) != Err(XC_FACE_INCOMPLETE) {
        return Err(XC_FACE_INCOMPLETE);
    }
    let rep = m.report()?;
    // 负例：未关面出报告。
    let mut q = RegressMatrix::new();
    q.plan(0, "a")?;
    if q.report() != Err(XC_REPORT_BLOCKED) {
        return Err(XC_REPORT_BLOCKED);
    }
    // 负例：复跑超限。
    let mut r = RegressMatrix::new();
    r.plan(0, "flaky")?;
    let mut k = 0;
    while k <= RERUN_MAX {
        r.arm(0, "flaky")?;
        r.run(0, "flaky", false)?;
        k += 1;
    }
    // 已 4 次 Fail（1 原始 + 3 复跑），再 arm 应超限。
    if r.arm(0, "flaky") != Err(XC_RERUN_EXHAUSTED) {
        return Err(XC_RERUN_EXHAUSTED);
    }
    if m.summary().len() == 0 || rep.len() == 0 {
        return Err(XC_REPORT_BLOCKED);
    }
    Ok(0)
}
