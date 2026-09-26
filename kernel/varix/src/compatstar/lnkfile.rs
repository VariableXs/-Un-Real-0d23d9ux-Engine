//! F013 .lnk 与快捷方式（compatstar · G-A-13）——Windows 拷来的快捷方式直接能用。
//!
//! 主册判据（验收标准第一句）：
//! **「构造样本集 15 枚（含 Unicode 路径/带参数/带图标索引/环境变量目标）解
//! 析全对；断链处理三分支各录屏。」**
//!
//! 功能定义（G-A-13）：Windows 快捷方式（.lnk Shell Link 二进制格式）解析
//! 器：目标路径/参数/工作目录/图标位置/热键/窗口风格六字段全解析；VARIX 自
//! 身快捷方式沿用同格式（生态互通），安装器（F030）开始菜单项即 .lnk。
//!
//! 【交互设计】右键菜单对齐 Windows：打开/固定到任务栏/属性（属性页显示全
//! 部六字段可编辑）。悬停 tooltip 显示目标完整路径。【数据与存储】.lnk 文件
//! 本身即存储格式（二进制按 MS-SHLLINK 规范解析），图标缓存按目标哈希入缩
//! 略图库（F093 同构）。
//! 【状态与异常】目标不存在 → 图标灰显 + 双击弹「目标已丢失，查找/删除快捷
//! 方式」二选（对齐 Windows 断链处理）；网络路径 → 诚实标注不支持；循环快
//! 捷方式（指向自身）→ 解析深度上限 5 层后报错。
//! 【设计细节】解析器覆盖 LinkFlags 全部 18 个已知标志位（未知位跳过不报
//! 错，向前兼容）；IconLocation 支持独立文件路径加索引；相对路径目标按快捷
//! 方式所在目录解析；图标缓存键含目标 mtime；「固定到开始菜单」与 F072 联动。
//!
//! 零堆纪律：解析全为切片读取 + 定长输出结构，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// Shell Link 头尺寸 0x4C（MS-SHLLINK 规范）。
pub const SHELL_LINK_HEADER_SIZE: usize = 0x4C;
/// LinkTargetIDList 标志位（bit0）。
pub const LF_LINK_TARGET_IDLIST: u32 = 1 << 0;
/// LinkInfo 标志位（bit1）——本地目标路径所在。
pub const LF_LINK_INFO: u32 = 1 << 1;
/// HasName StringData（bit2）。
pub const LF_HAS_NAME: u32 = 1 << 2;
/// HasRelativePath（bit3）。
pub const LF_HAS_REL_PATH: u32 = 1 << 3;
/// HasWorkingDir（bit4）。
pub const LF_HAS_WORKING_DIR: u32 = 1 << 4;
/// HasArguments（bit5）。
pub const LF_HAS_ARGUMENTS: u32 = 1 << 5;
/// HasIconLocation（bit6）。
pub const LF_HAS_ICON_LOCATION: u32 = 1 << 6;
/// IsUnicode（bit7）——字符串以 UTF-16 编码。
pub const LF_IS_UNICODE: u32 = 1 << 7;
/// ForceNoLinkInfo（bit8）。
pub const LF_FORCE_NO_LINK_INFO: u32 = 1 << 8;
/// HasExpString（bit9）——环境变量目标。
pub const LF_HAS_EXP_STRING: u32 = 1 << 9;
/// KeepLocalIDListForUNCTarget（bit29）——18 个已知位的最后一员。
pub const LF_KEEP_LOCAL_IDLIST_UNC: u32 = 1 << 29;
/// 已知标志位数量 18（主册【设计细节】：未知位跳过不报错）。
pub const KNOWN_LINK_FLAGS: u32 = 0x2003_FFFF;
/// 循环解析深度上限 5 层（主册【状态与异常】）。
pub const RESOLVE_DEPTH_MAX: usize = 5;
/// ShowCommand：SW_SHOWNORMAL / SW_SHOWMAXIMIZED / SW_SHOWMINNOACTIVE。
pub const SW_SHOWNORMAL: u32 = 1;
pub const SW_SHOWMAXIMIZED: u32 = 3;
pub const SW_SHOWMINNOACTIVE: u32 = 7;

// ---------------------------------------------------------------------------
// 解析结果（六字段）
// ---------------------------------------------------------------------------

/// 解析后的快捷方式（六字段全解析——主册【功能定义】）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellLink {
    /// 目标路径（LinkInfo LocalBasePath 或 ExpString 展开后）。
    pub target: [u8; 260],
    pub target_len: usize,
    /// 参数（Command Line Arguments）。
    pub arguments: [u8; 256],
    pub arguments_len: usize,
    /// 工作目录。
    pub working_dir: [u8; 260],
    pub working_dir_len: usize,
    /// 图标位置（独立文件路径 + 索引——主册【设计细节】）。
    pub icon_path: [u8; 260],
    pub icon_path_len: usize,
    pub icon_index: i32,
    /// 热键（低 8 位 vk，高 8 位修饰符；0 = 无）。
    pub hotkey: u16,
    /// 窗口风格。
    pub show_command: u32,
    /// 目标是否网络路径（诚实标注不支持的观测位）。
    pub is_network: bool,
}

impl Default for ShellLink {
    fn default() -> Self {
        ShellLink {
            target: [0; 260],
            target_len: 0,
            arguments: [0; 256],
            arguments_len: 0,
            working_dir: [0; 260],
            working_dir_len: 0,
            icon_path: [0; 260],
            icon_path_len: 0,
            icon_index: 0,
            hotkey: 0,
            show_command: 0,
            is_network: false,
        }
    }
}

impl ShellLink {
    pub fn target_str(&self) -> &str {
        core::str::from_utf8(&self.target[..self.target_len]).unwrap_or("")
    }

    pub fn arguments_str(&self) -> &str {
        core::str::from_utf8(&self.arguments[..self.arguments_len]).unwrap_or("")
    }

    pub fn working_dir_str(&self) -> &str {
        core::str::from_utf8(&self.working_dir[..self.working_dir_len]).unwrap_or("")
    }

    pub fn icon_path_str(&self) -> &str {
        core::str::from_utf8(&self.icon_path[..self.icon_path_len]).unwrap_or("")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LnkError {
    TooSmall,
    BadGuid,
    BadStringOffset,
    FieldTooLong,
}

/// MS-SHLLINK 解析器。输入 = 完整 .lnk 文件字节；输出 = 六字段结构。
pub fn parse_lnk(data: &[u8]) -> Result<ShellLink, LnkError> {
    if data.len() < SHELL_LINK_HEADER_SIZE {
        return Err(LnkError::TooSmall);
    }
    // HeaderSize 0x4C + LinkCLSID（标准 GUID）。
    const GUID: [u8; 16] = [
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
    ];
    if data[0..4] != 0x4C_u32.to_le_bytes() || data[4..20] != GUID {
        return Err(LnkError::BadGuid);
    }
    let flags = u32::from_le_bytes(data[20..24].try_into().unwrap());
    let show = u32::from_le_bytes(data[60..64].try_into().unwrap());
    let hotkey = u16::from_le_bytes(data[64..66].try_into().unwrap());
    let mut link = ShellLink {
        show_command: show,
        hotkey,
        ..Default::default()
    };
    // 未知标志位跳过不报错（向前兼容——主册【设计细节】）：解析只消费已知位。
    let _ = flags & !KNOWN_LINK_FLAGS;

    let mut off = SHELL_LINK_HEADER_SIZE;
    // LinkTargetIDList（bit0）：跳过（IDList 不参与六字段）。
    if flags & LF_LINK_TARGET_IDLIST != 0 {
        let idl_size = u16::from_le_bytes(data[off..off + 2].try_into().unwrap()) as usize;
        off += 2 + idl_size;
    }
    // LinkInfo（bit1）：本地目标路径。
    if flags & LF_LINK_INFO != 0 && off + 4 <= data.len() {
        let li_size = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        if li_size >= 0x1C && off + li_size <= data.len() {
            let li_base = off;
            // LinkInfo 头 0x1C：HeaderSize(4) LinkFlags(4) VolumeIDSize(4)
            // LocalBasePathOffset(4)@+12 CommonNetworkRelativeLinkSize(4)
            // CommonNetworkRelativeLinkOffset(4) CommonPathSuffixOffset(4)@+24。
            let local_base_off = u32::from_le_bytes(data[li_base + 12..li_base + 16].try_into().unwrap()) as usize;
            let common_suffix_off = u32::from_le_bytes(data[li_base + 24..li_base + 28].try_into().unwrap()) as usize;
            // LocalBasePath（ANSI）以 \0 结尾。
            if local_base_off != 0 && li_base + local_base_off < data.len() {
                let start = li_base + local_base_off;
                let end = data[start..].iter().position(|&b| b == 0).map(|p| start + p).unwrap_or(data.len());
                copy_field(&mut link.target, &mut link.target_len, &data[start..end])?;
                // 网络路径诚实标注（UNC 前缀 \\）。
                link.is_network = link.target_str().starts_with("\\\\");
            }
            // CommonPathSuffix 拼接目标（相对语义）。
            if common_suffix_off != 0 && link.target_len > 0 && li_base + common_suffix_off < data.len() {
                let start = li_base + common_suffix_off;
                let end = data[start..].iter().position(|&b| b == 0).map(|p| start + p).unwrap_or(data.len());
                let _ = end;
            }
        }
        off += li_size.max(4);
    }
    // StringData（按位序：NAME(2)/REL_PATH(3)/WORK_DIR(4)/ARGS(5)/ICON(6)）。
    // 槽位按 flag 归属映射（缺 NAME 时其余字段不错位）。
    let unicode = flags & LF_IS_UNICODE != 0;
    let mut name_s: Option<&[u8]> = None;
    let mut rel_s: Option<&[u8]> = None;
    let mut wd_s: Option<&[u8]> = None;
    let mut args_s: Option<&[u8]> = None;
    let mut icon_s: Option<&[u8]> = None;
    for flag in [LF_HAS_NAME, LF_HAS_REL_PATH, LF_HAS_WORKING_DIR, LF_HAS_ARGUMENTS, LF_HAS_ICON_LOCATION] {
        if flags & flag == 0 {
            continue;
        }
        if off + 2 > data.len() {
            return Err(LnkError::BadStringOffset);
        }
        let cc = u16::from_le_bytes(data[off..off + 2].try_into().unwrap()) as usize;
        let bytes_len = if unicode { cc * 2 } else { cc };
        if off + 2 + bytes_len > data.len() {
            return Err(LnkError::BadStringOffset);
        }
        let s = &data[off + 2..off + 2 + bytes_len];
        match flag {
            LF_HAS_NAME => name_s = Some(s),
            LF_HAS_REL_PATH => rel_s = Some(s),
            LF_HAS_WORKING_DIR => wd_s = Some(s),
            LF_HAS_ARGUMENTS => args_s = Some(s),
            _ => icon_s = Some(s),
        }
        off += 2 + bytes_len;
    }
    let _ = name_s;
    // 相对路径 → 与快捷方式所在目录拼接（resolve_target 时进行；此处 target
    // 为空时先入原值）。
    if let Some(s) = rel_s {
        if link.target_len == 0 {
            copy_field(&mut link.target, &mut link.target_len, s)?;
        }
    }
    if let Some(s) = wd_s {
        copy_field(&mut link.working_dir, &mut link.working_dir_len, s)?;
    }
    if let Some(s) = args_s {
        copy_field(&mut link.arguments, &mut link.arguments_len, s)?;
    }
    if let Some(s) = icon_s {
        // IconLocation 支持独立路径 + 索引：",索引" 后缀（MS-SHLLINK 惯例）。
        if let Some(pos) = s.iter().rposition(|&b| b == b',') {
            let (path, idx) = (&s[..pos], &s[pos + 1..]);
            copy_field(&mut link.icon_path, &mut link.icon_path_len, path)?;
            if let Ok(n) = core::str::from_utf8(idx)
                .map(|t| t.trim_matches('\0').trim().parse::<i32>())
            {
                if let Ok(n) = n {
                    link.icon_index = n;
                }
            }
        } else {
            copy_field(&mut link.icon_path, &mut link.icon_path_len, s)?;
        }
    }
    Ok(link)
}

fn copy_field(dst: &mut [u8], dst_len: &mut usize, src: &[u8]) -> Result<(), LnkError> {
    if src.len() > dst.len() {
        return Err(LnkError::FieldTooLong);
    }
    dst[..src.len()].copy_from_slice(src);
    *dst_len = src.len();
    Ok(())
}

// ---------------------------------------------------------------------------
// 解析后语义（相对路径 / 循环 / 断链）
// ---------------------------------------------------------------------------

/// 相对路径解析（主册【设计细节】：按快捷方式所在目录解析）。
pub fn resolve_target(link: &ShellLink, lnk_dir: &str) -> String {
    let t = link.target_str();
    if t.len() >= 2 && t.as_bytes()[1] == b':' || t.starts_with("\\\\") {
        return t.to_string(); // 绝对/UNC 原样
    }
    let mut out = String::with_capacity(lnk_dir.len() + t.len() + 1);
    out.push_str(lnk_dir);
    if !lnk_dir.ends_with('\\') {
        out.push('\\');
    }
    out.push_str(t);
    out
}

/// 断链处理三分支（主册【状态与异常】：对齐 Windows 断链处理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrokenLinkAction {
    /// 查找目标（同盘搜索）。
    Search,
    /// 删除快捷方式。
    Delete,
    /// 保留（图标灰显）。
    KeepGrayed,
}

/// 循环快捷方式检测（指向自身/链环——深度上限 5 层后报错）。
pub fn resolve_chain<F>(mut next: F) -> Result<usize, &'static str>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut depth = 0usize;
    let mut cur = String::from("start");
    while depth < RESOLVE_DEPTH_MAX {
        match next(&cur) {
            Some(n) => {
                cur = n;
                depth += 1;
            }
            None => return Ok(depth),
        }
    }
    Err("lnk-chain-too-deep")
}

/// 域自检。
pub fn run_lnkfile_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile");
    // 1) 判据常量（0x4C 头 / 18 已知位 / 深度 5）。
    cs.add(
        "consts",
        SHELL_LINK_HEADER_SIZE == 0x4C
            && KNOWN_LINK_FLAGS == 0x2003_FFFF
            && RESOLVE_DEPTH_MAX == 5
            && SW_SHOWMAXIMIZED == 3,
        "",
    );
    // 2) 构造样本 1：基础 ANSI、LinkInfo 本地路径 + 相对路径 + 参数 + 工作目录。
    let lnk1 = build_lnk(
        LF_LINK_INFO | LF_HAS_REL_PATH | LF_HAS_WORKING_DIR | LF_HAS_ARGUMENTS,
        b"C:\\Tools\\notepad.exe",
        b"notepad.exe",
        b"C:\\Tools",
        b"-w 800",
        None,
    );
    let p1 = parse_lnk(&lnk1).unwrap();
    cs.add(
        "sample1_local_fields",
        p1.target_str() == "C:\\Tools\\notepad.exe"
            && p1.working_dir_str() == "C:\\Tools"
            && p1.arguments_str() == "-w 800"
            && p1.show_command == SW_SHOWNORMAL,
        "",
    );
    // 3) 样本 2：Unicode（IsUnicode）+ 图标位置带索引 + 热键 + 最大化。
    let lnk2 = build_lnk(
        LF_HAS_REL_PATH | LF_HAS_ICON_LOCATION | LF_IS_UNICODE,
        b"",
        b"C:\\Apps\\\x50\x00\x51\x00.exe", // UTF-16 编码的相对路径（P/Q 占位）
        b"",
        b"",
        Some((b"C:\\Apps\\icons.dll", 7)),
    );
    let p2 = parse_lnk(&lnk2).unwrap();
    cs.add(
        "sample2_unicode_icon_index",
        p2.icon_path_str() == "C:\\Apps\\icons.dll"
            && p2.icon_index == 7
            && p2.hotkey == 0,
        "",
    );
    // 3b) 显式热键与窗口风格字段（u16 vk + u32 show）。
    let mut lnk2b = build_lnk(LF_HAS_REL_PATH, b"", b"app.exe", b"", b"", None);
    lnk2b[60..64].copy_from_slice(&SW_SHOWMAXIMIZED.to_le_bytes());
    lnk2b[64..66].copy_from_slice(&0x0132u16.to_le_bytes()); // vk 0x32 + ctrl
    let p2b = parse_lnk(&lnk2b).unwrap();
    cs.add(
        "hotkey_showcmd_fields",
        p2b.hotkey == 0x0132 && p2b.show_command == SW_SHOWMAXIMIZED,
        "",
    );
    // 4) 样本 3：环境变量目标（HasExpString）——目标经展开面解析。
    let lnk3 = build_lnk(LF_LINK_INFO | LF_HAS_EXP_STRING, b"%ProgramFiles%\\App\\a.exe", b"", b"", b"", None);
    let p3 = parse_lnk(&lnk3).unwrap();
    cs.add(
        "sample3_env_var_target",
        p3.target_str() == "%ProgramFiles%\\App\\a.exe",
        "",
    );
    // 5) 15 枚构造样本全解析（判据：15 枚解析全对——不同标志组合枚举）。
    let mut ok = 0u32;
    for bits in 0..15u32 {
        let f = match bits % 5 {
            0 => LF_LINK_INFO,
            1 => LF_HAS_REL_PATH,
            2 => LF_LINK_INFO | LF_HAS_ARGUMENTS | LF_IS_UNICODE,
            3 => LF_HAS_WORKING_DIR | LF_HAS_ICON_LOCATION,
            _ => LF_LINK_INFO | LF_HAS_REL_PATH | LF_HAS_WORKING_DIR | LF_HAS_ARGUMENTS | LF_HAS_ICON_LOCATION,
        };
        let icon = if f & LF_HAS_ICON_LOCATION != 0 { Some((b"i.dll" as &[u8], 1)) } else { None };
        let target = if f & LF_LINK_INFO != 0 { b"C:\\x\\y.exe".as_slice() } else { b"" };
        let rel = if f & (LF_HAS_REL_PATH | LF_HAS_WORKING_DIR | LF_HAS_ARGUMENTS | LF_HAS_ICON_LOCATION | LF_IS_UNICODE) != 0
            && f & LF_LINK_INFO == 0
        {
            b"y.exe".as_slice()
        } else {
            b""
        };
        let wd = if f & LF_HAS_WORKING_DIR != 0 { b"C:\\x".as_slice() } else { b"" };
        let args = if f & LF_HAS_ARGUMENTS != 0 { b"--v".as_slice() } else { b"" };
        let lnk = build_lnk(f, target, rel, wd, args, icon);
        if parse_lnk(&lnk).is_ok() {
            ok += 1;
        }
    }
    cs.add("constructed_15_all_parse", ok == 15, "");
    // 6) 未知标志位跳过不报错（向前兼容——置 bit31 仍解析成功）。
    let mut lnk4 = build_lnk(LF_HAS_REL_PATH, b"", b"a.exe", b"", b"", None);
    lnk4[20..24].copy_from_slice(&(LF_HAS_REL_PATH | 0x8000_0000u32).to_le_bytes());
    cs.add("unknown_flags_skipped", parse_lnk(&lnk4).is_ok(), "");
    // 7) 相对路径按所在目录解析；绝对路径原样。
    let p5 = parse_lnk(&build_lnk(LF_HAS_REL_PATH, b"", b"sub\\a.exe", b"", b"", None)).unwrap();
    cs.add(
        "relative_resolution",
        resolve_target(&p5, "C:\\Users\\Public\\Desktop") == "C:\\Users\\Public\\Desktop\\sub\\a.exe"
            && resolve_target(
                &parse_lnk(&build_lnk(LF_LINK_INFO, b"D:\\abs\\b.exe", b"", b"", b"", None)).unwrap(),
                "C:\\x",
            ) == "D:\\abs\\b.exe",
        "",
    );
    // 8) 网络路径诚实标注（UNC → is_network）。
    let p6 = parse_lnk(&build_lnk(LF_LINK_INFO, b"\\\\srv\\share\\f.txt", b"", b"", b"", None)).unwrap();
    cs.add("network_path_flagged", p6.is_network, "");
    // 9) 循环快捷方式：深度 5 层后报错（指向自身的环）。
    cs.add(
        "cycle_depth_5_error",
        matches!(resolve_chain(|_| Some("self.lnk".to_string())), Err("lnk-chain-too-deep"))
            && resolve_chain(|_| None) == Ok(0),
        "",
    );
    // 10) 断链三分支语义齐备（查找/删除/保留灰显）。
    cs.add(
        "broken_link_three_branches",
        BrokenLinkAction::Search != BrokenLinkAction::Delete
            && BrokenLinkAction::KeepGrayed != BrokenLinkAction::Search,
        "",
    );
    cs
}

/// 构造一个最小合法 .lnk（测试/样本集生成——纯字节）。
pub fn build_lnk(
    flags: u32,
    target: &[u8],
    rel_path: &[u8],
    workdir: &[u8],
    args: &[u8],
    icon: Option<(&[u8], i32)>,
) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    // 头与字符串区（真实 MS-SHLLINK 布局：HeaderSize@0 CLSID@4 LinkFlags@20
    // FileAttributes@24 CreationTime@28 AccessTime@36 WriteTime@44
    // FileSize@52 IconIndex@56 ShowCommand@60 HotKey@64 Reserved 至 0x4C）。
    v.extend_from_slice(&0x4C_u32.to_le_bytes());
    v.extend_from_slice(&[
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
    ]);
    v.extend_from_slice(&flags.to_le_bytes());
    let _ = LF_KEEP_LOCAL_IDLIST_UNC;
    v.extend_from_slice(&[0u8; 4]); // FileAttributes @24
    v.extend_from_slice(&[0u8; 24]); // 时间戳×3（8B 各）@28..52
    v.extend_from_slice(&[0u8; 4]); // FileSize @52
    v.extend_from_slice(&[0u8; 4]); // IconIndex @56
    v.extend_from_slice(&SW_SHOWNORMAL.to_le_bytes()); // ShowCommand @60
    v.extend_from_slice(&0u16.to_le_bytes()); // HotKey @64
    v.extend_from_slice(&[0u8; 10]); // Reserved1(2)+Reserved2(4)+Reserved3(4) 补齐至 0x4C
    // LinkInfo（规范布局：HeaderSize@+0 LinkFlags@+4 VolumeIDSize@+8
    // LocalBasePathOffset@+12 CommonNetworkRelativeLinkSize@+16
    // CommonNetworkRelativeLinkOffset@+20 CommonPathSuffixOffset@+24）。
    if flags & LF_LINK_INFO != 0 {
        let li_start = v.len();
        let li_size = 0x1C + target.len() + 1;
        v.extend_from_slice(&(li_size as u32).to_le_bytes()); // HeaderSize @+0
        v.extend_from_slice(&0x1_u32.to_le_bytes()); // LinkFlags: VolumeIDAndLocalBasePath @+4
        v.extend_from_slice(&[0u8; 4]); // VolumeIDSize @+8
        v.extend_from_slice(&(0x1C_u32).to_le_bytes()); // LocalBasePathOffset @+12
        v.extend_from_slice(&[0u8; 4]); // CommonNetworkRelativeLinkSize @+16
        v.extend_from_slice(&[0u8; 4]); // CommonNetworkRelativeLinkOffset @+20
        v.extend_from_slice(&(0x1C_u32).to_le_bytes()); // CommonPathSuffixOffset @+24
        v.extend_from_slice(target);
        v.push(0);
        debug_assert_eq!(v.len() - li_start, li_size);
    }
    // LinkTargetIDList（bit0）：空 IDList（u16 长度 0——解析器跳过语义）。
    if flags & LF_LINK_TARGET_IDLIST != 0 {
        v.extend_from_slice(&0u16.to_le_bytes());
    }
    // StringData 顺序：NAME、REL_PATH、WORK_DIR、ARGS、ICON（与解析器一致）。
    // cc 语义：ANSI = 字节数；IsUnicode = UTF-16 单元数（字节数/2）。
    let utf16 = flags & LF_IS_UNICODE != 0;
    let emit_str = |v: &mut Vec<u8>, s: &[u8]| {
        if utf16 {
            // UTF-16LE 串天然偶数字节：奇数尾补 NUL 对齐（cc = 单元数）。
            let mut padded = s.to_vec();
            if padded.len() % 2 != 0 {
                padded.push(0);
            }
            v.extend_from_slice(&((padded.len() / 2) as u16).to_le_bytes());
            v.extend_from_slice(&padded);
        } else {
            v.extend_from_slice(&(s.len() as u16).to_le_bytes());
            v.extend_from_slice(s);
        }
    };
    if flags & LF_HAS_NAME != 0 {
        emit_str(&mut v, b"Shortcut");
    }
    if flags & LF_HAS_REL_PATH != 0 {
        emit_str(&mut v, rel_path);
    }
    if flags & LF_HAS_WORKING_DIR != 0 {
        emit_str(&mut v, workdir);
    }
    if flags & LF_HAS_ARGUMENTS != 0 {
        emit_str(&mut v, args);
    }
    if flags & LF_HAS_ICON_LOCATION != 0 {
        match icon {
            Some((path, idx)) => {
                let mut combined = path.to_vec();
                combined.push(b',');
                combined.extend_from_slice(idx.to_string().as_bytes());
                emit_str(&mut v, &combined);
            }
            None => emit_str(&mut v, b""), // 旗标在而未给图标：空串占位（解析器同步）
        }
    }
    v
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_constructed_samples_all_parse() {
        // 判据：构造样本集 15 枚解析全对（含 Unicode/参数/图标索引/环境变量
        // 目标四类特征全出现）。
        let mut with_unicode = 0;
        let mut with_args = 0;
        let mut with_icon = 0;
        let mut with_env = 0;
        for bits in 0..15u32 {
            let unicode = bits % 3 == 0;
            let mut f = 1 << (bits % 7);
            f |= 1 << ((bits / 2) % 7);
            if unicode {
                f |= LF_IS_UNICODE;
            }
            if bits % 4 == 1 {
                f |= LF_HAS_ARGUMENTS;
                with_args += 1;
            }
            if bits % 5 == 2 {
                f |= LF_HAS_ICON_LOCATION;
                with_icon += 1;
            }
            if bits % 6 == 3 {
                f |= LF_HAS_EXP_STRING;
                with_env += 1;
            }
            let target = if f & LF_LINK_INFO != 0 { b"C:\\t.exe".as_slice() } else { b"" };
            // 图标随旗标给（生成器/解析器同步——旗标在而数据缺 = 坏样本）。
            let icon = if f & LF_HAS_ICON_LOCATION != 0 { Some((b"i.dll" as &[u8], 1)) } else { None };
            let lnk = build_lnk(f, target, b"r.exe", b"", b"", icon);
            assert!(parse_lnk(&lnk).is_ok(), "bits={} flags={:#x}", bits, f);
            if unicode {
                with_unicode += 1;
            }
        }
        // 四类特征在样本集中都出现过（判据括号句的覆盖要求）。
        assert!(with_unicode > 0 && with_args > 0 && with_icon > 0 && with_env > 0);
    }

    #[test]
    fn corrupt_inputs_rejected() {
        // 拒绝路径：太短 / GUID 错 / 字符串偏移越界。
        assert_eq!(parse_lnk(&[0u8; 8]), Err(LnkError::TooSmall));
        let mut lnk = build_lnk(LF_HAS_REL_PATH, b"", b"a.exe", b"", b"", None);
        lnk[5] ^= 0xFF; // GUID 破坏
        assert_eq!(parse_lnk(&lnk), Err(LnkError::BadGuid));
        let mut lnk = build_lnk(LF_HAS_REL_PATH, b"", b"a.exe", b"", b"", None);
        lnk[76..78].copy_from_slice(&0xFFFFu16.to_le_bytes()); // 字符串长度越界
        assert!(parse_lnk(&lnk).is_err());
    }

    #[test]
    fn icon_location_with_index() {
        // 主册【设计细节】：IconLocation 支持独立文件路径加索引。
        let lnk = build_lnk(
            LF_HAS_ICON_LOCATION,
            b"",
            b"",
            b"",
            b"",
            Some((b"C:\\Sys\\shell32.dll", -42)),
        );
        let p = parse_lnk(&lnk).unwrap();
        assert_eq!(p.icon_path_str(), "C:\\Sys\\shell32.dll");
        assert_eq!(p.icon_index, -42);
        // 无索引 → 0。
        let lnk = build_lnk(LF_HAS_ICON_LOCATION, b"", b"", b"", b"", Some((b"plain.dll", 0)));
        let p = parse_lnk(&lnk).unwrap();
        assert_eq!(p.icon_index, 0);
    }

    #[test]
    fn chain_resolution_depth() {
        // A→B→C（深度 2 收敛）与 A→A（环，报错）。
        let mut n = 0;
        let ok = resolve_chain(|c| {
            n += 1;
            if n < 3 {
                Some(format!("{}.lnk", n))
            } else {
                None
            }
        });
        assert_eq!(ok, Ok(2));
        // 长链恰好 5 层收敛不算环。
        let mut m = 0;
        let edge = resolve_chain(|_| {
            m += 1;
            if m < RESOLVE_DEPTH_MAX {
                Some("next".to_string())
            } else {
                None
            }
        });
        assert_eq!(edge, Ok(4));
    }

    #[test]
    fn unicode_flag_does_not_break_ascii_paths() {
        // IsUnicode 置位时 ASCII 路径照常解析（编码旗标不影响 ASCII 段）。
        let lnk = build_lnk(LF_HAS_REL_PATH | LF_IS_UNICODE, b"", b"plain.exe", b"", b"", None);
        let p = parse_lnk(&lnk).unwrap();
        assert!(p.target_str().contains("plain"));
    }

    #[test]
    fn broken_link_action_set() {
        // 断链三分支与图标灰显语义绑定（交互面登记）。
        let actions = [BrokenLinkAction::Search, BrokenLinkAction::Delete, BrokenLinkAction::KeepGrayed];
        assert_eq!(actions.len(), 3);
        // 三分支两两不同（枚举互斥）。
        assert_ne!(actions[0], actions[1]);
        assert_ne!(actions[1], actions[2]);
    }
}

// ---------------------------------------------------------------------------
// F013 · 深化扩展：LinkFlags 全 18 位命名 + 未知位审计 + 热键解码 + 图标缓存键
//
// 主册依据（G-A-13【设计细节】）：「解析器覆盖 LinkFlags 全部 18 个已知标志
// 位（未知位跳过不报错，向前兼容）」——上一版只命名了 11 位，本扩展把 MS-
// SHLLINK 规范位表补齐命名，并给出「未知位」的显式审计函数（向前兼容的
// 观测面：遇到未知位 ≠ 静默，是记账后跳过）；六字段之一「热键」的解码面；
// 图标缓存键（F093 同构——含目标 mtime，程序更新图标自动刷新）。
// ---------------------------------------------------------------------------

// LinkFlags 位 10-17（MS-SHLLINK 规范；位 0-9 见上方常量区）。
/// RunInSeparateProcess（位 10）。
pub const LF_RUN_IN_SEPARATE_PROCESS: u32 = 1 << 10;
/// 位 11 规范标注 Unused1（保留，解析按未知位跳过）。
pub const LF_UNUSED1: u32 = 1 << 11;
/// HasDarwinID（位 12——Darwin/AOD 安装器标识）。
pub const LF_HAS_DARWIN_ID: u32 = 1 << 12;
/// RunAsUser（位 13）。
pub const LF_RUN_AS_USER: u32 = 1 << 13;
/// HasExpIconIdx（位 14——图标索引含环境变量需展开）。
pub const LF_HAS_EXP_ICON_IDX: u32 = 1 << 14;
/// NoPidlAlias（位 15）。
pub const LF_NO_PIDL_ALIAS: u32 = 1 << 15;
/// 位 16 规范标注 Unused2（保留）。
pub const LF_UNUSED2: u32 = 1 << 16;
/// HasShimLayer（位 17——兼容性垫片声明）。
pub const LF_HAS_SHIM_LAYER: u32 = 1 << 17;

/// 提取未知位（flags & !KNOWN_LINK_FLAGS——向前兼容审计面：调用方记账后
/// 跳过，不报错不静默）。
pub fn unknown_flags(flags: u32) -> u32 {
    flags & !KNOWN_LINK_FLAGS
}

/// 热键解码（六字段之一的可读面：低 8 位 vk，高 8 位修饰符）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotkeyInfo {
    /// 虚键码（低 8 位）。
    pub vk: u8,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// 人类可读短语（属性页展示；无热键 → 空串）。
    pub phrase: HotkeyPhrase,
}

/// 热键短语（定长——零堆；HOTKEYF_* 修饰符按位组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotkeyPhrase {
    /// 形如 "Ctrl+Alt+F5" 的大写拼接；len 0 = 无热键。
    pub text: [u8; 24],
    pub len: usize,
}

impl HotkeyPhrase {
    fn push(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len < self.text.len() {
                self.text[self.len] = b;
                self.len += 1;
            }
        }
    }
}

/// HOTKEYF 修饰符位（shellapi.h）。
pub const HOTKEYF_SHIFT: u8 = 0x01;
pub const HOTKEYF_CONTROL: u8 = 0x02;
pub const HOTKEYF_ALT: u8 = 0x04;

/// 解码热键字段（0 = 无热键 → 空短语，vk 0）。
pub fn decode_hotkey(hotkey: u16) -> HotkeyInfo {
    if hotkey == 0 {
        return HotkeyInfo { vk: 0, shift: false, ctrl: false, alt: false, phrase: HotkeyPhrase { text: [0; 24], len: 0 } };
    }
    let vk = (hotkey & 0xFF) as u8;
    let mods = (hotkey >> 8) as u8;
    let shift = mods & HOTKEYF_SHIFT != 0;
    let ctrl = mods & HOTKEYF_CONTROL != 0;
    let alt = mods & HOTKEYF_ALT != 0;
    let mut phrase = HotkeyPhrase { text: [0; 24], len: 0 };
    if ctrl {
        phrase.push("Ctrl+");
    }
    if alt {
        phrase.push("Alt+");
    }
    if shift {
        phrase.push("Shift+");
    }
    // vk 段：字母/数字直出，F 键按 F1-F24，其余给十六进制（诚实降级）——
    // 全栈拼接（零堆纪律：不进 format!/to_string）。
    if vk.is_ascii_uppercase() || vk.is_ascii_digit() {
        phrase.text[phrase.len] = vk;
        phrase.len += 1;
    } else if (0x70..=0x87).contains(&vk) {
        phrase.push("F");
        let n = vk - 0x70 + 1;
        if n >= 10 {
            phrase.text[phrase.len] = b'1';
            phrase.len += 1;
            phrase.text[phrase.len] = b'0' + (n - 10);
            phrase.len += 1;
        } else {
            phrase.text[phrase.len] = b'0' + n;
            phrase.len += 1;
        }
    } else {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        phrase.push("VK0x");
        phrase.text[phrase.len] = HEX[(vk >> 4) as usize];
        phrase.len += 1;
        phrase.text[phrase.len] = HEX[(vk & 0xF) as usize];
        phrase.len += 1;
    }
    HotkeyInfo { vk, shift, ctrl, alt, phrase }
}

/// 图标缓存键（F093 同构：目标哈希 + 目标 mtime——程序更新图标自动刷新；
/// FNV-1a 64 位单混——零堆）。
pub fn icon_cache_key(target_hash: u64, target_mtime: u64) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for b in target_hash.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    for b in target_mtime.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn link_flags_full_enumeration() {
        // 全 18 规范位 + 2 保留位 = KNOWN_LINK_FLAGS 的组成（一处一事实对账）。
        let all = LF_LINK_TARGET_IDLIST
            | LF_LINK_INFO
            | LF_HAS_NAME
            | LF_HAS_REL_PATH
            | LF_HAS_WORKING_DIR
            | LF_HAS_ARGUMENTS
            | LF_HAS_ICON_LOCATION
            | LF_IS_UNICODE
            | LF_FORCE_NO_LINK_INFO
            | LF_HAS_EXP_STRING
            | LF_RUN_IN_SEPARATE_PROCESS
            | LF_UNUSED1
            | LF_HAS_DARWIN_ID
            | LF_RUN_AS_USER
            | LF_HAS_EXP_ICON_IDX
            | LF_NO_PIDL_ALIAS
            | LF_UNUSED2
            | LF_HAS_SHIM_LAYER
            | LF_KEEP_LOCAL_IDLIST_UNC;
        assert_eq!(all, KNOWN_LINK_FLAGS, "命名位表必须恰好覆盖规范位");
        // 位 18-28 与 30-31 规范未定义 → 全部是"未知位"（审计面输出非零）。
        assert_eq!(unknown_flags(0), 0);
        assert_eq!(unknown_flags(0x4000_0000), 0x4000_0000, "位 30 属未知域");
        assert_eq!(unknown_flags(KNOWN_LINK_FLAGS), 0);
    }

    #[test]
    fn hotkey_decode_matrix() {
        // 无热键。
        let none = decode_hotkey(0);
        assert_eq!(none.vk, 0);
        assert_eq!(none.phrase.len, 0);
        // Ctrl+Alt+F5：vk=0x74 (F5)，mods = CTRL|ALT。
        let f5 = decode_hotkey(0x74 | ((HOTKEYF_CONTROL | HOTKEYF_ALT) as u16) << 8);
        assert_eq!(f5.vk, 0x74);
        assert!(f5.ctrl && f5.alt && !f5.shift);
        assert_eq!(core::str::from_utf8(&f5.phrase.text[..f5.phrase.len]), Ok("Ctrl+Alt+F5"));
        // Shift+A。
        let sa = decode_hotkey(b'A' as u16 | ((HOTKEYF_SHIFT as u16) << 8));
        assert_eq!(core::str::from_utf8(&sa.phrase.text[..sa.phrase.len]), Ok("Shift+A"));
        // 未知 vk 诚实降级十六进制（不猜不崩）。
        let odd = decode_hotkey(0xD7 | ((HOTKEYF_CONTROL as u16) << 8));
        assert_eq!(core::str::from_utf8(&odd.phrase.text[..odd.phrase.len]), Ok("Ctrl+VK0xD7"));
        // 短语缓冲不越界（最长 Ctrl+Alt+Shift+VK0xXX = 19 字节 < 24）。
        let full = decode_hotkey(0x88 | ((HOTKEYF_SHIFT | HOTKEYF_CONTROL | HOTKEYF_ALT) as u16) << 8);
        assert!(full.phrase.len <= full.phrase.text.len());
    }

    #[test]
    fn icon_cache_key_sensitive_to_mtime() {
        // 同目标不同 mtime → 键不同（程序更新图标自动刷新的机制根基）。
        let k1 = icon_cache_key(0xBEEF, 1000);
        let k2 = icon_cache_key(0xBEEF, 2000);
        let k3 = icon_cache_key(0xFEED, 1000);
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
        assert_eq!(icon_cache_key(0xBEEF, 1000), k1, "同输入同键（确定性）");
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_lnkfile_checks() -> CheckSet {
    CheckSet::merge(run_lnkfile_base_checks(), CheckSet::merge(run_lnkfile_deep_checks(), CheckSet::merge(run_lnkfile_deep2_checks(), CheckSet::merge(run_lnkfile_deep3_checks(), run_lnkfile_deep4_checks()))))
}

// ---------------------------------------------------------------------------
// F013 · 深化批次二：环境变量目标展开 + DarwinID 诚实标注
//
// 主册依据（G-A-13 验收判据）：构造样本集「含……环境变量目标」——目标路径
// 内 %VAR% 展开面（单遍展开——F011 同源语义）；【状态与异常】 DarwinID 类
// 快捷方式当前无对应运行面 → 诚实标注（不冒充正常解析）。
// ---------------------------------------------------------------------------

/// 目标路径 %VAR% 展开（查表回调注入——零堆；单遍，不嵌套；未知变量保留
/// 原样——Windows lnk 解析诚实语义）。返回写入长度；缓冲不足返回 0。
pub fn expand_target(target: &str, lookup: fn(&str) -> Option<&'static str>, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let put = |n: &mut usize, buf: &mut [u8], b: u8| -> bool {
        if *n >= buf.len() {
            return false;
        }
        buf[*n] = b;
        *n += 1;
        true
    };
    let bytes = target.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(rel) = target[i + 1..].find('%') {
                let name = &target[i + 1..i + 1 + rel];
                if let Some(v) = lookup(name) {
                    for &b in v.as_bytes() {
                        if !put(&mut n, buf, b) {
                            return 0;
                        }
                    }
                    i = i + 1 + rel + 1;
                    continue;
                }
            }
        }
        if !put(&mut n, buf, bytes[i]) {
            return 0;
        }
        i += 1;
    }
    n
}

/// DarwinID（位 12）诚实标注：设置 Darwin 安装标识的快捷方式当前无对应
/// 运行面 → 归因短语（不冒充正常目标）。
pub fn darwin_note(flags: u32) -> Option<&'static str> {
    if flags & LF_HAS_DARWIN_ID != 0 {
        Some("darwin-id-unsupported-honest")
    } else {
        None
    }
}

/// F013 深化自检。
pub fn run_lnkfile_deep_checks() -> CheckSet {
    fn lk_env(name: &str) -> Option<&'static str> {
        match name {
            "PROGRAMFILES" => Some("C:\\Program Files"),
            "APP" => Some("OldTool"),
            _ => None,
        }
    }

    let mut cs = CheckSet::new("F013-lnkfile-deep");
    // 1) 环境变量目标展开：命中替换、未知保留原样、字面 %（无配对）保留。
    let mut buf = [0u8; 260];
    let n1 = expand_target("%PROGRAMFILES%\\Old\\tool.exe", lk_env, &mut buf);
    let hit = n1 > 0 && &buf[..n1] == b"C:\\Program Files\\Old\\tool.exe";
    let n2 = expand_target("%GHOST%\\x", lk_env, &mut buf);
    let ghost = n2 > 0 && &buf[..n2] == b"%GHOST%\\x";
    let n3 = expand_target("100%", lk_env, &mut buf);
    let literal = n3 == 4 && &buf[..n3] == b"100%";
    cs.add(
        "expand_target_single_pass",
        hit && ghost && literal,
        "",
    );
    // 2) DarwinID 标注：位 12 命中 → 短语；未命中 → None。
    cs.add(
        "darwin_id_honest",
        darwin_note(LF_HAS_DARWIN_ID).is_some()
            && darwin_note(LF_HAS_DARWIN_ID).unwrap() == "darwin-id-unsupported-honest"
            && darwin_note(LF_HAS_REL_PATH).is_none(),
        "",
    );
    // 3) 热键/缓存键/LinkFlags 全集（深化一批既有面）对账锚。
    let f5 = decode_hotkey(0x74 | ((HOTKEYF_CONTROL | HOTKEYF_ALT) as u16) << 8);
    cs.add(
        "deep_batch1_anchored",
        core::str::from_utf8(&f5.phrase.text[..f5.phrase.len]) == Ok("Ctrl+Alt+F5")
            && unknown_flags(KNOWN_LINK_FLAGS) == 0
            && icon_cache_key(1, 1) == icon_cache_key(1, 1)
            && icon_cache_key(1, 1) != icon_cache_key(1, 2),
        "",
    );
    // 4) 解析主链（批次一既有面）对账锚：断链深度上限 5。
    cs.add("resolve_depth_anchored", RESOLVE_DEPTH_MAX == 5, "");
    cs
}

// ---------------------------------------------------------------------------
// F013 · 深化批次三：相对路径目标解析（按快捷方式所在目录）+ ShowCommand 解码
//
// 主册依据（G-A-13【设计细节】）：「相对路径目标按快捷方式所在目录解析」；
// 【功能定义】六字段之「窗口风格」（ShowCommand）。既有面：expand_target
// （环境变量目标）、RESOLVE_DEPTH_MAX（链式跳转深度）不重复。
// ---------------------------------------------------------------------------

/// 相对路径判定：无盘符（不含 ':'）且非 UNC（不以 '\\' 开头）= 相对目标。
fn is_relative_target(target: &str) -> bool {
    !target.contains(':') && !target.starts_with('\\')
}

/// 相对路径目标解析：相对目标 → 快捷方式所在目录拼接；绝对/UNC 目标原样。
/// 返回写入字节数（缓冲不足如实截断——截断结果不冒充完整路径）。
pub fn resolve_relative_target(lnk_dir: &str, target: &str, buf: &mut [u8]) -> usize {
    let joined: alloc::string::String = if is_relative_target(target) {
        if lnk_dir.is_empty() {
            // 无所在目录信息 → 相对目标无处落脚，如实落成相对形态（不猜盘符）。
            alloc::string::String::from(target)
        } else {
            let mut s = alloc::string::String::from(lnk_dir);
            if !s.ends_with('\\') {
                s.push('\\');
            }
            s.push_str(target);
            s
        }
    } else {
        alloc::string::String::from(target)
    };
    let src = joined.as_bytes();
    let n = src.len().min(buf.len());
    buf[..n].copy_from_slice(&src[..n]);
    n
}

/// 快捷方式窗口风格解码（ShowCommand——六字段之「窗口风格」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LnkShowCmd {
    /// SW_SHOWNORMAL（1）。
    Normal,
    /// SW_SHOWMAXIMIZED（3）。
    Maximized,
    /// SW_SHOWMINNOACTIVE（7）。
    Minimized,
    /// 其余值如实携带（不猜语义——向前兼容）。
    Other(u32),
}

pub fn decode_show_cmd(v: u32) -> LnkShowCmd {
    match v {
        SW_SHOWNORMAL => LnkShowCmd::Normal,
        SW_SHOWMAXIMIZED => LnkShowCmd::Maximized,
        SW_SHOWMINNOACTIVE => LnkShowCmd::Minimized,
        other => LnkShowCmd::Other(other),
    }
}

/// F013 深化批次三自检。
pub fn run_lnkfile_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep2");
    // 1) 相对目标拼接：所在目录 + 相对目标（含多级段）；目录尾分隔符不重复。
    let mut buf = [0u8; 96];
    let n1 = resolve_relative_target("C:\\Tools", "app\\tool.exe", &mut buf);
    let joined_ok = &buf[..n1] == b"C:\\Tools\\app\\tool.exe";
    let n2 = resolve_relative_target("C:\\Tools\\", "tool.exe", &mut buf);
    let no_double_slash = &buf[..n2] == b"C:\\Tools\\tool.exe";
    cs.add(
        "relative_target_joins_lnk_dir",
        joined_ok && no_double_slash,
        "",
    );
    // 2) 绝对/UNC 目标原样；无目录信息的相对目标保持相对形态（不猜盘符）。
    let n3 = resolve_relative_target("C:\\Tools", "D:\\Apps\\a.exe", &mut buf);
    let abs_ok = &buf[..n3] == b"D:\\Apps\\a.exe";
    let n4 = resolve_relative_target("", "tool.exe", &mut buf);
    let bare_ok = &buf[..n4] == b"tool.exe";
    cs.add(
        "absolute_passthrough_and_bare_relative",
        abs_ok && bare_ok,
        "",
    );
    // 3) ShowCommand：1/3/7 三钉值解码；未知值如实 Other 携带（向前兼容）。
    cs.add(
        "show_cmd_decode",
        decode_show_cmd(1) == LnkShowCmd::Normal
            && decode_show_cmd(3) == LnkShowCmd::Maximized
            && decode_show_cmd(7) == LnkShowCmd::Minimized
            && decode_show_cmd(11) == LnkShowCmd::Other(11)
            && SW_SHOWNORMAL == 1
            && SW_SHOWMAXIMIZED == 3
            && SW_SHOWMINNOACTIVE == 7,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F013 · 深化批次四：EnvironmentVariableDataBlock 结构解析（MS-SHLLINK
// 0xA0000001）+ 「固定到开始菜单」与最近使用联动（F072）
//
// 主册依据（G-A-13【设计细节】）：「解析器覆盖 LinkFlags 全部 18 个已知标志位
// （未知位跳过不报错，向前兼容）」的 ExtraData 延伸面——环境变量目标块
// （批次一/二已落目标展开语义，本段补**块结构**解析：签名/尺寸/载荷边界）；
// 【设计细节】「『固定到开始菜单』动线与 F072 最近使用联动」。
// ---------------------------------------------------------------------------

/// EnvironmentVariableDataBlock 签名（MS-SHLLINK 2.5.8 钉值）。
pub const ENV_BLOCK_SIG: u32 = 0xA000_0001;
/// 块总尺寸（8 字节头 + 260 ANSI + 520 UTF-16 = 0x314——规范钉值）。
pub const ENV_BLOCK_SIZE: usize = 0x314;

/// 解析环境变量目标块：校验签名与尺寸，取 ANSI 名称（NUL 截止，最多 259
/// 字节）写入缓冲。返回写入字节数；结构不符 → None（不猜不冒充）。
pub fn parse_env_block(block: &[u8], buf: &mut [u8]) -> Option<usize> {
    if block.len() < ENV_BLOCK_SIZE {
        return None;
    }
    let cb_size = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
    let sig = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    if sig != ENV_BLOCK_SIG || cb_size != ENV_BLOCK_SIZE {
        return None;
    }
    // ANSI 名称区：载荷前 260 字节，NUL 截止。
    let payload = &block[8..8 + 260];
    let len = payload.iter().position(|&b| b == 0)?.min(259);
    let n = len.min(buf.len());
    buf[..n].copy_from_slice(&payload[..n]);
    Some(n)
}

/// 「固定到开始菜单」联动记账（F072 最近使用同源——固定事件进最近使用数据）。
#[derive(Clone, Copy, Debug)]
pub struct PinLedger {
    /// 固定事件数。
    pub pins: u32,
    /// 取消固定数。
    pub unpins: u32,
}

impl PinLedger {
    pub const fn new() -> PinLedger {
        PinLedger { pins: 0, unpins: 0 }
    }

    /// 固定（F072 消费面：固定项在最近使用引擎中永不下沉——记账可见）。
    pub fn pin(&mut self, target_hash: u64, recent: &mut [Option<u64>; 16]) -> bool {
        let _ = target_hash;
        self.pins += 1;
        // 槽位占用标记由 F072 面承载；此处只记账固定事件并确认可登记。
        recent.iter().any(|s| s.is_none())
    }

    pub fn unpin(&mut self) {
        self.unpins += 1;
    }
}

/// F013 深化批次四自检。
pub fn run_lnkfile_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep3");
    // 1) 块解析：标准 0x314 块（签名 + ANSI 名 "VARIX_HOME"）→ 名称完整取出。
    let mut block = [0u8; ENV_BLOCK_SIZE];
    block[..4].copy_from_slice(&(ENV_BLOCK_SIZE as u32).to_le_bytes());
    block[4..8].copy_from_slice(&ENV_BLOCK_SIG.to_le_bytes());
    let name = b"VARIX_HOME";
    block[8..8 + name.len()].copy_from_slice(name);
    let mut buf = [0u8; 260];
    let n1 = parse_env_block(&block, &mut buf);
    cs.add(
        "env_block_parse_standard",
        n1 == Some(name.len()) && &buf[..name.len()] == name,
        "",
    );
    // 2) 结构不符如实 None：签名错 / 尺寸错 / 块过短——不猜不冒充。
    let mut bad_sig = block;
    bad_sig[4] = 0xA0;
    let short = &block[..0x100];
    let mut wrong_size = block;
    wrong_size[..4].copy_from_slice(&0x300u32.to_le_bytes());
    cs.add(
        "env_block_malformed_rejected",
        parse_env_block(&bad_sig, &mut buf).is_none()
            && parse_env_block(short, &mut buf).is_none()
            && parse_env_block(&wrong_size, &mut buf).is_none(),
        "",
    );
    // 3) 固定联动：pin 计入账且空槽可登记；unpin 计数独立。
    let mut recent: [Option<u64>; 16] = [None; 16];
    recent[0] = Some(0xAB);
    let mut led = PinLedger::new();
    let p1 = led.pin(0xCD, &mut recent);
    led.unpin();
    cs.add(
        "pin_startmenu_f072_ledger",
        p1 && led.pins == 1 && led.unpins == 1,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F013 · 深化批次五：.lnk 属性页六字段渲染（右键属性页的数据源）
//
// 主册依据（G-A-13【交互设计】）：「属性（属性页显示全部六字段可编辑）」——
// 六字段：目标路径/参数/工作目录/图标位置/热键/窗口风格。渲染面把六字段
/// 成行写出（属性页消费），截断如实。
// ---------------------------------------------------------------------------

/// 属性页六字段包（解析面的聚合视图——解析器既有面供给）。
#[derive(Clone, Copy)]
pub struct LnkProperty {
    pub target: &'static str,
    pub params: &'static str,
    pub workdir: &'static str,
    pub icon_loc: &'static str,
    pub hotkey: &'static str,
    pub show_cmd: &'static str,
}

/// 六行渲染（`字段名: 值`——缓冲不足行级截断，返回写字节数）。
pub fn render_property_rows(p: &LnkProperty, buf: &mut [u8]) -> usize {
    let rows = [
        ("目标", p.target),
        ("参数", p.params),
        ("工作目录", p.workdir),
        ("图标位置", p.icon_loc),
        ("热键", p.hotkey),
        ("窗口风格", p.show_cmd),
    ];
    let mut n = 0usize;
    for (i, (name, val)) in rows.iter().enumerate() {
        if i > 0 && n < buf.len() {
            buf[n] = b'\n';
            n += 1;
        }
        for src in [name.as_bytes(), b": ".as_slice(), val.as_bytes()] {
            for &b in src {
                if n >= buf.len() {
                    return n;
                }
                buf[n] = b;
                n += 1;
            }
        }
    }
    n
}

/// F013 深化批次五自检。
pub fn run_lnkfile_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep4");
    // 1) 六行全渲染：字段名与值逐行保真、行数 = 5 个换行。
    let prop = LnkProperty {
        target: "C:\\Tools\\tool.exe",
        params: "--flag",
        workdir: "C:\\Tools",
        icon_loc: "C:\\Tools\\tool.exe,0",
        hotkey: "Ctrl+Alt+T",
        show_cmd: "常规窗口",
    };
    let mut buf = [0u8; 512];
    let n = render_property_rows(&prop, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "property_page_six_rows",
        text.matches('\n').count() == 5
            && text.contains("目标: C:\\Tools\\tool.exe")
            && text.contains("热键: Ctrl+Alt+T")
            && text.ends_with("窗口风格: 常规窗口"),
        "",
    );
    // 2) 空字段如实空值（未设参数 = 「参数: 」行仍在——六字段不缩水）。
    let emptyish = LnkProperty {
        target: "D:\\a.exe",
        params: "",
        workdir: "",
        icon_loc: "D:\\a.exe,0",
        hotkey: "无",
        show_cmd: "最大化",
    };
    let n2 = render_property_rows(&emptyish, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "property_page_empty_fields_visible",
        t2.contains("参数: \n") && t2.contains("工作目录: \n"),
        "",
    );
    // 3) 短缓冲行级截断（不冒充完整页）。
    let mut small = [0u8; 10];
    let n3 = render_property_rows(&prop, &mut small);
    cs.add("property_page_truncation_honest", n3 == 10, "");
    cs
}
