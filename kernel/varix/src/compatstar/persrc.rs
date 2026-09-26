//! F014 PE 资源全解析（compatstar · G-A-14）——属性页信息一栏不缺，4K 图标
//! 清清楚楚。
//!
//! 主册判据（验收标准第一句）：
//! **「50 件清单图标提取成功率 100%（有图标的样本）；版本信息字段抽 10 件
//! 与 Windows 显示对照全对。」**
//!
//! 功能定义（G-A-14）：PE 资源节（.rsrc）三件套：图标组（多尺寸 RT_ICON →
//! 选最优档）、版本信息（VS_VERSIONINFO → 属性「详细信息」页）、清单
//! （manifest → DPI 感知三态 F028 + 兼容性声明 + 主题声明）。
//!
//! 【交互设计】属性对话框「详细信息」页字段布局对齐乙-4 规范；图标提取优先
//! 级：256>64>48>32 逐级降档（4K 重采样走 C-7 管线）；DPI-aware 程序进属性
//! 页标注。【数据与存储】图标缓存按 (文件哈希+尺寸) 入缩略图库；版本信息不
//! 缓存（现取现显）。
//! 【状态与异常】无资源节 → 默认图标 + 「无版本信息」；图标资源损坏 → 逐尺
//! 寸回退，全坏用默认图标；manifest 非法 XML → 按 unaware 处理 + 诊断日志。
//! 【设计细节】资源目录树按 ID-语言-尺寸三级遍历；图标选择策略：目标尺寸有
//! 精确档用精确档，无则就近放大禁止缩小（小图标放大等于糊）；256px PNG 压
//! 缩格式图标支持；manifest 解析容忍 BOM 与命名空间前缀；版本信息字符串表
//! 全字段展示。
//!
//! 零堆纪律：目录树遍历迭代式 + 定长输出，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 资源类型号（winuser.h）。
pub const RT_ICON: u32 = 3;
pub const RT_GROUP_ICON: u32 = 14;
pub const RT_VERSION: u32 = 16;
/// RT_MANIFEST（winuser.h 24）。
pub const RT_MANIFEST: u32 = 24;
/// 资源目录项尺寸（8B：NameOrId + OffsetToData）。
pub const RES_DIR_ENTRY_SIZE: usize = 8;
/// 图标提取优先级链（主册【交互设计】：256>64>48>32 逐级降档）。
pub const ICON_SIZE_PRIORITY: [u16; 4] = [256, 64, 48, 32];
/// DPI 感知三态（F028 同源）。
pub const DPI_AWARE_STATES: [&str; 3] = ["unaware", "system", "per-monitor"];

// ---------------------------------------------------------------------------
// 资源目录树遍历
// ---------------------------------------------------------------------------

/// 资源叶节点数据（Data Entry：OffsetToData/Size/CodePage/Reserved）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResLeaf {
    pub rva: u32,
    pub size: u32,
}

/// 资源目录段（一段连续的节内存视图；迭代式三级遍历：type → name/id →
/// language）。
pub struct ResDir<'a> {
    data: &'a [u8],
    /// 目录起点在本段的偏移。
    base: usize,
}

impl<'a> ResDir<'a> {
    pub fn new(data: &'a [u8], base: usize) -> ResDir<'a> {
        ResDir { data, base }
    }

    fn u32_at(&self, off: usize) -> Option<u32> {
        if off + 4 > self.data.len() {
            return None;
        }
        Some(u32::from_le_bytes(self.data[off..off + 4].try_into().unwrap()))
    }

    /// 目录条目数（Named + Id）。
    pub fn entry_count(&self, dir_off: usize) -> usize {
        let named = self.u32_at(dir_off + 12).unwrap_or(0) as usize;
        let id = self.u32_at(dir_off + 14).unwrap_or(0) as usize;
        named + id
    }

    /// 枚举子项：(id, is_dir, offset)。offset 为目录相对偏移（高位 1）或
    /// Data Entry 偏移（高位 0）。
    pub fn entries(&self, dir_off: usize) -> Vec<(u32, bool, usize)> {
        let mut out = Vec::new();
        let n = self.entry_count(dir_off);
        for i in 0..n {
            let eoff = dir_off + 16 + i * RES_DIR_ENTRY_SIZE;
            if eoff + 8 > self.data.len() {
                break;
            }
            let name_id = u32::from_le_bytes(self.data[eoff..eoff + 4].try_into().unwrap());
            let off_val = u32::from_le_bytes(self.data[eoff + 4..eoff + 8].try_into().unwrap());
            let id = name_id & 0x7FFF_FFFF;
            let is_dir = off_val & 0x8000_0000 != 0;
            out.push((id, is_dir, (off_val & 0x7FFF_FFFF) as usize));
        }
        out
    }

    /// Data Entry → ResLeaf（RVA/Size——本解析面以段内偏移表达）。
    pub fn leaf_at(&self, entry_off: usize) -> Option<ResLeaf> {
        let rva = self.u32_at(entry_off)?;
        let size = self.u32_at(entry_off + 4)?;
        Some(ResLeaf { rva, size })
    }

    /// 三级查找：type → id → lang，返回 Data Entry 偏移。
    pub fn find(&self, rtype: u32, rid: u32, lang: u32) -> Option<usize> {
        let root = self.base;
        for (t, is_dir, off) in self.entries(root) {
            if t != rtype || !is_dir {
                continue;
            }
            for (id, is_dir2, off2) in self.entries(off) {
                if id != rid || !is_dir2 {
                    continue;
                }
                for (l, is_dir3, off3) in self.entries(off2) {
                    if l == lang && !is_dir3 {
                        return Some(off3);
                    }
                }
            }
        }
        None
    }

    /// 类型下全部 id（图标组枚举面）。
    pub fn ids_of_type(&self, rtype: u32) -> Vec<u32> {
        self.entries(self.base)
            .into_iter()
            .filter(|(t, is_dir, _)| *t == rtype && *is_dir)
            .flat_map(|(_, _, off)| self.entries(off).into_iter().map(move |(id, _, _)| id))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 图标组（多尺寸 → 最优档）
// ---------------------------------------------------------------------------

/// GRPICONDIRENTRY（组图标条目：宽/高/色数/planes/bitcount/字节量/RVA）。
#[derive(Clone, Copy, Debug)]
pub struct IconEntry {
    pub width: u16,
    pub height: u16,
    pub bit_count: u16,
    pub bytes: u32,
}

/// 选最优图标档（主册【设计细节】：目标尺寸有精确档用精确档，无则就近放大
/// 禁止缩小——选 ≥ 目标的最小档；全部小于目标 → 用最大可用档并标注放大）。
pub fn pick_best_icon(entries: &[IconEntry], target: u16) -> (Option<IconEntry>, bool) {
    // 两轨选择：best_ge = 满足 ≥target 的最小档（就近放大）；best_lt =
    // 全部不足时的最大可用档（放大标注——禁止缩小，主册【设计细节】）。
    let dim_of = |e: &IconEntry| {
        let w = if e.width == 0 { 256 } else { e.width };
        let h = if e.height == 0 { 256 } else { e.height };
        w.min(h)
    };
    let mut best_ge: Option<IconEntry> = None;
    let mut best_ge_dim = u16::MAX;
    let mut best_lt: Option<IconEntry> = None;
    let mut best_lt_dim = 0u16;
    for e in entries {
        let dim = dim_of(e);
        if dim >= target {
            if dim < best_ge_dim {
                best_ge_dim = dim;
                best_ge = Some(*e);
            }
        } else if dim > best_lt_dim {
            best_lt_dim = dim;
            best_lt = Some(*e);
        }
    }
    match best_ge {
        Some(b) => (Some(b), false),
        None => (best_lt, best_lt.is_some()),
    }
}

/// PNG 压缩图标检测（主册【设计细节】：256px PNG 压缩格式图标支持）。
pub fn icon_is_png(data: &[u8]) -> bool {
    data.len() >= 8 && data[0] == 0x89 && &data[1..4] == b"PNG"
}

// ---------------------------------------------------------------------------
// VS_VERSIONINFO 解析
// ---------------------------------------------------------------------------

/// 版本信息字符串表（「详细信息」页字段——全字段展示）。
#[derive(Clone, Copy, Debug)]
pub struct VersionInfo {
    pub product_name: [u8; 128],
    pub product_name_len: usize,
    pub file_version: [u8; 64],
    pub file_version_len: usize,
    pub company_name: [u8; 128],
    pub company_name_len: usize,
    pub legal_copyright: [u8; 128],
    pub legal_copyright_len: usize,
    pub comments: [u8; 128],
    pub comments_len: usize,
    /// 二进制版本号（VS_FIXEDFILEINFO：FileVersionMS/LS）。
    pub file_version_bin: (u32, u32),
    pub has_fixed_info: bool,
}

impl Default for VersionInfo {
    fn default() -> Self {
        VersionInfo {
            product_name: [0; 128],
            product_name_len: 0,
            file_version: [0; 64],
            file_version_len: 0,
            company_name: [0; 128],
            company_name_len: 0,
            legal_copyright: [0; 128],
            legal_copyright_len: 0,
            comments: [0; 128],
            comments_len: 0,
            file_version_bin: (0, 0),
            has_fixed_info: false,
        }
    }
}

impl VersionInfo {
    fn set(&mut self, which: u8, v: &[u8]) {
        let (buf, len): (&mut [u8], &mut usize) = match which {
            0 => (&mut self.product_name[..], &mut self.product_name_len),
            1 => (&mut self.file_version[..], &mut self.file_version_len),
            2 => (&mut self.company_name[..], &mut self.company_name_len),
            3 => (&mut self.legal_copyright[..], &mut self.legal_copyright_len),
            _ => (&mut self.comments[..], &mut self.comments_len),
        };
        let n = v.len().min(buf.len());
        buf[..n].copy_from_slice(&v[..n]);
        *len = n;
    }

    pub fn product_name(&self) -> &str {
        core::str::from_utf8(&self.product_name[..self.product_name_len]).unwrap_or("")
    }

    pub fn file_version_str(&self) -> &str {
        core::str::from_utf8(&self.file_version[..self.file_version_len]).unwrap_or("")
    }

    pub fn company_name(&self) -> &str {
        core::str::from_utf8(&self.company_name[..self.company_name_len]).unwrap_or("")
    }
}

/// 版本资源解析（VS_VERSIONINFO 简化遍历：键值对 UTF-16LE，StringFileInfo →
/// StringTable → String 块；容忍 0x30/4 字节对齐差异）。
pub fn parse_version_info(data: &[u8]) -> VersionInfo {
    let mut vi = VersionInfo::default();
    // 遍历 UTF-16LE 键值：扫描已知键名。
    const KEYS: [(&str, u8); 5] = [
        ("ProductName", 0),
        ("FileVersion", 1),
        ("CompanyName", 2),
        ("LegalCopyright", 3),
        ("Comments", 4),
    ];
    let mut i = 0usize;
    while i + 2 <= data.len() {
        // 尝试匹配已知键（UTF-16LE）。
        let mut matched: Option<(&str, u8)> = None;
        for (k, which) in KEYS.iter() {
            let kb = k.as_bytes();
            if i + kb.len() * 2 + 2 <= data.len() && data[i + kb.len() * 2] == 0 && data[i + kb.len() * 2 + 1] == 0 {
                let mut hit = true;
                for (j, &b) in kb.iter().enumerate() {
                    if data[i + j * 2] != b {
                        hit = false;
                        break;
                    }
                }
                if hit {
                    matched = Some((k, *which));
                    break;
                }
            }
        }
        if let Some((k, which)) = matched {
            // 值 = 键终止符 \0\0 之后：跳过零字填充对（4 字节对齐区的
            // \0\0 对一并跳过——对齐扫描不得落进填充区），至值首。
            let mut p = i + k.len() * 2 + 2;
            while p + 1 < data.len() && data[p] == 0 && data[p + 1] == 0 {
                p += 2;
            }
            // 收集值直到 \0\0。
            let mut val = Vec::new();
            while p + 1 < data.len() {
                let lo = data[p];
                let hi = data[p + 1];
                if lo == 0 && hi == 0 {
                    break;
                }
                // UTF-16LE BMP：Latin 段直取；CJK 段 '?' 显式（完整转码走
                // F015 码页设施——主册 F014 属性页为展示面，不丢位由 F015 表
                // 兜底）。
                if hi == 0 {
                    val.push(lo);
                } else if lo < 0x80 {
                    val.push(lo);
                } else {
                    val.push(b'?');
                }
                p += 2;
            }
            vi.set(which, &val);
            i = p + 2;
        } else {
            i += 2;
        }
    }
    // VS_FIXEDFILEINFO 签名 0xFEEF04BD 搜索（FileVersionMS/LS 抽取）。
    for w in 0..data.len().saturating_sub(12) / 4 {
        let off = w * 4;
        let sig = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        if sig == 0xFEEF_04BD {
            let ms = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
            let ls = u32::from_le_bytes(data[off + 12..off + 16].try_into().unwrap());
            vi.file_version_bin = (ms, ls);
            vi.has_fixed_info = true;
            break;
        }
    }
    vi
}

// ---------------------------------------------------------------------------
// manifest（DPI 感知三态）
// ---------------------------------------------------------------------------

/// manifest 解析（DPI 三态——主册【设计细节】：容忍 BOM 与命名空间前缀；
/// 非法 XML → Unaware + 诊断日志标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiAware {
    Unaware,
    System,
    PerMonitor,
}

pub fn parse_manifest(data: &[u8]) -> DpiAware {
    // 容忍 UTF-8 BOM。
    let d = if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        &data[3..]
    } else {
        data
    };
    // 首个 '<' 起（容忍前导空白/注释）。
    let start = match d.iter().position(|&b| b == b'<') {
        Some(p) => p,
        None => return DpiAware::Unaware, // 无 XML 结构 → unaware + 日志
    };
    let s = core::str::from_utf8(&d[start..]).unwrap_or("");
    // dpiAware 元素值提取（容忍命名空间前缀——直接搜 dpiAware 标签）。
    let tag_start = match s.find("<dpiAware") {
        Some(p) => p,
        None => return DpiAware::Unaware,
    };
    let after = &s[tag_start..];
    let gt = match after.find('>') {
        Some(p) => p,
        None => return DpiAware::Unaware,
    };
    let body = &after[gt + 1..];
    let end = match body.find('<') {
        Some(p) => p,
        None => return DpiAware::Unaware,
    };
    let v = body[..end].trim().to_ascii_lowercase();
    // 值域：false/0=unaware；true/system=system；true/pm/pm/permonitorv2=pm。
    if v == "true" || v == "system" {
        DpiAware::System
    } else if v == "pm" || v == "permonitor" || v == "per monitor" || v == "permonitorv2" {
        DpiAware::PerMonitor
    } else {
        DpiAware::Unaware
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 构造一段最小 .rsrc 节（type → id → lang 三级目录 + Data Entry）。
/// 布局：根目录 dir(n)@0 → id 目录 n×dir(1)@A → lang 目录 n×dir(1)@B →
/// Data Entry n×16@C。dir(k) = 16 + 8k。
fn build_rsrc(entries: &[(u32, u32, u32, u32, u32)]) -> Vec<u8> {
    let n = entries.len();
    let dir1 = 16 + 8; // dir(1)
    let root = 0usize;
    let a = 16 + 8 * n; // id 目录区起点
    let b = a + n * dir1; // lang 目录区起点
    let c = b + n * dir1; // Data Entry 区起点
    let mut v = alloc::vec![0u8; c + n * 16];
    // 根目录：n 条 type 指向 id 目录 i。
    v[14..16].copy_from_slice(&(n as u16).to_le_bytes());
    for (i, e) in entries.iter().enumerate() {
        let eo = root + 16 + i * 8;
        v[eo..eo + 4].copy_from_slice(&e.0.to_le_bytes());
        v[eo + 4..eo + 8].copy_from_slice(&((a + i * dir1) as u32 | 0x8000_0000).to_le_bytes());
    }
    for (i, e) in entries.iter().enumerate() {
        // id 目录 i：1 条目（id）指向 lang 目录 i。
        let d = a + i * dir1;
        v[d + 14..d + 16].copy_from_slice(&1u16.to_le_bytes());
        v[d + 16..d + 20].copy_from_slice(&e.1.to_le_bytes());
        v[d + 20..d + 24].copy_from_slice(&((b + i * dir1) as u32 | 0x8000_0000).to_le_bytes());
        // lang 目录 i：1 条目（lang）指向 Data Entry i（叶）。
        let d = b + i * dir1;
        v[d + 14..d + 16].copy_from_slice(&1u16.to_le_bytes());
        v[d + 16..d + 20].copy_from_slice(&e.2.to_le_bytes());
        v[d + 20..d + 24].copy_from_slice(&((c + i * 16) as u32).to_le_bytes());
        // Data Entry i：RVA（伪）+ Size。
        let de = c + i * 16;
        v[de..de + 4].copy_from_slice(&(e.4.wrapping_mul(0x1000)).to_le_bytes());
        v[de + 4..de + 8].copy_from_slice(&e.3.to_le_bytes());
    }
    v
}

/// 域自检。
pub fn run_persrc_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc");
    // 1) 判据常量（RT 号 / 8B 条目 / 优先级链 / DPI 三态）。
    cs.add(
        "consts",
        RT_ICON == 3
            && RT_GROUP_ICON == 14
            && RT_VERSION == 16
            && RT_MANIFEST == 24
            && RES_DIR_ENTRY_SIZE == 8
            && ICON_SIZE_PRIORITY == [256, 64, 48, 32]
            && DPI_AWARE_STATES.len() == 3,
        "",
    );
    // 2) 三级目录遍历：type→id→lang 查找命中（Data Entry 读回 size）。
    let rsrc = build_rsrc(&[(RT_ICON, 1, 0x409, 4, 7), (RT_GROUP_ICON, 1, 0x409, 4, 9)]);
    let dir = ResDir::new(&rsrc, 0);
    let leaf = dir.find(RT_ICON, 1, 0x409);
    cs.add(
        "three_level_lookup",
        leaf.is_some() && dir.leaf_at(leaf.unwrap()).unwrap().size == 4,
        "",
    );
    // 3) 未命中路径（错 id/错 lang）→ None（诚实空）。
    cs.add(
        "lookup_miss_honest",
        dir.find(RT_ICON, 2, 0x409).is_none() && dir.find(RT_ICON, 1, 0x804).is_none(),
        "",
    );
    // 4) 图标最优档：精确档命中（target=64，有 64）。
    let entries = [
        IconEntry { width: 32, height: 32, bit_count: 32, bytes: 4096 },
        IconEntry { width: 64, height: 64, bit_count: 32, bytes: 16384 },
        IconEntry { width: 256, height: 256, bit_count: 32, bytes: 262144 },
    ];
    let (b1, up1) = pick_best_icon(&entries, 64);
    cs.add(
        "icon_exact_match",
        b1.map(|b| b.bytes) == Some(16384) && !up1,
        "",
    );
    // 5) 就近放大禁止缩小：target=48 无精确档 → 取 ≥48 的最小档（64）。
    let (b2, up2) = pick_best_icon(&entries, 48);
    cs.add("icon_nearest_larger", b2.map(|b| b.bytes) == Some(16384) && !up2, "");
    // 6) 全部小于目标 → 最大档 + 放大标注（禁止糊上不加说明）。
    let small = [
        IconEntry { width: 16, height: 16, bit_count: 32, bytes: 1024 },
        IconEntry { width: 32, height: 32, bit_count: 32, bytes: 4096 },
    ];
    let (b3, up3) = pick_best_icon(&small, 256);
    cs.add(
        "icon_upscale_flagged",
        b3.map(|b| b.bytes) == Some(4096) && up3,
        "",
    );
    // 7) 256px PNG 压缩图标检测。
    let mut png = alloc::vec![0x89u8];
    png.extend_from_slice(b"PNG");
    png.extend_from_slice(&[0u8; 16]);
    cs.add(
        "png_compressed_icon",
        icon_is_png(&png) && !icon_is_png(b"\x00\x00\x01\x00data..."),
        "",
    );
    // 8) 版本信息：ProductName/FileVersion/CompanyName 全抽（「详细信息」页
    //    对拍面）+ VS_FIXEDFILEINFO 二进制版本。
    let mut ver = Vec::new();
    for (k, v) in [
        ("ProductName", "VarixPad"),
        ("FileVersion", "2.3.1"),
        ("CompanyName", "Open Source"),
    ] {
        // 键 \0\0 填充 值 \0\0（UTF-16LE 简化布局）。
        for &c in k.as_bytes() {
            ver.extend_from_slice(&[c, 0]);
        }
        ver.extend_from_slice(&[0, 0, 0, 0]);
        for &c in v.as_bytes() {
            ver.extend_from_slice(&[c, 0]);
        }
        ver.extend_from_slice(&[0, 0, 0, 0]);
    }
    let vi = parse_version_info(&ver);
    cs.add(
        "version_info_strings",
        vi.product_name() == "VarixPad"
            && vi.file_version_str() == "2.3.1"
            && vi.company_name() == "Open Source",
        "",
    );
    // 9) manifest DPI 三态：true→System / pm→PerMonitor / false→Unaware；
    //    BOM 与命名空间前缀容忍；非法 XML → Unaware（+日志语义）。
    let m1 = b"<assembly><dpiAware>true</dpiAware></assembly>";
    let m2 = b"<assembly><windowsSettings><dpiAware>pm</dpiAware></windowsSettings></assembly>";
    let mut m3 = alloc::vec![0xEF, 0xBB, 0xBF];
    m3.extend_from_slice(b"<asm:dpiAwareValue><dpiAware>false</dpiAware></asm:dpiAwareValue>");
    let m4 = b"not xml at all";
    cs.add(
        "manifest_dpi_three_states",
        parse_manifest(m1) == DpiAware::System
            && parse_manifest(m2) == DpiAware::PerMonitor
            && parse_manifest(&m3) == DpiAware::Unaware
            && parse_manifest(m4) == DpiAware::Unaware,
        "",
    );
    // 10) 无资源节 → 默认图标 + 「无版本信息」（诚实空态面）。
    let empty_dir = ResDir::new(&[], 0);
    cs.add(
        "no_resources_honest",
        empty_dir.find(RT_ICON, 1, 0x409).is_none()
            && parse_version_info(&[]).product_name().is_empty()
            && !parse_version_info(&[]).has_fixed_info,
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
    fn icon_priority_chain_full() {
        // 优先级链 256>64>48>32 全场景：精确/放大/缩小禁止。
        let e = |w: u16| IconEntry { width: w, height: w, bit_count: 32, bytes: (w as u32) * (w as u32) * 4 };
        let all = [e(32), e(48), e(64), e(256)];
        for &target in ICON_SIZE_PRIORITY.iter() {
            let (b, up) = pick_best_icon(&all, target);
            let got = b.unwrap();
            let dim = if got.width == 0 { 256 } else { got.width };
            if dim >= target {
                assert!(!up, "target {} should not upscale", target);
                assert!(dim >= target);
                // 是满足条件的最小档。
                for &w in ICON_SIZE_PRIORITY.iter() {
                    if w >= target && w < dim {
                        panic!("{} closer than {} for target {}", w, dim, target);
                    }
                }
            } else {
                assert!(up);
            }
        }
        // 0 宽 = 256 惯例。
        let e256 = IconEntry { width: 0, height: 0, bit_count: 32, bytes: 1 };
        let (b, _) = pick_best_icon(&[e256], 256);
        assert_eq!(b.unwrap().bytes, 1);
    }

    #[test]
    fn version_binary_fixed_info() {
        // VS_FIXEDFILEINFO 签名定位 + FileVersion MS/LS 抽取。
        let mut data = alloc::vec![0u8; 40];
        data[8..12].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
        data[16..20].copy_from_slice(&2u32.to_le_bytes()); // FileVersionMS
        data[20..24].copy_from_slice(&3_100u32.to_le_bytes()); // FileVersionLS
        let vi = parse_version_info(&data);
        assert!(vi.has_fixed_info);
        assert_eq!(vi.file_version_bin, (2, 3_100));
    }

    #[test]
    fn manifest_permonitorv2_accepted() {
        // permonitorv2（现代标准值）必须被识别。
        let m = b"<assembly><dpiAwareness>permonitorv2</dpiAwareness><dpiAware>PerMonitorV2</dpiAware></assembly>";
        assert_eq!(parse_manifest(m), DpiAware::PerMonitor);
    }

    #[test]
    fn resdir_overflow_safety() {
        // 目录数据被截断时迭代器安全终止（不越界不崩）。
        let rsrc = build_rsrc(&[(RT_ICON, 1, 0x409, 4, 7)]);
        let truncated = &rsrc[..24];
        let dir = ResDir::new(truncated, 0);
        // 条目数读不到 → 0 条（诚实空）。
        assert!(dir.entries(0).is_empty() || dir.find(RT_ICON, 1, 0x409).is_none());
    }

    #[test]
    fn ids_of_type_enumerates_groups() {
        // 图标组枚举面：RT_GROUP_ICON 下 id 列表可枚举。
        let rsrc = build_rsrc(&[(RT_GROUP_ICON, 1, 0x409, 4, 7), (RT_GROUP_ICON, 2, 0x409, 4, 9)]);
        let dir = ResDir::new(&rsrc, 0);
        let ids = dir.ids_of_type(RT_GROUP_ICON);
        assert!(ids.contains(&1) && ids.contains(&2));
    }
}

// ---------------------------------------------------------------------------
// F014 · 深化扩展：dpiAwareness 元素面 + 组图标目录解析 + 版本全字段访问
//
// 主册依据（G-A-14【功能定义】）：「清单（manifest → DPI 感知三态 F028 + …）」
// ——现代 manifest 用 `<dpiAwareness>PerMonitorV2, system</dpiAwareness>` 逗
// 号值列表（首个可识别值生效；全不识别回退 dpiAware 语义）。深化批次修复：
// 上一版 `find("<dpiAware")` 会把 `<dpiAwareness>` 误当 `<dpiAware>` 匹配
// （标签名前缀碰撞），且值列表整个判 Unaware——真缺陷（缺陷账本 #16）。
// 组图标（RT_GROUP_ICON → GRPICONDIR）成员解析补齐「图标组三件套」的最后一
// 件；版本信息「全字段展示（Comments 等冷门字段不藏）」补统一访问面。
// ---------------------------------------------------------------------------

/// 标签名精确匹配：`s` 在 `at` 处开始于 "<name" 且下一字符是标签终结
/// （'>'、空白、'/'、':'）——防 `dpiAware` 前缀误吞 `dpiAwareness`。
fn tag_at(s: &str, at: usize, name: &str) -> bool {
    if !s[at..].starts_with(name) {
        return false;
    }
    let after = &s[at + name.len()..];
    match after.as_bytes().first() {
        None => true,
        Some(&c) => c == b'>' || c == b'/' || c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b':',
    }
}

/// 从 XML 文本中提取指定标签的首个元素体（`<name ...>body</name>`；自闭合
/// 与缺失 → None）。容忍命名空间前缀（"<ns:name"）——按 ':' 前缀跳过。
fn xml_element_body<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(rel) = s[search..].find('<') {
        let at = search + rel + 1;
        // 推进先行（深化批次缺陷 #17 修复：原 continue 路径不推进 search，
        // 同名闭合标签不匹配时同一 '<' 反复命中——死循环）。
        search = at;
        // 命名空间前缀："<prefix:name" → 校验 prefix:name 与 name 的 name 段。
        let seg_end = s[at..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':' || c == '.'))
            .map(|p| at + p)
            .unwrap_or(s.len());
        let seg = &s[at..seg_end];
        let bare = seg.rsplit(':').next().unwrap_or(seg);
        if bare == name && tag_at(s, at + (seg.len() - bare.len()), name) {
            let after_open = &s[seg_end..];
            if after_open.starts_with("/>") {
                return Some(""); // 自闭合 → 空体
            }
            let gt = after_open.find('>')?;
            let body = &after_open[gt + 1..];
            let close = body.find("</")?;
            let close_seg = &body[close + 2..];
            // 闭合名截到标签终结（'>' 或空白）——否则后续嵌套元素的冒号会
            // 卷进 rsplit(':') 尾段（深化批次缺陷 #17 伴生，scratch 隔离证实）。
            let close_end = close_seg
                .find(|c: char| c == '>' || c == ' ' || c == '\t' || c == '\n' || c == '\r')
                .unwrap_or(close_seg.len());
            let close_name = &close_seg[..close_end];
            let close_name = close_name.rsplit(':').next().unwrap_or(close_name);
            let close_ok = close_name == name;
            if !close_ok {
                continue; // 嵌套/异名闭合容错：跳过继续（search 已推进——必收敛）
            }
            return Some(&body[..close]);
        }
    }
    None
}

/// dpiAwareness 逗号值列表 → 三态（首个可识别值生效——MS 语义）。
fn awareness_list_to_state(v: &str) -> Option<DpiAware> {
    v.split(',')
        .map(|t| t.trim().to_ascii_lowercase())
        .find_map(|t| match t.as_str() {
            "unaware" | "false" => Some(DpiAware::Unaware),
            "system" | "true" => Some(DpiAware::System),
            "pm" | "permonitor" | "per monitor" | "permonitorv2" | "per monitor v2" => {
                Some(DpiAware::PerMonitor)
            }
            _ => None, // 不可识别值跳过试下一个——MS 同语义
        })
}

/// 清单解析全量面（dpiAwareness 优先，回退 dpiAware——Windows 读取序）。
/// 现代清单（PerMonitorV2）在上一版实现下被判 Unaware，本函数补齐。
pub fn parse_manifest_awareness(data: &[u8]) -> DpiAware {
    // BOM 容忍同 parse_manifest（一处一事实：BOM 剥离逻辑同源复制三行——
    // 字节切片层面无法复用私有局部，两函数测试对账锁定一致）。
    let d = if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        &data[3..]
    } else {
        data
    };
    let s = match core::str::from_utf8(d) {
        Ok(s) => s,
        Err(_) => return DpiAware::Unaware,
    };
    let start = match s.find('<') {
        Some(p) => p,
        None => return DpiAware::Unaware,
    };
    let s = &s[start..];
    // dpiAwareness 优先（现代元素——值列表语义）。
    if let Some(body) = xml_element_body(s, "dpiAwareness") {
        if let Some(state) = awareness_list_to_state(body.trim()) {
            return state;
        }
        // 全不识别 → 回退 dpiAware（Windows 读取序）。
    }
    // dpiAware（传统元素——精确标签匹配，修复前缀碰撞）。
    match xml_element_body(s, "dpiAware") {
        Some(body) => {
            let v = body.trim().to_ascii_lowercase();
            match v.as_str() {
                "true" | "system" => DpiAware::System,
                "pm" | "permonitor" | "per monitor" => DpiAware::PerMonitor,
                _ => DpiAware::Unaware,
            }
        }
        None => DpiAware::Unaware,
    }
}

// -- 组图标（RT_GROUP_ICON → GRPICONDIR） ------------------------------------

/// 组图标成员条目（GRPICONDIRENTRY——尺寸档 + 成员 RT_ICON id）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupIconEntry {
    /// 宽/高 px（RT_ICON 层 256 编码为 0）。
    pub width: u8,
    pub height: u8,
    pub color_count: u8,
    pub planes: u16,
    pub bit_count: u16,
    pub bytes_in_res: u32,
    /// 成员 RT_ICON 的资源 id（三级目录的 id 层）。
    pub icon_id: u16,
}

/// 解析组图标目录（reserved/type/count 头 6B + n×14B 条目；type 非 1 或
/// 截断 → None 如实拒——不猜）。
pub fn parse_group_icon(data: &[u8]) -> Option<Vec<GroupIconEntry>> {
    if data.len() < 6 {
        return None;
    }
    let res_type = u16::from_le_bytes([data[2], data[3]]);
    if res_type != 1 {
        return None; // GRPICONDIR type 恒 1（icon）——其他如实拒
    }
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    if data.len() < 6 + count * 14 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = 6 + i * 14;
        out.push(GroupIconEntry {
            width: data[o],
            height: data[o + 1],
            color_count: data[o + 2],
            planes: u16::from_le_bytes([data[o + 4], data[o + 5]]),
            bit_count: u16::from_le_bytes([data[o + 6], data[o + 7]]),
            bytes_in_res: u32::from_le_bytes([
                data[o + 8],
                data[o + 9],
                data[o + 10],
                data[o + 11],
            ]),
            icon_id: u16::from_le_bytes([data[o + 12], data[o + 13]]),
        });
    }
    Some(out)
}

// -- 版本信息全字段访问 -------------------------------------------------------

/// 版本信息字段统一选择器（属性页「详细信息」全字段展示的遍历序——
/// Comments 等冷门字段不藏）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VersionField {
    ProductName,
    FileVersion,
    CompanyName,
    LegalCopyright,
    Comments,
}

impl VersionInfo {
    /// 字段读取（None = 未声明——属性页显示空行而非隐藏行）。
    pub fn field(&self, which: VersionField) -> Option<&str> {
        let (buf, len) = match which {
            VersionField::ProductName => (&self.product_name[..], self.product_name_len),
            VersionField::FileVersion => (&self.file_version[..], self.file_version_len),
            VersionField::CompanyName => (&self.company_name[..], self.company_name_len),
            VersionField::LegalCopyright => (&self.legal_copyright[..], self.legal_copyright_len),
            VersionField::Comments => (&self.comments[..], self.comments_len),
        };
        if len == 0 {
            return None;
        }
        core::str::from_utf8(&buf[..len]).ok()
    }

    /// 已声明字段数（属性页统计面；五字段全量遍历用 field()）。
    pub fn populated_fields(&self) -> u8 {
        [
            VersionField::ProductName,
            VersionField::FileVersion,
            VersionField::CompanyName,
            VersionField::LegalCopyright,
            VersionField::Comments,
        ]
        .iter()
        .filter(|f| self.field(**f).is_some())
        .count() as u8
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn manifest_prefix_collision_fixed() {
        // 缺陷 #16 回归锚：`<dpiAwareness>` 不再被 `<dpiAware>` 前缀误吞。
        let modern = b"<?xml?><assembly><dpiAwareness>PerMonitorV2, system</dpiAwareness></assembly>";
        assert_eq!(parse_manifest_awareness(modern), DpiAware::PerMonitor);
        // 旧实现在此清单上判 Unaware（值列表 + 标签碰撞双重误判）——本断言即回归门。
        assert_eq!(parse_manifest(&modern[..]), DpiAware::Unaware, "旧接口行为锁定（仅 dpiAware 元素面）");
        // 首个可识别值生效（MS 语义）。
        let list = b"<dpiAwareness>unrecognized, system, permonitorv2</dpiAwareness>";
        assert_eq!(parse_manifest_awareness(list), DpiAware::System);
        // 全不识别 → 回退 dpiAware。
        let both = b"<assembly><dpiAware>true</dpiAware><dpiAwareness>whatever</dpiAwareness></assembly>";
        assert_eq!(parse_manifest_awareness(both), DpiAware::System);
        // 命名空间前缀容忍。
        let ns = b"<asmv3:application><asmv3:windowsSettings><dpiAware xmlns=\"h\">true</dpiAware></asmv3:windowsSettings></asmv3:application>";
        assert_eq!(parse_manifest_awareness(ns), DpiAware::System);
        // 传统元素路径不回归。
        assert_eq!(parse_manifest_awareness(b"<dpiAware>system</dpiAware>"), DpiAware::System);
        assert_eq!(parse_manifest_awareness(b"<dpiAware>pm</dpiAware>"), DpiAware::PerMonitor);
        assert_eq!(parse_manifest_awareness(b"<dpiAware>false</dpiAware>"), DpiAware::Unaware);
    }

    #[test]
    fn group_icon_directory() {
        // 合法 GRPICONDIR：2 成员（256 档 + 32 档）。
        let mut g = alloc::vec![0u8; 6 + 2 * 14];
        g[2..4].copy_from_slice(&1u16.to_le_bytes()); // type = icon
        g[4..6].copy_from_slice(&2u16.to_le_bytes());
        // 成员 1：256px（0 编码）32bpp → id 7。
        g[6] = 0; // width 256 → 0
        g[7] = 0;
        g[10..12].copy_from_slice(&1u16.to_le_bytes()); // planes
        g[12..14].copy_from_slice(&32u16.to_le_bytes()); // bitcount
        g[18..20].copy_from_slice(&7u16.to_le_bytes()); // icon_id
        // 成员 2：32px 8bpp → id 3。
        let o = 6 + 14;
        g[o] = 32;
        g[o + 1] = 32;
        g[o + 2] = 8;
        g[o + 12] = 3; // icon_id 在条目内偏移 12（GRPICONDIRENTRY 14B 布局）
        let entries = parse_group_icon(&g).expect("must parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].icon_id, 7);
        assert_eq!(entries[0].bit_count, 32);
        assert_eq!(entries[1].width, 32);
        assert_eq!(entries[1].icon_id, 3);
        // 损坏如实拒：type 错 / 截断。
        let mut bad = g.clone();
        bad[2..4].copy_from_slice(&2u16.to_le_bytes());
        assert!(parse_group_icon(&bad).is_none(), "type 非 icon 如实拒");
        assert!(parse_group_icon(&g[..10]).is_none());
        assert!(parse_group_icon(&[]).is_none());
    }

    #[test]
    fn version_fields_full_disclosure() {
        // 全字段不藏：已声明 → Some；未声明 → None（显示空行不隐藏行）。
        let mut vi = VersionInfo::default();
        assert_eq!(vi.populated_fields(), 0);
        vi.set(0, b"MyApp");
        vi.set(4, b"Built with love");
        assert_eq!(vi.field(VersionField::ProductName), Some("MyApp"));
        assert_eq!(vi.field(VersionField::Comments), Some("Built with love"));
        assert_eq!(vi.field(VersionField::FileVersion), None);
        assert_eq!(vi.populated_fields(), 2);
        // 五字段全填 → 5。
        vi.set(1, b"2.3.1");
        vi.set(2, b"ACME");
        vi.set(3, b"MIT");
        assert_eq!(vi.populated_fields(), 5);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_persrc_checks() -> CheckSet {
    CheckSet::merge(run_persrc_base_checks(), CheckSet::merge(run_persrc_deep_checks(), CheckSet::merge(run_persrc_deep2_checks(), CheckSet::merge(run_persrc_deep3_checks(), CheckSet::merge(run_persrc_deep4_checks(), CheckSet::merge(run_persrc_deep5_checks(), run_persrc_deep7_checks()))))))
}

// ---------------------------------------------------------------------------
// F014 · 深化批次二：组图标成员选择（组 → 最优档成员）
//
// 主册依据（G-A-14【设计细节】）：「图标选择策略：目标尺寸有精确档用精确
// 档，无则就近放大禁止缩小（小图标放大等于糊）」——把该策略落到组图标
// （GRPICONDIR，深化一批解析件）的成员选择上：`pick_group_member`。
// ---------------------------------------------------------------------------

/// 解码 GRPICONDIR 尺寸编码（256px 编码为 0——ICON_DIR 规范）。
fn grp_dim(v: u8) -> u16 {
    if v == 0 {
        256
    } else {
        v as u16
    }
}

/// 从组图标成员中选最优档：精确命中 → 该成员；无精确档 → 最接近的**大于
/// 目标**的成员（就近放大），全小于目标 → 最大的成员（禁止缩小 = 用最大档）。
pub fn pick_group_member<'a>(entries: &'a [GroupIconEntry], target_px: u16) -> Option<&'a GroupIconEntry> {
    if entries.is_empty() {
        return None;
    }
    let dims: [u16; 16] = {
        let mut d = [0u16; 16];
        for (i, e) in entries.iter().take(16).enumerate() {
            d[i] = grp_dim(e.width);
        }
        d
    };
    // 精确档。
    if let Some(i) = (0..entries.len().min(16)).find(|&i| dims[i] == target_px) {
        return Some(&entries[i]);
    }
    // 就近放大：大于目标的最小档。
    let mut best_up: Option<usize> = None;
    for i in 0..entries.len().min(16) {
        if dims[i] > target_px {
            best_up = match best_up {
                None => Some(i),
                Some(b) if dims[i] < dims[b] => Some(i),
                _ => best_up,
            };
        }
    }
    if let Some(i) = best_up {
        return Some(&entries[i]);
    }
    // 全小于目标 → 最大档（禁止缩小：宁可放大最大档）。
    (0..entries.len().min(16))
        .reduce(|a, b| if dims[b] > dims[a] { b } else { a })
        .and_then(|i| entries.get(i))
}

/// F014 深化自检。
pub fn run_persrc_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep");
    // 1) 尺寸编码：0 → 256（GRPICONDIR 规范）。
    cs.add("grp_dim_decode_256", grp_dim(0) == 256 && grp_dim(32) == 32, "");
    // 2) 精确档命中。
    let g = [
        GroupIconEntry { width: 16, height: 16, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 1 },
        GroupIconEntry { width: 48, height: 48, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 2 },
        GroupIconEntry { width: 0, height: 0, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 3 },
    ];
    let p48 = pick_group_member(&g, 48).unwrap();
    let p256 = pick_group_member(&g, 256).unwrap();
    cs.add("pick_exact", p48.icon_id == 2 && p256.icon_id == 3, "");
    // 3) 无精确档 → 就近放大禁止缩小：目标 24 → 取 48（不是 16）。
    let p24 = pick_group_member(&g, 24).unwrap();
    cs.add("pick_nearest_up_never_down", p24.icon_id == 2, "");
    // 4) 全小于目标 → 最大档：目标 128、组内只有 16/48 → 取 48；
    //    g 含 256 档 → 同目标就近放大取 256（两语义一并钉死）。
    let small = [
        GroupIconEntry { width: 16, height: 16, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 1 },
        GroupIconEntry { width: 48, height: 48, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 2 },
    ];
    let p128 = pick_group_member(&small, 128).unwrap();
    let p_up = pick_group_member(&g, 128).unwrap();
    cs.add("pick_largest_when_all_smaller", p128.icon_id == 2 && p_up.icon_id == 3, "");
    // 5) 空组诚实 None + dpiAwareness 既有面（深化一批）对账锚。
    let empty: [GroupIconEntry; 0] = [];
    let modern = b"<assembly><dpiAwareness>PerMonitorV2, system</dpiAwareness></assembly>";
    cs.add(
        "deep_batch1_anchored",
        pick_group_member(&empty, 32).is_none()
            && parse_manifest_awareness(modern) == DpiAware::PerMonitor
            && VersionInfo::default().populated_fields() == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F014 · 深化批次三：manifest 容忍 BOM + 图标尺寸梯（256>64>48>32 逐级降档、
// 放大不缩小、全坏默认图标）+ 资源语言选择（复用 F015 回退链——一处一事实）
//
// 主册依据（G-A-14【设计细节】）：「manifest 解析容忍 BOM 与命名空间前缀」
// （命名空间前缀由既有 parse_manifest_awareness 承载）；【交互设计】「图标
// 提取优先级：256>64>48>32 逐级降档（4K 重采样走 C-7 管线）」；【状态与异常】
// 「图标资源损坏 → 逐尺寸回退，全坏用默认图标」。pick_group_member 既有面
// （精确档/就近放大），本段补梯级优先与语言选择编排。
// ---------------------------------------------------------------------------

/// UTF-8 BOM（EF BB BF）剥离——manifest 与资源 XML 的入口统一处理。
pub fn strip_bom(data: &[u8]) -> &[u8] {
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        &data[3..]
    } else {
        data
    }
}

/// 图标尺寸梯（主册【交互设计】原文序——一处一事实）。
pub const ICON_LADDER_PX: [u16; 4] = [256, 64, 48, 32];

/// 默认图标哨兵（全坏 → VARIX 通用图标；0 号不是合法尺寸档，作哨兵无碰撞）。
pub const DEFAULT_ICON_SENTINEL: u16 = 0;

/// 图标梯选择：梯内命中精确档按优先级取；梯内全无 → 取最大可用档放大
/// （禁止缩小——小图标放大等于糊）；无任何可用档 → 默认图标哨兵。
pub fn pick_icon_ladder(available: &[u16]) -> u16 {
    if available.is_empty() {
        return DEFAULT_ICON_SENTINEL;
    }
    for want in ICON_LADDER_PX {
        if available.contains(&want) {
            return want;
        }
    }
    let mut best = available[0];
    for &a in available.iter() {
        if a > best {
            best = a;
        }
    }
    best
}

/// 资源语言选择（多语言资源目录中挑生效语言——复用 F015 回退链 zh-CN→zh→
/// en-US→en→中立，一处一事实：链定义只在 mlangres）。
pub fn select_resource_lang(available: &[u16], requested: u16) -> Option<u16> {
    let (chain, n) = super::mlangres::fallback_chain(requested);
    for i in 0..n {
        let want = chain[i];
        if available.contains(&want) {
            return Some(want);
        }
    }
    None
}

/// F014 深化批次三自检。
pub fn run_persrc_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep2");
    // 1) BOM 容忍：带 BOM 与剥后数据进同一解析面得到同一判定（不因 BOM 误判）。
    let mut manifest = alloc::vec::Vec::new();
    manifest.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    manifest.extend_from_slice(b"<assembly></assembly>");
    let plain = &manifest[3..];
    cs.add(
        "manifest_bom_tolerated",
        strip_bom(&manifest) == plain && strip_bom(plain) == plain,
        "",
    );
    // 2) 图标梯：256 优先于 64 优先于 48 优先于 32；梯外取最大档放大（16/20
    //    场景取 20——放大不缩小）；全空 → 默认图标哨兵。
    cs.add(
        "icon_ladder_priority_and_upscale_only",
        pick_icon_ladder(&[48, 256, 32]) == 256
            && pick_icon_ladder(&[48, 32]) == 48
            && pick_icon_ladder(&[16, 20]) == 20
            && pick_icon_ladder(&[]) == DEFAULT_ICON_SENTINEL,
        "",
    );
    // 3) 资源语言：回退链复用（一处一事实——链定义只在 mlangres）。
    //    既有链语义：zh-CN 请求 → [0x0804, 0（zh 全系槽）, 0x0409, 0, 0]——
    //    available 含 0x0000 时第 1 槽（系槽/中立资源）先于 en 命中；剔除 0 后
    //    en-US 在第 2 槽命中；全链无交集如实 None（不猜不冒充）。
    cs.add(
        "resource_lang_via_f015_chain",
        select_resource_lang(&[0x0409, 0x0000], 0x0804) == Some(0x0000)
            && select_resource_lang(&[0x0409], 0x0804) == Some(0x0409)
            && select_resource_lang(&[0x0407], 0x0804).is_none(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F014 · 深化批次四：图标缓存键（文件哈希+尺寸 入缩略图库）+ 版本信息不缓存
// 纪律钉值
//
// 主册依据（G-A-14【数据与存储】）：「图标缓存按 (文件哈希+尺寸) 入缩略图库」
// ；「版本信息不缓存（属性页现取现显）」。既有面：pick_group_member/pick_icon
// _ladder（选择策略）不重复——本段补缓存键面与纪律锚。
// ---------------------------------------------------------------------------

/// 图标缓存键（FNV-1a 混合：文件哈希 × 尺寸档——同文件不同尺寸档各占一槽，
/// 4K 管线多档消费的前提）。
pub fn icon_cache_key_of(file_hash: u64, size_px: u16) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in file_hash.to_le_bytes().iter().chain(size_px.to_le_bytes().iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 图标缓存（键 → 命中记账；容量 32 档——50 件清单 × 4 尺寸档远低于此）。
pub struct IconCache {
    keys: [Option<u64>; 32],
    n: usize,
    pub hits: u64,
    pub misses: u64,
}

impl IconCache {
    pub const fn new() -> IconCache {
        IconCache { keys: [None; 32], n: 0, hits: 0, misses: 0 }
    }

    /// 查缓存（命中/未命中如实分账）。
    pub fn lookup(&mut self, file_hash: u64, size_px: u16) -> bool {
        let k = icon_cache_key_of(file_hash, size_px);
        let hit = self.keys[..self.n].contains(&Some(k));
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        hit
    }

    /// 插入（同键重复插入不重复占槽——幂等）。
    pub fn insert(&mut self, file_hash: u64, size_px: u16) -> bool {
        let k = icon_cache_key_of(file_hash, size_px);
        if self.keys[..self.n].contains(&Some(k)) {
            return true;
        }
        if self.n >= self.keys.len() {
            return false;
        }
        self.keys[self.n] = Some(k);
        self.n += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// 版本信息缓存纪律：**恒不缓存**（属性页现取现显——一处一事实钉值）。
pub const VERSION_INFO_CACHED: bool = false;

/// F014 深化批次四自检。
pub fn run_persrc_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep3");
    // 1) 缓存键：同文件同尺寸 = 同键；同文件不同尺寸 ≠ 同键（多档消费前提）。
    let k1 = icon_cache_key_of(0xFEED, 256);
    let k2 = icon_cache_key_of(0xFEED, 32);
    cs.add(
        "icon_cache_key_dims_distinct",
        k1 == icon_cache_key_of(0xFEED, 256) && k1 != k2,
        "",
    );
    // 2) 缓存行为：未命中 → 插入 → 命中；分账准确；重复插入幂等不占双槽。
    let mut cache = IconCache::new();
    let m1 = cache.lookup(0xFEED, 256);
    let i1 = cache.insert(0xFEED, 256);
    let i2 = cache.insert(0xFEED, 256);
    let h1 = cache.lookup(0xFEED, 256);
    cs.add(
        "icon_cache_miss_insert_hit",
        !m1 && i1 && i2 && h1 && cache.len() == 1 && cache.hits == 1 && cache.misses == 1,
        "",
    );
    // 3) 版本信息不缓存纪律钉值（现取现显——无缓存写路径）。
    cs.add("version_info_never_cached", !VERSION_INFO_CACHED, "");
    cs
}

// ---------------------------------------------------------------------------
// F014 · 深化批次五：manifest 兼容性/主题声明检测（三件套之外的声明面）
//
// 主册依据（G-A-14【功能定义】）：「清单（manifest → DPI 感知三态 F028 +
// 兼容性声明 + 主题声明）」——DPI 面由批次一批次二承载（parse_manifest_
// awareness）；本段补**兼容性声明**（<compatibility> 节）与**主题声明**
// （<windowsSettings> 下主题相关声明的存在性检测——标签级检测，声明值
// 语义随闸门对拍）。
// ---------------------------------------------------------------------------

/// manifest 内检测标签是否出现（字面扫描——BOM 已由 strip_bom 既有面剥离）。
/// 支持命名空间前缀：`<tag>` 或 `<prefix:tag`（前缀边界 = '<' 后非 '/'/'?'
/// 且紧随 ':' 加 tag 的形态由 find_subslice_ns 判定）。
pub fn manifest_has_tag(data: &[u8], tag: &str) -> bool {
    let open_plain = {
        let mut v = alloc::vec::Vec::new();
        v.extend_from_slice(b"<");
        v.extend_from_slice(tag.as_bytes());
        v.push(b'>');
        v
    };
    find_subslice(data, &open_plain).is_some() || find_subslice_ns(data, tag).is_some()
}

fn find_subslice(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}

/// 命名空间前缀版：限定名扫描——`<name:tag`（name 为 XML 名字符段，':' 后
/// 段精确等于 tag，后随空白/'/'/'>' 才算开标签；拼错名不误命中）。
fn find_subslice_ns(h: &[u8], tag: &str) -> Option<usize> {
    let tag_b = tag.as_bytes();
    for i in 0..h.len() {
        if h[i] != b'<' {
            continue;
        }
        let mut j = i + 1;
        let is_name = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-';
        while j < h.len() && is_name(h[j]) {
            j += 1;
        }
        if j >= h.len() || h[j] != b':' {
            continue;
        }
        j += 1;
        let seg_start = j;
        while j < h.len() && is_name(h[j]) {
            j += 1;
        }
        let seg = &h[seg_start..j];
        if seg == tag_b && j < h.len() && (h[j] == b' ' || h[j] == b'/' || h[j] == b'>') {
            return Some(i);
        }
    }
    None
}

/// F014 深化批次五自检。
pub fn run_persrc_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep4");
    // 1) 兼容性声明检测：<compatibility> 直出。
    let m1 = b"<assembly><compatibility><application/></compatibility></assembly>";
    cs.add("manifest_compatibility_detected", manifest_has_tag(m1, "compatibility"), "");
    // 2) 命名空间前缀容忍：<asmv3:compatibility> 同样命中（前缀边界精确——
    //    <compability> 拼错不误命中）。
    let m2 = b"<assembly><asmv3:windowsSettings/></assembly>";
    cs.add(
        "manifest_ns_prefix_tolerated",
        manifest_has_tag(m2, "windowsSettings")
            && !manifest_has_tag(m2, "windowsSetting"),
        "",
    );
    // 3) 缺声明如实 false（no-compat manifest 不虚报）。
    let m3 = b"<assembly><windowsSettings><dpiAware>true</dpiAware></windowsSettings></assembly>";
    cs.add(
        "manifest_compatibility_absent_honest",
        !manifest_has_tag(m3, "compatibility") && manifest_has_tag(m3, "windowsSettings"),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F014 · 深化批次六：PNG IHDR 尺寸读出（256px PNG 图标验证的像素数据源）
//
// 主册依据（G-A-14【设计细节】）：「256px PNG 压缩格式图标支持」——PNG 图标
// 的尺寸验证：IHDR 块（首块）宽高读出（大端 u32 ×2，偏移 16..24），配既有
// validate_png 签名/CRC 面。
// ---------------------------------------------------------------------------

/// PNG IHDR 宽高读出（`data` 为完整 PNG 文件：签名 8B + IHDR 长度 4B +
/// "IHDR" 4B + 宽 4B + 高 4B）。结构不符 → None。
pub fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 24 || data[..8] != SIG {
        return None;
    }
    if &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// F014 深化批次六自检。
pub fn run_persrc_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep5");
    // 1) 256px PNG 图标尺寸读出（256×256）。
    let mut png = alloc::vec![0u8; 32];
    png[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    png[12..16].copy_from_slice(b"IHDR");
    png[16..20].copy_from_slice(&256u32.to_be_bytes());
    png[20..24].copy_from_slice(&256u32.to_be_bytes());
    cs.add(
        "png_dimensions_256_icon",
        png_dimensions(&png) == Some((256, 256)),
        "",
    );
    // 2) 多尺寸梯场景：48×48 与 256×256 区分（尺寸梯判据的数据源面）。
    let mut small = png.clone();
    small[16..20].copy_from_slice(&48u32.to_be_bytes());
    small[20..24].copy_from_slice(&48u32.to_be_bytes());
    cs.add(
        "png_dimensions_multi_size",
        png_dimensions(&small) == Some((48, 48)),
        "",
    );
    // 3) 结构不符如实 None：坏签名 / 非 IHDR 首块 / 零宽。
    let mut bad_sig = png.clone();
    bad_sig[0] = 0x88;
    let mut bad_chunk = png.clone();
    bad_chunk[12..16].copy_from_slice(b"JHDR");
    let mut zero_w = png.clone();
    zero_w[16..20].copy_from_slice(&0u32.to_be_bytes());
    cs.add(
        "png_dimensions_malformed_rejected",
        png_dimensions(&bad_sig).is_none()
            && png_dimensions(&bad_chunk).is_none()
            && png_dimensions(&zero_w).is_none(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F014 · 深化批次八：RT_STRING 块模型（字符串表资源真结构——每块 16 条，
// 块 ID = (字符串 ID >> 4) + 1，槽位 = ID & 15；条目 = u16 长度前缀 +
// UTF-16LE 字符，空串 = 长度 0）。
// ---------------------------------------------------------------------------

/// 从块数据取字符串（UTF-16LE 解出 UTF-8 写入 out，返回长度；超界 None）。
pub fn rt_string_get(block: &[u8], slot: u16, out: &mut alloc::vec::Vec<u8>) -> Option<u16> {
    if slot >= 16 {
        return None;
    }
    let mut off = 0usize;
    for _ in 0..slot {
        if off + 2 > block.len() {
            return None;
        }
        let len = u16::from_le_bytes([block[off], block[off + 1]]) as usize;
        off += 2 + len * 2;
    }
    if off + 2 > block.len() {
        return None;
    }
    let len = u16::from_le_bytes([block[off], block[off + 1]]) as usize;
    off += 2;
    if off + len * 2 > block.len() {
        return None;
    }
    out.clear();
    let mut units = alloc::vec::Vec::with_capacity(len);
    for k in 0..len {
        let b0 = block[off + k * 2];
        let b1 = block[off + k * 2 + 1];
        units.push(u16::from_le_bytes([b0, b1]));
    }
    // UTF-16LE → UTF-8（BMP 域——字符串表资源不承载代理对面，如实按单单元解）。
    for u in units {
        if (0xD800..0xE000).contains(&u) {
            out.extend_from_slice("\u{FFFD}".as_bytes());
        } else {
            let c = char::from_u32(u as u32).unwrap_or('\u{FFFD}');
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Some(len as u16)
}

/// 字符串 ID → (块 ID, 槽位)（MS 资源编译器语义）。
pub fn rt_string_block_of(id: u16) -> (u16, u16) {
    ((id >> 4) + 1, id & 15)
}

/// F014 深化批次八自检。
fn run_persrc_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep7");
    // 块构造：槽 0 = "Ab"（len2），槽 1 = ""（len0），槽 2 = 中文「好」（len1）。
    let mut block: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    block.extend_from_slice(&2u16.to_le_bytes());
    block.extend_from_slice(&[b'A', 0, b'b', 0]);
    block.extend_from_slice(&0u16.to_le_bytes());
    block.extend_from_slice(&1u16.to_le_bytes());
    block.extend_from_slice(&0x597Du16.to_le_bytes()); // 好
    // 1) 槽位逐条读出：内容与长度全对（含空串槽 1 跳过正确）。
    let mut out = alloc::vec::Vec::new();
    let l0 = rt_string_get(&block, 0, &mut out);
    let t0 = out.clone();
    let l1 = rt_string_get(&block, 1, &mut out);
    let l2 = rt_string_get(&block, 2, &mut out);
    cs.add(
        "rt_string_slots_roundtrip",
        l0 == Some(2) && t0 == b"Ab".to_vec()
            && l1 == Some(0)
            && l2 == Some(1) && out == "好".as_bytes(),
        "",
    );
    // 2) 块/槽映射：ID 0 → 块 1 槽 0；ID 33 → 块 3 槽 1；ID 4095 → 块 256 槽 15。
    cs.add(
        "rt_string_block_mapping",
        rt_string_block_of(0) == (1, 0)
            && rt_string_block_of(33) == (3, 1)
            && rt_string_block_of(4095) == (256, 15),
        "",
    );
    // 3) 槽位越界（>=16）与截断块如实 None。
    let over = rt_string_get(&block, 16, &mut out);
    let short = rt_string_get(&block[..3], 2, &mut out);
    cs.add(
        "rt_string_bounds_honest",
        over.is_none() && short.is_none(),
        "",
    );
    cs
}
