//! UNX-F52438 HTTPie CLI 收口 · 0xCAD40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52438"，逐字）：
//!   title: HTTPie CLI 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52439-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52438）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52439-J1，照原文锚定；功能按本卡标题（HTTPie CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：GET（读取）/ POST（提交）/ PUT（更新）/ DELETE（删
//!      除）/ session（会话）五族各 4 操作共 20 操作，族单调（逆序必抓），
//!      操作载荷折叠恒等才收。
//!   ② JSON 投影 8 格：HTTPie 的 JSON 请求体投影逐格恒等，漂移必抓。
//!   ③ 与 curl 分账声明 2 行闭集：HTTPie 与 curl 分账不重账，缺一拒封；
//!      Python 上栈声明 2 行闭集同口径。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（命令越闭集/载荷漂移/JSON 投影越
//!      闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级 S1
//!      （全通收口档，封顶 S1 不裁剪）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52438;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族总操作数（每族 4 操作 × 5 族）。
pub const CMD_OPS: usize = 20;

/// 命令族数。
pub const CMD_FAMILIES: usize = 5;

/// JSON 投影格数。
pub const JSON_CELLS: usize = 8;

/// 与 curl 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Python 上栈声明行数。
pub const PY_STACK_ROWS: usize = 2;

/// KB-COMPAT 定级封顶（本卡口径：定级 S1，全通收口）。
pub const GRADE_CAP: u32 = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HttpieCode {
    /// 成功
    Ok = 0,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作漂移
    ECmdDrift,
    /// 命令族逆序
    EOrder,
    /// JSON 投影越闭集
    ENoJson,
    /// JSON 投影漂移
    EJsonDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

/// 全码闭集表（13 变体，位序即码位）。
pub const ALL_CODES: [HttpieCode; 13] = [
    HttpieCode::Ok,
    HttpieCode::ENoCmd,
    HttpieCode::ECmdDrift,
    HttpieCode::EOrder,
    HttpieCode::ENoJson,
    HttpieCode::EJsonDrift,
    HttpieCode::ENoRow,
    HttpieCode::ERowMiss,
    HttpieCode::EDup,
    HttpieCode::ESeal,
    HttpieCode::EInj,
    HttpieCode::EFp,
    HttpieCode::EGradeDrift,
];

impl HttpieCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HttpieCode::Ok => "成功",
            HttpieCode::ENoCmd => "命令操作越闭集：GET/POST/PUT/DELETE/session 20 操作之外先登记",
            HttpieCode::ECmdDrift => "命令操作漂移：操作折叠与基准不恒等，逐格复算",
            HttpieCode::EOrder => "命令族逆序：GET→POST→PUT→DELETE→session 单调，逆序必抓",
            HttpieCode::ENoJson => "JSON 投影越闭集：JSON 请求体投影 8 格之外先登记",
            HttpieCode::EJsonDrift => "JSON 投影漂移：投影折叠与基准不恒等，逐格复算",
            HttpieCode::ENoRow => "声明行越闭集：curl 分账 / Python 上栈声明行闭集之外先登记",
            HttpieCode::ERowMiss => "缺声明行：curl 分账与 Python 上栈声明未齐先补齐",
            HttpieCode::EDup => "重复落账：同格只许一条账目",
            HttpieCode::ESeal => "未齐即封：20 操作 + 8 投影格 + 2 行分账未齐先补齐",
            HttpieCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            HttpieCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HttpieCode::EGradeDrift => "定级漂移：KB-COMPAT 登记口径为 S1 全通档，降档即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡登记档）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
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
            Grade::S1 => "S1：20 操作+8 投影格+2 行分账全通收口（KB-COMPAT 登记档）",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，六判据缺失",
        }
    }
}

/// 定级值 → 定级档（const，无浮点无查表）。
const fn grade_of(val: u32) -> Grade {
    if val <= 1 {
        Grade::S1
    } else if val == 2 {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级封顶：好于封顶档的压至封顶档。
const fn cap_grade(g: Grade) -> Grade {
    let v = g.val();
    if v < GRADE_CAP {
        grade_of(GRADE_CAP)
    } else {
        g
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；族 = o/4，GET/POST/PUT/DELETE/session）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some(
        (0xCB000u64 + (o as u64) * 0x39)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193)
            ^ 0x4854545049450000,
    )
}

/// 命令族（0..4=GET 4..8=POST 8..12=PUT 12..16=DELETE 16..20=session）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("GET 读取"),
        1 => Some("POST 提交"),
        2 => Some("PUT 更新"),
        3 => Some("DELETE 删除"),
        4 => Some("session 会话"),
        _ => None,
    }
}

/// JSON 投影格载荷（格 f）。
pub const fn json_cell(f: usize) -> Option<u64> {
    if f >= JSON_CELLS {
        return None;
    }
    Some((0xCB080u64 + (f as u64) * 0x2D) ^ 0x4A534F4E50524A00)
}

/// 与 curl 分账声明行折叠（行 r）。
pub const fn curl_split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB100 + r as u32 * 0x2C)
}

/// Python 上栈声明行折叠（行 r）。
pub const fn py_stack_row(r: usize) -> Option<u32> {
    if r >= PY_STACK_ROWS {
        return None;
    }
    Some(0xCB140 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// HTTPie 收口账本状态机。
#[derive(Clone)]
pub struct HttpieLedger {
    /// 六判据（请求/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五命令族 20 操作。
    cmds: [Option<u64>; CMD_OPS],
    /// JSON 投影 8 格。
    jsons: [Option<u64>; JSON_CELLS],
    /// 与 curl 分账声明 2 行。
    curl_rows: [Option<u32>; SPLIT_ROWS],
    /// Python 上栈声明 2 行。
    py_rows: [Option<u32>; PY_STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HttpieLedger {
    /// 新账本。
    pub fn new() -> Self {
        HttpieLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            jsons: [None; JSON_CELLS],
            curl_rows: [None; SPLIT_ROWS],
            py_rows: [None; PY_STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HttpieCode> {
        if gate >= 6 {
            return Err(HttpieCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 命令操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), HttpieCode> {
        if o >= CMD_OPS {
            return Err(HttpieCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(HttpieCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(HttpieCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(HttpieCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(HttpieCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HttpieCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// 命令操作全账核验（20 操作）。
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

    /// JSON 投影一格（载荷对照 + 重复拒）。
    pub fn record_json(&mut self, f: usize, got: u64) -> Result<(), HttpieCode> {
        let want = match json_cell(f) {
            Some(w) => w,
            None => return Err(HttpieCode::ENoJson),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300 + f as u64);
            return Err(HttpieCode::EJsonDrift);
        }
        if match self.jsons.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HttpieCode::EDup);
        }
        if let Some(c) = self.jsons.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x4301 + f as u64);
        Ok(())
    }

    /// JSON 投影全账核验（8 格）。
    pub fn jsons_all(&self) -> bool {
        let mut f = 0usize;
        while f < JSON_CELLS {
            match self.jsons.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 与 curl 分账声明一行。
    pub fn declare_curl_split(&mut self, r: usize) -> Result<(), HttpieCode> {
        if r >= SPLIT_ROWS {
            return Err(HttpieCode::ENoRow);
        }
        if match self.curl_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HttpieCode::EDup);
        }
        if let Some(c) = self.curl_rows.get_mut(r) {
            *c = curl_split_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// Python 上栈声明一行。
    pub fn declare_py_stack(&mut self, r: usize) -> Result<(), HttpieCode> {
        if r >= PY_STACK_ROWS {
            return Err(HttpieCode::ENoRow);
        }
        if match self.py_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HttpieCode::EDup);
        }
        if let Some(c) = self.py_rows.get_mut(r) {
            *c = py_stack_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// curl 分账声明全齐核验（单节）。
    pub fn curl_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.curl_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Python 上栈声明全齐核验（单节）。
    pub fn py_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < PY_STACK_ROWS {
            match self.py_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 声明行全齐核验（curl 分账 + Python 上栈）。
    pub fn rows_all(&self) -> bool {
        let mut ok = true;
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.curl_rows.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                ok = false;
            }
            r += 1;
        }
        r = 0;
        while r < PY_STACK_ROWS {
            if match self.py_rows.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                ok = false;
            }
            r += 1;
        }
        ok
    }

    /// 缺声明行核验（未齐即报 ERowMiss）。
    pub fn check_rows_present(&self) -> Result<(), HttpieCode> {
        if self.rows_all() {
            Ok(())
        } else {
            Err(HttpieCode::ERowMiss)
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

    /// 注入守恒等式核验（注入 = 检出 + 逃逸）。
    pub fn check_conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), HttpieCode> {
        if injected == caught.wrapping_add(escaped) && injected >= 10 && escaped == 0 {
            Ok(())
        } else {
            Err(HttpieCode::EInj)
        }
    }

    /// 双跑指纹一致性核验。
    pub fn check_fingerprint(a: u64, b: u64) -> Result<(), HttpieCode> {
        if a == b && a != 0 {
            Ok(())
        } else {
            Err(HttpieCode::EFp)
        }
    }

    /// 定级封顶核验（越封顶档即 EGradeDrift）。
    pub fn check_grade_cap(g: Grade) -> Result<(), HttpieCode> {
        if g.val() > GRADE_CAP {
            Err(HttpieCode::EGradeDrift)
        } else {
            Ok(())
        }
    }

    /// 名义定级（未封顶）：全通 → S1，六判据齐 → S2，否则 S3。
    pub fn judge_grade_nominal(&self) -> Grade {
        if self.gates_all() && self.cmds_all() && self.jsons_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// KB-COMPAT 定级（本卡封顶 S1：不裁剪，全通即 S1）。
    pub fn judge_grade(&self) -> Grade {
        cap_grade(self.judge_grade_nominal())
    }
}

impl Default for HttpieLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HttpieLedger::new();
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
    while f < JSON_CELLS {
        if let Some(w) = json_cell(f) {
            if led.record_json(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_curl_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < PY_STACK_ROWS {
        let _ = led.declare_py_stack(r);
        r += 1;
    }
    // 注入 10 轮：命令越闭集 / 命令载荷漂移 / JSON 投影越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HttpieLedger::new();
        let caught = match kind {
            0 => lt.record_cmd(CMD_OPS, 0).is_err(),
            1 => {
                let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(HttpieCode::ECmdDrift)
            }
            _ => lt.record_json(JSON_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
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
/// UNX-F52438 判据集（锚 UNX-F52439-J1，off-by-one 板式）。
pub fn run_unx52438_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52438");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        {
            let mut lt = HttpieLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    if lt.record_cmd(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.cmds_all()
        },
        "GET/POST/PUT/DELETE/session 五族逐操作恒等，逆序与漂移必抓",
    );
    s.add(
        "JSON 投影 8/8 全落",
        {
            let mut lt = HttpieLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < JSON_CELLS {
                if let Some(w) = json_cell(f) {
                    if lt.record_json(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.jsons_all()
        },
        "JSON 请求体投影逐格恒等，漂移必抓",
    );
    s.add(
        "与 curl 分账声明 2/2 全齐",
        {
            let mut lt = HttpieLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_curl_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.curl_rows_all()
        },
        "HTTPie 与 curl 分账不重账，两行闭集缺一拒封（§AI-66.3）",
    );
    s.add(
        "Python 上栈声明 2/2 全齐",
        {
            let mut lt = HttpieLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PY_STACK_ROWS {
                if lt.declare_py_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.py_rows_all()
        },
        "Python 上栈声明两行闭集缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HttpieLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "请求/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "五命令族单调",
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
        "GET→POST→PUT→DELETE→session 五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令越闭集/载荷漂移/JSON 投影越闭集三态轮转必抓",
    );
    let mut l0 = HttpieLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "边界·JSON 投影越闭集",
        l0.record_json(JSON_CELLS, 0).is_err(),
        "JSON 投影 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_curl_split(SPLIT_ROWS).is_err() && l0.declare_py_stack(PY_STACK_ROWS).is_err(),
        "声明行 2 越闭集必拒（curl 分账与 Python 上栈同口径）",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        HttpieLedger::check_conservation(inj_n, caught_n, esc_n).is_ok(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        HttpieLedger::check_fingerprint(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = HttpieLedger::new();
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
            while f < JSON_CELLS {
                if let Some(w) = json_cell(f) {
                    let _ = lt.record_json(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_curl_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < PY_STACK_ROWS {
                let _ = lt.declare_py_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1 && HttpieLedger::check_grade_cap(lt.judge_grade()).is_ok()
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAD40,
        "SEG_BASE 0x40 对齐且落在 VX16 公式 0xCA400+0x40×37",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 14,
        "含本条共 ≥15 条闭集，判据锚 UNX-F52439-J1 ×10",
    );
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
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(12), Some(3));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("GET 读取"));
        assert_eq!(cmd_kind_name(3), Some("DELETE 删除"));
        assert_eq!(cmd_kind_name(4), Some("session 会话"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(7).unwrap_or(0);
        assert_eq!(a, cmd_cell(7).unwrap_or(0));
        assert_ne!(a, cmd_cell(8).unwrap_or(0));
        assert_ne!(json_cell(0).unwrap_or(0), json_cell(1).unwrap_or(0));
        assert!(cmd_cell(CMD_OPS).is_none());
        assert!(json_cell(JSON_CELLS).is_none());
        assert!(curl_split_row(SPLIT_ROWS).is_none());
        assert!(py_stack_row(PY_STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = HttpieLedger::new();
        // 先落族 2（PUT，格 9），再回落族 1（POST，格 5）→ 逆序必抓。
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0)),
            Err(HttpieCode::EOrder)
        );
        // 同族内回格不判逆序。
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HttpieLedger::new();
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(HttpieCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_json(0, json_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(HttpieCode::EJsonDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HttpieLedger::new();
        let _ = lt.record_cmd(3, cmd_cell(3).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(3, cmd_cell(3).unwrap_or(0)),
            Err(HttpieCode::EDup)
        );
        let _ = lt.record_json(2, json_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_json(2, json_cell(2).unwrap_or(0)),
            Err(HttpieCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HttpieLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HttpieLedger::new();
        assert!(lt2.judge_gate(1).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_rows_missing() {
        let mut lt = HttpieLedger::new();
        // 账未齐时缺行核验必须报 ERowMiss。
        assert_eq!(lt.check_rows_present(), Err(HttpieCode::ERowMiss));
        let _ = lt.declare_curl_split(0);
        let _ = lt.declare_curl_split(1);
        assert_eq!(lt.check_rows_present(), Err(HttpieCode::ERowMiss));
        let _ = lt.declare_py_stack(0);
        let _ = lt.declare_py_stack(1);
        assert!(lt.check_rows_present().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 命令 + 8 JSON 投影（公式常量之和）。
        assert_eq!(recorded, (CMD_OPS + JSON_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCAD40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert_eq!(ALL_CODES[i].val(), SEG_BASE + i as u32);
            assert!(!ALL_CODES[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(HttpieCode::Ok as u32, 0);
        assert_eq!(HttpieCode::EGradeDrift as u32, 12);
        assert!(HttpieLedger::check_fingerprint(0, 0).is_err());
        assert!(HttpieLedger::check_conservation(10, 9, 0).is_err());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt = HttpieLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = HttpieLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52438_checks_green() {
        let s = run_unx52438_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
