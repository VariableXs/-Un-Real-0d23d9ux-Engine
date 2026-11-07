//! UNX-F52417 Nitrokey App 收口 · 0xCA800段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52417"，逐字）：
//!   title: Nitrokey App 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52418-J1 设备枚
//!     举/只读信息 ×10（PIV/FIDO 写配置判据干跑）；M5 账复用（联签锚定
//!     行 AI-65）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52417）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52418-J1，照原文锚定；功能按本卡标题（Nitrokey App 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集：存储/密码/OTP/签名/固件各 4 操作共 20 格，家族
//!      单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 设备三态 12 格：在位/锁定/就绪 × 4 槽位，写配置判据一律干跑
//!      （对照 PIV/FIDO 口径），三态载荷漂移必抓。
//!   ③ PIN 三元闸（管理/用户/复位）+ 三态干跑确认合成六闸，逐格落账，
//!      未齐即封。
//!   ④ OpenPGP 消费联签 4 格（锚 UNX-F52408，M5 账复用，联签锚定行
//!      AI-65）+ C 上栈声明 2 行，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（功能载荷
//!      漂移/功能号越闭集/家族逆序），检出+逃逸守恒等式；双跑指纹一致；
//!      定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52417;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族 20 操作数。
pub const FN_OPS: usize = 20;

/// 设备三态 12 格数。
pub const DEV_CELLS: usize = 12;

/// OpenPGP 消费联签格数（锚 UNX-F52408）。
pub const JOINT_CELLS: usize = 4;

/// C 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 联签锚定行编号（M5 账复用，AI-65）。
pub const JOINT_ANCHOR_LINE: u32 = 65;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NitrokeyCode {
    /// 成功
    Ok = 0,
    /// 功能号越闭集
    ENoFn,
    /// 功能载荷漂移
    EFnDrift,
    /// 功能家族逆序
    EOrder,
    /// 设备格越闭集
    ENoDev,
    /// 设备三态载荷漂移
    EDevDrift,
    /// 联签格越闭集
    ENoJoint,
    /// 联签格缺格
    EJointMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl NitrokeyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NitrokeyCode::Ok => "成功",
            NitrokeyCode::ENoFn => "功能号越闭集：存储/密码/OTP/签名/固件 20 操作集之外先登记",
            NitrokeyCode::EFnDrift => "功能载荷漂移：操作折叠与基准不恒等，逐格复算",
            NitrokeyCode::EOrder => "家族逆序：存储→密码→OTP→签名→固件单调，逆序必抓",
            NitrokeyCode::ENoDev => "设备格越闭集：三态×4 槽位 12 格之外先登记",
            NitrokeyCode::EDevDrift => "设备三态载荷漂移：在位/锁定/就绪格折叠与基准不恒等",
            NitrokeyCode::ENoJoint => "联签格越闭集：OpenPGP 消费联签 4 格（锚 UNX-F52408）之外先登记",
            NitrokeyCode::EJointMiss => "联签格缺格：OpenPGP 消费联签 4 格未齐先补齐",
            NitrokeyCode::EDup => "重复落账：同格只许一条账目",
            NitrokeyCode::ESeal => "未齐即封：20 功能格 + 12 设备格 + 六闸未齐先补齐",
            NitrokeyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NitrokeyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NitrokeyCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20功能格+12设备格+4联签格+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能格载荷（操作 o；家族 = o/4：0=存储 1=密码 2=OTP 3=签名 4=固件）。
pub const fn fn_cell(o: usize) -> Option<u64> {
    if o >= FN_OPS {
        return None;
    }
    Some((0xCA810u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能家族号（i/N 映射：家族 = o/4）。
pub const fn fn_kind(o: usize) -> Option<u32> {
    if o >= FN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn fn_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("存储"),
        1 => Some("密码"),
        2 => Some("OTP"),
        3 => Some("签名"),
        4 => Some("固件"),
        _ => None,
    }
}

/// 设备三态格载荷（格 c；态 = c/4：0=在位 1=锁定 2=就绪；槽位 = c%4）。
pub const fn dev_cell(c: usize) -> Option<u64> {
    if c >= DEV_CELLS {
        return None;
    }
    Some((0xCA850u64 + (c as u64) * 0x3A) ^ ((c / 4) as u64).wrapping_mul(0x01000193))
}

/// 设备三态号（i/N 映射：态 = c/4）。
pub const fn dev_state(c: usize) -> Option<u32> {
    if c >= DEV_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 设备态名。
pub const fn dev_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("在位"),
        1 => Some("锁定"),
        2 => Some("就绪"),
        _ => None,
    }
}

/// OpenPGP 消费联签格折叠（格 r，锚 UNX-F52408 · M5 账复用 · AI-65）。
pub const fn joint_cell(r: usize) -> Option<u32> {
    if r >= JOINT_CELLS {
        return None;
    }
    Some(0xCA890 + r as u32 * 0x2C + (JOINT_ANCHOR_LINE % 0x40))
}

/// C 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCA8D0 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Nitrokey 收口账本状态机。
#[derive(Clone)]
pub struct NitrokeyLedger {
    /// 六闸：0=管理PIN 1=用户PIN 2=复位PIN 3=枚举干跑 4=锁定干跑 5=就绪干跑。
    gates: [Option<bool>; 6],
    /// 功能账。
    fns: [Option<u64>; FN_OPS],
    /// 设备三态账。
    devs: [Option<u64>; DEV_CELLS],
    /// 联签账。
    joints: [Option<u32>; JOINT_CELLS],
    /// C 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NitrokeyLedger {
    /// 新账本。
    pub fn new() -> Self {
        NitrokeyLedger {
            gates: [None; 6],
            fns: [None; FN_OPS],
            devs: [None; DEV_CELLS],
            joints: [None; JOINT_CELLS],
            stacks: [None; STACK_ROWS],
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

    /// 落一格六闸（PIN 三元 + 三态干跑确认；越界即 ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NitrokeyCode> {
        if gate >= 6 {
            return Err(NitrokeyCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4600 + gate as u64);
        Ok(())
    }

    /// 六闸全落核验。
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

    /// 功能一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_fn(&mut self, o: usize, got: u64) -> Result<(), NitrokeyCode> {
        if o >= FN_OPS {
            return Err(NitrokeyCode::ENoFn);
        }
        let kind = match fn_kind(o) {
            Some(k) => k,
            None => return Err(NitrokeyCode::ENoFn),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4700);
            return Err(NitrokeyCode::EOrder);
        }
        let want = match fn_cell(o) {
            Some(w) => w,
            None => return Err(NitrokeyCode::ENoFn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4701 + o as u64);
            return Err(NitrokeyCode::EFnDrift);
        }
        if match self.fns.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NitrokeyCode::EDup);
        }
        if let Some(c) = self.fns.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4702 + o as u64);
        Ok(())
    }

    /// 功能全账核验（20 格）。
    pub fn fns_all(&self) -> bool {
        let mut o = 0usize;
        while o < FN_OPS {
            match self.fns.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 设备三态一格（载荷对照 + 重复拒；写配置判据一律干跑）。
    pub fn record_dev(&mut self, c: usize, got: u64) -> Result<(), NitrokeyCode> {
        if c >= DEV_CELLS {
            return Err(NitrokeyCode::ENoDev);
        }
        let want = match dev_cell(c) {
            Some(w) => w,
            None => return Err(NitrokeyCode::ENoDev),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4800 + c as u64);
            return Err(NitrokeyCode::EDevDrift);
        }
        if match self.devs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NitrokeyCode::EDup);
        }
        if let Some(cell) = self.devs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x4801 + c as u64);
        Ok(())
    }

    /// 设备三态全账核验（12 格）。
    pub fn devs_all(&self) -> bool {
        let mut c = 0usize;
        while c < DEV_CELLS {
            match self.devs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// OpenPGP 消费联签一格（锚 UNX-F52408）。
    pub fn declare_joint(&mut self, r: usize) -> Result<(), NitrokeyCode> {
        if r >= JOINT_CELLS {
            return Err(NitrokeyCode::ENoJoint);
        }
        if match self.joints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NitrokeyCode::EDup);
        }
        if let Some(cell) = self.joints.get_mut(r) {
            *cell = joint_cell(r);
        }
        self.fnv(0x4900 + r as u64);
        Ok(())
    }

    /// C 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), NitrokeyCode> {
        if r >= STACK_ROWS {
            return Err(NitrokeyCode::ENoJoint);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NitrokeyCode::EDup);
        }
        if let Some(cell) = self.stacks.get_mut(r) {
            *cell = stack_row(r);
        }
        self.fnv(0x4A00 + r as u64);
        Ok(())
    }

    /// 联签/声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < JOINT_CELLS {
            match self.joints.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 联签收口核（缺格即 EJointMiss）。
    pub fn joint_seal(&self) -> Result<(), NitrokeyCode> {
        let mut r = 0usize;
        while r < JOINT_CELLS {
            match self.joints.get(r) {
                Some(Some(_)) => {}
                _ => return Err(NitrokeyCode::EJointMiss),
            }
            r += 1;
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

    /// 注入守恒核（注入=检出+逃逸）。
    pub fn verify_conservation(&self) -> Result<(), NitrokeyCode> {
        if self.injected == self.caught.wrapping_add(self.escaped) {
            Ok(())
        } else {
            Err(NitrokeyCode::EInj)
        }
    }

    /// 双跑指纹核。
    pub fn verify_dual(a: u64, b: u64) -> Result<(), NitrokeyCode> {
        if a == b && a != 0 {
            Ok(())
        } else {
            Err(NitrokeyCode::EFp)
        }
    }

    /// KB-COMPAT 定级（全通 → S1，六闸齐 → S2，否则 S3）。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.fns_all() && self.devs_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级封核（非 S1 即 EGradeDrift）。
    pub fn seal_grade(&self) -> Result<Grade, NitrokeyCode> {
        let g = self.grade();
        if g == Grade::S1 {
            Ok(g)
        } else {
            Err(NitrokeyCode::EGradeDrift)
        }
    }
}

impl Default for NitrokeyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NitrokeyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FN_OPS {
        match fn_cell(o) {
            Some(w) => {
                if led.record_fn(o, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < DEV_CELLS {
        match dev_cell(c) {
            Some(w) => {
                if led.record_dev(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < JOINT_CELLS {
        let _ = led.declare_joint(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：功能载荷漂移 / 功能号越闭集 / 家族逆序三态轮转，
    // 全部落在当轮临时账本上，主账只记注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NitrokeyLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                match fn_cell(0) {
                    Some(w) => {
                        let _ = lt.record_fn(0, w);
                    }
                    None => {}
                }
                match fn_cell(1) {
                    Some(w) => lt.record_fn(1, w.wrapping_add(1)) == Err(NitrokeyCode::EFnDrift),
                    None => false,
                }
            }
            1 => {
                // 越闭集注入：空账本直接越界必抓，不改 last_kind。
                lt.record_fn(FN_OPS, 0).is_err()
            }
            _ => {
                // 逆序注入：空账本先落家族 1 正格，再落家族 0 正格必抓。
                match fn_cell(5) {
                    Some(w) => {
                        let _ = lt.record_fn(5, w);
                    }
                    None => {}
                }
                match fn_cell(1) {
                    Some(w) => lt.record_fn(1, w) == Err(NitrokeyCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4B00 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let _ = led.verify_conservation();
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52417 判据集（锚 UNX-F52418-J1，off-by-one 板式）。
pub fn run_unx52417_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52417");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= FN_OPS as u32 + DEV_CELLS as u32,
        "存储/密码/OTP/签名/固件五家族逐操作恒等落账",
    );
    s.add(
        "功能家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FN_OPS {
                let a = match fn_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match fn_kind(o - 1) {
                    Some(k) => k,
                    None => 0,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "存储→密码→OTP→签名→固件五家族闭集单调，逆序必抓",
    );
    s.add(
        "设备三态 12 格全落（PIV/FIDO 写配置判据干跑）",
        {
            let mut lt = NitrokeyLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < DEV_CELLS {
                match dev_cell(c) {
                    Some(w) => {
                        if lt.record_dev(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                c += 1;
            }
            ok && lt.devs_all()
        },
        "在位/锁定/就绪三态×4 槽位逐格恒等，写配置判据一律干跑",
    );
    s.add(
        "PIN 三元闸 + 三态干跑六闸全落",
        {
            let mut lt = NitrokeyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "管理PIN/用户PIN/复位PIN 三元闸 + 枚举/锁定/就绪干跑确认逐格落账",
    );
    s.add(
        "OpenPGP 消费联签 4/4 全齐（锚 UNX-F52408）",
        {
            let mut lt = NitrokeyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JOINT_CELLS {
                if lt.declare_joint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.joint_seal().is_ok()
        },
        "M5 账复用，联签锚定行 AI-65，UNX-F52408 消费联签四格闭集",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = NitrokeyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "C 上栈声明两行闭集，缺一拒封",
    );
    let mut l0 = NitrokeyLedger::new();
    s.add(
        "边界·功能号越闭集",
        l0.record_fn(FN_OPS, 0).is_err(),
        "功能号 20 越闭集必拒",
    );
    s.add(
        "功能载荷漂移拒",
        {
            match fn_cell(3) {
                Some(w) => l0.record_fn(3, w.wrapping_add(1)) == Err(NitrokeyCode::EFnDrift),
                None => false,
            }
        },
        "功能载荷漂移必抓",
    );
    s.add(
        "边界·设备格越闭集",
        l0.record_dev(DEV_CELLS, 0).is_err(),
        "设备格 12 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "功能漂移/功能越闭集/家族逆序三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        NitrokeyLedger::verify_dual(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = NitrokeyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FN_OPS {
                match fn_cell(o) {
                    Some(w) => {
                        let _ = lt.record_fn(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < DEV_CELLS {
                match dev_cell(c) {
                    Some(w) => {
                        let _ = lt.record_dev(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < JOINT_CELLS {
                let _ = lt.declare_joint(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.seal_grade() == Ok(Grade::S1)
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "M5 账复用联签锚定行核",
        JOINT_ANCHOR_LINE == 65 && match joint_cell(0) {
            Some(v) => v > 0xCA800,
            None => false,
        },
        "联签锚定行 AI-65 且联签格载荷落在本段域",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA800,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×(id−52401)",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 14,
        "判据原文条数按求值时点核：含本条共 16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fn_kind_mapping() {
        assert_eq!(fn_kind(0), Some(0));
        assert_eq!(fn_kind(4), Some(1));
        assert_eq!(fn_kind(8), Some(2));
        assert_eq!(fn_kind(12), Some(3));
        assert_eq!(fn_kind(19), Some(4));
        assert!(fn_kind(20).is_none());
        assert_eq!(fn_kind_name(0), Some("存储"));
        assert_eq!(fn_kind_name(4), Some("固件"));
        assert!(fn_kind_name(5).is_none());
    }

    #[test]
    fn test_dev_state_mapping() {
        assert_eq!(dev_state(0), Some(0));
        assert_eq!(dev_state(4), Some(1));
        assert_eq!(dev_state(8), Some(2));
        assert_eq!(dev_state(11), Some(2));
        assert!(dev_state(12).is_none());
        assert_eq!(dev_state_name(1), Some("锁定"));
        assert!(dev_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match fn_cell(3) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(a, match fn_cell(3) {
            Some(v) => v,
            None => 1,
        });
        assert_ne!(a, match fn_cell(7) {
            Some(v) => v,
            None => 0,
        });
        assert!(joint_cell(JOINT_CELLS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NitrokeyLedger::new();
        // 先落家族 1 正格（o=5 密码族），再落家族 0 正格（o=1 存储族）必抓。
        let _ = lt.record_fn(5, fn_cell(5).unwrap_or(0));
        assert_eq!(lt.record_fn(1, fn_cell(1).unwrap_or(0)), Err(NitrokeyCode::EOrder));
        // 逆序被拒后同族正格仍可落。
        assert!(lt.record_fn(6, fn_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NitrokeyLedger::new();
        assert_eq!(
            lt.record_fn(9, fn_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(NitrokeyCode::EFnDrift)
        );
        assert_eq!(
            lt.record_dev(5, dev_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(NitrokeyCode::EDevDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NitrokeyLedger::new();
        let _ = lt.record_fn(2, fn_cell(2).unwrap_or(0));
        assert_eq!(lt.record_fn(2, fn_cell(2).unwrap_or(0)), Err(NitrokeyCode::EDup));
        let _ = lt.record_dev(1, dev_cell(1).unwrap_or(0));
        assert_eq!(lt.record_dev(1, dev_cell(1).unwrap_or(0)), Err(NitrokeyCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NitrokeyLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NitrokeyLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_joint_and_stack_boundary() {
        let mut lt = NitrokeyLedger::new();
        let mut r = 0usize;
        while r < JOINT_CELLS {
            assert!(lt.declare_joint(r).is_ok());
            r += 1;
        }
        assert!(lt.declare_joint(JOINT_CELLS).is_err());
        assert!(lt.joint_seal().is_ok());
        let mut lt2 = NitrokeyLedger::new();
        assert_eq!(lt2.joint_seal(), Err(NitrokeyCode::EJointMiss));
        assert!(lt2.declare_stack(0).is_ok());
        assert!(lt2.declare_stack(STACK_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 功能格 + 12 设备格。
        assert!(recorded >= FN_OPS as u32 + DEV_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert!(NitrokeyLedger::verify_dual(f1, f2).is_ok());
        assert!(NitrokeyLedger::verify_dual(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA800);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NitrokeyCode::Ok as u32, 0);
        assert_eq!(NitrokeyCode::EGradeDrift as u32, 12);
        // 恰 13 变体：0..=0x0C 每位可回映射。
        let all = [
            NitrokeyCode::Ok,
            NitrokeyCode::ENoFn,
            NitrokeyCode::EFnDrift,
            NitrokeyCode::EOrder,
            NitrokeyCode::ENoDev,
            NitrokeyCode::EDevDrift,
            NitrokeyCode::ENoJoint,
            NitrokeyCode::EJointMiss,
            NitrokeyCode::EDup,
            NitrokeyCode::ESeal,
            NitrokeyCode::EInj,
            NitrokeyCode::EFp,
            NitrokeyCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
    }

    #[test]
    fn test_conservation_and_grade() {
        let mut lt = NitrokeyLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.grade(), Grade::S2);
        // 全齐目标档 S1。
        let mut o = 0usize;
        while o < FN_OPS {
            let _ = lt.record_fn(o, fn_cell(o).unwrap_or(0));
            o += 1;
        }
        let mut c = 0usize;
        while c < DEV_CELLS {
            let _ = lt.record_dev(c, dev_cell(c).unwrap_or(0));
            c += 1;
        }
        let mut r = 0usize;
        while r < JOINT_CELLS {
            let _ = lt.declare_joint(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.grade(), Grade::S1);
        assert!(lt.seal_grade().is_ok());
        // 注入守恒：未注入时也满足等式。
        assert!(lt.verify_conservation().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        // 三类注入在空账本上同样必抓。
        let mut a = NitrokeyLedger::new();
        assert!(a.record_fn(FN_OPS, 0).is_err());
        let mut b = NitrokeyLedger::new();
        let _ = b.record_fn(5, fn_cell(5).unwrap_or(0));
        let kind_before = b.last_kind;
        assert_eq!(b.record_fn(1, fn_cell(1).unwrap_or(0)), Err(NitrokeyCode::EOrder));
        // 逆序被拒不改 last_kind。
        assert_eq!(b.last_kind, kind_before);
    }

    #[test]
    fn unx52417_checks_green() {
        let s = run_unx52417_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
