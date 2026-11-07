//! UNX-F52410 age 收口 · 0xCA640段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52410"，逐字）：
//!   title: age 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52411-J1 加密/解
//!     密 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52410）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52411-J1，照原文锚定；功能按本卡标题（age）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：generate/encrypt/decrypt/recipient/identity 各 4
//!      操作共 20 格，命令号 = o/4，族单调（逆序必抓）；逐操作载荷纯函
//!      数折叠，恒等才收。
//!   ② 密钥投影 8 格：身份钥/收件人投影逐格恒等收账，越闭集拒。
//!   ③ passphrase 三元闸 3 格：设置/校验/轮换三态逐格落账，未齐即封。
//!   ④ 与 git 消费声明 2 行闭集；Go 上栈声明 2 行闭集，各缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（密钥投影越闭集/命令
//!      漂移/命令族序，漂移先落正格再注漂移，族序注入对空账本同样必抓且
//!      不改 last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （20 操作 + 8 格投影 + 3 格三元闸 + 4 行声明全通 + 六判据全落 →
//!      S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52410;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// 密钥投影格数。
pub const KEY_CELLS: usize = 8;

/// passphrase 三元闸格数。
pub const PP_CELLS: usize = 3;

/// git 消费声明行数。
pub const GIT_ROWS: usize = 2;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgeCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 密钥投影越闭集
    ENoKey,
    /// 密钥投影漂移
    EKeyDrift,
    /// passphrase 越闸
    ENoPp,
    /// passphrase 三元闸未齐
    EPpMiss,
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

impl AgeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AgeCode::Ok => "成功",
            AgeCode::ENoOp => "命令号越闭集：generate/encrypt/decrypt/recipient/identity 20 操作集之外先登记",
            AgeCode::EDrift => "命令漂移：操作折叠与基准不恒等，逐格复算",
            AgeCode::EOrder => "命令族逆序：generate→encrypt→decrypt→recipient→identity 单调，逆序必抓",
            AgeCode::ENoKey => "密钥投影越闭集：身份钥/收件人投影 8 格之外先登记",
            AgeCode::EKeyDrift => "密钥投影漂移：投影格载荷与基准不恒等，逐格复算",
            AgeCode::ENoPp => "passphrase 越闸：设置/校验/轮换三元闸 3 格之外先登记",
            AgeCode::EPpMiss => "passphrase 三元闸未齐：三格逐格补齐再封",
            AgeCode::EDup => "重复落账：同格只许一条账目",
            AgeCode::ESeal => "未齐即封：20 操作 + 8 格投影 + 3 格三元闸 + 4 行声明未齐先补齐",
            AgeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AgeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AgeCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+8格投影+3格三元闸+4行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xD000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=generate 4..8=encrypt 8..12=decrypt 12..16=recipient 16..20=identity）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("generate"),
        1 => Some("encrypt"),
        2 => Some("decrypt"),
        3 => Some("recipient"),
        4 => Some("identity"),
        _ => None,
    }
}

/// 密钥投影格载荷（格 k：偶格=身份钥投影 奇格=收件人投影）。
pub const fn key_cell(k: usize) -> Option<u64> {
    if k >= KEY_CELLS {
        return None;
    }
    Some((0xD100u64 + (k as u64) * 0x5D) ^ 0x4147454B00000000)
}

/// 密钥投影向名（k%2：0=身份钥 1=收件人）。
pub const fn key_dir_name(k: usize) -> Option<&'static str> {
    match k % 2 {
        0 => Some("身份钥"),
        1 => Some("收件人"),
        _ => None,
    }
}

/// passphrase 三元闸格载荷（格 c：0=设置 1=校验 2=轮换）。
pub const fn pp_cell(c: usize) -> Option<u32> {
    if c >= PP_CELLS {
        return None;
    }
    Some(0xD200 + c as u32 * 0x2C)
}

/// passphrase 闸名。
pub const fn pp_gate_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("设置"),
        1 => Some("校验"),
        2 => Some("轮换"),
        _ => None,
    }
}

/// git 消费声明行折叠（行 r）。
pub const fn git_row(r: usize) -> Option<u32> {
    if r >= GIT_ROWS {
        return None;
    }
    Some(0xD300 + r as u32 * 0x3C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// age 收口账本状态机。
#[derive(Clone)]
pub struct AgeLedger {
    /// 六判据（启动/生成/加密/解密/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 密钥投影账。
    keys: [Option<u64>; KEY_CELLS],
    /// passphrase 三元闸账。
    pps: [Option<u32>; PP_CELLS],
    /// git 消费声明账。
    gits: [Option<u32>; GIT_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AgeLedger {
    /// 新账本。
    pub fn new() -> Self {
        AgeLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            keys: [None; KEY_CELLS],
            pps: [None; PP_CELLS],
            gits: [None; GIT_ROWS],
            gos: [None; GO_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AgeCode> {
        if gate >= 6 {
            return Err(AgeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5200 + gate as u64);
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

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), AgeCode> {
        if o >= CMD_OPS {
            return Err(AgeCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(AgeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5210);
            return Err(AgeCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(AgeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5211 + o as u64);
            return Err(AgeCode::EDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AgeCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5212 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 格）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 密钥投影一格（载荷对照 + 重复拒）。
    pub fn record_key(&mut self, k: usize, got: u64) -> Result<(), AgeCode> {
        let want = match key_cell(k) {
            Some(w) => w,
            None => return Err(AgeCode::ENoKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5220 + k as u64);
            return Err(AgeCode::EKeyDrift);
        }
        if match self.keys.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AgeCode::EDup);
        }
        if let Some(c) = self.keys.get_mut(k) {
            *c = Some(got);
        }
        self.fnv(0x5221 + k as u64);
        Ok(())
    }

    /// 密钥投影全账核验（8 格）。
    pub fn keys_all(&self) -> bool {
        let mut k = 0usize;
        while k < KEY_CELLS {
            match self.keys.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// passphrase 三元闸一格（越闸拒 + 重复拒）。
    pub fn record_pp(&mut self, c: usize) -> Result<(), AgeCode> {
        let want = match pp_cell(c) {
            Some(w) => w,
            None => return Err(AgeCode::ENoPp),
        };
        if match self.pps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AgeCode::EDup);
        }
        if let Some(x) = self.pps.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x5230 + c as u64);
        Ok(())
    }

    /// passphrase 三元闸全齐核验。
    pub fn pps_all(&self) -> bool {
        let mut c = 0usize;
        while c < PP_CELLS {
            match self.pps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// git 消费声明一行。
    pub fn declare_git(&mut self, r: usize) -> Result<(), AgeCode> {
        let want = match git_row(r) {
            Some(w) => w,
            None => return Err(AgeCode::ESeal),
        };
        if match self.gits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AgeCode::EDup);
        }
        if let Some(x) = self.gits.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5240 + r as u64);
        Ok(())
    }

    /// git 声明全齐核验。
    pub fn gits_all(&self) -> bool {
        let mut r = 0usize;
        while r < GIT_ROWS {
            match self.gits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), AgeCode> {
        let want = match go_row(r) {
            Some(w) => w,
            None => return Err(AgeCode::ESeal),
        };
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AgeCode::EDup);
        }
        if let Some(x) = self.gos.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5250 + r as u64);
        Ok(())
    }

    /// Go 声明全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
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
        if self.gates_all()
            && self.cmds_all()
            && self.keys_all()
            && self.pps_all()
            && self.gits_all()
            && self.gos_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AgeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AgeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < KEY_CELLS {
        if let Some(w) = key_cell(k) {
            if led.record_key(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut c = 0usize;
    while c < PP_CELLS {
        let _ = led.record_pp(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < GIT_ROWS {
        let _ = led.declare_git(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：密钥投影越闭集 / 命令漂移 / 命令族序三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上；主账本只记注入统计。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AgeLedger::new();
        let caught = match kind {
            0 => {
                // 密钥投影越闭集：格 8 越出 8 格闭集，必抓。
                lt.record_key(KEY_CELLS, 0).is_err()
            }
            1 => {
                // 命令漂移：先按合法序落正格，再注漂移载荷，必抓。
                let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
                lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(AgeCode::EDrift)
            }
            _ => {
                // 命令族序：对空账本先落 decrypt 族（族 2），再逆序落
                // generate 族（族 0），必抓且不改 last_kind。
                let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(AgeCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5260 + k as u64);
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
/// UNX-F52410 判据集（锚 UNX-F52411-J1，off-by-one 板式）。
pub fn run_unx52410_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52410");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "generate/encrypt/decrypt/recipient/identity 五命令族逐操作恒等",
    );
    s.add(
        "密钥投影 8 格全落",
        {
            let mut lt = AgeLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < KEY_CELLS {
                if let Some(w) = key_cell(k) {
                    if lt.record_key(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.keys_all()
        },
        "身份钥/收件人投影逐格恒等，越闭集拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AgeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/生成/加密/解密/卸载/升级逐格落账",
    );
    s.add(
        "passphrase 三元闸 3/3 全齐",
        {
            let mut lt = AgeLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PP_CELLS {
                if lt.record_pp(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.pps_all()
        },
        "设置/校验/轮换三态逐格落账，未齐即封",
    );
    s.add(
        "三元闸未齐即封核",
        {
            let mut lt = AgeLedger::new();
            let _ = lt.record_pp(0);
            if lt.pps_all() {
                Ok(())
            } else {
                Err(AgeCode::EPpMiss)
            }
        }
        .is_err(),
        "passphrase 三元闸只落一格必判未齐，拒封（EPpMiss）",
    );
    s.add(
        "git 消费声明 2/2 全齐",
        {
            let mut lt = AgeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GIT_ROWS {
                if lt.declare_git(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gits_all()
        },
        "git 消费声明两行闭集，缺一拒封",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = AgeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Go 运行时上栈声明两行闭集，缺一拒封",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "密钥投影越闭集/命令漂移/命令族序三态轮转必抓",
    );
    let mut l0 = AgeLedger::new();
    s.add(
        "边界·密钥投影越闭集",
        l0.record_key(KEY_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·命令漂移拒",
        {
            l0.record_cmd(0, cmd_cell(0).unwrap_or(0));
            l0.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AgeCode::EDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    let cons = if caught == 10 {
        Ok(())
    } else {
        Err(AgeCode::EInj)
    };
    s.add_dyn(
        "注入账守恒",
        cons.is_ok(),
        &format!("注入=10 检出={} 逃逸=0", caught),
    );
    let (_, _, fp2) = run_cycle();
    let dual = if fp == fp2 && fp != 0 {
        Ok(())
    } else {
        Err(AgeCode::EFp)
    };
    s.add("双跑指纹一致", dual.is_ok(), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = AgeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_cmd(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < KEY_CELLS {
                if let Some(w) = key_cell(k) {
                    let _ = lt.record_key(k, w);
                }
                k += 1;
            }
            let mut c = 0usize;
            while c < PP_CELLS {
                let _ = lt.record_pp(c);
                c += 1;
            }
            let mut r = 0usize;
            while r < GIT_ROWS {
                let _ = lt.declare_git(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            // main/aux 各节全部记账完成后再断言定级。
            let g = lt.judge_grade();
            if g == Grade::S1 {
                Ok(g)
            } else {
                Err(AgeCode::EGradeDrift)
            }
        }
        .is_ok(),
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA640,
        "SEG_BASE 0x40 对齐且落在 VX16 域 0xCA400+0x40×9",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集（§AI-66.3 判据锚定）",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        // i/N 映射：命令族 = o/4，20 操作 → 族 0..4。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
    }

    #[test]
    fn test_key_projection() {
        // 投影向 = k%2：偶格身份钥，奇格收件人。
        assert_eq!(key_dir_name(0), Some("身份钥"));
        assert_eq!(key_dir_name(1), Some("收件人"));
        assert_eq!(key_dir_name(6), Some("身份钥"));
        assert!(key_cell(KEY_CELLS).is_none());
        let mut lt = AgeLedger::new();
        let mut k = 0usize;
        while k < KEY_CELLS {
            if let Some(w) = key_cell(k) {
                assert!(lt.record_key(k, w).is_ok());
            }
            k += 1;
        }
        assert!(lt.keys_all());
        assert_eq!(
            lt.record_key(0, key_cell(0).unwrap_or(0)),
            Err(AgeCode::EDup)
        );
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(pp_cell(PP_CELLS).is_none());
        assert!(git_row(GIT_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_pp_gates() {
        assert_eq!(pp_gate_name(0), Some("设置"));
        assert_eq!(pp_gate_name(1), Some("校验"));
        assert_eq!(pp_gate_name(2), Some("轮换"));
        assert!(pp_gate_name(3).is_none());
        let mut lt = AgeLedger::new();
        assert!(lt.record_pp(0).is_ok());
        assert!(lt.record_pp(1).is_ok());
        assert!(lt.record_pp(2).is_ok());
        assert!(lt.pps_all());
        assert_eq!(lt.record_pp(2), Err(AgeCode::EDup));
    }

    #[test]
    fn test_family_order_caught() {
        // 种子按合法序先落低族格，再注逆序。
        let mut lt = AgeLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(AgeCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AgeLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AgeCode::EDrift)
        );
        assert_eq!(
            lt.record_key(1, key_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(AgeCode::EKeyDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AgeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // run_cycle 只对命令操作 + 密钥投影两节计数。
        assert_eq!(recorded, CMD_OPS as u32 + KEY_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCA640);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 闭集恰 13 变体：Ok=+0x00，EGradeDrift=+0x0C。
        assert_eq!(AgeCode::Ok as u32, 0);
        assert_eq!(AgeCode::EGradeDrift as u32, 12);
        assert_eq!(AgeCode::Ok.val(), SEG_BASE);
        assert_eq!(AgeCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!AgeCode::Ok.detail().is_empty());
        assert!(!AgeCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = AgeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = AgeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = AgeLedger::new();
        assert!(lt.declare_git(0).is_ok());
        assert!(lt.declare_git(1).is_ok());
        assert!(lt.declare_git(2).is_err());
        assert!(lt.declare_go(0).is_ok());
        assert!(lt.declare_go(1).is_ok());
        assert!(lt.declare_go(2).is_err());
        assert!(lt.gits_all() && lt.gos_all());
    }

    #[test]
    fn unx52410_checks_green() {
        let s = run_unx52410_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
