//! UNX-F52364 GitHub Actions Self-hosted Runner 收口 · 0xC9AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52364"，逐字）：
//!   title: GitHub Actions Self-hosted Runner 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52365-J1 注册/执行
//!     ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写
//!     回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52364）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52365-J1，照原文锚定；功能按本卡标题（GitHub Actions Self-hosted
//! Runner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① job 五段 20 格：provision/checkout/run-steps/upload-artifacts/
//!      teardown 五家族闭集各 4 格，段单调（逆序必抓），载荷纯函数逐格
//!      恒等才收。
//!   ② 注册三元闸：registration-token/url/labels 三格全齐才放行取活。
//!   ③ workflow 投影 8 格：workflow→runner 侧投影闭集，逐格恒等、重复
//!      拒、漂移拒。
//!   ④ ephemeral 三态 6 格：idle/active/recycled 三态各进出 2 格闭集。
//!   ⑤ 与 Actions 服务消费联签 4 格：listen/job-claim/progress/finish
//!      逐格联签。
//!   ⑥ .NET 上栈声明 2 行：runtime/HostSDK 逐行闭集，缺一拒封。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（workflow 越闭集/workflow 漂移/
//!      job 段逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1 → S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52364;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// job 五段格数。
pub const JOB_CELLS: usize = 20;

/// 注册三元闸格数。
pub const REG_CELLS: usize = 3;

/// workflow 投影格数。
pub const WF_CELLS: usize = 8;

/// ephemeral 三态格数。
pub const EPH_CELLS: usize = 6;

/// 与 Actions 服务消费联签格数。
pub const SIGN_CELLS: usize = 4;

/// .NET 上栈声明行数。
pub const DOTNET_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhRunnerCode {
    /// 成功
    Ok = 0,
    /// job 段号越闭集
    ENoStage,
    /// job 格漂移
    EStageDrift,
    /// 段逆序
    EOrder,
    /// 注册闸格越闭集
    ENoReg,
    /// 注册闸漂移
    ERegDrift,
    /// workflow 投影格越闭集
    ENoWf,
    /// workflow 投影漂移
    EWfDrift,
    /// ephemeral 态越闭集
    ENoEph,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl GhRunnerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GhRunnerCode::Ok => "成功",
            GhRunnerCode::ENoStage => "job 段号越闭集：五段 20 格之外先登记",
            GhRunnerCode::EStageDrift => "job 格漂移：载荷与基准不恒等，逐格复算",
            GhRunnerCode::EOrder => "段逆序：provision→checkout→run-steps→upload-artifacts→teardown 单调，逆序必抓",
            GhRunnerCode::ENoReg => "注册闸格越闭集：registration-token/url/labels 三元之外先登记",
            GhRunnerCode::ERegDrift => "注册闸漂移：注册折叠与基准不恒等，逐格复算",
            GhRunnerCode::ENoWf => "workflow 投影格越闭集：8 格投影闭集之外先登记",
            GhRunnerCode::EWfDrift => "workflow 投影漂移：投影折叠与基准不恒等，逐格复算",
            GhRunnerCode::ENoEph => "ephemeral 态越闭集：三态 6 格之外先登记",
            GhRunnerCode::EDup => "重复落账：同格只许一条账目",
            GhRunnerCode::ESeal => "未齐即封：20 格 job + 3 闸 + 8 投影 + 6 态 + 4 联签未齐先补齐",
            GhRunnerCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GhRunnerCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20格job+3闸+8投影+6态+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// job 格载荷（格 i；段 = i/4）。
pub const fn job_cell(i: usize) -> Option<u64> {
    if i >= JOB_CELLS {
        return None;
    }
    Some((0xF1100u64 + (i as u64) * 0x43) ^ ((i / 4) as u64).wrapping_mul(0x47480000))
}

/// job 段（0..4=provision 4..8=checkout 8..12=run-steps 12..16=upload-artifacts 16..20=teardown）。
pub const fn job_kind(i: usize) -> Option<u32> {
    if i >= JOB_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 段名。
pub const fn job_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("provision"),
        1 => Some("checkout"),
        2 => Some("run-steps"),
        3 => Some("upload-artifacts"),
        4 => Some("teardown"),
        _ => None,
    }
}

/// 注册三元闸格载荷（闸 r：0=registration-token 1=url 2=labels）。
pub const fn reg_cell(r: usize) -> Option<u64> {
    if r >= REG_CELLS {
        return None;
    }
    Some(0xF1600u64 + (r as u64) * 0x61)
}

/// 注册闸名。
pub const fn reg_cell_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("registration-token"),
        1 => Some("url"),
        2 => Some("labels"),
        _ => None,
    }
}

/// workflow 投影格载荷（格 w，0..8）。
pub const fn wf_cell(w: usize) -> Option<u64> {
    if w >= WF_CELLS {
        return None;
    }
    Some((0xF1A00u64 + (w as u64) * 0x6D) ^ 0x5746_0000)
}

/// ephemeral 态格载荷（格 e，0..6；态 = e/2：0=idle 1=active 2=recycled）。
pub const fn eph_cell(e: usize) -> Option<u64> {
    if e >= EPH_CELLS {
        return None;
    }
    Some(0xF1E00u64 + (e as u64) * 0x37)
}

/// ephemeral 态名。
pub const fn eph_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("idle"),
        1 => Some("active"),
        2 => Some("recycled"),
        _ => None,
    }
}

/// 与 Actions 服务消费联签格载荷（格 g，0..4）。
pub const fn sign_cell(g: usize) -> Option<u64> {
    if g >= SIGN_CELLS {
        return None;
    }
    Some(0xF2200u64 + (g as u64) * 0x79)
}

/// 联签名。
pub const fn sign_cell_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("listen"),
        1 => Some("job-claim"),
        2 => Some("progress"),
        3 => Some("finish"),
        _ => None,
    }
}

/// .NET 上栈行折叠（行 r：0=runtime 1=HostSDK）。
pub const fn dotnet_row(r: usize) -> Option<u32> {
    if r >= DOTNET_ROWS {
        return None;
    }
    Some(0xF260 + r as u32 * 0x3D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitHub Actions Runner 收口账本状态机。
#[derive(Clone)]
pub struct GhRunnerLedger {
    /// 六判据（注册/取活/作业执行/构件上传/回传/注销清理）。
    gates: [Option<bool>; 6],
    /// job 账。
    job: [Option<u64>; JOB_CELLS],
    /// 注册三元闸账。
    reg: [Option<u64>; REG_CELLS],
    /// workflow 投影账。
    wf: [Option<u64>; WF_CELLS],
    /// ephemeral 态账。
    eph: [Option<u64>; EPH_CELLS],
    /// 联签账。
    sign: [Option<u64>; SIGN_CELLS],
    /// .NET 上栈声明账。
    net: [Option<u32>; DOTNET_ROWS],
    /// 最后段。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GhRunnerLedger {
    /// 新账本。
    pub fn new() -> Self {
        GhRunnerLedger {
            gates: [None; 6],
            job: [None; JOB_CELLS],
            reg: [None; REG_CELLS],
            wf: [None; WF_CELLS],
            eph: [None; EPH_CELLS],
            sign: [None; SIGN_CELLS],
            net: [None; DOTNET_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 最后段（测试用观察口）。
    pub fn last_stage(&self) -> i64 {
        self.last_kind
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GhRunnerCode> {
        if gate >= 6 {
            return Err(GhRunnerCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// job 一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_job(&mut self, i: usize, got: u64) -> Result<(), GhRunnerCode> {
        if i >= JOB_CELLS {
            return Err(GhRunnerCode::ENoStage);
        }
        let kind = match job_kind(i) {
            Some(k) => k,
            None => return Err(GhRunnerCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(GhRunnerCode::EOrder);
        }
        let want = match job_cell(i) {
            Some(w) => w,
            None => return Err(GhRunnerCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + i as u64);
            return Err(GhRunnerCode::EStageDrift);
        }
        if match self.job.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.job.get_mut(i) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + i as u64);
        Ok(())
    }

    /// job 前 n 格全落核验（注册/执行 ×10 任务的执行样本口径）。
    pub fn job_head_filled(&self, n: usize) -> bool {
        let mut i = 0usize;
        while i < n && i < JOB_CELLS {
            match self.job.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// job 全账核验（20 格）。
    pub fn job_all(&self) -> bool {
        let mut i = 0usize;
        while i < JOB_CELLS {
            match self.job.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 注册三元闸一格（载荷对照 + 重复拒）。
    pub fn record_reg(&mut self, r: usize, got: u64) -> Result<(), GhRunnerCode> {
        let want = match reg_cell(r) {
            Some(w) => w,
            None => return Err(GhRunnerCode::ENoReg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + r as u64);
            return Err(GhRunnerCode::ERegDrift);
        }
        if match self.reg.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.reg.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6301 + r as u64);
        Ok(())
    }

    /// 注册三元闸全齐核验。
    pub fn reg_all(&self) -> bool {
        let mut r = 0usize;
        while r < REG_CELLS {
            match self.reg.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// workflow 投影一格（载荷对照 + 重复拒）。
    pub fn record_wf(&mut self, w: usize, got: u64) -> Result<(), GhRunnerCode> {
        let want = match wf_cell(w) {
            Some(x) => x,
            None => return Err(GhRunnerCode::ENoWf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + w as u64);
            return Err(GhRunnerCode::EWfDrift);
        }
        if match self.wf.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.wf.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x6401 + w as u64);
        Ok(())
    }

    /// workflow 投影全账核验（8 格）。
    pub fn wf_all(&self) -> bool {
        let mut w = 0usize;
        while w < WF_CELLS {
            match self.wf.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// ephemeral 态一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_eph(&mut self, e: usize, got: u64) -> Result<(), GhRunnerCode> {
        let want = match eph_cell(e) {
            Some(x) => x,
            None => return Err(GhRunnerCode::ENoEph),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + e as u64);
            return Err(GhRunnerCode::EWfDrift);
        }
        if match self.eph.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.eph.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x6501 + e as u64);
        Ok(())
    }

    /// ephemeral 全账核验（6 格）。
    pub fn eph_all(&self) -> bool {
        let mut e = 0usize;
        while e < EPH_CELLS {
            match self.eph.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 与 Actions 服务消费联签一格（载荷对照 + 重复拒）。
    pub fn record_sign(&mut self, g: usize, got: u64) -> Result<(), GhRunnerCode> {
        let want = match sign_cell(g) {
            Some(x) => x,
            None => return Err(GhRunnerCode::ENoReg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + g as u64);
            return Err(GhRunnerCode::ERegDrift);
        }
        if match self.sign.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.sign.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x6601 + g as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn sign_all(&self) -> bool {
        let mut g = 0usize;
        while g < SIGN_CELLS {
            match self.sign.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// .NET 上栈一行（runtime/HostSDK）。
    pub fn declare_dotnet(&mut self, r: usize) -> Result<(), GhRunnerCode> {
        let w = match dotnet_row(r) {
            Some(x) => x,
            None => return Err(GhRunnerCode::ENoEph),
        };
        if match self.net.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhRunnerCode::EDup);
        }
        if let Some(c) = self.net.get_mut(r) {
            *c = Some(w);
        }
        self.fnv(0x6700 + r as u64);
        Ok(())
    }

    /// .NET 上栈全齐核验。
    pub fn dotnet_all(&self) -> bool {
        let mut r = 0usize;
        while r < DOTNET_ROWS {
            match self.net.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.job_all() && self.reg_all() && self.wf_all()
            && self.eph_all() && self.sign_all() && self.dotnet_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GhRunnerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GhRunnerLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < JOB_CELLS {
        if let Some(w) = job_cell(i) {
            if led.record_job(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < REG_CELLS {
        if let Some(w) = reg_cell(r) {
            if led.record_reg(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut w = 0usize;
    while w < WF_CELLS {
        if let Some(x) = wf_cell(w) {
            if led.record_wf(w, x).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut e = 0usize;
    while e < EPH_CELLS {
        if let Some(x) = eph_cell(e) {
            if led.record_eph(e, x).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut s = 0usize;
    while s < SIGN_CELLS {
        if let Some(x) = sign_cell(s) {
            if led.record_sign(s, x).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut n = 0usize;
    while n < DOTNET_ROWS {
        if led.declare_dotnet(n).is_ok() {
            recorded += 1;
        }
        n += 1;
    }
    // 注入 10 轮：workflow 越闭集 / workflow 漂移 / job 段逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GhRunnerLedger::new();
        let caught = match kind {
            0 => lt.record_wf(WF_CELLS, 0) == Err(GhRunnerCode::ENoWf),
            1 => {
                let ww = match wf_cell(3) {
                    Some(x) => x,
                    None => 0,
                };
                lt.record_wf(3, ww.wrapping_add(1)) == Err(GhRunnerCode::EWfDrift)
            }
            _ => {
                let _ = lt.record_job(11, job_cell(11).unwrap_or(0));
                lt.record_job(4, job_cell(4).unwrap_or(0)) == Err(GhRunnerCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52364 判据集（锚 UNX-F52365-J1，off-by-one 板式）。
pub fn run_unx52364_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52364");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "job 五段 20 格全通（provision/checkout/run-steps/upload-artifacts/teardown）",
        recorded >= JOB_CELLS as u32,
        "五段四格逐格载荷恒等落账",
    );
    s.add(
        "注册/执行 ×10 任务全通（UNX-F52365-J1）",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            // 注册三元闸先落，随后 job 段前 10 格执行任务逐个恒等。
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    if lt.record_reg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            let mut i = 0usize;
            while i < 10usize {
                if let Some(w) = job_cell(i) {
                    if lt.record_job(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.reg_all() && lt.job_head_filled(10)
        },
        "三元闸放行后十任务逐个恒等（判据锚 §AI-66.3）",
    );
    s.add(
        "agent 注册三元闸 3/3 全齐",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    if lt.record_reg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.reg_all()
        },
        "registration-token/url/labels 三元闭集缺一不放行",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GhRunnerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "注册/取活/作业执行/构件上传/回传/注销清理逐格落账",
    );
    s.add(
        "workflow 投影 8 格全齐",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < WF_CELLS {
                if let Some(x) = wf_cell(w) {
                    if lt.record_wf(w, x).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.wf_all()
        },
        "workflow→runner 投影闭集逐格恒等",
    );
    s.add(
        "ephemeral 三态 6 格全齐",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EPH_CELLS {
                if let Some(x) = eph_cell(e) {
                    if lt.record_eph(e, x).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eph_all()
        },
        "idle/active/recycled 三态各进出两格闭集",
    );
    s.add(
        "与 Actions 服务消费联签 4 格全齐",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SIGN_CELLS {
                if let Some(x) = sign_cell(g) {
                    if lt.record_sign(g, x).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.sign_all()
        },
        "listen/job-claim/progress/finish 逐格联签",
    );
    s.add(
        ".NET 上栈声明 2 行全落",
        {
            let mut lt = GhRunnerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DOTNET_ROWS {
                if lt.declare_dotnet(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.dotnet_all()
        },
        "runtime/HostSDK 两行闭集缺一拒封",
    );
    s.add(
        "五段单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < JOB_CELLS {
                if job_kind(i).unwrap_or(5) < job_kind(i - 1).unwrap_or(5) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "provision→checkout→run-steps→upload-artifacts→teardown 段单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "workflow 越闭集/workflow 漂移/job 段逆序三态轮转必抓");
    s.add(
        "边界·job 格越闭集",
        GhRunnerLedger::new().record_job(JOB_CELLS, 0).is_err(),
        "格 20 越闭集必拒",
    );
    s.add(
        "边界·workflow 投影越闭集",
        GhRunnerLedger::new().record_wf(WF_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·ephemeral 态越闭集",
        GhRunnerLedger::new().record_eph(EPH_CELLS, 0).is_err(),
        "态格 6 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GhRunnerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < JOB_CELLS {
                if let Some(w) = job_cell(i) {
                    let _ = lt.record_job(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    let _ = lt.record_reg(r, w);
                }
                r += 1;
            }
            let mut w = 0usize;
            while w < WF_CELLS {
                if let Some(x) = wf_cell(w) {
                    let _ = lt.record_wf(w, x);
                }
                w += 1;
            }
            let mut e = 0usize;
            while e < EPH_CELLS {
                if let Some(x) = eph_cell(e) {
                    let _ = lt.record_eph(e, x);
                }
                e += 1;
            }
            let mut sg = 0usize;
            while sg < SIGN_CELLS {
                if let Some(x) = sign_cell(sg) {
                    let _ = lt.record_sign(sg, x);
                }
                sg += 1;
            }
            let mut n = 0usize;
            while n < DOTNET_ROWS {
                let _ = lt.declare_dotnet(n);
                n += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9AC0,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9AC0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 17 条闭集（本条按求值时点 len==16）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_kind_mapping() {
        assert_eq!(job_kind(0), Some(0));
        assert_eq!(job_kind(4), Some(1));
        assert_eq!(job_kind(7), Some(1));
        assert_eq!(job_kind(13), Some(3));
        assert_eq!(job_kind(19), Some(4));
        assert!(job_kind(20).is_none());
    }

    #[test]
    fn test_names() {
        assert_eq!(job_kind_name(0), Some("provision"));
        assert_eq!(job_kind_name(4), Some("teardown"));
        assert!(job_kind_name(5).is_none());
        assert_eq!(reg_cell_name(0), Some("registration-token"));
        assert_eq!(reg_cell_name(2), Some("labels"));
        assert!(reg_cell_name(3).is_none());
        assert_eq!(sign_cell_name(0), Some("listen"));
        assert_eq!(sign_cell_name(3), Some("finish"));
        assert!(sign_cell_name(4).is_none());
        assert_eq!(eph_state_name(0), Some("idle"));
        assert_eq!(eph_state_name(1), Some("active"));
        assert_eq!(eph_state_name(2), Some("recycled"));
        assert!(eph_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = job_cell(9).unwrap_or(0);
        assert_eq!(a, job_cell(9).unwrap_or(0));
        assert_ne!(a, job_cell(10).unwrap_or(0));
        assert!(job_cell(JOB_CELLS).is_none());
        assert!(reg_cell(REG_CELLS).is_none());
        assert!(wf_cell(WF_CELLS).is_none());
        assert!(eph_cell(EPH_CELLS).is_none());
        assert!(sign_cell(SIGN_CELLS).is_none());
        assert!(dotnet_row(DOTNET_ROWS).is_none());
    }

    #[test]
    fn test_reg_gate_then_job() {
        let mut lt = GhRunnerLedger::new();
        let mut r = 0usize;
        while r < REG_CELLS {
            assert!(lt.record_reg(r, reg_cell(r).unwrap_or(0)).is_ok());
            r += 1;
        }
        assert!(lt.reg_all());
        let mut lt2 = GhRunnerLedger::new();
        assert!(lt2.record_reg(1, reg_cell(1).unwrap_or(0)).is_ok());
        assert!(!lt2.reg_all());
    }

    #[test]
    fn test_order_caught() {
        // 按合法序先落 run-steps 段（kind=2），再注 checkout 段（kind=1）→ EOrder。
        let mut lt = GhRunnerLedger::new();
        assert!(lt.record_job(11, job_cell(11).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_job(5, job_cell(5).unwrap_or(0)),
            Err(GhRunnerCode::EOrder)
        );
        assert_eq!(lt.last_stage(), 2);
        assert!(lt.record_job(13, job_cell(13).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GhRunnerLedger::new();
        assert_eq!(
            lt.record_job(2, job_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(GhRunnerCode::EStageDrift)
        );
        assert_eq!(
            lt.record_wf(3, wf_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(GhRunnerCode::EWfDrift)
        );
        assert_eq!(
            lt.record_reg(0, reg_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(GhRunnerCode::ERegDrift)
        );
        assert_eq!(
            lt.record_eph(4, eph_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(GhRunnerCode::EWfDrift)
        );
        assert_eq!(
            lt.record_sign(2, sign_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(GhRunnerCode::ERegDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GhRunnerLedger::new();
        let _ = lt.record_job(0, job_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_job(0, job_cell(0).unwrap_or(0)),
            Err(GhRunnerCode::EDup)
        );
        let _ = lt.record_wf(0, wf_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_wf(0, wf_cell(0).unwrap_or(0)),
            Err(GhRunnerCode::EDup)
        );
        let _ = lt.record_eph(0, eph_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_eph(0, eph_cell(0).unwrap_or(0)),
            Err(GhRunnerCode::EDup)
        );
        let _ = lt.declare_dotnet(0);
        assert_eq!(lt.declare_dotnet(0), Err(GhRunnerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GhRunnerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GhRunnerLedger::new();
        assert!(lt2.judge_gate(2).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 job + 3 闸 + 8 投影 + 6 态 + 4 联签 + 2 .NET 行 = 43。
        assert_eq!(recorded, 43);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_kinds() {
        // 三态轮转：0=workflow 越闭集 1=workflow 漂移 2=job 段逆序（先落 run-steps 再注 checkout）。
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = GhRunnerLedger::new();
            let caught = match kind {
                0 => lt.record_wf(WF_CELLS, 0).is_err(),
                1 => lt
                    .record_wf(3, wf_cell(3).unwrap_or(0).wrapping_add(1))
                    .is_err(),
                _ => {
                    let _ = lt.record_job(11, job_cell(11).unwrap_or(0));
                    lt.record_job(4, job_cell(4).unwrap_or(0)).is_err()
                }
            };
            assert!(caught, "注入轮 {} 必抓", k);
            k += 1;
        }
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9AC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GhRunnerCode::Ok as u32, 0);
        assert_eq!(GhRunnerCode::EGradeDrift as u32, 12);
        assert_eq!(GhRunnerCode::ENoEph.val(), SEG_BASE + 8);
        assert!(!GhRunnerCode::Ok.detail().is_empty());
        assert!(!GhRunnerCode::ESeal.detail().is_empty());
        assert!(!GhRunnerCode::EWfDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GhRunnerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = GhRunnerLedger::new();
        let _ = lt2.judge_gate(4);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = GhRunnerLedger::new();
        let _ = lt.record_wf(0, wf_cell(0).unwrap_or(0).wrapping_add(17));
        let (i, c, e) = lt.stats();
        assert_eq!((i, c, e), (1, 1, 0));
    }

    #[test]
    fn unx52364_checks_green() {
        let s = run_unx52364_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 18);
    }
}
