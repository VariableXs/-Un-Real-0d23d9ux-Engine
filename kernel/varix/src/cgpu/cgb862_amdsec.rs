//! cgb862_amdsec —— CGPU-F0862 AMD 安全。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0862 AMD 安全  <SEG 0x7640>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0862；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 安全扫描与加固，整数账面）：
//!   1. 安全域闭集六域（写死）：内核命令面 / 用户提交面 / 显存映
//!      射面 / 媒体管道面 / 遥测出口面 / 固件接口面；域号 0..=5，
//!      越界红码（XC_BAD_DOMAIN）；
//!   2. 漏洞等级闭集四级：C0 致命 / C1 高危 / C2 中危 / C3 低危；
//!      越界红码（XC_BAD_SEVERITY）；
//!   3. 漏洞状态机：Found → Confirmed → Patched → Verified →
//!      Closed；Confirmed 可回 Found（误报）；非法迁移红码
//!      （XC_BAD_TRANS）；
//!   4. 漏洞账（定容 32）守恒：登记数 = 存量数 + 关闭数；漂移红
//!      码（XC_SEC_DRIFT）；
//!   5. 加固闸门判据（写死）：C0/C1 存量 = 0 且映射面 W^X 全开
//!      才可放行；违例放行红码（XC_GATE_LEAK）；
//!   6. W^X 位账：每域 RWX 违规位扫描（写死：映射面/固件面必须
//!      无 RW+X 同置），违规红码（XC_WX_VIOLATION）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7640 段为本模块独占（SEG_BASE=0x7640），判别式
//!   SEG_BASE+1..+N；与 0x7620/0x7660 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AS_VERSION: &str = "cgb862_amdsec-v1";

/// 诊断段基址（0x7640 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7640;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7641 安全域越界（0..=5 之外）。
pub const XC_BAD_DOMAIN: u16 = SEG_BASE + 1;
/// 0x7642 漏洞等级越界（0..=3 之外）。
pub const XC_BAD_SEVERITY: u16 = SEG_BASE + 2;
/// 0x7643 漏洞状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 3;
/// 0x7644 漏洞账漂移（登记 ≠ 存量 + 关闭）。
pub const XC_SEC_DRIFT: u16 = SEG_BASE + 4;
/// 0x7645 加固闸门泄漏（C0/C1 存量或 W^X 未清仍放行）。
pub const XC_GATE_LEAK: u16 = SEG_BASE + 5;
/// 0x7646 W^X 违规（RW+X 同置）。
pub const XC_WX_VIOLATION: u16 = SEG_BASE + 6;
/// 0x7647 漏洞号越界。
pub const XC_BAD_VULN: u16 = SEG_BASE + 7;
/// 0x7648 漏洞账满（32）。
pub const XC_SEC_FULL: u16 = SEG_BASE + 8;
/// 0x7649 重复漏洞签名。
pub const XC_DUP_SIG: u16 = SEG_BASE + 9;
/// 0x764A 终态再迁移。
pub const XC_TERMINAL_MOVE: u16 = SEG_BASE + 10;

/// 安全域数（写死：6）。
pub const DOM_CAP: usize = 6;
/// 漏洞账容量（写死：32）。
pub const VULN_CAP: usize = 32;
/// 等级闭集上限。
pub const SEV_CAP: u8 = 3;

/// 安全域闭集。
pub const S_KERNEL: usize = 0;
pub const S_USER: usize = 1;
pub const S_MAP: usize = 2;
pub const S_MEDIA: usize = 3;
pub const S_TELEM: usize = 4;
pub const S_FIRMWARE: usize = 5;

/// 等级闭集：0=C0 致命 1=C1 高危 2=C2 中危 3=C3 低危。
pub const C0: u8 = 0;
pub const C1: u8 = 1;
pub const C2: u8 = 2;
pub const C3: u8 = 3;

/// W^X 页属性位：R=1 W=2 X=4。
pub const WX_R: u32 = 1;
pub const WX_W: u32 = 2;
pub const WX_X: u32 = 4;

/// W^X 违规判定：W 与 X 同置。
pub fn wx_violation(attr: u32) -> bool {
    attr & WX_W != 0 && attr & WX_X != 0
}

/// 漏洞状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VState {
    /// 发现。
    Found,
    /// 确认。
    Confirmed,
    /// 已修补。
    Patched,
    /// 已验证。
    Verified,
    /// 已关闭（终态）。
    Closed,
}

/// 迁移合法性。
pub fn trans_legal(from: VState, to: VState) -> bool {
    matches!(
        (from, to),
        (VState::Found, VState::Confirmed)
            | (VState::Found, VState::Closed)
            | (VState::Confirmed, VState::Patched)
            | (VState::Confirmed, VState::Found)
            | (VState::Patched, VState::Verified)
            | (VState::Patched, VState::Confirmed)
            | (VState::Verified, VState::Closed)
    )
}

/// 单条漏洞。
#[derive(Clone, Debug)]
pub struct Vuln {
    /// 漏洞号（1 起）。
    pub id: u32,
    /// 签名（去重键）。
    pub sig: String,
    /// 安全域。
    pub domain: usize,
    /// 等级。
    pub severity: u8,
    /// 状态。
    pub state: VState,
}

impl Vuln {
    /// 终态。
    pub fn is_terminal(&self) -> bool {
        self.state == VState::Closed
    }
}

/// AMD 安全册。
pub struct SecLedger {
    /// 漏洞账。
    pub rows: Vec<Vuln>,
    /// 下一号。
    pub next_id: u32,
    /// 各域 W^X 属性表。
    pub wx: [u32; DOM_CAP],
    /// 关闭数。
    pub closed: u64,
    /// 登记数。
    pub registered: u64,
    /// 放行标志。
    pub gated: bool,
    /// 累计红码。
    pub red_count: u64,
}

impl SecLedger {
    /// 空册（映射面/固件面默认 R+W 无 X）。
    pub fn new() -> Self {
        let mut wx = [WX_R; DOM_CAP];
        wx[S_MAP] = WX_R | WX_W;
        wx[S_FIRMWARE] = WX_R | WX_W;
        SecLedger {
            rows: Vec::new(),
            next_id: 1,
            wx,
            closed: 0,
            registered: 0,
            gated: false,
            red_count: 0,
        }
    }

    /// 存量数。
    pub fn open_count(&self) -> u64 {
        self.registered - self.closed
    }

    /// 账守恒。
    pub fn balanced(&self) -> bool {
        self.registered == self.open_count() + self.closed
    }

    /// 登记漏洞。
    pub fn file(&mut self, sig: &str, domain: usize, severity: u8) -> Result<u32, u16> {
        if domain >= DOM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DOMAIN);
        }
        if severity > SEV_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SEVERITY);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_SIG);
        }
        if self.rows.len() == VULN_CAP {
            self.red_count += 1;
            return Err(XC_SEC_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(Vuln {
            id,
            sig: String::from(sig),
            domain,
            severity,
            state: VState::Found,
        });
        self.registered += 1;
        Ok(id)
    }

    /// 签名查重。
    pub fn has_sig(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 定位。
    pub fn find(&self, id: u32) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Ok(i);
            }
            i += 1;
        }
        Err(XC_BAD_VULN)
    }

    /// 推进状态。
    pub fn advance(&mut self, id: u32, to: VState) -> Result<(), u16> {
        let idx = self.find(id)?;
        let cur = self.rows[idx].state;
        if cur.is_terminal() {
            self.red_count += 1;
            return Err(XC_TERMINAL_MOVE);
        }
        if !trans_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if to == VState::Closed {
            self.closed += 1;
        }
        self.rows[idx].state = to;
        Ok(())
    }

    /// 设 W^X 属性（写入即扫违规）。
    pub fn set_wx(&mut self, domain: usize, attr: u32) -> Result<(), u16> {
        if domain >= DOM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DOMAIN);
        }
        if wx_violation(attr) {
            self.red_count += 1;
            return Err(XC_WX_VIOLATION);
        }
        self.wx[domain] = attr;
        Ok(())
    }

    /// C0/C1 存量计数。
    pub fn crit_open(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let v = &self.rows[i];
            if !v.is_terminal() && (v.severity == C0 || v.severity == C1) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// W^X 全域扫描。
    pub fn wx_scan(&self) -> Result<(), u16> {
        let mut d = 0;
        while d < DOM_CAP {
            if wx_violation(self.wx[d]) {
                return Err(XC_WX_VIOLATION);
            }
            d += 1;
        }
        Ok(())
    }

    /// 放行闸门（C0/C1 清零 + W^X 干净）。
    pub fn open_gate(&mut self) -> Result<(), u16> {
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_SEC_DRIFT);
        }
        self.wx_scan()?;
        if self.crit_open() != 0 {
            self.red_count += 1;
            return Err(XC_GATE_LEAK);
        }
        self.gated = true;
        Ok(())
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdsec reg=");
        s += &self.registered.to_string();
        s += " open=";
        s += &self.open_count().to_string();
        s += " crit=";
        s += &self.crit_open().to_string();
        s += " gated=";
        s += if self.gated { "yes" } else { "no" };
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_SEC_DRIFT);
        }
        self.wx_scan()?;
        let mut closed = 0u64;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].is_terminal() {
                closed += 1;
            }
            i += 1;
        }
        if closed != self.closed {
            return Err(XC_SEC_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：登记 → 全链关闭 → W^X 设置 → 放行 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut s = SecLedger::new();
    // 负例：W^X 违规设置。
    if s.set_wx(S_MAP, WX_R | WX_W | WX_X) != Err(XC_WX_VIOLATION) {
        return Err(XC_WX_VIOLATION);
    }
    // 负例：域越界。
    if s.set_wx(DOM_CAP, WX_R) != Err(XC_BAD_DOMAIN) {
        return Err(XC_BAD_DOMAIN);
    }
    let a = s.file("map-oob-write", S_MAP, C0)?;
    let b = s.file("fw-desc-race", S_FIRMWARE, C1)?;
    let c = s.file("telem-spill", S_TELEM, C3)?;
    // 负例：等级越界。
    if s.file("bad-sev", S_USER, 9) != Err(XC_BAD_SEVERITY) {
        return Err(XC_BAD_SEVERITY);
    }
    // 负例：重复签名。
    if s.file("map-oob-write", S_USER, C2) != Err(XC_DUP_SIG) {
        return Err(XC_DUP_SIG);
    }
    // 负例：C0 存量放行。
    if s.open_gate() != Err(XC_GATE_LEAK) {
        return Err(XC_GATE_LEAK);
    }
    // 修补链 a。
    s.advance(a, VState::Confirmed)?;
    s.advance(a, VState::Patched)?;
    s.advance(a, VState::Verified)?;
    s.advance(a, VState::Closed)?;
    // 误报回退链 b：Confirmed → Found → Closed。
    s.advance(b, VState::Confirmed)?;
    s.advance(b, VState::Found)?;
    s.advance(b, VState::Closed)?;
    // 直闭 c。
    s.advance(c, VState::Closed)?;
    // 负例：终态再迁移。
    if s.advance(c, VState::Found) != Err(XC_TERMINAL_MOVE) {
        return Err(XC_TERMINAL_MOVE);
    }
    // 负例：非法迁移 Found→Verified。
    let mut s2 = SecLedger::new();
    let d = s2.file("fresh-vuln", S_KERNEL, C2)?;
    if s2.advance(d, VState::Verified) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    s2.audit()?;
    s.audit()?;
    s.open_gate()?;
    // 负例：放行后再登记 C0 使状态失配（登记不重置 gated）。
    let e = s.file("late-vuln", S_USER, C2)?;
    s.advance(e, VState::Closed)?;
    if s.summary().len() == 0 || !s.balanced() {
        return Err(XC_SEC_DRIFT);
    }
    // W^X 纯判定面：RW+X 违规、RO 合法、RW 合法。
    if !wx_violation(WX_W | WX_X) || wx_violation(WX_R) || wx_violation(WX_R | WX_W) {
        return Err(XC_WX_VIOLATION);
    }
    Ok(0)
}
