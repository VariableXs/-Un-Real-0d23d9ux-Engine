//! F244 注册表本体：条目与身份、序列键承载结构、哈希身份、注册表核心
//! （表内核 / 注册 / 冲突即检 / 改键 / 禁用 / 恢复 / 查询 / 审计）与
//! 系统预置表。
//!
//! 序列键运行面、批量查询、持久化导出导入等扩展方法见
//! [`super::registry::ops`]（同类型的后续 `impl` 块）。

use super::decl::ParsedDecl;
use super::keys::{
    Combo, VK_DOWN, VK_ESCAPE, VK_F1, VK_LWIN, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SNAPSHOT, VK_TAB,
    VK_UP, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN,
};
use crate::star::sbase::RingLog;

/// 扩展方法子模块（chord 运行面 / 批量查询 / 持久化导出导入——
/// [`HotkeyReg`] 的后续 `impl` 块；后代模块可见表内核私有面）。
mod ops;

// ---------------------------------------------------------------------------
// 条目与身份
// ---------------------------------------------------------------------------

/// 第三方应用身份（vxapp，16 字节定长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppId {
    len: u8,
    bytes: [u8; 16],
}

impl AppId {
    pub const fn none() -> AppId {
        AppId { len: 0, bytes: [0u8; 16] }
    }

    pub fn from_bytes(src: &[u8]) -> AppId {
        let mut out = AppId::none();
        let n = src.len().min(16);
        out.bytes[..n].copy_from_slice(&src[..n]);
        out.len = n as u8;
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 说明文案缓存（设置中心「可查」面，48 字节定长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescBuf {
    len: u8,
    bytes: [u8; 48],
}

impl DescBuf {
    pub fn from_bytes(src: &[u8]) -> DescBuf {
        let mut out = DescBuf { len: 0, bytes: [0u8; 48] };
        let n = src.len().min(48);
        out.bytes[..n].copy_from_slice(&src[..n]);
        out.len = n as u8;
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// 条目来源：系统预置 / 第三方声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    System,
    App(AppId),
}

impl Source {
    pub fn is_system(&self) -> bool {
        matches!(self, Source::System)
    }

    pub fn app(&self) -> Option<AppId> {
        match self {
            Source::System => None,
            Source::App(a) => Some(*a),
        }
    }
}

/// 注册表条目。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// 系统条目语义名（如 "sys.copy"）；第三方为空串。
    pub label: &'static str,
    /// 第三方声明序号（同应用多条声明去重 uid 用；系统条目恒 0）。
    pub seq: u8,
    pub desc: DescBuf,
    /// 当前键位（改键后 ≠ def）。
    pub combo: Combo,
    /// 出厂键位（恢复默认锚）。
    pub def: Combo,
    /// 可禁用位。
    pub enabled: bool,
    /// 用户改过键（持久化导出标记）。
    pub overridden: bool,
    pub source: Source,
}

impl Entry {
    /// 稳定身份：系统 = 语义名 FNV-1a；第三方 = 应用 ID FNV-1a 循环移位
    /// 异或声明序号（同应用多条声明互不冲突）。
    pub fn uid(&self) -> u32 {
        if self.source.is_system() {
            fnv1a(self.label.as_bytes())
        } else {
            // 第三方分支：is_system 已排除 System 态，app_id() 恒命中——
            // 显性 match 展开代替 unwrap_or 兜底（态空间不留暗门）。
            fnv1a(self.app_id().as_bytes()).rotate_left(8) ^ (self.seq as u32)
        }
    }

    pub fn is_system(&self) -> bool {
        self.source.is_system()
    }

    /// 第三方应用 ID（系统条目返回空 ID——match 展开两条态，无兜底）。
    pub fn app_id(&self) -> AppId {
        match self.source {
            Source::System => AppId::none(),
            Source::App(a) => a,
        }
    }
}

/// 冲突占用方身份（判据「明确列出占用方」的承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occupant {
    /// 系统条目语义名（第三方占用时为空串）。
    pub label: &'static str,
    /// 第三方应用 ID（系统占用时为空 ID）。
    pub app: AppId,
}

impl Occupant {
    pub fn is_system(&self) -> bool {
        !self.label.is_empty()
    }
}

/// 注册结论（无静默：四种结局都可观测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegOutcome {
    Registered,
    /// 表满显性拒绝。
    Full,
    /// 键位非法（无键/纯修饰键）。
    Malformed,
    /// 同 uid 已在册（幂等注册面由调用方处理）。
    Duplicate,
    /// 冲突显性拒绝，携带占用方身份（先到先得）。
    Occupied(Occupant),
}

/// 改键结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebindOutcome {
    Done,
    NoSuchId,
    Malformed,
    Occupied(Occupant),
}

/// 变更种类（审计留痕面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Rebind,
    Enable,
    Disable,
    ResetOne,
    ResetAll,
    Register,
    Unregister,
}

/// 一条变更留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub ts: u64,
    pub kind: ChangeKind,
    pub uid: u32,
    pub old: Combo,
    pub new: Combo,
}

/// 注册表统计（设置中心概览面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegStats {
    pub total: usize,
    pub system: usize,
    pub app: usize,
    pub disabled: usize,
    pub overridden: usize,
}

// ---------------------------------------------------------------------------
// 序列键（chord）与派发留痕
// ---------------------------------------------------------------------------

/// 序列键表容量（两段式键的独立承载面，单键表之外）。
pub const CHORD_CAP: usize = 16;

/// 序列键第二段等待窗（ms）——第一段命中后仅在此窗内认第二段。
pub const CHORD_TIMEOUT_MS: u64 = 800;

/// 派发留痕环容量（运行面命中记录）。
pub const DISPATCH_CAP: usize = 32;

/// 序列键语义文档（判据「两段式序列键」的设置页说明锚）。
pub const CHORD_DOC: &str = "序列键：第一段按下后不立即触发，进入 800ms 等待窗；窗内按下第二段才执行。第一段键位不与任何单键冲突——冲突在注册时显性拒绝。";

/// 键位占用提示文档（设置页实时占位提示的文案锚）。
pub const CONFLICT_HINT_DOC: &str = "此键位已被占用：注册与改键一律过冲突门，先到先得；占用方身份（系统语义名或第三方应用）会明确列出。";

/// 一条序列键登记（两段均为合法单键位；段间冲突注册时拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordRec {
    /// 稳定语义名（如 "app.emacs.kill-region"）。
    pub label: &'static str,
    /// 第一段（命中即进入等待窗，不触发动作）。
    pub first: Combo,
    /// 第二段（等待窗内命中才触发）。
    pub second: Combo,
    /// 登记时刻（ms，注入式）。
    pub ts: u64,
    /// 段位被改过（差异面导出标记——与单键条目的 overridden 同纪律）。
    pub overridden: bool,
}

/// 序列键单步判定结论（运行面逐键调用 [`HotkeyReg::chord_key`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordStep {
    /// 非序列键键位——调用方继续走普通单键派发。
    Idle,
    /// 第一段命中：进入等待窗，本击不派发任何动作。
    Armed,
    /// 第二段命中（等待窗内）：序列触发，返回语义名。
    Fired { label: &'static str },
}

/// 一次派发留痕（审计面：谁在何时被哪个键位命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchRec {
    pub ts: u64,
    pub uid: u32,
    pub combo: Combo,
    /// 命中条目当时是否启用（禁用条目留痕但不触发）。
    pub active: bool,
}

/// 预置条目分组（设置中心分类树的数据面；下标入 [`CATEGORY_NAMES`]）。
pub const CATEGORY_NAMES: [&str; 4] = ["edit", "win", "sys", "desktop"];

/// 46 条预置的分组归属（与 [`PRESETS`] 一一对应——按主册功能域划分）。
pub const PRESET_CAT: [u8; 46] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // copy..print：编辑域
    1, 1, 1, 2, 1, 1, 1, 2, 2, 3, // close-tab..show-desktop
    2, 2, 3, 1, 1, 1, 1, 3, 3, 3, // lock..min-all
    3, 2, 2, 2, 2, 2, 2, 0, 0, 2, // clip-hist..find-next
    2, 1, 2, 1, 2, 1, // menubar..cycle-pane
];

// ---------------------------------------------------------------------------
// 哈希与身份
// ---------------------------------------------------------------------------

/// FNV-1a 32 位（uid 稳定身份；与持久化导出的 uid 域一致——v2 容器
/// 校验和同用此实现，消重归并原 vx2_fnv 副本）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 系统条目 uid 推导（调用方免持 Entry 也能定位）。
pub fn uid_of_label(label: &str) -> u32 {
    fnv1a(label.as_bytes())
}

/// 序列键 uid（语义名 FNV-1a——差异面持久化的稳定身份锚）。
pub fn chord_uid(c: &ChordRec) -> u32 {
    fnv1a(c.label.as_bytes())
}

// ---------------------------------------------------------------------------
// 注册表本体
// ---------------------------------------------------------------------------

/// 注册表容量——46 条预置 + 第三方余量（判据「全系统集中一处」）。
pub const HK_CAP: usize = 96;

/// 变更留痕环容量。
pub const CHANGE_CAP: usize = 64;

/// 冲突即检哈希：32 位码 × 黄金比例乘法散列 → 槽位。
fn slot_of(code: u32) -> usize {
    (code.wrapping_mul(0x9E37_79B1) % HK_CAP as u32) as usize
}

/// 全局快捷键注册表。
pub struct HotkeyReg {
    slots: [Option<Entry>; HK_CAP],
    pub count: usize,
    pub version: u32,
    changes: RingLog<Change, CHANGE_CAP>,
    /// 序列键表（与单键表独立；段位冲突注册时显性拒绝）。
    chords: [Option<ChordRec>; CHORD_CAP],
    /// 序列键等待窗状态：(表下标, 武装时刻)。None = 无武装。
    chord_armed: Option<(usize, u64)>,
    /// 派发留痕环（运行面命中记录）。
    dlog: RingLog<DispatchRec, DISPATCH_CAP>,
    /// 槽位级派发计数（热度画像；uid → find_uid_idx → hits[idx]）。
    hits: [u32; HK_CAP],
    /// 本会话安装的预置条目数（完整性审计面）。
    pub preset_installed: u32,
}

impl HotkeyReg {
    pub fn new() -> HotkeyReg {
        HotkeyReg {
            slots: [const { None }; HK_CAP],
            count: 0,
            version: 0,
            changes: RingLog::new(),
            chords: [const { None }; CHORD_CAP],
            chord_armed: None,
            dlog: RingLog::new(),
            hits: [0; HK_CAP],
            preset_installed: 0,
        }
    }

    // -- 表内核 ------------------------------------------------------------

    /// 线性探测：键位码 → 槽下标。删除走后退移位（无墓碑），探测链
    /// 不断裂，`None` 即可判终止。
    pub(crate) fn probe(&self, code: u32) -> Option<usize> {
        let start = slot_of(code);
        for k in 0..HK_CAP {
            let idx = (start + k) % HK_CAP;
            match self.slots[idx] {
                None => return None,
                Some(e) => {
                    if e.combo.code() == code {
                        return Some(idx);
                    }
                }
            }
        }
        None
    }

    /// 后退移位删除（Knuth TAOCP 6.4 Algorithm R，单调 j 版本）：
    /// 先腾空 `start`（保证环上存在空槽 → 必然终止），随后 j 单调前进，
    /// 「家址不在 (i, j] 环形区间」的条目前移填洞——线性探测链保持完整，
    /// 删除后全部条目仍可按探测规则命中（单测有专项验证）。
    /// 终止双保险：遇空槽即终；j 环绕一整圈回到洞位 i 也终（表曾被
    /// 100% 填满时首搬移会填掉唯一的空槽，靠回绕终止兜底——v2 批修复
    /// 的死循环缺陷，修复前 fuzz 满表删除会永久空转）。
    fn remove_at(&mut self, start: usize) {
        self.slots[start] = None;
        let mut i = start;
        let mut j = start;
        loop {
            j = (j + 1) % HK_CAP;
            if j == i {
                // 整表扫过一圈回到洞位：洞即终点（满表删除路径）。
                self.slots[i] = None;
                return;
            }
            // let-else 展平（原 match 嵌套 4 层 → 2 层）：空槽即终止——
            // 清掉最后一个洞（物理残留来自最近一次搬移）。
            let Some(e) = self.slots[j] else {
                self.slots[i] = None;
                return;
            };
            let h = slot_of(e.combo.code());
            let stays = if i < j {
                h > i && h <= j
            } else {
                h > i || h <= j
            };
            if !stays {
                self.slots[i] = Some(e);
                i = j; // 洞前移（table[j] 物理残留由后续搬移覆盖或终局清洞）
            }
        }
    }

    pub(crate) fn find_uid_idx(&self, uid: u32) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(e) if e.uid() == uid))
    }

    /// 槽位占用方读取（probe/find_uid_idx 命中后槽位必在册——表内核
    /// 不变量）。None 为理论不可达的防御分支：显性返回空占用方并留痕，
    /// 不 panic（裸 expect 消灭岗位：可恢复错误路径，回退分支可观测）。
    pub(crate) fn occupant_at(&self, idx: usize) -> Occupant {
        match self.slots[idx] {
            Some(e) => Occupant { label: e.label, app: e.app_id() },
            None => Occupant { label: "", app: AppId::none() },
        }
    }

    /// 从键位家址起探测第一个空槽落位（线性探测插入语义——v2 批修复：
    /// 修复前误用「物理顺序第一个空槽」，家址探测链会被无关空槽截断，
    /// 注册成功却派发不到）。注册/改键/差异装载三条路径共用——提取
    /// 公共函数消除重复。返回是否落位（表满 false，调用方显性上报）。
    pub(crate) fn place_at_home(&mut self, e: Entry) -> bool {
        let start = slot_of(e.combo.code());
        for k in 0..HK_CAP {
            let idx = (start + k) % HK_CAP;
            if self.slots[idx].is_none() {
                self.slots[idx] = Some(e);
                return true;
            }
        }
        false
    }

    pub(crate) fn push_change(&mut self, c: Change) {
        self.changes.push(c);
    }

    /// 变更留痕（新→旧，最多 64 条）。
    pub fn change_log(&self) -> [Option<Change>; CHANGE_CAP] {
        let mut out = [None; CHANGE_CAP];
        for (k, ev) in self.changes.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }

    /// 摘一插一回滚兜底（rebind / 差异装载共享——消重归并：两处同构
    /// 的「理论不可达」回滚留痕段）：重插失败时显性回滚旧键位与标志位
    /// 并走正规注册门留痕，不静默。`was_enabled` 为改写前的启用位原值。
    pub(crate) fn reinsert_entry(&mut self, mut e: Entry, old: Combo, was_overridden: bool, was_enabled: bool, ts: u64) {
        e.combo = old;
        e.overridden = was_overridden;
        e.enabled = was_enabled;
        let _ = self.insert_entry(e, ts);
    }

    // -- 注册 / 冲突即检 ----------------------------------------------------

    /// 注册一条系统条目（冲突即检：占用即显性拒绝并列占用方）。
    pub fn register_system(&mut self, label: &'static str, combo: Combo, desc: &'static str, ts: u64) -> RegOutcome {
        let uid = uid_of_label(label);
        if self.find_uid_idx(uid).is_some() {
            return RegOutcome::Duplicate;
        }
        let e = Entry {
            label,
            seq: 0,
            desc: DescBuf::from_bytes(desc.as_bytes()),
            combo,
            def: combo,
            enabled: true,
            overridden: false,
            source: Source::System,
        };
        self.insert_entry(e, ts)
    }

    /// 注册一条第三方声明（与系统同门：同一冲突即检、同样可见）。
    pub fn register_app_decl(&mut self, app: AppId, decl: &ParsedDecl, seq: u8, ts: u64) -> RegOutcome {
        let e = Entry {
            label: "",
            seq,
            desc: decl.desc,
            combo: Combo::new(decl.mods, decl.key),
            def: Combo::new(decl.mods, decl.key),
            enabled: true,
            overridden: false,
            source: Source::App(app),
        };
        self.insert_entry(e, ts)
    }

    pub(crate) fn insert_entry(&mut self, e: Entry, ts: u64) -> RegOutcome {
        if !e.combo.sane() {
            return RegOutcome::Malformed;
        }
        if let Some(i) = self.probe(e.combo.code()) {
            // probe 命中 ⇒ 槽位必在册；占用方读取走公共面（防御分支见
            // occupant_at——不 panic、不静默）。
            return RegOutcome::Occupied(self.occupant_at(i));
        }
        // 从家址起探测第一个空槽落位（线性探测插入语义——公共面
        // place_at_home，注释锚见其文档）。
        if self.place_at_home(e) {
            self.count += 1;
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::Register, uid: e.uid(), old: Combo::none(), new: e.combo });
            return RegOutcome::Registered;
        }
        RegOutcome::Full
    }

    /// 注销（第三方应用卸载时清理声明面）。
    pub fn unregister(&mut self, uid: u32, ts: u64) -> bool {
        let Some(i) = self.find_uid_idx(uid) else {
            return false;
        };
        // find_uid_idx 命中 ⇒ 槽位必在册；None 防御分支按「无此条目」
        // 显性回退（可恢复错误路径，不 panic）。
        let Some(e) = self.slots[i] else {
            return false;
        };
        self.remove_at(i);
        self.count -= 1;
        self.version += 1;
        self.push_change(Change { ts, kind: ChangeKind::Unregister, uid, old: e.combo, new: Combo::none() });
        true
    }

    // -- 改键 / 禁用 / 恢复 --------------------------------------------------

    /// 用户改键（判据「可改」）：过冲突门 → 更新键位 → 置改键标记。
    pub fn rebind(&mut self, uid: u32, new_combo: Combo, ts: u64) -> RebindOutcome {
        // 卫语句链：无此 uid / 键位非法 / 冲突占用——逐条提前返回。
        let Some(idx) = self.find_uid_idx(uid) else {
            return RebindOutcome::NoSuchId;
        };
        if !new_combo.sane() {
            return RebindOutcome::Malformed;
        }
        if let Some(j) = self.probe(new_combo.code()) {
            if j != idx {
                // probe 命中 ⇒ 槽位必在册（占用方读取走公共面）。
                return RebindOutcome::Occupied(self.occupant_at(j));
            }
        }
        // uid 命中 ⇒ 槽位必在册；None 防御分支按「无此条目」显性回退。
        let Some(mut e) = self.slots[idx] else {
            return RebindOutcome::NoSuchId;
        };
        let old = e.combo;
        if old != new_combo {
            // 改键 = 从旧键位探测链摘除 + 按新键位家址重插（v2 批修复：
            // 修复前原地改写槽位，新键位的家址探测链够不着该条目，
            // 改键后派发丢失；重插面提取为 place_at_home 公共函数）。
            // 摘除后表内必有空槽 → 重插必成功；万一不成立的兜底是显性
            // 回滚并留痕，不静默（回滚段归并 reinsert_entry 公共面）。
            let was_overridden = e.overridden;
            self.remove_at(idx);
            e.combo = new_combo;
            e.overridden = true;
            if !self.place_at_home(e) {
                self.reinsert_entry(e, old, was_overridden, e.enabled, ts);
            }
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::Rebind, uid, old, new: new_combo });
        }
        RebindOutcome::Done
    }

    /// 禁用/启用（判据「可禁用」）：禁用后活跃派发不命中、查询仍可见。
    pub fn set_enabled(&mut self, uid: u32, on: bool, ts: u64) -> bool {
        let Some(idx) = self.find_uid_idx(uid) else {
            return false;
        };
        // uid 命中 ⇒ 槽位必在册；None 防御分支显性回退（不 panic）。
        let Some(mut e) = self.slots[idx] else {
            return false;
        };
        if e.enabled == on {
            return true; // 幂等
        }
        let old = e.combo;
        e.enabled = on;
        self.slots[idx] = Some(e);
        self.version += 1;
        let kind = if on { ChangeKind::Enable } else { ChangeKind::Disable };
        self.push_change(Change { ts, kind, uid, old, new: e.combo });
        true
    }

    /// 单条恢复默认。
    pub fn reset_one(&mut self, uid: u32, ts: u64) -> bool {
        let Some(idx) = self.find_uid_idx(uid) else {
            return false;
        };
        // uid 命中 ⇒ 槽位必在册；None 防御分支显性回退（不 panic）。
        let Some(mut e) = self.slots[idx] else {
            return false;
        };
        if !e.overridden && e.enabled {
            return true;
        }
        let old = e.combo;
        e.combo = e.def;
        e.enabled = true;
        e.overridden = false;
        self.slots[idx] = Some(e);
        self.version += 1;
        self.push_change(Change { ts, kind: ChangeKind::ResetOne, uid, old, new: e.def });
        true
    }

    /// 恢复默认一键（判据「注册表带恢复默认一键」）：全表回出厂键位、
    /// 全部启用、清改键标记；返回实际恢复条数。
    pub fn reset_all(&mut self, ts: u64) -> usize {
        let mut n = 0;
        for idx in 0..HK_CAP {
            let Some(e) = self.slots[idx] else { continue };
            if e.overridden || !e.enabled || e.combo != e.def {
                let old = e.combo;
                let mut e2 = e;
                e2.combo = e2.def;
                e2.enabled = true;
                e2.overridden = false;
                self.slots[idx] = Some(e2);
                self.push_change(Change { ts, kind: ChangeKind::ResetOne, uid: e.uid(), old, new: e2.def });
                n += 1;
            }
        }
        if n > 0 {
            self.version += 1;
            self.push_change(Change { ts, kind: ChangeKind::ResetAll, uid: 0, old: Combo::none(), new: Combo::none() });
        }
        n
    }

    // -- 查询 / 审计 ----------------------------------------------------------

    /// 系统条目数 / 第三方条目数等（设置中心概览）。
    pub fn stats(&self) -> RegStats {
        let mut s = RegStats { total: 0, system: 0, app: 0, disabled: 0, overridden: 0 };
        for e in self.slots.iter().flatten() {
            s.total += 1;
            if e.is_system() {
                s.system += 1;
            } else {
                s.app += 1;
            }
            if !e.enabled {
                s.disabled += 1;
            }
            if e.overridden {
                s.overridden += 1;
            }
        }
        s
    }

    /// 去重列出已接入的第三方应用。
    pub fn apps(&self, out: &mut [AppId]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if let Source::App(a) = e.source {
                if k < out.len() && !out[..k].contains(&a) {
                    out[k] = a;
                    k += 1;
                }
            }
        }
        k
    }

    /// 预置完整性审计（判据本体）：46 条全量在册、键位=出厂、全启用、
    /// 无改键残留——「系统快捷键全量入表」的机器判定。
    pub fn audit_presets(&self) -> bool {
        if self.count < PRESETS.len() {
            return false;
        }
        PRESETS.iter().all(|(label, combo, _)| {
            self.entry_by_uid(uid_of_label(label))
                .map_or(false, |e| e.is_system() && e.combo == *combo && e.enabled && !e.overridden)
        })
    }
}

impl Default for HotkeyReg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 系统预置表（46 条全量——判据「系统快捷键全量入表」的数据面）
// ---------------------------------------------------------------------------

/// 系统级默认键位表：语义名 / 键位 / 说明。数值为主册判据与 F220/F249/
/// F081/F235 锚点的并集，每条可通过设置中心改键/禁用。
pub const PRESETS: [(&str, Combo, &str); 46] = [
    ("sys.copy", Combo::new(MOD_CTRL, 0x43), "复制选中内容"),
    ("sys.paste", Combo::new(MOD_CTRL, 0x56), "粘贴"),
    ("sys.cut", Combo::new(MOD_CTRL, 0x58), "剪切"),
    ("sys.undo", Combo::new(MOD_CTRL, 0x5A), "撤销"),
    ("sys.redo", Combo::new(MOD_CTRL, 0x59), "重做"),
    ("sys.find", Combo::new(MOD_CTRL, 0x46), "查找"),
    ("sys.paste-plain", Combo::new(MOD_CTRL | MOD_SHIFT, 0x56), "纯文本粘贴（F220）"),
    ("sys.select-all", Combo::new(MOD_CTRL, 0x41), "全选"),
    ("sys.save", Combo::new(MOD_CTRL, 0x53), "保存"),
    ("sys.print", Combo::new(MOD_CTRL, 0x50), "打印"),
    ("sys.close-tab", Combo::new(MOD_CTRL, 0x57), "关闭标签页"),
    ("sys.new-tab", Combo::new(MOD_CTRL, 0x54), "新建标签页"),
    ("sys.new-window", Combo::new(MOD_CTRL, 0x4E), "新建窗口"),
    ("sys.taskmgr", Combo::new(MOD_CTRL | MOD_SHIFT, VK_ESCAPE), "任务管理器"),
    ("sys.alt-tab", Combo::new(MOD_ALT, VK_TAB), "窗口切换"),
    ("sys.close-win", Combo::new(MOD_ALT, VK_F1 + 3), "关闭窗口"),
    ("sys.props", Combo::new(MOD_ALT, VK_RETURN), "属性"),
    ("sys.start-menu", Combo::new(0, VK_LWIN), "呼出开始菜单"),
    ("sys.explorer", Combo::new(MOD_WIN, 0x45), "打开资源管理器"),
    ("sys.show-desktop", Combo::new(MOD_WIN, 0x44), "显示桌面"),
    ("sys.lock", Combo::new(MOD_WIN, 0x4C), "锁定屏幕"),
    ("sys.run", Combo::new(MOD_WIN, 0x52), "运行"),
    ("sys.task-view", Combo::new(MOD_WIN, VK_TAB), "任务视图（F081）"),
    ("sys.snap-left", Combo::new(MOD_WIN, VK_LEFT), "窗口贴靠左半屏"),
    ("sys.snap-right", Combo::new(MOD_WIN, VK_RIGHT), "窗口贴靠右半屏"),
    ("sys.snap-max", Combo::new(MOD_WIN, VK_UP), "窗口最大化"),
    ("sys.snap-min", Combo::new(MOD_WIN, VK_DOWN), "窗口最小化"),
    ("sys.desk-prev", Combo::new(MOD_WIN | MOD_CTRL, VK_LEFT), "虚拟桌面左移（F235）"),
    ("sys.desk-next", Combo::new(MOD_WIN | MOD_CTRL, VK_RIGHT), "虚拟桌面右移（F235）"),
    ("sys.desk-new", Combo::new(MOD_WIN | MOD_CTRL, 0x44), "新建虚拟桌面"),
    ("sys.min-all", Combo::new(MOD_WIN, 0x4D), "最小化全部窗口"),
    ("sys.clip-hist", Combo::new(MOD_WIN, 0x56), "剪贴板历史"),
    ("sys.project", Combo::new(MOD_WIN, 0x50), "投影切换"),
    ("sys.settings", Combo::new(MOD_WIN, 0x49), "打开设置中心"),
    ("sys.quick-settings", Combo::new(MOD_WIN, 0x41), "快速设置（F249 右下热角同源）"),
    ("sys.notif-center", Combo::new(MOD_WIN, 0x4E), "通知中心（F249 右上热角同源）"),
    ("sys.help", Combo::new(0, VK_F1), "帮助"),
    ("sys.rename", Combo::new(0, VK_F1 + 1), "重命名"),
    ("sys.find-next", Combo::new(0, VK_F1 + 2), "查找下一个"),
    ("sys.refresh", Combo::new(0, VK_F1 + 4), "刷新"),
    ("sys.menubar", Combo::new(0, VK_F1 + 9), "菜单栏"),
    ("sys.fullscreen", Combo::new(0, VK_F1 + 10), "全屏切换"),
    ("sys.devtools", Combo::new(0, VK_F1 + 11), "开发者工具"),
    ("sys.address", Combo::new(MOD_ALT, 0x44), "聚焦地址栏"),
    ("sys.screenshot", Combo::new(0, VK_SNAPSHOT), "截屏"),
    ("sys.cycle-pane", Combo::new(0, VK_F1 + 5), "窗格循环聚焦"),
];

impl HotkeyReg {
    /// 安装全部系统预置（已存在的 uid 跳过——幂等重装）；返回新装条数。
    pub fn preset_all(&mut self, ts: u64) -> usize {
        let mut n = 0;
        for (label, combo, desc) in PRESETS.iter() {
            // *label/*desc：内层本就是 'static——不能靠 &&str 自动解引用
            // 强转 &'static str（外层借用的是 const 具象化临时值）。
            if self.register_system(*label, *combo, *desc, ts) == RegOutcome::Registered {
                n += 1;
            }
        }
        self.preset_installed += n as u32;
        n
    }
}
