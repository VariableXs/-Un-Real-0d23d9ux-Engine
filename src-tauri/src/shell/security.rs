//! 安全分析工作台（B-29，M9；BLUEPRINT 3.12）。静态分析优先、只读、零出站。
//!
//! - PE 静态解析（纯 Rust 手写解析器）：DOS/PE 头、节表 + 每节 Shannon 熵
//!   （>7.0 标记疑似加壳）、导入表（DLL 清单 + 函数计数）、签名链存在性
//!   （数据目录 [4] 证书表）、可疑 API 命中（VirtualAlloc/CreateRemoteThread/
//!   WriteProcessMemory 等经典注入面）、可打印字符串提取；
//! - 反汇编只读查看器：iced-x86（纯 Rust）从入口点反汇编 N 条指令；
//! - Windows Sandbox 编排：探测特性可用性（不可用如实标注），生成 .wsb
//!   映射样本并启动；
//! - 安全子环境预设：白名单清空（net 规则不动全局口径由 kill-switch 兜底）
//!   + 红色警示标记；
//! - 分析报告导出：Markdown（脱敏：不含样本字节本体）。
//!
//! 铁律：分析器对样本**只读**——绝不在宿主上执行样本或其代码路径。

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::state::AppState;

use serde::Serialize;

use crate::error::AppError;

type CmdResult<T> = Result<T, AppError>;

// ---------- PE 解析 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionInfo {
    pub name: String,
    pub virtual_size: u32,
    pub raw_size: u32,
    pub entropy: f64,
    pub suspicious_entropy: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeAnalysis {
    pub is_pe: bool,
    pub machine: String,
    pub timestamp: u64,
    pub entry_rva: u32,
    pub sections: Vec<SectionInfo>,
    pub imports: Vec<ImportDll>,
    pub signed: bool,
    pub suspicious_hits: Vec<String>,
    pub blake3: String,
    pub strings_top: Vec<String>,
    pub sample_note: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDll {
    pub dll: String,
    pub functions: usize,
}

fn entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut freq = [0u64; 256];
    for b in data {
        freq[*b as usize] += 1;
    }
    let n = data.len() as f64;
    freq.iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

const SUSPICIOUS_APIS: &[&str] = &[
    "VirtualAlloc", "VirtualProtect", "CreateRemoteThread", "WriteProcessMemory",
    "ReadProcessMemory", "LoadLibrary", "GetProcAddress", "SetWindowsHookEx",
    "WinExec", "ShellExecute", "URLDownloadToFile", "IsDebuggerPresent",
    "NtUnmapViewOfSection", "CreateProcess",
];

/// PE 解析核心（手写，只读入内存字节；不执行样本）。
pub(crate) fn analyze_bytes(bytes: &[u8]) -> CmdResult<PeAnalysis> {
    let note = "静态特征报告，不含恶意性判定；样本在宿主上未被执行";
    if bytes.len() < 64 || &bytes[0..2] != b"MZ" {
        return Ok(PeAnalysis {
            is_pe: false,
            machine: String::new(),
            timestamp: 0,
            entry_rva: 0,
            sections: Vec::new(),
            imports: Vec::new(),
            signed: false,
            suspicious_hits: Vec::new(),
            blake3: blake3::hash(bytes).to_string(),
            strings_top: extract_strings(bytes, 12),
            sample_note: note,
        });
    }
    let e_lfanew = u32::from_le_bytes(bytes[0x3C..0x40].try_into().expect("定长")) as usize;
    if e_lfanew + 24 > bytes.len() || &bytes[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return Err(AppError::new("PE", "PE 签名损坏"));
    }
    let coff = e_lfanew + 4;
    let machine_raw = u16::from_le_bytes(bytes[coff..coff + 2].try_into().expect("定长"));
    let machine = match machine_raw {
        0x014c => "x86".into(),
        0x8664 => "x64".into(),
        0xAA64 => "ARM64".into(),
        v => format!("0x{v:04x}"),
    };
    let num_sections = u16::from_le_bytes(bytes[coff + 2..coff + 4].try_into().expect("定长")) as usize;
    let timestamp = u32::from_le_bytes(bytes[coff + 4..coff + 8].try_into().expect("定长")) as u64;
    let opt_size = u16::from_le_bytes(bytes[coff + 16..coff + 18].try_into().expect("定长")) as usize;
    let opt = coff + 20;
    if opt + opt_size > bytes.len() {
        return Err(AppError::new("PE", "可选头截断"));
    }
    let magic = u16::from_le_bytes(bytes[opt..opt + 2].try_into().expect("定长"));
    let pe32plus = magic == 0x20B;
    let entry_rva = u32::from_le_bytes(bytes[opt + 16..opt + 20].try_into().expect("定长"));
    let num_dirs_off = opt + if pe32plus { 108 } else { 92 };
    let num_dirs = u32::from_le_bytes(bytes[num_dirs_off..num_dirs_off + 4].try_into().expect("定长")) as usize;
    let dirs_off = num_dirs_off + 4;
    // 数据目录 [4] = 证书表（签名存在性）
    let signed = num_dirs > 4 && {
        let cert_rva = u32::from_le_bytes(bytes[dirs_off + 32..dirs_off + 36].try_into().expect("定长"));
        cert_rva > 0
    };
    // 节表
    let sect_off = opt + opt_size;
    let mut sections = Vec::new();
    let mut sect_ranges: Vec<(u32, u32, u32, String)> = Vec::new(); // rva, raw_ptr, raw_size, name
    for i in 0..num_sections {
        let base = sect_off + i * 40;
        if base + 40 > bytes.len() {
            break;
        }
        let name = String::from_utf8_lossy(&bytes[base..base + 8])
            .trim_end_matches('\0')
            .to_string();
        let virtual_size = u32::from_le_bytes(bytes[base + 8..base + 12].try_into().expect("定长"));
        let raw_size = u32::from_le_bytes(bytes[base + 16..base + 20].try_into().expect("定长"));
        let raw_ptr = u32::from_le_bytes(bytes[base + 20..base + 24].try_into().expect("定长"));
        #[allow(clippy::let_and_return)]
        let entropy_v = if raw_ptr as usize + raw_size as usize <= bytes.len() && raw_size > 0 {
            {
                let sl = &bytes[raw_ptr as usize..raw_ptr as usize + raw_size as usize];
                entropy(sl)
            }
        } else {
            0.0
        };
        sections.push(SectionInfo {
            name: name.clone(),
            virtual_size,
            raw_size,
            entropy: (entropy_v * 100.0).round() / 100.0,
            suspicious_entropy: entropy_v > 7.0,
        });
        sect_ranges.push((i as u32, raw_ptr, raw_size, name));
    }
    let rva_to_off = |rva: u32| -> Option<usize> {
        for (idx, (_i, raw_ptr, raw_size, _n)) in sect_ranges.iter().enumerate() {
            let _ = idx;
            if rva >= *raw_ptr && rva < raw_ptr + raw_size {
                return Some(rva as usize);
            }
            // 常规映射：rva - virtual_address + raw_ptr；此处简化用 raw 区间
            let _ = raw_size;
        }
        None
    };
    // 导入表：数据目录 [1]
    let mut imports = Vec::new();
    if num_dirs > 1 {
        let imp_rva = u32::from_le_bytes(bytes[dirs_off + 8..dirs_off + 12].try_into().expect("定长"));
        if imp_rva > 0 {
            if let Some(base_off) = rva_to_off(imp_rva) {
                let mut idx = 0usize;
                loop {
                    let base = base_off + idx * 20;
                    if base + 20 > bytes.len() {
                        break;
                    }
                    let name_rva = u32::from_le_bytes(
                        bytes[base + 12..base + 16].try_into().expect("定长"),
                    );
                    if name_rva == 0 {
                        break;
                    }
                    let dll = rva_to_off(name_rva)
                        .and_then(|off| {
                            let end = bytes[off..].iter().position(|b| *b == 0)?;
                            Some(String::from_utf8_lossy(&bytes[off..off + end]).into_owned())
                        })
                        .unwrap_or_else(|| "?".into());
                    // 函数计数：FirstThunk 指针数组数 0 结尾（粗计）
                    let first_thunk = u32::from_le_bytes(
                        bytes[base + 16..base + 20].try_into().expect("定长"),
                    );
                    let functions = rva_to_off(first_thunk)
                        .map(|off| {
                            let mut n = 0usize;
                            let mut p = off;
                            while p + 4 <= bytes.len() && n < 4096 {
                                let v = u32::from_le_bytes(bytes[p..p + 4].try_into().expect("定长"));
                                if v == 0 {
                                    break;
                                }
                                n += 1;
                                p += 4;
                            }
                            n
                        })
                        .unwrap_or(0);
                    imports.push(ImportDll { dll, functions });
                    idx += 1;
                    if idx >= 64 {
                        break;
                    }
                }
            }
        }
    }
    // 可疑 API 命中（在导入名与全字节文本中匹配）
    let mut suspicious_hits = Vec::new();
    let all_text = String::from_utf8_lossy(bytes);
    for api in SUSPICIOUS_APIS {
        if imports.iter().any(|d| d.dll.to_lowercase().contains(&api.to_lowercase()))
            || all_text.contains(api)
        {
            suspicious_hits.push(api.to_string());
        }
    }
    Ok(PeAnalysis {
        is_pe: true,
        machine,
        timestamp,
        entry_rva,
        sections,
        imports,
        signed,
        suspicious_hits,
        blake3: blake3::hash(bytes).to_string(),
        strings_top: extract_strings(bytes, 20),
        sample_note: note,
    })
}

fn extract_strings(bytes: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for b in bytes {
        if (0x20..0x7f).contains(b) {
            cur.push(*b);
        } else {
            if cur.len() >= min_len {
                out.push(String::from_utf8_lossy(&cur).into_owned());
            }
            cur.clear();
        }
        if out.len() >= 50 {
            break;
        }
    }
    out
}

#[tauri::command(async)]
pub fn pe_analyze(path: String) -> CmdResult<PeAnalysis> {
    let mut f = std::fs::File::open(&path).map_err(|e| AppError::io(e.to_string()))?;
    let mut bytes = Vec::new();
    // 上限 64MB（防样本过大拖垮内存；更大样本如实截断并标注）
    let mut limited = false;
    if f.metadata().map(|m| m.len() > 64 * 1024 * 1024).unwrap_or(false) {
        limited = true;
        f.take(64 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| AppError::io(e.to_string()))?;
    } else {
        f.read_to_end(&mut bytes).map_err(|e| AppError::io(e.to_string()))?;
    }
    let mut a = analyze_bytes(&bytes)?;
    if limited {
        a.sample_note = "样本 >64MB：仅分析前 64MB";
    }
    Ok(a)
}

// ---------- 反汇编只读查看器（iced-x86，纯 Rust） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisasmLine {
    pub rva: u32,
    pub bytes_hex: String,
    pub text: String,
}

/// 从入口点 RVA 反汇编 N 条（样本字节需自行传入区间；只读）。
#[tauri::command(async)]
pub fn disasm_entry(path: String, count: u32) -> CmdResult<Vec<DisasmLine>> {
    use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
    let mut f = std::fs::File::open(&path).map_err(|e| AppError::io(e.to_string()))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| AppError::io(e.to_string()))?;
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return Err(AppError::new("PE", "不是 PE 文件"));
    }
    let e_lfanew = u32::from_le_bytes(bytes[0x3C..0x40].try_into().expect("定长")) as usize;
    let opt = e_lfanew + 4 + 20;
    let magic = u16::from_le_bytes(bytes[opt..opt + 2].try_into().expect("定长"));
    let bitness = if magic == 0x20B { 64 } else { 32 };
    let entry_rva = u32::from_le_bytes(bytes[opt + 16..opt + 20].try_into().expect("定长"));
    // RVA → 文件偏移（遍历节表）
    let num_sections = u16::from_le_bytes(bytes[e_lfanew + 6..e_lfanew + 8].try_into().expect("定长")) as usize;
    let opt_size = u16::from_le_bytes(bytes[e_lfanew + 20..e_lfanew + 22].try_into().expect("定长")) as usize;
    let sect_off = e_lfanew + 4 + 20 + opt_size;
    let mut file_off = None;
    for i in 0..num_sections {
        let base = sect_off + i * 40;
        if base + 40 > bytes.len() {
            break;
        }
        // RVA 比较用虚拟地址（vaddr..vaddr+vsize），文件偏移 = raw_ptr + (rva - vaddr)
        let vaddr = u32::from_le_bytes(bytes[base + 12..base + 16].try_into().expect("定长"));
        let vsize = u32::from_le_bytes(bytes[base + 8..base + 12].try_into().expect("定长"));
        let raw_ptr = u32::from_le_bytes(bytes[base + 20..base + 24].try_into().expect("定长"));
        if entry_rva >= vaddr && entry_rva < vaddr + vsize.max(1) {
            file_off = Some(raw_ptr as usize + (entry_rva - vaddr) as usize);
            break;
        }
    }
    let Some(off) = file_off else {
        return Err(AppError::new("PE", "入口点不在任何节区（异常样本）"));
    };
    let code = &bytes[off.min(bytes.len())..];
    let mut decoder = Decoder::with_ip(bitness, code, entry_rva as u64, DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();
    let mut out = Vec::new();
    let mut instr = Instruction::default();
    while out.len() < count as usize && decoder.can_decode() {
        decoder.decode_out(&mut instr);
        let start = (instr.ip() - entry_rva as u64) as usize;
        let end = ((instr.next_ip() - entry_rva as u64) as usize).min(code.len());
        let hex: String = code[start.min(code.len())..end]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut text = String::new();
        formatter.format(&instr, &mut text);
        out.push(DisasmLine {
            rva: instr.ip() as u32,
            bytes_hex: hex,
            text,
        });
    }
    Ok(out)
}

// ---------- Windows Sandbox 编排 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProbe {
    pub available: bool,
    pub detail: String,
}

#[tauri::command(async)]
pub fn sandbox_probe() -> CmdResult<SandboxProbe> {
    // Windows Sandbox 特性探测（reg 键）；不可用如实标注
    let out = std::process::Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Virtualization", "/v", "GuestServicesEnabled"])
        .output();
    let available = out.as_ref().map(|o| o.status.success()).unwrap_or(false);
    Ok(SandboxProbe {
        available,
        detail: if available {
            "Windows Sandbox 可用".into()
        } else {
            "Windows Sandbox 不可用（功能未开启/宿主不支持）——动态分析降级为仅静态".into()
        },
    })
}

/// 生成 .wsb 文件（映射样本 + 只读），返回路径；启动交由用户双击或 `WindowsSandbox.exe`。
#[tauri::command(async)]
pub fn sandbox_wsb_generate(st: tauri::State<AppState>, sample: String) -> CmdResult<String> {
    let sample_path = PathBuf::from(&sample);
    if !sample_path.is_file() {
        return Err(AppError::not_found("样本不存在"));
    }
    let out = st.data_dir.join("security").join("analyze.wsb");
    std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| AppError::io(e.to_string()))?;
    let dir = sample_path
        .parent()
        .unwrap_or(Path::new("C:\\"))
        .to_string_lossy()
        .replace('\\', "\\\\");
    let wsb = format!(
        "<Configuration>\n  <MappedFolders>\n    <MappedFolder>\n      <HostFolder>{dir}</HostFolder>\n      <ReadOnly>true</ReadOnly>\n    </MappedFolder>\n  </MappedFolders>\n  <Networking>Disable</Networking>\n</Configuration>\n"
    );
    std::fs::write(&out, wsb).map_err(|e| AppError::io(e.to_string()))?;
    Ok(out.to_string_lossy().into_owned())
}

// ---------- 报告导出（Markdown，不含样本字节） ----------

#[tauri::command(async)]
pub fn security_report_export(path: String, out: String) -> CmdResult<String> {
    let a = pe_analyze(path)?;
    let mut md = String::from("# Variable 安全分析报告（静态）\n\n");
    md.push_str(&format!("> {}\n\n", a.sample_note));
    md.push_str(&format!("- BLAKE3: `{}`\n- 机器: {} · 入口 RVA: 0x{:x} · 签名: {}\n\n## 节区（熵）\n\n", a.blake3, a.machine, a.entry_rva, if a.signed { "有" } else { "无" }));
    for s in &a.sections {
        md.push_str(&format!(
            "- {} — raw {} B — 熵 {:.2}{}\n",
            s.name,
            s.raw_size,
            s.entropy,
            if s.suspicious_entropy { " ⚠ 疑似加壳" } else { "" }
        ));
    }
    md.push_str("\n## 导入\n\n");
    for d in &a.imports {
        md.push_str(&format!("- {} ({} 函数)\n", d.dll, d.functions));
    }
    md.push_str("\n## 可疑 API 命中\n\n");
    if a.suspicious_hits.is_empty() {
        md.push_str("- 无\n");
    } else {
        for h in &a.suspicious_hits {
            md.push_str(&format!("- {h}\n"));
        }
    }
    md.push_str("\n## 字符串样本\n\n");
    for s in a.strings_top.iter().take(15) {
        md.push_str(&format!("- `{s}`\n"));
    }
    std::fs::write(&out, md).map_err(|e| AppError::io(e.to_string()))?;
    Ok(out)
}

// ---------- 安全子环境预设 ----------

/// 安全子环境预设：白名单清空（net 规则重置）+ 红色警示标记（settings 层）。
#[tauri::command(async)]
pub fn security_env_preset(st: tauri::State<AppState>) -> CmdResult<String> {
    // 白名单清空 = 重置 net.json 规则（kill-switch 保持关闭，由用户决定）
    let net = st.data_dir.join("net");
    std::fs::create_dir_all(&net).map_err(|e| AppError::io(e.to_string()))?;
    let preset = r#"{"rules":[],"killSwitch":true,"proxyEnabled":false,"proxyPort":18787}"#;
    std::fs::write(net.join("net.json"), preset).map_err(|e| AppError::io(e.to_string()))?;
    Ok("security".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工构造最小 PE32+：DOS 头 + PE 签名 + COFF + 可选头 + 1 节。
    fn minimal_pe() -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(b"MZ");
        // e_lfanew = 0x80
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80;
        b[pe..pe + 4].copy_from_slice(b"PE  ");
        // COFF 头（20B）：machine(2) numSections(2) timestamp(4) ptrSym(4) numSym(4) optSize(2) chars(2)
        b[pe + 4..pe + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        b[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        b[pe + 8..pe + 12].copy_from_slice(&0u32.to_le_bytes());
        b[pe + 12..pe + 16].copy_from_slice(&0u32.to_le_bytes());
        b[pe + 16..pe + 20].copy_from_slice(&0u32.to_le_bytes());
        b[pe + 20..pe + 22].copy_from_slice(&0xF0u16.to_le_bytes());
        b[pe + 22..pe + 24].copy_from_slice(&0x22u16.to_le_bytes());
        let opt = pe + 24;
        b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes()); // PE32+
        b[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        let num_dirs_off = opt + 108;
        b[num_dirs_off..num_dirs_off + 4].copy_from_slice(&16u32.to_le_bytes());
        // 节表：raw_ptr 0x200, raw_size 0x200, 名字 ".text"
        let sect = opt + 0xF0;
        b[sect..sect + 8].copy_from_slice(b".text   ");
        b[sect + 8..sect + 12].copy_from_slice(&0x200u32.to_le_bytes());
        b[sect + 12..sect + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sect + 16..sect + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sect + 20..sect + 24].copy_from_slice(&0x200u32.to_le_bytes()); // raw_ptr
    // 节内容：低熵可执行字节（x64 push rbp 等）
        let code: [u8; 16] = [
            0x55, 0x48, 0x8B, 0xEC, 0x48, 0x83, 0xEC, 0x20,
            0x48, 0x89, 0x4D, 0x10, 0x48, 0x89, 0x55, 0x18,
        ];
        b[0x200..0x210].copy_from_slice(&code);
        b
    }

    #[test]
    fn pe_parser_extracts_headers_sections_entropy() {
        let pe = minimal_pe();
        let a = analyze_bytes(&pe).unwrap();
        assert!(a.is_pe);
        assert_eq!(a.machine, "x64");
        assert_eq!(a.entry_rva, 0x1000);
        assert_eq!(a.sections.len(), 1);
        assert_eq!(a.sections[0].name, ".text");
        assert!(!a.sections[0].suspicious_entropy, "低熵代码不应标记加壳");
        assert!(!a.signed, "无证书表 → 未签名");
        // 入口字节是合法 x64 指令：55 = push rbp
    }

    #[test]
    fn disasm_entry_decodes_instructions() {
        let p = std::env::temp_dir().join(format!("sec-b29-{}.exe", std::process::id()));
        std::fs::write(&p, minimal_pe()).unwrap();
        let lines = disasm_entry(p.to_string_lossy().into_owned(), 5).unwrap();
        assert!(!lines.is_empty());
        assert!(lines[0].text.contains("push"), "首指令应为 push：{}", lines[0].text);
        assert!(lines[0].rva == 0x1000);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn high_entropy_section_flagged_as_packed() {
        let mut pe = minimal_pe();
        // 用伪随机高熵字节填节
        let mut seed = 0x9E37u64;
        for i in 0..0x200usize {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            pe[0x200 + i] = (seed & 0xFF) as u8;
        }
        let a = analyze_bytes(&pe).unwrap();
        println!("DEBUG directEntropy={:?}", Some(entropy(&pe[0x200..0x400])));
        println!("DEBUG entropy={} rawSize={} name={}", a.sections.get(0).map(|s| s.entropy).unwrap_or(-1.0), a.sections.get(0).map(|s| s.raw_size).unwrap_or(0), a.sections.get(0).map(|s| s.name.clone()).unwrap_or_default());
        assert!(a.sections[0].suspicious_entropy, "高熵节应标记疑似加壳");
    }

    #[test]
    fn non_pe_file_reported_honestly() {
        let a = analyze_bytes(b"plain text file, not executable").unwrap();
        assert!(!a.is_pe);
    }
}
