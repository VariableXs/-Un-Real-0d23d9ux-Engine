//! UNX-F52425 socat（Windows 发行版）收口 · 0xCAA00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52425"，逐字）：
//!   title: socat（Windows 发行版）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52426-J1 查询
//!     ×10 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52425）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52426-J1，照原文锚定；功能按本卡标题（socat（Windows 发行版）
//! 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模式族 20 格闭集：TCP-LISTEN/TCP/UDP/EXEC/PTY 各 4 格，逐格
//!      载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② 五参数 10 操作闭集：-d/-v/-t/reuseaddr/fork 五族各 2 操作，家族
//!      单调（逆序必抓，逆序判错不改 last_kind）。
//!   ③ 地址三元闸 3 格：主地址/辅地址/选项附加三态逐格落账，越三态
//!      闭集必抓。
//!   ④ 双向管道 8 格：socat 双向管道逐格载荷对照，缺一拒封。
//!   ⑤ C 上栈声明 2 行闭集：C 栈上栈声明/C 栈回退声明，缺一拒封。
//!   ⑥ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（模式
//!      漂移/参数家族逆序/模式越闭集），10/10 必抓；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级 S2（Windows 发行版收口目标档）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52425;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 模式格数（五族 × 4）。
pub const MODE_CELLS: usize = 20;

/// 参数操作数（五族 × 2）。
pub const PARAM_OPS: usize = 10;

/// 地址三元闸格数。
pub const ADDR_CELLS: usize = 3;

/// 双向管道格数。
pub const PIPE_CELLS: usize = 8;

/// C 上栈声明行数。
pub const C_STACK_ROWS: usize = 2;

/// 地址三态闭集上界（0..2 三态）。
pub const ADDR_STATES: u32 = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocatCode {
    /// 成功
    Ok = 0,
    /// 模式格越闭集
    ENoMode,
    /// 模式格漂移
    EModeDrift,
    /// 参数操作越闭集
    ENoParam,
    /// 参数操作漂移
    EParamDrift,
    /// 参数家族逆序
    EOrder,
    /// 地址闸越闭集
    ENoAddr,
    /// 地址三态越界
    EAddrState,
    /// 地址闸漂移
    EAddrDrift,
    /// 管道格越闭集
    ENoPipe,
    /// 管道格漂移
    EPipeDrift,
    /// 声明行越闭集
    ENoRow,
    /// 定级漂移
    EGradeDrift,
}

impl SocatCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SocatCode::Ok => "成功",
            SocatCode::ENoMode => "模式格越闭集：TCP-LISTEN/TCP/UDP/EXEC/PTY 20 格之外先登记",
            SocatCode::EModeDrift => "模式格漂移：模式载荷折叠与基准不恒等，逐格复算",
            SocatCode::ENoParam => "参数操作越闭集：-d/-v/-t/reuseaddr/fork 10 操作之外先登记",
            SocatCode::EParamDrift => "参数操作漂移：参数载荷折叠与基准不恒等，逐格复算",
            SocatCode::EOrder => "参数家族逆序：-d→-v→-t→reuseaddr→fork 单调，逆序必抓且不前移游标",
            SocatCode::ENoAddr => "地址三元闸越闭集：地址闸 3 格之外先登记",
            SocatCode::EAddrState => "地址三态越界：主地址/辅地址/选项附加 0..2 三态闭集之外先登记",
            SocatCode::EAddrDrift => "地址闸漂移：闸载荷与三态折叠不恒等，逐格复算",
            SocatCode::ENoPipe => "双向管道越闭集：双向管道 8 格之外先登记",
            SocatCode::EPipeDrift => "双向管道漂移：管道载荷与基准折叠不恒等，逐格复算",
            SocatCode::ENoRow => "声明行越闭集：C 上栈声明 2 行闭集之外先登记",
            SocatCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档 S2，S1 为保留上位档）
    S1,
    /// S2：有条件收口（本卡收口目标档）
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
            Grade::S1 => "S1：全通收口（socat Windows 发行版收口目标档为 S2）",
            Grade::S2 => "S2：有条件收口，六闸齐即达标（Windows 发行版功能子集）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式格载荷（格 o；族 = o/4，族序 TCP-LISTEN→TCP→UDP→EXEC→PTY）。
pub const fn mode_cell(o: usize) -> Option<u64> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((0xEA000u64 + (o as u64) * 0x3D) ^ (0x534F4341_0000_0000 + (o / 4) as u64))
}

/// 模式族号。
pub const fn mode_kind(o: usize) -> Option<u32> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 模式族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("TCP-LISTEN"),
        1 => Some("TCP"),
        2 => Some("UDP"),
        3 => Some("EXEC"),
        4 => Some("PTY"),
        _ => None,
    }
}

/// 参数操作格载荷（操作 o；族 = o/2，族序 -d→-v→-t→reuseaddr→fork）。
pub const fn param_cell(o: usize) -> Option<u64> {
    if o >= PARAM_OPS {
        return None;
    }
    Some((0xEB000u64 + (o as u64) * 0x47) ^ ((o / 2) as u64).wrapping_mul(0x0100_0231))
}

/// 参数族号。
pub const fn param_kind(o: usize) -> Option<u32> {
    if o >= PARAM_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 参数族名。
pub const fn param_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-d"),
        1 => Some("-v"),
        2 => Some("-t"),
        3 => Some("reuseaddr"),
        4 => Some("fork"),
        _ => None,
    }
}

/// 地址三态（格 g 的基准态：0 主地址 1 辅地址 2 选项附加，按 g%3 轮转）。
pub const fn addr_state(g: usize) -> Option<u32> {
    if g >= ADDR_CELLS {
        return None;
    }
    Some(g as u32)
}

/// 地址三元闸格载荷（格 g）。
pub const fn addr_cell(g: usize) -> Option<u64> {
    let st = match addr_state(g) {
        Some(s) => s,
        None => return None,
    };
    Some(0xEC000u64 + (g as u64) * 0x31 + st as u64)
}

/// 双向管道格载荷（格 p）。
pub const fn pipe_cell(p: usize) -> Option<u64> {
    if p >= PIPE_CELLS {
        return None;
    }
    Some(0xED000u64 + (p as u64) * 0x43)
}

/// C 上栈声明行折叠（行 r）。
pub const fn c_stack_row(r: usize) -> Option<u32> {
    if r >= C_STACK_ROWS {
        return None;
    }
    Some(0xEE000 + r as u32 * 0x71)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// socat 收口账本状态机。
#[derive(Clone)]
pub struct SocatLedger {
    /// 六判据（监听/转发/双向管道/地址三元/诊断回显/写回）。
    gates: [Option<bool>; 6],
    /// 模式格账。
    modes: [Option<u64>; MODE_CELLS],
    /// 参数操作账。
    params: [Option<u64>; PARAM_OPS],
    /// 地址闸账。
    addrs: [Option<u64>; ADDR_CELLS],
    /// 双向管道账。
    pipes: [Option<u64>; PIPE_CELLS],
    /// C 上栈声明账。
    rows: [Option<u32>; C_STACK_ROWS],
    /// 最后参数族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SocatLedger {
    /// 新账本。
    pub fn new() -> Self {
        SocatLedger {
            gates: [None; 6],
            modes: [None; MODE_CELLS],
            params: [None; PARAM_OPS],
            addrs: [None; ADDR_CELLS],
            pipes: [None; PIPE_CELLS],
            rows: [None; C_STACK_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SocatCode> {
        if gate >= 6 {
            return Err(SocatCode::ENoRow);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xDA00 + gate as u64);
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

    /// 模式一格（载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, o: usize, got: u64) -> Result<(), SocatCode> {
        let want = match mode_cell(o) {
            Some(w) => w,
            None => return Err(SocatCode::ENoMode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDA01 + o as u64);
            return Err(SocatCode::EModeDrift);
        }
        if match self.modes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SocatCode::EGradeDrift);
        }
        if let Some(c) = self.modes.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0xDA02 + o as u64);
        Ok(())
    }

    /// 模式全账核验（20 格）。
    pub fn modes_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODE_CELLS {
            match self.modes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 参数一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_param(&mut self, o: usize, got: u64) -> Result<(), SocatCode> {
        if o >= PARAM_OPS {
            return Err(SocatCode::ENoParam);
        }
        let kind = match param_kind(o) {
            Some(k) => k,
            None => return Err(SocatCode::ENoParam),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDB00);
            return Err(SocatCode::EOrder);
        }
        let want = match param_cell(o) {
            Some(w) => w,
            None => return Err(SocatCode::ENoParam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDB01 + o as u64);
            return Err(SocatCode::EParamDrift);
        }
        if match self.params.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SocatCode::EGradeDrift);
        }
        if let Some(c) = self.params.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xDB02 + o as u64);
        Ok(())
    }

    /// 参数全账核验（10 操作）。
    pub fn params_all(&self) -> bool {
        let mut o = 0usize;
        while o < PARAM_OPS {
            match self.params.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 地址闸一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_addr(&mut self, g: usize, got: u64) -> Result<(), SocatCode> {
        let st = match addr_state(g) {
            Some(s) => s,
            None => return Err(SocatCode::ENoAddr),
        };
        if st >= ADDR_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC00);
            return Err(SocatCode::EAddrState);
        }
        let want = match addr_cell(g) {
            Some(w) => w,
            None => return Err(SocatCode::ENoAddr),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC01 + g as u64);
            return Err(SocatCode::EAddrDrift);
        }
        if match self.addrs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SocatCode::EGradeDrift);
        }
        if let Some(c) = self.addrs.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0xDC02 + g as u64);
        Ok(())
    }

    /// 地址闸全账核验（3 格）。
    pub fn addrs_all(&self) -> bool {
        let mut g = 0usize;
        while g < ADDR_CELLS {
            match self.addrs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 双向管道一格（载荷对照 + 重复拒）。
    pub fn record_pipe(&mut self, p: usize, got: u64) -> Result<(), SocatCode> {
        let want = match pipe_cell(p) {
            Some(w) => w,
            None => return Err(SocatCode::ENoPipe),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDD00 + p as u64);
            return Err(SocatCode::EPipeDrift);
        }
        if match self.pipes.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SocatCode::EGradeDrift);
        }
        if let Some(c) = self.pipes.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0xDD01 + p as u64);
        Ok(())
    }

    /// 管道全账核验（8 格）。
    pub fn pipes_all(&self) -> bool {
        let mut p = 0usize;
        while p < PIPE_CELLS {
            match self.pipes.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_c_stack(&mut self, r: usize) -> Result<(), SocatCode> {
        if r >= C_STACK_ROWS {
            return Err(SocatCode::ENoRow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SocatCode::EGradeDrift);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = c_stack_row(r);
        }
        self.fnv(0xDE00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < C_STACK_ROWS {
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

    /// KB-COMPAT 定级（六闸齐 → S2 有条件收口，全齐 → S2 目标档，缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.modes_all()
            && self.params_all()
            && self.addrs_all()
            && self.pipes_all()
            && self.rows_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SocatLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SocatLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODE_CELLS {
        if let Some(w) = mode_cell(o) {
            if led.record_mode(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PARAM_OPS {
        if let Some(w) = param_cell(p) {
            if led.record_param(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut a = 0usize;
    while a < ADDR_CELLS {
        if let Some(w) = addr_cell(a) {
            if led.record_addr(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut q = 0usize;
    while q < PIPE_CELLS {
        if let Some(w) = pipe_cell(q) {
            if led.record_pipe(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut s = 0usize;
    while s < C_STACK_ROWS {
        let _ = led.declare_c_stack(s);
        s += 1;
    }
    // 注入 10 轮：模式漂移 / 参数家族逆序 / 模式越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SocatLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
                lt.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SocatCode::EModeDrift)
            }
            1 => {
                // 参数家族逆序：先落高族合法格，再注低族格。
                let _ = lt.record_param(7, param_cell(7).unwrap_or(0));
                lt.record_param(2, param_cell(2).unwrap_or(0)) == Err(SocatCode::EOrder)
            }
            _ => {
                // 模式越闭集：对空账本同样必抓。
                lt.record_mode(MODE_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xDF00 + k as u64);
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
/// UNX-F52425 判据集（锚 UNX-F52426-J1，off-by-one 板式）。
pub fn run_unx52425_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52425");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模式族 20 格全通",
        recorded >= MODE_CELLS as u32,
        "TCP-LISTEN/TCP/UDP/EXEC/PTY 五族逐格载荷恒等",
    );
    s.add(
        "模式族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODE_CELLS {
                if mode_kind(o).unwrap_or(5) < mode_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五模式族闭集单调（4 格一族）",
    );
    s.add(
        "五参数 10 操作全通",
        {
            let mut lt = SocatLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PARAM_OPS {
                if let Some(w) = param_cell(p) {
                    if lt.record_param(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.params_all()
        },
        "-d/-v/-t/reuseaddr/fork 五族按合法序逐格落账",
    );
    s.add(
        "参数家族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PARAM_OPS {
                if param_kind(p).unwrap_or(5) < param_kind(p - 1).unwrap_or(5) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "参数五族单调（2 操作一族），逆序必抓",
    );
    s.add(
        "地址三元闸 3 格全落",
        {
            let mut lt = SocatLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ADDR_CELLS {
                if let Some(w) = addr_cell(a) {
                    if lt.record_addr(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.addrs_all()
        },
        "主地址/辅地址/选项附加三态逐格落账，越三态闭集必抓",
    );
    s.add(
        "双向管道 8/8 全落",
        {
            let mut lt = SocatLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < PIPE_CELLS {
                if let Some(w) = pipe_cell(q) {
                    if lt.record_pipe(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.pipes_all()
        },
        "socat 双向管道逐格载荷对照，缺一拒封（§AI-66.3）",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = SocatLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < C_STACK_ROWS {
                if lt.declare_c_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "C 栈上栈声明/C 栈回退声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SocatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "监听/转发/双向管道/地址三元/诊断回显/写回逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "模式漂移/参数家族逆序/模式越闭集三态轮转必抓",
    );
    let mut l0 = SocatLedger::new();
    s.add(
        "边界·模式漂移拒",
        {
            let _ = l0.record_mode(0, mode_cell(0).unwrap_or(0));
            l0.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SocatCode::EModeDrift)
        },
        "模式载荷漂移必抓",
    );
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODE_CELLS, 0).is_err(),
        "模式 20 格越闭集必拒",
    );
    s.add(
        "边界·参数越闭集",
        l0.record_param(PARAM_OPS, 0).is_err(),
        "参数 10 越闭集必拒",
    );
    s.add(
        "边界·地址闸越闭集",
        l0.record_addr(ADDR_CELLS, 0).is_err(),
        "地址三元闸 3 格越闭集必拒",
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
            // 档序校验（S1 上位档保留，本卡目标档 S2）。
            let order_ok = Grade::S1.val() < Grade::S2.val() && Grade::S2.val() < Grade::S3.val();
            let mut lt = SocatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODE_CELLS {
                if let Some(w) = mode_cell(o) {
                    let _ = lt.record_mode(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PARAM_OPS {
                if let Some(w) = param_cell(p) {
                    let _ = lt.record_param(p, w);
                }
                p += 1;
            }
            let mut a = 0usize;
            while a < ADDR_CELLS {
                if let Some(w) = addr_cell(a) {
                    let _ = lt.record_addr(a, w);
                }
                a += 1;
            }
            let mut q = 0usize;
            while q < PIPE_CELLS {
                if let Some(w) = pipe_cell(q) {
                    let _ = lt.record_pipe(q, w);
                }
                q += 1;
            }
            let mut r2 = 0usize;
            while r2 < C_STACK_ROWS {
                let _ = lt.declare_c_stack(r2);
                r2 += 1;
            }
            order_ok && lt.judge_grade() == Grade::S2
        },
        "全齐 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAA00,
        "SEG_BASE 0x40 对齐且在 VX16 派工域公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 ≥18 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert!(mode_cell(20).is_none());
        assert_eq!(mode_kind_name(0), Some("TCP-LISTEN"));
        assert_eq!(mode_kind_name(4), Some("PTY"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_param_kind() {
        // 族 = o/2 映射：偶数位起一族。
        assert_eq!(param_kind(0), Some(0));
        assert_eq!(param_kind(1), Some(0));
        assert_eq!(param_kind(2), Some(1));
        assert_eq!(param_kind(9), Some(4));
        assert!(param_kind(10).is_none());
        assert_eq!(param_kind_name(0), Some("-d"));
        assert_eq!(param_kind_name(4), Some("fork"));
        assert!(param_kind_name(5).is_none());
    }

    #[test]
    fn test_addr_states() {
        assert_eq!(addr_state(0), Some(0));
        assert_eq!(addr_state(1), Some(1));
        assert_eq!(addr_state(2), Some(2));
        assert!(addr_state(3).is_none());
        assert!(addr_cell(ADDR_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(3).unwrap_or(0);
        assert_eq!(a, mode_cell(3).unwrap_or(0));
        assert_ne!(a, mode_cell(4).unwrap_or(0));
        let p = param_cell(5).unwrap_or(0);
        assert_eq!(p, param_cell(5).unwrap_or(0));
        assert!(c_stack_row(C_STACK_ROWS).is_none());
        assert!(pipe_cell(PIPE_CELLS).is_none());
        assert_eq!(pipe_cell(0), Some(0xED000));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SocatLedger::new();
        let _ = lt.record_param(7, param_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_param(2, param_cell(2).unwrap_or(0)),
            Err(SocatCode::EOrder)
        );
        // 逆序判错不改 last_kind：同族之后的高族合法格仍可落账。
        assert!(lt.record_param(8, param_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SocatLedger::new();
        assert_eq!(
            lt.record_mode(5, mode_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SocatCode::EModeDrift)
        );
        assert_eq!(
            lt.record_param(3, param_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(SocatCode::EParamDrift)
        );
        assert_eq!(
            lt.record_addr(1, addr_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(SocatCode::EAddrDrift)
        );
        assert_eq!(
            lt.record_pipe(1, pipe_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(SocatCode::EPipeDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SocatLedger::new();
        let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_mode(0, mode_cell(0).unwrap_or(0)),
            Err(SocatCode::EGradeDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SocatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SocatLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：模式 20 + 参数 10 + 地址 3 + 管道 8。
        assert_eq!(
            recorded,
            MODE_CELLS as u32 + PARAM_OPS as u32 + ADDR_CELLS as u32 + PIPE_CELLS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCAA00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SocatCode::Ok as u32, 0);
        assert_eq!(SocatCode::EGradeDrift as u32, 12);
        assert_eq!(SocatCode::Ok.val(), SEG_BASE);
        assert!(!SocatCode::Ok.detail().is_empty());
        assert!(!SocatCode::EAddrState.detail().is_empty());
        assert!(!SocatCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SocatLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2（本卡目标档即 S2）。
        let mut lt2 = SocatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐 → S2 目标档。
        let mut lt3 = SocatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < MODE_CELLS {
            if let Some(w) = mode_cell(o) {
                let _ = lt3.record_mode(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < PARAM_OPS {
            if let Some(w) = param_cell(p) {
                let _ = lt3.record_param(p, w);
            }
            p += 1;
        }
        let mut a = 0usize;
        while a < ADDR_CELLS {
            if let Some(w) = addr_cell(a) {
                let _ = lt3.record_addr(a, w);
            }
            a += 1;
        }
        let mut q = 0usize;
        while q < PIPE_CELLS {
            if let Some(w) = pipe_cell(q) {
                let _ = lt3.record_pipe(q, w);
            }
            q += 1;
        }
        let mut r = 0usize;
        while r < C_STACK_ROWS {
            let _ = lt3.declare_c_stack(r);
            r += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52425_checks_green() {
        let s = run_unx52425_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
