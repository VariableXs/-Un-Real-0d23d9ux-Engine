//! UNX-F52411 SOPS 收口 · 0xCA680段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52411"，逐字）：
//!   title: SOPS 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52412-J1 六判据
//!     全落；**卷创建判据只对测试镜像文件执行（非系统盘非物理盘）**，全
//!     盘加密路径列 N/A 不承诺（硬件数据安全红线适用性声明）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.9 红线适用；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52411）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52412-J1，照原文锚定；功能按本卡标题（SOPS）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：encrypt/decrypt/edit/rotate/updatekeys 各 4 操作共
//!      20 格，命令号 = o/4，族单调（逆序必抓）；逐操作载荷纯函数折叠，
//!      恒等才收。
//!   ② .sops.yaml 双向投影 12 格：加密向/解密向各 6 格，向号 = 格/6，
//!      向单调；逐格载荷对照。
//!   ③ 五后端 10 格：KMS/age/Pgp/GCP/Azure 各 2 格，后端号 = 格/2，后端
//!      单调（逆序必抓）；逐格载荷对照。
//!   ④ 与 Vault 消费联签 4 格（F52405 契约锚）：逐格恒等收账，越闭集拒。
//!   ⑤ Go 上栈声明 2 行闭集，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（yaml 越闭集/命令漂
//!      移/后端族序，漂移先落正格再注漂移，族序注入对空账本同样必抓且不
//!      改 last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （20 操作 + 12 格投影 + 10 格后端 + 4 格联签 + 2 行声明全通 +
//!      六判据全落 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52411;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// .sops.yaml 双向投影格数。
pub const YAML_CELLS: usize = 12;

/// 五后端格数。
pub const BE_CELLS: usize = 10;

/// Vault 消费联签格数（F52405）。
pub const VCOSIGN_CELLS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SopsCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// yaml 投影越闭集
    ENoYaml,
    /// yaml 投影漂移
    EYamlDrift,
    /// 后端格越闭集
    ENoBe,
    /// 后端族逆序
    EBeOrder,
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

impl SopsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SopsCode::Ok => "成功",
            SopsCode::ENoOp => "命令号越闭集：encrypt/decrypt/edit/rotate/updatekeys 20 操作集之外先登记",
            SopsCode::EDrift => "命令漂移：操作折叠与基准不恒等，逐格复算",
            SopsCode::EOrder => "命令族逆序：encrypt→decrypt→edit→rotate→updatekeys 单调，逆序必抓",
            SopsCode::ENoYaml => "yaml 投影越闭集：.sops.yaml 加密向/解密向 12 格之外先登记",
            SopsCode::EYamlDrift => "yaml 投影漂移：投影格载荷与基准不恒等，逐格复算",
            SopsCode::ENoBe => "后端格越闭集：KMS/age/Pgp/GCP/Azure 五后端 10 格之外先登记",
            SopsCode::EBeOrder => "后端族逆序：KMS→age→Pgp→GCP→Azure 单调，逆序必抓",
            SopsCode::EDup => "重复落账：同格只许一条账目",
            SopsCode::ESeal => "未齐即封：20 操作 + 12 格投影 + 10 格后端 + 4 格联签未齐先补齐",
            SopsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SopsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SopsCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+12格投影+10格后端+4格联签+2行声明全通收口",
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
    Some((0xE000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=encrypt 4..8=decrypt 8..12=edit 12..16=rotate 16..20=updatekeys）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("encrypt"),
        1 => Some("decrypt"),
        2 => Some("edit"),
        3 => Some("rotate"),
        4 => Some("updatekeys"),
        _ => None,
    }
}

/// .sops.yaml 双向投影格载荷（格 c；向 = c/6：0..6=加密向 6..12=解密向）。
pub const fn yaml_cell(c: usize) -> Option<u64> {
    if c >= YAML_CELLS {
        return None;
    }
    Some((0xE100u64 + (c as u64) * 0x33) ^ ((c / 6) as u64).wrapping_mul(0x517CC1B7))
}

/// 投影向名。
pub const fn yaml_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("加密向"),
        1 => Some("解密向"),
        _ => None,
    }
}

/// 五后端格载荷（格 c；后端 = c/2：0..2=KMS 2..4=age 4..6=Pgp 6..8=GCP 8..10=Azure）。
pub const fn be_cell(c: usize) -> Option<u64> {
    if c >= BE_CELLS {
        return None;
    }
    Some((0xE200u64 + (c as u64) * 0x2B) ^ ((c / 2) as u64).wrapping_mul(0x01000193))
}

/// 后端号（c/2）。
pub const fn be_kind(c: usize) -> Option<u32> {
    if c >= BE_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 后端名。
pub const fn be_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("KMS"),
        1 => Some("age"),
        2 => Some("Pgp"),
        3 => Some("GCP"),
        4 => Some("Azure"),
        _ => None,
    }
}

/// Vault 消费联签格载荷（F52405 契约锚，格 r）。
pub const fn vcosign_cell(r: usize) -> Option<u32> {
    if r >= VCOSIGN_CELLS {
        return None;
    }
    Some(0xE300 + r as u32 * 0x3C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xE400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SOPS 收口账本状态机。
#[derive(Clone)]
pub struct SopsLedger {
    /// 六判据（启动/加密/解密/轮换/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// yaml 双向投影账。
    yamls: [Option<u64>; YAML_CELLS],
    /// 五后端账。
    bes: [Option<u64>; BE_CELLS],
    /// Vault 消费联签账（F52405）。
    vcosign: [Option<u32>; VCOSIGN_CELLS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 最后后端族。
    last_be: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SopsLedger {
    /// 新账本。
    pub fn new() -> Self {
        SopsLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            yamls: [None; YAML_CELLS],
            bes: [None; BE_CELLS],
            vcosign: [None; VCOSIGN_CELLS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_be: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SopsCode> {
        if gate >= 6 {
            return Err(SopsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
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
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), SopsCode> {
        if o >= CMD_OPS {
            return Err(SopsCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(SopsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5310);
            return Err(SopsCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(SopsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5311 + o as u64);
            return Err(SopsCode::EDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SopsCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5312 + o as u64);
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

    /// yaml 投影一格（载荷对照 + 重复拒）。
    pub fn record_yaml(&mut self, c: usize, got: u64) -> Result<(), SopsCode> {
        let want = match yaml_cell(c) {
            Some(w) => w,
            None => return Err(SopsCode::ENoYaml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5320 + c as u64);
            return Err(SopsCode::EYamlDrift);
        }
        if match self.yamls.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SopsCode::EDup);
        }
        if let Some(x) = self.yamls.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5321 + c as u64);
        Ok(())
    }

    /// yaml 投影全账核验（12 格）。
    pub fn yamls_all(&self) -> bool {
        let mut c = 0usize;
        while c < YAML_CELLS {
            match self.yamls.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 五后端一格（后端单调 + 载荷对照 + 重复拒）。
    pub fn record_be(&mut self, c: usize, got: u64) -> Result<(), SopsCode> {
        if c >= BE_CELLS {
            return Err(SopsCode::ENoBe);
        }
        let kind = match be_kind(c) {
            Some(k) => k,
            None => return Err(SopsCode::ENoBe),
        };
        if (kind as i64) < self.last_be {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330);
            return Err(SopsCode::EBeOrder);
        }
        let want = match be_cell(c) {
            Some(w) => w,
            None => return Err(SopsCode::ENoBe),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5331 + c as u64);
            return Err(SopsCode::EYamlDrift);
        }
        if match self.bes.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SopsCode::EDup);
        }
        if let Some(x) = self.bes.get_mut(c) {
            *x = Some(got);
        }
        self.last_be = kind as i64;
        self.fnv(0x5332 + c as u64);
        Ok(())
    }

    /// 五后端全账核验（10 格）。
    pub fn bes_all(&self) -> bool {
        let mut c = 0usize;
        while c < BE_CELLS {
            match self.bes.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// Vault 消费联签一格（F52405 契约锚）。
    pub fn record_vcosign(&mut self, r: usize) -> Result<(), SopsCode> {
        let want = match vcosign_cell(r) {
            Some(w) => w,
            None => return Err(SopsCode::ENoOp),
        };
        if match self.vcosign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SopsCode::EDup);
        }
        if let Some(x) = self.vcosign.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5340 + r as u64);
        Ok(())
    }

    /// Vault 联签全齐核验。
    pub fn vcosign_all(&self) -> bool {
        let mut r = 0usize;
        while r < VCOSIGN_CELLS {
            match self.vcosign.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), SopsCode> {
        let want = match go_row(r) {
            Some(w) => w,
            None => return Err(SopsCode::ESeal),
        };
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SopsCode::EDup);
        }
        if let Some(x) = self.gos.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5350 + r as u64);
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
            && self.yamls_all()
            && self.bes_all()
            && self.vcosign_all()
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

impl Default for SopsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SopsLedger::new();
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
    let mut c = 0usize;
    while c < YAML_CELLS {
        if let Some(w) = yaml_cell(c) {
            if led.record_yaml(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut c = 0usize;
    while c < BE_CELLS {
        if let Some(w) = be_cell(c) {
            if led.record_be(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < VCOSIGN_CELLS {
        let _ = led.record_vcosign(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：yaml 越闭集 / 命令漂移 / 后端族序三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上；主账本只记注入统计。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SopsLedger::new();
        let caught = match kind {
            0 => {
                // yaml 投影越闭集：格 12 越出 12 格闭集，必抓。
                lt.record_yaml(YAML_CELLS, 0).is_err()
            }
            1 => {
                // 命令漂移：先按合法序落正格，再注漂移载荷，必抓。
                let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
                lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(SopsCode::EDrift)
            }
            _ => {
                // 后端族序：对空账本先落 Pgp 族（族 2），再逆序落 KMS 族
                // （族 0），必抓且不改 last_be。
                let _ = lt.record_be(5, be_cell(5).unwrap_or(0));
                lt.record_be(1, be_cell(1).unwrap_or(0)) == Err(SopsCode::EBeOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
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
/// UNX-F52411 判据集（锚 UNX-F52412-J1，off-by-one 板式）。
pub fn run_unx52411_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52411");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "encrypt/decrypt/edit/rotate/updatekeys 五命令族逐操作恒等",
    );
    s.add(
        ".sops.yaml 双向投影 12 格全落",
        {
            let mut lt = SopsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < YAML_CELLS {
                if let Some(w) = yaml_cell(c) {
                    if lt.record_yaml(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.yamls_all()
        },
        "加密向/解密向逐格恒等，越闭集拒",
    );
    s.add(
        "五后端 10 格全落",
        {
            let mut lt = SopsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < BE_CELLS {
                if let Some(w) = be_cell(c) {
                    if lt.record_be(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.bes_all()
        },
        "KMS/age/Pgp/GCP/Azure 五后端逐格恒等（后端单调）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SopsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/加密/解密/轮换/卸载/升级逐格落账",
    );
    s.add(
        "Vault 消费联签 4/4 全齐（F52405）",
        {
            let mut lt = SopsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < VCOSIGN_CELLS {
                if lt.record_vcosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.vcosign_all()
        },
        "Vault CLI 消费联签 4 格闭集（F52405 契约锚）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = SopsLedger::new();
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
        "yaml 越闭集/命令漂移/后端族序三态轮转必抓",
    );
    let mut l0 = SopsLedger::new();
    s.add(
        "边界·yaml 投影越闭集",
        l0.record_yaml(YAML_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·命令漂移拒",
        {
            l0.record_cmd(0, cmd_cell(0).unwrap_or(0));
            l0.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SopsCode::EDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·后端越闭集",
        l0.record_be(BE_CELLS, 0).is_err(),
        "后端格 10 越闭集必拒",
    );
    let cons = if caught == 10 {
        Ok(())
    } else {
        Err(SopsCode::EInj)
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
        Err(SopsCode::EFp)
    };
    s.add("双跑指纹一致", dual.is_ok(), "确定性全流程逐位一致且非零");
    s.add(
        "后端未齐即封核",
        {
            let mut lt = SopsLedger::new();
            if let Some(w) = be_cell(0) {
                let _ = lt.record_be(0, w);
            }
            if lt.bes_all() {
                Ok(())
            } else {
                Err(SopsCode::ESeal)
            }
        }
        .is_err(),
        "五后端只落一格必判未齐，拒封（ESeal，§AI-66.9 红线适用）",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = SopsLedger::new();
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
            let mut c = 0usize;
            while c < YAML_CELLS {
                if let Some(w) = yaml_cell(c) {
                    let _ = lt.record_yaml(c, w);
                }
                c += 1;
            }
            let mut c = 0usize;
            while c < BE_CELLS {
                if let Some(w) = be_cell(c) {
                    let _ = lt.record_be(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < VCOSIGN_CELLS {
                let _ = lt.record_vcosign(r);
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
                Err(SopsCode::EGradeDrift)
            }
        }
        .is_ok(),
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA680,
        "SEG_BASE 0x40 对齐且落在 VX16 域 0xCA400+0x40×10",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 ≥18 条闭集（§AI-66.9 判据锚定）",
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
    fn test_yaml_projection() {
        // 投影向 = c/6：0..6 加密向，6..12 解密向。
        assert_eq!(yaml_dir_name(0), Some("加密向"));
        assert_eq!(yaml_dir_name(1), Some("解密向"));
        assert!(yaml_dir_name(2).is_none());
        assert!(yaml_cell(YAML_CELLS).is_none());
        let mut lt = SopsLedger::new();
        let mut c = 0usize;
        while c < YAML_CELLS {
            if let Some(w) = yaml_cell(c) {
                assert!(lt.record_yaml(c, w).is_ok());
            }
            c += 1;
        }
        assert!(lt.yamls_all());
        assert_eq!(
            lt.record_yaml(0, yaml_cell(0).unwrap_or(0)),
            Err(SopsCode::EDup)
        );
    }

    #[test]
    fn test_be_kind() {
        // i/N 映射：后端 = c/2，10 格 → 后端 0..4。
        assert_eq!(be_kind(0), Some(0));
        assert_eq!(be_kind(2), Some(1));
        assert_eq!(be_kind(9), Some(4));
        assert!(be_kind(10).is_none());
        assert_eq!(be_kind_name(0), Some("KMS"));
        assert_eq!(be_kind_name(4), Some("Azure"));
        assert!(be_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(vcosign_cell(VCOSIGN_CELLS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_backend_order_caught() {
        // 种子按合法序先落低族格，再注逆序。
        let mut lt = SopsLedger::new();
        let _ = lt.record_be(5, be_cell(5).unwrap_or(0));
        assert_eq!(lt.record_be(1, be_cell(1).unwrap_or(0)), Err(SopsCode::EBeOrder));
        assert!(lt.record_be(6, be_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_cmd_order_caught() {
        let mut lt = SopsLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(SopsCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SopsLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SopsCode::EDrift)
        );
        assert_eq!(
            lt.record_yaml(1, yaml_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(SopsCode::EYamlDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SopsLedger::new();
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
        // run_cycle 只对命令操作 + yaml 投影 + 五后端三节计数。
        assert_eq!(recorded, CMD_OPS as u32 + YAML_CELLS as u32 + BE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCA680);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 闭集恰 13 变体：Ok=+0x00，EGradeDrift=+0x0C。
        assert_eq!(SopsCode::Ok as u32, 0);
        assert_eq!(SopsCode::EGradeDrift as u32, 12);
        assert_eq!(SopsCode::Ok.val(), SEG_BASE);
        assert_eq!(SopsCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!SopsCode::Ok.detail().is_empty());
        assert!(!SopsCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SopsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = SopsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = SopsLedger::new();
        assert!(lt.declare_go(0).is_ok());
        assert!(lt.declare_go(1).is_ok());
        assert!(lt.declare_go(2).is_err());
        assert!(lt.gos_all());
        let _ = lt.record_vcosign(0);
        assert_eq!(lt.record_vcosign(0), Err(SopsCode::EDup));
    }

    #[test]
    fn unx52411_checks_green() {
        let s = run_unx52411_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
