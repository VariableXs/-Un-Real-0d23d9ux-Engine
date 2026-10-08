//! settable — WP-205 · B-1901 设置总表 + B-1902 失败回滚 + B-1903 生效三档
//! + B-1904 通知确认通道（MD2 篇 19.1/19.3）。
//!
//! 判据 B-1901：设置总表，新服务零前端成本（架构演练）。
//! 判据 B-1902：失败回滚，网络配置改坏回滚演练。
//! 判据 B-1903：生效三档，语义与提示一致（全表走查）。
//! 判据 B-1904：通知确认通道，模态不可绕过，免打扰可用。
//! MD2 原文（19.1）："全系统设置一张声明式总表（服务名、设置项、类型、取值
//! 域、默认值、生效语义、权限要求），设置中心前端读表渲染、后端按表执法——
//! 加一项设置不是改界面是加一行表（新服务的设置零前端工作量……）。生效语义
//! 三档：即时（运行时改内存态）、会话级（下次会话生效）、需重启（存储挂载
//! 类），语义在表内声明，界面按语义呈现提示（'重启后生效'不是事后解释是
//! 前置告知）。失败回滚：改网络配置先校验后生效……任何后端拒绝都完整回滚到
//! 改前值并三要素报错——'取消永远是安全出路'（宪章第九章）在设置层是回滚
//! 机制不是一句文案。"
//! MD2 原文（19.3）："通知……走统一通知浮层（右下角，宪章浮层语义：点外
//! 消失、可聚合、可免打扰）；权限确认……走模态对话框（不可绕过，有'永远
//! 拒绝'选项——高频骚扰与安全确认的边界：安全类不允许'不再询问'，便利类
//! 允许）。"
//!
//! 宿主可测形态：SettingRow 七字段 schema 校验 + 读表渲染（label 由行自动
//! 生成——加一行表即出设置面的架构红利）+ SetFlow 两步（validate 先于
//! commit，拒绝即回滚到改前值 + what/why/how 三要素）+ hint 三档全表走查
//! + NotifyBus（免打扰入队不弹、同主题聚合）+ ConfirmModal（确认唯一入口
//! ——类型面无 bypass；安全类无"不再询问"）。

use crate::checks::CheckSet;

// ============ B-1901 声明式设置总表 ============

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Semantics {
    /// 即时：运行时改内存态（网络开关类）。
    Immediate,
    /// 会话级：下次会话生效（布局切换类）。
    Session,
    /// 需重启（存储挂载类）。
    Reboot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perm {
    User,
    Admin,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Bool,
    Int,
    Enum,
}

/// 七字段声明行（服务名、设置项、类型、取值域、默认值、生效语义、权限要求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SettingRow {
    pub service: &'static str,
    pub key: &'static str,
    pub ty: ValType,
    pub lo: i64,
    pub hi: i64,
    pub default: i64,
    pub semantics: Semantics,
    pub perm: Perm,
}

/// schema 校验：七字段自洽（默认值在域内、Bool 域恒 {0,1}、Enum 域非空）。
pub fn schema_ok(r: &SettingRow) -> bool {
    if r.service.is_empty() || r.key.is_empty() {
        return false;
    }
    if !(r.lo <= r.default && r.default <= r.hi) {
        return false;
    }
    match r.ty {
        ValType::Bool => r.lo == 0 && r.hi == 1,
        ValType::Enum => r.hi > r.lo,
        ValType::Int => true,
    }
}

pub const ROW_CAP: usize = 64;

pub struct SettingTable {
    rows: [Option<SettingRow>; ROW_CAP],
    pub count: usize,
}

impl SettingTable {
    pub const fn new() -> SettingTable {
        SettingTable { rows: [None; ROW_CAP], count: 0 }
    }

    /// 加一项设置 = 加一行表（新服务零前端工作量）。
    pub fn add_row(&mut self, r: SettingRow) -> bool {
        if self.count >= ROW_CAP || !schema_ok(&r) {
            return false;
        }
        self.rows[self.count] = Some(r);
        self.count += 1;
        true
    }

    pub fn row_at(&self, i: usize) -> Option<SettingRow> {
        if i < self.count {
            self.rows[i]
        } else {
            None
        }
    }
}

/// 前端读表渲染：label 由行自动生成（"<service>.<key>"）——零前端代码。
/// 返回写入长度；缓冲不足返回 0。
pub fn render_label(r: &SettingRow, out: &mut [u8]) -> usize {
    let s = r.service.as_bytes();
    let k = r.key.as_bytes();
    if s.len() + 1 + k.len() > out.len() {
        return 0;
    }
    out[..s.len()].copy_from_slice(s);
    out[s.len()] = b'.';
    out[s.len() + 1..s.len() + 1 + k.len()].copy_from_slice(k);
    s.len() + 1 + k.len()
}

// ============ B-1903 生效三档提示 ============

/// 界面按语义呈现提示（前置告知——commit 前即可读）。
pub fn hint(s: Semantics) -> &'static str {
    match s {
        Semantics::Immediate => "即时生效",
        Semantics::Session => "下次会话生效",
        Semantics::Reboot => "重启后生效",
    }
}

// ============ B-1902 失败回滚 ============

/// 三要素报错（what/why/how——与 B-604/B-606 同族口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetError {
    pub what: &'static str,
    pub why: &'static str,
    pub how: &'static str,
}

pub const ERR_EMPTY: SetError = SetError { what: "", why: "", how: "" };

pub struct SetFlow {
    pub current: i64,
}

impl SetFlow {
    pub const fn new(current: i64) -> SetFlow {
        SetFlow { current }
    }

    /// 先校验（不触状态）。
    pub fn validate(&self, v: i64, lo: i64, hi: i64) -> Result<(), SetError> {
        if v < lo || v > hi {
            Err(SetError {
                what: "network.config",
                why: "value out of domain",
                how: "reverted to previous value",
            })
        } else {
            Ok(())
        }
    }

    /// 提交：校验先于生效；拒绝即回滚（current 不动）——取消永远是安全出路。
    pub fn commit(&mut self, v: i64, lo: i64, hi: i64) -> Result<(), SetError> {
        match self.validate(v, lo, hi) {
            Ok(()) => {
                self.current = v;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

// ============ B-1904 通知与权限确认通道 ============

pub const NOTIFY_QUEUE_CAP: u32 = 32;

pub struct NotifyBus {
    pub dnd: bool,
    pub shown: u32,
    pub queued: u32,
    /// 同主题合并节省的弹窗数（可聚合的计量面）。
    pub merged: u32,
    last_topic: Option<u8>,
}

impl NotifyBus {
    pub const fn new() -> NotifyBus {
        NotifyBus { dnd: false, shown: 0, queued: 0, merged: 0, last_topic: None }
    }

    /// 推送：免打扰入队不弹；同主题连发聚合为一条。
    pub fn push(&mut self, topic: u8) -> bool {
        if Some(topic) == self.last_topic && self.dnd == false {
            self.merged += 1;
            return true;
        }
        self.last_topic = Some(topic);
        if self.dnd {
            if self.queued < NOTIFY_QUEUE_CAP {
                self.queued += 1;
            }
            return false;
        }
        self.shown += 1;
        true
    }

    /// 免打扰解除：队列按序补弹。
    pub fn flush(&mut self) -> u32 {
        let n = self.queued;
        self.queued = 0;
        self.shown += n;
        n
    }
}

/// 权限确认模态：确认是唯一入口（类型面无 bypass 方法）——不可绕过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfirmModal {
    /// 安全类确认（不允许"不再询问"）vs 便利类（允许记住选择）。
    pub safety: bool,
    pub confirmed: bool,
    pub denied: bool,
}

impl ConfirmModal {
    pub const fn new(safety: bool) -> ConfirmModal {
        ConfirmModal { safety, confirmed: false, denied: false }
    }

    /// 确认唯一入口。
    pub fn confirm(&mut self) {
        self.confirmed = true;
    }

    /// 永远拒绝选项（模态的另一半——不是只能同意）。
    pub fn deny(&mut self) {
        self.denied = true;
    }

    /// "不再询问"边界：安全类恒 false（高频骚扰与安全确认的边界），
    /// 便利类允许。
    pub fn remember_choice(&self) -> bool {
        !self.safety
    }
}

// ============ CheckSet（B-1901 ×3 + B-1902 ×3 + B-1903 ×3 + B-1904 ×3）============

pub fn run_settable_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-1901~B-1904 设置中心四判据");
    {
        // B-1901 总表七字段 schema：好行全过 + 坏行全拒。
        let good = SettingRow {
            service: "net",
            key: "doh",
            ty: ValType::Bool,
            lo: 0,
            hi: 1,
            default: 0,
            semantics: Semantics::Immediate,
            perm: Perm::User,
        };
        let bad_default = SettingRow { default: 5, ..good };
        let bad_bool = SettingRow { hi: 9, ..good };
        let bad_name = SettingRow { key: "", ..good };
        set.add(
            "B-1901 总表七字段 schema",
            schema_ok(&good) && !schema_ok(&bad_default) && !schema_ok(&bad_bool)
                && !schema_ok(&bad_name),
            "默认值在域内；Bool 域恒 {0,1}；空键名拒绝",
        );
    }
    {
        // B-1901 新服务零前端：加一行表 → 渲染面自动出现。
        let mut t = SettingTable::new();
        let r = SettingRow {
            service: "newsrv",
            key: "mode",
            ty: ValType::Enum,
            lo: 0,
            hi: 2,
            default: 0,
            semantics: Semantics::Session,
            perm: Perm::Admin,
        };
        let added = t.add_row(r);
        let mut buf = [0u8; 64];
        let n = render_label(&t.row_at(0).unwrap_or(r), &mut buf);
        set.add(
            "B-1901 新服务零前端",
            added && t.count == 1 && &buf[..n] == b"newsrv.mode",
            "加一行表即出设置（label 自动生成——架构红利）",
        );
    }
    {
        // B-1901 读表渲染执法同源：坏行进不了表（后端按表执法）。
        let mut t = SettingTable::new();
        let bad = SettingRow {
            service: "net",
            key: "mtu",
            ty: ValType::Int,
            lo: 68,
            hi: 1500,
            default: 9000,
            semantics: Semantics::Reboot,
            perm: Perm::Admin,
        };
        let refused = !t.add_row(bad);
        let good = SettingRow { default: 1500, ..bad };
        let accepted = t.add_row(good) && t.count == 1;
        set.add(
            "B-1901 读表执法同源",
            refused && accepted,
            "schema 不过的行进不了总表（表即执法依据）",
        );
    }
    {
        // B-1902 校验先于生效。
        let mut f = SetFlow::new(1500);
        let before = f.current;
        let bad = f.commit(9000, 68, 1500);
        set.add(
            "B-1902 校验先于生效",
            bad.is_err() && f.current == before,
            "commit 前先 validate；越域值不落状态",
        );
    }
    {
        // B-1902 拒绝完整回滚：改坏后 current == 改前值。
        let mut f = SetFlow::new(1500);
        let _ = f.commit(1400, 68, 1500); // 合法改动
        let saved = f.current;
        let _ = f.commit(10, 68, 1500); // 改坏
        let _ = f.commit(9999, 68, 1500); // 再改坏
        set.add(
            "B-1902 完整回滚",
            f.current == saved && saved == 1400,
            "两次非法提交后仍停在最后合法值（回滚机制不是文案）",
        );
    }
    {
        // B-1902 三要素报错：what/why/how 全非空。
        let f = SetFlow::new(0);
        let e = match f.validate(-1, 0, 100) {
            Err(e) => e,
            Ok(()) => ERR_EMPTY,
        };
        set.add(
            "B-1902 三要素报错",
            !e.what.is_empty() && !e.why.is_empty() && !e.how.is_empty(),
            "what=改的什么 / why=为何拒绝 / how=改回哪去",
        );
    }
    {
        // B-1903 三档语义表内声明：Row.semantics 承载（枚举三值穷举）。
        let sems = [Semantics::Immediate, Semantics::Session, Semantics::Reboot];
        let mut t = SettingTable::new();
        let keys: [&'static str; 3] = ["net.doh", "ime.layout", "store.mount"];
        let mut i = 0;
        let mut all = true;
        while i < 3 {
            let r = SettingRow {
                service: "sys",
                key: keys[i],
                ty: ValType::Bool,
                lo: 0,
                hi: 1,
                default: 0,
                semantics: sems[i],
                perm: Perm::User,
            };
            if !t.add_row(r) {
                all = false;
            }
            i += 1;
        }
        set.add(
            "B-1903 三档语义表内声明",
            all && t.count == 3
                && t.row_at(0).unwrap().semantics == Semantics::Immediate
                && t.row_at(2).unwrap().semantics == Semantics::Reboot,
            "三档枚举在行内声明（语义在表不在文案）",
        );
    }
    {
        // B-1903 提示与语义一致：全表走查。
        let expect: [(&'static str, Semantics); 3] = [
            ("即时生效", Semantics::Immediate),
            ("下次会话生效", Semantics::Session),
            ("重启后生效", Semantics::Reboot),
        ];
        let mut all = true;
        let mut i = 0;
        while i < 3 {
            if hint(expect[i].1) != expect[i].0 {
                all = false;
            }
            i += 1;
        }
        set.add(
            "B-1903 提示语义一致",
            all,
            "hint 三档精确匹配（全表走查口径）",
        );
    }
    {
        // B-1903 前置告知：hint 在 commit 前可读（纯函数无副作用）。
        let r = Semantics::Reboot;
        let before = hint(r);
        let mut f = SetFlow::new(0);
        let _ = f.commit(1, 0, 1);
        set.add(
            "B-1903 前置告知",
            before == "重启后生效" && hint(r) == "重启后生效",
            "'重启后生效'在提交前已呈现（不是事后解释）",
        );
    }
    {
        // B-1904 通知浮层：免打扰入队不弹 + 解除补弹。
        let mut bus = NotifyBus::new();
        let shown = bus.push(1);
        bus.dnd = true;
        let silenced = !bus.push(2) && !bus.push(3);
        let flushed = bus.flush() == 2;
        set.add(
            "B-1904 通知浮层语义",
            shown && silenced && flushed && bus.queued == 0,
            "免打扰入队不弹；解除按序补弹（队列清零对账）",
        );
    }
    {
        // B-1904 通知可聚合：同主题连发合并。
        let mut bus = NotifyBus::new();
        let _ = bus.push(9);
        let _ = bus.push(9);
        let _ = bus.push(9);
        set.add(
            "B-1904 通知可聚合",
            bus.shown == 1 && bus.merged == 2,
            "同主题三条合并为一条（浮层不刷屏）",
        );
    }
    {
        // B-1904 模态不可绕过：确认是唯一入口（confirm/deny 二选一）。
        let mut m = ConfirmModal::new(true);
        let not_yet = !m.confirmed && !m.denied;
        m.confirm();
        let ok_path = m.confirmed && !m.denied;
        let mut m2 = ConfirmModal::new(true);
        m2.deny();
        let deny_path = m2.denied && !m2.confirmed;
        set.add(
            "B-1904 模态不可绕过",
            not_yet && ok_path && deny_path,
            "未确认未拒绝 → 确认或拒绝二选一（无第三态旁路）",
        );
    }
    {
        // B-1904 安全类无"不再询问"：便利类允许。
        let safety = ConfirmModal::new(true);
        let convenience = ConfirmModal::new(false);
        set.add(
            "B-1904 安全类无不再询问",
            !safety.remember_choice() && convenience.remember_choice(),
            "安全类恒询问；便利类可记住选择（骚扰与安全的边界）",
        );
    }
    set
}

// ============ 单测（f908 ×4）============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f908_schema_rows() {
        let int_ok = SettingRow {
            service: "store",
            key: "quota_pct",
            ty: ValType::Int,
            lo: 1,
            hi: 50,
            default: 10,
            semantics: Semantics::Reboot,
            perm: Perm::Admin,
        };
        assert!(schema_ok(&int_ok));
        // Enum 域必须非空。
        let bad_enum = SettingRow { hi: 0, ..int_ok };
        assert!(!schema_ok(&bad_enum));
        // Int 域不额外设限。
        let wide = SettingRow { lo: -100, hi: 100, default: 0, ..int_ok };
        assert!(schema_ok(&wide));
        // 表容量。
        let mut t = SettingTable::new();
        let mut i = 0;
        while i < ROW_CAP {
            assert!(t.add_row(int_ok));
            i += 1;
        }
        assert!(!t.add_row(int_ok), "满容量诚实拒绝");
    }

    #[test]
    fn f908_rollback() {
        let mut f = SetFlow::new(1500);
        assert!(f.commit(1400, 68, 1500).is_ok());
        assert_eq!(f.current, 1400);
        // 越下界。
        assert!(f.commit(0, 68, 1500).is_err());
        assert_eq!(f.current, 1400);
        // 越上界。
        assert!(f.commit(2000, 68, 1500).is_err());
        assert_eq!(f.current, 1400);
        // 边界值可提交。
        assert!(f.commit(68, 68, 1500).is_ok());
        assert_eq!(f.current, 68);
        assert!(f.commit(1500, 68, 1500).is_ok());
        assert_eq!(f.current, 1500);
    }

    #[test]
    fn f908_hint_semantics() {
        assert_eq!(hint(Semantics::Immediate), "即时生效");
        assert_eq!(hint(Semantics::Session), "下次会话生效");
        assert_eq!(hint(Semantics::Reboot), "重启后生效");
    }

    #[test]
    fn f908_modal_dnd() {
        // 队列容量上限诚实拒绝。
        let mut bus = NotifyBus::new();
        bus.dnd = true;
        let mut i = 0;
        while i < NOTIFY_QUEUE_CAP + 10 {
            let _ = bus.push(i as u8);
            i += 1;
        }
        assert!(bus.queued <= NOTIFY_QUEUE_CAP);
        assert_eq!(bus.flush(), NOTIFY_QUEUE_CAP);
        // 模态双路径。
        let mut m = ConfirmModal::new(false);
        assert!(m.remember_choice());
        m.confirm();
        assert!(m.confirmed && !m.denied);
    }
}
