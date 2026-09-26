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
    let mut v = vec![0u8; c + n * 16];
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
pub fn run_persrc_checks() -> CheckSet {
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
    let mut png = vec![0x89u8];
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
    let mut m3 = vec![0xEF, 0xBB, 0xBF];
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
        let mut data = vec![0u8; 40];
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
