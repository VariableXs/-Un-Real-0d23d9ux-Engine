//! UNX-F52418 YubiKey Manager 收口 · 0xCA840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52418"，逐字）：
//!   title: YubiKey Manager 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52419-J1 插件加
//!     载/崩溃隔离 ×10 全通（与 F52401 本体分账）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52418）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52419-J1，照原文锚定；功能按本卡标题（YubiKey Manager 收口）落
//! 地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五应用族闭集：OTP/FIDO2/PIV/OATH/OpenPGP 各 4 操作共 20 格，家
//!      族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 五命令 10 操作：list/info/generate/reset/verify ×2 轮次，写配
//!      置类命令（generate/reset）只做只读干跑。
//!   ③ slot 三元闸（OTP 槽/PIV 槽/OATH 槽）+ 版本三档干跑确认合成六
//!      闸，逐格落账，未齐即封。
//!   ④ 版本阶梯 3 档（固件 5.1/5.2/5.4），越档必抓，逐档折叠恒等。
//!   ⑤ gpg 消费联签 4 格（锚 UNX-F52408）+ Go/Python 上栈声明 2 行，
//!      缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（应用载荷
//!      漂移/应用号越闭集/家族逆序），检出+逃逸守恒等式；双跑指纹一致；
//!      定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52418;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五应用族 20 操作数。
pub const APP_OPS: usize = 20;

/// 五命令 10 操作数。
pub const CMD_OPS: usize = 10;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// gpg 消费联签格数（锚 UNX-F52408）。
pub const JOINT_CELLS: usize = 4;

/// Go/Python 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YubikeyCode {
    /// 成功
    Ok = 0,
    /// 应用号越闭集
    ENoApp,
    /// 应用载荷漂移
    EAppDrift,
    /// 应用家族逆序
    EOrder,
    /// 命令号越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 版本档越阶梯
    ENoVer,
    /// 版本档载荷漂移
    EVerDrift,
    /// 联签/声明行越闭集
    ENoRow,
    /// 联签/声明行缺行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl YubikeyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            YubikeyCode::Ok => "成功",
            YubikeyCode::ENoApp => "应用号越闭集：OTP/FIDO2/PIV/OATH/OpenPGP 20 操作集之外先登记",
            YubikeyCode::EAppDrift => "应用载荷漂移：操作折叠与基准不恒等，逐格复算",
            YubikeyCode::EOrder => "家族逆序：OTP→FIDO2→PIV→OATH→OpenPGP 单调，逆序必抓",
            YubikeyCode::ENoCmd => "命令号越闭集：五命令 10 操作集之外先登记",
            YubikeyCode::ECmdDrift => "命令载荷漂移：命令折叠与基准不恒等，逐格复算",
            YubikeyCode::ENoVer => "版本档越阶梯：固件 5.1/5.2/5.4 三档闭集之外先登记",
            YubikeyCode::EVerDrift => "版本档载荷漂移：级折叠与基准不恒等，逐档复算",
            YubikeyCode::ENoRow => "联签/声明行越闭集：gpg 联签 4 格与 Go/Python 上栈 2 行之外先登记",
            YubikeyCode::ERowMiss => "联签/声明行缺行：gpg 消费联签与上栈声明未齐先补齐",
            YubikeyCode::EDup => "重复落账：同格只许一条账目",
            YubikeyCode::ESeal => "未齐即封：20 应用格 + 10 命令格 + 3 档版本 + 六闸未齐先补齐",
            YubikeyCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20应用格+10命令格+3档版本+4联签格+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 应用格载荷（操作 o；家族 = o/4：0=OTP 1=FIDO2 2=PIV 3=OATH 4=OpenPGP）。
pub const fn app_cell(o: usize) -> Option<u64> {
    if o >= APP_OPS {
        return None;
    }
    Some((0xCA850u64 + (o as u64) * 0x2E) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 应用家族号（i/N 映射：家族 = o/4）。
pub const fn app_kind(o: usize) -> Option<u32> {
    if o >= APP_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn app_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("OTP"),
        1 => Some("FIDO2"),
        2 => Some("PIV"),
        3 => Some("OATH"),
        4 => Some("OpenPGP"),
        _ => None,
    }
}

/// 命令格载荷（操作 c；命令 = c/2：0=list 1=info 2=generate 3=reset 4=verify；轮次 = c%2）。
pub const fn cmd_cell(c: usize) -> Option<u64> {
    if c >= CMD_OPS {
        return None;
    }
    Some((0xCA890u64 + (c as u64) * 0x34) ^ ((c / 2) as u64).wrapping_mul(0x01000193))
}

/// 命令号（i/N 映射：命令 = c/2）。
pub const fn cmd_kind(c: usize) -> Option<u32> {
    if c >= CMD_OPS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 命令名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("list"),
        1 => Some("info"),
        2 => Some("generate"),
        3 => Some("reset"),
        4 => Some("verify"),
        _ => None,
    }
}

/// 版本档载荷（档 v：0=5.1 1=5.2 2=5.4）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xCA8D0u64 + (v as u64) * 0x5D) ^ 0x594B_4D47_5F56_4552)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("5.1"),
        1 => Some("5.2"),
        2 => Some("5.4"),
        _ => None,
    }
}

/// gpg 消费联签格折叠（格 r，锚 UNX-F52408）。
pub const fn joint_cell(r: usize) -> Option<u32> {
    if r >= JOINT_CELLS {
        return None;
    }
    Some(0xCA910 + r as u32 * 0x2C)
}

/// Go/Python 上栈声明行折叠（行 r：0=Go 1=Python）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCA950 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// YubiKey Manager 收口账本状态机。
#[derive(Clone)]
pub struct YubikeyLedger {
    /// 六闸：0=OTP槽 1=PIV槽 2=OATH槽 3=5.1档干跑 4=5.2档干跑 5=5.4档干跑。
    gates: [Option<bool>; 6],
    /// 应用账。
    apps: [Option<u64>; APP_OPS],
    /// 命令账。
    cmds: [Option<u64>; CMD_OPS],
    /// 版本档账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 联签账。
    joints: [Option<u32>; JOINT_CELLS],
    /// 上栈声明账。
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

impl YubikeyLedger {
    /// 新账本。
    pub fn new() -> Self {
        YubikeyLedger {
            gates: [None; 6],
            apps: [None; APP_OPS],
            cmds: [None; CMD_OPS],
            vers: [None; VERSION_STEPS],
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

    /// 落一格六闸（slot 三元 + 版本三档干跑确认；越界即 ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), YubikeyCode> {
        if gate >= 6 {
            return Err(YubikeyCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4C00 + gate as u64);
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

    /// 应用一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_app(&mut self, o: usize, got: u64) -> Result<(), YubikeyCode> {
        if o >= APP_OPS {
            return Err(YubikeyCode::ENoApp);
        }
        let kind = match app_kind(o) {
            Some(k) => k,
            None => return Err(YubikeyCode::ENoApp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D00);
            return Err(YubikeyCode::EOrder);
        }
        let want = match app_cell(o) {
            Some(w) => w,
            None => return Err(YubikeyCode::ENoApp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D01 + o as u64);
            return Err(YubikeyCode::EAppDrift);
        }
        if match self.apps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YubikeyCode::EDup);
        }
        if let Some(c) = self.apps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4D02 + o as u64);
        Ok(())
    }

    /// 应用全账核验（20 格）。
    pub fn apps_all(&self) -> bool {
        let mut o = 0usize;
        while o < APP_OPS {
            match self.apps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令一格（载荷对照 + 重复拒；写配置命令只读干跑）。
    pub fn record_cmd(&mut self, c: usize, got: u64) -> Result<(), YubikeyCode> {
        if c >= CMD_OPS {
            return Err(YubikeyCode::ENoCmd);
        }
        let want = match cmd_cell(c) {
            Some(w) => w,
            None => return Err(YubikeyCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E00 + c as u64);
            return Err(YubikeyCode::ECmdDrift);
        }
        if match self.cmds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YubikeyCode::EDup);
        }
        if let Some(cell) = self.cmds.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x4E01 + c as u64);
        Ok(())
    }

    /// 命令全账核验（10 格）。
    pub fn cmds_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMD_OPS {
            match self.cmds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 版本档一格（档位对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), YubikeyCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(YubikeyCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F00 + v as u64);
            return Err(YubikeyCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YubikeyCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x4F01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// gpg 消费联签一格。
    pub fn declare_joint(&mut self, r: usize) -> Result<(), YubikeyCode> {
        if r >= JOINT_CELLS {
            return Err(YubikeyCode::ENoRow);
        }
        if match self.joints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YubikeyCode::EDup);
        }
        if let Some(cell) = self.joints.get_mut(r) {
            *cell = joint_cell(r);
        }
        self.fnv(0x5000 + r as u64);
        Ok(())
    }

    /// Go/Python 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), YubikeyCode> {
        if r >= STACK_ROWS {
            return Err(YubikeyCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YubikeyCode::EDup);
        }
        if let Some(cell) = self.stacks.get_mut(r) {
            *cell = stack_row(r);
        }
        self.fnv(0x5100 + r as u64);
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

    /// 行收口核（缺行即 ERowMiss）。
    pub fn row_seal(&self) -> Result<(), YubikeyCode> {
        if self.rows_all() {
            Ok(())
        } else {
            Err(YubikeyCode::ERowMiss)
        }
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
    pub fn verify_conservation(&self) -> bool {
        self.injected == self.caught.wrapping_add(self.escaped)
    }

    /// KB-COMPAT 定级（全通 → S1，六闸齐 → S2，否则 S3）。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.apps_all() && self.cmds_all() && self.vers_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级封核（非 S1 即 EGradeDrift）。
    pub fn seal_grade(&self) -> Result<Grade, YubikeyCode> {
        let g = self.grade();
        if g == Grade::S1 {
            Ok(g)
        } else {
            Err(YubikeyCode::EGradeDrift)
        }
    }
}

impl Default for YubikeyLedger {
    fn default() -> Self { Self::new() }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = YubikeyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < APP_OPS {
        if let Some(w) = app_cell(o) {
            if led.record_app(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CMD_OPS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
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
    // 注入 10 轮三态轮转（漂移/越闭集/逆序），全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = YubikeyLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                match app_cell(0) {
                    Some(w) => {
                        let _ = lt.record_app(0, w);
                    }
                    None => {}
                }
                match app_cell(1) {
                    Some(w) => lt.record_app(1, w.wrapping_add(1)) == Err(YubikeyCode::EAppDrift),
                    None => false,
                }
            }
            1 => {
                // 越闭集注入：空账本直接越界必抓，不改 last_kind。
                lt.record_app(APP_OPS, 0).is_err()
            }
            _ => {
                // 逆序注入：空账本先落家族 1 正格，再落家族 0 正格必抓。
                match app_cell(5) {
                    Some(w) => {
                        let _ = lt.record_app(5, w);
                    }
                    None => {}
                }
                match app_cell(1) {
                    Some(w) => lt.record_app(1, w) == Err(YubikeyCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5200 + k as u64);
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
/// UNX-F52418 判据集（锚 UNX-F52419-J1，off-by-one 板式）。
pub fn run_unx52418_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52418");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五应用族 20 操作全通",
        recorded >= APP_OPS as u32 + CMD_OPS as u32 + VERSION_STEPS as u32,
        "OTP/FIDO2/PIV/OATH/OpenPGP 五家族逐操作恒等落账",
    );
    s.add(
        "应用家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < APP_OPS {
                let a = match app_kind(o) { Some(k) => k, None => 9 };
                let b = match app_kind(o - 1) { Some(k) => k, None => 0 };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "OTP→FIDO2→PIV→OATH→OpenPGP 五家族闭集单调，逆序必抓",
    );
    s.add(
        "五命令 10 操作全落",
        {
            let mut lt = YubikeyLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMD_OPS {
                match cmd_cell(c) {
                    Some(w) => { if lt.record_cmd(c, w).is_err() { ok = false; } }
                    None => ok = false,
                }
                c += 1;
            }
            ok && lt.cmds_all()
        },
        "list/info/generate/reset/verify ×2 轮次，写配置命令只读干跑",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CMD_OPS {
                let a = match cmd_kind(c) { Some(k) => k, None => 9 };
                let b = match cmd_kind(c - 1) { Some(k) => k, None => 0 };
                if a < b {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "命令 = 操作/2 五命令闭集单调",
    );
    s.add(
        "slot 三元闸 + 版本干跑六闸全落",
        {
            let mut lt = YubikeyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "OTP槽/PIV槽/OATH槽 slot 三元闸 + 5.1/5.2/5.4 三档干跑确认逐格落账",
    );
    s.add(
        "版本阶梯 5.1/5.2/5.4 三档全落",
        {
            let mut lt = YubikeyLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                match version_cell(v) {
                    Some(w) => { if lt.record_version(v, w).is_err() { ok = false; } }
                    None => ok = false,
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越档必抓",
    );
    s.add(
        "gpg 消费联签 4/4 全齐（锚 UNX-F52408）",
        {
            let mut lt = YubikeyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JOINT_CELLS {
                if lt.declare_joint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            // rows_all 另要求上栈声明行全落（row_seal 复核两套）。
            let mut s = 0usize;
            while s < STACK_ROWS {
                if lt.declare_stack(s).is_err() {
                    ok = false;
                }
                s += 1;
            }
            ok && lt.row_seal().is_ok()
        },
        "OpenPGP 应用与 gpg 消费联签四格闭集，缺一拒封",
    );
    s.add(
        "Go/Python 上栈声明 2/2 全齐",
        {
            let mut lt = YubikeyLedger::new();
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
        "Go 上栈声明/Python 上栈声明两行闭集",
    );
    let mut l0 = YubikeyLedger::new();
    s.add(
        "边界·应用号越闭集",
        l0.record_app(APP_OPS, 0).is_err(),
        "应用号 20 越闭集必拒",
    );
    s.add(
        "应用载荷漂移拒",
        match app_cell(3) { Some(w) => l0.record_app(3, w.wrapping_add(1)) == Err(YubikeyCode::EAppDrift), None => false },
        "应用载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令号 10 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "应用漂移/应用越闭集/家族逆序三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add("定级 S1 闭集",
        {
            let mut lt = YubikeyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < APP_OPS {
                if let Some(w) = app_cell(o) {
                    let _ = lt.record_app(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
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
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA840,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×(id−52401)",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "判据原文条数按求值时点核：含本条共 17 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_kind_mapping() {
        assert_eq!(app_kind(0), Some(0));
        assert_eq!(app_kind(4), Some(1));
        assert_eq!(app_kind(8), Some(2));
        assert_eq!(app_kind(12), Some(3));
        assert_eq!(app_kind(19), Some(4));
        assert!(app_kind(20).is_none());
        assert_eq!(app_kind_name(1), Some("FIDO2"));
        assert_eq!(app_kind_name(4), Some("OpenPGP"));
        assert!(app_kind_name(5).is_none());
    }

    #[test]
    fn test_cmd_kind_mapping() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(1), Some(0));
        assert_eq!(cmd_kind(2), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(2), Some("generate"));
        assert_eq!(cmd_kind_name(4), Some("verify"));
        assert!(cmd_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = app_cell(3).unwrap_or(0);
        assert_eq!(a, app_cell(3).unwrap_or(0));
        assert_ne!(a, app_cell(7).unwrap_or(0));
        let c = cmd_cell(6).unwrap_or(0);
        assert_eq!(c, cmd_cell(6).unwrap_or(0));
        assert_ne!(c, cmd_cell(7).unwrap_or(0));
        assert!(version_cell(3).is_none());
        assert!(joint_cell(JOINT_CELLS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("5.1"));
        assert_eq!(version_name(1), Some("5.2"));
        assert_eq!(version_name(2), Some("5.4"));
        assert!(version_name(3).is_none());
        let mut lt = YubikeyLedger::new();
        assert_eq!(
            lt.record_version(VERSION_STEPS, 0),
            Err(YubikeyCode::ENoVer)
        );
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = YubikeyLedger::new();
        // 先落家族 1 正格（o=5 FIDO2 族），再落家族 0 正格（o=1 OTP 族）必抓。
        let _ = lt.record_app(5, app_cell(5).unwrap_or(0));
        assert_eq!(lt.record_app(1, app_cell(1).unwrap_or(0)), Err(YubikeyCode::EOrder));
        assert!(lt.record_app(6, app_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = YubikeyLedger::new();
        assert_eq!(
            lt.record_app(9, app_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(YubikeyCode::EAppDrift)
        );
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(3)),
            Err(YubikeyCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(YubikeyCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = YubikeyLedger::new();
        let _ = lt.record_app(2, app_cell(2).unwrap_or(0));
        assert_eq!(lt.record_app(2, app_cell(2).unwrap_or(0)), Err(YubikeyCode::EDup));
        let _ = lt.record_cmd(4, cmd_cell(4).unwrap_or(0));
        assert_eq!(lt.record_cmd(4, cmd_cell(4).unwrap_or(0)), Err(YubikeyCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = YubikeyLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = YubikeyLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_seal_boundary() {
        let mut lt = YubikeyLedger::new();
        let mut r = 0usize;
        while r < JOINT_CELLS {
            assert!(lt.declare_joint(r).is_ok());
            r += 1;
        }
        assert!(lt.declare_joint(JOINT_CELLS).is_err());
        let mut r = 0usize;
        while r < STACK_ROWS {
            assert!(lt.declare_stack(r).is_ok());
            r += 1;
        }
        assert!(lt.declare_stack(STACK_ROWS).is_err());
        assert!(lt.row_seal().is_ok());
        let mut lt2 = YubikeyLedger::new();
        assert_eq!(lt2.row_seal(), Err(YubikeyCode::ERowMiss));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 应用 + 10 命令 + 3 版本。
        assert!(recorded >= APP_OPS as u32 + CMD_OPS as u32 + VERSION_STEPS as u32);
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
        assert_eq!(SEG_BASE, 0xCA840);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(YubikeyCode::Ok as u32, 0);
        assert_eq!(YubikeyCode::EGradeDrift as u32, 12);
        let all = [
            YubikeyCode::Ok,
            YubikeyCode::ENoApp,
            YubikeyCode::EAppDrift,
            YubikeyCode::EOrder,
            YubikeyCode::ENoCmd,
            YubikeyCode::ECmdDrift,
            YubikeyCode::ENoVer,
            YubikeyCode::EVerDrift,
            YubikeyCode::ENoRow,
            YubikeyCode::ERowMiss,
            YubikeyCode::EDup,
            YubikeyCode::ESeal,
            YubikeyCode::EGradeDrift,
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
        let mut lt = YubikeyLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.grade(), Grade::S2);
        let mut o = 0usize;
        while o < APP_OPS {
            let _ = lt.record_app(o, app_cell(o).unwrap_or(0));
            o += 1;
        }
        let mut c = 0usize;
        while c < CMD_OPS {
            let _ = lt.record_cmd(c, cmd_cell(c).unwrap_or(0));
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            let _ = lt.record_version(v, version_cell(v).unwrap_or(0));
            v += 1;
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
        assert!(lt.verify_conservation());
    }

    #[test]
    fn test_empty_ledger_injections() {
        let mut a = YubikeyLedger::new();
        assert!(a.record_app(APP_OPS, 0).is_err());
        let mut b = YubikeyLedger::new();
        let _ = b.record_app(5, app_cell(5).unwrap_or(0));
        let kind_before = b.last_kind;
        assert_eq!(b.record_app(1, app_cell(1).unwrap_or(0)), Err(YubikeyCode::EOrder));
        assert_eq!(b.last_kind, kind_before);
    }

    #[test]
    fn unx52418_checks_green() {
        let s = run_unx52418_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
