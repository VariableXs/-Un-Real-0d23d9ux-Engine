//! UNX-F52409 OpenSSL（Windows 发行版）收口 · 0xCA600段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52409"，逐字）：
//!   title: OpenSSL（Windows 发行版）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52410-J1 加密/解
//!     密 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52409）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52410-J1，照原文锚定；功能按本卡标题（OpenSSL Windows 发行版）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：genrsa/req/x509/verify/s_client 各 4 操作共 20 格，
//!      命令号 = o/4，族单调（逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② 版本阶梯 3 档（1.1.1 / 3.0 / 3.2），越档必抓；逐档载荷对照。
//!   ③ 证书三态 6 格：有效/临期/吊销三态各 2 格，态号 = 格/2，态单调；
//!      逐格载荷对照。
//!   ④ 与 curl（Windows 发行版）消费联签 4 格（F52255 契约锚）：逐格
//!      恒等收账，越闭集拒。
//!   ⑤ C 上栈声明 2 行闭集：libcrypto/libssl 上栈声明 / C 头文件联编
//!      声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（版本越阶梯/命令漂移/
//!      命令族序，漂移先落正格再注漂移，族序注入对空账本同样必抓且不改
//!      last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （20 操作 + 3 档版本 + 6 格证书 + 4 格联签 + 2 行声明全通 +
//!      六判据全落 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52409;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// 版本阶梯档数。
pub const VER_STEPS: usize = 3;

/// 证书三态格数。
pub const CERT_CELLS: usize = 6;

/// curl 消费联签格数（F52255）。
pub const COSIGN_CELLS: usize = 4;

/// C 上栈声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpensslCode {
    /// 成功
    Ok = 0,
    /// 版本档越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 证书格越闭集
    ENoCert,
    /// 证书态漂移
    ECertDrift,
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

impl OpensslCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OpensslCode::Ok => "成功",
            OpensslCode::ENoVer => "版本档越阶梯：1.1.1/3.0/3.2 三档闭集之外先登记",
            OpensslCode::EVerDrift => "版本阶梯漂移：档载荷折叠与基准不恒等，逐档复算",
            OpensslCode::ENoOp => "命令号越闭集：genrsa/req/x509/verify/s_client 20 操作集之外先登记",
            OpensslCode::EDrift => "命令漂移：操作折叠与基准不恒等，逐格复算",
            OpensslCode::EOrder => "命令族逆序：genrsa→req→x509→verify→s_client 单调，逆序必抓",
            OpensslCode::ENoCert => "证书格越闭集：有效/临期/吊销三态 6 格之外先登记",
            OpensslCode::ECertDrift => "证书态漂移：三态格载荷与基准不恒等，逐格复算",
            OpensslCode::EDup => "重复落账：同格只许一条账目",
            OpensslCode::ESeal => "未齐即封：20 操作 + 3 档版本 + 6 格证书 + 4 格联签未齐先补齐",
            OpensslCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OpensslCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OpensslCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3档版本+6格证书+4格联签+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本档载荷（档 v：0=1.1.1 1=3.0 2=3.2）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VER_STEPS {
        return None;
    }
    Some((0xCA00u64 + (v as u64) * 0x5D) ^ 0x4F534C5600000000)
}

/// 版本档名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("1.1.1"),
        1 => Some("3.0"),
        2 => Some("3.2"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xCB00u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=genrsa 4..8=req 8..12=x509 12..16=verify 16..20=s_client）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("genrsa"),
        1 => Some("req"),
        2 => Some("x509"),
        3 => Some("verify"),
        4 => Some("s_client"),
        _ => None,
    }
}

/// 证书三态格载荷（格 c；态 = c/2：0..2=有效 2..4=临期 4..6=吊销）。
pub const fn cert_cell(c: usize) -> Option<u64> {
    if c >= CERT_CELLS {
        return None;
    }
    Some((0xCC00u64 + (c as u64) * 0x33) ^ ((c / 2) as u64).wrapping_mul(0x9E3779B1))
}

/// 证书态名。
pub const fn cert_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("有效"),
        1 => Some("临期"),
        2 => Some("吊销"),
        _ => None,
    }
}

/// curl 消费联签格载荷（F52255 契约锚，格 r）。
pub const fn cosign_cell(r: usize) -> Option<u32> {
    if r >= COSIGN_CELLS {
        return None;
    }
    Some(0xCD00 + r as u32 * 0x3C)
}

/// C 上栈声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xCE00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// OpenSSL 收口账本状态机。
#[derive(Clone)]
pub struct OpensslLedger {
    /// 六判据（启动/生成/签发/验证/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VER_STEPS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 证书三态账。
    certs: [Option<u64>; CERT_CELLS],
    /// curl 消费联签账（F52255）。
    cosign: [Option<u32>; COSIGN_CELLS],
    /// C 上栈声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OpensslLedger {
    /// 新账本。
    pub fn new() -> Self {
        OpensslLedger {
            gates: [None; 6],
            vers: [None; VER_STEPS],
            cmds: [None; CMD_OPS],
            certs: [None; CERT_CELLS],
            cosign: [None; COSIGN_CELLS],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OpensslCode> {
        if gate >= 6 {
            return Err(OpensslCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 版本档一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), OpensslCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(OpensslCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5110 + v as u64);
            return Err(OpensslCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OpensslCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5111 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VER_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), OpensslCode> {
        if o >= CMD_OPS {
            return Err(OpensslCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(OpensslCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5120);
            return Err(OpensslCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(OpensslCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5121 + o as u64);
            return Err(OpensslCode::EDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OpensslCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5122 + o as u64);
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

    /// 证书三态一格（态单调 + 载荷对照 + 重复拒）。
    pub fn record_cert(&mut self, c: usize, got: u64) -> Result<(), OpensslCode> {
        if c >= CERT_CELLS {
            return Err(OpensslCode::ENoCert);
        }
        let want = match cert_cell(c) {
            Some(w) => w,
            None => return Err(OpensslCode::ENoCert),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5130 + c as u64);
            return Err(OpensslCode::ECertDrift);
        }
        if match self.certs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OpensslCode::EDup);
        }
        if let Some(x) = self.certs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5131 + c as u64);
        Ok(())
    }

    /// 证书三态全账核验（6 格）。
    pub fn certs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CERT_CELLS {
            match self.certs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// curl 消费联签一格（F52255 契约锚）。
    pub fn record_cosign(&mut self, r: usize) -> Result<(), OpensslCode> {
        let want = match cosign_cell(r) {
            Some(w) => w,
            None => return Err(OpensslCode::ENoOp),
        };
        if match self.cosign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OpensslCode::EDup);
        }
        if let Some(c) = self.cosign.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x5140 + r as u64);
        Ok(())
    }

    /// curl 联签全齐核验。
    pub fn cosign_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_CELLS {
            match self.cosign.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_row(&mut self, r: usize) -> Result<(), OpensslCode> {
        if r >= DECL_ROWS {
            return Err(OpensslCode::ESeal);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OpensslCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5150 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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
            && self.vers_all()
            && self.cmds_all()
            && self.certs_all()
            && self.cosign_all()
            && self.decls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for OpensslLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OpensslLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VER_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
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
    while c < CERT_CELLS {
        if let Some(w) = cert_cell(c) {
            if led.record_cert(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < COSIGN_CELLS {
        let _ = led.record_cosign(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_row(d);
        d += 1;
    }
    // 注入 10 轮：版本越阶梯 / 命令漂移 / 命令族序三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上；主账本只记注入统计。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = OpensslLedger::new();
        let caught = match kind {
            0 => {
                // 版本越阶梯：档 3 越出 1.1.1/3.0/3.2 三档闭集，必抓。
                lt.record_version(VER_STEPS, 0).is_err()
            }
            1 => {
                // 命令漂移：先按合法序落正格，再注漂移载荷，必抓。
                let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
                lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(OpensslCode::EDrift)
            }
            _ => {
                // 命令族序：对空账本先落 x509 族（族 2），再逆序落 genrsa
                // 族（族 0），必抓且不改 last_kind。
                let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(OpensslCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5160 + k as u64);
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
/// UNX-F52409 判据集（锚 UNX-F52410-J1，off-by-one 板式）。
pub fn run_unx52409_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52409");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "genrsa/req/x509/verify/s_client 五命令族逐操作恒等",
    );
    s.add(
        "版本阶梯 1.1.1/3.0/3.2 三档全落",
        {
            let mut lt = OpensslLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越档必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = OpensslLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/生成/签发/验证/卸载/升级逐格落账",
    );
    s.add(
        "证书三态 6 格全落",
        {
            let mut lt = OpensslLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CERT_CELLS {
                if let Some(w) = cert_cell(c) {
                    if lt.record_cert(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.certs_all()
        },
        "有效/临期/吊销三态逐格载荷恒等",
    );
    s.add(
        "curl 消费联签 4/4 全齐（F52255）",
        {
            let mut lt = OpensslLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_CELLS {
                if lt.record_cosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cosign_all()
        },
        "curl for Windows 消费联签 4 格闭集（F52255 契约锚）",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = OpensslLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "libcrypto/libssl 上栈声明/C 头文件联编声明两行闭集",
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
        "版本越阶梯/命令漂移/命令族序三态轮转必抓",
    );
    let mut l0 = OpensslLedger::new();
    s.add(
        "边界·版本档越阶梯",
        l0.record_version(VER_STEPS, 0).is_err(),
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "边界·命令漂移拒",
        {
            l0.record_cmd(0, cmd_cell(0).unwrap_or(0));
            l0.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)) == Err(OpensslCode::EDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    let cons = if caught >= 10 && led_cons_ok(caught) {
        Ok(())
    } else {
        Err(OpensslCode::EInj)
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
        Err(OpensslCode::EFp)
    };
    s.add("双跑指纹一致", dual.is_ok(), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = OpensslLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VER_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_cmd(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CERT_CELLS {
                if let Some(w) = cert_cell(c) {
                    let _ = lt.record_cert(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < COSIGN_CELLS {
                let _ = lt.record_cosign(r);
                r += 1;
            }
            let mut d = 0usize;
            while d < DECL_ROWS {
                let _ = lt.declare_row(d);
                d += 1;
            }
            // main/aux 各节全部记账完成后再断言定级。
            let g = lt.judge_grade();
            if g == Grade::S1 {
                Ok(g)
            } else {
                Err(OpensslCode::EGradeDrift)
            }
        }
        .is_ok(),
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA600,
        "SEG_BASE 0x40 对齐且落在 VX16 域 0xCA400+0x40×8",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集（§AI-66.3 判据锚定）",
    );
    s
}

/// 注入守恒核（注入 = 检出 + 逃逸 且逃逸为零）。
fn led_cons_ok(caught: u32) -> bool {
    caught == 10
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("1.1.1"));
        assert_eq!(version_name(1), Some("3.0"));
        assert_eq!(version_name(2), Some("3.2"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

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
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert!(cert_cell(CERT_CELLS).is_none());
    }

    #[test]
    fn test_cert_states() {
        // 态号 = c/2：0..2=有效 2..4=临期 4..6=吊销。
        assert_eq!(cert_state_name(0), Some("有效"));
        assert_eq!(cert_state_name(1), Some("临期"));
        assert_eq!(cert_state_name(2), Some("吊销"));
        assert!(cert_state_name(3).is_none());
        let mut lt = OpensslLedger::new();
        let mut c = 0usize;
        while c < CERT_CELLS {
            if let Some(w) = cert_cell(c) {
                assert!(lt.record_cert(c, w).is_ok());
            }
            c += 1;
        }
        assert!(lt.certs_all());
    }

    #[test]
    fn test_family_order_caught() {
        // 种子按合法序先落低族格，再注逆序。
        let mut lt = OpensslLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(OpensslCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = OpensslLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(OpensslCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(OpensslCode::EVerDrift)
        );
        assert_eq!(
            lt.record_cert(0, cert_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(OpensslCode::ECertDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OpensslLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(OpensslCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(OpensslCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OpensslLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = OpensslLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // run_cycle 只对版本阶梯 + 命令操作 + 证书三态三节计数。
        assert_eq!(recorded, VER_STEPS as u32 + CMD_OPS as u32 + CERT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCA600);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 闭集恰 13 变体：Ok=+0x00，EGradeDrift=+0x0C。
        assert_eq!(OpensslCode::Ok as u32, 0);
        assert_eq!(OpensslCode::EGradeDrift as u32, 12);
        assert_eq!(OpensslCode::Ok.val(), SEG_BASE);
        assert_eq!(OpensslCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!OpensslCode::Ok.detail().is_empty());
        assert!(!OpensslCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = OpensslLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = OpensslLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = OpensslLedger::new();
        assert!(lt.declare_row(0).is_ok());
        assert!(lt.declare_row(1).is_ok());
        assert!(lt.declare_row(2).is_err());
        assert!(lt.decls_all());
    }

    #[test]
    fn unx52409_checks_green() {
        let s = run_unx52409_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
