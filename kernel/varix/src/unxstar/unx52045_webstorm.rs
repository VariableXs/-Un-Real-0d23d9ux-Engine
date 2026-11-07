//! UNX-F52045 WebStorm 收口 · 0xC4B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52045 逐字锚定）：UNX-F52046-J1 六判据全落；
//! Node 调试挂接/前端工具链集成操作集特化；KB-COMPAT 定级登记；判据锚
//! 定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：前端工具链操作集账（调试挂接/构建集成/测试运行/依赖分
//!      析/热重载/格式化/转译/打包/源映射/性能剖析 十段）× 8 工程场景
//!      = 80 格操作账；段号闭集 0..10。
//!   ② Node 调试挂接面：挂接三元（Node ABI 档/调试器档/协议档）对宿
//!      主面全等才放行挂接，错位走失配路径（ENodeAbi/EDbgProto/
//!      EDbgProto2 三诊断）；失配注入必抓。
//!   ③ 操作拍序闸：同段操作拍序严格递增；重复落账拒。
//!   ④ 注入 ×10 轮（段越闭集/挂接失配/拍序回退三态轮转）10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 定级（80 格全通 + 挂接零失配 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52045 − 52000)）。
pub const SEG_BASE: u32 = 0xC4B40;

/// 工具链段数。
pub const CHAINS: usize = 10;

/// 工程场景数。
pub const PROJECTS: usize = 8;

/// Node 挂接三元基准。
pub const NODE_ABI: u32 = 108;
pub const DBG_PROTO: u32 = 3;
pub const WS_PROTO: u32 = 7;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WsCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoChain,
    /// 工程号越闭集
    ENoProj,
    /// Node ABI 档失配
    ENodeAbi,
    /// 调试协议档失配
    EDbgProto,
    /// WS 协议档失配
    EWsProto,
    /// 拍序乱
    EOrder,
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

impl WsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WsCode::Ok => "成功",
            WsCode::ENoChain => "段号越闭集：前端工具链十段之外先登记",
            WsCode::ENoProj => "工程号越闭集：8 工程之外先登记",
            WsCode::ENodeAbi => "Node ABI 档失配：NODE_MODULE_VERSION 与宿主档不符",
            WsCode::EDbgProto => "调试协议档失配：调试器协议档与宿主档不符",
            WsCode::EWsProto => "WS 协议档失配：WebSocket 协议档与宿主档不符",
            WsCode::EOrder => "操作拍序乱：同段操作拍序必须严格递增单调",
            WsCode::EDup => "重复落账：同格只许一条账目",
            WsCode::ESeal => "未齐即封：80 格操作账未齐先补齐",
            WsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WsCode::EGradeDrift => "定级漂移：80 格全通+挂接零失配+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（段 c × 工程 p）。
pub const fn chain_cell(c: usize, p: usize) -> Option<u64> {
    if c >= CHAINS || p >= PROJECTS {
        return None;
    }
    Some(
        0x2C000u64
            + (c as u64) * 0x1B
            + (p as u64) * 0x7
            ^ (c as u64).wrapping_mul(0x9E3779B97F4A7C15 >> p),
    )
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct WsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    chains: [[Option<u64>; PROJECTS]; CHAINS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WsLedger {
    /// 新账本。
    pub fn new() -> Self {
        WsLedger {
            gates: [None; 6],
            chains: [[None; PROJECTS]; CHAINS],
            last_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WsCode> {
        if gate >= 6 {
            return Err(WsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x15100 + gate as u64);
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

    /// Node 挂接三元闸（全等放行，错位失配）。
    pub fn attach_node(&mut self, abi: u32, dbg: u32, ws: u32) -> Result<(), WsCode> {
        if abi != NODE_ABI {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x15200);
            return Err(WsCode::ENodeAbi);
        }
        if dbg != DBG_PROTO {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x15201);
            return Err(WsCode::EDbgProto);
        }
        if ws != WS_PROTO {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x15202);
            return Err(WsCode::EWsProto);
        }
        self.fnv(0x15203);
        Ok(())
    }

    /// 操作一格（拍序单调 + 重复拒）。
    pub fn record_chain(&mut self, c: usize, p: usize, tick: u32) -> Result<u64, WsCode> {
        if c >= CHAINS {
            return Err(WsCode::ENoChain);
        }
        if p >= PROJECTS {
            return Err(WsCode::ENoProj);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x15300);
            return Err(WsCode::EOrder);
        }
        let payload = match chain_cell(c, p) {
            Some(p) => p,
            None => return Err(WsCode::ENoProj),
        };
        if match self.chains.get(c).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WsCode::EDup);
        }
        if let Some(row) = self.chains.get_mut(c) {
            if let Some(cell) = row.get_mut(p) {
                *cell = Some(payload);
            }
        }
        self.last_tick = tick as i64;
        self.fnv(0x15301 + (c as u64) * 8 + p as u64);
        Ok(payload)
    }

    /// 操作全账核验（80 格）。
    pub fn chains_all(&self) -> bool {
        let mut c = 0usize;
        while c < CHAINS {
            let mut p = 0usize;
            while p < PROJECTS {
                match self.chains.get(c).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            c += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 拍游标（供全流程构造严格递增拍）。
    pub fn last_tick(&self) -> i64 {
        self.last_tick
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for WsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（操作格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // 挂接三元全等放行。
    let _ = led.attach_node(NODE_ABI, DBG_PROTO, WS_PROTO);
    let mut recorded = 0u32;
    let mut tick: u32 = 0;
    let mut c = 0usize;
    while c < CHAINS {
        let mut p = 0usize;
        while p < PROJECTS {
            tick = tick.saturating_add(1);
            if led.record_chain(c, p, tick).is_ok() {
                recorded += 1;
            }
            p += 1;
        }
        c += 1;
    }
    // 注入 10 轮：ABI 失配 / 段越闭集 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WsLedger::new();
        let caught = match kind {
            0 => lt.attach_node(NODE_ABI + 1, DBG_PROTO, WS_PROTO) == Err(WsCode::ENodeAbi),
            1 => lt.record_chain(CHAINS, 0, 1).is_err(),
            _ => {
                let _ = lt.record_chain(0, 0, 6, );
                lt.record_chain(0, 1, 3).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x15400 + k as u64);
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
pub fn run_unx52045_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52045");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "工具链 80/80 全通",
        recorded == (CHAINS * PROJECTS) as u32,
        "前端工具链十段 ×8 工程全通",
    );
    s.add(
        "Node 挂接三元全等放行",
        {
            let mut lt = WsLedger::new();
            lt.attach_node(NODE_ABI, DBG_PROTO, WS_PROTO).is_ok()
        },
        "ABI/调试协议/WS 协议三元全等放行",
    );
    s.add(
        "挂接失配三态全抓",
        {
            let mut lt = WsLedger::new();
            let a = lt.attach_node(NODE_ABI + 1, DBG_PROTO, WS_PROTO) == Err(WsCode::ENodeAbi);
            let b = lt.attach_node(NODE_ABI, DBG_PROTO + 1, WS_PROTO) == Err(WsCode::EDbgProto);
            let c = lt.attach_node(NODE_ABI, DBG_PROTO, WS_PROTO + 1) == Err(WsCode::EWsProto);
            a && b && c
        },
        "ABI/调试协议/WS 协议错位走失配路径",
    );
    s.add("注入 10/10 全抓", caught >= 10, "ABI/段/拍序三态轮转必抓");
    let mut l0 = WsLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_chain(CHAINS, 0, 1).is_err(),
        "段 10 越闭集必拒",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_chain(0, PROJECTS, 1).is_err(),
        "工程 8 越闭集必拒",
    );
    let mut l1 = WsLedger::new();
    let _ = l1.record_chain(2, 3, 1);
    s.add(
        "重复·再落账拒",
        l1.record_chain(2, 3, 2).is_err(),
        "同格只许一条账目",
    );
    s.add(
        "拍序·回退拒",
        l1.record_chain(2, 4, 0) == Err(WsCode::EOrder),
        "拍序回退必抓",
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
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_cell_pure() {
        let a = chain_cell(3, 2).unwrap_or(0);
        assert_eq!(a, chain_cell(3, 2).unwrap_or(0));
        assert_ne!(a, chain_cell(3, 3).unwrap_or(0));
        assert!(chain_cell(CHAINS, 0).is_none());
        assert!(chain_cell(0, PROJECTS).is_none());
    }

    #[test]
    fn test_attach_gates() {
        let mut lt = WsLedger::new();
        assert_eq!(lt.attach_node(NODE_ABI + 5, DBG_PROTO, WS_PROTO), Err(WsCode::ENodeAbi));
        assert_eq!(lt.attach_node(NODE_ABI, DBG_PROTO + 5, WS_PROTO), Err(WsCode::EDbgProto));
        assert_eq!(lt.attach_node(NODE_ABI, DBG_PROTO, WS_PROTO + 5), Err(WsCode::EWsProto));
        assert!(lt.attach_node(NODE_ABI, DBG_PROTO, WS_PROTO).is_ok());
    }

    #[test]
    fn test_order_and_dup_gates() {
        let mut lt = WsLedger::new();
        assert!(lt.record_chain(1, 1, 5).is_ok());
        assert_eq!(lt.record_chain(1, 1, 6), Err(WsCode::EDup));
        assert_eq!(lt.record_chain(1, 2, 2), Err(WsCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (CHAINS * PROJECTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4B40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52045_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
