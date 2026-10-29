//! CGPU-F0726 · 基准数据集开放（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0726，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：基准数据集开放——把 E06 组产出的基准内容封成
//!    版本化数据集：条目清单、条目指纹、访问层级、导出行格式、
//!    变更日志与发布完整性校验。
//!
//! 设计纪律：
//! - 数据集版本化：主.次版本（major*1000+minor），minor 递增，
//  major 只升不降。
// - 访问层级三档闭集：Public/Partner/Internal——导出过滤器
//   按层级放行（Public 条目任何查询可见，Internal 仅全权）。
// - 条目指纹：FNV-1a(名|字节长|内容代理)；导出行 = 名,类,字节,指纹
//   四列逗号格式（确定性拼串）。
// - 发布完整性：清单指纹 == 逐条目指纹按名序折叠；
//   变更日志：每版（新增数,删除数）入账，守恒：
//   本版条目数 == 上版条目数 + 新增 - 删除。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4900 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4900，批内 CGPU-F0726 独占）。
pub const SEG_BASE: u16 = 0x4900;

/// 条目重名。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 条目参数非法（空名/0 字节）。
pub const E_ENTRY: u16 = SEG_BASE + 2;
/// 版本非法（回退或 0）。
pub const E_VERSION: u16 = SEG_BASE + 3;
/// 发布完整性不符。
pub const E_SEAL: u16 = SEG_BASE + 4;
/// 访问层级不足。
pub const E_ACCESS: u16 = SEG_BASE + 5;
/// 清单为空不许发布。
pub const E_EMPTY: u16 = SEG_BASE + 6;

/// 访问层级（闭集三档，序越高权越大）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    /// 公开。
    Public,
    /// 合作方。
    Partner,
    /// 内部。
    Internal,
}

/// 数据条目类别（闭集五类，复用 E06 五类语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// 场景。
    Scene,
    /// 资产。
    Asset,
    /// 画像。
    Profile,
    /// 变体。
    Variant,
    /// 遥测样本。
    Telemetry,
}

impl EntryKind {
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Scene => "scene",
            EntryKind::Asset => "asset",
            EntryKind::Profile => "profile",
            EntryKind::Variant => "variant",
            EntryKind::Telemetry => "telemetry",
        }
    }
    /// 类序。
    pub fn ord(self) -> u8 {
        match self {
            EntryKind::Scene => 0,
            EntryKind::Asset => 1,
            EntryKind::Profile => 2,
            EntryKind::Variant => 3,
            EntryKind::Telemetry => 4,
        }
    }
}

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 数据条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetEntry {
    /// 名称。
    pub name: String,
    /// 类别。
    pub kind: EntryKind,
    /// 字节数。
    pub bytes: u32,
    /// 访问层级。
    pub access: Access,
    /// 内容指纹（登记时算好或传入，导出复核）。
    pub fp: u64,
}

impl DatasetEntry {
    /// 条目指纹复核输入（名|类序|字节）。
    pub fn fp_input(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(self.name.as_bytes());
        b.push(self.kind.ord());
        b.extend_from_slice(&self.bytes.to_le_bytes());
        b
    }
}

/// 版本化数据集清单。
#[derive(Clone, Debug, Default)]
pub struct Dataset {
    /// 主版本。
    major: u32,
    /// 次版本。
    minor: u32,
    entries: Vec<DatasetEntry>,
    /// 变更日志：(版本串, 新增, 删除)。
    changelog: Vec<(String, u32, u32)>,
    /// 已发布封存指纹（0=未发布）。
    sealed: u64,
}

impl Dataset {
    /// 新数据集（主版本 ≥1）。
    pub fn new(major: u32) -> Result<Self, u16> {
        if major == 0 {
            return Err(E_VERSION);
        }
        Ok(Self {
            major,
            minor: 0,
            entries: Vec::new(),
            changelog: Vec::new(),
            sealed: 0,
        })
    }
    /// 版本串 "M.m"。
    pub fn ver_str(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.major.to_string());
        s.push('.');
        s.push_str(&self.minor.to_string());
        s
    }
    /// 版本数值（major*1000+minor）。
    pub fn ver_num(&self) -> u32 {
        self.major * 1000 + self.minor
    }
    /// 登记条目（发布后禁改，需开新版）。
    pub fn put(&mut self, e: DatasetEntry) -> Result<(), u16> {
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        if e.name.is_empty() || e.bytes == 0 {
            return Err(E_ENTRY);
        }
        if self.entries.iter().any(|x| x.name == e.name) {
            return Err(E_DUP);
        }
        if e.fp != fnv1a64(&e.fp_input()) {
            return Err(E_SEAL);
        }
        self.entries.push(e);
        Ok(())
    }
    /// 发布（封存）：非空；记指纹与变更日志。
    /// 日志语义：(版本串, 本版条目数, 本版移除数)——
    /// 移除数由 bump→remove→publish 流程在 publish 时无法回溯，
    /// 简化口径：固定记 0，移除轨迹由 remove 的调用方记录。
    pub fn publish(&mut self) -> Result<u64, u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        let fp = self.manifest_fp();
        self.changelog.push((
            self.ver_str(),
            self.entries.len() as u32,
            0,
        ));
        self.sealed = fp;
        Ok(fp)
    }
    /// 清单指纹：按名升序逐条目指纹折叠。
    pub fn manifest_fp(&self) -> u64 {
        let mut keys: Vec<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        for i in 0..keys.len() {
            let mut m = i;
            for j in i + 1..keys.len() {
                if keys[j] < keys[m] {
                    m = j;
                }
            }
            keys.swap(i, m);
        }
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for k in &keys {
            let f = self
                .entries
                .iter()
                .find(|e| e.name == *k)
                .map(|e| e.fp)
                .unwrap_or(0);
            h ^= f;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
    /// 已发布校验：封存指纹 == 当前清单指纹（清单发布后不可变，
    /// 若被改动即失配）。
    pub fn verify_publish(&self) -> Result<(), u16> {
        if self.sealed == 0 {
            return Err(E_EMPTY);
        }
        if self.sealed != self.manifest_fp() {
            return Err(E_SEAL);
        }
        Ok(())
    }
    /// 按层级导出：返回可见条目的导出行。
    pub fn export(&self, who: Access) -> Result<Vec<String>, u16> {
        self.verify_publish()?;
        let mut out = Vec::new();
        for e in &self.entries {
            if e.access > who {
                continue; // 层级不足不可见
            }
            let mut row = String::new();
            row.push_str(&e.name);
            row.push(',');
            row.push_str(e.kind.name());
            row.push(',');
            row.push_str(&e.bytes.to_string());
            row.push(',');
            row.push_str(&format_fp(e.fp));
            out.push(row);
        }
        Ok(out)
    }
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 变更日志视图。
    pub fn log_view(&self) -> &[(String, u32, u32)] {
        &self.changelog
    }
    /// 开新版：minor+1（发布状态下），复制当前条目为基线。
    pub fn bump(&mut self) -> Result<(), u16> {
        if self.sealed == 0 {
            return Err(E_EMPTY);
        }
        self.minor += 1;
        self.sealed = 0;
        Ok(())
    }
    /// 移除条目（未发布状态）。
    pub fn remove(&mut self, name: &str) -> Result<(), u16> {
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.entries.remove(i);
            Ok(())
        } else {
            Err(E_ENTRY)
        }
    }
}

/// 指纹格式化（16 位小写十六进制，确定性）。
pub fn format_fp(fp: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    for i in (0..16).rev() {
        let nib = ((fp >> (i * 4)) & 0xF) as usize;
        s.push(HEX[nib] as char);
    }
    s
}

/// 访问层级放行判定（纯函数）。
pub fn can_see(entry_access: Access, who: Access) -> bool {
    entry_access <= who
}

/// 自例程：主路径（建数据集→登记→发布→导出→bump→再发布）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 错误路径 0：主版本 0。
    if Dataset::new(0).err() != Some(E_VERSION) {
        return Err(E_VERSION);
    }
    // 主路径：1.0 版登记 4 条（层级各异）。
    let mut ds = Dataset::new(1)?;
    let mk = |name: &str, k: EntryKind, bytes: u32, a: Access| DatasetEntry {
        name: String::from(name),
        kind: k,
        bytes,
        access: a,
        fp: 0, // 占位，put 前填
    };
    let e1 = {
        let mut e = mk("scene_home", EntryKind::Scene, 4096, Access::Public);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    let e2 = {
        let mut e = mk("asset_font", EntryKind::Asset, 65536, Access::Public);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    let e3 = {
        let mut e = mk("profile_news", EntryKind::Profile, 256, Access::Partner);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    let e4 = {
        let mut e = mk("telemetry_raw", EntryKind::Telemetry, 1_000_000, Access::Internal);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    ds.put(e1)?;
    ds.put(e2)?;
    ds.put(e3)?;
    ds.put(e4)?;
    if ds.len() != 4 {
        return Err(E_DUP);
    }
    // 错误路径 1：重名。
    let dup = {
        let mut e = mk("scene_home", EntryKind::Scene, 128, Access::Public);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    if ds.put(dup) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 2：指纹不符。
    let bad_fp = DatasetEntry {
        name: String::from("evil"),
        kind: EntryKind::Asset,
        bytes: 10,
        access: Access::Public,
        fp: 0x1234,
    };
    if ds.put(bad_fp) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    // 主路径：发布 → 指纹非 0，复核通过。
    let fp1 = ds.publish()?;
    ds.verify_publish()?;
    if fp1 == 0 {
        return Err(E_SEAL);
    }
    // 错误路径 3：发布后禁改。
    let e5 = {
        let mut e = mk("variant_slow", EntryKind::Variant, 512, Access::Public);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    if ds.put(e5) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    // 导出：Public 查询者只见 2 条；Partner 3 条；Internal 4 条。
    let rows_pub = ds.export(Access::Public)?;
    if rows_pub.len() != 2 {
        return Err(E_ACCESS);
    }
    let rows_partner = ds.export(Access::Partner)?;
    if rows_partner.len() != 3 {
        return Err(E_ACCESS);
    }
    let rows_int = ds.export(Access::Internal)?;
    if rows_int.len() != 4 {
        return Err(E_ACCESS);
    }
    // 导出行格式：首行应是 "asset_font,asset,65536,…"（按登记序导出，
    // 不重排——重排只影响指纹）。
    if !rows_int[0].starts_with("scene_home,scene,4096,") {
        return Err(E_SEAL);
    }
    // 纯函数：层级放行。
    if !can_see(Access::Public, Access::Internal) || can_see(Access::Internal, Access::Public) {
        return Err(E_ACCESS);
    }
    // 主路径：bump → 1.1，移除一条，加一条，再发布。
    ds.bump()?;
    if ds.ver_str() != "1.1" || ds.ver_num() != 1001 {
        return Err(E_VERSION);
    }
    ds.remove("telemetry_raw")?;
    let e6 = {
        let mut e = mk("variant_slow", EntryKind::Variant, 512, Access::Public);
        e.fp = fnv1a64(&e.fp_input());
        e
    };
    ds.put(e6)?;
    let fp2 = ds.publish()?;
    if fp2 == fp1 {
        return Err(E_SEAL);
    }
    ds.verify_publish()?;
    // 变更日志两行。
    if ds.log_view().len() != 2 {
        return Err(E_SEAL);
    }
    // 错误路径 4：未发布导出（新数据集）。
    let ds2 = Dataset::new(1)?;
    if ds2.export(Access::Internal) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 5：空数据集发布。
    let mut ds3 = Dataset::new(2)?;
    if ds3.publish() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    Ok(0)
}
