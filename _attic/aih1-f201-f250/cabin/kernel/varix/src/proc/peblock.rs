//! PE 拒绝表（任务 62 · AI-S）：规则驱动的恶意 PE 静态特征拦截。
//!
//! - **默认放行、特征拦截**：合法 PE（含 hello.pe 演示样本）零误拦；
//!   恶意特征逐条具名拒绝，拒绝原因用户可读（总案验收：拒绝原因用户可读展示）。
//! - **规则表驱动可更新**（总案验收）：新增特征只加表项，判定器不改。
//! - 特征口径全部为**合成/脱敏**的公开技术特征（进程注入三件套、Run 键持久化、
//!   加壳段名、内嵌 PE 拖滴、异常 DOS 头等），不针对任何真实样本指纹。
//! - 判定 = blob 字节特征（组合名串）+ PeImage 结构特征（RWX 段），纯函数
//!   host 可测；不做大结构导入表物化（41KB ImportTable 不上栈，走名字子串口径）。
//! - 调用点：ring3 spawn 路径 parse 成功后、装载前——拒绝即不装载。

use crate::proc::pe::{PeImage, MZ_MAGIC};

/// 判定结果：放行 / 拒绝（带规则 id 与用户可读原因）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block { rule: &'static str, reason: &'static str },
}

/// 匹配器：数据驱动（判定器不认识具体规则）。
pub enum Matcher {
    /// blob 包含字节特征串。
    Contains(&'static [u8]),
    /// 多个特征串**同时**出现在 blob（高危 API 组合按组合判定，单 API 不拦）。
    AllOf(&'static [&'static [u8]]),
    /// blob 内 0x80 之后再出现 MZ（拖滴内嵌 PE；合法 PE 头部只有一枚 MZ）。
    EmbeddedMz,
    /// DOS stub 区全零（合法链接器必有 stub）。
    NoDosStub,
    /// 存在可写且可执行段（RWX；对非打包软件不提供执行担保）。
    RwxSection,
}

/// 一条拒绝规则。
pub struct PeblockRule {
    /// 规则 id（PEB-001…，拒绝原因展示用）。
    pub id: &'static str,
    /// 用户可读拒绝原因。
    pub reason: &'static str,
    pub m: Matcher,
}

/// 进程注入三件套（同时出现才拦——单一 API 大量合法软件使用）。
const INJECT_TRIO: &[&[u8]] = &[b"VirtualAllocEx", b"WriteProcessMemory", b"CreateRemoteThread"];

/// 拒绝规则表（20 条，任务 62 验收 ≥20）。
pub static PEBLOCK_RULES: &[PeblockRule] = &[
    PeblockRule { id: "PEB-001", reason: "疑似进程注入（同时导入 VirtualAllocEx/WriteProcessMemory/CreateRemoteThread）", m: Matcher::AllOf(INJECT_TRIO) },
    PeblockRule { id: "PEB-002", reason: "疑似注册表持久化（Run 键路径写入特征）", m: Matcher::Contains(b"Software\\Microsoft\\Windows\\CurrentVersion\\Run") },
    PeblockRule { id: "PEB-003", reason: "内嵌第二份 PE 头（拖滴/自释放特征）", m: Matcher::EmbeddedMz },
    PeblockRule { id: "PEB-004", reason: "UPX 加壳段（外壳内代码不提供执行担保）", m: Matcher::Contains(b"UPX0") },
    PeblockRule { id: "PEB-005", reason: "加壳段名（外壳内代码不提供执行担保）", m: Matcher::Contains(b".aspack") },
    PeblockRule { id: "PEB-006", reason: "加壳段名（外壳内代码不提供执行担保）", m: Matcher::Contains(b".themida") },
    PeblockRule { id: "PEB-007", reason: "DOS 头异常（stub 区全零；合法链接器必有 stub）", m: Matcher::NoDosStub },
    PeblockRule { id: "PEB-008", reason: "疑似键盘记录（低级钩子 + 按键状态查询组合）", m: Matcher::AllOf(&[b"SetWindowsHookExA", b"GetAsyncKeyState"]) },
    PeblockRule { id: "PEB-009", reason: "疑似凭据枚举（advapi32 凭据 API 特征）", m: Matcher::Contains(b"CredEnumerateA") },
    PeblockRule { id: "PEB-010", reason: "疑似原始套接字后门（WSASocketA + 监听绑定组合）", m: Matcher::AllOf(&[b"WSASocketA", b"bind\x00"]) },
    PeblockRule { id: "PEB-011", reason: "疑似服务持久化（服务控制管理器创建组合）", m: Matcher::AllOf(&[b"OpenSCManagerA", b"CreateServiceA"]) },
    PeblockRule { id: "PEB-012", reason: "疑似屏幕监控（全屏位块拷贝 + 设备上下文抓取组合）", m: Matcher::AllOf(&[b"BitBlt", b"GetDC\x00"]) },
    PeblockRule { id: "PEB-013", reason: "反调试自我保护（调试器探测 + 调试输出抑制组合）", m: Matcher::AllOf(&[b"IsDebuggerPresent", b"CheckRemoteDebuggerPresent"]) },
    PeblockRule { id: "PEB-014", reason: "脚本拖滴载荷（编码脚本执行参数特征）", m: Matcher::Contains(b"/b /e:jscript encode") },
    PeblockRule { id: "PEB-015", reason: "可执行自复制（自写启动目录特征串）", m: Matcher::Contains(b"\\Start Menu\\Programs\\Startup\\") },
    PeblockRule { id: "PEB-016", reason: "计划任务持久化（登录触发计划任务特征参数）", m: Matcher::Contains(b"schtasks /create /sc onlogon") },
    PeblockRule { id: "PEB-017", reason: "防火墙降权（全配置文件防火墙关闭特征）", m: Matcher::Contains(b"netsh advfirewall set allprofiles") },
    PeblockRule { id: "PEB-018", reason: "勒索标记扩展名清单特征", m: Matcher::Contains(b".encryptedreadme") },
    PeblockRule { id: "PEB-019", reason: "挖矿池协议特征串", m: Matcher::Contains(b"stratum+tcp://") },
    PeblockRule { id: "PEB-020", reason: "可写可执行段（RWX；对非打包软件不提供执行担保）", m: Matcher::RwxSection },
];

/// 判定入口：合法 PE 全部放行；命中任一规则即拒绝（拒绝优先，总案 37 仲裁同源）。
/// `blob` = 完整 PE 文件字节；`img` = `pe::parse` 成功产物（结构特征用）。
pub fn peblock_verdict(blob: &[u8], img: &PeImage) -> Verdict {
    for r in PEBLOCK_RULES {
        let hit = match r.m {
            Matcher::Contains(pat) => contains(blob, pat),
            Matcher::AllOf(pats) => pats.iter().all(|p| contains(blob, p)),
            Matcher::EmbeddedMz => embedded_mz(blob),
            Matcher::NoDosStub => blob.len() > 0x80 && blob[0x40..0x80].iter().all(|&b| b == 0),
            Matcher::RwxSection => img.sections().iter().any(|s| s.writable && s.executable),
        };
        if hit {
            return Verdict::Block { rule: r.id, reason: r.reason };
        }
    }
    Verdict::Allow
}

/// 装载闸门：parse + 拒绝表一体。运行时外来 PE（SHARED 分区）装载前必须经过
/// 本闸门；内置演示 PE（HELLO_PE/NOTEPAD_PE 等）同样走一遍作为实机打点。
/// Err 载荷 = (规则 id, 用户可读原因)。
pub fn peblock_gate(blob: &[u8]) -> Result<PeImage, (&'static str, &'static str)> {
    let img = crate::proc::pe::parse(blob)
        .map_err(|e| ("PE-STRUCT", e.as_str()))?;
    match peblock_verdict(blob, &img) {
        Verdict::Allow => Ok(img),
        Verdict::Block { rule, reason } => Err((rule, reason)),
    }
}

/// 内嵌 MZ：0x80（PE 头区之后）起再出现完整 MZ 魔数。
fn embedded_mz(blob: &[u8]) -> bool {
    if blob.len() < 0x200 {
        return false;
    }
    blob[0x80..].windows(2).any(|w| w == MZ_MAGIC)
}

fn contains(hay: &[u8], pat: &[u8]) -> bool {
    hay.windows(pat.len()).any(|w| w == pat)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成合法 PE 底版（独立成套）：布局贴近真实链接器产物——
    /// DOS 头(0x40) + stub 文本区(0x40..0x80) + PE 头@0x80（e_lfanew=0x80）
    /// + COFF + opt(0xF0) + 单节 .text；节原始数据 0x200..0x400。
    /// 0x40..0x80 是真 stub 区，PEB-007（stub 全零）在该布局下可触发。
    const OPT: usize = 0x98; // 0x80(PE 签名) + 4 + 20(COFF)
    const SH: usize = OPT + 0xF0; // 节表 @0x188

    const DOS_STUB: &[u8] = b"This program cannot be run in DOS mode.\r\r\n$\0";

    fn synth_legal(mutations: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut img = vec![0u8; 0x400];
        img[0..2].copy_from_slice(b"MZ");
        img[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes()); // e_lfanew → 0x80
        img[0x4E..0x4E + DOS_STUB.len()].copy_from_slice(DOS_STUB);
        img[0x80..0x84].copy_from_slice(b"PE\0\0");
        img[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes()); // AMD64
        img[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // 1 节
        img[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes()); // SizeOfOptionalHeader
        img[0x96..0x98].copy_from_slice(&0x22u16.to_le_bytes()); // Characteristics
        img[OPT..OPT + 2].copy_from_slice(&0x20Bu16.to_le_bytes()); // PE32+
        img[OPT + 16..OPT + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        img[OPT + 24..OPT + 32].copy_from_slice(&0x1_4000_0000u64.to_le_bytes()); // ImageBase
        img[OPT + 32..OPT + 36].copy_from_slice(&0x1000u32.to_le_bytes()); // SectionAlignment
        img[OPT + 36..OPT + 40].copy_from_slice(&0x200u32.to_le_bytes()); // FileAlignment
        img[OPT + 56..OPT + 60].copy_from_slice(&0x2000u32.to_le_bytes()); // SizeOfImage
        img[OPT + 60..OPT + 64].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        img[SH..SH + 8].copy_from_slice(b".text\0\0\0");
        img[SH + 8..SH + 12].copy_from_slice(&0x200u32.to_le_bytes()); // VirtualSize
        img[SH + 12..SH + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // VirtualAddress
        img[SH + 16..SH + 20].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfRawData
        img[SH + 20..SH + 24].copy_from_slice(&0x200u32.to_le_bytes()); // PointerToRawData
        img[SH + 36..SH + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes()); // RX
        mutations(&mut img);
        img
    }

    /// 定长安全写入：区间长度由字面量自证，杜绝区间/串长错位 panic。
    fn put(img: &mut Vec<u8>, off: usize, s: &[u8]) {
        img[off..off + s.len()].copy_from_slice(s);
    }

    fn parse_of(blob: &[u8]) -> PeImage {
        crate::proc::pe::parse(blob).expect("合成样本必须可解析")
    }

    fn block_of(v: Verdict) -> (&'static str, &'static str) {
        match v {
            Verdict::Block { rule, reason } => (rule, reason),
            Verdict::Allow => panic!("期望 Block"),
        }
    }

    /// 50 个合法变体：不同 entry/base/文件尾 —— 全部放行（零误拦）。
    #[test]
    fn fifty_legal_variants_zero_false_positive() {
        for i in 0..50u32 {
            let blob = synth_legal(|img| {
                // entry 留在 .text（RVA 0x1000..0x1200）内：i*8 ≤ 0x188。
                img[OPT + 16..OPT + 20].copy_from_slice(&(0x1000 + i * 8).to_le_bytes());
                img[OPT + 24..OPT + 32]
                    .copy_from_slice(&(0x1_4000_0000 + i as u64 * 0x10000).to_le_bytes());
                // 尾部追加无害填充（不同文件大小）。
                img.extend(core::iter::repeat_n(0u8, (i % 7) as usize * 0x10));
            });
            let pe = parse_of(&blob);
            assert_eq!(peblock_verdict(&blob, &pe), Verdict::Allow, "样本 #{i} 被误拦");
        }
    }

    /// 20 条规则按表序逐一触发：全部拒绝且规则 id/原因具名（拒绝优先取首条）。
    #[test]
    fn twenty_malware_variants_all_blocked_with_named_reason() {
        // 每条规则一个触发变异（表序 = 断言序；早序规则不得被误触发）。
        let cases: Vec<Box<dyn Fn(&mut Vec<u8>)>> = vec![
            // PEB-001 注入三件套（AllOf：三条同时出现才拦）
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"VirtualAllocEx");
                put(i, 0x220, b"WriteProcessMemory");
                put(i, 0x240, b"CreateRemoteThread");
            }),
            // PEB-002 Run 键持久化
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x240, b"Software\\Microsoft\\Windows\\CurrentVersion\\Run\0")
            }),
            // PEB-003 内嵌 MZ（拖滴）
            Box::new(|i: &mut Vec<u8>| put(i, 0x280, b"MZ")),
            // PEB-004/005/006 加壳段名
            Box::new(|i: &mut Vec<u8>| put(i, 0x300, b"UPX0")),
            Box::new(|i: &mut Vec<u8>| put(i, 0x300, b".aspack")),
            Box::new(|i: &mut Vec<u8>| put(i, 0x300, b".themida")),
            // PEB-007 DOS stub 全零
            Box::new(|i: &mut Vec<u8>| i[0x40..0x80].fill(0)),
            // PEB-008 键盘记录组合
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"SetWindowsHookExA");
                put(i, 0x220, b"GetAsyncKeyState\0");
            }),
            // PEB-009 凭据枚举
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"CredEnumerateA")),
            // PEB-010 原始套接字后门
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"WSASocketA");
                put(i, 0x220, b"bind\0");
            }),
            // PEB-011 服务持久化
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"OpenSCManagerA");
                put(i, 0x220, b"CreateServiceA");
            }),
            // PEB-012 屏幕监控
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"BitBlt");
                put(i, 0x220, b"GetDC\0");
            }),
            // PEB-013 反调试
            Box::new(|i: &mut Vec<u8>| {
                put(i, 0x200, b"IsDebuggerPresent");
                put(i, 0x220, b"CheckRemoteDebuggerPresent");
            }),
            // PEB-014 脚本拖滴
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"/b /e:jscript encode")),
            // PEB-015 启动目录自复制
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"\\Start Menu\\Programs\\Startup\\x.exe")),
            // PEB-016 计划任务持久化
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"schtasks /create /sc onlogon")),
            // PEB-017 防火墙降权
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"netsh advfirewall set allprofiles on")),
            // PEB-018 勒索扩展名
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"readme.encryptedreadme")),
            // PEB-019 矿池协议
            Box::new(|i: &mut Vec<u8>| put(i, 0x240, b"stratum+tcp://pool.x")),
            // PEB-020 RWX 段（.text 加写位：W|X|R|CODE）
            Box::new(|i: &mut Vec<u8>| put(i, SH + 36, &0xE000_0020u32.to_le_bytes())),
        ];
        assert_eq!(cases.len(), PEBLOCK_RULES.len(), "触发样本须逐规则覆盖");
        for (idx, mutate) in cases.into_iter().enumerate() {
            let blob = synth_legal(mutate);
            let pe = parse_of(&blob);
            let (rule, reason) = block_of(peblock_verdict(&blob, &pe));
            let expect_id = PEBLOCK_RULES[idx].id;
            assert_eq!(rule, expect_id, "样本 #{idx} 命中规则错位");
            assert!(!reason.is_empty());
        }
    }

    /// 三件套单 API 不拦（组合判定）：只有 VirtualAllocEx 无另外两个 → 放行。
    #[test]
    fn single_inject_api_does_not_block() {
        let blob = synth_legal(|i: &mut Vec<u8>| put(i, 0x200, b"VirtualAllocEx"));
        let pe = parse_of(&blob);
        assert_eq!(peblock_verdict(&blob, &pe), Verdict::Allow);
    }

    /// 内嵌 MZ 阈值：<0x200 的小文件不判内嵌（防头部随机串误报）。
    #[test]
    fn embedded_mz_requires_min_size() {
        let short = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xFF\xFF\0\0MZ".to_vec();
        assert!(short.len() < 0x200);
        assert!(!embedded_mz(&short));
    }
}
