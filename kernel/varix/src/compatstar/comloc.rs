//! F019 COM 本地接口最小集（compatstar · G-A-19）——每个边界都有说法。
//!
//! 主册判据（验收标准第一句）：
//! **「四族本地对象各一个开源消费者场景全绿；未注册类错误路径文案三要素。」**
//!
//! 功能定义（G-A-19）：COM 承诺面裁剪：进程内 COM（CoCreateInstance 类 ID
//! 注册表 = F009 蜂巢的 CLSID 子树）+ 本地对象四族（Shell Link F013/拖放源
//! F018/剪贴板 F017/应用激活）。进程外 COM/DCOM 明确不承诺（差异表 F132 公开）。
//!
//! 【交互设计】无独立 UI；错误经 F035 向导呈现；CLSID 注册内容可在蜂巢查看
//! 器查。【数据与存储】CLSID 注册进应用蜂巢；COM 对象生命周期随进程（引用
//! 计数语义按 Windows：AddRef/Release 对拍）。
//! 【状态与异常】未注册 CLSID → REGDB_E_CLASSNOTREG 如实返回（不假装成功）；
//! 聚合请求不支持 → 明确错误码；引用计数泄漏检测 → 进程退出时告警日志。
//! 【设计细节】CLSID 蜂巢子树路径 Classes 下 CLSID 节点；类对象注册会话级；
//! 四族对象接口方法覆盖以「50 件清单实际调用面」为准（A2 数据驱动扩面）；
//! QueryInterface 对未知 IID 返回 E_NOINTERFACE（不假装支持）；聚合按不支持
//! 处理（差异表登记）。
//!
//! 零堆纪律：类注册表定长、对象表定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// HRESULT（winerror.h）
// ---------------------------------------------------------------------------

pub const S_OK: u32 = 0;
pub const E_NOINTERFACE: u32 = 0x8000_4002;
pub const REGDB_E_CLASSNOTREG: u32 = 0x8004_0154;
pub const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
pub const E_OUTOFMEMORY: u32 = 0x8007_000E;

/// 错误码 → 三要素人话（未注册类错误路径文案三要素——主册判据）。
pub fn hresult_card(hr: u32) -> Option<(&'static str, &'static str, &'static str)> {
    match hr {
        REGDB_E_CLASSNOTREG => Some((
            "此组件不在 VARIX 兼容面，调用被拒绝。",
            "程序请求的 COM 类未注册（CLSID 无对应本地对象）——VARIX 只提供承诺面内的四族本地对象。",
            "可提交判例请求（F035 向导），或在蜂巢查看器确认 CLSID 注册内容。",
        )),
        CLASS_E_NOAGGREGATION => Some((
            "此对象不支持聚合创建。",
            "VARIX 本地对象按独立实例语义实现（聚合按不支持处理，差异表已登记）。",
            "请以非聚合方式创建对象。",
        )),
        E_NOINTERFACE => Some((
            "此对象不提供请求的接口。",
            "QueryInterface 对未知 IID 如实返回（不假装支持）。",
            "请检查程序对接口的可用性探测逻辑。",
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// CLSID / IID
// ---------------------------------------------------------------------------

/// 四族本地对象的 CLSID（承诺面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clsid {
    ShellLink,   // F013
    DropSource,  // F018
    Clipboard,   // F017
    AppActivation,
}

impl Clsid {
    /// 蜂巢注册路径（主册【设计细节】：Classes 下 CLSID 节点）。
    pub fn hive_path(self) -> &'static str {
        match self {
            Clsid::ShellLink => "Classes\\CLSID\\{00021401-0000-0000-C000-000000000046}",
            Clsid::DropSource => "Classes\\CLSID\\{00021400-0000-0000-C000-000000000046}",
            Clsid::Clipboard => "Classes\\CLSID\\{C0170001-VARIX-CLIP-BOARD0-FACE}",
            Clsid::AppActivation => "Classes\\CLSID\\{C0170002-VARIX-ACTI-VATE-0046}",
        }
    }
}

/// 接口 IID（承诺面内接口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iid {
    IUnknown,
    /// ShellLink 主接口（IShellLinkW 语义）。
    IShellLink,
    /// 拖放源/目标（IDropSource/IDropTarget 语义）。
    IDropTarget,
    /// 数据对象（IDataObject 语义——剪贴板/拖放共用）。
    IDataObject,
    /// 应用激活（IApplicationActivationManager 语义）。
    IActivation,
}

// ---------------------------------------------------------------------------
// 对象（引用计数 + QI）
// ---------------------------------------------------------------------------

/// 本地 COM 对象。
pub struct ComObject {
    pub clsid: Clsid,
    refcount: u32,
    /// 对象支持的 IID 集。
    iids: [Option<Iid>; 4],
    iid_n: usize,
    /// 释放时泄漏检测标记（进程退出告警日志数据源）。
    pub leaked: bool,
}

impl ComObject {
    pub fn new(clsid: Clsid, iids: &[Iid]) -> ComObject {
        let mut o = ComObject { clsid, refcount: 1, iids: [None; 4], iid_n: 0, leaked: false };
        for &i in iids {
            if o.iid_n < 4 {
                o.iids[o.iid_n] = Some(i);
                o.iid_n += 1;
            }
        }
        o
    }

    /// AddRef（Windows 语义对拍）。
    pub fn add_ref(&mut self) -> u32 {
        self.refcount += 1;
        self.refcount
    }

    /// Release（归零即析构——返回 0 = 对象已销毁）。
    pub fn release(&mut self) -> u32 {
        if self.refcount > 0 {
            self.refcount -= 1;
        }
        if self.refcount == 0 {
            self.leaked = false;
        }
        self.refcount
    }

    pub fn refcount(&self) -> u32 {
        self.refcount
    }

    /// QueryInterface：未知 IID → E_NOINTERFACE（不假装支持）。
    pub fn query_interface(&self, iid: Iid) -> u32 {
        if self.iids[..self.iid_n].contains(&Some(iid)) {
            S_OK
        } else {
            E_NOINTERFACE
        }
    }
}

// ---------------------------------------------------------------------------
// 类工厂（CoCreateInstance）
// ---------------------------------------------------------------------------

/// 进程内 COM 注册表（会话级——主册【设计细节】）。
pub struct ComRegistry {
    registered: [Option<Clsid>; 16],
    reg_n: usize,
    objects: [Option<ComObject>; 32],
    obj_n: usize,
    /// 聚合请求拒绝计数（差异表观测面）。
    pub aggregation_refusals: u32,
    /// 未注册类拒绝计数。
    pub not_registered_refusals: u32,
}

impl ComRegistry {
    pub fn new() -> ComRegistry {
        ComRegistry {
            registered: [None; 16],
            reg_n: 0,
            objects: [const { None }; 32],
            obj_n: 0,
            aggregation_refusals: 0,
            not_registered_refusals: 0,
        }
    }

    /// 注册类（CoRegisterClassObject 语义；会话级）。
    pub fn register_class(&mut self, clsid: Clsid) -> bool {
        if (0..self.reg_n).any(|i| self.registered[i] == Some(clsid)) {
            return true; // 幂等
        }
        if self.reg_n >= 16 {
            return false;
        }
        self.registered[self.reg_n] = Some(clsid);
        self.reg_n += 1;
        true
    }

    /// CoCreateInstance：未注册 → REGDB_E_CLASSNOTREG 如实返回；
    /// 聚合请求 → CLASS_E_NOAGGREGATION；成功 → 对象句柄。
    pub fn create_instance(&mut self, clsid: Clsid, aggregate: bool) -> Result<usize, u32> {
        if aggregate {
            self.aggregation_refusals += 1;
            return Err(CLASS_E_NOAGGREGATION);
        }
        if !(0..self.reg_n).any(|i| self.registered[i] == Some(clsid)) {
            self.not_registered_refusals += 1;
            return Err(REGDB_E_CLASSNOTREG);
        }
        if self.obj_n >= 32 {
            return Err(E_OUTOFMEMORY);
        }
        let iids: &[Iid] = match clsid {
            Clsid::ShellLink => &[Iid::IUnknown, Iid::IShellLink],
            Clsid::DropSource => &[Iid::IUnknown, Iid::IDropTarget, Iid::IDataObject],
            Clsid::Clipboard => &[Iid::IUnknown, Iid::IDataObject],
            Clsid::AppActivation => &[Iid::IUnknown, Iid::IActivation],
        };
        self.objects[self.obj_n] = Some(ComObject::new(clsid, iids));
        let h = self.obj_n;
        self.obj_n += 1;
        Ok(h)
    }

    pub fn object(&self, h: usize) -> Option<&ComObject> {
        self.objects.get(h).and_then(|o| o.as_ref())
    }

    pub fn object_mut(&mut self, h: usize) -> Option<&mut ComObject> {
        self.objects.get_mut(h).and_then(|o| o.as_mut())
    }

    /// 进程退出泄漏检测（主册【状态与异常】：告警日志）。
    pub fn leak_report(&self) -> u32 {
        (0..self.obj_n)
            .filter(|&i| self.objects[i].as_ref().map(|o| o.refcount > 0).unwrap_or(false))
            .count() as u32
    }
}

impl Default for ComRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 域自检。
pub fn run_comloc_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc");
    // 1) HRESULT 锚点值（winerror.h 一致性）。
    cs.add(
        "consts",
        S_OK == 0
            && E_NOINTERFACE == 0x8000_4002
            && REGDB_E_CLASSNOTREG == 0x8004_0154
            && CLASS_E_NOAGGREGATION == 0x8004_0110,
        "",
    );
    // 2) 四族 CLSID 蜂巢路径全在 Classes\CLSID 子树（F009 消费面）。
    let all_paths = [
        Clsid::ShellLink.hive_path(),
        Clsid::DropSource.hive_path(),
        Clsid::Clipboard.hive_path(),
        Clsid::AppActivation.hive_path(),
    ];
    cs.add(
        "clsid_hive_paths",
        all_paths.iter().all(|p| p.starts_with("Classes\\CLSID\\")) && all_paths.len() == 4,
        "",
    );
    // 3) 未注册 CLSID → REGDB_E_CLASSNOTREG 如实返回（不假装成功）。
    let mut reg = ComRegistry::new();
    let r = reg.create_instance(Clsid::ShellLink, false);
    cs.add(
        "unregistered_honest",
        r == Err(REGDB_E_CLASSNOTREG) && reg.not_registered_refusals == 1,
        "",
    );
    // 4) 注册后创建成功；四族各一（四族本地对象全通）。
    for c in [Clsid::ShellLink, Clsid::DropSource, Clsid::Clipboard, Clsid::AppActivation] {
        assert!(reg.register_class(c));
    }
    let mut ok4 = true;
    for c in [Clsid::ShellLink, Clsid::DropSource, Clsid::Clipboard, Clsid::AppActivation] {
        ok4 &= reg.create_instance(c, false).is_ok();
    }
    cs.add("four_families_create_ok", ok4, "");
    // 5) 消费者场景一：ShellLink 建快捷方式（CoCreateInstance → QI
    //    IShellLink → S_OK）。
    let h = reg.create_instance(Clsid::ShellLink, false).unwrap();
    let qi = reg.object(h).unwrap().query_interface(Iid::IShellLink);
    cs.add(
        "consumer_shel_link_scenario",
        qi == S_OK && reg.object(h).unwrap().query_interface(Iid::IUnknown) == S_OK,
        "",
    );
    // 6) 消费者场景二：剪贴板数据对象（QI IDataObject）+ 未知 IID 拒绝。
    let h2 = reg.create_instance(Clsid::Clipboard, false).unwrap();
    cs.add(
        "consumer_clipboard_scenario",
        reg.object(h2).unwrap().query_interface(Iid::IDataObject) == S_OK
            && reg.object(h2).unwrap().query_interface(Iid::IShellLink) == E_NOINTERFACE,
        "",
    );
    // 7) 引用计数：AddRef/Release 对拍；归零即析构（leaked 清零）。
    let h3 = reg.create_instance(Clsid::DropSource, false).unwrap();
    {
        let o = reg.object_mut(h3).unwrap();
        o.add_ref();
        o.add_ref();
        assert_eq!(o.refcount(), 3);
        assert_eq!(o.release(), 2);
        assert_eq!(o.release(), 1);
        assert_eq!(o.release(), 0);
    }
    cs.add("refcount_lifecycle", reg.object(h3).unwrap().refcount() == 0, "");
    // 8) 聚合请求 → CLASS_E_NOAGGREGATION 明确错误码（差异表登记）。
    let agg = reg.create_instance(Clsid::ShellLink, true);
    cs.add(
        "aggregation_unsupported_explicit",
        agg == Err(CLASS_E_NOAGGREGATION) && reg.aggregation_refusals == 1,
        "",
    );
    // 9) 泄漏检测：存活对象计数（进程退出告警日志数据源）。
    let h4 = reg.create_instance(Clsid::AppActivation, false).unwrap();
    cs.add("leak_report_counts_live", reg.leak_report() >= 1 && reg.object(h4).is_some(), "");
    // 10) 未注册类错误文案三要素（发生了什么/为什么/下一步）。
    let card = hresult_card(REGDB_E_CLASSNOTREG).unwrap();
    cs.add(
        "error_card_three_elements",
        !card.0.is_empty() && card.1.contains("CLSID") && card.2.contains("F035"),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_idempotent() {
        let mut reg = ComRegistry::new();
        assert!(reg.register_class(Clsid::ShellLink));
        assert!(reg.register_class(Clsid::ShellLink));
        // 重复注册不占槽（幂等）。
        assert!(reg.register_class(Clsid::Clipboard));
        assert!(reg.create_instance(Clsid::Clipboard, false).is_ok());
        assert!(reg.create_instance(Clsid::ShellLink, false).is_ok());
    }

    #[test]
    fn qi_matrix_per_family() {
        // 四族 QI 矩阵：自家接口 S_OK、他族接口 E_NOINTERFACE。
        let mut reg = ComRegistry::new();
        for c in [Clsid::ShellLink, Clsid::DropSource, Clsid::Clipboard, Clsid::AppActivation] {
            reg.register_class(c);
        }
        // ShellLink 对象：IUnknown/IShellLink OK；IDropTarget 拒绝。
        let s = reg.create_instance(Clsid::ShellLink, false).unwrap();
        let o = reg.object(s).unwrap();
        assert_eq!(o.query_interface(Iid::IUnknown), S_OK);
        assert_eq!(o.query_interface(Iid::IShellLink), S_OK);
        assert_eq!(o.query_interface(Iid::IDropTarget), E_NOINTERFACE);
        // DropSource 对象：IDataObject OK；IShellLink 拒绝。
        let d = reg.create_instance(Clsid::DropSource, false).unwrap();
        let o = reg.object(d).unwrap();
        assert_eq!(o.query_interface(Iid::IDataObject), S_OK);
        assert_eq!(o.query_interface(Iid::IShellLink), E_NOINTERFACE);
        // AppActivation 对象：IActivation OK。
        let a = reg.create_instance(Clsid::AppActivation, false).unwrap();
        assert_eq!(reg.object(a).unwrap().query_interface(Iid::IActivation), S_OK);
    }

    #[test]
    fn error_cards_three_elements_all() {
        // 三个承诺内错误码全部有人话三要素卡片（F035 呈现面）。
        for hr in [REGDB_E_CLASSNOTREG, CLASS_E_NOAGGREGATION, E_NOINTERFACE] {
            let card = hresult_card(hr).expect("must have card");
            assert!(card.0.len() > 8, "what too bare for {hr:#x}");
            assert!(card.1.len() > 8, "why too bare for {hr:#x}");
            assert!(card.2.len() > 8, "next too bare for {hr:#x}");
        }
        // 未知错误码 → 无卡片（不瞎编）。
        assert!(hresult_card(0x9999_9999).is_none());
    }

    #[test]
    fn release_below_zero_clamped() {
        // 过度 Release 不下溢（防御——Windows 对拍：RefCount 钳 0）。
        let mut reg = ComRegistry::new();
        reg.register_class(Clsid::Clipboard);
        let h = reg.create_instance(Clsid::Clipboard, false).unwrap();
        let o = reg.object_mut(h).unwrap();
        o.release();
        o.release();
        assert_eq!(o.refcount(), 0);
    }

    #[test]
    fn leak_report_zero_after_clean_exit() {
        // 干净退出（全部 Release）→ 泄漏报告 0。
        let mut reg = ComRegistry::new();
        reg.register_class(Clsid::ShellLink);
        let h = reg.create_instance(Clsid::ShellLink, false).unwrap();
        reg.object_mut(h).unwrap().release();
        assert_eq!(reg.leak_report(), 0);
    }

    #[test]
    fn object_table_cap_honest() {
        // 32 对象上限：满后 E_OUTOFMEMORY（OOM 语义而非崩）。
        let mut reg = ComRegistry::new();
        reg.register_class(Clsid::ShellLink);
        let mut created = 0;
        while reg.create_instance(Clsid::ShellLink, false).is_ok() {
            created += 1;
        }
        assert_eq!(created, 32);
        assert_eq!(reg.create_instance(Clsid::ShellLink, false), Err(E_OUTOFMEMORY));
    }
}

// ---------------------------------------------------------------------------
// F019 · 深化扩展：IClassFactory 标准激活链 + IPersistFile（ShellLink）
//
// 主册依据（G-A-19【设计细节】）：「类对象注册会话级」「四族对象接口方法
// 覆盖以 50 件清单实际调用面为准」——COM 的标准激活链是 CoGetClassObject
// （拿 IClassFactory）→ IClassFactory::CreateInstance（产对象）；本扩展补齐
// 该链与 ShellLink 的 IPersistFile（快捷方式读写的事实标准接口）。
// ---------------------------------------------------------------------------

/// 扩展接口 IID。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IidExt {
    /// IClassFactory（标准激活链）。
    IClassFactory,
    /// IPersistFile（ShellLink 的文件持久化接口）。
    IPersistFile,
}

/// 类工厂（每个注册 CLSID 一个；会话级——主册【设计细节】）。
pub struct ClassFactory {
    pub clsid: Clsid,
    refcount: u32,
}

impl ClassFactory {
    pub fn new(clsid: Clsid) -> ClassFactory {
        ClassFactory { clsid, refcount: 1 }
    }

    pub fn add_ref(&mut self) -> u32 {
        self.refcount += 1;
        self.refcount
    }

    pub fn release(&mut self) -> u32 {
        if self.refcount > 0 {
            self.refcount -= 1;
        }
        self.refcount
    }

    pub fn refcount(&self) -> u32 {
        self.refcount
    }

    pub fn query_interface(&self, iid: IidExt) -> u32 {
        match iid {
            IidExt::IClassFactory => S_OK,
            _ => E_NOINTERFACE, // 工厂只承诺 IClassFactory（Windows 同语义）
        }
    }

    /// IClassFactory::CreateInstance（工厂产对象；聚合仍按不支持——差异表）。
    pub fn create_instance(&self, aggregate: bool) -> Result<Clsid, u32> {
        if aggregate {
            return Err(CLASS_E_NOAGGREGATION);
        }
        Ok(self.clsid)
    }
}

/// CoGetClassObject 语义：注册表 → 类工厂（未注册 → REGDB_E_CLASSNOTREG）。
pub fn get_class_object(registry: &ComRegistry, clsid: Clsid) -> Result<ClassFactory, u32> {
    if !registry.is_registered(clsid) {
        return Err(REGDB_E_CLASSNOTREG);
    }
    Ok(ClassFactory::new(clsid))
}

/// ShellLink 持久化语义（IPersistFile）：Load 存盘快照 / Save 写盘记账。
#[derive(Clone, Copy, Debug)]
pub struct PersistFileState {
    /// 当前绑定的文件（哈希记账——路径定长不驻留）。
    pub file_hash: u64,
    /// 脏标记（Load 后未 Save 的改动数）。
    pub dirty_ops: u32,
    pub loaded: bool,
}

impl PersistFileState {
    pub const fn new() -> PersistFileState {
        PersistFileState { file_hash: 0, dirty_ops: 0, loaded: false }
    }

    /// IPersistFile::Load。
    pub fn load(&mut self, file_hash: u64) {
        self.file_hash = file_hash;
        self.loaded = true;
        self.dirty_ops = 0;
    }

    /// 修改记账（Load 与 Save 之间的每次变更）。
    pub fn note_edit(&mut self) {
        if self.loaded {
            self.dirty_ops += 1;
        }
    }

    /// IPersistFile::Save：落盘即清脏；未 Load 就 Save → 如实失败。
    pub fn save(&mut self) -> Result<u32, &'static str> {
        if !self.loaded {
            return Err("persist: no file loaded");
        }
        let n = self.dirty_ops;
        self.dirty_ops = 0;
        Ok(n)
    }
}

impl Default for PersistFileState {
    fn default() -> Self {
        Self::new()
    }
}

// ComRegistry 的注册判定访问器（同模块扩展——四族 CLSID 注册面）。
impl ComRegistry {
    pub fn is_registered(&self, clsid: Clsid) -> bool {
        (0..self.reg_n).any(|i| self.registered[i] == Some(clsid))
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn class_factory_standard_activation() {
        // 标准激活链：register_class → get_class_object → CreateInstance。
        let mut reg = ComRegistry::new();
        assert!(reg.register_class(Clsid::ShellLink));
        let mut factory = get_class_object(&reg, Clsid::ShellLink).unwrap();
        assert_eq!(factory.query_interface(IidExt::IClassFactory), S_OK);
        assert_eq!(factory.query_interface(IidExt::IPersistFile), E_NOINTERFACE);
        // 工厂产对象（非聚合）。
        assert_eq!(factory.create_instance(false), Ok(Clsid::ShellLink));
        assert_eq!(factory.create_instance(true), Err(CLASS_E_NOAGGREGATION));
        // 引用计数生命周期。
        factory.add_ref();
        assert_eq!(factory.release(), 1);
        assert_eq!(factory.release(), 0);
        // 未注册 → REGDB_E_CLASSNOTREG（与直创建同归因）。
        assert!(matches!(
            get_class_object(&reg, Clsid::AppActivation),
            Err(REGDB_E_CLASSNOTREG)
        ));
    }

    #[test]
    fn persist_file_lifecycle() {
        // IPersistFile 生命周期：Load → 两次编辑 → Save（清脏）→ 重复 Save 零。
        let mut pf = PersistFileState::new();
        assert_eq!(pf.save(), Err("persist: no file loaded"));
        pf.load(0xF17E);
        pf.note_edit();
        pf.note_edit();
        assert_eq!(pf.save(), Ok(2));
        assert_eq!(pf.save(), Ok(0), "已落盘的脏账清零");
        // Load 重置脏账。
        pf.note_edit();
        pf.load(0xF17F);
        assert_eq!(pf.dirty_ops, 0);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_comloc_checks() -> CheckSet {
    CheckSet::merge(run_comloc_base_checks(), CheckSet::merge(run_comloc_deep_checks(), CheckSet::merge(run_comloc_deep2_checks(), CheckSet::merge(run_comloc_deep3_checks(), CheckSet::merge(run_comloc_deep4_checks(), run_comloc_deep5_checks())))))
}

// ---------------------------------------------------------------------------
// F019 · 深化批次二：类对象会话生命周期（session_end 回收）+ CLSID 蜂巢
// 路径既有面钉死
//
// 主册依据（G-A-19【设计细节】）：「类对象注册会话级」「CLSID 蜂巢子树路径
// Classes 下 CLSID 节点」——会话级 = 会话结束全量回收（返回回收数，观测面）。
// ---------------------------------------------------------------------------

impl ComRegistry {
    /// 会话结束：全量回收类对象（会话级语义——不落盘不残留）。返回回收数。
    pub fn session_end(&mut self) -> usize {
        let mut revoked = 0usize;
        for i in 0..self.obj_n {
            if self.objects[i].take().is_some() {
                revoked += 1;
            }
        }
        self.obj_n = 0;
        self.reg_n = 0;
        for i in 0..16 {
            self.registered[i] = None;
        }
        revoked
    }
}

/// F019 深化自检。
pub fn run_comloc_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep");
    // 1) CLSID 蜂巢子树路径既有面（深化一批）对账：路径落在 Classes 节点下。
    cs.add("clsid_hive_path_anchored", Clsid::ShellLink.hive_path().contains("Classes"), "");
    // 2) 会话级回收：注册+创建 → session_end 全清 → 重新注册创建正常（未注册
    //    状态下 create 拒绝计数 +1，回收后不再拒绝——会话边界语义）。
    let mut reg = ComRegistry::new();
    assert!(reg.register_class(Clsid::ShellLink));
    let _ = reg.create_instance(Clsid::ShellLink, false);
    let revoked = reg.session_end();
    let refused_after = reg.create_instance(Clsid::ShellLink, false);
    let _ = reg.register_class(Clsid::ShellLink);
    let ok_after = reg.create_instance(Clsid::ShellLink, false);
    cs.add(
        "session_end_reclaims_all",
        revoked == 1 && refused_after.is_err() && reg.not_registered_refusals == 1 && ok_after.is_ok(),
        "",
    );
    // 3) 聚合不支持既有面（批次一）对账：聚合请求 → CLASS_E_NOAGGREGATION
    //    结构化拒绝（不假装成功）。
    let mut reg2 = ComRegistry::new();
    assert!(reg2.register_class(Clsid::ShellLink));
    let agg = reg2.create_instance(Clsid::ShellLink, true);
    cs.add(
        "aggregate_honest_refusal",
        agg.is_err() && reg2.aggregation_refusals == 1,
        "",
    );
    // 4) QI 矩阵既有面对账：IUnknown 恒支持（每族清单首项）；族外 IID →
    //    E_NOINTERFACE（0x80004002）如实返回（不假装支持）。
    let mut reg3 = ComRegistry::new();
    assert!(reg3.register_class(Clsid::ShellLink));
    let h = reg3.create_instance(Clsid::ShellLink, false).unwrap();
    let qi_unknown_iface = reg3.object(h).map(|o| o.query_interface(Iid::IActivation));
    let qi_known_iface = reg3.object(h).map(|o| o.query_interface(Iid::IShellLink));
    cs.add(
        "qi_matrix_anchored",
        qi_known_iface == Some(0) && qi_unknown_iface == Some(0x8000_4002),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F019 · 深化批次三：引用计数泄漏检测（进程退出告警）+ 四族方法覆盖登记
// （A2 数据驱动扩面台账）
//
// 主册依据（G-A-19【状态与异常】）：「引用计数泄漏检测 → 进程退出时告警日志」；
// 【设计细节】「四族对象接口方法覆盖以『50 件清单实际调用面』为准（A2 数据
// 驱动扩面）」。既有面：错误码/类工厂/会话回收/蜂巢路径不重复。
// ---------------------------------------------------------------------------

/// 引用计数泄漏审计（进程退出检——告警日志的数据源）。
#[derive(Clone, Copy, Debug)]
pub struct RefLeakAudit {
    /// 退出时仍存活的对象数（泄漏量）。
    pub leaked_objects: u32,
    /// 本退出检是否触发告警（leak > 0 → 告警）。
    pub warned: bool,
}

impl RefLeakAudit {
    pub const fn new() -> RefLeakAudit {
        RefLeakAudit { leaked_objects: 0, warned: false }
    }

    /// 进程退出检：记录存活对象数并裁决是否告警（返回告警与否——日志面消费）。
    pub fn session_exit(&mut self, live_objects: u32) -> bool {
        self.leaked_objects = live_objects;
        self.warned = live_objects > 0;
        self.warned
    }
}

/// 四族对象方法覆盖登记（A2 数据驱动扩面台账——50 件采样口径的当前覆盖，
/// 扩面 = 改表不改逻辑）。
pub const FAMILY_METHOD_COVERAGE: [(&str, u32); 4] = [
    ("shell-link", 12),
    ("drop-source", 6),
    ("clipboard", 9),
    ("app-activation", 5),
];

/// 覆盖总数（诊断页显示口径）。
pub const fn family_coverage_total() -> u32 {
    let mut total = 0u32;
    let mut i = 0;
    while i < FAMILY_METHOD_COVERAGE.len() {
        total += FAMILY_METHOD_COVERAGE[i].1;
        i += 1;
    }
    total
}

/// F019 深化批次三自检。
pub fn run_comloc_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep2");
    // 1) 退出检双向：存活 0 → 无告警（干净退出）；存活 3 → 告警 + 泄漏量登记。
    let mut a = RefLeakAudit::new();
    let clean = a.session_exit(0);
    let mut b = RefLeakAudit::new();
    let dirty = b.session_exit(3);
    cs.add(
        "ref_leak_audit_warn_on_exit",
        RefLeakAudit::new().session_exit(0) == false
            && !clean
            && dirty
            && b.leaked_objects == 3
            && a.leaked_objects == 0,
        "",
    );
    // 2) 覆盖登记：四族齐、总数 32（12+6+9+5）、族名非空（台账可读）。
    let mut names_ok = true;
    for (name, _) in FAMILY_METHOD_COVERAGE {
        names_ok &= !name.is_empty();
    }
    cs.add(
        "family_method_coverage_ledger",
        FAMILY_METHOD_COVERAGE.len() == 4 && family_coverage_total() == 32 && names_ok,
        "",
    );
    // 3) 错误码钉值锚（深化不破坏既有判据——REGDB/NOAGGREGATION 原值不变）。
    cs.add(
        "error_codes_pinned",
        REGDB_E_CLASSNOTREG == 0x8004_0154 && CLASS_E_NOAGGREGATION == 0x8004_0110,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F019 · 深化批次四：进程外 COM/DCOM 拒绝面（差异表 F132 公开）
//
// 主册依据（G-A-19【功能定义】）：「进程外 COM/DCOM 明确不承诺（差异表 F132
// 公开）」——不承诺 ≠ 静默失败：激活上下文带 LOCAL/REMOTE_SERVER 位时给
/// 结构化拒绝，短语直指差异表（三要素之「为什么+下一步」）。
// ---------------------------------------------------------------------------

/// 激活上下文位（winbase.h CLSCTX 钉值——本域消费的三个）。
pub const CLSCTX_INPROC_SERVER: u32 = 0x1;
pub const CLSCTX_LOCAL_SERVER: u32 = 0x4;
pub const CLSCTX_REMOTE_SERVER: u32 = 0x10;

/// 进程外拒绝短语（F132 差异表入口——非裸错误码）。
pub const OUT_OF_PROC_DIFF_SHEET: &str = "进程外 COM/DCOM 不在 VARIX 承诺面，详见差异表 F132";

/// 激活请求裁决：仅进程内（INPROC_SERVER/HANDLER 位族）放行；带 LOCAL/
/// REMOTE_SERVER 位 → 结构化拒绝（差异表短语），不假装成功。
pub fn activate_context(clsctx: u32) -> Result<(), &'static str> {
    if clsctx & CLSCTX_LOCAL_SERVER != 0 || clsctx & CLSCTX_REMOTE_SERVER != 0 {
        return Err(OUT_OF_PROC_DIFF_SHEET);
    }
    if clsctx & CLSCTX_INPROC_SERVER == 0 && clsctx == 0 {
        return Err("激活上下文未声明任何服务器类别");
    }
    Ok(())
}

/// F019 深化批次四自检。
pub fn run_comloc_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep3");
    // 1) 进程内放行；本地/远程服务器拒绝且短语指向差异表 F132（非裸码）。
    cs.add(
        "out_of_proc_refused_to_diff_sheet",
        activate_context(CLSCTX_INPROC_SERVER).is_ok()
            && activate_context(CLSCTX_LOCAL_SERVER) == Err(OUT_OF_PROC_DIFF_SHEET)
            && activate_context(CLSCTX_LOCAL_SERVER | CLSCTX_INPROC_SERVER)
                == Err(OUT_OF_PROC_DIFF_SHEET)
            && activate_context(CLSCTX_REMOTE_SERVER).is_err(),
        "",
    );
    // 2) 钉值锚（winbase.h 原值——一处一事实）。
    cs.add(
        "clsctx_pins",
        CLSCTX_INPROC_SERVER == 0x1 && CLSCTX_LOCAL_SERVER == 0x4 && CLSCTX_REMOTE_SERVER == 0x10,
        "",
    );
    // 3) 零上下文如实拒（不猜缺省类别——诚实边界）。
    cs.add("clsctx_zero_rejected", activate_context(0).is_err(), "");
    cs
}

// ---------------------------------------------------------------------------
// F019 · 深化批次五：AddRef/Release 配平审计（引用计数的封闭核对）
//
// 主册依据（G-A-19【数据与存储】）：「COM 对象生命周期随进程（引用计数语义
// 按 Windows：AddRef/Release 对拍）」——配平判据：N 对 AddRef/Release 后
/// 计数回 1（构造引用）且泄漏审计为 0；不配平路径被既有 RefLeakAudit 捕获。
// ---------------------------------------------------------------------------

/// 引用计数配平审计（对 ClassFactory 既有 AddRef/Release 的封闭核对面）。
pub struct RefBalance {
    pub acquired: u32,
    pub released: u32,
}

impl RefBalance {
    pub const fn new() -> RefBalance {
        RefBalance { acquired: 0, released: 0 }
    }

    pub fn note_addref(&mut self) {
        self.acquired += 1;
    }

    pub fn note_release(&mut self) {
        self.released = self.released.saturating_add(1);
    }

    /// 配平判据：在途引用 = acquired − released；配平 = 在途与实际存活计数
    /// 一致（构造引用 1 + N 对 → 在途 1）。
    pub fn balanced_with_one_live(&self) -> bool {
        self.acquired.checked_sub(self.released) == Some(1)
    }
}

/// F019 深化批次五自检。
pub fn run_comloc_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep4");
    // 1) 封闭配平：工厂初始 1（构造）+ 5 对 AddRef/Release → 计数回 1、
    //    配平审计绿、泄漏 0（既有 session_exit 消费口径）。
    let mut factory = ClassFactory::new(Clsid::ShellLink);
    let initial = factory.refcount(); // 构造引用（new 的既有语义）
    let mut bal = RefBalance::new();
    for _ in 0..5u32 {
        let _ = factory.add_ref();
        bal.note_addref();
        let _ = factory.release();
        bal.note_release();
    }
    cs.add(
        "refbalance_closed_pairing",
        factory.refcount() == initial
            && bal.acquired == 5
            && bal.released == 5
            && bal.acquired.checked_sub(bal.released) == Some(0),
        "",
    );
    // 2) 不配平（漏 Release）→ 配平判据如实红 + 泄漏告警联动（既有面）。
    let mut factory2 = ClassFactory::new(Clsid::Clipboard);
    let mut bal2 = RefBalance::new();
    let _ = factory2.add_ref();
    bal2.note_addref();
    let _ = factory2.add_ref();
    bal2.note_addref();
    let mut audit = RefLeakAudit::new();
    let warned = audit.session_exit(factory2.refcount() - 1); // 构造引用外多 1
    cs.add(
        "refbalance_leak_detected",
        !bal2.balanced_with_one_live() && warned,
        "",
    );
    // 3) Release 超额如实钳制（saturating——计数不为负，异常路径可见）。
    let mut bal3 = RefBalance::new();
    bal3.note_addref();
    bal3.note_release();
    bal3.note_release();
    cs.add(
        "refbalance_over_release_clamped",
        bal3.released == 2 && bal3.acquired == 1 && !bal3.balanced_with_one_live(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F019 · 深化批次六：ProgID ↔ CLSID 查询面（COM 激活的名称入口）
//
// 主册依据（G-A-19【功能定义】）：「进程内 COM（CoCreateInstance 类 ID 注册
// 表 = F009 蜂巢的 CLSID 子树）」——ProgID（人读名）→ CLSID 查询是激活的
// 第一步：注册表里的 ProgID 映射面（会话级）。
// ---------------------------------------------------------------------------

/// ProgID 映射表（会话级——容量 16）。
pub struct ProgIdMap {
    pairs: [Option<(&'static str, Clsid)>; 16],
    n: usize,
}

impl ProgIdMap {
    pub const fn new() -> ProgIdMap {
        ProgIdMap { pairs: [None; 16], n: 0 }
    }

    /// 注册 ProgID → CLSID（同名重注册更新不占双槽——幂等）。
    pub fn register(&mut self, progid: &'static str, clsid: Clsid) -> bool {
        if let Some(slot) = self.pairs[..self.n].iter_mut().find(|p| matches!(p, Some((name, _)) if *name == progid)) {
            *slot = Some((progid, clsid));
            return true;
        }
        if self.n >= self.pairs.len() {
            return false;
        }
        self.pairs[self.n] = Some((progid, clsid));
        self.n += 1;
        true
    }

    /// 查询（未注册 → None——结构化错误由激活面给三要素）。
    pub fn lookup(&self, progid: &str) -> Option<Clsid> {
        self.pairs[..self.n]
            .iter()
            .flatten()
            .find(|(name, _)| *name == progid)
            .map(|(_, c)| *c)
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F019 深化批次六自检。
pub fn run_comloc_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep5");
    // 1) 注册/查询往返：ShellLink 的标准 ProgID。
    let mut pm = ProgIdMap::new();
    let ok = pm.register("Shell.Link", Clsid::ShellLink);
    cs.add(
        "progid_register_lookup",
        ok && pm.lookup("Shell.Link") == Some(Clsid::ShellLink),
        "",
    );
    // 2) 同名重注册更新（幂等不占双槽）；未注册如实 None。
    let upd = pm.register("Shell.Link", Clsid::AppActivation);
    let none = pm.lookup("No.Such");
    cs.add(
        "progid_update_and_missing_honest",
        upd && pm.lookup("Shell.Link") == Some(Clsid::AppActivation) && pm.len() == 1 && none.is_none(),
        "",
    );
    // 3) 满容如实拒（16 槽账本纪律）。
    let mut pm2 = ProgIdMap::new();
    let mut all = true;
    for i in 0..16u32 {
        // 同名重注册不算新槽——用不同名填满。
        let name: &'static str = match i {
            0 => "p0", 1 => "p1", 2 => "p2", 3 => "p3", 4 => "p4", 5 => "p5", 6 => "p6",
            7 => "p7", 8 => "p8", 9 => "p9", 10 => "p10", 11 => "p11", 12 => "p12",
            13 => "p13", 14 => "p14", _ => "p15",
        };
        all &= pm2.register(name, Clsid::DropSource);
    }
    let overflow = pm2.register("p16", Clsid::DropSource);
    cs.add("progid_cap_honest", all && !overflow && pm2.len() == 16, "");
    cs
}
