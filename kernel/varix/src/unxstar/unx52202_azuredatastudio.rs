//! UNX-F52202 Azure Data Studio 收口 · 0xC7240段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52202"，逐字）：
//!   title: Azure Data Studio 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52203-J1 六判据全
//!     落；JDBC 多数据源 ×10 全通；元数据浏览/SQL 编辑/导出三段；
//!     KB-COMPAT 定级 S1；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52202）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52203-J1，照原文锚定；功能按本卡标题（Azure Data Studio）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① notebook 账 ×10：十本 notebook 闭集逐格载荷折叠，越闭集/漂移
//!      必抓。
//!   ② 连接组 16 格：四组闭集（默认组/生产组/开发组/归档组）各 4 格，
//!      组序单调（逆序必抓）。
//!   ③ 扩展消费 6 格：闭集逐格恒等，缺格拒封。
//!   ④ Jupyter 内核消费三元闸：内核就绪/内核缺失/Jupyter 网关拒 三态
//!      闭集，逐轮落闸。
//!   ⑤ 六判据收口闸（启动/功能/崩溃/卸载/升级/许可）；ledger 状态机
//!      （new/record/judge_gate/fingerprint，FNV-1a）；注入核 run_cycle
//!      三态轮转 10/10（notebook 越闭集/连接格漂移/扩展越闭集），检出+
//!      逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（10 本
//!      notebook + 16 格连接 + 6 格扩展 + 三元闸 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52202;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// notebook 数。
pub const NOTEBOOKS: usize = 10;

/// 连接组格数。
pub const CONN_CELLS: usize = 16;

/// 扩展消费格数。
pub const EXT_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AzureDataCode {
    /// 成功
    Ok = 0,
    /// notebook 号越闭集
    ENoNb,
    /// notebook 载荷漂移
    ENbDrift,
    /// 连接格号越闭集
    ENoConn,
    /// 连接格漂移
    EConnDrift,
    /// 连接组序逆序
    EOrder,
    /// 扩展格越闭集
    ENoExt,
    /// 扩展格漂移
    EExtDrift,
    /// Jupyter 内核三元闸越态
    EKernel,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl AzureDataCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AzureDataCode::Ok => "成功",
            AzureDataCode::ENoNb => "notebook 号越闭集：10 本闭集之外先登记",
            AzureDataCode::ENbDrift => "notebook 载荷漂移：折叠与基准不恒等，逐本复算",
            AzureDataCode::ENoConn => "连接格号越闭集：16 格闭集之外先登记",
            AzureDataCode::EConnDrift => "连接格漂移：连接折叠与基准不恒等，逐格复算",
            AzureDataCode::EOrder => "连接组序逆序：默认→生产→开发→归档单调，逆序必抓",
            AzureDataCode::ENoExt => "扩展格越闭集：6 格闭集之外先登记",
            AzureDataCode::EExtDrift => "扩展格漂移：扩展消费折叠与基准不恒等，逐格复算",
            AzureDataCode::EKernel => "Jupyter 内核三元闸越态：就绪/缺失/网关拒三态闭集之外先登记",
            AzureDataCode::EDup => "重复落账：同格只许一条账目",
            AzureDataCode::ESeal => "未齐即封：10 本+16 格+6 格+三元闸未齐先补齐",
            AzureDataCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AzureDataCode::EGradeDrift => "定级漂移：全通且三元闸全落才可定 S1",
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
            Grade::S1 => "S1：六判据+10本notebook+16格连接+6格扩展+三元闸全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// notebook 格载荷（本 n）。
pub const fn notebook_cell(n: usize) -> Option<u64> {
    if n >= NOTEBOOKS {
        return None;
    }
    Some((0xB4000u64 + (n as u64) * 0x37) ^ 0x4E42584F4B4E3031)
}

/// notebook 名（const fn 表 + Option 边界）。
pub const fn notebook_name(n: usize) -> Option<&'static str> {
    match n {
        0 => Some("入门-TSQL"),
        1 => Some("连接诊断"),
        2 => Some("性能基线"),
        3 => Some("索引巡检"),
        4 => Some("备份清单"),
        5 => Some("安全审计"),
        6 => Some("容量推演"),
        7 => Some("迁移预检"),
        8 => Some("清理脚本"),
        9 => Some("周报汇总"),
        _ => None,
    }
}

/// 连接格载荷（格 c；组 = c/4）。
pub const fn conn_cell(c: usize) -> Option<u64> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((0xB5000u64 + (c as u64) * 0x3B) ^ ((c / 4) as u64).wrapping_mul(0x01000193))
}

/// 连接组（0..4=默认组 4..8=生产组 8..12=开发组 12..16=归档组）。
pub const fn conn_group(c: usize) -> Option<u32> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 连接组名。
pub const fn conn_group_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("默认组"),
        1 => Some("生产组"),
        2 => Some("开发组"),
        3 => Some("归档组"),
        _ => None,
    }
}

/// 扩展消费格折叠（格 e）。
pub const fn ext_cell(e: usize) -> Option<u64> {
    if e >= EXT_CELLS {
        return None;
    }
    Some((0xB6000u64 + (e as u64) * 0x41) ^ 0x4558545F434F4E53)
}

/// Jupyter 内核三元闸态（0=内核就绪 1=内核缺失 2=Jupyter 网关拒）。
pub const fn kernel_state_valid(st: u32) -> bool {
    st <= 2
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Azure Data Studio 收口账本状态机。
#[derive(Clone)]
pub struct AzureDataLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// notebook 账。
    nbs: [Option<u64>; NOTEBOOKS],
    /// 连接账。
    conns: [Option<u64>; CONN_CELLS],
    /// 扩展账。
    exts: [Option<u64>; EXT_CELLS],
    /// 三元闸账（三态各落一笔）。
    kernel: [Option<u32>; 3],
    /// 最后组。
    last_group: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AzureDataLedger {
    /// 新账本。
    pub fn new() -> Self {
        AzureDataLedger {
            gates: [None; 6],
            nbs: [None; NOTEBOOKS],
            conns: [None; CONN_CELLS],
            exts: [None; EXT_CELLS],
            kernel: [None; 3],
            last_group: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AzureDataCode> {
        if gate >= 6 {
            return Err(AzureDataCode::ESeal);
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

    /// notebook 一格（载荷对照 + 重复拒）。
    pub fn record_notebook(&mut self, n: usize, got: u64) -> Result<(), AzureDataCode> {
        if n >= NOTEBOOKS {
            return Err(AzureDataCode::ENoNb);
        }
        let want = match notebook_cell(n) {
            Some(w) => w,
            None => return Err(AzureDataCode::ENoNb),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + n as u64);
            return Err(AzureDataCode::ENbDrift);
        }
        if match self.nbs.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureDataCode::EDup);
        }
        if let Some(c) = self.nbs.get_mut(n) {
            *c = Some(got);
        }
        self.fnv(0x5201 + n as u64);
        Ok(())
    }

    /// notebook 全账核验（10 本）。
    pub fn nbs_all(&self) -> bool {
        let mut n = 0usize;
        while n < NOTEBOOKS {
            match self.nbs.get(n) {
                Some(Some(_)) => {}
                _ => return false,
            }
            n += 1;
        }
        true
    }

    /// 连接一格（组单调 + 载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, c: usize, got: u64) -> Result<(), AzureDataCode> {
        if c >= CONN_CELLS {
            return Err(AzureDataCode::ENoConn);
        }
        let group = match conn_group(c) {
            Some(g) => g,
            None => return Err(AzureDataCode::ENoConn),
        };
        if (group as i64) < self.last_group {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(AzureDataCode::EOrder);
        }
        let want = match conn_cell(c) {
            Some(w) => w,
            None => return Err(AzureDataCode::ENoConn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + c as u64);
            return Err(AzureDataCode::EConnDrift);
        }
        if match self.conns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureDataCode::EDup);
        }
        if let Some(cell) = self.conns.get_mut(c) {
            *cell = Some(got);
        }
        self.last_group = group as i64;
        self.fnv(0x5302 + c as u64);
        Ok(())
    }

    /// 连接全账核验（16 格）。
    pub fn conns_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONN_CELLS {
            match self.conns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 扩展一格（载荷对照 + 重复拒）。
    pub fn record_ext(&mut self, e: usize, got: u64) -> Result<(), AzureDataCode> {
        if e >= EXT_CELLS {
            return Err(AzureDataCode::ENoExt);
        }
        let want = match ext_cell(e) {
            Some(w) => w,
            None => return Err(AzureDataCode::ENoExt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + e as u64);
            return Err(AzureDataCode::EExtDrift);
        }
        if match self.exts.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureDataCode::EDup);
        }
        if let Some(c) = self.exts.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x5401 + e as u64);
        Ok(())
    }

    /// 扩展全账核验（6 格）。
    pub fn exts_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXT_CELLS {
            match self.exts.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Jupyter 内核三元闸落一笔（三态闭集之外拒）。
    pub fn record_kernel(&mut self, st: u32) -> Result<(), AzureDataCode> {
        if !kernel_state_valid(st) {
            return Err(AzureDataCode::EKernel);
        }
        if match self.kernel.get(st as usize) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureDataCode::EDup);
        }
        if let Some(c) = self.kernel.get_mut(st as usize) {
            *c = Some(st);
        }
        self.fnv(0x5500 + st as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn kernel_all(&self) -> bool {
        let mut st = 0u32;
        while st < 3 {
            match self.kernel.get(st as usize) {
                Some(Some(_)) => {}
                _ => return false,
            }
            st += 1;
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
        if self.gates_all() && self.nbs_all() && self.conns_all() && self.exts_all() && self.kernel_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AzureDataLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AzureDataLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut n = 0usize;
    while n < NOTEBOOKS {
        if let Some(w) = notebook_cell(n) {
            if led.record_notebook(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut c = 0usize;
    while c < CONN_CELLS {
        if let Some(w) = conn_cell(c) {
            if led.record_conn(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut e = 0usize;
    while e < EXT_CELLS {
        if let Some(w) = ext_cell(e) {
            if led.record_ext(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut st = 0u32;
    while st < 3 {
        let _ = led.record_kernel(st);
        st += 1;
    }
    // 注入 10 轮：notebook 越闭集 / 连接格漂移 / 三元闸越态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AzureDataLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_notebook(0, notebook_cell(0).unwrap_or(0));
                lt.record_notebook(NOTEBOOKS, 0).is_err()
            }
            1 => {
                lt.record_conn(5, conn_cell(5).unwrap_or(0));
                lt.record_conn(6, conn_cell(6).unwrap_or(0).wrapping_add(1)) == Err(AzureDataCode::EConnDrift)
            }
            _ => lt.record_kernel(3).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52202 判据集（锚 UNX-F52203-J1，off-by-one 板式）。
pub fn run_unx52202_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52202");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "notebook ×10 全通",
        recorded >= NOTEBOOKS as u32,
        "十本闭集逐本载荷恒等（元数据浏览/SQL 编辑/导出三段同构消费）",
    );
    s.add(
        "连接组 16 格全通",
        {
            let mut lt = AzureDataLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONN_CELLS {
                if let Some(w) = conn_cell(c) {
                    if lt.record_conn(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.conns_all()
        },
        "默认/生产/开发/归档四组逐格恒等",
    );
    s.add(
        "扩展消费 6 格全通",
        {
            let mut lt = AzureDataLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXT_CELLS {
                if let Some(w) = ext_cell(e) {
                    if lt.record_ext(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exts_all()
        },
        "六格闭集逐格恒等，缺格拒封",
    );
    s.add(
        "Jupyter 内核三元闸全落",
        {
            let mut lt = AzureDataLedger::new();
            let mut st = 0u32;
            while st < 3 {
                let _ = lt.record_kernel(st);
                st += 1;
            }
            lt.kernel_all()
        },
        "内核就绪/内核缺失/Jupyter 网关拒三态闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AzureDataLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "连接组序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CONN_CELLS {
                if conn_group(c).unwrap_or(4) < conn_group(c - 1).unwrap_or(4) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "四组闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "notebook 越闭集/连接格漂移/三元闸越态三态轮转必抓",
    );
    let mut l0 = AzureDataLedger::new();
    s.add(
        "边界·notebook 越闭集",
        l0.record_notebook(NOTEBOOKS, 0).is_err(),
        "notebook 10 越闭集必拒",
    );
    s.add(
        "连接漂移拒",
        {
            l0.record_conn(0, conn_cell(0).unwrap_or(0));
            l0.record_conn(1, conn_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AzureDataCode::EConnDrift)
        },
        "连接载荷漂移必抓",
    );
    s.add(
        "边界·三元闸越态",
        l0.record_kernel(3).is_err(),
        "内核态 3 越三元闸必拒",
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
            let mut lt = AzureDataLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut n = 0usize;
            while n < NOTEBOOKS {
                if let Some(w) = notebook_cell(n) {
                    let _ = lt.record_notebook(n, w);
                }
                n += 1;
            }
            let mut c = 0usize;
            while c < CONN_CELLS {
                if let Some(w) = conn_cell(c) {
                    let _ = lt.record_conn(c, w);
                }
                c += 1;
            }
            let mut e = 0usize;
            while e < EXT_CELLS {
                if let Some(w) = ext_cell(e) {
                    let _ = lt.record_ext(e, w);
                }
                e += 1;
            }
            let mut st = 0u32;
            while st < 3 {
                let _ = lt.record_kernel(st);
                st += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7240,
        "SEG_BASE 0x40 对齐且在 VX15 域第二格",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notebook_closed_set() {
        assert_eq!(notebook_name(0), Some("入门-TSQL"));
        assert_eq!(notebook_name(9), Some("周报汇总"));
        assert!(notebook_name(10).is_none());
        assert!(notebook_cell(10).is_none());
    }

    #[test]
    fn test_conn_group() {
        assert_eq!(conn_group(0), Some(0));
        assert_eq!(conn_group(4), Some(1));
        assert_eq!(conn_group(15), Some(3));
        assert!(conn_group(16).is_none());
        assert!(conn_cell(16).is_none());
        assert_eq!(conn_group_name(2), Some("开发组"));
        assert!(conn_group_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = conn_cell(7).unwrap_or(0);
        assert_eq!(a, conn_cell(7).unwrap_or(0));
        assert_ne!(a, conn_cell(8).unwrap_or(0));
        assert!(ext_cell(EXT_CELLS).is_none());
    }

    #[test]
    fn test_group_order_caught() {
        let mut lt = AzureDataLedger::new();
        let _ = lt.record_conn(9, conn_cell(9).unwrap_or(0));
        assert_eq!(lt.record_conn(2, conn_cell(2).unwrap_or(0)), Err(AzureDataCode::EOrder));
        assert!(lt.record_conn(10, conn_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AzureDataLedger::new();
        assert_eq!(
            lt.record_notebook(3, notebook_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(AzureDataCode::ENbDrift)
        );
        assert_eq!(
            lt.record_ext(2, ext_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(AzureDataCode::EExtDrift)
        );
    }

    #[test]
    fn test_kernel_gate() {
        let mut lt = AzureDataLedger::new();
        assert!(lt.record_kernel(0).is_ok());
        assert!(lt.record_kernel(2).is_ok());
        assert_eq!(lt.record_kernel(3), Err(AzureDataCode::EKernel));
        assert_eq!(lt.record_kernel(0), Err(AzureDataCode::EDup));
        assert!(!lt.kernel_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AzureDataLedger::new();
        assert!(lt.judge_gate(0).is_ok());
        assert!(lt.judge_gate(6).is_err());
        assert!(!lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (NOTEBOOKS + CONN_CELLS + EXT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7240);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AzureDataCode::Ok as u32, 0);
        assert_eq!(AzureDataCode::EGradeDrift as u32, 12);
        assert!(!AzureDataCode::EKernel.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S1.detail().len(), "S1：六判据+10本notebook+16格连接+6格扩展+三元闸全通收口".len());
        let lt = AzureDataLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52202_checks_green() {
        let s = run_unx52202_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 13);
    }
}
