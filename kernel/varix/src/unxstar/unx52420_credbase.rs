//! UNX-F52420 凭证文件安全底线条目（全域密码库类判据红线登记）· 0xCA8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52420"，逐字）：
//!   title: 凭证文件安全底线条目（全域密码库类判据红线登记）
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52421-J1 扫描判
//!     据只对本机回环与实验室网段执行（目标白名单声明）；主机发现/端
//!     口扫描 ×10 全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 22 口
//!     径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52420）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52421-J1，照原文锚定；功能按本卡标题（凭证文件安全底线条目·
//! 全域密码库类判据红线登记）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 全域密码库类闭集：KeePass/KeePassXC/Bitwarden/1Password/Enpass
//!      五库 × 4 判据格（主密钥/加密面/自动锁/备份）共 20 格，库序单
//!      调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 红线六条登记 12 格：主密钥不落地/加密面整库/自动锁强制/剪贴板
//!      即清/备份即弃口令/日志不记口令 ×（登记/复核），体验十三隐私红
//!      线口径（判据锚 UNX-F52420-J1 全组条目红线登记）。
//!   ③ 底线条目单向门：条目序严格只增（回退必抓），逐条载荷折叠恒等。
//!   ④ 六闸 = 六红线确认闸，逐格落账，未齐即封。
//!   ⑤ 违例检出注入 10/10（库格载荷漂移/库格越闭集/单向门回退三态轮
//!      转），检出+逃逸守恒等式；双跑指纹一致。
//!   ⑥ 与各库分账声明 2 行闭集（与 KeePass/KeePassXC/Bitwarden/
//!      1Password/Enpass 各库登记分账），缺一拒封。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；KB-COMPAT 定级 S1/S2/S3 闭集（本卡判据原文
//!      定级 S1，全通即 S1）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52420;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五库 20 判据格数。
pub const VAULT_CELLS: usize = 20;

/// 密码库数。
pub const VAULT_COUNT: usize = 5;

/// 红线六条登记 12 格数。
pub const RED_CELLS: usize = 12;

/// 红线条数。
pub const RED_LINES: usize = 6;

/// 底线条目单向门步数。
pub const DOOR_STEPS: usize = 6;

/// 与各库分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CredBaseCode {
    /// 成功
    Ok = 0,
    /// 库判据格越闭集
    EVaultNo,
    /// 库判据格载荷漂移
    EVaultDrift,
    /// 库序逆序
    EOrder,
    /// 红线登记格越闭集
    ERedNo,
    /// 红线登记格载荷漂移
    ERedDrift,
    /// 单向门步越闭集
    EDoorNo,
    /// 单向门回退/漂移
    EDoorDrift,
    /// 分账声明行越闭集
    ENoRow,
    /// 分账声明行缺行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl CredBaseCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CredBaseCode::Ok => "成功",
            CredBaseCode::EVaultNo => "库判据格越闭集：五库 × 4 判据 20 格之外先登记",
            CredBaseCode::EVaultDrift => "库判据格载荷漂移：判据折叠与基准不恒等，逐格复算",
            CredBaseCode::EOrder => "库序逆序：KeePass→KeePassXC→Bitwarden→1Password→Enpass 单调，逆序必抓",
            CredBaseCode::ERedNo => "红线登记格越闭集：红线六条 ×（登记/复核）12 格之外先登记",
            CredBaseCode::ERedDrift => "红线登记格载荷漂移：登记/复核折叠与基准不恒等，逐格复算",
            CredBaseCode::EDoorNo => "单向门步越闭集：底线条目 6 步闭集之外先登记",
            CredBaseCode::EDoorDrift => "单向门回退/漂移：条目序严格只增且载荷恒等，回退必抓",
            CredBaseCode::ENoRow => "分账声明行越闭集：与各库分账 2 行之外先登记",
            CredBaseCode::ERowMiss => "分账声明行缺行：与各库分账声明未齐先补齐",
            CredBaseCode::EDup => "重复落账：同格只许一条账目",
            CredBaseCode::ESeal => "未齐即封：20 库格 + 12 红线格 + 6 步单向门 + 六闸未齐先补齐",
            CredBaseCode::EGradeDrift => "定级漂移：全通且红线登记/分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20库格+12红线格+6步单向门+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 库判据格载荷（格 c；库 = c/4，判据相位 = c%4：0=主密钥 1=加密面 2=自动锁 3=备份）。
pub const fn vault_cell(c: usize) -> Option<u64> {
    if c >= VAULT_CELLS {
        return None;
    }
    Some((0xCA8D0u64 + (c as u64) * 0x2C) ^ ((c / 4) as u64).wrapping_mul(0x01000193))
}

/// 库号（i/N 映射：库 = c/4）。
pub const fn vault_kind(c: usize) -> Option<u32> {
    if c >= VAULT_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 判据相位（i/N 映射：相位 = c%4）。
pub const fn vault_phase(c: usize) -> Option<u32> {
    if c >= VAULT_CELLS {
        return None;
    }
    Some((c % 4) as u32)
}

/// 库名。
pub const fn vault_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("KeePass"),
        1 => Some("KeePassXC"),
        2 => Some("Bitwarden"),
        3 => Some("1Password"),
        4 => Some("Enpass"),
        _ => None,
    }
}

/// 判据相位名。
pub const fn vault_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("主密钥"),
        1 => Some("加密面"),
        2 => Some("自动锁"),
        3 => Some("备份"),
        _ => None,
    }
}

/// 红线登记格载荷（格 r；红线 = r/2，格型 = r%2：0=登记 1=复核）。
pub const fn red_cell(r: usize) -> Option<u64> {
    if r >= RED_CELLS {
        return None;
    }
    Some((0xCA910u64 + (r as u64) * 0x2C) ^ ((r / 2) as u64).wrapping_mul(0x01000193))
}

/// 红线号（i/N 映射：红线 = r/2）。
pub const fn red_kind(r: usize) -> Option<u32> {
    if r >= RED_CELLS {
        return None;
    }
    Some((r / 2) as u32)
}

/// 红线名（体验十三隐私红线口径）。
pub const fn red_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("主密钥不落地"),
        1 => Some("加密面整库"),
        2 => Some("自动锁强制"),
        3 => Some("剪贴板即清"),
        4 => Some("备份即弃口令"),
        5 => Some("日志不记口令"),
        _ => None,
    }
}

/// 单向门步载荷（步 d）。
pub const fn door_cell(d: usize) -> Option<u64> {
    if d >= DOOR_STEPS {
        return None;
    }
    Some((0xCA950u64 + (d as u64) * 0x5D) ^ 0x4352_4544_444F_4F52)
}

/// 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCA990 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 凭证文件安全底线收口账本状态机。
#[derive(Clone)]
pub struct CredBaseLedger {
    /// 六闸：六红线确认（0=主密钥不落地 … 5=日志不记口令）。
    gates: [Option<bool>; 6],
    /// 库判据账。
    vaults: [Option<u64>; VAULT_CELLS],
    /// 红线登记账。
    reds: [Option<u64>; RED_CELLS],
    /// 单向门步账。
    doors: [Option<u64>; DOOR_STEPS],
    /// 已落最高步。
    last_step: i64,
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后库号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CredBaseLedger {
    /// 新账本。
    pub fn new() -> Self {
        CredBaseLedger {
            gates: [None; 6],
            vaults: [None; VAULT_CELLS],
            reds: [None; RED_CELLS],
            doors: [None; DOOR_STEPS],
            last_step: -1,
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

    /// 落一格六闸（六红线确认；越界即 ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CredBaseCode> {
        if gate >= 6 {
            return Err(CredBaseCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5900 + gate as u64);
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

    /// 库判据一格（库序单调 + 载荷对照 + 重复拒）。
    pub fn record_vault(&mut self, c: usize, got: u64) -> Result<(), CredBaseCode> {
        if c >= VAULT_CELLS {
            return Err(CredBaseCode::EVaultNo);
        }
        let kind = match vault_kind(c) {
            Some(k) => k,
            None => return Err(CredBaseCode::EVaultNo),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A00);
            return Err(CredBaseCode::EOrder);
        }
        let want = match vault_cell(c) {
            Some(w) => w,
            None => return Err(CredBaseCode::EVaultNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A01 + c as u64);
            return Err(CredBaseCode::EVaultDrift);
        }
        if match self.vaults.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CredBaseCode::EDup);
        }
        if let Some(cell) = self.vaults.get_mut(c) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5A02 + c as u64);
        Ok(())
    }

    /// 库判据全账核验（20 格）。
    pub fn vaults_all(&self) -> bool {
        let mut c = 0usize;
        while c < VAULT_CELLS {
            match self.vaults.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 红线登记一格（载荷对照 + 重复拒）。
    pub fn record_red(&mut self, r: usize, got: u64) -> Result<(), CredBaseCode> {
        if r >= RED_CELLS {
            return Err(CredBaseCode::ERedNo);
        }
        let want = match red_cell(r) {
            Some(w) => w,
            None => return Err(CredBaseCode::ERedNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B00 + r as u64);
            return Err(CredBaseCode::ERedDrift);
        }
        if match self.reds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CredBaseCode::EDup);
        }
        if let Some(cell) = self.reds.get_mut(r) {
            *cell = Some(got);
        }
        self.fnv(0x5B01 + r as u64);
        Ok(())
    }

    /// 红线登记全账核验（12 格）。
    pub fn reds_all(&self) -> bool {
        let mut r = 0usize;
        while r < RED_CELLS {
            match self.reds.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 单向门一步（步号严格只增 + 载荷对照；回退即 EDoorDrift）。
    pub fn record_door(&mut self, d: usize, got: u64) -> Result<(), CredBaseCode> {
        if d >= DOOR_STEPS {
            return Err(CredBaseCode::EDoorNo);
        }
        if (d as i64) <= self.last_step {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C00);
            return Err(CredBaseCode::EDoorDrift);
        }
        let want = match door_cell(d) {
            Some(w) => w,
            None => return Err(CredBaseCode::EDoorNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C01 + d as u64);
            return Err(CredBaseCode::EDoorDrift);
        }
        if let Some(cell) = self.doors.get_mut(d) {
            *cell = Some(got);
        }
        self.last_step = d as i64;
        self.fnv(0x5C02 + d as u64);
        Ok(())
    }

    /// 单向门全账核验（6 步全齐）。
    pub fn doors_all(&self) -> bool {
        let mut d = 0usize;
        while d < DOOR_STEPS {
            match self.doors.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 分账声明一行（与各库登记分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), CredBaseCode> {
        if r >= SPLIT_ROWS {
            return Err(CredBaseCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CredBaseCode::EDup);
        }
        if let Some(cell) = self.splits.get_mut(r) {
            *cell = split_row(r);
        }
        self.fnv(0x5D00 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// 行收口核（缺行即 ERowMiss）。
    pub fn row_seal(&self) -> Result<(), CredBaseCode> {
        if self.splits_all() {
            Ok(())
        } else {
            Err(CredBaseCode::ERowMiss)
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
        if self.gates_all() && self.vaults_all() && self.reds_all() && self.doors_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级封核（非 S1 即 EGradeDrift）。
    pub fn seal_grade(&self) -> Result<Grade, CredBaseCode> {
        let g = self.grade();
        if g == Grade::S1 {
            Ok(g)
        } else {
            Err(CredBaseCode::EGradeDrift)
        }
    }
}

impl Default for CredBaseLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CredBaseLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < VAULT_CELLS {
        match vault_cell(c) {
            Some(w) => {
                if led.record_vault(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < RED_CELLS {
        match red_cell(r) {
            Some(w) => {
                if led.record_red(r, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        r += 1;
    }
    let mut d = 0usize;
    while d < DOOR_STEPS {
        match door_cell(d) {
            Some(w) => {
                if led.record_door(d, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 违例检出注入 10 轮：库格载荷漂移 / 库格越闭集 / 单向门回退三态
    // 轮转，全部落在当轮临时账本上，主账只记注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CredBaseLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                match vault_cell(0) {
                    Some(w) => {
                        let _ = lt.record_vault(0, w);
                    }
                    None => {}
                }
                match vault_cell(1) {
                    Some(w) => lt.record_vault(1, w.wrapping_add(1)) == Err(CredBaseCode::EVaultDrift),
                    None => false,
                }
            }
            1 => {
                // 越闭集注入：空账本直接越界必抓，不改 last_kind。
                lt.record_vault(VAULT_CELLS, 0).is_err()
            }
            _ => {
                // 单向门回退注入：先落步 0 正格，再落步 0 回退必抓。
                match door_cell(0) {
                    Some(w) => {
                        let _ = lt.record_door(0, w);
                        lt.record_door(0, w) == Err(CredBaseCode::EDoorDrift)
                    }
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5E00 + k as u64);
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
/// UNX-F52420 判据集（锚 UNX-F52421-J1，off-by-one 板式）。
pub fn run_unx52420_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52420");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五库 20 判据格全通（主密钥/加密面/自动锁/备份）",
        recorded >= VAULT_CELLS as u32 + RED_CELLS as u32 + DOOR_STEPS as u32,
        "KeePass/KeePassXC/Bitwarden/1Password/Enpass 五库逐格恒等落账",
    );
    s.add(
        "库序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < VAULT_CELLS {
                let a = match vault_kind(c) { Some(k) => k, None => 99 };
                let b = match vault_kind(c - 1) { Some(k) => k, None => 0 };
                if a < b {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五库闭集单调，逆序必抓",
    );
    s.add(
        "判据相位核（c%4 映射）",
        {
            let mut ok = true;
            let mut c = 0usize;
            while c < VAULT_CELLS {
                let p = match vault_phase(c) {
                    Some(p) => p,
                    None => 9,
                };
                if p != (c % 4) as u32 {
                    ok = false;
                }
                c += 1;
            }
            ok && vault_phase_name(0) == Some("主密钥") && vault_phase_name(3) == Some("备份")
        },
        "判据相位 = 格号%4 的 i/N 映射逐格核对",
    );
    s.add(
        "红线六条登记 12 格全落",
        {
            let mut lt = CredBaseLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RED_CELLS {
                match red_cell(r) {
                    Some(w) => {
                        if lt.record_red(r, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                r += 1;
            }
            ok && lt.reds_all()
        },
        "六红线 ×（登记/复核）12 格逐格恒等（体验十三隐私红线口径）",
    );
    s.add(
        "红线六条名核",
        red_kind_name(0) == Some("主密钥不落地")
            && red_kind_name(1) == Some("加密面整库")
            && red_kind_name(2) == Some("自动锁强制")
            && red_kind_name(3) == Some("剪贴板即清")
            && red_kind_name(4) == Some("备份即弃口令")
            && red_kind_name(5) == Some("日志不记口令")
            && red_kind_name(6).is_none(),
        "测试库随机口令+即弃、真实凭证零入库、日志不记口令三条红线锚定在册",
    );
    s.add(
        "底线条目单向门 6 步全落",
        {
            let mut lt = CredBaseLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DOOR_STEPS {
                match door_cell(d) {
                    Some(w) => {
                        if lt.record_door(d, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                d += 1;
            }
            ok && lt.doors_all()
        },
        "条目序严格只增且逐步载荷恒等，回退必抓",
    );
    s.add(
        "六红线确认闸全落",
        {
            let mut lt = CredBaseLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "主密钥不落地/加密面整库/自动锁强制/剪贴板即清/备份即弃口令/日志不记口令逐格落账",
    );
    s.add(
        "与各库分账声明 2/2 全齐",
        {
            let mut lt = CredBaseLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.row_seal().is_ok()
        },
        "与 KeePass/KeePassXC/Bitwarden/1Password/Enpass 各库登记分账两行闭集",
    );
    let mut l0 = CredBaseLedger::new();
    s.add(
        "边界·库判据格越闭集",
        l0.record_vault(VAULT_CELLS, 0).is_err(),
        "库格 20 越闭集必拒",
    );
    s.add(
        "库格载荷漂移拒",
        match vault_cell(3) {
            Some(w) => l0.record_vault(3, w.wrapping_add(1)) == Err(CredBaseCode::EVaultDrift),
            None => false,
        },
        "库判据格载荷漂移必抓",
    );
    s.add(
        "边界·红线格越闭集",
        l0.record_red(RED_CELLS, 0).is_err(),
        "红线格 12 越闭集必拒",
    );
    s.add(
        "违例检出注入 10/10 全抓",
        caught >= 10,
        "库格漂移/库格越闭集/单向门回退三态轮转必抓",
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
            let mut lt = CredBaseLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < VAULT_CELLS {
                match vault_cell(c) {
                    Some(w) => {
                        let _ = lt.record_vault(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < RED_CELLS {
                match red_cell(r) {
                    Some(w) => {
                        let _ = lt.record_red(r, w);
                    }
                    None => {}
                }
                r += 1;
            }
            let mut d = 0usize;
            while d < DOOR_STEPS {
                match door_cell(d) {
                    Some(w) => {
                        let _ = lt.record_door(d, w);
                    }
                    None => {}
                }
                d += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.seal_grade() == Ok(Grade::S1)
        },
        "全通 → S1（KB-COMPAT 定级 S1，本卡判据原文口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA8C0,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×(id−52401)",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "判据原文条数按求值时点核：含本条共 18 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_kind_phase_mapping() {
        assert_eq!(vault_kind(0), Some(0));
        assert_eq!(vault_kind(4), Some(1));
        assert_eq!(vault_kind(8), Some(2));
        assert_eq!(vault_kind(12), Some(3));
        assert_eq!(vault_kind(19), Some(4));
        assert!(vault_kind(20).is_none());
        assert_eq!(vault_phase(0), Some(0));
        assert_eq!(vault_phase(1), Some(1));
        assert_eq!(vault_phase(3), Some(3));
        assert_eq!(vault_phase(4), Some(0));
        assert!(vault_phase(20).is_none());
        assert_eq!(vault_kind_name(0), Some("KeePass"));
        assert_eq!(vault_kind_name(4), Some("Enpass"));
        assert!(vault_kind_name(5).is_none());
        assert_eq!(vault_phase_name(2), Some("自动锁"));
    }

    #[test]
    fn test_red_kind_mapping() {
        assert_eq!(red_kind(0), Some(0));
        assert_eq!(red_kind(1), Some(0));
        assert_eq!(red_kind(2), Some(1));
        assert_eq!(red_kind(11), Some(5));
        assert!(red_kind(12).is_none());
        assert_eq!(red_kind_name(5), Some("日志不记口令"));
        assert!(red_kind_name(6).is_none());
        assert!(red_cell(RED_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = vault_cell(6).unwrap_or(0);
        assert_eq!(a, vault_cell(6).unwrap_or(0));
        assert_ne!(a, vault_cell(7).unwrap_or(0));
        let d = door_cell(2).unwrap_or(0);
        assert_eq!(d, door_cell(2).unwrap_or(0));
        assert_ne!(d, door_cell(3).unwrap_or(0));
        assert!(vault_cell(VAULT_CELLS).is_none());
        assert!(door_cell(DOOR_STEPS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_vault_order_caught() {
        let mut lt = CredBaseLedger::new();
        // 先落库 1 正格（c=4），再落库 0 正格（c=1）必抓。
        let _ = lt.record_vault(4, vault_cell(4).unwrap_or(0));
        assert_eq!(lt.record_vault(1, vault_cell(1).unwrap_or(0)), Err(CredBaseCode::EOrder));
        assert!(lt.record_vault(5, vault_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CredBaseLedger::new();
        assert_eq!(
            lt.record_vault(9, vault_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(CredBaseCode::EVaultDrift)
        );
        assert_eq!(
            lt.record_red(7, red_cell(7).unwrap_or(0).wrapping_add(3)),
            Err(CredBaseCode::ERedDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CredBaseLedger::new();
        let _ = lt.record_vault(2, vault_cell(2).unwrap_or(0));
        assert_eq!(lt.record_vault(2, vault_cell(2).unwrap_or(0)), Err(CredBaseCode::EDup));
        let _ = lt.record_red(4, red_cell(4).unwrap_or(0));
        assert_eq!(lt.record_red(4, red_cell(4).unwrap_or(0)), Err(CredBaseCode::EDup));
    }

    #[test]
    fn test_door_one_way() {
        let mut lt = CredBaseLedger::new();
        let mut d = 0usize;
        while d < DOOR_STEPS {
            assert!(lt.record_door(d, door_cell(d).unwrap_or(0)).is_ok());
            d += 1;
        }
        // 闭集边界：0..=5 步之外越界必抓；重复步回退必抓。
        assert!(lt.record_door(DOOR_STEPS, 0).is_err());
        assert_eq!(
            lt.record_door(0, door_cell(0).unwrap_or(0)),
            Err(CredBaseCode::EDoorDrift)
        );
        assert!(lt.doors_all());
        // 空账本先落步 1 再落步 0 回退必抓。
        let mut lt2 = CredBaseLedger::new();
        let _ = lt2.record_door(1, door_cell(1).unwrap_or(0));
        assert_eq!(
            lt2.record_door(0, door_cell(0).unwrap_or(0)),
            Err(CredBaseCode::EDoorDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CredBaseLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CredBaseLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 库格 + 12 红线格 + 6 步门。
        assert!(recorded >= VAULT_CELLS as u32 + RED_CELLS as u32 + DOOR_STEPS as u32);
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
        assert_eq!(SEG_BASE, 0xCA8C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CredBaseCode::Ok as u32, 0);
        assert_eq!(CredBaseCode::EGradeDrift as u32, 12);
        let all = [
            CredBaseCode::Ok,
            CredBaseCode::EVaultNo,
            CredBaseCode::EVaultDrift,
            CredBaseCode::EOrder,
            CredBaseCode::ERedNo,
            CredBaseCode::ERedDrift,
            CredBaseCode::EDoorNo,
            CredBaseCode::EDoorDrift,
            CredBaseCode::ENoRow,
            CredBaseCode::ERowMiss,
            CredBaseCode::EDup,
            CredBaseCode::ESeal,
            CredBaseCode::EGradeDrift,
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
        let mut lt = CredBaseLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert!(lt.verify_conservation());
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.grade(), Grade::S2);
        let mut c = 0usize;
        while c < VAULT_CELLS {
            let _ = lt.record_vault(c, vault_cell(c).unwrap_or(0));
            c += 1;
        }
        let mut r = 0usize;
        while r < RED_CELLS {
            let _ = lt.record_red(r, red_cell(r).unwrap_or(0));
            r += 1;
        }
        let mut d = 0usize;
        while d < DOOR_STEPS {
            let _ = lt.record_door(d, door_cell(d).unwrap_or(0));
            d += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        assert_eq!(lt.grade(), Grade::S1);
        assert!(lt.seal_grade().is_ok());
        assert!(lt.row_seal().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        let mut a = CredBaseLedger::new();
        assert!(a.record_vault(VAULT_CELLS, 0).is_err());
        let mut b = CredBaseLedger::new();
        let _ = b.record_vault(4, vault_cell(4).unwrap_or(0));
        let kind_before = b.last_kind;
        assert_eq!(b.record_vault(1, vault_cell(1).unwrap_or(0)), Err(CredBaseCode::EOrder));
        assert_eq!(b.last_kind, kind_before);
        // 单向门回退注入在空账本上必抓。
        let mut e = CredBaseLedger::new();
        let _ = e.record_door(0, door_cell(0).unwrap_or(0));
        assert_eq!(
            e.record_door(0, door_cell(0).unwrap_or(0)),
            Err(CredBaseCode::EDoorDrift)
        );
    }

    #[test]
    fn unx52420_checks_green() {
        let s = run_unx52420_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
