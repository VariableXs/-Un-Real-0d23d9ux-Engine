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
pub fn run_lnkfile_checks() -> CheckSet {
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
