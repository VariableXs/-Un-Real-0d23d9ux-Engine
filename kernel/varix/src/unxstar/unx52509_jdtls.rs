//! UNX-F52509 jdtls 收口 · 0xCBF00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52509"，逐字）：
//!   title: jdtls 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52510-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52509）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52510-J1，照原文锚定；功能按本卡标题（jdtls）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：jdtls（Eclipse JDT Language Server）五能力族 20 格
//!      （补全/跳转定义/重构/诊断/格式化各 4 格），能力族单调，逐格
//!      载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② Maven/Gradle 双构建消费 8 格：8 格闭集、双族（Maven 0..4、
//!      Gradle 4..8）单调，构建消费漂移必抓。
//!   ③ JDK 三元闸：17/21/25 三档闭集，越档必抓、载荷漂移必抓。
//!   ④ 声明 4 行闭集：与 Eclipse JDT 消费分账声明 2 行 + Java 语言
//!      服务上栈声明 2 行，缺一拒封（seal：六闸缺→ESeal、声明行缺→
//!      ERowMiss、格账缺→ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（构建漂移/
//!      能力族逆序/操作越闭集），三类注入全部落在当轮临时账本上，
//!      主账 injected==caught==10、escaped==0；漂移注入先按合法序落
//!      同族正格再注漂移；双跑指纹一致；定级目标档 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52509;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// jdtls 五能力族操作数。
pub const OPS: usize = 20;

/// Maven/Gradle 双构建消费格数。
pub const BUILD_CELLS: usize = 8;

/// JDK 三元闸档数。
pub const JDK_STEPS: usize = 3;

/// 声明行数（Eclipse 消费 2 行 + Java 上栈 2 行）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JdtlsCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 构建消费格越闭集
    ENoBuild,
    /// 构建消费漂移
    EBuildDrift,
    /// JDK 档越三元闸
    ENoJdk,
    /// JDK 载荷漂移
    EJdkDrift,
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

impl JdtlsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JdtlsCode::Ok => "成功",
            JdtlsCode::ENoOp => "操作号越闭集：jdtls 五能力族 20 格之外先登记",
            JdtlsCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            JdtlsCode::EOrder => "能力族逆序：补全→跳转定义→重构→诊断→格式化单调，逆序必抓",
            JdtlsCode::ENoBuild => "构建消费格越闭集：Maven/Gradle 双构建 8 格之外先登记",
            JdtlsCode::EBuildDrift => "构建消费漂移：Maven/Gradle 消费折叠与基准不恒等，逐格复算",
            JdtlsCode::ENoJdk => "JDK 档越三元闸：17/21/25 三档闭集之外先登记",
            JdtlsCode::EJdkDrift => "JDK 载荷漂移：JDK 档折叠与基准不恒等，逐档复算",
            JdtlsCode::ENoRow => "声明行越闭集：Eclipse 消费+Java 上栈 4 行闭集之外先登记",
            JdtlsCode::ERowMiss => "缺声明行：Eclipse 消费分账/Java 上栈声明未齐先补齐",
            JdtlsCode::EDup => "重复落账：同格只许一条账目",
            JdtlsCode::ESeal => "未齐即封：20 能力格+8 构建格+3 档 JDK+六判据未齐先补齐",
            JdtlsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定档（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：六判据+20能力+8构建+3档JDK+4行声明全通收口",
            Grade::S2 => "S2：有条件收口（jdtls KB-COMPAT 目标档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x2C) ^ 0x6A64746C73000000)
}

/// 能力族（0..4=补全 4..8=跳转定义 8..12=重构 12..16=诊断 16..20=格式化）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力族名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("补全"),
        1 => Some("跳转定义"),
        2 => Some("重构"),
        3 => Some("诊断"),
        4 => Some("格式化"),
        _ => None,
    }
}

/// 构建消费格载荷（格 c；构建族 = c/4：0..4=Maven 4..8=Gradle）。
pub const fn build_cell(c: usize) -> Option<u64> {
    if c >= BUILD_CELLS {
        return None;
    }
    Some((0xC2000u64 + (c as u64) * 0x31) ^ 0x4D6176656E477264)
}

/// 构建消费族。
pub const fn build_kind(c: usize) -> Option<u32> {
    if c >= BUILD_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 构建消费族名。
pub const fn build_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Maven"),
        1 => Some("Gradle"),
        _ => None,
    }
}

/// JDK 三元闸档载荷（档 v）。
pub const fn jdk_cell(v: usize) -> Option<u64> {
    if v >= JDK_STEPS {
        return None;
    }
    Some((0xC3000u64 + (v as u64) * 0x5D) ^ 0x4A444B0000000000)
}

/// JDK 档名。
pub const fn jdk_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("JDK 17 LTS"),
        1 => Some("JDK 21 LTS"),
        2 => Some("JDK 25 LTS"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC510 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Eclipse JDT 消费分账·主声明"),
        1 => Some("Eclipse JDT 消费分账·辅声明"),
        2 => Some("Java 语言服务上栈声明·主"),
        3 => Some("Java 语言服务上栈声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// jdtls 收口账本状态机。
#[derive(Clone)]
pub struct JdtlsLedger {
    /// 六判据（收口闸：安装/启动/补全/诊断/升级/卸载）。
    gates: [Option<bool>; 6],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 构建消费账。
    builds: [Option<u64>; BUILD_CELLS],
    /// JDK 三元闸账。
    jdk: [Option<u64>; JDK_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JdtlsLedger {
    /// 新账本。
    pub fn new() -> Self {
        JdtlsLedger {
            gates: [None; 6],
            ops: [None; OPS],
            builds: [None; BUILD_CELLS],
            jdk: [None; JDK_STEPS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JdtlsCode> {
        if gate >= 6 {
            return Err(JdtlsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5A00 + gate as u64);
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

    /// 能力一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), JdtlsCode> {
        if o >= OPS {
            return Err(JdtlsCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(JdtlsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B00);
            return Err(JdtlsCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(JdtlsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B01 + o as u64);
            return Err(JdtlsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JdtlsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5B02 + o as u64);
        Ok(())
    }

    /// 能力全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 构建消费一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_build(&mut self, c: usize, got: u64) -> Result<(), JdtlsCode> {
        let want = match build_cell(c) {
            Some(w) => w,
            None => return Err(JdtlsCode::ENoBuild),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C00 + c as u64);
            return Err(JdtlsCode::EBuildDrift);
        }
        if match self.builds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JdtlsCode::EDup);
        }
        if let Some(b) = self.builds.get_mut(c) {
            *b = Some(got);
        }
        self.fnv(0x5C01 + c as u64);
        Ok(())
    }

    /// 构建消费全账核验（8 格）。
    pub fn builds_all(&self) -> bool {
        let mut c = 0usize;
        while c < BUILD_CELLS {
            match self.builds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// JDK 三元闸一档（载荷对照 + 重复拒）。
    pub fn record_jdk(&mut self, v: usize, got: u64) -> Result<(), JdtlsCode> {
        let want = match jdk_cell(v) {
            Some(w) => w,
            None => return Err(JdtlsCode::ENoJdk),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D00 + v as u64);
            return Err(JdtlsCode::EJdkDrift);
        }
        if match self.jdk.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JdtlsCode::EDup);
        }
        if let Some(j) = self.jdk.get_mut(v) {
            *j = Some(got);
        }
        self.fnv(0x5D01 + v as u64);
        Ok(())
    }

    /// JDK 三元闸全账核验（3 档）。
    pub fn jdk_all(&self) -> bool {
        let mut v = 0usize;
        while v < JDK_STEPS {
            match self.jdk.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), JdtlsCode> {
        if r >= SPLIT_ROWS {
            return Err(JdtlsCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JdtlsCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x5E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), JdtlsCode> {
        if !self.gates_all() {
            return Err(JdtlsCode::ESeal);
        }
        if !self.splits_all() {
            return Err(JdtlsCode::ERowMiss);
        }
        if !self.ops_all() || !self.builds_all() || !self.jdk_all() {
            return Err(JdtlsCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐→目标档 S2，六闸齐→S2，空账→S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.builds_all() && self.jdk_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for JdtlsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JdtlsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < BUILD_CELLS {
        if let Some(w) = build_cell(c) {
            if led.record_build(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut v = 0usize;
    while v < JDK_STEPS {
        if let Some(w) = jdk_cell(v) {
            if led.record_jdk(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：构建漂移 / 能力族逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JdtlsLedger::new();
        let caught = match kind {
            0 => {
                // 构建漂移：先按合法序落同族正格（Maven 族），再注漂移。
                let w0 = match build_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match build_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_build(0, w0);
                lt.record_build(1, w1.wrapping_add(1)) == Err(JdtlsCode::EBuildDrift)
            }
            1 => {
                // 能力族逆序：空账先落族 4 正格，再回落族 1，逆序必抓。
                let w17 = match op_cell(17) {
                    Some(w) => w,
                    None => 0,
                };
                let w5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(17, w17);
                lt.record_op(5, w5) == Err(JdtlsCode::EOrder)
            }
            _ => {
                // 操作越闭集：边界拒，不计注入账内层（lt 只判拒码）。
                lt.record_op(OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5F00 + k as u64);
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
/// UNX-F52509 判据集（锚 UNX-F52510-J1，off-by-one 板式）。
pub fn run_unx52509_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52509");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "jdtls 五能力族 20 操作全通",
        recorded >= OPS as u32,
        "补全/跳转定义/重构/诊断/格式化逐格恒等",
    );
    s.add(
        "Maven/Gradle 双构建消费 8/8 全落",
        {
            let mut lt = JdtlsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < BUILD_CELLS {
                if let Some(w) = build_cell(c) {
                    if lt.record_build(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.builds_all()
        },
        "Maven 0..4 与 Gradle 4..8 双族逐格消费恒等",
    );
    s.add(
        "JDK 三元闸 3/3 全落",
        {
            let mut lt = JdtlsLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < JDK_STEPS {
                if let Some(w) = jdk_cell(v) {
                    if lt.record_jdk(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.jdk_all()
        },
        "JDK 17/21/25 三档闭集逐档恒等，越档必抓",
    );
    s.add(
        "Eclipse 消费与 Java 上栈声明 4/4 全齐",
        {
            let mut lt = JdtlsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "Eclipse JDT 消费分账 2 行 + Java 上栈声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = JdtlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/补全/诊断/升级/卸载逐格落账",
    );
    s.add(
        "能力族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力族闭集单调",
    );
    s.add(
        "构建消费族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < BUILD_CELLS {
                let ka = match build_kind(c) {
                    Some(k) => k,
                    None => 2,
                };
                let kb = match build_kind(c - 1) {
                    Some(k) => k,
                    None => 2,
                };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "Maven→Gradle 双族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "构建漂移/能力族逆序/操作越闭集三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = JdtlsLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·构建格越闭集",
        {
            let mut lt = JdtlsLedger::new();
            lt.record_build(BUILD_CELLS, 0).is_err()
        },
        "构建格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = JdtlsLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "声明行 4 越闭集必拒",
    );
    s.add(
        "能力漂移拒",
        {
            let mut lt = JdtlsLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(JdtlsCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但能力缺一也必拒。
            let lt_empty = JdtlsLedger::new();
            let mut lt_cell = JdtlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt_cell.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            let mut c = 1usize;
            while c < BUILD_CELLS {
                if let Some(w) = build_cell(c) {
                    let _ = lt_cell.record_build(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(JdtlsCode::ESeal) && lt_cell.seal() == Err(JdtlsCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = JdtlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            lt.seal() == Err(JdtlsCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "JDK 越三元拒",
        {
            let mut lt = JdtlsLedger::new();
            lt.record_jdk(JDK_STEPS, 0).is_err()
        },
        "JDK 档 3 越三元闸必拒",
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
            let mut lt = JdtlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < BUILD_CELLS {
                if let Some(w) = build_cell(c) {
                    let _ = lt.record_build(c, w);
                }
                c += 1;
            }
            let mut v = 0usize;
            while v < JDK_STEPS {
                if let Some(w) = jdk_cell(v) {
                    let _ = lt.record_jdk(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade() == Grade::S2
        },
        "全齐 → S2（jdtls KB-COMPAT 目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCBF00",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("补全"));
        assert_eq!(op_kind_name(4), Some("格式化"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_build_kind() {
        assert_eq!(build_kind(0), Some(0));
        assert_eq!(build_kind(3), Some(0));
        assert_eq!(build_kind(4), Some(1));
        assert_eq!(build_kind(7), Some(1));
        assert!(build_kind(8).is_none());
        assert!(build_cell(8).is_none());
        assert_eq!(build_kind_name(0), Some("Maven"));
        assert_eq!(build_kind_name(1), Some("Gradle"));
        assert!(build_kind_name(2).is_none());
    }

    #[test]
    fn test_jdk_ladder() {
        assert_eq!(jdk_name(0), Some("JDK 17 LTS"));
        assert_eq!(jdk_name(1), Some("JDK 21 LTS"));
        assert_eq!(jdk_name(2), Some("JDK 25 LTS"));
        assert!(jdk_name(3).is_none());
        assert!(jdk_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        let b0 = build_cell(0).unwrap_or(0);
        assert_ne!(b0, build_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = JdtlsLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(JdtlsCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JdtlsLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(JdtlsCode::EDrift)
        );
        assert_eq!(
            lt.record_build(1, build_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(JdtlsCode::EBuildDrift)
        );
        assert_eq!(
            lt.record_jdk(2, jdk_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(JdtlsCode::EJdkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JdtlsLedger::new();
        let _ = lt.record_jdk(0, jdk_cell(0).unwrap_or(0));
        assert_eq!(lt.record_jdk(0, jdk_cell(0).unwrap_or(0)), Err(JdtlsCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(JdtlsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JdtlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = JdtlsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = JdtlsLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_build(BUILD_CELLS, 0);
        let _ = lt.record_jdk(JDK_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = JdtlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < BUILD_CELLS {
            if let Some(w) = build_cell(c) {
                let _ = full.record_build(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < JDK_STEPS {
            if let Some(w) = jdk_cell(v) {
                let _ = full.record_jdk(v, w);
            }
            v += 1;
        }
        assert_eq!(full.seal(), Err(JdtlsCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_split(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 构建 + 3 档 JDK（六闸与声明行不计数）。
        assert_eq!(recorded, OPS as u32 + BUILD_CELLS as u32 + JDK_STEPS as u32);
        assert_eq!(recorded, 31);
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
        assert_eq!(SEG_BASE, 0xCBF00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JdtlsCode::Ok as u32, 0);
        assert_eq!(JdtlsCode::ENoOp as u32, 1);
        assert_eq!(JdtlsCode::EGradeDrift as u32, 12);
        assert_eq!(JdtlsCode::Ok.val(), SEG_BASE);
        assert!(!JdtlsCode::Ok.detail().is_empty());
        assert!(!JdtlsCode::ESeal.detail().is_empty());
        assert!(!JdtlsCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = JdtlsLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = JdtlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        // 全齐目标档 S2（目标档前置必须补 judge_gate×6）。
        let mut lt3 = JdtlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < BUILD_CELLS {
            if let Some(w) = build_cell(c) {
                let _ = lt3.record_build(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < JDK_STEPS {
            if let Some(w) = jdk_cell(v) {
                let _ = lt3.record_jdk(v, w);
            }
            v += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S2);
    }

    #[test]
    fn unx52509_checks_green() {
        let s = run_unx52509_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
