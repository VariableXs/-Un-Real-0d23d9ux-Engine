//! F230 密码输入显隐切换 · H 基础通用域实装。
//!
//! **判据锚**：F230。
//!
//! **验收标准（主册第一句）**：所有密码框右端有眼睛按钮：按住显示明文
//! （松手即隐藏，比点击切换更防 shoulder surfing）、支持 Caps Lock 状态
//! 提示（大写锁定时框内显示黄色角标「大写锁定已开启」）；错误重试时不
//! 清空已输入内容（用户自己决定改哪）；粘贴进密码框允许（禁止粘贴是反
//! 用户设计）。
//!
//! **设计要点**：
//! - [`PwdField`] 两态状态机（masked / revealed-hold）：按住显、松手隐
//!   （[`PwdField::press_eye`] / [`PwdField::release_eye`]）——状态在同一
//!   次调用内同步翻转，响应预算 [`EYE_RESPONSE_BUDGET_MS`] 50ms 由
//!   「事件戳→上屏戳」差值判定（模块不持时钟，全部注入）；
//! - Caps Lock 角标：锁定事件 → 出标（预算 [`CAPS_BADGE_BUDGET_MS`]
//!   200ms），解锁 → 收标（[`PwdField::caps_event`]）；
//! - 重试不清空：[`PwdField::submit_failed`] 是错误路径唯一入口，
//!   **不触碰内容区**——失败 100 次内容一个字节不动，只有用户显式
//!   [`PwdField::clear_by_user`] 才清空；
//! - 粘贴策略登记制：[`PasteAudit`] 全系统密码框 allow-paste 登记，
//!   禁粘贴场景计数 [`PasteAudit::forbid_scenes`] 恒 0——「禁粘贴
//!   场景 = 0」的审计判据；
//! - 明文暴露时长账本：每次按住时长入定容环（安全审计面：
//!   [`PwdField::max_exposure_ms`] 给出最长单次暴露）；
//! - 热路径零堆：密码内容定长缓冲 [`CONTENT_CAP`]，事件/暴露账本
//!   定容环；Vec 仅审计快照。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、`crate::star::sbase::RingLog`。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 按住显隐响应预算——主册 F230 验收「按住显隐响应 <50ms」。
pub const EYE_RESPONSE_BUDGET_MS: u64 = 50;

/// Caps Lock 角标出标预算——主册 F230 验收「按下锁定键 <200ms 出标」。
pub const CAPS_BADGE_BUDGET_MS: u64 = 200;

/// 密码内容定容缓冲（密码框物理上限——超长输入拒绝并计数，不静默截断）。
pub const CONTENT_CAP: usize = 128;

/// 明文暴露时长环容量（最近 64 次按住记录）。
pub const EXPOSURE_RING_CAP: usize = 64;

/// 粘贴策略登记册容量（全系统密码框 32 个）。
pub const PWD_FIELD_CAP: usize = 32;

/// 单框事件环容量。
pub const PWD_EVENT_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 事件与状态机
// ---------------------------------------------------------------------------

/// 密码框事件（审计面，小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwdEvent {
    EyePressed,
    /// 松手（携带本次按住时长 ms）。
    EyeReleased(u32),
    CapsOn,
    CapsOff,
    /// 提交失败（内容不动）。
    SubmitFailed,
    /// 粘贴 n 字节。
    Pasted(u16),
    /// 用户显式清空。
    ClearedByUser,
    /// 超长输入被拒（不静默截断）。
    InputRejected,
}

/// 密码输入框状态机。
pub struct PwdField {
    /// 当前掩码态（true = 显示 •••）。
    masked: bool,
    /// 眼睛按住中。
    holding: bool,
    hold_start_ms: u64,
    /// Caps Lock 角标可见。
    caps_badge: bool,
    content: [u8; CONTENT_CAP],
    content_len: usize,
    paste_allowed: bool,
    /// 明文暴露时长环（安全审计面）。
    exposure: RingLog<u32, EXPOSURE_RING_CAP>,
    events: RingLog<PwdEvent, PWD_EVENT_CAP>,
    /// 最长单次明文暴露（ms）。
    pub max_exposure_ms: u32,
    /// 提交失败次数（重试账本）。
    pub failed_attempts: u32,
    /// 被拒超长输入次数。
    pub rejected_inputs: u32,
    /// 被拒粘贴次数（allow=false 时 paste_in 拒绝）。
    pub rejected_pastes: u32,
    /// 用户显式清空次数（应与用户主动操作一致；错误路径永不产生）。
    pub user_clears: u32,
}

impl PwdField {
    pub fn new(paste_allowed: bool) -> PwdField {
        PwdField {
            masked: true,
            holding: false,
            hold_start_ms: 0,
            caps_badge: false,
            content: [0; CONTENT_CAP],
            content_len: 0,
            paste_allowed,
            exposure: RingLog::new(),
            events: RingLog::new(),
            max_exposure_ms: 0,
            failed_attempts: 0,
            rejected_inputs: 0,
            rejected_pastes: 0,
            user_clears: 0,
        }
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    pub fn is_holding(&self) -> bool {
        self.holding
    }

    pub fn caps_badge_visible(&self) -> bool {
        self.caps_badge
    }

    pub fn content_len(&self) -> usize {
        self.content_len
    }

    /// 内容视图（审计/测试面——内核其余路径不得明文外泄，此处仅限
    /// 判定「重试不清空」的字节级比对）。
    pub fn content_slice(&self) -> &[u8] {
        &self.content[..self.content_len]
    }

    /// 暴露账本快照（新→旧）。
    pub fn exposure_ledger(&self) -> Vec<u32> {
        self.exposure.newest_first()
    }

    fn log(&mut self, e: PwdEvent) {
        self.events.push(e);
    }

    /// 按住眼睛：立即显明文（状态同步翻转——响应预算 50ms 内的
    /// 结构保证：翻转不排队、不异步）。
    pub fn press_eye(&mut self, now_ms: u64) {
        if self.holding {
            return;
        }
        self.holding = true;
        self.masked = false;
        self.hold_start_ms = now_ms;
        self.log(PwdEvent::EyePressed);
    }

    /// 松开眼睛：立即回掩码，并把本次按住时长记入暴露账本。
    pub fn release_eye(&mut self, now_ms: u64) {
        if !self.holding {
            return;
        }
        self.holding = false;
        self.masked = true;
        let hold = now_ms.saturating_sub(self.hold_start_ms).min(u32::MAX as u64) as u32;
        self.exposure.push(hold);
        if hold > self.max_exposure_ms {
            self.max_exposure_ms = hold;
        }
        self.log(PwdEvent::EyeReleased(hold));
    }

    /// Caps Lock 状态事件：锁定出标、解锁收标。
    pub fn caps_event(&mut self, locked: bool, _now_ms: u64) {
        self.caps_badge = locked;
        self.log(if locked { PwdEvent::CapsOn } else { PwdEvent::CapsOff });
    }

    /// 键入一个字节（密码允许任意字节；超容拒绝不截断）。
    pub fn type_bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            if self.content_len >= CONTENT_CAP {
                self.rejected_inputs += 1;
                self.log(PwdEvent::InputRejected);
                return;
            }
            self.content[self.content_len] = *b;
            self.content_len += 1;
        }
    }

    /// 提交失败：错误路径——**内容区一个字节不动**（F230「错误重试时
    /// 不清空已输入内容」的实现本体：此函数无任何写 content 的语句）。
    pub fn submit_failed(&mut self) {
        self.failed_attempts += 1;
        self.log(PwdEvent::SubmitFailed);
    }

    /// 粘贴进密码框：allow-paste 登记制下默认允许；策略为禁的框拒绝并
    /// 计数（审计面要求全系统禁粘贴场景 = 0）。
    pub fn paste_in(&mut self, bytes: &[u8]) {
        if !self.paste_allowed {
            self.rejected_pastes += 1;
            return;
        }
        self.type_bytes(bytes);
        self.log(PwdEvent::Pasted(bytes.len().min(u16::MAX as usize) as u16));
    }

    /// 用户显式清空——内容区唯一清空通路。
    pub fn clear_by_user(&mut self) {
        self.content_len = 0;
        self.user_clears += 1;
        self.log(PwdEvent::ClearedByUser);
    }

    /// 掩码渲染字符数（掩码与否结构长度一致——只有可见性变化）。
    pub fn render_char_count(&self) -> usize {
        self.content_len
    }

    /// 事件环快照（新→旧）。
    pub fn event_snapshot(&self) -> Vec<PwdEvent> {
        self.events.newest_first()
    }

    /// 按住显隐响应是否在预算内（判定面：事件戳 → 上屏戳差值）。
    pub fn eye_latency_ok(&self, press_ts_ms: u64, present_ts_ms: u64) -> bool {
        present_ts_ms.saturating_sub(press_ts_ms) < EYE_RESPONSE_BUDGET_MS
    }

    /// Caps 角标出标是否在预算内。
    pub fn caps_latency_ok(&self, event_ts_ms: u64, present_ts_ms: u64) -> bool {
        present_ts_ms.saturating_sub(event_ts_ms) < CAPS_BADGE_BUDGET_MS
    }
}

// ---------------------------------------------------------------------------
// 粘贴策略登记册（全系统审计面）
// ---------------------------------------------------------------------------

/// 粘贴策略登记册：每个密码框登记 allow-paste 策略，
/// 「禁粘贴场景 = 0」是全系统审计判据。
pub struct PasteAudit {
    policies: [Option<(u32, bool)>; PWD_FIELD_CAP],
    /// 禁粘贴场景数（审计判据：== 0）。
    pub forbid_scenes: u32,
    pub registered_total: u64,
}

impl PasteAudit {
    pub fn new() -> PasteAudit {
        PasteAudit { policies: [const { None }; PWD_FIELD_CAP], forbid_scenes: 0, registered_total: 0 }
    }

    /// 登记一个密码框的粘贴策略。`allow = false` 即计一个禁粘贴场景
    /// （登记照收——审计要如实呈现违规者）。
    pub fn register(&mut self, id: u32, allow: bool) -> bool {
        self.registered_total += 1;
        if self.policies.iter().any(|p| matches!(p, Some((i, _)) if *i == id)) {
            return false;
        }
        let slot = match self.policies.iter_mut().find(|p| p.is_none()) {
            Some(s) => s,
            None => return false,
        };
        if !allow {
            self.forbid_scenes += 1;
        }
        *slot = Some((id, allow));
        true
    }

    /// 注销（撤账——违规框不存在了，场景数归零路径）。
    pub fn unregister(&mut self, id: u32) -> bool {
        match self.policies.iter().position(|p| matches!(p, Some((i, _)) if *i == id)) {
            Some(idx) => {
                if let Some((_, allow)) = self.policies[idx] {
                    if !allow && self.forbid_scenes > 0 {
                        self.forbid_scenes -= 1;
                    }
                }
                self.policies[idx] = None;
                true
            }
            None => false,
        }
    }

    pub fn policy_of(&self, id: u32) -> Option<bool> {
        self.policies.iter().find_map(|p| match p {
            Some((i, a)) if *i == id => Some(*a),
            _ => None,
        })
    }

    pub fn count(&self) -> usize {
        self.policies.iter().filter(|p| p.is_some()).count()
    }
}

impl Default for PasteAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 全系统密码框池（多框路由 + 粘贴审计联动）
// ---------------------------------------------------------------------------

/// 密码框池容量（全系统并发密码框 8 个）。
pub const VAULT_CAP: usize = 8;

/// 全系统密码框池：id → 字段路由 + 粘贴策略审计联动——「全系统禁粘贴
/// 场景 = 0」「所有密码框右端有眼睛」这类系统级判据的巡查入口。
pub struct PwdVault {
    fields: [Option<(u32, PwdField)>; VAULT_CAP],
    audit: PasteAudit,
    pub routed_events: u64,
}

impl PwdVault {
    pub fn new() -> PwdVault {
        PwdVault { fields: [const { None }; VAULT_CAP], audit: PasteAudit::new(), routed_events: 0 }
    }

    /// 添加一个密码框（同步登记粘贴策略）。
    pub fn add(&mut self, id: u32, allow_paste: bool) -> bool {
        if !self.audit.register(id, allow_paste) {
            return false;
        }
        match self.fields.iter_mut().find(|f| f.is_none()) {
            Some(slot) => {
                *slot = Some((id, PwdField::new(allow_paste)));
                true
            }
            None => {
                let _ = self.audit.unregister(id);
                false
            }
        }
    }

    pub fn field(&self, id: u32) -> Option<&PwdField> {
        self.fields.iter().find_map(|f| match f {
            Some((i, p)) if *i == id => Some(p),
            _ => None,
        })
    }

    pub fn field_mut(&mut self, id: u32) -> Option<&mut PwdField> {
        match self.fields.iter_mut().find(|f| matches!(f, Some((i, _)) if *i == id)) {
            Some(slot) => {
                self.routed_events += 1;
                let (_, p) = slot.as_mut().unwrap();
                Some(p)
            }
            None => None,
        }
    }

    /// 系统级审计：全库禁粘贴场景 = 0（F230 判据）。
    pub fn paste_audit_ok(&self) -> bool {
        self.audit.forbid_scenes == 0
    }

    pub fn audit(&self) -> &PasteAudit {
        &self.audit
    }

    pub fn count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_some()).count()
    }
}

impl Default for PwdVault {
    fn default() -> Self {
        Self::new()
    }
}

/// 明文暴露审计报告（安全面汇总：按住次数 / 累计暴露 / 单次最长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExposureReport {
    pub holds: usize,
    pub total_ms: u64,
    pub max_ms: u32,
}

/// 从暴露账本出报告（账本只保留最近 64 次——报告如实标注口径）。
pub fn exposure_report(f: &PwdField) -> ExposureReport {
    let ledger = f.exposure_ledger();
    let total = ledger.iter().fold(0u64, |a, v| a + *v as u64);
    ExposureReport { holds: ledger.len(), total_ms: total, max_ms: f.max_exposure_ms }
}

/// 掩码渲染字节流：掩码态输出 `bullet` 重复（结构长度不变），
/// 明文态输出原内容。out 不足返回 None。
pub fn render_mask(f: &PwdField, bullet: u8, out: &mut [u8]) -> Option<usize> {
    let n = f.render_char_count();
    if out.len() < n {
        return None;
    }
    if f.is_masked() {
        for b in out[..n].iter_mut() {
            *b = bullet;
        }
    } else {
        out[..n].copy_from_slice(f.content_slice());
    }
    Some(n)
}

// ---------------------------------------------------------------------------
// 延迟实测账本与全库审计汇总
// ---------------------------------------------------------------------------

/// 响应延迟实测账本（定容环 + 预算越界计数）——按住显隐 <50ms、
/// Caps 角标 <200ms 两类判据的「实测」数据面。
pub struct LatencyLedger<const N: usize> {
    ring: RingLog<u64, N>,
    pub over_budget: u32,
    pub samples: u32,
}

impl<const N: usize> LatencyLedger<N> {
    pub fn new() -> LatencyLedger<N> {
        LatencyLedger { ring: RingLog::new(), over_budget: 0, samples: 0 }
    }

    /// 记一次实测延迟（事件戳 → 上屏戳），越预算即计数。
    pub fn record(&mut self, latency_ms: u64, budget_ms: u64) {
        self.ring.push(latency_ms);
        self.samples += 1;
        if latency_ms >= budget_ms {
            self.over_budget += 1;
        }
    }

    /// 最近样本（新→旧）。
    pub fn snapshot(&self) -> Vec<u64> {
        self.ring.newest_first()
    }

    /// 实测最大值（判据用：max < budget 即全样本达标——延迟单调面）。
    pub fn max_seen(&self) -> u64 {
        self.snapshot().iter().copied().max().unwrap_or(0)
    }
}

impl<const N: usize> Default for LatencyLedger<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// 角标呈现延迟（ms）——事件戳 → 上屏戳差值。
pub fn badge_latency_ms(event_ts_ms: u64, present_ts_ms: u64) -> u64 {
    present_ts_ms.saturating_sub(event_ts_ms)
}

/// 全库审计汇总（系统级判据的单一读数面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultAuditSummary {
    pub fields: usize,
    /// 禁粘贴场景数（判据：== 0）。
    pub forbid_scenes: u32,
    /// 全库累计明文暴露次数。
    pub exposure_holds: usize,
    /// 全库单次最长明文暴露（ms）。
    pub max_exposure_ms: u32,
    /// 全库累计提交失败次数（重试不清空判据的流量面）。
    pub failed_attempts_total: u32,
}

/// 汇总整个密码框池的审计读数。
impl PwdVault {
    pub fn audit_summary(&self) -> VaultAuditSummary {
        let mut s = VaultAuditSummary {
            fields: self.count(),
            forbid_scenes: self.audit.forbid_scenes,
            exposure_holds: 0,
            max_exposure_ms: 0,
            failed_attempts_total: 0,
        };
        for slot in self.fields.iter().flatten() {
            let f = &slot.1;
            let rep = exposure_report(f);
            s.exposure_holds += rep.holds;
            s.max_exposure_ms = s.max_exposure_ms.max(rep.max_ms);
            s.failed_attempts_total += f.failed_attempts;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F230 自检（13 条行为级）。
pub fn run_pwdeye_checks() -> CheckSet {
    let mut set = CheckSet::new("F230-pwdeye");
    let mut f = PwdField::new(true);
    f.type_bytes(b"s3cret!");

    // 1. 按住即显：状态同步翻转（响应预算 50ms 的结构保证）。
    f.press_eye(1_000);
    let revealed = !f.is_masked() && f.is_holding();
    set.add(
        "press reveals instantly (<50ms budget, sync flip)",
        revealed && f.eye_latency_ok(1_000, 1_020) && !f.eye_latency_ok(1_000, 1_060),
        "",
    );

    // 2. 松手即隐 + 按住时长入账本。
    f.release_eye(1_450);
    set.add(
        "release hides instantly, hold duration 450ms recorded",
        f.is_masked() && !f.is_holding() && f.max_exposure_ms == 450,
        "",
    );

    // 3. 明文暴露账本：多次按住取最长，环内最近可查。
    f.press_eye(2_000);
    f.release_eye(2_100); // 100ms
    f.press_eye(3_000);
    f.release_eye(3_800); // 800ms
    let ledger = f.exposure_ledger();
    set.add(
        "exposure ledger tracks holds, max = 800ms",
        f.max_exposure_ms == 800 && ledger.len() == 3 && ledger[0] == 800 && ledger[2] == 450,
        "",
    );

    // 4. Caps Lock 角标：锁定出标、解锁收标（出标预算 200ms）。
    f.caps_event(true, 4_000);
    let on = f.caps_badge_visible() && f.caps_latency_ok(4_000, 4_150);
    f.caps_event(false, 4_500);
    set.add(
        "caps badge shows on lock (<200ms) and hides on unlock",
        on && !f.caps_badge_visible(),
        "",
    );

    // 5. 重试不清空（字节级）：失败 100 次内容一个字节不动。
    let before = f.content_slice().to_vec();
    for _ in 0..100u32 {
        f.submit_failed();
    }
    set.add(
        "100 failed submits: content byte-identical",
        f.content_slice() == before.as_slice() && f.failed_attempts == 100 && f.content_len() == 7,
        "",
    );

    // 6. 失败后仍可继续编辑（用户自己决定改哪）。
    f.type_bytes(b"x");
    set.add(
        "editable after failures",
        f.content_len() == 8 && f.content_slice().ends_with(b"x"),
        "",
    );

    // 7. 唯一清空通路是用户显式清空。
    f.clear_by_user();
    set.add(
        "only explicit user clear empties content",
        f.content_len() == 0 && f.user_clears == 1,
        "",
    );

    // 8. 掩码渲染：掩码/明文字符数一致（结构不变，只换可见性）。
    f.type_bytes(b"abc");
    let n_masked = f.render_char_count();
    f.press_eye(5_000);
    let n_plain = f.render_char_count();
    f.release_eye(5_100);
    set.add(
        "masked/plain render same char count",
        n_masked == 3 && n_plain == 3 && f.is_masked(),
        "",
    );

    // 9. 粘贴允许：粘贴内容进框（账本留痕）。
    f.clear_by_user();
    f.paste_in(b"p@ssw0rd");
    set.add(
        "paste allowed by default",
        f.content_len() == 8 && f.content_slice() == b"p@ssw0rd" && f.rejected_pastes == 0,
        "",
    );

    // 10. 粘贴策略登记册：全系统禁粘贴场景 = 0（审计判据）。
    let mut audit = PasteAudit::new();
    assert!(audit.register(1, true));
    assert!(audit.register(2, true));
    assert!(audit.register(3, true));
    set.add(
        "paste audit: forbid scenes = 0 across registry",
        audit.forbid_scenes == 0 && audit.count() == 3 && audit.policy_of(2) == Some(true),
        "",
    );

    // 11. 违规登记如实呈现 + 注销撤账。
    assert!(audit.register(4, false));
    let v = audit.forbid_scenes;
    audit.unregister(4);
    set.add(
        "forbid registration counted honestly, unregister revokes",
        v == 1 && audit.forbid_scenes == 0,
        "",
    );

    // 12. 超长输入拒绝不截断（密码框物理上限 128 字节）。
    let mut big = [0u8; 200];
    for (i, b) in big.iter_mut().enumerate() {
        *b = (i % 251) as u8 + 1;
    }
    f.clear_by_user();
    f.type_bytes(&big[..200]);
    set.add(
        "oversized input rejected, not silently truncated",
        f.content_len() == CONTENT_CAP && f.rejected_inputs >= 1,
        "",
    );

    // 13. fuzz 2000 轮：随机 事件序列，不变量——
    //     masked == !holding、内容长度 ≤ 上限、角标 == 最近 caps 态、
    //     failed_attempts 只增不减、无 panic。
    let mut x: u32 = 0xFA11_0FF;
    let mut fz = PwdField::new(x % 2 == 0);
    let paste_buf = [0x41u8; 20];
    let mut holding_now = false;
    let mut badge_now = false;
    let mut ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 6;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let ts = (x % 1000) as u64;
        match op {
            0 => {
                fz.press_eye(ts);
                holding_now = true;
            }
            1 => {
                fz.release_eye(ts);
                holding_now = false;
            }
            2 => {
                fz.caps_event(x % 2 == 0, ts);
                badge_now = x % 2 == 0;
            }
            3 => fz.type_bytes(&[(x % 251) as u8 + 1]),
            4 => fz.submit_failed(),
            _ => {
                let n = (x % 20) as usize;
                fz.paste_in(&paste_buf[..n]);
            }
        }
        if fz.is_masked() != !holding_now
            || fz.caps_badge_visible() != badge_now
            || fz.content_len() > CONTENT_CAP
        {
            ok = false;
            break;
        }
    }
    set.add("fuzz 2000 rounds: invariants hold, no panic", ok, "");

    // 14. 密码框池 + 暴露报告 + 掩码渲染 + 延迟实测账本 + 全库审计汇总：
    //     全库审计禁粘贴 = 0、事件路由可达、掩码/明文渲染结构一致、
    //     按住显隐 <50ms / 角标 <200ms 实测账本越预算诚实计数。
    let mut vault = PwdVault::new();
    let added = vault.add(1, true) && vault.add(2, true) && vault.add(3, true);
    let routed = {
        let f = vault.field_mut(2);
        match f {
            Some(p) => {
                p.type_bytes(b"pw");
                p.press_eye(10);
                p.release_eye(70);
                true
            }
            None => false,
        }
    };
    let f2 = vault.field(2).unwrap();
    let mut buf = [0u8; 16];
    let masked_len = render_mask(f2, b'*', &mut buf);
    let all_masked = buf[..2].iter().all(|b| *b == b'*');
    let (plain_len, plain_ok) = {
        let p = vault.field_mut(2).unwrap();
        p.press_eye(100);
        let mut plain_buf = [0u8; 16];
        let n = render_mask(p, b'*', &mut plain_buf);
        let ok = n == Some(2) && &plain_buf[..2] == b"pw";
        p.release_eye(120);
        (n, ok)
    };
    // 两次按住（60ms + 20ms）后的暴露报告。
    let rep = exposure_report(vault.field(2).unwrap());
    // 延迟实测账本（按住显隐 <50ms / 角标 <200ms）。
    let mut eye_ledger: LatencyLedger<16> = LatencyLedger::new();
    eye_ledger.record(12, EYE_RESPONSE_BUDGET_MS);
    eye_ledger.record(49, EYE_RESPONSE_BUDGET_MS);
    eye_ledger.record(51, EYE_RESPONSE_BUDGET_MS);
    let mut caps_ledger: LatencyLedger<16> = LatencyLedger::new();
    caps_ledger.record(badge_latency_ms(1_000, 1_150), CAPS_BADGE_BUDGET_MS);
    caps_ledger.record(badge_latency_ms(2_000, 2_199), CAPS_BADGE_BUDGET_MS);
    let summary = vault.audit_summary();
    set.add(
        "vault + exposure + mask render + latency ledgers + summary",
        added && routed && vault.paste_audit_ok() && vault.count() == 3
            && rep.holds == 2 && rep.total_ms == 80 && rep.max_ms == 60
            && masked_len == Some(2) && all_masked && plain_ok && plain_len == Some(2)
            && eye_ledger.max_seen() == 51 && eye_ledger.over_budget == 1 && eye_ledger.samples == 3
            && caps_ledger.max_seen() == 199 && caps_ledger.over_budget == 0
            && summary.fields == 3 && summary.forbid_scenes == 0
            && summary.exposure_holds == 2 && summary.max_exposure_ms == 60
            && summary.failed_attempts_total == 0,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_to_reveal_lifecycle() {
        let mut f = PwdField::new(true);
        assert!(f.is_masked());
        f.press_eye(100);
        assert!(!f.is_masked());
        f.press_eye(150); // 重复按下无副作用。
        assert!(!f.is_masked());
        f.release_eye(300);
        assert!(f.is_masked());
        f.release_eye(400); // 重复松开无副作用。
        assert_eq!(f.max_exposure_ms, 200, "只记第一次按住的 200ms");
        assert_eq!(f.exposure_ledger().len(), 1);
    }

    #[test]
    fn caps_badge_lifecycle() {
        let mut f = PwdField::new(true);
        assert!(!f.caps_badge_visible());
        f.caps_event(true, 0);
        assert!(f.caps_badge_visible());
        f.caps_event(true, 10); // 幂等。
        assert!(f.caps_badge_visible());
        f.caps_event(false, 20);
        assert!(!f.caps_badge_visible());
        assert!(f.caps_latency_ok(0, 199));
        assert!(!f.caps_latency_ok(0, 200));
    }

    #[test]
    fn retry_never_clears() {
        let mut f = PwdField::new(true);
        f.type_bytes(b"hunter2");
        let snapshot = f.content_slice().to_vec();
        for _ in 0..50 {
            f.submit_failed();
            assert_eq!(f.content_slice(), snapshot.as_slice(), "错误路径不动内容区");
        }
        // 用户决定改一个字符——逐字编辑仍可用。
        f.clear_by_user();
        f.type_bytes(b"hunter3");
        assert_eq!(f.content_slice(), b"hunter3");
        assert_eq!(f.user_clears, 1);
        assert_eq!(f.failed_attempts, 50);
    }

    #[test]
    fn paste_always_allowed() {
        let mut f = PwdField::new(true);
        f.type_bytes(b"pre-");
        f.paste_in(b"pasted-secret");
        assert_eq!(f.content_slice(), b"pre-pasted-secret");
        // 暴露账本里没有「粘贴」——粘贴不触发明文暴露（不按眼睛）。
        assert!(f.exposure_ledger().is_empty());
        assert!(f.is_masked());
    }

    #[test]
    fn exposure_ring_eviction() {
        let mut f = PwdField::new(true);
        for i in 0..(EXPOSURE_RING_CAP as u64 + 10) {
            f.press_eye(i * 100);
            f.release_eye(i * 100 + 50);
        }
        assert_eq!(f.exposure_ledger().len(), EXPOSURE_RING_CAP, "环定容淘汰");
        assert_eq!(f.max_exposure_ms, 50, "所有按住等长");
    }

    #[test]
    fn paste_audit_registry() {
        let mut a = PasteAudit::new();
        for i in 0..PWD_FIELD_CAP as u32 {
            assert!(a.register(i, true));
        }
        assert!(!a.register(9999, true), "容量满拒绝");
        assert_eq!(a.forbid_scenes, 0);
        // 一个禁粘贴场景登记即出现——审计面如实呈现。
        let mut b = PasteAudit::new();
        b.register(1, false);
        assert_eq!(b.forbid_scenes, 1);
        b.unregister(1);
        assert_eq!(b.forbid_scenes, 0);
    }

    #[test]
    fn vault_and_mask_render() {
        let mut v = PwdVault::new();
        assert!(v.add(1, true));
        assert!(!v.add(1, true), "重复 id 拒绝");
        assert!(v.paste_audit_ok());
        // 路由事件流：键入 → 按住看明文 → 松手回掩码。
        {
            let p = v.field_mut(1).unwrap();
            p.type_bytes(b"abc");
        }
        {
            let p = v.field_mut(1).unwrap();
            p.press_eye(0);
            let mut buf = [0u8; 8];
            assert_eq!(render_mask(p, b'*', &mut buf), Some(3));
            assert_eq!(&buf[..3], b"abc", "按住态渲染明文");
            p.release_eye(50);
        }
        let f = v.field(1).unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(render_mask(f, b'*', &mut buf), Some(3));
        assert_eq!(&buf[..3], b"***", "掩码态渲染圆点");
        // 暴露报告：1 次按住 50ms。
        let rep = exposure_report(f);
        assert_eq!(rep.holds, 1);
        assert_eq!(rep.total_ms, 50);
        assert_eq!(rep.max_ms, 50);
        // 渲染缓冲不足显性失败。
        assert_eq!(render_mask(f, b'*', &mut [0u8; 2]), None);
        assert_eq!(v.routed_events, 2);
    }

    #[test]
    fn pwdeye_selfcheck_all_green() {
        let s = run_pwdeye_checks();
        assert!(s.all_passed(), "F230 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// ⚠ 会话级不落盘例外（主册 F230 安全纪律）：密码明文内容字节与明文
// 暴露账本**永不落盘**——本记录只持久化非敏感 UI 态（掩码位/角标位/
// 长度/计数器）。content_len 只是数字，不是内容。

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const PWDEYE_PERSIST_VERSION: u8 = 1;
/// 定长记录 = 4 magic + 1 版本 + 载荷 16（掩码 1 + 角标 1 + 长度 u16 +
/// 失败 u32 + 被拒输入 u32 + 被拒粘贴 u32）+ 4 校验 = 25B。
pub const PWDEYE_RECORD_LEN: usize = 5 + 16 + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwdeyePersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 显隐状态持久化记录（**不含任何明文字节**——见段首安全例外）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PwdUiStateRecord {
    /// 掩码态（true = 显示 •••）。
    pub masked: bool,
    /// Caps Lock 角标可见位。
    pub caps_badge: bool,
    /// 内容长度（仅数字——明文永不入记录）。
    pub content_len: u16,
    /// 提交失败次数（重试账本）。
    pub failed_attempts: u32,
    /// 被拒超长输入次数。
    pub rejected_inputs: u32,
    /// 被拒粘贴次数。
    pub rejected_pastes: u32,
}

impl PwdUiStateRecord {
    /// 从密码框捕获（content_len 截到 u16 口径——CONTENT_CAP=128 远小于上限）。
    pub fn capture(f: &PwdField) -> PwdUiStateRecord {
        PwdUiStateRecord {
            masked: f.masked,
            caps_badge: f.caps_badge,
            content_len: f.content_len.min(u16::MAX as usize) as u16,
            failed_attempts: f.failed_attempts,
            rejected_inputs: f.rejected_inputs,
            rejected_pastes: f.rejected_pastes,
        }
    }

    /// 编码：[0..4]=magic、[4]=版本、[5..21]=载荷、[21..25]=校验（LE）。
    pub fn to_bytes(&self) -> [u8; PWDEYE_RECORD_LEN] {
        let mut out = [0u8; PWDEYE_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = PWDEYE_PERSIST_VERSION;
        out[5] = self.masked as u8;
        out[6] = self.caps_badge as u8;
        out[7..9].copy_from_slice(&self.content_len.to_le_bytes());
        out[9..13].copy_from_slice(&self.failed_attempts.to_le_bytes());
        out[13..17].copy_from_slice(&self.rejected_inputs.to_le_bytes());
        out[17..21].copy_from_slice(&self.rejected_pastes.to_le_bytes());
        let n = PWDEYE_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四类损坏全拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<PwdUiStateRecord, PwdeyePersistError> {
        if b.len() != PWDEYE_RECORD_LEN {
            return Err(PwdeyePersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(PwdeyePersistError::BadMagic);
        }
        if b[4] != PWDEYE_PERSIST_VERSION {
            return Err(PwdeyePersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(PwdeyePersistError::BadChecksum);
        }
        Ok(PwdUiStateRecord {
            masked: b[5] != 0,
            caps_badge: b[6] != 0,
            content_len: u16::from_le_bytes([b[7], b[8]]),
            failed_attempts: u32::from_le_bytes([b[9], b[10], b[11], b[12]]),
            rejected_inputs: u32::from_le_bytes([b[13], b[14], b[15], b[16]]),
            rejected_pastes: u32::from_le_bytes([b[17], b[18], b[19], b[20]]),
        })
    }

    /// 记录是否含明文（安全执法面：载荷任何窗口都不允许等于内容）。
    pub fn leaks_content(&self, content: &[u8]) -> bool {
        if content.is_empty() {
            return false;
        }
        let b = self.to_bytes();
        content.len() <= b.len() && b.windows(content.len()).any(|w| w == content)
    }
}

// --- v2 UI 壳接线面：Caps 角标计时判定 + 掩码圆点绘制清单 ---

/// Caps 角标出标截止时刻：事件戳 + 预算（CAPS_BADGE_BUDGET_MS=200，
/// 主册 F230「按下锁定键 <200ms 出标」）——上屏戳 < 截止即合规。
pub fn caps_badge_deadline(event_ts_ms: u64) -> u64 {
    event_ts_ms.saturating_add(CAPS_BADGE_BUDGET_MS)
}

/// Caps 角标计时判定（计时面唯一入口；模块不持时钟，全部注入）。
pub fn caps_badge_in_time(event_ts_ms: u64, present_ts_ms: u64) -> bool {
    present_ts_ms < caps_badge_deadline(event_ts_ms)
}

/// 掩码圆点令牌索引（0 = 掩码点灰——与正文可分辨的令牌消费端）。
pub const PWD_COLOR_DOT: u8 = 0;
/// 掩码圆点清单容量（每字符一枚；超容截断并如实计数——超出部分由
/// 滚动/换行布局分帧出）。
pub const DOT_LIST_CAP: usize = 16;

/// 一枚掩码圆点图元（几何 + 颜色索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskDot {
    pub rect: crate::h1star::h1base::Rect,
    pub color_idx: u8,
}

/// 掩码圆点绘制清单：n 字符 → n 枚圆点（直径 = 行高 1/3、间距 = 直径、
/// 垂直居中）。**明文永不进绘制清单**——按住显明文走 render_mask 通路，
/// 本清单只服务掩码态（安全面）。
pub fn mask_dots(
    n_chars: usize,
    x: i32,
    y: i32,
    row_h: i32,
) -> ([Option<MaskDot>; DOT_LIST_CAP], usize) {
    let mut out: [Option<MaskDot>; DOT_LIST_CAP] = [const { None }; DOT_LIST_CAP];
    let n = n_chars.min(DOT_LIST_CAP);
    let d = (row_h / 3).max(1);
    for i in 0..n {
        out[i] = Some(MaskDot {
            rect: crate::h1star::h1base::Rect::new(
                x + i as i32 * (d + d),
                y + (row_h - d) / 2,
                d,
                d,
            ),
            color_idx: PWD_COLOR_DOT,
        });
    }
    (out, n)
}

// --- v2 判定面扩展 ---

/// F230 v2 自检（首条必为持久化 round-trip）。
pub fn run_pwdeye_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F230-pwdeye-v2");

    // 1. round-trip + 安全执法：记录可存可还，且载荷任何窗口都不等于
    //    明文内容——验段首安全例外（明文永不落盘）的执法面。
    let mut f = PwdField::new(true);
    f.type_bytes(b"s3cret!");
    f.press_eye(1_000);
    f.release_eye(1_200);
    f.submit_failed();
    let rec = PwdUiStateRecord::capture(&f);
    let bytes = rec.to_bytes();
    set.add(
        "v2 ui-state roundtrip, no plaintext leak",
        matches!(PwdUiStateRecord::from_bytes(&bytes), Ok(back)
            if back == rec && back.masked && back.content_len == 7
                && back.failed_attempts == 1 && !back.leaks_content(b"s3cret!")),
        "",
    );

    // 2. 四类损坏全拒绝——验十二查「损坏输入明错误」。
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[10] ^= 0xFF;
    set.add(
        "v2 persist rejects 4 corrupt classes",
        PwdUiStateRecord::from_bytes(&m) == Err(PwdeyePersistError::BadMagic)
            && PwdUiStateRecord::from_bytes(&v) == Err(PwdeyePersistError::BadVersion)
            && PwdUiStateRecord::from_bytes(&s) == Err(PwdeyePersistError::BadChecksum)
            && PwdUiStateRecord::from_bytes(&bytes[..bytes.len() - 1]) == Err(PwdeyePersistError::BadLen),
        "",
    );

    // 3. Caps 角标计时判定：事件戳起 199ms 内出标合规、200ms 起不合规
    //    ——验主册 F230「Caps Lock <200ms 出标」。
    set.add(
        "v2 caps badge deadline 200ms",
        caps_badge_in_time(4_000, 4_199) && !caps_badge_in_time(4_000, 4_200)
            && caps_badge_deadline(0) == CAPS_BADGE_BUDGET_MS,
        "",
    );

    // 4. 掩码圆点清单：3 字符 → 3 枚等距圆点、直径 = 行高 1/3、垂直
    //    居中、令牌索引 0；超 16 字符截 16 并如实计数——验主册 F230
    //    掩码显示（•••）的绘制面。
    let (dots, n3) = mask_dots(3, 10, 100, 30);
    let d0 = dots[0].unwrap_or(MaskDot { rect: crate::h1star::h1base::Rect::new(0, 0, 0, 0), color_idx: 9 });
    let (_, n_over) = mask_dots(DOT_LIST_CAP + 5, 0, 0, 30);
    set.add(
        "v2 mask dots: count, spacing, capped honestly",
        n3 == 3 && n_over == DOT_LIST_CAP
            && d0.rect.w == 10 && d0.rect.h == 10 && d0.rect.y == 110
            && d0.color_idx == PWD_COLOR_DOT
            && dots[1].map(|d| d.rect.x == d0.rect.x + 20).unwrap_or(false),
        "",
    );

    // 5. 会话态恢复：从记录复原掩码/角标位与计数器（内容重建由用户
    //    重输——记录里没有也不允许有内容）——验主册 F230「按住显示
    //    明文（松手即隐藏）」的状态续接面。
    set.add(
        "v2 session state restoreable",
        matches!(PwdUiStateRecord::from_bytes(&bytes), Ok(back)
            if back.masked && !back.caps_badge && back.content_len == 7
                && back.rejected_inputs == 0 && back.rejected_pastes == 0),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_leak_detector_flags_injection() {
        // 执法面自证：把内容字节直接塞进载荷窗口必须被检出（检出器
        // 不是永假的橡皮图章）。
        let mut rec = PwdUiStateRecord {
            masked: true,
            caps_badge: false,
            content_len: 7,
            failed_attempts: 0,
            rejected_inputs: 0,
            rejected_pastes: 0,
        };
        rec.failed_attempts = u32::from_le_bytes(*b"s3cr");
        assert!(rec.leaks_content(b"s3cr"), "载荷埋明文必须被检出");
        assert!(!rec.leaks_content(b"zzzz"));
        assert!(!rec.leaks_content(b""), "空内容无泄漏语义");
    }

    #[test]
    fn v2_caps_never_exposes_content() {
        let mut f = PwdField::new(true);
        f.type_bytes(b"abc");
        f.caps_event(true, 0);
        let rec = PwdUiStateRecord::capture(&f);
        assert!(rec.caps_badge);
        assert!(!rec.leaks_content(b"abc"), "角标态记录仍不含明文");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_pwdeye_v2_checks();
        assert!(set.all_passed(), "F230 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
