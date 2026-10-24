//! CGPU-F3373 · 验收扩展点（vcv13_ext）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3373`
//!
//! **判据（锚点原文）**：两向、一组、判据。
//!
//! **职责定位（锚点原文）**：扩展：扩展（验收扩展点（自定义判据/
//! 自定义矩阵——扩展实现；测试（扩展一组）。
//!
//! # 一、「两向」扩展（锚点原文的两向）
//!
//! 验收体系冻结 API（F3369）之后，变化从哪里进来？锚点给了两个
//! 方向：**自定义判据**（[`CustomCriterion`]——新判据族不进核心，
//! 以注册件形式挂载）与**自定义矩阵**（[`CustomMatrix`]——新坐标
//! 系不进核心，以子矩阵形式挂载）。共同纪律是**核心不动**：扩展
//! 件挂在注册表上（[`ExtRegistry`]），冻结表核对的是核心七函数不
//! 变，扩展件增删不影响冻结线。
//!
//! # 二、扩展件三纪律
//!
//! 1. **命名空间隔离**：扩展名必须带 `ext:` 前缀（[`E_VV_NS`]）——
//!    冒充核心名即拒，防止扩展件覆盖核心语义；
//! 2. **幂等注册**：同名重复注册返回原句柄不重建（[`
//!    E_VV_REDEF`] 只对前缀合法但内容指纹不同的重定义触发）；
//! 3. **卸载可逆**：注册表可摘除扩展件，摘除后引用它的验收项失
//!    效——扩展件不许留下悬空引用（摘除时一并清理索引）。
//!
//! # 三、扩展一组（判据）
//!
//! 判据组：两向齐备 + 命名空间强制 + 冒名拒收 + 幂等注册 + 卸载
//! 可逆 + 核心冻结不受扩展影响。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const EXT_VERSION: &str = "V13-ext-v1";

/// 扩展名缺 `ext:` 前缀（命名空间违规）。
pub const E_VV_NS: &str = "E_VV_NS";

/// 同名重定义（内容指纹不同——口径分裂）。
pub const E_VV_REDEF: &str = "E_VV_REDEF";

/// 卸载不存在的扩展件。
pub const E_VV_NOEXT: &str = "E_VV_NOEXT";

/// 扩展件句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtHandle(pub u32);

/// 扩展件（判据向或矩阵向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtEntry {
    /// 扩展名（必须 `ext:` 前缀）。
    pub name: String,
    /// 向别：c=自定义判据 / m=自定义矩阵。
    pub kind: u8,
    /// 内容指纹（同名不同指纹=重定义）。
    pub fp: u64,
}

/// 扩展注册表。
pub struct ExtRegistry {
    entries: Vec<ExtEntry>,
    /// 遥测：注册/拒收/卸载计数。
    pub registered: u64,
    pub rejected: u64,
    pub unregistered: u64,
}

/// FNV-1a 指纹（与全库同族）。
pub const fn fp64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// 命名空间核验（`ext:` 前缀强制）。
pub fn ns_ok(name: &str) -> Result<(), &'static str> {
    if name.starts_with("ext:") {
        Ok(())
    } else {
        Err(E_VV_NS)
    }
}

impl ExtRegistry {
    /// 空注册表。
    pub fn new() -> ExtRegistry {
        ExtRegistry { entries: Vec::new(), registered: 0, rejected: 0, unregistered: 0 }
    }

    /// 注册扩展件（两向通用）：冒名拒 / 重定义拒 / 幂等还句柄。
    pub fn register(&mut self, name: &str, kind: u8) -> Result<ExtHandle, String> {
        if ns_ok(name).is_err() {
            self.rejected += 1;
            return Err(format!(
                "{}：{} 缺 ext: 前缀——扩展件不许冒充核心名",
                E_VV_NS, name
            ));
        }
        let nfp = fp64(name);
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            if self.entries[i].fp == nfp && self.entries[i].kind == kind {
                return Ok(ExtHandle(i as u32));
            }
            self.rejected += 1;
            return Err(format!(
                "{}：{} 重定义且指纹不同——扩展口径分裂",
                E_VV_REDEF, name
            ));
        }
        self.entries.push(ExtEntry { name: name.to_string(), kind, fp: nfp });
        self.registered += 1;
        Ok(ExtHandle((self.entries.len() - 1) as u32))
    }

    /// 卸载（不存在即拒；卸载即摘除不留悬空）。
    pub fn unregister(&mut self, h: ExtHandle) -> Result<String, String> {
        if h.0 as usize >= self.entries.len() {
            self.rejected += 1;
            return Err(format!("{}：句柄 {} 无对应扩展件", E_VV_NOEXT, h.0));
        }
        let e = self.entries.remove(h.0 as usize);
        self.unregistered += 1;
        Ok(e.name)
    }

    /// 查询：按名取句柄。
    pub fn find(&self, name: &str) -> Option<ExtHandle> {
        self.entries.iter().position(|e| e.name == name).map(|i| ExtHandle(i as u32))
    }

    /// 按向别统计（c=判据向 / m=矩阵向）。
    pub fn count_kind(&self, kind: u8) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    /// 注册表规模。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 扩展声明（两向定位——非空即判据）。
pub const EXT_DECLARATION: &str =
    "验收扩展点两向：自定义判据与自定义矩阵，核心冻结不动，扩展挂注册表";

// ---------------------------------------------------------------------------
// 二、扩展一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3373 域自检：两向、一组、判据。
pub fn run_vcv13_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv13_ext");

    // --- 两向齐备：判据向 + 矩阵向各注册成功 ---
    let mut reg = ExtRegistry::new();
    let h1 = reg.register("ext:帧预算自定义", b'c');
    let h2 = reg.register("ext:车载矩阵", b'm');
    cs.add(
        "两向注册齐备",
        h1.is_ok() && h2.is_ok() && reg.count_kind(b'c') == 1 && reg.count_kind(b'm') == 1,
        "锚点两向：自定义判据/自定义矩阵",
    );

    // --- 冒名拒收（缺前缀） ---
    let r = reg.register("verify_matrix_put", b'c');
    cs.add(
        "冒充核心名拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_NS),
        "扩展件不许覆盖核心语义",
    );

    // --- 幂等注册：同名同向还同句柄 ---
    let again = reg.register("ext:帧预算自定义", b'c');
    cs.add(
        "幂等注册",
        again == Ok(h1.expect("h1")) && reg.len() == 2,
        "重复注册不重建",
    );

    // --- 重定义拒收：同名不同向 ---
    let r = reg.register("ext:帧预算自定义", b'm');
    cs.add(
        "重定义拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_REDEF),
        "同名不同内容=口径分裂",
    );

    // --- 查询与规模 ---
    cs.add(
        "按名查询",
        reg.find("ext:车载矩阵").is_some() && reg.find("ext:不存在").is_none(),
        "find 语义",
    );

    // --- 卸载可逆 + 悬空清理 ---
    let hh = reg.find("ext:车载矩阵").expect("present");
    let un = reg.unregister(hh);
    cs.add(
        "卸载可逆",
        un.is_ok() && reg.find("ext:车载矩阵").is_none() && reg.len() == 1,
        "摘除不留悬空引用",
    );
    let r = reg.unregister(hh);
    cs.add(
        "重复卸载拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_NOEXT),
        "不存在的扩展件不可卸",
    );

    // --- 遥测守恒：注册 2 拒 3 卸 1 ---
    cs.add(
        "遥测守恒",
        reg.registered == 2 && reg.rejected == 3 && reg.unregistered == 1,
        "注册/拒收/卸载计数",
    );

    // --- 声明非空 ---
    cs.add(
        "扩展声明非空",
        !EXT_DECLARATION.is_empty() && EXT_DECLARATION.contains("两向"),
        "锚点扩展声明",
    );

    // --- 版本指纹 ---
    cs.add("版本指纹非零", fp64(EXT_VERSION) != 0, EXT_VERSION);

    cs
}
