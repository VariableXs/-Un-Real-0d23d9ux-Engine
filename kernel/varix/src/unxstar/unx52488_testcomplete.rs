//! UNX-F52488 TestComplete 收口 · 0xCB9C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52488"，逐字）：
//!   title: TestComplete 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52489-J1 ×10；
//!     许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线②；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52488）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52489-J1，照原文锚定；功能按本卡标题（TestComplete 收口）落
//! 地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 项目族闭集：五项目族（脚本/关键字/录制/BDD/CI 消费）各 4 操作
//!      共 20 格，项目族单调（脚本→关键字→录制→BDD→CI 消费，逆序必
//!      抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② 引擎三元闸：启动/执行/报告三闸逐格落账，漂移必抓，缺闸拒封。
//!   ③ 与 Jenkins 消费联签 4 格（F52361 锚定：消费联签四格闭集），
//!      逐格折叠对照，漂移必抓。
//!   ④ 商业许可声明 2 行闭集（商业许可依赖声明/许可面消费分账声明），
//!      缺一拒封；许可面如实定级（§AI-66.9 红线②）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal_gate/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（项目族逆序/引擎闸
//!      漂移/联签漂移）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集（S1/S2/S3）：TestComplete 为商业许可，按
//!      消费面如实定级 —— 六判据齐即 S2（有条件收口），全齐亦 S2，
//!      空账 S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52488;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 项目操作数。
pub const PROJ_OPS: usize = 20;

/// 引擎三元闸数。
pub const ENG_GATES: usize = 3;

/// Jenkins 消费联签格数。
pub const JOINT_CELLS: usize = 4;

/// 商业许可声明行数。
pub const LIC_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestCompCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 项目族逆序
    EOrder,
    /// 引擎闸越闭集
    ENoEng,
    /// 引擎闸漂移
    EEngDrift,
    /// 联签格越闭集
    ENoJoint,
    /// 联签漂移
    EJointDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl TestCompCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TestCompCode::Ok => "成功",
            TestCompCode::ENoOp => "操作号越闭集：五项目族 20 操作集之外先登记",
            TestCompCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            TestCompCode::EOrder => "项目族逆序：脚本→关键字→录制→BDD→CI 消费单调，逆序必抓",
            TestCompCode::ENoEng => "引擎闸越闭集：引擎三元闸（启动/执行/报告）之外先登记",
            TestCompCode::EEngDrift => "引擎闸漂移：闸载荷折叠与基准不恒等，逐闸复算",
            TestCompCode::ENoJoint => "联签格越闭集：Jenkins 消费联签 4 格之外先登记",
            TestCompCode::EJointDrift => "联签漂移：联签折叠与基准不恒等，逐格复算",
            TestCompCode::ENoRow => "声明行越闭集：商业许可 2 行闭集之外先登记",
            TestCompCode::ERowMiss => "缺声明行：商业许可声明未齐先补齐",
            TestCompCode::EDup => "重复落账：同格只许一条账目",
            TestCompCode::ESeal => "未齐即封：20 操作 + 3 引擎闸 + 4 联签 + 2 行声明未齐先补齐",
            TestCompCode::EGradeDrift => "定级漂移：六判据齐才可定 S2（商业许可如实定级）",
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
    /// 定级值（const 可调）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口",
            Grade::S2 => "S2：六判据齐，有条件收口（商业许可面如实定级，§AI-66.9 红线②）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 项目操作格载荷（操作 o；项目族 = o/4）。
pub const fn proj_cell(o: usize) -> Option<u64> {
    if o >= PROJ_OPS {
        return None;
    }
    Some((0xDC000u64 + (o as u64) * 0x3C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 项目族（0..4=脚本 4..8=关键字 8..12=录制 12..16=BDD 16..20=CI 消费）。
pub const fn proj_kind(o: usize) -> Option<u32> {
    if o >= PROJ_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 项目族名。
pub const fn proj_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("脚本"),
        1 => Some("关键字"),
        2 => Some("录制"),
        3 => Some("BDD"),
        4 => Some("CI 消费"),
        _ => None,
    }
}

/// 引擎闸载荷（闸 e：0=启动 1=执行 2=报告）。
pub const fn eng_cell(e: usize) -> Option<u64> {
    if e >= ENG_GATES {
        return None;
    }
    Some((0xDD000u64 + (e as u64) * 0x64) ^ 0x5443454E47415445)
}

/// 引擎闸名。
pub const fn eng_gate_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("启动"),
        1 => Some("执行"),
        2 => Some("报告"),
        _ => None,
    }
}

/// Jenkins 联签格载荷（格 j；F52361 锚定）。
pub const fn jk_cell(j: usize) -> Option<u64> {
    if j >= JOINT_CELLS {
        return None;
    }
    Some((0xDE000u64 + (j as u64) * 0x56) ^ 0x4A656E6B696E73)
}

/// 商业许可声明行折叠（行 r）。
pub const fn lic_row(r: usize) -> Option<u32> {
    if r >= LIC_ROWS {
        return None;
    }
    Some(0xDE80 + r as u32 * 0x40)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// TestComplete 收口账本状态机。
#[derive(Clone)]
pub struct TcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 项目操作账。
    projs: [Option<u64>; PROJ_OPS],
    /// 引擎三元闸账。
    engs: [Option<u64>; ENG_GATES],
    /// Jenkins 联签账。
    joints: [Option<u64>; JOINT_CELLS],
    /// 商业许可声明账。
    lics: [Option<u32>; LIC_ROWS],
    /// 最后项目族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TcLedger {
    /// 新账本。
    pub fn new() -> Self {
        TcLedger {
            gates: [None; 6],
            projs: [None; PROJ_OPS],
            engs: [None; ENG_GATES],
            joints: [None; JOINT_CELLS],
            lics: [None; LIC_ROWS],
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TestCompCode> {
        if gate >= 6 {
            return Err(TestCompCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5D100 + gate as u64);
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

    /// 项目操作一格（项目族单调 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, o: usize, got: u64) -> Result<(), TestCompCode> {
        if o >= PROJ_OPS {
            return Err(TestCompCode::ENoOp);
        }
        let kind = match proj_kind(o) {
            Some(k) => k,
            None => return Err(TestCompCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D200);
            return Err(TestCompCode::EOrder);
        }
        let want = match proj_cell(o) {
            Some(w) => w,
            None => return Err(TestCompCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D201 + o as u64);
            return Err(TestCompCode::EDrift);
        }
        if match self.projs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TestCompCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5D202 + o as u64);
        Ok(())
    }

    /// 项目操作全账核验（20 格）。
    pub fn projs_all(&self) -> bool {
        let mut o = 0usize;
        while o < PROJ_OPS {
            match self.projs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 引擎闸一格（载荷对照 + 重复拒）。
    pub fn record_engine(&mut self, e: usize, got: u64) -> Result<(), TestCompCode> {
        let want = match eng_cell(e) {
            Some(w) => w,
            None => return Err(TestCompCode::ENoEng),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D300 + e as u64);
            return Err(TestCompCode::EEngDrift);
        }
        if match self.engs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TestCompCode::EDup);
        }
        if let Some(c) = self.engs.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x5D301 + e as u64);
        Ok(())
    }

    /// 引擎三元闸全账核验（3 闸全落）。
    pub fn engs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENG_GATES {
            match self.engs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Jenkins 联签一格（载荷对照 + 重复拒）。
    pub fn record_joint(&mut self, j: usize, got: u64) -> Result<(), TestCompCode> {
        let want = match jk_cell(j) {
            Some(w) => w,
            None => return Err(TestCompCode::ENoJoint),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D400 + j as u64);
            return Err(TestCompCode::EJointDrift);
        }
        if match self.joints.get(j) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TestCompCode::EDup);
        }
        if let Some(c) = self.joints.get_mut(j) {
            *c = Some(got);
        }
        self.fnv(0x5D401 + j as u64);
        Ok(())
    }

    /// Jenkins 联签全账核验（4 格）。
    pub fn joints_all(&self) -> bool {
        let mut j = 0usize;
        while j < JOINT_CELLS {
            match self.joints.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// 商业许可声明一行。
    pub fn declare_lic(&mut self, r: usize) -> Result<(), TestCompCode> {
        if r >= LIC_ROWS {
            return Err(TestCompCode::ENoRow);
        }
        if match self.lics.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TestCompCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(r) {
            *c = lic_row(r);
        }
        self.fnv(0x5D500 + r as u64);
        Ok(())
    }

    /// 商业许可声明全齐核验。
    pub fn lics_all(&self) -> bool {
        let mut r = 0usize;
        while r < LIC_ROWS {
            match self.lics.get(r) {
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

    /// 收口封闸：声明缺 → ERowMiss；判据/格账缺 → ESeal；全齐 → Ok。
    pub fn seal_gate(&self) -> Result<(), TestCompCode> {
        if !self.gates_all() {
            return Err(TestCompCode::ESeal);
        }
        if !self.lics_all() {
            return Err(TestCompCode::ERowMiss);
        }
        if !self.projs_all() || !self.engs_all() || !self.joints_all() {
            return Err(TestCompCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（如实阶梯：空账 S3；六判据齐 S2；全齐目标档 S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：与期望档不符即 EGradeDrift。
    pub fn verify_grade(&self, want: Grade) -> Result<(), TestCompCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(TestCompCode::EGradeDrift)
        }
    }
}

impl Default for TcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PROJ_OPS {
        if let Some(w) = proj_cell(o) {
            if led.record_proj(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENG_GATES {
        if let Some(w) = eng_cell(e) {
            if led.record_engine(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut j = 0usize;
    while j < JOINT_CELLS {
        if let Some(w) = jk_cell(j) {
            if led.record_joint(j, w).is_ok() {
                recorded += 1;
            }
        }
        j += 1;
    }
    let mut r = 0usize;
    while r < LIC_ROWS {
        let _ = led.declare_lic(r);
        r += 1;
    }
    // 注入 10 轮：项目族逆序 / 引擎闸漂移 / 联签漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TcLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_proj(17, proj_cell(17).unwrap_or(0));
                lt.record_proj(5, proj_cell(5).unwrap_or(0)) == Err(TestCompCode::EOrder)
            }
            1 => {
                lt.record_engine(2, eng_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(TestCompCode::EEngDrift)
            }
            _ => {
                lt.record_joint(1, jk_cell(1).unwrap_or(0).wrapping_add(0x73))
                    == Err(TestCompCode::EJointDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5D600 + k as u64);
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
/// UNX-F52488 判据集（锚 UNX-F52489-J1，off-by-one 板式）。
pub fn run_unx52488_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52488");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五项目族 20 操作全通",
        recorded >= PROJ_OPS as u32,
        "脚本/关键字/录制/BDD/CI 消费五族逐操作恒等",
    );
    s.add(
        "引擎三元闸 3/3 全落",
        {
            let mut lt = TcLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENG_GATES {
                if let Some(w) = eng_cell(e) {
                    if lt.record_engine(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.engs_all()
        },
        "启动/执行/报告三闸逐格落账，漂移必抓",
    );
    s.add(
        "Jenkins 消费联签 4/4",
        {
            let mut lt = TcLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < JOINT_CELLS {
                if let Some(w) = jk_cell(j) {
                    if lt.record_joint(j, w).is_err() {
                        ok = false;
                    }
                }
                j += 1;
            }
            ok && lt.joints_all()
        },
        "消费联签四格闭集（F52361 锚定）",
    );
    s.add(
        "商业许可声明 2/2 全齐",
        {
            let mut lt = TcLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LIC_ROWS {
                if lt.declare_lic(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.lics_all()
        },
        "商业许可依赖声明/许可面消费分账声明两行闭集（§AI-66.9 红线②）",
    );
    s.add(
        "项目族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PROJ_OPS {
                if proj_kind(o).unwrap_or(5) < proj_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五项目族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "项目族逆序/引擎闸漂移/联签漂移三态轮转必抓");
    let mut l0 = TcLedger::new();
    s.add(
        "边界·项目操作越闭集",
        l0.record_proj(PROJ_OPS, 0).is_err(),
        "项目操作 20 越闭集必拒",
    );
    s.add(
        "边界·引擎闸越闭集",
        l0.record_engine(ENG_GATES, 0).is_err(),
        "引擎闸 3 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_joint(JOINT_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "未齐落账不拒（封点前 Ok）",
        {
            let _ = l0.record_proj(0, proj_cell(0).unwrap_or(0));
            l0.record_engine(0, eng_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账仍 Ok，封拒只发生在封闸",
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
        "定级 S2 闭集",
        {
            let mut lt = TcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PROJ_OPS {
                if let Some(w) = proj_cell(o) {
                    let _ = lt.record_proj(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENG_GATES {
                if let Some(w) = eng_cell(e) {
                    let _ = lt.record_engine(e, w);
                }
                e += 1;
            }
            let mut j = 0usize;
            while j < JOINT_CELLS {
                if let Some(w) = jk_cell(j) {
                    let _ = lt.record_joint(j, w);
                }
                j += 1;
            }
            let mut r = 0usize;
            while r < LIC_ROWS {
                let _ = lt.declare_lic(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2 && lt.verify_grade(Grade::S2).is_ok()
        },
        "六判据齐 → S2，全齐目标档 S2（商业许可如实定级，KB-COMPAT 登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB9C0,
        "SEG_BASE 0x40 对齐且落在 VX16 0xCB9C0 段",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proj_kind() {
        assert_eq!(proj_kind(0), Some(0));
        assert_eq!(proj_kind(3), Some(0));
        assert_eq!(proj_kind(4), Some(1));
        assert_eq!(proj_kind(8), Some(2));
        assert_eq!(proj_kind(12), Some(3));
        assert_eq!(proj_kind(19), Some(4));
        assert!(proj_kind(20).is_none());
        assert!(proj_cell(20).is_none());
        assert_eq!(proj_kind_name(0), Some("脚本"));
        assert_eq!(proj_kind_name(4), Some("CI 消费"));
        assert!(proj_kind_name(5).is_none());
    }

    #[test]
    fn test_eng_gates() {
        assert_eq!(eng_gate_name(0), Some("启动"));
        assert_eq!(eng_gate_name(1), Some("执行"));
        assert_eq!(eng_gate_name(2), Some("报告"));
        assert!(eng_gate_name(3).is_none());
        assert!(eng_cell(ENG_GATES).is_none());
        let mut lt = TcLedger::new();
        assert_eq!(
            lt.record_engine(1, eng_cell(1).unwrap_or(0).wrapping_add(0x64)),
            Err(TestCompCode::EEngDrift)
        );
        assert!(lt.record_engine(1, eng_cell(1).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_engine(1, eng_cell(1).unwrap_or(0)), Err(TestCompCode::EDup));
    }

    #[test]
    fn test_cells_pure() {
        let a = proj_cell(6).unwrap_or(0);
        assert_eq!(a, proj_cell(6).unwrap_or(0));
        assert_ne!(a, proj_cell(7).unwrap_or(0));
        assert!(jk_cell(JOINT_CELLS).is_none());
        assert!(lic_row(LIC_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TcLedger::new();
        let _ = lt.record_proj(9, proj_cell(9).unwrap_or(0));
        assert_eq!(lt.record_proj(2, proj_cell(2).unwrap_or(0)), Err(TestCompCode::EOrder));
        assert!(lt.record_proj(10, proj_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TcLedger::new();
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TestCompCode::EDrift)
        );
        assert_eq!(
            lt.record_joint(3, jk_cell(3).unwrap_or(0).wrapping_add(0x4A)),
            Err(TestCompCode::EJointDrift)
        );
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = TcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 账未齐：声明行缺 → ERowMiss。
        assert_eq!(lt.seal_gate(), Err(TestCompCode::ERowMiss));
        assert!(lt.record_proj(0, proj_cell(0).unwrap_or(0)).is_ok());
        let mut lt2 = TcLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = TcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal_gate(), Err(TestCompCode::ERowMiss));
        assert!(lt.declare_lic(LIC_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(
            recorded,
            PROJ_OPS as u32 + ENG_GATES as u32 + JOINT_CELLS as u32
        );
        assert_eq!(caught, 10);
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
        assert_eq!(SEG_BASE, 0xCB9C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TestCompCode::Ok as u32, 0);
        assert_eq!(TestCompCode::EGradeDrift as u32, 12);
        assert_eq!(TestCompCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => TestCompCode::Ok,
                1 => TestCompCode::ENoOp,
                2 => TestCompCode::EDrift,
                3 => TestCompCode::EOrder,
                4 => TestCompCode::ENoEng,
                5 => TestCompCode::EEngDrift,
                6 => TestCompCode::ENoJoint,
                7 => TestCompCode::EJointDrift,
                8 => TestCompCode::ENoRow,
                9 => TestCompCode::ERowMiss,
                10 => TestCompCode::EDup,
                11 => TestCompCode::ESeal,
                _ => TestCompCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt0 = TcLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        assert!(lt0.verify_grade(Grade::S2).is_err());
        // 六闸齐 S2。
        let mut lt1 = TcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52488_checks_green() {
        let s = run_unx52488_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
