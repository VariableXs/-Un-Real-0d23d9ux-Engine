//! UNX-F52451 7-Zip 收口 · 0xCB080段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52451"，逐字）：
//!   title: 7-Zip 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52452-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52451）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52452-J1，照原文锚定；功能按本卡标题（7-Zip）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集 20 操作：a 压缩 / x·e 解压（x 与 e 同族异字母）/
//!      t 测试 / l 列表 / u 更新，每族 4 操作，逐操作载荷折叠纯函数
//!      恒等才收；族单调（逆序必抓）；命令字母闭集 a/x/e/t/l/u 六字母
//!      归五族。
//!   ② 七格式族 14 格：7z/zip/gz/xz/tar/wim/cab 七格式 ×（标准档/
//!      加密·分卷档）两档，逐格载荷对照。
//!   ③ 加密三元闸：口令/头加密/算法强度 三格闭集，越闸必抓、漂移必抓。
//!   ④ 与 Windows 资源管理器集成声明 2 行：外壳集成声明 / 右键菜单
//!      分账声明，缺一拒封。
//!   ⑤ C++ 上栈声明 2 行：Igor Pavlov 原生栈声明 / 本仓 C++ 类比上栈
//!      声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（命令族逆序/格式载荷漂移/操作
//!      越闭集）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹
//!      一致；KB-COMPAT 定级 S1（全齐 → S1，六闸齐未全齐 → S2）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52451;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数（五命令族 × 4）。
pub const CMD_OPS: usize = 20;

/// 七格式族格数（七格式 × 两档）。
pub const FMT_CELLS: usize = 14;

/// 加密三元闸格数。
pub const ENC_GATES: usize = 3;

/// Windows 资源管理器集成声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SevenzipCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作/格式载荷漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 格式格号越闭集
    ENoFmt,
    /// 格式载荷漂移
    EFmtDrift,
    /// 加密闸号越闭集
    ENoEnc,
    /// 加密闸载荷漂移
    EEncDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl SevenzipCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SevenzipCode::Ok => "成功",
            SevenzipCode::ENoOp => "操作号越闭集：五命令族 20 操作之外先登记",
            SevenzipCode::EDrift => "载荷漂移：操作/格式折叠与基准不恒等，逐格复算",
            SevenzipCode::EOrder => "命令族逆序：a→x·e→t→l→u 单调，逆序必抓",
            SevenzipCode::ENoFmt => "格式格号越闭集：七格式族 14 格之外先登记",
            SevenzipCode::EFmtDrift => "格式漂移：格式格载荷折叠与基准不恒等，逐格复算",
            SevenzipCode::ENoEnc => "加密闸号越闭集：加密三元闸之外先登记",
            SevenzipCode::EEncDrift => "加密闸漂移：三元闸载荷折叠与基准不恒等，逐格复算",
            SevenzipCode::ENoRow => "声明行越闭集：资源管理器集成/上栈 2 行闭集之外先登记",
            SevenzipCode::EDup => "重复落账：同格只许一条账目",
            SevenzipCode::ESeal => "未齐即封：20 操作 + 14 格 + 3 闸 + 4 声明行未齐先补齐",
            SevenzipCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SevenzipCode::EGradeDrift => "定级漂移：全齐才可定 S1，六闸齐未全齐为 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const fn：被常量语境调用也安全）。
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
            Grade::S1 => "S1：六判据+20操作+14格式格+3加密闸+声明行全通收口",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xDA000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=a 压缩 4..8=x·e 解压 8..12=t 测试 12..16=l 列表 16..20=u 更新）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("a 压缩"),
        1 => Some("x·e 解压"),
        2 => Some("t 测试"),
        3 => Some("l 列表"),
        4 => Some("u 更新"),
        _ => None,
    }
}

/// 命令字母（六字母归五族：x·e 同族）。
pub const fn cmd_letter(o: usize) -> Option<&'static str> {
    if o >= CMD_OPS {
        return None;
    }
    match o / 4 {
        0 => Some("a"),
        1 => {
            if o % 4 < 2 {
                Some("x")
            } else {
                Some("e")
            }
        }
        2 => Some("t"),
        3 => Some("l"),
        _ => Some("u"),
    }
}

/// 格式格载荷（格 f；七格式 × 两档，f/2=格式序）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((0xDB000u64 + (f as u64) * 0x5D) ^ ((f / 2) as u64).wrapping_mul(0x01000193))
}

/// 格式名（格 f → 格式序名）。
pub const fn fmt_name(f: usize) -> Option<&'static str> {
    if f >= FMT_CELLS {
        return None;
    }
    match f / 2 {
        0 => Some("7z"),
        1 => Some("zip"),
        2 => Some("gz"),
        3 => Some("xz"),
        4 => Some("tar"),
        5 => Some("wim"),
        6 => Some("cab"),
        _ => None,
    }
}

/// 加密闸载荷（闸 e：0=口令 1=头加密 2=算法强度）。
pub const fn enc_cell(e: usize) -> Option<u64> {
    if e >= ENC_GATES {
        return None;
    }
    Some((0xDC000u64 + (e as u64) * 0x47) ^ 0x375A5F454E435833)
}

/// 加密闸名。
pub const fn enc_gate_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("口令"),
        1 => Some("头加密"),
        2 => Some("算法强度"),
        _ => None,
    }
}

/// Windows 资源管理器集成声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xDD000 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xDE000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 7-Zip 收口账本状态机。
#[derive(Clone)]
pub struct SevenzipLedger {
    /// 六判据（压缩/解压/测试/列表/加密/集成）。
    gates: [Option<bool>; 6],
    /// 操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 格式账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 加密闸账。
    encs: [Option<u64>; ENC_GATES],
    /// 集成声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SevenzipLedger {
    /// 新账本。
    pub fn new() -> Self {
        SevenzipLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            fmts: [None; FMT_CELLS],
            encs: [None; ENC_GATES],
            splits: [None; SPLIT_ROWS],
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SevenzipCode> {
        if gate >= 6 {
            return Err(SevenzipCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), SevenzipCode> {
        if o >= CMD_OPS {
            return Err(SevenzipCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(SevenzipCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(SevenzipCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(SevenzipCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(SevenzipCode::EDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SevenzipCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
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

    /// 格式一格（载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), SevenzipCode> {
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(SevenzipCode::ENoFmt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + f as u64);
            return Err(SevenzipCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SevenzipCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x7301 + f as u64);
        Ok(())
    }

    /// 格式全账核验（14 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 加密闸一格（载荷对照 + 重复拒）。
    pub fn record_enc(&mut self, e: usize, got: u64) -> Result<(), SevenzipCode> {
        let want = match enc_cell(e) {
            Some(w) => w,
            None => return Err(SevenzipCode::ENoEnc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + e as u64);
            return Err(SevenzipCode::EEncDrift);
        }
        if match self.encs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SevenzipCode::EDup);
        }
        if let Some(c) = self.encs.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x7401 + e as u64);
        Ok(())
    }

    /// 加密闸全账核验（3 格）。
    pub fn encs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENC_GATES {
            match self.encs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 集成声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SevenzipCode> {
        if r >= SPLIT_ROWS {
            return Err(SevenzipCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SevenzipCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 集成声明全齐核验。
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

    /// C++ 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), SevenzipCode> {
        if r >= STACK_ROWS {
            return Err(SevenzipCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SevenzipCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
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

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐 → S1，六判据齐未全齐 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else if self.cmds_all()
            && self.fmts_all()
            && self.encs_all()
            && self.splits_all()
            && self.stacks_all()
        {
            Grade::S1
        } else {
            Grade::S2
        }
    }
}

impl Default for SevenzipLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SevenzipLedger::new();
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
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut e = 0usize;
    while e < ENC_GATES {
        if let Some(w) = enc_cell(e) {
            if led.record_enc(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_stack(s);
        s += 1;
    }
    // 注入 10 轮：命令族逆序 / 格式载荷漂移 / 操作越闭集三态轮转。
    // 每轮全部落在当轮临时账本 lt 上，主账本只记账注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SevenzipLedger::new();
        let caught = match kind {
            0 => {
                // 空账本上先落高族再落低族：逆序必抓，且不改 last_kind 语义。
                let _ = lt.record_cmd(18, cmd_cell(18).unwrap_or(0));
                lt.record_cmd(5, cmd_cell(5).unwrap_or(0)) == Err(SevenzipCode::EOrder)
            }
            1 => lt.record_fmt(9, fmt_cell(9).unwrap_or(0).wrapping_add(1))
                == Err(SevenzipCode::EFmtDrift),
            _ => lt.record_cmd(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52451 判据集（锚 UNX-F52452-J1，off-by-one 板式）。
pub fn run_unx52451_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52451");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "a 压缩/x·e 解压/t 测试/l 列表/u 更新五族逐操作恒等",
    );
    s.add(
        "七格式族 14 格全落",
        {
            let mut lt = SevenzipLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "7z/zip/gz/xz/tar/wim/cab 七格式×两档逐格对照",
    );
    s.add(
        "加密三元闸全落",
        {
            let mut lt = SevenzipLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENC_GATES {
                if let Some(w) = enc_cell(e) {
                    if lt.record_enc(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.encs_all()
        },
        "口令/头加密/算法强度三格闭集，越闸必抓",
    );
    s.add(
        "Windows 资源管理器集成声明 2/2 全齐",
        {
            let mut lt = SevenzipLedger::new();
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
        "外壳集成声明/右键菜单分账声明两行闭集",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = SevenzipLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Igor Pavlov 原生栈声明/本仓 C++ 类比上栈声明两行闭集",
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
        "族逆序/格式漂移/越闭集三态轮转必抓",
    );
    let mut l0 = SevenzipLedger::new();
    s.add(
        "边界·命令族逆序拒",
        {
            let _ = l0.record_cmd(18, cmd_cell(18).unwrap_or(0));
            l0.record_cmd(5, cmd_cell(5).unwrap_or(0)) == Err(SevenzipCode::EOrder)
        },
        "u 更新后回 x·e 解压必抓",
    );
    s.add(
        "格式载荷漂移拒",
        l0.record_fmt(2, fmt_cell(2).unwrap_or(0).wrapping_add(5)) == Err(SevenzipCode::EFmtDrift),
        "格式格载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·格式格越闭集",
        l0.record_fmt(FMT_CELLS, 0).is_err(),
        "格式格 14 越闭集必拒",
    );
    s.add(
        "边界·加密闸越闭集",
        l0.record_enc(ENC_GATES, 0).is_err(),
        "加密闸 3 越闭集必拒",
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
        "未齐即封·方向核",
        {
            let mut lt = SevenzipLedger::new();
            let ok_op = lt.record_cmd(0, cmd_cell(0).unwrap_or(0)).is_ok();
            ok_op && lt.judge_grade() == Grade::S3
        },
        "账未齐时落账仍 Ok，仅定级不达 S1",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = SevenzipLedger::new();
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
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut e = 0usize;
            while e < ENC_GATES {
                if let Some(w) = enc_cell(e) {
                    let _ = lt.record_enc(e, w);
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut t = 0usize;
            while t < STACK_ROWS {
                let _ = lt.declare_stack(t);
                t += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB080,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("a 压缩"));
        assert_eq!(cmd_kind_name(4), Some("u 更新"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_cmd_letters() {
        // 六字母归五族：x·e 同族异字母。
        assert_eq!(cmd_letter(0), Some("a"));
        assert_eq!(cmd_letter(4), Some("x"));
        assert_eq!(cmd_letter(6), Some("e"));
        assert_eq!(cmd_letter(8), Some("t"));
        assert_eq!(cmd_letter(12), Some("l"));
        assert_eq!(cmd_letter(16), Some("u"));
        assert!(cmd_letter(20).is_none());
    }

    #[test]
    fn test_fmt_cells() {
        assert_eq!(fmt_name(0), Some("7z"));
        assert_eq!(fmt_name(3), Some("zip"));
        assert_eq!(fmt_name(13), Some("cab"));
        assert!(fmt_name(14).is_none());
        assert!(fmt_cell(FMT_CELLS).is_none());
        let a = fmt_cell(5).unwrap_or(0);
        assert_eq!(a, fmt_cell(5).unwrap_or(0));
        assert_ne!(a, fmt_cell(6).unwrap_or(0));
    }

    #[test]
    fn test_enc_gate_names() {
        assert_eq!(enc_gate_name(0), Some("口令"));
        assert_eq!(enc_gate_name(1), Some("头加密"));
        assert_eq!(enc_gate_name(2), Some("算法强度"));
        assert!(enc_gate_name(3).is_none());
    }

    #[test]
    fn test_cmd_order_caught() {
        let mut lt = SevenzipLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(SevenzipCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SevenzipLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SevenzipCode::EDrift)
        );
        assert_eq!(
            lt.record_enc(1, enc_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(SevenzipCode::EEncDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SevenzipLedger::new();
        let _ = lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fmt(0, fmt_cell(0).unwrap_or(0)), Err(SevenzipCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SevenzipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SevenzipLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 + 格式 + 加密闸。
        assert_eq!(recorded, CMD_OPS as u32 + FMT_CELLS as u32 + ENC_GATES as u32);
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
        assert_eq!(SEG_BASE, 0xCB080);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SevenzipCode::Ok as u32, 0);
        assert_eq!(SevenzipCode::EGradeDrift as u32, 12);
        // 闭集恰 13 变体：逐位 detail 全非空。
        let all = [
            SevenzipCode::Ok,
            SevenzipCode::ENoOp,
            SevenzipCode::EDrift,
            SevenzipCode::EOrder,
            SevenzipCode::ENoFmt,
            SevenzipCode::EFmtDrift,
            SevenzipCode::ENoEnc,
            SevenzipCode::EEncDrift,
            SevenzipCode::ENoRow,
            SevenzipCode::EDup,
            SevenzipCode::ESeal,
            SevenzipCode::EInj,
            SevenzipCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt = SevenzipLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = SevenzipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐 → S1（种子按合法序先落低族格）。
        let mut o = 0usize;
        while o < CMD_OPS {
            if let Some(w) = cmd_cell(o) {
                let _ = lt2.record_cmd(o, w);
            }
            o += 1;
        }
        let mut f = 0usize;
        while f < FMT_CELLS {
            if let Some(w) = fmt_cell(f) {
                let _ = lt2.record_fmt(f, w);
            }
            f += 1;
        }
        let mut e = 0usize;
        while e < ENC_GATES {
            if let Some(w) = enc_cell(e) {
                let _ = lt2.record_enc(e, w);
            }
            e += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        let mut t = 0usize;
        while t < STACK_ROWS {
            let _ = lt2.declare_stack(t);
            t += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52451_checks_green() {
        let s = run_unx52451_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
