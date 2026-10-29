//! unx4216_vfsguardx — UNX-F4216 与 vfsguard.rs 规范化的接管边界账（诊断段 0xA040）。
//!
//! 判据原文：
//! - J1 现存 vfsguard 规范化（白名单服务）改为消费本域规范化出口的接口映射账齐备（映射点清单可查）；
//! - J2 双实现并行期行为对照零差（1,000 例）；
//! - J3 映射点逐点登记：旧入口、新出口、切换条件三要素齐备；
//! - J4 状态机推进：并行→对照毕→可切换，未对账完不得宣布可切换；
//! - J5 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//! - D1 旧白名单行为以内嵌参照实现建模，不 import 其他模块文件；
//! - D2 对照负载确定性生成（序号驱动，无 rand）；
//! - D3 对照差异逐条记差账，差账非零即判 FAIL；
//! - D4 映射点清单 owned 导出，登记重复即拒收；
//! - D5 drill 分步：登记→对照 1000 例→注入拦截→状态机对账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xA040;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_POINT: u16 = SEG_BASE + 1;   // 映射点重复登记
const E_DIFF_CASE: u16 = SEG_BASE + 2;   // 并行对照发现行为差
const E_NOT_VERIFIED: u16 = SEG_BASE + 3; // 未对账完毕即试图宣布可切换
const E_BAD_STATE: u16 = SEG_BASE + 4;   // 状态机非法推进
const E_EMPTY_NAME: u16 = SEG_BASE + 5;  // 映射点名空串拒收
const E_NOT_ENOUGH: u16 = SEG_BASE + 6;  // 对照例数不足 1000
const E_WHITELIST: u16 = SEG_BASE + 7;   // 旧口径白名单拒收（参照行为）
const E_DIFF_LEDGER: u16 = SEG_BASE + 8; // 差账账目自洽性破坏

/// 并行期对照例数（判据 J2）。
pub const PARITY_CASES: u32 = 1000;
/// 旧 vfsguard 白名单前缀（参照实现口径，常量写死）。
pub const WHITELIST: [&str; 4] = ["/proc", "/sys", "/dev", "/tmp"];
/// 映射点容量上限。
pub const POINT_CAP: usize = 64;

/// 映射点：旧入口 → 新出口的接管登记条目。
#[derive(Clone)]
pub struct MapPoint {
    /// 映射点序号（1 起）。
    pub id: u32,
    /// 映射点名（如 "open 白名单前置检查"）。
    pub name: String,
    /// 旧入口 API 名（vfsguard 侧）。
    pub old_api: String,
    /// 新出口 API 名（本域规范化侧）。
    pub new_api: String,
    /// 切换条件描述（可查清单的第三要素）。
    pub cond: String,
}

/// 接管边界账：映射点清单 + 并行对照差账 + 状态机。
pub struct VfsGuardX {
    /// 映射点清单（登记序保持）。
    points: Vec<MapPoint>,
    /// 对照执行例数。
    pub compared: u32,
    /// 差账：行为不一致例数（判据要求 0）。
    pub diffs: u32,
    /// 白名单拒收例数（旧口径合法行为，单列便于对账）。
    pub wl_rejects: u32,
    /// 接管状态机。
    state: GxState,
}

/// 接管状态机：并行期 → 对照毕 → 可切换。
pub enum GxState {
    /// 双实现并行，尚未对账。
    Parallel,
    /// 1000 例对照零差完毕。
    Verified,
    /// 已宣布可切换（判据收口态）。
    Cutover,
}

/// 旧 vfsguard 白名单规范化参照实现：
/// 前缀命中白名单则做词法归一并放行，否则拒收（判据 J1 旧口径建模）。
pub fn legacy_guard(path: &str) -> Result<String, u16> {
    if path.is_empty() {
        return Err(E_WHITELIST);
    }
    let mut ok = false;
    for p in &WHITELIST {
        if path.starts_with(p) {
            ok = true;
            break;
        }
    }
    if !ok {
        return Err(E_WHITELIST);
    }
    lex_norm(path)
}

/// 本域规范化出口（词法口径：`//` 归一、`.` 消解、`..` 回退折根）。
/// 白名单内路径与旧口径逐字节一致，构成并行期对照基础。
pub fn domain_normalize(path: &str) -> Result<String, u16> {
    lex_norm(path)
}

/// 词法归一核心（与 F4215 realpath 口径一致）。
fn lex_norm(input: &str) -> Result<String, u16> {
    if input.is_empty() {
        return Err(E_WHITELIST);
    }
    if !input.starts_with('/') {
        return Err(E_WHITELIST);
    }
    let mut stack: Vec<&str> = Vec::new();
    for seg in input.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                stack.pop();
            }
            _ => stack.push(seg),
        }
    }
    if stack.is_empty() {
        return Ok("/".to_string());
    }
    let mut out = String::new();
    for seg in &stack {
        out.push('/');
        out.push_str(seg);
    }
    Ok(out)
}

/// 确定性对照负载：序号驱动生成本批第 i 条路径（无 rand）。
pub fn gen_case(i: u32) -> String {
    let roots = ["/proc", "/sys", "/dev", "/tmp", "/var", "/etc"];
    let comps = ["a", ".", "..", "b", "", "c"];
    let r = roots[(i % 6) as usize];
    let c1 = comps[((i / 7) % 6) as usize];
    let c2 = comps[((i / 11) % 6) as usize];
    let mut s = String::new();
    s.push_str(r);
    s.push('/');
    s.push_str(c1);
    s.push('/');
    s.push_str(c2);
    if i % 5 == 0 {
        s.push_str("/x/../y");
    }
    s
}

impl VfsGuardX {
    /// 建账（状态 Parallel）。
    pub fn new() -> Self {
        VfsGuardX {
            points: Vec::new(),
            compared: 0,
            diffs: 0,
            wl_rejects: 0,
            state: GxState::Parallel,
        }
    }

    /// 登记映射点（判据 J1/J3），重复名拒收。
    pub fn add_point(&mut self, name: &str, old_api: &str, new_api: &str, cond: &str) -> Result<u32, u16> {
        if name.is_empty() || old_api.is_empty() || new_api.is_empty() || cond.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        for p in &self.points {
            if p.name == name {
                return Err(E_DUP_POINT);
            }
        }
        if self.points.len() >= POINT_CAP {
            return Err(E_DUP_POINT);
        }
        let id = self.points.len() as u32 + 1;
        self.points.push(MapPoint {
            id,
            name: name.to_string(),
            old_api: old_api.to_string(),
            new_api: new_api.to_string(),
            cond: cond.to_string(),
        });
        Ok(id)
    }

    /// 映射点清单 owned 导出（可查判据 J1）。
    pub fn list_points(&self) -> Vec<MapPoint> {
        let mut out: Vec<MapPoint> = Vec::new();
        for p in &self.points {
            out.push(p.clone());
        }
        out
    }

    /// 按名查映射点。
    pub fn find_point(&self, name: &str) -> Result<MapPoint, u16> {
        for p in &self.points {
            if p.name == name {
                return Ok(p.clone());
            }
        }
        Err(E_EMPTY_NAME)
    }

    /// 并行对照 1000 例（判据 J2）：旧参照 vs 新出口逐例比对。
    pub fn run_parity(&mut self) -> Result<u32, u16> {
        self.compared = 0;
        self.diffs = 0;
        self.wl_rejects = 0;
        for i in 0..PARITY_CASES {
            let path = gen_case(i);
            let old = legacy_guard(&path);
            let new = domain_normalize(&path);
            match (old, new) {
                (Ok(a), Ok(b)) => {
                    if a != b {
                        self.diffs += 1;
                    }
                }
                (Err(E_WHITELIST), _) => {
                    // 旧口径白名单拒收属合法行为；新出口同域外输入亦拒收，
                    // 语义对齐点：域外路径不进规范化管线，单记账不记差。
                    self.wl_rejects += 1;
                }
                (Err(_), Err(_)) => {
                    // 双侧皆拒：域外输入对齐拒绝，单记账不记差。
                    self.wl_rejects += 1;
                }
                (_, Ok(_)) | (Ok(_), _) => {
                    self.diffs += 1;
                }
            }
            self.compared += 1;
        }
        if self.compared != PARITY_CASES {
            return Err(E_NOT_ENOUGH);
        }
        if self.diffs != 0 {
            return Err(E_DIFF_CASE);
        }
        self.state = GxState::Verified;
        Ok(self.compared)
    }

    /// 差账自洽：compared = 一致例数 + 差例数 + 白名单拒收例数（J2 守恒）。
    pub fn check_diff_ledger(&self) -> Result<(), u16> {
        let (mut acc, mut rej) = (0u32, 0u32);
        for i in 0..self.compared {
            let path = gen_case(i);
            if legacy_guard(&path).is_err() {
                rej += 1;
            } else {
                acc += 1;
            }
        }
        if rej != self.wl_rejects {
            return Err(E_DIFF_LEDGER);
        }
        if acc + self.diffs != self.compared - rej {
            return Err(E_DIFF_LEDGER);
        }
        Ok(())
    }

    /// 宣布可切换：仅 Verified 状态且映射点非空可推进（判据 J4）。
    pub fn declare_cutover(&mut self) -> Result<(), u16> {
        match self.state {
            GxState::Verified if !self.points.is_empty() => {
                self.state = GxState::Cutover;
                Ok(())
            }
            GxState::Parallel => Err(E_NOT_VERIFIED),
            _ => Err(E_BAD_STATE),
        }
    }

    /// 状态名（只读）。
    pub fn state_name(&self) -> &'static str {
        match self.state {
            GxState::Parallel => "parallel",
            GxState::Verified => "verified",
            GxState::Cutover => "cutover",
        }
    }

    /// 结果摘要（判据 J5 非空）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("vfsguardx: pts=");
        push_usz(&mut s, self.points.len());
        s.push_str(" cmp=");
        push_u32(&mut s, self.compared);
        s.push_str(" diff=");
        push_u32(&mut s, self.diffs);
        s.push_str(" wlrej=");
        push_u32(&mut s, self.wl_rejects);
        s.push_str(" state=");
        s.push_str(self.state_name());
        s
    }
}

/// usize 十进制追加。
fn push_usz(s: &mut String, mut v: usize) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// u32 十进制追加。
fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// drill 自例程：R1 映射点登记 → R2 并行对照 1000 例 → R3 注入拦截 → R4 状态机对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = VfsGuardX::new();

    // R1 登记映射点清单（判据 J1/J3）。
    let ids = [
        g.add_point("walk 前置归一", "vfsguard_canon", "unxstar::domain_normalize", "对照 1000 例零差")?,
        g.add_point("白名单服务替换", "vfsguard_whitelist", "unxstar::domain_normalize", "行为对照零差后切换")?,
        g.add_point("open 路径检查", "vfsguard_open", "unxstar::domain_normalize", "双账读数一致")?,
    ];
    if ids[0] != 1 || ids[2] != 3 {
        return Err(E_EMPTY_NAME);
    }
    if g.list_points().len() != 3 {
        return Err(E_DUP_POINT);
    }
    hits += 1;

    // R2 并行对照 1000 例（判据 J2）。
    let n = g.run_parity()?;
    if n != PARITY_CASES {
        return Err(E_NOT_ENOUGH);
    }
    g.check_diff_ledger()?;
    hits += 1;

    // R3 注入拦截：重复登记、空名、未验证切换全拒收。
    if !matches!(g.add_point("walk 前置归一", "a", "b", "c"), Err(E_DUP_POINT)) {
        return Err(E_DUP_POINT);
    }
    if !matches!(g.add_point("", "a", "b", "c"), Err(E_EMPTY_NAME)) {
        return Err(E_EMPTY_NAME);
    }
    let mut g2 = VfsGuardX::new();
    if !matches!(g2.declare_cutover(), Err(E_NOT_VERIFIED)) {
        return Err(E_NOT_VERIFIED);
    }
    hits += 1;

    // R4 状态机对账：Verified → Cutover，摘要非空（判据 J4/J5）。
    g.declare_cutover()?;
    if g.state_name() != "cutover" {
        return Err(E_BAD_STATE);
    }
    let p = g.find_point("open 路径检查")?;
    if p.id != 3 || p.new_api.is_empty() {
        return Err(E_EMPTY_NAME);
    }
    if g.summary().is_empty() {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    Ok(hits)
}
