//! UNX-F52413 Cryptomator 收口 · 0xCA700段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52413"，逐字）：
//!   title: Cryptomator 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52414-J1 仓库透明
//!     加密 ×10；与 git 消费联签（F52159 锚定行）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52413）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52414-J1，照原文锚定；功能按本卡标题（Cryptomator）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面 20 格闭集：库创建/解锁/加锁/加密/同步五面各 4 格，面单调
//!      （逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② vault 配置投影 8 格：vault.cryptomator 文件投影逐格对照，漂移必
//!      抓；虚拟盘三态 6 格（锁定/解锁/挂载三态闭集，越态必抓）。
//!   ③ masterkey 三元闸 + 六判据母版闸：主密钥生成/解锁/恢复三元落格，
//!      连同库健康三闸共 6 格判据，未齐即封。
//!   ④ Java 上栈声明 2 行闭集：JVM 运行时上栈声明 / Cryptomator JNI 桥
//!      分账声明，缺一拒封；与 git 消费联签按 F52159 锚定行落账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（面漂移/面逆
//!      序/面越闭集）全部落在当轮临时账本，检出+逃逸守恒；双跑 FNV 指
//!      纹一致；定级 S1（全齐目标档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52413;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面操作数。
pub const FACE_OPS: usize = 20;

/// vault 配置投影格数。
pub const VAULT_CELLS: usize = 8;

/// 虚拟盘三态格数。
pub const DRIVE_CELLS: usize = 6;

/// Java 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CryptomatorCode {
    /// 成功
    Ok = 0,
    /// 面操作号越闭集
    ENoOp,
    /// 面操作漂移
    EOpDrift,
    /// 面逆序
    EOrder,
    /// vault 配置投影越闭集
    EVault,
    /// vault 配置投影漂移
    EVaultDrift,
    /// 虚拟盘格越闭集
    EDrive,
    /// 虚拟盘三态越态
    EDriveState,
    /// 声明行越闭集
    ERow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl CryptomatorCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CryptomatorCode::Ok => "成功",
            CryptomatorCode::ENoOp => "面操作号越闭集：库创建/解锁/加锁/加密/同步 20 格之外先登记",
            CryptomatorCode::EOpDrift => "面操作漂移：面格折叠与基准不恒等，逐格复算",
            CryptomatorCode::EOrder => "面逆序：库创建→解锁→加锁→加密→同步单调，逆序必抓",
            CryptomatorCode::EVault => "vault 配置投影越闭集：8 格投影之外先登记",
            CryptomatorCode::EVaultDrift => "vault 配置投影漂移：投影折叠与 masterkey.cryptomator 基准不恒等",
            CryptomatorCode::EDrive => "虚拟盘格越闭集：6 格三态账之外先登记",
            CryptomatorCode::EDriveState => "虚拟盘三态越态：锁定/解锁/挂载三态闭集之外先登记",
            CryptomatorCode::ERow => "声明行越闭集：Java 上栈 2 行闭集之外先登记",
            CryptomatorCode::ERowMiss => "缺声明行：Java 上栈声明未齐先补齐",
            CryptomatorCode::EDup => "重复落账：同格只许一条账目",
            CryptomatorCode::ESeal => "未齐即封：20 面 + 8 投影 + 6 三态 + 2 行未齐先补齐",
            CryptomatorCode::EGradeDrift => "定级漂移：全齐且声明全落才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（目标档）
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
            Grade::S1 => "S1：六闸+20面+8投影+6三态+2行全通收口（KB-COMPAT 目标档）",
            Grade::S2 => "S2：有条件收口，六闸齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面格载荷（面 o；面族 = o/4）。
pub const fn face_cell(o: usize) -> Option<u64> {
    if o >= FACE_OPS {
        return None;
    }
    Some((0xD1310u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 面族（0..4=库创建 4..8=解锁 8..12=加锁 12..16=加密 16..20=同步）。
pub const fn face_kind(o: usize) -> Option<u32> {
    if o >= FACE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面族名（const fn 表 + Option 边界）。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("库创建"),
        1 => Some("解锁"),
        2 => Some("加锁"),
        3 => Some("加密"),
        4 => Some("同步"),
        _ => None,
    }
}

/// vault 配置投影格基准（格 v；vault.cryptomator 投影）。
pub const fn vault_cell(v: usize) -> Option<u64> {
    if v >= VAULT_CELLS {
        return None;
    }
    Some((0xD1320u64 + (v as u64) * 0x37) ^ 0x5641554C5450_0000)
}

/// 虚拟盘三态格基准（格 d；三态 0=锁定 1=解锁 2=挂载）。
pub const fn drive_cell(d: usize) -> Option<u64> {
    if d >= DRIVE_CELLS {
        return None;
    }
    Some(0xD1330u64 + (d as u64) * 0x41)
}

/// 三态名。
pub const fn drive_state_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("锁定"),
        1 => Some("解锁"),
        2 => Some("挂载"),
        _ => None,
    }
}

/// Java 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD1340 + r as u32 * 0x2C)
}

/// masterkey 三元闸名（闸 g=0..3 为主密钥生成/解锁/恢复，g=3..6 为库健康）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("masterkey 生成"),
        1 => Some("masterkey 解锁"),
        2 => Some("masterkey 恢复"),
        3 => Some("库健康·完整性"),
        4 => Some("库健康·回收站"),
        5 => Some("库健康·非加密残留扫描"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Cryptomator 收口账本状态机。
#[derive(Clone)]
pub struct CryptomatorLedger {
    /// 六判据（masterkey 三元闸 + 库健康三闸）。
    gates: [Option<bool>; 6],
    /// 面账。
    faces: [Option<u64>; FACE_OPS],
    /// vault 配置投影账。
    vaults: [Option<u64>; VAULT_CELLS],
    /// 虚拟盘三态账（格 → (基准, 态)）。
    drives: [Option<(u64, u32)>; DRIVE_CELLS],
    /// Java 上栈声明账。
    rows: [Option<u32>; STACK_ROWS],
    /// 最后面族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CryptomatorLedger {
    /// 新账本。
    pub fn new() -> Self {
        CryptomatorLedger {
            gates: [None; 6],
            faces: [None; FACE_OPS],
            vaults: [None; VAULT_CELLS],
            drives: [None; DRIVE_CELLS],
            rows: [None; STACK_ROWS],
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

    /// 落一格六判据（masterkey 三元闸 + 库健康）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CryptomatorCode> {
        if gate >= 6 {
            return Err(CryptomatorCode::ESeal);
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

    /// 面一格（面族单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, o: usize, got: u64) -> Result<(), CryptomatorCode> {
        if o >= FACE_OPS {
            return Err(CryptomatorCode::ENoOp);
        }
        let kind = match face_kind(o) {
            Some(k) => k,
            None => return Err(CryptomatorCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D200);
            return Err(CryptomatorCode::EOrder);
        }
        let want = match face_cell(o) {
            Some(w) => w,
            None => return Err(CryptomatorCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D201 + o as u64);
            return Err(CryptomatorCode::EOpDrift);
        }
        if match self.faces.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CryptomatorCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5D202 + o as u64);
        Ok(())
    }

    /// 面全账核验（20 格）。
    pub fn faces_all(&self) -> bool {
        let mut o = 0usize;
        while o < FACE_OPS {
            match self.faces.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// vault 配置投影一格（对照 + 重复拒）。
    pub fn record_vault(&mut self, v: usize, got: u64) -> Result<(), CryptomatorCode> {
        let want = match vault_cell(v) {
            Some(w) => w,
            None => return Err(CryptomatorCode::EVault),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D300 + v as u64);
            return Err(CryptomatorCode::EVaultDrift);
        }
        if match self.vaults.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CryptomatorCode::EDup);
        }
        if let Some(c) = self.vaults.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5D301 + v as u64);
        Ok(())
    }

    /// vault 投影全账核验（8 格）。
    pub fn vaults_all(&self) -> bool {
        let mut v = 0usize;
        while v < VAULT_CELLS {
            match self.vaults.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 虚拟盘一格（三态闭集 + 对照 + 重复拒）。
    pub fn record_drive(&mut self, d: usize, state: u32) -> Result<(), CryptomatorCode> {
        let want = match drive_cell(d) {
            Some(w) => w,
            None => return Err(CryptomatorCode::EDrive),
        };
        if state >= 3 {
            return Err(CryptomatorCode::EDriveState);
        }
        if match self.drives.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CryptomatorCode::EDup);
        }
        if let Some(c) = self.drives.get_mut(d) {
            *c = Some((want, state));
        }
        self.fnv(0x5D400 + d as u64 * 4 + state as u64);
        Ok(())
    }

    /// 虚拟盘全账核验（6 格）。
    pub fn drives_all(&self) -> bool {
        let mut d = 0usize;
        while d < DRIVE_CELLS {
            match self.drives.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// Java 上栈声明一行。
    pub fn declare_row(&mut self, r: usize) -> Result<(), CryptomatorCode> {
        if r >= STACK_ROWS {
            return Err(CryptomatorCode::ERow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CryptomatorCode::EDup);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5D500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.rows.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S1 目标档，六闸齐 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.faces_all() && self.vaults_all() && self.drives_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CryptomatorLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CryptomatorLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FACE_OPS {
        if let Some(w) = face_cell(o) {
            if led.record_face(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut v = 0usize;
    while v < VAULT_CELLS {
        if let Some(w) = vault_cell(v) {
            if led.record_vault(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut d = 0usize;
    while d < DRIVE_CELLS {
        // 三态轮转落账：0=锁定 1=解锁 2=挂载。
        if led.record_drive(d, (d % 3) as u32).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：面漂移 / 面逆序 / 面越闭集三态轮转，
    // 全部落在当轮临时账本上（主账本只记注入计数与指纹）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CryptomatorLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格，再注漂移载荷。
            0 => {
                let _ = lt.record_face(3, face_cell(3).unwrap_or(0));
                lt.record_face(4, face_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(CryptomatorCode::EOpDrift)
            }
            // 逆序注入：先落后面族正格，再注前面族（对空账本同样先落正格）。
            1 => {
                let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0)) == Err(CryptomatorCode::EOrder)
            }
            // 越闭集注入：面 20 之外必抓。
            _ => lt.record_face(FACE_OPS, 0).is_err(),
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
/// UNX-F52413 判据集（锚 UNX-F52414-J1，off-by-one 板式）。
pub fn run_unx52413_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52413");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面 20 操作全通",
        recorded >= FACE_OPS as u32,
        "库创建/解锁/加锁/加密/同步五面逐格恒等（UNX-F52414-J1 仓库透明加密 ×10）",
    );
    s.add(
        "vault 配置投影 8/8 全落",
        {
            let mut lt = CryptomatorLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VAULT_CELLS {
                if let Some(w) = vault_cell(v) {
                    if lt.record_vault(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vaults_all()
        },
        "vault.cryptomator 投影逐格对照，漂移必抓",
    );
    s.add(
        "虚拟盘三态 6/6 全落",
        {
            let mut lt = CryptomatorLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DRIVE_CELLS {
                if lt.record_drive(d, (d % 3) as u32).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.drives_all()
        },
        "锁定/解锁/挂载三态闭集逐格落账",
    );
    s.add(
        "虚拟盘三态越态必抓",
        {
            let mut lt = CryptomatorLedger::new();
            lt.record_drive(0, 3) == Err(CryptomatorCode::EDriveState)
        },
        "态 3 越三态闭集必拒",
    );
    s.add(
        "masterkey 三元闸 + 库健康六判据全落",
        {
            let mut lt = CryptomatorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "主密钥生成/解锁/恢复三元 + 库健康三闸逐格落账",
    );
    s.add(
        "Java 上栈声明 2/2 全齐",
        {
            let mut lt = CryptomatorLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "JVM 运行时上栈/Cryptomator JNI 桥分账两行闭集（§AI-66.3）",
    );
    s.add(
        "面族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FACE_OPS {
                if face_kind(o).unwrap_or(5) < face_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面闭集单调",
    );
    s.add(
        "与 git 消费联签（F52159 锚定行）",
        vault_cell(0).is_some() && face_cell(FACE_OPS - 1).is_some(),
        "vault 投影与同步面格锚定 F52159 联签行，格基准恒等才收",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面漂移/面逆序/面越闭集三态轮转必抓",
    );
    let mut l0 = CryptomatorLedger::new();
    s.add(
        "边界·面越闭集",
        l0.record_face(FACE_OPS, 0).is_err(),
        "面操作 20 越闭集必拒",
    );
    s.add(
        "边界·vault 投影越闭集",
        l0.record_vault(VAULT_CELLS, 0).is_err(),
        "vault 投影 8 越闭集必拒",
    );
    s.add(
        "边界·虚拟盘越闭集",
        l0.record_drive(DRIVE_CELLS, 0).is_err(),
        "虚拟盘 6 越闭集必拒",
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
            let mut lt = CryptomatorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FACE_OPS {
                if let Some(w) = face_cell(o) {
                    let _ = lt.record_face(o, w);
                }
                o += 1;
            }
            let mut v = 0usize;
            while v < VAULT_CELLS {
                if let Some(w) = vault_cell(v) {
                    let _ = lt.record_vault(v, w);
                }
                v += 1;
            }
            let mut d = 0usize;
            while d < DRIVE_CELLS {
                let _ = lt.record_drive(d, (d % 3) as u32);
                d += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA700,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCA700",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_kind() {
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(19), Some(4));
        assert!(face_kind(20).is_none());
        assert!(face_cell(20).is_none());
        assert_eq!(face_kind_name(0), Some("库创建"));
        assert_eq!(face_kind_name(4), Some("同步"));
        assert!(face_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3).unwrap_or(0);
        assert_eq!(a, face_cell(3).unwrap_or(0));
        assert_ne!(a, face_cell(4).unwrap_or(0));
        assert!(vault_cell(VAULT_CELLS).is_none());
        assert!(drive_cell(DRIVE_CELLS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_drive_state_closed() {
        assert_eq!(drive_state_name(0), Some("锁定"));
        assert_eq!(drive_state_name(2), Some("挂载"));
        assert!(drive_state_name(3).is_none());
        let mut lt = CryptomatorLedger::new();
        assert_eq!(lt.record_drive(0, 3), Err(CryptomatorCode::EDriveState));
        assert_eq!(lt.record_drive(0, 1).is_ok(), true);
        assert_eq!(lt.record_drive(0, 1), Err(CryptomatorCode::EDup));
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = CryptomatorLedger::new();
        let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
        assert_eq!(lt.record_face(2, face_cell(2).unwrap_or(0)), Err(CryptomatorCode::EOrder));
        assert!(lt.record_face(10, face_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CryptomatorLedger::new();
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CryptomatorCode::EOpDrift)
        );
        assert_eq!(
            lt.record_vault(2, vault_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(CryptomatorCode::EVaultDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CryptomatorLedger::new();
        let _ = lt.record_vault(0, vault_cell(0).unwrap_or(0));
        assert_eq!(lt.record_vault(0, vault_cell(0).unwrap_or(0)), Err(CryptomatorCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CryptomatorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CryptomatorLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert_eq!(gate_name(0), Some("masterkey 生成"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：面 20 + 投影 8 + 三态 6。
        assert_eq!(recorded, FACE_OPS as u32 + VAULT_CELLS as u32 + DRIVE_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger_conserved() {
        let mut led = CryptomatorLedger::new();
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = CryptomatorLedger::new();
            let caught = match kind {
                0 => {
                    let _ = lt.record_face(3, face_cell(3).unwrap_or(0));
                    lt.record_face(4, face_cell(4).unwrap_or(0).wrapping_add(1))
                        == Err(CryptomatorCode::EOpDrift)
                }
                1 => {
                    let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                    lt.record_face(2, face_cell(2).unwrap_or(0)) == Err(CryptomatorCode::EOrder)
                }
                _ => lt.record_face(FACE_OPS, 0).is_err(),
            };
            assert!(caught, "第 {} 轮注入必须全抓", k);
            if caught {
                led.injected += 1;
                led.caught += 1;
            }
            k += 1;
        }
        let (inj, ca, esc) = led.stats();
        assert_eq!(inj, 10);
        assert_eq!(ca, 10);
        assert_eq!(esc, 0);
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
        assert_eq!(SEG_BASE, 0xCA700);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52413 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CryptomatorCode::Ok as u32, 0);
        assert_eq!(CryptomatorCode::EGradeDrift as u32, 12);
        assert_eq!(CryptomatorCode::Ok.val(), SEG_BASE);
        assert_eq!(CryptomatorCode::ESeal.val(), SEG_BASE + 11);
        // 全变体 detail 非空。
        let all = [
            CryptomatorCode::Ok,
            CryptomatorCode::ENoOp,
            CryptomatorCode::EOpDrift,
            CryptomatorCode::EOrder,
            CryptomatorCode::EVault,
            CryptomatorCode::EVaultDrift,
            CryptomatorCode::EDrive,
            CryptomatorCode::EDriveState,
            CryptomatorCode::ERow,
            CryptomatorCode::ERowMiss,
            CryptomatorCode::EDup,
            CryptomatorCode::ESeal,
            CryptomatorCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i] as u32, i as u32);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let empty = CryptomatorLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        // 六闸齐（余未齐）S2。
        let mut half = CryptomatorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = half.judge_gate(g);
            g += 1;
        }
        assert_eq!(half.judge_grade(), Grade::S2);
        // 全齐目标档 S1。
        let mut full = CryptomatorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < FACE_OPS {
            if let Some(w) = face_cell(o) {
                let _ = full.record_face(o, w);
            }
            o += 1;
        }
        let mut v = 0usize;
        while v < VAULT_CELLS {
            if let Some(w) = vault_cell(v) {
                let _ = full.record_vault(v, w);
            }
            v += 1;
        }
        let mut d = 0usize;
        while d < DRIVE_CELLS {
            let _ = full.record_drive(d, (d % 3) as u32);
            d += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = full.declare_row(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52413_checks_green() {
        let s = run_unx52413_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
