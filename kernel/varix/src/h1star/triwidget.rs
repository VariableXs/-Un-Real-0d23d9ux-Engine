//! F216 复选/单选/开关三控件规范 · 判据实装（H 基础通用域 · AI-H1）。
//!
//! **判据锚**：主册 F216「复选/单选/开关三控件规范」。
//!
//! **验收标准（主册第一句）**：全系统控件用法审计表（每处标注类型+
//! 理由）；尺寸/动画实测对基线；混用场景=0；开关切换响应 <100ms。
//!
//! **设计要点**：
//! - 三控件职责写死：复选（方形）=「这些都选上」的多选属性组；单选
//!   （圆形）=互斥「哪个」；开关（胶囊）=即时生效的「要不要」——
//!   语义错配在**审计层**判红（Switch 回答 WhichOne = 混用）；
//! - 提交语义分叉：开关切换**即时生效**（可改可退，E 域三铁律）；
//!   复选/单选在表单里随「确定」提交（staged → commit）；
//! - 尺寸/动画基线：复选/单选 16px、开关 40×20px、动画 120ms 进入
//!   曲线（h1base F124 谱），开关切换响应预算 <100ms。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Curve/MotionPolicy）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy};

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 复选/单选边长——主册 F216：「复选/单选 16px」。
pub const CHECK_RADIO_PX: i32 = 16;

/// 开关尺寸——主册 F216：「开关 40×20px」。
pub const SWITCH_SIZE: (i32, i32) = (40, 20);

/// 控件动画时长——主册 F216：「动画 120ms 进入曲线」。
pub const CONTROL_ANIM_MS: u32 = 120;

/// 开关切换响应预算——主册 F216：「开关切换响应 <100ms」。
pub const SWITCH_RESPONSE_MS: u32 = 100;

// ---------------------------------------------------------------------------
// 控件语义模型
// ---------------------------------------------------------------------------

/// 三控件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// 复选框（方形）。
    Checkbox,
    /// 单选（圆形）。
    Radio,
    /// 开关（胶囊）。
    Switch,
}

/// 使用处的语义问题（控件在回答什么问题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Question {
    /// 「要不要」——独立设置。
    Whether,
    /// 「哪个」——互斥单选。
    WhichOne,
    /// 「这些都选上」——多选属性组。
    MultiProps,
}

/// 用法审计条目（「每处标注类型+理由」的登记结构）。
#[derive(Clone, Copy, Debug)]
pub struct UsageSite {
    pub where_: &'static str,
    pub kind: ControlKind,
    pub question: Question,
}

/// 单点混用判定（规则唯一实现点）：
/// - Switch 只许回答 Whether；
/// - Radio 只许回答 WhichOne；
/// - Checkbox 只许回答 MultiProps；
/// - Switch + WhichOne / Radio + Whether / Checkbox + Whether = 混用。
pub fn site_ok(site: &UsageSite) -> bool {
    matches!(
        (site.kind, site.question),
        (ControlKind::Switch, Question::Whether)
            | (ControlKind::Radio, Question::WhichOne)
            | (ControlKind::Checkbox, Question::MultiProps)
    )
}

/// 全系统审计表扫描：混用场景=0 才算过。
pub fn audit_sites(sites: &[UsageSite]) -> Vec<&'static str> {
    sites.iter().filter(|s| !site_ok(s)).map(|s| s.where_).collect()
}

// ---------------------------------------------------------------------------
// 提交语义：开关即时 / 复选单选随确定提交
// ---------------------------------------------------------------------------

/// 开关切换：返回 (新状态, 响应是否达标)——状态立即翻转（即时生效），
/// 响应耗时（调用方注入实测值）< 预算。
pub fn toggle_switch(now_on: bool, applied_within_ms: u32) -> (bool, bool) {
    (!now_on, applied_within_ms < SWITCH_RESPONSE_MS)
}

/// 表单暂存区：复选/单选的改动先入暂存，随「确定」一次性提交。
#[derive(Clone)]
pub struct FormStage {
    staged: Vec<(String, bool)>,
    committed: Vec<(String, bool)>,
}

impl FormStage {
    pub fn new() -> FormStage {
        FormStage { staged: Vec::new(), committed: Vec::new() }
    }

    /// 暂存一次改动（复选/单选）。
    pub fn stage(&mut self, key: &str, value: bool) {
        if let Some(slot) = self.staged.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
        } else {
            self.staged.push((key.to_string(), value));
        }
    }

    /// 「确定」提交：暂存清空、生效区更新。返回生效条目数。
    pub fn commit(&mut self) -> usize {
        let n = self.staged.len();
        for (k, v) in self.staged.drain(..) {
            if let Some(slot) = self.committed.iter_mut().find(|(c, _)| c == &k) {
                slot.1 = v;
            } else {
                self.committed.push((k, v));
            }
        }
        n
    }

    /// 暂存未提交数（取消路径用：丢弃即无残留）。
    pub fn pending(&self) -> usize {
        self.staged.len()
    }

    /// 生效值读取。
    pub fn value_of(&self, key: &str) -> Option<bool> {
        self.committed.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    /// 「取消」：清暂存，生效区不动（可改可退的表单面）。
    pub fn cancel(&mut self) {
        self.staged.clear();
    }
}

impl Default for FormStage {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F216 自检（判据面：审计表 + 尺寸动画基线 + 混用=0 + 开关响应）。
pub fn run_triwidget_checks() -> CheckSet {
    let mut set = CheckSet::new("F216-triwidget");

    // 1. 审计表：每处标注类型+理由（question 语义即理由面）——结构在位。
    let sites = [
        UsageSite { where_: "settings.dark", kind: ControlKind::Switch, question: Question::Whether },
        UsageSite { where_: "theme.pick", kind: ControlKind::Radio, question: Question::WhichOne },
        UsageSite { where_: "export.fields", kind: ControlKind::Checkbox, question: Question::MultiProps },
    ];
    set.add("usage sites registered with reasons", sites.iter().all(site_ok), "");

    // 2. 混用场景=0：三类错配全部判红（审计可捕获）。
    let mixed = [
        UsageSite { where_: "bad1", kind: ControlKind::Switch, question: Question::WhichOne },
        UsageSite { where_: "bad2", kind: ControlKind::Radio, question: Question::Whether },
        UsageSite { where_: "bad3", kind: ControlKind::Checkbox, question: Question::Whether },
    ];
    set.add(
        "misuse audit catches all three",
        audit_sites(&mixed).len() == 3 && audit_sites(&sites).is_empty(),
        "",
    );

    // 3. 尺寸基线：复选/单选 16px、开关 40×20px。
    set.add(
        "sizes on baseline",
        CHECK_RADIO_PX == 16 && SWITCH_SIZE == (40, 20),
        "",
    );

    // 4. 动画基线：120ms 进入曲线；F245 降级 80ms。
    set.add(
        "anim on baseline",
        CONTROL_ANIM_MS == 120
            && MotionPolicy::normal().duration_ms(Curve::Enter, CONTROL_ANIM_MS) == 120
            && MotionPolicy::reduced().duration_ms(Curve::Enter, CONTROL_ANIM_MS) == 80,
        "",
    );

    // 5. 开关切换响应 <100ms（50ms 实测样本过线、120ms 样本判红）。
    let (_, fast_ok) = toggle_switch(false, 50);
    let (_, slow_ok) = toggle_switch(false, 120);
    set.add("switch response <100ms", fast_ok && !slow_ok, "");

    // 6. 开关即时生效：状态立即翻转（不进暂存区）。
    let (on, _) = toggle_switch(false, 10);
    set.add("switch flips immediately", on, "");

    // 7. 复选/单选随确定提交：暂存→确定→生效；取消→零残留。
    let mut f = FormStage::new();
    f.stage("a", true);
    f.stage("b", false);
    f.stage("a", true); // 同键覆盖不重复
    set.add(
        "form commit semantics",
        f.pending() == 2 && f.commit() == 2 && f.value_of("a") == Some(true) && f.value_of("b") == Some(false),
        "",
    );

    // 8. 取消路径：暂存丢弃、生效区不动。
    let mut f = FormStage::new();
    f.stage("x", true);
    f.commit();
    f.stage("x", false);
    f.cancel();
    set.add("cancel keeps committed values", f.pending() == 0 && f.value_of("x") == Some(true), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misuse_matrix_complete() {
        // 3×3 全矩阵：只有对角线合法。
        for kind in [ControlKind::Checkbox, ControlKind::Radio, ControlKind::Switch] {
            for q in [Question::Whether, Question::WhichOne, Question::MultiProps] {
                let s = UsageSite { where_: "m", kind, question: q };
                assert_eq!(site_ok(&s), matches!(
                    (kind, q),
                    (ControlKind::Switch, Question::Whether)
                        | (ControlKind::Radio, Question::WhichOne)
                        | (ControlKind::Checkbox, Question::MultiProps)
                ));
            }
        }
    }

    #[test]
    fn switch_toggle_roundtrip() {
        let (a, _) = toggle_switch(false, 10);
        assert!(a);
        let (b, _) = toggle_switch(a, 10);
        assert!(!b);
    }

    #[test]
    fn form_stage_overwrite_and_commit() {
        let mut f = FormStage::new();
        f.stage("k", true);
        f.stage("k", false);
        assert_eq!(f.pending(), 1);
        f.commit();
        assert_eq!(f.value_of("k"), Some(false));
        assert_eq!(f.pending(), 0);
    }

    #[test]
    fn triwidget_selfcheck_all_green() {
        let set = run_triwidget_checks();
        assert!(set.all_passed(), "F216 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 6 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F216 验收定义的实装细化，非新立项）：持久化面 = 三控
// 件审计记录（类型+理由逐条）的 VXH1 定长记录；壳接线面 = 三控件命中区几
// 何（基线尺寸+触控外扩）+ 开关切换动画帧清单；判定面 = v2 checks。零堆定长。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 审计槽位容量（16 处控件用法登记封顶）与记录定长（4+1+33+4）。
pub const V2_AUDIT_CAP: usize = 16;
pub const V2_RECORD_BYTES: usize = 42;

/// v2 持久化错误：损坏输入显性拒绝（四类 + 字段域越界一类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength, BadField }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 控件类型/语义理由的持久化编码与解码（越界 → None）。
fn kind_code(k: ControlKind) -> u8 {
    match k {
        ControlKind::Checkbox => 0,
        ControlKind::Radio => 1,
        ControlKind::Switch => 2,
    }
}
fn kind_from_code(v: u8) -> Option<ControlKind> {
    match v {
        0 => Some(ControlKind::Checkbox),
        1 => Some(ControlKind::Radio),
        2 => Some(ControlKind::Switch),
        _ => None,
    }
}
fn question_code(q: Question) -> u8 {
    match q {
        Question::Whether => 0,
        Question::WhichOne => 1,
        Question::MultiProps => 2,
    }
}
fn question_from_code(v: u8) -> Option<Question> {
    match v {
        0 => Some(Question::Whether),
        1 => Some(Question::WhichOne),
        2 => Some(Question::MultiProps),
        _ => None,
    }
}

/// 三控件审计记录（持久化面）：判据「全系统控件用法审计表（每处标注
/// 类型+理由）」的存档载体——逐处登记类型与语义理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageAuditRecord {
    pub count: u8,
    pub kind: [u8; V2_AUDIT_CAP],
    pub question: [u8; V2_AUDIT_CAP],
}

impl UsageAuditRecord {
    /// 采集：审计表逐条登记（超容量如实返回 None——不静默截断）。
    pub fn capture(sites: &[UsageSite]) -> Option<UsageAuditRecord> {
        if sites.len() > V2_AUDIT_CAP {
            return None;
        }
        let mut rec = UsageAuditRecord { count: sites.len() as u8, kind: [0; V2_AUDIT_CAP], question: [0; V2_AUDIT_CAP] };
        for (i, s) in sites.iter().enumerate() {
            rec.kind[i] = kind_code(s.kind);
            rec.question[i] = question_code(s.question);
        }
        Some(rec)
    }

    /// 解码回审计条目并逐条跑 site_ok（混用场景=0 的存档复核面）。
    pub fn audit_ok(&self) -> bool {
        (0..self.count as usize).all(|i| match (kind_from_code(self.kind[i]), question_from_code(self.question[i])) {
            (Some(k), Some(q)) => site_ok(&UsageSite { where_: "archived", kind: k, question: q }),
            _ => false,
        })
    }

    /// 编码：VXH1 + 版本 + 33 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.count;
        for i in 0..V2_AUDIT_CAP {
            out[6 + i] = self.kind[i];
            out[22 + i] = self.question[i];
        }
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门 + 槽位值域校验。
    pub fn from_bytes(b: &[u8]) -> Result<UsageAuditRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[38], b[39], b[40], b[41]]);
        if v2_fnv1a(&b[..38]) != sum { return Err(V2PersistError::BadChecksum); }
        if b[5] as usize > V2_AUDIT_CAP {
            return Err(V2PersistError::BadLength);
        }
        for i in 0..V2_AUDIT_CAP {
            if kind_from_code(b[6 + i]).is_none() || question_from_code(b[22 + i]).is_none() {
                return Err(V2PersistError::BadField);
            }
        }
        let mut rec = UsageAuditRecord { count: b[5], kind: [0; V2_AUDIT_CAP], question: [0; V2_AUDIT_CAP] };
        rec.kind.copy_from_slice(&b[6..22]);
        rec.question.copy_from_slice(&b[22..38]);
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：三控件命中区几何 + 开关切换动画帧清单
// ---------------------------------------------------------------------------

/// 命中区触控外扩（命中判据与尺寸判据分离：显示尺寸对基线，命中区外扩）。
pub const HIT_PADDING_PX: i32 = 8;

/// 控件本体矩形（对基线：复选/单选 16px 方形、开关 40×20 胶囊）。
pub fn control_rect(kind: ControlKind, x: i32, y: i32) -> crate::h1star::h1base::Rect {
    match kind {
        ControlKind::Checkbox | ControlKind::Radio => crate::h1star::h1base::Rect::new(x, y, CHECK_RADIO_PX, CHECK_RADIO_PX),
        ControlKind::Switch => crate::h1star::h1base::Rect::new(x, y, SWITCH_SIZE.0, SWITCH_SIZE.1),
    }
}

/// 命中测试（点→是否命中该控件）：本体 + 8px 触控外扩区。
pub fn hit_test(kind: ControlKind, x: i32, y: i32, px: i32, py: i32) -> bool {
    let r = control_rect(kind, x, y);
    let hit = crate::h1star::h1base::Rect::new(
        r.x - HIT_PADDING_PX,
        r.y - HIT_PADDING_PX,
        r.w + 2 * HIT_PADDING_PX,
        r.h + 2 * HIT_PADDING_PX,
    );
    hit.contains(px, py)
}

/// 开关滑块动画帧清单容量（120ms 进入曲线按 12ms 步进 11 帧含终态）。
pub const SWITCH_FRAME_CAP: usize = 11;

/// 生成开关开启动画的滑块 x 偏移清单（0 → 行程 20px；F124 进入曲线整数
/// 定点——「尺寸/动画实测对基线」的帧面；F245 降级全帧直切到行程）。
pub fn switch_thumb_frames(policy: MotionPolicy) -> ([i32; SWITCH_FRAME_CAP], usize) {
    let travel = SWITCH_SIZE.0 - SWITCH_SIZE.1; // 40-20 = 20px 行程
    let dur = policy.duration_ms(Curve::Enter, CONTROL_ANIM_MS);
    let mut out = [0i32; SWITCH_FRAME_CAP];
    let step = (dur / (SWITCH_FRAME_CAP as u32 - 1)).max(1);
    for (i, slot) in out.iter_mut().enumerate() {
        let p = policy.progress(Curve::Enter, step * i as u32, CONTROL_ANIM_MS);
        *slot = (travel as u32 * p / 1000) as i32;
    }
    (out, SWITCH_FRAME_CAP)
}

/// F216 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_triwidget_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F216-triwidget-v2");
    let sites = [
        UsageSite { where_: "settings.dark", kind: ControlKind::Switch, question: Question::Whether },
        UsageSite { where_: "theme.pick", kind: ControlKind::Radio, question: Question::WhichOne },
        UsageSite { where_: "export.fields", kind: ControlKind::Checkbox, question: Question::MultiProps },
    ];
    let rec = match UsageAuditRecord::capture(&sites) {
        Some(r) => r,
        None => UsageAuditRecord { count: 0, kind: [0; V2_AUDIT_CAP], question: [0; V2_AUDIT_CAP] },
    };
    let blob = rec.to_bytes();
    // 1. round-trip：审计记录采集→编码→解码逐字段相等（v2 记录纪律）。
    set.add(
        "v2 record round-trip usage audit",
        UsageAuditRecord::from_bytes(&blob) == Ok(rec) && rec.count == 3,
        "",
    );
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        UsageAuditRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && UsageAuditRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && UsageAuditRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && UsageAuditRecord::from_bytes(&blob[..41]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 验「每处标注类型+理由」存档复核面：合法审计全过、槽位越界显性拒绝。
    // 缺陷账本：现象=「archived audit ok + field domain」红；根因=翻位字节
    // [6] 落在校验和覆盖区（[..38]）且未重算校验和，校验门先行拒绝返回
    // BadChecksum，BadField 分支从未被真测；修法=翻转载荷字节后按同一
    // FNV-1a 口径重算校验和再送入（判据「损坏输入显性拒绝」须逐分支可达）。
    let mut bad_field = blob;
    bad_field[6] = 9; // kind 越界
    let sum = v2_fnv1a(&bad_field[..V2_RECORD_BYTES - 4]);
    bad_field[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
    set.add(
        "archived audit ok + field domain",
        rec.audit_ok() && UsageAuditRecord::from_bytes(&bad_field) == Err(V2PersistError::BadField),
        "",
    );
    // 4. 验「尺寸/动画实测对基线」命中面：本体 16px/40×20、8px 外扩边界。
    set.add(
        "hit region baseline + padding",
        control_rect(ControlKind::Checkbox, 0, 0).w == 16
            && control_rect(ControlKind::Switch, 0, 0).w == 40
            && hit_test(ControlKind::Checkbox, 0, 0, 23, 0)
            && !hit_test(ControlKind::Checkbox, 0, 0, 24, 0)
            && hit_test(ControlKind::Switch, 0, 0, 47, 10),
        "",
    );
    // 5. 验动画基线帧面：首帧 0、末帧=行程 20、单调不减；F245 降级全帧直切。
    let (frames, n) = switch_thumb_frames(MotionPolicy::normal());
    let mono = (1..n).all(|i| frames[i] >= frames[i - 1]);
    let (rframes, _) = switch_thumb_frames(MotionPolicy::reduced());
    set.add(
        "switch frames 0->travel + reduced cut",
        n == SWITCH_FRAME_CAP && frames[0] == 0 && frames[n - 1] == SWITCH_SIZE.0 - SWITCH_SIZE.1 && mono
            && rframes.iter().all(|&f| f == SWITCH_SIZE.0 - SWITCH_SIZE.1),
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn usage_record_round_trip_and_reject() {
        let sites = [UsageSite { where_: "a", kind: ControlKind::Switch, question: Question::Whether }];
        let rec = UsageAuditRecord::capture(&sites).unwrap();
        let blob = rec.to_bytes();
        assert_eq!(UsageAuditRecord::from_bytes(&blob), Ok(rec));
        assert!(UsageAuditRecord::from_bytes(&vec![0u8; 5]).is_err());
        // 超容量采集如实拒绝。
        let many = [UsageSite { where_: "x", kind: ControlKind::Radio, question: Question::WhichOne }; V2_AUDIT_CAP + 1];
        assert!(UsageAuditRecord::capture(&many).is_none());
    }

    #[test]
    fn hit_and_frames_edges() {
        assert!(!hit_test(ControlKind::Radio, 100, 100, 100, 124)); // 下方外扩恰出界
        assert!(hit_test(ControlKind::Radio, 100, 100, 100, 123));
        let (frames, n) = switch_thumb_frames(MotionPolicy::normal());
        assert_eq!(frames[n - 1], 20);
    }

    #[test]
    fn triwidget_v2_selfcheck_all_green() {
        let set = run_triwidget_v2_checks();
        assert!(set.all_passed(), "F216 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
