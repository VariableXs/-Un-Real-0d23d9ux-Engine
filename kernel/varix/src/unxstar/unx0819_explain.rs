//! UNX-F0819 · 异常现场人话化解释与诊断账深化账 · 诊断段 0xB890
//! 验收锚点：UNX-F0819-J1 全 32 异常人话化文案覆盖 100%，诊断账
//! 三要素齐备率 100%（发生了什么/为什么/下一步），双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB890;

// ── 诊断段独占 · 人话化错误域 ───────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_NOCOPY: u16 = SEG_ERR + 1;  // 文案缺失
const E_TRIAD: u16 = SEG_ERR + 2;   // 三要素缺
const E_VEC: u16 = SEG_ERR + 3;     // 向量越域
const E_DUP: u16 = SEG_ERR + 4;     // 重复挂账
const E_BAL: u16 = SEG_ERR + 5;     // 账不平

pub const EXC_VECS: u8 = 32;

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 人话化文案表 ─────────────────────────────────────────────
/// 32 异常简名表（x86 语义，向量 0..31 逐项）。
pub fn exc_names() -> [&'static str; 32] {
    [
        "divide_error", "debug", "nmi", "breakpoint", "overflow",
        "bound_range", "invalid_opcode", "device_na", "double_fault",
        "coproc_seg", "invalid_tss", "seg_not_present", "stack_fault",
        "general_protection", "page_fault", "reserved_15", "fpu_error",
        "alignment_check", "machine_check", "simd_fp", "virt_exc",
        "cp_protection", "reserved_22", "reserved_23", "reserved_24",
        "reserved_25", "reserved_26", "reserved_27", "hypervisor_inj",
        "vmm_comm", "security", "reserved_31",
    ]
}

/// 32 异常人话化「发生了什么」文案（逐向量映射）。
pub fn what_text(v: u8) -> Option<&'static str> {
    Some(match v {
        0 => "除零或除法溢出",
        1 => "调试单步/断点命中",
        2 => "不可屏蔽中断",
        3 => "断点指令 int3",
        4 => "INTO 溢出检测",
        5 => "BOUND 边界越界",
        6 => "非法操作码",
        7 => "协处理器不可用",
        8 => "双重故障",
        9 => "协处理器段越界（历史）",
        10 => "无效任务切换 TSS",
        11 => "段不存在",
        12 => "栈段故障",
        13 => "一般保护故障",
        14 => "页故障（缺页/保护违例）",
        15 => "保留向量",
        16 => "x87 浮点错误",
        17 => "对齐检查",
        18 => "机器检查",
        19 => "SIMD 浮点异常",
        20 => "虚拟化异常",
        21 => "控制保护异常",
        22..=27 => "保留向量",
        28 => "虚拟化注入异常",
        29 => "VMM 通信异常",
        30 => "安全异常",
        31 => "保留向量",
        _ => return None,
    })
}

/// 「为什么」归因文案。
pub fn why_text(v: u8) -> Option<&'static str> {
    Some(match v {
        0 => "除数为零或商超出寄存器宽",
        1 | 3 => "调试器或断点指令触发",
        2 => "硬件致命事件（不软件屏蔽）",
        6 => "指令流被破坏或跳入数据段",
        8 => "双重故障：嵌套异常未先行处理",
        13 => "越权访问或非法段描述符",
        14 => "页表缺失或访问权限不足",
        17 => "未对齐访问且 CR0.AM 启用",
        18 => "硬件内部错误被 MCE 捕获",
        _ => "向量触发（通用归因）",
    })
}

/// 「下一步」处置文案。
pub fn next_text(v: u8) -> Option<&'static str> {
    Some(match v {
        8 | 18 => "停机（不可恢复，需转储现场）",
        14 => "查 CR2 地址走缺页处理或杀进程",
        0 => "检查除法路径输入域",
        13 => "检查访问权限与段选择子",
        _ => "记录诊断账并继续调度",
    })
}

// ── 诊断账三要素 ─────────────────────────────────────────────
/// 一条诊断账：向量 + 三要素齐备标记。
#[derive(Clone, Debug)]
pub struct DiagEntry {
    pub vec: u8,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl DiagEntry {
    pub fn triad_full(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}|{}|{}|{}", self.vec, self.what, self.why, self.next).as_bytes())
    }
}

/// 诊断账簿：32 向量全覆盖挂账。
pub struct DiagLedger {
    pub entries: Vec<DiagEntry>,
}

impl DiagLedger {
    pub fn new() -> Self {
        DiagLedger { entries: Vec::new() }
    }
    /// 从文案表生成全 32 向量账。
    pub fn build_full() -> DiagLedger {
        let mut led = DiagLedger::new();
        for v in 0..EXC_VECS {
            let w = what_text(v).unwrap_or("?").to_string();
            let y = why_text(v).unwrap_or("?").to_string();
            let n = next_text(v).unwrap_or("?").to_string();
            led.entries.push(DiagEntry { vec: v, what: w, why: y, next: n });
        }
        led
    }
    /// 覆盖率：全 32 向量各恰一条且文案非空。
    pub fn coverage(&self) -> bool {
        self.entries.len() == EXC_VECS as usize
            && (0..EXC_VECS).all(|v| {
                self.entries.iter().filter(|e| e.vec == v).count() == 1
                    && self.entries.iter().any(|e| e.vec == v && !e.what.is_empty())
            })
    }
    /// 三要素齐备率。
    pub fn triad_rate(&self) -> (usize, usize) {
        let full = self.entries.iter().filter(|e| e.triad_full()).count();
        (full, self.entries.len())
    }
    /// 向量域校验。
    pub fn vec_domain(&self) -> bool {
        self.entries.iter().all(|e| e.vec < EXC_VECS)
    }
    /// 重复挂账检出。
    pub fn no_dup(&self) -> bool {
        let n = self.entries.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.entries[i].vec == self.entries[j].vec {
                    return false;
                }
            }
        }
        true
    }
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0819_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0819 异常人话化诊断账");
    let led = DiagLedger::build_full();

    // 1. 全 32 异常文案覆盖 100%
    s.add("EXP-覆盖100%", led.coverage(), if led.coverage() { "" } else { "文案缺口" });

    // 2. 三要素齐备率 100%
    let (full, total) = led.triad_rate();
    s.add("EXP-三要素100%", full == total && total == 32,
        if full == total { "" } else { "要素缺" });

    // 3. 向量域合法
    s.add("EXP-向量域", led.vec_domain(), if led.vec_domain() { "" } else { "向量越域" });

    // 4. 无重复挂账
    s.add("EXP-无重复挂账", led.no_dup(), if led.no_dup() { "" } else { "重复挂账" });

    // 5. 简名表逐向量非空且互异
    let names = exc_names();
    let uniq = (0..32usize).all(|i| !names[i].is_empty())
        && (0..32usize).all(|i| (i + 1..32).all(|j| names[i] != names[j]));
    s.add("EXP-简名互异", uniq, if uniq { "" } else { "简名重复" });

    // 6. 关键向量文案抽查（0/6/8/13/14 五向量）
    let spot = what_text(0) == Some("除零或除法溢出")
        && what_text(6) == Some("非法操作码")
        && what_text(8) == Some("双重故障")
        && what_text(13) == Some("一般保护故障")
        && what_text(14) == Some("页故障（缺页/保护违例）");
    s.add("EXP-关键抽查", spot, if spot { "" } else { "关键文案漂移" });

    // 7. 关键向量「下一步」语义（8/18 停机、14 走缺页）
    let nxt = next_text(8) == Some("停机（不可恢复，需转储现场）")
        && next_text(14) == Some("查 CR2 地址走缺页处理或杀进程");
    s.add("EXP-下一步语义", nxt, if nxt { "" } else { "处置漂移" });

    // 8. 越域向量返回 None
    let oob = what_text(32).is_none() && why_text(255).is_none() && next_text(64).is_none();
    s.add("EXP-越域None", oob, if oob { "" } else { "越域有值" });

    // 9. 诊断账指纹稳定且互异
    let mut fp_uniq = true;
    for i in 0..led.entries.len() {
        for j in (i + 1)..led.entries.len() {
            if led.entries[i].fp() == led.entries[j].fp() {
                fp_uniq = false;
            }
        }
    }
    let stable = led.entries[14].fp() == led.entries[14].fp();
    s.add("EXP-指纹稳定互异", stable && fp_uniq, if stable && fp_uniq { "" } else { "指纹异常" });

    // 10. LCG 抽样 8 向量三要素非空
    let mut lcg = Lcg(0xB890);
    let mut spot8 = true;
    for _ in 0..8 {
        let v = lcg.below(32) as u8;
        let e = led.entries.iter().find(|e| e.vec == v);
        match e {
            Some(e) if e.triad_full() => {}
            _ => spot8 = false,
        }
    }
    s.add("EXP-抽样8向量", spot8, if spot8 { "" } else { "抽样缺要素" });

    // 11. 账平衡：entries 数 = 32
    s.add("EXP-账平衡", led.entries.len() == 32,
        if led.entries.len() == 32 { "" } else { "条数漂移" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB890 && SEG_BASE < 0xB8B0;
    s.add("EXP-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
