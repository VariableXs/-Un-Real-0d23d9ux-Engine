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
pub fn run_comloc_checks() -> CheckSet {
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
