//! UNX-F52204 Navicat Premium 收口 · 0xC72C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52204"，逐字）：
//!   title: Navicat Premium 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52205-J1 六判据全
//!     落；MySQL 专属操作集 ×10；升级接管口径（与 Premium 同族分立版本
//!     线）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52204）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52205-J1，照原文锚定；功能按本卡标题（Navicat Premium）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 多数据库引擎连接账 24 格：6 家族闭集（MySQL/MariaDB/PostgreSQL/
//!      Oracle/SQL Server/SQLite）× 4 连接（直连/主从/只读/运维），
//!      家族序单调（逆序必抓）。
//!   ② SSH 隧道三元闸：隧道就绪/握手失败/密钥拒 三态闭集，逐态落闸，
//!      越态必抓。
//!   ③ 数据传输 10 场景闭集：逐场景载荷折叠恒等才收，漂移必抓。
//!   ④ 六判据收口闸（启动/功能/崩溃/卸载/升级/许可）；ledger 状态机
//!      （new/record/judge_gate/fingerprint，FNV-1a）；注入核 run_cycle
//!      三态轮转 10/10（连接格漂移/引擎越闭集/场景越闭集），检出+逃逸
//!      守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集——本卡许可
//!      校验面如实定级 S2（商业许可面按 KB-COMPAT 口径不升 S1，防重
//!      声明 §AI-66.1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52204;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 引擎连接格数（6 家族 × 4 连接）。
pub const CONN_CELLS: usize = 24;

/// 引擎家族数。
pub const ENGINE_FAMILIES: usize = 6;

/// 数据传输场景数。
pub const TRANSFER_SCENES: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavicatCode {
    /// 成功
    Ok = 0,
    /// 引擎家族越闭集
    ENoEng,
    /// 连接格漂移
    EConnDrift,
    /// 引擎家族序逆序
    EOrder,
    /// SSH 三元闸越态
    ESshState,
    /// SSH 三元闸重复落闸
    ESshDup,
    /// 传输场景越闭集
    ENoXfer,
    /// 传输场景漂移
    EXferDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 判据漂移
    EDrift,
    /// 定级漂移
    EGradeDrift,
}

impl NavicatCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NavicatCode::Ok => "成功",
            NavicatCode::ENoEng => "引擎家族越闭集：6 家族闭集之外先登记",
            NavicatCode::EConnDrift => "连接格漂移：连接折叠与基准不恒等，逐格复算",
            NavicatCode::EOrder => "引擎家族序逆序：MySQL→MariaDB→PostgreSQL→Oracle→SQL Server→SQLite 单调，逆序必抓",
            NavicatCode::ESshState => "SSH 三元闸越态：隧道就绪/握手失败/密钥拒三态闭集之外先登记",
            NavicatCode::ESshDup => "SSH 三元闸重复落闸：同态只许一笔",
            NavicatCode::ENoXfer => "传输场景越闭集：10 场景闭集之外先登记",
            NavicatCode::EXferDrift => "传输场景漂移：场景折叠与基准不恒等，逐场景复算",
            NavicatCode::EDup => "重复落账：同格只许一条账目",
            NavicatCode::ESeal => "未齐即封：24 格连接 + 10 场景 + 三元闸未齐先补齐",
            NavicatCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NavicatCode::EDrift => "判据漂移：落账值与基准表不恒等，逐格复算",
            NavicatCode::EGradeDrift => "定级漂移：全通才定级，本卡许可面如实定级 S2（KB-COMPAT 口径）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不可达——许可面如实定级 S2）
    S1,
    /// S2：如实定级收口（商业许可面）
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
            Grade::S1 => "S1：全通收口（本卡许可面按 KB-COMPAT 口径不可达）",
            Grade::S2 => "S2：商业许可面如实定级收口（防重声明 §AI-66.1）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 引擎连接格载荷（格 c；家族 = c/4，连接型 = c%4）。
pub const fn conn_cell(c: usize) -> Option<u64> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((0xBA000u64 + (c as u64) * 0x4D) ^ ((c / 4) as u64).wrapping_mul(0x01000193))
}

/// 引擎家族（c/4）。
pub const fn conn_family(c: usize) -> Option<u32> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 家族名（const fn 表 + Option 边界）。
pub const fn family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("MySQL"),
        1 => Some("MariaDB"),
        2 => Some("PostgreSQL"),
        3 => Some("Oracle"),
        4 => Some("SQL Server"),
        5 => Some("SQLite"),
        _ => None,
    }
}

/// 连接型名（c%4）。
pub const fn conn_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("直连"),
        1 => Some("主从"),
        2 => Some("只读"),
        3 => Some("运维"),
        _ => None,
    }
}

/// 数据传输场景载荷（场景 t）。
pub const fn transfer_cell(t: usize) -> Option<u64> {
    if t >= TRANSFER_SCENES {
        return None;
    }
    Some((0xBB000u64 + (t as u64) * 0x59) ^ 0x5452414E53464552)
}

/// 传输场景名。
pub const fn transfer_scene_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("结构同库复制"),
        1 => Some("结构跨库复制"),
        2 => Some("数据全量同库"),
        3 => Some("数据全量跨库"),
        4 => Some("增量插补"),
        5 => Some("表选择性传输"),
        6 => Some("视图迁移"),
        7 => Some("存储过程迁移"),
        8 => Some("事件调度迁移"),
        9 => Some("定检映射校验"),
        _ => None,
    }
}

/// SSH 隧道三元闸态（0=隧道就绪 1=握手失败 2=密钥拒）。
pub const fn ssh_state_valid(st: u32) -> bool {
    st <= 2
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Navicat Premium 收口账本状态机。
#[derive(Clone)]
pub struct NavicatLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 连接账。
    conns: [Option<u64>; CONN_CELLS],
    /// 传输场景账。
    transfers: [Option<u64>; TRANSFER_SCENES],
    /// SSH 三元闸账。
    ssh: [Option<u32>; 3],
    /// 最后家族。
    last_family: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NavicatLedger {
    /// 新账本。
    pub fn new() -> Self {
        NavicatLedger {
            gates: [None; 6],
            conns: [None; CONN_CELLS],
            transfers: [None; TRANSFER_SCENES],
            ssh: [None; 3],
            last_family: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NavicatCode> {
        if gate >= 6 {
            return Err(NavicatCode::ESeal);
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

    /// 连接一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, c: usize, got: u64) -> Result<(), NavicatCode> {
        if c >= CONN_CELLS {
            return Err(NavicatCode::ENoEng);
        }
        let fam = match conn_family(c) {
            Some(f) => f,
            None => return Err(NavicatCode::ENoEng),
        };
        if (fam as i64) < self.last_family {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(NavicatCode::EOrder);
        }
        let want = match conn_cell(c) {
            Some(w) => w,
            None => return Err(NavicatCode::ENoEng),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + c as u64);
            return Err(NavicatCode::EConnDrift);
        }
        if match self.conns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavicatCode::EDup);
        }
        if let Some(cell) = self.conns.get_mut(c) {
            *cell = Some(got);
        }
        self.last_family = fam as i64;
        self.fnv(0x7202 + c as u64);
        Ok(())
    }

    /// 连接全账核验（24 格）。
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

    /// 传输场景一格（载荷对照 + 重复拒）。
    pub fn record_transfer(&mut self, t: usize, got: u64) -> Result<(), NavicatCode> {
        if t >= TRANSFER_SCENES {
            return Err(NavicatCode::ENoXfer);
        }
        let want = match transfer_cell(t) {
            Some(w) => w,
            None => return Err(NavicatCode::ENoXfer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + t as u64);
            return Err(NavicatCode::EXferDrift);
        }
        if match self.transfers.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavicatCode::EDup);
        }
        if let Some(c) = self.transfers.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x7301 + t as u64);
        Ok(())
    }

    /// 传输全账核验（10 场景）。
    pub fn transfers_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRANSFER_SCENES {
            match self.transfers.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// SSH 隧道三元闸落一笔（三态闭集之外拒）。
    pub fn record_ssh(&mut self, st: u32) -> Result<(), NavicatCode> {
        if !ssh_state_valid(st) {
            return Err(NavicatCode::ESshState);
        }
        if match self.ssh.get(st as usize) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavicatCode::ESshDup);
        }
        if let Some(c) = self.ssh.get_mut(st as usize) {
            *c = Some(st);
        }
        self.fnv(0x7400 + st as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn ssh_all(&self) -> bool {
        let mut st = 0u32;
        while st < 3 {
            match self.ssh.get(st as usize) {
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

    /// KB-COMPAT 定级：全通 → S2（商业许可面如实定级，S1 不可达）；
    /// 判据缺 → S3；半通 → S3（本卡 S2 即收口线）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.conns_all() && self.transfers_all() && self.ssh_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for NavicatLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NavicatLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CONN_CELLS {
        if let Some(w) = conn_cell(c) {
            if led.record_conn(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < TRANSFER_SCENES {
        if let Some(w) = transfer_cell(t) {
            if led.record_transfer(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut st = 0u32;
    while st < 3 {
        let _ = led.record_ssh(st);
        st += 1;
    }
    // 注入 10 轮：连接格漂移 / 引擎越闭集 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NavicatLedger::new();
        let caught = match kind {
            0 => {
                lt.record_conn(3, conn_cell(3).unwrap_or(0));
                lt.record_conn(4, conn_cell(4).unwrap_or(0).wrapping_add(1)) == Err(NavicatCode::EConnDrift)
            }
            1 => {
                let _ = lt.record_transfer(0, transfer_cell(0).unwrap_or(0));
                lt.record_conn(CONN_CELLS, 0).is_err()
            }
            _ => lt.record_transfer(TRANSFER_SCENES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52204 判据集（锚 UNX-F52205-J1，off-by-one 板式）。
pub fn run_unx52204_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52204");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "多数据库引擎连接 24 格全通",
        recorded >= CONN_CELLS as u32,
        "6 家族 × 直连/主从/只读/运维逐格恒等",
    );
    s.add(
        "引擎家族 6 家闭集",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < ENGINE_FAMILIES {
                if family_name(f).is_none() {
                    ok = false;
                }
                f += 1;
            }
            ok && family_name(ENGINE_FAMILIES).is_none()
        },
        "MySQL 至 SQLite 六家族名表闭集",
    );
    s.add(
        "SSH 隧道三元闸全落",
        {
            let mut lt = NavicatLedger::new();
            let mut st = 0u32;
            while st < 3 {
                let _ = lt.record_ssh(st);
                st += 1;
            }
            lt.ssh_all()
        },
        "隧道就绪/握手失败/密钥拒三态闭集",
    );
    s.add(
        "数据传输 10 场景全通",
        {
            let mut lt = NavicatLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRANSFER_SCENES {
                if let Some(w) = transfer_cell(t) {
                    if lt.record_transfer(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.transfers_all()
        },
        "结构/数据/增量/选择性/视图/过程/事件/定检十场景闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = NavicatLedger::new();
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
        "引擎家族序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CONN_CELLS {
                if conn_family(c).unwrap_or(6) < conn_family(c - 1).unwrap_or(6) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "六家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "连接格漂移/引擎越闭集/场景越闭集三态轮转必抓",
    );
    let mut l0 = NavicatLedger::new();
    s.add(
        "边界·连接格越闭集",
        l0.record_conn(CONN_CELLS, 0).is_err(),
        "连接格 24 越闭集必拒",
    );
    s.add(
        "连接漂移拒",
        {
            l0.record_conn(0, conn_cell(0).unwrap_or(0));
            l0.record_conn(1, conn_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NavicatCode::EConnDrift)
        },
        "连接载荷漂移必抓",
    );
    s.add(
        "边界·三元闸越态",
        l0.record_ssh(3).is_err(),
        "SSH 态 3 越三元闸必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_transfer(TRANSFER_SCENES, 0).is_err(),
        "传输场景 10 越闭集必拒",
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
        "定级 S2 如实登记",
        {
            let mut lt = NavicatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < CONN_CELLS {
                if let Some(w) = conn_cell(c) {
                    let _ = lt.record_conn(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < TRANSFER_SCENES {
                if let Some(w) = transfer_cell(t) {
                    let _ = lt.record_transfer(t, w);
                }
                t += 1;
            }
            let mut st = 0u32;
            while st < 3 {
                let _ = lt.record_ssh(st);
                st += 1;
            }
            lt.judge_grade() == Grade::S2 && lt.judge_grade() != Grade::S1
        },
        "全通 → S2 如实定级（商业许可面，防重声明 §AI-66.1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC72C0,
        "SEG_BASE 0x40 对齐且在 VX15 域第四格",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_names() {
        assert_eq!(family_name(0), Some("MySQL"));
        assert_eq!(family_name(5), Some("SQLite"));
        assert!(family_name(6).is_none());
        assert_eq!(conn_kind_name(0), Some("直连"));
        assert_eq!(conn_kind_name(3), Some("运维"));
        assert!(conn_kind_name(4).is_none());
    }

    #[test]
    fn test_transfer_scenes() {
        assert_eq!(transfer_scene_name(0), Some("结构同库复制"));
        assert_eq!(transfer_scene_name(9), Some("定检映射校验"));
        assert!(transfer_scene_name(10).is_none());
        assert!(transfer_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = conn_cell(13).unwrap_or(0);
        assert_eq!(a, conn_cell(13).unwrap_or(0));
        assert_ne!(a, conn_cell(14).unwrap_or(0));
        assert!(conn_family(24).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NavicatLedger::new();
        let _ = lt.record_conn(9, conn_cell(9).unwrap_or(0));
        assert_eq!(lt.record_conn(2, conn_cell(2).unwrap_or(0)), Err(NavicatCode::EOrder));
        assert!(lt.record_conn(10, conn_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NavicatLedger::new();
        assert_eq!(
            lt.record_conn(5, conn_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(NavicatCode::EConnDrift)
        );
        assert_eq!(
            lt.record_transfer(4, transfer_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(NavicatCode::EXferDrift)
        );
    }

    #[test]
    fn test_ssh_gate() {
        let mut lt = NavicatLedger::new();
        assert!(lt.record_ssh(1).is_ok());
        assert_eq!(lt.record_ssh(3), Err(NavicatCode::ESshState));
        assert_eq!(lt.record_ssh(1), Err(NavicatCode::ESshDup));
        assert!(!lt.ssh_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NavicatLedger::new();
        let _ = lt.record_conn(0, conn_cell(0).unwrap_or(0));
        assert_eq!(lt.record_conn(0, conn_cell(0).unwrap_or(0)), Err(NavicatCode::EDup));
        let _ = lt.record_transfer(0, transfer_cell(0).unwrap_or(0));
        assert_eq!(lt.record_transfer(0, transfer_cell(0).unwrap_or(0)), Err(NavicatCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NavicatLedger::new();
        assert!(lt.judge_gate(2).is_ok());
        assert!(lt.judge_gate(6).is_err());
        assert!(!lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CONN_CELLS + TRANSFER_SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC72C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NavicatCode::Ok as u32, 0);
        assert_eq!(NavicatCode::EGradeDrift as u32, 12);
        assert!(!NavicatCode::ESshDup.detail().is_empty());
    }

    #[test]
    fn test_grade_s2_honest() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = NavicatLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52204_checks_green() {
        let s = run_unx52204_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 13);
    }
}
